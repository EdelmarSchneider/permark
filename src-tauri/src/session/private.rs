//! The private state folders of a project, `.permark/local/` and `.permark/cache/`: what makes it safe to write
//! there.
//!
//! A repository decides what is in its `.permark/` folder, links included. A link in place of `.permark/` or
//! `local/` would send what PerMark writes there (the session, the comments, the agent's context) and the file
//! it deletes to wherever the link points, so a folder that is a link (a symbolic link, or a junction on Windows) is
//! never used. And the project's own `.gitignore` may or may not say that these folders are private, so each of them
//! ignores itself: a `.gitignore` with `*` inside keeps its contents out of the repository whatever the parent says.

use std::io::{self, Write};
use std::path::Path;

use crate::util::{write_atomic, write_atomic_unsynced};

const PERMARK_DIR: &str = ".permark";
/// What a state folder holds in its own `.gitignore`: everything, itself included.
const SELF_IGNORE: &str = "*\n";

/// What is on disk at a path, without following a link.
enum Entry {
    Missing,
    /// A real folder.
    Folder,
    /// A link, a file, or anything that cannot be looked at.
    Other,
}

fn entry(path: &Path) -> Entry {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Entry::Folder,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Entry::Missing,
        Ok(_) | Err(_) => Entry::Other,
    }
}

/// Whether `path` is a real folder, not a link to one.
pub(super) fn is_real_folder(path: &Path) -> bool {
    matches!(entry(path), Entry::Folder)
}

/// Whether a project can keep its state in `permark` (its `.permark` path): a real folder, whose `local/` is a
/// real folder or not made yet.
pub(super) fn usable_in(permark: &Path) -> bool {
    is_real_folder(permark) && !matches!(entry(&permark.join("local")), Entry::Other)
}

fn refused(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "{} is a link or not a folder, and PerMark keeps no files behind one",
            path.display()
        ),
    )
}

/// The `.permark` folder `dir` is in, when `dir` is one of its folders.
fn permark_of(dir: &Path) -> Option<&Path> {
    dir.parent().filter(|parent| {
        parent
            .file_name()
            .is_some_and(|name| name == PERMARK_DIR || name == super::legacy::LEGACY_STATE_DIR)
    })
}

/// The state folder of a project (`.permark/local`, or the older `.trailmark/local`) that `dir` is, or that
/// holds it directly: the folder of copies the review keeps in `local/` is private like the files next to it.
fn state_folder_of(dir: &Path) -> Option<&Path> {
    if permark_of(dir).is_some() {
        Some(dir)
    } else {
        dir.parent().filter(|parent| permark_of(parent).is_some())
    }
}

/// Fails when `dir`, a state folder of a project (or a folder in one), or the `.permark` folder above it is a link.
fn check_not_linked(dir: &Path) -> io::Result<()> {
    let Some(state) = state_folder_of(dir) else {
        return Ok(());
    };
    let Some(permark) = permark_of(state) else {
        return Ok(());
    };
    [permark, state, dir]
        .into_iter()
        .find(|folder| matches!(entry(folder), Entry::Other))
        .map_or(Ok(()), |folder| Err(refused(folder)))
}

/// Writes `content` to `path` unless something is there already (a link included, which is not followed).
pub(super) fn create_if_missing(path: &Path, content: &str) -> io::Result<()> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file.write_all(content.as_bytes()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

/// Makes a state folder of a project ready: made when it is missing, never behind a link, and ignoring itself. A folder
/// inside a state folder is made ready the same way and left to the state folder's own `.gitignore`. Any other folder
/// is left to the caller.
pub(super) fn prepare(dir: &Path) -> io::Result<()> {
    let Some(state) = state_folder_of(dir) else {
        return Ok(());
    };
    check_not_linked(dir)?;
    std::fs::create_dir_all(dir)?;
    // The folder is looked at again once it exists: it is the folder that gets the file.
    check_not_linked(dir)?;
    create_if_missing(&state.join(".gitignore"), SELF_IGNORE)
}

/// Writes a file of the local state (atomically, see [`write_atomic`]). In a project's `.permark/` folder it first
/// makes sure the folder is not a link and ignores itself; anywhere else (the app's own data folder) it is just
/// [`write_atomic`].
pub(crate) fn write_local_state(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        prepare(dir)?;
    }
    write_atomic(path, bytes)
}

/// Makes the folder of a series of [`write_prepared_cache`] ready, once: in a project's `.permark/` folder it is made
/// when missing, never behind a link, and ignoring itself (see [`write_local_state`]).
pub(crate) fn prepare_local_folder(dir: &Path) -> io::Result<()> {
    prepare(dir)
}

/// Writes a file of the local state that can be made again (a copy checked by its hash when it is read): as
/// [`write_local_state`], without flushing it to the disk (see [`write_atomic_unsynced`]).
pub(crate) fn write_local_cache(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        prepare(dir)?;
    }
    write_atomic_unsynced(path, bytes)
}

/// [`write_local_cache`] in a folder that [`prepare_local_folder`] made ready: for the second file on, there is nothing
/// left to check.
pub(crate) fn write_prepared_cache(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_unsynced(path, bytes)
}

/// Opens the file that stands for a lock on another file of the local state (made when missing), for a program that
/// keeps `File::lock` on it while it reads, changes and writes that file. The lock file itself holds nothing. It is
/// made with the same care as the files it protects, and a link in its place is not written through.
pub(crate) fn open_lock_file(path: &Path) -> io::Result<std::fs::File> {
    if let Some(dir) = path.parent() {
        prepare(dir)?;
        std::fs::create_dir_all(dir)?;
    }
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            std::fs::OpenOptions::new().read(true).open(path)
        }
        made => made,
    }
}

/// Deletes a file of the local state, unless it is behind a link.
pub(crate) fn remove_local_state(path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        check_not_linked(dir)?;
    }
    std::fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::util::testing::{dir_link, remove_link};

    fn project() -> tempfile::TempDir {
        tempfile::tempdir().expect("project")
    }

    #[test]
    fn a_real_folder_is_usable_and_a_link_is_not() {
        let project = project();
        let outside = tempfile::tempdir().expect("outside");
        let permark = project.path().join(".permark");
        assert!(!usable_in(&permark), "missing");
        fs::create_dir(&permark).expect("mkdir");
        assert!(usable_in(&permark), "a real folder without a local one");
        fs::create_dir(permark.join("local")).expect("mkdir");
        assert!(usable_in(&permark), "a real local folder");
        fs::remove_dir(permark.join("local")).expect("rmdir");
        dir_link(&permark.join("local"), outside.path());
        assert!(!usable_in(&permark), "local is a link");
        remove_link(&permark.join("local"));
        fs::remove_dir(&permark).expect("rmdir");
        dir_link(&permark, outside.path());
        assert!(!usable_in(&permark), ".permark is a link");
        remove_link(&permark);
    }

    #[test]
    fn a_file_in_place_of_the_folder_is_not_usable() {
        let project = project();
        let permark = project.path().join(".permark");
        fs::write(&permark, "x").expect("write");
        assert!(!usable_in(&permark));
    }

    #[test]
    fn the_state_folder_ignores_itself() {
        let project = project();
        let file = project.path().join(".permark/local/comments.json");
        write_local_state(&file, b"{}").expect("write");
        assert_eq!(fs::read(&file).expect("read"), b"{}");
        let ignore = fs::read_to_string(project.path().join(".permark/local/.gitignore"))
            .expect("a .gitignore");
        assert!(ignore.lines().any(|line| line == "*"), "{ignore:?}");
        // writing again leaves it as it is, and a .gitignore somebody wrote is kept
        fs::write(project.path().join(".permark/local/.gitignore"), "mine\n").expect("write");
        write_local_state(&file, b"[]").expect("write again");
        assert_eq!(
            fs::read_to_string(project.path().join(".permark/local/.gitignore")).expect("read"),
            "mine\n"
        );
    }

    #[test]
    fn a_cache_file_is_written_like_a_state_file_in_a_private_folder_and_a_batch_prepares_it_once()
    {
        let project = project();
        let local = project.path().join(".permark/local");
        write_local_cache(&local.join("reviewed/one"), b"1").expect("write");
        let ignore = fs::read_to_string(local.join(".gitignore")).expect("ignores itself");
        assert!(ignore.lines().any(|line| line == "*"));

        let folder = local.join("batch");
        prepare_local_folder(&folder).expect("prepare");
        write_prepared_cache(&folder.join("two"), b"2").expect("write");
        write_prepared_cache(&folder.join("three"), b"3").expect("write");
        assert_eq!(fs::read(folder.join("three")).expect("read"), b"3");
    }

    #[test]
    fn a_cache_file_is_not_written_behind_a_link_either() {
        let project = project();
        let outside = tempfile::tempdir().expect("outside");
        let local = project.path().join(".permark/local");
        fs::create_dir_all(&local).expect("mkdir");
        dir_link(&local.join("reviewed"), outside.path());
        assert!(write_local_cache(&local.join("reviewed/abc"), b"copy").is_err());
        assert!(prepare_local_folder(&local.join("reviewed")).is_err());
        assert!(!outside.path().join("abc").exists());
        remove_link(&local.join("reviewed"));
    }

    #[test]
    fn a_folder_inside_the_state_folder_is_private_too() {
        let project = project();
        let file = project.path().join(".permark/local/reviewed/abc");
        write_local_state(&file, b"copy").expect("write");
        assert_eq!(fs::read(&file).expect("read"), b"copy");
        let ignore = fs::read_to_string(project.path().join(".permark/local/.gitignore"))
            .expect("the state folder's .gitignore covers it");
        assert!(ignore.lines().any(|line| line == "*"), "{ignore:?}");
        assert!(!project
            .path()
            .join(".permark/local/reviewed/.gitignore")
            .exists());
        remove_local_state(&file).expect("deleted");
        assert!(!file.exists());
    }

    #[test]
    fn nothing_is_written_or_deleted_behind_a_link_in_a_folder_inside_the_state_folder() {
        let project = project();
        let outside = tempfile::tempdir().expect("outside");
        fs::write(outside.path().join("abc"), "precious").expect("write");
        let local = project.path().join(".permark/local");
        fs::create_dir_all(&local).expect("mkdir");
        dir_link(&local.join("reviewed"), outside.path());
        let file = local.join("reviewed/abc");
        assert!(write_local_state(&file, b"copy").is_err());
        assert!(remove_local_state(&file).is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("abc")).expect("read"),
            "precious"
        );
        remove_link(&local.join("reviewed"));

        // and when local itself is the link
        fs::remove_dir(&local).expect("rmdir");
        dir_link(&local, outside.path());
        assert!(write_local_state(&local.join("reviewed/abc"), b"copy").is_err());
        assert!(!outside.path().join("reviewed").exists());
        remove_link(&local);
    }

    #[test]
    fn other_folders_are_written_as_they_are() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("projects/abc/comments.json");
        write_local_state(&file, b"{}").expect("write");
        assert!(!dir.path().join("projects/abc/.gitignore").exists());
    }

    #[test]
    fn nothing_is_written_or_deleted_behind_a_link() {
        let project = project();
        let outside = tempfile::tempdir().expect("outside");
        fs::write(outside.path().join("agent.json"), "precious").expect("write");
        let permark = project.path().join(".permark");
        fs::create_dir(&permark).expect("mkdir");
        dir_link(&permark.join("local"), outside.path());
        let file = permark.join("local/agent.json");
        assert!(write_local_state(&file, b"{}").is_err());
        assert!(remove_local_state(&file).is_err());
        assert!(prepare(&permark.join("local")).is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("agent.json")).expect("read"),
            "precious"
        );
        assert!(!outside.path().join(".gitignore").exists());
        remove_link(&permark.join("local"));

        // the same when .permark itself is the link
        fs::remove_dir(&permark).expect("rmdir");
        dir_link(&permark, outside.path());
        assert!(write_local_state(&permark.join("local/comments.json"), b"{}").is_err());
        assert!(!outside.path().join("local").exists());
        remove_link(&permark);
    }

    #[test]
    fn a_file_is_deleted_only_when_it_is_there() {
        let project = project();
        let file = project.path().join(".permark/local/agent.json");
        assert!(remove_local_state(&file).is_err(), "nothing to delete");
        write_local_state(&file, b"{}").expect("write");
        remove_local_state(&file).expect("deleted");
        assert!(!file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_a_file_is_replaced_not_followed() {
        let project = project();
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("target.txt");
        fs::write(&target, "precious").expect("write");
        let local = project.path().join(".permark/local");
        fs::create_dir_all(&local).expect("mkdir");
        std::os::unix::fs::symlink(&target, local.join("comments.json")).expect("symlink");
        write_local_state(&local.join("comments.json"), b"{}").expect("write");
        assert_eq!(fs::read_to_string(&target).expect("read"), "precious");
        assert_eq!(fs::read(local.join("comments.json")).expect("read"), b"{}");
        std::os::unix::fs::symlink(&target, local.join("agent.json")).expect("symlink");
        remove_local_state(&local.join("agent.json")).expect("deleted");
        assert_eq!(fs::read_to_string(&target).expect("read"), "precious");
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_is_not_written_through() {
        let project = project();
        let outside = tempfile::tempdir().expect("outside");
        let created = outside.path().join("created.txt");
        let tm = project.path().join(".permark");
        fs::create_dir(&tm).expect("mkdir");
        std::os::unix::fs::symlink(&created, tm.join(".gitignore")).expect("symlink");
        create_if_missing(&tm.join(".gitignore"), "cache/\n").expect("left alone");
        assert!(!created.exists(), "the link was not followed");
    }
}
