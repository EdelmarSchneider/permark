//! Round 3 on real repositories (`projects.md` §8), both backends where `git` exists: ahead | behind, last commit,
//! root commit, gitfile members, the root status without member files, the member pool against real repositories,
//! and the plugin in a `ProjectCore` with an included project (repos, interest, snapshot, review reader, first
//! commit written to the record, leave out).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use git2::{IndexAddOption, Repository, Signature};
use permark_core::conditions::Distribution;
use permark_core::events::NoEvents;
use permark_core::paths::Root;
use permark_core::plugins::{PluginId, PluginRegistry, ProjectFacts, RepoHandle, RpcCall};
use permark_core::projects::ProjectUid;
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};

use super::*;
use crate::ops::{Baseline, Discovery, GitOps};
use crate::service::{BackendChoice, ServiceArgs, Timings};

fn backends() -> Vec<(&'static str, Arc<dyn GitOps>)> {
    let mut list: Vec<(&'static str, Arc<dyn GitOps>)> = vec![("builtin", Arc::new(BuiltinGit))];
    if let Some(system) = SystemGit::find() {
        list.push(("system", Arc::new(system)));
    } else {
        eprintln!("SKIPPED: no git executable on PATH, the System backend cases are not run");
    }
    list
}

fn sig() -> Signature<'static> {
    Signature::now("Tester", "tester@example.com").unwrap()
}

/// Stages everything and commits on HEAD; returns the id.
fn commit_all(repo: &Repository, message: &str) -> git2::Oid {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .unwrap();
    index.update_all(["*"].iter(), None).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
    repo.commit(Some("HEAD"), &sig(), &sig(), message, &tree, &parents)
        .unwrap()
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn location_of(folder: &Path) -> RepoLocation {
    member_location(folder).unwrap()
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A temp folder with a Unicode + space name, canonical.
fn base() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("raíz é");
    std::fs::create_dir_all(&root).unwrap();
    (dir, root)
}

#[test]
fn ahead_behind_against_the_upstream_as_last_fetched() {
    let (_dir, root) = base();
    let repo = Repository::init(&root).unwrap();
    write(&root, "a.md", "a\n");
    let first = commit_all(&repo, "first");
    let branch = repo.head().unwrap().shorthand().unwrap().to_owned();
    // no upstream yet
    for (name, ops) in backends() {
        assert_eq!(
            ops.ahead_behind(&location_of(&root)).unwrap(),
            None,
            "{name}"
        );
    }
    repo.remote("origin", "https://example.invalid/r.git")
        .unwrap();
    repo.reference(
        &format!("refs/remotes/origin/{branch}"),
        first,
        true,
        "fetched",
    )
    .unwrap();
    let mut config = repo.config().unwrap();
    config
        .set_str(&format!("branch.{branch}.remote"), "origin")
        .unwrap();
    config
        .set_str(
            &format!("branch.{branch}.merge"),
            &format!("refs/heads/{branch}"),
        )
        .unwrap();
    write(&root, "b.md", "b\n");
    commit_all(&repo, "second");
    write(&root, "c.md", "c\n");
    let third = commit_all(&repo, "third\n\nbody line");
    // the upstream got one commit of its own (a side commit on top of `first`)
    let tree = repo.find_commit(first).unwrap().tree().unwrap();
    let side = repo
        .commit(
            None,
            &sig(),
            &sig(),
            "side",
            &tree,
            &[&repo.find_commit(first).unwrap()],
        )
        .unwrap();
    repo.reference(
        &format!("refs/remotes/origin/{branch}"),
        side,
        true,
        "fetched",
    )
    .unwrap();
    for (name, ops) in backends() {
        let loc = location_of(&root);
        assert_eq!(ops.ahead_behind(&loc).unwrap(), Some((2, 1)), "{name}");
        let (at, subject) = ops.last_commit(&loc).unwrap().unwrap();
        assert_eq!(subject, "third", "{name}");
        assert!(at > 1_600_000_000_000, "{name}: ms");
        assert_eq!(
            ops.root_commit(&loc).unwrap(),
            Some(first.to_string()),
            "{name}"
        );
    }
    // detached: no upstream to compare with
    repo.set_head_detached(third).unwrap();
    for (name, ops) in backends() {
        assert_eq!(
            ops.ahead_behind(&location_of(&root)).unwrap(),
            None,
            "{name}"
        );
    }
}

#[test]
fn unborn_repositories_answer_nothing_and_several_roots_give_the_smallest() {
    let (_dir, root) = base();
    let repo = Repository::init(&root).unwrap();
    for (name, ops) in backends() {
        let loc = location_of(&root);
        assert_eq!(ops.last_commit(&loc).unwrap(), None, "{name}");
        assert_eq!(ops.root_commit(&loc).unwrap(), None, "{name}");
        assert_eq!(ops.ahead_behind(&loc).unwrap(), None, "{name}");
    }
    write(&root, "a.md", "a\n");
    let one = commit_all(&repo, "one");
    // an orphan history merged in: two roots
    write(&root, "b.md", "b\n");
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let orphan = repo
        .commit(None, &sig(), &sig(), "orphan", &tree, &[])
        .unwrap();
    let merge = repo
        .commit(
            Some("HEAD"),
            &sig(),
            &sig(),
            "merge",
            &tree,
            &[
                &repo.find_commit(one).unwrap(),
                &repo.find_commit(orphan).unwrap(),
            ],
        )
        .unwrap();
    let smallest = one.min(orphan).to_string();
    for (name, ops) in backends() {
        let loc = location_of(&root);
        assert_eq!(
            ops.root_commit(&loc).unwrap(),
            Some(smallest.clone()),
            "{name}"
        );
        assert_eq!(ops.last_commit(&loc).unwrap().unwrap().1, "merge", "{name}");
    }
    let facts = cheap_facts(&root.join(".git")).unwrap();
    assert_eq!(
        facts.head.oid,
        Some(merge.to_string()),
        "cheap HEAD = git's"
    );
}

#[test]
fn a_linked_worktree_member_works_through_its_gitfile() {
    let (dir, root) = base();
    let repo = Repository::init(&root).unwrap();
    write(&root, "a.md", "a\n");
    commit_all(&repo, "first");
    let linked = dunce::canonicalize(dir.path()).unwrap().join("linked wt");
    repo.worktree("side", &linked, None).unwrap();
    write(&linked, "new.md", "n\n");
    let loc = location_of(&linked);
    assert!(
        loc.git_dir.ends_with(Path::new("worktrees").join("side")),
        "{loc:?}"
    );
    let facts = cheap_facts(&loc.git_dir).unwrap();
    assert_eq!(facts.head.branch.as_deref(), Some("side"));
    assert!(!facts.head.unborn, "refs read from the common folder");
    for (name, ops) in backends() {
        let status = ops.status(&loc, &Baseline::Head).unwrap();
        let paths: Vec<&str> = status.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["new.md"], "{name}");
        assert_eq!(status.head.branch.as_deref(), Some("side"), "{name}");
    }
}

#[test]
fn the_root_status_leaves_out_included_projects_with_their_own_repository() {
    let (_dir, root) = base();
    let outer = Repository::init(&root).unwrap();
    write(&root, "top.md", "t\n");
    commit_all(&outer, "outer");
    let member = root.join("hubs").join("a b");
    let inner = Repository::init(&member).unwrap();
    write(&member, "in.md", "i\n");
    commit_all(&inner, "inner");
    write(&member, "in.md", "changed\n");
    write(&root, "top.md", "changed\n");
    write(&root, "hubs/loose.md", "l\n");
    for (name, ops) in backends() {
        let snapshots: Arc<Mutex<Vec<Option<GitSnapshot>>>> = Arc::default();
        let heard = Arc::clone(&snapshots);
        let service = GitService::start(ServiceArgs {
            root: root.clone(),
            worktrees: Vec::new(),
            choice: BackendChoice::Fixed(Arc::clone(&ops)),
            home: None,
            timings: Timings {
                quiet: Duration::from_millis(20),
                max_wait: Duration::from_millis(100),
                lock_wait: Duration::from_millis(100),
            },
            facts: ProjectFacts::new(None),
            announce: Arc::new(move |s| lock_vec(&heard).push(s.cloned())),
            prefs: None,
            visit: None,
        });
        let paths = |service: &GitService| -> Vec<String> {
            service
                .snapshot()
                .map(|s| s.files.iter().map(|f| f.path.clone()).collect())
                .unwrap_or_default()
        };
        wait_until("first snapshot", || service.snapshot().is_some());
        let before = paths(&service);
        assert!(
            before.iter().any(|p| p.starts_with("hubs/a b")),
            "{name}: nested repo listed before: {before:?}"
        );
        service.set_excluded(vec![permark_core::paths::key_of("hubs/a b")]);
        wait_until("member left out", || {
            !paths(&service).iter().any(|p| p.starts_with("hubs/a b"))
        });
        let after = paths(&service);
        assert!(after.contains(&"top.md".to_owned()), "{name}: {after:?}");
        assert!(
            after.contains(&"hubs/loose.md".to_owned()),
            "{name}: {after:?}"
        );
        // a sibling with a longer name is not the member
        assert!(!excluded_for_tests(&["hubs/a b"], "hubs/a bc/x.md"));
        assert!(excluded_for_tests(&["hubs/a b"], "hubs/a b/"));
        assert!(excluded_for_tests(&["hubs/a b"], "hubs/a b/x/y.md"));
    }
}

fn lock_vec<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn excluded_for_tests(keys: &[&str], path: &str) -> bool {
    let keys: Vec<String> = keys
        .iter()
        .map(|k| permark_core::paths::key_of(k))
        .collect();
    crate::service::excluded_path(&keys, path)
}

/// Env for the pool against a real backend.
fn real_env(ops: Arc<dyn GitOps>) -> Env {
    Env {
        backends: Arc::new(BackendCell::new(BackendChoice::Fixed(ops))),
        timings: Timings {
            quiet: Duration::from_millis(20),
            max_wait: Duration::from_millis(100),
            lock_wait: Duration::from_millis(100),
        },
        on_repos: Arc::new(|_| {}),
        on_snapshot: Arc::new(|_| {}),
        baseline: Arc::new(|| BaselineKind::Head),
        exclude: Arc::new(|_| {}),
        visit_file: Arc::new(|_| None),
        first_commits: Arc::new(|_| {}),
        announce_after: Duration::from_millis(10),
        first_commit_after: Duration::from_millis(10),
    }
}

fn member(uid: &str, rel: &str, folder: &Path) -> MemberRef {
    MemberRef {
        uid: uid.to_owned(),
        path: rel.to_owned(),
        key: permark_core::paths::key_of(rel),
        folder: folder.to_path_buf(),
        first_commit: false,
    }
}

#[test]
fn the_pool_on_real_repositories_counts_caps_and_survives_a_deleted_repository() {
    let (_dir, root) = base();
    let big = root.join("big one");
    let repo = Repository::init(&big).unwrap();
    write(&big, "a.md", "a\n");
    commit_all(&repo, "first");
    for n in 0..5001 {
        write(&big, &format!("many/f{n:05}.txt"), "x");
    }
    let gone = root.join("gone");
    let repo2 = Repository::init(&gone).unwrap();
    write(&gone, "a.md", "a\n");
    commit_all(&repo2, "first");
    drop(repo2);
    for (name, ops) in backends() {
        let members = Members::new(real_env(Arc::clone(&ops)));
        members.sync(&[
            member("ubig", "big one", &big),
            member("ugone", "gone", &gone),
        ]);
        members.interest("w", InterestLevel::Glance, &["ubig".to_owned()]);
        wait_until("the big status", || {
            members
                .summaries()
                .iter()
                .any(|s| s.id == "ubig" && s.changed.is_some() && !s.pending)
        });
        let s = members
            .summaries()
            .into_iter()
            .find(|s| s.id == "ubig")
            .unwrap();
        assert_eq!(
            s.changed,
            Some(u32::try_from(crate::ops::MAX_FILES).unwrap()),
            "{name}: cut at the cap"
        );
        assert!(members.snapshot("ubig").unwrap().truncated, "{name}");
        // the repository is deleted under the pool: a row error (or a quiet answer), never a hang | panic
        let _ = std::fs::remove_dir_all(gone.join(".git"));
        members.interest("w", InterestLevel::Detail, &["ugone".to_owned()]);
        wait_until("the gone one answered", || {
            members
                .summaries()
                .iter()
                .any(|s| s.id == "ugone" && !s.pending)
        });
        let g = members
            .summaries()
            .into_iter()
            .find(|s| s.id == "ugone")
            .unwrap();
        assert!(g.error.is_some(), "{name}: {g:?}");
        drop(members);
        // restore for the next backend
        let repo2 = Repository::init(&gone).unwrap();
        commit_all(&repo2, "again");
    }
}

// ---- the plugin in a project with an included project ------------------------------------------------------------

fn env_for(base: &Path) -> CoreEnv {
    CoreEnv::new(
        AppDirs::under(base),
        Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
        Arc::new(NoEvents),
        Distribution::Direct,
        AppSettings {
            plugins: BTreeMap::from([(PluginId::of("git"), true)]),
            ..AppSettings::default()
        },
    )
}

fn call(core: &ProjectCore, method: &str, args: serde_json::Value) -> serde_json::Value {
    core.rpc()
        .call(
            "git",
            method,
            &RpcCall {
                window: "w".to_owned(),
                args,
            },
        )
        .unwrap()
}

#[test]
fn an_included_project_has_its_own_repository_in_the_root_tab() {
    let (_dir, root) = base();
    let outer = Repository::init(&root).unwrap();
    write(&root, "top.md", "t\n");
    write(&root, ".gitignore", "hubs/\n");
    commit_all(&outer, "outer");
    let folder = root.join("hubs").join("hub ü");
    let inner = Repository::init(&folder).unwrap();
    write(&folder, "doc.md", "committed\n");
    let first = commit_all(&inner, "inner first");
    write(&folder, "doc.md", "edited\n");
    write(
        &root,
        ".permark/projects.toml",
        "version = 1\n\n[[project]]\nid = \"k7m2q7xd2a\"\npath = \"hubs/hub ü\"\n",
    );
    let state = tempfile::tempdir().unwrap();
    let env = env_for(state.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    wait_until("the member repo", || {
        call(&core, "repos", serde_json::Value::Null)
            .as_array()
            .is_some_and(|a| a.len() == 1)
    });
    let repos = call(&core, "repos", serde_json::Value::Null);
    assert_eq!(repos[0]["id"], "k7m2q7xd2a");
    assert_eq!(repos[0]["path"], "hubs/hub ü");
    assert!(
        repos[0]["branch"].is_string(),
        "cheap facts at once: {repos}"
    );
    // no list before a view asks
    assert!(call(
        &core,
        "snapshot",
        serde_json::json!({"project": "k7m2q7xd2a"})
    )
    .is_null());
    call(
        &core,
        "interest",
        serde_json::json!({"ids": ["k7m2q7xd2a"], "level": "detail"}),
    );
    wait_until("the member snapshot", || {
        !call(
            &core,
            "snapshot",
            serde_json::json!({"project": "k7m2q7xd2a"}),
        )
        .is_null()
    });
    let snapshot = call(
        &core,
        "snapshot",
        serde_json::json!({"project": "k7m2q7xd2a"}),
    );
    assert_eq!(
        snapshot["files"][0]["path"], "hubs/hub ü/doc.md",
        "{snapshot}"
    );
    wait_until("detail", || {
        call(&core, "repos", serde_json::Value::Null)[0]["lastCommit"]["subject"] == "inner first"
    });
    // the root's status never lists the member's files
    wait_until("root snapshot", || {
        !call(&core, "snapshot", serde_json::Value::Null).is_null()
    });
    let root_snapshot = call(&core, "snapshot", serde_json::Value::Null);
    assert!(
        root_snapshot["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| !f["path"].as_str().unwrap().starts_with("hubs/hub ü")),
        "{root_snapshot}"
    );
    // a change inside the member: its own count follows (the watch routes it to the member repository)
    write(&folder, "new note.md", "n\n");
    wait_until("member count follows", || {
        call(&core, "repos", serde_json::Value::Null)[0]["changed"] == 2
    });
    // review's "last commit" of a member file comes from the member repository
    let reader = core.services().services.get::<RepoHandle>().unwrap();
    assert_eq!(
        reader.0.blob_at_head("hubs/hub ü/doc.md").as_deref(),
        Some(&b"committed\n"[..])
    );
    assert_eq!(
        reader.0.blob_at_head("top.md").as_deref(),
        Some(&b"t\n"[..])
    );
    // the first commit is written to the record (once, batched)
    wait_until("first commit in the record", || {
        std::fs::read_to_string(root.join(".permark/projects.toml"))
            .is_ok_and(|t| t.contains(&first.to_string()))
    });
    // left out: the repository is no longer followed
    core.projects()
        .leave_out(&[ProjectUid::parse("k7m2q7xd2a").unwrap()])
        .unwrap();
    wait_until("member stopped", || {
        call(&core, "repos", serde_json::Value::Null)
            .as_array()
            .is_some_and(Vec::is_empty)
    });
    drop(core);
}

#[test]
fn a_project_renamed_while_closed_is_found_again_by_its_first_commit() {
    let (_dir, root) = base();
    let folder = root.join("beta");
    let inner = Repository::init(&folder).unwrap();
    write(&folder, "doc.md", "d\n");
    let first = commit_all(&inner, "first");
    drop(inner);
    let record = format!(
        "version = 1\n\n[[project]]\nid = \"k7m2q7xd2a\"\npath = \"beta\"\nfirst_commit = \"{first}\"\n"
    );
    write(&root, ".permark/projects.toml", &record);
    // moved while closed (same name, other parent): attached again by first commit + name, no question
    std::fs::create_dir_all(root.join("inner")).unwrap();
    std::fs::rename(&folder, root.join("inner").join("beta")).unwrap();
    let state = tempfile::tempdir().unwrap();
    let env = env_for(state.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    wait_until("re-attached", || {
        std::fs::read_to_string(root.join(".permark/projects.toml"))
            .is_ok_and(|t| t.contains("path = \"inner/beta\""))
    });
    let view = core.projects_view();
    assert!(view.ask.is_none());
    drop(core);
    // renamed while closed (other name): the person is asked
    std::fs::rename(
        root.join("inner").join("beta"),
        root.join("inner").join("beta2"),
    )
    .unwrap();
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    wait_until("the move question", || core.projects_view().ask.is_some());
    let ask = core.projects_view().ask.unwrap();
    assert_eq!(ask.candidate, "inner/beta2");
    assert_eq!(ask.was, "inner/beta");
    drop(core);
}

#[test]
fn route_sends_member_paths_to_their_repository_only() {
    use permark_core::projects::{AgentRules, HiddenRule, MemberInfo, ProjectMap, ProjectMarker};
    use permark_core::watch::{Change, ChangeKind, FsBatch};
    let info = |uid: &str, path: &str, marker| {
        let path = permark_core::paths::VaultPath::parse(path).unwrap();
        MemberInfo {
            uid: ProjectUid::parse(uid).unwrap(),
            key: path.key(),
            path,
            marker,
            hidden: HiddenRule::Own,
            name: String::new(),
            short: String::new(),
            agent_off: false,
            agent: AgentRules::default(),
            first_commit: None,
        }
    };
    let map = ProjectMap::new(
        1,
        true,
        vec![
            info("aaaaaaaaaa", "hubs/a", ProjectMarker::Git),
            info("bbbbbbbbbb", "hubs/a/notes", ProjectMarker::Permark),
            info("cccccccccc", "hubs/a/sub", ProjectMarker::Both),
        ],
        Vec::new(),
        Vec::new(),
    );
    let batch = |paths: &[&str]| FsBatch {
        changes: paths
            .iter()
            .map(|p| Change {
                path: (*p).to_owned(),
                kind: ChangeKind::Modified,
                is_dir: false,
                own: false,
            })
            .collect(),
        ..FsBatch::default()
    };
    let r = route(Some(&map), &batch(&["hubs/a/.git/HEAD"]));
    assert_eq!(
        r,
        (None, vec![("aaaaaaaaaa".to_owned(), true)]),
        "member signal never wakes the root"
    );
    let r = route(
        Some(&map),
        &batch(&[
            "hubs/a/notes/x.md",
            "hubs/a/sub/y.md",
            "hubs/a/sub/.git/index",
        ]),
    );
    assert_eq!(
        r,
        (
            None,
            vec![
                ("aaaaaaaaaa".to_owned(), false),
                ("cccccccccc".to_owned(), true)
            ]
        ),
        "a .permark-only project belongs to the repository above it"
    );
    let r = route(Some(&map), &batch(&["top.md", ".git/HEAD"]));
    assert_eq!(r, (Some(Why::Signal), vec![]));
    let r = route(Some(&map), &batch(&["hubs/ab/x.md"]));
    assert_eq!(r, (Some(Why::Files), vec![]));
    let r = route(None, &batch(&["hubs/a/x.md"]));
    assert_eq!(
        r,
        (Some(Why::Files), vec![]),
        "no projects: the root as before"
    );
    let rescan = FsBatch {
        rescan: true,
        ..FsBatch::default()
    };
    assert_eq!(route(Some(&map), &rescan), (Some(Why::Files), vec![]));
    let _ = Discovery::NoRepo;
}
