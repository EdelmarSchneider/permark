//! Round 3 seams of the projects service (`projects.md` §8, §4.4, §9.1): map listeners, first commits written once,
//! the move question by first commit (+ name) | card id (never a silent re-attach found at open, D31; never a refused
//! folder, D28), the profile state folder following a moved member, member stores. A fake root-commit source stands in
//! for the `git` plugin: it answers later, on its own thread, as the plugin's pool does.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::conditions::Distribution;
use crate::events::Recorder;
use crate::paths::Root;
use crate::plugins::PluginRegistry;
use crate::settings::AppSettings;
use crate::state::{AppDirs, ProjectId};
use crate::workspace::{CoreEnv, ProjectCore};

fn env(base: &Path) -> CoreEnv {
    CoreEnv::new(
        AppDirs::under(base),
        Arc::new(PluginRegistry::new(Vec::new()).unwrap()),
        Arc::new(Recorder::default()),
        Distribution::Direct,
        AppSettings::default(),
    )
}

fn wait(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn root_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("root ü");
    fs::create_dir_all(root.join(".permark")).unwrap();
    (dir, root)
}

fn repo(root: &Path, rel: &str) {
    fs::create_dir_all(root.join(rel).join(".git")).unwrap();
}

fn record(root: &Path, text: &str) {
    fs::write(root.join(".permark/projects.toml"), text).unwrap();
}

fn read_record_text(root: &Path) -> String {
    fs::read_to_string(root.join(".permark/projects.toml")).unwrap()
}

const UID: &str = "k7m2q7xd2a";
const ROOT_A: &str = "0123456789abcdef0123456789abcdef01234567";

/// Root commits by folder name (the fake git): `beta*` folders share `ROOT_A`. Answers on another thread after
/// `delay` per folder; counts folders asked.
fn source_with(
    calls: &Arc<AtomicUsize>,
    projects: &Arc<Projects>,
    delay: Duration,
) -> RootCommitAsk {
    let calls = Arc::clone(calls);
    let weak = Arc::downgrade(projects);
    Arc::new(move |folders: Vec<(String, PathBuf)>| {
        let calls = Arc::clone(&calls);
        let weak = weak.clone();
        std::thread::spawn(move || {
            let answers: Vec<(String, Option<String>)> = folders
                .into_iter()
                .map(|(key, folder)| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(delay);
                    let name = folder
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (key, name.starts_with("beta").then(|| ROOT_A.to_owned()))
                })
                .collect();
            if let Some(projects) = weak.upgrade() {
                projects.root_commits_learned(&answers);
            }
        });
    })
}

fn source(calls: &Arc<AtomicUsize>, projects: &Arc<Projects>) -> RootCommitAsk {
    source_with(calls, projects, Duration::ZERO)
}

#[test]
fn map_listeners_hear_every_change_until_dropped() {
    let (_dir, root) = root_dir();
    repo(&root, "hubs/a");
    record(
        &root,
        &format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"hubs/a\"\n"),
    );
    let base = tempfile::tempdir().unwrap();
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    let heard: Arc<Mutex<Vec<usize>>> = Arc::default();
    let sink = Arc::clone(&heard);
    let lease = core
        .projects()
        .subscribe_map(move |map| sink.lock().unwrap().push(map.members().len()));
    core.projects()
        .leave_out(&[ProjectUid::parse(UID).unwrap()])
        .unwrap();
    assert_eq!(heard.lock().unwrap().last(), Some(&0));
    drop(lease);
    let n = heard.lock().unwrap().len();
    core.projects()
        .include(&ProjectPick::All, &|| Ok(()))
        .unwrap();
    assert_eq!(
        heard.lock().unwrap().len(),
        n,
        "nothing after the lease went"
    );
}

#[test]
fn first_commits_are_written_once_per_entry_surgically() {
    let (_dir, root) = root_dir();
    repo(&root, "hubs/a");
    repo(&root, "hubs/b");
    let text = format!(
        "# mine\nversion = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"hubs/a\"\nnote = \"kept\"\n\n[[project]]\nid = \"abcdefghij\"\npath = \"hubs/b\"\nfirst_commit = \"89412c0d\"\n"
    );
    record(&root, &text.replace('\n', "\r\n"));
    let base = tempfile::tempdir().unwrap();
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    let a = ProjectUid::parse(UID).unwrap();
    let b = ProjectUid::parse("abcdefghij").unwrap();
    core.projects()
        .set_first_commits(&[
            (a.clone(), ROOT_A.to_uppercase()),
            (b.clone(), ROOT_A.to_owned()),
            (a.clone(), "ffff".to_owned()),
        ])
        .unwrap();
    let after = read_record_text(&root);
    assert!(
        after.contains(&format!("first_commit = \"{ROOT_A}\"\r\n")),
        "{after}"
    );
    assert!(
        after.contains("first_commit = \"89412c0d\""),
        "an entry that has one keeps it"
    );
    assert_eq!(after.matches("first_commit").count(), 2);
    assert!(after.starts_with("# mine\r\n") && after.contains("note = \"kept\"\r\n"));
    let map = core.projects().snapshot();
    assert_eq!(
        map.member(&a).unwrap().first_commit.as_deref(),
        Some(ROOT_A)
    );
    // again: nothing written
    let stamp = fs::metadata(root.join(".permark/projects.toml"))
        .unwrap()
        .modified()
        .unwrap();
    core.projects()
        .set_first_commits(&[(a, "abcd".to_owned())])
        .unwrap();
    assert_eq!(
        fs::metadata(root.join(".permark/projects.toml"))
            .unwrap()
            .modified()
            .unwrap(),
        stamp
    );
    assert_eq!(read_record_text(&root), after);
}

#[test]
fn moved_while_closed_is_asked_by_first_commit_and_name_and_its_state_follows_the_yes() {
    let (dir, root) = root_dir();
    repo(&root, "inner/beta");
    record(
        &root,
        &format!(
            "version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"beta\"\nfirst_commit = \"{}\"\n",
            &ROOT_A[..12]
        ),
    );
    // state the member kept in the profile under its old path
    let base = tempfile::tempdir().unwrap();
    let dirs = AppDirs::under(base.path());
    let old_id = crate::fsx::hash(
        crate::paths::RootKey::of(&root.join("beta"))
            .as_str()
            .as_bytes(),
    )
    .hex();
    let old_dir = dirs.profile.join("projects").join(&old_id);
    fs::create_dir_all(old_dir.join("state/plugins/git")).unwrap();
    fs::write(
        old_dir.join("state/plugins/git/visit.json"),
        "{\"version\":1,\"data\":{\"head\":null}}",
    )
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
    let lease = core
        .projects()
        .provide_root_commits(source(&calls, core.projects()));
    // D31: a match found at open is a question (clones share the first commit), never a silent re-attach
    wait("the question", || core.projects_view().ask.is_some());
    let ask = core.projects_view().ask.unwrap();
    assert_eq!(
        (ask.was.as_str(), ask.candidate.as_str()),
        ("beta", "inner/beta")
    );
    assert!(read_record_text(&root).contains("path = \"beta\""));
    let heard: Arc<Mutex<Vec<ProjectEvent>>> = Arc::default();
    let sink = Arc::clone(&heard);
    let _news = core
        .projects()
        .subscribe(move |e| sink.lock().unwrap().push(e.clone()));
    core.projects()
        .answer_move(&ask.uid, &ask.candidate, true)
        .unwrap();
    assert!(read_record_text(&root).contains("path = \"inner/beta\""));
    assert!(core.projects_view().ask.is_none());
    assert!(
        heard
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, ProjectEvent::Back { path, .. } if path == "inner/beta")),
        "Changes row back"
    );
    let new_id = ProjectId::of(&Root::open(&root.join("inner/beta")).unwrap());
    let new_dir = dirs.profile.join("projects").join(new_id.as_str());
    assert!(
        new_dir.join("state/plugins/git/visit.json").is_file(),
        "state followed the move"
    );
    assert!(!old_dir.exists());
    assert!(core
        .projects()
        .store_of(&crate::paths::VaultPath::parse("inner/beta").unwrap())
        .is_some());
    assert!(core
        .projects()
        .store_of(&crate::paths::VaultPath::parse("beta").unwrap())
        .is_none());
    drop(lease);
    drop(core);
    drop(dir);
}

#[test]
fn renamed_while_closed_with_the_same_first_commit_asks_and_lookalikes_never_attach() {
    let (_dir, root) = root_dir();
    repo(&root, "beta2");
    repo(&root, "other");
    record(
        &root,
        &format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"beta\"\nfirst_commit = \"{ROOT_A}\"\n"),
    );
    let base = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
    let _lease = core
        .projects()
        .provide_root_commits(source(&calls, core.projects()));
    wait("the question", || core.projects_view().ask.is_some());
    let ask = core.projects_view().ask.unwrap();
    assert_eq!(
        (ask.was.as_str(), ask.candidate.as_str()),
        ("beta", "beta2")
    );
    assert!(
        read_record_text(&root).contains("path = \"beta\""),
        "nothing written before the answer"
    );
    // asked once per found folder this session
    let asked = calls.load(Ordering::SeqCst);
    assert!(asked <= 2, "{asked}");
    core.projects()
        .answer_move(&ProjectUid::parse(UID).unwrap(), "beta2", true)
        .unwrap();
    assert!(read_record_text(&root).contains("path = \"beta2\""));

    // two folders with the same first commit: no question by commit
    let (_dir2, root2) = root_dir();
    repo(&root2, "beta-one");
    repo(&root2, "beta-two");
    record(
        &root2,
        &format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"beta\"\nfirst_commit = \"{ROOT_A}\"\n"),
    );
    let core2 = ProjectCore::open(Root::open(&root2).unwrap(), &env(base.path()));
    let _lease2 = core2
        .projects()
        .provide_root_commits(source(&calls, core2.projects()));
    assert!(core2.projects().wait_ready(Duration::from_secs(10)));
    std::thread::sleep(Duration::from_millis(300));
    assert!(core2.projects_view().ask.is_none());
    assert!(read_record_text(&root2).contains("path = \"beta\""));
}

#[test]
fn the_profile_state_folder_follows_and_merges() {
    let base = tempfile::tempdir().unwrap();
    let dirs = AppDirs::under(base.path());
    let from = base.path().join("a");
    let to = base.path().join("b");
    let id = |p: &Path| crate::fsx::hash(crate::paths::RootKey::of(p).as_str().as_bytes()).hex();
    let old = dirs.profile.join("projects").join(id(&from));
    let new = dirs.profile.join("projects").join(id(&to));
    // nothing kept: nothing moved
    assert!(!crate::state::move_profile_state(&dirs, &from, &to).unwrap());
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("x.json"), "old").unwrap();
    fs::write(old.join("y.json"), "old").unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(new.join("x.json"), "new").unwrap();
    assert!(crate::state::move_profile_state(&dirs, &from, &to).unwrap());
    assert_eq!(
        fs::read_to_string(new.join("x.json")).unwrap(),
        "new",
        "the target wins"
    );
    assert_eq!(fs::read_to_string(new.join("y.json")).unwrap(), "old");
    assert!(!old.exists());
    // same id (case-only rename where the disk ignores case): nothing done
    let upper = base.path().join("B");
    if crate::paths::CASE_INSENSITIVE {
        assert!(!crate::state::move_profile_state(&dirs, &to, &upper).unwrap());
        assert!(new.exists());
    }
}

// ---- verifier (round 3): clones share a first commit -------------------------------------------------------------

fn row_state(core: &ProjectCore, path: &str) -> Option<crate::projects::view::MemberState> {
    core.projects_view()
        .rows
        .iter()
        .find(|r| r.path == path)
        .map(|r| r.state)
}

/// The person said No to a clone (`archive/beta`); the included `beta` is deleted while closed. Step 3 must never
/// attach the entry to the refused clone (D28: the person's no beats; nothing re-points the record to it silently).
#[test]
fn verifier_a_declined_clone_is_never_attached_by_first_commit() {
    let (_dir, root) = root_dir();
    repo(&root, "beta");
    repo(&root, "archive/beta");
    record(
        &root,
        &format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"beta\"\nfirst_commit = \"{ROOT_A}\"\n"),
    );
    let base = tempfile::tempdir().unwrap();
    {
        let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
        assert!(core.projects().wait_ready(Duration::from_secs(10)));
        core.projects()
            .decline(&ProjectPick::Paths {
                paths: vec!["archive/beta".to_owned()],
            })
            .unwrap();
        assert_eq!(
            row_state(&core, "archive/beta"),
            Some(crate::projects::view::MemberState::Declined)
        );
    }
    fs::remove_dir_all(root.join("beta")).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    // still refused after the reopen, before git can say anything
    assert_eq!(
        row_state(&core, "archive/beta"),
        Some(crate::projects::view::MemberState::Declined)
    );
    let _lease = core
        .projects()
        .provide_root_commits(source(&calls, core.projects()));
    std::thread::sleep(Duration::from_millis(800));
    let text = read_record_text(&root);
    eprintln!(
        "record after reopen:\n{text}\nrows: {:?}\nroot-commit calls: {}",
        core.projects_view()
            .rows
            .iter()
            .map(|r| (r.path.clone(), r.state))
            .collect::<Vec<_>>(),
        calls.load(Ordering::SeqCst)
    );
    assert!(
        text.contains("path = \"beta\""),
        "record re-pointed at the declined clone:\n{text}"
    );
    assert_ne!(
        row_state(&core, "archive/beta"),
        Some(crate::projects::view::MemberState::Included)
    );
    assert!(
        core.projects_view().ask.is_none(),
        "a refused folder is never even a candidate"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0, "nothing looked up for it");
}

/// The included `beta` is deleted (not moved) while closed; another clone with the same folder name that the person
/// never included sits elsewhere. D31: the person is asked; a No leaves everything as it was.
#[test]
fn verifier_a_never_included_clone_after_the_original_was_deleted() {
    let (_dir, root) = root_dir();
    repo(&root, "beta");
    repo(&root, "archive/beta");
    record(
        &root,
        &format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"beta\"\nfirst_commit = \"{ROOT_A}\"\n"),
    );
    let base = tempfile::tempdir().unwrap();
    fs::remove_dir_all(root.join("beta")).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
    let _lease = core
        .projects()
        .provide_root_commits(source(&calls, core.projects()));
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    wait("the question", || core.projects_view().ask.is_some());
    let ask = core.projects_view().ask.unwrap();
    assert_eq!(
        (ask.was.as_str(), ask.candidate.as_str()),
        ("beta", "archive/beta")
    );
    assert!(
        read_record_text(&root).contains("path = \"beta\""),
        "nothing silent"
    );
    core.projects()
        .answer_move(&ask.uid, &ask.candidate, false)
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(core.projects_view().ask.is_none(), "not asked again");
    assert!(read_record_text(&root).contains("path = \"beta\""));
    assert_eq!(
        row_state(&core, "archive/beta"),
        Some(crate::projects::view::MemberState::Offered)
    );
}

/// S2: root commits are asked only after the first map, never waited for: a slow git per unlisted repository never
/// delays `ready` (agent `ERR busy`, Explorer nodes); each folder asked once; nothing after close.
#[test]
fn verifier_learning_root_commits_never_delays_the_first_map() {
    let (_dir, root) = root_dir();
    for n in 0..10 {
        repo(&root, &format!("x{n}"));
    }
    record(
        &root,
        &format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"gone\"\nfirst_commit = \"{ROOT_A}\"\n"),
    );
    let base = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let asked_before_ready = Arc::new(AtomicUsize::new(0));
    let start = Instant::now();
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
    let inner = source_with(&calls, core.projects(), Duration::from_millis(500));
    let weak = Arc::downgrade(core.projects());
    let early = Arc::clone(&asked_before_ready);
    let lease = core
        .projects()
        .provide_root_commits(Arc::new(move |folders| {
            if weak.upgrade().is_some_and(|p| !p.snapshot().ready) {
                early.fetch_add(folders.len(), Ordering::SeqCst);
            }
            inner(folders);
        }));
    assert!(core.projects().wait_ready(Duration::from_secs(15)));
    let ready_after = start.elapsed();
    // 10 folders x 500 ms = 5 s if the map waited for git
    assert!(
        ready_after < Duration::from_secs(3),
        "ready after {ready_after:?}"
    );
    wait("every folder asked", || calls.load(Ordering::SeqCst) == 10);
    assert_eq!(
        asked_before_ready.load(Ordering::SeqCst),
        0,
        "asked before the map"
    );
    // a later pass asks nothing again
    core.projects().watch(&crate::watch::FsBatch {
        rescan: true,
        ..crate::watch::FsBatch::default()
    });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(calls.load(Ordering::SeqCst), 10, "once per folder");
    // closing: late answers change nothing
    drop(lease);
    core.projects().stop();
    core.projects()
        .root_commits_learned(&[("x0".to_owned(), Some(ROOT_A.to_owned()))]);
    assert!(core.projects_view().ask.is_none());
}

/// Same class, step 2 (round 2 code): a declined clone carrying the same card id (`.permark/project.toml` committed
/// in the repository, so every clone has it).
#[test]
fn verifier_a_declined_clone_with_the_same_card_id_is_never_attached() {
    let (_dir, root) = root_dir();
    for rel in ["beta", "archive/beta"] {
        fs::create_dir_all(root.join(rel).join(".permark")).unwrap();
        fs::write(
            root.join(rel).join(".permark/project.toml"),
            format!("version = 1\nid = \"{UID}\"\n"),
        )
        .unwrap();
    }
    record(
        &root,
        &format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"beta\"\n"),
    );
    let base = tempfile::tempdir().unwrap();
    {
        let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
        assert!(core.projects().wait_ready(Duration::from_secs(10)));
        core.projects()
            .decline(&ProjectPick::Paths {
                paths: vec!["archive/beta".to_owned()],
            })
            .unwrap();
    }
    fs::remove_dir_all(root.join("beta")).unwrap();
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    std::thread::sleep(Duration::from_millis(500));
    let text = read_record_text(&root);
    assert!(text.contains("path = \"beta\""), "{text}");
    assert_eq!(
        row_state(&core, "archive/beta"),
        Some(crate::projects::view::MemberState::Declined)
    );
    assert!(core.projects_view().ask.is_none(), "never a candidate");
}

/// D31: clones carrying the same card id, original deleted while closed: asked one candidate at a time, the next
/// after a No; nothing written without a Yes.
#[test]
fn clones_with_the_same_card_id_are_asked_one_at_a_time() {
    let (_dir, root) = root_dir();
    for rel in ["archive/beta", "backup/beta"] {
        fs::create_dir_all(root.join(rel).join(".permark")).unwrap();
        fs::write(
            root.join(rel).join(".permark/project.toml"),
            format!("version = 1\nid = \"{UID}\"\n"),
        )
        .unwrap();
    }
    record(
        &root,
        &format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"beta\"\n"),
    );
    let base = tempfile::tempdir().unwrap();
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env(base.path()));
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    let first = core.projects_view().ask.expect("asked");
    assert_eq!(first.candidate, "archive/beta");
    core.projects()
        .answer_move(&first.uid, &first.candidate, false)
        .unwrap();
    let second = core.projects_view().ask.expect("the next one");
    assert_eq!(second.candidate, "backup/beta");
    core.projects()
        .answer_move(&second.uid, &second.candidate, false)
        .unwrap();
    assert!(core.projects_view().ask.is_none());
    assert!(read_record_text(&root).contains("path = \"beta\""));
}
