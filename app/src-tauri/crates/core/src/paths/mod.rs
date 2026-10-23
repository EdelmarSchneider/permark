//! Paths + the open-folder boundary. Raw input -> validated types at the edge: [`Root`] (canonical folder),
//! [`VaultPath`] (project-relative), [`Vault`] (roots + `resolve`), [`FileScope`] (single file + its folder).
//! Every file access in core goes through `Vault::resolve` | `FileScope::resolve_asset`.

mod plain;
mod root;
mod scope;
mod vault;
mod vault_path;

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix, PrefixComponent};

pub(crate) use plain::strip_prefix_ci;
#[cfg(all(test, windows))]
pub(crate) use plain::via_admin_share;
pub use plain::{canonicalize, simplified, simplified_text};
pub(crate) use root::legacy_unc_key;
pub use root::{Root, RootKey};
pub use scope::FileScope;
pub use vault::{Vault, Worktree, WORKTREE_PREFIX};
pub(crate) use vault_path::windows_name_ok;
pub use vault_path::VaultPath;

use crate::error::{ErrorKind, IpcError};

/// File systems of Windows (NTFS) + macOS (APFS, HFS+) ignore case by default: identity compares folded.
pub const CASE_INSENSITIVE: bool = cfg!(any(windows, target_os = "macos"));

/// Identity key of a project path as written (NFC + case folded where the OS ignores case); text that is no valid
/// project path is only folded.
#[must_use]
pub fn key_of(path: &str) -> String {
    VaultPath::parse(path).map_or_else(|_| fold(path), |p| p.key())
}

/// Case-folded form for identity keys (see [`CASE_INSENSITIVE`]).
#[must_use]
pub fn fold(text: &str) -> String {
    if CASE_INSENSITIVE {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("path is outside the folder: {0}")]
    Escapes(String),
    #[error("invalid path: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("not a folder: {0}")]
    NotAFolder(String),
    #[error("not a file: {0}")]
    NotAFile(String),
    #[error("cannot access {path}: {source}")]
    Io { path: String, source: io::Error },
}

impl VaultError {
    pub(crate) fn io(path: &Path, source: io::Error) -> Self {
        if source.kind() == io::ErrorKind::NotFound {
            Self::NotFound(path.display().to_string())
        } else {
            Self::Io {
                path: path.display().to_string(),
                source,
            }
        }
    }
}

impl From<VaultError> for IpcError {
    fn from(error: VaultError) -> Self {
        match error {
            VaultError::Escapes(path) => IpcError::new(ErrorKind::PathEscape).subject(path),
            VaultError::Invalid(path) => IpcError::new(ErrorKind::InvalidPath).subject(path),
            VaultError::NotFound(path) => IpcError::new(ErrorKind::NotFound).subject(path),
            VaultError::NotAFolder(path) | VaultError::NotAFile(path) => {
                IpcError::new(ErrorKind::Unsupported).subject(path)
            }
            VaultError::Io { path, source } => IpcError::io(path, &source),
        }
    }
}

fn name_eq(a: &OsStr, b: &OsStr) -> bool {
    if CASE_INSENSITIVE {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// `C:` == `\\?\C:`; `\\server\share` == `\\?\UNC\server\share` (canonicalize keeps verbatim for long paths).
fn prefix_eq(a: PrefixComponent<'_>, b: PrefixComponent<'_>) -> bool {
    match (a.kind(), b.kind()) {
        (Prefix::Disk(x) | Prefix::VerbatimDisk(x), Prefix::Disk(y) | Prefix::VerbatimDisk(y)) => {
            x.eq_ignore_ascii_case(&y)
        }
        (
            Prefix::UNC(server_a, share_a) | Prefix::VerbatimUNC(server_a, share_a),
            Prefix::UNC(server_b, share_b) | Prefix::VerbatimUNC(server_b, share_b),
        ) => name_eq(server_a, server_b) && name_eq(share_a, share_b),
        _ => name_eq(a.as_os_str(), b.as_os_str()),
    }
}

fn component_eq(a: Component<'_>, b: Component<'_>) -> bool {
    match (a, b) {
        (Component::Prefix(x), Component::Prefix(y)) => prefix_eq(x, y),
        _ => name_eq(a.as_os_str(), b.as_os_str()),
    }
}

/// Components of `path` below `root`, if `path` is `root` or inside it (case per OS, prefixes normalized).
pub(crate) fn strip_root<'a>(path: &'a Path, root: &Path) -> Option<Vec<&'a OsStr>> {
    let mut rest = path.components();
    for want in root.components() {
        let got = rest.next()?;
        if !component_eq(got, want) {
            return None;
        }
    }
    Some(rest.map(Component::as_os_str).collect())
}

/// Same volume (drive | share | Unix root) as `root`: spelling retries never probe other volumes (a disconnected
/// network drive can block for a minute).
pub(crate) fn same_volume(path: &Path, root: &Path) -> bool {
    match (path.components().next(), root.components().next()) {
        (Some(Component::Prefix(a)), Some(Component::Prefix(b))) => prefix_eq(a, b),
        (Some(Component::RootDir), Some(Component::RootDir)) => true,
        _ => false,
    }
}

/// Canonical deepest existing ancestor of `path` + missing tail (path order). Follows symlinks | junctions, expands
/// 8.3 names, fixes case; `\\?\` dropped when possible ([`simplified`]). `None` when nothing exists | path ends in `..`.
pub(crate) fn split_existing(path: &Path) -> Option<(PathBuf, Vec<&OsStr>)> {
    let mut existing = path;
    let mut tail: Vec<&OsStr> = Vec::new();
    let canon = loop {
        if let Ok(canon) = canonicalize(existing) {
            break canon;
        }
        tail.push(existing.file_name()?);
        existing = existing.parent()?;
    };
    tail.reverse();
    Some((canon, tail))
}

/// `path` canonical even when its last components do not exist yet.
pub(crate) fn canonicalize_lenient(path: &Path) -> Option<PathBuf> {
    let (mut canon, tail) = split_existing(path)?;
    canon.extend(tail);
    Some(canon)
}

/// Path as shown | keyed: `/` separators.
pub(crate) fn slash(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_follows_the_os() {
        assert_eq!(fold("A/B.md") == fold("a/b.md"), CASE_INSENSITIVE);
    }

    #[cfg(windows)]
    #[test]
    fn strip_root_treats_verbatim_and_plain_prefixes_alike() {
        let rest = |p: &str, r: &str| {
            strip_root(Path::new(p), Path::new(r)).map(|v| {
                v.iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(
            rest(r"\\?\C:\Users\me\docs\a.md", r"C:\Users\me"),
            Some(vec!["docs".to_owned(), "a.md".to_owned()])
        );
        assert_eq!(
            rest(r"c:\users\ME\a.md", r"\\?\C:\Users\me"),
            Some(vec!["a.md".to_owned()])
        );
        assert_eq!(
            rest(r"\\?\UNC\server\share\dir\a.md", r"\\server\share\dir"),
            Some(vec!["a.md".to_owned()])
        );
        assert_eq!(rest(r"\\?\D:\Users\me\a.md", r"C:\Users\me"), None);
        assert_eq!(rest(r"C:\Users\meX\a.md", r"C:\Users\me"), None);
    }

    #[cfg(windows)]
    #[test]
    fn other_volumes_are_not_the_same_volume() {
        assert!(same_volume(Path::new(r"c:\x"), Path::new(r"C:\root")));
        assert!(same_volume(Path::new(r"\\?\C:\x"), Path::new(r"C:\root")));
        assert!(!same_volume(Path::new(r"Q:\x"), Path::new(r"C:\root")));
        assert!(!same_volume(
            Path::new(r"\\server\share\x"),
            Path::new(r"C:\root")
        ));
        assert!(!same_volume(
            Path::new(r"relative\x"),
            Path::new(r"C:\root")
        ));
    }

    #[test]
    fn split_existing_separates_the_missing_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new").join("deeper").join("x.md");
        let (canon, tail) = split_existing(&path).unwrap();
        assert_eq!(canon, dunce::canonicalize(dir.path()).unwrap());
        assert_eq!(
            tail,
            [OsStr::new("new"), OsStr::new("deeper"), OsStr::new("x.md")]
        );
    }
}
