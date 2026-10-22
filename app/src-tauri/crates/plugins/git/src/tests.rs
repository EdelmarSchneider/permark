//! Spec + adversarial tests. Fixture repositories are built with libgit2 in temp folders (Unicode and a space in the
//! path); the same cases run against the built-in backend and, when a `git` executable exists, the system one.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use git2::{IndexAddOption, Repository, Signature};
use permark_core::conditions::{Distribution, GitCondition, RepoState};
use permark_core::events::NoEvents;
use permark_core::paths::Root;
use permark_core::plugins::{PluginId, PluginRegistry, ProjectFacts, RpcCall};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};

use super::*;

// ---- fixtures ------------------------------------------------------------------------------------------------------

struct Fixture {
    dir: tempfile::TempDir,
    root: PathBuf,
    repo: Repository,
}

impl Fixture {
    /// A repository in `pro ü` (no commit yet).
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap().join("pro ü");
        std::fs::create_dir_all(&root).unwrap();
        let repo = Repository::init(&root).unwrap();
        Self { dir, root, repo }
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn remove(&self, rel: &str) {
        std::fs::remove_file(self.root.join(rel)).unwrap();
    }

    /// Stages everything (new, changed, deleted) and commits; returns the commit id.
    fn commit(&self, message: &str) -> String {
        let mut index = self.repo.index().unwrap();
        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .unwrap();
        index.update_all(["*"].iter(), None).unwrap();
        index.write().unwrap();
        let tree = self.repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("Tester", "tester@example.com").unwrap();
        let parent = self.repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
        self.repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .unwrap()
            .to_string()
    }

    /// Stages one path (new, changed or deleted).
    fn stage(&self, rel: &str) {
        let mut index = self.repo.index().unwrap();
        if self.root.join(rel).exists() {
            index.add_path(Path::new(rel)).unwrap();
        } else {
            index.remove_path(Path::new(rel)).unwrap();
        }
        index.write().unwrap();
    }

    fn head_ref(&self) -> String {
        self.repo.head().unwrap().name().unwrap().to_owned()
    }
}

fn backends() -> Vec<(&'static str, Arc<dyn GitOps>)> {
    let mut list: Vec<(&'static str, Arc<dyn GitOps>)> = vec![("builtin", Arc::new(BuiltinGit))];
    if let Some(system) = SystemGit::find() {
        list.push(("system", Arc::new(system)));
    } else {
        eprintln!("SKIPPED: no git executable on PATH, the System backend cases are not run");
    }
    list
}

fn location(ops: &dyn GitOps, root: &Path) -> RepoLocation {
    match ops.discover(root, None) {
        Discovery::Repo(location) => location,
        other => panic!("no repository at {}: {other:?}", root.display()),
    }
}

fn letters(status: &RawStatus) -> Vec<(String, Letter, bool)> {
    status
        .files
        .iter()
        .map(|f| (f.path.clone(), f.letter, f.staged))
        .collect()
}

fn each(test: impl Fn(&str, &dyn GitOps)) {
    for (name, ops) in backends() {
        test(name, ops.as_ref());
    }
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ---- discovery -----------------------------------------------------------------------------------------------------

#[test]
fn discovery_tells_plain_folders_from_repositories_and_finds_the_prefix() {
    each(|name, ops| {
        let plain = tempfile::tempdir().unwrap();
        assert_eq!(
            ops.discover(plain.path(), None),
            Discovery::NoRepo,
            "{name}"
        );

        let fx = Fixture::new();
        fx.write("docs/deep/a.md", "a\n");
        fx.commit("first");
        let at_root = location(ops, &fx.root);
        assert_eq!(at_root.prefix, "", "{name}");
        assert_eq!(at_root.workdir, fx.root, "{name}");
        assert!(
            at_root.git_dir.ends_with(".git"),
            "{name}: {:?}",
            at_root.git_dir
        );
        let below = location(ops, &fx.root.join("docs").join("deep"));
        assert_eq!(below.prefix, "docs/deep", "{name}");
        assert_eq!(below.workdir, fx.root, "{name}");
    });
}

#[test]
fn a_nested_repository_is_the_repository_of_its_folder() {
    each(|name, ops| {
        let outer = Fixture::new();
        outer.write("a.md", "a\n");
        outer.commit("outer");
        let inner_root = outer.root.join("inner");
        std::fs::create_dir_all(&inner_root).unwrap();
        let _inner = Repository::init(&inner_root).unwrap();
        let found = location(ops, &inner_root);
        assert_eq!(found.workdir, inner_root, "{name}");
        assert_eq!(found.prefix, "", "{name}");
    });
}

#[test]
fn a_repository_that_is_too_broad_or_ignores_the_project_is_no_repository() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.write("proj/a.md", "a\n");
        fx.write("skipped/b.md", "b\n");
        fx.write(".gitignore", "skipped/\n");
        fx.commit("first");
        // the repository is the person's home: a folder below it does not bind to it ...
        assert_eq!(
            ops.discover(&fx.root.join("proj"), Some(&fx.root)),
            Discovery::NoRepo,
            "{name}: home"
        );
        // ... unless the project is the home folder itself
        assert!(
            matches!(ops.discover(&fx.root, Some(&fx.root)), Discovery::Repo(_)),
            "{name}"
        );
        // the repository ignores the project folder
        assert_eq!(
            ops.discover(&fx.root.join("skipped"), None),
            Discovery::NoRepo,
            "{name}: ignored"
        );
        assert!(
            matches!(
                ops.discover(&fx.root.join("proj"), None),
                Discovery::Repo(_)
            ),
            "{name}"
        );
    });
}

// ---- status --------------------------------------------------------------------------------------------------------

#[test]
fn status_letters_for_every_kind_of_change() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.write("kept.md", "kept\n");
        fx.write("edited.md", "one\n");
        fx.write("gone.md", "gone\n");
        fx.write(
            "renamed from.md",
            "a long enough text to be recognized as the same file\nsecond line\nthird line\n",
        );
        fx.write("ñ ü/日本.md", "x\n");
        fx.commit("first");
        // a new size: an edit of the same size inside the timestamp tick of the commit is "racily clean" to git (the
        // index's size + time still match), a git limit, not what this test is about
        fx.write("edited.md", "two, longer\n");
        fx.remove("gone.md");
        fx.write("added.md", "new\n");
        fx.stage("added.md");
        fx.write("untracked ü.md", "u\n");
        fx.write(".gitignore", "ignored.md\n");
        fx.write("ignored.md", "i\n");
        std::fs::rename(
            fx.root.join("renamed from.md"),
            fx.root.join("renamed to.md"),
        )
        .unwrap();
        fx.stage("renamed from.md");
        fx.stage("renamed to.md");
        fx.write("ñ ü/日本.md", "changed\n");

        let status = ops
            .status(&location(ops, &fx.root), &Baseline::Head)
            .unwrap();
        assert_eq!(
            letters(&status),
            vec![
                (".gitignore".to_owned(), Letter::Untracked, false),
                ("added.md".to_owned(), Letter::Added, true),
                ("edited.md".to_owned(), Letter::Modified, false),
                ("gone.md".to_owned(), Letter::Deleted, false),
                ("renamed to.md".to_owned(), Letter::Renamed, true),
                ("untracked ü.md".to_owned(), Letter::Untracked, false),
                ("ñ ü/日本.md".to_owned(), Letter::Modified, false),
            ],
            "{name}"
        );
        let renamed = status
            .files
            .iter()
            .find(|f| f.letter == Letter::Renamed)
            .unwrap();
        assert_eq!(renamed.from.as_deref(), Some("renamed from.md"), "{name}");
        assert!(!status.truncated, "{name}");
        assert!(
            status.head.oid.is_some() && status.head.branch.is_some(),
            "{name}"
        );
    });
}

#[test]
fn a_clean_repository_has_no_files() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.write("a.md", "a\n");
        fx.commit("first");
        let status = ops
            .status(&location(ops, &fx.root), &Baseline::Head)
            .unwrap();
        assert!(status.files.is_empty(), "{name}");
    });
}

#[test]
fn heads_without_a_commit_and_detached_heads() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.write("a.md", "a\n");
        let unborn = ops
            .status(&location(ops, &fx.root), &Baseline::Head)
            .unwrap();
        assert!(unborn.head.unborn, "{name}");
        assert_eq!(unborn.head.oid, None, "{name}");
        assert!(
            unborn.head.branch.is_some(),
            "{name}: the branch has a name"
        );
        assert_eq!(
            letters(&unborn),
            vec![("a.md".to_owned(), Letter::Untracked, false)],
            "{name}"
        );

        let oid = fx.commit("first");
        let named = ops
            .status(&location(ops, &fx.root), &Baseline::Head)
            .unwrap();
        assert!(!named.head.unborn && !named.head.detached, "{name}");
        assert_eq!(named.head.oid.as_deref(), Some(oid.as_str()), "{name}");
        fx.repo
            .set_head_detached(git2::Oid::from_str(&oid).unwrap())
            .unwrap();
        let detached = ops
            .status(&location(ops, &fx.root), &Baseline::Head)
            .unwrap();
        assert!(detached.head.detached, "{name}");
        assert_eq!(detached.head.branch, None, "{name}");
        assert_eq!(detached.head.oid.as_deref(), Some(oid.as_str()), "{name}");
    });
}

#[test]
fn a_project_below_the_repository_sees_only_its_own_files_with_relative_paths() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.write("proj/a.md", "a\n");
        fx.write("proj/sub dir/b.md", "b\n");
        fx.write("other/c.md", "c\n");
        fx.write("proj-twin/d.md", "d\n");
        fx.commit("first");
        fx.write("proj/a.md", "changed\n");
        fx.write("proj/sub dir/new.md", "new\n");
        fx.write("other/c.md", "changed\n");
        fx.write("proj-twin/d.md", "changed\n");
        let status = ops
            .status(&location(ops, &fx.root.join("proj")), &Baseline::Head)
            .unwrap();
        assert_eq!(
            letters(&status),
            vec![
                ("a.md".to_owned(), Letter::Modified, false),
                ("sub dir/new.md".to_owned(), Letter::Untracked, false),
            ],
            "{name}"
        );
    });
}

#[test]
fn conflicts_and_the_merge_state() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.write("a.md", "base\n");
        let base = fx.commit("base");
        let main_ref = fx.head_ref();
        let base_commit = fx
            .repo
            .find_commit(git2::Oid::from_str(&base).unwrap())
            .unwrap();
        fx.repo.branch("other", &base_commit, false).unwrap();
        fx.repo.set_head("refs/heads/other").unwrap();
        fx.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        fx.write("a.md", "from other\n");
        let other = fx.commit("other");
        fx.repo.set_head(&main_ref).unwrap();
        fx.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        fx.write("a.md", "from main\n");
        fx.commit("main");
        let annotated = fx
            .repo
            .find_annotated_commit(git2::Oid::from_str(&other).unwrap())
            .unwrap();
        fx.repo.merge(&[&annotated], None, None).unwrap();

        let loc = location(ops, &fx.root);
        let status = ops.status(&loc, &Baseline::Head).unwrap();
        assert_eq!(
            letters(&status).first().map(|(p, l, _)| (p.as_str(), *l)),
            Some(("a.md", Letter::Conflict)),
            "{name}: {:?}",
            status.files
        );
        assert_eq!(repo_state_of(&loc), RepoState::Merging, "{name}");
    });
}

fn repo_state_of(location: &RepoLocation) -> RepoState {
    ops::repo_state(&location.git_dir)
}

#[test]
fn the_commit_baseline_sees_later_commits_and_untracked_files() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.write("a.md", "a\n");
        fx.write("b.md", "b\n");
        let first = fx.commit("first");
        fx.write("a.md", "a changed\n");
        fx.write("new.md", "n\n");
        fx.remove("b.md");
        fx.commit("second");
        fx.write("late ü.md", "l\n");
        fx.write("a.md", "a changed again\n");
        let loc = location(ops, &fx.root);
        let status = ops.status(&loc, &Baseline::Commit(first.clone())).unwrap();
        assert_eq!(
            letters(&status),
            vec![
                ("a.md".to_owned(), Letter::Modified, false),
                ("b.md".to_owned(), Letter::Deleted, false),
                ("late ü.md".to_owned(), Letter::Untracked, false),
                ("new.md".to_owned(), Letter::Added, false),
            ],
            "{name}"
        );
        assert!(!status.head.unborn, "{name}");
        // against HEAD only the last edits show
        let head = ops.status(&loc, &Baseline::Head).unwrap();
        assert_eq!(
            letters(&head),
            vec![
                ("a.md".to_owned(), Letter::Modified, false),
                ("late ü.md".to_owned(), Letter::Untracked, false),
            ],
            "{name}"
        );
        assert!(ops.commit_exists(&loc, &first), "{name}");
        assert!(!ops.commit_exists(&loc, &"0".repeat(40)), "{name}");
        for bad in ["", "--upload-pack=x", "HEAD", "zz", "abc"] {
            assert!(!ops.commit_exists(&loc, bad), "{name}: {bad:?}");
        }
        assert!(
            ops.status(&loc, &Baseline::Commit("--output=x".to_owned()))
                .is_err()
                || name == "builtin",
            "{name}"
        );
    });
}

#[test]
fn blobs_at_head_come_back_as_committed() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.write("docs/a.md", "committed\n");
        fx.write("big.bin", &"x".repeat(100_000));
        let loc_before = location(ops, &fx.root);
        assert_eq!(
            ops.blob_at_head(&loc_before, "docs/a.md").unwrap(),
            None,
            "{name}: unborn"
        );
        fx.commit("first");
        fx.write("docs/a.md", "edited after\n");
        let loc = location(ops, &fx.root);
        assert_eq!(
            ops.blob_at_head(&loc, "docs/a.md").unwrap().as_deref(),
            Some(b"committed\n".as_slice()),
            "{name}"
        );
        assert_eq!(
            ops.blob_at_head(&loc, "missing.md").unwrap(),
            None,
            "{name}"
        );
        assert_eq!(
            ops.blob_at_head(&loc, "docs").unwrap(),
            None,
            "{name}: a folder is no blob"
        );
        assert_eq!(ops.blob_at_head(&loc, "a\0b").unwrap(), None, "{name}");
        assert_eq!(
            ops.blob_at_head(&loc, "big.bin").unwrap().map(|b| b.len()),
            Some(100_000),
            "{name}"
        );
        // a project below the repository asks with project paths
        let below = location(ops, &fx.root.join("docs"));
        assert_eq!(
            ops.blob_at_head(&below, "a.md").unwrap().as_deref(),
            Some(b"committed\n".as_slice()),
            "{name}"
        );
    });
}

#[test]
fn both_backends_answer_alike() {
    let list = backends();
    if list.len() < 2 {
        return;
    }
    let fx = Fixture::new();
    fx.write("a.md", "a\n");
    fx.write("b/c.md", "c\n");
    fx.write("d.md", "to delete\n");
    let first = fx.commit("first");
    fx.write("a.md", "edited\n");
    fx.write("e.md", "new\n");
    fx.stage("e.md");
    fx.remove("d.md");
    fx.write("b/untracked.md", "u\n");
    fx.commit("second");
    fx.write("b/c.md", "edited\n");
    fx.write("more ü.md", "m\n");
    let mut seen: Vec<(String, RawStatus, RawStatus)> = Vec::new();
    for (name, ops) in &list {
        let loc = location(ops.as_ref(), &fx.root);
        seen.push((
            (*name).to_owned(),
            ops.status(&loc, &Baseline::Head).unwrap(),
            ops.status(&loc, &Baseline::Commit(first.clone())).unwrap(),
        ));
    }
    let (a, b) = (&seen[0], &seen[1]);
    assert_eq!(a.1, b.1, "{} vs {}: Head", a.0, b.0);
    assert_eq!(a.2, b.2, "{} vs {}: Commit", a.0, b.0);
}

// ---- the system backend's limits ------------------------------------------------------------------------------------

#[test]
fn a_slow_git_is_killed_and_a_flood_of_output_is_cut() {
    let Some(exe) = find_git() else {
        eprintln!("SKIPPED: no git executable on PATH");
        return;
    };
    let here = std::env::current_dir().unwrap();
    let started = Instant::now();
    let slow = system::run(
        &exe,
        &here,
        &[
            OsStr::new("-c"),
            OsStr::new("alias.slow=!sleep 5"),
            OsStr::new("slow"),
        ],
        Duration::from_millis(300),
        1024,
    );
    assert_eq!(slow.unwrap_err(), GitError::Timeout);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "killed, not waited for"
    );

    let fx = Fixture::new();
    fx.write("big.txt", &"y".repeat(50_000));
    fx.commit("big");
    let flood = system::run(
        &exe,
        &fx.root,
        &[
            OsStr::new("cat-file"),
            OsStr::new("blob"),
            OsStr::new("HEAD:big.txt"),
        ],
        Duration::from_secs(10),
        1000,
    );
    assert_eq!(flood.unwrap_err(), GitError::TooLarge(1000));
    // a program that is not there
    let missing = system::run(
        &fx.root.join("no-such-git"),
        &fx.root,
        &[OsStr::new("--version")],
        Duration::from_secs(1),
        100,
    );
    assert!(matches!(missing, Err(GitError::Spawn(_))));
    assert!(SystemGit::at(fx.root.join("no-such-git")).is_none());
}

/// Run 39774909321: a stopped git left its child running in the folder (Git for Windows' `git.exe` is a launcher;
/// killing it orphaned the real git) -> the person's rename failed with OS error 32. The whole tree goes.
#[test]
fn a_cancelled_git_leaves_nothing_running_in_the_folder() {
    let Some(exe) = find_git() else {
        eprintln!("SKIPPED: no git executable on PATH");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("work ü");
    std::fs::create_dir_all(&folder).unwrap();
    Repository::init(&folder).unwrap();
    let marker = dir.path().join("started.txt");
    let marker_text = marker.to_string_lossy().replace('\\', "/");
    let alias = format!("alias.hold=!echo started > \"{marker_text}\"; sleep 30");
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker = {
        let (exe, folder, cancel) = (exe.clone(), folder.clone(), Arc::clone(&cancel));
        std::thread::spawn(move || {
            system::run_until(
                &exe,
                &folder,
                &[OsStr::new("-c"), OsStr::new(&alias), OsStr::new("hold")],
                Duration::from_secs(60),
                1024,
                Some(&cancel),
            )
        })
    };
    let started = Instant::now();
    while !marker.exists() {
        assert!(started.elapsed() < Duration::from_secs(30), "never started");
        std::thread::sleep(Duration::from_millis(5));
    }
    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        worker.join().unwrap().unwrap_err(),
        GitError::Failed("stopped".to_owned())
    );
    // at once: no grandchild (shell, sleep) holds the folder as its working directory
    std::fs::rename(&folder, dir.path().join("renamed ü")).unwrap();
}

#[test]
fn only_the_repository_own_filter_drivers_are_named() {
    let out = b"system\0filter.lfs.clean\ngit-lfs clean -- %f\0global\0filter.lfs.process\ngit-lfs filter-process\0local\0filter.x.clean\nsh -c 'evil'\0local\0filter.x.smudge\ncat\0worktree\0filter.a.b.process\nrun\0command\0filter.c.clean\nok\0local\0broken";
    assert_eq!(system::repo_filter_drivers(out), ["x", "a.b"]);
    assert!(system::repo_filter_drivers(b"").is_empty());
}

#[test]
fn a_filter_command_from_the_repository_config_never_runs() {
    let Some(git) = SystemGit::find() else {
        eprintln!("SKIPPED: no git executable on PATH");
        return;
    };
    let fx = Fixture::new();
    fx.write(".gitattributes", "* filter=x\n");
    fx.write("a.md", "a\n");
    fx.commit("first");
    let marker = fx.dir.path().join("ran.txt");
    let marker_text = marker.to_string_lossy().replace('\\', "/");
    let mut config = fx.repo.config().unwrap();
    config
        .set_str(
            "filter.x.clean",
            &format!("sh -c 'echo ran > \"{marker_text}\"; cat'"),
        )
        .unwrap();
    config.set_bool("filter.x.required", true).unwrap();
    // a fresh copy: every file must be looked at again
    std::thread::sleep(Duration::from_millis(1100));
    fx.write("a.md", "a\n");
    let at = location(&git, &fx.root);
    let status = git.status(&at, &Baseline::Head).unwrap();
    assert!(!marker.exists(), "the repository's filter ran");
    let head = fx.repo.head().unwrap().target().unwrap().to_string();
    git.status(&at, &Baseline::Commit(head)).unwrap();
    assert!(!marker.exists(), "the repository's filter ran on diff");
    assert!(status.files.iter().all(|f| f.path != ".gitattributes"));
}

// ---- the worker ------------------------------------------------------------------------------------------------------

type Announced = Arc<Mutex<Vec<Option<GitSnapshot>>>>;

struct Running {
    service: GitService,
    announced: Announced,
    facts: Arc<ProjectFacts>,
}

fn timings() -> Timings {
    Timings {
        quiet: Duration::from_millis(20),
        max_wait: Duration::from_millis(120),
        lock_wait: Duration::from_millis(300),
    }
}

fn start(root: &Path, ops: &Arc<dyn GitOps>, worktrees: Vec<(String, PathBuf)>) -> Running {
    start_with(root, ops, worktrees, None, None)
}

fn start_with(
    root: &Path,
    ops: &Arc<dyn GitOps>,
    worktrees: Vec<(String, PathBuf)>,
    prefs: Option<permark_core::state::StateFile<Prefs>>,
    visit: Option<permark_core::state::StateFile<Visit>>,
) -> Running {
    let announced: Announced = Arc::default();
    let facts = ProjectFacts::new(None);
    let service = GitService::start(ServiceArgs {
        root: root.to_path_buf(),
        worktrees,
        choice: BackendChoice::Fixed(Arc::clone(ops)),
        home: None,
        timings: timings(),
        facts: Arc::clone(&facts),
        announce: {
            let announced = Arc::clone(&announced);
            Arc::new(move |snapshot| announced.lock().unwrap().push(snapshot.cloned()))
        },
        prefs,
        visit,
        excluded: Box::new(Vec::new),
    });
    Running {
        service,
        announced,
        facts,
    }
}

fn files_of(snapshot: &GitSnapshot) -> Vec<(&str, Letter)> {
    snapshot
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.letter))
        .collect()
}

#[test]
fn the_worker_announces_changes_once_and_follows_the_files() {
    each(|name, ops| {
        let ops: Arc<dyn GitOps> = if name == "system" {
            Arc::new(SystemGit::find().unwrap())
        } else {
            let _ = ops;
            Arc::new(BuiltinGit)
        };
        let fx = Fixture::new();
        fx.write("a.md", "a\n");
        let first = fx.commit("first");
        let running = start(&fx.root, &ops, Vec::new());
        wait_until("the first snapshot", || {
            running.service.snapshot().is_some()
        });
        let snap = running.service.snapshot().unwrap();
        assert!(snap.files.is_empty() && snap.seq > 0, "{name}");
        assert_eq!(snap.head.oid.as_deref(), Some(first.as_str()), "{name}");
        assert_eq!(snap.backend, ops.kind(), "{name}");
        assert_eq!(
            running.facts.git(),
            GitCondition::Repo {
                state: RepoState::Clean
            },
            "{name}"
        );

        fx.write("a.md", "edited\n");
        running.service.trigger(Why::Files);
        wait_until("the change", || {
            running
                .service
                .snapshot()
                .is_some_and(|s| s.files.len() == 1)
        });
        let after = running.service.snapshot().unwrap();
        assert_eq!(files_of(&after), [("a.md", Letter::Modified)], "{name}");
        assert!(after.seq > snap.seq, "{name}");

        // a service that starts again (the backend setting changed) numbers its pictures above the old one's
        let again = start(&fx.root, &ops, Vec::new());
        wait_until("the picture of the new service", || {
            again.service.snapshot().is_some()
        });
        assert!(again.service.snapshot().unwrap().seq > after.seq, "{name}");

        // nothing changed: a trigger announces nothing
        let before = running.announced.lock().unwrap().len();
        for _ in 0..5 {
            running.service.trigger(Why::Files);
        }
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(running.announced.lock().unwrap().len(), before, "{name}");
    });
}

#[test]
fn a_burst_of_triggers_is_one_refresh() {
    let ops: Arc<dyn GitOps> = Arc::new(BuiltinGit);
    let fx = Fixture::new();
    fx.write("a.md", "a\n");
    fx.commit("first");
    let running = start(&fx.root, &ops, Vec::new());
    wait_until("the first snapshot", || {
        running.service.snapshot().is_some()
    });
    for n in 0..30 {
        fx.write("a.md", &format!("edit {n}\n"));
        running.service.trigger(Why::Files);
    }
    wait_until("the burst", || {
        running
            .service
            .snapshot()
            .is_some_and(|s| s.files.len() == 1)
    });
    std::thread::sleep(Duration::from_millis(300));
    // the first snapshot and one for the burst (a straggler is allowed, thirty are not)
    assert!(running.announced.lock().unwrap().len() <= 3);
}

#[test]
fn a_folder_without_a_repository_waits_for_one() {
    each(|name, _| {
        let ops: Arc<dyn GitOps> = if name == "system" {
            Arc::new(SystemGit::find().unwrap())
        } else {
            Arc::new(BuiltinGit)
        };
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join("a.md"), "a\n").unwrap();
        let running = start(&root, &ops, Vec::new());
        std::thread::sleep(Duration::from_millis(300));
        assert!(running.service.snapshot().is_none(), "{name}");
        assert_eq!(running.facts.git(), GitCondition::NoRepo, "{name}");
        // plain file changes do not look for a repository ...
        let _repo = Repository::init(&root).unwrap();
        running.service.trigger(Why::Files);
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            running.service.snapshot().is_none(),
            "{name}: files do not rediscover"
        );
        // ... a git signal does
        running.service.trigger(Why::Signal);
        wait_until("the repository", || running.service.snapshot().is_some());
        assert!(
            matches!(running.facts.git(), GitCondition::Repo { .. }),
            "{name}"
        );
        assert_eq!(
            files_of(&running.service.snapshot().unwrap()),
            [("a.md", Letter::Untracked)],
            "{name}"
        );
    });
}

#[test]
fn a_missing_backend_is_unavailable_and_dropping_the_service_stops_it() {
    // a system backend that is not there
    let facts = ProjectFacts::new(None);
    let service = GitService::start(ServiceArgs {
        root: std::env::temp_dir(),
        worktrees: Vec::new(),
        choice: BackendChoice::Pref(permark_core::settings::GitBackendPref::System),
        home: None,
        timings: timings(),
        facts: Arc::clone(&facts),
        announce: Arc::new(|_| {}),
        prefs: None,
        visit: None,
        excluded: Box::new(Vec::new),
    });
    if SystemGit::find().is_none() {
        wait_until("the verdict", || facts.git() == GitCondition::Unavailable);
    }
    let started = Instant::now();
    drop(service);
    assert!(started.elapsed() < Duration::from_secs(3), "joins promptly");
}

#[test]
fn the_state_of_a_merge_and_the_wait_for_the_index_lock() {
    let ops: Arc<dyn GitOps> = Arc::new(BuiltinGit);
    let fx = Fixture::new();
    fx.write("a.md", "a\n");
    fx.commit("first");
    let running = start(&fx.root, &ops, Vec::new());
    wait_until("the first snapshot", || {
        running.service.snapshot().is_some()
    });
    let git_dir = fx.repo.path().to_path_buf();
    std::fs::write(
        git_dir.join("MERGE_HEAD"),
        "0000000000000000000000000000000000000000\n",
    )
    .unwrap();
    running.service.trigger(Why::Signal);
    wait_until("merging", || {
        running.facts.git()
            == GitCondition::Repo {
                state: RepoState::Merging,
            }
    });
    assert_eq!(
        running.service.snapshot().unwrap().state,
        RepoState::Merging
    );
    std::fs::remove_file(git_dir.join("MERGE_HEAD")).unwrap();
    // git is writing: the refresh waits, then goes on after the lock (or its limit)
    std::fs::write(git_dir.join("index.lock"), "").unwrap();
    running.service.trigger(Why::Signal);
    let started = Instant::now();
    wait_until("busy", || {
        running.facts.git()
            == GitCondition::Repo {
                state: RepoState::Busy,
            }
    });
    assert!(
        started.elapsed() >= Duration::from_millis(250),
        "waited for the lock"
    );
    std::fs::remove_file(git_dir.join("index.lock")).unwrap();
    running.service.trigger(Why::Signal);
    wait_until("clean again", || {
        running.facts.git()
            == GitCondition::Repo {
                state: RepoState::Clean,
            }
    });
}

#[test]
fn since_the_last_visit_and_the_choice_of_baseline_are_remembered() {
    let ops: Arc<dyn GitOps> = Arc::new(BuiltinGit);
    let fx = Fixture::new();
    fx.write("a.md", "a\n");
    let first = fx.commit("first");
    let store_dir = tempfile::tempdir().unwrap();
    let store = permark_core::state::StateStore::for_project(
        &AppDirs::under(store_dir.path()),
        &Root::open(&fx.root).unwrap(),
    );
    let id = PluginId::of("git");
    // visit 1: nothing before it; closing remembers where HEAD was
    {
        let running = start_with(
            &fx.root,
            &ops,
            Vec::new(),
            Some(store.plugin_file(&id, "prefs.json")),
            Some(store.plugin_file(&id, "visit.json")),
        );
        wait_until("visit 1", || running.service.snapshot().is_some());
        let snap = running.service.snapshot().unwrap();
        assert_eq!(snap.baseline.kind, BaselineKind::LastVisit);
        assert_eq!(snap.baseline.oid, None, "first visit: nothing is new");
    }
    // while closed: an agent commits and leaves a loose file
    fx.write("a.md", "a by the agent\n");
    fx.write("new.md", "n\n");
    let second = fx.commit("agent");
    fx.write("loose.md", "l\n");
    // visit 2: measured from where visit 1 ended
    let running = start_with(
        &fx.root,
        &ops,
        Vec::new(),
        Some(store.plugin_file(&id, "prefs.json")),
        Some(store.plugin_file(&id, "visit.json")),
    );
    wait_until("visit 2", || running.service.snapshot().is_some());
    let snap = running.service.snapshot().unwrap();
    assert_eq!(snap.baseline.oid.as_deref(), Some(first.as_str()));
    assert_eq!(snap.head.oid.as_deref(), Some(second.as_str()));
    assert_eq!(
        files_of(&snap),
        [
            ("a.md", Letter::Modified),
            ("loose.md", Letter::Untracked),
            ("new.md", Letter::Added)
        ]
    );
    // since the last commit
    running.service.set_baseline(BaselineKind::Head);
    wait_until("head baseline", || {
        running
            .service
            .snapshot()
            .is_some_and(|s| s.baseline.kind == BaselineKind::Head)
    });
    assert_eq!(
        files_of(&running.service.snapshot().unwrap()),
        [("loose.md", Letter::Untracked)]
    );
    drop(running);
    // the choice survives a restart
    let again = start_with(
        &fx.root,
        &ops,
        Vec::new(),
        Some(store.plugin_file(&id, "prefs.json")),
        Some(store.plugin_file(&id, "visit.json")),
    );
    wait_until("visit 3", || again.service.snapshot().is_some());
    assert_eq!(
        again.service.snapshot().unwrap().baseline.kind,
        BaselineKind::Head
    );
    // a stored commit the repository does not have falls back to HEAD
    drop(again);
    store
        .plugin_file::<Visit>(&id, "visit.json")
        .save(&Visit {
            head: Some("f".repeat(40)),
        })
        .unwrap();
    store
        .plugin_file::<Prefs>(&id, "prefs.json")
        .save(&Prefs {
            baseline: BaselineKind::LastVisit,
        })
        .unwrap();
    let lost = start_with(
        &fx.root,
        &ops,
        Vec::new(),
        Some(store.plugin_file(&id, "prefs.json")),
        Some(store.plugin_file(&id, "visit.json")),
    );
    wait_until("visit 4", || lost.service.snapshot().is_some());
    assert_eq!(lost.service.snapshot().unwrap().baseline.oid, None);
}

#[test]
fn a_session_that_starts_before_the_first_commit_measures_against_head() {
    let ops: Arc<dyn GitOps> = Arc::new(BuiltinGit);
    let fx = Fixture::new();
    fx.write("a.md", "a\n");
    let running = start(&fx.root, &ops, Vec::new());
    wait_until("unborn", || {
        running.service.snapshot().is_some_and(|s| s.head.unborn)
    });
    fx.commit("first");
    running.service.trigger(Why::Signal);
    wait_until("born", || {
        running.service.snapshot().is_some_and(|s| !s.head.unborn)
    });
    assert!(running.service.snapshot().unwrap().files.is_empty());
}

#[test]
fn linked_worktrees_are_found_listed_and_given_their_own_letters() {
    each(|name, _| {
        let ops: Arc<dyn GitOps> = if name == "system" {
            Arc::new(SystemGit::find().unwrap())
        } else {
            Arc::new(BuiltinGit)
        };
        let fx = Fixture::new();
        fx.write("a.md", "a\n");
        fx.commit("first");
        let wt_dir = fx.dir.path().join("wt ü");
        fx.repo.worktree("feat", &wt_dir, None).unwrap();
        let linked = linked_worktrees(&fx.root, None);
        assert_eq!(linked.len(), 1, "{name}: {linked:?}");
        assert_eq!(linked[0].name, "feat");
        std::fs::write(wt_dir.join("a.md"), "changed in the worktree\n").unwrap();
        std::fs::write(wt_dir.join("only there.md"), "n\n").unwrap();
        let roots: Vec<(String, PathBuf)> =
            linked.into_iter().map(|l| (l.name, l.folder)).collect();
        let running = start(&fx.root, &ops, roots);
        wait_until("worktree letters", || {
            running
                .service
                .snapshot()
                .is_some_and(|s| !s.files.is_empty())
        });
        let snap = running.service.snapshot().unwrap();
        assert_eq!(
            files_of(&snap),
            [
                ("@wt/feat/a.md", Letter::Modified),
                ("@wt/feat/only there.md", Letter::Untracked)
            ],
            "{name}"
        );
        assert_eq!(snap.worktrees.len(), 1);
        assert_eq!(snap.worktrees[0].branch.as_deref(), Some("feat"), "{name}");
        // the reader answers for both trees
        let reader = running.service.reader();
        assert_eq!(
            reader.blob_at_head("a.md").as_deref(),
            Some(b"a\n".as_slice()),
            "{name}"
        );
        assert_eq!(
            reader.blob_at_head("@wt/feat/a.md").as_deref(),
            Some(b"a\n".as_slice()),
            "{name}"
        );
        assert_eq!(reader.blob_at_head("@wt/nope/a.md"), None);
        assert_eq!(reader.blob_at_head("@wt/feat"), None);
        assert_eq!(reader.blob_at_head("missing.md"), None);
    });
}

// ---- the plugin in a project -----------------------------------------------------------------------------------------

fn env_with_git(base: &Path, enabled: bool) -> CoreEnv {
    let settings = AppSettings {
        plugins: BTreeMap::from([(PluginId::of("git"), enabled)]),
        ..AppSettings::default()
    };
    CoreEnv::new(
        AppDirs::under(base),
        Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
        Arc::new(NoEvents),
        Distribution::Direct,
        settings,
    )
}

fn call(
    core: &ProjectCore,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, permark_core::IpcError> {
    core.rpc().call(
        "git",
        method,
        &RpcCall {
            window: "w".to_owned(),
            args,
        },
    )
}

#[test]
fn the_plugin_feeds_the_condition_the_vault_and_the_rpc_and_leaves_nothing_when_off() {
    let fx = Fixture::new();
    fx.write("a.md", "a\n");
    fx.commit("first");
    let wt_dir = fx.dir.path().join("linked");
    fx.repo.worktree("side", &wt_dir, None).unwrap();
    std::fs::write(fx.root.join("a.md"), "edited\n").unwrap();
    let base = tempfile::tempdir().unwrap();
    let env = env_with_git(base.path(), true);

    let core = ProjectCore::open(Root::open(&fx.root).unwrap(), &env);
    // the worktree is a root of the project before the plugin even runs
    let names: Vec<&str> = core
        .vault()
        .worktrees()
        .iter()
        .map(|w| w.name.as_str())
        .collect();
    assert_eq!(names, ["side"]);
    wait_until("the condition", || {
        matches!(core.facts().git(), GitCondition::Repo { .. })
    });
    wait_until("the snapshot", || {
        call(&core, "snapshot", serde_json::Value::Null)
            .is_ok_and(|v| v["files"].as_array().is_some_and(|f| !f.is_empty()))
    });
    let snapshot = call(&core, "snapshot", serde_json::Value::Null).unwrap();
    assert_eq!(snapshot["files"][0]["path"], "a.md");
    assert_eq!(snapshot["files"][0]["letter"], "modified");
    assert_eq!(snapshot["worktrees"][0]["name"], "side");
    call(&core, "refresh", serde_json::Value::Null).unwrap();
    call(&core, "setBaseline", serde_json::json!({"kind": "head"})).unwrap();
    wait_until("head baseline", || {
        call(&core, "snapshot", serde_json::Value::Null)
            .is_ok_and(|v| v["baseline"]["kind"] == "head")
    });
    assert!(call(
        &core,
        "setBaseline",
        serde_json::json!({"kind": "sideways"})
    )
    .is_err());

    // switched off: the condition goes, nobody answers, the vault keeps what it was built with
    env.set_settings(AppSettings {
        plugins: BTreeMap::from([(PluginId::of("git"), false)]),
        ..AppSettings::default()
    });
    core.apply_plugins(&env);
    assert_eq!(core.facts().git(), GitCondition::NoRepo);
    assert_eq!(
        call(&core, "snapshot", serde_json::Value::Null)
            .unwrap_err()
            .kind,
        permark_core::ErrorKind::Unavailable
    );
    assert!(core.rpc().served().is_empty());
    // and on again
    env.set_settings(AppSettings {
        plugins: BTreeMap::from([(PluginId::of("git"), true)]),
        ..AppSettings::default()
    });
    core.apply_plugins(&env);
    wait_until("on again", || {
        matches!(core.facts().git(), GitCondition::Repo { .. })
    });
    drop(core);
}

#[test]
fn the_plugin_is_off_in_a_folder_without_a_repository_and_reports_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("a.md"), "a\n").unwrap();
    let base = tempfile::tempdir().unwrap();
    let env = env_with_git(base.path(), true);
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(core.facts().git(), GitCondition::NoRepo);
    assert_eq!(
        call(&core, "snapshot", serde_json::Value::Null).unwrap(),
        serde_json::Value::Null
    );
}

#[test]
fn the_backend_setting_is_read_from_the_plugin_table() {
    let table: toml_edit::DocumentMut = "backend = \"system\"\n".parse().unwrap();
    assert_eq!(
        backend_pref(Some(table.as_table())),
        permark_core::settings::GitBackendPref::System
    );
    let bad: toml_edit::DocumentMut = "backend = 3\n".parse().unwrap();
    assert_eq!(
        backend_pref(Some(bad.as_table())),
        permark_core::settings::GitBackendPref::Automatic
    );
    assert_eq!(
        backend_pref(None),
        permark_core::settings::GitBackendPref::Automatic
    );
}
