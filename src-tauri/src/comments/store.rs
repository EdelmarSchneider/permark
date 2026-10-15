//! `comments.json`: reading and writing the comments of a project.
//!
//! Every change reads the file again, changes it and writes it back (atomically), under a lock, so two parts of
//! this program never overwrite each other and a change made by `trailmark mcp` while the window was closed is
//! not lost. The lock is a `comments.lock` file next to the comments, because the window and `trailmark mcp` are two
//! programs. A file that is not valid comments is set aside, not overwritten: the comments in it are the person's. A file
//! that merely cannot be read right now is left alone, and the change fails.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::session::{open_lock_file, write_local_state};
use crate::util::{now_ms, read_with_retry};

use super::{Comment, CommentError};

/// The version of the file's layout. A file written by a newer version is set aside rather than rewritten.
const VERSION: u32 = 1;
/// How long a change waits for another program to finish its own. A change takes a few milliseconds.
const LOCK_WAIT: Duration = Duration::from_secs(10);
/// How often it looks while waiting.
const LOCK_POLL: Duration = Duration::from_millis(10);

#[derive(Serialize, Deserialize)]
struct Contents {
    version: u32,
    comments: Vec<Comment>,
}

/// What reading the file found.
enum Read {
    /// There is no file yet.
    Missing,
    Comments(Vec<Comment>),
    /// There is a file, and what is in it is not comments this version can use.
    Unusable,
}

/// The file's comments. A file that cannot be read at all (as opposed to one that was read and is no good) is an error:
/// it may be fine a moment later.
fn read(path: &Path) -> std::io::Result<Read> {
    let bytes = match read_with_retry(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Read::Missing),
        Err(error) => return Err(error),
    };
    Ok(match serde_json::from_slice::<Contents>(&bytes) {
        Ok(contents) if contents.version <= VERSION => Read::Comments(contents.comments),
        _ => Read::Unusable,
    })
}

/// The lock file of the comments file `path`: `comments.lock` for `comments.json`.
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
                    "another program is changing the comments",
                ))
            }
            Err(std::fs::TryLockError::WouldBlock) => std::thread::sleep(LOCK_POLL),
        }
    }
}

fn io_error(path: &Path, source: std::io::Error) -> CommentError {
    CommentError::Io {
        path: path.display().to_string(),
        source,
    }
}

pub(crate) struct CommentStore {
    path: Mutex<PathBuf>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl CommentStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path: Mutex::new(path),
        }
    }

    /// The project keeps its state somewhere else from now on (it was initialized).
    pub(crate) fn relocate(&self, path: PathBuf) {
        *lock(&self.path) = path;
    }

    /// The comments, in the order they were made. A file that cannot be read has no comments to show; it is left alone.
    pub(crate) fn load(&self) -> Vec<Comment> {
        let path = lock(&self.path);
        match read(&path) {
            Ok(Read::Comments(comments)) => comments,
            Ok(Read::Missing | Read::Unusable) | Err(_) => Vec::new(),
        }
    }

    /// What is wrong with the file when that is why [`CommentStore::load`] has no comments to show; `None` when the
    /// file is fine, or there is none and never was.
    pub(crate) fn problem(&self) -> Option<Problem> {
        let path = lock(&self.path);
        match read(&path) {
            Ok(Read::Comments(_)) => None,
            Ok(Read::Unusable) => Some(Problem::Damaged),
            Ok(Read::Missing) => set_aside_exists(&path).then_some(Problem::SetAside),
            Err(_) => Some(Problem::Unreadable),
        }
    }

    /// Changes the comments. `change` says whether it changed anything; only then is the file written.
    pub(crate) fn update<T>(
        &self,
        change: impl FnOnce(&mut Vec<Comment>) -> Result<(T, bool), CommentError>,
    ) -> Result<T, CommentError> {
        let path = lock(&self.path);
        // Other programs (the window, `trailmark mcp`) change the same file: nobody reads or writes it meanwhile.
        let _locked =
            acquire(&lock_path(&path), LOCK_WAIT).map_err(|error| io_error(&path, error))?;
        let mut comments = match read(&path).map_err(|error| io_error(&path, error))? {
            Read::Comments(comments) => comments,
            Read::Missing => Vec::new(),
            Read::Unusable => {
                set_aside(&path)?;
                Vec::new()
            }
        };
        let (value, changed) = change(&mut comments)?;
        if changed {
            let bytes = serde_json::to_vec_pretty(&Contents {
                version: VERSION,
                comments,
            })
            .map_err(|error| io_error(&path, std::io::Error::other(error)))?;
            write_local_state(&path, &bytes).map_err(|error| io_error(&path, error))?;
        }
        Ok(value)
    }
}

/// What is wrong with the comments file, for a list that is empty because of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Problem {
    /// The file is there and holds nothing this version can use. The next change sets it aside.
    Damaged,
    /// A file like that was set aside (see [`set_aside`]) and no comment has been saved since.
    SetAside,
    /// The file could not be read just now; it may be fine a moment later.
    Unreadable,
}

/// The name a file set aside starts with: `comments.unreadable-` for `comments.json`.
fn set_aside_prefix(path: &Path) -> String {
    let stem = path.file_stem().map_or_else(
        || "comments".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    format!("{stem}.unreadable-")
}

/// Renames a file this version cannot read to `comments.unreadable-<time>.json` next to it, so a new file can be written
/// and the old one can be recovered by hand.
fn set_aside(path: &Path) -> Result<(), CommentError> {
    let target = path.with_file_name(format!("{}{}.json", set_aside_prefix(path), now_ms()));
    std::fs::rename(path, &target).map_err(|error| io_error(path, error))
}

/// Whether a file was set aside next to `path` (see [`set_aside`]).
fn set_aside_exists(path: &Path) -> bool {
    let Some(folder) = path.parent() else {
        return false;
    };
    let prefix = set_aside_prefix(path);
    std::fs::read_dir(folder).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(".json")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::{Anchor, DoneBy};

    fn comment(id: &str) -> Comment {
        Comment {
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

    fn store() -> (tempfile::TempDir, CommentStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CommentStore::new(dir.path().join("local").join("comments.json"));
        (dir, store)
    }

    #[test]
    fn a_project_without_a_file_has_no_comments() {
        let (_dir, store) = store();
        assert!(store.load().is_empty(), "nothing to show");
    }

    #[test]
    fn comments_are_written_where_asked_and_read_back() {
        let (dir, store) = store();
        store
            .update(|comments| {
                comments.push(comment("c1"));
                Ok(((), true))
            })
            .expect("update");
        let path = dir.path().join("local").join("comments.json");
        let text = std::fs::read_to_string(&path).expect("the file exists");
        assert!(text.contains("\"version\": 1"), "{text}");
        assert!(text.contains("\"headingPath\""), "{text}");
        assert_eq!(store.load(), vec![comment("c1")]);
    }

    #[test]
    fn nothing_is_written_when_nothing_changed() {
        let (dir, store) = store();
        store.update(|_| Ok(((), false))).expect("update");
        assert!(!dir.path().join("local").join("comments.json").exists());
    }

    #[test]
    fn an_error_in_the_change_leaves_the_file_as_it_was() {
        let (_dir, store) = store();
        store
            .update(|comments| {
                comments.push(comment("c1"));
                Ok(((), true))
            })
            .expect("update");
        let failed: Result<(), _> = store.update(|comments| {
            comments.clear();
            Err(CommentError::Empty)
        });
        assert!(failed.is_err());
        assert_eq!(store.load().len(), 1);
    }

    #[test]
    fn a_change_made_by_another_program_is_not_overwritten() {
        let (dir, store) = store();
        store
            .update(|comments| {
                comments.push(comment("c1"));
                Ok(((), true))
            })
            .expect("update");
        // `trailmark mcp` resolved the comment while the window was closed
        let other = CommentStore::new(dir.path().join("local").join("comments.json"));
        other
            .update(|comments| {
                comments[0].done = Some(crate::comments::Done {
                    at_ms: 5,
                    by: DoneBy::Agent,
                    reply: Some("done".to_owned()),
                });
                Ok(((), true))
            })
            .expect("update");
        store
            .update(|comments| {
                comments.push(comment("c2"));
                Ok(((), true))
            })
            .expect("update");
        let comments = store.load();
        assert_eq!(comments.len(), 2);
        assert_eq!(
            comments[0].done.as_ref().and_then(|d| d.reply.as_deref()),
            Some("done")
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_is_set_aside_not_overwritten() {
        let (dir, store) = store();
        let path = dir.path().join("local").join("comments.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "{ not json").expect("write");
        // reading shows nothing and touches nothing
        assert!(store.load().is_empty(), "nothing to show");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "{ not json");
        // the first change keeps the old file under another name and starts a new one
        store
            .update(|comments| {
                comments.push(comment("c1"));
                Ok(((), true))
            })
            .expect("update");
        assert_eq!(store.load().len(), 1);
        let kept: Vec<_> = std::fs::read_dir(path.parent().expect("parent"))
            .expect("ls")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("comments.unreadable-"))
            .collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        let old = std::fs::read_to_string(path.with_file_name(&kept[0])).expect("read");
        assert_eq!(old, "{ not json");
    }

    #[test]
    fn what_is_wrong_with_the_file_is_told_apart_when_it_leaves_no_comments() {
        let (dir, store) = store();
        let path = dir.path().join("local").join("comments.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        // nothing yet: no problem
        assert_eq!(store.problem(), None);
        // a file of comments: no problem
        store
            .update(|comments| {
                comments.push(comment("c1"));
                Ok(((), true))
            })
            .expect("update");
        assert_eq!(store.problem(), None);
        // a file that is no good
        std::fs::write(&path, "{ not json").expect("write");
        assert_eq!(store.problem(), Some(Problem::Damaged));
        std::fs::write(&path, r#"{"version": 99, "comments": []}"#).expect("write");
        assert_eq!(store.problem(), Some(Problem::Damaged), "a newer version");
        // set aside by the next change, which leaves the list empty until a comment is saved
        store
            .update(|_| Ok(((), false)))
            .expect("a change that writes nothing still sets the file aside");
        assert!(!path.exists());
        assert_eq!(store.problem(), Some(Problem::SetAside));
        // and once a comment is saved there is a file again
        store
            .update(|comments| {
                comments.push(comment("c2"));
                Ok(((), true))
            })
            .expect("update");
        assert_eq!(store.problem(), None);
    }

    #[test]
    fn a_file_that_cannot_be_read_now_is_a_problem_of_its_own() {
        let (dir, store) = store();
        let path = dir.path().join("local").join("comments.json");
        // a folder where the file should be: reading it fails
        std::fs::create_dir_all(&path).expect("mkdir");
        assert_eq!(store.problem(), Some(Problem::Unreadable));
    }

    /// A file that cannot be read for now (an antivirus scan, a sync client, a disk hiccup) is not a file that is
    /// no good: the comments in it are still there when it can be read again.
    #[test]
    fn a_file_that_cannot_be_read_is_an_error_and_is_never_set_aside() {
        let (dir, store) = store();
        let path = dir.path().join("local").join("comments.json");
        // a folder where the file should be: reading it fails, and it is not a damaged file
        std::fs::create_dir_all(&path).expect("mkdir");
        let result = store.update(|comments| {
            comments.push(comment("c1"));
            Ok(((), true))
        });
        assert!(matches!(result, Err(CommentError::Io { .. })), "{result:?}");
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
        const COMMENTS_EACH: usize = 15;
        let (dir, _store) = store();
        let path = dir.path().join("local").join("comments.json");
        let handles: Vec<_> = (0..WRITERS)
            .map(|writer| {
                // a store of its own, as another program has: they share nothing but the file
                let store = CommentStore::new(path.clone());
                std::thread::spawn(move || {
                    for n in 0..COMMENTS_EACH {
                        store
                            .update(|comments| {
                                comments.push(comment(&format!("w{writer}n{n}")));
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
        assert_eq!(
            CommentStore::new(path).load().len(),
            WRITERS * COMMENTS_EACH
        );
    }

    #[test]
    fn a_lock_that_stays_taken_is_given_up_after_the_wait() {
        let (dir, _store) = store();
        let path = dir.path().join("local").join("comments.json");
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
        let path = dir.path().join("local").join("comments.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, r#"{"version": 99, "comments": []}"#).expect("write");
        assert!(store.load().is_empty(), "nothing to show");
        store
            .update(|comments| {
                comments.push(comment("c1"));
                Ok(((), true))
            })
            .expect("update");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("\"version\": 1"));
    }

    #[test]
    fn comments_are_not_written_behind_a_link() {
        use crate::util::testing::{dir_link, remove_link};

        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        let trailmark = dir.path().join(".trailmark");
        std::fs::create_dir(&trailmark).expect("mkdir");
        dir_link(&trailmark.join("local"), outside.path());
        let store = CommentStore::new(trailmark.join("local").join("comments.json"));
        let result = store.update(|comments| {
            comments.push(comment("c1"));
            Ok(((), true))
        });
        assert!(matches!(result, Err(CommentError::Io { .. })), "{result:?}");
        assert!(std::fs::read_dir(outside.path())
            .expect("ls")
            .next()
            .is_none());
        remove_link(&trailmark.join("local"));
    }

    #[test]
    fn the_store_follows_the_state_to_a_new_place() {
        let (dir, store) = store();
        let moved = dir.path().join("elsewhere").join("comments.json");
        store.relocate(moved.clone());
        store
            .update(|comments| {
                comments.push(comment("c1"));
                Ok(((), true))
            })
            .expect("update");
        assert!(moved.is_file());
        assert!(!dir.path().join("local").join("comments.json").exists());
    }
}
