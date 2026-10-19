//! The read-only operations both git backends implement, and the pure rules they share (paths below the project,
//! "too broad" repositories, repository state from marker files). NEVER commits, stages or creates anything.

use std::path::{Path, PathBuf};

use permark_core::conditions::RepoState;

use crate::types::{BackendKind, HeadInfo, Letter};

/// More changed files than this are cut off (`truncated`).
pub const MAX_FILES: usize = 5000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GitError {
    #[error("cannot run git: {0}")]
    Spawn(String),
    #[error("git failed: {0}")]
    Failed(String),
    #[error("git did not answer in time")]
    Timeout,
    #[error("git printed more than {0} bytes")]
    TooLarge(usize),
    #[error("unreadable git output: {0}")]
    Parse(String),
}

impl From<git2::Error> for GitError {
    fn from(error: git2::Error) -> Self {
        Self::Failed(error.message().to_owned())
    }
}

/// Where the repository of a project is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoLocation {
    /// The working tree folder (where `.git` is, or the linked worktree).
    pub workdir: PathBuf,
    /// The git folder of that working tree (`.git`, or `.git/worktrees/<name>`).
    pub git_dir: PathBuf,
    /// The project folder below `workdir`, `/`-separated, no slashes at the ends; empty = the same folder.
    pub prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discovery {
    Repo(RepoLocation),
    NoRepo,
    /// A repository is there but cannot be used (the sentence says why).
    Error(String),
}

/// What status is measured against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Baseline {
    Head,
    /// The working tree against this commit (full hex id), untracked files included.
    Commit(String),
}

/// A changed file, path relative to the project folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFile {
    pub path: String,
    pub letter: Letter,
    pub staged: bool,
    pub from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawStatus {
    pub head: HeadInfo,
    /// Sorted by path, at most [`MAX_FILES`].
    pub files: Vec<RawFile>,
    pub truncated: bool,
}

/// One git backend. Stateless: every call stands alone (no handle is shared across threads).
pub trait GitOps: Send + Sync {
    fn kind(&self) -> BackendKind;
    /// Finds the repository of the folder `root` (walking up), refusing the ones that are too broad.
    fn discover(&self, root: &Path, home: Option<&Path>) -> Discovery;
    /// The changed files below the project folder, and where HEAD is.
    ///
    /// # Errors
    /// Git failed, timed out or printed something unreadable.
    fn status(&self, location: &RepoLocation, baseline: &Baseline) -> Result<RawStatus, GitError>;
    /// The file `path` (relative to the project folder) as committed at HEAD; `None`: not there.
    ///
    /// # Errors
    /// Git failed, timed out or the file is too large.
    fn blob_at_head(
        &self,
        location: &RepoLocation,
        path: &str,
    ) -> Result<Option<Vec<u8>>, GitError>;
    /// Whether the repository has the commit.
    fn commit_exists(&self, location: &RepoLocation, oid: &str) -> bool;
}

/// `path` (relative to the working tree) below the project folder `prefix`: the part below it; `None`: outside.
#[must_use]
pub fn below_prefix<'a>(prefix: &str, path: &'a str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(path);
    }
    path.strip_prefix(prefix)?.strip_prefix('/')
}

/// The pathspec that limits git to the project folder (`None`: the whole working tree).
#[must_use]
pub fn pathspec(prefix: &str) -> Option<String> {
    (!prefix.is_empty()).then(|| format!(":(literal){prefix}"))
}

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn same_folder(a: &Path, b: &Path) -> bool {
    canonical(a) == canonical(b)
}

/// A repository whose working tree is a filesystem or drive root, or the home folder, is no project's repository
/// (a dotfiles repository, or a `.git` planted where every folder would bind to it), unless the project is that folder.
#[must_use]
pub fn too_broad(workdir: &Path, project: &Path, home: Option<&Path>) -> bool {
    if same_folder(workdir, project) {
        return false;
    }
    workdir.parent().is_none() || home.is_some_and(|home| same_folder(workdir, home))
}

/// The project folder below `workdir` as a `/`-separated prefix; `None`: not below it.
#[must_use]
pub fn prefix_of(workdir: &Path, project: &Path) -> Option<String> {
    let rest = canonical(project)
        .strip_prefix(canonical(workdir))
        .ok()?
        .to_path_buf();
    Some(
        rest.iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// The state of the repository from the marker files in its git folder.
#[must_use]
pub fn repo_state(git_dir: &Path) -> RepoState {
    let has = |name: &str| git_dir.join(name).exists();
    if has("rebase-merge") || has("rebase-apply") {
        RepoState::Rebasing
    } else if has("MERGE_HEAD") {
        RepoState::Merging
    } else if has("CHERRY_PICK_HEAD") {
        RepoState::CherryPicking
    } else if has("REVERT_HEAD") {
        RepoState::Reverting
    } else if has("BISECT_LOG") {
        RepoState::Bisecting
    } else if has("index.lock") {
        RepoState::Busy
    } else {
        RepoState::Clean
    }
}

/// Sorts by path and cuts at [`MAX_FILES`].
pub fn finish(mut files: Vec<RawFile>) -> (Vec<RawFile>, bool) {
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    let truncated = files.len() > MAX_FILES;
    files.truncate(MAX_FILES);
    (files, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_below_the_project_prefix() {
        assert_eq!(below_prefix("", "a/b.md"), Some("a/b.md"));
        assert_eq!(below_prefix("docs", "docs/a.md"), Some("a.md"));
        assert_eq!(below_prefix("docs", "docs2/a.md"), None);
        assert_eq!(below_prefix("docs", "docs"), None);
        assert_eq!(below_prefix("docs", "other/a.md"), None);
        assert_eq!(pathspec(""), None);
        assert_eq!(pathspec("a/b").as_deref(), Some(":(literal)a/b"));
    }

    #[test]
    fn broad_repositories_are_refused_unless_they_are_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("proj")).unwrap();
        // the home folder holds the repository: refused for a folder below it, fine for home itself
        assert!(too_broad(&home, &home.join("proj"), Some(&home)));
        assert!(!too_broad(&home, &home, Some(&home)));
        // an ordinary parent repository is fine
        assert!(!too_broad(dir.path(), &home.join("proj"), Some(&home)));
        // a filesystem root has no parent
        let root = dir.path().ancestors().last().unwrap().to_path_buf();
        assert!(too_broad(&root, dir.path(), None));
    }

    #[test]
    fn the_prefix_of_a_project_below_its_repository() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("a b").join("ü");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(prefix_of(dir.path(), &sub).as_deref(), Some("a b/ü"));
        assert_eq!(prefix_of(dir.path(), dir.path()).as_deref(), Some(""));
        assert_eq!(prefix_of(&sub, dir.path()), None);
    }

    #[test]
    fn repo_state_reads_marker_files_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path();
        assert_eq!(repo_state(git), RepoState::Clean);
        std::fs::write(git.join("index.lock"), b"").unwrap();
        assert_eq!(repo_state(git), RepoState::Busy);
        std::fs::write(git.join("BISECT_LOG"), b"").unwrap();
        assert_eq!(repo_state(git), RepoState::Bisecting);
        std::fs::write(git.join("REVERT_HEAD"), b"").unwrap();
        assert_eq!(repo_state(git), RepoState::Reverting);
        std::fs::write(git.join("CHERRY_PICK_HEAD"), b"").unwrap();
        assert_eq!(repo_state(git), RepoState::CherryPicking);
        std::fs::write(git.join("MERGE_HEAD"), b"").unwrap();
        assert_eq!(repo_state(git), RepoState::Merging);
        std::fs::create_dir(git.join("rebase-merge")).unwrap();
        assert_eq!(repo_state(git), RepoState::Rebasing);
    }

    #[test]
    fn finish_sorts_dedups_and_cuts() {
        let file = |path: &str| RawFile {
            path: path.to_owned(),
            letter: Letter::Modified,
            staged: false,
            from: None,
        };
        let (files, truncated) = finish(vec![file("b"), file("a"), file("b")]);
        assert_eq!(
            files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(!truncated);
        let many = (0..MAX_FILES + 3)
            .map(|n| file(&format!("f{n:05}")))
            .collect();
        let (files, truncated) = finish(many);
        assert_eq!(files.len(), MAX_FILES);
        assert!(truncated);
    }
}
