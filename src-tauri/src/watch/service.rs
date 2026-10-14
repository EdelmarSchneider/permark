//! The live watcher: `notify` on each root, feeding the aggregator and the disk reconciliation.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::event::{CreateKind, ModifyKind, RenameMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::aggregate::{Aggregator, Hint, RawEvent, MAX_WAIT, QUIET};
use super::filter::PathFilter;
use super::reconcile::{reconcile, Env};
use super::sigs::SigStore;
use super::types::{Bulk, BulkReason, FsBatch, FsChange, BULK_PATH_CAP};
use super::WatchError;
use crate::util::now_ms;
use crate::vault::{strip_root, Vault};

/// More changes than this in one flush are collapsed into a bulk batch.
pub const BULK_THRESHOLD: usize = 50;
/// A window in which HEAD moved and at least this many files changed is a git operation (a checkout,
/// merge, pull or reset), even though the command is long finished when the window is flushed.
/// Fewer files are far more likely to be an agent saving a doc right before it commits.
pub const GIT_OPERATION_MIN_FILES: usize = 3;

/// Whether a flush that saw HEAD move and `count` changed files was most likely one git command.
pub fn head_move_is_operation(head_moved: bool, count: usize) -> bool {
    head_moved && count >= GIT_OPERATION_MIN_FILES
}

/// Detects a git operation in progress by looking for its marker files.
#[derive(Debug, Clone, Default)]
pub struct GitProbe {
    dirs: Vec<PathBuf>,
}

impl GitProbe {
    const MARKERS: [&'static str; 7] = [
        "index.lock",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "BISECT_LOG",
    ];

    /// `git_dir` is the repository's own directory, `common_dir` the shared one (they differ for worktrees).
    pub fn new(git_dir: Option<PathBuf>, common_dir: Option<PathBuf>) -> Self {
        let mut dirs: Vec<PathBuf> = git_dir.into_iter().chain(common_dir).collect();
        dirs.dedup();
        Self { dirs }
    }

    pub fn operation_in_progress(&self) -> bool {
        self.dirs
            .iter()
            .any(|d| Self::MARKERS.iter().any(|m| d.join(m).exists()))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WatchConfig {
    pub quiet: Duration,
    pub max_wait: Duration,
    pub bulk_threshold: usize,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            quiet: QUIET,
            max_wait: MAX_WAIT,
            bulk_threshold: BULK_THRESHOLD,
        }
    }
}

/// What one flush produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchOutput {
    /// The payload of the `fs-change` event (bulk batches carry at most 200 changes).
    pub batch: FsBatch,
    /// Every change, uncapped, for internal consumers such as the search index.
    pub all_changes: Vec<FsChange>,
    /// Files or folders appeared or disappeared: emit `tree-changed`.
    pub tree_changed: bool,
    /// A `.git` entry changed: the git worker should re-check.
    pub git_touched: bool,
}

impl WatchOutput {
    /// Whether the UI needs an `fs-change` event for this output.
    pub fn has_batch(&self) -> bool {
        !self.batch.changes.is_empty() || self.batch.bulk.is_some() || self.batch.rescan
    }
}

/// What else is known about a flush besides the changes themselves.
#[derive(Debug, Clone, Copy, Default)]
pub struct FlushFacts {
    pub tree_changed: bool,
    pub git_touched: bool,
    /// `Overflow` when events were lost, `GitOperation` while a git command is running.
    pub pressure: Option<BulkReason>,
}

/// Builds the output of a flush, deciding between a plain list and a bulk batch.
/// Returns `None` when there is nothing to tell anyone.
pub fn assemble(
    changes: Vec<FsChange>,
    facts: FlushFacts,
    bulk_threshold: usize,
) -> Option<WatchOutput> {
    let FlushFacts {
        tree_changed,
        git_touched,
        pressure,
    } = facts;
    let overflow = pressure == Some(BulkReason::Overflow);
    if changes.is_empty() && !tree_changed && !git_touched && !overflow {
        return None;
    }
    let count = changes.len();
    let reason = match pressure {
        Some(BulkReason::Overflow) => Some(BulkReason::Overflow),
        Some(BulkReason::GitOperation) if count > 0 => Some(BulkReason::GitOperation),
        _ if count > bulk_threshold => Some(BulkReason::Large),
        _ => None,
    };
    let shown: Vec<FsChange> = if reason.is_some() {
        changes.iter().take(BULK_PATH_CAP).cloned().collect()
    } else {
        changes.clone()
    };
    let bulk = reason.map(|reason| Bulk {
        count,
        reason,
        paths: shown.iter().map(|c| c.path.clone()).collect(),
    });
    Some(WatchOutput {
        batch: FsBatch {
            changes: shown,
            bulk,
            rescan: overflow,
        },
        all_changes: changes,
        tree_changed,
        git_touched,
    })
}

/// Translates a `notify` event into path hints.
pub(crate) fn raw_events(ev: &notify::Event) -> Vec<RawEvent> {
    let mk = |abs: &PathBuf, hint, is_dir| RawEvent {
        abs: abs.clone(),
        hint,
        is_dir,
        rename_from: None,
    };
    match ev.kind {
        EventKind::Access(_) => Vec::new(),
        EventKind::Create(kind) => ev
            .paths
            .iter()
            .map(|p| {
                mk(
                    p,
                    Hint::Create,
                    (kind == CreateKind::Folder).then_some(true),
                )
            })
            .collect(),
        EventKind::Remove(_) => ev.paths.iter().map(|p| mk(p, Hint::Remove, None)).collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            ev.paths.iter().map(|p| mk(p, Hint::Remove, None)).collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            ev.paths.iter().map(|p| mk(p, Hint::Create, None)).collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if ev.paths.len() == 2 => {
            let mut to = mk(&ev.paths[1], Hint::Create, None);
            to.rename_from = Some(ev.paths[0].clone());
            vec![mk(&ev.paths[0], Hint::Remove, None), to]
        }
        _ => ev.paths.iter().map(|p| mk(p, Hint::Modify, None)).collect(),
    }
}

enum Msg {
    Event(notify::Result<notify::Event>),
    Stop,
}

/// Owns the OS watchers and the aggregator thread. Dropping it stops both.
pub struct WatchService {
    watchers: Vec<RecommendedWatcher>,
    tx: Sender<Msg>,
    handle: Option<JoinHandle<()>>,
}

impl WatchService {
    /// Starts watching every root of `vault` (worktrees nested inside the main root are covered by it).
    /// `sink` is called on the aggregator thread, once per non-empty flush.
    pub fn start(
        vault: Arc<Vault>,
        filter: Arc<PathFilter>,
        sigs: Arc<SigStore>,
        probe: GitProbe,
        cfg: WatchConfig,
        sink: impl FnMut(WatchOutput) + Send + 'static,
    ) -> Result<Self, WatchError> {
        let (tx, rx) = mpsc::channel::<Msg>();
        let mut watchers = Vec::new();
        let main = vault.root().to_path_buf();
        for (idx, root) in vault.root_paths().into_iter().enumerate() {
            if idx > 0 && strip_root(root, &main).is_some() {
                continue;
            }
            let mode = if vault.is_shallow() {
                RecursiveMode::NonRecursive
            } else {
                RecursiveMode::Recursive
            };
            match watch_root(root, mode, tx.clone()) {
                Ok(w) => watchers.push(w),
                Err(e) if idx == 0 => return Err(e),
                Err(_) => {} // a worktree that vanished: the rest keep working
            }
        }
        let handle = thread::Builder::new()
            .name("trailmark-watch".to_owned())
            .spawn(move || run(&rx, &vault, &filter, &sigs, &probe, cfg, sink))
            .map_err(|e| WatchError::Spawn(e.to_string()))?;
        Ok(Self {
            watchers,
            tx,
            handle: Some(handle),
        })
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.watchers.clear(); // stops OS events first
        let _ = self.tx.send(Msg::Stop);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for WatchService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn watch_root(
    root: &Path,
    mode: RecursiveMode,
    tx: Sender<Msg>,
) -> Result<RecommendedWatcher, WatchError> {
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(Msg::Event(res));
    })
    .map_err(|e| WatchError::Notify(e.to_string()))?;
    watcher
        .watch(root, mode)
        .map_err(|e| WatchError::Notify(format!("{}: {e}", root.display())))?;
    Ok(watcher)
}

fn run(
    rx: &mpsc::Receiver<Msg>,
    vault: &Vault,
    filter: &PathFilter,
    sigs: &SigStore,
    probe: &GitProbe,
    cfg: WatchConfig,
    mut sink: impl FnMut(WatchOutput),
) {
    let mut agg = Aggregator::new(cfg.quiet, cfg.max_wait);
    loop {
        let wait = agg.deadline().map_or(Duration::from_hours(1), |d| {
            d.saturating_duration_since(Instant::now())
        });
        match rx.recv_timeout(wait) {
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Msg::Event(Ok(ev))) => {
                let now = Instant::now();
                if ev.need_rescan() {
                    agg.mark_overflow(now);
                }
                for raw in raw_events(&ev) {
                    agg.push(raw, vault, filter, now);
                }
            }
            Ok(Msg::Event(Err(_))) => agg.mark_overflow(Instant::now()),
            Err(RecvTimeoutError::Timeout) => {}
        }
        if agg.due(Instant::now()) {
            let flushed = agg.take();
            // After an overflow the remembered signatures may be stale, but they are kept: a stale one
            // only makes the next event for that file look like a change (which it is), while a wiped
            // store would report every antivirus touch as one. The consumer rescans on `rescan`.
            let env = Env {
                vault,
                filter,
                sigs,
            };
            let rec = reconcile(&env, flushed.paths, now_ms());
            for p in rec.deferred {
                agg.requeue(p, Instant::now());
            }
            // A moved-in folder that was only partly listed needs the same rescan as lost events.
            let pressure = if flushed.overflow || rec.truncated {
                Some(BulkReason::Overflow)
            } else if probe.operation_in_progress()
                || head_move_is_operation(flushed.head_moved, rec.changes.len())
            {
                Some(BulkReason::GitOperation)
            } else {
                None
            };
            let facts = FlushFacts {
                tree_changed: rec.tree_changed,
                git_touched: flushed.git_touched,
                pressure,
            };
            if let Some(out) = assemble(rec.changes, facts, cfg.bulk_threshold) {
                sink(out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::types::ChangeKind;
    use std::fs;
    use std::sync::mpsc::Receiver;

    fn change(path: &str) -> FsChange {
        FsChange {
            kind: ChangeKind::Modified,
            path: path.to_owned(),
            from: None,
            at_ms: 1,
        }
    }

    fn many(n: usize) -> Vec<FsChange> {
        (0..n).map(|i| change(&format!("f{i}.md"))).collect()
    }

    #[test]
    fn nothing_to_report_gives_no_output() {
        assert!(assemble(vec![], FlushFacts::default(), 50).is_none());
    }

    #[test]
    fn small_batches_are_plain() {
        let out = assemble(many(3), FlushFacts::default(), 50).expect("output");
        assert!(out.batch.bulk.is_none());
        assert_eq!(out.batch.changes.len(), 3);
        assert!(!out.batch.rescan);
        assert!(out.has_batch());
    }

    #[test]
    fn more_than_fifty_changes_are_bulk_large() {
        let out = assemble(many(50), FlushFacts::default(), 50).expect("output");
        assert!(out.batch.bulk.is_none(), "50 is still a normal batch");
        let out = assemble(
            many(143),
            FlushFacts {
                tree_changed: true,
                ..FlushFacts::default()
            },
            50,
        )
        .expect("output");
        let bulk = out.batch.bulk.expect("bulk");
        assert_eq!((bulk.count, bulk.reason), (143, BulkReason::Large));
        assert_eq!(bulk.paths.len(), 143.min(BULK_PATH_CAP));
        assert_eq!(out.all_changes.len(), 143);
        assert!(out.tree_changed);
    }

    #[test]
    fn bulk_paths_and_changes_are_capped_at_200() {
        let out = assemble(many(500), FlushFacts::default(), 50).expect("output");
        let bulk = out.batch.bulk.expect("bulk");
        assert_eq!(bulk.count, 500);
        assert_eq!(bulk.paths.len(), 200);
        assert_eq!(out.batch.changes.len(), 200);
        assert_eq!(out.all_changes.len(), 500);
    }

    #[test]
    fn git_operation_in_progress_makes_any_batch_bulk() {
        let out = assemble(
            many(2),
            FlushFacts {
                pressure: Some(BulkReason::GitOperation),
                ..FlushFacts::default()
            },
            50,
        )
        .expect("output");
        let bulk = out.batch.bulk.expect("bulk");
        assert_eq!(bulk.reason, BulkReason::GitOperation);
        assert_eq!(bulk.count, 2);
        // With nothing changed on disk there is nothing to collapse.
        let out = assemble(
            vec![],
            FlushFacts {
                git_touched: true,
                pressure: Some(BulkReason::GitOperation),
                ..FlushFacts::default()
            },
            50,
        )
        .expect("output");
        assert!(out.batch.bulk.is_none());
        assert!(!out.has_batch());
        assert!(out.git_touched);
    }

    #[test]
    fn overflow_requests_a_rescan() {
        let out = assemble(
            vec![],
            FlushFacts {
                pressure: Some(BulkReason::Overflow),
                ..FlushFacts::default()
            },
            50,
        )
        .expect("output");
        assert!(out.batch.rescan);
        assert!(out.has_batch());
        assert_eq!(
            out.batch.bulk.map(|b| (b.count, b.reason)),
            Some((0, BulkReason::Overflow))
        );
    }

    #[test]
    fn a_moved_head_with_several_changed_files_is_a_git_operation() {
        assert!(head_move_is_operation(true, GIT_OPERATION_MIN_FILES));
        assert!(head_move_is_operation(true, 8));
        assert!(!head_move_is_operation(true, GIT_OPERATION_MIN_FILES - 1));
        assert!(!head_move_is_operation(false, 200));
    }

    #[test]
    fn git_probe_sees_lock_and_operation_markers() {
        let d = tempfile::tempdir().expect("tempdir");
        let probe = GitProbe::new(Some(d.path().to_path_buf()), None);
        assert!(!probe.operation_in_progress());
        fs::write(d.path().join("index.lock"), "").expect("write");
        assert!(probe.operation_in_progress());
        fs::remove_file(d.path().join("index.lock")).expect("rm");
        fs::create_dir(d.path().join("rebase-merge")).expect("mkdir");
        assert!(probe.operation_in_progress());
        assert!(!GitProbe::default().operation_in_progress());
    }

    fn ev(kind: EventKind, paths: &[&str]) -> notify::Event {
        let mut e = notify::Event::new(kind);
        e.paths = paths.iter().map(PathBuf::from).collect();
        e
    }

    #[test]
    fn notify_events_map_to_hints() {
        use notify::event::{AccessKind, RemoveKind};
        let hints = |e: &notify::Event| -> Vec<(Hint, Option<String>)> {
            raw_events(e)
                .into_iter()
                .map(|r| {
                    (
                        r.hint,
                        r.rename_from.map(|p| p.to_string_lossy().into_owned()),
                    )
                })
                .collect()
        };
        assert!(raw_events(&ev(EventKind::Access(AccessKind::Any), &["a"])).is_empty());
        assert_eq!(
            hints(&ev(EventKind::Create(CreateKind::File), &["a"])),
            [(Hint::Create, None)]
        );
        assert_eq!(
            hints(&ev(EventKind::Remove(RemoveKind::File), &["a"])),
            [(Hint::Remove, None)]
        );
        assert_eq!(
            hints(&ev(
                EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
                &["a"]
            )),
            [(Hint::Modify, None)]
        );
        assert_eq!(
            hints(&ev(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                &["a"]
            )),
            [(Hint::Remove, None)]
        );
        assert_eq!(
            hints(&ev(
                EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                &["b"]
            )),
            [(Hint::Create, None)]
        );
        assert_eq!(
            hints(&ev(
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                &["a", "b"]
            )),
            [(Hint::Remove, None), (Hint::Create, Some("a".to_owned()))]
        );
        let folder = raw_events(&ev(EventKind::Create(CreateKind::Folder), &["d"]));
        assert_eq!(folder[0].is_dir, Some(true));
    }

    struct Live {
        _dir: tempfile::TempDir,
        root: PathBuf,
        svc: WatchService,
        rx: Receiver<WatchOutput>,
    }

    fn live() -> Live {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(Vault::open(dir.path()).expect("open"));
        let root = vault.root().to_path_buf();
        let filter = Arc::new(PathFilter::new(&[(root.clone(), None)], &[]));
        let (tx, rx) = mpsc::channel();
        let svc = WatchService::start(
            vault,
            filter,
            Arc::new(SigStore::new()),
            GitProbe::default(),
            WatchConfig {
                quiet: Duration::from_millis(60),
                max_wait: Duration::from_millis(400),
                bulk_threshold: 50,
            },
            move |out| {
                let _ = tx.send(out);
            },
        )
        .expect("start");
        Live {
            _dir: dir,
            root,
            svc,
            rx,
        }
    }

    /// Collects outputs until `done` is satisfied or the timeout passes.
    fn wait_for(
        rx: &Receiver<WatchOutput>,
        mut done: impl FnMut(&[WatchOutput]) -> bool,
    ) -> Vec<WatchOutput> {
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(out) = rx.recv_timeout(Duration::from_millis(200)) {
                seen.push(out);
                if done(&seen) {
                    break;
                }
            }
        }
        seen
    }

    fn all_changes(outs: &[WatchOutput]) -> Vec<(ChangeKind, String)> {
        outs.iter()
            .flat_map(|o| o.all_changes.iter().map(|c| (c.kind, c.path.clone())))
            .collect()
    }

    #[test]
    fn real_filesystem_events_become_one_clean_batch() {
        let live = live();
        fs::write(live.root.join("a.md"), "hello").expect("write");
        fs::write(live.root.join(".a.md.swp"), "junk").expect("write");
        fs::write(live.root.join("tmp.tmp"), "junk").expect("write");
        let outs = wait_for(&live.rx, |o| {
            all_changes(o).iter().any(|(_, p)| p == "a.md")
        });
        let changes = all_changes(&outs);
        assert!(
            changes
                .iter()
                .any(|(k, p)| p == "a.md" && *k == ChangeKind::Created),
            "{changes:?}"
        );
        assert!(
            changes.iter().all(|(_, p)| p == "a.md"),
            "noise leaked: {changes:?}"
        );
        assert!(outs.iter().any(|o| o.tree_changed));

        // A real edit is reported as modified; re-saving identical bytes is not reported.
        std::thread::sleep(Duration::from_millis(30));
        fs::write(live.root.join("a.md"), "hello world").expect("write");
        let outs = wait_for(&live.rx, |o| !all_changes(o).is_empty());
        assert_eq!(
            all_changes(&outs),
            [(ChangeKind::Modified, "a.md".to_owned())]
        );

        live.svc.stop();
    }

    #[test]
    fn moving_in_a_huge_folder_asks_the_consumers_to_rescan() {
        let live = live();
        let outside = tempfile::tempdir().expect("tempdir");
        let big = outside.path().join("big");
        fs::create_dir(&big).expect("mkdir");
        for i in 0..520 {
            fs::write(big.join(format!("n{i}.md")), "x").expect("write");
        }
        fs::rename(&big, live.root.join("big")).expect("move in");
        let outs = wait_for(&live.rx, |o| o.iter().any(|out| out.batch.rescan));
        assert!(
            outs.iter().any(|o| o.batch.rescan),
            "the files past the listing cap must trigger a rescan: {} outputs",
            outs.len()
        );
    }

    #[test]
    fn deleting_a_file_is_reported() {
        let live = live();
        fs::write(live.root.join("gone.md"), "x").expect("write");
        wait_for(&live.rx, |o| !all_changes(o).is_empty());
        fs::remove_file(live.root.join("gone.md")).expect("rm");
        let outs = wait_for(&live.rx, |o| !all_changes(o).is_empty());
        assert_eq!(
            all_changes(&outs),
            [(ChangeKind::Removed, "gone.md".to_owned())]
        );
        assert!(outs.iter().any(|o| o.tree_changed));
    }

    #[test]
    fn stopping_twice_or_dropping_does_not_hang() {
        let live = live();
        drop(live.svc);
    }
}
