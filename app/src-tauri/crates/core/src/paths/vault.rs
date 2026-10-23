//! `Vault`: main root + linked worktrees; the only way a project-relative path becomes a filesystem path.

use std::path::{Path, PathBuf};

use super::{
    canonicalize_lenient, fold, same_volume, split_existing, strip_root, Root, VaultError,
    VaultPath,
};

/// Prefix of worktree paths: `@wt/<name>/relative/path`.
pub const WORKTREE_PREFIX: &str = "@wt/";

/// Linked git worktree shown as an extra root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Path-safe, unique (case-folded) name.
    pub name: String,
    pub root: Root,
}

#[derive(Debug, Clone)]
pub struct Vault {
    main: Root,
    worktrees: Vec<Worktree>,
}

impl Vault {
    #[must_use]
    pub fn new(main: Root) -> Self {
        Self {
            main,
            worktrees: Vec::new(),
        }
    }

    /// Attaches worktrees `(name, folder)`. Skipped: missing folders, the main root | a folder containing it,
    /// duplicates. Names sanitized (`/ \ : NUL` -> `_`, empty -> `worktree`) + made unique (`-2`, `-3`, ...).
    #[must_use]
    pub fn with_worktrees(mut self, found: impl IntoIterator<Item = (String, PathBuf)>) -> Self {
        let mut accepted: Vec<Worktree> = Vec::new();
        for (raw_name, path) in found {
            let Ok(root) = Root::open(&path) else {
                continue;
            };
            let contains_main = strip_root(self.main.path(), root.path()).is_some();
            if contains_main || accepted.iter().any(|w| w.root == root) {
                continue;
            }
            let base: String = raw_name
                .chars()
                .map(|c| {
                    if matches!(c, '/' | '\\' | ':' | '\0') {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
            let base = if base.trim().is_empty() {
                "worktree".to_owned()
            } else {
                base
            };
            let mut name = base.clone();
            let mut n = 2;
            while accepted.iter().any(|w| fold(&w.name) == fold(&name)) {
                name = format!("{base}-{n}");
                n += 1;
            }
            accepted.push(Worktree { name, root });
        }
        self.worktrees = accepted;
        self
    }

    #[must_use]
    pub fn main(&self) -> &Root {
        &self.main
    }

    #[must_use]
    pub fn worktrees(&self) -> &[Worktree] {
        &self.worktrees
    }

    /// Root a path belongs to + its part below that root.
    fn split<'a>(&'a self, path: &'a VaultPath) -> Result<(&'a Root, Vec<&'a str>), VaultError> {
        let mut segments = path.segments();
        let first = path.as_str().strip_prefix(WORKTREE_PREFIX);
        if first.is_none() {
            return Ok((&self.main, segments.collect()));
        }
        segments.next(); // "@wt"
        let name = segments
            .next()
            .ok_or_else(|| VaultError::NotFound(path.to_string()))?;
        let worktree = self
            .worktrees
            .iter()
            .find(|w| fold(&w.name) == fold(name))
            .ok_or_else(|| VaultError::NotFound(format!("{WORKTREE_PREFIX}{name}")))?;
        Ok((&worktree.root, segments.collect()))
    }

    /// Absolute path of `path`, checked on its real location: the deepest existing ancestor is canonicalized
    /// (symlinks, junctions) and MUST stay under its root. A missing tail is allowed (new files).
    ///
    /// # Errors
    /// `Escapes` (link leads outside), `NotFound` (unknown worktree | nothing exists).
    pub fn resolve(&self, path: &VaultPath) -> Result<PathBuf, VaultError> {
        let (root, parts) = self.split(path)?;
        resolve_under(root, &parts, path)
    }

    /// Project-relative path of an absolute one: most specific root wins. A different spelling (8.3 short name,
    /// symlinked alias) is retried canonical, only on a volume a root lives on.
    #[must_use]
    pub fn relativize(&self, abs: &Path) -> Option<VaultPath> {
        let abs = super::simplified(abs);
        let abs: &Path = &abs;
        self.locate(abs).or_else(|| {
            let roots = std::iter::once(&self.main).chain(self.worktrees.iter().map(|w| &w.root));
            if !roots.into_iter().any(|r| same_volume(abs, r.path())) {
                return None;
            }
            let canon = canonicalize_lenient(abs).filter(|c| c != abs)?;
            self.locate(&canon)
        })
    }

    fn locate(&self, abs: &Path) -> Option<VaultPath> {
        let mut best: Option<(Option<&str>, Vec<String>)> = None;
        let candidates = std::iter::once((None, &self.main)).chain(
            self.worktrees
                .iter()
                .map(|w| (Some(w.name.as_str()), &w.root)),
        );
        for (name, root) in candidates {
            if let Some(rest) = strip_root(abs, root.path()) {
                if best.as_ref().is_none_or(|(_, b)| rest.len() < b.len()) {
                    let rest = rest
                        .iter()
                        .map(|s| s.to_string_lossy().into_owned())
                        .collect();
                    best = Some((name, rest));
                }
            }
        }
        let (name, rest) = best?;
        let prefix = name.map(|n| ["@wt".to_owned(), n.to_owned()]);
        let all: Vec<&str> = prefix
            .iter()
            .flatten()
            .map(String::as_str)
            .chain(rest.iter().map(String::as_str))
            .collect();
        Some(VaultPath::from_segments(all))
    }
}

/// `root` + `parts`, containment re-checked on the canonical deepest existing ancestor.
pub(super) fn resolve_under(
    root: &Root,
    parts: &[&str],
    shown: &VaultPath,
) -> Result<PathBuf, VaultError> {
    let mut abs = root.path().to_path_buf();
    abs.extend(parts);
    let Some((canon, tail)) = split_existing(&abs) else {
        return Err(VaultError::NotFound(shown.to_string()));
    };
    if strip_root(&canon, root.path()).is_none() {
        return Err(VaultError::Escapes(shown.to_string()));
    }
    let mut full = canon;
    full.extend(tail);
    Ok(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault_with(files: &[&str]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let p = dir.path().join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, "x").unwrap();
        }
        let v = Vault::new(Root::open(dir.path()).unwrap());
        (dir, v)
    }

    fn vp(raw: &str) -> VaultPath {
        VaultPath::parse(raw).unwrap()
    }

    #[test]
    fn resolves_nested_existing_and_missing_paths() {
        let (_d, v) = vault_with(&["a/b/c.md"]);
        assert!(v.resolve(&vp("a/b/c.md")).unwrap().is_file());
        let new = v.resolve(&vp("new/dir/file.md")).unwrap();
        assert!(!new.exists());
        assert!(new.starts_with(v.main().path()));
        assert_eq!(v.resolve(&VaultPath::root()).unwrap(), v.main().path());
    }

    #[cfg(windows)]
    #[test]
    fn resolve_returns_the_real_on_disk_case() {
        let (_d, v) = vault_with(&["Docs/ReadMe.md"]);
        let abs = v.resolve(&vp("docs/README.MD")).unwrap();
        assert!(abs.ends_with("Docs/ReadMe.md"));
        assert_eq!(v.relativize(&abs).unwrap().as_str(), "Docs/ReadMe.md");
    }

    #[test]
    fn relativize_round_trips_and_rejects_outside() {
        let (d, v) = vault_with(&["docs/文档 a.md"]);
        let abs = v.resolve(&vp("docs/文档 a.md")).unwrap();
        assert_eq!(v.relativize(&abs).unwrap().as_str(), "docs/文档 a.md");
        assert_eq!(v.relativize(v.main().path()).unwrap(), VaultPath::root());
        let other = tempfile::tempdir().unwrap();
        assert_eq!(v.relativize(&other.path().join("x.md")), None);
        // sibling with the root's name as prefix is outside
        let sibling = PathBuf::from(format!("{}x", d.path().display()));
        assert_eq!(v.relativize(&sibling.join("a.md")), None);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_refused_and_inside_link_allowed() {
        let (d, v) = vault_with(&["a.md", "inner/b.md"]);
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.md"), "s").unwrap();
        std::os::unix::fs::symlink(outside.path(), d.path().join("link")).unwrap();
        std::os::unix::fs::symlink(d.path().join("inner"), d.path().join("alias")).unwrap();
        assert!(matches!(
            v.resolve(&vp("link/secret.md")),
            Err(VaultError::Escapes(_))
        ));
        assert!(matches!(
            v.resolve(&vp("link/new.md")),
            Err(VaultError::Escapes(_))
        ));
        assert!(matches!(
            v.resolve(&vp("link")),
            Err(VaultError::Escapes(_))
        ));
        assert!(v.resolve(&vp("alias/b.md")).unwrap().is_file());
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[cfg(windows)]
    #[test]
    fn junction_escape_is_refused() {
        let (d, v) = vault_with(&["a.md"]);
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.md"), "s").unwrap();
        if !junction(&d.path().join("jump"), outside.path()) {
            eprintln!("skipped: cannot create a junction here");
            return;
        }
        assert!(matches!(
            v.resolve(&vp("jump/secret.md")),
            Err(VaultError::Escapes(_))
        ));
        assert!(matches!(
            v.resolve(&vp("jump/new/x.md")),
            Err(VaultError::Escapes(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn paths_longer_than_260_characters_resolve_and_stay_closed() {
        let (d, v) = vault_with(&["a.md"]);
        let mut rel = String::new();
        while d.path().display().to_string().len() + rel.len() < 320 {
            rel.push_str("segment-with-a-long-name/");
        }
        rel.push_str("deep.md");
        let abs = d.path().join(&rel);
        fs::create_dir_all(abs.parent().unwrap()).unwrap();
        fs::write(&abs, "x").unwrap();
        let resolved = v.resolve(&vp(&rel)).unwrap();
        assert!(resolved.is_file());
        assert_eq!(v.relativize(&resolved).unwrap().as_str(), rel);
        // verbatim spelling of an inside path is inside
        let verbatim = PathBuf::from(format!(r"\\?\{}", v.main().path().join("a.md").display()));
        assert_eq!(v.relativize(&verbatim).unwrap().as_str(), "a.md");
    }

    #[test]
    fn worktrees_are_prefixed_resolvable_and_sanitized() {
        let (d, v) = vault_with(&["a.md", "inner/wt/w.md"]);
        let wt = tempfile::tempdir().unwrap();
        fs::write(wt.path().join("w.md"), "x").unwrap();
        let v = v.with_worktrees([
            ("gone".to_owned(), d.path().join("missing")),
            ("self".to_owned(), d.path().to_path_buf()),
            ("a/b".to_owned(), wt.path().to_path_buf()),
            ("dup".to_owned(), wt.path().to_path_buf()),
            ("inner".to_owned(), d.path().join("inner").join("wt")),
            ("A_B".to_owned(), d.path().join("inner")),
        ]);
        let names: Vec<&str> = v.worktrees().iter().map(|w| w.name.as_str()).collect();
        if crate::paths::CASE_INSENSITIVE {
            assert_eq!(names, ["a_b", "inner", "A_B-2"]);
        } else {
            assert_eq!(names, ["a_b", "inner", "A_B"]);
        }
        assert!(v.resolve(&vp("@wt/a_b/w.md")).unwrap().is_file());
        assert!(matches!(
            v.resolve(&vp("@wt/other/w.md")),
            Err(VaultError::NotFound(_))
        ));
        // `..` out of a worktree lands in the main root (still inside); one more escapes
        assert_eq!(vp("@wt/a_b/../../a.md").as_str(), "a.md");
        assert!(matches!(
            VaultPath::parse("@wt/a_b/../../../x"),
            Err(VaultError::Escapes(_))
        ));
        // nested worktree wins over the main root and over its containing worktree
        let nested = d.path().join("inner").join("wt").join("w.md");
        assert_eq!(v.relativize(&nested).unwrap().as_str(), "@wt/inner/w.md");
        assert_eq!(
            v.relativize(&d.path().join("a.md")).unwrap().as_str(),
            "a.md"
        );
    }

    #[test]
    fn unicode_and_space_roots_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Meu projeto 项目");
        fs::create_dir_all(root.join("sub dir")).unwrap();
        fs::write(root.join("sub dir").join("ñ.md"), "x").unwrap();
        let v = Vault::new(Root::open(&root).unwrap());
        assert!(v.resolve(&vp("sub dir/ñ.md")).unwrap().is_file());
    }
}
