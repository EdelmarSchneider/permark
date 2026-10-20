//! The comments of one project: `comments.json` (the person's own data, layout of 0.5) and the work of finding every
//! passage again when its document changes.
//!
//! Every change reads the file again when its length or time differ (a change made by `permark mcp` with the window
//! closed is not lost), changes it and writes it back atomically. A file that is not valid comments (or comes from a
//! newer layout) is set aside, not overwritten: the comments in it are the person's. A file that merely cannot be read
//! right now is left alone and the change fails.

use std::collections::{BTreeMap, HashMap};
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
use permark_core::state::RawFile;
use permark_core::watch::{ChangeKind, FsBatch};

use crate::anchor::{fold, Analysis, Placement, Probe, Resolution};
use crate::model::{
    Anchor, Book, By, Comment, CommentScope, CommentView, PassageState, Reply, ReplyView,
    MAX_COMMENTS, MAX_COMMENT_CHARS, MAX_QUOTE_CHARS, MAX_REPLIES, MAX_REPLY_CHARS, VERSION,
};

/// Documents larger than this are not read for comments.
const MAX_DOC_BYTES: u64 = 5 * 1024 * 1024;
/// How long changes of documents are gathered before the passages are looked for again.
const QUIET: Duration = Duration::from_millis(150);

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

struct State {
    book: Book,
    /// Length and time of the file as last read or written.
    stamp: Option<(u64, u128)>,
    /// The file exists and holds nothing this version can use: it is put aside before the first write.
    damaged: bool,
    /// The file could not be read just now (it may be fine a moment later).
    unreadable: Option<String>,
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
    file: RawFile,
    scope: DocScope,
    announce: Arc<dyn Fn() + Send + Sync>,
    state: Mutex<State>,
    docs: Mutex<HashMap<String, Cached>>,
    counter: AtomicU64,
    rev: AtomicU64,
}

fn now_ms() -> u64 {
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
        (self.announce)();
    }

    /// Reads the file again when it is not what was last read.
    fn refresh(&self, state: &mut State) {
        let stamp = self.file.stamp();
        if state.stamp == stamp && state.unreadable.is_none() {
            return;
        }
        match self.file.read() {
            Ok(Some(bytes)) => {
                let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
                match serde_json::from_slice::<Book>(bytes) {
                    Ok(book) if book.version <= VERSION => {
                        state.book = book;
                        state.damaged = false;
                    }
                    _ => {
                        state.book = Book::default();
                        state.damaged = true;
                    }
                }
                state.unreadable = None;
            }
            Ok(None) => {
                state.book = Book::default();
                state.damaged = false;
                state.unreadable = None;
            }
            Err(error) => {
                state.unreadable = Some(error.to_string());
                return;
            }
        }
        state.stamp = stamp;
    }

    /// Writes the book (the unreadable file is put aside first).
    fn save(&self, state: &mut State) -> Result<(), IpcError> {
        if state.damaged {
            let aside = self
                .file
                .path()
                .with_file_name(format!("comments.unreadable-{}.json", now_ms()));
            std::fs::rename(self.file.path(), aside)
                .map_err(|e| IpcError::io("comments.json", &e))?;
            state.damaged = false;
        }
        let bytes = serde_json::to_vec_pretty(&state.book)
            .map_err(|e| IpcError::new(ErrorKind::Internal).detail(e.to_string()))?;
        self.file.write(&bytes)?;
        state.stamp = self.file.stamp();
        Ok(())
    }

    /// Changes the comments. `change` says whether it changed anything; only then is the file written.
    fn change<T>(
        &self,
        change: impl FnOnce(&mut Book) -> Result<(T, bool), IpcError>,
    ) -> Result<T, IpcError> {
        let mut state = self.lock();
        self.refresh(&mut state);
        if let Some(why) = &state.unreadable {
            return Err(IpcError::new(ErrorKind::Unavailable)
                .subject("comments.json")
                .detail(why.clone()));
        }
        let (value, changed) = change(&mut state.book)?;
        if changed {
            self.save(&mut state)?;
            drop(state);
            self.bump();
        }
        Ok(value)
    }

    fn comments(&self) -> Vec<Comment> {
        let mut state = self.lock();
        self.refresh(&mut state);
        state.book.comments.clone()
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

impl CommentsService {
    /// Reads the file and starts the worker that keeps the passages found.
    #[must_use]
    pub fn start(file: RawFile, scope: DocScope, announce: Arc<dyn Fn() + Send + Sync>) -> Self {
        let shared = Arc::new(Shared {
            file,
            scope,
            announce,
            state: Mutex::new(State {
                book: Book::default(),
                stamp: None,
                damaged: false,
                unreadable: None,
            }),
            docs: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
            rev: AtomicU64::new(1),
        });
        {
            let mut state = shared.lock();
            shared.refresh(&mut state);
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
            self.shared.refresh(&mut state);
            if let Some(why) = &state.unreadable {
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
            id: self.new_id(),
            path,
            text,
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

    fn new_id(&self) -> String {
        let n = self.shared.counter.fetch_add(1, Ordering::Relaxed);
        format!("c{:x}{:03x}", now_ms(), n % 4096)
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

    /// Marks a comment done by an agent, with its short reply (the only change an agent makes). Already done: left
    /// as it is.
    ///
    /// # Errors
    /// No such comment, or the file cannot be used.
    pub fn resolve_by_agent(&self, id: &str, reply: Option<&str>) -> Result<(), IpcError> {
        let reply = reply
            .map(|r| cut(r.trim(), MAX_REPLY_CHARS))
            .filter(|r| !r.is_empty());
        self.shared.change(|book| {
            let comment = book
                .comments
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| not_found(id))?;
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
                extra: BTreeMap::default(),
            });
            Ok(((), true))
        })
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
