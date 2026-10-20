//! Safety of `.permark/` state folders: a repository decides what its `.permark/` holds, links included. A link in
//! place of `.permark/` | `local/` | `cache/` would send our writes (and deletes) wherever it points -> never used.
//! Each state folder ignores itself (`.gitignore` = `*`): private whatever the project's own ignore says.

use std::io::{self, Write};
use std::path::Path;

/// `.gitignore` of a state folder: everything, itself included.
pub(super) const SELF_IGNORE: &str = "*\n";

/// What is on disk at a path, links NOT followed.
pub(super) enum Entry {
    Missing,
    Folder,
    /// Link, file, or unreadable.
    Other,
}

pub(super) fn entry(path: &Path) -> Entry {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() && !is_reparse(&meta) => Entry::Folder,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Entry::Missing,
        Ok(_) | Err(_) => Entry::Other,
    }
}

/// Junctions are directories to `symlink_metadata` on some std versions: check the reparse flag explicitly.
#[cfg(windows)]
fn is_reparse(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse(_: &std::fs::Metadata) -> bool {
    false
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

/// Makes `dir` (a project state folder `.permark/<sub>`, or a folder inside one) ready: never behind a link,
/// created when missing, its state folder self-ignoring. `permark` = the `.permark` folder, `state` = `local` | `cache`.
pub(super) fn prepare(permark: &Path, state: &Path, dir: &Path) -> io::Result<()> {
    let check = || -> io::Result<()> {
        for folder in [permark, state] {
            if matches!(entry(folder), Entry::Other) {
                return Err(refused(folder));
            }
        }
        // every folder from the state folder down to `dir` (plugin namespaces) is real too
        let mut walk = dir;
        while walk != state {
            if matches!(entry(walk), Entry::Other) {
                return Err(refused(walk));
            }
            walk = walk.parent().ok_or_else(|| refused(dir))?;
        }
        Ok(())
    };
    if !matches!(entry(permark), Entry::Folder) {
        return Err(refused(permark));
    }
    check()?;
    std::fs::create_dir_all(dir)?;
    // looked at again once it exists: a link swapped in meanwhile is still refused
    check()?;
    create_if_missing(&state.join(".gitignore"), SELF_IGNORE)
}
