//! The unread service: a worker thread looks at the disk when the project opens, follows what the watch reports, and
//! keeps `read.json` and the picture of the last visit. The rules live in [`Tracker`]; this holds the threads and files.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use permark_core::document::DocScope;
use permark_core::fsx;
use permark_core::state::{Loaded, StateData, StateFile};
use permark_core::tree::Explorer;
use permark_core::watch::{ChangeKind, FsBatch};
use serde::{Deserialize, Serialize};

use crate::tracker::{is_tracked, Doc, Tracker};
use crate::types::{SinceVisit, UnreadList, VisitItem, VisitKind};

/// Documents larger than this are not tracked (hashing them would stall the thread).
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;
/// Items the since-last-visit list carries.
const MAX_VISIT_ITEMS: usize = 1000;

/// `read.json`: the hash (16 hex digits) of what was read, per document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadFile {
    pub docs: BTreeMap<String, String>,
}

impl StateData for ReadFile {
    const VERSION: u32 = 1;
}

/// `visit.json`: what every document held when the project was last closed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisitFile {
    pub at_ms: u64,
    pub docs: BTreeMap<String, String>,
}

impl StateData for VisitFile {
    const VERSION: u32 = 1;
}

fn hex_of(hash: u64) -> String {
    fsx::ContentHash(hash).hex()
}

/// A hash as the files spell it: exactly 16 hex digits.
#[must_use]
pub fn hash_from_hex(text: &str) -> Option<u64> {
    (text.len() == 16 && text.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u64::from_str_radix(text, 16).ok())
        .flatten()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Whoever follows the read state (the review of changes).
pub trait ReadObserver: Send + Sync {
    /// A document whose bytes are the version last read (found by the first look, changed back, or just read).
    fn at_rest(&self, path: &str, hash: u64, bytes: &[u8]);
    /// The person read the document: it went from `before` (never: `None`) to `now`.
    fn read(&self, path: &str, before: Option<u64>, now: u64);
    /// A document moved.
    fn moved(&self, from: &str, to: &str);
    /// These documents count as read as they stand (a folder, everything): any open review of them is over.
    fn settled(&self, paths: &[String]);
}

/// What the plugin hands the plugins that depend on it.
pub struct UnreadHub {
    tracker: Arc<Mutex<Tracker>>,
    observers: Mutex<Vec<(u64, Arc<dyn ReadObserver>)>>,
    next: AtomicU64,
    scope: DocScope,
    tx: Sender<Msg>,
}

impl std::fmt::Debug for UnreadHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnreadHub").finish_non_exhaustive()
    }
}

impl UnreadHub {
    fn new(tracker: Arc<Mutex<Tracker>>, scope: DocScope, tx: Sender<Msg>) -> Arc<Self> {
        Arc::new(Self {
            tracker,
            observers: Mutex::new(Vec::new()),
            next: AtomicU64::new(0),
            scope,
            tx,
        })
    }

    /// The person read `path` as it was when it held `hash` (observers hear it; the state is saved and announced).
    pub fn mark_read(&self, path: &str, hash: u64) {
        let (before, wrote) = lock(&self.tracker).mark_read(path, hash);
        if wrote {
            if self.has_observers() {
                // the bytes are at hand only while the disk still holds that version
                if let Some((disk_hash, bytes)) = read_document(&self.scope, path) {
                    if disk_hash == hash {
                        self.each(|o| o.at_rest(path, hash, &bytes));
                    }
                }
            }
            self.each(|o| o.read(path, before, hash));
        }
        let _ = self.tx.send(Msg::Touched { wrote });
    }

    /// Hears the read state until the lease is dropped.
    #[must_use]
    pub fn subscribe(self: &Arc<Self>, observer: Arc<dyn ReadObserver>) -> ObserverLease {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.observers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((id, observer));
        ObserverLease {
            hub: Arc::downgrade(self),
            id,
        }
    }

    /// The hash of what was read of a document.
    #[must_use]
    pub fn read_hash(&self, path: &str) -> Option<u64> {
        lock(&self.tracker).read_of(path)
    }

    /// The hash of what a document holds on disk now (as far as known).
    #[must_use]
    pub fn current_hash(&self, path: &str) -> Option<u64> {
        lock(&self.tracker).current_of(path)
    }

    /// Every hash that stands for a version somebody read.
    #[must_use]
    pub fn read_hashes(&self) -> HashSet<u64> {
        lock(&self.tracker).read_hashes()
    }

    /// The first look at the disk is done.
    #[must_use]
    pub fn ready(&self) -> bool {
        lock(&self.tracker).looked()
    }

    fn has_observers(&self) -> bool {
        !self
            .observers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }

    /// Calls every observer (outside the lock: an observer may call back).
    fn each(&self, f: impl Fn(&dyn ReadObserver)) {
        let observers: Vec<Arc<dyn ReadObserver>> = self
            .observers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(_, o)| Arc::clone(o))
            .collect();
        for observer in &observers {
            f(observer.as_ref());
        }
    }
}

/// Teardown symmetry: the observer is not told any more when this goes.
pub struct ObserverLease {
    hub: std::sync::Weak<UnreadHub>,
    id: u64,
}

impl Drop for ObserverLease {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            hub.observers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|(id, _)| *id != self.id);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How the worker waits.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Changes wait this long before they are announced.
    pub announce: Duration,
    /// ... and this long before `read.json` is written.
    pub save: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            announce: Duration::from_millis(150),
            save: Duration::from_millis(500),
        }
    }
}

enum Msg {
    Batch(FsBatch),
    /// The app changed the state (a mark): save and announce.
    Touched {
        wrote: bool,
    },
    /// These documents count as read as they stand: take copies, tell the observers.
    Settle(Vec<String>),
    Stop,
}

/// Announces the unread list (name, payload of `unread:changed`).
pub type Announce = Arc<dyn Fn(&UnreadList) + Send + Sync>;

pub struct ServiceArgs {
    pub scope: DocScope,
    pub explorer: Arc<Explorer>,
    /// Prefixes of the roots: `""`, then `@wt/<name>` for each linked worktree.
    pub roots: Vec<String>,
    pub read_file: Option<StateFile<ReadFile>>,
    pub visit_file: Option<StateFile<VisitFile>>,
    pub announce: Announce,
    pub timing: Timing,
}

pub struct UnreadService {
    hub: Arc<UnreadHub>,
    since: Arc<Mutex<Option<SinceVisit>>>,
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for UnreadService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnreadService").finish_non_exhaustive()
    }
}

impl UnreadService {
    #[must_use]
    pub fn start(args: ServiceArgs) -> Self {
        let saved = args.read_file.as_ref().and_then(|file| match file.load() {
            Loaded::Found(read) => Some(
                read.docs
                    .into_iter()
                    .filter_map(|(path, hash)| Some((path, hash_from_hex(&hash)?)))
                    .collect(),
            ),
            // no state yet, or a damaged one: a project with no read state
            Loaded::Missing | Loaded::Recovered { .. } => None,
        });
        let last_visit = args.visit_file.as_ref().and_then(|file| match file.load() {
            Loaded::Found(visit) => Some(visit),
            Loaded::Missing | Loaded::Recovered { .. } => None,
        });
        let tracker = Arc::new(Mutex::new(Tracker::new(saved)));
        let (tx, rx) = mpsc::channel();
        let hub = UnreadHub::new(Arc::clone(&tracker), args.scope.clone(), tx.clone());
        let since = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            args,
            tracker,
            hub: Arc::clone(&hub),
            since: Arc::clone(&since),
            stop: Arc::clone(&stop),
            last_visit,
            announced: None,
        };
        let thread = thread::Builder::new()
            .name("permark-unread".to_owned())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || worker.run(&rx))
            .ok();
        Self {
            hub,
            since,
            tx,
            stop,
            thread,
        }
    }

    #[must_use]
    pub fn hub(&self) -> &Arc<UnreadHub> {
        &self.hub
    }

    /// A watch batch (cheap: the work is the worker's).
    pub fn feed(&self, batch: &FsBatch) {
        let _ = self.tx.send(Msg::Batch(batch.clone()));
    }

    #[must_use]
    pub fn list(&self) -> UnreadList {
        list_of(&lock(&self.hub.tracker))
    }

    #[must_use]
    pub fn since_last_visit(&self) -> SinceVisit {
        lock(&self.since).clone().unwrap_or(SinceVisit {
            ready: false,
            first: false,
            at_ms: 0,
            items: Vec::new(),
            more: 0,
        })
    }

    /// The person read `path` as it was when it held `hash`.
    pub fn mark_read(&self, path: &str, hash: u64) {
        self.hub.mark_read(path, hash);
    }

    pub fn mark_unread(&self, path: &str) {
        let wrote = lock(&self.hub.tracker).mark_unread(path);
        let _ = self.tx.send(Msg::Touched { wrote });
    }

    /// A folder (`""`: everything) counts as read as it stands.
    pub fn mark_folder_read(&self, folder: &str) {
        let (paths, wrote) = lock(&self.hub.tracker).mark_folder_read(folder);
        if !paths.is_empty() {
            let _ = self.tx.send(Msg::Settle(paths));
        }
        let _ = self.tx.send(Msg::Touched { wrote });
    }
}

impl Drop for UnreadService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn list_of(tracker: &Tracker) -> UnreadList {
    UnreadList {
        ready: tracker.looked(),
        unread: tracker.unread(),
    }
}

/// The hash and bytes of a document on disk; `None`: gone, too large, unreadable.
fn read_document(scope: &DocScope, path: &str) -> Option<(u64, Vec<u8>)> {
    let (_, abs) = scope.resolve(path).ok()?;
    let bytes = fsx::read_bytes(&abs, MAX_BYTES).ok()?;
    Some((fsx::hash(&bytes).0, bytes))
}

struct Worker {
    args: ServiceArgs,
    tracker: Arc<Mutex<Tracker>>,
    hub: Arc<UnreadHub>,
    since: Arc<Mutex<Option<SinceVisit>>>,
    stop: Arc<AtomicBool>,
    last_visit: Option<VisitFile>,
    announced: Option<UnreadList>,
}

impl Worker {
    fn run(mut self, rx: &Receiver<Msg>) {
        self.scan();
        let mut save_at: Option<Instant> = None;
        let mut announce_at: Option<Instant> = None;
        // the first look just changed things
        announce_at.get_or_insert_with(Instant::now);
        save_at.get_or_insert_with(|| Instant::now() + self.args.timing.save);
        loop {
            let next = [save_at, announce_at].into_iter().flatten().min();
            let msg = match next {
                Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                    Ok(msg) => Some(msg),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(msg) => Some(msg),
                    Err(_) => break,
                },
            };
            if let Some(msg) = msg {
                let wrote = match msg {
                    Msg::Stop => break,
                    Msg::Batch(batch) => self.batch(&batch),
                    Msg::Touched { wrote } => wrote,
                    Msg::Settle(paths) => {
                        self.settle(&paths);
                        false
                    }
                };
                if self.stop.load(Ordering::SeqCst) {
                    break;
                }
                announce_at.get_or_insert_with(|| Instant::now() + self.args.timing.announce);
                if wrote {
                    save_at.get_or_insert_with(|| Instant::now() + self.args.timing.save);
                }
            }
            let now = Instant::now();
            if announce_at.is_some_and(|at| at <= now) {
                announce_at = None;
                self.announce();
            }
            if save_at.is_some_and(|at| at <= now) {
                save_at = None;
                self.save();
            }
        }
        self.save();
        self.write_visit();
    }

    fn announce(&mut self) {
        let list = list_of(&lock(&self.tracker));
        if self.announced.as_ref() != Some(&list) {
            (self.args.announce)(&list);
            self.announced = Some(list);
        }
    }

    fn save(&self) {
        let Some(file) = &self.args.read_file else {
            return;
        };
        let tracker = lock(&self.tracker);
        // nothing is saved before the baseline is taken
        if tracker.baseline_pending() {
            return;
        }
        let docs = tracker
            .saved()
            .into_iter()
            .map(|(path, hash)| (path, hex_of(hash)))
            .collect();
        drop(tracker);
        // a state that cannot be written does not stop anything; the next change tries again
        let _ = file.save(&ReadFile { docs });
    }

    /// The project closes: what every document holds now is the picture the next visit compares with.
    fn write_visit(&self) {
        let Some(file) = &self.args.visit_file else {
            return;
        };
        let tracker = lock(&self.tracker);
        if !tracker.looked() {
            return;
        }
        let docs = tracker
            .current()
            .into_iter()
            .map(|(path, hash)| (path, hex_of(hash)))
            .collect();
        drop(tracker);
        let _ = file.save(&VisitFile {
            at_ms: now_ms(),
            docs,
        });
    }

    /// A full look at the disk.
    fn scan(&mut self) {
        lock(&self.tracker).begin_scan();
        let mut files: Vec<(String, u64)> = Vec::new();
        for prefix in &self.args.roots {
            let (documents, _) = self.args.explorer.scan_documents(prefix);
            for path in documents {
                if self.stop.load(Ordering::SeqCst) {
                    lock(&self.tracker).abort_scan();
                    return;
                }
                if !is_tracked(&path) {
                    continue;
                }
                let Some((hash, bytes)) = read_document(&self.args.scope, &path) else {
                    continue;
                };
                let at_rest = {
                    let tracker = lock(&self.tracker);
                    tracker.baseline_pending() || tracker.read_of(&path) == Some(hash)
                };
                if at_rest && self.hub.has_observers() {
                    self.hub.each(|o| o.at_rest(&path, hash, &bytes));
                }
                files.push((path, hash));
            }
        }
        let first_look = !lock(&self.tracker).looked();
        lock(&self.tracker).scanned(files);
        if first_look {
            self.compute_since_visit();
        }
    }

    fn compute_since_visit(&self) {
        let current = lock(&self.tracker).current();
        let Some(visit) = &self.last_visit else {
            *lock(&self.since) = Some(SinceVisit {
                ready: true,
                first: true,
                at_ms: 0,
                items: Vec::new(),
                more: 0,
            });
            return;
        };
        let was: BTreeMap<String, u64> = visit
            .docs
            .iter()
            .filter_map(|(p, h)| Some((p.clone(), hash_from_hex(h)?)))
            .collect();
        let mut items = Vec::new();
        for (path, hash) in &current {
            match was.get(path) {
                None => items.push(VisitItem {
                    path: path.clone(),
                    kind: VisitKind::New,
                }),
                Some(before) if before != hash => items.push(VisitItem {
                    path: path.clone(),
                    kind: VisitKind::Changed,
                }),
                Some(_) => {}
            }
        }
        for path in was.keys() {
            if !current.contains_key(path) {
                items.push(VisitItem {
                    path: path.clone(),
                    kind: VisitKind::Removed,
                });
            }
        }
        items.sort_by(|a, b| a.path.cmp(&b.path));
        let more = u32::try_from(items.len().saturating_sub(MAX_VISIT_ITEMS)).unwrap_or(u32::MAX);
        items.truncate(MAX_VISIT_ITEMS);
        *lock(&self.since) = Some(SinceVisit {
            ready: true,
            first: false,
            at_ms: visit.at_ms,
            items,
            more,
        });
    }

    /// One watch batch. Returns whether `read.json` must be written.
    fn batch(&mut self, batch: &FsBatch) -> bool {
        // events were lost, or a folder appeared with more files than the watch lists: look at the disk again
        let cut_off = batch.bulk
            && batch
                .changes
                .iter()
                .any(|c| c.is_dir && matches!(c.kind, ChangeKind::Created));
        if batch.rescan || cut_off {
            self.scan();
            return true;
        }
        let mut wrote = false;
        // moves first, then what went, then what came: a move the watch only half paired (a folder renamed with
        // some files paired and some not) still finds the documents it moved
        for change in &batch.changes {
            let ChangeKind::Renamed { from } = &change.kind else {
                continue;
            };
            let path = change.path.as_str();
            if !(change.is_dir || is_tracked(from) || is_tracked(path)) {
                continue;
            }
            let (moves, w) = lock(&self.tracker).renamed(from, path);
            wrote |= w;
            for (a, b) in &moves {
                self.hub.each(|o| o.moved(a, b));
            }
        }
        let mut gone: Vec<Doc> = Vec::new();
        for change in &batch.changes {
            if matches!(change.kind, ChangeKind::Removed)
                && (change.is_dir || is_tracked(&change.path))
            {
                let (docs, w) = lock(&self.tracker).removed_docs(&change.path);
                wrote |= w;
                gone.extend(docs);
            }
        }
        for change in &batch.changes {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let path = change.path.as_str();
            // a folder's files are listed on their own
            if !matches!(change.kind, ChangeKind::Created | ChangeKind::Modified)
                || change.is_dir
                || !is_tracked(path)
            {
                continue;
            }
            let created = matches!(change.kind, ChangeKind::Created);
            wrote |= self.changed(path, change.own, created, &mut gone);
        }
        wrote
    }

    /// A document was created or changed. `gone`: documents that went in this batch (a creation with the content
    /// of one of them is that document, moved).
    fn changed(&self, path: &str, own: bool, created: bool, gone: &mut Vec<Doc>) -> bool {
        let Some((hash, bytes)) = read_document(&self.args.scope, path) else {
            // gone again (or unreadable): the watch will say so; until then it is not news
            return lock(&self.tracker).removed(path);
        };
        if created && !own {
            if let Some(at) = gone.iter().position(|d| d.current == Some(hash)) {
                let doc = gone.swap_remove(at);
                let wrote = lock(&self.tracker).adopt(path, hash, doc.read);
                self.hub.each(|o| o.moved(&doc.path, path));
                if doc.read == Some(hash) && self.hub.has_observers() {
                    self.hub.each(|o| o.at_rest(path, hash, &bytes));
                }
                return wrote;
            }
        }
        let (wrote, at_rest) = {
            let mut tracker = lock(&self.tracker);
            let wrote = tracker.changed(path, hash, own);
            (wrote, tracker.read_of(path) == Some(hash))
        };
        if at_rest && self.hub.has_observers() {
            self.hub.each(|o| o.at_rest(path, hash, &bytes));
        }
        wrote
    }

    fn settle(&self, paths: &[String]) {
        if self.hub.has_observers() {
            for path in paths {
                if let Some((hash, bytes)) = read_document(&self.args.scope, path) {
                    self.hub.each(|o| o.at_rest(path, hash, &bytes));
                }
            }
        }
        self.hub.each(|o| o.settled(paths));
    }
}
