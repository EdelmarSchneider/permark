//! Collects raw filesystem events into one pending set per path and decides when to flush it
//! (150 ms of quiet, but never later than 1 s after the first event). Pure logic: it never touches
//! the disk, so it is tested with synthetic events and synthetic clocks.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::filter::{PathFilter, Verdict};
use crate::util::Coalescer;
use crate::vault::{key, Vault};

pub const QUIET: Duration = Duration::from_millis(150);
pub const MAX_WAIT: Duration = Duration::from_secs(1);

/// What the OS said happened to a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    Create,
    Modify,
    Remove,
}

#[derive(Debug, Clone)]
pub struct RawEvent {
    pub abs: PathBuf,
    pub hint: Hint,
    /// `Some(true)` when the OS said the path is a folder.
    pub is_dir: Option<bool>,
    /// The old path when this event is the target side of a rename.
    pub rename_from: Option<PathBuf>,
}

/// Everything seen for one path during the current window.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // independent facts observed about one path
pub struct PendingPath {
    pub abs: PathBuf,
    /// Vault-relative path in the UI form.
    pub rel: String,
    pub created: bool,
    pub removed: bool,
    pub modified: bool,
    pub is_dir: bool,
    /// UI path of the old name when the path was the target of a rename.
    pub rename_from: Option<String>,
    /// Set once a truncated (0-byte) file was given one more tick to receive its content.
    pub deferred: bool,
}

/// The result of [`Aggregator::take`].
#[derive(Debug, Default)]
pub struct Flushed {
    pub paths: Vec<PendingPath>,
    /// The OS dropped events (or the watcher failed): a full rescan is needed.
    pub overflow: bool,
    /// A `.git` entry appeared or disappeared (the git worker should look again).
    pub git_touched: bool,
    /// HEAD's reflog grew during the window: a commit, checkout, merge, reset, pull or rebase happened.
    pub head_moved: bool,
}

pub struct Aggregator {
    pending: HashMap<String, PendingPath>,
    order: Vec<String>,
    coalescer: Coalescer,
    overflow: bool,
    git_touched: bool,
    head_moved: bool,
}

impl Aggregator {
    pub fn new(quiet: Duration, max_wait: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            order: Vec::new(),
            coalescer: Coalescer::new(quiet, max_wait),
            overflow: false,
            git_touched: false,
            head_moved: false,
        }
    }

    /// Adds an event, unless the filter says it is noise or it lies outside every root.
    pub fn push(&mut self, ev: RawEvent, vault: &Vault, filter: &PathFilter, now: Instant) {
        let Some(loc) = vault.locate(&ev.abs) else {
            return;
        };
        let (rel, root) = (loc.rel, loc.root);
        if is_head_log(&loc.inner) {
            // Not a change anyone should see; it only tells the flush that a git command moved HEAD.
            self.head_moved = true;
            return;
        }
        let is_dir_guess = ev.is_dir.unwrap_or_else(|| ev.abs.is_dir());
        match filter.classify(root, &loc.inner, is_dir_guess) {
            Verdict::Ignore => return,
            Verdict::GitOnly => {
                self.git_touched = true;
                self.coalescer.touch(now);
                return;
            }
            Verdict::Emit => {}
        }
        let k = key(&rel);
        let order = &mut self.order;
        let entry = self.pending.entry(k.clone()).or_insert_with(|| {
            order.push(k);
            PendingPath {
                abs: ev.abs.clone(),
                rel: rel.clone(),
                created: false,
                removed: false,
                modified: false,
                is_dir: false,
                rename_from: None,
                deferred: false,
            }
        });
        match ev.hint {
            Hint::Create => entry.created = true,
            Hint::Modify => entry.modified = true,
            Hint::Remove => entry.removed = true,
        }
        entry.is_dir |= ev.is_dir == Some(true);
        if let Some(from) = ev.rename_from {
            entry.rename_from = vault.to_rel(&from).map(|(r, _)| r);
        }
        self.coalescer.touch(now);
    }

    /// Re-queues a path (a 0-byte file that gets one more tick).
    pub fn requeue(&mut self, mut path: PendingPath, now: Instant) {
        path.deferred = true;
        let k = key(&path.rel);
        if !self.pending.contains_key(&k) {
            self.order.push(k.clone());
            self.pending.insert(k, path);
        }
        self.coalescer.touch(now);
    }

    /// The OS lost events or the watcher errored.
    pub fn mark_overflow(&mut self, now: Instant) {
        self.overflow = true;
        self.coalescer.touch(now);
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.coalescer.deadline()
    }

    pub fn due(&self, now: Instant) -> bool {
        self.coalescer.due(now)
    }

    #[cfg(test)]
    pub fn is_pending(&self) -> bool {
        self.coalescer.is_pending()
    }

    /// Drains the window.
    pub fn take(&mut self) -> Flushed {
        let mut pending = std::mem::take(&mut self.pending);
        let paths = std::mem::take(&mut self.order)
            .into_iter()
            .filter_map(|k| pending.remove(&k))
            .collect();
        self.coalescer.reset();
        Flushed {
            paths,
            overflow: std::mem::take(&mut self.overflow),
            git_touched: std::mem::take(&mut self.git_touched),
            head_moved: std::mem::take(&mut self.head_moved),
        }
    }
}

/// `.git/logs/HEAD` (or a linked worktree's `.git/worktrees/<name>/logs/HEAD`): appended whenever HEAD moves.
fn is_head_log(inner: &str) -> bool {
    let comps: Vec<&str> = inner.split('/').filter(|c| !c.is_empty()).collect();
    let is = |c: &str, want: &str| c.eq_ignore_ascii_case(want);
    match comps.as_slice() {
        [git, logs, head] => is(git, ".git") && is(logs, "logs") && is(head, "HEAD"),
        [git, wts, _, logs, head] => {
            is(git, ".git") && is(wts, "worktrees") && is(logs, "logs") && is(head, "HEAD")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        dir: tempfile::TempDir,
        vault: Vault,
        filter: PathFilter,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = Vault::open(dir.path()).expect("open");
        let filter = PathFilter::new(&[(vault.root().to_path_buf(), None)], &[]);
        Fixture { dir, vault, filter }
    }

    fn ev(f: &Fixture, rel: &str, hint: Hint) -> RawEvent {
        RawEvent {
            abs: f.vault.root().join(rel),
            hint,
            is_dir: None,
            rename_from: None,
        }
    }

    #[test]
    fn events_for_one_path_merge_into_one_entry_in_arrival_order() {
        let f = fixture();
        let t0 = Instant::now();
        let mut a = Aggregator::new(QUIET, MAX_WAIT);
        a.push(ev(&f, "b.md", Hint::Create), &f.vault, &f.filter, t0);
        a.push(ev(&f, "a.md", Hint::Modify), &f.vault, &f.filter, t0);
        a.push(ev(&f, "b.md", Hint::Modify), &f.vault, &f.filter, t0);
        let out = a.take();
        let paths: Vec<&str> = out.paths.iter().map(|p| p.rel.as_str()).collect();
        assert_eq!(paths, ["b.md", "a.md"]);
        assert!(out.paths[0].created && out.paths[0].modified && !out.paths[0].removed);
        assert!(!a.is_pending());
    }

    #[test]
    fn noise_is_dropped_before_it_is_pending() {
        let f = fixture();
        let t0 = Instant::now();
        let mut a = Aggregator::new(QUIET, MAX_WAIT);
        for rel in [
            ".git/index",
            "node_modules/x.js",
            ".a.md.swp",
            "a.md~",
            "4913",
            "x.tmp",
            ".trailmark/local/s.json",
        ] {
            a.push(ev(&f, rel, Hint::Modify), &f.vault, &f.filter, t0);
        }
        assert!(!a.is_pending());
        assert!(a.take().paths.is_empty());
    }

    #[test]
    fn events_outside_every_root_are_dropped() {
        let f = fixture();
        let other = tempfile::tempdir().expect("tempdir");
        let mut a = Aggregator::new(QUIET, MAX_WAIT);
        a.push(
            RawEvent {
                abs: other.path().join("a.md"),
                hint: Hint::Modify,
                is_dir: None,
                rename_from: None,
            },
            &f.vault,
            &f.filter,
            Instant::now(),
        );
        assert!(!a.is_pending());
        drop(f.dir);
    }

    #[test]
    fn flush_timing_is_quiet_then_capped() {
        let f = fixture();
        let t0 = Instant::now();
        let mut a = Aggregator::new(QUIET, MAX_WAIT);
        a.push(ev(&f, "a.md", Hint::Modify), &f.vault, &f.filter, t0);
        assert!(!a.due(t0 + Duration::from_millis(100)));
        assert!(a.due(t0 + Duration::from_millis(150)));
        // A steady stream keeps postponing the flush, but only up to one second.
        for ms in (50..1000).step_by(50) {
            a.push(
                ev(&f, "a.md", Hint::Modify),
                &f.vault,
                &f.filter,
                t0 + Duration::from_millis(ms),
            );
        }
        assert!(!a.due(t0 + Duration::from_millis(990)));
        assert!(a.due(t0 + Duration::from_secs(1)));
    }

    #[test]
    fn git_entry_wakes_git_without_becoming_a_change() {
        let f = fixture();
        let t0 = Instant::now();
        let mut a = Aggregator::new(QUIET, MAX_WAIT);
        a.push(ev(&f, ".git", Hint::Create), &f.vault, &f.filter, t0);
        assert!(a.is_pending());
        let out = a.take();
        assert!(out.git_touched);
        assert!(out.paths.is_empty());
    }

    #[test]
    fn head_reflog_growth_is_remembered_without_becoming_a_change() {
        let f = fixture();
        let mut a = Aggregator::new(QUIET, MAX_WAIT);
        a.push(
            ev(&f, ".git/logs/HEAD", Hint::Modify),
            &f.vault,
            &f.filter,
            Instant::now(),
        );
        a.push(
            ev(&f, ".git/refs/heads/main", Hint::Modify),
            &f.vault,
            &f.filter,
            Instant::now(),
        );
        let out = a.take();
        assert!(out.head_moved);
        assert!(out.paths.is_empty());
        assert!(!a.take().head_moved, "reported once");
    }

    #[test]
    fn only_the_head_reflog_counts_as_head_moving() {
        assert!(is_head_log(".git/logs/HEAD"));
        assert!(is_head_log(".GIT/logs/head"));
        assert!(is_head_log(".git/worktrees/ui/logs/HEAD"));
        assert!(!is_head_log(".git/logs/refs/heads/main"));
        assert!(!is_head_log(".git/HEAD"));
        assert!(!is_head_log("docs/logs/HEAD"));
        assert!(!is_head_log("sub/.git/logs/HEAD"));
    }

    #[test]
    fn overflow_is_reported_once() {
        let mut a = Aggregator::new(QUIET, MAX_WAIT);
        a.mark_overflow(Instant::now());
        assert!(a.take().overflow);
        assert!(!a.take().overflow);
    }

    #[test]
    fn rename_target_remembers_the_old_path() {
        let f = fixture();
        let mut a = Aggregator::new(QUIET, MAX_WAIT);
        let mut e = ev(&f, "new.md", Hint::Create);
        e.rename_from = Some(f.vault.root().join("old.md"));
        a.push(e, &f.vault, &f.filter, Instant::now());
        let out = a.take();
        assert_eq!(out.paths[0].rename_from.as_deref(), Some("old.md"));
    }

    #[test]
    fn case_differences_merge_on_windows() {
        let f = fixture();
        let mut a = Aggregator::new(QUIET, MAX_WAIT);
        let t0 = Instant::now();
        a.push(ev(&f, "Readme.md", Hint::Modify), &f.vault, &f.filter, t0);
        a.push(ev(&f, "README.md", Hint::Modify), &f.vault, &f.filter, t0);
        assert_eq!(a.take().paths.len(), if cfg!(windows) { 1 } else { 2 });
    }

    #[test]
    fn requeued_paths_are_marked_deferred() {
        let f = fixture();
        let mut a = Aggregator::new(QUIET, MAX_WAIT);
        a.push(
            ev(&f, "a.md", Hint::Modify),
            &f.vault,
            &f.filter,
            Instant::now(),
        );
        let p = a.take().paths.remove(0);
        a.requeue(p, Instant::now());
        let again = a.take().paths;
        assert_eq!(again.len(), 1);
        assert!(again[0].deferred);
    }
}
