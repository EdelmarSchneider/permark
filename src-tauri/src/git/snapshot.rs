//! Computing a snapshot: HEAD info, repository state, and changed files against a baseline.
//!
//! The baseline is either HEAD (via `statuses()`, which also gives staged/renamed/conflicted flags)
//! or the commit stored when the project was last closed (via `diff_tree_to_workdir_with_index`).
//! Both are limited to the vault folder with a literal pathspec and never write to the index.

use std::collections::{BTreeMap, BTreeSet};

use git2::{
    Delta, DiffFindOptions, DiffOptions, ErrorCode, Oid, Repository, RepositoryState, Status,
    StatusOptions, Tree,
};

use super::discover::GitContext;
use super::{BaselineKind, GitFile, GitState, HeadInfo, RepoState};

/// More changed files than this are cut off (`truncated`).
pub(crate) const MAX_FILES: usize = 5000;
/// Rename detection is skipped for very large diffs.
const MAX_DELTAS_FOR_RENAMES: usize = 2000;

pub(crate) struct Computed {
    pub head: HeadInfo,
    pub repo_state: RepoState,
    pub files: BTreeMap<String, GitFile>,
    pub truncated: bool,
    /// The commit the files were compared against.
    pub baseline_oid: Option<String>,
}

pub(crate) fn head_info(repo: &Repository) -> HeadInfo {
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
            // A repository without commits: the branch name lives in the symbolic HEAD.
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

pub(crate) fn repo_state(repo: &Repository) -> RepoState {
    match repo.state() {
        RepositoryState::Clean => RepoState::Clean,
        RepositoryState::Merge => RepoState::Merge,
        RepositoryState::Revert | RepositoryState::RevertSequence => RepoState::Revert,
        RepositoryState::CherryPick | RepositoryState::CherryPickSequence => RepoState::CherryPick,
        RepositoryState::Bisect => RepoState::Bisect,
        RepositoryState::Rebase
        | RepositoryState::RebaseInteractive
        | RepositoryState::RebaseMerge => RepoState::Rebase,
        RepositoryState::ApplyMailbox | RepositoryState::ApplyMailboxOrRebase => RepoState::Other,
    }
}

/// Maps a repository-relative path to a vault-relative one (`None` when outside the vault or ours).
fn to_vault_path(ctx: &GitContext, repo_path: &str) -> Option<String> {
    let p = repo_path.trim_end_matches('/');
    let rel = if ctx.vault_prefix.is_empty() {
        p
    } else {
        p.strip_prefix(&ctx.vault_prefix)?.strip_prefix('/')?
    };
    if rel.is_empty()
        || rel.starts_with(".permark/cache/")
        || rel.starts_with(".permark/local/")
        || rel.starts_with(".trailmark/cache/")
        || rel.starts_with(".trailmark/local/")
    {
        return None;
    }
    Some(rel.to_owned())
}

fn commit_tree(repo: &Repository, oid: Oid) -> Option<Tree<'_>> {
    repo.find_commit(oid).ok()?.tree().ok()
}

fn head_tree(repo: &Repository) -> Option<Tree<'_>> {
    repo.head().ok()?.peel_to_tree().ok()
}

/// What "since your last visit" is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionBaseline {
    /// The commit stored at the last close, or HEAD at the start of this session on a first visit.
    Commit(Oid),
    /// The session started in a repository without commits: everything committed since is new.
    Empty,
}

/// The stored commit when the repository still has it, otherwise the current HEAD, otherwise nothing.
pub(crate) fn pick_session_baseline(repo: &Repository, stored: Option<Oid>) -> SessionBaseline {
    if let Some(oid) = stored.filter(|o| repo.find_commit(*o).is_ok()) {
        return SessionBaseline::Commit(oid);
    }
    repo.head()
        .and_then(|h| h.peel_to_commit())
        .map_or(SessionBaseline::Empty, |c| SessionBaseline::Commit(c.id()))
}

/// Compares the working tree with the baseline.
///
/// With [`BaselineKind::LastVisit`] the files are measured against `session`; a commit that is missing
/// from the repository (a different clone, garbage collection) falls back to HEAD.
pub(crate) fn compute(
    repo: &Repository,
    ctx: &GitContext,
    kind: BaselineKind,
    session: SessionBaseline,
) -> Result<Computed, git2::Error> {
    let head = head_info(repo);
    let repo_state = repo_state(repo);

    let mut baseline_oid = head.oid.clone();
    let mut tree: Option<Tree<'_>> = head_tree(repo);
    if kind == BaselineKind::LastVisit {
        match session {
            SessionBaseline::Commit(oid) => {
                if let Some(t) = commit_tree(repo, oid) {
                    baseline_oid = Some(oid.to_string());
                    tree = Some(t);
                }
            }
            SessionBaseline::Empty => {
                baseline_oid = None;
                tree = None;
            }
        }
    }

    let (files, truncated) = match kind {
        BaselineKind::Head => status_files(repo, ctx)?,
        BaselineKind::LastVisit => diff_files(repo, ctx, tree.as_ref())?,
    };
    Ok(Computed {
        head,
        repo_state,
        files,
        truncated,
        baseline_oid,
    })
}

fn add_file(
    files: &mut BTreeMap<String, GitFile>,
    ctx: &GitContext,
    path: &str,
    file: &GitFile,
) -> bool {
    let Some(rel) = to_vault_path(ctx, path) else {
        return true;
    };
    if files.len() >= MAX_FILES {
        return false;
    }
    let from = file.from.as_deref().and_then(|f| to_vault_path(ctx, f));
    files.insert(
        rel,
        GitFile {
            state: file.state,
            staged: file.staged,
            from,
        },
    );
    true
}

fn status_files(
    repo: &Repository,
    ctx: &GitContext,
) -> Result<(BTreeMap<String, GitFile>, bool), git2::Error> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .exclude_submodules(true)
        .renames_head_to_index(true)
        .update_index(false)
        .disable_pathspec_match(true);
    if !ctx.vault_prefix.is_empty() {
        opts.pathspec(&ctx.vault_prefix);
    }
    let statuses = repo.statuses(Some(&mut opts))?;
    let mut files = BTreeMap::new();
    let mut truncated = false;
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
        let state = if flags.contains(Status::CONFLICTED) {
            GitState::Conflicted
        } else if flags.intersects(Status::INDEX_RENAMED | Status::WT_RENAMED) {
            GitState::Renamed
        } else if flags.intersects(Status::INDEX_DELETED | Status::WT_DELETED) {
            GitState::Deleted
        } else if flags.contains(Status::INDEX_NEW) {
            GitState::Added
        } else if flags.contains(Status::WT_NEW) {
            GitState::Untracked
        } else if flags.intersects(
            Status::INDEX_MODIFIED
                | Status::WT_MODIFIED
                | Status::INDEX_TYPECHANGE
                | Status::WT_TYPECHANGE,
        ) {
            GitState::Modified
        } else {
            continue;
        };
        // For renames `entry.path()` is the OLD path; the new one lives in the delta.
        let (path, from) = if state == GitState::Renamed {
            let delta = entry.head_to_index().or_else(|| entry.index_to_workdir());
            let new = delta
                .as_ref()
                .and_then(|d| d.new_file().path())
                .map(|p| p.to_string_lossy().into_owned());
            let old = delta
                .as_ref()
                .and_then(|d| d.old_file().path())
                .map(|p| p.to_string_lossy().into_owned());
            (new.unwrap_or_else(|| path.to_owned()), old)
        } else {
            (path.to_owned(), None)
        };
        if !add_file(
            &mut files,
            ctx,
            &path,
            &GitFile {
                state,
                staged,
                from,
            },
        ) {
            truncated = true;
            break;
        }
    }
    Ok((files, truncated))
}

fn diff_files(
    repo: &Repository,
    ctx: &GitContext,
    baseline: Option<&Tree<'_>>,
) -> Result<(BTreeMap<String, GitFile>, bool), git2::Error> {
    let new_opts = || {
        let mut o = DiffOptions::new();
        o.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_typechange(true)
            .include_ignored(false)
            .disable_pathspec_match(true);
        if !ctx.vault_prefix.is_empty() {
            o.pathspec(&ctx.vault_prefix);
        }
        o
    };
    let mut diff = repo.diff_tree_to_workdir_with_index(baseline, Some(&mut new_opts()))?;
    if diff.deltas().len() <= MAX_DELTAS_FOR_RENAMES {
        diff.find_similar(Some(DiffFindOptions::new().renames(true)))?;
    }

    // "Staged" means the index differs from HEAD for that path.
    let mut staged: BTreeSet<String> = BTreeSet::new();
    let head = head_tree(repo);
    let index_diff = repo.diff_tree_to_index(head.as_ref(), None, Some(&mut new_opts()))?;
    for d in index_diff.deltas() {
        if let Some(p) = d.new_file().path() {
            staged.insert(p.to_string_lossy().into_owned());
        }
    }

    let mut files = BTreeMap::new();
    let mut truncated = false;
    for delta in diff.deltas() {
        let state = match delta.status() {
            Delta::Added | Delta::Copied => GitState::Added,
            Delta::Untracked => GitState::Untracked,
            Delta::Deleted => GitState::Deleted,
            Delta::Modified | Delta::Typechange => GitState::Modified,
            Delta::Renamed => GitState::Renamed,
            Delta::Conflicted => GitState::Conflicted,
            Delta::Unmodified | Delta::Ignored | Delta::Unreadable => continue,
        };
        let path = if state == GitState::Deleted {
            delta.old_file().path()
        } else {
            delta.new_file().path()
        };
        let Some(path) = path.map(|p| p.to_string_lossy().into_owned()) else {
            continue;
        };
        let from = (state == GitState::Renamed)
            .then(|| {
                delta
                    .old_file()
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .flatten();
        let file = GitFile {
            state,
            staged: staged.contains(&path),
            from,
        };
        if !add_file(&mut files, ctx, &path, &file) {
            truncated = true;
            break;
        }
    }
    Ok((files, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::discover::{discover, Discovery};
    use crate::git::testutil::{commit_all, init_repo, stage, write};
    use std::path::Path;

    fn ctx_for(vault: &Path) -> GitContext {
        let vault = dunce::canonicalize(vault).expect("canonicalize");
        match discover(&vault, None) {
            Discovery::Repo(c) => c,
            other => panic!("expected a repo, got {other:?}"),
        }
    }

    fn states(c: &Computed) -> Vec<(String, GitState, bool)> {
        c.files
            .iter()
            .map(|(p, f)| (p.clone(), f.state, f.staged))
            .collect()
    }

    fn compute_head(repo: &Repository, ctx: &GitContext) -> Computed {
        compute(repo, ctx, BaselineKind::Head, SessionBaseline::Empty).expect("compute")
    }

    #[test]
    fn head_baseline_reports_modified_untracked_added_deleted() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        for f in ["keep.md", "edit.md", "gone.md"] {
            write(d.path(), f, "one\n");
        }
        commit_all(&repo, "init");
        write(d.path(), "edit.md", "two\n");
        std::fs::remove_file(d.path().join("gone.md")).expect("rm");
        write(d.path(), "new.md", "x\n");
        write(d.path(), "staged.md", "x\n");
        stage(&repo, "staged.md");

        let c = compute_head(&repo, &ctx_for(d.path()));
        assert_eq!(
            states(&c),
            [
                ("edit.md".to_owned(), GitState::Modified, false),
                ("gone.md".to_owned(), GitState::Deleted, false),
                ("new.md".to_owned(), GitState::Untracked, false),
                ("staged.md".to_owned(), GitState::Added, true),
            ]
        );
        assert!(!c.truncated);
        assert_eq!(c.head.branch.as_deref(), Some("main"));
        assert!(c.head.oid.is_some() && !c.head.detached && !c.head.unborn);
        assert_eq!(c.repo_state, RepoState::Clean);
        assert_eq!(c.baseline_oid, c.head.oid);
    }

    #[test]
    fn staged_rename_is_reported_with_its_origin() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(
            d.path(),
            "old.md",
            "some reasonably long content\nline two\nline three\n",
        );
        commit_all(&repo, "init");
        std::fs::rename(d.path().join("old.md"), d.path().join("new.md")).expect("mv");
        let mut index = repo.index().expect("index");
        index.remove_path(Path::new("old.md")).expect("rm");
        index.add_path(Path::new("new.md")).expect("add");
        index.write().expect("write");
        let c = compute_head(&repo, &ctx_for(d.path()));
        let f = c.files.get("new.md").expect("new.md");
        assert_eq!(f.state, GitState::Renamed);
        assert_eq!(f.from.as_deref(), Some("old.md"));
        assert!(f.staged);
    }

    #[test]
    fn last_visit_baseline_includes_commits_made_since() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "a.md", "one\n");
        let visited = commit_all(&repo, "visit");
        // The agent commits twice after the user's last visit.
        write(d.path(), "b.md", "new\n");
        write(d.path(), "a.md", "two\n");
        commit_all(&repo, "agent 1");
        write(d.path(), "c.md", "dirty\n");
        let ctx = ctx_for(d.path());

        let since = compute(
            &repo,
            &ctx,
            BaselineKind::LastVisit,
            SessionBaseline::Commit(visited),
        )
        .expect("compute");
        assert_eq!(
            states(&since),
            [
                ("a.md".to_owned(), GitState::Modified, false),
                ("b.md".to_owned(), GitState::Added, false),
                ("c.md".to_owned(), GitState::Untracked, false),
            ]
        );
        assert_eq!(since.baseline_oid, Some(visited.to_string()));

        // Against HEAD only the uncommitted file remains.
        let head = compute_head(&repo, &ctx);
        assert_eq!(
            states(&head),
            [("c.md".to_owned(), GitState::Untracked, false)]
        );
    }

    #[test]
    fn unknown_last_visit_commit_falls_back_to_head() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "a.md", "one\n");
        let head = commit_all(&repo, "init");
        write(d.path(), "b.md", "x\n");
        let bogus = Oid::from_str("1234567890123456789012345678901234567890").expect("oid");
        let c = compute(
            &repo,
            &ctx_for(d.path()),
            BaselineKind::LastVisit,
            SessionBaseline::Commit(bogus),
        )
        .expect("compute");
        assert_eq!(c.baseline_oid, Some(head.to_string()));
        assert_eq!(
            states(&c),
            [("b.md".to_owned(), GitState::Untracked, false)]
        );
    }

    #[test]
    fn unborn_head_reports_the_branch_and_untracked_files() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "a.md", "x\n");
        let ctx = ctx_for(d.path());
        for kind in [BaselineKind::Head, BaselineKind::LastVisit] {
            let c =
                compute(&repo, &ctx, kind, pick_session_baseline(&repo, None)).expect("compute");
            assert!(c.head.unborn);
            assert_eq!(c.head.branch.as_deref(), Some("main"));
            assert_eq!(c.head.oid, None);
            assert_eq!(c.baseline_oid, None);
            assert_eq!(
                states(&c),
                [("a.md".to_owned(), GitState::Untracked, false)]
            );
        }
    }

    #[test]
    fn a_session_started_before_the_first_commit_keeps_an_empty_baseline() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        let session = pick_session_baseline(&repo, None);
        assert_eq!(session, SessionBaseline::Empty);
        write(
            d.path(),
            "a.md",
            "x
",
        );
        commit_all(&repo, "agent");
        let ctx = ctx_for(d.path());
        let c = compute(&repo, &ctx, BaselineKind::LastVisit, session).expect("compute");
        // Committed since the session began, so it still counts as new.
        assert_eq!(states(&c), [("a.md".to_owned(), GitState::Added, false)]);
        assert_eq!(c.baseline_oid, None);
    }

    #[test]
    fn session_baseline_prefers_the_stored_commit_and_falls_back_to_head() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(
            d.path(),
            "a.md",
            "one
",
        );
        let first = commit_all(&repo, "first");
        write(
            d.path(),
            "a.md",
            "two
",
        );
        let second = commit_all(&repo, "second");
        assert_eq!(
            pick_session_baseline(&repo, Some(first)),
            SessionBaseline::Commit(first)
        );
        assert_eq!(
            pick_session_baseline(&repo, None),
            SessionBaseline::Commit(second)
        );
        let bogus = Oid::from_str("1234567890123456789012345678901234567890").expect("oid");
        assert_eq!(
            pick_session_baseline(&repo, Some(bogus)),
            SessionBaseline::Commit(second)
        );
    }

    #[test]
    fn detached_head_has_no_branch() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "a.md", "x\n");
        let oid = commit_all(&repo, "init");
        repo.set_head_detached(oid).expect("detach");
        let c = compute_head(&repo, &ctx_for(d.path()));
        assert!(c.head.detached);
        assert_eq!(c.head.branch, None);
        assert_eq!(c.head.oid, Some(oid.to_string()));
    }

    #[test]
    fn repository_states_are_mapped() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "a.md", "x\n");
        let oid = commit_all(&repo, "init");
        let ctx = ctx_for(d.path());
        let git_dir = &ctx.git_dir;
        let cases = [
            ("MERGE_HEAD", RepoState::Merge),
            ("CHERRY_PICK_HEAD", RepoState::CherryPick),
            ("REVERT_HEAD", RepoState::Revert),
            ("BISECT_LOG", RepoState::Bisect),
        ];
        for (marker, expected) in cases {
            std::fs::write(git_dir.join(marker), format!("{oid}\n")).expect("marker");
            assert_eq!(repo_state(&repo), expected, "{marker}");
            std::fs::remove_file(git_dir.join(marker)).expect("rm");
        }
        std::fs::create_dir(git_dir.join("rebase-merge")).expect("mkdir");
        std::fs::write(git_dir.join("rebase-merge").join("interactive"), "").expect("write");
        assert_eq!(repo_state(&repo), RepoState::Rebase);
    }

    #[test]
    fn vault_in_a_subfolder_only_sees_its_own_files_with_vault_relative_paths() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "docs/a.md", "one\n");
        write(d.path(), "src/x.rs", "one\n");
        commit_all(&repo, "init");
        write(d.path(), "docs/a.md", "two\n");
        write(d.path(), "docs/sub/new.md", "x\n");
        write(d.path(), "src/x.rs", "two\n");
        write(d.path(), "outside.md", "x\n");
        let ctx = ctx_for(&d.path().join("docs"));
        assert_eq!(ctx.vault_prefix, "docs");
        for kind in [BaselineKind::Head, BaselineKind::LastVisit] {
            let c =
                compute(&repo, &ctx, kind, pick_session_baseline(&repo, None)).expect("compute");
            assert_eq!(
                states(&c),
                [
                    ("a.md".to_owned(), GitState::Modified, false),
                    ("sub/new.md".to_owned(), GitState::Untracked, false),
                ],
                "{kind:?}"
            );
        }
    }

    #[test]
    fn folder_names_with_glob_characters_are_matched_literally() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "notes[1]/a.md", "one\n");
        write(d.path(), "notes1/b.md", "one\n");
        commit_all(&repo, "init");
        write(d.path(), "notes[1]/a.md", "two\n");
        write(d.path(), "notes1/b.md", "two\n");
        let ctx = ctx_for(&d.path().join("notes[1]"));
        for kind in [BaselineKind::Head, BaselineKind::LastVisit] {
            let c =
                compute(&repo, &ctx, kind, pick_session_baseline(&repo, None)).expect("compute");
            assert_eq!(
                states(&c),
                [("a.md".to_owned(), GitState::Modified, false)],
                "{kind:?}"
            );
        }
    }

    #[test]
    fn permark_private_folders_are_not_listed() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "a.md", "x\n");
        commit_all(&repo, "init");
        write(d.path(), ".permark/local/session.json", "{}");
        write(d.path(), ".permark/cache/index.bin", "x");
        write(d.path(), ".permark/config.toml", "x");
        let c = compute_head(&repo, &ctx_for(d.path()));
        assert_eq!(
            states(&c),
            [(
                ".permark/config.toml".to_owned(),
                GitState::Untracked,
                false
            )]
        );
    }

    #[test]
    fn gitignored_files_are_not_reported() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), ".gitignore", "*.log\n");
        write(d.path(), "a.md", "x\n");
        commit_all(&repo, "init");
        write(d.path(), "debug.log", "x");
        let c = compute_head(&repo, &ctx_for(d.path()));
        assert!(c.files.is_empty(), "{:?}", c.files);
    }

    #[test]
    fn conflicts_are_reported() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "a.md", "base\n");
        let base = commit_all(&repo, "base");
        write(d.path(), "a.md", "ours\n");
        let ours = commit_all(&repo, "ours");
        repo.set_head_detached(base).expect("detach");
        repo.reset(
            &repo.find_object(base, None).expect("obj"),
            git2::ResetType::Hard,
            None,
        )
        .expect("reset");
        write(d.path(), "a.md", "theirs\n");
        let theirs = commit_all(&repo, "theirs");
        repo.set_head_detached(ours).expect("detach");
        repo.reset(
            &repo.find_object(ours, None).expect("obj"),
            git2::ResetType::Hard,
            None,
        )
        .expect("reset");
        let their_commit = repo.find_annotated_commit(theirs).expect("annotated");
        repo.merge(&[&their_commit], None, None).expect("merge");
        let c = compute_head(&repo, &ctx_for(d.path()));
        assert_eq!(
            c.files.get("a.md").map(|f| f.state),
            Some(GitState::Conflicted)
        );
        assert_eq!(c.repo_state, RepoState::Merge);
    }

    #[test]
    fn path_mapping_rules() {
        let ctx = GitContext {
            workdir: "/w".into(),
            git_dir: "/w/.git".into(),
            common_dir: "/w/.git".into(),
            vault_prefix: "docs".to_owned(),
        };
        assert_eq!(to_vault_path(&ctx, "docs/a.md").as_deref(), Some("a.md"));
        assert_eq!(to_vault_path(&ctx, "docs/sub/").as_deref(), Some("sub"));
        assert_eq!(to_vault_path(&ctx, "docs"), None);
        assert_eq!(to_vault_path(&ctx, "docs2/a.md"), None);
        assert_eq!(to_vault_path(&ctx, "other/a.md"), None);
    }
}
