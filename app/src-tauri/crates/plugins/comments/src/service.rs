//! The comments of one project: `comments.json` (the person's own data, layout of 0.5) and the work of finding every
//! passage again when its document changes.
//!
//! Every change reads the file again when its length or time differ (a change made by `permark mcp` with the window
//! closed is not lost), changes it and writes it back atomically. A file that is not valid comments (or comes from a
//! newer layout) is set aside, not overwritten: the comments in it are the person's. A file that merely cannot be read
//! right now is left alone and the change fails.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use permark_core::document::DocScope;
use permark_core::error::{ErrorKind, IpcError};
use permark_core::format::units;
use permark_core::fsx;
use permark_core::paths::{key_of, VaultPath};
use permark_core::projects::{OwnerSlot, Owners, OwnersChange, OwnersGate, ProjectUid};
use permark_core::state::shared_write::lock_within;
use permark_core::state::{RawFile, StateStore};
use permark_core::watch::{ChangeKind, FsBatch};

use crate::anchor::{fold, Analysis, Placement, Probe, Resolution};
use crate::kept::{text_of, Keeper};
use crate::model::{
    Anchor, Book, By, Comment, CommentAside, CommentScope, CommentView, PassageState, Reply,
    ReplyView, MAX_COMMENTS, MAX_COMMENT_CHARS, MAX_QUOTE_CHARS, MAX_REPLIES, MAX_REPLY_CHARS,
    VERSION,
};

/// Comment ids made by this app (every core).
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Documents larger than this are not read for comments.
const MAX_DOC_BYTES: u64 = 5 * 1024 * 1024;
/// How long changes of documents are gathered before the passages are looked for again.
const QUIET: Duration = Duration::from_millis(150);
/// Most time a look at a changed file waits for the writers' lock before it compares the file with the kept copy
/// (busy longer: compared at the next look | write instead). Short: the look holds every comments call meanwhile.
const LOOK_WAIT: Duration = Duration::from_millis(200);

/// Tells the windows: `changed` (the comments) | `aside` (the comments set aside).
pub type Announce = Arc<dyn Fn(&str) + Send + Sync>;

/// What the window sends when the person writes a comment on a selection.
#[derive(Debug, Clone)]
pub struct NewComment {
    pub path: String,
    /// The selected text.
    pub quote: String,
    /// What the person wrote.
    pub text: String,
    /// Where the window saw the selection (the headings above it and its lines): the fallback when the document does
    /// not hold the quote.
    pub heading_path: Vec<String>,
    pub line: u32,
    pub end_line: u32,
}

/// What an edit changes: the comment's own text, or the text of its reply at this index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditTarget {
    Comment,
    Reply(usize),
}

/// One owner's `comments.json` (the opened folder's, or an included project's: `projects.md` §9.2), paths relative
/// to that owner.
struct Slot {
    file: RawFile,
    /// This install's kept copy of the file and the comments an outside write removed; `None`: no install key.
    keeper: Option<Keeper>,
    book: Book,
    /// The comments as last read or written: what this core changed since is laid on top of the file as it is when
    /// written (another tab | app | `permark mcp` may have written it meanwhile; `projects.md` §14 D51).
    base: Vec<Comment>,
    /// Length and time of the file as last read or written.
    stamp: Option<(u64, u128)>,
    /// The file exists and holds nothing this version can use: it is put aside before the first write.
    damaged: bool,
    /// The file could not be read just now (it may be fine a moment later).
    unreadable: Option<String>,
    /// The file was read since the slot was made (the first look compares a missing file with the kept copy too); a
    /// look that found the writers' lock busy leaves it unset, so the next look reads and compares again.
    looked: bool,
}

/// The comments of a file's bytes (`None`: not comments, or a newer layout).
pub(crate) fn book_of(bytes: &[u8]) -> Option<Book> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    serde_json::from_slice::<Book>(bytes)
        .ok()
        .filter(|book| book.version <= VERSION)
}

/// Three-way merge by comment id: what this core added, changed or removed since `base` goes on top of `fresh` (the
/// file now); every other comment is the file's. A comment removed elsewhere stays removed, even when this core changed
/// it meanwhile.
pub(crate) fn merge_book(base: &[Comment], mine: &[Comment], fresh: Book) -> Book {
    let base_by: HashMap<&str, &Comment> = base.iter().map(|c| (c.id.as_str(), c)).collect();
    let mine_by: HashMap<&str, &Comment> = mine.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut comments = Vec::with_capacity(fresh.comments.len() + mine.len());
    for comment in fresh.comments {
        seen.insert(comment.id.clone());
        match (
            base_by.get(comment.id.as_str()),
            mine_by.get(comment.id.as_str()),
        ) {
            // removed here
            (Some(_), None) => {}
            // changed here
            (Some(was), Some(now)) if was != now => comments.push((*now).clone()),
            _ => comments.push(comment),
        }
    }
    for comment in mine {
        if !base_by.contains_key(comment.id.as_str()) && !seen.contains(&comment.id) {
            comments.push(comment.clone());
        }
    }
    Book {
        version: fresh.version,
        comments,
        extra: fresh.extra,
    }
}

impl Slot {
    fn new(file: RawFile, keeper: Option<Keeper>) -> Self {
        Self {
            // written under the plugin's lock: a hung other writer never holds the lists up long (N10)
            file: file.brief_lock_wait(),
            keeper,
            book: Book::default(),
            base: Vec::new(),
            stamp: None,
            damaged: false,
            unreadable: None,
            looked: false,
        }
    }

    /// The comments file of an owner's state, with this install's kept copy and asides.
    fn of(store: &StateStore, announce: &Announce) -> Self {
        let announce = Arc::clone(announce);
        Self::new(
            store.raw_file(crate::FILE).capped(crate::MAX_FILE_BYTES),
            Keeper::of(store, Arc::new(move || announce(crate::ASIDE_EVENT))),
        )
    }

    /// Reads the file again when it is not what was last read. What an outside write removed is set aside before
    /// the comments are replaced.
    fn refresh(&mut self) {
        let stamp = self.file.stamp();
        if self.looked && self.stamp == stamp && self.unreadable.is_none() {
            return;
        }
        // compared under the writers' lock: a core of this install writing meanwhile (kept copy first, then the file)
        // never shows its own change as a removal
        let lock = self
            .keeper
            .as_ref()
            .map(|_| lock_within(self.file.path(), LOOK_WAIT));
        let read = self.file.read();
        let busy = matches!(lock, Some(Err(_)));
        if let (Some(keeper), Some(Ok(_)), Ok(bytes)) = (&self.keeper, &lock, &read) {
            if !self.looked {
                // expired asides go when the comments start
                keeper.asides(now_ms());
            }
            keeper.follow(bytes.as_deref(), now_ms());
        }
        drop(lock);
        self.looked = !busy;
        match read {
            Ok(Some(bytes)) => {
                if let Some(book) = book_of(&bytes) {
                    self.book = book;
                    self.damaged = false;
                } else {
                    self.book = Book::default();
                    self.damaged = true;
                }
                self.unreadable = None;
            }
            Ok(None) => {
                self.book = Book::default();
                self.damaged = false;
                self.unreadable = None;
            }
            Err(error) => {
                self.unreadable = Some(error.to_string());
                return;
            }
        }
        self.base.clone_from(&self.book.comments);
        self.stamp = stamp;
    }

    /// Writes the book: this core's changes since it last read the file go on top of the file as it is now (the
    /// unreadable file is put aside first).
    fn save(&mut self) -> Result<(), IpcError> {
        if self.damaged {
            let aside = self
                .file
                .path()
                .with_file_name(format!("comments.unreadable-{}.json", now_ms()));
            match std::fs::rename(self.file.path(), aside) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(IpcError::io("comments.json", &e)),
            }
            self.damaged = false;
        }
        let Self {
            file,
            keeper,
            book,
            base,
            ..
        } = self;
        let keeper = keeper.as_ref();
        let mut merged: Option<Book> = None;
        let mut foreign = false;
        let mut encode_error = None;
        // the kept copy before this write, then each text of the file this write read: what an outside write removed
        // in between is set aside before the kept copy takes this write's text
        let mut seen: Option<Option<String>> = None;
        file.update(|now| {
            if let Some(keeper) = keeper {
                let text = text_of(now);
                let reference = seen.get_or_insert_with(|| keeper.kept());
                if let Some(was) = reference.as_deref().filter(|was| *was != text) {
                    let _ = keeper.observe(was, now, now_ms());
                }
                *reference = Some(text.into_owned());
            }
            let fresh = match now {
                None => Book::default(),
                Some(bytes) => {
                    let Some(fresh) = book_of(bytes) else {
                        foreign = true;
                        if let Some(keeper) = keeper {
                            keeper.keep(&text_of(now));
                        }
                        return None;
                    };
                    fresh
                }
            };
            let next = merge_book(base, &book.comments, fresh);
            match serde_json::to_vec_pretty(&next) {
                Ok(bytes) => {
                    // kept before the file is written: a core comparing them in between sees this install's own
                    // write; a write that fails leaves an aside of what it held, never a loss
                    if let Some(keeper) = keeper {
                        keeper.keep(&String::from_utf8_lossy(&bytes));
                    }
                    merged = Some(next);
                    Some(bytes)
                }
                Err(e) => {
                    encode_error = Some(e.to_string());
                    None
                }
            }
        })?;
        if let Some(why) = encode_error {
            return Err(IpcError::new(ErrorKind::Internal).detail(why));
        }
        if foreign {
            // another program wrote something this version cannot read: never written over
            self.stamp = None;
            return Err(IpcError::new(ErrorKind::Unavailable).subject("comments.json"));
        }
        if let Some(next) = merged {
            self.base.clone_from(&next.comments);
            self.book = next;
        }
        self.stamp = self.file.stamp();
        Ok(())
    }
}

/// The comments of every owner (one file each); `owners` none: one file, no projects.
struct State {
    owners: Option<Owners>,
    slots: Vec<Slot>,
    /// The owners are changing: changes wait (refused for that moment).
    paused: bool,
}

impl State {
    /// Reads every owner's file again when it is not what was last read.
    fn refresh(&mut self) {
        for slot in &mut self.slots {
            slot.refresh();
        }
    }

    fn unreadable(&self) -> Option<&String> {
        self.slots.iter().find_map(|s| s.unreadable.as_ref())
    }

    /// What the aside ids of slot `i` start with: nothing for the opened folder, `<uid>:` for an included project.
    /// `None`: a project without an id (its asides are not offered).
    fn aside_prefix(&self, i: usize) -> Option<String> {
        if i == 0 {
            return Some(String::new());
        }
        let uid = self.owners.as_ref()?.slots().get(i)?.uid.as_ref()?;
        Some(format!("{uid}:"))
    }

    /// The slot and number an aside id names: `<digits>` (the opened folder's) | `<uid>:<digits>` (an included
    /// project's); never a path. `Ok(None)`: no such project (any more).
    fn aside_of(&self, id: &str) -> Result<Option<(usize, u64)>, IpcError> {
        let invalid = || IpcError::new(ErrorKind::InvalidData).subject("id");
        let (uid, digits) = match id.split_once(':') {
            Some((uid, digits)) => (Some(uid), digits),
            None => (None, id),
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        let n = digits.parse::<u64>().map_err(|_| invalid())?;
        let Some(uid) = uid else {
            return Ok(Some((0, n)));
        };
        let uid = ProjectUid::parse(uid).ok_or_else(invalid)?;
        Ok(self
            .owners
            .as_ref()
            .and_then(|o| o.slots().iter().position(|s| s.uid.as_ref() == Some(&uid)))
            .filter(|i| *i > 0)
            .map(|i| (i, n)))
    }

    /// Whether a comment kept in slot `i` (owner-relative path) belongs to a deeper owner (unused while it is
    /// included).
    fn dormant(&self, i: usize, path: &str) -> bool {
        self.owners
            .as_ref()
            .is_some_and(|o| o.owner_of(&o.join(i, path)) != i)
    }

    /// The comments of every owner in one book, paths root-relative. An id taken by an earlier owner gets the owner's
    /// uid in front (`<uid>:<id>`; `projects.md` §9.2).
    fn merged(&self) -> Book {
        let Some(owners) = &self.owners else {
            return self
                .slots
                .first()
                .map(|s| s.book.clone())
                .unwrap_or_default();
        };
        let mut out = Book::default();
        let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (i, slot) in self.slots.iter().enumerate() {
            for comment in &slot.book.comments {
                if self.dormant(i, &comment.path) {
                    continue;
                }
                let mut comment = comment.clone();
                comment.path = owners.join(i, &comment.path);
                if !ids.insert(comment.id.clone()) {
                    if let Some(uid) = &owners.slots()[i].uid {
                        comment.id = format!("{uid}:{}", comment.id);
                        ids.insert(comment.id.clone());
                    }
                }
                out.comments.push(comment);
            }
        }
        out
    }

    /// Lays a changed merged book back into the owners' books; returns the slots whose book changed.
    fn split(&mut self, merged: Book) -> Vec<usize> {
        let Some(owners) = self.owners.clone() else {
            let changed = self.slots[0].book.comments != merged.comments;
            self.slots[0].book.comments = merged.comments;
            return if changed { vec![0] } else { Vec::new() };
        };
        let mut parts: Vec<Vec<Comment>> = (0..self.slots.len()).map(|_| Vec::new()).collect();
        for mut comment in merged.comments {
            let i = owners.owner_of(&comment.path);
            comment.path = owners.rel(i, &comment.path);
            // a prefixed id goes back to its file's own spelling
            for slot in owners.slots().iter().skip(1) {
                if let Some(uid) = &slot.uid {
                    let prefix = format!("{uid}:");
                    if comment.id.starts_with(&prefix) {
                        comment.id.drain(..prefix.len());
                        break;
                    }
                }
            }
            parts[i].push(comment);
        }
        let mut changed = Vec::new();
        for (i, part) in parts.into_iter().enumerate() {
            let mut fresh: Vec<Option<Comment>> = part.into_iter().map(Some).collect();
            let mut list = Vec::with_capacity(self.slots[i].book.comments.len());
            for old in &self.slots[i].book.comments {
                if self.dormant(i, &old.path) {
                    list.push(old.clone());
                } else if let Some(new) = fresh
                    .iter_mut()
                    .find(|c| c.as_ref().is_some_and(|c| c.id == old.id))
                    .and_then(Option::take)
                {
                    list.push(new);
                }
            }
            list.extend(fresh.into_iter().flatten());
            if list != self.slots[i].book.comments {
                self.slots[i].book.comments = list;
                changed.push(i);
            }
        }
        changed
    }
}

/// A document as far as a comment is concerned.
#[derive(Clone)]
enum Doc {
    Text(Arc<Analysis>),
    /// There is no such file (any more), or it cannot be read as text.
    Missing,
}

struct Cached {
    len: u64,
    mtime: u128,
    doc: Doc,
}

struct Shared {
    scope: DocScope,
    announce: Announce,
    state: Mutex<State>,
    docs: Mutex<HashMap<String, Cached>>,
    rev: AtomicU64,
    /// Closed while the plugin started before the owners were known: changes wait for them.
    gate: OwnersGate,
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn cut(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn bump(&self) {
        self.rev.fetch_add(1, Ordering::Relaxed);
        (self.announce)(crate::CHANGED_EVENT);
    }

    /// Changes the comments (one root-relative book over every owner's file). `change` says whether it changed
    /// anything; only then are the files of the owners whose comments changed written.
    fn change<T>(
        &self,
        change: impl FnOnce(&mut Book) -> Result<(T, bool), IpcError>,
    ) -> Result<T, IpcError> {
        self.gate.wait();
        let mut state = self.lock();
        state.refresh();
        if let Some(why) = state.unreadable() {
            return Err(IpcError::new(ErrorKind::Unavailable)
                .subject("comments.json")
                .detail(why.clone()));
        }
        if state.paused {
            return Err(IpcError::new(ErrorKind::Unavailable).subject("comments.json"));
        }
        let mut book = state.merged();
        let (value, changed) = change(&mut book)?;
        if changed {
            let mut failed = None;
            for i in state.split(book) {
                if let Err(error) = state.slots[i].save() {
                    // N9: a change that was not written is not shown either (the person was told; adding again
                    // never makes a twin): that file is read again as it is on disk
                    state.slots[i].stamp = None;
                    failed = Some(error);
                }
            }
            state.refresh();
            drop(state);
            self.bump();
            if let Some(error) = failed {
                return Err(error);
            }
        }
        Ok(value)
    }

    /// The owners change: writes stop until they are known again; then every owner's file is read again.
    fn follow_owners(&self, change: &OwnersChange<'_>) {
        match change {
            OwnersChange::Pause => self.lock().paused = true,
            OwnersChange::Resume(owners) => {
                {
                    let mut state = self.lock();
                    // already running on these owners (the `Resume` told when the owners became known): nothing to do
                    if !state.paused && state.owners.as_ref().is_some_and(|o| o.same(owners)) {
                        drop(state);
                        self.gate.open();
                        return;
                    }
                    state.slots = owners
                        .slots()
                        .iter()
                        .map(|s| Slot::of(&s.store, &self.announce))
                        .collect();
                    state.owners = Some((*owners).clone());
                    state.paused = false;
                    state.refresh();
                }
                self.docs
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clear();
                self.bump();
                self.gate.open();
            }
        }
    }

    fn comments(&self) -> Vec<Comment> {
        let mut state = self.lock();
        state.refresh();
        state.merged().comments
    }

    /// Every owner's asides that hold comments its file lacks now.
    fn asides(&self) -> Vec<CommentAside> {
        let mut state = self.lock();
        state.refresh();
        let now = now_ms();
        let mut out = Vec::new();
        for (i, slot) in state.slots.iter().enumerate() {
            let (Some(keeper), Some(prefix)) = (&slot.keeper, state.aside_prefix(i)) else {
                continue;
            };
            for aside in keeper.asides(now) {
                let count = missing(&aside.comments, &slot.book).len();
                if count > 0 {
                    out.push(CommentAside {
                        id: format!("{prefix}{}", aside.id),
                        count: u32::try_from(count).unwrap_or(u32::MAX),
                        at: aside.at,
                        big: aside.big,
                    });
                }
            }
        }
        out
    }

    /// Adds back the comments of aside `id` the file lacks (never one it holds), through the normal save; the aside
    /// goes once they are written. The number added (0: no such aside, restored | dismissed meanwhile).
    fn restore(&self, id: &str) -> Result<usize, IpcError> {
        self.gate.wait();
        let mut state = self.lock();
        let Some((i, n)) = state.aside_of(id)? else {
            return Ok(0);
        };
        state.refresh();
        let not_now = || IpcError::new(ErrorKind::Unavailable).subject("comments.json");
        if state.paused {
            return Err(not_now());
        }
        let slot = &mut state.slots[i];
        if let Some(why) = &slot.unreadable {
            return Err(not_now().detail(why.clone()));
        }
        let Some(aside) = slot
            .keeper
            .as_ref()
            .and_then(|k| k.asides(now_ms()).into_iter().find(|a| a.id == n))
        else {
            return Ok(0);
        };
        let back: Vec<Comment> = missing(&aside.comments, &slot.book)
            .into_iter()
            .cloned()
            .collect();
        if slot.book.comments.len() + back.len() > MAX_COMMENTS {
            return Err(IpcError::new(ErrorKind::TooLarge).subject("comments"));
        }
        let added = back.len();
        if added > 0 {
            slot.book.comments.extend(back);
            if let Err(error) = slot.save() {
                // not written: the file is read again as it is, the aside stays
                slot.stamp = None;
                state.refresh();
                return Err(not_now().detail(error.to_string()));
            }
        }
        // restored (or nothing left to restore): a failure to drop it only leaves an aside with nothing to add
        if let Some(keeper) = &state.slots[i].keeper {
            let _ = keeper.drop_aside(n);
        }
        state.refresh();
        drop(state);
        if added > 0 {
            self.bump();
        }
        Ok(added)
    }

    /// Lets aside `id` go without restoring it.
    fn dismiss_aside(&self, id: &str) -> Result<(), IpcError> {
        let state = self.lock();
        if let Some((i, n)) = state.aside_of(id)? {
            if let Some(keeper) = &state.slots[i].keeper {
                keeper.drop_aside(n)?;
            }
        }
        Ok(())
    }

    /// The document `path` as comments see it (read again when its length or time changed).
    fn doc(&self, path: &str) -> Doc {
        let Ok((key, abs)) = self.scope.resolve(path) else {
            return Doc::Missing;
        };
        let Ok(meta) = std::fs::metadata(&abs) else {
            self.docs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&key);
            return Doc::Missing;
        };
        if !meta.is_file() || meta.len() > MAX_DOC_BYTES {
            return Doc::Missing;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis());
        if let Some(cached) = self
            .docs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
            .filter(|c| c.len == meta.len() && c.mtime == mtime)
        {
            return cached.doc.clone();
        }
        let doc = fsx::read_text(&abs, MAX_DOC_BYTES).map_or(Doc::Missing, |decoded| {
            let (parts, last) = units(&decoded.text);
            Doc::Text(Arc::new(Analysis::of(&parts, last)))
        });
        self.docs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                key,
                Cached {
                    len: meta.len(),
                    mtime,
                    doc: doc.clone(),
                },
            );
        doc
    }

    fn forget_doc(&self, path: &str) {
        self.docs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&key_of(path));
    }
}

/// The comments of an aside whose id the file's `book` does not hold (each id once).
fn missing<'a>(aside: &'a [Comment], book: &Book) -> Vec<&'a Comment> {
    let mut seen: HashSet<&str> = book.comments.iter().map(|c| c.id.as_str()).collect();
    aside
        .iter()
        .filter(|c| seen.insert(c.id.as_str()))
        .collect()
}

/// Where a comment is now: its state, its lines, and the place when the quote was found.
fn place(comment: &Comment, doc: &Doc) -> (PassageState, u32, u32, Option<Placement>) {
    let anchor = &comment.anchor;
    match doc {
        Doc::Missing => (
            PassageState::DocumentMissing,
            anchor.line,
            anchor.end_line,
            None,
        ),
        Doc::Text(analysis) => match analysis.resolve(anchor) {
            Resolution::Found(found) => {
                let state = if found.start_line == anchor.line && found.end_line == anchor.end_line
                {
                    PassageState::Anchored
                } else {
                    PassageState::Moved
                };
                (state, found.start_line, found.end_line, Some(found))
            }
            Resolution::Changed { line } => (PassageState::PassageChanged, line, line, None),
        },
    }
}

fn view_of(comment: &Comment, doc: &Doc) -> (CommentView, Option<Placement>) {
    let (state, line, end_line, found) = place(comment, doc);
    let heading_path = found.as_ref().map_or_else(
        || comment.anchor.heading_path.clone(),
        |f| f.heading_path.clone(),
    );
    let view = CommentView {
        id: comment.id.clone(),
        path: comment.path.clone(),
        text: comment.text.clone(),
        quote: comment.anchor.quote.clone(),
        heading_path,
        line,
        end_line,
        created_ms: comment.created_ms,
        edited_ms: comment.edited_ms,
        state,
        done: comment.done.is_some(),
        done_ms: comment.done.as_ref().map(|d| d.at_ms),
        done_by: comment.done.as_ref().map(|d| d.by),
        reply: comment.done.as_ref().and_then(|d| d.reply.clone()),
        replies: comment
            .replies
            .iter()
            .map(|r| ReplyView {
                at_ms: r.at_ms,
                by: r.by,
                text: r.text.clone(),
                edited_ms: r.edited_ms,
            })
            .collect(),
    };
    (view, found)
}

enum Msg {
    /// Paths (documents or folders) that changed, were removed or moved, and the moves.
    Changed {
        paths: Vec<String>,
        renames: Vec<(String, String)>,
    },
    Stop,
}

pub struct CommentsService {
    shared: Arc<Shared>,
    tx: Sender<Msg>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn not_found(subject: &str) -> IpcError {
    IpcError::new(ErrorKind::NotFound).subject(subject)
}

/// The owners whose comments stay away from agents, decided once for one call ([`CommentsService::withheld`]).
pub struct Withheld {
    owners: Option<Owners>,
    slots: Vec<bool>,
}

impl Withheld {
    /// Whether the comment at root-relative `path` is kept by a withheld owner.
    #[must_use]
    pub fn holds(&self, path: &str) -> bool {
        self.owners.as_ref().is_some_and(|owners| {
            self.slots
                .get(owners.owner_of(path))
                .copied()
                .unwrap_or(false)
        })
    }
}

impl CommentsService {
    /// Reads the file and starts the worker that keeps the passages found.
    #[must_use]
    pub fn start(file: RawFile, scope: DocScope, announce: Announce) -> Self {
        Self::start_with(None, vec![Slot::new(file, None)], scope, announce)
    }

    /// The comments of every owner (the opened folder + included projects, `projects.md` §9.2).
    #[must_use]
    pub fn start_owners(owners: Owners, scope: DocScope, announce: Announce) -> Self {
        let slots = owners
            .slots()
            .iter()
            .map(|s| Slot::of(&s.store, &announce))
            .collect();
        Self::start_with(Some(owners), slots, scope, announce)
    }

    /// The owners changed (`Projects::follow_owners`).
    pub fn follow_owners(&self, change: &OwnersChange<'_>) {
        self.shared.follow_owners(change);
    }

    /// Started before the owners were known: changes wait for the first `Resume`.
    pub fn start_paused(&self) {
        self.shared.gate.close();
        self.shared.follow_owners(&OwnersChange::Pause);
    }

    fn start_with(
        owners: Option<Owners>,
        slots: Vec<Slot>,
        scope: DocScope,
        announce: Announce,
    ) -> Self {
        let shared = Arc::new(Shared {
            scope,
            announce,
            state: Mutex::new(State {
                owners,
                slots,
                paused: false,
            }),
            docs: Mutex::new(HashMap::new()),
            rev: AtomicU64::new(1),
            gate: OwnersGate::default(),
        });
        {
            let mut state = shared.lock();
            state.refresh();
        }
        let (tx, rx) = channel();
        let worker = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("permark-comments".to_owned())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || run(&worker, &rx))
            .ok();
        Self {
            shared,
            tx,
            thread: Mutex::new(thread),
        }
    }

    /// The project's watch reported a batch (called on the watch thread: quick).
    pub fn feed(&self, batch: &FsBatch) {
        let mut paths = Vec::new();
        for change in &batch.changes {
            if let ChangeKind::Renamed { from } = &change.kind {
                paths.push(from.clone());
            }
            paths.push(change.path.clone());
        }
        if batch.bulk || batch.rescan {
            paths.push(String::new());
        }
        if !paths.is_empty() {
            // a moved document or folder takes its comments along
            let renames: Vec<(String, String)> = batch
                .changes
                .iter()
                .filter_map(|c| match &c.kind {
                    ChangeKind::Renamed { from } => Some((from.clone(), c.path.clone())),
                    _ => None,
                })
                .collect();
            let _ = self.tx.send(Msg::Changed { paths, renames });
        }
    }

    /// The comments with their places worked out against the documents as they are now. A passage that moved is
    /// written down, so a later edit that removes it still finds its way back to roughly the same section.
    ///
    /// # Errors
    /// The file cannot be read (it may be fine a moment later).
    pub fn list(
        &self,
        path: Option<&str>,
        scope: CommentScope,
    ) -> Result<Vec<CommentView>, IpcError> {
        {
            let mut state = self.shared.lock();
            state.refresh();
            if let Some(why) = state.unreadable() {
                return Err(IpcError::new(ErrorKind::Unavailable)
                    .subject("comments.json")
                    .detail(why.clone()));
            }
        }
        let wanted = path.map(key_of);
        let mut views = Vec::new();
        let mut moved: Vec<(String, Placement)> = Vec::new();
        let mut docs: HashMap<String, Doc> = HashMap::new();
        for comment in self.shared.comments() {
            if wanted.as_ref().is_some_and(|w| *w != key_of(&comment.path)) {
                continue;
            }
            let doc = docs
                .entry(key_of(&comment.path))
                .or_insert_with(|| self.shared.doc(&comment.path))
                .clone();
            let (view, found) = view_of(&comment, &doc);
            if view.state == PassageState::Moved && comment.done.is_none() {
                moved.extend(found.map(|f| (comment.id.clone(), f)));
            }
            if scope.admits(view.done) {
                views.push(view);
            }
        }
        if !moved.is_empty() {
            write_down(&self.shared, &moved);
        }
        views.sort_by(|a, b| {
            a.path
                .to_lowercase()
                .cmp(&b.path.to_lowercase())
                .then(a.line.cmp(&b.line))
                .then(a.created_ms.cmp(&b.created_ms))
                .then(a.id.cmp(&b.id))
        });
        Ok(views)
    }

    /// The document and heading comment `id` holds as stored: no document is read and nothing is written (`list`
    /// would look for every passage again and may write the places that moved).
    #[must_use]
    pub fn about(&self, id: &str) -> Option<(String, Vec<String>)> {
        self.shared
            .comments()
            .into_iter()
            .find(|c| c.id == id)
            .map(|c| (c.path, c.anchor.heading_path))
    }

    /// Writes a comment on a passage. The document decides where the passage is when it holds the quote; the window's
    /// values are the fallback.
    ///
    /// # Errors
    /// Not a document of the project, an empty quote or text, too many comments, or the file cannot be used.
    pub fn add(&self, new: &NewComment) -> Result<CommentView, IpcError> {
        let path = VaultPath::parse(&new.path)?.to_string();
        let quote: String = cut(new.quote.trim(), MAX_QUOTE_CHARS);
        let text = cut(new.text.trim(), MAX_COMMENT_CHARS);
        if fold(&quote).is_empty() {
            return Err(IpcError::new(ErrorKind::InvalidData).subject("quote"));
        }
        if text.is_empty() {
            return Err(IpcError::new(ErrorKind::InvalidData).subject("text"));
        }
        let Doc::Text(analysis) = self.shared.doc(&path) else {
            return Err(not_found(&path));
        };
        let probe = Probe {
            line: new.line,
            ..Probe::default()
        };
        let anchor = match analysis.locate(&quote, &probe) {
            Some(found) => Anchor {
                quote,
                before: found.before,
                after: found.after,
                heading_path: found.heading_path,
                line: found.start_line,
                end_line: found.end_line,
                extra: BTreeMap::default(),
            },
            None => Anchor {
                quote,
                heading_path: new.heading_path.clone(),
                line: new.line.max(1),
                end_line: new.end_line.max(new.line).max(1),
                ..Anchor::default()
            },
        };
        let comment = Comment {
            id: Self::new_id(),
            path,
            text,
            edited_ms: None,
            anchor,
            created_ms: now_ms(),
            done: None,
            replies: Vec::new(),
            extra: BTreeMap::default(),
        };
        let kept = comment.clone();
        self.shared.change(move |book| {
            if book.comments.len() >= MAX_COMMENTS {
                return Err(IpcError::new(ErrorKind::TooLarge).subject("comments"));
            }
            book.comments.push(comment);
            Ok(((), true))
        })?;
        let doc = Doc::Text(analysis);
        Ok(view_of(&kept, &doc).0)
    }

    fn new_id() -> String {
        // unique across the cores of this app (one counter) and across apps writing the same file (process id)
        let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        format!("c{:x}{:x}-{:x}", now_ms(), n % 4096, std::process::id())
    }

    /// Marks a comment done (the person did) or open again.
    ///
    /// # Errors
    /// No such comment, or the file cannot be used.
    pub fn set_done(&self, id: &str, done: bool) -> Result<(), IpcError> {
        self.shared.change(|book| {
            let comment = book
                .comments
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| not_found(id))?;
            let was = comment.done.is_some();
            if done && !was {
                comment.done = Some(crate::model::Done {
                    at_ms: now_ms(),
                    by: By::User,
                    reply: None,
                    extra: BTreeMap::default(),
                });
            } else if !done {
                comment.done = None;
            }
            Ok(((), was != comment.done.is_some()))
        })
    }

    /// Which owners' comments stay away from agents now: `tracked` answers per owner (it may ask git, so it runs
    /// outside the comments lock, once per owner).
    #[must_use]
    pub fn withheld(&self, tracked: impl Fn(&OwnerSlot) -> bool) -> Withheld {
        let owners = self.shared.lock().owners.clone();
        let slots = owners
            .as_ref()
            .map_or_else(Vec::new, |o| o.slots().iter().map(tracked).collect());
        Withheld { owners, slots }
    }

    /// Marks a comment done by an agent, with its short reply (the only change an agent makes). Already done: left
    /// as it is.
    ///
    /// # Errors
    /// No such comment, one of a `withheld` owner (`Denied`), or the file cannot be used.
    pub fn resolve_by_agent(
        &self,
        id: &str,
        reply: Option<&str>,
        withheld: &Withheld,
    ) -> Result<(), IpcError> {
        let reply = reply
            .map(|r| cut(r.trim(), MAX_REPLY_CHARS))
            .filter(|r| !r.is_empty());
        self.shared.change(|book| {
            let comment = book
                .comments
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| not_found(id))?;
            if withheld.holds(&comment.path) {
                return Err(IpcError::new(ErrorKind::Denied).subject(id));
            }
            if comment.done.is_some() {
                return Ok(((), false));
            }
            comment.done = Some(crate::model::Done {
                at_ms: now_ms(),
                by: By::Agent,
                reply,
                extra: BTreeMap::default(),
            });
            Ok(((), true))
        })
    }

    /// Deletes a comment.
    ///
    /// # Errors
    /// No such comment, or the file cannot be used.
    pub fn remove(&self, id: &str) -> Result<(), IpcError> {
        self.shared.change(|book| {
            let before = book.comments.len();
            book.comments.retain(|c| c.id != id);
            if book.comments.len() == before {
                return Err(not_found(id));
            }
            Ok(((), true))
        })
    }

    /// Adds an answer under a comment.
    ///
    /// # Errors
    /// No such comment, an empty text, too many replies, or the file cannot be used.
    pub fn reply(&self, id: &str, text: &str) -> Result<(), IpcError> {
        let text = cut(text.trim(), MAX_REPLY_CHARS);
        if text.is_empty() {
            return Err(IpcError::new(ErrorKind::InvalidData).subject("text"));
        }
        self.shared.change(|book| {
            let comment = book
                .comments
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| not_found(id))?;
            if comment.replies.len() >= MAX_REPLIES {
                return Err(IpcError::new(ErrorKind::TooLarge).subject("replies"));
            }
            comment.replies.push(Reply {
                at_ms: now_ms(),
                by: By::User,
                text,
                edited_ms: None,
                extra: BTreeMap::default(),
            });
            Ok(((), true))
        })
    }

    /// Changes the text of the person's comment, or of one of the person's replies under it. Only that text and its
    /// edit time change; the same text again writes nothing.
    ///
    /// # Errors
    /// No such comment | reply (a stale window), a reply the agent wrote, an empty text, a text over the limit, or the
    /// file cannot be used.
    pub fn edit(&self, id: &str, target: EditTarget, text: &str) -> Result<(), IpcError> {
        let limit = match target {
            EditTarget::Comment => MAX_COMMENT_CHARS,
            EditTarget::Reply(_) => MAX_REPLY_CHARS,
        };
        let text = text.trim();
        if text.is_empty() {
            return Err(IpcError::new(ErrorKind::InvalidData).subject("text"));
        }
        // refused rather than cut (as adding does): cutting text the person already wrote would lose some silently
        if text.chars().count() > limit {
            return Err(IpcError::new(ErrorKind::TooLarge).subject("text"));
        }
        self.shared.change(|book| {
            let comment = book
                .comments
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| not_found(id))?;
            let (kept, edited) = match target {
                EditTarget::Comment => (&mut comment.text, &mut comment.edited_ms),
                EditTarget::Reply(n) => {
                    let reply = comment
                        .replies
                        .get_mut(n)
                        .ok_or_else(|| not_found(&format!("{id}/{n}")))?;
                    if reply.by != By::User {
                        return Err(IpcError::new(ErrorKind::Denied).subject("reply"));
                    }
                    (&mut reply.text, &mut reply.edited_ms)
                }
            };
            if kept.as_str() == text {
                return Ok(((), false));
            }
            text.clone_into(kept);
            *edited = Some(now_ms());
            Ok(((), true))
        })
    }

    /// The comments an outside write removed, per owner, while the file still lacks some of them.
    #[must_use]
    pub fn asides(&self) -> Vec<CommentAside> {
        self.shared.asides()
    }

    /// Adds back the comments of an aside the file lacks; the number added (0: no such aside any more).
    ///
    /// # Errors
    /// An id that is not `<digits>` | `<uid>:<digits>`, or the file cannot be written now (the aside stays).
    pub fn restore(&self, id: &str) -> Result<usize, IpcError> {
        self.shared.restore(id)
    }

    /// Lets an aside go (no such aside: nothing to do).
    ///
    /// # Errors
    /// An id that is not `<digits>` | `<uid>:<digits>`, or the asides cannot be written.
    pub fn dismiss_aside(&self, id: &str) -> Result<(), IpcError> {
        self.shared.dismiss_aside(id)
    }

    /// Changes with every update the window should hear about.
    #[must_use]
    pub fn rev(&self) -> u64 {
        self.shared.rev.load(Ordering::Relaxed)
    }
}

impl Drop for CommentsService {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = thread.join();
        }
    }
}

/// The worker: gathers the changed paths, then looks the affected passages up again.
fn run(shared: &Shared, rx: &std::sync::mpsc::Receiver<Msg>) {
    let mut paths: Vec<String> = Vec::new();
    let mut renames: Vec<(String, String)> = Vec::new();
    let mut deadline: Option<Instant> = None;
    loop {
        let message = match deadline {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match message {
            Ok(Msg::Changed {
                paths: more,
                renames: moves,
            }) => {
                paths.extend(more);
                renames.extend(moves);
                deadline.get_or_insert_with(|| Instant::now() + QUIET);
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                deadline = None;
                let moves = std::mem::take(&mut renames);
                if !moves.is_empty() {
                    let _ = shared.change(|book| {
                        let mut changed = false;
                        for (from, to) in &moves {
                            changed |= rename_in(book, from, to);
                        }
                        Ok(((), changed))
                    });
                }
                reanchor(shared, &std::mem::take(&mut paths));
            }
        }
    }
}

/// A document with comments changed: its passages are looked for again, and the window hears about it when anything
/// moved.
fn reanchor(shared: &Shared, paths: &[String]) {
    let everything = paths.iter().any(String::is_empty);
    let keys: Vec<String> = paths.iter().map(|p| key_of(p)).collect();
    let affected: Vec<Comment> = shared
        .comments()
        .into_iter()
        .filter(|c| {
            let key = key_of(&c.path);
            everything
                || keys
                    .iter()
                    .any(|k| *k == key || key.starts_with(&format!("{k}/")))
        })
        .collect();
    if affected.is_empty() {
        return;
    }
    for comment in &affected {
        shared.forget_doc(&comment.path);
    }
    // the passages that moved are written down; the window hears about it either way (a state may have changed)
    let mut moved: Vec<(String, Placement)> = Vec::new();
    for comment in affected.iter().filter(|c| c.done.is_none()) {
        let doc = shared.doc(&comment.path);
        if let (PassageState::Moved, _, _, Some(found)) = place(comment, &doc) {
            moved.push((comment.id.clone(), found));
        }
    }
    if moved.is_empty() || !write_down(shared, &moved) {
        shared.bump();
    }
}

/// Writes down where passages moved to (the file is written and the window told); `false` when nothing changed.
fn write_down(shared: &Shared, moved: &[(String, Placement)]) -> bool {
    shared
        .change(|book| {
            let mut changed = false;
            for (id, place) in moved {
                if let Some(comment) = book.comments.iter_mut().find(|c| &c.id == id) {
                    comment.anchor.line = place.start_line;
                    comment.anchor.end_line = place.end_line;
                    comment.anchor.before.clone_from(&place.before);
                    comment.anchor.after.clone_from(&place.after);
                    comment.anchor.heading_path.clone_from(&place.heading_path);
                    changed = true;
                }
            }
            Ok((changed, changed))
        })
        .unwrap_or(false)
}

/// A document or folder moved: the comments of what is inside follow it. `true` when any did.
fn rename_in(book: &mut Book, from: &str, to: &str) -> bool {
    let from_key = key_of(from);
    let prefix = format!("{from_key}/");
    let mut changed = false;
    for comment in &mut book.comments {
        let key = key_of(&comment.path);
        if key == from_key {
            to.clone_into(&mut comment.path);
            changed = true;
        } else if key.starts_with(&prefix) {
            if let Some(rest) = comment.path.get(from.len()..) {
                comment.path = format!("{to}{rest}");
                changed = true;
            }
        }
    }
    changed
}
