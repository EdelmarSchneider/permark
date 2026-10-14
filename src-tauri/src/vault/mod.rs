//! The vault: the main root folder plus linked worktrees, and everything path-related.
//!
//! Paths crossing the UI boundary are vault-relative, `/`-separated and use the real on-disk case.
//! Worktree paths carry an `@wt/<name>/` prefix. [`Vault::resolve`] is the only way a UI-supplied path
//! becomes a filesystem path, and it rejects anything that could leave the allowed roots.

mod tree;

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

pub use tree::{NodeKind, TreeNode};

/// Prefix of worktree paths: `@wt/<name>/relative/path`.
pub const WORKTREE_PREFIX: &str = "@wt/";

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("path is outside the folder: {0}")]
    Escapes(String),
    #[error("invalid path: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("cannot access {path}: {source}")]
    Io { path: String, source: io::Error },
}

impl VaultError {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Escapes(_) => "pathEscape",
            Self::Invalid(_) => "invalidPath",
            Self::NotFound(_) => "notFound",
            Self::Io { .. } => "io",
        }
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultMode {
    Ide,
    Simple,
}

/// A linked git worktree shown as an extra root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Worktree {
    pub name: String,
    pub path: PathBuf,
    /// The checked-out branch (`None` for a detached HEAD or when it cannot be read).
    pub branch: Option<String>,
}

/// A path that passed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Absolute path (existing part in real on-disk case, `\\?\` prefix removed).
    pub abs: PathBuf,
    /// Vault-relative path in the UI form (`@wt/<name>/` prefix for worktrees).
    pub rel: String,
    /// Index of the owning root: 0 is the main root, `n` is `worktrees[n - 1]`.
    pub root: usize,
}

#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
    worktrees: Vec<Worktree>,
    mode: VaultMode,
    initial_file: Option<String>,
}

/// Case-folded form of a vault-relative path, used as a map key. Windows file systems are
/// case-insensitive, so `README.md` and `readme.md` are the same file there.
pub fn key(rel: &str) -> String {
    if cfg!(windows) {
        rel.to_lowercase()
    } else {
        rel.to_owned()
    }
}

fn component_eq(a: &OsStr, b: &OsStr) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// If `path` is inside `root` (comparing case-insensitively on Windows), the remaining components.
pub(crate) fn strip_root<'a>(path: &'a Path, root: &Path) -> Option<Vec<&'a OsStr>> {
    let mut rest = path.components();
    for want in root.components() {
        let got = rest.next()?;
        if !component_eq(got.as_os_str(), want.as_os_str()) {
            return None;
        }
    }
    Some(rest.map(Component::as_os_str).collect())
}

fn is_drive_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

impl Vault {
    /// Opens a folder (IDE mode) or a file (Simple mode: the root is the file's folder).
    pub fn open(path: &Path) -> Result<Self, VaultError> {
        let canon = dunce::canonicalize(path).map_err(|e| VaultError::io(path, e))?;
        let meta = std::fs::metadata(&canon).map_err(|e| VaultError::io(&canon, e))?;
        if meta.is_dir() {
            Ok(Self {
                root: canon,
                worktrees: Vec::new(),
                mode: VaultMode::Ide,
                initial_file: None,
            })
        } else {
            let parent = canon
                .parent()
                .ok_or_else(|| VaultError::Invalid(canon.display().to_string()))?
                .to_path_buf();
            let file = canon
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .ok_or_else(|| VaultError::Invalid(canon.display().to_string()))?;
            Ok(Self {
                root: parent,
                worktrees: Vec::new(),
                mode: VaultMode::Simple,
                initial_file: Some(file),
            })
        }
    }

    /// Attaches linked worktrees. Missing folders, the main root itself and duplicates are skipped;
    /// names are made unique and safe to use as a path segment.
    #[must_use]
    pub fn with_worktrees(mut self, found: Vec<Worktree>) -> Self {
        let mut accepted: Vec<Worktree> = Vec::new();
        for wt in found {
            let Ok(path) = dunce::canonicalize(&wt.path) else {
                continue;
            };
            if !path.is_dir()
                // The main root itself, or a folder that contains it (the vault is inside that worktree).
                || strip_root(&self.root, &path).is_some()
                || accepted
                    .iter()
                    .any(|a| strip_root(&path, &a.path).is_some_and(|rest| rest.is_empty()))
            {
                continue;
            }
            let base: String = wt
                .name
                .chars()
                .map(|c| {
                    if matches!(c, '/' | '\\' | ':' | '\0') {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
            let base = if base.is_empty() {
                "worktree".to_owned()
            } else {
                base
            };
            let mut name = base.clone();
            let mut n = 2;
            while accepted.iter().any(|a| key(&a.name) == key(&name)) {
                name = format!("{base}-{n}");
                n += 1;
            }
            accepted.push(Worktree {
                name,
                path,
                branch: wt.branch,
            });
        }
        self.worktrees = accepted;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn worktrees(&self) -> &[Worktree] {
        &self.worktrees
    }

    pub fn mode(&self) -> VaultMode {
        self.mode
    }

    /// Simple mode looks at one folder level only (no recursive walk, watch or index).
    pub fn is_shallow(&self) -> bool {
        self.mode == VaultMode::Simple
    }

    pub fn initial_file(&self) -> Option<&str> {
        self.initial_file.as_deref()
    }

    /// Display name of the vault (the folder name).
    pub fn name(&self) -> String {
        folder_name(&self.root)
    }

    /// All roots: main first, then worktrees.
    pub fn root_paths(&self) -> Vec<&Path> {
        std::iter::once(self.root.as_path())
            .chain(self.worktrees.iter().map(|w| w.path.as_path()))
            .collect()
    }

    pub(crate) fn root_path(&self, idx: usize) -> Option<&Path> {
        if idx == 0 {
            Some(&self.root)
        } else {
            self.worktrees.get(idx - 1).map(|w| w.path.as_path())
        }
    }

    pub(crate) fn prefix_of(&self, idx: usize) -> String {
        if idx == 0 {
            String::new()
        } else {
            format!("{WORKTREE_PREFIX}{}/", self.worktrees[idx - 1].name)
        }
    }

    /// Finds the root a UI path belongs to and the path relative to that root.
    fn split_prefix<'a>(&self, rel: &'a str) -> Result<(usize, &'a str), VaultError> {
        let Some(rest) = rel.strip_prefix(WORKTREE_PREFIX) else {
            return Ok((0, rel));
        };
        let (name, inner) = rest.split_once('/').unwrap_or((rest, ""));
        let idx = self
            .worktrees
            .iter()
            .position(|w| key(&w.name) == key(name))
            .ok_or_else(|| VaultError::NotFound(format!("worktree {name}")))?;
        Ok((idx + 1, inner))
    }

    /// Validates a UI path and maps it to an absolute path inside one of the roots.
    ///
    /// Rejects absolute paths, drive letters, NUL bytes, `..` that climbs above the root, alternate
    /// data streams on Windows, and anything whose real location (after symlinks/junctions) is
    /// outside the root.
    pub fn resolve(&self, rel: &str) -> Result<Resolved, VaultError> {
        if rel.contains('\0') {
            return Err(VaultError::Invalid("contains NUL".to_owned()));
        }
        let rel = rel.replace('\\', "/");
        if rel.starts_with('/') || is_drive_path(&rel) {
            return Err(VaultError::Invalid(format!("absolute path: {rel}")));
        }
        let (root_idx, inner) = self.split_prefix(&rel)?;
        let root = self
            .root_path(root_idx)
            .ok_or_else(|| VaultError::NotFound(rel.clone()))?;

        let mut parts: Vec<&str> = Vec::new();
        for seg in inner.split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err(VaultError::Escapes(rel.clone()));
                    }
                }
                s => {
                    if cfg!(windows) && s.contains(':') {
                        return Err(VaultError::Invalid(format!("':' in path segment: {s}")));
                    }
                    parts.push(s);
                }
            }
        }
        let mut abs = root.to_path_buf();
        abs.extend(&parts);

        // Re-check containment on the real location: the deepest existing ancestor is canonicalized
        // (following symlinks and junctions) and must still be inside the root.
        let mut existing = abs.as_path();
        let mut tail: Vec<&OsStr> = Vec::new();
        let canon_existing = loop {
            if let Ok(c) = dunce::canonicalize(existing) {
                break c;
            }
            let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
                return Err(VaultError::NotFound(rel));
            };
            tail.push(name);
            existing = parent;
        };
        if strip_root(&canon_existing, root).is_none() {
            return Err(VaultError::Escapes(rel));
        }
        let mut full = canon_existing;
        full.extend(tail.iter().rev());

        let display = self.to_rel(&full).map_or_else(
            || {
                let mut s = self.prefix_of(root_idx);
                s.push_str(&parts.join("/"));
                s
            },
            |(r, _)| r,
        );
        Ok(Resolved {
            abs: full,
            rel: display,
            root: root_idx,
        })
    }

    /// Maps an absolute path to the UI form, using the most specific root that contains it.
    pub fn to_rel(&self, abs: &Path) -> Option<(String, usize)> {
        self.locate(abs).map(|l| (l.rel, l.root))
    }

    /// Like [`Vault::to_rel`], also returning the path relative to the owning root.
    pub fn locate(&self, abs: &Path) -> Option<Located> {
        let abs = dunce::simplified(abs);
        let mut best: Option<(usize, Vec<&OsStr>)> = None;
        for (idx, root) in self.root_paths().into_iter().enumerate() {
            if let Some(rest) = strip_root(abs, root) {
                if best.as_ref().is_none_or(|(_, b)| rest.len() < b.len()) {
                    best = Some((idx, rest));
                }
            }
        }
        let (root, rest) = best?;
        let inner = rest
            .iter()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let rel = if root == 0 {
            inner.clone()
        } else if inner.is_empty() {
            format!("{WORKTREE_PREFIX}{}", self.worktrees[root - 1].name)
        } else {
            format!("{WORKTREE_PREFIX}{}/{inner}", self.worktrees[root - 1].name)
        };
        Some(Located { rel, root, inner })
    }
}

/// An absolute path placed inside the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// UI form (`@wt/<name>/` prefix for worktrees).
    pub rel: String,
    pub root: usize,
    /// Path relative to the owning root.
    pub inner: String,
}

pub(crate) fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    pub(crate) fn vault_with(files: &[&str]) -> (TempDir, Vault) {
        let dir = tempfile::tempdir().expect("tempdir");
        for f in files {
            let p = dir.path().join(f);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(&p, "x").expect("write");
        }
        let v = Vault::open(dir.path()).expect("open");
        (dir, v)
    }

    #[test]
    fn resolve_accepts_nested_and_dotted_paths() {
        let (_d, v) = vault_with(&["a/b/c.md"]);
        let r = v.resolve("a/./b/../b/c.md").expect("resolve");
        assert_eq!(r.rel, "a/b/c.md");
        assert!(r.abs.ends_with("c.md"));
        assert_eq!(r.root, 0);
    }

    #[test]
    fn resolve_rejects_escapes_and_absolute_paths() {
        let (_d, v) = vault_with(&["a.md"]);
        assert!(matches!(v.resolve("../x.md"), Err(VaultError::Escapes(_))));
        assert!(matches!(
            v.resolve("a/../../x"),
            Err(VaultError::Escapes(_))
        ));
        assert!(matches!(
            v.resolve("/etc/passwd"),
            Err(VaultError::Invalid(_))
        ));
        assert!(matches!(
            v.resolve("C:/Windows/x"),
            Err(VaultError::Invalid(_))
        ));
        assert!(matches!(
            v.resolve("C:\\Windows"),
            Err(VaultError::Invalid(_))
        ));
        assert!(matches!(v.resolve("a\0b"), Err(VaultError::Invalid(_))));
        assert!(matches!(
            v.resolve("\\\\server\\share"),
            Err(VaultError::Invalid(_))
        ));
    }

    #[test]
    fn resolve_treats_backslashes_as_separators() {
        let (_d, v) = vault_with(&["a/b.md"]);
        assert_eq!(v.resolve("a\\b.md").expect("resolve").rel, "a/b.md");
        assert!(matches!(
            v.resolve("a\\..\\..\\x"),
            Err(VaultError::Escapes(_))
        ));
    }

    #[test]
    fn resolve_empty_is_the_root() {
        let (_d, v) = vault_with(&["a.md"]);
        let r = v.resolve("").expect("resolve");
        assert_eq!(r.abs, v.root());
        assert_eq!(r.rel, "");
    }

    #[test]
    fn resolve_allows_missing_files_inside_the_root() {
        let (_d, v) = vault_with(&["a.md"]);
        let r = v.resolve("new/dir/file.md").expect("resolve");
        assert!(!r.abs.exists());
        assert_eq!(r.rel, "new/dir/file.md");
    }

    #[cfg(windows)]
    #[test]
    fn resolve_rejects_alternate_data_streams() {
        let (_d, v) = vault_with(&["a.md"]);
        assert!(matches!(
            v.resolve("a.md:evil"),
            Err(VaultError::Invalid(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn resolve_returns_the_real_on_disk_case() {
        let (_d, v) = vault_with(&["Docs/ReadMe.md"]);
        let r = v.resolve("docs/README.MD").expect("resolve");
        assert_eq!(r.rel, "Docs/ReadMe.md");
    }

    #[test]
    fn key_folds_case_only_on_windows() {
        assert_eq!(key("A/b.md") == key("a/B.md"), cfg!(windows));
    }

    #[test]
    fn worktrees_are_prefixed_and_resolvable() {
        let (_d, v) = vault_with(&["a.md"]);
        let wt = tempfile::tempdir().expect("tempdir");
        fs::write(wt.path().join("w.md"), "x").expect("write");
        let v = v.with_worktrees(vec![Worktree {
            name: "feature-x".to_owned(),
            path: wt.path().to_path_buf(),
            branch: None,
        }]);
        let r = v.resolve("@wt/feature-x/w.md").expect("resolve");
        assert_eq!(r.root, 1);
        assert_eq!(r.rel, "@wt/feature-x/w.md");
        assert!(r.abs.ends_with("w.md"));
        assert!(matches!(
            v.resolve("@wt/other/w.md"),
            Err(VaultError::NotFound(_))
        ));
        assert!(matches!(
            v.resolve("@wt/feature-x/../../x"),
            Err(VaultError::Escapes(_))
        ));
        let (rel, idx) = v
            .to_rel(&wt.path().join("sub").join("n.md"))
            .expect("to_rel");
        assert_eq!((rel.as_str(), idx), ("@wt/feature-x/sub/n.md", 1));
    }

    #[test]
    fn nested_worktree_wins_over_the_main_root() {
        let (d, v) = vault_with(&["a.md", "inner/wt/w.md"]);
        let v = v.with_worktrees(vec![Worktree {
            name: "inner".to_owned(),
            path: d.path().join("inner").join("wt"),
            branch: None,
        }]);
        let (rel, idx) = v
            .to_rel(&d.path().join("inner").join("wt").join("w.md"))
            .expect("to_rel");
        assert_eq!((rel.as_str(), idx), ("@wt/inner/w.md", 1));
        let (rel, idx) = v.to_rel(&d.path().join("a.md")).expect("to_rel");
        assert_eq!((rel.as_str(), idx), ("a.md", 0));
    }

    #[test]
    fn with_worktrees_skips_missing_main_and_duplicate_entries() {
        let (d, v) = vault_with(&["a.md"]);
        let wt = tempfile::tempdir().expect("tempdir");
        let v = v.with_worktrees(vec![
            Worktree {
                name: "gone".to_owned(),
                path: d.path().join("does-not-exist"),
                branch: None,
            },
            Worktree {
                name: "self".to_owned(),
                path: d.path().to_path_buf(),
                branch: None,
            },
            Worktree {
                name: "a/b".to_owned(),
                path: wt.path().to_path_buf(),
                branch: None,
            },
            Worktree {
                name: "dup".to_owned(),
                path: wt.path().to_path_buf(),
                branch: None,
            },
        ]);
        assert_eq!(v.worktrees().len(), 1);
        assert_eq!(v.worktrees()[0].name, "a_b");
    }

    #[test]
    fn open_a_file_uses_simple_mode() {
        let (d, _v) = vault_with(&["notes/todo.md"]);
        let v = Vault::open(&d.path().join("notes").join("todo.md")).expect("open");
        assert_eq!(v.mode(), VaultMode::Simple);
        assert_eq!(v.initial_file(), Some("todo.md"));
        assert_eq!(v.name(), "notes");
        assert!(v.resolve("todo.md").expect("resolve").abs.is_file());
    }

    #[test]
    fn open_missing_path_is_not_found() {
        let d = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            Vault::open(&d.path().join("nope")),
            Err(VaultError::NotFound(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_symlinks_that_leave_the_root() {
        let (d, v) = vault_with(&["a.md"]);
        let outside = tempfile::tempdir().expect("tempdir");
        fs::write(outside.path().join("secret.md"), "s").expect("write");
        std::os::unix::fs::symlink(outside.path(), d.path().join("link")).expect("symlink");
        assert!(matches!(
            v.resolve("link/secret.md"),
            Err(VaultError::Escapes(_))
        ));
    }
}
