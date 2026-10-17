//! The project state folder under its earlier name.
//!
//! Projects used to keep their state in `.trailmark/`. The first time PerMark meets such a project it renames the folder
//! to `.permark/` (one atomic rename, so nothing is copied or lost). When the rename cannot be done (the folder is locked
//! or read-only) the old folder keeps being used, so no setting, comment or reading mark disappears. When both folders
//! exist, `.permark/` wins and the old one is left alone.

use std::path::{Path, PathBuf};

use super::private::is_real_folder;

/// The current name of the state folder.
pub(crate) const STATE_DIR: &str = ".permark";
/// The name the state folder had before.
pub(crate) const LEGACY_STATE_DIR: &str = ".trailmark";

/// The state folder of the project in `root`: `.permark/`, after renaming a `.trailmark/` that was there first. The old
/// folder is returned when it could not be renamed. The folder is not made when neither exists.
pub(crate) fn state_dir(root: &Path) -> PathBuf {
    let current = root.join(STATE_DIR);
    let legacy = root.join(LEGACY_STATE_DIR);
    if std::fs::symlink_metadata(&current).is_ok() || !is_real_folder(&legacy) {
        return current;
    }
    match std::fs::rename(&legacy, &current) {
        Ok(()) => current,
        Err(error) => {
            eprintln!(
                "cannot rename {} to {}: {error}; the old folder is used",
                legacy.display(),
                current.display()
            );
            legacy
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_legacy_folder_is_renamed_with_its_content() {
        let dir = tempfile::tempdir().expect("tmp");
        let old = dir.path().join(LEGACY_STATE_DIR);
        std::fs::create_dir_all(old.join("local")).expect("mkdir");
        std::fs::write(old.join("local").join("read.json"), "{}").expect("write");

        let found = state_dir(dir.path());

        assert_eq!(found, dir.path().join(STATE_DIR));
        assert!(!old.exists());
        assert!(found.join("local").join("read.json").is_file());
    }

    #[test]
    fn when_both_exist_the_current_one_wins_and_the_old_one_stays() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir(dir.path().join(STATE_DIR)).expect("mkdir");
        std::fs::create_dir(dir.path().join(LEGACY_STATE_DIR)).expect("mkdir");

        assert_eq!(state_dir(dir.path()), dir.path().join(STATE_DIR));
        assert!(dir.path().join(LEGACY_STATE_DIR).is_dir());
    }

    #[test]
    fn without_either_folder_nothing_is_made() {
        let dir = tempfile::tempdir().expect("tmp");

        assert_eq!(state_dir(dir.path()), dir.path().join(STATE_DIR));
        assert!(!dir.path().join(STATE_DIR).exists());
        assert!(!dir.path().join(LEGACY_STATE_DIR).exists());
    }

    #[test]
    fn a_legacy_file_is_not_a_state_folder() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join(LEGACY_STATE_DIR), "x").expect("write");

        assert_eq!(state_dir(dir.path()), dir.path().join(STATE_DIR));
        assert!(dir.path().join(LEGACY_STATE_DIR).is_file());
    }
}
