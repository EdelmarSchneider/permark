//! Review changes: what an agent changed in a document since the person last read it, and the means to keep
//! or take back each part.
//!
//! Every document has a *reviewed version*: the text the person approved. The review is the difference between
//! the file now and that version, block by block ([`diff`]). **Keep** makes a change part of the approved
//! version; **Revert** writes the old text of it back into the file. The design is in `docs/design/review-changes.md`.
//!
//! What lives where:
//!
//! * [`book`] knows, for every document, its approved version (a hash), the earlier version held for a review
//!   that is open, and which documents differ. Plain data, tested without files.
//! * [`store`] keeps the text of those versions in a folder next to the local state, one file for each, named for
//!   its hash. [`committed`] is where the text of a version comes from when no copy was kept yet.
//! * [`diff`] is the block diff and the splices that keep or undo one change; [`view`] is what the UI is given.
//! * [`ReviewService`] ties them to a project: it follows the unread markers ([`ReadObserver`]), keeps the files
//!   (`reviewed.json` and the copies) on a thread of its own, and answers the commands.
//!
//! **The approved version follows what was read** (level 1). Whenever the unread markers count a document as
//! read, a copy of that text is kept, and it becomes the document's approved version. A copy is taken when the
//! person reads (the text is at hand), when the person saves (see [`ReviewService::wrote`]), and otherwise
//! from the disk if the file still holds that version, or from the last commit if it has that text. A document
//! with a version and no copy of it can only be told to have changed: it can be reviewed from its next change.
//!
//! **A review that is open keeps the version before it** (see [`book`]): the person reads a changed document,
//! which approves what it holds now, and the changes stay to be kept or reverted until the review is closed
//! with [`ReviewService::release`] or the document is back to that version.
//!
//! **Reverting is a safe write.** A Revert writes only if the file still has the hash the changes were drawn
//! from; if somebody wrote in between, the call fails with [`ReviewError::Stale`] and nothing is overwritten.
//! The write is announced as the app's own, so the watcher does not report it as news. The approved version
//! does not move: the change just disappears from the difference.

pub mod book;
pub mod committed;
pub mod diff;
pub mod store;
pub mod view;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub use book::{Entry, Status};
pub use committed::{Committed, GitBlobs};
pub use view::{ReviewView, ViewStatus};

use book::{Book, Want};
use diff::Comparison;
use store::{CopyStore, MAX_COPY_BYTES};
use view::changes_view;

use crate::edit::{EditError, Editor, SaveOutcome};
use crate::render::markdown::is_markdown_path;
use crate::session::write_local_state;
use crate::unread::{decode, encode, hash_to_hex, DocState, ReadObserver, Timing};
use crate::util::{hash_bytes, read_with_retry, Coalescer};
use crate::vault::{key, Vault, VaultError};
use crate::watch::OwnWrites;

/// Called with the documents that differ from their approved version each time they change.
pub type Notify = Arc<dyn Fn(&[Entry]) + Send + Sync>;

/// A file larger than this is not read for a review at all.
const MAX_READ_BYTES: u64 = 32 * 1024 * 1024;

/// A copy that was just written is kept from the clean-up for this long, whether or not a version points to it yet.
const FRESH_FOR: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Edit(#[from] EditError),
    /// The document is not what the changes were drawn from: the file, or the approved version, is another by now.
    #[error("{0} changed since the review was drawn")]
    Stale(String),
    /// There is no approved text to compare the document with, or to put back.
    #[error("there is no approved version of {0} to compare with")]
    NoBaseline(String),
    /// The reviews are off in this window: a single file or the notes are open.
    #[error("changes are not reviewed here")]
    Off,
    #[error("cannot read {path}: {source}")]
    Io { path: String, source: io::Error },
}

impl ReviewError {
    /// A short stable identifier for the UI.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Vault(e) => e.kind(),
            Self::Edit(e) => e.kind(),
            Self::Stale(_) => "reviewStale",
            Self::NoBaseline(_) => "reviewNoBaseline",
            Self::Off => "reviewOff",
            Self::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied => "denied",
            Self::Io { .. } => "io",
        }
    }

    fn io(path: &str, source: io::Error) -> Self {
        if source.kind() == io::ErrorKind::NotFound {
            Self::Vault(VaultError::NotFound(path.to_owned()))
        } else {
            Self::Io {
                path: path.to_owned(),
                source,
            }
        }
    }
}

/// What the changes of a review were drawn from. Both are given back to the UI in [`ReviewView`], and the UI
/// hands them back with every Keep and Revert. Hashes are 16 hex digits; `None` is "no file" and "no approved
/// version" respectively.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Basis {
    /// The hash of the file.
    pub hash: Option<String>,
    /// The hash of the approved version.
    pub reviewed: Option<String>,
}

/// What a Revert did, which the caller has to tell the rest of the app about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reverted {
    /// The file was written: it held `old_hash` and holds `new_hash`.
    Wrote { old_hash: String, new_hash: String },
    /// The document is new, so reverting it is deleting it: sending it to the recycle bin is left to the caller.
    NewDocument,
    /// The document was deleted and is back: the file holds `hash`.
    Restored { hash: String },
}

/// Where the review keeps its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// `reviewed.json`.
    pub state: PathBuf,
    /// The folder of the copies.
    pub copies: PathBuf,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn utf8(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

struct Inner {
    book: Book,
    store: CopyStore,
    /// Where `reviewed.json` is.
    path: PathBuf,
    /// The hashes that have a copy.
    have: HashSet<u64>,
    /// Versions whose text could not be found: not looked for again until the document holds that version.
    missing: HashSet<(String, u64)>,
    /// Copies written lately, which the clean-up leaves alone while a version may not point to them yet.
    fresh: HashMap<u64, Instant>,
    /// The hashes that were in use at the last clean-up check.
    roots: HashSet<u64>,
    /// `reviewed.json` is behind the book.
    dirty: bool,
    /// Copies may be in the folder that nothing points to.
    cleanup: bool,
    /// The documents last announced.
    announced: Vec<Entry>,
}

struct Shared {
    inner: Mutex<Inner>,
    /// Taken for a whole announcement, so that two threads cannot announce out of order.
    turn: Mutex<()>,
    notify: Notify,
}

impl Shared {
    /// Notes that the book changed (`saved`: in a way that `reviewed.json` shows) and whether the clean-up has
    /// more to do. Returns whether the thread has anything to write.
    fn after_change(inner: &mut Inner, saved: bool) -> bool {
        inner.dirty |= saved;
        let roots = inner.book.roots();
        if roots != inner.roots {
            inner.roots = roots;
            inner.cleanup = true;
        }
        inner.dirty || inner.cleanup
    }

    /// Records that a copy of `hash` is about to be written, so that no clean-up deletes it before something
    /// points to it. Call it before the write.
    fn reserve(&self, hash: u64) {
        let mut g = lock(&self.inner);
        g.fresh.insert(hash, Instant::now());
        g.have.insert(hash);
    }
}

enum Msg {
    /// The unread markers changed.
    Changed {
        moves: Vec<(String, String)>,
        looked: bool,
        docs: Vec<DocState>,
    },
    /// The person is reading a document, which holds the version with this hash.
    Opening(String, u64),
    /// Something to write: `reviewed.json`, or a clean-up.
    Touch,
    /// The files live somewhere else now.
    Relocate(Paths),
    /// Write now, and say so.
    Flush(Sender<()>),
    Stop,
}

/// The thread's own state.
struct Worker {
    vault: Arc<Vault>,
    committed: Option<Arc<dyn Committed>>,
    shared: Arc<Shared>,
    service: Arc<Announcer>,
    stop: Arc<AtomicBool>,
}

/// What it takes to tell the listener which documents differ: the thread and the commands both do it.
struct Announcer {
    vault: Arc<Vault>,
    shared: Arc<Shared>,
}

impl Announcer {
    /// The documents that differ from their approved version. A deleted document whose file is there
    /// after all (the project stopped following it) is left out.
    fn entries(&self) -> Vec<Entry> {
        let entries = lock(&self.shared.inner).book.entries();
        entries
            .into_iter()
            .filter(|e| {
                e.status != Status::Deleted
                    || self
                        .vault
                        .resolve(&e.path)
                        .map_or(true, |r| std::fs::symlink_metadata(&r.abs).is_err())
            })
            .collect()
    }

    /// Tells the listener if the documents are different from the last time.
    fn announce(&self) {
        let _turn = lock(&self.shared.turn);
        let now = self.entries();
        let mut g = lock(&self.shared.inner);
        if now != g.announced {
            g.announced.clone_from(&now);
            drop(g);
            (self.shared.notify)(&now);
        }
    }
}

impl Worker {
    /// The text of a version, from the disk if the file still has it, else from the last commit.
    fn fetch(&self, want: &Want) -> Option<Vec<u8>> {
        if want.current == Some(want.hash) {
            if let Ok(resolved) = self.vault.resolve(&want.path) {
                let small = std::fs::metadata(&resolved.abs)
                    .is_ok_and(|m| m.is_file() && m.len() <= MAX_COPY_BYTES as u64);
                if small {
                    if let Ok(bytes) = read_with_retry(&resolved.abs) {
                        if hash_bytes(&bytes) == want.hash {
                            return Some(bytes);
                        }
                    }
                }
            }
        }
        let bytes = self.committed.as_ref()?.blob(&want.path)?;
        (hash_bytes(&bytes) == want.hash).then_some(bytes)
    }

    /// Keeps a copy of every version that has none, if its text can be found.
    fn copy_missing(&self) {
        let (wants, store) = {
            let g = lock(&self.shared.inner);
            let wants: Vec<Want> = g
                .book
                .wanted()
                .into_iter()
                .filter(|w| {
                    !g.have.contains(&w.hash)
                        && (w.current == Some(w.hash)
                            || !g.missing.contains(&(key(&w.path), w.hash)))
                })
                .collect();
            (wants, g.store.clone())
        };
        for want in wants {
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            let missing = (key(&want.path), want.hash);
            let Some(bytes) = self.fetch(&want) else {
                lock(&self.shared.inner).missing.insert(missing);
                continue;
            };
            self.shared.reserve(want.hash);
            let kept = store.put(&bytes);
            let mut g = lock(&self.shared.inner);
            match kept {
                Ok(Some(_)) => {
                    g.missing.remove(&missing);
                }
                // too large for a copy: not one to expect
                Ok(None) => {
                    g.have.remove(&want.hash);
                    g.missing.insert(missing);
                }
                // the folder cannot be written now: tried again with the next look
                Err(_) => {
                    g.have.remove(&want.hash);
                }
            }
        }
    }

    /// Takes in what the unread markers said. Returns whether there is something to write.
    fn changed(&self, moves: &[(String, String)], looked: bool, docs: &[DocState]) -> bool {
        let wrote = {
            let mut g = lock(&self.shared.inner);
            let saved = g.book.observe(moves, looked, docs);
            Shared::after_change(&mut g, saved)
        };
        self.copy_missing();
        let lost = self.let_go_of_lost_holds();
        self.service.announce();
        wrote || lost
    }

    /// A version that is held for a review but whose text could not be found holds nothing: the document is
    /// compared with the version it was read as, and the review of it can start from its next change.
    fn let_go_of_lost_holds(&self) -> bool {
        let mut g = lock(&self.shared.inner);
        let lost: Vec<String> = g
            .book
            .holds()
            .into_iter()
            .filter(|(_, hash)| !g.have.contains(hash))
            .map(|(path, _)| path)
            .collect();
        for path in &lost {
            g.book.release(path);
        }
        !lost.is_empty() && Shared::after_change(&mut g, false)
    }

    /// Writes `reviewed.json` if it is behind, then deletes the copies nothing points to. False when the write
    /// failed (it is still behind).
    fn write(&self) -> bool {
        let (bytes, path) = {
            let mut g = lock(&self.shared.inner);
            let bytes = if g.dirty {
                let Some(bytes) = encode(&g.book.saved()) else {
                    return true;
                };
                g.dirty = false;
                Some(bytes)
            } else {
                None
            };
            (bytes, g.path.clone())
        };
        if let Some(bytes) = bytes {
            if write_local_state(&path, &bytes).is_err() {
                lock(&self.shared.inner).dirty = true;
                return false;
            }
        }
        self.clean_up();
        true
    }

    /// Deletes the copies that no version points to. Under the lock, so that a copy being kept cannot be taken
    /// away before the version that points to it is set.
    fn clean_up(&self) {
        let mut g = lock(&self.shared.inner);
        if !g.cleanup {
            return;
        }
        g.cleanup = false;
        let now = Instant::now();
        g.fresh.retain(|_, at| now.duration_since(*at) < FRESH_FOR);
        let used: HashSet<u64> = g.roots.iter().chain(g.fresh.keys()).copied().collect();
        g.store.remove_unused(&used);
        g.have = g.store.hashes();
    }

    /// Moves the files to another place: the copies in use go along.
    fn relocate(&self, paths: Paths) {
        let mut g = lock(&self.shared.inner);
        let old = g.store.clone();
        if old.dir() == paths.copies {
            // the same place: only the state file may be somewhere else
            g.path = paths.state;
            g.dirty = true;
            return;
        }
        let new = CopyStore::new(paths.copies);
        for hash in g.roots.clone() {
            if let Some(bytes) = old.get(hash) {
                let _ = new.put(&bytes);
            }
        }
        // what was kept at the old place is not wanted twice
        old.remove_unused(&HashSet::new());
        g.have = new.hashes();
        g.store = new;
        g.path = paths.state;
        g.dirty = true;
    }
}

/// The thread: follows the unread markers, and writes `reviewed.json` and cleans the copies once things settle
/// (and again after `timing.retry` while writing fails).
fn run(rx: &Receiver<Msg>, worker: &Worker, timing: Timing) {
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
            Ok(Msg::Changed {
                moves,
                looked,
                docs,
            }) => {
                if worker.changed(&moves, looked, &docs) {
                    pending.touch(now);
                }
            }
            Ok(Msg::Opening(path, hash)) => lock(&worker.shared.inner).book.opening(&path, hash),
            Ok(Msg::Touch) => pending.touch(now),
            Ok(Msg::Relocate(paths)) => {
                worker.relocate(paths);
                pending.touch(now);
            }
            Ok(Msg::Flush(ack)) => {
                pending.reset();
                retry_at = (!worker.write()).then(|| now + timing.retry);
                let _ = ack.send(());
                continue;
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if pending.due(now) {
            pending.reset();
            retry_at = (!worker.write()).then(|| now + timing.retry);
        } else if retry_at.is_some_and(|at| now >= at) {
            retry_at = (!worker.write()).then(|| now + timing.retry);
        }
    }
    worker.write();
}

/// What a file is, for a review.
enum Disk {
    Gone,
    /// Too large to read.
    Huge,
    File {
        hash: u64,
        bytes: Vec<u8>,
    },
}

/// A document with everything a review of it needs, read at one moment.
struct Loaded {
    rel: String,
    disk: Disk,
    /// What the document is compared with.
    baseline: Option<u64>,
    /// Its text, if a copy is kept.
    baseline_bytes: Option<Vec<u8>>,
}

impl Loaded {
    fn hash_hex(&self) -> Option<String> {
        match &self.disk {
            Disk::File { hash, .. } => Some(hash_to_hex(*hash)),
            Disk::Gone | Disk::Huge => None,
        }
    }

    fn reviewed_hex(&self) -> Option<String> {
        self.baseline.map(hash_to_hex)
    }
}

/// The review of one open project.
pub struct ReviewService {
    vault: Arc<Vault>,
    own: Arc<OwnWrites>,
    shared: Arc<Shared>,
    announcer: Arc<Announcer>,
    /// `None` when the review is off: a single file is open, or the notes.
    tx: Option<Sender<Msg>>,
    stop: Arc<AtomicBool>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl ReviewService {
    fn build(
        vault: Arc<Vault>,
        own: Arc<OwnWrites>,
        paths: Paths,
        notify: Notify,
        saved: BTreeMap<String, u64>,
    ) -> (Self, Arc<Shared>) {
        let store = CopyStore::new(paths.copies);
        let book = Book::new(saved);
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                roots: book.roots(),
                have: store.hashes(),
                book,
                store,
                path: paths.state,
                missing: HashSet::new(),
                fresh: HashMap::new(),
                dirty: false,
                // the copies of an earlier run may include some nothing points to
                cleanup: true,
                announced: Vec::new(),
            }),
            turn: Mutex::new(()),
            notify,
        });
        let announcer = Arc::new(Announcer {
            vault: Arc::clone(&vault),
            shared: Arc::clone(&shared),
        });
        let service = Self {
            vault,
            own,
            shared: Arc::clone(&shared),
            announcer,
            tx: None,
            stop: Arc::new(AtomicBool::new(false)),
            handle: Mutex::new(None),
        };
        (service, shared)
    }

    /// A review that is off: no thread, no file, nothing ever differs. The notes folder has it, and so does a
    /// single file that is open on its own.
    pub fn off(vault: Arc<Vault>, own: Arc<OwnWrites>) -> Self {
        let paths = Paths {
            state: PathBuf::new(),
            copies: PathBuf::new(),
        };
        Self::build(
            vault,
            own,
            paths,
            Arc::new(|_: &[Entry]| {}),
            BTreeMap::default(),
        )
        .0
    }

    /// Loads `reviewed.json` (a missing or unreadable file means no document has an approved version yet; they
    /// get one from what the unread markers say was read) and starts the thread. `committed` is where the text of
    /// a version comes from when it has no copy and the file has changed. `notify` is called with the documents
    /// that differ from their approved version each time they change.
    pub fn open(
        vault: Arc<Vault>,
        own: Arc<OwnWrites>,
        paths: Paths,
        committed: Option<Arc<dyn Committed>>,
        timing: Timing,
        notify: Notify,
    ) -> Self {
        if vault.is_shallow() {
            return Self::off(vault, own);
        }
        let saved = std::fs::read(&paths.state)
            .ok()
            .and_then(|bytes| decode(&bytes))
            .unwrap_or_default();
        let (mut service, shared) = Self::build(Arc::clone(&vault), own, paths, notify, saved);
        let (tx, rx) = mpsc::channel();
        let worker = Worker {
            vault,
            committed,
            shared,
            service: Arc::clone(&service.announcer),
            stop: Arc::clone(&service.stop),
        };
        let handle = thread::Builder::new()
            .name("trailmark-review".to_owned())
            .spawn(move || run(&rx, &worker, timing))
            .ok();
        service.tx = Some(tx);
        service.handle = Mutex::new(handle);
        service
    }

    fn send(&self, msg: Msg) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(msg);
        }
    }

    fn enabled(&self) -> Result<(), ReviewError> {
        if self.tx.is_some() {
            Ok(())
        } else {
            Err(ReviewError::Off)
        }
    }

    // ---- copies --------------------------------------------------------------------------------

    /// Keeps a copy of `bytes` and returns their hash, or `None` when they are too large for a copy.
    fn keep_copy(&self, bytes: &[u8]) -> io::Result<Option<u64>> {
        if bytes.len() > MAX_COPY_BYTES {
            return Ok(None);
        }
        let hash = hash_bytes(bytes);
        self.shared.reserve(hash);
        let store = lock(&self.shared.inner).store.clone();
        let kept = store.put(bytes);
        if !matches!(kept, Ok(Some(_))) {
            lock(&self.shared.inner).have.remove(&hash);
        }
        kept
    }

    /// The person saved a document: `bytes` is what it holds now, which counts as read and approved. A copy of it is
    /// kept, so that the next change to it can be reviewed.
    pub fn wrote(&self, bytes: &[u8]) {
        if self.tx.is_some() {
            let _ = self.keep_copy(bytes);
        }
    }

    // ---- what is open --------------------------------------------------------------------------

    /// The documents that differ from their approved version, sorted. Nothing is listed until the project has
    /// been looked at.
    pub fn entries(&self) -> Vec<Entry> {
        self.announcer.entries()
    }

    /// The review of a document is closed: the version held for it is let go, and what was not reviewed counts
    /// as approved.
    pub fn release(&self, path: &str) -> Result<(), ReviewError> {
        let resolved = self.vault.resolve(path)?;
        if self.tx.is_some() {
            let mut g = lock(&self.shared.inner);
            g.book.release(&resolved.rel);
            let wrote = Shared::after_change(&mut g, false);
            drop(g);
            self.touch(wrote);
            self.announcer.announce();
        }
        Ok(())
    }

    fn touch(&self, wrote: bool) {
        if wrote {
            self.send(Msg::Touch);
        }
    }

    // ---- looking at a document -----------------------------------------------------------------

    fn load(&self, path: &str) -> Result<Loaded, ReviewError> {
        self.enabled()?;
        let resolved = self.vault.resolve(path)?;
        if !is_markdown_path(Path::new(&resolved.rel)) {
            return Err(EditError::NotMarkdown(resolved.rel).into());
        }
        let rel = resolved.rel;
        let disk = match std::fs::metadata(&resolved.abs) {
            Ok(meta) if meta.is_file() && meta.len() > MAX_READ_BYTES => Disk::Huge,
            Ok(meta) if meta.is_file() => match read_with_retry(&resolved.abs) {
                Ok(bytes) => Disk::File {
                    hash: hash_bytes(&bytes),
                    bytes,
                },
                Err(e) if e.kind() == io::ErrorKind::NotFound => Disk::Gone,
                Err(e) => return Err(ReviewError::io(&rel, e)),
            },
            Ok(_) => return Err(VaultError::NotFound(rel).into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Disk::Gone,
            Err(e) => return Err(ReviewError::io(&rel, e)),
        };
        let (baseline, store) = {
            let g = lock(&self.shared.inner);
            (g.book.baseline(&rel), g.store.clone())
        };
        let baseline_bytes = baseline.and_then(|hash| store.get(hash));
        Ok(Loaded {
            rel,
            disk,
            baseline,
            baseline_bytes,
        })
    }

    /// The changes of a document: its file now against its approved version.
    pub fn view(&self, path: &str) -> Result<ReviewView, ReviewError> {
        let loaded = self.load(path)?;
        Ok(self.view_of(&loaded))
    }

    fn view_of(&self, loaded: &Loaded) -> ReviewView {
        let path = loaded.rel.as_str();
        let reviewed = loaded.reviewed_hex();
        let bare = |status| ReviewView::bare(path, status, loaded.hash_hex(), reviewed.clone());
        let unchanged = || {
            // nothing is left to review: the version held for it has done its work
            let mut g = lock(&self.shared.inner);
            if g.book.held(path).is_some() {
                g.book.release(path);
                let wrote = Shared::after_change(&mut g, false);
                drop(g);
                self.touch(wrote);
            }
            bare(ViewStatus::Unchanged)
        };
        let with_changes = |status, cmp: &Comparison<'_>| {
            let mut view = bare(status);
            view.changes = changes_view(cmp);
            view
        };
        match &loaded.disk {
            Disk::Huge => bare(ViewStatus::TooLarge),
            Disk::Gone => match (
                loaded.baseline,
                loaded.baseline_bytes.as_deref().and_then(utf8),
            ) {
                // never there, never approved
                (None, _) => bare(ViewStatus::Unchanged),
                (Some(_), None) => bare(ViewStatus::Deleted),
                (Some(_), Some(old)) => {
                    let mut view = with_changes(ViewStatus::Deleted, &Comparison::new(old, ""));
                    view.restorable = true;
                    view
                }
            },
            Disk::File { hash, bytes } => {
                if bytes.len() > MAX_COPY_BYTES {
                    return bare(ViewStatus::TooLarge);
                }
                let Some(current) = utf8(bytes) else {
                    return bare(ViewStatus::NotText);
                };
                match (loaded.baseline, loaded.baseline_bytes.as_deref()) {
                    (None, _) => {
                        let cmp = Comparison::new("", current);
                        if cmp.changes().is_empty() {
                            unchanged()
                        } else {
                            with_changes(ViewStatus::New, &cmp)
                        }
                    }
                    (Some(approved), _) if approved == *hash => unchanged(),
                    (Some(_), None) => bare(ViewStatus::NoBaseline),
                    (Some(_), Some(approved)) => {
                        let Some(approved) = utf8(approved) else {
                            return bare(ViewStatus::NotText);
                        };
                        let cmp = Comparison::new(approved, current);
                        if cmp.changes().is_empty() {
                            unchanged()
                        } else {
                            with_changes(ViewStatus::Changes, &cmp)
                        }
                    }
                }
            }
        }
    }

    /// The document is what the review was drawn from.
    fn check(loaded: &Loaded, basis: &Basis) -> Result<(), ReviewError> {
        if loaded.hash_hex() == basis.hash && loaded.reviewed_hex() == basis.reviewed {
            Ok(())
        } else {
            Err(ReviewError::Stale(loaded.rel.clone()))
        }
    }

    /// The file's text, which must be there and be text a review can compare.
    fn current_text(loaded: &Loaded) -> Result<&str, ReviewError> {
        match &loaded.disk {
            Disk::File { bytes, .. } if bytes.len() > MAX_COPY_BYTES => {
                Err(EditError::TooLarge(loaded.rel.clone()).into())
            }
            Disk::File { bytes, .. } => {
                utf8(bytes).ok_or_else(|| EditError::Encoding(loaded.rel.clone()).into())
            }
            Disk::Huge => Err(EditError::TooLarge(loaded.rel.clone()).into()),
            Disk::Gone => Err(VaultError::NotFound(loaded.rel.clone()).into()),
        }
    }

    /// The approved text of a document, `""` for one that was never approved.
    fn baseline_text(loaded: &Loaded) -> Result<&str, ReviewError> {
        match (loaded.baseline, loaded.baseline_bytes.as_deref()) {
            (None, _) => Ok(""),
            (Some(_), Some(bytes)) => {
                utf8(bytes).ok_or_else(|| EditError::Encoding(loaded.rel.clone()).into())
            }
            (Some(_), None) => Err(ReviewError::NoBaseline(loaded.rel.clone())),
        }
    }

    // ---- keeping -------------------------------------------------------------------------------

    /// Sets the baseline of the document with `change` of the book, if it still is what `loaded` found.
    fn settle(
        &self,
        loaded: &Loaded,
        change: impl FnOnce(&mut Book) -> bool,
    ) -> Result<(), ReviewError> {
        let mut g = lock(&self.shared.inner);
        if g.book.baseline(&loaded.rel) != loaded.baseline {
            return Err(ReviewError::Stale(loaded.rel.clone()));
        }
        let saved = change(&mut g.book);
        let wrote = Shared::after_change(&mut g, saved);
        drop(g);
        self.touch(wrote);
        self.announcer.announce();
        Ok(())
    }

    /// Keeps change `index`: it becomes part of the approved version.
    pub fn keep(&self, path: &str, basis: &Basis, index: usize) -> Result<ReviewView, ReviewError> {
        let loaded = self.load(path)?;
        Self::check(&loaded, basis)?;
        if matches!(loaded.disk, Disk::Gone) {
            return self.keep_all_of(&loaded);
        }
        let current = Self::current_text(&loaded)?;
        let cmp = Comparison::new(Self::baseline_text(&loaded)?, current);
        if index >= cmp.changes().len() {
            return Err(ReviewError::Stale(loaded.rel));
        }
        let kept = cmp.kept(index);
        let hash = self
            .keep_copy(kept.as_bytes())
            .map_err(|e| ReviewError::io(&loaded.rel, e))?
            .ok_or_else(|| EditError::TooLarge(loaded.rel.clone()))?;
        self.settle(&loaded, |book| book.set_baseline(&loaded.rel, hash))?;
        self.view(&loaded.rel)
    }

    /// Keeps all changes: the document, as it is, is the approved version. For a deleted document, the deletion
    /// is accepted.
    pub fn keep_all(&self, path: &str, basis: &Basis) -> Result<ReviewView, ReviewError> {
        let loaded = self.load(path)?;
        Self::check(&loaded, basis)?;
        self.keep_all_of(&loaded)
    }

    fn keep_all_of(&self, loaded: &Loaded) -> Result<ReviewView, ReviewError> {
        match &loaded.disk {
            Disk::Gone => self.settle(loaded, |book| book.forget(&loaded.rel))?,
            Disk::Huge => return Err(EditError::TooLarge(loaded.rel.clone()).into()),
            Disk::File { hash, bytes } => {
                // a document too large for a copy is approved all the same: it is only ever "changed" after
                self.keep_copy(bytes)
                    .map_err(|e| ReviewError::io(&loaded.rel, e))?;
                self.settle(loaded, |book| book.accept(&loaded.rel, *hash))?;
            }
        }
        self.view(&loaded.rel)
    }

    // ---- reverting -----------------------------------------------------------------------------

    /// Reverts change `index`: the old text takes the place of the new in the file, if the file still is what
    /// the change was drawn from. The approved version stays as it was.
    pub fn revert(&self, path: &str, basis: &Basis, index: usize) -> Result<Reverted, ReviewError> {
        let loaded = self.load(path)?;
        Self::check(&loaded, basis)?;
        if matches!(loaded.disk, Disk::Gone) {
            return self.restore_of(&loaded);
        }
        if loaded.baseline.is_none() {
            return Ok(Reverted::NewDocument);
        }
        let current = Self::current_text(&loaded)?;
        let cmp = Comparison::new(Self::baseline_text(&loaded)?, current);
        if index >= cmp.changes().len() {
            return Err(ReviewError::Stale(loaded.rel));
        }
        let text = cmp.reverted(index);
        self.write_file(&loaded, &text)
    }

    /// Reverts all changes: the approved text is written over the file. For a deleted document, the document is
    /// put back; for a new one, the caller deletes it.
    pub fn revert_all(&self, path: &str, basis: &Basis) -> Result<Reverted, ReviewError> {
        let loaded = self.load(path)?;
        Self::check(&loaded, basis)?;
        if matches!(loaded.disk, Disk::Gone) {
            return self.restore_of(&loaded);
        }
        if loaded.baseline.is_none() {
            return Ok(Reverted::NewDocument);
        }
        Self::current_text(&loaded)?;
        let text = Self::baseline_text(&loaded)?.to_owned();
        self.write_file(&loaded, &text)
    }

    /// Writes `text` over the file of `loaded`, unless the file changed since it was read.
    fn write_file(&self, loaded: &Loaded, text: &str) -> Result<Reverted, ReviewError> {
        let Some(old_hash) = loaded.hash_hex() else {
            return Err(ReviewError::Stale(loaded.rel.clone()));
        };
        // the copy first: the version that is read next is this one
        self.keep_copy(text.as_bytes())
            .map_err(|e| ReviewError::io(&loaded.rel, e))?;
        match Editor::new(&self.vault, &self.own).save(&loaded.rel, text, &old_hash)? {
            SaveOutcome::Saved { hash, .. } => Ok(Reverted::Wrote {
                old_hash,
                new_hash: hash,
            }),
            SaveOutcome::Conflict { .. } => Err(ReviewError::Stale(loaded.rel.clone())),
        }
    }

    // ---- deleted documents ---------------------------------------------------------------------

    /// Puts a deleted document back, as it was approved.
    pub fn restore(&self, path: &str, basis: &Basis) -> Result<Reverted, ReviewError> {
        let loaded = self.load(path)?;
        Self::check(&loaded, basis)?;
        self.restore_of(&loaded)
    }

    fn restore_of(&self, loaded: &Loaded) -> Result<Reverted, ReviewError> {
        if !matches!(loaded.disk, Disk::Gone) {
            return Err(ReviewError::Stale(loaded.rel.clone()));
        }
        let text = Self::baseline_text(loaded)?;
        if loaded.baseline.is_none() {
            return Err(ReviewError::NoBaseline(loaded.rel.clone()));
        }
        // the folder it was in may be gone too
        let resolved = self.vault.resolve(&loaded.rel)?;
        if let Some(folder) = resolved.abs.parent() {
            std::fs::create_dir_all(folder).map_err(|e| ReviewError::io(&loaded.rel, e))?;
        }
        match Editor::new(&self.vault, &self.own).create(&loaded.rel, text) {
            Ok(created) => Ok(Reverted::Restored { hash: created.hash }),
            Err(EditError::Exists(_)) => Err(ReviewError::Stale(loaded.rel.clone())),
            Err(e) => Err(e.into()),
        }
    }

    // ---- life of the thread --------------------------------------------------------------------

    /// The files of the review live somewhere else now (the project got a `.trailmark/` folder): the copies in
    /// use go along, and `reviewed.json` is written there.
    pub fn relocate(&self, paths: Paths) {
        self.send(Msg::Relocate(paths));
    }

    /// Writes `reviewed.json` now if it is behind, and waits for the thread to have caught up with everything sent
    /// to it before, but not longer than `limit`. Returns whether the thread answered in time.
    pub fn flush_within(&self, limit: Duration) -> bool {
        let Some(tx) = &self.tx else {
            return true;
        };
        let (ack, done) = mpsc::channel();
        tx.send(Msg::Flush(ack)).is_ok() && done.recv_timeout(limit).is_ok()
    }

    /// [`ReviewService::flush_within`] for as long as it takes: how the tests wait for the thread.
    #[cfg(test)]
    pub fn flush(&self) {
        self.flush_within(Duration::MAX);
    }

    /// Makes the copies written so far old enough for the clean-up to take them away, and asks for one: how the tests
    /// get past the time copies are kept for.
    #[cfg(test)]
    fn age_copies(&self) {
        let mut g = lock(&self.shared.inner);
        g.fresh.clear();
        g.cleanup = true;
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

impl ReadObserver for ReviewService {
    fn reading(&self, path: &str, hash: u64, bytes: &[u8]) {
        if self.tx.is_some() {
            let _ = self.keep_copy(bytes);
            self.send(Msg::Opening(path.to_owned(), hash));
        }
    }

    fn changed(&self, moves: Vec<(String, String)>, looked: bool, docs: Vec<DocState>) {
        self.send(Msg::Changed {
            moves,
            looked,
            docs,
        });
    }
}

impl Drop for ReviewService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.send(Msg::Stop);
    }
}
