//! Finding the repository for a vault, and its linked worktrees.

use std::path::{Path, PathBuf};
use std::sync::Once;

use git2::{ErrorCode, Repository};

use crate::vault::{strip_root, Worktree};

/// Where the repository is, relative to the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitContext {
    /// Canonical working directory of the repository (or of the linked worktree the vault is in).
    pub workdir: PathBuf,
    /// The repository's own directory; for a linked worktree this is `<common>/worktrees/<name>`.
    pub git_dir: PathBuf,
    /// The shared directory holding refs, objects and `info/exclude`.
    pub common_dir: PathBuf,
    /// The vault's folder relative to `workdir` (`/`-separated; empty when they are the same folder).
    pub vault_prefix: String,
}

impl GitContext {
    /// Opens the repository (each thread opens its own handle).
    pub fn open(&self) -> Result<Repository, git2::Error> {
        init_libgit2();
        Repository::open(&self.workdir)
    }

    /// `.git/info/exclude`, if it exists.
    pub fn info_exclude(&self) -> Option<PathBuf> {
        let p = self.common_dir.join("info").join("exclude");
        p.is_file().then_some(p)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discovery {
    Repo(GitContext),
    /// Not a repository, or one that must not be used (home folder, or the vault is ignored by it).
    NoRepo,
    /// A repository exists but could not be opened; shown to the user in `gitError`.
    Error(String),
}

/// One-time libgit2 setup. Owner validation is turned off: TrailMark only reads repositories, and
/// libgit2's check makes repositories on non-system drives (the `F:` case) look like "not a repo".
pub(crate) fn init_libgit2() {
    static ONCE: Once = Once::new();
    ONCE.call_once(disable_owner_validation);
}

// SAFETY: `set_verify_owner_validation` is `unsafe` only because it flips a process-wide libgit2
// option. It is called once (guarded by `Once`) before TrailMark opens any repository, and the
// option is never changed again.
#[allow(unsafe_code)]
fn disable_owner_validation() {
    // The function cannot fail in practice; there is nothing useful to do if it did.
    let _ = unsafe { git2::opts::set_verify_owner_validation(false) };
}

fn same_folder(a: &Path, b: &Path) -> bool {
    strip_root(a, b).is_some_and(|rest| rest.is_empty())
}

/// Finds the repository containing `vault_root` (canonical), walking up from it.
///
/// A repository found above the vault is ignored when its working directory is `home` (a dotfiles
/// repo) or when the repository ignores the vault folder itself.
pub fn discover(vault_root: &Path, home: Option<&Path>) -> Discovery {
    init_libgit2();
    let repo = match Repository::discover(vault_root) {
        Ok(r) => r,
        Err(e) if e.code() == ErrorCode::NotFound => return Discovery::NoRepo,
        Err(e) => return Discovery::Error(e.message().to_owned()),
    };
    let Some(workdir) = repo.workdir().and_then(|w| dunce::canonicalize(w).ok()) else {
        return Discovery::NoRepo; // bare repository
    };
    if let Some(home) = home.and_then(|h| dunce::canonicalize(h).ok()) {
        if same_folder(&workdir, &home) {
            return Discovery::NoRepo;
        }
    }
    let Some(rest) = strip_root(vault_root, &workdir) else {
        return Discovery::NoRepo;
    };
    let vault_prefix = rest
        .iter()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if !vault_prefix.is_empty() && repo.is_path_ignored(&vault_prefix).unwrap_or(false) {
        return Discovery::NoRepo;
    }
    let simplify = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    Discovery::Repo(GitContext {
        workdir,
        git_dir: simplify(repo.path()),
        common_dir: simplify(repo.commondir()),
        vault_prefix,
    })
}

/// Linked worktrees of the repository (`git worktree add`), skipping missing folders.
pub fn list_worktrees(ctx: &GitContext) -> Vec<Worktree> {
    let Ok(repo) = ctx.open() else {
        return Vec::new();
    };
    let Ok(names) = repo.worktrees() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in names.iter().flatten().flatten() {
        let Ok(wt) = repo.find_worktree(name) else {
            continue;
        };
        let path = wt.path().to_path_buf();
        if path.is_dir() {
            out.push(Worktree {
                name: name.to_owned(),
                branch: worktree_branch(&wt),
                path,
            });
        }
    }
    out
}

/// The branch checked out in a linked worktree (`None` when detached, unborn or unreadable).
fn worktree_branch(wt: &git2::Worktree) -> Option<String> {
    let repo = Repository::open_from_worktree(wt).ok()?;
    let head = repo.head().ok()?;
    if head.is_branch() {
        head.shorthand().ok().map(str::to_owned)
    } else {
        None
    }
}

/// The commit HEAD points to, as a hex string (`None` for an unborn branch or on error).
pub fn read_head_oid(ctx: &GitContext) -> Option<String> {
    let repo = ctx.open().ok()?;
    let head = repo.head().ok()?;
    let commit = head.peel_to_commit().ok()?;
    let oid = commit.id().to_string();
    Some(oid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::{commit_all, init_repo, write};

    fn canon(p: &Path) -> PathBuf {
        dunce::canonicalize(p).expect("canonicalize")
    }

    #[test]
    fn plain_folder_is_not_a_repo() {
        let d = tempfile::tempdir().expect("tempdir");
        assert_eq!(discover(&canon(d.path()), None), Discovery::NoRepo);
    }

    #[test]
    fn repo_root_vault_has_an_empty_prefix() {
        let d = tempfile::tempdir().expect("tempdir");
        init_repo(d.path());
        let Discovery::Repo(ctx) = discover(&canon(d.path()), None) else {
            panic!("expected a repo");
        };
        assert_eq!(ctx.vault_prefix, "");
        assert_eq!(ctx.workdir, canon(d.path()));
        assert!(ctx.git_dir.ends_with(".git"));
        assert_eq!(ctx.git_dir, ctx.common_dir);
        assert!(ctx.open().is_ok());
    }

    #[test]
    fn subfolder_vault_finds_the_repo_above_it() {
        let d = tempfile::tempdir().expect("tempdir");
        init_repo(d.path());
        write(d.path(), "docs/deep/a.md", "x");
        let vault = canon(&d.path().join("docs").join("deep"));
        let Discovery::Repo(ctx) = discover(&vault, None) else {
            panic!("expected a repo");
        };
        assert_eq!(ctx.vault_prefix, "docs/deep");
        assert_eq!(ctx.workdir, canon(d.path()));
    }

    #[test]
    fn a_repo_in_the_home_folder_is_not_the_vaults_repo() {
        let home = tempfile::tempdir().expect("tempdir");
        init_repo(home.path());
        write(home.path(), "notes/a.md", "x");
        let vault = canon(&home.path().join("notes"));
        assert_eq!(discover(&vault, Some(home.path())), Discovery::NoRepo);
        // The same layout is fine when the parent is not the home folder.
        assert!(matches!(discover(&vault, None), Discovery::Repo(_)));
    }

    #[test]
    fn a_vault_ignored_by_the_parent_repo_is_not_in_a_repo() {
        let d = tempfile::tempdir().expect("tempdir");
        init_repo(d.path());
        write(d.path(), ".gitignore", "scratch/\n");
        write(d.path(), "scratch/a.md", "x");
        write(d.path(), "tracked/a.md", "x");
        assert_eq!(
            discover(&canon(&d.path().join("scratch")), None),
            Discovery::NoRepo
        );
        assert!(matches!(
            discover(&canon(&d.path().join("tracked")), None),
            Discovery::Repo(_)
        ));
    }

    #[test]
    fn worktrees_are_listed_by_name_and_path() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "a.md", "x");
        commit_all(&repo, "init");
        let wt_dir = tempfile::tempdir().expect("tempdir");
        let wt_path = wt_dir.path().join("feature-x");
        repo.worktree("feature-x", &wt_path, None)
            .expect("worktree");
        let Discovery::Repo(ctx) = discover(&canon(d.path()), None) else {
            panic!("expected a repo");
        };
        let list = list_worktrees(&ctx);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "feature-x");
        assert_eq!(canon(&list[0].path), canon(&wt_path));
        // `git worktree add` creates a branch named after the worktree and checks it out.
        assert_eq!(list[0].branch.as_deref(), Some("feature-x"));

        // From inside the linked worktree the vault still finds the repo, with its own git dir.
        let Discovery::Repo(wt_ctx) = discover(&canon(&wt_path), None) else {
            panic!("expected a repo in the worktree");
        };
        assert_eq!(wt_ctx.workdir, canon(&wt_path));
        assert_ne!(wt_ctx.git_dir, wt_ctx.common_dir);
        assert_eq!(wt_ctx.common_dir, ctx.common_dir);
    }

    #[test]
    fn head_oid_of_unborn_and_committed_repos() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        let Discovery::Repo(ctx) = discover(&canon(d.path()), None) else {
            panic!("expected a repo");
        };
        assert_eq!(read_head_oid(&ctx), None);
        write(d.path(), "a.md", "x");
        let oid = commit_all(&repo, "init");
        assert_eq!(read_head_oid(&ctx), Some(oid.to_string()));
    }
}
