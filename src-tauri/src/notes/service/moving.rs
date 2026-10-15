//! Moving a note into a project (docs/design/v0.5.md section 4): the text is written where the person chose in the Save dialog,
//! and the note is removed from the notes (to the recycle bin, like a delete), and the copy is listed under **Files**. Undo
//! brings the note back and takes the copy away, unless the copy was changed since.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::super::names::{first_free, is_note_name, EXTENSION};
use super::super::text::{head_of, stem_from_title, title_of};
use super::{lock, MovedNote, NotesError, NotesService};
use crate::edit::{validate_file_name, EditError};
use crate::util::{hash_bytes, read_with_retry};
use crate::vault::{strip_root, VaultError};

/// A note that was moved. Matches the UI's `NoteMoved`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMoved {
    /// Where the copy is (a full path of the disk: the person chose it).
    pub destination: String,
    /// The path of the copy in the list of the files: the row the note became.
    pub file: String,
    /// Asks for the note back; none when the note is too big to keep for it.
    pub undo_id: Option<u64>,
}

/// What Undo did. Matches the UI's `MoveUndone`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveUndone {
    /// The note that is back.
    pub note: String,
    /// The copy in the project was taken away; false when it was changed since and was left there.
    pub removed: bool,
}

/// Creates the file `target`, which must not exist, and writes all of `bytes` to it.
fn write_new(target: &Path, bytes: &[u8]) -> Result<(), NotesError> {
    let name = target
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                NotesError::Exists(name.clone())
            } else {
                NotesError::io(&name, e)
            }
        })?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(e) = written {
        drop(file);
        let _ = std::fs::remove_file(target);
        return Err(NotesError::io(&name, e));
    }
    Ok(())
}

impl NotesService {
    /// The file name the Save dialog starts with for a note: made from its title, and not the name of a file that is in
    /// `folder` already (a number follows it when it is).
    pub fn suggest_name(&self, path: &str, folder: Option<&Path>) -> Result<String, NotesError> {
        let (name, abs) = self.resolve_note(path)?;
        let stem = stem_from_title(&title_of(&head_of(&abs))).unwrap_or_else(|| {
            Path::new(&name)
                .file_stem()
                .map_or_else(|| name.clone(), |stem| stem.to_string_lossy().into_owned())
        });
        Ok(match folder {
            Some(folder) => first_free(&stem, " ", |candidate| folder.join(candidate).exists()),
            None => format!("{stem}.{EXTENSION}"),
        })
    }

    /// Where a note is moved to, checked: a full path in a folder that is there and is not the notes folder, with a name a file
    /// can have and a Markdown extension (`.md` is added to a name without one).
    fn checked_destination(&self, destination: &Path) -> Result<PathBuf, NotesError> {
        if !destination.is_absolute() {
            return Err(
                VaultError::Invalid(format!("not a full path: {}", destination.display())).into(),
            );
        }
        let name = destination
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| VaultError::Invalid(destination.display().to_string()))?;
        let name = if is_note_name(&name) {
            name
        } else if Path::new(&name).extension().is_none() {
            format!("{name}.{EXTENSION}")
        } else {
            return Err(EditError::NotMarkdown(name).into());
        };
        validate_file_name(&name).map_err(EditError::InvalidName)?;
        let folder = destination
            .parent()
            .ok_or_else(|| VaultError::Invalid(destination.display().to_string()))?;
        let shown = folder.display().to_string();
        let folder = dunce::canonicalize(folder).map_err(|e| NotesError::io(&shown, e))?;
        if !folder.is_dir() {
            return Err(VaultError::NotFound(shown).into());
        }
        if strip_root(&folder, self.root()).is_some() {
            return Err(NotesError::MoveInside);
        }
        Ok(folder.join(name))
    }

    /// Moves a note into a project: its exact bytes are written to `destination` (a file that must not be there already), then
    /// the note goes to the recycle bin and the copy is listed under Files. A note that was pinned has its copy pinned.
    pub fn move_to(
        &self,
        path: &str,
        destination: &Path,
        now_ms: u64,
    ) -> Result<NoteMoved, NotesError> {
        self.require_writable()?;
        let (name, abs) = self.resolve_note(path)?;
        let target = self.checked_destination(destination)?;
        let bytes = read_with_retry(&abs).map_err(|e| NotesError::io(&name, e))?;
        let hash = hash_bytes(&bytes);
        write_new(&target, &bytes)?;
        // what was written is what the note holds, or nothing is taken from the notes
        if std::fs::read(&target)
            .map(|written| hash_bytes(&written))
            .ok()
            != Some(hash)
        {
            let _ = std::fs::remove_file(&target);
            return Err(NotesError::io(
                &name,
                std::io::Error::other("the copy is not what the note holds"),
            ));
        }
        let pinned = lock(&self.state).data.is_pinned(&name);
        let deleted = match self.delete(&[path.to_owned()]) {
            Ok(deleted) => deleted,
            Err(error) => {
                // the note stays where it is, and so does only one of the two
                let _ = std::fs::remove_file(&target);
                return Err(error);
            }
        };
        let file = self
            .open_file(&target, now_ms)
            .map(|opened| opened.path)
            .unwrap_or_default();
        if pinned && !file.is_empty() {
            let _ = self.set_pinned(&file, true);
        }
        if let Some(undo_id) = deleted.undo_id {
            lock(&self.state).moves.push(MovedNote {
                undo_id,
                destination: target.clone(),
                hash,
                file: file.clone(),
            });
        }
        Ok(NoteMoved {
            destination: target.display().to_string(),
            file,
            undo_id: deleted.undo_id,
        })
    }

    /// Undoes a move: the note is back, and the copy in the project is taken away if it is still what was written (one that
    /// was changed since stays, and stays listed).
    pub fn undo_move(&self, undo_id: u64) -> Result<MoveUndone, NotesError> {
        self.require_writable()?;
        let record = {
            let mut state = lock(&self.state);
            let at = state
                .moves
                .iter()
                .position(|moved| moved.undo_id == undo_id)
                .ok_or(NotesError::UnknownUndo)?;
            state.moves.remove(at)
        };
        let back = self.undo_delete(undo_id)?;
        let unchanged = read_with_retry(&record.destination)
            .is_ok_and(|bytes| hash_bytes(&bytes) == record.hash);
        let removed = unchanged
            && self
                .recycler
                .recycle(std::slice::from_ref(&record.destination))
                .is_ok();
        if removed {
            let _ = self.remove_file(&record.file);
        }
        Ok(MoveUndone {
            note: back.into_iter().next().unwrap_or_default(),
            removed,
        })
    }
}
