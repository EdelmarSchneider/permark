//! The size and position of the window, remembered per project and app-wide.
//!
//! A project folder keeps the geometry the window had the last time that project was open, next to its
//! session (so never in a committed file). The app keeps one more copy, "the last window", for the
//! welcome screen, for a project opened for the first time, and for Simple mode (a single file).

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{or_default, SessionError, StateStore};
use crate::util::write_atomic;

/// Name of the file, in a project's state folder and in the app's own folder.
pub(crate) const FILE_NAME: &str = "window.json";

/// No real window is this big; a bigger number is damaged data.
const MAX_EXTENT: u32 = 100_000;
/// The scale factors a display can have; anything else is damaged data and reads as 1.
const SCALE_RANGE: std::ops::RangeInclusive<f64> = 0.25..=16.0;

/// Where the window was. Positions and sizes are physical pixels on the whole desktop (what the window
/// system reports), and `scale` is the scale factor of the display the window was on, which is what
/// lets a size mean the same thing after the display's scale changes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    /// The outer top-left corner, as the window system reports it.
    pub x: i32,
    pub y: i32,
    /// The size of the content area. While maximized this is the size the window has when restored.
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub maximized: bool,
}

/// What a file may hold: every field optional and every value lenient, so a file written by a newer
/// version, edited by hand or half-written never fails the app.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Stored {
    #[serde(deserialize_with = "or_default")]
    x: Option<i32>,
    #[serde(deserialize_with = "or_default")]
    y: Option<i32>,
    #[serde(deserialize_with = "or_default")]
    width: Option<u32>,
    #[serde(deserialize_with = "or_default")]
    height: Option<u32>,
    #[serde(deserialize_with = "or_default")]
    scale: Option<f64>,
    #[serde(deserialize_with = "or_default")]
    maximized: bool,
}

impl WindowState {
    /// Reads the saved shape. Without a usable position and size there is nothing to restore, so the
    /// result is `None`; a missing or unreadable scale reads as 1 and a missing `maximized` as false.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let stored: Stored = serde_json::from_slice(bytes).ok()?;
        let extent = |v: u32| (1..=MAX_EXTENT).contains(&v);
        Some(Self {
            x: stored.x?,
            y: stored.y?,
            width: stored.width.filter(|&v| extent(v))?,
            height: stored.height.filter(|&v| extent(v))?,
            scale: stored
                .scale
                .filter(|s| SCALE_RANGE.contains(s))
                .unwrap_or(1.0),
            maximized: stored.maximized,
        })
    }

    /// The window state saved in `dir`, if there is a usable one (unreadable data counts as none).
    pub fn load(dir: &Path) -> Option<Self> {
        Self::parse(&std::fs::read(dir.join(FILE_NAME)).ok()?)
    }

    /// Saves this state in `dir` (created when missing).
    pub fn save(&self, dir: &Path) -> Result<(), SessionError> {
        let bytes =
            serde_json::to_vec_pretty(self).map_err(|e| SessionError::Json(e.to_string()))?;
        let path = dir.join(FILE_NAME);
        write_atomic(&path, &bytes).map_err(|e| SessionError::io(&path, e))
    }
}

/// The geometry to restore when the app starts, before anything is open: the saved window of the
/// folder that will open (the path given on the command line), or else the app's last window. A file
/// opens in Simple mode, which has no per-project entry of its own and uses the app's last window.
pub fn saved_for_launch(app_data_dir: &Path, opening: Option<&Path>) -> Option<WindowState> {
    opening
        .filter(|path| path.is_dir())
        .and_then(|folder| StateStore::open(app_data_dir, folder).load_window())
        .or_else(|| WindowState::load(app_data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample() -> WindowState {
        WindowState {
            x: -1200,
            y: 80,
            width: 1280,
            height: 800,
            scale: 1.5,
            maximized: true,
        }
    }

    #[test]
    fn a_state_round_trips_through_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(WindowState::load(dir.path()), None, "nothing saved yet");
        sample().save(dir.path()).expect("save");
        assert_eq!(WindowState::load(dir.path()), Some(sample()));
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join(FILE_NAME)).expect("read"))
                .expect("json");
        assert_eq!(
            json,
            serde_json::json!({"x": -1200, "y": 80, "width": 1280, "height": 800,
                               "scale": 1.5, "maximized": true})
        );
    }

    #[test]
    fn saving_creates_the_folder_and_replaces_the_previous_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("not").join("yet");
        sample().save(&nested).expect("save");
        let moved = WindowState {
            x: 5,
            maximized: false,
            ..sample()
        };
        moved.save(&nested).expect("save again");
        assert_eq!(WindowState::load(&nested), Some(moved));
    }

    #[test]
    fn damaged_files_count_as_nothing_saved() {
        for text in ["", "{ not json", "[]", "7", "null", "{}", "\"window\""] {
            assert_eq!(WindowState::parse(text.as_bytes()), None, "{text}");
        }
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(FILE_NAME), [0xff, 0xfe, 0x00]).expect("write");
        assert_eq!(WindowState::load(dir.path()), None, "not even text");
    }

    #[test]
    fn a_position_or_size_that_cannot_be_read_leaves_nothing_to_restore() {
        let ok = r#""x": 10, "y": 20, "width": 900, "height": 600"#;
        assert!(WindowState::parse(format!("{{{ok}}}").as_bytes()).is_some());
        for broken in [
            r#"{"y": 20, "width": 900, "height": 600}"#,
            r#"{"x": "10", "y": 20, "width": 900, "height": 600}"#,
            r#"{"x": 10.5, "y": 20, "width": 900, "height": 600}"#,
            r#"{"x": 10, "y": null, "width": 900, "height": 600}"#,
            r#"{"x": 10, "y": 20, "width": 0, "height": 600}"#,
            r#"{"x": 10, "y": 20, "width": 900, "height": -600}"#,
            r#"{"x": 10, "y": 20, "width": 900}"#,
            r#"{"x": 10, "y": 20, "width": 100001, "height": 600}"#,
            r#"{"x": 99999999999, "y": 20, "width": 900, "height": 600}"#,
        ] {
            assert_eq!(WindowState::parse(broken.as_bytes()), None, "{broken}");
        }
    }

    #[test]
    fn the_other_fields_fall_back_to_their_defaults() {
        let s =
            WindowState::parse(br#"{"x": 1, "y": 2, "width": 900, "height": 600}"#).expect("parse");
        assert!((s.scale - 1.0).abs() < f64::EPSILON, "no scale means 1");
        assert!(!s.maximized, "no flag means a normal window");
        for scale in ["0", "-2", "\"big\"", "null", "[]", "1000", "0.01"] {
            let json = format!(
                r#"{{"x": 1, "y": 2, "width": 900, "height": 600, "scale": {scale}, "maximized": true}}"#
            );
            let s = WindowState::parse(json.as_bytes()).expect("parse");
            assert!((s.scale - 1.0).abs() < f64::EPSILON, "{scale}");
            assert!(s.maximized);
        }
        for flag in ["\"yes\"", "1", "null", "[]", "{}"] {
            let json = format!(
                r#"{{"x": 1, "y": 2, "width": 900, "height": 600, "scale": 2, "maximized": {flag}}}"#
            );
            let s = WindowState::parse(json.as_bytes()).expect("parse");
            assert!(!s.maximized, "{flag}");
            assert!((s.scale - 2.0).abs() < f64::EPSILON, "the rest is kept");
        }
    }

    #[test]
    fn fields_from_a_newer_version_are_ignored() {
        let s = WindowState::parse(
            br#"{"x": 1, "y": 2, "width": 900, "height": 600, "scale": 1.25,
                 "monitor": "DISPLAY2", "layout": {"tabs": [1, 2]}}"#,
        )
        .expect("parse");
        assert_eq!((s.x, s.y, s.width, s.height), (1, 2, 900, 600));
    }

    #[test]
    fn a_launch_restores_the_window_of_the_folder_it_opens() {
        let app = tempfile::tempdir().expect("tempdir");
        let project = tempfile::tempdir().expect("tempdir");
        let other = tempfile::tempdir().expect("tempdir");
        let global = WindowState { x: 1, ..sample() };
        let own = WindowState { x: 2, ..sample() };
        global.save(app.path()).expect("save");
        StateStore::open(app.path(), project.path())
            .save_window(&own)
            .expect("save");

        let launch = |path: Option<&Path>| saved_for_launch(app.path(), path);
        assert_eq!(launch(Some(project.path())), Some(own));
        // a project opened for the first time, no project at all (the welcome screen), and a path
        // that does not exist all use the app's last window
        assert_eq!(launch(Some(other.path())), Some(global));
        assert_eq!(launch(None), Some(global));
        assert_eq!(launch(Some(&project.path().join("missing"))), Some(global));
    }

    #[test]
    fn a_single_file_opens_with_the_last_window_even_when_its_folder_has_an_entry() {
        let app = tempfile::tempdir().expect("tempdir");
        let project = tempfile::tempdir().expect("tempdir");
        let global = WindowState { x: 1, ..sample() };
        global.save(app.path()).expect("save");
        StateStore::open(app.path(), project.path())
            .save_window(&WindowState { x: 2, ..sample() })
            .expect("save");
        let file = project.path().join("note.md");
        fs::write(&file, "# note").expect("write");
        assert_eq!(saved_for_launch(app.path(), Some(&file)), Some(global));
    }

    #[test]
    fn a_launch_with_nothing_saved_restores_nothing() {
        let app = tempfile::tempdir().expect("tempdir");
        let project = tempfile::tempdir().expect("tempdir");
        assert_eq!(saved_for_launch(app.path(), None), None);
        assert_eq!(saved_for_launch(app.path(), Some(project.path())), None);
        // the project's own state can be damaged: the last window is used instead
        let store = StateStore::open(app.path(), project.path());
        fs::create_dir_all(store.dir()).expect("mkdir");
        fs::write(store.dir().join(FILE_NAME), "{ not json").expect("write");
        let global = sample();
        global.save(app.path()).expect("save");
        assert_eq!(
            saved_for_launch(app.path(), Some(project.path())),
            Some(global)
        );
    }
}
