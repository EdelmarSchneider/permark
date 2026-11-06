//! Files of the workspace a window shows, for an App-scope plugin that works on what the person points at (the
//! converter reads a file and writes its result next to it). The shell offers it through the app's services; the
//! window's own scope applies (open-folder boundary, single file, Notes).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use unicode_normalization::UnicodeNormalization;

use crate::error::{ErrorKind, IpcError};
use crate::paths::{canonicalize_lenient, RootKey};
use crate::tree::{Scan, ScanFile};
use crate::watch::{FsBatch, Subscription};

/// What a walk below a folder of a project found.
#[derive(Debug, Clone)]
pub struct FolderScan {
    /// The project folder on disk, as the windows name the project: what results of the project are kept under.
    pub project: String,
    pub scan: Scan,
}

/// Hears the verified changes of a project.
pub type ChangeListener = Arc<dyn Fn(&FsBatch) + Send + Sync>;

pub trait WindowFiles: Send + Sync {
    /// The text of `path` in the workspace of `window`, refused when it has more than `cap` bytes.
    ///
    /// # Errors
    /// No such window | outside | missing | too large | not text.
    fn read(&self, window: &str, path: &str, cap: u64) -> Result<String, IpcError>;

    /// A new file `name` next to `beside` in the workspace of `window`, never over a file (a taken name gets ` (2)`).
    /// Returns its path.
    ///
    /// # Errors
    /// No such window | bad name | outside | the write failed.
    fn write_beside(
        &self,
        window: &str,
        beside: &str,
        name: &str,
        text: &str,
    ) -> Result<String, IpcError>;

    /// Notes something the person did in the activity journal of the window's project (nothing for a single file
    /// | the Notes).
    fn note(&self, window: &str, fact: crate::activity::Fact);

    /// Every file below `folder` (a project path; `""` = the project folder) in the project of `window` that the
    /// project's ignore rules leave in, with size and modified time, none of it read. `stop` sees each file and ends
    /// the walk by answering `true`; `cancel` ends it too. The scan is `cut_off` then.
    ///
    /// # Errors
    /// No such window | no project | outside | not a folder.
    fn scan_folder(
        &self,
        window: &str,
        folder: &str,
        cancel: &AtomicBool,
        stop: &mut dyn FnMut(&ScanFile) -> bool,
    ) -> Result<FolderScan, IpcError> {
        let _ = (window, folder, cancel, stop);
        Err(IpcError::new(ErrorKind::Unavailable).subject("project"))
    }

    /// Listens to the verified changes of the project of `window` until the subscription is dropped (`None`: the
    /// window shows no project). The listener runs on the watch thread: keep it short.
    fn watch(&self, window: &str, listener: ChangeListener) -> Option<Subscription> {
        let _ = (window, listener);
        None
    }
}

/// The service plugins look up (`AppServices.services.get::<WindowFilesApi>()`).
#[derive(Clone)]
pub struct WindowFilesApi(pub Arc<dyn WindowFiles>);

/// Project folders the person works in (open in a window | listed as recent), for an App-scope plugin that keeps its
/// own data in a folder the person picks (it MUST never land inside a project, nor hold one).
pub trait ProjectPlaces: Send + Sync {
    /// Folders of the open + recent projects (as they were opened; may be missing now).
    fn folders(&self) -> Vec<PathBuf>;

    /// `path` is one of [`ProjectPlaces::folders`], inside one | holds one. Identity: canonical where the disk has it
    /// (links, 8.3 names, case on disk), missing tail as written, NFC, folded case where the OS ignores case. A
    /// relative `path` overlaps everything (never trusted).
    fn overlaps(&self, path: &Path) -> bool {
        overlapping(path, &self.folders())
    }
}

/// The service plugins look up (`AppServices.services.get::<ProjectPlacesApi>()`).
#[derive(Clone)]
pub struct ProjectPlacesApi(pub Arc<dyn ProjectPlaces>);

/// Shows a folder in the system's file manager (a plugin's own folder: the person asked to see it).
pub trait FolderReveal: Send + Sync {
    /// Opens the file manager on `folder`.
    ///
    /// # Errors
    /// Missing | the system refused.
    fn reveal(&self, folder: &Path) -> Result<(), IpcError>;
}

/// The service plugins look up (`AppServices.services.get::<FolderRevealApi>()`).
#[derive(Clone)]
pub struct FolderRevealApi(pub Arc<dyn FolderReveal>);

/// Identity text of a folder that may not exist yet (see [`ProjectPlaces::overlaps`]); `None`: not absolute.
fn place_key(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return None;
    }
    let canon = canonicalize_lenient(path).unwrap_or_else(|| path.to_path_buf());
    Some(RootKey::of(&canon).as_str().nfc().collect())
}

/// `a` is `b` or below it (keys of [`place_key`]).
fn within(a: &str, b: &str) -> bool {
    a == b
        || a.strip_prefix(b)
            .is_some_and(|rest| b.ends_with('/') || rest.starts_with('/'))
}

/// Where one folder is from another (identity of [`ProjectPlaces::overlaps`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nesting {
    Same,
    /// The first is below the second.
    Inside,
    /// The second is below the first.
    Holds,
    Apart,
}

/// Where folder `a` is from folder `b` (either may not exist yet). A relative path is never apart (never trusted).
#[must_use]
pub fn nesting(a: &Path, b: &Path) -> Nesting {
    let (Some(a), Some(b)) = (place_key(a), place_key(b)) else {
        return Nesting::Same;
    };
    if a == b {
        Nesting::Same
    } else if within(&a, &b) {
        Nesting::Inside
    } else if within(&b, &a) {
        Nesting::Holds
    } else {
        Nesting::Apart
    }
}

fn overlapping(path: &Path, folders: &[PathBuf]) -> bool {
    !path.is_absolute()
        || folders
            .iter()
            .any(|folder| nesting(path, folder) != Nesting::Apart)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::CASE_INSENSITIVE;

    struct Fixed(Vec<PathBuf>);

    impl ProjectPlaces for Fixed {
        fn folders(&self) -> Vec<PathBuf> {
            self.0.clone()
        }
    }

    #[test]
    fn a_folder_inside_or_holding_a_project_overlaps() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("work").join("notes");
        std::fs::create_dir_all(&project).unwrap();
        let places = Fixed(vec![project.clone()]);
        assert!(places.overlaps(&project));
        assert!(places.overlaps(&project.join("models")));
        assert!(places.overlaps(&project.join("a").join("b")));
        assert!(places.overlaps(&tmp.path().join("work")));
        assert!(places.overlaps(tmp.path()));
        assert!(!places.overlaps(&tmp.path().join("models")));
        assert!(!places.overlaps(&tmp.path().join("work").join("notes2")));
        assert!(!places.overlaps(&tmp.path().join("work").join("other")));
        assert!(Fixed(Vec::new()).overlaps(Path::new("relative/models")));
        assert!(!Fixed(Vec::new()).overlaps(&project));
    }

    #[test]
    fn identity_ignores_spelling_case_and_unicode_form() {
        let tmp = tempfile::tempdir().unwrap();
        // decomposed on disk, composed in the request
        let project = tmp.path().join("Cafe\u{301} Proj");
        std::fs::create_dir_all(&project).unwrap();
        let places = Fixed(vec![project.clone()]);
        assert!(places.overlaps(&tmp.path().join("Caf\u{e9} Proj").join("models")));
        assert!(places.overlaps(&project.join("..").join("Cafe\u{301} Proj").join("x")));
        let upper = tmp.path().join("CAFE\u{301} PROJ").join("models");
        assert_eq!(places.overlaps(&upper), CASE_INSENSITIVE);
        // a recent project gone from the disk still counts
        let gone = Fixed(vec![tmp.path().join("gone")]);
        assert!(gone.overlaps(&tmp.path().join("gone").join("m")));
        assert_eq!(
            gone.overlaps(&tmp.path().join("GONE").join("m")),
            CASE_INSENSITIVE
        );
    }

    #[test]
    fn nesting_names_the_direction() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        assert_eq!(nesting(&a, &a), Nesting::Same);
        assert_eq!(nesting(&a.join("b"), &a), Nesting::Inside);
        assert_eq!(nesting(&a, &a.join("b")), Nesting::Holds);
        assert_eq!(nesting(&a, &tmp.path().join("ab")), Nesting::Apart);
        assert_eq!(nesting(Path::new("a"), &a), Nesting::Same);
    }

    #[test]
    fn a_drive_root_holds_everything_below_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp
            .path()
            .ancestors()
            .last()
            .map(Path::to_path_buf)
            .unwrap();
        assert!(Fixed(vec![tmp.path().to_path_buf()]).overlaps(&root));
        assert!(Fixed(vec![root]).overlaps(&tmp.path().join("models")));
    }
}
