//! The review service: copies of what the person read, the review that is open (`held`), the block diff of a document
//! against its base, keep and revert (a surgical line save through the editor), undo.
//!
//! The base of a document is the version the person last read (a copy kept when it was read), or the last commit.
//! Reading a changed document moves the read version to the current one; the version before is `held` for the review
//! until it is released (the person leaves the document, or says Done), so that the changes stay to be kept or
//! reverted. Every call that changes the file says the version it was drawn from: another version on disk = stale.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use permark_core::edit::lines::apply;
use permark_core::edit::{LineEdit, SaveOutcome};
use permark_core::error::{ErrorKind, IpcError};
use permark_core::format::{Block, Body, RenderCx};
use permark_core::fsx;
use permark_core::plugins::{PluginId, ProjectServices, RepoHandle};
use permark_core::state::{Loaded, StateData, StateFile, StateStore};
use permark_plugin_unread::{hash_from_hex, ReadObserver, UnreadHub};
use serde::{Deserialize, Serialize};

use crate::diff::{self, Change, Kind};
use crate::splice::{self, Span, Text};
use crate::store::{CopyStore, MAX_COPY_BYTES};
use crate::types::{Basis, ReviewChange, ReviewChangeKind, ReviewState, ReviewStatus, ReviewView};

/// Copies kept at most.
const MAX_COPIES: usize = 5000;
/// Undo steps kept per document.
const UNDO_DEPTH: usize = 20;
/// Copies waiting for the copier at most (the first look at a big project waits for it: memory stays bounded).
const QUEUE: usize = 64;

/// `held.json`: the versions kept for the reviews that are open (document -> hash).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldFile {
    pub docs: BTreeMap<String, String>,
}

impl StateData for HeldFile {
    const VERSION: u32 = 1;
}

/// `prefs.json`: what documents are compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefs {
    pub basis: Basis,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { basis: Basis::Read }
    }
}

impl StateData for Prefs {
    const VERSION: u32 = 1;
}

enum Undo {
    /// A keep: the version held before it (`None`: none).
    Keep { prev: Option<u64> },
    /// A revert: the edits that put the lines back, valid on the version `hash_after`.
    Revert {
        edits: Vec<LineEdit>,
        hash_after: String,
    },
}

struct Held {
    path: String,
    hash: u64,
}

pub struct Files {
    pub held: Option<StateFile<HeldFile>>,
    pub prefs: Option<StateFile<Prefs>>,
}

struct Shared {
    project: ProjectServices,
    store: CopyStore,
    hub: Arc<UnreadHub>,
    files: Files,
    held: Mutex<BTreeMap<String, Held>>,
    prefs: Mutex<Prefs>,
    undo: Mutex<HashMap<String, Vec<Undo>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn key_of(path: &str) -> String {
    permark_plugin_unread::key_of(path)
}

/// A version of a document: its hash (of the bytes) and its text with every line break as `\n`.
struct Version {
    hash: u64,
    text: String,
}

/// What a document is compared with.
struct Base {
    text: String,
    used: Basis,
}

/// Both versions rendered, and what differs.
struct Computed {
    current: Vec<Block>,
    old: Vec<Block>,
    changes: Vec<Change>,
}

fn invalid(subject: &str, detail: &str) -> IpcError {
    IpcError::new(ErrorKind::InvalidData)
        .subject(subject)
        .detail(detail)
}

fn is_markdown(path: &str) -> bool {
    permark_plugin_unread::is_tracked(path)
}

fn spans(blocks: &[Block]) -> Vec<Span> {
    blocks
        .iter()
        .map(|b| Span {
            start: b.line_start,
            end: b.line_end,
        })
        .collect()
}

fn ids(blocks: &[Block]) -> Vec<String> {
    blocks.iter().map(|b| b.id.clone()).collect()
}

impl Shared {
    // ---- reading ---------------------------------------------------------------------------------------------

    /// The document as it is on disk. `None`: gone.
    fn current(&self, path: &str) -> Result<Option<Version>, IpcError> {
        let (_, abs) = self.project.documents.scope().resolve(path)?;
        let bytes = match fsx::read_bytes(&abs, MAX_COPY_BYTES as u64) {
            Ok(bytes) => bytes,
            Err(fsx::FsxError::TooLarge { .. }) => {
                return Err(IpcError::new(ErrorKind::TooLarge).subject(path))
            }
            Err(fsx::FsxError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(None)
            }
            Err(e) => return Err(e.into()),
        };
        let hash = fsx::hash(&bytes).0;
        let decoded = fsx::decode(&bytes)
            .filter(|d| !d.lossy)
            .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).subject(path))?;
        Ok(Some(Version {
            hash,
            text: permark_core::edit::lines::normalized(&decoded.text),
        }))
    }

    fn held_hash(&self, path: &str) -> Option<u64> {
        lock(&self.held).get(&key_of(path)).map(|h| h.hash)
    }

    fn commit_text(&self, path: &str) -> Option<String> {
        let repo = self.project.services.get::<RepoHandle>()?;
        let bytes = repo.0.blob_at_head(path)?;
        if bytes.len() > MAX_COPY_BYTES {
            return None;
        }
        let decoded = fsx::decode(&bytes).filter(|d| !d.lossy)?;
        Some(permark_core::edit::lines::normalized(&decoded.text))
    }

    fn base(&self, path: &str, asked: Basis) -> Option<Base> {
        if asked == Basis::Read {
            let hash = self.held_hash(path).or_else(|| self.hub.read_hash(path));
            if let Some(hash) = hash {
                if let Some(text) = self.store.get(hash) {
                    return Some(Base {
                        text,
                        used: Basis::Read,
                    });
                }
            }
        }
        // no read version kept (or the last commit was asked for)
        self.commit_text(path).map(|text| Base {
            text,
            used: Basis::Commit,
        })
    }

    /// The blocks of a Markdown text, rendered like the page renders them.
    fn render(&self, path: &str, text: &str) -> Option<Vec<Block>> {
        let formats = self.project.formats();
        let format = formats.pick(path, text.as_bytes().get(..4096).unwrap_or(text.as_bytes()))?;
        let known = HashSet::new();
        let cx = RenderCx {
            path,
            env: self.project.documents.scope(),
            features: formats.features(),
            known: &known,
        };
        match format.render(text, &cx).body {
            Body::Markdown { blocks, .. } => Some(blocks),
            _ => None,
        }
    }

    fn compute(&self, path: &str, current: &str, base: &str) -> Option<Computed> {
        let current = self.render(path, current)?;
        let old = self.render(path, base)?;
        let changes = diff::changes(&ids(&old), &ids(&current));
        Some(Computed {
            current,
            old,
            changes,
        })
    }

    // ---- questions -------------------------------------------------------------------------------------------

    fn status(&self, path: &str) -> Result<ReviewStatus, IpcError> {
        let asked = lock(&self.prefs).basis;
        let status = |state| ReviewStatus {
            state,
            basis: asked,
        };
        if !is_markdown(path) {
            return Ok(status(ReviewState::Unsupported));
        }
        let current = match self.current(path) {
            Ok(Some(current)) => current,
            Ok(None) => return Ok(status(ReviewState::NoBasis)),
            Err(e) if e.kind == ErrorKind::TooLarge => return Ok(status(ReviewState::TooLarge)),
            Err(e) if e.kind == ErrorKind::Unsupported => {
                return Ok(status(ReviewState::Unsupported))
            }
            Err(e) => return Err(e),
        };
        let Some(base) = self.base(path, asked) else {
            return Ok(ReviewStatus {
                state: ReviewState::NoBasis,
                basis: asked,
            });
        };
        let state = if base.text == current.text {
            ReviewState::Same
        } else {
            match self.compute(path, &current.text, &base.text) {
                Some(computed) if computed.changes.is_empty() => ReviewState::Same,
                Some(computed) => ReviewState::Changes {
                    count: u32::try_from(computed.changes.len()).unwrap_or(u32::MAX),
                },
                None => ReviewState::Unsupported,
            }
        };
        Ok(ReviewStatus {
            state,
            basis: base.used,
        })
    }

    fn view(&self, path: &str, asked: Basis) -> Result<ReviewView, IpcError> {
        let current = self
            .current(path)?
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(path))?;
        let base = self
            .base(path, asked)
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("noBasis"))?;
        let computed = self
            .compute(path, &current.text, &base.text)
            .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).subject(path))?;
        Ok(view_of(path, &current, &base, asked, &computed))
    }

    // ---- changes ---------------------------------------------------------------------------------------------

    /// The state both sides of a call are drawn from; stale when the file is not the version the caller saw.
    fn draw(
        &self,
        path: &str,
        seen: &str,
        asked: Basis,
    ) -> Result<(Version, Base, Computed), IpcError> {
        let current = self
            .current(path)?
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(path))?;
        if fsx::ContentHash(current.hash).hex() != seen {
            return Err(IpcError::new(ErrorKind::Conflict)
                .subject(path)
                .detail("stale"));
        }
        let base = self
            .base(path, asked)
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("noBasis"))?;
        let computed = self
            .compute(path, &current.text, &base.text)
            .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).subject(path))?;
        Ok((current, base, computed))
    }

    fn keep(
        &self,
        path: &str,
        seen: &str,
        index: usize,
        all: bool,
        asked: Basis,
    ) -> Result<ReviewView, IpcError> {
        let (current, base, computed) = self.draw(path, seen, asked)?;
        if base.used != Basis::Read {
            return Err(invalid(path, "commit"));
        }
        let prev = self.held_hash(path);
        let new_hash = if all {
            // the version on disk is approved: it counts as read, and nothing is held any more
            let _ = self.store.put(current.hash, &current.text);
            self.hub.mark_read(path, current.hash);
            // marking read starts a review of what was read before; this one is over
            self.set_held(path, None);
            None
        } else {
            let change = computed
                .changes
                .get(index)
                .ok_or_else(|| invalid(path, "index"))?;
            let (cur_spans, old_spans) = (spans(&computed.current), spans(&computed.old));
            let edit = splice::edit_for(
                &Text::new(&base.text, &old_spans),
                &change.old,
                &Text::new(&current.text, &cur_spans),
                &change.new,
            )
            .map_err(|_| invalid(path, "splice"))?;
            let text = apply(&base.text, &[edit]).map_err(|_| invalid(path, "splice"))?;
            let hash = fsx::hash(text.as_bytes()).0;
            if !self.store.put(hash, &text) {
                return Err(IpcError::new(ErrorKind::TooLarge).subject(path));
            }
            self.set_held(path, Some(hash));
            Some(hash)
        };
        self.push_undo(path, Undo::Keep { prev });
        let _ = new_hash;
        self.finish(path, asked)
    }

    fn revert(
        &self,
        path: &str,
        seen: &str,
        index: usize,
        all: bool,
        asked: Basis,
    ) -> Result<ReviewView, IpcError> {
        let (current, base, computed) = self.draw(path, seen, asked)?;
        let (cur_spans, old_spans) = (spans(&computed.current), spans(&computed.old));
        let target = Text::new(&current.text, &cur_spans);
        let source = Text::new(&base.text, &old_spans);
        let mut edits = Vec::new();
        let picked: Vec<&Change> = if all {
            computed.changes.iter().collect()
        } else {
            vec![computed
                .changes
                .get(index)
                .ok_or_else(|| invalid(path, "index"))?]
        };
        for change in picked {
            edits.push(
                splice::edit_for(&target, &change.new, &source, &change.old)
                    .map_err(|_| invalid(path, "splice"))?,
            );
        }
        edits.sort_by_key(|e| e.at);
        if edits.is_empty() {
            return self.finish(path, asked);
        }
        let inverse = splice::invert(&edits, &target.lines);
        let outcome = self
            .project
            .with_editor(|editor| editor.save(path, seen, &edits))?;
        match outcome {
            SaveOutcome::Saved { hash, .. } => {
                self.push_undo(
                    path,
                    Undo::Revert {
                        edits: inverse,
                        hash_after: hash,
                    },
                );
            }
            SaveOutcome::Conflict { .. } => {
                return Err(IpcError::new(ErrorKind::Conflict)
                    .subject(path)
                    .detail("stale"))
            }
        }
        self.finish(path, asked)
    }

    fn undo(&self, path: &str, asked: Basis) -> Result<ReviewView, IpcError> {
        let step = lock(&self.undo)
            .get_mut(&key_of(path))
            .and_then(Vec::pop)
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("undo"))?;
        match step {
            Undo::Keep { prev } => self.set_held(path, prev),
            Undo::Revert { edits, hash_after } => {
                let outcome = self
                    .project
                    .with_editor(|editor| editor.save(path, &hash_after, &edits))?;
                if matches!(outcome, SaveOutcome::Conflict { .. }) {
                    return Err(IpcError::new(ErrorKind::Conflict)
                        .subject(path)
                        .detail("stale"));
                }
            }
        }
        self.view(path, asked)
    }

    /// The review after a change: over when nothing differs any more.
    fn finish(&self, path: &str, asked: Basis) -> Result<ReviewView, IpcError> {
        let view = self.view(path, asked)?;
        if view.count == 0 {
            self.set_held(path, None);
        }
        Ok(view)
    }

    fn restore(&self, path: &str) -> Result<(), IpcError> {
        if self.current(path)?.is_some() {
            return Err(IpcError::new(ErrorKind::Exists).subject(path));
        }
        let base = self
            .base(path, Basis::Read)
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("noBasis"))?;
        self.project
            .with_editor(|editor| editor.save_as(path, &base.text, true))?;
        Ok(())
    }

    // ---- what is held ----------------------------------------------------------------------------------------

    fn set_held(&self, path: &str, hash: Option<u64>) {
        {
            let mut held = lock(&self.held);
            match hash {
                Some(hash) => {
                    held.insert(
                        key_of(path),
                        Held {
                            path: path.to_owned(),
                            hash,
                        },
                    );
                }
                None => {
                    held.remove(&key_of(path));
                }
            }
        }
        self.save_held();
    }

    fn save_held(&self) {
        let Some(file) = &self.files.held else { return };
        let docs = lock(&self.held)
            .values()
            .map(|h| (h.path.clone(), fsx::ContentHash(h.hash).hex()))
            .collect();
        let _ = file.save(&HeldFile { docs });
    }

    fn push_undo(&self, path: &str, step: Undo) {
        let mut undo = lock(&self.undo);
        let stack = undo.entry(key_of(path)).or_default();
        stack.push(step);
        if stack.len() > UNDO_DEPTH {
            stack.remove(0);
        }
    }

    fn release(&self, path: &str) {
        self.set_held(path, None);
        lock(&self.undo).remove(&key_of(path));
    }
}

fn view_of(
    path: &str,
    current: &Version,
    base: &Base,
    asked: Basis,
    computed: &Computed,
) -> ReviewView {
    let changes: Vec<ReviewChange> = computed
        .changes
        .iter()
        .enumerate()
        .map(|(index, change)| ReviewChange {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            kind: match change.kind() {
                Kind::Added => ReviewChangeKind::Added,
                Kind::Removed => ReviewChangeKind::Removed,
                Kind::Edited => ReviewChangeKind::Edited,
            },
            start: u32::try_from(change.new.start).unwrap_or(u32::MAX),
            len: u32::try_from(change.new.len()).unwrap_or(u32::MAX),
            ghosts: computed.old[change.old.clone()].to_vec(),
        })
        .collect();
    ReviewView {
        path: path.to_owned(),
        hash: fsx::ContentHash(current.hash).hex(),
        basis: base.used,
        asked,
        count: u32::try_from(changes.len()).unwrap_or(u32::MAX),
        can_keep: base.used == Basis::Read,
        changes,
    }
}

// ---- the observer and the copier ----------------------------------------------------------------------------------

enum CopyMsg {
    Copy { hash: u64, bytes: Vec<u8> },
    Stop,
}

struct Observer {
    shared: Arc<Shared>,
    copies: SyncSender<CopyMsg>,
}

impl ReadObserver for Observer {
    fn at_rest(&self, _path: &str, hash: u64, bytes: &[u8]) {
        if bytes.len() <= MAX_COPY_BYTES && !self.shared.store.has(hash) {
            let _ = self.copies.send(CopyMsg::Copy {
                hash,
                bytes: bytes.to_vec(),
            });
        }
    }

    fn read(&self, path: &str, before: Option<u64>, now: u64) {
        // the person reads a changed document: the version before it stays the base of the review that is open
        if let Some(before) = before.filter(|b| *b != now) {
            if self.shared.held_hash(path).is_none() && self.shared.store.has(before) {
                self.shared.set_held(path, Some(before));
            }
        }
    }

    fn moved(&self, from: &str, to: &str) {
        let moved = lock(&self.shared.held).remove(&key_of(from));
        if let Some(held) = moved {
            lock(&self.shared.held).insert(
                key_of(to),
                Held {
                    path: to.to_owned(),
                    hash: held.hash,
                },
            );
            self.shared.save_held();
        }
    }

    fn settled(&self, paths: &[String]) {
        for path in paths {
            self.shared.set_held(path, None);
        }
    }
}

pub struct ReviewService {
    shared: Arc<Shared>,
    lease: Option<permark_plugin_unread::ObserverLease>,
    copies: SyncSender<CopyMsg>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ReviewService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReviewService").finish_non_exhaustive()
    }
}

impl ReviewService {
    #[must_use]
    pub fn start(
        project: ProjectServices,
        hub: &Arc<UnreadHub>,
        store: StateStore,
        id: PluginId,
        files: Files,
    ) -> Self {
        let held: BTreeMap<String, Held> = files
            .held
            .as_ref()
            .and_then(|file| match file.load() {
                Loaded::Found(held) => Some(held),
                Loaded::Missing | Loaded::Recovered { .. } => None,
            })
            .map(|held| {
                held.docs
                    .into_iter()
                    .filter_map(|(path, hash)| {
                        Some((
                            key_of(&path),
                            Held {
                                path,
                                hash: hash_from_hex(&hash)?,
                            },
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let prefs = files
            .prefs
            .as_ref()
            .map(|file| file.load().into_value())
            .unwrap_or_default();
        let shared = Arc::new(Shared {
            project,
            store: CopyStore::new(store, id),
            hub: Arc::clone(hub),
            files,
            held: Mutex::new(held),
            prefs: Mutex::new(prefs),
            undo: Mutex::new(HashMap::new()),
        });
        let (copies, rx) = mpsc::sync_channel::<CopyMsg>(QUEUE);
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let shared = Arc::clone(&shared);
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("permark-review".to_owned())
                .spawn(move || {
                    let mut cleaned = false;
                    let mut kept = shared.store.count();
                    loop {
                        if stop.load(Ordering::SeqCst) {
                            break;
                        }
                        match rx.recv_timeout(Duration::from_millis(250)) {
                            Ok(CopyMsg::Copy { hash, bytes }) => {
                                if kept >= MAX_COPIES || shared.store.has(hash) {
                                    continue;
                                }
                                if let Some(text) = fsx::decode(&bytes)
                                    .filter(|d| !d.lossy)
                                    .map(|d| permark_core::edit::lines::normalized(&d.text))
                                {
                                    if shared.store.put(hash, &text) {
                                        kept += 1;
                                    }
                                }
                            }
                            Ok(CopyMsg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                            Err(RecvTimeoutError::Timeout) => {}
                        }
                        // once the first look is done: drop the copies nobody needs any more
                        if !cleaned && !stop.load(Ordering::SeqCst) && shared.hub.ready() {
                            cleaned = true;
                            let mut keep = shared.hub.read_hashes();
                            keep.extend(lock(&shared.held).values().map(|h| h.hash));
                            shared.store.remove_unused(&keep);
                            kept = shared.store.count();
                        }
                    }
                })
                .ok()
        };
        let lease = hub.subscribe(Arc::new(Observer {
            shared: Arc::clone(&shared),
            copies: copies.clone(),
        }));
        Self {
            shared,
            lease: Some(lease),
            copies,
            stop,
            thread,
        }
    }

    fn asked(&self, basis: Option<Basis>) -> Basis {
        basis.unwrap_or_else(|| lock(&self.shared.prefs).basis)
    }

    /// # Errors
    /// Outside the project | unreadable.
    pub fn status(&self, path: &str) -> Result<ReviewStatus, IpcError> {
        self.shared.status(path)
    }

    /// # Errors
    /// Gone | too large | not text | no version to compare with.
    pub fn open(&self, path: &str, basis: Option<Basis>) -> Result<ReviewView, IpcError> {
        self.shared.view(path, self.asked(basis))
    }

    /// # Errors
    /// Stale (the file is not the version `seen`) | no read version | bad index.
    pub fn keep(
        &self,
        path: &str,
        seen: &str,
        index: usize,
        basis: Option<Basis>,
    ) -> Result<ReviewView, IpcError> {
        self.shared
            .keep(path, seen, index, false, self.asked(basis))
    }

    /// # Errors
    /// Stale | no read version.
    pub fn keep_all(
        &self,
        path: &str,
        seen: &str,
        basis: Option<Basis>,
    ) -> Result<ReviewView, IpcError> {
        self.shared.keep(path, seen, 0, true, self.asked(basis))
    }

    /// # Errors
    /// Stale | bad index | the file cannot be written.
    pub fn revert(
        &self,
        path: &str,
        seen: &str,
        index: usize,
        basis: Option<Basis>,
    ) -> Result<ReviewView, IpcError> {
        self.shared
            .revert(path, seen, index, false, self.asked(basis))
    }

    /// # Errors
    /// Stale | the file cannot be written.
    pub fn revert_all(
        &self,
        path: &str,
        seen: &str,
        basis: Option<Basis>,
    ) -> Result<ReviewView, IpcError> {
        self.shared.revert(path, seen, 0, true, self.asked(basis))
    }

    /// # Errors
    /// Nothing to undo | stale.
    pub fn undo(&self, path: &str, basis: Option<Basis>) -> Result<ReviewView, IpcError> {
        self.shared.undo(path, self.asked(basis))
    }

    /// The review of `path` is over (the person left it).
    pub fn release(&self, path: &str) {
        self.shared.release(path);
    }

    /// Writes a deleted document back as it was last read.
    ///
    /// # Errors
    /// The file exists | no version kept.
    pub fn restore(&self, path: &str) -> Result<(), IpcError> {
        self.shared.restore(path)
    }

    pub fn set_basis(&self, basis: Basis) {
        *lock(&self.shared.prefs) = Prefs { basis };
        if let Some(file) = &self.shared.files.prefs {
            let _ = file.save(&Prefs { basis });
        }
    }

    #[must_use]
    pub fn basis(&self) -> Basis {
        lock(&self.shared.prefs).basis
    }
}

impl Drop for ReviewService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // no more copies are offered; the copier stops within a moment
        self.lease.take();
        let _ = self.copies.try_send(CopyMsg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
