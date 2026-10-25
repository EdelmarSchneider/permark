//! Where the notes live.
//!
//! The folder is `Documents/PerMark Notes` (or `~/PerMark Notes` when the system has no Documents folder), as in 0.5. It
//! is not in the app's data folder: that one is removed when the app is uninstalled with its data, and by the Store's
//! uninstall, which would take months of notes with it. The person can point PerMark at another folder (a synced one).
//!
//! A debug build never defaults to the real folder: it uses `<profile>/notes`, and it refuses a chosen folder that is
//! the real default, so that development and tests cannot touch real notes.
//!
//! The default folder is made quietly the first time it is needed: nobody is asked about a folder they never heard of.
//! A folder the person chose that is not there (a disconnected drive, a renamed folder) is reported as missing instead,
//! and is never made quietly: an empty folder made in its place would split the notes in two.

use std::io;
use std::path::{Path, PathBuf};

use permark_core::paths::fold;
use serde::Serialize;
use ts_rs::TS;

/// The name of the default folder.
pub const FOLDER_NAME: &str = "PerMark Notes";

/// What is known about the places notes can go on this system.
#[derive(Debug, Clone)]
pub struct Places {
    /// The system's Documents folder, when it has one.
    pub documents: Option<PathBuf>,
    pub home: Option<PathBuf>,
    /// PerMark's own data folder.
    pub profile: PathBuf,
    /// Whether this is a debug build.
    pub debug: bool,
}

/// The notes folder, and whether it is the default one or one the person chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub path: PathBuf,
    pub is_default: bool,
}

impl Places {
    /// The places on this system for the app whose data is in `profile`.
    #[must_use]
    pub fn current(profile: &Path) -> Self {
        Self {
            documents: dirs::document_dir().filter(|dir| dir.is_dir()),
            home: dirs::home_dir(),
            profile: profile.to_path_buf(),
            debug: cfg!(debug_assertions),
        }
    }

    /// `Documents/PerMark Notes`, or `~/PerMark Notes` without a Documents folder.
    #[must_use]
    pub fn real_default(&self) -> PathBuf {
        self.documents
            .as_ref()
            .or(self.home.as_ref())
            .map_or_else(|| self.profile.join("notes"), |dir| dir.join(FOLDER_NAME))
    }

    /// The folder notes go to when nothing else is chosen: the real one in a release build, a private one in a debug
    /// build.
    #[must_use]
    pub fn default_folder(&self) -> PathBuf {
        if self.debug {
            self.profile.join("notes")
        } else {
            self.real_default()
        }
    }

    /// The notes folder for the folder the person chose (empty = none chosen). A debug build refuses a chosen folder
    /// that is the real default.
    #[must_use]
    pub fn resolve(&self, chosen: &str) -> Resolved {
        let chosen = Some(chosen.trim())
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute());
        match chosen {
            Some(path) if !(self.debug && same_place(&path, &self.real_default())) => Resolved {
                path,
                is_default: false,
            },
            _ => Resolved {
                path: self.default_folder(),
                is_default: true,
            },
        }
    }
}

/// Whether two paths name the same place, ignoring case where the system does and a trailing separator. A path that is
/// there is compared as the system resolves it (links, a Windows 8.3 short name): a folder reached two ways is one.
#[must_use]
pub fn same_place(a: &Path, b: &Path) -> bool {
    place_key(a) == place_key(b)
}

/// The comparable spelling of a path: resolved when it is there, `/` separators, no trailing one, case folded where
/// the system ignores case.
fn place_key(path: &Path) -> String {
    let real = permark_core::paths::canonicalize(path)
        .unwrap_or_else(|_| permark_core::paths::simplified(path).into_owned());
    let text = real.to_string_lossy().replace('\\', "/");
    fold(text.trim_end_matches('/'))
}

/// Whether the notes folder is there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum FolderStatus {
    /// There, or the default folder, which is made when it is needed.
    Ready,
    /// A folder the person chose that is not there: they can locate it or create it.
    Missing,
    /// The notes can be read but the system does not let this program write here.
    ReadOnly { reason: String },
}

/// The notes folder as the window needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderInfo {
    pub path: String,
    pub status: FolderStatus,
    /// The folder is the default one, not one the person chose.
    pub is_default: bool,
}

/// What is known about the folder. Nothing is made here: the default folder counts as ready even before it exists,
/// since [`ensure`] makes it when it is needed; only a folder the person chose can be missing.
#[must_use]
pub fn info_of(resolved: &Resolved) -> FolderInfo {
    let path = &resolved.path;
    let status = match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => {
            read_only_reason(&meta).map_or(FolderStatus::Ready, |reason| FolderStatus::ReadOnly {
                reason: reason.to_owned(),
            })
        }
        Err(_) if resolved.is_default => FolderStatus::Ready,
        Ok(_) | Err(_) => FolderStatus::Missing,
    };
    FolderInfo {
        path: path.display().to_string(),
        status,
        is_default: resolved.is_default,
    }
}

/// Why the folder cannot be written, when the system says so up front. Windows marks special folders (Documents among
/// them) read-only for other reasons, so there a refused write is what tells.
#[cfg(unix)]
fn read_only_reason(meta: &std::fs::Metadata) -> Option<&'static str> {
    use std::os::unix::fs::PermissionsExt as _;
    (meta.permissions().mode() & 0o222 == 0).then_some("permissions")
}

#[cfg(not(unix))]
fn read_only_reason(_meta: &std::fs::Metadata) -> Option<&'static str> {
    None
}

/// Makes the folder (parents too): the default folder when it is first needed, a chosen one when the person asked to
/// create it.
///
/// # Errors
/// The system refused.
pub fn create(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Whether `path` is the notes folder or inside it. The agent connection never serves it.
#[must_use]
pub fn is_inside(path: &Path, folder: &Path) -> bool {
    let (path, folder) = (place_key(path), place_key(folder));
    path == folder || path.starts_with(&format!("{folder}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places(debug: bool) -> Places {
        Places {
            documents: Some(PathBuf::from("/home/ana/Documents")),
            home: Some(PathBuf::from("/home/ana")),
            profile: PathBuf::from("/home/ana/.local/PerMark"),
            debug,
        }
    }

    #[test]
    fn the_default_is_documents_permark_notes_as_in_0_5() {
        let places = places(false);
        assert_eq!(
            places.default_folder(),
            PathBuf::from("/home/ana/Documents").join("PerMark Notes")
        );
        let r = places.resolve("");
        assert!(r.is_default);
        assert_eq!(r.path, places.default_folder());
    }

    #[test]
    fn without_documents_the_home_folder_holds_it_and_without_both_the_profile() {
        let mut p = places(false);
        p.documents = None;
        assert_eq!(p.default_folder(), PathBuf::from("/home/ana/PerMark Notes"));
        p.home = None;
        assert_eq!(p.default_folder(), p.profile.join("notes"));
    }

    #[test]
    fn a_debug_build_uses_a_private_folder_and_refuses_the_real_one() {
        let p = places(true);
        assert_eq!(p.default_folder(), p.profile.join("notes"));
        let real = p.real_default().display().to_string();
        // the setting names the real folder: ignored in a debug build
        let r = p.resolve(&real);
        assert!(r.is_default);
        assert_eq!(r.path, p.profile.join("notes"));
        // another place is honored
        let other = if cfg!(windows) {
            "C:\\notes-x"
        } else {
            "/tmp/notes-x"
        };
        assert_eq!(p.resolve(other).path, PathBuf::from(other));
    }

    #[test]
    fn a_chosen_folder_must_be_absolute() {
        let p = places(false);
        let r = p.resolve("relative/notes");
        assert!(r.is_default);
        let abs = if cfg!(windows) {
            "D:\\sync\\notes"
        } else {
            "/srv/notes"
        };
        let r = p.resolve(&format!("  {abs}  "));
        assert!(!r.is_default);
        assert_eq!(r.path, PathBuf::from(abs));
    }

    #[test]
    fn places_compare_without_case_or_trailing_separators_where_the_system_ignores_them() {
        assert!(same_place(Path::new("/a/b/"), Path::new("/a/b")));
        assert!(same_place(Path::new("C:\\a\\b"), Path::new("C:/a/b/")));
        if cfg!(any(windows, target_os = "macos")) {
            assert!(same_place(Path::new("/A/B"), Path::new("/a/b")));
        }
        assert!(!same_place(Path::new("/a/b"), Path::new("/a/bc")));
    }

    #[test]
    fn a_folder_that_is_there_is_one_place_however_it_is_reached() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(notes.join("sub")).unwrap();
        let roundabout = notes.join("sub").join("..");
        assert!(same_place(&roundabout, &notes));
        assert!(same_place(&dunce::canonicalize(&notes).unwrap(), &notes));
        std::fs::write(notes.join("a.md"), "x").unwrap();
        assert!(is_inside(&roundabout.join("a.md"), &notes));
        assert!(is_inside(
            &notes.join("a.md"),
            &dunce::canonicalize(&notes).unwrap()
        ));
        assert!(!is_inside(dir.path(), &notes));
    }

    #[test]
    fn the_default_folder_is_ready_before_it_exists_and_a_chosen_one_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        let default = info_of(&Resolved {
            path: gone.clone(),
            is_default: true,
        });
        assert_eq!(default.status, FolderStatus::Ready);
        let chosen = info_of(&Resolved {
            path: gone.clone(),
            is_default: false,
        });
        assert_eq!(chosen.status, FolderStatus::Missing);
        assert!(!gone.exists(), "nothing is made by asking");
        create(&gone).unwrap();
        let chosen = info_of(&Resolved {
            path: gone,
            is_default: false,
        });
        assert_eq!(chosen.status, FolderStatus::Ready);
    }

    #[test]
    fn a_file_in_place_of_the_folder_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes");
        std::fs::write(&file, "x").unwrap();
        let info = info_of(&Resolved {
            path: file,
            is_default: false,
        });
        assert_eq!(info.status, FolderStatus::Missing);
    }

    #[test]
    fn inside_means_the_folder_itself_or_below_it_not_a_sibling_with_the_same_start() {
        let folder = Path::new("/home/ana/Documents/PerMark Notes");
        assert!(is_inside(folder, folder));
        assert!(is_inside(&folder.join("2026.md"), folder));
        assert!(!is_inside(
            Path::new("/home/ana/Documents/PerMark Notes 2/x.md"),
            folder
        ));
        assert!(!is_inside(Path::new("/home/ana/Documents"), folder));
    }
}
