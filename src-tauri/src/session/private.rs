//! The private state folders of a project, `.trailmark/local/` and `.trailmark/cache/`: what makes it safe to write
//! there.
//!
//! A repository decides what is in its `.trailmark/` folder, links included. A link in place of `.trailmark/` or
//! `local/` would send what TrailMark writes there (the session, the review notes, the agent's context) and the file
//! it deletes to wherever the link points, so a folder that is a link (a symbolic link, or a junction on Windows) is
//! never used. And the project's own `.gitignore` may or may not say that these folders are private, so each of them
//! ignores itself: a `.gitignore` with `*` inside keeps its contents out of the repository whatever the parent says.

use std::io::{self, Write};
use std::path::Path;

use crate::util::write_atomic;

const TRAILMARK_DIR: &str = ".trailmark";
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

/// Whether a project can keep its state in `trailmark` (its `.trailmark` path): a real folder, whose `local/` is a
/// real folder or not made yet.
pub(super) fn usable_in(trailmark: &Path) -> bool {
    is_real_folder(trailmark) && !matches!(entry(&trailmark.join("local")), Entry::Other)
}

fn refused(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "{} is a link or not a folder, and TrailMark keeps no files behind one",
            path.display()
        ),
    )
}

/// The `.trailmark` folder `dir` is in, when `dir` is one of its folders.
fn trailmark_of(dir: &Path) -> Option<&Path> {
    dir.parent()
        .filter(|parent| parent.file_name().is_some_and(|name| name == TRAILMARK_DIR))
}

/// Fails when `dir`, a state folder of a project, or the `.trailmark` folder above it is a link.
fn check_not_linked(dir: &Path) -> io::Result<()> {
    let Some(trailmark) = trailmark_of(dir) else {
        return Ok(());
    };
    [trailmark, dir]
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

/// Makes a state folder of a project ready: made when it is missing, never behind a link, and ignoring itself. Any
/// other folder is left to the caller.
pub(super) fn prepare(dir: &Path) -> io::Result<()> {
    if trailmark_of(dir).is_none() {
        return Ok(());
    }
    check_not_linked(dir)?;
    std::fs::create_dir_all(dir)?;
    // The folder is looked at again once it exists: it is the folder that gets the file.
    check_not_linked(dir)?;
    create_if_missing(&dir.join(".gitignore"), SELF_IGNORE)
}

/// Writes a file of the local state (atomically, see [`write_atomic`]). In a project's `.trailmark/` folder it first
/// makes sure the folder is not a link and ignores itself; anywhere else (the app's own data folder) it is just
/// [`write_atomic`].
pub(crate) fn write_local_state(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        prepare(dir)?;
    }
    write_atomic(path, bytes)
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
        let trailmark = project.path().join(".trailmark");
        assert!(!usable_in(&trailmark), "missing");
        fs::create_dir(&trailmark).expect("mkdir");
        assert!(usable_in(&trailmark), "a real folder without a local one");
        fs::create_dir(trailmark.join("local")).expect("mkdir");
        assert!(usable_in(&trailmark), "a real local folder");
        fs::remove_dir(trailmark.join("local")).expect("rmdir");
        dir_link(&trailmark.join("local"), outside.path());
        assert!(!usable_in(&trailmark), "local is a link");
        remove_link(&trailmark.join("local"));
        fs::remove_dir(&trailmark).expect("rmdir");
        dir_link(&trailmark, outside.path());
        assert!(!usable_in(&trailmark), ".trailmark is a link");
        remove_link(&trailmark);
    }

    #[test]
    fn a_file_in_place_of_the_folder_is_not_usable() {
        let project = project();
        let trailmark = project.path().join(".trailmark");
        fs::write(&trailmark, "x").expect("write");
        assert!(!usable_in(&trailmark));
    }

    #[test]
    fn the_state_folder_ignores_itself() {
        let project = project();
        let file = project.path().join(".trailmark/local/notes.json");
        write_local_state(&file, b"{}").expect("write");
        assert_eq!(fs::read(&file).expect("read"), b"{}");
        let ignore = fs::read_to_string(project.path().join(".trailmark/local/.gitignore"))
            .expect("a .gitignore");
        assert!(ignore.lines().any(|line| line == "*"), "{ignore:?}");
        // writing again leaves it as it is, and a .gitignore somebody wrote is kept
        fs::write(project.path().join(".trailmark/local/.gitignore"), "mine\n").expect("write");
        write_local_state(&file, b"[]").expect("write again");
        assert_eq!(
            fs::read_to_string(project.path().join(".trailmark/local/.gitignore")).expect("read"),
            "mine\n"
        );
    }

    #[test]
    fn other_folders_are_written_as_they_are() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("projects/abc/notes.json");
        write_local_state(&file, b"{}").expect("write");
        assert!(!dir.path().join("projects/abc/.gitignore").exists());
    }

    #[test]
    fn nothing_is_written_or_deleted_behind_a_link() {
        let project = project();
        let outside = tempfile::tempdir().expect("outside");
        fs::write(outside.path().join("agent.json"), "precious").expect("write");
        let trailmark = project.path().join(".trailmark");
        fs::create_dir(&trailmark).expect("mkdir");
        dir_link(&trailmark.join("local"), outside.path());
        let file = trailmark.join("local/agent.json");
        assert!(write_local_state(&file, b"{}").is_err());
        assert!(remove_local_state(&file).is_err());
        assert!(prepare(&trailmark.join("local")).is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("agent.json")).expect("read"),
            "precious"
        );
        assert!(!outside.path().join(".gitignore").exists());
        remove_link(&trailmark.join("local"));

        // the same when .trailmark itself is the link
        fs::remove_dir(&trailmark).expect("rmdir");
        dir_link(&trailmark, outside.path());
        assert!(write_local_state(&trailmark.join("local/notes.json"), b"{}").is_err());
        assert!(!outside.path().join("local").exists());
        remove_link(&trailmark);
    }

    #[test]
    fn a_file_is_deleted_only_when_it_is_there() {
        let project = project();
        let file = project.path().join(".trailmark/local/agent.json");
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
        let local = project.path().join(".trailmark/local");
        fs::create_dir_all(&local).expect("mkdir");
        std::os::unix::fs::symlink(&target, local.join("notes.json")).expect("symlink");
        write_local_state(&local.join("notes.json"), b"{}").expect("write");
        assert_eq!(fs::read_to_string(&target).expect("read"), "precious");
        assert_eq!(fs::read(local.join("notes.json")).expect("read"), b"{}");
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
        let tm = project.path().join(".trailmark");
        fs::create_dir(&tm).expect("mkdir");
        std::os::unix::fs::symlink(&created, tm.join(".gitignore")).expect("symlink");
        create_if_missing(&tm.join(".gitignore"), "cache/\n").expect("left alone");
        assert!(!created.exists(), "the link was not followed");
    }
}
