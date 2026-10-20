//! `NotesScope`: where the documents of the Notes tab are. The notes folder (a vault: notes and the pictures
//! next to them) and the loose files (any text file the person opened there, one scope each).
//!
//! Document paths: a note is its file name in the folder; a loose file is `@file/<id>/<name>`; anything else under
//! `@file/<id>/` is a file next to it (a picture). The window never sees a loose file's real path.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use crate::edit::SaveObserver;
use crate::error::IpcError;
use crate::paths::{fold, FileScope, Root, Vault, VaultError, VaultPath};

/// What starts the document path of a loose file.
pub const LOOSE_PREFIX: &str = "@file/";
/// How many hexadecimal digits a loose file's id has.
const ID_DIGITS: usize = 8;

/// `@file/<id>/<name>`.
#[must_use]
pub fn loose_path(id: &str, name: &str) -> String {
    format!("{LOOSE_PREFIX}{id}/{name}")
}

/// `@file/<id>`: what pins and the open note are kept under, so they follow a file that is renamed.
#[must_use]
pub fn loose_key(id: &str) -> String {
    format!("{LOOSE_PREFIX}{id}")
}

/// Whether `id` is an id: eight lowercase hexadecimal digits.
#[must_use]
pub fn is_loose_id(id: &str) -> bool {
    id.len() == ID_DIGITS
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A document path of a loose file taken apart: the id and what follows it (the file's name, or a file next to it).
#[must_use]
pub fn parse_loose(path: &str) -> Option<(&str, &str)> {
    let (id, rest) = path.strip_prefix(LOOSE_PREFIX)?.split_once('/')?;
    (is_loose_id(id) && !rest.is_empty()).then_some((id, rest))
}

/// The id of a loose file out of a key (`@file/<id>`) or a document path.
#[must_use]
pub fn loose_id_of(key_or_path: &str) -> Option<&str> {
    let id = key_or_path.strip_prefix(LOOSE_PREFIX)?.split('/').next()?;
    is_loose_id(id).then_some(id)
}

#[derive(Default)]
struct Inner {
    folder: Option<Vault>,
    loose: BTreeMap<String, FileScope>,
}

/// The documents of the Notes tab. Changed at run time (the folder is chosen, files are opened), so the state sits
/// behind a lock; reads clone what they need.
#[derive(Default)]
pub struct NotesScope {
    inner: RwLock<Inner>,
    observer: Mutex<Option<Arc<dyn SaveObserver>>>,
}

impl std::fmt::Debug for NotesScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotesScope").finish_non_exhaustive()
    }
}

/// Teardown symmetry: the observer goes with the lease.
pub struct ObserverLease {
    scope: std::sync::Weak<NotesScope>,
}

impl Drop for ObserverLease {
    fn drop(&mut self) {
        if let Some(scope) = self.scope.upgrade() {
            *scope
                .observer
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = None;
        }
    }
}

impl NotesScope {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The notes folder is now `root` (`None`: there is none to read).
    pub fn set_folder(&self, root: Option<Root>) {
        self.inner
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .folder = root.map(Vault::new);
    }

    /// The notes folder, when there is one.
    #[must_use]
    pub fn folder(&self) -> Option<Root> {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .folder
            .as_ref()
            .map(|vault| vault.main().clone())
    }

    /// Makes the file at `path` the loose file `id` (replaces an older file of that id).
    ///
    /// # Errors
    /// The file is not there | not a file.
    pub fn set_loose(&self, id: &str, path: &Path) -> Result<(), VaultError> {
        let scope = FileScope::open(path)?;
        self.inner
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .loose
            .insert(id.to_owned(), scope);
        Ok(())
    }

    /// Forgets the loose file `id`.
    pub fn remove_loose(&self, id: &str) {
        self.inner
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .loose
            .remove(id);
    }

    /// The real file of the loose file `id`.
    #[must_use]
    pub fn loose_file(&self, id: &str) -> Option<PathBuf> {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .loose
            .get(id)
            .map(|scope| scope.file().to_path_buf())
    }

    /// Lets `observer` hear the saves of the Notes tab until the lease is dropped.
    #[must_use]
    pub fn install_observer(self: &Arc<Self>, observer: Arc<dyn SaveObserver>) -> ObserverLease {
        *self.observer.lock().unwrap_or_else(PoisonError::into_inner) = Some(observer);
        ObserverLease {
            scope: Arc::downgrade(self),
        }
    }

    /// Who hears the saves now.
    #[must_use]
    pub fn observer(&self) -> Option<Arc<dyn SaveObserver>> {
        self.observer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn loose_scope(&self, id: &str) -> Result<FileScope, VaultError> {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .loose
            .get(id)
            .cloned()
            .ok_or_else(|| VaultError::NotFound(loose_key(id)))
    }

    fn vault(&self) -> Result<Vault, VaultError> {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .folder
            .clone()
            .ok_or_else(|| VaultError::NotFound("notes folder".to_owned()))
    }

    /// Absolute path of a parsed path. `document`: the path must be a document (a loose file: the file itself).
    fn locate(&self, parsed: &VaultPath, document: bool) -> Result<PathBuf, VaultError> {
        let mut segments = parsed.segments();
        if segments.next() != Some("@file") {
            return self.vault()?.resolve(parsed);
        }
        let id = segments.next().unwrap_or_default();
        if !is_loose_id(id) {
            return Err(VaultError::NotFound(parsed.to_string()));
        }
        let scope = self.loose_scope(id)?;
        let rest: Vec<&str> = segments.collect();
        if rest.is_empty() {
            return Err(VaultError::NotFound(parsed.to_string()));
        }
        if document {
            return match rest.as_slice() {
                [name] if fold(name) == fold(&scope.name()) => Ok(scope.file().to_path_buf()),
                _ => Err(VaultError::NotFound(parsed.to_string())),
            };
        }
        scope.resolve_asset(&VaultPath::parse(&rest.join("/"))?)
    }

    /// Identity key + absolute path of a document (checked against the boundaries).
    ///
    /// # Errors
    /// Escapes the folder | invalid | not found.
    pub fn resolve(&self, path: &str) -> Result<(String, PathBuf), IpcError> {
        let parsed = VaultPath::parse(path)?;
        let abs = self.locate(&parsed, true)?;
        Ok((parsed.key(), abs))
    }

    /// Like [`NotesScope::resolve`] for a file that may not exist yet (a copy next to a note, a picture).
    ///
    /// # Errors
    /// Escapes the folder | invalid.
    pub fn resolve_new(&self, path: &str) -> Result<(String, PathBuf), IpcError> {
        let parsed = VaultPath::parse(path)?;
        let abs = self.locate(&parsed, false)?;
        Ok((parsed.key(), abs))
    }

    /// A file the window may show (a picture next to a note, a loose file or what is next to it).
    ///
    /// # Errors
    /// Escapes the folder | not found.
    pub fn resolve_asset(&self, path: &VaultPath) -> Result<PathBuf, VaultError> {
        self.locate(path, false)
    }

    /// The vault and the path inside it that the links of `doc` are resolved against. A loose file's links are inert
    /// (its folder is not part of the notes).
    #[must_use]
    pub fn link_base(&self, doc: &str) -> Option<(Vault, VaultPath)> {
        let parsed = VaultPath::parse(doc).ok()?;
        if parsed.segments().next() == Some("@file") {
            return None;
        }
        Some((self.vault().ok()?, parsed))
    }

    /// Whether `path` is an existing file the window can reach.
    #[must_use]
    pub fn file_exists(&self, path: &str) -> bool {
        let Ok(parsed) = VaultPath::parse(path) else {
            return false;
        };
        self.locate(&parsed, false)
            .is_ok_and(|abs| std::fs::metadata(abs).is_ok_and(|m| m.is_file()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope_with(dir: &Path) -> Arc<NotesScope> {
        let scope = NotesScope::new();
        scope.set_folder(Some(Root::open(dir).unwrap()));
        scope
    }

    #[test]
    fn loose_paths_round_trip_and_ids_are_eight_hex_digits() {
        let path = loose_path("0a1b2c3d", "plan.md");
        assert_eq!(path, "@file/0a1b2c3d/plan.md");
        assert_eq!(parse_loose(&path), Some(("0a1b2c3d", "plan.md")));
        assert_eq!(loose_id_of("@file/0a1b2c3d"), Some("0a1b2c3d"));
        assert_eq!(loose_id_of("@file/0a1b2c3d/x.png"), Some("0a1b2c3d"));
        for bad in [
            "@file/",
            "@file/zz/a.md",
            "@file/0A1B2C3D/a.md",
            "@file/0a1b2c3d",
            "a.md",
            "@file/0a1b2c3d4/a.md",
        ] {
            assert_eq!(parse_loose(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_note_resolves_inside_the_folder_and_never_outside() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("2026-10-16 0140.md"), "x").unwrap();
        let scope = scope_with(dir.path());
        let (key, abs) = scope.resolve("2026-10-16 0140.md").unwrap();
        assert!(abs.ends_with("2026-10-16 0140.md"));
        assert_ne!(key, "");
        for bad in [
            "../x.md",
            "a/../../x.md",
            "/etc/passwd",
            "C:\\x.md",
            "x\0.md",
        ] {
            assert!(scope.resolve(bad).is_err(), "{bad}");
        }
        assert!(scope.file_exists("2026-10-16 0140.md"));
        assert!(!scope.file_exists("missing.md"));
    }

    #[test]
    fn without_a_folder_nothing_resolves() {
        let scope = NotesScope::new();
        assert!(scope.resolve("a.md").is_err());
        assert!(!scope.file_exists("a.md"));
        assert!(scope.link_base("a.md").is_none());
    }

    #[test]
    fn a_loose_file_resolves_as_itself_and_its_neighbours_as_assets_only() {
        let notes = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let file = other.path().join("Ask me.md");
        std::fs::write(&file, "# Ask").unwrap();
        std::fs::write(other.path().join("pic.png"), "p").unwrap();
        std::fs::write(other.path().join("secret.md"), "s").unwrap();
        let scope = scope_with(notes.path());
        scope.set_loose("0a1b2c3d", &file).unwrap();
        let (_, abs) = scope.resolve("@file/0a1b2c3d/Ask me.md").unwrap();
        assert_eq!(abs, dunce::canonicalize(&file).unwrap());
        // a neighbour is not a document
        assert!(scope.resolve("@file/0a1b2c3d/secret.md").is_err());
        // but it is an asset, and a new file may be made next to the file
        let asset = scope
            .resolve_asset(&VaultPath::parse("@file/0a1b2c3d/pic.png").unwrap())
            .unwrap();
        assert!(asset.is_file());
        assert!(scope.resolve_new("@file/0a1b2c3d/Ask me-mine.md").is_ok());
        // `..` is settled before anything is looked up: it climbs out of the loose file's id to the notes
        // folder, never to the loose file's folder or above
        let climbed = scope.resolve_new("@file/0a1b2c3d/../../x.md").unwrap().1;
        assert!(
            climbed.starts_with(notes.path())
                || climbed.starts_with(dunce::canonicalize(notes.path()).unwrap())
        );
        assert!(scope.resolve_new("@file/0a1b2c3d/../x.md").is_err());
        assert!(scope.resolve_new("@file/0a1b2c3d/../../../x.md").is_err());
        // unknown ids and a missing name
        assert!(scope.resolve("@file/ffffffff/Ask me.md").is_err());
        assert!(scope.resolve("@file/0a1b2c3d").is_err());
        assert!(scope.link_base("@file/0a1b2c3d/Ask me.md").is_none());
        scope.remove_loose("0a1b2c3d");
        assert!(scope.resolve("@file/0a1b2c3d/Ask me.md").is_err());
    }

    #[test]
    fn a_loose_file_that_is_not_there_cannot_be_added() {
        let dir = tempfile::tempdir().unwrap();
        let scope = NotesScope::new();
        assert!(scope
            .set_loose("0a1b2c3d", &dir.path().join("none.md"))
            .is_err());
        assert!(scope.set_loose("0a1b2c3d", dir.path()).is_err());
    }

    #[test]
    fn the_observer_is_installed_until_its_lease_is_dropped() {
        struct Nop;
        impl SaveObserver for Nop {
            fn replacing(&self, _: &str, _: &[u8]) {}
            fn replaced(&self, _: &str) {}
        }
        let scope = NotesScope::new();
        assert!(scope.observer().is_none());
        let lease = scope.install_observer(Arc::new(Nop));
        assert!(scope.observer().is_some());
        drop(lease);
        assert!(scope.observer().is_none());
    }
}
