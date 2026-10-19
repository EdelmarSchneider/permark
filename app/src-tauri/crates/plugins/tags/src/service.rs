//! The tags service of one project: the app's entries in memory, written to `meta.toml` a moment after the last
//! change (surgically, see [`crate::meta`]); the front matter tags found in the documents (a catalog kept by its own
//! thread); the answers the UI and search ask for.
//!
//! `meta.toml` lives where the state store says: `.permark/meta.toml` when the project has `.permark/` (watched: an
//! edit by someone else is read in, the changes made here and not written yet laid on top), else in the profile. A file
//! that cannot be read is never written over: the app's tags are read-only until it is fixed, changes made before it
//! broke are parked and written once it reads again.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, UNIX_EPOCH};

use permark_core::document::DocScope;
use permark_core::error::{ErrorKind, IpcError};
use permark_core::fsx;
use permark_core::organizing::TagLookup;
use permark_core::paths::{key_of, VaultPath, WORKTREE_PREFIX};
use permark_core::state::RawFile;
use permark_core::tree::{category_of, Category, Explorer};
use permark_core::watch::{ChangeKind, FsBatch, OwnWrites};

use crate::catalog::{self, DocTags, Docs};
use crate::frontmatter::{from_text, HEAD_BYTES};
use crate::meta::{edit_text, Entries, Op, Problem};
use crate::normalize::normalize_tags;
use crate::types::{
    FileMeta, IndexEntry, MetaProblem, TagCountView, TagRefView, TagStyleView, TaggedFile,
    TagsIndex, TagsLocation, TagsOverview, TagsStatus,
};

/// Files a "files with these tags" answer carries at most.
const MAX_LISTED: usize = 5000;

/// When writes happen.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Written this long after the last change...
    pub quiet: Duration,
    /// ...but never later than this after the first one.
    pub max_wait: Duration,
    /// Pauses before the next try after a failed write (the last one repeats).
    pub retry: [Duration; 3],
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(400),
            max_wait: Duration::from_secs(2),
            retry: [
                Duration::from_millis(500),
                Duration::from_secs(2),
                Duration::from_secs(10),
            ],
        }
    }
}

/// What the service is made of.
pub struct ServiceArgs {
    /// Where the entries are kept.
    pub file: RawFile,
    /// The profile's copy from before the project had `.permark/` (merged in, then deleted).
    pub legacy: Option<RawFile>,
    pub location: TagsLocation,
    /// Project path of `file` when the watch reports it (`.permark/meta.toml`).
    pub watched: Option<String>,
    pub own: Arc<OwnWrites>,
    pub scope: DocScope,
    pub explorer: Arc<Explorer>,
    /// Called when something the UI shows changed.
    pub announce: Arc<dyn Fn() + Send + Sync>,
    pub timing: Timing,
}

struct Inner {
    entries: Entries,
    /// Changes made here and not written yet, oldest first.
    pending: Vec<Op>,
    problem: Option<Problem>,
    /// The last read or write failed (it is tried again).
    io_error: Option<String>,
    /// Hash of the bytes last read or written (`None`: no file).
    disk_hash: Option<u64>,
    loaded: bool,
    /// The legacy file is merged into the pending changes and is deleted after the next successful write.
    legacy_merged: bool,
}

struct Shared {
    file: RawFile,
    legacy: Option<RawFile>,
    location: TagsLocation,
    watched: Option<String>,
    own: Arc<OwnWrites>,
    scope: DocScope,
    explorer: Arc<Explorer>,
    announce: Arc<dyn Fn() + Send + Sync>,
    timing: Timing,
    inner: Mutex<Inner>,
    docs: RwLock<Docs>,
    /// The first look at the documents is done.
    catalog_ready: AtomicBool,
    rev: AtomicU64,
}

enum MetaMsg {
    /// Something was edited: write after the quiet period.
    Edited,
    /// The file may have changed.
    Reload,
    Stop,
}

enum CatalogMsg {
    Batch(FsBatch),
    Stop,
}

enum Flush {
    /// Nothing to write, or written.
    Done,
    /// The file cannot be read: wait for it to change.
    Parked,
    /// Writing failed: try again later.
    Failed,
}

fn bump(shared: &Shared) {
    shared.rev.fetch_add(1, Ordering::Relaxed);
    (shared.announce)();
}

fn mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn is_worktree(path: &str) -> bool {
    path.starts_with(WORKTREE_PREFIX)
}

fn is_markdown(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    category_of(name) == Category::Markdown
}

/// The bytes of a `meta.toml` as text (a BOM is not part of it).
fn text_of(bytes: &[u8]) -> Result<(bool, &str), Problem> {
    let (bom, rest) = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    std::str::from_utf8(rest)
        .map(|t| (bom, t))
        .map_err(|_| Problem {
            message: "the file is not valid UTF-8".to_owned(),
            line: None,
        })
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the file on disk as the truth, then lays what was changed here and not written yet on top of it.
    fn reload(&self) {
        let mut read = self.file.read();
        // many programs write by truncating first: an empty file is believed only after a moment
        for pause in [100, 300] {
            let looks_cut = matches!(&read, Ok(None))
                || matches!(&read, Ok(Some(b)) if b.iter().all(u8::is_ascii_whitespace));
            if !looks_cut || self.lock().entries.is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(pause));
            read = self.file.read();
        }
        let bytes = match read {
            Ok(bytes) => bytes,
            Err(error) => {
                self.lock().io_error = Some(error.to_string());
                bump(self);
                return;
            }
        };
        let hash = bytes.as_deref().map(|b| fsx::hash(b).0);
        {
            let inner = self.lock();
            if inner.loaded && inner.disk_hash == hash {
                return;
            }
        }
        let parsed = match &bytes {
            None => Ok(Entries::default()),
            Some(bytes) => text_of(bytes).and_then(|(_, text)| Entries::parse(text)),
        };
        let mut inner = self.lock();
        inner.loaded = true;
        inner.disk_hash = hash;
        inner.io_error = None;
        match parsed {
            Ok(mut entries) => {
                for op in &inner.pending {
                    entries.apply(op);
                }
                inner.entries = entries;
                inner.problem = None;
            }
            Err(problem) => inner.problem = Some(problem),
        }
        drop(inner);
        bump(self);
    }

    /// Writes the pending changes into the file as it is now.
    fn flush(&self) -> Flush {
        let ops = {
            let inner = self.lock();
            if inner.pending.is_empty() && !inner.legacy_merged {
                return Flush::Done;
            }
            if inner.problem.is_some() {
                return Flush::Parked;
            }
            inner.pending.clone()
        };
        let fail = |message: String| {
            self.lock().io_error = Some(message);
            bump(self);
            Flush::Failed
        };
        let bytes = match self.file.read() {
            Ok(bytes) => bytes,
            Err(error) => return fail(error.to_string()),
        };
        let (bom, text) = match bytes.as_deref().map(text_of) {
            None => (false, ""),
            Some(Ok(parts)) => parts,
            Some(Err(problem)) => {
                self.lock().problem = Some(problem);
                bump(self);
                return Flush::Parked;
            }
        };
        let new_text = match edit_text(text, &ops) {
            Ok(text) => text,
            Err(problem) => {
                self.lock().problem = Some(problem);
                bump(self);
                return Flush::Parked;
            }
        };
        let mut out = Vec::with_capacity(new_text.len() + 3);
        if bom {
            out.extend_from_slice(b"\xEF\xBB\xBF");
        }
        out.extend_from_slice(new_text.as_bytes());
        let unchanged = bytes.as_deref() == Some(out.as_slice());
        let hash = fsx::hash(&out);
        if !unchanged && !ops.is_empty() {
            if let Some(path) = &self.watched {
                self.own.expect_write(path, hash);
            }
            if let Err(error) = self.file.write(&out) {
                if let Some(path) = &self.watched {
                    self.own.retract_write(path, hash);
                }
                return fail(error.to_string());
            }
        }
        let entries = Entries::parse(&new_text).unwrap_or_default();
        let legacy_done = {
            let mut inner = self.lock();
            let done = ops.len().min(inner.pending.len());
            inner.pending.drain(..done);
            let mut entries = entries;
            for op in &inner.pending {
                entries.apply(op);
            }
            inner.entries = entries;
            inner.disk_hash = if unchanged || !ops.is_empty() {
                Some(hash.0)
            } else {
                inner.disk_hash
            };
            inner.io_error = None;
            std::mem::take(&mut inner.legacy_merged)
        };
        if legacy_done {
            if let Some(legacy) = &self.legacy {
                // nothing is lost: it is all in `meta.toml` now
                let _ = legacy.remove();
            }
        }
        bump(self);
        Flush::Done
    }

    /// Merges the profile's copy (from before the project had `.permark/`) into the entries as pending changes.
    fn merge_legacy(&self) {
        let Some(legacy) = &self.legacy else { return };
        let Ok(Some(bytes)) = legacy.read() else {
            return;
        };
        let Ok(old) = text_of(&bytes).and_then(|(_, text)| Entries::parse(text)) else {
            // unreadable: left where it is, tried again at the next start
            return;
        };
        let mut inner = self.lock();
        let mut ops = Vec::new();
        for (path, entry) in old.iter() {
            let current = inner.entries.get(path).cloned().unwrap_or_default();
            let mut tags = current.tags.clone();
            for tag in &entry.tags {
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
            if tags != current.tags {
                ops.push(Op::SetTags {
                    path: path.to_owned(),
                    tags,
                });
            }
            if current.description.is_empty() && !entry.description.is_empty() {
                ops.push(Op::SetDescription {
                    path: path.to_owned(),
                    description: entry.description.clone(),
                });
            }
        }
        for (tag, style) in old.styles() {
            if inner.entries.style(tag).is_none() {
                ops.push(Op::SetStyle {
                    tag: tag.to_owned(),
                    color: style.color,
                    icon: style.icon,
                });
            }
        }
        for op in &ops {
            inner.entries.apply(op);
        }
        inner.pending.extend(ops);
        inner.legacy_merged = true;
    }
}

/// The writer: waits for changes, writes after the quiet period, retries failures, reads in outside edits.
fn meta_loop(shared: &Shared, rx: &Receiver<MetaMsg>) {
    let timing = shared.timing;
    let mut first: Option<Instant> = None;
    let mut deadline: Option<Instant> = None;
    let mut failures = 0usize;
    if !shared.lock().pending.is_empty() || shared.lock().legacy_merged {
        deadline = Some(Instant::now());
    }
    loop {
        let message = match deadline {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match message {
            Ok(MetaMsg::Edited) => {
                let now = Instant::now();
                let start = *first.get_or_insert(now);
                deadline = Some((now + timing.quiet).min(start + timing.max_wait));
            }
            Ok(MetaMsg::Reload) => {
                shared.reload();
                let waiting = !shared.lock().pending.is_empty();
                if waiting && shared.lock().problem.is_none() {
                    deadline = Some(Instant::now());
                }
            }
            Ok(MetaMsg::Stop) | Err(RecvTimeoutError::Disconnected) => {
                // what was changed is written before the service goes
                let _ = shared.flush();
                return;
            }
            Err(RecvTimeoutError::Timeout) => match shared.flush() {
                Flush::Done => {
                    failures = 0;
                    first = None;
                    deadline = None;
                }
                Flush::Parked => {
                    first = None;
                    deadline = None;
                }
                Flush::Failed => {
                    let pause = timing.retry[failures.min(timing.retry.len() - 1)];
                    failures += 1;
                    deadline = Some(Instant::now() + pause);
                }
            },
        }
    }
}

// ---- the catalog of front matter tags ----------------------------------------------------------------------------

/// What the front matter of one document says, read again only when its length or time changed.
fn read_doc(shared: &Shared, path: &str, known: Option<&DocTags>) -> Option<DocTags> {
    let (_, abs) = shared.scope.resolve(path).ok()?;
    read_doc_at(&abs, known)
}

fn read_doc_at(abs: &Path, known: Option<&DocTags>) -> Option<DocTags> {
    let meta = std::fs::metadata(abs).ok()?;
    if !meta.is_file() {
        return None;
    }
    let (len, mtime) = (meta.len(), mtime_ms(&meta));
    if let Some(known) = known.filter(|k| k.len == len && k.mtime_ms == mtime) {
        return Some(known.clone());
    }
    let mut head = Vec::new();
    fsx::retry(
        &[Duration::from_millis(50), Duration::from_millis(150)],
        || {
            head.clear();
            std::fs::File::open(abs)?
                .take(HEAD_BYTES as u64)
                .read_to_end(&mut head)
                .map(|_| ())
        },
    )
    .ok()?;
    let text = fsx::decode(&head)?.text;
    Some(DocTags {
        tags: from_text(&text),
        len,
        mtime_ms: mtime,
    })
}

fn scan_all(shared: &Shared, stop: &AtomicBool) {
    let (paths, _) = shared.explorer.scan_documents("");
    let known = shared
        .docs
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mut docs = Docs::new();
    for (at, path) in paths.iter().enumerate() {
        if at % 64 == 0 && stop.load(Ordering::Relaxed) {
            return;
        }
        if let Some(doc) = read_doc(shared, path, known.get(path)) {
            docs.insert(path.clone(), doc);
        }
    }
    *shared.docs.write().unwrap_or_else(PoisonError::into_inner) = docs;
    shared.catalog_ready.store(true, Ordering::Release);
    bump(shared);
}

/// Applies one batch to the catalog; `true` when something changed.
fn apply_batch(shared: &Shared, batch: &FsBatch) -> bool {
    let mut docs = shared.docs.write().unwrap_or_else(PoisonError::into_inner);
    let mut changed = false;
    for change in &batch.changes {
        if is_worktree(&change.path) {
            continue;
        }
        let below = format!("{}/", change.path);
        match &change.kind {
            ChangeKind::Removed => {
                if change.is_dir {
                    let before = docs.len();
                    docs.retain(|p, _| !p.starts_with(&below));
                    changed |= docs.len() != before;
                } else {
                    changed |= docs.remove(&change.path).is_some();
                }
            }
            ChangeKind::Renamed { from } => {
                if change.is_dir {
                    let from_below = format!("{from}/");
                    let moved: Vec<String> = docs
                        .keys()
                        .filter(|p| p.starts_with(&from_below))
                        .cloned()
                        .collect();
                    for old in moved {
                        if let Some(doc) = docs.remove(&old) {
                            docs.insert(format!("{}{}", below, &old[from_below.len()..]), doc);
                            changed = true;
                        }
                    }
                } else {
                    changed |= docs.remove(from).is_some();
                    changed |= refresh(shared, &mut docs, &change.path);
                }
            }
            ChangeKind::Created | ChangeKind::Modified => {
                if !change.is_dir {
                    changed |= refresh(shared, &mut docs, &change.path);
                }
            }
        }
    }
    changed
}

/// Reads `path` again into `docs`; `true` when the entry changed.
fn refresh(shared: &Shared, docs: &mut Docs, path: &str) -> bool {
    if !is_markdown(path) {
        return false;
    }
    let before = docs.get(path).cloned();
    match read_doc(shared, path, before.as_ref()) {
        Some(doc) => {
            let differs = before.as_ref().map(|b| &b.tags) != Some(&doc.tags);
            docs.insert(path.to_owned(), doc);
            differs
        }
        None => docs.remove(path).is_some(),
    }
}

fn catalog_loop(shared: &Shared, rx: &Receiver<CatalogMsg>, stop: &AtomicBool) {
    scan_all(shared, stop);
    while let Ok(message) = rx.recv() {
        let CatalogMsg::Batch(batch) = message else {
            return;
        };
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if batch.bulk || batch.rescan {
            scan_all(shared, stop);
        } else if apply_batch(shared, &batch) {
            bump(shared);
        }
    }
}

// ---- the service -------------------------------------------------------------------------------------------------

pub struct TagsService {
    shared: Arc<Shared>,
    meta_tx: Sender<MetaMsg>,
    catalog_tx: Sender<CatalogMsg>,
    stop: Arc<AtomicBool>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

fn denied(path: &str, why: &str) -> IpcError {
    IpcError::new(ErrorKind::Denied).subject(path).detail(why)
}

impl TagsService {
    /// Reads `meta.toml` (and merges the legacy copy), then starts the writer and the catalog.
    #[must_use]
    pub fn start(args: ServiceArgs) -> Self {
        let shared = Arc::new(Shared {
            file: args.file,
            legacy: args.legacy,
            location: args.location,
            watched: args.watched,
            own: args.own,
            scope: args.scope,
            explorer: args.explorer,
            announce: args.announce,
            timing: args.timing,
            inner: Mutex::new(Inner {
                entries: Entries::default(),
                pending: Vec::new(),
                problem: None,
                io_error: None,
                disk_hash: None,
                loaded: false,
                legacy_merged: false,
            }),
            docs: RwLock::new(Docs::new()),
            catalog_ready: AtomicBool::new(false),
            rev: AtomicU64::new(1),
        });
        shared.reload();
        // a legacy copy is merged only into a file that reads (the merge is written over it)
        if shared.lock().problem.is_none() {
            shared.merge_legacy();
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (meta_tx, meta_rx) = channel();
        let (catalog_tx, catalog_rx) = channel();
        let mut threads = Vec::new();
        let writer = Arc::clone(&shared);
        threads.extend(
            thread::Builder::new()
                .name("permark-tags-meta".to_owned())
                .spawn(move || meta_loop(&writer, &meta_rx))
                .ok(),
        );
        let scanner = Arc::clone(&shared);
        let flag = Arc::clone(&stop);
        threads.extend(
            thread::Builder::new()
                .name("permark-tags-catalog".to_owned())
                .stack_size(4 * 1024 * 1024)
                .spawn(move || catalog_loop(&scanner, &catalog_rx, &flag))
                .ok(),
        );
        Self {
            shared,
            meta_tx,
            catalog_tx,
            stop,
            threads: Mutex::new(threads),
        }
    }

    /// The project's watch reported a batch (called on the watch thread: quick).
    pub fn feed(&self, batch: &FsBatch) {
        if batch.is_empty() {
            return;
        }
        // moved files take their entries along
        let mut ops = Vec::new();
        let mut reload = false;
        for change in &batch.changes {
            if let Some(watched) = &self.shared.watched {
                if !change.own && key_of(&change.path) == key_of(watched) {
                    reload = true;
                }
            }
            if let ChangeKind::Renamed { from } = &change.kind {
                if is_worktree(from) || is_worktree(&change.path) {
                    continue;
                }
                ops.push(if change.is_dir {
                    Op::RenameTree {
                        from: from.clone(),
                        to: change.path.clone(),
                    }
                } else {
                    Op::Rename {
                        from: from.clone(),
                        to: change.path.clone(),
                    }
                });
            }
        }
        if !ops.is_empty() {
            self.queue(ops);
        }
        if reload {
            let _ = self.meta_tx.send(MetaMsg::Reload);
        }
        let _ = self.catalog_tx.send(CatalogMsg::Batch(batch.clone()));
    }

    /// Changes that follow from the project (moves): kept even while the file cannot be read.
    fn queue(&self, ops: Vec<Op>) {
        let mut inner = self.shared.lock();
        let touching: Vec<Op> = ops
            .into_iter()
            .filter(|op| match op {
                Op::Rename { from, .. } => inner.entries.get(from).is_some(),
                Op::RenameTree { from, .. } => {
                    let prefix = format!("{}/", key_of(from));
                    inner
                        .entries
                        .iter()
                        .any(|(p, _)| key_of(p).starts_with(&prefix))
                }
                _ => true,
            })
            .collect();
        if touching.is_empty() {
            return;
        }
        for op in &touching {
            inner.entries.apply(op);
        }
        inner.pending.extend(touching);
        drop(inner);
        bump(&self.shared);
        let _ = self.meta_tx.send(MetaMsg::Edited);
    }

    /// A change the person made.
    fn edit(&self, path: Option<&str>, op: Op) -> Result<(), IpcError> {
        if let Some(path) = path {
            if is_worktree(path) {
                return Err(denied(path, "worktree"));
            }
        }
        {
            let mut inner = self.shared.lock();
            if let Some(problem) = &inner.problem {
                return Err(IpcError::new(ErrorKind::Unavailable)
                    .subject("meta.toml")
                    .detail(match problem.line {
                        Some(line) => format!("line {line}: {}", problem.message),
                        None => problem.message.clone(),
                    }));
            }
            inner.entries.apply(&op);
            inner.pending.push(op);
        }
        bump(&self.shared);
        let _ = self.meta_tx.send(MetaMsg::Edited);
        Ok(())
    }

    /// A file of the project (not a folder, not missing).
    fn existing_file(&self, path: &str) -> Result<String, IpcError> {
        let parsed = VaultPath::parse(path)?;
        let (_, abs) = self.shared.scope.resolve(parsed.as_str())?;
        if !abs.is_file() {
            return Err(IpcError::new(ErrorKind::NotFound).subject(path));
        }
        Ok(parsed.to_string())
    }

    /// Replaces the tags the app keeps for a file.
    ///
    /// # Errors
    /// Not a file of the project, a worktree file, or `meta.toml` cannot be read.
    pub fn set_tags(&self, path: &str, tags: &[String]) -> Result<FileMeta, IpcError> {
        let path = self.existing_file(path)?;
        self.edit(Some(&path), Op::set_tags(&path, tags))?;
        self.file_meta(&path)
    }

    /// Replaces the description of a file.
    ///
    /// # Errors
    /// As [`TagsService::set_tags`].
    pub fn set_description(&self, path: &str, description: &str) -> Result<FileMeta, IpcError> {
        let path = self.existing_file(path)?;
        self.edit(Some(&path), Op::set_description(&path, description))?;
        self.file_meta(&path)
    }

    /// Gives a tag a color and an icon (a name outside the sets means none).
    ///
    /// # Errors
    /// The tag name is empty, or `meta.toml` cannot be read.
    pub fn set_style(
        &self,
        tag: &str,
        color: Option<&str>,
        icon: Option<&str>,
    ) -> Result<(), IpcError> {
        let op = Op::set_style(tag, color, icon)
            .ok_or_else(|| IpcError::new(ErrorKind::InvalidData).subject("tag"))?;
        self.edit(None, op)
    }

    #[must_use]
    pub fn status(&self) -> TagsStatus {
        let inner = self.shared.lock();
        TagsStatus {
            location: self.shared.location,
            problem: inner.problem.as_ref().map(|p| MetaProblem {
                message: p.message.clone(),
                line: p.line.and_then(|l| u32::try_from(l).ok()),
            }),
            write_error: inner.io_error.clone(),
            parked: u32::try_from(inner.pending.len()).unwrap_or(u32::MAX),
        }
    }

    fn rev(&self) -> u64 {
        self.shared.rev.load(Ordering::Relaxed)
    }

    /// Every file that has a tag or a description, and the looks of the tags.
    #[must_use]
    pub fn index(&self) -> TagsIndex {
        let rev = self.rev();
        let status = self.status();
        let inner = self.shared.lock();
        let docs = self
            .shared
            .docs
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        let entries = catalog::index(&inner.entries, &docs)
            .into_iter()
            .map(|row| IndexEntry {
                path: row.path,
                tags: row.tags,
                description: row.description,
            })
            .collect();
        let styles = inner
            .entries
            .styles()
            .map(|(tag, style)| {
                (
                    tag.to_owned(),
                    TagStyleView {
                        color: style.color.map(str::to_owned),
                        icon: style.icon.map(str::to_owned),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        TagsIndex {
            rev,
            entries,
            styles,
            status,
        }
    }

    /// Entries whose file is gone (an entry that is no valid project path is left alone).
    #[must_use]
    pub fn orphans(&self) -> Vec<String> {
        let paths: Vec<String> = self
            .shared
            .lock()
            .entries
            .iter()
            .map(|(p, _)| p.to_owned())
            .collect();
        paths
            .into_iter()
            .filter(|path| {
                !is_worktree(path)
                    && VaultPath::parse(path).is_ok()
                    && self
                        .shared
                        .scope
                        .resolve(path)
                        .is_ok_and(|(_, abs)| std::fs::symlink_metadata(abs).is_err())
            })
            .collect()
    }

    /// Tags with counts, untagged documents, orphans.
    #[must_use]
    pub fn overview(&self) -> TagsOverview {
        let rev = self.rev();
        let orphans = self.orphans().len();
        let inner = self.shared.lock();
        let docs = self
            .shared
            .docs
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        TagsOverview {
            rev,
            tags: catalog::tag_counts(&inner.entries, &docs)
                .into_iter()
                .map(|t| TagCountView {
                    name: t.name,
                    count: u32::try_from(t.count).unwrap_or(u32::MAX),
                    only_in_files: t.only_in_files,
                })
                .collect(),
            untagged: u32::try_from(catalog::untagged(&inner.entries, &docs).len())
                .unwrap_or(u32::MAX),
            orphans: u32::try_from(orphans).unwrap_or(u32::MAX),
        }
    }

    /// The files that have every one of `tags`, or (`untagged`) the documents that have none.
    #[must_use]
    pub fn files_with(&self, tags: &[String], untagged: bool) -> Vec<TaggedFile> {
        let (paths, descriptions, stamps) = {
            let inner = self.shared.lock();
            let docs = self
                .shared
                .docs
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            let mut paths = if untagged {
                catalog::untagged(&inner.entries, &docs)
            } else {
                catalog::files_with(&inner.entries, &docs, tags)
            };
            paths.truncate(MAX_LISTED);
            let descriptions: Vec<String> = paths
                .iter()
                .map(|p| {
                    inner
                        .entries
                        .get(p)
                        .map(|e| e.description.clone())
                        .unwrap_or_default()
                })
                .collect();
            let stamps: Vec<Option<u64>> = paths
                .iter()
                .map(|p| docs.get(p).map(|d| d.mtime_ms))
                .collect();
            (paths, descriptions, stamps)
        };
        paths
            .into_iter()
            .zip(descriptions)
            .zip(stamps)
            .map(|((path, description), stamp)| TaggedFile {
                modified_ms: stamp.unwrap_or_else(|| self.modified_ms(&path)),
                path,
                description,
            })
            .collect()
    }

    fn modified_ms(&self, path: &str) -> u64 {
        self.shared
            .scope
            .resolve(path)
            .ok()
            .and_then(|(_, abs)| std::fs::metadata(abs).ok())
            .map_or(0, |m| mtime_ms(&m))
    }

    /// The tags and description of one file.
    ///
    /// # Errors
    /// Not a valid project path.
    pub fn file_meta(&self, path: &str) -> Result<FileMeta, IpcError> {
        let parsed = VaultPath::parse(path)?;
        let path = parsed.to_string();
        let worktree = is_worktree(&path);
        let file_tags = if worktree || !is_markdown(&path) {
            if is_markdown(&path) {
                self.shared
                    .scope
                    .resolve(&path)
                    .ok()
                    .and_then(|(_, abs)| read_doc_at(&abs, None))
                    .map(|d| d.tags)
                    .unwrap_or_default()
            } else {
                Vec::new()
            }
        } else {
            self.shared
                .docs
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&path)
                .map(|d| d.tags.clone())
                .unwrap_or_default()
        };
        let inner = self.shared.lock();
        let entry = if worktree {
            None
        } else {
            inner.entries.get(&path)
        };
        let app = entry.map(|e| e.tags.clone()).unwrap_or_default();
        Ok(FileMeta {
            tags: catalog::combine(&file_tags, &app)
                .into_iter()
                .map(|t| TagRefView {
                    name: t.name,
                    file: t.file,
                    app: t.app,
                })
                .collect(),
            description: entry.map(|e| e.description.clone()).unwrap_or_default(),
            editable: !worktree && inner.problem.is_none(),
            path,
        })
    }

    /// Removes the entries of files that are gone; how many went.
    ///
    /// # Errors
    /// `meta.toml` cannot be read.
    pub fn clean_up(&self) -> Result<usize, IpcError> {
        let orphans = self.orphans();
        let count = orphans.len();
        for path in orphans {
            self.edit(None, Op::Remove { path })?;
        }
        Ok(count)
    }

    /// The answers search and the agent ask for.
    #[must_use]
    pub fn lookup(&self) -> Arc<dyn TagLookup> {
        Arc::new(Lookup(Arc::clone(&self.shared)))
    }

    /// Waits until the first look at the documents is done (tests).
    #[must_use]
    pub fn wait_catalog(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            if self.shared.catalog_ready.load(Ordering::Acquire) {
                return true;
            }
            if start.elapsed() > timeout {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Writes what is pending now (tests, stop).
    pub fn flush_now(&self) {
        let _ = self.shared.flush();
    }
}

impl Drop for TagsService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.meta_tx.send(MetaMsg::Stop);
        let _ = self.catalog_tx.send(CatalogMsg::Stop);
        let threads =
            std::mem::take(&mut *self.threads.lock().unwrap_or_else(PoisonError::into_inner));
        for thread in threads {
            let _ = thread.join();
        }
    }
}

struct Lookup(Arc<Shared>);

impl TagLookup for Lookup {
    fn files_with_all(&self, tags: &[String]) -> std::collections::HashSet<String> {
        let inner = self.0.lock();
        let docs = self.0.docs.read().unwrap_or_else(PoisonError::into_inner);
        catalog::files_with_set(&inner.entries, &docs, tags)
    }

    fn tags_of(&self, path: &str) -> Vec<String> {
        let inner = self.0.lock();
        let docs = self.0.docs.read().unwrap_or_else(PoisonError::into_inner);
        catalog::tags_of(&inner.entries, &docs, path)
    }
}

/// Tags typed by the person, normalized (shared with the RPC layer).
#[must_use]
pub fn clean_tags(raw: &[String]) -> Vec<String> {
    normalize_tags(raw)
}
