//! The tags service of one project: the app's entries in memory, written to `meta.toml` a moment after the last
//! change (surgically, see [`crate::meta`]); the front matter tags the index pass found (core harvest, `projects.md`
//! §7.3: the documents are never read again here); the answers the UI and search ask for.
//!
//! `meta.toml` lives where each owner's state store says (`projects.md` §9.2): the opened folder's, and one per
//! included project (the file it uses opened alone). Each is `.permark/meta.toml` when that folder has `.permark/`
//! (watched: an edit by someone else is read in, the changes made here and not written yet laid on top), else in the
//! profile. One root-relative view over all of them; each change goes to the file of the path's owner. A file that
//! cannot be read is never written over: its tags are read-only until it is fixed, changes made before it broke are
//! parked and written once it reads again.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, UNIX_EPOCH};

use permark_core::document::DocScope;
use permark_core::error::{ErrorKind, IpcError};
use permark_core::fsx;
use permark_core::organizing::{TagCount, TagLookup, TagsSummary};
use permark_core::paths::{key_of, VaultPath, WORKTREE_PREFIX};
use permark_core::projects::{
    OwnerSlot, Owners, OwnersChange, ProjectUid, Projects, ProjectsSubscription, VocabKind,
};
use permark_core::search::harvest::{Harvest, HarvestSubscription};
use permark_core::state::{RawFile, StateLocation};
use permark_core::tree::{category_of, Category};
use permark_core::watch::{ChangeKind, FsBatch, OwnWrites};

use crate::catalog::{self, Docs};
use crate::frontmatter::{from_text, HEAD_BYTES};
use crate::meta::{edit_text, Entries, Op, Problem};
use crate::normalize::normalize_tags;
use crate::types::{
    FileMeta, IndexEntry, MetaProblem, TagCountView, TagRefView, TagStyleView, TaggedFile,
    TagsIndex, TagsLocation, TagsOverview, TagsStatus,
};
use crate::{META_FILE, META_PATH};

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
    /// Where the entries are kept: the opened folder + each included project (`projects.md` §9.2).
    pub owners: Owners,
    /// Merge (then delete) the profile copies from before a folder had `.permark/` (not for a reader with no window).
    pub legacy: bool,
    pub own: Arc<OwnWrites>,
    pub scope: DocScope,
    /// Front matter tags from the index pass.
    pub harvest: Arc<Harvest>,
    /// Official vocabularies + owner chains (status marks); `None` without projects.
    pub projects: Option<Arc<Projects>>,
    /// Called when something the UI shows changed.
    pub announce: Arc<dyn Fn() + Send + Sync>,
    pub timing: Timing,
}

struct Inner {
    /// Owner-relative paths.
    entries: Entries,
    /// Changes made here and not written yet, oldest first (owner-relative).
    pending: Vec<Op>,
    problem: Option<Problem>,
    /// The last read or write failed (it is tried again).
    io_error: Option<String>,
    /// Hash of the bytes last read or written (`None`: no file).
    disk_hash: Option<u64>,
    loaded: bool,
    /// The legacy file is merged into the pending changes and is deleted after the next successful write.
    legacy_merged: bool,
    /// When the writer looks at this file next.
    deadline: Option<Instant>,
    /// The first change of the current run (writes never wait longer than `max_wait` after it).
    first: Option<Instant>,
    failures: usize,
}

/// One owner's `meta.toml`.
struct Unit {
    uid: Option<ProjectUid>,
    file: RawFile,
    legacy: Option<RawFile>,
    /// Project path of `file` when the watch reports it (`<owner>/.permark/meta.toml`).
    watched: Option<String>,
    inner: Mutex<Inner>,
    /// One write at a time (the writer thread and a pause may both flush).
    writing: Mutex<()>,
}

impl Unit {
    fn new(slot: &OwnerSlot, legacy: bool) -> Self {
        let in_project = slot.store.location() == StateLocation::Project;
        let watched = in_project.then(|| {
            if slot.path.is_empty() {
                META_PATH.to_owned()
            } else {
                format!("{}/{META_PATH}", slot.path)
            }
        });
        Self {
            uid: slot.uid.clone(),
            file: slot.store.shared_file(META_FILE),
            legacy: if legacy {
                slot.store.legacy_shared_file(META_FILE)
            } else {
                None
            },
            watched,
            inner: Mutex::new(Inner {
                entries: Entries::default(),
                pending: Vec::new(),
                problem: None,
                io_error: None,
                disk_hash: None,
                loaded: false,
                legacy_merged: false,
                deadline: None,
                first: None,
                failures: 0,
            }),
            writing: Mutex::new(()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The owners' files now.
struct Units {
    owners: Owners,
    list: Vec<Arc<Unit>>,
    /// The owners are changing: the person's edits are refused for that moment, moves wait in `held`.
    paused: bool,
    held: Vec<Op>,
}

struct Shared {
    units: RwLock<Units>,
    location: TagsLocation,
    legacy: bool,
    own: Arc<OwnWrites>,
    scope: DocScope,
    harvest: Arc<Harvest>,
    projects: Option<Arc<Projects>>,
    announce: Arc<dyn Fn() + Send + Sync>,
    timing: Timing,
    rev: AtomicU64,
    /// The merged root-relative entries as of `rev`.
    merged: Mutex<Option<(u64, Arc<Entries>)>>,
}

enum MetaMsg {
    /// Something was edited: write after the quiet period.
    Edited,
    /// This file may have changed.
    Reload(Arc<Unit>),
    Stop,
}

enum Flush {
    /// Nothing to write, or written.
    Done,
    /// The file cannot be read: wait for it to change.
    Parked,
    /// Writing failed: try again later.
    Failed,
    /// Another program wrote the file while this write was being made: nothing written; read it again first.
    Raced,
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

/// Rebases the paths of an op into an owner's file.
fn rebased(op: &Op, owners: &Owners, i: usize) -> Op {
    let rel = |p: &str| owners.rel(i, p);
    match op {
        Op::SetTags { path, tags } => Op::SetTags {
            path: rel(path),
            tags: tags.clone(),
        },
        Op::SetDescription { path, description } => Op::SetDescription {
            path: rel(path),
            description: description.clone(),
        },
        Op::Remove { path } => Op::Remove { path: rel(path) },
        Op::Rename { from, to } => Op::Rename {
            from: rel(from),
            to: rel(to),
        },
        Op::RenameTree { from, to } => Op::RenameTree {
            from: rel(from),
            to: rel(to),
        },
        Op::SetStyle { .. } => op.clone(),
    }
}

/// Whether `key` is `base` or inside it.
fn within(key: &str, base: &str) -> bool {
    key == base
        || key
            .strip_prefix(base)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl Shared {
    fn units(&self) -> std::sync::RwLockReadGuard<'_, Units> {
        self.units.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn rev(&self) -> u64 {
        self.rev.load(Ordering::Relaxed)
    }

    /// Every owner's entries in one root-relative store (dormant entries left out; the opened folder's looks first).
    fn merged(&self) -> Arc<Entries> {
        let rev = self.rev();
        let mut cache = self.merged.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, entries)) = cache.as_ref() {
            if *at == rev {
                return Arc::clone(entries);
            }
        }
        let units = self.units();
        let entries = if units.list.len() == 1 {
            units.list[0].lock().entries.clone()
        } else {
            let mut out = Entries::default();
            for (i, unit) in units.list.iter().enumerate() {
                let inner = unit.lock();
                for (path, entry) in inner.entries.iter() {
                    let at = units.owners.join(i, path);
                    if units.owners.owner_of(&at) == i {
                        out.insert(&at, entry.clone());
                    }
                }
                for (tag, style) in inner.entries.styles() {
                    out.insert_style(tag, style.clone());
                }
            }
            out
        };
        let entries = Arc::new(entries);
        *cache = Some((rev, Arc::clone(&entries)));
        entries
    }

    /// Takes the file on disk as the truth, then lays what was changed here and not written yet on top of it.
    fn reload(&self, unit: &Unit) {
        let mut read = unit.file.read();
        // many programs write by truncating first: an empty file is believed only after a moment
        for pause in [100, 300] {
            let looks_cut = matches!(&read, Ok(None))
                || matches!(&read, Ok(Some(b)) if b.iter().all(u8::is_ascii_whitespace));
            if !looks_cut || unit.lock().entries.is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(pause));
            read = unit.file.read();
        }
        let bytes = match read {
            Ok(bytes) => bytes,
            Err(error) => {
                unit.lock().io_error = Some(error.to_string());
                bump(self);
                return;
            }
        };
        let hash = bytes.as_deref().map(|b| fsx::hash(b).0);
        {
            let inner = unit.lock();
            if inner.loaded && inner.disk_hash == hash {
                return;
            }
        }
        let parsed = match &bytes {
            None => Ok(Entries::default()),
            Some(bytes) => text_of(bytes).and_then(|(_, text)| Entries::parse(text)),
        };
        let mut inner = unit.lock();
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

    /// Writes the pending changes of one file into it as it is now.
    fn flush(&self, unit: &Unit) -> Flush {
        let _writing = unit.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let ops = {
            let inner = unit.lock();
            if inner.pending.is_empty() && !inner.legacy_merged {
                return Flush::Done;
            }
            if inner.problem.is_some() {
                return Flush::Parked;
            }
            inner.pending.clone()
        };
        let fail = |message: String| {
            unit.lock().io_error = Some(message);
            bump(self);
            Flush::Failed
        };
        let bytes = match unit.file.read() {
            Ok(bytes) => bytes,
            Err(error) => return fail(error.to_string()),
        };
        let (bom, text) = match bytes.as_deref().map(text_of) {
            None => (false, ""),
            Some(Ok(parts)) => parts,
            Some(Err(problem)) => {
                unit.lock().problem = Some(problem);
                bump(self);
                return Flush::Parked;
            }
        };
        let new_text = match edit_text(text, &ops) {
            Ok(text) => text,
            Err(problem) => {
                unit.lock().problem = Some(problem);
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
            if let Some(path) = &unit.watched {
                self.own.expect_write(path, hash);
            }
            let written = unit.file.write_over(bytes.as_deref(), &out);
            if !matches!(written, Ok(true)) {
                if let Some(path) = &unit.watched {
                    self.own.retract_write(path, hash);
                }
            }
            match written {
                Ok(true) => {}
                Ok(false) => return Flush::Raced,
                Err(error) => return fail(error.to_string()),
            }
        }
        let entries = Entries::parse(&new_text).unwrap_or_default();
        let legacy_done = {
            let mut inner = unit.lock();
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
            if let Some(legacy) = &unit.legacy {
                // nothing is lost: it is all in `meta.toml` now
                let _ = legacy.remove();
            }
        }
        bump(self);
        Flush::Done
    }

    /// Writes one file now: an outside write in between is read first, then the rest goes on top (stop, pause).
    fn flush_settled(&self, unit: &Unit) {
        if matches!(self.flush(unit), Flush::Raced) {
            self.reload(unit);
            let _ = self.flush(unit);
        }
    }

    /// Merges the profile's copy (from before the folder had `.permark/`) into the entries as pending changes.
    fn merge_legacy(unit: &Unit) {
        let Some(legacy) = &unit.legacy else { return };
        let Ok(Some(bytes)) = legacy.read() else {
            return;
        };
        let Ok(old) = text_of(&bytes).and_then(|(_, text)| Entries::parse(text)) else {
            // unreadable: left where it is, tried again at the next start
            return;
        };
        let mut inner = unit.lock();
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

    /// Reads a unit's file (and merges its legacy copy) the first time.
    fn open_unit(&self, unit: &Unit) {
        self.reload(unit);
        // a legacy copy is merged only into a file that reads (the merge is written over it)
        if unit.lock().problem.is_none() {
            Self::merge_legacy(unit);
        }
        let mut inner = unit.lock();
        if !inner.pending.is_empty() || inner.legacy_merged {
            inner.deadline = Some(Instant::now());
        }
    }

    /// Splits a root-relative op into owner ops (`(unit, op)`); a move across owners becomes a removal in one file
    /// and the entry written in the other. `merged` = the root-relative entries (taken before any lock).
    fn route(owners: &Owners, merged: &Entries, op: &Op) -> Vec<(usize, Op)> {
        if owners.is_single() {
            return vec![(0, op.clone())];
        }
        let one = |path: &str| {
            let i = owners.owner_of(path);
            (i, rebased(op, owners, i))
        };
        match op {
            Op::SetTags { path, .. } | Op::SetDescription { path, .. } | Op::Remove { path } => {
                vec![one(path)]
            }
            // looks set here belong to the opened folder's file
            Op::SetStyle { .. } => vec![(0, op.clone())],
            Op::Rename { from, to } => {
                let (a, b) = (owners.owner_of(from), owners.owner_of(to));
                if a == b {
                    return vec![one(from)];
                }
                Self::moved_entry(owners, merged, from, to)
            }
            Op::RenameTree { from, to } => {
                let (a, b) = (owners.owner_of(from), owners.owner_of(to));
                let from_key = key_of(from);
                // an owner below the moved folder: its own file is not touched (its paths are its own)
                let crosses = owners
                    .slots()
                    .iter()
                    .skip(1)
                    .any(|s| within(s.key(), &from_key));
                if a == b && !crosses {
                    return vec![one(from)];
                }
                let moving: Vec<String> = merged
                    .iter()
                    .map(|(p, _)| p.to_owned())
                    .filter(|p| within(&key_of(p), &from_key) && owners.owner_of(p) == a)
                    .collect();
                let mut out = Vec::new();
                for path in moving {
                    let rest = path.get(from.len()..).unwrap_or_default();
                    out.extend(Self::moved_entry(
                        owners,
                        merged,
                        &path,
                        &format!("{to}{rest}"),
                    ));
                }
                out
            }
        }
    }

    /// One entry moved across owners: written in the target's file, removed from the source's.
    fn moved_entry(owners: &Owners, merged: &Entries, from: &str, to: &str) -> Vec<(usize, Op)> {
        let Some(entry) = merged.get(from).cloned() else {
            return Vec::new();
        };
        let (a, b) = (owners.owner_of(from), owners.owner_of(to));
        let target = owners.rel(b, to);
        let mut out = vec![(
            a,
            Op::Remove {
                path: owners.rel(a, from),
            },
        )];
        if !entry.tags.is_empty() {
            out.push((
                b,
                Op::SetTags {
                    path: target.clone(),
                    tags: entry.tags,
                },
            ));
        }
        if !entry.description.is_empty() {
            out.push((
                b,
                Op::SetDescription {
                    path: target,
                    description: entry.description,
                },
            ));
        }
        out
    }

    /// The status of a tag for a file (`projects.md` §7.2); `None`: no projects, or marks dormant.
    fn status_for(
        &self,
        tag: &str,
        path: Option<&str>,
    ) -> (
        Option<permark_core::projects::TagStatus>,
        Option<ProjectUid>,
    ) {
        let Some(projects) = &self.projects else {
            return (None, None);
        };
        let vocab = projects.vocabularies();
        let found = match path {
            Some(path) => {
                let chain = projects.snapshot().owner_of(path).chain;
                vocab.status(VocabKind::Tag, tag, &chain)
            }
            None => vocab.status_anywhere(VocabKind::Tag, tag),
        };
        found.map_or((None, None), |(status, owner)| (Some(status), owner))
    }
}

/// The writer: waits for changes, writes each file after its quiet period, retries failures, reads in outside edits.
fn meta_loop(shared: &Shared, rx: &Receiver<MetaMsg>) {
    let timing = shared.timing;
    loop {
        let next = shared
            .units()
            .list
            .iter()
            .filter_map(|u| u.lock().deadline)
            .min();
        let message = match next {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match message {
            Ok(MetaMsg::Edited) => {
                let now = Instant::now();
                for unit in &shared.units().list {
                    let mut inner = unit.lock();
                    if inner.pending.is_empty() || inner.problem.is_some() {
                        continue;
                    }
                    let start = *inner.first.get_or_insert(now);
                    let due = (now + timing.quiet).min(start + timing.max_wait);
                    inner.deadline = Some(inner.deadline.map_or(due, |d| d.min(due)));
                }
            }
            Ok(MetaMsg::Reload(unit)) => {
                shared.reload(&unit);
                let mut inner = unit.lock();
                if !inner.pending.is_empty() && inner.problem.is_none() {
                    // written once the file is quiet: another program may be halfway through writing it
                    inner.deadline = Some(Instant::now() + timing.quiet);
                }
            }
            Ok(MetaMsg::Stop) | Err(RecvTimeoutError::Disconnected) => {
                // what was changed is written before the service goes
                let list = shared.units().list.clone();
                for unit in &list {
                    shared.flush_settled(unit);
                }
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                let due: Vec<Arc<Unit>> = shared
                    .units()
                    .list
                    .iter()
                    .filter(|u| u.lock().deadline.is_some_and(|d| d <= now))
                    .cloned()
                    .collect();
                for unit in due {
                    let result = shared.flush(&unit);
                    if matches!(result, Flush::Raced) {
                        // the other program's version is read first, then what is waiting goes on top of it
                        shared.reload(&unit);
                    }
                    let mut inner = unit.lock();
                    match result {
                        Flush::Done => {
                            inner.failures = 0;
                            inner.first = None;
                            inner.deadline = None;
                        }
                        Flush::Parked => {
                            inner.first = None;
                            inner.deadline = None;
                        }
                        Flush::Raced => {
                            inner.first = None;
                            let waiting = !inner.pending.is_empty() && inner.problem.is_none();
                            inner.deadline = waiting.then(|| Instant::now() + timing.quiet);
                        }
                        Flush::Failed => {
                            let pause = timing.retry[inner.failures.min(timing.retry.len() - 1)];
                            inner.failures += 1;
                            inner.deadline = Some(Instant::now() + pause);
                        }
                    }
                }
            }
        }
    }
}

// ---- front matter tags of files the index does not cover (linked worktrees, files on demand) -------------------

fn read_doc_at(abs: &Path) -> Option<Vec<String>> {
    let meta = std::fs::metadata(abs).ok()?;
    if !meta.is_file() {
        return None;
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
    Some(from_text(&text))
}

// ---- the service -------------------------------------------------------------------------------------------------

pub struct TagsService {
    shared: Arc<Shared>,
    meta_tx: Sender<MetaMsg>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    harvest_sub: Mutex<Option<HarvestSubscription>>,
    map_sub: Mutex<Option<ProjectsSubscription>>,
}

fn denied(path: &str, why: &str) -> IpcError {
    IpcError::new(ErrorKind::Denied).subject(path).detail(why)
}

impl TagsService {
    /// Reads every owner's `meta.toml` (and merges the legacy copies), then starts the writer; the catalog follows the
    /// index pass.
    #[must_use]
    pub fn start(args: ServiceArgs) -> Self {
        let location = if args.owners.root().store.location() == StateLocation::Project {
            TagsLocation::Project
        } else {
            TagsLocation::Profile
        };
        let list: Vec<Arc<Unit>> = args
            .owners
            .slots()
            .iter()
            .map(|s| Arc::new(Unit::new(s, args.legacy)))
            .collect();
        let shared = Arc::new(Shared {
            units: RwLock::new(Units {
                owners: args.owners,
                list: list.clone(),
                paused: false,
                held: Vec::new(),
            }),
            location,
            legacy: args.legacy,
            own: args.own,
            scope: args.scope,
            harvest: args.harvest,
            projects: args.projects,
            announce: args.announce,
            timing: args.timing,
            rev: AtomicU64::new(1),
            merged: Mutex::new(None),
        });
        for unit in &list {
            shared.open_unit(unit);
        }
        let (meta_tx, meta_rx) = channel();
        let mut threads = Vec::new();
        let writer = Arc::clone(&shared);
        threads.extend(
            thread::Builder::new()
                .name("permark-tags-meta".to_owned())
                .spawn(move || meta_loop(&writer, &meta_rx))
                .ok(),
        );
        let harvest_sub = shared.harvest.subscribe({
            let weak = Arc::downgrade(&shared);
            move || {
                if let Some(shared) = weak.upgrade() {
                    bump(&shared);
                }
            }
        });
        // official lists | owner chains changed: the status marks change
        let map_sub = shared.projects.as_ref().map(|p| {
            let weak = Arc::downgrade(&shared);
            p.subscribe_map(move |_| {
                if let Some(shared) = weak.upgrade() {
                    bump(&shared);
                }
            })
        });
        Self {
            shared,
            meta_tx,
            threads: Mutex::new(threads),
            harvest_sub: Mutex::new(Some(harvest_sub)),
            map_sub: Mutex::new(map_sub),
        }
    }

    /// The owners change: before, every file is written and changes wait; after, the files of the new owners are read
    /// (a change waiting in a file that could not be written goes on in the same owner's new file).
    pub fn follow_owners(&self, change: &OwnersChange<'_>) {
        match change {
            OwnersChange::Pause => {
                let list = self.shared.units().list.clone();
                for unit in &list {
                    self.shared.flush_settled(unit);
                }
                self.shared
                    .units
                    .write()
                    .unwrap_or_else(PoisonError::into_inner)
                    .paused = true;
            }
            OwnersChange::Resume(owners) => {
                let old = self.shared.units().list.clone();
                let list: Vec<Arc<Unit>> = owners
                    .slots()
                    .iter()
                    .map(|s| Arc::new(Unit::new(s, self.shared.legacy)))
                    .collect();
                for unit in &list {
                    // changes that could not be written before stay with their owner
                    if let Some(before) = old.iter().find(|u| u.uid == unit.uid) {
                        let pending = std::mem::take(&mut before.lock().pending);
                        unit.lock().pending = pending;
                    }
                    self.shared.open_unit(unit);
                }
                let held = {
                    let mut units = self
                        .shared
                        .units
                        .write()
                        .unwrap_or_else(PoisonError::into_inner);
                    units.owners = (*owners).clone();
                    units.list = list;
                    units.paused = false;
                    std::mem::take(&mut units.held)
                };
                bump(&self.shared);
                if !held.is_empty() {
                    self.queue(held);
                }
                let _ = self.meta_tx.send(MetaMsg::Edited);
            }
        }
    }

    /// The project's watch reported a batch (called on the watch thread: quick).
    pub fn feed(&self, batch: &FsBatch) {
        if batch.is_empty() {
            return;
        }
        // moved files take their entries along
        let mut ops = Vec::new();
        let mut reload: Vec<Arc<Unit>> = Vec::new();
        {
            let units = self.shared.units();
            let owner_folders: Vec<&str> = units
                .owners
                .slots()
                .iter()
                .skip(1)
                .map(OwnerSlot::key)
                .collect();
            for change in &batch.changes {
                for unit in &units.list {
                    if let Some(watched) = &unit.watched {
                        if !change.own
                            && key_of(&change.path) == key_of(watched)
                            && !reload.iter().any(|u| Arc::ptr_eq(u, unit))
                        {
                            reload.push(Arc::clone(unit));
                        }
                    }
                }
                if let ChangeKind::Renamed { from } = &change.kind {
                    if is_worktree(from) || is_worktree(&change.path) {
                        continue;
                    }
                    // an included project moved as a whole: its own file moves with it (or its profile folder does)
                    if owner_folders.contains(&key_of(from).as_str()) {
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
        }
        if !ops.is_empty() {
            self.queue(ops);
        }
        for unit in reload {
            let _ = self.meta_tx.send(MetaMsg::Reload(unit));
        }
    }

    /// Changes that follow from the project (moves): kept even while a file cannot be read; held while the owners
    /// change.
    fn queue(&self, ops: Vec<Op>) {
        let merged = self.shared.merged();
        let touching: Vec<Op> = ops
            .into_iter()
            .filter(|op| match op {
                Op::Rename { from, .. } => merged.get(from).is_some(),
                Op::RenameTree { from, .. } => {
                    let prefix = format!("{}/", key_of(from));
                    merged.iter().any(|(p, _)| key_of(p).starts_with(&prefix))
                }
                _ => true,
            })
            .collect();
        if touching.is_empty() {
            return;
        }
        {
            let mut units = self
                .shared
                .units
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            if units.paused {
                units.held.extend(touching);
                return;
            }
            let routed: Vec<(usize, Op)> = touching
                .iter()
                .flat_map(|op| Shared::route(&units.owners, &merged, op))
                .collect();
            for (i, op) in routed {
                let Some(unit) = units.list.get(i) else {
                    continue;
                };
                let mut inner = unit.lock();
                inner.entries.apply(&op);
                inner.pending.push(op);
            }
            drop(units);
        }
        bump(&self.shared);
        let _ = self.meta_tx.send(MetaMsg::Edited);
    }

    /// A change the person made (root-relative).
    fn edit(&self, path: Option<&str>, op: &Op) -> Result<(), IpcError> {
        if let Some(path) = path {
            if is_worktree(path) {
                return Err(denied(path, "worktree"));
            }
        }
        let merged = self.shared.merged();
        {
            let units = self.shared.units();
            if units.paused {
                return Err(IpcError::new(ErrorKind::Unavailable).subject("meta.toml"));
            }
            let routed = Shared::route(&units.owners, &merged, op);
            // refused as a whole when a file it touches cannot be read
            for (i, _) in &routed {
                if let Some(problem) = units.list.get(*i).and_then(|u| u.lock().problem.clone()) {
                    return Err(IpcError::new(ErrorKind::Unavailable)
                        .subject("meta.toml")
                        .detail(match problem.line {
                            Some(line) => format!("line {line}: {}", problem.message),
                            None => problem.message.clone(),
                        }));
                }
            }
            for (i, op) in routed {
                if let Some(unit) = units.list.get(i) {
                    let mut inner = unit.lock();
                    inner.entries.apply(&op);
                    inner.pending.push(op);
                }
            }
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
    /// Not a file of the project, a worktree file, or its `meta.toml` cannot be read.
    pub fn set_tags(&self, path: &str, tags: &[String]) -> Result<FileMeta, IpcError> {
        let path = self.existing_file(path)?;
        self.edit(Some(&path), &Op::set_tags(&path, tags))?;
        self.file_meta(&path)
    }

    /// Replaces the description of a file.
    ///
    /// # Errors
    /// As [`TagsService::set_tags`].
    pub fn set_description(&self, path: &str, description: &str) -> Result<FileMeta, IpcError> {
        let path = self.existing_file(path)?;
        self.edit(Some(&path), &Op::set_description(&path, description))?;
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
        self.edit(None, &op)
    }

    /// Where the tags live, and the first problem of any owner's file (the opened folder's first).
    #[must_use]
    pub fn status(&self) -> TagsStatus {
        let units = self.shared.units();
        let mut problem = None;
        let mut write_error = None;
        let mut parked = 0usize;
        for unit in &units.list {
            let inner = unit.lock();
            if problem.is_none() {
                problem = inner.problem.as_ref().map(|p| MetaProblem {
                    message: p.message.clone(),
                    line: p.line.and_then(|l| u32::try_from(l).ok()),
                });
            }
            if write_error.is_none() {
                write_error.clone_from(&inner.io_error);
            }
            parked += inner.pending.len();
        }
        TagsStatus {
            location: self.shared.location,
            problem,
            write_error,
            parked: u32::try_from(parked).unwrap_or(u32::MAX),
        }
    }

    fn rev(&self) -> u64 {
        self.shared.rev()
    }

    fn docs(&self) -> Arc<Docs> {
        self.shared.harvest.snapshot()
    }

    /// Every file that has a tag or a description, and the looks of the tags (an official look wins).
    #[must_use]
    pub fn index(&self) -> TagsIndex {
        let rev = self.rev();
        let status = self.status();
        let entries = self.shared.merged();
        let docs = self.docs();
        let rows = catalog::index(&entries, &docs)
            .into_iter()
            .map(|row| IndexEntry {
                path: row.path,
                tags: row.tags,
                description: row.description,
            })
            .collect();
        let mut styles = entries
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
        if let Some(projects) = &self.shared.projects {
            let vocab = projects.vocabularies();
            if let Some(official) = vocab.official.vocab() {
                for (tag, style) in &official.tags {
                    if style.color.is_some() || style.icon.is_some() {
                        styles.insert(
                            tag.clone(),
                            TagStyleView {
                                color: style.color.clone(),
                                icon: style.icon.clone(),
                            },
                        );
                    }
                }
            }
        }
        TagsIndex {
            rev,
            entries: rows,
            styles,
            status,
        }
    }

    /// Entries whose file is gone (an entry that is no valid project path is left alone).
    #[must_use]
    pub fn orphans(&self) -> Vec<String> {
        self.shared
            .merged()
            .iter()
            .map(|(p, _)| p.to_owned())
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

    /// Tags with counts (+ official | local | unlisted while official lists exist), untagged documents, orphans.
    #[must_use]
    pub fn overview(&self) -> TagsOverview {
        let rev = self.rev();
        let orphans = self.orphans().len();
        let entries = self.shared.merged();
        let docs = self.docs();
        let marks = self
            .shared
            .projects
            .as_ref()
            .is_some_and(|p| p.vocabularies().active());
        TagsOverview {
            rev,
            tags: catalog::tag_counts(&entries, &docs)
                .into_iter()
                .map(|t| {
                    let (status, owner) = if marks {
                        self.shared.status_for(&t.name, None)
                    } else {
                        (None, None)
                    };
                    TagCountView {
                        name: t.name,
                        count: u32::try_from(t.count).unwrap_or(u32::MAX),
                        only_in_files: t.only_in_files,
                        status,
                        owner,
                    }
                })
                .collect(),
            untagged: u32::try_from(catalog::untagged(&entries, &docs).len()).unwrap_or(u32::MAX),
            orphans: u32::try_from(orphans).unwrap_or(u32::MAX),
            marks,
        }
    }

    /// The files that have every one of `tags`, or (`untagged`) the documents that have none.
    #[must_use]
    pub fn files_with(&self, tags: &[String], untagged: bool) -> Vec<TaggedFile> {
        let entries = self.shared.merged();
        let docs = self.docs();
        let mut paths = if untagged {
            catalog::untagged(&entries, &docs)
        } else {
            catalog::files_with(&entries, &docs, tags)
        };
        paths.truncate(MAX_LISTED);
        paths
            .into_iter()
            .map(|path| TaggedFile {
                description: entries
                    .get(&path)
                    .map(|e| e.description.clone())
                    .unwrap_or_default(),
                modified_ms: docs
                    .get(&path)
                    .map_or_else(|| self.modified_ms(&path), |d| d.mtime_ms),
                path,
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
        let file_tags = if !is_markdown(&path) {
            Vec::new()
        } else if worktree {
            self.shared
                .scope
                .resolve(&path)
                .ok()
                .and_then(|(_, abs)| read_doc_at(&abs))
                .unwrap_or_default()
        } else {
            self.shared.harvest.tags_of(&path).unwrap_or_default()
        };
        let entries = self.shared.merged();
        let entry = if worktree { None } else { entries.get(&path) };
        let app = entry.map(|e| e.tags.clone()).unwrap_or_default();
        let editable = !worktree && {
            let units = self.shared.units();
            let i = units.owners.owner_of(&path);
            !units.paused
                && units
                    .list
                    .get(i)
                    .is_some_and(|u| u.lock().problem.is_none())
        };
        let marks = self
            .shared
            .projects
            .as_ref()
            .is_some_and(|p| p.vocabularies().active());
        Ok(FileMeta {
            tags: catalog::combine(&file_tags, &app)
                .into_iter()
                .map(|t| {
                    let (status, owner) = if marks && !worktree {
                        self.shared.status_for(&t.name, Some(&path))
                    } else {
                        (None, None)
                    };
                    TagRefView {
                        name: t.name,
                        file: t.file,
                        app: t.app,
                        status,
                        owner,
                    }
                })
                .collect(),
            description: entry.map(|e| e.description.clone()).unwrap_or_default(),
            editable,
            path,
        })
    }

    /// Removes the entries of files that are gone; how many went.
    ///
    /// # Errors
    /// A `meta.toml` cannot be read.
    pub fn clean_up(&self) -> Result<usize, IpcError> {
        let orphans = self.orphans();
        let count = orphans.len();
        for path in orphans {
            self.edit(None, &Op::Remove { path })?;
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
        self.shared.harvest.wait_ready(timeout)
    }

    /// Writes what is pending now (tests, stop).
    pub fn flush_now(&self) {
        let list = self.shared.units().list.clone();
        for unit in &list {
            let _ = self.shared.flush(unit);
        }
    }
}

impl Drop for TagsService {
    fn drop(&mut self) {
        self.harvest_sub
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.map_sub
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let _ = self.meta_tx.send(MetaMsg::Stop);
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
        catalog::files_with_set(&self.0.merged(), &self.0.harvest.snapshot(), tags)
    }

    fn tags_of(&self, path: &str) -> Vec<String> {
        catalog::tags_of(&self.0.merged(), &self.0.harvest.snapshot(), path)
    }

    fn summary(&self) -> TagsSummary {
        let entries = self.0.merged();
        let docs = self.0.harvest.snapshot();
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        TagsSummary {
            tags: catalog::tag_counts(&entries, &docs)
                .into_iter()
                .map(|t| TagCount {
                    tag: t.name,
                    count: count(t.count),
                })
                .collect(),
            documents: count(docs.len()),
            untagged: count(catalog::untagged(&entries, &docs).len()),
        }
    }

    fn description_of(&self, path: &str) -> String {
        self.0
            .merged()
            .get(path)
            .map(|e| e.description.clone())
            .unwrap_or_default()
    }

    fn wait_ready(&self, timeout: Duration) -> bool {
        self.0.harvest.wait_ready(timeout)
    }
}

/// Tags typed by the person, normalized (shared with the RPC layer).
#[must_use]
pub fn clean_tags(raw: &[String]) -> Vec<String> {
    normalize_tags(raw)
}
