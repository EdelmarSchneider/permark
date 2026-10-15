//! `notes.json`: reading and writing the notes of a project.
//!
//! Every change reads the file again, changes it and writes it back (atomically), under a lock, so two parts of
//! this program never overwrite each other and a change made by `trailmark mcp` while the window was closed is
//! not lost. The lock is a `notes.lock` file next to the notes, because the window and `trailmark mcp` are two
//! programs. A file that is not valid notes is set aside, not overwritten: the notes in it are the person's. A file
//! that merely cannot be read right now is left alone, and the change fails.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::session::{open_lock_file, write_local_state};
use crate::util::{now_ms, read_with_retry};

use super::{Note, NoteError};

/// The version of the file's layout. A file written by a newer version is set aside rather than rewritten.
const VERSION: u32 = 1;
/// How long a change waits for another program to finish its own. A change takes a few milliseconds.
const LOCK_WAIT: Duration = Duration::from_secs(10);
/// How often it looks while waiting.
const LOCK_POLL: Duration = Duration::from_millis(10);

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
    /// There is a file, and what is in it is not notes this version can use.
    Unusable,
}

/// The file's notes. A file that cannot be read at all (as opposed to one that was read and is no good) is an error:
/// it may be fine a moment later.
fn read(path: &Path) -> std::io::Result<Read> {
    let bytes = match read_with_retry(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Read::Missing),
        Err(error) => return Err(error),
    };
    Ok(match serde_json::from_slice::<Contents>(&bytes) {
        Ok(contents) if contents.version <= VERSION => Read::Notes(contents.notes),
        _ => Read::Unusable,
    })
}

/// The lock file of the notes file `path`: `notes.lock` for `notes.json`.
fn lock_path(path: &Path) -> PathBuf {
    path.with_extension("lock")
}

/// Takes the exclusive lock on `lock` (a file for that, see [`open_lock_file`]), waiting up to `wait` for another
/// program to let go. It is released when the returned file is dropped, or when the program ends.
fn acquire(lock: &Path, wait: Duration) -> std::io::Result<std::fs::File> {
    let file = open_lock_file(lock)?;
    let deadline = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() >= deadline => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "another program is changing the notes",
                ))
            }
            Err(std::fs::TryLockError::WouldBlock) => std::thread::sleep(LOCK_POLL),
        }
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
            Ok(Read::Notes(notes)) => notes,
            Ok(Read::Missing | Read::Unusable) | Err(_) => Vec::new(),
        }
    }

    /// Changes the notes. `change` says whether it changed anything; only then is the file written.
    pub(crate) fn update<T>(
        &self,
        change: impl FnOnce(&mut Vec<Note>) -> Result<(T, bool), NoteError>,
    ) -> Result<T, NoteError> {
        let path = lock(&self.path);
        // Other programs (the window, `trailmark mcp`) change the same file: nobody reads or writes it meanwhile.
        let _locked =
            acquire(&lock_path(&path), LOCK_WAIT).map_err(|error| io_error(&path, error))?;
        let mut notes = match read(&path).map_err(|error| io_error(&path, error))? {
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

    /// A file that cannot be read for now (an antivirus scan, a sync client, a disk hiccup) is not a file that is
    /// no good: the notes in it are still there when it can be read again.
    #[test]
    fn a_file_that_cannot_be_read_is_an_error_and_is_never_set_aside() {
        let (dir, store) = store();
        let path = dir.path().join("local").join("notes.json");
        // a folder where the file should be: reading it fails, and it is not a damaged file
        std::fs::create_dir_all(&path).expect("mkdir");
        let result = store.update(|notes| {
            notes.push(note("n1"));
            Ok(((), true))
        });
        assert!(matches!(result, Err(NoteError::Io { .. })), "{result:?}");
        assert!(path.is_dir(), "left where it was");
        let names: Vec<_> = std::fs::read_dir(path.parent().expect("parent"))
            .expect("ls")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|name| !name.contains("unreadable")),
            "nothing was renamed: {names:?}"
        );
        assert!(store.load().is_empty(), "nothing to show meanwhile");
    }

    /// The window and `trailmark mcp` are two programs: each change reads the file again under a lock they share.
    #[test]
    fn writers_in_different_programs_do_not_lose_each_others_changes() {
        const WRITERS: usize = 4;
        const NOTES_EACH: usize = 15;
        let (dir, _store) = store();
        let path = dir.path().join("local").join("notes.json");
        let handles: Vec<_> = (0..WRITERS)
            .map(|writer| {
                // a store of its own, as another program has: they share nothing but the file
                let store = NoteStore::new(path.clone());
                std::thread::spawn(move || {
                    for n in 0..NOTES_EACH {
                        store
                            .update(|notes| {
                                notes.push(note(&format!("w{writer}n{n}")));
                                Ok(((), true))
                            })
                            .expect("update");
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("writer");
        }
        assert_eq!(NoteStore::new(path).load().len(), WRITERS * NOTES_EACH);
    }

    #[test]
    fn a_lock_that_stays_taken_is_given_up_after_the_wait() {
        let (dir, _store) = store();
        let path = dir.path().join("local").join("notes.json");
        let lock = lock_path(&path);
        let held = acquire(&lock, Duration::ZERO).expect("free");
        let error = acquire(&lock, Duration::ZERO).expect_err("taken");
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        drop(held);
        acquire(&lock, Duration::ZERO).expect("free again");
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
