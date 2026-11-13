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
use permark_core::plugins::{
    CommitId, FileVersion, FileVersions, NoBlob, NoVersions, PluginId, PluginRegistry,
    ProjectFacts, RepoReader, RpcCall,
};
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

    /// Stages everything (new, changed, deleted): the tree of the next commit.
    fn staged(&self) -> git2::Tree<'_> {
        let mut index = self.repo.index().unwrap();
        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .unwrap();
        index.update_all(["*"].iter(), None).unwrap();
        index.write().unwrap();
        self.repo.find_tree(index.write_tree().unwrap()).unwrap()
    }

    /// Stages everything (new, changed, deleted) and commits; returns the commit id.
    fn commit(&self, message: &str) -> String {
        let tree = self.staged();
        let sig = Signature::now("Tester", "tester@example.com").unwrap();
        let parent = self.repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
        self.repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .unwrap()
            .to_string()
    }

    /// [`Self::commit`] by `author`, committed at `seconds` by another person (the author wrote it an hour before).
    /// The versions of a file are listed by commit time: every commit of such a test gets its own second.
    fn commit_at(&self, seconds: i64, author: &str, message: &str) -> String {
        self.merge_at(seconds, author, message, &[])
    }

    /// [`Self::commit_at`] with more parents than HEAD: a merge with `others`, holding the files as they are now.
    fn merge_at(&self, seconds: i64, author: &str, message: &str, others: &[&str]) -> String {
        let tree = self.staged();
        let author = Signature::new(
            author,
            "author@example.com",
            &git2::Time::new(seconds - 3600, 0),
        )
        .unwrap();
        let committer = Signature::new(
            "Committer",
            "committer@example.com",
            &git2::Time::new(seconds, 0),
        )
        .unwrap();
        let mut parents: Vec<git2::Commit<'_>> = self
            .repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok())
            .into_iter()
            .collect();
        parents.extend(others.iter().map(|id| self.commit_of(id)));
        let parents: Vec<&git2::Commit<'_>> = parents.iter().collect();
        self.repo
            .commit(Some("HEAD"), &author, &committer, message, &tree, &parents)
            .unwrap()
            .to_string()
    }

    fn commit_of(&self, id: &str) -> git2::Commit<'_> {
        self.repo
            .find_commit(git2::Oid::from_str(id).unwrap())
            .unwrap()
    }

    /// The id of the content `rel` has at the commit `id`.
    fn blob_id(&self, id: &str, rel: &str) -> String {
        let tree = self.commit_of(id).tree().unwrap();
        tree.get_path(Path::new(rel)).unwrap().id().to_string()
    }

    /// Moves HEAD to the branch `reference` (a full name) and makes the files those of its commit.
    fn switch(&self, reference: &str) {
        self.repo.set_head(reference).unwrap();
        self.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
    }

    /// Makes `rel` executable without touching its bytes: in the index, and on disk where the system keeps the bit
    /// (elsewhere git keeps the mode of the index). The next commit changes the mode of the file only.
    fn make_executable(&self, rel: &str) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(self.root.join(rel), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let mut index = self.repo.index().unwrap();
        let mut entry = index.get_path(Path::new(rel), 0).unwrap();
        entry.mode = 0o100_755;
        index.add(&entry).unwrap();
        index.write().unwrap();
    }

    /// Line endings go into commits as written, whatever this computer's git settings say.
    fn keep_line_endings(&self) {
        self.repo
            .config()
            .unwrap()
            .set_bool("core.autocrlf", false)
            .unwrap();
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

fn commit_id(hex: &str) -> CommitId {
    CommitId::parse(hex).unwrap()
}

/// The `file://` address of a folder (a plain path is copied whole: no filter applies).
fn file_url(folder: &Path) -> String {
    let path = folder.to_string_lossy().replace('\\', "/");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
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
fn blobs_at_a_commit_are_the_bytes_committed_then() {
    each(|name, ops| {
        let fx = Fixture::new();
        // line endings go in as written, whatever this computer's git settings say
        fx.repo
            .config()
            .unwrap()
            .set_bool("core.autocrlf", false)
            .unwrap();
        fx.write("docs/a.md", "one\n");
        fx.write("docs/crlf.md", "a\r\nb\r\n");
        let first = commit_id(&fx.commit("first"));
        fx.write("docs/a.md", "two\n");
        let second = commit_id(&fx.commit("second"));
        fx.write("docs/a.md", "three\n");
        fx.write("docs/later.md", "l\n");
        let third = commit_id(&fx.commit("third"));
        fx.write("docs/a.md", "edited after\n");
        let loc = location(ops, &fx.root);
        let at = |commit: CommitId, path: &str| ops.blob_at(&loc, commit, path).unwrap();
        assert_eq!(
            at(first, "docs/a.md").as_deref(),
            Some(b"one\n".as_slice()),
            "{name}"
        );
        assert_eq!(
            at(second, "docs/a.md").as_deref(),
            Some(b"two\n".as_slice()),
            "{name}"
        );
        assert_eq!(
            at(third, "docs/a.md").as_deref(),
            Some(b"three\n".as_slice()),
            "{name}"
        );
        assert_eq!(
            at(third, "docs/a.md"),
            ops.blob_at_head(&loc, "docs/a.md").unwrap(),
            "{name}: the id of HEAD"
        );
        assert_eq!(
            at(first, "docs/crlf.md").as_deref(),
            Some(b"a\r\nb\r\n".as_slice()),
            "{name}: raw, no line-ending conversion"
        );
        // not there at that commit
        assert_eq!(at(first, "docs/later.md"), None, "{name}: added later");
        assert_eq!(at(third, "missing.md"), None, "{name}");
        assert_eq!(at(third, "docs"), None, "{name}: a folder is no blob");
        assert_eq!(at(third, "a\0b"), None, "{name}");
        assert_eq!(at(third, "docs/a.md\0"), None, "{name}");
        // well-formed ids the repository does not have
        assert_eq!(at(commit_id(&"0".repeat(40)), "docs/a.md"), None, "{name}");
        assert_eq!(at(commit_id(&"1".repeat(40)), "docs/a.md"), None, "{name}");
        assert_eq!(
            at(commit_id(&"f".repeat(64)), "docs/a.md"),
            None,
            "{name}: an id of the other hash length"
        );
        // an id that is no commit: read as git reads `<id>:<path>` (through a tree), alike in both backends
        let tree = fx.repo.head().unwrap().peel_to_tree().unwrap();
        assert_eq!(
            at(commit_id(&tree.id().to_string()), "docs/a.md").as_deref(),
            Some(b"three\n".as_slice()),
            "{name}: a tree id"
        );
        let blob = tree.get_path(Path::new("docs/a.md")).unwrap().id();
        assert_eq!(
            at(commit_id(&blob.to_string()), "docs/a.md"),
            None,
            "{name}: a blob id leads to no tree"
        );
        // a project below the repository asks with project paths
        let below = location(ops, &fx.root.join("docs"));
        assert_eq!(
            ops.blob_at(&below, first, "a.md").unwrap().as_deref(),
            Some(b"one\n".as_slice()),
            "{name}"
        );
        assert_eq!(
            ops.blob_at(&below, first, "docs/a.md").unwrap(),
            None,
            "{name}: repository paths are not project paths"
        );
    });
}

#[test]
fn a_committed_file_over_the_limit_is_too_large() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.write("huge.bin", &"x".repeat(8 * 1024 * 1024 + 1));
        fx.write("small.md", "s\n");
        let first = commit_id(&fx.commit("first"));
        let loc = location(ops, &fx.root);
        assert!(
            matches!(
                ops.blob_at(&loc, first, "huge.bin"),
                Err(GitError::TooLarge(_))
            ),
            "{name}"
        );
        assert!(
            matches!(
                ops.blob_at_head(&loc, "huge.bin"),
                Err(GitError::TooLarge(_))
            ),
            "{name}: at HEAD"
        );
        assert_eq!(
            ops.blob_at(&loc, first, "small.md").unwrap().as_deref(),
            Some(b"s\n".as_slice()),
            "{name}: other files of the commit are read"
        );
    });
}

/// File names git could take for something else: an option, a pattern, a revision, pathspec magic.
fn hostile_names() -> Vec<&'static str> {
    let mut names = vec![
        "ü ñ 文.md",
        "with space.md",
        "[x].md",
        "-rf.md",
        "--output=x",
        "@{u}.md",
        "{x}/-y [1]/é ü.md",
    ];
    // not valid file names on Windows
    if !cfg!(windows) {
        names.extend(["*.md", "what?.md", ":colon.md", "a:b.md", "{x}/:deep.md"]);
    }
    names
}

#[test]
fn files_with_hostile_names_are_read_at_a_commit() {
    let names = hostile_names();
    each(|name, ops| {
        let fx = Fixture::new();
        for (n, file) in names.iter().enumerate() {
            fx.write(file, &format!("{n} then\n"));
        }
        let first = commit_id(&fx.commit("first"));
        for (n, file) in names.iter().enumerate() {
            fx.write(file, &format!("{n} now\n"));
        }
        fx.commit("second");
        // asked from the repository's folder, and from a project below it
        for root in ["", "{x}"] {
            let loc = location(ops, &fx.root.join(root));
            for (n, file) in names.iter().enumerate() {
                let Some(path) = crate::ops::below_prefix(root, file) else {
                    continue;
                };
                assert_eq!(
                    ops.blob_at(&loc, first, path).unwrap(),
                    Some(format!("{n} then\n").into_bytes()),
                    "{name}: {path:?} below {root:?}"
                );
            }
        }
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

// ---- the versions of a file ------------------------------------------------------------------------------------------

/// A commit time no test repository reaches by itself (the fixtures count seconds from here).
const T: i64 = 1_700_000_000;

/// Every page of the versions of `path`, `limit` changes at a time, until the end.
fn pages(ops: &dyn GitOps, loc: &RepoLocation, path: &str, limit: usize) -> Vec<FileVersions> {
    let mut pages = Vec::new();
    let mut after = 0;
    loop {
        let page = ops.file_versions(loc, path, after, limit).unwrap();
        let next = page.next;
        pages.push(page);
        let Some(next) = next else {
            return pages;
        };
        assert!(
            next > after,
            "{path}: a page goes forward ({after} -> {next})"
        );
        assert!(pages.len() < 200, "{path}: the pages do not end");
        after = next;
    }
}

/// The versions of `path` in one page.
fn versions(ops: &dyn GitOps, loc: &RepoLocation, path: &str) -> FileVersions {
    ops.file_versions(loc, path, 0, 100).unwrap()
}

fn commits(rows: &[FileVersion]) -> Vec<String> {
    rows.iter().map(|row| row.commit.to_string()).collect()
}

fn joined(pages: &[FileVersions]) -> Vec<FileVersion> {
    pages.iter().flat_map(|page| page.rows.clone()).collect()
}

/// `notes/linear.md` changed by 3 commits, with 2 commits between that leave it alone: the 3, newest first.
fn linear_history(fx: &Fixture, t: i64) -> [String; 3] {
    fx.write("notes/linear.md", "one\n");
    fx.write("notes/else.md", "e\n");
    let first = fx.commit_at(t, "Ann Ü", "Add the notes\n\nA body line.\n");
    fx.write("notes/else.md", "ee\n");
    fx.commit_at(t + 10, "Ann Ü", "Something else");
    fx.write("notes/linear.md", "two, longer\n");
    let message = format!(
        "  {}  \n\nCo-authored-by: Cy Young <cy@example.com>\nCo-authored-by: Bea <bea@example.com>\n",
        "long subject ".repeat(30)
    );
    let second = fx.commit_at(t + 20, "Bea", &message);
    fx.write("notes/else.md", "eee\n");
    fx.commit_at(t + 30, "Ann Ü", "Something else again");
    fx.write("notes/linear.md", "three, the longest\n");
    let third = fx.commit_at(t + 40, "Ann Ü", "Third");
    [third, second, first]
}

/// The commits that changed `doc.md` and `kept.md`, each newest first, and the merge that joined the two lines.
struct Merged {
    rows: [String; 5],
    kept: [String; 2],
    merge: String,
}

/// `doc.md` changed on the main line and on a side branch, then merged by hand (the merge holds a text of its own)
/// and changed once more; both lines also hold a commit that leaves it alone. `kept.md` changed on the side branch
/// only, and the merge kept the main line's text. Commit times interleave.
fn merged_history(fx: &Fixture, t: i64) -> Merged {
    fx.write("doc.md", "one\n");
    fx.write("other.md", "o\n");
    fx.write("kept.md", "kept\n");
    let base = fx.commit_at(t, "Ann", "Base");
    let main_line = fx.head_ref();
    fx.write("doc.md", "two, main\n");
    let main = fx.commit_at(t + 10, "Ann", "Main changes the document");
    fx.write("other.md", "o, main\n");
    fx.commit_at(t + 30, "Ann", "Main changes something else");
    fx.repo.branch("side", &fx.commit_of(&base), false).unwrap();
    fx.switch("refs/heads/side");
    fx.write("doc.md", "two, side\n");
    fx.write("kept.md", "kept, changed on the side\n");
    let side = fx.commit_at(t + 20, "Bea", "Side changes the document");
    fx.write("other.md", "o, side\n");
    fx.commit_at(t + 40, "Bea", "Side changes something else");
    fx.write("doc.md", "three, side\n");
    let side_again = fx.commit_at(t + 50, "Bea", "Side changes the document again");
    fx.switch(&main_line);
    fx.write("doc.md", "merged by hand\n");
    fx.write("other.md", "o, merged\n");
    let merge = fx.merge_at(t + 60, "Ann", "Merge branch 'side'", &[&side_again]);
    fx.write("doc.md", "after the merge\n");
    let after = fx.commit_at(t + 70, "Ann", "After the merge");
    Merged {
        kept: [side.clone(), base.clone()],
        rows: [after, side_again, side, main, base],
        merge,
    }
}

/// The commits that made a version of `churn.md`, newest first, and two that touched it without making one.
struct Churned {
    rows: [String; 3],
    mode_only: String,
    deleted: String,
}

/// `churn.md` added, made executable, given other line endings, deleted, and added again with its first bytes; one
/// commit between leaves it alone.
fn churned_history(fx: &Fixture, t: i64) -> Churned {
    fx.write("churn.md", "a\nb\n");
    let added = fx.commit_at(t, "Ann", "Add");
    fx.make_executable("churn.md");
    let mode_only = fx.commit_at(t + 10, "Ann", "Make it executable");
    fx.write("churn.md", "a\r\nb\r\n");
    let line_endings = fx.commit_at(t + 20, "Ann", "Other line endings");
    fx.remove("churn.md");
    let deleted = fx.commit_at(t + 30, "Ann", "Delete");
    fx.write("unrelated.md", &format!("unrelated {t}\n"));
    fx.commit_at(t + 40, "Ann", "Unrelated");
    fx.write("churn.md", "a\nb\n");
    let again = fx.commit_at(t + 50, "Ann", "Add again");
    Churned {
        rows: [again, line_endings, added],
        mode_only,
        deleted,
    }
}

/// The commits of `old name.md` and of `docs/new ü.md` (the same file after its rename), each newest first.
struct Renamed {
    old: [String; 2],
    new: [String; 2],
}

/// `old name.md` changed, renamed to `docs/new ü.md` and changed again; `docs/x.md` renamed to `docs/y.md`;
/// `docs/fresh.md` added and never renamed.
fn renamed_history(fx: &Fixture, t: i64) -> Renamed {
    let text = "a text long enough to be found again under another name\nsecond line\nthird line\n";
    fx.write("old name.md", text);
    let added = fx.commit_at(t, "Ann Ü", "Add under the old name");
    fx.write("old name.md", &format!("{text}fourth line\n"));
    let changed = fx.commit_at(t + 10, "Ann Ü", "Change under the old name");
    std::fs::create_dir_all(fx.root.join("docs")).unwrap();
    std::fs::rename(fx.root.join("old name.md"), fx.root.join("docs/new ü.md")).unwrap();
    let renamed = fx.commit_at(t + 20, "Ann Ü", "Rename");
    fx.write("docs/new ü.md", &format!("{text}fourth line\nfifth line\n"));
    let changed_again = fx.commit_at(t + 30, "Ann Ü", "Change under the new name");
    fx.write("docs/x.md", &format!("{text}of another file\n"));
    fx.write("docs/fresh.md", "just added\n");
    fx.commit_at(t + 40, "Ann Ü", "Add two files");
    std::fs::rename(fx.root.join("docs/x.md"), fx.root.join("docs/y.md")).unwrap();
    fx.commit_at(t + 50, "Ann Ü", "Rename inside the folder");
    Renamed {
        old: [changed, added],
        new: [changed_again, renamed],
    }
}

#[test]
fn the_versions_of_a_file_are_the_commits_that_changed_it_newest_first() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.keep_line_endings();
        let [third, second, first] = linear_history(&fx, T);
        let loc = location(ops, &fx.root);
        let found = ops.file_versions(&loc, "notes/linear.md", 0, 50).unwrap();
        let row = |id: &str, seconds: i64, author: &str, co_authors: &[&str], subject: &str| {
            FileVersion {
                commit: commit_id(id),
                blob: fx.blob_id(id, "notes/linear.md"),
                time_ms: seconds * 1000,
                author: author.to_owned(),
                co_authors: co_authors.iter().map(|&n| n.to_owned()).collect(),
                subject: subject.to_owned(),
            }
        };
        let long = ops::subject_of(&"long subject ".repeat(30));
        assert_eq!(long.chars().count(), ops::MAX_SUBJECT);
        let expected = vec![
            row(&third, T + 40, "Ann Ü", &[], "Third"),
            // the committer's time, the author's name; a co-author who is the author is not named twice
            row(&second, T + 20, "Bea", &["Cy Young"], &long),
            row(&first, T, "Ann Ü", &[], "Add the notes"),
        ];
        assert_eq!(found.rows, expected, "{name}");
        assert_ne!(found.rows[0].blob, found.rows[1].blob, "{name}");
        assert_ne!(found.rows[1].blob, found.rows[2].blob, "{name}");
        assert_eq!(
            found.head_blob.as_ref(),
            Some(&found.rows[0].blob),
            "{name}"
        );
        assert_eq!(
            (found.next, found.renamed_from.as_deref(), found.cut),
            (None, None, false),
            "{name}"
        );
    });
}

#[test]
fn the_commits_of_a_merged_branch_are_versions_and_the_merge_is_none() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.keep_line_endings();
        let history = merged_history(&fx, T);
        let loc = location(ops, &fx.root);
        let found = versions(ops, &loc, "doc.md");
        assert_eq!(commits(&found.rows), history.rows, "{name}");
        let authors: Vec<&str> = found.rows.iter().map(|row| row.author.as_str()).collect();
        assert_eq!(authors, ["Ann", "Bea", "Bea", "Ann", "Ann"], "{name}");
        // a change the merge did not keep is a version all the same: HEAD holds the text of an older one
        let kept = versions(ops, &loc, "kept.md");
        assert_eq!(commits(&kept.rows), history.kept, "{name}");
        assert_eq!(kept.head_blob.as_ref(), Some(&kept.rows[1].blob), "{name}");
        assert_eq!(
            found.head_blob.as_ref(),
            Some(&found.rows[0].blob),
            "{name}"
        );
        // HEAD at the merge (no branch): the walk starts there, and the text the merge holds is no version's
        fx.repo
            .set_head_detached(git2::Oid::from_str(&history.merge).unwrap())
            .unwrap();
        let at_merge = versions(ops, &loc, "doc.md");
        assert_eq!(commits(&at_merge.rows), history.rows[1..], "{name}");
        let merged = fx.blob_id(&history.merge, "doc.md");
        assert_eq!(at_merge.head_blob.as_ref(), Some(&merged), "{name}");
        assert!(at_merge.rows.iter().all(|row| row.blob != merged), "{name}");
    });
}

#[test]
fn a_change_of_mode_or_a_deletion_is_no_version_and_other_line_endings_are_one() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.keep_line_endings();
        let history = churned_history(&fx, T);
        // the fixture: one commit changed the mode and nothing else
        let entry = |id: &str| {
            let tree = fx.commit_of(id).tree().unwrap();
            let entry = tree.get_path(Path::new("churn.md")).unwrap();
            (entry.filemode(), entry.id().to_string())
        };
        let (added, mode_only) = (entry(&history.rows[2]), entry(&history.mode_only));
        assert_eq!((added.0, mode_only.0), (0o100_644, 0o100_755));
        assert_eq!(added.1, mode_only.1);

        let loc = location(ops, &fx.root);
        let found = versions(ops, &loc, "churn.md");
        assert_eq!(commits(&found.rows), history.rows, "{name}");
        assert_eq!(
            found.rows[0].blob, found.rows[2].blob,
            "{name}: added again with its first bytes"
        );
        assert_ne!(
            found.rows[1].blob, found.rows[2].blob,
            "{name}: only the line endings differ"
        );
        assert_eq!(
            found.head_blob.as_ref(),
            Some(&found.rows[0].blob),
            "{name}"
        );
        // while HEAD is the commit that deleted the file: its earlier versions, and no file at HEAD
        fx.repo
            .set_head_detached(git2::Oid::from_str(&history.deleted).unwrap())
            .unwrap();
        let gone = versions(ops, &loc, "churn.md");
        assert_eq!(commits(&gone.rows), history.rows[1..], "{name}");
        assert_eq!((gone.head_blob, gone.next), (None, None), "{name}");
    });
}

#[test]
fn the_versions_come_in_pages_that_join_to_the_whole_list() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.keep_line_endings();
        let linear = linear_history(&fx, T);
        churned_history(&fx, T + 1000);
        let loc = location(ops, &fx.root);
        // two at a time
        let first = ops.file_versions(&loc, "notes/linear.md", 0, 2).unwrap();
        assert_eq!(commits(&first.rows), linear[..2], "{name}");
        let next = first.next.expect("a second page");
        let second = ops.file_versions(&loc, "notes/linear.md", next, 2).unwrap();
        assert_eq!(commits(&second.rows), linear[2..], "{name}");
        assert_eq!(second.next, None, "{name}");
        // a page that is filled by the last versions knows the list ends with it
        let exact = ops.file_versions(&loc, "notes/linear.md", 0, 3).unwrap();
        assert_eq!((exact.rows.len(), exact.next), (3, None), "{name}");
        for path in ["notes/linear.md", "churn.md"] {
            let whole = versions(ops, &loc, path);
            assert_eq!(whole.rows.len(), 3, "{name}: {path}");
            for limit in 1..=6 {
                let pages = pages(ops, &loc, path, limit);
                assert_eq!(joined(&pages), whole.rows, "{name}: {path} by {limit}");
                assert!(
                    pages
                        .iter()
                        .all(|page| page.rows.len() <= limit && page.head_blob == whole.head_blob),
                    "{name}: {path} by {limit}"
                );
            }
        }
        // a change that is no version fills a page too: a page without rows still goes forward
        let rows_of_pages: Vec<usize> = pages(ops, &loc, "churn.md", 1)
            .iter()
            .map(|page| page.rows.len())
            .collect();
        assert_eq!(rows_of_pages, [1, 0, 1, 0, 1], "{name}");
        // no page is empty by request, and past the end there is nothing
        let least = ops.file_versions(&loc, "churn.md", 0, 0).unwrap();
        assert_eq!(least.rows.len(), 1, "{name}");
        assert!(least.next.is_some(), "{name}");
        let past = ops.file_versions(&loc, "churn.md", 10_000, 5).unwrap();
        assert_eq!(
            (past.rows.len(), past.next, past.head_blob),
            (0, None, whole_head(ops, &loc, "churn.md")),
            "{name}"
        );
    });
}

fn whole_head(ops: &dyn GitOps, loc: &RepoLocation, path: &str) -> Option<String> {
    versions(ops, loc, path).head_blob
}

#[test]
fn the_built_in_walk_hands_back_what_it_has_when_its_time_is_up() {
    let fx = Fixture::new();
    fx.keep_line_endings();
    linear_history(&fx, T);
    churned_history(&fx, T + 1000);
    let loc = location(&BuiltinGit, &fx.root);
    for path in ["notes/linear.md", "churn.md", "never.md"] {
        let whole = versions(&BuiltinGit, &loc, path);
        // no time at all: every call still walks on, and the pages join to the same list
        let (mut rows, mut after, mut calls) = (Vec::new(), 0, 0);
        loop {
            let page = builtin::versions(&loc, path, after, 100, Duration::ZERO).unwrap();
            rows.extend(page.rows);
            calls += 1;
            let Some(next) = page.next else { break };
            assert!(next > after, "{path}: {after} -> {next}");
            assert!(calls < 100, "{path}: the walk does not end");
            after = next;
        }
        assert_eq!(rows, whole.rows, "{path}");
        assert!(calls > 3, "{path}: {calls} calls");
    }
}

#[test]
fn a_file_that_was_renamed_names_the_path_it_had_before() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.keep_line_endings();
        let history = renamed_history(&fx, T);
        let loc = location(ops, &fx.root);
        // the list starts at the rename: nothing is followed to the old name, which is told
        let new = versions(ops, &loc, "docs/new ü.md");
        assert_eq!(commits(&new.rows), history.new, "{name}");
        assert_eq!(new.renamed_from.as_deref(), Some("old name.md"), "{name}");
        // the old name keeps its own history, up to the commit before it left
        let old = versions(ops, &loc, "old name.md");
        assert_eq!(commits(&old.rows), history.old, "{name}");
        assert_eq!(
            (old.renamed_from, old.head_blob, old.next),
            (None, None, None),
            "{name}"
        );
        // a page that does not reach the end does not know yet
        let first = ops.file_versions(&loc, "docs/new ü.md", 0, 1).unwrap();
        assert_eq!(commits(&first.rows), history.new[..1], "{name}");
        assert_eq!(first.renamed_from, None, "{name}");
        let next = first.next.expect("a second page");
        let last = ops.file_versions(&loc, "docs/new ü.md", next, 1).unwrap();
        assert_eq!(commits(&last.rows), history.new[1..], "{name}");
        assert_eq!(
            (last.renamed_from.as_deref(), last.next),
            (Some("old name.md"), None),
            "{name}"
        );
        // a file that was just added had no name before
        let fresh = versions(ops, &loc, "docs/fresh.md");
        assert_eq!((fresh.rows.len(), fresh.renamed_from), (1, None), "{name}");
        let moved = versions(ops, &loc, "docs/y.md");
        assert_eq!(
            (moved.rows.len(), moved.renamed_from.as_deref()),
            (1, Some("docs/x.md")),
            "{name}"
        );
        // a project below the repository hears project paths; a name outside the project is not told
        let below = location(ops, &fx.root.join("docs"));
        let moved = versions(ops, &below, "y.md");
        assert_eq!(
            (moved.rows.len(), moved.renamed_from.as_deref()),
            (1, Some("x.md")),
            "{name}"
        );
        let outside = versions(ops, &below, "new ü.md");
        assert_eq!(outside.rows, new.rows, "{name}");
        assert_eq!(outside.renamed_from, None, "{name}");
    });
}

#[test]
fn a_shallow_copy_says_that_its_history_may_be_cut() {
    let fx = Fixture::new();
    fx.keep_line_endings();
    let [third, _, first] = linear_history(&fx, T);
    // the marker alone (naming the first commit cuts nothing off)
    each(|name, ops| {
        let loc = location(ops, &fx.root);
        assert!(!versions(ops, &loc, "notes/linear.md").cut, "{name}");
        let list = fx.repo.path().join("shallow");
        std::fs::write(&list, format!("{first}\n")).unwrap();
        let marked = versions(ops, &loc, "notes/linear.md");
        std::fs::remove_file(&list).unwrap();
        assert!(marked.cut, "{name}");
        assert_eq!(marked.rows.len(), 3, "{name}");
    });
    // a copy that holds the last two commits only, made by a plain git call
    let Some(exe) = find_git() else {
        eprintln!("SKIPPED: no git executable on PATH");
        return;
    };
    let copy = fx.root.with_file_name("shallow ü");
    let made = std::process::Command::new(&exe)
        .current_dir(fx.dir.path())
        .args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--quiet",
            "--no-checkout",
            "--depth",
            "2",
            file_url(&fx.root).as_str(),
        ])
        .arg(&copy)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "no shallow copy: {}",
        String::from_utf8_lossy(&made.stderr)
    );
    let boundary = fx.commit_of(&third).parent_id(0).unwrap().to_string();
    each(|name, ops| {
        let loc = location(ops, &copy);
        let found = versions(ops, &loc, "notes/linear.md");
        assert!(found.cut, "{name}");
        // the history ends at the oldest commit of the copy, which looks like the one that added the file
        assert_eq!(
            commits(&found.rows),
            [third.clone(), boundary.clone()],
            "{name}"
        );
        assert_eq!((found.next, found.renamed_from), (None, None), "{name}");
    });
}

#[test]
fn files_with_hostile_names_have_their_versions_listed() {
    let names = hostile_names();
    each(|name, ops| {
        let fx = Fixture::new();
        for (n, file) in names.iter().enumerate() {
            fx.write(file, &format!("{n} then\n"));
        }
        let first = fx.commit_at(T, "Ann", "first");
        for (n, file) in names.iter().enumerate() {
            fx.write(file, &format!("{n} now, longer\n"));
        }
        let second = fx.commit_at(T + 10, "Ann", "second");
        // asked from the repository's folder, and from a project below it
        for root in ["", "{x}"] {
            let loc = location(ops, &fx.root.join(root));
            for file in &names {
                let Some(path) = crate::ops::below_prefix(root, file) else {
                    continue;
                };
                let found = versions(ops, &loc, path);
                assert_eq!(
                    commits(&found.rows),
                    [second.clone(), first.clone()],
                    "{name}: {path:?} below {root:?}"
                );
                assert_eq!(
                    found.head_blob,
                    Some(fx.blob_id(&second, file)),
                    "{name}: {path:?} below {root:?}"
                );
            }
        }
    });
}

#[test]
fn a_file_without_history_has_an_empty_page_and_is_no_failure() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.write("a.md", "a\n");
        fx.write("docs/b.md", "b\n");
        let unborn = location(ops, &fx.root);
        assert_eq!(
            ops.file_versions(&unborn, "a.md", 0, 50).unwrap(),
            FileVersions::default(),
            "{name}: no commit yet"
        );
        let first = fx.commit_at(T, "Ann", "first");
        fx.write("never.md", "not committed\n");
        let loc = location(ops, &fx.root);
        for path in ["never.md", "missing/deep.md", "", "a\0b", "a.md\0"] {
            assert_eq!(
                ops.file_versions(&loc, path, 0, 50).unwrap(),
                FileVersions::default(),
                "{name}: {path:?}"
            );
        }
        // a folder is no file: nothing is listed for it, and its pages end
        assert!(joined(&pages(ops, &loc, "docs", 1)).is_empty(), "{name}");
        assert_eq!(versions(ops, &loc, "docs").head_blob, None, "{name}");
        // a project below the repository asks with project paths
        let below = location(ops, &fx.root.join("docs"));
        assert_eq!(
            commits(&versions(ops, &below, "b.md").rows),
            [first],
            "{name}"
        );
        assert!(
            versions(ops, &below, "docs/b.md").rows.is_empty(),
            "{name}: repository paths are not project paths"
        );
    });
}

#[test]
fn both_backends_list_the_same_versions_page_by_page() {
    let list = backends();
    if list.len() < 2 {
        return;
    }
    let fx = Fixture::new();
    fx.keep_line_endings();
    linear_history(&fx, T);
    merged_history(&fx, T + 1000);
    churned_history(&fx, T + 2000);
    renamed_history(&fx, T + 3000);
    let paths = [
        "notes/linear.md",
        "doc.md",
        "other.md",
        "kept.md",
        "churn.md",
        "old name.md",
        "docs/new ü.md",
        "docs/y.md",
        "docs/fresh.md",
        "docs",
        "never.md",
    ];
    // everything of a page but `next`, which is each backend's own number: only whether there is one
    let seen = |ops: &dyn GitOps, root: &str, path: &str, limit: usize| {
        let loc = location(ops, &fx.root.join(root));
        pages(ops, &loc, path, limit)
            .into_iter()
            .map(|page| FileVersions {
                next: page.next.map(|_| 0),
                ..page
            })
            .collect::<Vec<_>>()
    };
    let mut rows = 0;
    for root in ["", "docs"] {
        for path in paths {
            let Some(path) = crate::ops::below_prefix(root, path) else {
                continue;
            };
            for limit in [1, 3, 50] {
                let (a, b) = (
                    seen(list[0].1.as_ref(), root, path, limit),
                    seen(list[1].1.as_ref(), root, path, limit),
                );
                assert_eq!(a, b, "{path:?} below {root:?} by {limit}");
                rows += joined(&a).len();
            }
        }
    }
    assert!(rows > 60, "the comparison saw versions: {rows}");
}

/// A repository's own settings can make `log` answer another way (follow a rename, hide what the first commit added,
/// print names in another encoding, shorten ids, color its output): the list is the same whatever they say.
#[test]
fn the_settings_of_a_repository_do_not_change_the_list() {
    each(|name, ops| {
        let fx = Fixture::new();
        fx.keep_line_endings();
        let history = renamed_history(&fx, T);
        let loc = location(ops, &fx.root);
        let paths = ["old name.md", "docs/new ü.md", "docs/y.md", "docs/fresh.md"];
        let listed = || paths.map(|path| versions(ops, &loc, path));
        let before = listed();
        let mut config = fx.repo.config().unwrap();
        for (key, value) in [
            ("log.follow", "true"),
            ("log.showRoot", "false"),
            ("log.abbrevCommit", "true"),
            ("log.date", "relative"),
            ("log.decorate", "full"),
            ("core.abbrev", "7"),
            ("core.quotePath", "true"),
            ("diff.renames", "copies"),
            ("diff.noprefix", "true"),
            ("color.ui", "always"),
            ("format.pretty", "oneline"),
            ("i18n.logOutputEncoding", "ISO-8859-1"),
        ] {
            config.set_str(key, value).unwrap();
        }
        assert_eq!(listed(), before, "{name}");
        // the first commit is a version, the list ends at the rename, the names are whole
        assert_eq!(commits(&before[0].rows), history.old, "{name}");
        assert_eq!(commits(&before[1].rows), history.new, "{name}");
        assert_eq!(before[1].rows[0].author, "Ann Ü", "{name}");
        assert_eq!(before[1].rows[0].blob.len(), 40, "{name}");
    });
}

/// Checking a signature starts the program the repository's config names (`gpg.program`): listing the versions of a
/// file never checks one.
#[test]
fn a_signed_commit_is_listed_and_the_signature_program_never_runs() {
    let Some(git) = SystemGit::find() else {
        eprintln!("SKIPPED: no git executable on PATH");
        return;
    };
    let fx = Fixture::new();
    fx.write("a.md", "one\n");
    let first = fx.commit_at(T, "Ann", "first");
    // a commit that carries a signature
    fx.write("a.md", "two, signed\n");
    let tree = fx.staged();
    let who = Signature::new("Ann", "author@example.com", &git2::Time::new(T + 10, 0)).unwrap();
    let content = fx
        .repo
        .commit_create_buffer(&who, &who, "signed", &tree, &[&fx.commit_of(&first)])
        .unwrap();
    let signed = fx
        .repo
        .commit_signed(
            content.as_str().unwrap(),
            "-----BEGIN PGP SIGNATURE-----\n\nnot a signature\n-----END PGP SIGNATURE-----",
            None,
        )
        .unwrap();
    fx.repo
        .reference(&fx.head_ref(), signed, true, "signed")
        .unwrap();
    // the program a check would start leaves a marker
    let marker = fx.dir.path().join("checked.txt");
    let program = fx.dir.path().join("check.sh");
    let script = format!(
        "#!/bin/sh\necho ran > \"{}\"\nexit 1\n",
        marker.to_string_lossy().replace('\\', "/")
    );
    std::fs::write(&program, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut config = fx.repo.config().unwrap();
    config.set_bool("log.showSignature", true).unwrap();
    config
        .set_str("gpg.program", &program.to_string_lossy().replace('\\', "/"))
        .unwrap();
    let loc = location(&git, &fx.root);
    let found = git.file_versions(&loc, "a.md", 0, 50).unwrap();
    assert_eq!(commits(&found.rows), [signed.to_string(), first]);
    assert_eq!(found.rows[0].subject, "signed");
    assert!(
        !marker.exists(),
        "the signature program of the repository ran"
    );
}

#[test]
fn a_backend_that_cannot_read_the_repository_is_a_failure_for_the_reader() {
    each(|name, ops| {
        let dir = tempfile::tempdir().unwrap();
        let folder = dunce::canonicalize(dir.path()).unwrap();
        let nowhere = RepoLocation {
            git_dir: folder.join(".git"),
            workdir: folder,
            prefix: String::new(),
        };
        assert_eq!(
            ops::versions_of(ops, &nowhere, "a.md", 0, 50),
            Err(NoVersions::Failed),
            "{name}"
        );
    });
}

#[test]
fn the_reader_lists_versions_and_tells_a_file_of_a_nested_repository() {
    each(|name, _| {
        let ops: Arc<dyn GitOps> = if name == "system" {
            Arc::new(SystemGit::find().unwrap())
        } else {
            Arc::new(BuiltinGit)
        };
        let fx = Fixture::new();
        fx.write("a.md", "a\n");
        fx.write("docs/b.md", "b\n");
        let first = fx.commit_at(T, "Ann", "first");
        // a repository of its own below the project's
        let inner = fx.root.join("docs").join("inner ü");
        std::fs::create_dir_all(&inner).unwrap();
        let _inner = Repository::init(&inner).unwrap();
        std::fs::write(inner.join("doc.md"), "d\n").unwrap();
        let running = start(&fx.root, &ops, Vec::new());
        wait_until("the first snapshot", || {
            running.service.snapshot().is_some()
        });
        let reader = running.service.reader();
        let found = reader.file_versions("docs/b.md", 0, 50).unwrap();
        assert_eq!(commits(&found.rows), [first], "{name}");
        assert_eq!(
            reader.file_versions("never.md", 0, 50),
            Ok(FileVersions::default()),
            "{name}"
        );
        for path in ["docs/inner ü/doc.md", "docs/inner ü/deep/doc.md"] {
            assert_eq!(
                reader.file_versions(path, 0, 50),
                Err(NoVersions::OtherRepository),
                "{name}: {path}"
            );
        }
    });
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
    // at once: no grandchild (shell, sleep) holds the folder as its working directory. The system may need a few ms
    // after the job is empty to let go of a working directory (verifier: 1 in 1200 at once, fine after 10 ms): a short
    // retry, never more than 200 ms (an orphan that keeps running holds it for 30 s)
    let target = dir.path().join("renamed ü");
    let started = Instant::now();
    loop {
        match std::fs::rename(&folder, &target) {
            Ok(()) => break,
            Err(e) if started.elapsed() < Duration::from_millis(200) => {
                eprintln!("rename retried: {e}");
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("the folder is still held: {e}"),
        }
    }
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

/// A partial copy of a repository names a place to fetch from, and a fetch may run a program the repository's config
/// names: an object that is not on this computer is a miss, and stays one.
#[test]
fn a_file_that_was_not_downloaded_is_a_miss_and_nothing_is_fetched() {
    let Some(exe) = find_git() else {
        eprintln!("SKIPPED: no git executable on PATH");
        return;
    };
    let fx = Fixture::new();
    fx.repo
        .config()
        .unwrap()
        .set_bool("uploadpack.allowFilter", true)
        .unwrap();
    fx.write("a.md", "kept at the source\n");
    let first = commit_id(&fx.commit("first"));
    let blob = fx
        .repo
        .head()
        .unwrap()
        .peel_to_tree()
        .unwrap()
        .get_name("a.md")
        .unwrap()
        .id();
    let copy = fx.root.with_file_name("partial ü");
    let url = file_url(&fx.root);
    // a plain git call makes the copy: the plugin's own runner allows no transport at all
    let made = std::process::Command::new(&exe)
        .current_dir(fx.dir.path())
        .args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--quiet",
            "--no-checkout",
            "--filter=blob:none",
            url.as_str(),
        ])
        .arg(&copy)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "no partial copy: {}",
        String::from_utf8_lossy(&made.stderr)
    );
    // the copy's own config allows the transport: a default policy alone would not stop the fetch
    Repository::open(&copy)
        .unwrap()
        .config()
        .unwrap()
        .set_str("protocol.file.allow", "always")
        .unwrap();
    let downloaded = || Repository::open(&copy).unwrap().odb().unwrap().exists(blob);
    assert!(!downloaded(), "the copy came with the file");
    each(|name, ops| {
        let loc = location(ops, &copy);
        assert_eq!(ops.blob_at(&loc, first, "a.md").unwrap(), None, "{name}");
        assert_eq!(ops.blob_at_head(&loc, "a.md").unwrap(), None, "{name}");
        assert!(!downloaded(), "{name}: the file was fetched");
    });
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
        assert_eq!(
            running
                .service
                .reader()
                .blob_at(commit_id(&"a".repeat(40)), "a.md"),
            Err(NoBlob::Unavailable),
            "{name}"
        );
        assert_eq!(
            running.service.reader().file_versions("a.md", 0, 50),
            Err(NoVersions::Unavailable),
            "{name}"
        );
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

/// A commit made on the worktree's branch is read through either tree, and every miss says why.
fn the_reader_answers_at_a_commit(
    name: &str,
    reader: &dyn RepoReader,
    wt_dir: &Path,
    first: CommitId,
) {
    let wt_repo = Repository::open(wt_dir).unwrap();
    let mut index = wt_repo.index().unwrap();
    index.add_path(Path::new("a.md")).unwrap();
    index.write().unwrap();
    let tree = wt_repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = Signature::now("Tester", "tester@example.com").unwrap();
    let parent = wt_repo.head().unwrap().peel_to_commit().unwrap();
    let second = wt_repo
        .commit(
            Some("HEAD"),
            &sig,
            &sig,
            "in the worktree",
            &tree,
            &[&parent],
        )
        .unwrap();
    let second = commit_id(&second.to_string());
    for path in ["a.md", "@wt/feat/a.md"] {
        assert_eq!(
            reader.blob_at(first, path).as_deref(),
            Ok(b"a\n".as_slice()),
            "{name}: {path}"
        );
        assert_eq!(
            reader.blob_at(second, path).as_deref(),
            Ok(b"changed in the worktree\n".as_slice()),
            "{name}: {path}"
        );
    }
    assert_eq!(
        reader.blob_at_head("@wt/feat/a.md").as_deref(),
        Some(b"changed in the worktree\n".as_slice()),
        "{name}: the worktree's own HEAD"
    );
    assert_eq!(
        reader.blob_at(first, "@wt/nope/a.md"),
        Err(NoBlob::Unavailable),
        "{name}: no such worktree"
    );
    assert_eq!(
        reader.blob_at(first, "@wt/feat"),
        Err(NoBlob::Unavailable),
        "{name}"
    );
    assert_eq!(
        reader.blob_at(first, "missing.md"),
        Err(NoBlob::Missing),
        "{name}"
    );
    assert_eq!(
        reader.blob_at(first, "@wt/feat/only there.md"),
        Err(NoBlob::Missing),
        "{name}: never committed"
    );
    assert_eq!(
        reader.blob_at(commit_id(&"0".repeat(40)), "a.md"),
        Err(NoBlob::Missing),
        "{name}: no such commit"
    );
    // each tree lists the history of its own HEAD
    let versions = |path: &str| -> Vec<CommitId> {
        let found = reader.file_versions(path, 0, 50).unwrap();
        found.rows.iter().map(|row| row.commit).collect()
    };
    assert_eq!(versions("a.md"), [first], "{name}");
    assert_eq!(versions("@wt/feat/a.md"), [second, first], "{name}");
    assert!(versions("@wt/feat/only there.md").is_empty(), "{name}");
    assert_eq!(
        reader.file_versions("@wt/nope/a.md", 0, 50),
        Err(NoVersions::Unavailable),
        "{name}: no such worktree"
    );
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
        let first = commit_id(&fx.commit("first"));
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
        the_reader_answers_at_a_commit(name, reader.as_ref(), &wt_dir, first);
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
