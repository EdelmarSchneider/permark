//! `Root`: a canonical absolute folder. Only `Root::open` builds one.

use std::fmt;
use std::path::{Path, PathBuf};

use super::{fold, slash, VaultError};

/// Canonical absolute existing folder (symlinks resolved, case as on disk, `\\?\` dropped unless needed).
#[derive(Debug, Clone)]
pub struct Root {
    path: PathBuf,
    key: RootKey,
}

/// Identity of a root: `/` separators, folded case on case-insensitive systems. Same folder -> same key, whatever
/// spelling (8.3 names, `..`, case, symlinked alias) it was opened with.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootKey(String);

impl RootKey {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn of(path: &Path) -> Self {
        let mut text = slash(path);
        while text.len() > 1 && text.ends_with('/') && !text.ends_with(":/") {
            text.pop();
        }
        Self(fold(&text))
    }
}

impl Root {
    /// Opens a folder: canonicalized; must exist + be a folder.
    ///
    /// # Errors
    /// `NotFound`, `NotAFolder`, `Io` (access denied, ...).
    pub fn open(path: &Path) -> Result<Self, VaultError> {
        let path = dunce::canonicalize(path).map_err(|e| VaultError::io(path, e))?;
        let meta = std::fs::metadata(&path).map_err(|e| VaultError::io(&path, e))?;
        if !meta.is_dir() {
            return Err(VaultError::NotAFolder(path.display().to_string()));
        }
        let key = RootKey::of(&path);
        Ok(Self { path, key })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn key(&self) -> &RootKey {
        &self.key
    }

    /// Folder name for titles (drive root -> the path itself).
    #[must_use]
    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

impl PartialEq for Root {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for Root {}

impl fmt::Display for Root {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.path.display().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spelling_of_a_folder_is_one_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let a = Root::open(dir.path()).unwrap();
        let b = Root::open(&dir.path().join("sub").join("..")).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.key(), b.key());
        assert!(!a.path().to_string_lossy().starts_with(r"\\?\"));
    }

    #[test]
    fn missing_and_file_paths_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            Root::open(&dir.path().join("nope")),
            Err(VaultError::NotFound(_))
        ));
        let file = dir.path().join("a.md");
        std::fs::write(&file, "x").unwrap();
        assert!(matches!(Root::open(&file), Err(VaultError::NotAFolder(_))));
    }

    #[test]
    fn unicode_and_space_folders_open() {
        let dir = tempfile::tempdir().unwrap();
        let odd = dir.path().join("Projeto ação 文档 📁");
        std::fs::create_dir(&odd).unwrap();
        let root = Root::open(&odd).unwrap();
        assert_eq!(root.name(), "Projeto ação 文档 📁");
    }

    #[cfg(windows)]
    #[test]
    fn case_and_verbatim_spellings_share_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let a = Root::open(dir.path()).unwrap();
        let upper = PathBuf::from(dir.path().to_string_lossy().to_uppercase());
        let b = Root::open(&upper).unwrap();
        assert_eq!(a.key(), b.key());
        let verbatim = PathBuf::from(format!(r"\\?\{}", a.path().display()));
        assert_eq!(Root::open(&verbatim).unwrap().key(), a.key());
    }

    #[test]
    fn drive_root_key_keeps_its_slash() {
        #[cfg(windows)]
        assert_eq!(RootKey::of(Path::new("C:\\")).as_str(), "c:/");
        #[cfg(unix)]
        assert_eq!(RootKey::of(Path::new("/")).as_str(), "/");
    }
}
