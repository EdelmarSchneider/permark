//! The built-in git (libgit2) as a backend: works where git is not installed. It reads only; it runs no hooks,
//! filters or helper programs. Each call opens the repository on its own (no handle crosses threads).

use std::path::Path;
use std::sync::Once;

use git2::{Delta, DiffOptions, ErrorCode, ObjectType, Oid, Repository, Status, StatusOptions};

use crate::ops::{
    below_prefix, finish, prefix_of, too_broad, Baseline, Discovery, GitError, GitOps, RawFile,
    RawStatus, RepoLocation,
};
use crate::types::{BackendKind, HeadInfo, Letter};

/// Most bytes of a committed file.
const MAX_BLOB: usize = 8 * 1024 * 1024;

static INIT: Once = Once::new();

/// One-time libgit2 setup. Owner validation is off: PerMark only reads, and libgit2's check makes repositories on
/// other drives or owned by another account look like "not a repository". Without it, a `.git` planted by another
/// local user would be trusted, so repositories at a drive root or in the home folder are refused (`too_broad`).
fn init() {
    INIT.call_once(disable_owner_validation);
}

// SAFETY: `set_verify_owner_validation` is `unsafe` only because it flips a process-wide libgit2 option. It runs
// once (guarded by `Once`) before PerMark opens a repository, and the option is never changed again.
#[allow(unsafe_code)]
fn disable_owner_validation() {
    // cannot fail in practice; nothing useful to do if it did
    let _ = unsafe { git2::opts::set_verify_owner_validation(false) };
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BuiltinGit;

fn open(location: &RepoLocation) -> Result<Repository, GitError> {
    init();
    // the git folder first; a gitfile's target without `core.worktree` (a hand-made submodule-like folder) reads as
    // bare there: then the working tree, which follows its `.git` file
    match Repository::open(&location.git_dir) {
        Ok(repo) if !repo.is_bare() => Ok(repo),
        first => Ok(Repository::open(&location.workdir).or(first)?),
    }
}

/// HEAD names a branch without a commit yet.
fn unborn(repo: &Repository) -> bool {
    matches!(repo.head(), Err(e) if matches!(e.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound))
}

fn head_info(repo: &Repository) -> HeadInfo {
    match repo.head() {
        Ok(head) => {
            let detached = repo.head_detached().unwrap_or(false);
            HeadInfo {
                branch: if detached {
                    None
                } else {
                    head.shorthand().ok().map(str::to_owned)
                },
                oid: head.peel_to_commit().ok().map(|c| c.id().to_string()),
                detached,
                unborn: false,
            }
        }
        Err(e) if matches!(e.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) => {
            // no commit yet: the branch name lives in the symbolic HEAD
            let branch = repo
                .find_reference("HEAD")
                .ok()
                .and_then(|r| r.symbolic_target().ok().flatten().map(str::to_owned))
                .map(|t| t.strip_prefix("refs/heads/").unwrap_or(&t).to_owned());
            HeadInfo {
                branch,
                oid: None,
                detached: false,
                unborn: true,
            }
        }
        Err(_) => HeadInfo::default(),
    }
}

fn raw_file(
    prefix: &str,
    path: &str,
    letter: Letter,
    staged: bool,
    from: Option<&str>,
) -> Option<RawFile> {
    Some(RawFile {
        path: below_prefix(prefix, path)?.to_owned(),
        letter,
        staged,
        from: from
            .and_then(|f| below_prefix(prefix, f))
            .map(str::to_owned),
    })
}

fn head_status(repo: &Repository, prefix: &str) -> Result<Vec<RawFile>, GitError> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .exclude_submodules(true)
        .renames_head_to_index(true)
        .update_index(false)
        .disable_pathspec_match(true);
    if !prefix.is_empty() {
        options.pathspec(prefix);
    }
    let statuses = repo.statuses(Some(&mut options))?;
    let mut files = Vec::new();
    for entry in statuses.iter() {
        let Ok(path) = entry.path() else { continue };
        let flags = entry.status();
        let staged = flags.intersects(
            Status::INDEX_NEW
                | Status::INDEX_MODIFIED
                | Status::INDEX_DELETED
                | Status::INDEX_RENAMED
                | Status::INDEX_TYPECHANGE,
        );
        let letter = if flags.contains(Status::CONFLICTED) {
            Letter::Conflict
        } else if flags.intersects(Status::INDEX_RENAMED | Status::WT_RENAMED) {
            Letter::Renamed
        } else if flags.intersects(Status::INDEX_DELETED | Status::WT_DELETED) {
            Letter::Deleted
        } else if flags.contains(Status::INDEX_NEW) {
            Letter::Added
        } else if flags.contains(Status::WT_NEW) {
            Letter::Untracked
        } else if flags.intersects(
            Status::INDEX_MODIFIED
                | Status::WT_MODIFIED
                | Status::INDEX_TYPECHANGE
                | Status::WT_TYPECHANGE,
        ) {
            Letter::Modified
        } else {
            continue;
        };
        // for a rename `entry.path()` is the old path; the new one is in the delta
        let (path, from) = if letter == Letter::Renamed {
            let delta = entry.head_to_index().or_else(|| entry.index_to_workdir());
            let name =
                |file: git2::DiffFile<'_>| file.path().map(|p| p.to_string_lossy().into_owned());
            (
                delta
                    .as_ref()
                    .and_then(|d| name(d.new_file()))
                    .unwrap_or_else(|| path.to_owned()),
                delta.as_ref().and_then(|d| name(d.old_file())),
            )
        } else {
            (path.to_owned(), None)
        };
        files.extend(raw_file(prefix, &path, letter, staged, from.as_deref()));
    }
    Ok(files)
}

fn commit_status(repo: &Repository, prefix: &str, oid: &str) -> Result<Vec<RawFile>, GitError> {
    let commit = repo
        .revparse_single(&format!("{oid}^{{commit}}"))?
        .peel_to_commit()?;
    let tree = commit.tree()?;
    let mut options = DiffOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_typechange(true)
        .include_ignored(false)
        .disable_pathspec_match(true);
    if !prefix.is_empty() {
        options.pathspec(prefix);
    }
    let diff = repo.diff_tree_to_workdir_with_index(Some(&tree), Some(&mut options))?;
    let mut files = Vec::new();
    for delta in diff.deltas() {
        let letter = match delta.status() {
            Delta::Added | Delta::Copied => Letter::Added,
            Delta::Untracked => Letter::Untracked,
            Delta::Deleted => Letter::Deleted,
            Delta::Modified | Delta::Typechange => Letter::Modified,
            Delta::Renamed => Letter::Renamed,
            Delta::Conflicted => Letter::Conflict,
            Delta::Unmodified | Delta::Ignored | Delta::Unreadable => continue,
        };
        let file = if letter == Letter::Deleted {
            delta.old_file()
        } else {
            delta.new_file()
        };
        let Some(path) = file.path().map(|p| p.to_string_lossy().into_owned()) else {
            continue;
        };
        files.extend(raw_file(prefix, &path, letter, false, None));
    }
    Ok(files)
}

impl GitOps for BuiltinGit {
    fn kind(&self) -> BackendKind {
        BackendKind::Builtin
    }

    fn discover(&self, root: &Path, home: Option<&Path>) -> Discovery {
        init();
        let repo = match Repository::discover(root) {
            Ok(repo) => repo,
            Err(e) if e.code() == ErrorCode::NotFound => return Discovery::NoRepo,
            Err(e) => return Discovery::Error(e.message().to_owned()),
        };
        // a bare repository has no working tree
        let Some(workdir) = repo.workdir().map(dunce::simplified).map(Path::to_path_buf) else {
            return Discovery::NoRepo;
        };
        if too_broad(&workdir, root, home) {
            return Discovery::NoRepo;
        }
        let Some(prefix) = prefix_of(&workdir, root) else {
            return Discovery::NoRepo;
        };
        if !prefix.is_empty() && repo.is_path_ignored(&prefix).unwrap_or(false) {
            return Discovery::NoRepo;
        }
        Discovery::Repo(RepoLocation {
            workdir: dunce::canonicalize(&workdir).unwrap_or(workdir),
            git_dir: dunce::canonicalize(repo.path()).unwrap_or_else(|_| repo.path().to_path_buf()),
            prefix,
        })
    }

    fn status(&self, location: &RepoLocation, baseline: &Baseline) -> Result<RawStatus, GitError> {
        let repo = open(location)?;
        let head = head_info(&repo);
        let files = match baseline {
            Baseline::Head => head_status(&repo, &location.prefix)?,
            Baseline::Commit(oid) => commit_status(&repo, &location.prefix, oid)?,
        };
        let (files, truncated) = finish(files);
        Ok(RawStatus {
            head,
            files,
            truncated,
        })
    }

    fn blob_at_head(
        &self,
        location: &RepoLocation,
        path: &str,
    ) -> Result<Option<Vec<u8>>, GitError> {
        if path.contains(' ') {
            return Ok(None);
        }
        let repo = open(location)?;
        let full = if location.prefix.is_empty() {
            path.to_owned()
        } else {
            format!("{}/{path}", location.prefix)
        };
        let tree = match repo.head().and_then(|head| head.peel_to_tree()) {
            Ok(tree) => tree,
            Err(e) if matches!(e.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) => {
                return Ok(None)
            }
            Err(e) => return Err(e.into()),
        };
        let entry = match tree.get_path(Path::new(&full)) {
            Ok(entry) => entry,
            Err(e) if e.code() == ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if entry.kind() != Some(ObjectType::Blob) {
            return Ok(None);
        }
        let blob = repo.find_blob(entry.id())?;
        if blob.size() > MAX_BLOB {
            return Err(GitError::TooLarge(MAX_BLOB));
        }
        Ok(Some(blob.content().to_vec()))
    }

    fn commit_count(&self, location: &RepoLocation, old: &str, new: &str) -> Option<u32> {
        let repo = open(location).ok()?;
        let (old, new) = (Oid::from_str(old).ok()?, Oid::from_str(new).ok()?);
        let mut walk = repo.revwalk().ok()?;
        walk.push(new).ok()?;
        walk.hide(old).ok()?;
        u32::try_from(walk.flatten().count()).ok()
    }

    fn commit_exists(&self, location: &RepoLocation, oid: &str) -> bool {
        let Ok(repo) = open(location) else {
            return false;
        };
        Oid::from_str(oid).is_ok() && repo.revparse_single(&format!("{oid}^{{commit}}")).is_ok()
    }

    fn ahead_behind(&self, location: &RepoLocation) -> Result<Option<(u32, u32)>, GitError> {
        let repo = open(location)?;
        if unborn(&repo) {
            return Ok(None);
        }
        let Ok(head) = repo.head() else {
            return Ok(None);
        };
        if !head.is_branch() {
            return Ok(None);
        }
        let (Some(local), Ok(upstream)) = (head.target(), git2::Branch::wrap(head).upstream())
        else {
            return Ok(None);
        };
        let Some(remote) = upstream.get().target() else {
            return Ok(None);
        };
        let (ahead, behind) = repo.graph_ahead_behind(local, remote)?;
        Ok(Some((
            u32::try_from(ahead).unwrap_or(u32::MAX),
            u32::try_from(behind).unwrap_or(u32::MAX),
        )))
    }

    fn last_commit(&self, location: &RepoLocation) -> Result<Option<(i64, String)>, GitError> {
        let repo = open(location)?;
        if unborn(&repo) {
            return Ok(None);
        }
        let commit = match repo.head().and_then(|head| head.peel_to_commit()) {
            Ok(commit) => commit,
            Err(e) if matches!(e.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) => {
                return Ok(None)
            }
            Err(e) => return Err(e.into()),
        };
        let message = String::from_utf8_lossy(commit.message_bytes()).into_owned();
        let seconds = commit.committer().when().seconds();
        Ok(Some((
            seconds.saturating_mul(1000),
            crate::ops::subject_of(&message),
        )))
    }

    fn root_commit(&self, location: &RepoLocation) -> Result<Option<String>, GitError> {
        let repo = open(location)?;
        if unborn(&repo) {
            return Ok(None);
        }
        let mut walk = repo.revwalk()?;
        match walk.push_head() {
            Ok(()) => {}
            Err(e) if matches!(e.code(), ErrorCode::UnbornBranch | ErrorCode::NotFound) => {
                return Ok(None)
            }
            Err(e) => return Err(e.into()),
        }
        let mut root: Option<Oid> = None;
        for oid in walk {
            let oid = oid?;
            if repo.find_commit(oid)?.parent_count() == 0 && root.is_none_or(|r| oid < r) {
                root = Some(oid);
            }
        }
        Ok(root.map(|oid| oid.to_string()))
    }
}
