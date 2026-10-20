//! `FileScope`: a single-file workspace. The file + read access to assets in its folder; no tree.

use std::path::{Path, PathBuf};

use super::vault::resolve_under;
use super::{fold, slash, Root, VaultError, VaultPath};

#[derive(Debug, Clone)]
pub struct FileScope {
    file: PathBuf,
    folder: Root,
}

impl FileScope {
    /// Opens an existing file (canonical).
    ///
    /// # Errors
    /// `NotFound`, `NotAFile` (a folder), `Io`.
    pub fn open(path: &Path) -> Result<Self, VaultError> {
        let file = dunce::canonicalize(path).map_err(|e| VaultError::io(path, e))?;
        let meta = std::fs::metadata(&file).map_err(|e| VaultError::io(&file, e))?;
        if !meta.is_file() {
            return Err(VaultError::NotAFile(file.display().to_string()));
        }
        let parent = file
            .parent()
            .ok_or_else(|| VaultError::Invalid(file.display().to_string()))?;
        let folder = Root::open(parent)?;
        Ok(Self { file, folder })
    }

    #[must_use]
    pub fn file(&self) -> &Path {
        &self.file
    }

    #[must_use]
    pub fn folder(&self) -> &Root {
        &self.folder
    }

    /// File name for titles.
    #[must_use]
    pub fn name(&self) -> String {
        self.file
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    /// Identity: canonical path, folded where the OS ignores case.
    #[must_use]
    pub fn key(&self) -> String {
        fold(&slash(&self.file))
    }

    /// Asset next to the document (relative to the file's folder), read access only; same boundary checks as a vault.
    ///
    /// # Errors
    /// `Escapes` (outside the folder | via a link), `NotFound`.
    pub fn resolve_asset(&self, path: &VaultPath) -> Result<PathBuf, VaultError> {
        let parts: Vec<&str> = path.segments().collect();
        resolve_under(&self.folder, &parts, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_a_file_with_its_folder_and_refuses_folders() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note book.md");
        std::fs::write(&file, "x").unwrap();
        let scope = FileScope::open(&file).unwrap();
        assert_eq!(scope.name(), "note book.md");
        assert_eq!(scope.folder(), &Root::open(dir.path()).unwrap());
        assert!(matches!(
            FileScope::open(dir.path()),
            Err(VaultError::NotAFile(_))
        ));
        assert!(matches!(
            FileScope::open(&dir.path().join("nope.md")),
            Err(VaultError::NotFound(_))
        ));
    }

    #[test]
    fn assets_stay_inside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");
        std::fs::create_dir_all(docs.join("img")).unwrap();
        std::fs::write(docs.join("a.md"), "x").unwrap();
        std::fs::write(docs.join("img").join("p.png"), "x").unwrap();
        std::fs::write(dir.path().join("secret.md"), "s").unwrap();
        let scope = FileScope::open(&docs.join("a.md")).unwrap();
        assert!(scope
            .resolve_asset(&VaultPath::parse("img/p.png").unwrap())
            .unwrap()
            .is_file());
        assert!(VaultPath::parse("../secret.md").is_err());
    }
}
