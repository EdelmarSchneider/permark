//! `notes.json`: reading and writing the notes of a project.
//!
//! Every change reads the file again, changes it and writes it back (atomically), under a lock, so two parts of
//! this program never overwrite each other and a change made by `trailmark mcp` while the window was closed is
//! not lost. A file that cannot be read is set aside, not overwritten: the notes in it are the person's.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

use crate::session::write_local_state;
use crate::util::now_ms;

use super::{Note, NoteError};

/// The version of the file's layout. A file written by a newer version is set aside rather than rewritten.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Contents {
    version: u32,
    notes: Vec<Note>,
}

/// What reading the file found.
enum Read {
    /// There is no file yet.
    Missing,
    Notes(Vec<Note>),
    /// There is a file this version cannot use.
    Unusable,
}

fn read(path: &Path) -> Read {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Read::Missing,
        Err(_) => return Read::Unusable,
    };
    match serde_json::from_slice::<Contents>(&bytes) {
        Ok(contents) if contents.version <= VERSION => Read::Notes(contents.notes),
        _ => Read::Unusable,
    }
}

fn io_error(path: &Path, source: std::io::Error) -> NoteError {
    NoteError::Io {
        path: path.display().to_string(),
        source,
    }
}

pub(crate) struct NoteStore {
    path: Mutex<PathBuf>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl NoteStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path: Mutex::new(path),
        }
    }

    /// The project keeps its state somewhere else from now on (it was initialized).
    pub(crate) fn relocate(&self, path: PathBuf) {
        *lock(&self.path) = path;
    }

    /// The notes, in the order they were made. A file that cannot be read has no notes to show; it is left alone.
    pub(crate) fn load(&self) -> Vec<Note> {
        let path = lock(&self.path);
        match read(&path) {
            Read::Notes(notes) => notes,
            Read::Missing | Read::Unusable => Vec::new(),
        }
    }

    /// Changes the notes. `change` says whether it changed anything; only then is the file written.
    pub(crate) fn update<T>(
        &self,
        change: impl FnOnce(&mut Vec<Note>) -> Result<(T, bool), NoteError>,
    ) -> Result<T, NoteError> {
        let path = lock(&self.path);
        let mut notes = match read(&path) {
            Read::Notes(notes) => notes,
            Read::Missing => Vec::new(),
            Read::Unusable => {
                set_aside(&path)?;
                Vec::new()
            }
        };
        let (value, changed) = change(&mut notes)?;
        if changed {
            let bytes = serde_json::to_vec_pretty(&Contents {
                version: VERSION,
                notes,
            })
            .map_err(|error| io_error(&path, std::io::Error::other(error)))?;
            write_local_state(&path, &bytes).map_err(|error| io_error(&path, error))?;
        }
        Ok(value)
    }
}

/// Renames a file this version cannot read to `notes.unreadable-<time>.json` next to it, so a new file can be written
/// and the old one can be recovered by hand.
fn set_aside(path: &Path) -> Result<(), NoteError> {
    let stem = path.file_stem().map_or_else(
        || "notes".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let target = path.with_file_name(format!("{stem}.unreadable-{}.json", now_ms()));
    std::fs::rename(path, &target).map_err(|error| io_error(path, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{Anchor, DoneBy};

    fn note(id: &str) -> Note {
        Note {
            id: id.to_owned(),
            path: "docs/a.md".to_owned(),
            text: "text".to_owned(),
            anchor: Anchor {
                quote: "quote".to_owned(),
                line: 3,
                end_line: 3,
                ..Anchor::default()
            },
            created_ms: 10,
            done: None,
        }
    }

    fn store() -> (tempfile::TempDir, NoteStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = NoteStore::new(dir.path().join("local").join("notes.json"));
        (dir, store)
    }

    #[test]
    fn a_project_without_a_file_has_no_notes() {
        let (_dir, store) = store();
        assert!(store.load().is_empty(), "nothing to show");
    }

    #[test]
    fn notes_are_written_where_asked_and_read_back() {
        let (dir, store) = store();
        store
            .update(|notes| {
                notes.push(note("n1"));
                Ok(((), true))
            })
            .expect("update");
        let path = dir.path().join("local").join("notes.json");
        let text = std::fs::read_to_string(&path).expect("the file exists");
        assert!(text.contains("\"version\": 1"), "{text}");
        assert!(text.contains("\"headingPath\""), "{text}");
        assert_eq!(store.load(), vec![note("n1")]);
    }

    #[test]
    fn nothing_is_written_when_nothing_changed() {
        let (dir, store) = store();
        store.update(|_| Ok(((), false))).expect("update");
        assert!(!dir.path().join("local").join("notes.json").exists());
    }

    #[test]
    fn an_error_in_the_change_leaves_the_file_as_it_was() {
        let (_dir, store) = store();
        store
            .update(|notes| {
                notes.push(note("n1"));
                Ok(((), true))
            })
            .expect("update");
        let failed: Result<(), _> = store.update(|notes| {
            notes.clear();
            Err(NoteError::Empty)
        });
        assert!(failed.is_err());
        assert_eq!(store.load().len(), 1);
    }

    #[test]
    fn a_change_made_by_another_program_is_not_overwritten() {
        let (dir, store) = store();
        store
            .update(|notes| {
                notes.push(note("n1"));
                Ok(((), true))
            })
            .expect("update");
        // `trailmark mcp` resolved the note while the window was closed
        let other = NoteStore::new(dir.path().join("local").join("notes.json"));
        other
            .update(|notes| {
                notes[0].done = Some(crate::notes::Done {
                    at_ms: 5,
                    by: DoneBy::Agent,
                    reply: Some("done".to_owned()),
                });
                Ok(((), true))
            })
            .expect("update");
        store
            .update(|notes| {
                notes.push(note("n2"));
                Ok(((), true))
            })
            .expect("update");
        let notes = store.load();
        assert_eq!(notes.len(), 2);
        assert_eq!(
            notes[0].done.as_ref().and_then(|d| d.reply.as_deref()),
            Some("done")
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_is_set_aside_not_overwritten() {
        let (dir, store) = store();
        let path = dir.path().join("local").join("notes.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "{ not json").expect("write");
        // reading shows nothing and touches nothing
        assert!(store.load().is_empty(), "nothing to show");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "{ not json");
        // the first change keeps the old file under another name and starts a new one
        store
            .update(|notes| {
                notes.push(note("n1"));
                Ok(((), true))
            })
            .expect("update");
        assert_eq!(store.load().len(), 1);
        let kept: Vec<_> = std::fs::read_dir(path.parent().expect("parent"))
            .expect("ls")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("notes.unreadable-"))
            .collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        let old = std::fs::read_to_string(path.with_file_name(&kept[0])).expect("read");
        assert_eq!(old, "{ not json");
    }

    #[test]
    fn a_file_from_a_newer_version_is_set_aside_too() {
        let (dir, store) = store();
        let path = dir.path().join("local").join("notes.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, r#"{"version": 99, "notes": []}"#).expect("write");
        assert!(store.load().is_empty(), "nothing to show");
        store
            .update(|notes| {
                notes.push(note("n1"));
                Ok(((), true))
            })
            .expect("update");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("\"version\": 1"));
    }

    #[test]
    fn notes_are_not_written_behind_a_link() {
        use crate::util::testing::{dir_link, remove_link};

        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        let trailmark = dir.path().join(".trailmark");
        std::fs::create_dir(&trailmark).expect("mkdir");
        dir_link(&trailmark.join("local"), outside.path());
        let store = NoteStore::new(trailmark.join("local").join("notes.json"));
        let result = store.update(|notes| {
            notes.push(note("n1"));
            Ok(((), true))
        });
        assert!(matches!(result, Err(NoteError::Io { .. })), "{result:?}");
        assert!(std::fs::read_dir(outside.path())
            .expect("ls")
            .next()
            .is_none());
        remove_link(&trailmark.join("local"));
    }

    #[test]
    fn the_store_follows_the_state_to_a_new_place() {
        let (dir, store) = store();
        let moved = dir.path().join("elsewhere").join("notes.json");
        store.relocate(moved.clone());
        store
            .update(|notes| {
                notes.push(note("n1"));
                Ok(((), true))
            })
            .expect("update");
        assert!(moved.is_file());
        assert!(!dir.path().join("local").join("notes.json").exists());
    }
}
