//! Where the notes live.
//!
//! The folder is `Documents/TrailMark Notes` (or `~/TrailMark Notes` when the system has no Documents folder). It is not in
//! the app's data folder: that one is removed when the app is uninstalled with its data, and by the Store's uninstall,
//! which would take months of notes with it. The person can point TrailMark at another folder (a synced one, say).
//!
//! A debug build never defaults to the real folder: it uses `<app data>/notes`, and honours `TRAILMARK_TEST_NOTES_DIR`
//! (like `TRAILMARK_TEST_DATA_DIR` for the app's data), so that development and tests cannot touch real notes.
//!
//! A folder that is configured but not there (a disconnected drive, a renamed folder) is reported as missing. It is never
//! created quietly: the person chooses to locate it or to create it.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The name of the default folder.
pub(crate) const FOLDER_NAME: &str = "TrailMark Notes";
/// The environment variable that points a debug build at another notes folder. Release builds ignore it.
pub(crate) const TEST_NOTES_DIR_VARIABLE: &str = "TRAILMARK_TEST_NOTES_DIR";

/// What is known about the places notes can go on this system.
#[derive(Debug, Clone)]
pub(crate) struct Places {
    /// The system's Documents folder, when it has one.
    pub documents: Option<PathBuf>,
    pub home: Option<PathBuf>,
    /// TrailMark's own data folder.
    pub app_data: PathBuf,
    /// Whether this is a debug build.
    pub debug: bool,
}

impl Places {
    /// The places on this system for the app whose data is in `app_data`.
    pub(crate) fn current(app_data: &Path) -> Self {
        Self {
            documents: dirs::document_dir().filter(|dir| dir.is_dir()),
            home: dirs::home_dir(),
            app_data: app_data.to_path_buf(),
            debug: cfg!(debug_assertions),
        }
    }

    /// The folder notes go to when nothing else is chosen: the real one in a release build, a private one in a debug build.
    pub(crate) fn default_folder(&self) -> PathBuf {
        if self.debug {
            return self.app_data.join("notes");
        }
        self.real_default()
    }

    /// `Documents/TrailMark Notes`, or `~/TrailMark Notes` without a Documents folder.
    fn real_default(&self) -> PathBuf {
        self.documents
            .as_ref()
            .or(self.home.as_ref())
            .map_or_else(|| self.app_data.join("notes"), |dir| dir.join(FOLDER_NAME))
    }

    /// The notes folder: `from_env` (the test variable, honoured in debug builds only), else the folder the person chose,
    /// else the default. A debug build also refuses a chosen folder that is the real default, which a development build
    /// sharing the person's settings would otherwise open.
    pub(crate) fn resolve(&self, chosen: Option<&str>, from_env: Option<OsString>) -> PathBuf {
        if self.debug {
            if let Some(dir) = from_env.filter(|dir| !dir.is_empty()) {
                return PathBuf::from(dir);
            }
        }
        let chosen = chosen
            .map(str::trim)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute());
        match chosen {
            Some(dir) if !(self.debug && same_place(&dir, &self.real_default())) => dir,
            _ => self.default_folder(),
        }
    }
}

/// Whether two paths name the same place, ignoring case on Windows and a trailing separator.
fn same_place(a: &Path, b: &Path) -> bool {
    let key = |p: &Path| {
        let text = dunce::simplified(p)
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_owned();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    key(a) == key(b)
}

/// The notes folder for the app whose data is in `app_data`, given what the person chose.
pub(crate) fn configured(app_data: &Path, chosen: Option<&str>) -> PathBuf {
    Places::current(app_data).resolve(chosen, std::env::var_os(TEST_NOTES_DIR_VARIABLE))
}

/// Whether the notes folder is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderStatus {
    Ready,
    /// Not there: the person can locate it or create it.
    Missing,
}

/// The notes folder as the UI needs it. Matches the UI's `NotesFolder`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesFolder {
    pub path: String,
    pub status: FolderStatus,
    /// The folder is the default one, not one the person chose.
    pub is_default: bool,
}

/// What is known about the notes folder `path`.
pub(crate) fn describe(path: &Path, is_default: bool) -> NotesFolder {
    NotesFolder {
        path: path.display().to_string(),
        status: if path.is_dir() {
            FolderStatus::Ready
        } else {
            FolderStatus::Missing
        },
        is_default,
    }
}

/// Makes the folder the person asked to have created ("Create it"). Parent folders are made too.
pub(crate) fn create(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Whether `path` is the notes folder or inside it. The agent connection never serves it.
pub(crate) fn contains(app_data: &Path, chosen: Option<&str>, path: &Path) -> bool {
    let folder = configured(app_data, chosen);
    let canonical =
        |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| dunce::simplified(p).to_path_buf());
    crate::vault::strip_root(&canonical(path), &canonical(&folder)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full path on any system: `C:` is put in front of it on Windows.
    fn abs(path: &str) -> String {
        if cfg!(windows) {
            format!("C:{path}")
        } else {
            path.to_owned()
        }
    }

    fn places(debug: bool, documents: Option<&str>) -> Places {
        Places {
            documents: documents.map(|dir| PathBuf::from(abs(dir))),
            home: Some(PathBuf::from(abs("/home/ana"))),
            app_data: PathBuf::from(abs("/data/dev.example.app")),
            debug,
        }
    }

    #[test]
    fn a_release_build_defaults_to_documents_then_to_the_home_folder() {
        assert_eq!(
            places(false, Some("/home/ana/Documents")).default_folder(),
            Path::new(&abs("/home/ana/Documents")).join(FOLDER_NAME)
        );
        assert_eq!(
            places(false, None).default_folder(),
            Path::new(&abs("/home/ana")).join(FOLDER_NAME),
            "a Linux without an XDG Documents folder"
        );
        let nowhere = Places {
            home: None,
            ..places(false, None)
        };
        assert_eq!(
            nowhere.default_folder(),
            Path::new(&abs("/data/dev.example.app")).join("notes")
        );
    }

    #[test]
    fn a_debug_build_never_defaults_to_the_real_folder() {
        let debug = places(true, Some("/home/ana/Documents"));
        let own = Path::new(&abs("/data/dev.example.app")).join("notes");
        assert_eq!(debug.default_folder(), own);
        assert_eq!(debug.resolve(None, None), own);
    }

    #[test]
    fn the_test_variable_wins_in_a_debug_build_and_is_ignored_in_a_release_build() {
        let var = Some(OsString::from(abs("/tmp/test-notes")));
        let debug = places(true, Some("/home/ana/Documents"));
        let elsewhere = abs("/elsewhere");
        assert_eq!(
            debug.resolve(Some(&elsewhere), var.clone()),
            Path::new(&abs("/tmp/test-notes"))
        );
        assert_eq!(
            debug.resolve(None, Some(OsString::new())),
            debug.default_folder()
        );
        let release = places(false, Some("/home/ana/Documents"));
        assert_eq!(release.resolve(None, var), release.default_folder());
    }

    #[test]
    fn the_folder_the_person_chose_beats_the_default() {
        let release = places(false, Some("/home/ana/Documents"));
        let sync = abs("/sync/notes");
        assert_eq!(release.resolve(Some(&sync), None), Path::new(&sync));
        assert_eq!(
            release.resolve(Some(&format!("  {sync}  ")), None),
            Path::new(&sync)
        );
        // a choice that is not a full path is not a choice
        for odd in ["", "   ", "notes", "./notes"] {
            assert_eq!(
                release.resolve(Some(odd), None),
                release.default_folder(),
                "{odd:?}"
            );
        }
    }

    #[test]
    fn a_debug_build_does_not_open_the_real_folder_even_when_the_settings_name_it() {
        let debug = places(true, Some("/home/ana/Documents"));
        let own = debug.default_folder();
        let real = abs("/home/ana/Documents/TrailMark Notes");
        assert_eq!(debug.resolve(Some(&real), None), own);
        assert_eq!(debug.resolve(Some(&format!("{real}/")), None), own);
        // a folder the person picked on purpose is theirs
        let sync = abs("/sync/notes");
        assert_eq!(debug.resolve(Some(&sync), None), Path::new(&sync));
    }

    #[test]
    fn a_folder_that_is_not_there_is_reported_and_not_made() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("TrailMark Notes");
        let folder = describe(&missing, true);
        assert_eq!(folder.status, FolderStatus::Missing);
        assert!(folder.is_default);
        assert!(!missing.exists(), "describing never creates it");
        assert_eq!(describe(dir.path(), false).status, FolderStatus::Ready);
        create(&missing).expect("create");
        assert_eq!(describe(&missing, true).status, FolderStatus::Ready);
    }

    #[test]
    fn the_notes_tree_is_the_folder_and_what_is_inside_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(notes.join("sub")).expect("mkdir");
        let chosen = notes.display().to_string();
        assert!(contains(dir.path(), Some(&chosen), &notes));
        assert!(contains(dir.path(), Some(&chosen), &notes.join("sub")));
        assert!(contains(
            dir.path(),
            Some(&chosen),
            &notes.join("not-yet").join("x.md")
        ));
        assert!(!contains(dir.path(), Some(&chosen), dir.path()));
        assert!(!contains(
            dir.path(),
            Some(&chosen),
            &dir.path().join("other")
        ));
    }
}
