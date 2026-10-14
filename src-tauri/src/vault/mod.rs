//! The vault: the main root folder plus linked worktrees, and everything path-related.
//!
//! Paths crossing the UI boundary are vault-relative, `/`-separated and use the real on-disk case.
//! Worktree paths carry an `@wt/<name>/` prefix. [`Vault::resolve`] is the only way a UI-supplied path
//! becomes a filesystem path, and it rejects anything that could leave the allowed roots.

mod tree;

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix, PrefixComponent};

use serde::Serialize;

use crate::util::{canonicalize_lenient, split_existing};

pub use tree::{NodeKind, TreeNode, WalkItem};

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
    /// Absolute path (existing part in real on-disk case). The `\\?\` prefix is removed unless the
    /// path is too long to work without it (over 260 characters).
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

fn name_eq(a: &OsStr, b: &OsStr) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Windows path prefixes compare equal when they name the same place: `C:` and the verbatim `\\?\C:`
/// (which `canonicalize` keeps for paths over 260 characters), `\\server\share` and `\\?\UNC\server\share`.
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

/// If `path` is inside `root` (comparing case-insensitively on Windows), the remaining components.
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
        let Some((canon_existing, tail)) = split_existing(&abs) else {
            return Err(VaultError::NotFound(rel));
        };
        if strip_root(&canon_existing, root).is_none() {
            return Err(VaultError::Escapes(rel));
        }
        let mut full = canon_existing;
        full.extend(tail);

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
    ///
    /// The roots are canonical, so a path spelled differently (8.3 short names such as `RUNNER~1`,
    /// a symlinked folder, `/var` versus `/private/var`) is compared in its canonical form when it
    /// does not match as written. Paths from the walker and the watcher match on the first try.
    pub fn locate(&self, abs: &Path) -> Option<Located> {
        let abs = dunce::simplified(abs);
        self.locate_in_roots(abs).or_else(|| {
            if !self.shares_volume_with_a_root(abs) {
                return None;
            }
            let canon = canonicalize_lenient(abs).filter(|c| c != abs)?;
            self.locate_in_roots(&canon)
        })
    }

    /// Whether `abs` starts on a drive or share that a root lives on. Spelling differences (short
    /// names, case, symlinks) stay on the volume, so a path on any other one is not worth touching
    /// the disk for: probing a disconnected network drive (`Z:\...`) can block for a minute.
    fn shares_volume_with_a_root(&self, abs: &Path) -> bool {
        let first = abs.components().next();
        self.root_paths()
            .into_iter()
            .any(|root| match (first, root.components().next()) {
                (Some(Component::Prefix(a)), Some(Component::Prefix(b))) => prefix_eq(a, b),
                (Some(Component::RootDir), Some(Component::RootDir)) => true,
                _ => false,
            })
    }

    fn locate_in_roots(&self, abs: &Path) -> Option<Located> {
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

    /// Creates `dir/<long path>/deep.md` with an absolute path of more than 300 characters.
    #[cfg(windows)]
    fn deep_file(dir: &Path) -> String {
        let mut rel = String::new();
        while dir.display().to_string().len() + rel.len() < 320 {
            rel.push_str("segment-with-a-long-name/");
        }
        rel.push_str("deep.md");
        let abs = dir.join(&rel);
        fs::create_dir_all(abs.parent().expect("parent")).expect("mkdir");
        fs::write(&abs, "x").expect("write");
        rel
    }

    #[cfg(windows)]
    #[test]
    fn files_under_paths_longer_than_260_characters_resolve() {
        let (d, _v) = vault_with(&["a.md"]);
        let rel = deep_file(d.path());
        let v = Vault::open(d.path()).expect("open");
        let r = v.resolve(&rel).expect("a long path stays inside the root");
        assert_eq!(r.rel, rel);
        assert!(r.abs.is_file());
        assert!(r.abs.display().to_string().len() > 300);
        assert_eq!(v.to_rel(&r.abs).map(|(rel, _)| rel), Some(rel.clone()));
        // A file that does not exist yet below such a folder is still inside the root.
        let missing = rel.replace("deep.md", "new.md");
        assert_eq!(
            v.resolve(&missing).expect("missing but inside").rel,
            missing
        );
        // And the way out is still closed.
        assert!(matches!(
            v.resolve(&format!("{rel}{}", "/..".repeat(40))),
            Err(VaultError::Escapes(_))
        ));
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
        assert_eq!(rest(r"\\?\UNC\other\share\a.md", r"\\server\share"), None);
    }

    /// A folder with a long name, so that it has a distinct 8.3 short spelling, with its short path.
    /// `None` (the test is skipped) when the volume has 8.3 names disabled.
    #[cfg(windows)]
    fn long_folder_with_short_name(files: &[&str]) -> Option<(TempDir, PathBuf, PathBuf)> {
        let dir = tempfile::tempdir().expect("tempdir");
        let long = dir.path().join("a-long-folder-name");
        for f in files {
            let p = long.join(f);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(&p, "x").expect("write");
        }
        let short = crate::util::short_path(&long);
        if short.is_none() {
            eprintln!("skipped: 8.3 short names are disabled on this volume");
        }
        short.map(|short| (dir, long, short))
    }

    #[cfg(windows)]
    #[test]
    fn paths_spelled_with_8dot3_short_names_are_located() {
        let Some((_d, long, short)) = long_folder_with_short_name(&["docs/a.md"]) else {
            return;
        };
        let v = Vault::open(&long).expect("open");
        let located = |p: PathBuf| v.to_rel(&p);
        assert_eq!(
            located(short.join("docs").join("a.md")),
            Some(("docs/a.md".to_owned(), 0))
        );
        // A file that does not exist (yet) below the short spelling is placed too.
        assert_eq!(
            located(short.join("docs").join("new").join("n.md")),
            Some(("docs/new/n.md".to_owned(), 0))
        );
        // The short spelling of somewhere else is still outside.
        let other = tempfile::tempdir().expect("tempdir");
        assert_eq!(located(other.path().join("x.md")), None);
    }

    #[cfg(windows)]
    #[test]
    fn worktrees_given_by_their_8dot3_short_path_are_located() {
        let Some((_w, long, short)) = long_folder_with_short_name(&["w.md"]) else {
            return;
        };
        let (_d, v) = vault_with(&["a.md"]);
        let v = v.with_worktrees(vec![Worktree {
            name: "feature-x".to_owned(),
            path: short.clone(),
            branch: None,
        }]);
        assert_eq!(v.worktrees().len(), 1);
        assert_eq!(
            v.worktrees()[0].path,
            dunce::canonicalize(&long).expect("canon")
        );
        let (rel, idx) = v.to_rel(&short.join("w.md")).expect("to_rel");
        assert_eq!((rel.as_str(), idx), ("@wt/feature-x/w.md", 1));
        let r = v.resolve("@wt/feature-x/w.md").expect("resolve");
        assert!(r.abs.is_file());
    }

    #[cfg(windows)]
    #[test]
    fn other_drives_are_not_probed() {
        let (_d, v) = vault_with(&["a.md"]);
        let drive = v.root().display().to_string()[..1].to_owned();
        assert!(v.shares_volume_with_a_root(Path::new(&format!("{}:\\x", drive.to_lowercase()))));
        assert!(v.shares_volume_with_a_root(Path::new(&format!("\\\\?\\{drive}:\\x"))));
        let other = if drive.eq_ignore_ascii_case("Q") {
            "R"
        } else {
            "Q"
        };
        assert!(!v.shares_volume_with_a_root(Path::new(&format!("{other}:\\x"))));
        assert!(!v.shares_volume_with_a_root(Path::new(r"\\server\share\x")));
        assert!(!v.shares_volume_with_a_root(Path::new("relative\\x")));
    }

    /// A different spelling of the same place: the vault is reached through a symlink (like `/var`
    /// versus `/private/var` on macOS).
    #[cfg(unix)]
    #[test]
    fn paths_reached_through_a_symlink_are_located() {
        let (_d, v) = vault_with(&["docs/a.md"]);
        let elsewhere = tempfile::tempdir().expect("tempdir");
        let link = elsewhere.path().join("alias");
        std::os::unix::fs::symlink(v.root(), &link).expect("symlink");
        assert_eq!(
            v.to_rel(&link.join("docs").join("a.md")),
            Some(("docs/a.md".to_owned(), 0))
        );
        assert_eq!(
            v.to_rel(&link.join("docs").join("new.md")),
            Some(("docs/new.md".to_owned(), 0))
        );
        assert_eq!(v.to_rel(&elsewhere.path().join("x.md")), None);
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
