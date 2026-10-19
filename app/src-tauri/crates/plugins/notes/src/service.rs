//! The notes of one folder: listing them, creating, deleting (with undo), renaming, pinning, sharing, keeping their
//! earlier versions and the text a crash would lose. Plain Rust over the folder and the person's profile, tested without
//! a window.
//!
//! A note is a Markdown file directly in the folder; its identity is its file name. The service never asks for a name
//! and never asks "save changes?": the editor saves by itself (autosave, the editing machine) and the journal keeps
//! what is typed in between.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, UNIX_EPOCH};

use permark_core::error::{ErrorKind, IpcError};
use permark_core::fsx;
use permark_core::notes::{loose_key, parse_loose, NotesScope};
use permark_core::paths::{fold, Root};
use permark_core::plugins::PluginId;
use permark_core::state::{Loaded, StateFile, StateStore};

use crate::folder::{create as create_folder, info_of, FolderInfo, FolderStatus, Places, Resolved};
use crate::journal::{Entry as JournalEntry, Journal};
use crate::names::{
    copy_base, first_free, is_note_name, name_problem, note_stem, numbered, restored_stem, Moment,
    EXTENSION, MAX_TRIES,
};
use crate::state::{read_legacy, FolderState, Stamp};
use crate::text::{head_of, is_blank, snippet_of, title_of};
use crate::types::{Deleted, EntryKind, NoteEntry, NoteList, NotesSummary};
use crate::versions::{note_id, Cadence, Version, VersionStore};

pub(crate) const ID: PluginId = PluginId::of("notes");

/// Deleted batches kept for Undo, and the biggest note kept in one.
const UNDO_BATCHES: usize = 20;
const UNDO_NOTE_BYTES: u64 = 5 * 1024 * 1024;

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Milliseconds since the epoch on the system clock.
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A title and snippet as they were when the file had a given stamp (a list is made without reading every file).
#[derive(Debug, Clone)]
pub(crate) struct Cached {
    stamp: Stamp,
    title: String,
    snippet: String,
}

/// A note kept in memory while it is in the recycle bin.
#[derive(Debug)]
pub(crate) struct DeletedNote {
    pub name: String,
    pub bytes: Option<Vec<u8>>,
    pub modified_ms: u64,
    pub pinned: bool,
    pub shared: bool,
}

#[derive(Debug)]
pub(crate) struct DeletedBatch {
    pub id: u64,
    pub notes: Vec<DeletedNote>,
}

/// A note moved into a project, for Undo.
#[derive(Debug)]
pub(crate) struct MovedNote {
    pub undo_id: u64,
    pub destination: PathBuf,
    pub hash: String,
}

/// Everything that belongs to the folder that is open now. Replaced as a whole when the folder changes.
pub(crate) struct Ctx {
    pub resolved: Resolved,
    pub file: StateFile<FolderState>,
    pub state: Mutex<FolderState>,
    pub versions: VersionStore,
    pub journal: Journal,
    pub cadence: Mutex<Cadence>,
    pub titles: Mutex<HashMap<String, Cached>>,
    pub deleted: Mutex<VecDeque<DeletedBatch>>,
    pub moves: Mutex<Vec<MovedNote>>,
    /// Journal keys whose recovered text was already kept as a version.
    pub kept: Mutex<HashSet<String>>,
    pub next_undo: AtomicU64,
    pruned: Mutex<bool>,
}

impl Ctx {
    /// Changes the state and keeps it (best effort: it is all rebuilt from the files).
    pub(crate) fn update<R>(&self, change: impl FnOnce(&mut FolderState) -> R) -> R {
        let mut state = lock(&self.state);
        let before = state.clone();
        let result = change(&mut state);
        if *state != before {
            let _ = self.file.save(&state);
        }
        result
    }

    pub(crate) fn read<R>(&self, read: impl FnOnce(&FolderState) -> R) -> R {
        read(&lock(&self.state))
    }

    pub(crate) fn folder(&self) -> &Path {
        &self.resolved.path
    }

    pub(crate) fn note_path(&self, name: &str) -> PathBuf {
        self.folder().join(name)
    }
}

/// Where a document path points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Note(String),
    Loose { id: String },
}

pub struct NotesService {
    places: Places,
    plugin_dir: PathBuf,
    store: StateStore,
    pub(crate) scope: Arc<NotesScope>,
    chosen: Box<dyn Fn() -> String + Send + Sync>,
    ctx: RwLock<Arc<Ctx>>,
    /// When this run began: text journaled before it is text a crash left (the editors of this run keep theirs too).
    pub(crate) started_ms: u64,
}

impl NotesService {
    /// Opens the service for the folder the settings say (`chosen`: the setting now; empty = the default).
    #[must_use]
    pub fn open(
        places: Places,
        store: StateStore,
        scope: Arc<NotesScope>,
        chosen: Box<dyn Fn() -> String + Send + Sync>,
    ) -> Arc<Self> {
        let plugin_dir = store.plugin_dir(&ID);
        let ctx = Self::build(&places, &plugin_dir, &store, &chosen());
        let service = Arc::new(Self {
            places,
            plugin_dir,
            store,
            scope,
            chosen,
            ctx: RwLock::new(ctx),
            started_ms: now_ms(),
        });
        service.sync_scope(&service.ctx());
        service
    }

    fn build(places: &Places, plugin_dir: &Path, store: &StateStore, chosen: &str) -> Arc<Ctx> {
        let resolved = places.resolve(chosen);
        let id =
            fsx::hash(fold(&resolved.path.to_string_lossy().replace('\\', "/")).as_bytes()).hex();
        let file: StateFile<FolderState> = store.plugin_file(&ID, &format!("folder-{id}.json"));
        let mut state = match file.load() {
            Loaded::Found(state) => state,
            Loaded::Missing | Loaded::Recovered { .. } => FolderState::default(),
        };
        if !state.imported {
            // 0.5 kept pins, shared notes and loose files in the folder: carried over once, the old file untouched
            state = read_legacy(&resolved.path).unwrap_or(FolderState {
                imported: true,
                ..state
            });
            let _ = file.save(&state);
        }
        Arc::new(Ctx {
            versions: VersionStore::new(plugin_dir.join("versions").join(&id)),
            journal: Journal::new(plugin_dir.join("journal").join(&id)),
            resolved,
            file,
            state: Mutex::new(state),
            cadence: Mutex::new(Cadence::default()),
            titles: Mutex::new(HashMap::new()),
            deleted: Mutex::new(VecDeque::new()),
            moves: Mutex::new(Vec::new()),
            kept: Mutex::new(HashSet::new()),
            next_undo: AtomicU64::new(1),
            pruned: Mutex::new(false),
        })
    }

    pub(crate) fn ctx(&self) -> Arc<Ctx> {
        Arc::clone(&self.ctx.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// The setting changed: another folder is the notes folder from now on.
    pub fn settings_changed(&self) {
        let chosen = (self.chosen)();
        let wanted = self.places.resolve(&chosen);
        if wanted == self.ctx().resolved {
            return;
        }
        let ctx = Self::build(&self.places, &self.plugin_dir, &self.store, &chosen);
        *self.ctx.write().unwrap_or_else(PoisonError::into_inner) = Arc::clone(&ctx);
        self.sync_scope(&ctx);
    }

    /// The folder and what is known about it.
    #[must_use]
    pub fn folder(&self) -> FolderInfo {
        info_of(&self.ctx().resolved)
    }

    /// Core's scope follows the folder: set when the folder is there, cleared when it is not; the loose files too.
    pub(crate) fn sync_scope(&self, ctx: &Ctx) {
        let there = ctx.folder().is_dir();
        let current = self.scope.folder().map(|root| root.path().to_path_buf());
        if there {
            let same = current
                .as_deref()
                .is_some_and(|c| crate::folder::same_place(c, ctx.folder()));
            if !same {
                self.scope.set_folder(Root::open(ctx.folder()).ok());
            }
        } else if current.is_some() {
            self.scope.set_folder(None);
        }
        let loose = ctx.read(|state| state.loose.clone());
        for entry in loose {
            if self.scope.loose_file(&entry.id).is_none() {
                let _ = self.scope.set_loose(&entry.id, Path::new(&entry.path));
            }
        }
    }

    /// Makes the folder when the person asked ("Create it"); the default folder is made when first needed.
    ///
    /// # Errors
    /// The system refused.
    pub fn create_folder(&self) -> Result<FolderInfo, IpcError> {
        let ctx = self.ctx();
        create_folder(ctx.folder())
            .map_err(|e| IpcError::io(ctx.folder().display().to_string(), &e))?;
        self.sync_scope(&ctx);
        Ok(info_of(&ctx.resolved))
    }

    /// The folder to write in: the default one is made when it is not there, a chosen one that is not there is an error.
    pub(crate) fn ready_folder(&self, ctx: &Ctx) -> Result<PathBuf, IpcError> {
        let subject = ctx.folder().display().to_string();
        match info_of(&ctx.resolved).status {
            FolderStatus::Ready => {}
            FolderStatus::Missing => {
                return Err(IpcError::new(ErrorKind::NotFound).subject(subject))
            }
            FolderStatus::ReadOnly { reason } => {
                return Err(IpcError::new(ErrorKind::Denied)
                    .subject(subject)
                    .detail(reason))
            }
        }
        if !ctx.folder().is_dir() {
            create_folder(ctx.folder()).map_err(|e| IpcError::io(subject, &e))?;
            self.sync_scope(ctx);
        }
        Ok(ctx.folder().to_path_buf())
    }

    /// Takes a document path apart. Notes are the files directly inside the folder.
    pub(crate) fn target(path: &str) -> Result<Target, IpcError> {
        if let Some((id, _)) = parse_loose(path) {
            return Ok(Target::Loose { id: id.to_owned() });
        }
        if path.is_empty()
            || path.contains(['/', '\\', '\0'])
            || path == "."
            || path == ".."
            || !is_note_name(path)
        {
            return Err(IpcError::new(ErrorKind::NotFound).subject(path));
        }
        Ok(Target::Note(path.to_owned()))
    }

    /// The note a path means, which must be there.
    pub(crate) fn existing_note(ctx: &Ctx, path: &str) -> Result<(String, PathBuf), IpcError> {
        match Self::target(path)? {
            Target::Note(name) => {
                let abs = ctx.note_path(&name);
                if std::fs::metadata(&abs).is_ok_and(|m| m.is_file()) {
                    Ok((name, abs))
                } else {
                    Err(IpcError::new(ErrorKind::NotFound).subject(path))
                }
            }
            Target::Loose { .. } => Err(IpcError::new(ErrorKind::Unsupported).subject(path)),
        }
    }

    // ---- the list ----------------------------------------------------------------------------------------------

    /// The notes and the loose files, in the order the list shows them.
    #[must_use]
    pub fn list(&self, now_ms: u64) -> NoteList {
        let ctx = self.ctx();
        let info = info_of(&ctx.resolved);
        self.sync_scope(&ctx);
        let load_journal = || -> HashMap<String, JournalEntry> {
            ctx.journal
                .all()
                .into_iter()
                .map(|entry| (entry.key.clone(), entry))
                .collect()
        };
        let mut journal = load_journal();
        let (mut notes, mut names, listed) =
            Self::read_notes(&ctx, &info, &journal, self.started_ms);
        if listed {
            if self.recover_orphans(&ctx, &names, &journal, now_ms) {
                // text that was only in the journal is a note now
                journal = load_journal();
                (notes, names, _) = Self::read_notes(&ctx, &info, &journal, self.started_ms);
            }
            ctx.update(|state| {
                let loose_keys: HashSet<String> =
                    state.loose.iter().map(|e| loose_key(&e.id)).collect();
                state.retain_existing(|key| loose_keys.contains(key) || names.contains(key));
            });
            Self::prune_once(&ctx, &names, now_ms);
        }
        let mut files = self.loose_entries(&ctx, &journal);
        let (pins, open) = ctx.read(|state| (state.pins.clone(), state.open.clone()));
        let mut ordered: Vec<NoteEntry> = Vec::with_capacity(notes.len() + files.len());
        for key in &pins {
            if let Some(at) = notes
                .iter()
                .position(|n| &n.name == key && n.kind == EntryKind::Note)
            {
                ordered.push(notes.remove(at));
            } else if let Some(at) = files
                .iter()
                .position(|f| parse_loose(&f.path).is_some_and(|(id, _)| loose_key(id) == *key))
            {
                ordered.push(files.remove(at));
            }
        }
        notes.sort_by(|a, b| {
            b.modified_ms
                .cmp(&a.modified_ms)
                .then_with(|| a.name.cmp(&b.name))
        });
        ordered.extend(notes);
        ordered.extend(files);
        let open = open.and_then(|key| {
            ordered
                .iter()
                .find(|entry| match entry.kind {
                    EntryKind::Note => entry.name == key,
                    EntryKind::File => {
                        parse_loose(&entry.path).is_some_and(|(id, _)| loose_key(id) == key)
                    }
                })
                .map(|entry| entry.path.clone())
        });
        NoteList {
            folder: info,
            entries: ordered,
            open,
        }
    }

    /// The pinned notes and the latest ones, for the welcome screen.
    #[must_use]
    pub fn summary(&self, now_ms: u64) -> NotesSummary {
        let list = self.list(now_ms);
        let notes: Vec<&NoteEntry> = list
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Note)
            .collect();
        NotesSummary {
            pinned: list
                .entries
                .iter()
                .filter(|e| e.pinned)
                .take(5)
                .cloned()
                .collect(),
            latest: notes
                .iter()
                .filter(|e| !e.pinned)
                .take(5)
                .map(|e| (*e).clone())
                .collect(),
            count: u32::try_from(notes.len()).unwrap_or(u32::MAX),
        }
    }

    /// The notes of the folder (not ordered), their names, and whether the folder could be read.
    fn read_notes(
        ctx: &Ctx,
        info: &FolderInfo,
        journal: &HashMap<String, JournalEntry>,
        started_ms: u64,
    ) -> (Vec<NoteEntry>, HashSet<String>, bool) {
        let mut notes = Vec::new();
        let mut names = HashSet::new();
        if matches!(info.status, FolderStatus::Missing) {
            return (notes, names, false);
        }
        let Ok(dir) = std::fs::read_dir(ctx.folder()) else {
            return (notes, names, !ctx.folder().exists() && info.is_default);
        };
        let (pins, shared, copied) =
            ctx.read(|s| (s.pins.clone(), s.shared.clone(), s.copied.clone()));
        let mut titles = lock(&ctx.titles);
        for entry in dir.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_note_name(&name) {
                continue;
            }
            let path = entry.path();
            let meta = match entry.file_type() {
                Ok(kind) if kind.is_symlink() => std::fs::metadata(&path),
                Ok(_) => entry.metadata(),
                Err(e) => Err(e),
            };
            let Ok(meta) = meta else { continue };
            if !meta.is_file() {
                continue;
            }
            let stamp = Stamp::of_meta(&meta);
            let cached = match titles.get(&name) {
                Some(cached) if cached.stamp == stamp => cached.clone(),
                _ => {
                    let head = head_of(&path);
                    let fresh = Cached {
                        stamp,
                        title: title_of(&head),
                        snippet: snippet_of(&head),
                    };
                    titles.insert(name.clone(), fresh.clone());
                    fresh
                }
            };
            let recovered = journal.get(&name).is_some_and(|entry| {
                entry.saved_ms < started_ms && differs_from_file(entry, &path)
            });
            if recovered {
                Self::keep_recovered(ctx, &name, journal.get(&name));
            }
            names.insert(name.clone());
            notes.push(NoteEntry {
                path: name.clone(),
                kind: EntryKind::Note,
                pinned: pins.contains(&name),
                shared: shared.contains(&name),
                copied: copied.get(&name) == Some(&stamp),
                recovered,
                title: cached.title,
                snippet: cached.snippet,
                modified_ms: stamp.modified_ms,
                size: stamp.size,
                name,
                folder: None,
                state: None,
                pending: false,
            });
        }
        titles.retain(|name, _| names.contains(name));
        (notes, names, true)
    }

    /// The entry was kept by an earlier run: nobody is typing it now.
    pub(crate) fn left_by_a_crash(&self, entry: &JournalEntry) -> bool {
        entry.saved_ms < self.started_ms
    }

    /// Recovered text is also a version, so it stays whatever the person decides.
    fn keep_recovered(ctx: &Ctx, name: &str, entry: Option<&JournalEntry>) {
        let Some(entry) = entry else { return };
        if !lock(&ctx.kept).insert(entry.key.clone()) {
            return;
        }
        let _ = ctx
            .versions
            .snapshot(name, entry.text.as_bytes(), entry.saved_ms.max(1));
    }

    /// Text journaled for a note whose file is gone becomes a note of its own: nothing typed is lost.
    fn recover_orphans(
        &self,
        ctx: &Ctx,
        names: &HashSet<String>,
        journal: &HashMap<String, JournalEntry>,
        now_ms: u64,
    ) -> bool {
        let mut made = false;
        for (key, entry) in journal {
            if key.starts_with(permark_core::notes::LOOSE_PREFIX)
                || names.contains(key)
                || is_blank(&entry.text)
                || !is_note_name(key)
            {
                continue;
            }
            let stem = crate::names::copy_base(&title_of(&entry.text), key);
            let Ok(dir) = self.ready_folder(ctx) else {
                continue;
            };
            let name = first_free(&format!("{stem} (recovered)"), " ", |n| {
                dir.join(n).exists()
            });
            if write_new(&dir.join(&name), entry.text.as_bytes()).is_ok() {
                ctx.journal.clear(key);
                let _ = ctx
                    .versions
                    .snapshot(&name, entry.text.as_bytes(), now_ms.max(1));
                made = true;
            }
        }
        made
    }

    /// Once per folder: versions of notes that are gone for long are removed.
    fn prune_once(ctx: &Ctx, names: &HashSet<String>, now_ms: u64) {
        let mut pruned = lock(&ctx.pruned);
        if *pruned {
            return;
        }
        *pruned = true;
        let existing: BTreeSet<String> = names.iter().map(|n| note_id(n)).collect();
        ctx.versions.prune_orphans(&existing, now_ms);
    }

    // ---- making, deleting, renaming ------------------------------------------------------------------------------

    /// Makes a new note: an empty file named after `moment` (`2026-10-15 1432.md`, then `-2`, `-3`). It is a note like
    /// any other from this moment: it is in the list, it is there after a restart, and nothing removes it but the
    /// person. Returns its name (the document path).
    ///
    /// # Errors
    /// The folder is missing | read-only | the system refused.
    pub fn create(&self, moment: Moment) -> Result<String, IpcError> {
        let ctx = self.ctx();
        let dir = self.ready_folder(&ctx)?;
        let stem = note_stem(moment);
        for n in 1..=MAX_TRIES {
            let name = numbered(&stem, n, "-");
            match write_new(&dir.join(&name), b"") {
                Ok(()) => return Ok(name),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(IpcError::io(name, &e)),
            }
        }
        Err(IpcError::new(ErrorKind::Exists).subject(stem))
    }

    /// Sends notes to the recycle bin. Their text is kept in memory for [`NotesService::undo_delete`].
    ///
    /// # Errors
    /// One of them is not a note there | the bin refused (nothing is deleted then).
    pub fn delete(&self, paths: &[String]) -> Result<Deleted, IpcError> {
        let ctx = self.ctx();
        self.ready_folder(&ctx)?;
        let mut batch = Vec::new();
        let mut files = Vec::new();
        let mut keepable = true;
        for path in paths {
            let (name, abs) = Self::existing_note(&ctx, path)?;
            let meta = std::fs::metadata(&abs).map_err(|e| IpcError::io(&name, &e))?;
            let bytes = (meta.len() <= UNDO_NOTE_BYTES)
                .then(|| fsx::retry(&fsx::RETRY_DELAYS, || std::fs::read(&abs)).ok())
                .flatten();
            keepable &= bytes.is_some();
            let (pinned, shared) = ctx.read(|s| (s.is_pinned(&name), s.is_shared(&name)));
            batch.push(DeletedNote {
                modified_ms: Stamp::of_meta(&meta).modified_ms,
                bytes,
                pinned,
                shared,
                name,
            });
            files.push(abs);
        }
        fsx::recycle(&files).map_err(|reason| IpcError::new(ErrorKind::Trash).detail(reason))?;
        let names: Vec<String> = batch.iter().map(|n| n.name.clone()).collect();
        for name in &names {
            ctx.journal.clear(name);
        }
        ctx.update(|state| {
            for name in &names {
                state.pins.retain(|p| p != name);
                state.shared.retain(|s| s != name);
                state.copied.remove(name);
                if state.open.as_deref() == Some(name) {
                    state.open = None;
                }
            }
        });
        let undo_id = keepable.then(|| {
            let id = ctx.next_undo.fetch_add(1, Ordering::Relaxed);
            let mut deleted = lock(&ctx.deleted);
            deleted.push_back(DeletedBatch { id, notes: batch });
            while deleted.len() > UNDO_BATCHES {
                deleted.pop_front();
            }
            id
        });
        Ok(Deleted { undo_id, names })
    }

    /// Puts back the notes of a deletion, with the text and the modification time they had; a note whose name is taken
    /// again comes back as "<title> (restored)". Returns the names they have now.
    ///
    /// # Errors
    /// Unknown undo id | none could be written.
    pub fn undo_delete(&self, undo_id: u64) -> Result<Vec<String>, IpcError> {
        let ctx = self.ctx();
        let dir = self.ready_folder(&ctx)?;
        let batch = {
            let mut deleted = lock(&ctx.deleted);
            let at = deleted
                .iter()
                .position(|b| b.id == undo_id)
                .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject("undo"))?;
            deleted.remove(at)
        };
        let Some(batch) = batch else {
            return Err(IpcError::new(ErrorKind::NotFound).subject("undo"));
        };
        let mut back = Vec::new();
        let mut failed = Vec::new();
        let mut first_error = None;
        for note in batch.notes {
            match Self::put_back(&ctx, &dir, &note) {
                Ok(name) => back.push(name),
                Err(error) => {
                    first_error.get_or_insert(error);
                    failed.push(note);
                }
            }
        }
        if back.is_empty() {
            if let Some(error) = first_error {
                lock(&ctx.deleted).push_back(DeletedBatch {
                    id: undo_id,
                    notes: failed,
                });
                return Err(error);
            }
        }
        if !failed.is_empty() {
            lock(&ctx.deleted).push_back(DeletedBatch {
                id: undo_id,
                notes: failed,
            });
        }
        Ok(back)
    }

    fn put_back(ctx: &Ctx, dir: &Path, note: &DeletedNote) -> Result<String, IpcError> {
        let bytes = note
            .bytes
            .as_deref()
            .ok_or_else(|| IpcError::new(ErrorKind::TooLarge).subject(&note.name))?;
        let name = if dir.join(&note.name).exists() {
            let text = String::from_utf8_lossy(bytes);
            let stem = restored_stem(&copy_base(&title_of(&text), &note.name));
            first_free(&stem, " ", |n| dir.join(n).exists())
        } else {
            note.name.clone()
        };
        let abs = dir.join(&name);
        write_new(&abs, bytes).map_err(|e| IpcError::io(&name, &e))?;
        set_modified(&abs, note.modified_ms);
        ctx.update(|state| {
            if note.pinned && !state.pins.contains(&name) {
                state.pins.push(name.clone());
            }
            if note.shared && !state.shared.contains(&name) {
                state.shared.push(name.clone());
            }
        });
        Ok(name)
    }

    /// Gives a note (or a loose file) a real file name. A name with no extension gets `.md` (a loose file keeps its own
    /// extension). Nothing is overwritten. Returns the new document path.
    ///
    /// # Errors
    /// `InvalidName`, `Exists`, the file is not there, the system refused.
    pub fn rename(&self, path: &str, new_name: &str) -> Result<String, IpcError> {
        let ctx = self.ctx();
        match Self::target(path)? {
            Target::Note(old) => {
                let dir = self.ready_folder(&ctx)?;
                let (_, abs) = Self::existing_note(&ctx, path)?;
                let name = checked_name(new_name, Some(EXTENSION))?;
                if !is_note_name(&name) {
                    return Err(IpcError::new(ErrorKind::InvalidName).detail("notMarkdown"));
                }
                if name != old {
                    let target = dir.join(&name);
                    // the same file under another case is a rename; any other existing file is in the way
                    let same_file = fold(&name) == fold(&old);
                    if target.exists() && !same_file {
                        return Err(IpcError::new(ErrorKind::Exists).subject(name));
                    }
                    std::fs::rename(&abs, &target).map_err(|e| IpcError::io(&name, &e))?;
                    ctx.update(|state| state.rename(&old, &name));
                    ctx.journal.rename(&old, &name);
                    ctx.versions.rename(&old, &name);
                    lock(&ctx.cadence).renamed(&old, &name);
                    lock(&ctx.titles).remove(&old);
                }
                Ok(name)
            }
            Target::Loose { id } => self.rename_loose(&ctx, &id, new_name),
        }
    }

    // ---- what the person keeps about a note -----------------------------------------------------------------------

    /// What a path is kept under: a note's file name, a loose file's `@file/<id>`; the file must be there (notes) | listed.
    fn key_of(ctx: &Ctx, path: &str) -> Result<String, IpcError> {
        match Self::target(path)? {
            Target::Note(_) => Ok(Self::existing_note(ctx, path)?.0),
            Target::Loose { id } => {
                if ctx.read(|s| s.loose.iter().any(|e| e.id == id)) {
                    Ok(loose_key(&id))
                } else {
                    Err(IpcError::new(ErrorKind::NotFound).subject(path))
                }
            }
        }
    }

    /// Pins a document (it goes to the end of the pinned ones) or unpins it.
    ///
    /// # Errors
    /// Not there.
    pub fn set_pinned(&self, path: &str, pinned: bool) -> Result<(), IpcError> {
        let ctx = self.ctx();
        let key = Self::key_of(&ctx, path)?;
        ctx.update(|state| {
            state.pins.retain(|p| *p != key);
            if pinned {
                state.pins.push(key);
            }
        });
        Ok(())
    }

    /// Sets the order of the pinned documents: ones not pinned are ignored; pinned ones the order leaves out keep their
    /// place after the others.
    pub fn set_pin_order(&self, order: &[String]) {
        let ctx = self.ctx();
        ctx.update(|state| {
            let keys: Vec<String> = order
                .iter()
                .filter_map(|path| {
                    Self::target(path).ok().map(|target| match target {
                        Target::Note(name) => name,
                        Target::Loose { id } => loose_key(&id),
                    })
                })
                .filter(|key| state.pins.contains(key))
                .collect();
            let mut next: Vec<String> = Vec::new();
            for key in keys.into_iter().chain(state.pins.iter().cloned()) {
                if !next.contains(&key) {
                    next.push(key);
                }
            }
            state.pins = next;
        });
    }

    /// Shares a note with agents, or stops sharing it. Only notes, one by one; a loose file never reaches an agent.
    ///
    /// # Errors
    /// Not a note there.
    pub fn set_shared(&self, path: &str, shared: bool) -> Result<(), IpcError> {
        let ctx = self.ctx();
        let (name, _) = Self::existing_note(&ctx, path)?;
        ctx.update(|state| {
            state.shared.retain(|s| *s != name);
            if shared {
                state.shared.push(name);
            }
        });
        Ok(())
    }

    /// The note was copied out: it shows as copied until it is edited.
    ///
    /// # Errors
    /// Not a note there.
    pub fn mark_copied(&self, path: &str) -> Result<(), IpcError> {
        let ctx = self.ctx();
        let (name, abs) = Self::existing_note(&ctx, path)?;
        let stamp =
            Stamp::of(&abs).ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(path))?;
        ctx.update(|state| {
            state.copied.insert(name, stamp);
        });
        Ok(())
    }

    /// Sends the notes that were copied and not edited since to the recycle bin. Pinned notes stay.
    ///
    /// # Errors
    /// See [`NotesService::delete`].
    pub fn delete_copied(&self) -> Result<Deleted, IpcError> {
        let ctx = self.ctx();
        let copied: Vec<String> = ctx.read(|state| {
            state
                .copied
                .iter()
                .filter(|(name, stamp)| {
                    !state.is_pinned(name) && Stamp::of(&ctx.note_path(name)) == Some(**stamp)
                })
                .map(|(name, _)| name.clone())
                .collect()
        });
        if copied.is_empty() {
            return Ok(Deleted {
                undo_id: None,
                names: Vec::new(),
            });
        }
        self.delete(&copied)
    }

    /// A document was opened (`None`: none is open any more): it is the one to come back to, and a note's next save
    /// takes a version. A loose file goes to the top of the files.
    pub fn opened(&self, path: Option<&str>, now_ms: u64) {
        let ctx = self.ctx();
        let Some(path) = path else {
            ctx.update(|state| state.open = None);
            return;
        };
        match Self::target(path) {
            Ok(Target::Note(name)) => {
                lock(&ctx.cadence).opened(&name);
                ctx.update(|state| state.open = Some(name));
            }
            Ok(Target::Loose { id }) => Self::loose_opened(&ctx, &id, now_ms),
            Err(_) => {}
        }
    }

    // ---- versions ----------------------------------------------------------------------------------------------

    /// The earlier versions of a note, the newest first.
    ///
    /// # Errors
    /// Not a note.
    pub fn versions(&self, path: &str) -> Result<Vec<Version>, IpcError> {
        let ctx = self.ctx();
        match Self::target(path)? {
            Target::Note(name) => Ok(ctx.versions.versions(&name)),
            Target::Loose { .. } => Err(IpcError::new(ErrorKind::Unsupported).subject(path)),
        }
    }

    /// The text of an earlier version.
    ///
    /// # Errors
    /// Not a note | not a version.
    pub fn version_text(&self, path: &str, id: &str) -> Result<String, IpcError> {
        let ctx = self.ctx();
        let Target::Note(name) = Self::target(path)? else {
            return Err(IpcError::new(ErrorKind::Unsupported).subject(path));
        };
        let bytes = ctx
            .versions
            .read(&name, id)
            .map_err(|e| IpcError::io(id, &e))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Puts an earlier version back as the note's text. The text the note has now becomes a version first, so the
    /// restore can be undone.
    ///
    /// # Errors
    /// Not a note there | not a version | the system refused.
    pub fn restore_version(&self, path: &str, id: &str, now_ms: u64) -> Result<(), IpcError> {
        let ctx = self.ctx();
        self.ready_folder(&ctx)?;
        let (name, abs) = Self::existing_note(&ctx, path)?;
        let bytes = ctx
            .versions
            .read(&name, id)
            .map_err(|e| IpcError::io(id, &e))?;
        if let Ok(current) = std::fs::read(&abs) {
            let _ = ctx.versions.snapshot(&name, &current, now_ms);
            lock(&ctx.cadence).taken(&name, now_ms);
        }
        fsx::write_atomic(&abs, &bytes).map_err(|e| IpcError::io(&name, &e))?;
        ctx.journal.clear(&name);
        Ok(())
    }

    // ---- the journal --------------------------------------------------------------------------------------------

    /// Keeps the text the editor holds and the file does not (`text`; `None`: it is on disk, the entry goes).
    ///
    /// # Errors
    /// Not a document of the notes | the entry could not be kept.
    pub fn buffer(
        &self,
        path: &str,
        base_hash: &str,
        text: Option<&str>,
        now_ms: u64,
    ) -> Result<(), IpcError> {
        let ctx = self.ctx();
        let key = match Self::target(path)? {
            Target::Note(name) => name,
            Target::Loose { id } => loose_key(&id),
        };
        match text {
            None => ctx.journal.clear(&key),
            Some(text) => ctx
                .journal
                .save(&JournalEntry {
                    key,
                    base_hash: base_hash.to_owned(),
                    text: text.to_owned(),
                    saved_ms: now_ms,
                })
                .map_err(|e| IpcError::io(path, &e))?,
        }
        Ok(())
    }

    /// The text typed and never saved that was found for a document.
    #[must_use]
    pub fn recovered_text(&self, path: &str) -> Option<String> {
        let ctx = self.ctx();
        let key = match Self::target(path).ok()? {
            Target::Note(name) => name,
            Target::Loose { id } => loose_key(&id),
        };
        ctx.journal.load(&key).map(|entry| entry.text)
    }

    /// The person keeps the recovered text: it is written over the note (the text the note has is kept as a version).
    ///
    /// # Errors
    /// Nothing recovered | not there | the system refused.
    pub fn keep_recovered_text(&self, path: &str, now_ms: u64) -> Result<(), IpcError> {
        let ctx = self.ctx();
        self.ready_folder(&ctx)?;
        let (name, abs) = Self::existing_note(&ctx, path)?;
        let entry = ctx
            .journal
            .load(&name)
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject("recovered"))?;
        if let Ok(current) = std::fs::read(&abs) {
            let _ = ctx.versions.snapshot(&name, &current, now_ms);
        }
        write_like(&abs, &entry.text).map_err(|e| IpcError::io(&name, &e))?;
        ctx.journal.clear(&name);
        Ok(())
    }

    /// The person discards the recovered text (it stays among the versions).
    pub fn discard_recovered(&self, path: &str) {
        if let Ok(Target::Note(name)) = Self::target(path) {
            self.ctx().journal.clear(&name);
        }
    }
}

/// The name the person typed, checked; `extension` is added when the name has none.
pub(crate) fn checked_name(typed: &str, extension: Option<&str>) -> Result<String, IpcError> {
    let name = typed.trim();
    if let Some(reason) = name_problem(name) {
        return Err(IpcError::new(ErrorKind::InvalidName)
            .subject(name)
            .detail(reason));
    }
    let has_extension = std::path::Path::new(name)
        .extension()
        .is_some_and(|e| !e.is_empty());
    Ok(match extension {
        Some(extension) if !has_extension => format!("{name}.{extension}"),
        _ => name.to_owned(),
    })
}

/// Creates `path` (which must not exist) holding `bytes`, durably.
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(e) = written {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(())
}

/// The modification time a file had (best effort).
fn set_modified(path: &Path, modified_ms: u64) {
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(UNIX_EPOCH + Duration::from_millis(modified_ms));
    }
}

/// The text of the file the way the editor holds it (every line break `\n`, no byte order mark).
pub(crate) fn editor_text(path: &Path) -> Option<String> {
    let bytes = fsx::read_bytes(path, 6 * 1024 * 1024).ok()?;
    let decoded = fsx::decode(&bytes)?;
    Some(permark_core::edit::lines::normalized(&decoded.text))
}

/// Whether the journal entry holds text the file does not: typed and never saved.
pub(crate) fn differs_from_file(entry: &JournalEntry, path: &Path) -> bool {
    match editor_text(path) {
        Some(text) => text != entry.text,
        None => !is_blank(&entry.text),
    }
}

/// Writes `text` (editor form) over the file the way the file is written: its encoding and its line ending. A file that
/// is not there is written as UTF-8 with `\n`.
pub(crate) fn write_like(path: &Path, text: &str) -> std::io::Result<()> {
    let (ending, encoding) = match fsx::read_text(path, 6 * 1024 * 1024) {
        Ok(decoded) => (
            permark_core::edit::lines::majority(&permark_core::edit::lines::split(&decoded.text))
                .to_owned(),
            decoded.encoding,
        ),
        Err(_) => ("\n".to_owned(), fsx::Encoding::Utf8),
    };
    let body = if ending == "\n" {
        text.to_owned()
    } else {
        text.replace('\n', &ending)
    };
    fsx::write_atomic(path, &permark_core::edit::lines::encode(&body, encoding))
}

impl NotesService {
    /// A snapshot is due before this save replaces `old`: the first one after the note was opened, then every ten minutes.
    pub(crate) fn before_replace(&self, path: &str, old: &[u8], now_ms: u64) {
        let ctx = self.ctx();
        let Ok(Target::Note(name)) = Self::target(path) else {
            return;
        };
        let due = lock(&ctx.cadence).due(&name, now_ms);
        if due && ctx.versions.snapshot(&name, old, now_ms).is_ok() {
            lock(&ctx.cadence).taken(&name, now_ms);
        }
    }
}

impl NotesService {
    /// The editor saved `path`: a loose file looks different now, and that is not a change somewhere else.
    pub(crate) fn after_replace(&self, path: &str) {
        self.loose_saved(path);
    }

    /// The notes the person shared, as an agent may list them.
    pub(crate) fn shared_notes(&self) -> Vec<permark_core::notes::SharedNote> {
        let ctx = self.ctx();
        let shared = ctx.read(|s| s.shared.clone());
        shared
            .into_iter()
            .filter_map(|name| {
                let abs = ctx.note_path(&name);
                let stamp = Stamp::of(&abs)?;
                Some(permark_core::notes::SharedNote {
                    title: title_of(&head_of(&abs)),
                    updated_ms: stamp.modified_ms,
                    id: name,
                })
            })
            .collect()
    }

    /// The text of a shared note as it is on disk. A note that is not shared does not exist for an agent.
    pub(crate) fn read_shared(&self, id: &str) -> Result<String, IpcError> {
        let ctx = self.ctx();
        if !ctx.read(|s| s.is_shared(id)) {
            return Err(IpcError::new(ErrorKind::NotFound).subject(id));
        }
        let (_, abs) = Self::existing_note(&ctx, id)?;
        let bytes = fsx::read_bytes(&abs, 6 * 1024 * 1024).map_err(IpcError::from)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}
