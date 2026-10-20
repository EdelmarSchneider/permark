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
use super::sigs::{sig_of, SigStore};
use super::types::{FsBatch, BULK_CHANGES};
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
}

/// What the project's tree holds at a project path: `Some(is_folder)` | `None` (nothing).
pub type PathLookup = Box<dyn Fn(&str) -> Option<bool> + Send + Sync>;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
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

/// Git is in the middle of an operation (checkout, merge, rebase, ...): its marker files exist.
fn git_operation(root: &Path) -> bool {
    let git = root.join(".git");
    [
        "index.lock",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-merge",
        "rebase-apply",
    ]
    .iter()
    .any(|marker| git.join(marker).exists())
}

impl Watcher {
    /// Starts watching every root of `vault`. Never fails: when the OS watchers cannot start, [`Watcher::status`] says
    /// so and the open documents are polled.
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
        });
        let mut natives = Vec::new();
        if native {
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

    /// Where the watch learns what was there before it began: a file or folder that goes away without ever having
    /// been news is then reported as what it was. Set once.
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
        let mut polled = lock(&self.shared.polled);
        match document {
            Some(path) => {
                // seed the signature: the first poll must not report a change that is none
                if let (Some(vp), Ok(meta)) = (
                    self.shared.vault.relativize(path),
                    std::fs::symlink_metadata(path),
                ) {
                    if let Some(sig) = sig_of(path, &meta) {
                        self.shared.sigs.set(key_of(vp.as_str()), vp.as_str(), sig);
                    }
                }
                polled.insert(window.to_owned(), path.to_path_buf());
            }
            None => {
                polled.remove(window);
            }
        }
    }

    /// Test hook: feeds a raw hint as if the OS had sent it.
    #[cfg(test)]
    pub(crate) fn inject(&self, hint: Pending) {
        let _ = self.tx.send(Msg::Hint(hint));
    }

    /// Test hook: an overflow.
    #[cfg(test)]
    pub(crate) fn inject_rescan(&self) {
        let _ = self.tx.send(Msg::Rescan);
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
        let handler = move |result: notify::Result<Event>| match result {
            Ok(event) => {
                if event.need_rescan() {
                    let _ = sender.send(Msg::Rescan);
                }
                for hint in hints_from(&event) {
                    let _ = sender.send(Msg::Hint(hint));
                }
            }
            Err(_) => {
                let _ = sender.send(Msg::Failed);
            }
        };
        let watcher = notify::recommended_watcher(handler)
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

fn run(shared: &Shared, rx: &Receiver<Msg>) {
    let mut pending = PendingSet::default();
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
                let now = Instant::now();
                first.get_or_insert(now);
                last = now;
                pending.add(hint);
                if pending.len() > MAX_PENDING {
                    pending.take();
                    rescan = true;
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
                let deferred = flush(shared, pending.take(), std::mem::take(&mut rescan));
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
                        deferred: false,
                    });
                }
            }
        }
    }
}

/// Reconciles `pending` with the disk and publishes the result. Returns paths to look at again.
fn flush(shared: &Shared, pending: Vec<Pending>, rescan: bool) -> Vec<Pending> {
    // an ignore file that changed counts for the rest of this batch: rules read before it are stale
    let is_ignore_file = |abs: &Path| {
        abs.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(Ignorer::is_ignore_file)
    };
    if rescan
        || pending
            .iter()
            .any(|p| is_ignore_file(&p.abs) || p.rename_from.as_deref().is_some_and(is_ignore_file))
    {
        for (_, ignorer) in &shared.ignorers {
            ignorer.clear();
        }
    }
    let known = |path: &str| {
        shared
            .own
            .moved_from(path)
            .or_else(|| shared.tree.get().and_then(|tree| tree(path)))
    };
    let env = Env {
        vault: &shared.vault,
        sigs: &shared.sigs,
        ignorers: &shared.ignorers,
        known: &known,
    };
    let mut done = reconcile(&env, pending);
    shared.own.pair_moves(&mut done.changes);
    shared.own.mark(&mut done.changes, |path| {
        shared.sigs.get(&key_of(path)).map(|s| s.hash)
    });
    let bulk = done.changes.len() > BULK_CHANGES
        || done.truncated
        || rescan
        || git_operation(shared.vault.main().path());
    let batch = FsBatch {
        changes: done.changes,
        bulk,
        rescan,
    };
    if !batch.is_empty() {
        shared.hub.publish(&batch);
    }
    done.deferred
}
