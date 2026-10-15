//! The notes of one folder: listing them, writing them without ever losing text, and keeping their earlier versions.
//!
//! Everything here is plain Rust over a [`Vault`] rooted at the notes folder, so it is tested without a window. A note is a
//! Markdown file in the folder; its identity is its file name. The service never asks the person for a name and never
//! asks "save changes?": it keeps the text safe instead.
//!
//! - **A new note has no file** until its first character that is not blank is saved (see [`NotesService::reserve_new`]).
//! - **A save** is the guarded atomic write of the editor. A note that changed on disk meanwhile (another computer
//!   through a synced folder, another editor) is not overwritten: the text the person has is kept as a note of its own,
//!   "{title} (conflict {time})".
//! - **A version** of the replaced text is kept at the first save after a note is opened, then at most every ten
//!   minutes (see [`history`](super::history)).
//! - **Deleting** sends a note to the system's recycle bin and keeps its bytes for an undo for as long as the program
//!   runs.
//! - **Another program** that has the folder open holds a lock; this one then reads and writes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, UNIX_EPOCH};

use serde::Serialize;

use super::data::{Cached, NoteView, NotesData, Stamp};
use super::history::{Cadence, History};
use super::lock::{self, Access, FolderLock};
use super::names::{
    conflict_stem, copy_base, first_free, is_note_name, note_stem, recovered_stem, Moment,
    EXTENSION, MAX_TRIES,
};
use super::recover::{parse_temp_name, recoverable_text, recovered_name};
use super::text::{is_blank, snippet_of, title_of, HEAD_BYTES};
use crate::edit::{EditError, Editor, SaveOutcome};
use crate::fileops::Recycler;
use crate::render::MAX_RENDER_BYTES;
use crate::util::{hash_bytes, hash_hex, modified_ms, read_with_retry};
use crate::vault::{Vault, VaultError, CASE_INSENSITIVE};
use crate::watch::OwnWrites;

/// The bytes of the notes deleted lately that are kept for an undo, in all.
const UNDO_BUDGET_BYTES: usize = 20 * 1024 * 1024;
/// How many deletions can be undone.
const UNDO_DEPTH: usize = 8;
/// The notes the welcome screen lists after the pinned ones.
pub const SUMMARY_LATEST: usize = 3;

#[derive(Debug, thiserror::Error)]
pub enum NotesError {
    #[error(transparent)]
    Edit(#[from] EditError),
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("cannot access {path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("these notes are open in another TrailMark window, so they can only be read here")]
    Locked,
    #[error("the notes folder cannot be written")]
    Unwritable,
    #[error("a note named {0} already exists")]
    Exists(String),
    #[error("{0} is not a note")]
    NotANote(String),
    #[error("there is nothing to undo")]
    UnknownUndo,
    #[error("that earlier version is not there")]
    NoSuchVersion,
    #[error("{0} is not text that can be restored")]
    NotText(String),
    #[error("{0} changed while it was being restored")]
    Changed(String),
    #[error("this window does not show notes")]
    NotOpen,
}

impl NotesError {
    /// A short stable identifier for the UI.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Edit(e) => e.kind(),
            Self::Vault(e) => e.kind(),
            Self::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied => "denied",
            Self::Io { .. } => "io",
            Self::Locked => "notesLocked",
            Self::Unwritable => "notesUnwritable",
            Self::Exists(_) => "exists",
            Self::NotANote(_) => "notANote",
            Self::UnknownUndo => "unknownUndo",
            Self::NoSuchVersion => "noSuchVersion",
            Self::NotText(_) => "notText",
            Self::Changed(_) => "changed",
            Self::NotOpen => "notesNotOpen",
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

/// Why the notes cannot be written here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReadOnlyReason {
    /// Another TrailMark window has them open.
    Locked,
    /// The folder does not let this program write.
    Unwritable,
}

/// What opening the notes found. Matches the UI's `NotesOpened` (besides the folder's `VaultInfo`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenReport {
    /// Set when the notes can only be read.
    pub read_only: Option<ReadOnlyReason>,
    /// The notes made from text that a crash left in temporary files.
    pub recovered: Vec<String>,
}

/// A note in the list. Matches the UI's `NoteEntry`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEntry {
    /// The note's file name in the notes folder.
    pub path: String,
    /// The first line, without Markdown marks. Empty for a note with no text.
    pub title: String,
    pub snippet: String,
    pub modified_ms: u64,
    pub size_bytes: u64,
    pub pinned: bool,
    /// The note was copied and not edited since.
    pub copied: bool,
    /// Where the person left it.
    pub view: Option<NoteView>,
}

/// The notes of the folder, the pinned ones first in the person's order, then the others by last change.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteList {
    pub notes: Vec<NoteEntry>,
    /// The note that was open when the window was last closed.
    pub open: Option<String>,
}

/// What the welcome screen shows of the notes: the pinned ones, then the latest.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesSummary {
    pub pinned: Vec<NoteEntry>,
    pub latest: Vec<NoteEntry>,
}

/// The result of saving a note. Matches the UI's `NoteSaved`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NoteSaved {
    /// The text is in the note's file. `path` is the file's name, which differs from the one asked for only when a
    /// new note's name was taken meanwhile.
    Saved {
        path: String,
        hash: String,
        modified_ms: u64,
        /// The file did not exist before.
        created: bool,
    },
    /// A new note whose text is still blank: no file was made.
    Blank,
    /// The note was changed by someone else since it was opened. Nothing of theirs was overwritten: the text that was
    /// being saved is in the note `copy`.
    Conflict { copy: String },
}

/// An earlier version of a note. Matches the UI's `NoteVersion`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteVersion {
    pub id: String,
    pub written_ms: u64,
    pub size_bytes: u64,
}

/// A note restored from an earlier version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub hash: String,
    pub modified_ms: u64,
}

/// Notes sent to the recycle bin. Matches the UI's `NoteDeleted`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDeleted {
    pub deleted: Vec<String>,
    /// Asks for the notes back; none when they are too big to keep for it.
    pub undo_id: Option<u64>,
}

/// One note that was deleted, with what it takes to put it back.
struct DeletedNote {
    name: String,
    bytes: Vec<u8>,
    modified_ms: u64,
}

struct DeletedBatch {
    id: u64,
    notes: Vec<DeletedNote>,
}

struct State {
    data: NotesData,
    cadence: Cadence,
    /// New notes that have a name and no file yet: name to the base it was made from.
    reserved: BTreeMap<String, String>,
    undo: Vec<DeletedBatch>,
    next_undo: u64,
}

/// Whether this program may write the notes, and the lock that says so.
enum Writing {
    Allowed(#[allow(dead_code)] FolderLock),
    Refused(ReadOnlyReason),
}

pub struct NotesService {
    vault: Arc<Vault>,
    own: Arc<OwnWrites>,
    recycler: Arc<dyn Recycler>,
    history: History,
    writing: Writing,
    state: Mutex<State>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A note file found in the folder.
struct Found {
    name: String,
    stamp: Stamp,
}

/// The note files directly inside `root`: Markdown files whose names do not start with a dot.
fn scan(root: &Path) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !is_note_name(&name) {
                return None;
            }
            let meta = entry.metadata().ok().filter(std::fs::Metadata::is_file)?;
            Some(Found {
                name,
                stamp: Stamp {
                    size: meta.len(),
                    modified_ms: modified_ms(&meta),
                },
            })
        })
        .collect()
}

/// The start of a note's file, which is all a title and a snippet need.
fn read_head(path: &Path) -> String {
    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    if file
        .take(HEAD_BYTES as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return String::new();
    }
    if bytes[..bytes.len().min(512)].contains(&0) {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The list of the notes in `root`, using and refreshing the cache of titles in `data`. Returns whether `data` changed.
fn entries(root: &Path, data: &mut NotesData) -> (Vec<NoteEntry>, bool) {
    let found = scan(root);
    let mut changed = false;
    let present: BTreeSet<&str> = found.iter().map(|f| f.name.as_str()).collect();
    let before = data.cache.len();
    data.cache.retain(|name, _| present.contains(name.as_str()));
    changed |= data.cache.len() != before;

    let mut notes: Vec<NoteEntry> = found
        .iter()
        .map(|file| {
            let cached = data
                .cache
                .get(&file.name)
                .filter(|cached| cached.stamp == file.stamp);
            let (title, snippet) = if let Some(cached) = cached {
                (cached.title.clone(), cached.snippet.clone())
            } else {
                let head = read_head(&root.join(&file.name));
                let (title, snippet) = (title_of(&head), snippet_of(&head));
                data.cache.insert(
                    file.name.clone(),
                    Cached {
                        stamp: file.stamp,
                        title: title.clone(),
                        snippet: snippet.clone(),
                    },
                );
                changed = true;
                (title, snippet)
            };
            NoteEntry {
                path: file.name.clone(),
                title,
                snippet,
                modified_ms: file.stamp.modified_ms,
                size_bytes: file.stamp.size,
                pinned: data.is_pinned(&file.name),
                copied: data.copied.get(&file.name) == Some(&file.stamp),
                view: data.views.get(&file.name).cloned(),
            }
        })
        .collect();
    notes.sort_by(|a, b| {
        let rank = |note: &NoteEntry| data.pins.iter().position(|pin| *pin == note.path);
        match (rank(a), rank(b)) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b
                .modified_ms
                .cmp(&a.modified_ms)
                .then_with(|| a.path.cmp(&b.path)),
        }
    });
    (notes, changed)
}

/// What the welcome screen shows of the notes in `root`: read from the files, without a service and without a lock, so
/// any window can ask.
pub fn summary(root: &Path) -> NotesSummary {
    let mut data = NotesData::load(root);
    let (notes, _) = entries(root, &mut data);
    let (pinned, others): (Vec<_>, Vec<_>) = notes.into_iter().partition(|note| note.pinned);
    NotesSummary {
        pinned,
        latest: others.into_iter().take(SUMMARY_LATEST).collect(),
    }
}

impl NotesService {
    /// Opens the notes of the folder `vault` is rooted at: takes the lock (or finds it taken and goes read-only), tidies
    /// what is kept about notes that are gone, prunes old versions, and keeps the text that a crash left in temporary
    /// files. `is_running` says whether a process is still running (a temporary file of one that is is left alone).
    pub fn open(
        vault: Arc<Vault>,
        own: Arc<OwnWrites>,
        recycler: Arc<dyn Recycler>,
        now_ms: u64,
        is_running: &dyn Fn(u32) -> bool,
    ) -> (Self, OpenReport) {
        let writing = match lock::acquire(vault.root()) {
            Access::Writable(lock) => Writing::Allowed(lock),
            Access::Locked => Writing::Refused(ReadOnlyReason::Locked),
            Access::Unwritable => Writing::Refused(ReadOnlyReason::Unwritable),
        };
        let service = Self {
            history: History::new(vault.root()),
            vault,
            own,
            recycler,
            writing,
            state: Mutex::new(State {
                data: NotesData::default(),
                cadence: Cadence::default(),
                reserved: BTreeMap::new(),
                undo: Vec::new(),
                next_undo: 1,
            }),
        };
        let root = service.root().to_path_buf();
        let mut data = NotesData::load(&root);
        let names: BTreeSet<String> = scan(&root).into_iter().map(|f| f.name).collect();
        data.retain_existing(&names);
        lock(&service.state).data = data;
        let recovered = if service.can_write() {
            service.history.prune_all(&names, now_ms);
            service.recover_leftovers(is_running)
        } else {
            Vec::new()
        };
        service.persist();
        let report = OpenReport {
            read_only: match service.writing {
                Writing::Refused(reason) => Some(reason),
                Writing::Allowed(_) => None,
            },
            recovered,
        };
        (service, report)
    }

    fn root(&self) -> &Path {
        self.vault.root()
    }

    fn can_write(&self) -> bool {
        matches!(self.writing, Writing::Allowed(_))
    }

    /// Whether the notes can be written here; the error says why not.
    pub fn writable(&self) -> Result<(), NotesError> {
        self.require_writable()
    }

    fn require_writable(&self) -> Result<(), NotesError> {
        match self.writing {
            Writing::Allowed(_) => Ok(()),
            Writing::Refused(ReadOnlyReason::Locked) => Err(NotesError::Locked),
            Writing::Refused(ReadOnlyReason::Unwritable) => Err(NotesError::Unwritable),
        }
    }

    fn editor(&self) -> Editor<'_> {
        Editor::new(&self.vault, &self.own)
    }

    /// Writes `notes.json` when something changed. A failure is not worth stopping for: it is all rebuilt from the files.
    fn persist(&self) {
        if self.can_write() {
            let _ = lock(&self.state).data.save(self.root());
        }
    }

    /// The name and place of the note a UI path means. Notes are the files directly inside the folder.
    fn resolve_note(&self, path: &str) -> Result<(String, PathBuf), NotesError> {
        let resolved = self.vault.resolve(path)?;
        if resolved.rel.contains('/')
            || resolved.rel.starts_with('.')
            || !is_note_name(&resolved.rel)
        {
            return Err(NotesError::NotANote(path.to_owned()));
        }
        Ok((resolved.rel, resolved.abs))
    }

    // ---- the list ------------------------------------------------------------------------------------

    /// The notes, in the order the list shows them.
    pub fn list(&self) -> NoteList {
        let (notes, changed) = {
            let mut state = lock(&self.state);
            let (notes, changed) = entries(self.root(), &mut state.data);
            if state
                .data
                .open
                .as_ref()
                .is_some_and(|open| !notes.iter().any(|note| note.path == *open))
            {
                state.data.open = None;
            }
            (notes, changed)
        };
        if changed {
            self.persist();
        }
        let open = lock(&self.state).data.open.clone();
        NoteList { notes, open }
    }

    // ---- new notes -----------------------------------------------------------------------------------

    /// Gives a new note its name, made from `moment`. No file is made: that happens when the first text that is not blank
    /// is saved. The name is not given twice while the note has no file.
    pub fn reserve_new(&self, moment: Moment) -> Result<String, NotesError> {
        self.require_writable()?;
        let stem = note_stem(moment);
        let mut state = lock(&self.state);
        let name = first_free(&stem, "-", |name| {
            self.root().join(name).exists() || state.reserved.contains_key(name)
        });
        state.reserved.insert(name.clone(), stem);
        Ok(name)
    }

    /// Whether `path` names a new note that has no file yet.
    pub fn is_unborn(&self, path: &str) -> bool {
        self.vault.resolve(path).is_ok_and(|resolved| {
            lock(&self.state).reserved.contains_key(&resolved.rel) && !resolved.abs.exists()
        })
    }

    /// A new note that was left without being written is forgotten.
    pub fn release_new(&self, path: &str) {
        lock(&self.state).reserved.remove(path);
    }

    // ---- saving --------------------------------------------------------------------------------------

    /// Saves the text of a note. `expected_hash` is the hash of the file the editor started from; empty for a new note
    /// that has no file yet.
    pub fn save(
        &self,
        path: &str,
        text: &str,
        expected_hash: &str,
        now_ms: u64,
        moment: Moment,
    ) -> Result<NoteSaved, NotesError> {
        self.save_with(path, text, expected_hash, now_ms, moment, || {})
    }

    /// [`NotesService::save`] with `after_check` run between the first comparison with the file and the write: the moment
    /// in which a test makes another writer get in.
    fn save_with(
        &self,
        path: &str,
        text: &str,
        expected_hash: &str,
        now_ms: u64,
        moment: Moment,
        after_check: impl FnOnce(),
    ) -> Result<NoteSaved, NotesError> {
        self.require_writable()?;
        let (name, abs) = self.resolve_note(path)?;
        if expected_hash.is_empty() {
            if is_blank(text) {
                return Ok(NoteSaved::Blank);
            }
            return self.create_note(&name, text, now_ms);
        }
        let on_disk = match read_with_retry(&abs) {
            Ok(bytes) => bytes,
            // taken away behind the editor's back: the text comes back under its own name
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return self.create_note(&name, text, now_ms);
            }
            Err(e) => return Err(NotesError::io(&name, e)),
        };
        if hash_hex(&on_disk) != expected_hash {
            return self.keep_as_copy(&name, text, moment);
        }
        if on_disk != text.as_bytes() {
            self.take_version(&name, &abs, &on_disk, now_ms, false);
        }
        after_check();
        match self.editor().save(&name, text, expected_hash)? {
            SaveOutcome::Saved { hash, modified_ms } => Ok(NoteSaved::Saved {
                path: name,
                hash,
                modified_ms,
                created: false,
            }),
            SaveOutcome::Conflict { .. } => self.keep_as_copy(&name, text, moment),
        }
    }

    /// Makes the file of a note, under `name` or the next free name when it is taken.
    fn create_note(&self, name: &str, text: &str, now_ms: u64) -> Result<NoteSaved, NotesError> {
        let stem = lock(&self.state)
            .reserved
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.trim_end_matches(&format!(".{EXTENSION}")).to_owned());
        let mut wanted = name.to_owned();
        for _ in 0..MAX_TRIES {
            match self.editor().create(&wanted, text) {
                Ok(created) => {
                    let mut state = lock(&self.state);
                    state.reserved.remove(name);
                    state.cadence.taken(&created.path, now_ms);
                    return Ok(NoteSaved::Saved {
                        path: created.path,
                        hash: created.hash,
                        modified_ms: created.modified_ms,
                        created: true,
                    });
                }
                Err(EditError::Exists(_)) => {
                    wanted =
                        first_free(&stem, "-", |candidate| self.root().join(candidate).exists());
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(NotesError::Exists(name.to_owned()))
    }

    /// Keeps `text`, which could not be saved over the note `name` because the note is not what the editor started from,
    /// as a note of its own.
    fn keep_as_copy(
        &self,
        name: &str,
        text: &str,
        moment: Moment,
    ) -> Result<NoteSaved, NotesError> {
        let stem = conflict_stem(&copy_base(&title_of(text), name), moment);
        let copy = self.create_copy(&stem, text)?;
        Ok(NoteSaved::Conflict { copy })
    }

    /// Makes a note named after `stem` (with a number after it when that name is taken) holding `text`.
    fn create_copy(&self, stem: &str, text: &str) -> Result<String, NotesError> {
        for _ in 0..MAX_TRIES {
            let wanted = first_free(stem, " ", |candidate| self.root().join(candidate).exists());
            match self.editor().create(&wanted, text) {
                Ok(created) => return Ok(created.path),
                Err(EditError::Exists(_)) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(NotesError::Exists(stem.to_owned()))
    }

    /// Keeps `bytes`, the text a save is about to replace, as an earlier version when one is due (or `force`d). Best effort:
    /// a version that cannot be kept never stops a save.
    fn take_version(&self, name: &str, abs: &Path, bytes: &[u8], now_ms: u64, force: bool) {
        {
            let mut state = lock(&self.state);
            if !force && !state.cadence.due(name, now_ms) {
                return;
            }
            state.cadence.taken(name, now_ms);
        }
        let written = std::fs::metadata(abs).map_or(now_ms, |meta| modified_ms(&meta));
        let _ = self.history.snapshot(name, bytes, written);
        self.history.prune(name, now_ms);
    }

    // ---- earlier versions -----------------------------------------------------------------------------

    /// The earlier versions of a note, the newest first.
    pub fn versions(&self, path: &str) -> Result<Vec<NoteVersion>, NotesError> {
        let (name, _) = self.resolve_note(path)?;
        Ok(self
            .history
            .versions(&name)
            .into_iter()
            .map(|v| NoteVersion {
                id: v.id,
                written_ms: v.written_ms,
                size_bytes: v.size,
            })
            .collect())
    }

    /// The text of an earlier version.
    pub fn version_text(&self, path: &str, id: &str) -> Result<String, NotesError> {
        let (name, _) = self.resolve_note(path)?;
        let bytes = self
            .history
            .read(&name, id)
            .map_err(|_| NotesError::NoSuchVersion)?;
        String::from_utf8(bytes).map_err(|_| NotesError::NotText(name))
    }

    /// Puts an earlier version back as the note's text. The text the note has now becomes a version first, so the restore
    /// can be undone.
    pub fn restore_version(
        &self,
        path: &str,
        id: &str,
        now_ms: u64,
    ) -> Result<Restored, NotesError> {
        self.require_writable()?;
        let (name, abs) = self.resolve_note(path)?;
        let text = self.version_text(path, id)?;
        let current = match read_with_retry(&abs) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(NotesError::io(&name, e)),
        };
        let Some(current) = current else {
            let created = self.editor().create(&name, &text)?;
            return Ok(Restored {
                hash: created.hash,
                modified_ms: created.modified_ms,
            });
        };
        self.take_version(&name, &abs, &current, now_ms, true);
        match self.editor().save(&name, &text, &hash_hex(&current))? {
            SaveOutcome::Saved { hash, modified_ms } => Ok(Restored { hash, modified_ms }),
            SaveOutcome::Conflict { .. } => Err(NotesError::Changed(name)),
        }
    }

    // ---- renaming and deleting -----------------------------------------------------------------------

    /// Gives a note a real file name. A name with no extension gets `.md`. Nothing is overwritten.
    pub fn rename(&self, path: &str, new_name: &str) -> Result<String, NotesError> {
        self.require_writable()?;
        let (from, from_abs) = self.resolve_note(path)?;
        let typed = new_name.trim();
        crate::edit::validate_file_name(typed)
            .map_err(|problem| NotesError::Edit(EditError::InvalidName(problem)))?;
        let to = if is_note_name(typed) {
            typed.to_owned()
        } else {
            format!("{typed}.{EXTENSION}")
        };
        // a checked name has no separator in it, so the file stays in the folder
        let to_abs = self.root().join(&to);
        if from == to {
            return Ok(to);
        }
        // on a system that ignores case, `a.md` and `A.md` are one file: that is only a change of case
        let same_file = CASE_INSENSITIVE && from.to_lowercase() == to.to_lowercase();
        if to_abs.exists() && !same_file {
            return Err(NotesError::Exists(to));
        }
        self.own.expect_moves(&[(from.clone(), to.clone())]);
        if let Err(e) = std::fs::rename(&from_abs, &to_abs) {
            self.own.retract_moves(&[(from.clone(), to.clone())]);
            return Err(NotesError::io(&from, e));
        }
        self.history.rename(&from, &to);
        {
            let mut state = lock(&self.state);
            state.data.rename(&from, &to);
            state.cadence.renamed(&from, &to);
        }
        self.persist();
        Ok(to)
    }

    /// Sends notes to the system's recycle bin. Their text is kept in memory for [`NotesService::undo_delete`].
    pub fn delete(&self, paths: &[String]) -> Result<NoteDeleted, NotesError> {
        self.require_writable()?;
        let mut targets: Vec<(String, PathBuf)> = Vec::new();
        for path in paths {
            let (name, abs) = self.resolve_note(path)?;
            if !abs.is_file() {
                return Err(VaultError::NotFound(name).into());
            }
            if !targets.iter().any(|(known, _)| *known == name) {
                targets.push((name, abs));
            }
        }
        let mut kept = Vec::new();
        let mut keepable = true;
        for (name, abs) in &targets {
            let modified = std::fs::metadata(abs).map_or(0, |meta| modified_ms(&meta));
            match read_with_retry(abs) {
                Ok(bytes) if bytes.len() as u64 <= MAX_RENDER_BYTES => kept.push(DeletedNote {
                    name: name.clone(),
                    bytes,
                    modified_ms: modified,
                }),
                _ => keepable = false,
            }
        }
        let names: Vec<String> = targets.iter().map(|(name, _)| name.clone()).collect();
        self.own.expect_removals(&names);
        let recycled = self.recycler.recycle(
            &targets
                .iter()
                .map(|(_, abs)| abs.clone())
                .collect::<Vec<_>>(),
        );
        if let Err(message) = recycled {
            self.own.retract_removals(&names);
            return Err(NotesError::Io {
                path: names.join(", "),
                source: io::Error::other(message),
            });
        }
        let undo_id = keepable.then(|| {
            let mut state = lock(&self.state);
            let id = state.next_undo;
            state.next_undo += 1;
            state.undo.push(DeletedBatch { id, notes: kept });
            // the oldest deletions are forgotten first, by count and by size
            while state.undo.len() > UNDO_DEPTH
                || (state.undo.len() > 1
                    && state
                        .undo
                        .iter()
                        .flat_map(|batch| &batch.notes)
                        .map(|note| note.bytes.len())
                        .sum::<usize>()
                        > UNDO_BUDGET_BYTES)
            {
                state.undo.remove(0);
            }
            id
        });
        Ok(NoteDeleted {
            deleted: names,
            undo_id,
        })
    }

    /// Puts back the notes of a deletion, with the text and the modification time they had. A note whose name is taken
    /// again comes back as "{title} (restored)". Returns the names they have now.
    pub fn undo_delete(&self, undo_id: u64) -> Result<Vec<String>, NotesError> {
        self.require_writable()?;
        let batch = {
            let mut state = lock(&self.state);
            let at = state
                .undo
                .iter()
                .position(|batch| batch.id == undo_id)
                .ok_or(NotesError::UnknownUndo)?;
            state.undo.remove(at)
        };
        let mut back = Vec::new();
        for note in batch.notes {
            let stem = note
                .name
                .trim_end_matches(&format!(".{EXTENSION}"))
                .to_owned();
            let name = if self.root().join(&note.name).exists() {
                first_free(&format!("{stem} (restored)"), " ", |candidate| {
                    self.root().join(candidate).exists()
                })
            } else {
                note.name.clone()
            };
            self.restore_file(&name, &note)?;
            back.push(name);
        }
        Ok(back)
    }

    /// Writes a deleted note back under `name`, with its old modification time.
    fn restore_file(&self, name: &str, note: &DeletedNote) -> Result<(), NotesError> {
        let abs = self.root().join(name);
        let hash = hash_bytes(&note.bytes);
        self.own.expect(name, hash);
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&abs)
            .and_then(|mut file| {
                file.write_all(&note.bytes)?;
                file.sync_all()?;
                // the note keeps the place it had in the list
                let _ = file.set_modified(UNIX_EPOCH + Duration::from_millis(note.modified_ms));
                Ok(())
            });
        written.map_err(|e| {
            self.own.retract(name, hash);
            NotesError::io(name, e)
        })
    }

    /// Removes a note that was never written in: blank, not pinned. Returns whether it was removed. The person's own text
    /// is never touched: a note with anything but blank characters stays.
    pub fn discard_blank(&self, path: &str) -> Result<bool, NotesError> {
        self.require_writable()?;
        let (name, abs) = self.resolve_note(path)?;
        if lock(&self.state).data.is_pinned(&name) {
            return Ok(false);
        }
        let Ok(bytes) = std::fs::read(&abs) else {
            return Ok(false);
        };
        let blank = String::from_utf8(bytes).is_ok_and(|text| is_blank(&text));
        if !blank {
            return Ok(false);
        }
        self.own.expect_removals(std::slice::from_ref(&name));
        match std::fs::remove_file(&abs) {
            Ok(()) => Ok(true),
            Err(e) => {
                self.own.retract_removals(std::slice::from_ref(&name));
                Err(NotesError::io(&name, e))
            }
        }
    }

    // ---- what is remembered ---------------------------------------------------------------------------

    /// Pins a note (it goes to the end of the pinned ones) or unpins it.
    pub fn set_pinned(&self, path: &str, pinned: bool) -> Result<(), NotesError> {
        self.require_writable()?;
        let (name, abs) = self.resolve_note(path)?;
        if pinned && !abs.is_file() {
            return Err(VaultError::NotFound(name).into());
        }
        {
            let mut state = lock(&self.state);
            state.data.pins.retain(|pin| *pin != name);
            if pinned {
                state.data.pins.push(name);
            }
        }
        self.persist();
        Ok(())
    }

    /// Sets the order of the pinned notes. Names that are not pinned are ignored; pinned notes the order leaves out keep
    /// their place after the others.
    pub fn set_pin_order(&self, order: &[String]) -> Result<(), NotesError> {
        self.require_writable()?;
        {
            let mut state = lock(&self.state);
            let pinned = state.data.pins.clone();
            let mut next: Vec<String> = Vec::new();
            for name in order {
                if pinned.contains(name) && !next.contains(name) {
                    next.push(name.clone());
                }
            }
            let left_out: Vec<String> = pinned
                .into_iter()
                .filter(|name| !next.contains(name))
                .collect();
            next.extend(left_out);
            state.data.pins = next;
        }
        self.persist();
        Ok(())
    }

    /// The note was opened: it is the one to come back to, and its next save takes a version.
    pub fn opened(&self, path: &str) {
        let Ok((name, _)) = self.resolve_note(path) else {
            return;
        };
        {
            let mut state = lock(&self.state);
            state.data.open = Some(name.clone());
            state.cadence.opened(&name);
        }
        self.persist();
    }

    /// No note is open any more.
    pub fn closed(&self) {
        lock(&self.state).data.open = None;
        self.persist();
    }

    /// Remembers where a note was left.
    pub fn set_view(&self, path: &str, view: NoteView) {
        let Ok((name, _)) = self.resolve_note(path) else {
            return;
        };
        lock(&self.state).data.views.insert(name, view);
        self.persist();
    }

    /// The note was copied: it shows as copied until it is edited.
    pub fn mark_copied(&self, path: &str) -> Result<(), NotesError> {
        let (name, abs) = self.resolve_note(path)?;
        let meta = std::fs::metadata(&abs).map_err(|e| NotesError::io(&name, e))?;
        lock(&self.state).data.copied.insert(
            name,
            Stamp {
                size: meta.len(),
                modified_ms: modified_ms(&meta),
            },
        );
        self.persist();
        Ok(())
    }

    /// Sends the notes that were copied and not edited since to the recycle bin. Pinned notes stay.
    pub fn delete_copied(&self) -> Result<NoteDeleted, NotesError> {
        let list = self.list();
        let copied: Vec<String> = list
            .notes
            .into_iter()
            .filter(|note| note.copied && !note.pinned)
            .map(|note| note.path)
            .collect();
        if copied.is_empty() {
            return Ok(NoteDeleted {
                deleted: Vec::new(),
                undo_id: None,
            });
        }
        self.delete(&copied)
    }

    // ---- after a crash --------------------------------------------------------------------------------

    /// Keeps the text that temporary files of programs that are gone hold, as "{title} (recovered)". A temporary file is
    /// only used when it is newer than its note. None is deleted: a recovered one is renamed so it is not found again.
    fn recover_leftovers(&self, is_running: &dyn Fn(u32) -> bool) -> Vec<String> {
        let root = self.root();
        let Ok(entries) = std::fs::read_dir(root) else {
            return Vec::new();
        };
        let mut leftovers: Vec<(String, String)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let file = entry.file_name().to_string_lossy().into_owned();
                let (note, pid) = parse_temp_name(&file)?;
                (is_note_name(note) && !is_running(pid)).then(|| (file.clone(), note.to_owned()))
            })
            .collect();
        leftovers.sort();
        let mut recovered = Vec::new();
        for (file, note) in leftovers {
            let (temp, note_path) = (root.join(&file), root.join(&note));
            let newer = match (modified_of(&temp), modified_of(&note_path)) {
                (Some(temp), Some(note)) => temp > note,
                (Some(_), None) => true,
                _ => false,
            };
            if !newer {
                continue;
            }
            let Some(text) = recoverable_text(&temp, &note_path) else {
                continue;
            };
            let stem = recovered_stem(&copy_base(&title_of(&text), &note));
            let Ok(name) = self.create_copy(&stem, &text) else {
                continue;
            };
            // the temporary file stays, under a name that says it was used
            let _ = std::fs::rename(&temp, root.join(recovered_name(&file)));
            recovered.push(name);
        }
        recovered
    }
}

/// The modification time of a file in milliseconds.
fn modified_of(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|meta| modified_ms(&meta))
}

#[cfg(test)]
mod tests;
