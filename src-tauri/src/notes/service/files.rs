//! Loose files in the service: opening them, finding out what happened to them on disk, saving them when the person asks,
//! and keeping the text typed into them meanwhile (see [`loose`] and [`pending`](super::super::pending)).
//!
//! Nothing here writes a loose file on its own. The one write is [`NotesService::save_file`], which the person asks for, and
//! it is the guarded save of the editor: a file that is not the version the person started from is left alone.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::super::data::{NotesData, Stamp};
use super::super::loose::{self, LooseFile};
use super::super::names::{edits_stem, is_note_name};
use super::super::pending::PendingEdits;
use super::super::text::HEAD_BYTES;
use super::{lock, EntryKind, NoteEntry, NotesError, NotesService, State};
use crate::edit::{CreatedDoc, EditError, Editor, SaveOutcome};
use crate::render::{decode_text, MAX_RENDER_BYTES};
use crate::vault::{Vault, VaultError};
use crate::watch::OwnWrites;

/// What opening a file made of it. Matches the UI's `OpenedFile`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedFile {
    /// A note when the file is one of the notes folder, else a loose file.
    pub kind: EntryKind,
    /// The path to open: the note's name, or the UI path of the loose file.
    pub path: String,
}

/// What a loose file looks like on disk compared with what was last read here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LooseState {
    /// As it was read.
    Same,
    /// Changed since: someone else wrote it.
    Changed,
    /// Not there.
    Missing,
}

/// One loose file's state. Matches the UI's `LooseStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LooseStatus {
    pub path: String,
    pub state: LooseState,
    /// How the file looks now (size and time), for the UI to tell one change from the next.
    pub stamp: String,
    /// The path the file had before it was found again under another name in its folder.
    pub was: Option<String>,
}

/// What a UI path of a loose file means: the file's own scope.
pub struct LooseDoc {
    pub id: String,
    /// The file's folder as a vault of one file (shallow): the document and the images next to it, nothing else.
    pub vault: Vault,
    /// The file's name in that vault.
    pub name: String,
    /// The UI path of the file itself.
    pub ui_path: String,
    /// The image next to the file that the path asks for, when it is not the file.
    pub asset: Option<String>,
}

/// Whether `path` is the absolute place of the file named `name`, with nothing that climbs out of a folder.
fn recorded_file(path: &Path, name: &str) -> bool {
    path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
        && path
            .file_name()
            .is_some_and(|file_name| loose::same_path(&file_name.to_string_lossy(), name))
}

/// Whether the start of a file is text (or no text we know of), which is what a file must be to be listed.
fn is_text_file(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return true;
    };
    let mut head = Vec::new();
    if file.take(HEAD_BYTES as u64).read_to_end(&mut head).is_err() {
        return true;
    }
    head.is_empty() || decode_text(&head).is_some()
}

impl NotesService {
    // ---- the list ------------------------------------------------------------------------------------

    /// The loose files as rows of the list, the one opened last first. Nothing is read from them but their size and time.
    pub(super) fn loose_entries(&self, data: &NotesData) -> Vec<NoteEntry> {
        let unsaved = self.pending.ids();
        let mut files: Vec<&LooseFile> = data.loose.iter().collect();
        files.sort_by(|a, b| b.opened_ms.cmp(&a.opened_ms).then_with(|| a.id.cmp(&b.id)));
        files
            .into_iter()
            .map(|file| {
                let stamp = loose::stamp_of(file.abs());
                NoteEntry {
                    kind: EntryKind::File,
                    path: file.ui_path(),
                    title: file.name(),
                    snippet: String::new(),
                    modified_ms: stamp.map_or(0, |stamp| stamp.modified_ms),
                    size_bytes: stamp.map_or(file.stamp.size, |stamp| stamp.size),
                    pinned: data.is_pinned(&file.key()),
                    copied: false,
                    view: data.views.get(&file.key()).cloned(),
                    folder: Some(loose::folder_label(file.abs(), self.home.as_deref())),
                    missing: stamp.is_none(),
                    unsaved: unsaved.contains(&file.id),
                    shared: false,
                }
            })
            .collect()
    }

    /// Lists again, from the text kept for them, the loose files that `data` lost (a damaged `notes.json`).
    pub(super) fn relist_unsaved(&self, data: &mut NotesData) {
        for id in self.pending.ids() {
            if !loose::is_id(&id) || data.loose.iter().any(|file| file.id == id) {
                continue;
            }
            let Some(edits) = self.pending.load(&id) else {
                continue;
            };
            if edits.path.is_empty()
                || data
                    .loose
                    .iter()
                    .any(|file| loose::same_path(&file.path, &edits.path))
            {
                continue;
            }
            data.loose.push(LooseFile {
                id,
                path: edits.path,
                opened_ms: edits.saved_ms,
                stamp: Stamp::default(),
            });
        }
    }

    /// The loose file a UI path belongs to.
    pub(super) fn loose_file(&self, path: &str) -> Result<LooseFile, NotesError> {
        let id = loose::id_of(path).ok_or_else(|| NotesError::NotANote(path.to_owned()))?;
        lock(&self.state)
            .data
            .loose
            .iter()
            .find(|file| file.id == id)
            .cloned()
            .ok_or_else(|| NotesError::NotANote(path.to_owned()))
    }

    // ---- opening and removing --------------------------------------------------------------------------

    /// Opens a file: a note when it is one of the notes folder, else a loose file, which is listed (or listed again, at the
    /// top of the files). Fails for what is not a file, and for binary files.
    pub fn open_file(&self, path: &Path, now_ms: u64) -> Result<OpenedFile, NotesError> {
        self.require_writable()?;
        let shown = path.display().to_string();
        let canonical = dunce::canonicalize(path).map_err(|e| NotesError::io(&shown, e))?;
        let meta = std::fs::metadata(&canonical).map_err(|e| NotesError::io(&shown, e))?;
        let name = canonical
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if !meta.is_file() {
            return Err(NotesError::NotAFile(name));
        }
        // a note of the notes folder is opened as a note
        if let Some(rest) = crate::vault::strip_root(&canonical, self.root()) {
            if rest.len() == 1 && !name.starts_with('.') && is_note_name(&name) {
                return Ok(OpenedFile {
                    kind: EntryKind::Note,
                    path: name,
                });
            }
        }
        if !is_text_file(&canonical) {
            return Err(NotesError::BinaryFile(name));
        }
        let abs = canonical.display().to_string();
        let stamp = loose::stamp_of(&canonical).unwrap_or_default();
        let ui_path = {
            let mut state = lock(&self.state);
            let known = state
                .data
                .loose
                .iter()
                .position(|file| loose::same_path(&file.path, &abs));
            let file = if let Some(at) = known {
                let file = &mut state.data.loose[at];
                file.opened_ms = now_ms;
                file.stamp = stamp;
                file.clone()
            } else {
                let id = loose::new_id(&abs, now_ms, |id| {
                    state.data.loose.iter().any(|file| file.id == id)
                });
                let file = LooseFile {
                    id,
                    path: abs,
                    opened_ms: now_ms,
                    stamp,
                };
                state.data.loose.push(file.clone());
                file
            };
            self.prune_files(&mut state);
            file.ui_path()
        };
        self.persist();
        Ok(OpenedFile {
            kind: EntryKind::File,
            path: ui_path,
        })
    }

    /// Drops the oldest loose files past the limit, keeping the pinned ones and the ones with text that was not saved.
    fn prune_files(&self, state: &mut State) {
        let pinned: BTreeSet<String> = state.data.pins.iter().cloned().collect();
        let unsaved = self.pending.ids();
        let before: Vec<LooseFile> = state.data.loose.clone();
        loose::prune(&mut state.data.loose, |file| {
            pinned.contains(&file.key()) || unsaved.contains(&file.id)
        });
        let gone: Vec<&LooseFile> = before
            .iter()
            .filter(|file| !state.data.loose.iter().any(|kept| kept.id == file.id))
            .collect();
        for file in gone {
            state.data.forget_loose(&file.key());
            state.seen.remove(&file.id);
        }
    }

    /// A loose file was opened in the window: it is the one to come back to, and goes to the top of the files.
    pub(super) fn loose_opened(&self, path: &str) {
        let Some(id) = loose::id_of(path) else {
            return;
        };
        {
            let mut state = lock(&self.state);
            let Some(file) = state.data.loose.iter_mut().find(|file| file.id == id) else {
                return;
            };
            file.opened_ms = crate::util::now_ms();
            let key = file.key();
            state.data.open = Some(key);
        }
        self.persist();
    }

    /// Takes a file out of the list. The file itself is never touched; the text typed into it and not saved goes (the person
    /// was asked).
    pub fn remove_file(&self, path: &str) -> Result<(), NotesError> {
        self.require_writable()?;
        let file = self.loose_file(path)?;
        {
            let mut state = lock(&self.state);
            state.data.loose.retain(|listed| listed.id != file.id);
            state.data.forget_loose(&file.key());
            state.seen.remove(&file.id);
        }
        self.pending.clear(&file.id);
        self.persist();
        Ok(())
    }

    /// Points a listed file at another file ("Locate..."): its place in the list, its pin and its unsaved text stay with it.
    pub fn locate_file(
        &self,
        path: &str,
        new_path: &Path,
        now_ms: u64,
    ) -> Result<OpenedFile, NotesError> {
        self.require_writable()?;
        let file = self.loose_file(path)?;
        let shown = new_path.display().to_string();
        let canonical = dunce::canonicalize(new_path).map_err(|e| NotesError::io(&shown, e))?;
        let meta = std::fs::metadata(&canonical).map_err(|e| NotesError::io(&shown, e))?;
        let name = canonical
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if !meta.is_file() {
            return Err(NotesError::NotAFile(name));
        }
        if !is_text_file(&canonical) {
            return Err(NotesError::BinaryFile(name));
        }
        let abs = canonical.display().to_string();
        let stamp = loose::stamp_of(&canonical).unwrap_or_default();
        let ui_path = {
            let mut state = lock(&self.state);
            if state
                .data
                .loose
                .iter()
                .any(|other| other.id != file.id && loose::same_path(&other.path, &abs))
            {
                return Err(NotesError::Exists(name));
            }
            let Some(entry) = state.data.loose.iter_mut().find(|e| e.id == file.id) else {
                return Err(NotesError::NotANote(path.to_owned()));
            };
            entry.path = abs;
            entry.opened_ms = now_ms;
            entry.stamp = stamp;
            let ui_path = entry.ui_path();
            state.seen.remove(&file.id);
            ui_path
        };
        self.persist();
        Ok(OpenedFile {
            kind: EntryKind::File,
            path: ui_path,
        })
    }

    // ---- what is on disk -------------------------------------------------------------------------------

    /// What a UI path of a loose file means: the file's own scope.
    pub fn loose_doc(&self, path: &str) -> Result<LooseDoc, NotesError> {
        let (id, rest) = loose::parse(path).ok_or_else(|| NotesError::NotANote(path.to_owned()))?;
        let file = self.loose_file(path)?;
        let vault = Vault::open(file.abs())?;
        let name = vault
            .initial_file()
            .map(str::to_owned)
            .ok_or_else(|| NotesError::NotANote(path.to_owned()))?;
        let asset = if loose::same_path(rest, &name) {
            None
        } else if rest.contains(['/', '\\']) {
            return Err(VaultError::Escapes(path.to_owned()).into());
        } else {
            Some(rest.to_owned())
        };
        Ok(LooseDoc {
            id: id.to_owned(),
            ui_path: loose::ui_path(id, &name),
            vault,
            name,
            asset,
        })
    }

    /// The last known place of a loose file, when `path` names that file and the file is not there.
    ///
    /// "Copy path" still has the place the file was opened at. That place was made canonical then, and it is
    /// returned only when it is still that file: an absolute path, with no `..`, whose name is the file's name.
    /// A different name is refused. `Ok(None)` when `path` is not a loose file, or the file is still there —
    /// the ordinary resolve is the one to use then.
    pub fn remembered_file(&self, path: &str) -> Result<Option<PathBuf>, NotesError> {
        let Some((id, rest)) = loose::parse(path) else {
            return Ok(None);
        };
        if rest.contains(['/', '\\']) || rest == "." || rest == ".." {
            return Err(VaultError::Escapes(path.to_owned()).into());
        }
        let file = self.loose_file(path)?;
        // A different name is not this file: an image next to it, or another file, is not copied as if it were.
        if file.id != id || !loose::same_path(rest, &file.name()) {
            return Err(NotesError::NotANote(path.to_owned()));
        }
        let stored = file.abs().to_path_buf();
        if !recorded_file(&stored, &file.name()) {
            return Err(NotesError::NotANote(path.to_owned()));
        }
        if stored.is_file() {
            return Ok(None);
        }
        Ok(Some(stored))
    }

    /// The file `id` was read: this is the version the person sees, and a different one on disk later is a change.
    pub fn mark_seen(&self, id: &str) {
        let mut state = lock(&self.state);
        let Some(file) = state.data.loose.iter_mut().find(|file| file.id == id) else {
            return;
        };
        if let Some(stamp) = loose::stamp_of(file.abs()) {
            // the stamp the file is looked for by when it is renamed is the one it has now (kept the next time notes.json is)
            file.stamp = stamp;
            state.seen.insert(id.to_owned(), stamp);
        }
    }

    /// Looks at every listed file: which ones are as they were read, which changed since, and which are gone. A file that is
    /// gone and is found again under another name in its folder is followed.
    pub fn loose_status(&self) -> Vec<LooseStatus> {
        let files: Vec<LooseFile> = lock(&self.state).data.loose.clone();
        let mut followed: Vec<(String, PathBuf)> = Vec::new();
        let mut statuses = Vec::new();
        for file in &files {
            if let Some(stamp) = loose::stamp_of(file.abs()) {
                let seen = lock(&self.state).seen.get(&file.id).copied();
                let state = if seen.is_some_and(|seen| seen != stamp) {
                    LooseState::Changed
                } else {
                    LooseState::Same
                };
                statuses.push(LooseStatus {
                    path: file.ui_path(),
                    state,
                    stamp: loose::stamp_label(stamp),
                    was: None,
                });
                continue;
            }
            let found = loose::find_renamed(file, |candidate| {
                let candidate = candidate.display().to_string();
                files
                    .iter()
                    .any(|other| loose::same_path(&other.path, &candidate))
            });
            match found {
                Some(renamed) => {
                    let was = file.ui_path();
                    followed.push((file.id.clone(), renamed.clone()));
                    let name = renamed
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    statuses.push(LooseStatus {
                        path: loose::ui_path(&file.id, &name),
                        state: LooseState::Same,
                        stamp: loose::stamp_of(&renamed)
                            .map_or_else(String::new, loose::stamp_label),
                        was: Some(was),
                    });
                }
                None => statuses.push(LooseStatus {
                    path: file.ui_path(),
                    state: LooseState::Missing,
                    stamp: String::new(),
                    was: None,
                }),
            }
        }
        if !followed.is_empty() && self.can_write() {
            {
                let mut state = lock(&self.state);
                for (id, renamed) in &followed {
                    if let Some(entry) = state.data.loose.iter_mut().find(|e| e.id == *id) {
                        entry.path = renamed.display().to_string();
                    }
                }
            }
            self.persist();
        }
        statuses
    }

    // ---- saving, when the person asks ---------------------------------------------------------------------

    /// Writes `text` over the loose file, unless the file is not the version with hash `expected_hash`: then nothing is
    /// written and the outcome says so. The text kept for the file (the unsaved edits) goes once it is in the file.
    pub fn save_file(
        &self,
        path: &str,
        text: &str,
        expected_hash: &str,
    ) -> Result<SaveOutcome, NotesError> {
        self.require_writable()?;
        // a file that is not there is a file someone else took away: nothing is written, and the editor is told
        if !self.loose_file(path)?.abs().is_file() {
            return Ok(SaveOutcome::Conflict { disk_hash: None });
        }
        let doc = self.loose_doc(path)?;
        if doc.asset.is_some() {
            return Err(NotesError::NotANote(path.to_owned()));
        }
        // the watcher of the notes does not see these files, so there is nothing to tell it
        let own = OwnWrites::new();
        let outcome =
            Editor::new(&doc.vault, &own)
                .with_text_files()
                .save(&doc.name, text, expected_hash)?;
        if matches!(outcome, SaveOutcome::Saved { .. }) {
            self.pending.clear(&doc.id);
            self.mark_seen(&doc.id);
            self.refresh_stamp(&doc.id);
        }
        Ok(outcome)
    }

    /// Writes the file again where it was, with `text`, when it is gone ("Save it again"). Nothing is overwritten.
    pub fn recreate_file(&self, path: &str, text: &str) -> Result<CreatedDoc, NotesError> {
        self.require_writable()?;
        let file = self.loose_file(path)?;
        let (Some(folder), name) = (file.abs().parent(), file.name()) else {
            return Err(NotesError::NotANote(path.to_owned()));
        };
        let vault = Vault::open(folder)?;
        let own = OwnWrites::new();
        let created = Editor::new(&vault, &own)
            .with_text_files()
            .create(&name, text)?;
        self.pending.clear(&file.id);
        self.mark_seen(&file.id);
        self.refresh_stamp(&file.id);
        Ok(CreatedDoc {
            path: file.ui_path(),
            ..created
        })
    }

    /// The file `id` was just written: it looks like this now.
    fn refresh_stamp(&self, id: &str) {
        {
            let mut state = lock(&self.state);
            let Some(file) = state.data.loose.iter_mut().find(|file| file.id == id) else {
                return;
            };
            if let Some(stamp) = loose::stamp_of(file.abs()) {
                file.stamp = stamp;
            }
        }
        self.persist();
    }

    // ---- text typed and not saved --------------------------------------------------------------------------

    /// Keeps the text typed into a loose file, with the hash of the version of the file it was typed over.
    pub fn save_pending(
        &self,
        path: &str,
        text: &str,
        base_hash: &str,
        now_ms: u64,
    ) -> Result<(), NotesError> {
        self.require_writable()?;
        let file = self.loose_file(path)?;
        if text.len() as u64 > MAX_RENDER_BYTES {
            return Err(NotesError::Edit(EditError::TooLarge(file.name())));
        }
        self.pending
            .save(
                &file.id,
                &PendingEdits {
                    path: file.path.clone(),
                    base_hash: base_hash.to_owned(),
                    text: text.to_owned(),
                    saved_ms: now_ms,
                },
            )
            .map_err(|e| NotesError::io(&file.name(), e))
    }

    /// The text kept for a loose file, if any.
    pub fn load_pending(&self, path: &str) -> Result<Option<PendingEdits>, NotesError> {
        let file = self.loose_file(path)?;
        Ok(self.pending.load(&file.id))
    }

    /// Forgets the text kept for a loose file (it was saved, or undone, or given up).
    pub fn clear_pending(&self, path: &str) -> Result<(), NotesError> {
        self.require_writable()?;
        let file = self.loose_file(path)?;
        self.pending.clear(&file.id);
        Ok(())
    }

    /// Makes a note of the text typed into a loose file, "{name} (my edits)", for the person who takes the file's own version
    /// instead: nothing typed is lost. The text kept for the file goes. Returns the note's name.
    pub fn keep_edits_as_note(&self, path: &str, text: &str) -> Result<String, NotesError> {
        self.require_writable()?;
        let file = self.loose_file(path)?;
        let stem = file
            .abs()
            .file_stem()
            .map_or_else(|| file.name(), |stem| stem.to_string_lossy().into_owned());
        let note = self.create_copy(&edits_stem(&stem), text)?;
        self.pending.clear(&file.id);
        Ok(note)
    }
}
