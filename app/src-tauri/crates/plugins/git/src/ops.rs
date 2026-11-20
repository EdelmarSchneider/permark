//! The read-only operations both git backends implement, and the pure rules they share (paths below the project,
//! "too broad" repositories, repository state from marker files). NEVER commits, stages or creates anything.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use permark_core::conditions::RepoState;
use permark_core::fsx::{same_file, SameFile};
use permark_core::plugins::{CommitId, FileVersion, FileVersions, NoBlob, NoTracking, NoVersions};

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
    /// A `.git` is there, but opening it would make git look up a path off this computer | through a link: git
    /// features stay off as without a repository, yet nothing can be told of its index.
    Refused,
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
    /// Whether the index holds the folder `path` (relative to the project folder), anything below it, or a submodule |
    /// link at a folder above it inside the project ([`tracked_entry`], other spellings on disk via [`disk_aliases`]):
    /// entries of every stage (a conflict), intent-to-add and skip-worktree ones too, before the first commit as well.
    /// Names compare without case when the repository is caseless (`core.ignorecase`).
    ///
    /// # Errors
    /// Git failed | timed out; a backend that does not read the index fails.
    fn tracked_under(&self, _location: &RepoLocation, _path: &str) -> Result<bool, GitError> {
        Err(GitError::Failed(
            "this backend does not read the index".to_owned(),
        ))
    }
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

/// Whether the index of the repository holds the folder `path` (relative to the project folder) or anything below it
/// ([`GitOps::tracked_under`]), as the reader other plugins use gives it.
///
/// # Errors
/// [`NoTracking`]: see [`tracked_question`]; git was too slow, or the call failed.
pub fn tracked_of(
    backend: &dyn GitOps,
    location: &RepoLocation,
    path: &str,
) -> Result<bool, NoTracking> {
    tracked_question(location, path)?;
    backend
        .tracked_under(location, path)
        .map_err(|error| match error {
            GitError::Timeout => NoTracking::Slow,
            _ => NoTracking::Failed,
        })
}

/// What is known of the tracked question about the folder `path` (relative to the project folder) before the index is
/// read: cheap, read from the folders as they are now.
///
/// # Errors
/// [`NoTracking::OtherRepository`]: the folder is inside another repository below this one (the index is not asked);
/// [`NoTracking::Failed`]: `path` is not plain names (`.`, `..`, empty names: both backends would read it differently).
pub fn tracked_question(location: &RepoLocation, path: &str) -> Result<(), NoTracking> {
    if path.contains('\0') || path.split('/').any(|name| matches!(name, "" | "." | "..")) {
        return Err(NoTracking::Failed);
    }
    // the trailing slash makes every name a folder: a repository at the folder itself holds what is below it too
    let folder = format!("{}/", repo_path(&location.prefix, path));
    if in_nested_repository(&location.workdir, &folder) {
        return Err(NoTracking::OtherRepository);
    }
    Ok(())
}

/// Whether the index entry `entry` (a path relative to the working tree) brings content into the folder `path`
/// (relative to the project folder `prefix`) at a checkout: the entry is the folder, below it, or at a folder above it
/// inside the project (a submodule | a link there delivers what is below). Whole names only (`a/b-c` is not below
/// `a/b`); the project folder itself is not inside the project. `caseless`: ASCII letters compare without case, as git
/// and libgit2 compare the names of a caseless repository.
///
/// The name that holds the folder (`.permark` of `<member>/.permark/local`) may also be another spelling that the
/// volume resolves to it (a Windows short name such as `PERMAR~1`, a Unicode case fold of a caseless volume) when the
/// folder's own name follows it, its ASCII letters in any case (the volume finds it so): a name [`alias_candidate`]
/// takes is put to `alias`, which says what the volume answers ([`disk_aliases`]); unknown counts as the same folder.
/// Every other name compares as written.
#[must_use]
pub fn tracked_entry(
    entry: &[u8],
    prefix: &str,
    path: &str,
    caseless: bool,
    alias: &mut dyn FnMut(&[u8]) -> SameFile,
) -> bool {
    let same = |held: &[u8], name: &str| {
        if caseless {
            held.eq_ignore_ascii_case(name.as_bytes())
        } else {
            held == name.as_bytes()
        }
    };
    let mut held = entry.split(|byte| *byte == b'/');
    for name in prefix.split('/').filter(|name| !name.is_empty()) {
        if !held.next().is_some_and(|part| same(part, name)) {
            return false;
        }
    }
    // the place of the holding name in `path` (the number of names above it) and the folder's own name
    let (holder, last) = path.rsplit_once('/').map_or((None, path), |(above, last)| {
        (Some(above.matches('/').count()), last)
    });
    let mut below = false;
    // one of the two is the other or starts it: every name both have agrees
    for (index, name) in path.split('/').enumerate() {
        let Some(part) = held.next() else {
            break;
        };
        below = true;
        if same(part, name) {
            continue;
        }
        // the holding name is the last but one: past it and the folder's name, every asked name agreed
        return Some(index) == holder
            && alias_candidate(part)
            && held
                .next()
                .is_some_and(|next| next.eq_ignore_ascii_case(last.as_bytes()))
            && alias(part) != SameFile::Other;
    }
    below
}

/// A name some volume may resolve to a folder of another name: a short name (`~`), a letter beyond ASCII (case folds
/// such as KELVIN SIGN to `k`), a last `.` | space (dropped by Windows). Ordinary ASCII names never are: they never cost
/// a look at the disk.
fn alias_candidate(name: &[u8]) -> bool {
    name.iter().any(|byte| *byte == b'~' || !byte.is_ascii())
        || matches!(name.last(), Some(b'.' | b' '))
}

/// Most folders | links [`disk_aliases`] puts to the volume for one question; past it a further one is not asked and
/// the answer is unknown (counted as the same folder): an index crafted with countless such names costs no more.
/// Files and missing names are not counted: they never hold a folder.
const MAX_ALIASES: usize = 64;

/// The `alias` of [`tracked_entry`] for the folder `path` (relative to the project folder of `location`): whether a
/// name beside the name that holds the folder is that same folder on disk ([`same_file`], links not followed; no file
/// ids on the volume (FAT, exFAT) -> the canonical paths compare). A file | nothing at the name | at the holding
/// folder, not one plain name -> another folder; a link -> where it leads; no UTF-8 | a look that fails otherwise ->
/// unknown. Each name is looked at once.
pub fn disk_aliases(location: &RepoLocation, path: &str) -> impl FnMut(&[u8]) -> SameFile {
    let mut base = location.workdir.clone();
    base.extend(location.prefix.split('/').filter(|name| !name.is_empty()));
    // a folder of one name has no holding name below the project folder: never asked about
    let holder = path.rsplit_once('/').map_or("", |(above, _)| above);
    let (above, name) = holder.rsplit_once('/').unwrap_or(("", holder));
    base.extend(above.split('/').filter(|name| !name.is_empty()));
    let folder = base.join(name);
    let mut seen: HashMap<Vec<u8>, SameFile> = HashMap::new();
    let mut looked = 0;
    move |name| {
        if let Some(answer) = seen.get(name) {
            return *answer;
        }
        let answer = volume_alias(&base, &folder, name, &mut looked);
        seen.insert(name.to_vec(), answer);
        answer
    }
}

/// [`disk_aliases`] of one name; `looked`: the folders | links put to the volume so far.
fn volume_alias(base: &Path, folder: &Path, name: &[u8], looked: &mut usize) -> SameFile {
    let Ok(name) = std::str::from_utf8(name) else {
        return SameFile::Unknown;
    };
    let mut parts = Path::new(name).components();
    if !matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return SameFile::Other;
    }
    let there = base.join(name);
    let kind = match std::fs::symlink_metadata(&there) {
        Ok(meta) => meta.file_type(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return SameFile::Other,
        Err(_) => return SameFile::Unknown,
    };
    if !kind.is_dir() && !kind.is_symlink() {
        return SameFile::Other;
    }
    match std::fs::symlink_metadata(folder) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return SameFile::Other,
        Err(_) => return SameFile::Unknown,
    }
    if *looked >= MAX_ALIASES {
        return SameFile::Unknown;
    }
    *looked += 1;
    // the ids of a link are its own: where it leads tells
    let answer = if kind.is_symlink() {
        SameFile::Unknown
    } else {
        same_file(&there, folder)
    };
    if answer != SameFile::Unknown {
        return answer;
    }
    let key = |path: &Path| {
        permark_core::paths::canonicalize(path)
            .ok()
            .and_then(|canonical| canonical.to_str().map(permark_core::paths::key_of))
    };
    match (key(&there), key(folder)) {
        (Some(there), Some(folder)) if there == folder => SameFile::Yes,
        (Some(_), Some(_)) => SameFile::Other,
        _ => SameFile::Unknown,
    }
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

/// Whether the git folder file `path` is there; a link counts as there and is never followed (its target may be on a
/// share, and even a look connects to the server).
fn marker(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// The state of the repository from the marker files in its git folder.
#[must_use]
pub fn repo_state(git_dir: &Path) -> RepoState {
    let has = |name: &str| marker(&git_dir.join(name));
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
    marker(&common_dir(git_dir).join("shallow"))
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

    /// A marker file that is a link is there as a link: its target is never looked up (it may be on a share, and even
    /// a look connects to the server). A link to nothing still marks the state.
    #[test]
    fn marker_links_are_never_followed() {
        use crate::worktrees::tests::link_folder;
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir_all(&git).unwrap();
        let gone = dir.path().join("gone");
        std::fs::create_dir_all(&gone).unwrap();
        if !link_folder(&gone, &git.join("shallow")) {
            eprintln!("links could not be made here; skipped");
            return;
        }
        assert!(link_folder(&gone, &git.join("MERGE_HEAD")));
        std::fs::remove_dir(&gone).unwrap();
        assert!(shallow(&git));
        assert_eq!(repo_state(&git), RepoState::Merging);
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
