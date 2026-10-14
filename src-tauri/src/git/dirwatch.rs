//! Watching the git directory itself, so commits, checkouts and index changes are noticed even when
//! `.git` lies outside the vault (vault in a subfolder, or inside a linked worktree).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::discover::GitContext;
use crate::vault::strip_root;

/// Does a change at `path` (inside the git dir or the shared common dir) possibly alter the status?
///
/// Interesting: `HEAD`, `index`, `packed-refs`, merge/cherry-pick/revert/bisect markers, rebase
/// folders, `logs/HEAD` (the best single trigger for commits and checkouts), anything under `refs/`
/// and `info/exclude`. Ignored: lock files and everything else (notably `objects/`).
pub(crate) fn is_git_trigger(git_dir: &Path, common_dir: &Path, path: &Path) -> bool {
    let path = dunce::simplified(path);
    let Some(parts) = strip_root(path, git_dir).or_else(|| strip_root(path, common_dir)) else {
        return false;
    };
    let parts: Vec<String> = parts
        .iter()
        .map(|p| p.to_string_lossy().to_ascii_lowercase())
        .collect();
    let Some(first) = parts.first() else {
        return false;
    };
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("lock"))
    {
        return false;
    }
    match first.as_str() {
        "head" | "index" | "packed-refs" | "merge_head" | "cherry_pick_head" | "revert_head"
        | "bisect_log" | "rebase-merge" | "rebase-apply" | "refs" => true,
        "logs" => parts.len() == 2 && parts[1] == "head",
        "info" => parts.len() == 2 && parts[1] == "exclude",
        _ => false,
    }
}

/// Keeps the OS watchers alive; dropping it stops them.
pub(crate) struct GitDirWatcher {
    _watcher: RecommendedWatcher,
}

impl GitDirWatcher {
    /// Calls `on_trigger` whenever something interesting changes. `None` when watching is impossible.
    pub(crate) fn start(ctx: &GitContext, on_trigger: Arc<dyn Fn() + Send + Sync>) -> Option<Self> {
        let (git_dir, common_dir) = (ctx.git_dir.clone(), ctx.common_dir.clone());
        let (g, c) = (git_dir.clone(), common_dir.clone());
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            if matches!(ev.kind, EventKind::Access(_)) {
                return;
            }
            if ev.paths.iter().any(|p| is_git_trigger(&g, &c, p)) {
                on_trigger();
            }
        })
        .ok()?;

        let mut targets: Vec<(PathBuf, RecursiveMode)> = vec![
            (git_dir.clone(), RecursiveMode::NonRecursive),
            (git_dir.join("logs"), RecursiveMode::NonRecursive),
            (common_dir.join("refs"), RecursiveMode::Recursive),
            (common_dir.join("info"), RecursiveMode::NonRecursive),
        ];
        if common_dir != git_dir {
            targets.push((common_dir, RecursiveMode::NonRecursive));
        }
        let mut watching = 0;
        for (path, mode) in targets {
            if path.is_dir() && watcher.watch(&path, mode).is_ok() {
                watching += 1;
            }
        }
        (watching > 0).then_some(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::discover::{discover, Discovery};
    use crate::git::testutil::{commit_all, init_repo, write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    fn g() -> (PathBuf, PathBuf) {
        (PathBuf::from("/r/.git"), PathBuf::from("/r/.git"))
    }

    #[test]
    fn interesting_git_paths_trigger() {
        let (git, common) = g();
        for rel in [
            "HEAD",
            "index",
            "packed-refs",
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "BISECT_LOG",
            "rebase-merge",
            "rebase-apply",
            "refs/heads/main",
            "refs/remotes/origin/main",
            "logs/HEAD",
            "info/exclude",
        ] {
            assert!(is_git_trigger(&git, &common, &git.join(rel)), "{rel}");
        }
    }

    #[test]
    fn noise_does_not_trigger() {
        let (git, common) = g();
        for rel in [
            "index.lock",
            "HEAD.lock",
            "refs/heads/main.lock",
            "objects/ab/cdef",
            "objects/pack/pack-1.idx",
            "logs/refs/heads/main",
            "COMMIT_EDITMSG",
            "FETCH_HEAD",
            "config",
            "worktrees/x/HEAD",
            "hooks/pre-commit",
        ] {
            assert!(!is_git_trigger(&git, &common, &git.join(rel)), "{rel}");
        }
        assert!(!is_git_trigger(&git, &common, Path::new("/elsewhere/HEAD")));
        assert!(!is_git_trigger(&git, &common, &git));
    }

    #[test]
    fn worktree_git_dir_and_common_dir_both_count() {
        let git = PathBuf::from("/r/.git/worktrees/wt");
        let common = PathBuf::from("/r/.git");
        assert!(is_git_trigger(&git, &common, &git.join("HEAD")));
        assert!(is_git_trigger(&git, &common, &git.join("index")));
        assert!(is_git_trigger(&git, &common, &common.join("packed-refs")));
        assert!(is_git_trigger(
            &git,
            &common,
            &common.join("refs/heads/feature")
        ));
        assert!(!is_git_trigger(
            &git,
            &common,
            &common.join("worktrees/other/HEAD")
        ));
    }

    #[test]
    fn real_commits_trigger_the_watcher_but_object_writes_alone_do_not() {
        let d = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(d.path());
        write(d.path(), "a.md", "one\n");
        commit_all(&repo, "init");
        let vault = dunce::canonicalize(d.path()).expect("canonicalize");
        let Discovery::Repo(ctx) = discover(&vault, None) else {
            panic!("expected a repo");
        };
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        let _w = GitDirWatcher::start(
            &ctx,
            Arc::new(move || {
                h.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .expect("watcher");

        write(d.path(), "a.md", "two\n");
        commit_all(&repo, "second");
        let deadline = Instant::now() + Duration::from_secs(10);
        while hits.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            hits.load(Ordering::SeqCst) > 0,
            "a commit must wake the watcher"
        );
    }
}
