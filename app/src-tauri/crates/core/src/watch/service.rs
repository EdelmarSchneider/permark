//! `Watcher`: the OS watchers of a project + the aggregator thread. Raw events become hints; after a quiet moment
//! (150 ms, at most 1 s) the hinted paths are reconciled with the disk and published as one [`FsBatch`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use super::own::OwnWrites;
use super::reconcile::{reconcile, Env, Pending};
use super::sigs::{sig_of, stat_sig, SigStore, HASH_LIMIT};
use super::types::{FsBatch, BULK_CHANGES};
use super::wsl::is_wsl;
use super::{LiveStatus, StatusHook, WatchHub};
use crate::paths::Vault;
use crate::tree::{key_of, Ignorer};

/// Pending paths beyond this: the picture is given up and a rescan is asked for.
const MAX_PENDING: usize = 50_000;

/// Timings of the aggregation.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Quiet time after the last event before a flush.
    pub quiet: Duration,
    /// Longest time between the first event and the flush.
    pub max: Duration,
    /// Poll interval of the open documents while live updates are unavailable.
    pub poll: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(150),
            max: Duration::from_secs(1),
            poll: Duration::from_secs(5),
        }
    }
}

enum Msg {
    Hint(Pending),
    /// Events were lost.
    Rescan,
    /// The watcher reported an error (text for the log; the root being gone makes updates unavailable).
    Failed,
    Stop,
}

struct Shared {
    vault: Vault,
    ignorers: Vec<(String, Arc<Ignorer>)>,
    status: Mutex<LiveStatus>,
    own: Arc<OwnWrites>,
    sigs: SigStore,
    hub: Arc<WatchHub>,
    /// Open document per window, polled while live updates are unavailable.
    polled: Mutex<HashMap<String, PathBuf>>,
    hook: Option<StatusHook>,
    timing: Timing,
    tree: OnceLock<PathLookup>,
    /// Hints taken into the pending set (each one is looked at on disk at the flush).
    #[cfg(test)]
    admitted: std::sync::atomic::AtomicUsize,
}

/// What the tree holds at a project path, once it is built. `None` from the lookup: no node (or the tree is not built).
///
/// [`PathHold::OpenedIgnored`] is an ignored directory the person opened. A container and a normal folder are
/// [`PathHold::Dir`]: still folders for `known`, not a reason to let an ignored child through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathHold {
    File,
    /// Any other directory: closed ignored, large, a normal folder, or a container.
    Dir,
    /// `node.ignored` and `NodeKind::Dir { unlisted: None }`.
    OpenedIgnored,
}

/// What the project's tree holds at a project path.
pub type PathLookup = Box<dyn Fn(&str) -> Option<PathHold> + Send + Sync>;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    /// What the reconcile pass knows of this project: the tree before the watch knew anything, opened ignored folders.
    fn with_env<R>(&self, f: impl FnOnce(&Env<'_>) -> R) -> R {
        let known = |path: &str| {
            self.own.moved_from(path).or_else(|| {
                self.tree
                    .get()
                    .and_then(|tree| tree(path).map(|hold| !matches!(hold, PathHold::File)))
            })
        };
        let opened_ignored = |path: &str| {
            self.tree
                .get()
                .is_some_and(|tree| matches!(tree(path), Some(PathHold::OpenedIgnored)))
        };
        let wanted = |path: &str| self.own.expects_write(path);
        f(&Env {
            vault: &self.vault,
            sigs: &self.sigs,
            ignorers: &self.ignorers,
            known: &known,
            opened_ignored: &opened_ignored,
            wanted: &wanted,
        })
    }

    fn set_unavailable(&self, reason: &str) {
        let changed = {
            let mut status = lock(&self.status);
            let next = LiveStatus::Unavailable {
                reason: reason.to_owned(),
            };
            let changed = *status != next;
            *status = next;
            changed
        };
        if changed {
            if let Some(hook) = &self.hook {
                hook();
            }
        }
    }
}

pub struct Watcher {
    shared: Arc<Shared>,
    tx: Sender<Msg>,
    natives: Mutex<Vec<RecommendedWatcher>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Watcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watcher").finish_non_exhaustive()
    }
}

/// Hints for the paths of one OS event.
pub(crate) fn hints_from(event: &Event) -> Vec<Pending> {
    let blank = |abs: &Path| Pending {
        abs: abs.to_path_buf(),
        created: false,
        removed: false,
        modified: false,
        rename_from: None,
        after_removal: None,
        deferred: false,
    };
    let mut hints = Vec::new();
    match &event.kind {
        EventKind::Create(_) => {
            for path in &event.paths {
                hints.push(Pending {
                    created: true,
                    ..blank(path)
                });
            }
        }
        EventKind::Remove(_) => {
            for path in &event.paths {
                hints.push(Pending {
                    removed: true,
                    ..blank(path)
                });
            }
        }
        EventKind::Modify(ModifyKind::Name(mode)) => match (mode, event.paths.as_slice()) {
            (RenameMode::Both, [from, to]) => {
                hints.push(Pending {
                    removed: true,
                    ..blank(from)
                });
                hints.push(Pending {
                    created: true,
                    rename_from: Some(from.clone()),
                    after_removal: None,
                    ..blank(to)
                });
            }
            (RenameMode::From, paths) => {
                for path in paths {
                    hints.push(Pending {
                        removed: true,
                        ..blank(path)
                    });
                }
            }
            (RenameMode::To, paths) => {
                for path in paths {
                    hints.push(Pending {
                        created: true,
                        ..blank(path)
                    });
                }
            }
            // no direction (macOS): the disk says which side this path is
            (_, paths) => {
                for path in paths {
                    let exists = std::fs::symlink_metadata(path).is_ok();
                    hints.push(Pending {
                        created: exists,
                        removed: !exists,
                        ..blank(path)
                    });
                }
            }
        },
        EventKind::Modify(_) | EventKind::Any => {
            for path in &event.paths {
                hints.push(Pending {
                    modified: true,
                    ..blank(path)
                });
            }
        }
        EventKind::Access(_) | EventKind::Other => {}
    }
    hints
}

/// Pairs the two halves of a move the OS reports as two events in a row: a rename (Windows: old name, then new name;
/// macOS: two events without direction) -> the new name's hint learns where it came from, as with a one-event rename;
/// a removal, then a creation (Windows: a move to another parent) -> the creation's hint keeps it as a maybe
/// (`after_removal`). Any other event in between ends the pair.
#[derive(Debug, Default)]
pub(crate) struct RenamePairs {
    /// (path, half of a rename | a plain removal)
    out: Option<(PathBuf, bool)>,
}

impl RenamePairs {
    /// [`hints_from`], a hint linked to the removal just before it.
    pub(crate) fn hints(&mut self, event: &Event) -> Vec<Pending> {
        let mut hints = hints_from(event);
        let (half, removal, creation) = match &event.kind {
            EventKind::Modify(ModifyKind::Name(mode)) => {
                (!matches!(mode, RenameMode::Both), false, false)
            }
            EventKind::Remove(_) => (false, true, false),
            EventKind::Create(_) => (false, false, true),
            _ => (false, false, false),
        };
        let out = self.out.take();
        if let [one] = hints.as_mut_slice() {
            if (half || removal) && one.removed {
                self.out = Some((one.abs.clone(), half));
            } else if let Some((from, renamed)) = out.filter(|(from, _)| *from != one.abs) {
                if half && renamed && one.created {
                    one.rename_from = Some(from);
                } else if creation && !renamed {
                    one.after_removal = Some(from);
                }
            }
        }
        hints
    }
}

/// Markers git leaves in its folder for the length of an operation: a merge | cherry-pick | revert waiting for its
/// commit, a rebase, a bisect (they may stay for hours).
const OPERATION_MARKERS: [&str; 6] = [
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "BISECT_LOG",
    "rebase-merge",
    "rebase-apply",
];

/// Which [`OPERATION_MARKERS`] exist in the project's git folder (bit `i` = marker `i`).
fn operation_markers(root: &Path) -> u8 {
    let git = root.join(".git");
    OPERATION_MARKERS
        .iter()
        .enumerate()
        .filter(|(_, marker)| git.join(marker).exists())
        .fold(0, |bits, (i, _)| bits | (1 << i))
}

/// Whether git's work makes this flush a bulk one. `markers`: the operation markers as the last flush found them
/// (updated). The index lock counts while it exists (git is writing now, or a crashed git left it); a long-lived marker
/// counts only on the flush where the markers appear | change, or when the batch carries the repository's own signals
/// (HEAD, index, refs: the operation moved on). A plain save during a merge is no bulk batch.
fn git_operation(root: &Path, markers: &mut u8, git_signals: bool) -> bool {
    let now = operation_markers(root);
    let changed = now != *markers;
    *markers = now;
    root.join(".git").join("index.lock").exists() || (now != 0 && (changed || git_signals))
}

impl Watcher {
    /// Starts watching every root of `vault`. Never fails: when the OS watchers cannot start (or the project is in a
    /// WSL folder, not watched), [`Watcher::status`] says so and the open documents are polled.
    #[must_use]
    pub fn start(
        vault: &Vault,
        ignorers: Vec<(String, Arc<Ignorer>)>,
        hub: Arc<WatchHub>,
        own: Arc<OwnWrites>,
        hook: Option<StatusHook>,
    ) -> Self {
        Self::start_with(vault, ignorers, hub, own, hook, Timing::default(), true)
    }

    /// [`Watcher::start`] with timings; `native: false` starts without OS watchers (live updates unavailable).
    #[must_use]
    pub fn start_with(
        vault: &Vault,
        ignorers: Vec<(String, Arc<Ignorer>)>,
        hub: Arc<WatchHub>,
        own: Arc<OwnWrites>,
        hook: Option<StatusHook>,
        timing: Timing,
        native: bool,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            vault: vault.clone(),
            ignorers,
            status: Mutex::new(LiveStatus::Available),
            own,
            sigs: SigStore::default(),
            hub,
            polled: Mutex::new(HashMap::new()),
            hook,
            timing,
            tree: OnceLock::new(),
            #[cfg(test)]
            admitted: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut natives = Vec::new();
        if native && is_wsl(vault.main().path()) {
            // the WSL share accepts a watch and never reports: say so rather than follow nothing
            *lock(&shared.status) = LiveStatus::Unavailable {
                reason: "wsl".to_owned(),
            };
        } else if native {
            match start_natives(vault, &tx) {
                Ok(started) => natives = started,
                Err(()) => {
                    *lock(&shared.status) = LiveStatus::Unavailable {
                        reason: "watchFailed".to_owned(),
                    }
                }
            }
        } else {
            *lock(&shared.status) = LiveStatus::Unavailable {
                reason: "watchFailed".to_owned(),
            };
        }
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("permark-watch".to_owned())
                .spawn(move || run(&shared, &rx))
                .ok()
        };
        if thread.is_none() {
            shared.set_unavailable("watchFailed");
        }
        Self {
            shared,
            tx,
            natives: Mutex::new(natives),
            thread,
        }
    }

    /// Where the watch learns what was there before it began, and which ignored directories are opened
    /// ([`PathHold::OpenedIgnored`], not every folder and not a container). A file or folder that goes away without
    /// ever having been news is then reported as what it was. Set once.
    pub fn knows_tree(&self, lookup: PathLookup) {
        let _ = self.shared.tree.set(lookup);
    }

    #[must_use]
    pub fn status(&self) -> LiveStatus {
        lock(&self.shared.status).clone()
    }

    /// The app's own writes: announce before doing them.
    #[must_use]
    pub fn own(&self) -> &Arc<OwnWrites> {
        &self.shared.own
    }

    #[must_use]
    pub fn hub(&self) -> &Arc<WatchHub> {
        &self.shared.hub
    }

    /// The document a window shows: polled every few seconds while live updates are unavailable. `None`: none.
    pub fn poll_document(&self, window: &str, document: Option<&Path>) {
        let polling = matches!(*lock(&self.shared.status), LiveStatus::Unavailable { .. });
        let mut polled = lock(&self.shared.polled);
        match document {
            Some(path) => {
                if let (Some(vp), Ok(meta)) = (
                    self.shared.vault.relativize(path),
                    std::fs::symlink_metadata(path),
                ) {
                    let (key, sigs) = (key_of(vp.as_str()), &self.shared.sigs);
                    let held = sigs.get(&key);
                    // with live updates what the watch holds for the path stays: the file is read only for a path
                    // it knows nothing of
                    if polling || held.is_none() {
                        match sig_of(path, &meta) {
                            // nothing but the polls tells of the file: what the window read is what is known, and
                            // the first poll must not report a change that is none
                            Some(sig) if polling => sigs.set(key, vp.as_str(), sig),
                            // the window's reading tells the subscribers nothing: kept apart from what the watch
                            // told (a new file the app made and shows at once is still to be news)
                            Some(sig) => sigs.note_read(key, vp.as_str(), sig),
                            None => {}
                        }
                    } else if held.is_some_and(|s| s.hash == 0)
                        && meta.is_file()
                        && meta.len() <= HASH_LIMIT
                    {
                        // a new big file the watch left unread: shown now, so read once, or every touch that
                        // leaves its bytes would be news. Taken only if nothing changed it while it was read
                        if let Some(sig) = sig_of(path, &meta) {
                            let stat = (sig.len, sig.mtime_ms);
                            if std::fs::symlink_metadata(path).is_ok_and(|m| stat_sig(&m) == stat) {
                                sigs.fill_hash(&key, stat, sig.hash);
                            }
                        }
                    }
                }
                polled.insert(window.to_owned(), path.to_path_buf());
            }
            None => {
                polled.remove(window);
            }
        }
    }

    /// Asks everything that keeps a picture of the disk to build it again, as when reports were lost (the system
    /// drops them without a word when too many come at once): one bulk batch that asks to rebuild follows after the
    /// quiet moment. For the person's "Refresh".
    pub fn rescan(&self) {
        let _ = self.tx.send(Msg::Rescan);
    }

    /// Test hook: feeds a raw hint as if the OS had sent it.
    #[cfg(test)]
    pub(crate) fn inject(&self, hint: Pending) {
        let _ = self.tx.send(Msg::Hint(hint));
    }

    /// Test hook: how many hints were taken into the pending set so far.
    #[cfg(test)]
    pub(crate) fn admitted(&self) -> usize {
        self.shared
            .admitted
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Test hook: live updates stop, as when the OS watch fails with the project folder out of reach.
    #[cfg(test)]
    pub(crate) fn stop_live(&self) {
        self.shared.set_unavailable("watchFailed");
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // OS watchers first: no event arrives after this; then the thread
        self.natives
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            // dropped from a subscriber running on the watch thread itself: it ends on its own
            if thread.thread().id() != std::thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

/// OS watchers for the project folder and the worktrees outside it. `Err`: the project folder cannot be watched.
fn start_natives(vault: &Vault, tx: &Sender<Msg>) -> Result<Vec<RecommendedWatcher>, ()> {
    let mut roots: Vec<(PathBuf, bool)> = vec![(vault.main().path().to_path_buf(), true)];
    for worktree in vault.worktrees() {
        let nested = worktree.root.path().starts_with(vault.main().path());
        if !nested {
            roots.push((worktree.root.path().to_path_buf(), false));
        }
    }
    let mut started = Vec::new();
    for (path, required) in roots {
        let sender = tx.clone();
        let mut pairs = RenamePairs::default();
        let handler = move |result: notify::Result<Event>| match result {
            Ok(event) => {
                if event.need_rescan() {
                    let _ = sender.send(Msg::Rescan);
                }
                for hint in pairs.hints(&event) {
                    let _ = sender.send(Msg::Hint(hint));
                }
            }
            Err(_) => {
                let _ = sender.send(Msg::Failed);
            }
        };
        // a link inside the project is never followed (inotify | kqueue would watch its target, `/` included); the
        // other systems never follow one
        let config = notify::Config::default().with_follow_symlinks(false);
        let watcher = RecommendedWatcher::new(handler, config)
            .and_then(|mut w| w.watch(&path, RecursiveMode::Recursive).map(|()| w));
        match watcher {
            Ok(watcher) => started.push(watcher),
            Err(_) if required => return Err(()),
            Err(_) => {}
        }
    }
    Ok(started)
}

/// Pending paths in arrival order, merged per path.
#[derive(Default)]
struct PendingSet {
    order: Vec<String>,
    by_key: HashMap<String, Pending>,
}

impl PendingSet {
    fn add(&mut self, hint: Pending) {
        let key = key_of(&hint.abs.to_string_lossy());
        if let Some(existing) = self.by_key.get_mut(&key) {
            existing.merge(&hint);
        } else {
            self.order.push(key.clone());
            self.by_key.insert(key, hint);
        }
    }

    fn len(&self) -> usize {
        self.order.len()
    }

    fn take(&mut self) -> Vec<Pending> {
        let order = std::mem::take(&mut self.order);
        let mut by_key = std::mem::take(&mut self.by_key);
        order
            .into_iter()
            .filter_map(|k| by_key.remove(&k))
            .collect()
    }
}

/// Whether a hint may change the ignore decisions read so far: an ignore file, or a `.git` that came | went | moved
/// (a nested repository has rules of its own).
fn rules_may_change(hint: &Pending) -> bool {
    let named = |abs: &Path, is: fn(&str) -> bool| {
        abs.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is)
    };
    [Some(hint.abs.as_path()), hint.rename_from.as_deref()]
        .into_iter()
        .flatten()
        .any(|abs| named(abs, Ignorer::is_ignore_file) || named(abs, Ignorer::is_git_name))
}

fn run(shared: &Shared, rx: &Receiver<Msg>) {
    // an operation already under way when the watch began is no news by itself
    let mut markers = operation_markers(shared.vault.main().path());
    let mut pending = PendingSet::default();
    // folders found ignored while hints arrive ([`Env::ignored_early`]); `None` until the next flush once the rules
    // may be changing (an ignore file, a `.git` came or went: the flush reads them again)
    let mut ignored_folders: Option<HashMap<PathBuf, bool>> = Some(HashMap::new());
    let mut rescan = false;
    let mut first: Option<Instant> = None;
    let mut last = Instant::now();
    let mut next_poll = Instant::now() + shared.timing.poll;
    loop {
        let now = Instant::now();
        let wait = match first {
            Some(first) => {
                let quiet_at = last + shared.timing.quiet;
                let max_at = first + shared.timing.max;
                quiet_at.min(max_at).saturating_duration_since(now)
            }
            None => next_poll.saturating_duration_since(now),
        };
        match rx.recv_timeout(wait) {
            Ok(Msg::Hint(hint)) => {
                // dropped hints still reach the flush check below: a storm never holds a due flush back. An ignore
                // file in a closed ignored folder (packages ship them) decides nothing outside it: dropped as well
                let dropped = ignored_folders.as_mut().is_some_and(|folders| {
                    shared.with_env(|env| env.ignored_early(&hint, folders))
                });
                if !dropped && rules_may_change(&hint) {
                    ignored_folders = None;
                }
                if !dropped {
                    let now = Instant::now();
                    first.get_or_insert(now);
                    last = now;
                    #[cfg(test)]
                    shared
                        .admitted
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    pending.add(hint);
                    if pending.len() > MAX_PENDING {
                        pending.take();
                        rescan = true;
                    }
                }
            }
            Ok(Msg::Rescan) => {
                let now = Instant::now();
                first.get_or_insert(now);
                last = now;
                rescan = true;
            }
            Ok(Msg::Failed) => {
                if shared.vault.main().path().exists() {
                    let now = Instant::now();
                    first.get_or_insert(now);
                    last = now;
                    rescan = true;
                } else {
                    shared.set_unavailable("watchFailed");
                }
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if let Some(started) = first {
            let due = now.saturating_duration_since(last) >= shared.timing.quiet
                || now.saturating_duration_since(started) >= shared.timing.max;
            if due {
                let deferred = flush(
                    shared,
                    pending.take(),
                    std::mem::take(&mut rescan),
                    &mut markers,
                );
                ignored_folders = Some(HashMap::new());
                first = None;
                for hint in deferred {
                    first.get_or_insert(now);
                    last = now;
                    pending.add(hint);
                }
            }
        }
        if now >= next_poll {
            next_poll = now + shared.timing.poll;
            if matches!(*lock(&shared.status), LiveStatus::Unavailable { .. }) {
                let polled: Vec<PathBuf> = lock(&shared.polled).values().cloned().collect();
                for abs in polled {
                    let missing = std::fs::symlink_metadata(&abs).is_err();
                    first.get_or_insert(now);
                    last = now;
                    pending.add(Pending {
                        abs,
                        created: false,
                        removed: missing,
                        modified: !missing,
                        rename_from: None,
                        after_removal: None,
                        deferred: false,
                    });
                }
            }
        }
    }
}

/// Reconciles `pending` with the disk and publishes the result. Returns paths to look at again. `markers`: see
/// [`git_operation`].
fn flush(shared: &Shared, pending: Vec<Pending>, rescan: bool, markers: &mut u8) -> Vec<Pending> {
    // an ignore file that changed counts for the rest of this batch: rules read before it are stale
    let is_ignore_file = |abs: &Path| {
        abs.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(Ignorer::is_ignore_file)
    };
    if rescan {
        // events were lost: projects may have come or gone unseen
        for (_, ignorer) in &shared.ignorers {
            ignorer.forget_projects();
        }
    } else if pending
        .iter()
        .any(|p| is_ignore_file(&p.abs) || p.rename_from.as_deref().is_some_and(is_ignore_file))
    {
        for (_, ignorer) in &shared.ignorers {
            ignorer.clear();
        }
    }
    let mut done = shared.with_env(|env| reconcile(env, pending));
    // a nested project came, went or moved: the changes above were decided with what was known before (a project
    // that went still reports its files); from now on the new layout counts, and everything that keeps a picture of
    // the disk builds it again (tree, index, unread)
    let rescan = rescan || done.projects_moved;
    if done.projects_moved {
        for (_, ignorer) in &shared.ignorers {
            ignorer.forget_projects();
        }
    }
    shared.own.pair_moves(&mut done.changes);
    shared.own.mark(&mut done.changes, |path| {
        shared.sigs.get(&key_of(path)).map(|s| s.hash)
    });
    // the project's own repository moved (its signals are project paths below `.git/`)
    let git_signals = done.changes.iter().any(|c| c.path.starts_with(".git/"));
    let operation = git_operation(shared.vault.main().path(), markers, git_signals);
    let bulk = done.changes.len() > BULK_CHANGES || done.truncated || rescan || operation;
    let batch = FsBatch {
        changes: done.changes,
        bulk,
        rescan,
        moved: done.moved,
    };
    if !batch.is_empty() {
        shared.hub.publish(&batch);
    }
    done.deferred
}
