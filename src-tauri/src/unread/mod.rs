//! Unread markers: which Markdown documents changed since the person last read them.
//!
//! What a document held when it was last read is kept, per project, in `read.json` next to the other
//! local state (`.trailmark/local/` in an initialized project, the app's data folder otherwise). The file
//! is a JSON object from the document's path in the project to the xxh3 hash of its bytes at that time:
//!
//! ```json
//! { "docs/spec.md": "a1b2c3d4e5f60718" }
//! ```
//!
//! The hash is 16 lowercase hex digits (the same form the editor uses for a document's hash). A document
//! that is not listed was never read; one whose hash differs from the file's now has changed since. The
//! first time a project is opened it has no `read.json`, and every Markdown document that exists then counts
//! as read; the file is written once that baseline is taken. Linked worktrees are tracked like the rest, under
//! their `@wt/<name>/` paths.
//!
//! [`state`] holds the rules and is tested without files. [`UnreadService`] keeps them true: a thread looks at
//! the disk when the project opens, follows what the watcher reports, and writes `read.json` a moment after
//! the last change. Whoever asked is told through the `notify` callback whenever the set of unread documents
//! changes.

mod state;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub use state::DocState;
use state::{is_tracked, Tracker};

/// Called with the unread documents each time they change.
pub type Notify = Arc<dyn Fn(&[String]) + Send + Sync>;

/// Follows the read state: what the review of changes needs to keep the text of the version that was read.
pub trait ReadObserver: Send + Sync {
    /// The person is reading the document, which holds `bytes` (their hash is `hash`). It counts as read from
    /// the [`ReadObserver::changed`] that follows, which says so.
    fn reading(&self, path: &str, hash: u64, bytes: &[u8]);

    /// Something changed. `moves` are the documents that moved since the last call (`(from, to)`); `docs` is
    /// every document now, sorted; `looked` says whether the disk has been looked at (before that, no document
    /// has a `current` hash). Called under the service's lock, so the calls arrive in order and must return
    /// quickly.
    fn changed(&self, moves: Vec<(String, String)>, looked: bool, docs: Vec<DocState>);
}

use crate::render::markdown::is_markdown_path;
use crate::session::write_local_state;
use crate::util::{hash_bytes, modified_ms, read_with_retry, Coalescer};
use crate::vault::{key, NodeKind, Vault, VaultError, WalkItem};
use crate::watch::{FsChange, PathFilter, Verdict};

/// Documents larger than this are not tracked (hashing them would stall the thread).
const MAX_HASHED_BYTES: u64 = 32 * 1024 * 1024;

/// How long writes wait for more changes, and how soon a failed write is tried again.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Written this long after the last change...
    pub quiet: Duration,
    /// ...but never later than this after the first one.
    pub max_wait: Duration,
    /// The pause before a failed write is tried again.
    pub retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(300),
            max_wait: Duration::from_secs(2),
            retry: Duration::from_secs(5),
        }
    }
}

/// A hash as `read.json` spells it: 16 lowercase hex digits.
pub(crate) fn hash_to_hex(hash: u64) -> String {
    format!("{hash:016x}")
}

pub(crate) fn hash_from_hex(text: &str) -> Option<u64> {
    if text.len() == 16 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        u64::from_str_radix(text, 16).ok()
    } else {
        None
    }
}

/// The content of `read.json`; `None` when it is not a JSON object (the project then has no read state).
/// An entry that is not a path with a hash is dropped, which leaves that document unread.
pub(crate) fn decode(bytes: &[u8]) -> Option<BTreeMap<String, u64>> {
    let map: BTreeMap<String, serde_json::Value> = serde_json::from_slice(bytes).ok()?;
    Some(
        map.into_iter()
            .filter_map(|(path, value)| Some((path, hash_from_hex(value.as_str()?)?)))
            .collect(),
    )
}

pub(crate) fn encode(read: &BTreeMap<String, u64>) -> Option<Vec<u8>> {
    let map: BTreeMap<&str, String> = read
        .iter()
        .map(|(path, hash)| (path.as_str(), hash_to_hex(*hash)))
        .collect();
    serde_json::to_vec_pretty(&map).ok()
}

/// What a path is on disk right now.
enum Disk {
    /// A document that can be hashed, with its hash and its bytes.
    Doc { hash: u64, bytes: Vec<u8> },
    /// Nothing is there.
    Gone,
    /// A folder, a file too large to hash, or something that cannot be read now.
    Other,
}

fn look(abs: &Path) -> Disk {
    match std::fs::metadata(abs) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_HASHED_BYTES => {
            match read_with_retry(abs) {
                Ok(bytes) => Disk::Doc {
                    hash: hash_bytes(&bytes),
                    bytes,
                },
                Err(e) if e.kind() == io::ErrorKind::NotFound => Disk::Gone,
                Err(_) => Disk::Other,
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Disk::Gone,
        Ok(_) | Err(_) => Disk::Other,
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Inner {
    tracker: Tracker,
    /// Where `read.json` is.
    path: PathBuf,
    /// The unread documents last announced.
    announced: Vec<String>,
    /// `read.json` is behind the tracker.
    dirty: bool,
}

struct Shared {
    inner: Mutex<Inner>,
    notify: Notify,
    observer: Option<Arc<dyn ReadObserver>>,
}

impl Shared {
    /// Changes the tracker, tells the listener if the unread documents are different now, and returns
    /// whether `read.json` is behind. The listener and the observer are called under the lock, so what they
    /// hear is in order.
    fn update(&self, change: impl FnOnce(&mut Tracker) -> bool) -> bool {
        let mut g = lock(&self.inner);
        let wrote = change(&mut g.tracker);
        g.dirty |= wrote;
        let now = g.tracker.unread();
        if now != g.announced {
            (self.notify)(&now);
            g.announced = now;
        }
        if let Some(observer) = &self.observer {
            let moves = g.tracker.take_moves();
            observer.changed(moves, g.tracker.looked(), g.tracker.states());
        }
        wrote
    }

    /// Writes `read.json` if it is behind. False when the write failed (it is still behind).
    fn write(&self) -> bool {
        let (bytes, path) = {
            let mut g = lock(&self.inner);
            // Until the baseline is taken the tracker knows next to nothing: a file written now would
            // make every document unread the next time.
            if !g.dirty || g.tracker.baseline_pending() {
                return true;
            }
            let Some(bytes) = encode(&g.tracker.saved()) else {
                return true;
            };
            g.dirty = false;
            (bytes, g.path.clone())
        };
        if write_local_state(&path, &bytes).is_ok() {
            true
        } else {
            lock(&self.inner).dirty = true;
            false
        }
    }
}

enum Msg {
    /// Look at every document on disk (the project just opened, or events were lost).
    Scan,
    /// Watcher output.
    Changes(Vec<FsChange>),
    /// `read.json` is behind: write it after the quiet period.
    Touch,
    /// The state lives somewhere else now.
    Relocate(PathBuf),
    /// Write now, and say so.
    Flush(Sender<()>),
    Stop,
}

/// The thread's own state: it looks at the disk, so it needs the project.
struct Worker {
    vault: Arc<Vault>,
    filter: Arc<PathFilter>,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    /// Size, modification time and hash of each document a scan hashed, so a later scan reads only what changed.
    stamps: HashMap<String, (u64, u64, u64)>,
}

/// Every Markdown document of the project that the watcher would report.
fn documents(vault: &Vault, filter: &PathFilter) -> Vec<WalkItem> {
    vault
        .walk()
        .into_iter()
        .filter(|item| item.kind == NodeKind::File && is_markdown_path(Path::new(&item.rel)))
        .filter(|item| {
            vault
                .locate(&item.abs)
                .is_some_and(|l| filter.classify(l.root, &l.inner, false) == Verdict::Emit)
        })
        .collect()
}

/// The documents that are unread, worked out from `read.json` and the files as they are now, with no service
/// running: what an agent is told when the window is closed. Like the service, a project with no `read.json`
/// has every document read, and a document is unread when it was never read or has changed since.
pub fn unread_on_disk(vault: &Vault, filter: &PathFilter, read_path: &Path) -> Vec<String> {
    let saved = std::fs::read(read_path).ok().and_then(|b| decode(&b));
    let mut tracker = Tracker::new(saved);
    let found = documents(vault, filter)
        .into_iter()
        .filter_map(|item| {
            let meta = std::fs::metadata(&item.abs).ok()?;
            if !meta.is_file() || meta.len() > MAX_HASHED_BYTES {
                return None;
            }
            Some((item.rel, hash_bytes(&read_with_retry(&item.abs).ok()?)))
        })
        .collect();
    tracker.scanned(found);
    tracker.unread()
}

impl Worker {
    /// Every Markdown document of the project that the watcher would report.
    fn documents(&self) -> Vec<WalkItem> {
        documents(&self.vault, &self.filter)
    }

    /// Reads every document and hands the answer to the tracker. Returns whether `read.json` is behind.
    fn scan(&mut self) -> bool {
        match self.walk() {
            Some(found) => self.settle(found),
            None => false,
        }
    }

    /// Hashes every document of the project: the tracker is told first, so that what the app changes while the
    /// walk is under way (it takes a while on a big project) is not undone by its answer. `None` when the service
    /// was stopped midway.
    fn walk(&mut self) -> Option<Vec<(String, u64)>> {
        lock(&self.shared.inner).tracker.begin_scan();
        let mut found = Vec::new();
        for item in self.documents() {
            if self.stop.load(Ordering::Relaxed) {
                return None;
            }
            let Ok(meta) = std::fs::metadata(&item.abs) else {
                continue;
            };
            if !meta.is_file() || meta.len() > MAX_HASHED_BYTES {
                continue;
            }
            let (len, mtime) = (meta.len(), modified_ms(&meta));
            let k = key(&item.rel);
            let hash = match self.stamps.get(&k) {
                Some(&(l, m, hash)) if (l, m) == (len, mtime) => hash,
                _ => match read_with_retry(&item.abs) {
                    Ok(bytes) => hash_bytes(&bytes),
                    Err(_) => continue,
                },
            };
            self.stamps.insert(k, (len, mtime, hash));
            found.push((item.rel, hash));
        }
        let present: HashSet<String> = found.iter().map(|(path, _)| key(path)).collect();
        self.stamps.retain(|k, _| present.contains(k));
        Some(found)
    }

    /// Hands what the walk found to the tracker. Returns whether `read.json` is behind.
    fn settle(&self, found: Vec<(String, u64)>) -> bool {
        self.shared.update(|tracker| tracker.scanned(found))
    }

    /// Follows what the watcher reported. The disk decides what each change means: a path that is there is
    /// looked at, whatever the event was, and one that is not is gone. Returns whether `read.json` is behind.
    fn apply(&self, changes: &[FsChange]) -> bool {
        enum Step {
            Renamed(String, String),
            Removed(String),
            Changed(String, u64),
        }
        let mut steps = Vec::new();
        for change in changes {
            // A big batch (a checkout) is a lot of reading: closing does not wait for all of it.
            if self.stop.load(Ordering::Relaxed) {
                break;
            }
            if let Some(from) = &change.from {
                steps.push(Step::Renamed(from.clone(), change.path.clone()));
            }
            let Ok(resolved) = self.vault.resolve(&change.path) else {
                continue;
            };
            match look(&resolved.abs) {
                Disk::Doc { hash, .. } if is_tracked(&resolved.rel) => {
                    steps.push(Step::Changed(resolved.rel, hash));
                }
                Disk::Gone => steps.push(Step::Removed(resolved.rel)),
                Disk::Doc { .. } | Disk::Other => {}
            }
        }
        self.shared.update(|tracker| {
            let mut wrote = false;
            for step in steps {
                match step {
                    Step::Renamed(from, to) => wrote |= tracker.renamed(&from, &to),
                    Step::Removed(path) => wrote |= tracker.removed(&path),
                    Step::Changed(path, hash) => tracker.changed(&path, hash),
                }
            }
            wrote
        })
    }
}

/// The thread: first look at the disk, then follow the watcher, and write `read.json` once things settle
/// (and again after `timing.retry` while writing fails).
fn run(rx: &Receiver<Msg>, mut worker: Worker, timing: Timing) {
    let mut pending = Coalescer::new(timing.quiet, timing.max_wait);
    let mut retry_at: Option<Instant> = None;
    loop {
        let wake = [pending.deadline(), retry_at].into_iter().flatten().min();
        let msg = match wake {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let now = Instant::now();
        match msg {
            Ok(Msg::Scan) => {
                if worker.scan() {
                    pending.touch(now);
                }
            }
            Ok(Msg::Changes(changes)) => {
                if worker.apply(&changes) {
                    pending.touch(now);
                }
            }
            Ok(Msg::Touch) => pending.touch(now),
            Ok(Msg::Relocate(path)) => {
                {
                    let mut g = lock(&worker.shared.inner);
                    g.path = path;
                    g.dirty = true;
                }
                pending.touch(now);
            }
            Ok(Msg::Flush(ack)) => {
                pending.reset();
                retry_at = (!worker.shared.write()).then(|| now + timing.retry);
                let _ = ack.send(());
                continue;
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if pending.due(now) {
            pending.reset();
            retry_at = (!worker.shared.write()).then(|| now + timing.retry);
        } else if retry_at.is_some_and(|at| now >= at) {
            retry_at = (!worker.shared.write()).then(|| now + timing.retry);
        }
    }
    worker.shared.write();
}

/// The unread markers of one open project.
pub struct UnreadService {
    vault: Arc<Vault>,
    shared: Arc<Shared>,
    /// `None` when the markers are off: a single file is open, with no project around it to keep state for.
    tx: Option<Sender<Msg>>,
    stop: Arc<AtomicBool>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl UnreadService {
    /// Unread markers that are off: no thread, no file, nothing ever unread. The notes folder has them: the person
    /// writes those documents, and no other program is followed there.
    pub fn off(vault: Arc<Vault>) -> Self {
        Self {
            vault,
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    tracker: Tracker::new(None),
                    path: PathBuf::new(),
                    announced: Vec::new(),
                    dirty: false,
                }),
                notify: Arc::new(|_: &[String]| {}),
                observer: None,
            }),
            tx: None,
            stop: Arc::new(AtomicBool::new(false)),
            handle: Mutex::new(None),
        }
    }

    /// Loads `read_path` (a missing or unreadable file means the project has no read state yet) and starts
    /// the thread, which looks at every document first. `notify` is called with the unread documents each time
    /// they change. A vault that is a single file has no markers. `observer` follows the read state (see
    /// [`ReadObserver`]).
    pub fn open(
        vault: Arc<Vault>,
        filter: Arc<PathFilter>,
        read_path: PathBuf,
        timing: Timing,
        notify: Notify,
        observer: Option<Arc<dyn ReadObserver>>,
    ) -> Self {
        let saved = std::fs::read(&read_path).ok().and_then(|b| decode(&b));
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                tracker: Tracker::new(saved),
                path: read_path,
                announced: Vec::new(),
                dirty: false,
            }),
            notify,
            observer,
        });
        let stop = Arc::new(AtomicBool::new(false));
        if vault.is_shallow() {
            return Self {
                vault,
                shared,
                tx: None,
                stop,
                handle: Mutex::new(None),
            };
        }
        let (tx, rx) = mpsc::channel();
        let worker = Worker {
            vault: Arc::clone(&vault),
            filter,
            shared: Arc::clone(&shared),
            stop: Arc::clone(&stop),
            stamps: HashMap::new(),
        };
        let handle = thread::Builder::new()
            .name("trailmark-unread".to_owned())
            .spawn(move || run(&rx, worker, timing))
            .ok();
        let _ = tx.send(Msg::Scan);
        Self {
            vault,
            shared,
            tx: Some(tx),
            stop,
            handle: Mutex::new(handle),
        }
    }

    fn send(&self, msg: Msg) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(msg);
        }
    }

    /// Schedules the write of `read.json` if `wrote` says it is behind.
    fn touch(&self, wrote: bool) {
        if wrote {
            self.send(Msg::Touch);
        }
    }

    /// The documents that are unread now, sorted. Empty until the first look at the disk is done.
    pub fn unread(&self) -> Vec<String> {
        lock(&self.shared.inner).tracker.unread()
    }

    /// The person read the document: whatever it holds now counts as read. Anything but a Markdown document
    /// that exists is ignored.
    pub fn mark_read(&self, path: &str) -> Result<(), VaultError> {
        let resolved = self.vault.resolve(path)?;
        if self.tx.is_none() || !is_tracked(&resolved.rel) {
            return Ok(());
        }
        if let Disk::Doc { hash, bytes } = look(&resolved.abs) {
            if let Some(observer) = &self.shared.observer {
                observer.reading(&resolved.rel, hash, &bytes);
            }
            let wrote = self
                .shared
                .update(|tracker| tracker.mark_read(&resolved.rel, hash));
            self.touch(wrote);
        }
        Ok(())
    }

    /// A document the app just wrote (a save from the editor, a new file) is read: the person made it.
    /// `hash` is the one the write returned.
    pub fn mark_written(&self, path: &str, hash: &str) {
        let (Some(hash), Ok(resolved), Some(_)) = (
            hash_from_hex(hash),
            self.vault.resolve(path),
            self.tx.as_ref(),
        ) else {
            return;
        };
        let wrote = self
            .shared
            .update(|tracker| tracker.mark_read(&resolved.rel, hash));
        self.touch(wrote);
    }

    /// The document counts as unread again.
    pub fn mark_unread(&self, path: &str) -> Result<(), VaultError> {
        let resolved = self.vault.resolve(path)?;
        if self.tx.is_some() {
            let wrote = self
                .shared
                .update(|tracker| tracker.mark_unread(&resolved.rel));
            self.touch(wrote);
        }
        Ok(())
    }

    /// Everything counts as read.
    pub fn mark_all_read(&self) {
        if self.tx.is_some() {
            let wrote = self.shared.update(Tracker::mark_all_read);
            self.touch(wrote);
        }
    }

    /// Everything in a folder counts as read (the project root is every folder).
    pub fn mark_folder_read(&self, folder: &str) -> Result<(), VaultError> {
        let resolved = self.vault.resolve(folder)?;
        if self.tx.is_some() {
            let wrote = self.shared.update(|tracker| {
                if resolved.rel.is_empty() {
                    tracker.mark_all_read()
                } else {
                    tracker.mark_folder_read(&resolved.rel)
                }
            });
            self.touch(wrote);
        }
        Ok(())
    }

    /// Follows what the watcher reported.
    pub fn apply(&self, changes: &[FsChange]) {
        if !changes.is_empty() {
            self.send(Msg::Changes(changes.to_vec()));
        }
    }

    /// Looks at the disk again: events were lost, or this folder cannot report its changes.
    pub fn rescan(&self) {
        self.send(Msg::Scan);
    }

    /// The app moved a file or folder: what was read stays read.
    pub fn renamed(&self, from: &str, to: &str) {
        if self.tx.is_some() {
            let wrote = self.shared.update(|tracker| tracker.renamed(from, to));
            self.touch(wrote);
        }
    }

    /// The app rewrote the links in a document (or put them back): its hash was `old` and is `new`.
    /// A document that had been read stays read.
    pub fn rewritten(&self, path: &str, old: &str, new: &str) {
        let (Some(old), Some(new)) = (hash_from_hex(old), hash_from_hex(new)) else {
            return;
        };
        if self.tx.is_some() {
            let wrote = self
                .shared
                .update(|tracker| tracker.rewritten(path, old, new));
            self.touch(wrote);
        }
    }

    /// The project's state moved to another place (it got a `.trailmark/` folder): write it there.
    pub fn relocate(&self, read_path: PathBuf) {
        self.send(Msg::Relocate(read_path));
    }

    /// Writes `read.json` now if it is behind, and waits for the thread to have caught up with everything sent
    /// to it before, but not longer than `limit`: the thread may be in the middle of a look at a big project, and
    /// whoever is waiting (a window that is closing) must not hang until it is done. Returns whether the thread
    /// answered in time. When it did not, it still writes as soon as it gets to it, and always once it stops.
    pub fn flush_within(&self, limit: Duration) -> bool {
        let Some(tx) = &self.tx else {
            return true;
        };
        let (ack, done) = mpsc::channel();
        tx.send(Msg::Flush(ack)).is_ok() && done.recv_timeout(limit).is_ok()
    }

    /// [`UnreadService::flush_within`] for as long as it takes: how the tests wait for the thread.
    #[cfg(test)]
    pub fn flush(&self) {
        self.flush_within(Duration::MAX);
    }

    /// Writes what is pending and stops the thread. Safe to call more than once.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.send(Msg::Stop);
        if let Some(handle) = lock(&self.handle).take() {
            let _ = handle.join();
        }
    }
}

impl Drop for UnreadService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.send(Msg::Stop);
    }
}

#[cfg(test)]
mod tests;
