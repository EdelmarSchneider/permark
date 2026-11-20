//! Safety of `.permark/` state folders: a repository decides what its `.permark/` holds, links included. A link in
//! place of `.permark/` | `local/` | `cache/` would send our writes (and deletes) wherever it points -> never used.
//! Each state folder ignores itself (`.gitignore` = `*`, set back at each write when the folder brought another one):
//! private whatever the project's own ignore says. The cache folder carries a cache directory tag for backup tools.

use std::fs::Metadata;
use std::io::{self, Write};
use std::path::Path;

use crate::fsx::{read_regular, write_atomic, Zone};

/// `.gitignore` of a state folder: everything, itself included.
pub(super) const SELF_IGNORE: &str = "*\n";
/// Name of the state folder that holds only rebuildable files.
const CACHE: &str = "cache";
/// `CACHEDIR.TAG` (<https://bford.info/cachedir/>): backup tools that honor it skip the folder.
const CACHEDIR_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n\
# This file is a cache directory tag created by PerMark.\n\
# For information about cache directory tags, see:\n\
#\thttps://bford.info/cachedir/\n";

/// What is on disk at a path, links NOT followed.
pub(super) enum Entry {
    Missing,
    Folder,
    /// Link, file, or unreadable.
    Other,
}

/// Any reparse point counts as a link here, a cloud placeholder folder too: this decides where a project's state lives,
/// so a `.permark` in a synced folder keeps its state in the profile (a move to `.permark/local/` would leave the state
/// kept there unread).
pub(super) fn entry(path: &Path) -> Entry {
    entry_as(path, crate::tree::is_link_meta)
}

/// What is on disk at a path, `link` telling what counts as a link (never followed).
fn entry_as(path: &Path, link: fn(&Metadata) -> bool) -> Entry {
    match std::fs::symlink_metadata(path) {
        // a junction is a directory to `symlink_metadata` on some std versions: `link` looks at it too
        Ok(meta) if meta.file_type().is_dir() && !link(&meta) => Entry::Folder,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Entry::Missing,
        Ok(_) | Err(_) => Entry::Other,
    }
}

pub(super) fn is_real_folder(path: &Path) -> bool {
    matches!(entry(path), Entry::Folder)
}

fn refused(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "{} is a link or not a folder; PerMark keeps no files behind one",
            path.display()
        ),
    )
}

/// Creates `path` with `content` unless anything (a link included, not followed) is there.
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

/// Makes the file at `path` (below `permark`) hold exactly `content`: written when missing or different. A link or a
/// FIFO in its place is replaced by the rename, never followed, read or written through; a folder is left, as it cannot
/// be replaced without deleting what it holds, and so is a folder link on Windows (a file cannot be renamed over one).
fn enforce(permark: &Path, path: &Path, content: &str) -> io::Result<()> {
    // the cap is the content's length: anything longer differs without being read
    let limit = content.len() as u64;
    if read_regular(path, limit, Zone::Inside(permark)).is_ok_and(|b| b == content.as_bytes()) {
        return Ok(());
    }
    // the entry itself: a link followed here could reach a network share
    if std::fs::symlink_metadata(path).is_ok_and(|meta| holds_folder(&meta)) {
        return Ok(());
    }
    write_atomic(path, content.as_bytes())
}

/// A folder, or on Windows a folder link | junction: a rename of a file over it fails.
fn holds_folder(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        meta.is_dir() || meta.file_type().is_symlink_dir()
    }
    #[cfg(not(windows))]
    {
        meta.is_dir()
    }
}

/// Makes `dir` (a project state folder `.permark/<sub>`, or a folder inside one) ready: never behind a link,
/// created when missing, its state folder self-ignoring. `permark` = the `.permark` folder, `state` = `local` | `cache`.
pub(super) fn prepare(permark: &Path, state: &Path, dir: &Path) -> io::Result<()> {
    // a link as the zone reads see one: a cloud placeholder folder on the way is a folder, so saves go where loads read
    let walked = |path: &Path| entry_as(path, crate::fsx::is_link);
    // top-down, `.permark` to `dir` (plugin namespaces): a link is refused before anything below it is looked up (a
    // lookup through it could reach a network share); below a missing folder nothing is there yet
    let check = || -> io::Result<()> {
        let below = crate::paths::strip_root(dir, state).ok_or_else(|| refused(dir))?;
        if matches!(walked(permark), Entry::Other) {
            return Err(refused(permark));
        }
        let mut walk = state.to_path_buf();
        let mut names = below.into_iter();
        loop {
            match walked(&walk) {
                Entry::Other => return Err(refused(&walk)),
                Entry::Missing => break,
                Entry::Folder => {}
            }
            let Some(name) = names.next() else {
                break;
            };
            walk.push(name);
        }
        Ok(())
    };
    if !matches!(walked(permark), Entry::Folder) {
        return Err(refused(permark));
    }
    check()?;
    std::fs::create_dir_all(dir)?;
    // looked at again once it exists: a link swapped in meanwhile is still refused
    check()?;
    // best effort: git ignore files keep the state out of commits but guard nothing, so they never refuse a write
    let _ = enforce(permark, &state.join(".gitignore"), SELF_IGNORE);
    if state.file_name().is_some_and(|name| name == CACHE) {
        let _ = create_if_missing(&state.join("CACHEDIR.TAG"), CACHEDIR_TAG);
    }
    Ok(())
}
