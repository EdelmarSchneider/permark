//! What PerMark 0.5 kept in its own profile folder (`<data dir>/dev.permark.app`, named after the app identifier),
//! taken over once: copied, never moved, changed or deleted (0.5 still reads it after a downgrade).
//!
//! - Project data of a project without `.permark/` (`projects/<0.5 key>/`): tags (`meta.toml`), comments
//!   (`comments.json`), silence marks (`quiet.json`), copied into this version's profile folder of the project when
//!   it has none of its own yet. A mark file records that the copy was made, so data the person removes later never
//!   comes back.
//! - App settings (`settings.json`): see [`crate::settings::SettingsFile::adopt_older`].

use std::path::{Path, PathBuf};

use super::{AppDirs, StateLocation, StateStore};
use crate::paths::Root;

/// 0.5's profile folder name (its app identifier).
pub const OLDER_FOLDER: &str = "dev.permark.app";
/// Project files 0.5 kept in its profile that hold the person's data.
pub const OLDER_PROJECT_FILES: [&str; 3] = ["meta.toml", "comments.json", "quiet.json"];
/// Marks a project profile folder whose 0.5 data was looked at (copied or absent).
const TAKEN_OVER: &str = "from-0.5";
/// Largest file copied (a bigger one stays where it is).
const MAX_FILE: u64 = 64 * 1024 * 1024;

/// 0.5's profile folder, beside this version's (same data folder of the system).
#[must_use]
pub fn older_profile(dirs: &AppDirs) -> Option<PathBuf> {
    let older = dirs.profile.parent()?.join(OLDER_FOLDER);
    older.is_dir().then_some(older)
}

/// 0.5's key of a project folder: xxh3 of its path with `/` separators, trailing separators dropped, lower case on
/// Windows (the folder as opened: canonical, without the `\\?\` prefix).
#[must_use]
pub fn older_project_key(root: &Path) -> String {
    let mut text = dunce::simplified(root).to_string_lossy().replace('\\', "/");
    while text.ends_with('/') && text.len() > 1 {
        text.pop();
    }
    if cfg!(windows) {
        text = text.to_lowercase();
    }
    crate::fsx::hash(text.as_bytes()).hex()
}

/// Copies 0.5's data of a project without `.permark/` into its profile folder, once (see the module). Best effort:
/// a file that cannot be copied stays where it is, and the next start tries again.
pub fn take_over_project(dirs: &AppDirs, root: &Root, store: &StateStore) {
    if store.location() != StateLocation::Profile {
        return;
    }
    let target = store.dir();
    if target.join(TAKEN_OVER).exists() {
        return;
    }
    let Some(older) = older_profile(dirs) else {
        return;
    };
    let source = older.join("projects").join(older_project_key(root.path()));
    let mut complete = true;
    if source.is_dir() {
        for name in OLDER_PROJECT_FILES {
            complete &= copy_if_missing(&source.join(name), &target.join(name));
        }
    }
    if complete && std::fs::create_dir_all(target).is_ok() {
        let _ = std::fs::write(target.join(TAKEN_OVER), b"");
    }
}

/// `from` -> `to` when `from` is a regular file (no link) and `to` does not exist. False when a copy failed.
fn copy_if_missing(from: &Path, to: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(from) else {
        return true;
    };
    if !meta.is_file() || meta.len() > MAX_FILE || std::fs::symlink_metadata(to).is_ok() {
        return true;
    }
    let Ok(bytes) = crate::fsx::read_bytes(from, MAX_FILE) else {
        return false;
    };
    let Some(dir) = to.parent() else {
        return false;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    // never over a file that appeared meanwhile
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(&bytes)?;
            file.sync_all()
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, AppDirs, Root, PathBuf) {
        let base = tempfile::tempdir().unwrap();
        // this version's profile is `<data>/PerMark`; 0.5's is `<data>/dev.permark.app`
        let dirs = AppDirs {
            profile: base.path().join("data").join("PerMark"),
            cache: base.path().join("cache").join("PerMark"),
        };
        let project = base.path().join("Pro jeto ü");
        std::fs::create_dir_all(&project).unwrap();
        let root = Root::open(&project).unwrap();
        let older = base
            .path()
            .join("data")
            .join(OLDER_FOLDER)
            .join("projects")
            .join(older_project_key(root.path()));
        std::fs::create_dir_all(&older).unwrap();
        (base, dirs, root, older)
    }

    #[test]
    fn the_key_is_the_one_0_5_computed() {
        // 0.5: xxh3 of "c:/users/ana/notes" on Windows, of "/home/ana/notes" elsewhere
        let key = older_project_key(Path::new(if cfg!(windows) {
            r"C:\Users\Ana\Notes\"
        } else {
            "/home/ana/notes/"
        }));
        let text = if cfg!(windows) {
            "c:/users/ana/notes"
        } else {
            "/home/ana/notes"
        };
        assert_eq!(
            key,
            format!("{:016x}", xxhash_rust::xxh3::xxh3_64(text.as_bytes()))
        );
    }

    #[test]
    fn data_of_a_project_without_permark_is_copied_once_and_never_over_newer_data() {
        let (_base, dirs, root, older) = setup();
        std::fs::write(older.join("comments.json"), "{\r\n  \"version\": 1\r\n}").unwrap();
        std::fs::write(
            older.join("meta.toml"),
            "[files.\"a.md\"]\ntags = [\"x\"]\n",
        )
        .unwrap();
        std::fs::write(older.join("session.json"), "{}").unwrap();
        let store = StateStore::for_project(&dirs, &root);
        std::fs::create_dir_all(store.dir()).unwrap();
        std::fs::write(store.dir().join("quiet.json"), "{\"mine\":\"quiet\"}").unwrap();
        std::fs::write(older.join("quiet.json"), "{\"old\":\"quiet\"}").unwrap();
        take_over_project(&dirs, &root, &store);
        assert_eq!(
            std::fs::read(store.dir().join("comments.json")).unwrap(),
            b"{\r\n  \"version\": 1\r\n}",
            "byte for byte"
        );
        assert!(store.dir().join("meta.toml").is_file());
        assert!(
            !store.dir().join("session.json").exists(),
            "only the person's data"
        );
        assert_eq!(
            std::fs::read_to_string(store.dir().join("quiet.json")).unwrap(),
            "{\"mine\":\"quiet\"}",
            "this version's own file wins"
        );
        assert!(older.join("comments.json").is_file(), "0.5's copy stays");
        // the person deletes every comment: the old ones never come back
        std::fs::remove_file(store.dir().join("comments.json")).unwrap();
        take_over_project(&dirs, &root, &store);
        assert!(!store.dir().join("comments.json").exists());
    }

    #[test]
    fn a_project_with_permark_or_without_older_data_takes_nothing() {
        let (_base, dirs, root, older) = setup();
        std::fs::write(older.join("comments.json"), "{}").unwrap();
        std::fs::create_dir_all(root.path().join(".permark")).unwrap();
        let store = StateStore::for_project(&dirs, &root);
        take_over_project(&dirs, &root, &store);
        assert!(!store.dir().join("comments.json").exists());
        let none = AppDirs {
            profile: dirs
                .profile
                .parent()
                .unwrap()
                .join("elsewhere")
                .join("PerMark"),
            cache: dirs.cache.clone(),
        };
        std::fs::remove_dir_all(root.path().join(".permark")).unwrap();
        let store = StateStore::for_project(&none, &root);
        take_over_project(&none, &root, &store);
        assert!(!store.dir().join("comments.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_older_folder_is_not_followed() {
        let (base, dirs, root, older) = setup();
        let secret = base.path().join("secret.json");
        std::fs::write(&secret, "{}").unwrap();
        std::os::unix::fs::symlink(&secret, older.join("comments.json")).unwrap();
        let store = StateStore::for_project(&dirs, &root);
        take_over_project(&dirs, &root, &store);
        assert!(!store.dir().join("comments.json").exists());
    }
}
