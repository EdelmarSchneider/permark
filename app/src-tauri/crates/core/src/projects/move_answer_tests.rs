//! "No, it's new" to the move question (`projects.md` §4.4): kept in the profile (`membership.json` `notMoved`), so
//! a new core (in-place tab switch, restart) does not ask again while the entry stays missing; forgotten once the
//! entry is here again (back at its path, Yes, Locate…). Reported case (real-input 0.7.0 r3/1): `gamma` renamed
//! `g-new` while closed (same first commit), answered No, asked again after the root loaded in another view.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::fsm::{Membership, NotMoved, MEMBERSHIP_FILE};
use super::view::MemberState;
use super::{ProjectUid, Projects, RootCommitAsk};
use crate::conditions::Distribution;
use crate::events::Recorder;
use crate::paths::Root;
use crate::plugins::PluginRegistry;
use crate::settings::AppSettings;
use crate::state::{AppDirs, StateStore};
use crate::workspace::{CoreEnv, ProjectCore};

const UID: &str = "k7m2q7xd2a";
const ROOT_A: &str = "0123456789abcdef0123456789abcdef01234567";

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

/// Root `root ü` (space + non-ASCII) listing `gamma` (with its first commit), which now sits at `g-new`.
fn renamed_while_closed() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("root ü");
    fs::create_dir_all(root.join(".permark")).unwrap();
    fs::create_dir_all(root.join("g-new/.git")).unwrap();
    fs::write(
        root.join(".permark/projects.toml"),
        format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"gamma\"\nfirst_commit = \"{ROOT_A}\"\n"),
    )
    .unwrap();
    (dir, root)
}

/// The fake `git` plugin: every repository has root commit `ROOT_A`; answers on its own thread; counts folders.
fn source(calls: &Arc<AtomicUsize>, projects: &Arc<Projects>) -> RootCommitAsk {
    let calls = Arc::clone(calls);
    let weak = Arc::downgrade(projects);
    Arc::new(move |folders: Vec<(String, PathBuf)>| {
        let calls = Arc::clone(&calls);
        let weak = weak.clone();
        std::thread::spawn(move || {
            let answers: Vec<(String, Option<String>)> = folders
                .into_iter()
                .map(|(key, _)| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    (key, Some(ROOT_A.to_owned()))
                })
                .collect();
            if let Some(projects) = weak.upgrade() {
                projects.root_commits_learned(&answers);
            }
        });
    })
}

/// Opens a core with the fake git; returns it once ready and, when `asked`, once the root commits came back.
fn open(root: &Path, base: &Path, asked: bool) -> (Arc<ProjectCore>, super::ProjectsSubscription) {
    let calls = Arc::new(AtomicUsize::new(0));
    let core = ProjectCore::open(Root::open(root).unwrap(), &env(base));
    let lease = core
        .projects()
        .provide_root_commits(source(&calls, core.projects()));
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    if asked {
        wait("root commits asked", || calls.load(Ordering::SeqCst) > 0);
        // the answer lands on the worker: give it a moment to decide (a question would appear now)
        std::thread::sleep(Duration::from_millis(300));
    }
    (core, lease)
}

fn membership_path(root: &Path, base: &Path) -> PathBuf {
    StateStore::for_project(&AppDirs::under(base), &Root::open(root).unwrap())
        .profile_file::<Membership>(MEMBERSHIP_FILE)
        .path()
        .to_path_buf()
}

fn membership_text(root: &Path, base: &Path) -> String {
    fs::read_to_string(membership_path(root, base)).unwrap_or_default()
}

fn state(core: &ProjectCore, path: &str) -> Option<MemberState> {
    core.projects_view()
        .rows
        .iter()
        .find(|r| r.path == path)
        .map(|r| r.state)
}

fn answer_no(core: &ProjectCore) {
    wait("the question", || core.projects_view().ask.is_some());
    let ask = core.projects_view().ask.unwrap();
    assert_eq!(
        (ask.was.as_str(), ask.candidate.as_str()),
        ("gamma", "g-new")
    );
    core.projects()
        .answer_move(&ask.uid, &ask.candidate, false)
        .unwrap();
}

/// Spec (the reported case): No -> a new core of the same root does not ask again; `g-new` stays a new project
/// (Offered), `gamma` stays Not found; nothing written in the folder.
#[test]
fn a_no_is_remembered_by_a_new_core_of_the_same_root() {
    let (_dir, root) = renamed_while_closed();
    let base = tempfile::tempdir().unwrap();
    let record_before = fs::read(root.join(".permark/projects.toml")).unwrap();
    let (core, lease) = open(&root, base.path(), false);
    answer_no(&core);
    assert!(core.projects_view().ask.is_none());
    assert!(membership_text(&root, base.path()).contains("notMoved"));
    drop(lease);
    drop(core);

    let (core, _lease) = open(&root, base.path(), true);
    assert!(core.projects_view().ask.is_none(), "not asked again");
    assert_eq!(state(&core, "g-new"), Some(MemberState::Offered));
    assert_eq!(state(&core, "gamma"), Some(MemberState::Missing));
    assert_eq!(
        fs::read(root.join(".permark/projects.toml")).unwrap(),
        record_before,
        "record untouched"
    );
}

/// Adversarial: after a No the missing project comes back at its old path -> found again as usual, and the No goes
/// with it: when it later moves away again, the person is asked again.
#[test]
fn a_no_goes_once_the_project_is_back_at_its_path() {
    let (_dir, root) = renamed_while_closed();
    let base = tempfile::tempdir().unwrap();
    let (core, lease) = open(&root, base.path(), false);
    answer_no(&core);
    drop(lease);
    drop(core);

    fs::create_dir_all(root.join("gamma/.git")).unwrap();
    let (core, lease) = open(&root, base.path(), false);
    assert_eq!(state(&core, "gamma"), Some(MemberState::Included));
    assert!(core.projects_view().ask.is_none());
    assert!(
        !membership_text(&root, base.path()).contains("g-new"),
        "the No is forgotten"
    );
    drop(lease);
    drop(core);

    fs::remove_dir_all(root.join("gamma")).unwrap();
    let (core, _lease) = open(&root, base.path(), false);
    wait("asked again", || core.projects_view().ask.is_some());
}

/// Adversarial: after a No the person locates the project at the very folder they said no to -> Locate… works
/// (the No never blocks the person's own choice) and the No goes.
#[test]
fn locate_after_a_no_attaches_the_entry_there() {
    let (_dir, root) = renamed_while_closed();
    let base = tempfile::tempdir().unwrap();
    let (core, lease) = open(&root, base.path(), false);
    answer_no(&core);
    drop(lease);
    drop(core);

    let (core, _lease) = open(&root, base.path(), true);
    assert!(core.projects_view().ask.is_none());
    let refusal = core
        .projects()
        .locate(&ProjectUid::parse(UID).unwrap(), &root.join("g-new"))
        .unwrap();
    assert_eq!(refusal, None);
    assert_eq!(state(&core, "g-new"), Some(MemberState::Included));
    assert!(fs::read_to_string(root.join(".permark/projects.toml"))
        .unwrap()
        .contains("path = \"g-new\""));
    assert!(!membership_text(&root, base.path()).contains("g-new"));
}

/// Saved data (0.7.0 file, no `notMoved`): read as "none answered" (asked as before); the No is added and every
/// other answer in the file stays.
#[test]
fn a_membership_file_of_0_7_0_reads_and_keeps_its_answers() {
    let (_dir, root) = renamed_while_closed();
    let base = tempfile::tempdir().unwrap();
    let path = membership_path(&root, base.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, include_str!("fixtures/membership-0.7.0.json")).unwrap();
    let (core, _lease) = open(&root, base.path(), false);
    answer_no(&core);
    let saved: serde_json::Value =
        serde_json::from_str(&membership_text(&root, base.path())).unwrap();
    assert_eq!(saved["version"], 1);
    assert_eq!(saved["data"]["answer"], "answered");
    assert_eq!(saved["data"]["declined"][0], "tools/notes-app");
    assert_eq!(saved["data"]["notMoved"][0]["uid"], UID);
    assert_eq!(saved["data"]["notMoved"][0]["path"], "g-new");
}

/// Adversarial: a hand-damaged `notMoved` (no object, bad uid) skips those items only: the file is not moved aside,
/// the valid No holds, the other answers stay.
#[test]
fn a_damaged_not_moved_list_skips_the_bad_items_only() {
    let (_dir, root) = renamed_while_closed();
    let base = tempfile::tempdir().unwrap();
    let path = membership_path(&root, base.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!(
            r#"{{"version":1,"data":{{"answer":"answered","declined":["x"],"notMoved":[5,{{"uid":"BAD!","path":"g-new"}},{{"uid":"{UID}","path":"g-new"}}]}}}}"#
        ),
    )
    .unwrap();
    let (core, _lease) = open(&root, base.path(), true);
    assert!(core.projects_view().ask.is_none(), "the valid No holds");
    assert!(path.exists(), "not moved aside");
    let text = membership_text(&root, base.path());
    assert!(text.contains("\"declined\":[\"x\"]") || text.contains("\"x\""));

    // not a list at all: read as none answered
    let m: Membership =
        serde_json::from_str(r#"{"answer":"answered","notMoved":"g-new"}"#).unwrap();
    assert_eq!(m.not_moved, Vec::<NotMoved>::new());
}
