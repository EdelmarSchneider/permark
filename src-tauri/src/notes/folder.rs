//! Where the notes live.
//!
//! The folder is `Documents/TrailMark Notes` (or `~/TrailMark Notes` when the system has no Documents folder). It is not in
//! the app's data folder: that one is removed when the app is uninstalled with its data, and by the Store's uninstall,
//! which would take months of notes with it. The person can point TrailMark at another folder (a synced one, say).
//!
//! A debug build never defaults to the real folder: it uses `<app data>/notes`, and honours `TRAILMARK_TEST_NOTES_DIR`
//! (like `TRAILMARK_TEST_DATA_DIR` for the app's data), so that development and tests cannot touch real notes.
//!
//! The default folder is made quietly the first time it is needed: nobody is asked about a folder they never heard of. A folder
//! the person chose that is not there (a disconnected drive, a renamed folder) is reported as missing instead, and is never made
//! quietly: an empty folder made in its place would split the notes in two. The person chooses to locate it or to create it.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::util::canonicalize_lenient;

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
        self.resolve_origin(chosen, from_env).path
    }

    /// [`Places::resolve`], and whether the folder is the default one (the test variable counts as the default's place: it is
    /// where a development build keeps its notes) or one the person chose.
    pub(crate) fn resolve_origin(
        &self,
        chosen: Option<&str>,
        from_env: Option<OsString>,
    ) -> Resolved {
        if self.debug {
            if let Some(dir) = from_env.filter(|dir| !dir.is_empty()) {
                return Resolved {
                    path: PathBuf::from(dir),
                    is_default: true,
                };
            }
        }
        let chosen = chosen
            .map(str::trim)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute());
        match chosen {
            Some(dir) if !(self.debug && same_place(&dir, &self.real_default())) => Resolved {
                path: dir,
                is_default: false,
            },
            _ => Resolved {
                path: self.default_folder(),
                is_default: true,
            },
        }
    }
}

/// The notes folder, and whether it is the default one or one the person chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub path: PathBuf,
    pub is_default: bool,
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

/// The notes folder for the app whose data is in `app_data`, given what the person chose, and whether it is the default one.
pub(crate) fn configured_origin(app_data: &Path, chosen: Option<&str>) -> Resolved {
    Places::current(app_data).resolve_origin(chosen, std::env::var_os(TEST_NOTES_DIR_VARIABLE))
}

/// Whether the notes folder is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderStatus {
    /// There, or the default folder, which is made when it is needed.
    Ready,
    /// A folder the person chose that is not there: they can locate it or create it. The default folder is never missing.
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

/// What is known about the notes folder `path`. Nothing is made here. The default folder is reported as ready even before it
/// exists, since [`ensure`] makes it when it is needed; only a folder the person chose can be missing.
pub(crate) fn describe(path: &Path, is_default: bool) -> NotesFolder {
    NotesFolder {
        path: path.display().to_string(),
        status: if is_default || path.is_dir() {
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

/// Gets the folder ready for the notes to be opened: the default folder is made when it is not there, silently. A folder the
/// person chose is left alone, there or not: if it is gone the person is told ([`FolderStatus::Missing`]) and decides.
pub(crate) fn ensure(path: &Path, is_default: bool) -> io::Result<()> {
    if is_default && !path.is_dir() {
        return create(path);
    }
    Ok(())
}

/// Whether `path` is the notes folder or inside it. The agent connection never serves it.
pub(crate) fn contains(app_data: &Path, chosen: Option<&str>, path: &Path) -> bool {
    let folder = configured(app_data, chosen);
    // canonical even where the end of the path is not there (yet): `/var` is `/private/var` on macOS, a short name is a long one
    let canonical = |p: &Path| canonicalize_lenient(p).unwrap_or_else(|| p.to_path_buf());
    crate::vault::strip_root(&canonical(path), &canonical(&folder)).is_some()
}

/// The notes folder, when it lies strictly inside the project in `root` (a project that holds the folder of the notes, or a notes
/// folder chosen inside a project), as the system spells it. It is what the agent connection of that project must never reach.
pub(crate) fn inside(app_data: &Path, chosen: Option<&str>, root: &Path) -> Option<PathBuf> {
    let folder = configured(app_data, chosen);
    let canonical = |p: &Path| canonicalize_lenient(p).unwrap_or_else(|| p.to_path_buf());
    let (folder, root) = (canonical(&folder), canonical(root));
    let rest = crate::vault::strip_root(&folder, &root)?;
    (!rest.is_empty()).then_some(folder)
}

/// The ignore line that keeps the notes folder out of the project in `root`, when the notes folder lies inside it (a project
/// that holds the folder of the notes, or a notes folder chosen inside a project). The line is anchored at the project's root,
/// so it names this folder only, and it is added to the project's own ignore globs: what is ignored is not indexed, not searched
/// and not listed as unread, so an agent connected to the project is never shown a note by them (a note the person shares is read
/// through `read_note`, one at a time).
pub(crate) fn ignore_inside(app_data: &Path, chosen: Option<&str>, root: &Path) -> Option<String> {
    let folder = configured(app_data, chosen);
    let canonical = |p: &Path| canonicalize_lenient(p).unwrap_or_else(|| p.to_path_buf());
    let (folder, root) = (canonical(&folder), canonical(root));
    let inside = crate::vault::strip_root(&folder, &root)?;
    if inside.is_empty() {
        return None;
    }
    let mut line = String::from("/");
    for part in inside {
        line.push_str(&escaped_glob(&part.to_string_lossy()));
        line.push('/');
    }
    Some(line)
}

/// A name as one piece of an ignore line, with the characters that mean something in one taken literally.
fn escaped_glob(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    // a space at the end of a line is dropped unless it is escaped
    if out.ends_with(' ') {
        out.pop();
        out.push_str("\\ ");
    }
    out
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
    fn a_project_that_holds_the_notes_folder_ignores_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dunce::canonicalize(dir.path()).expect("canonical");
        let app = root.join("app-data");
        let notes = root.join("docs").join("My [notes]");
        let chosen = notes.display().to_string();
        assert_eq!(
            ignore_inside(&app, Some(&chosen), &root).as_deref(),
            Some("/docs/My \\[notes\\]/")
        );
        // the folder that is not there yet is ignored as well
        assert!(!notes.exists());
        // a project beside the notes, or the notes folder itself, has nothing to ignore
        let beside = root.join("project");
        assert_eq!(ignore_inside(&app, Some(&chosen), &beside), None);
        assert_eq!(ignore_inside(&app, Some(&chosen), &notes), None);
        // and a project inside the notes folder is not one (it is never found), so it has nothing to ignore either
        assert_eq!(ignore_inside(&app, Some(&chosen), &notes.join("sub")), None);
    }

    #[test]
    fn a_name_is_taken_literally_in_an_ignore_line() {
        assert_eq!(escaped_glob("plain"), "plain");
        assert_eq!(escaped_glob("a*b?c[d]e\\f"), "a\\*b\\?c\\[d\\]e\\\\f");
        assert_eq!(escaped_glob("trailing "), "trailing\\ ");
    }

    #[test]
    fn the_default_folder_is_made_when_it_is_needed_and_never_by_looking_at_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let default = dir.path().join("Documents").join("TrailMark Notes");
        // looking at it says it is ready, and makes nothing: the welcome screen only reads
        let folder = describe(&default, true);
        assert_eq!(folder.status, FolderStatus::Ready);
        assert!(folder.is_default);
        assert!(!default.exists(), "describing never creates it");
        // opening the notes makes it, with the folders above it, and again is harmless
        ensure(&default, true).expect("ensure");
        assert!(default.is_dir());
        ensure(&default, true).expect("ensure again");
        assert_eq!(describe(&default, true).status, FolderStatus::Ready);
    }

    #[test]
    fn a_folder_the_person_chose_that_is_gone_is_reported_and_never_made() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gone = dir.path().join("Unplugged drive").join("Notes");
        let folder = describe(&gone, false);
        assert_eq!(folder.status, FolderStatus::Missing);
        assert!(!folder.is_default);
        // opening the notes does not make it: an empty folder there would split the notes in two
        ensure(&gone, false).expect("ensure");
        assert!(!gone.exists());
        assert_eq!(describe(&gone, false).status, FolderStatus::Missing);
        // the person can still ask for it ("Create it")
        create(&gone).expect("create");
        assert_eq!(describe(&gone, false).status, FolderStatus::Ready);
        assert_eq!(describe(dir.path(), false).status, FolderStatus::Ready);
    }

    #[test]
    fn a_default_folder_that_cannot_be_made_is_an_error_not_a_missing_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        // a file where the folder should be
        let blocked = dir.path().join("TrailMark Notes");
        std::fs::write(&blocked, "not a folder").expect("write");
        assert!(ensure(&blocked, true).is_err());
        assert!(blocked.is_file(), "nothing was replaced");
    }

    #[test]
    fn the_folder_is_the_default_unless_the_person_chose_it() {
        let release = places(false, Some("/home/ana/Documents"));
        let sync = abs("/sync/notes");
        let chosen = release.resolve_origin(Some(&sync), None);
        assert_eq!(chosen.path, Path::new(&sync));
        assert!(!chosen.is_default);
        for none_chosen in [None, Some(""), Some("   "), Some("notes")] {
            let resolved = release.resolve_origin(none_chosen, None);
            assert_eq!(resolved.path, release.default_folder(), "{none_chosen:?}");
            assert!(resolved.is_default, "{none_chosen:?}");
        }
        // a person who picks the very folder that is the default made it their own choice: it is reported when it is gone
        let same = release.default_folder().display().to_string();
        assert!(!release.resolve_origin(Some(&same), None).is_default);

        // a development build: its own folder, the test variable, and a refusal of the real folder, all of them the default's place
        let debug = places(true, Some("/home/ana/Documents"));
        assert!(debug.resolve_origin(None, None).is_default);
        let var = Some(OsString::from(abs("/tmp/test-notes")));
        let tested = debug.resolve_origin(Some(&sync), var);
        assert_eq!(tested.path, Path::new(&abs("/tmp/test-notes")));
        assert!(tested.is_default);
        let real = abs("/home/ana/Documents/TrailMark Notes");
        let refused = debug.resolve_origin(Some(&real), None);
        assert_eq!(refused.path, debug.default_folder());
        assert!(refused.is_default);
        assert!(!debug.resolve_origin(Some(&sync), None).is_default);
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
