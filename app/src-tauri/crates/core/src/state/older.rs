//! What PerMark 0.5 kept in its own profile folder (`<data dir>/dev.permark.app`, named after the app identifier),
//! taken over once: copied, never moved, changed or deleted (0.5 still reads it after a downgrade).
//!
//! - Project data of a project without `.permark/` (`projects/<0.5 key>/`): tags (`meta.toml`), comments
//!   (`comments.json`), silence marks (`quiet.json`), copied into this version's profile folder of the project when
//!   it has none of its own yet. A mark file records that the copy was made, so data the person removes later never
//!   comes back.
//! - App settings (`settings.json`): see [`crate::settings::SettingsFile::adopt_older`].
//! - Last window (`window.json`): geometry of the first window while this version's session has none.

use std::path::{Path, PathBuf};

use super::{AppDirs, StateLocation, StateStore};
use crate::paths::Root;
use crate::session::Geometry;

/// 0.5's profile folder name (its app identifier).
pub const OLDER_FOLDER: &str = "dev.permark.app";
/// Project files 0.5 kept in its profile that hold the person's data.
pub const OLDER_PROJECT_FILES: [&str; 3] = ["meta.toml", "comments.json", "quiet.json"];
/// Marks a project profile folder whose 0.5 data was looked at (copied or absent).
const TAKEN_OVER: &str = "from-0.5";
/// Largest file copied (a bigger one stays where it is).
const MAX_FILE: u64 = 64 * 1024 * 1024;
/// 0.5's last window geometry file, in its profile folder.
pub const OLDER_WINDOW: &str = "window.json";
/// Largest `window.json` read (a real one is ~100 bytes).
const MAX_WINDOW_FILE: u64 = 64 * 1024;

/// 0.5's profile folder, beside this version's (same data folder of the system).
#[must_use]
pub fn older_profile(dirs: &AppDirs) -> Option<PathBuf> {
    let older = dirs.profile.parent()?.join(OLDER_FOLDER);
    older.is_dir().then_some(older)
}

/// 0.5's last window (`window.json`), when it holds a usable one: unreadable | damaged -> none. Read only.
#[must_use]
pub fn older_window(dirs: &AppDirs) -> Option<Geometry> {
    let file = older_profile(dirs)?.join(OLDER_WINDOW);
    if !std::fs::symlink_metadata(&file).ok()?.is_file() {
        return None;
    }
    parse_older_window(&crate::fsx::read_bytes(&file, MAX_WINDOW_FILE).ok()?)
}

/// 0.5's `window.json`: `{ x, y, width, height, scale?, maximized? }` (outer corner + inner size, physical px).
/// Lenient like 0.5: unknown fields ignored; position | size missing | not an integer | out of range -> none;
/// scale missing | outside 0.25..16 -> 1; `maximized` not a boolean -> false.
#[must_use]
pub fn parse_older_window(bytes: &[u8]) -> Option<Geometry> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let int = |key: &str| value.get(key)?.as_i64().and_then(|v| i32::try_from(v).ok());
    let extent = |key: &str| value.get(key)?.as_u64().and_then(|v| u32::try_from(v).ok());
    Geometry {
        x: int("x")?,
        y: int("y")?,
        width: extent("width")?,
        height: extent("height")?,
        scale: value
            .get("scale")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0),
        maximized: value
            .get("maximized")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    }
    .sane()
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

    #[test]
    fn the_last_window_of_0_5_is_read_like_0_5_wrote_it() {
        let g = parse_older_window(
            br#"{"x": -1200, "y": 80, "width": 1280, "height": 800, "scale": 1.5, "maximized": true}"#,
        )
        .unwrap();
        assert_eq!(
            g,
            Geometry {
                x: -1200,
                y: 80,
                width: 1280,
                height: 800,
                scale: 1.5,
                maximized: true
            }
        );
        // pretty-printed with CRLF, unknown fields, no scale | flag
        let g = parse_older_window(
            b"{\r\n  \"x\": 1,\r\n  \"y\": 2,\r\n  \"width\": 900,\r\n  \"height\": 600,\r\n  \"monitor\": \"D2\"\r\n}",
        )
        .unwrap();
        assert_eq!((g.x, g.y, g.width, g.height), (1, 2, 900, 600));
        assert!((g.scale - 1.0).abs() < f64::EPSILON && !g.maximized);
        for scale in ["0", "-2", "\"big\"", "null", "1000", "0.01"] {
            let json = format!(
                r#"{{"x": 1, "y": 2, "width": 900, "height": 600, "scale": {scale}, "maximized": "yes"}}"#
            );
            let g = parse_older_window(json.as_bytes()).unwrap();
            assert!(
                (g.scale - 1.0).abs() < f64::EPSILON && !g.maximized,
                "{scale}"
            );
        }
    }

    #[test]
    fn a_damaged_last_window_of_0_5_is_none() {
        for text in [
            "",
            "{ not json",
            "[]",
            "7",
            "null",
            "{}",
            r#"{"y": 20, "width": 900, "height": 600}"#,
            r#"{"x": "10", "y": 20, "width": 900, "height": 600}"#,
            r#"{"x": 10.5, "y": 20, "width": 900, "height": 600}"#,
            r#"{"x": 10, "y": null, "width": 900, "height": 600}"#,
            r#"{"x": 10, "y": 20, "width": 0, "height": 600}"#,
            r#"{"x": 10, "y": 20, "width": 900, "height": -600}"#,
            r#"{"x": 10, "y": 20, "width": 100001, "height": 600}"#,
            r#"{"x": 99999999999, "y": 20, "width": 900, "height": 600}"#,
        ] {
            assert_eq!(parse_older_window(text.as_bytes()), None, "{text}");
        }
        assert_eq!(parse_older_window(&[0xff, 0xfe, 0x00]), None);
    }

    #[test]
    fn the_last_window_of_0_5_comes_from_its_profile_folder_and_stays_there() {
        let base = tempfile::tempdir().unwrap();
        let data = base.path().join("Dados do usuário ção");
        let dirs = AppDirs {
            profile: data.join("PerMark"),
            cache: base.path().join("cache").join("PerMark"),
        };
        assert_eq!(older_window(&dirs), None, "no 0.5 folder");
        std::fs::create_dir_all(data.join(OLDER_FOLDER)).unwrap();
        assert_eq!(older_window(&dirs), None, "no file");
        let file = data.join(OLDER_FOLDER).join(OLDER_WINDOW);
        let text = r#"{"x": 40, "y": 30, "width": 1400, "height": 900, "scale": 1.25, "maximized": false}"#;
        std::fs::write(&file, text).unwrap();
        assert_eq!(
            older_window(&dirs).map(|g| (g.x, g.width)),
            Some((40, 1400))
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text, "read only");
        std::fs::write(&file, "{ half").unwrap();
        assert_eq!(older_window(&dirs), None, "corrupt");
        std::fs::write(&file, vec![b' '; 70 * 1024]).unwrap();
        assert_eq!(older_window(&dirs), None, "too large");
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        assert_eq!(older_window(&dirs), None, "a folder");
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
