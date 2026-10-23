//! Moving a note into a project: its exact bytes are written where the person chose, then the note is removed from the
//! notes (to the recycle bin, like a delete). Undo brings the note back and takes the copy away, unless the copy was
//! changed since.

use std::path::{Path, PathBuf};

use permark_core::error::{ErrorKind, IpcError};
use permark_core::fsx;

use crate::folder::is_inside;
use crate::names::{first_free, is_note_name, EXTENSION};
use crate::service::{checked_name, lock, write_new, MovedNote, NotesService};
use crate::text::{head_of, stem_from_title, title_of};
use crate::types::{MoveUndone, Moved};

impl NotesService {
    /// The file name the dialog starts with for a note: made from its title, and not the name of a file that is in
    /// `folder` already (a number follows it when it is).
    ///
    /// # Errors
    /// Not a note there.
    pub fn suggest_name(&self, path: &str, folder: Option<&str>) -> Result<String, IpcError> {
        let ctx = self.ctx();
        let (name, abs) = Self::existing_note(&ctx, path)?;
        let stem = stem_from_title(&title_of(&head_of(&abs))).unwrap_or_else(|| {
            Path::new(&name)
                .file_stem()
                .map_or_else(|| name.clone(), |s| s.to_string_lossy().into_owned())
        });
        Ok(match folder.map(Path::new).filter(|f| f.is_dir()) {
            Some(folder) => first_free(&stem, " ", |candidate| folder.join(candidate).exists()),
            None => format!("{stem}.{EXTENSION}"),
        })
    }

    /// The place the copy goes, checked: a folder that is there and is not the notes folder, a name a file can have
    /// and a Markdown extension (`.md` is added to a name without one).
    fn checked_destination(&self, folder: &str, name: &str) -> Result<PathBuf, IpcError> {
        let ctx = self.ctx();
        let folder = Path::new(folder);
        if !folder.is_absolute() {
            return Err(IpcError::new(ErrorKind::InvalidPath).subject(folder.display().to_string()));
        }
        let shown = folder.display().to_string();
        let folder =
            permark_core::paths::canonicalize(folder).map_err(|e| IpcError::io(&shown, &e))?;
        if !folder.is_dir() {
            return Err(IpcError::new(ErrorKind::NotFound).subject(shown));
        }
        if is_inside(&folder, ctx.folder()) {
            return Err(IpcError::new(ErrorKind::IntoItself).subject(shown));
        }
        let name = checked_name(name, Some(EXTENSION))?;
        if !is_note_name(&name) {
            return Err(IpcError::new(ErrorKind::InvalidName).detail("notMarkdown"));
        }
        Ok(folder.join(name))
    }

    /// Moves a note into a project folder: its exact bytes are written to `folder/name` (a file that must not be there
    /// already), then the note goes to the recycle bin. A note that changed while it was copied stays too (`kept`).
    ///
    /// # Errors
    /// Not a note there | the destination is not acceptable | a file is there already | the system refused.
    pub fn move_to_project(&self, path: &str, folder: &str, name: &str) -> Result<Moved, IpcError> {
        self.move_after(path, folder, name, || {})
    }

    /// [`NotesService::move_to_project`] with `after_copy` run between the copy and the check that the note is still what
    /// was copied (tests let a writer in).
    pub(crate) fn move_after(
        &self,
        path: &str,
        folder: &str,
        name: &str,
        after_copy: impl FnOnce(),
    ) -> Result<Moved, IpcError> {
        let ctx = self.ctx();
        self.ready_folder(&ctx)?;
        let (note, abs) = Self::existing_note(&ctx, path)?;
        let target = self.checked_destination(folder, name)?;
        let bytes = fsx::retry(&fsx::RETRY_DELAYS, || std::fs::read(&abs))
            .map_err(|e| IpcError::io(&note, &e))?;
        let hash = fsx::hash(&bytes).hex();
        write_new(&target, &bytes).map_err(|e| IpcError::io(target.display().to_string(), &e))?;
        after_copy();
        // what was written is what the note holds, or nothing is taken from the notes
        let written = std::fs::read(&target).map(|b| fsx::hash(&b).hex()).ok();
        let now = std::fs::read(&abs).map(|b| fsx::hash(&b).hex()).ok();
        let destination = target.display().to_string();
        if written.as_deref() != Some(hash.as_str()) {
            let _ = std::fs::remove_file(&target);
            return Err(IpcError::new(ErrorKind::Io)
                .subject(note)
                .detail("the copy is not what the note holds"));
        }
        if now.as_deref() != Some(hash.as_str()) {
            return Ok(Moved {
                destination,
                undo_id: None,
                kept: true,
            });
        }
        let deleted = match self.delete(&[path.to_owned()]) {
            Ok(deleted) => deleted,
            Err(error) => {
                // the note stays where it is, and so does only one of the two
                let _ = std::fs::remove_file(&target);
                return Err(error);
            }
        };
        if let Some(undo_id) = deleted.undo_id {
            lock(&ctx.moves).push(MovedNote {
                undo_id,
                destination: target,
                hash,
            });
        }
        Ok(Moved {
            destination,
            undo_id: deleted.undo_id,
            kept: false,
        })
    }

    /// Undoes a move: the note is back, and the copy in the project is taken away if it is still what was written (one
    /// that was changed since stays).
    ///
    /// # Errors
    /// Unknown undo id | the note could not be put back.
    pub fn undo_move(&self, undo_id: u64) -> Result<MoveUndone, IpcError> {
        let ctx = self.ctx();
        let record = {
            let mut moves = lock(&ctx.moves);
            let at = moves
                .iter()
                .position(|m| m.undo_id == undo_id)
                .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject("undo"))?;
            moves.remove(at)
        };
        let back = match self.undo_delete(undo_id) {
            Ok(back) => back,
            Err(error) => {
                // nothing was put back, so the record stays: asking again must not find the move forgotten
                lock(&ctx.moves).push(record);
                return Err(error);
            }
        };
        let unchanged = std::fs::read(&record.destination)
            .is_ok_and(|bytes| fsx::hash(&bytes).hex() == record.hash);
        let removed = unchanged && fsx::recycle(std::slice::from_ref(&record.destination)).is_ok();
        Ok(MoveUndone {
            note: back.into_iter().next().unwrap_or_default(),
            removed,
        })
    }
}
