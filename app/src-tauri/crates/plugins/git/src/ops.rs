//! The read-only operations both git backends implement, and the pure rules they share (paths below the project,
//! "too broad" repositories, repository state from marker files). NEVER commits, stages or creates anything.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use permark_core::conditions::RepoState;
use permark_core::plugins::{CommitId, FileVersion, FileVersions, NoBlob, NoVersions};

use crate::types::{BackendKind, HeadInfo, Letter};
use crate::worktrees::{common_dir, in_nested_repository};

/// More changed files than this are cut off (`truncated`).
pub const MAX_FILES: usize = 5000;
/// Most changes of a file one page of its versions reads.
pub const MAX_VERSIONS: usize = 500;
/// Most paths a commit may have changed for the name a file had before to be looked for in it. Finding a rename
/// compares every file the commit added with every file it deleted: a commit that changed more is not searched, and
/// each backend passes this number as the limit of that search (not the `diff.renameLimit` of the repository).
pub const MAX_RENAME_PATHS: usize = 1000;
/// Most co-authors kept of a commit message.
pub const MAX_CO_AUTHORS: usize = 16;

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
    /// The file `path` (relative to the project folder) as committed at `commit`, raw (no filter, no line-ending
    /// conversion); `None`: not there (no such commit, no such file, or the object is not on this computer: nothing
    /// is fetched). Only an id that leads to a commit is read from (a tag of one is followed): the id of a tree names
    /// any folder of the repository, also one outside the project folder, so it reads as not there.
    ///
    /// # Errors
    /// Git failed, timed out or the file is too large.
    fn blob_at(
        &self,
        location: &RepoLocation,
        commit: CommitId,
        path: &str,
    ) -> Result<Option<Vec<u8>>, GitError>;
    /// One page of the versions of the file `path` (relative to the project folder), newest first by commit time.
    /// Every commit reachable from HEAD is looked at (merged branches included); a version is a commit with at most
    /// one parent that holds a file at the path whose content its parent does not hold there ([`is_version`]).
    /// Renames are not followed. `after`: 0 | the `next` of the page before (any other number is answered too: one
    /// past the end of the history is the end of the list). `limit` (held in 1..=[`MAX_VERSIONS`]):
    /// most changes of the path read; a commit that only deleted the file | changed its mode is such a change and no
    /// row, so a page may hold fewer rows. No commit yet | a path that was never committed: an empty page at its end.
    ///
    /// # Errors
    /// Git failed | timed out.
    fn file_versions(
        &self,
        location: &RepoLocation,
        path: &str,
        after: usize,
        limit: usize,
    ) -> Result<FileVersions, GitError>;
    /// Whether the repository has the commit.
    fn commit_exists(&self, location: &RepoLocation, oid: &str) -> bool;
    /// Commits reachable from `new` and not from `old` (`None`: a commit unknown | git failed).
    fn commit_count(&self, location: &RepoLocation, old: &str, new: &str) -> Option<u32>;
    /// `(ahead, behind)` of HEAD against its branch's upstream as last fetched (PerMark never fetches); `None`: no
    /// upstream, detached, no commit.
    ///
    /// # Errors
    /// Git failed | timed out.
    fn ahead_behind(&self, location: &RepoLocation) -> Result<Option<(u32, u32)>, GitError>;
    /// The commit at HEAD: committer time (ms since epoch) + subject (first line, <= [`MAX_SUBJECT`] chars); `None`:
    /// no commit yet.
    ///
    /// # Errors
    /// Git failed | timed out.
    fn last_commit(&self, location: &RepoLocation) -> Result<Option<(i64, String)>, GitError>;
    /// The root commit of HEAD's history (several roots -> the smallest id); `None`: no commit yet.
    ///
    /// # Errors
    /// Git failed | timed out.
    fn root_commit(&self, location: &RepoLocation) -> Result<Option<String>, GitError>;
    /// The same backend whose calls stop when `flag` is set (a running program is killed); `None`: calls of this
    /// backend cannot be stopped (they run to their end; the caller drops the answer).
    fn cancellable(&self, _flag: &Arc<AtomicBool>) -> Option<Arc<dyn GitOps>> {
        None
    }
}

/// A backend's answer for a file at a commit ([`GitOps::blob_at`]) as the reader other plugins use gives it.
///
/// # Errors
/// [`NoBlob`]: not there, too large, or the call failed.
pub fn blob_or_miss(read: Result<Option<Vec<u8>>, GitError>) -> Result<Vec<u8>, NoBlob> {
    match read {
        Ok(Some(bytes)) => Ok(bytes),
        Ok(None) => Err(NoBlob::Missing),
        Err(GitError::TooLarge(_)) => Err(NoBlob::TooLarge),
        Err(_) => Err(NoBlob::Unavailable),
    }
}

/// Longest subject kept of a commit message.
pub const MAX_SUBJECT: usize = 200;

/// The subject of a commit message: its first non-empty line, trimmed, cut at [`MAX_SUBJECT`] chars.
#[must_use]
pub fn subject_of(message: &str) -> String {
    let line = message
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    line.chars().take(MAX_SUBJECT).collect()
}

/// Whether a tree entry with this mode is a regular file (plain | executable).
#[must_use]
pub const fn regular(mode: u32) -> bool {
    mode == 0o100_644 || mode == 0o100_755
}

/// Whether a commit made a version of a file. `now`: what its tree holds at the path, `before`: what its parent's
/// tree holds there (mode + object id; `None`: nothing). A version holds a regular file whose content the parent does
/// not hold as a file: a commit that deleted the file or changed only its mode made none.
#[must_use]
pub fn is_version<I: PartialEq>(before: Option<(u32, I)>, now: Option<(u32, I)>) -> bool {
    match now {
        Some((mode, id)) if regular(mode) => {
            !before.is_some_and(|(mode, held)| regular(mode) && held == id)
        }
        _ => false,
    }
}

/// The names in the `Co-authored-by: Name <mail>` lines of a commit message, in order: no name twice, never
/// `author`, each cut at [`MAX_SUBJECT`] chars, at most [`MAX_CO_AUTHORS`].
#[must_use]
pub fn co_authors_of(message: &str, author: &str) -> Vec<String> {
    const KEY: &str = "co-authored-by:";
    let mut names: Vec<String> = Vec::new();
    for line in message.lines() {
        // cut where the key would end, only if a character ends there (the line may hold any text)
        let Some((_, person)) = line
            .split_at_checked(KEY.len())
            .filter(|(key, _)| key.eq_ignore_ascii_case(KEY))
        else {
            continue;
        };
        let name = name_of(person.split('<').next().unwrap_or(""));
        if !name.is_empty() && name != author && !names.contains(&name) {
            names.push(name);
            if names.len() == MAX_CO_AUTHORS {
                break;
            }
        }
    }
    names
}

/// A person's name as a row holds it: trimmed, cut at [`MAX_SUBJECT`] chars.
fn name_of(text: &str) -> String {
    text.trim().chars().take(MAX_SUBJECT).collect()
}

/// The row of a version from what a backend read of its commit: `seconds` = the committer's time, `author` = the
/// author's name, `message` = the whole message. Both backends build their rows here, so they read alike.
#[must_use]
pub fn version(
    commit: CommitId,
    blob: String,
    seconds: i64,
    author: &str,
    message: &str,
) -> FileVersion {
    let author = name_of(author);
    FileVersion {
        commit,
        blob,
        time_ms: seconds.saturating_mul(1000),
        co_authors: co_authors_of(message, &author),
        subject: subject_of(message),
        author,
    }
}

/// A page of the versions of a file ([`GitOps::file_versions`]) as the reader other plugins use gives it.
///
/// # Errors
/// [`NoVersions`]: the file is inside another repository below this one, git was too slow, or the call failed.
pub fn versions_of(
    backend: &dyn GitOps,
    location: &RepoLocation,
    path: &str,
    after: usize,
    limit: usize,
) -> Result<FileVersions, NoVersions> {
    if in_nested_repository(&location.workdir, &repo_path(&location.prefix, path)) {
        return Err(NoVersions::OtherRepository);
    }
    backend
        .file_versions(location, path, after, limit)
        .map_err(|error| match error {
            GitError::Timeout => NoVersions::Slow,
            _ => NoVersions::Failed,
        })
}

/// `path` (relative to the working tree) below the project folder `prefix`: the part below it; `None`: outside.
#[must_use]
pub fn below_prefix<'a>(prefix: &str, path: &'a str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(path);
    }
    path.strip_prefix(prefix)?.strip_prefix('/')
}

/// The other way: `path` (relative to the project folder `prefix`) relative to the working tree.
#[must_use]
pub fn repo_path(prefix: &str, path: &str) -> String {
    if prefix.is_empty() {
        path.to_owned()
    } else {
        format!("{prefix}/{path}")
    }
}

/// The pathspec that limits git to the project folder (`None`: the whole working tree).
#[must_use]
pub fn pathspec(prefix: &str) -> Option<String> {
    (!prefix.is_empty()).then(|| format!(":(literal){prefix}"))
}

fn canonical(path: &Path) -> PathBuf {
    permark_core::paths::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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

/// Whether the repository is a shallow copy (it holds only the newest part of the history). Git lists the commits
/// the history is cut at in the file `shallow`, kept with the objects: a linked worktree shares the one of its
/// common git folder.
#[must_use]
pub fn shallow(git_dir: &Path) -> bool {
    common_dir(git_dir).join("shallow").exists()
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
        assert_eq!(repo_path("", "a/b.md"), "a/b.md");
        assert_eq!(repo_path("docs/x y", "-a.md"), "docs/x y/-a.md");
        assert_eq!(
            below_prefix("docs", &repo_path("docs", "a.md")),
            Some("a.md")
        );
    }

    #[test]
    fn a_backend_answer_for_a_file_at_a_commit_becomes_bytes_or_the_typed_miss() {
        assert_eq!(blob_or_miss(Ok(Some(b"x".to_vec()))), Ok(b"x".to_vec()));
        assert_eq!(blob_or_miss(Ok(Some(Vec::new()))), Ok(Vec::new()));
        assert_eq!(blob_or_miss(Ok(None)), Err(NoBlob::Missing));
        assert_eq!(
            blob_or_miss(Err(GitError::TooLarge(8))),
            Err(NoBlob::TooLarge)
        );
        for failed in [
            GitError::Timeout,
            GitError::Spawn("no git".to_owned()),
            GitError::Failed("stopped".to_owned()),
            GitError::Parse("?".to_owned()),
        ] {
            assert_eq!(blob_or_miss(Err(failed)), Err(NoBlob::Unavailable));
        }
    }

    #[test]
    fn a_version_is_a_file_whose_content_its_parent_does_not_hold() {
        const FILE: u32 = 0o100_644;
        const EXEC: u32 = 0o100_755;
        const LINK: u32 = 0o120_000;
        const TREE: u32 = 0o040_000;
        const SUBMODULE: u32 = 0o160_000;
        // (before, now, a version?)
        let cases = [
            (None, Some((FILE, "a")), true),
            (None, Some((EXEC, "a")), true),
            (Some((FILE, "a")), Some((FILE, "b")), true),
            (Some((FILE, "a")), Some((EXEC, "b")), true),
            // the mode only
            (Some((FILE, "a")), Some((EXEC, "a")), false),
            (Some((EXEC, "a")), Some((FILE, "a")), false),
            // untouched | deleted | never there
            (Some((FILE, "a")), Some((FILE, "a")), false),
            (Some((FILE, "a")), None, false),
            (None, None, false),
            // something else than a file before: the same bytes are a new file
            (Some((LINK, "a")), Some((FILE, "a")), true),
            (Some((TREE, "t")), Some((FILE, "a")), true),
            (Some((SUBMODULE, "c")), Some((EXEC, "a")), true),
            // something else than a file now
            (Some((FILE, "a")), Some((LINK, "b")), false),
            (None, Some((LINK, "a")), false),
            (None, Some((TREE, "t")), false),
            (None, Some((SUBMODULE, "c")), false),
        ];
        for (before, now, expected) in cases {
            assert_eq!(is_version(before, now), expected, "{before:?} -> {now:?}");
        }
        assert!(regular(FILE) && regular(EXEC));
        assert!(!regular(LINK) && !regular(TREE) && !regular(SUBMODULE) && !regular(0));
    }

    #[test]
    fn co_authors_are_the_names_of_the_trailer_lines() {
        let message = "Subject\n\nBody, Co-authored-by: Nobody <n@x>\n\n\
            Co-authored-by: Bea Ü <bea@example.com>\n\
            co-authored-by:   Cy  \n\
            CO-AUTHORED-BY: Ann <ann@other.example>\n\
            Co-authored-by: Bea Ü <second@example.com>\n\
            Co-authored-by: <only@mail>\n\
            Co-authored-by:\n\
            Co-authored-by: Dee <d@x> trailing\r\n\
            Signed-off-by: Eve <e@x>\n";
        assert_eq!(co_authors_of(message, "Ann"), ["Bea Ü", "Cy", "Dee"]);
        assert_eq!(
            co_authors_of(message, "Someone else"),
            ["Bea Ü", "Cy", "Ann", "Dee"]
        );
        assert!(co_authors_of("Subject\n\nBody\n", "Ann").is_empty());
        assert!(co_authors_of("", "").is_empty());
        // a message cannot fill a row with names
        let many = (0..100).fold(String::new(), |all, n| {
            format!("{all}Co-authored-by: P{n} <p{n}@x>\n")
        });
        assert_eq!(co_authors_of(&many, "Ann").len(), MAX_CO_AUTHORS);
        let long = format!("Co-authored-by: {} <l@x>\n", "é".repeat(MAX_SUBJECT + 50));
        assert_eq!(
            co_authors_of(&long, "Ann"),
            ["é".repeat(MAX_SUBJECT)],
            "cut at a character"
        );
    }

    #[test]
    fn a_row_is_built_from_what_a_backend_read_of_the_commit() {
        let commit = CommitId::parse(&"ab".repeat(20)).unwrap();
        let row = version(
            commit,
            "cd".repeat(20),
            1_700_000_000,
            "  Ann Ü ",
            "\n  The subject  \n\nBody\nCo-authored-by: Bea <b@x>\nCo-authored-by: Ann Ü <a@x>\n",
        );
        assert_eq!(
            row,
            FileVersion {
                commit,
                blob: "cd".repeat(20),
                time_ms: 1_700_000_000_000,
                author: "Ann Ü".to_owned(),
                co_authors: vec!["Bea".to_owned()],
                subject: "The subject".to_owned(),
            }
        );
        let odd = version(
            commit,
            String::new(),
            i64::MAX,
            &"n".repeat(MAX_SUBJECT + 9),
            &"s".repeat(MAX_SUBJECT + 9),
        );
        assert_eq!(odd.time_ms, i64::MAX, "no overflow");
        assert_eq!(odd.author.chars().count(), MAX_SUBJECT);
        assert_eq!(odd.subject, subject_of(&"s".repeat(MAX_SUBJECT + 9)));
        assert_eq!(version(commit, String::new(), -5, "", "").time_ms, -5000);
    }

    #[test]
    fn a_shallow_copy_is_told_by_its_marker_file() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join("main").join(".git");
        std::fs::create_dir_all(&git).unwrap();
        assert!(!shallow(&git));
        // a linked worktree keeps its objects (and the marker) in the common git folder
        let admin = git.join("worktrees").join("w");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        assert!(!shallow(&admin));
        std::fs::write(git.join("shallow"), format!("{}\n", "a".repeat(40))).unwrap();
        assert!(shallow(&git));
        assert!(shallow(&admin));
        assert!(!shallow(&dir.path().join("nowhere")));
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
