//! The lock on the notes folder. The Notes window of one TrailMark holds it while it is open; another TrailMark that
//! opens the same folder (another install, a development build) finds it taken and shows the notes read-only, so two
//! programs never write the same notes at the same time. The operating system releases it when the program ends,
//! however it ends.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use super::data::STATE_DIR;

const FILE_NAME: &str = "notes.lock";

/// The lock, held for as long as this value lives.
#[derive(Debug)]
pub(crate) struct FolderLock {
    _file: File,
}

/// Whether this program may write the notes.
#[derive(Debug)]
pub(crate) enum Access {
    /// The lock is ours.
    Writable(FolderLock),
    /// Another program holds it.
    Locked,
    /// The lock file cannot be made or opened: the folder cannot be written either.
    Unwritable,
}

/// Takes the lock of the notes folder `root`.
pub(crate) fn acquire(root: &Path) -> Access {
    let dir = root.join(STATE_DIR);
    if std::fs::create_dir_all(&dir).is_err() {
        return Access::Unwritable;
    }
    let Ok(file) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(FILE_NAME))
    else {
        return Access::Unwritable;
    };
    match file.try_lock() {
        Ok(()) => Access::Writable(FolderLock { _file: file }),
        Err(TryLockError::WouldBlock) => Access::Locked,
        Err(TryLockError::Error(_)) => Access::Unwritable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_holder_is_refused_until_the_first_lets_go() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = acquire(dir.path());
        assert!(matches!(first, Access::Writable(_)));
        // each call opens the file again, as a second program would
        assert!(matches!(acquire(dir.path()), Access::Locked));
        assert!(matches!(acquire(dir.path()), Access::Locked));
        drop(first);
        assert!(matches!(acquire(dir.path()), Access::Writable(_)));
    }

    #[test]
    fn another_folder_has_a_lock_of_its_own() {
        let (one, other) = (
            tempfile::tempdir().expect("tempdir"),
            tempfile::tempdir().expect("tempdir"),
        );
        let _held = acquire(one.path());
        assert!(matches!(acquire(other.path()), Access::Writable(_)));
    }

    #[test]
    fn a_folder_that_is_not_there_cannot_be_locked() {
        let dir = tempfile::tempdir().expect("tempdir");
        // a file where the folder should be
        let file = dir.path().join("file");
        std::fs::write(&file, "x").expect("write");
        assert!(matches!(acquire(&file), Access::Unwritable));
    }
}
