//! Round 4 (`projects.md` §9.2, §5.3 D17): silence marks live with the project that owns the path (its own
//! `quiet.json`, the file it uses opened alone); the closest mark across the chain's files wins; the opened folder's
//! 0.6 marks inside the project are copied once and stay as written there.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::events::NoEvents;
use permark_core::organizing::SilenceApi;
use permark_core::paths::Root;
use permark_core::plugins::{PluginRegistry, RpcCall};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use serde_json::{json, Value};

use super::*;

const MEMBER: &str = "项目 ü";

fn env(base: &Path) -> CoreEnv {
    CoreEnv::new(
        AppDirs::under(base),
        Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
        Arc::new(NoEvents),
        Distribution::Direct,
        AppSettings::default(),
    )
}

fn call(core: &ProjectCore, method: &str, args: Value) -> Value {
    core.rpc()
        .call(
            "quiet",
            method,
            &RpcCall {
                window: "w".to_owned(),
                args,
            },
        )
        .unwrap()
}

fn silenced(core: &ProjectCore, path: &str) -> bool {
    core.services()
        .services
        .get::<SilenceApi>()
        .unwrap()
        .0
        .is_silenced(path)
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

/// A root with `.permark/` holding 0.6 marks, and an included member (own `.permark/`).
fn setup() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("root ü");
    std::fs::create_dir_all(root.join(".permark/local")).unwrap();
    std::fs::create_dir_all(root.join(MEMBER).join(".permark")).unwrap();
    std::fs::write(root.join(MEMBER).join("a.md"), "# A\n").unwrap();
    std::fs::write(
        root.join(".permark/local/quiet.json"),
        format!("{{\n  \"\": \"quiet\",\n  \"{MEMBER}/drafts\": \"speaking\"\n}}"),
    )
    .unwrap();
    std::fs::write(
        root.join(".permark/projects.toml"),
        format!("version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"{MEMBER}\"\n"),
    )
    .unwrap();
    (dir, root)
}

#[test]
fn marks_live_with_their_project_and_the_closest_mark_across_files_wins() {
    let (_dir, root) = setup();
    let root_file = root.join(".permark/local/quiet.json");
    let root_bytes = std::fs::read(&root_file).unwrap();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    // one root-relative view: the copied mark shows once
    assert_eq!(
        call(&core, "rules", Value::Null),
        json!({ "marks": { "": "quiet", format!("{MEMBER}/drafts"): "speaking" } })
    );
    let member_file = root.join(MEMBER).join(".permark/local/quiet.json");
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&member_file).unwrap()).unwrap(),
        json!({ "drafts": "speaking" })
    );
    // the root's mark reaches into the project through the chain; the project's own mark is closer
    assert!(silenced(&core, &format!("{MEMBER}/a.md")));
    assert!(!silenced(&core, &format!("{MEMBER}/drafts/d.md")));

    call(
        &core,
        "set",
        json!({ "paths": [format!("{MEMBER}/notes")], "interest": "quiet" }),
    );
    call(
        &core,
        "set",
        json!({ "paths": [MEMBER], "interest": "speaking" }),
    );
    assert!(
        !silenced(&core, &format!("{MEMBER}/a.md")),
        "project folder speaks"
    );
    assert!(silenced(&core, &format!("{MEMBER}/notes/n.md")));
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&member_file).unwrap()).unwrap(),
        json!({ "": "speaking", "notes": "quiet" }),
        "the member's own file, its own paths (\"\" = the project); `drafts` speaking changes nothing now: dropped"
    );
    assert_eq!(
        std::fs::read(&root_file).unwrap(),
        root_bytes,
        "the root's file: untouched (its entry inside the project rests)"
    );

    // the same marks from the project opened alone
    let alone = ProjectCore::open(Root::open(&root.join(MEMBER)).unwrap(), &env);
    let marks = call(&alone, "rules", Value::Null);
    assert_eq!(marks["marks"]["notes"], json!("quiet"));
    assert_eq!(marks["marks"][""], json!("speaking"));
    drop(alone);

    // left out: the root's own marks are in use again (dormant entry back), the project's stay in its file
    let uid = core.projects().snapshot().members()[0].uid.clone();
    core.projects().leave_out(&[uid]).unwrap();
    wait("owners", || core.projects().owners().is_single());
    assert_eq!(
        call(&core, "rules", Value::Null),
        json!({ "marks": { "": "quiet", format!("{MEMBER}/drafts"): "speaking" } }),
        "the root's own entries, as before the project was included"
    );
    assert!(member_file.is_file());
}

#[test]
fn a_mark_set_while_the_owners_change_is_written_to_the_new_owner() {
    let (_dir, root) = setup();
    std::fs::remove_file(root.join(".permark/projects.toml")).unwrap();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    call(
        &core,
        "set",
        json!({ "paths": [format!("{MEMBER}/x")], "interest": "quiet" }),
    );
    // not included yet: the root keeps it
    let root_file = root.join(".permark/local/quiet.json");
    assert!(std::fs::read_to_string(&root_file).unwrap().contains("/x"));
    core.projects_include(
        &env,
        &permark_core::projects::ProjectPick::Paths {
            paths: vec![MEMBER.to_owned()],
        },
    )
    .unwrap();
    // included: copied, and a new mark goes to the project's file only
    call(
        &core,
        "set",
        json!({ "paths": [format!("{MEMBER}/y")], "interest": "quiet" }),
    );
    let member: Value = serde_json::from_slice(
        &std::fs::read(root.join(MEMBER).join(".permark/local/quiet.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        member,
        json!({ "drafts": "speaking", "x": "quiet", "y": "quiet" })
    );
    assert!(!std::fs::read_to_string(&root_file).unwrap().contains("/y"));
}

// ---- verifier round 4 (adversarial) -----------------------------------------------------------------------------

/// V5: root tab + project opened alone, one after the other (no race): a mark set in the project tab survives a later
/// mark set from the root tab in the same project.
#[test]
fn verify_marks_from_two_tabs_on_one_project_both_survive() {
    let (_dir, root) = setup();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    let alone = ProjectCore::open(Root::open(&root.join(MEMBER)).unwrap(), &env);
    call(
        &alone,
        "set",
        json!({ "paths": ["from-alone"], "interest": "quiet" }),
    );
    call(
        &core,
        "set",
        json!({ "paths": [format!("{MEMBER}/from-root")], "interest": "quiet" }),
    );
    let member_file = root.join(MEMBER).join(".permark/local/quiet.json");
    let marks: Value = serde_json::from_slice(&std::fs::read(&member_file).unwrap()).unwrap();
    assert!(
        marks.get("from-alone").is_some() && marks.get("from-root").is_some(),
        "a mark set in the other tab was overwritten: {marks}"
    );
}

/// V6: an ancestor of the member renamed while open: the member's own marks stay in its file and keep working.
#[test]
fn verify_ancestor_rename_keeps_member_marks() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("root");
    let member = root.join("hubs").join("m");
    std::fs::create_dir_all(root.join(".permark/local")).unwrap();
    std::fs::create_dir_all(member.join(".permark/local")).unwrap();
    std::fs::create_dir_all(member.join("drafts")).unwrap();
    std::fs::write(member.join("drafts/d.md"), "# d\n").unwrap();
    std::fs::write(
        member.join(".permark/local/quiet.json"),
        "{\n  \"drafts\": \"quiet\"\n}",
    )
    .unwrap();
    std::fs::write(
        root.join(".permark/projects.toml"),
        "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"hubs/m\"\n",
    )
    .unwrap();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    assert!(silenced(&core, "hubs/m/drafts/d.md"));
    std::fs::rename(root.join("hubs"), root.join("hubs2")).unwrap();
    let uid = permark_core::projects::ProjectUid::parse("aaaaaaaaaa").unwrap();
    wait("member moved", || {
        core.projects()
            .snapshot()
            .member(&uid)
            .is_some_and(|m| m.path.as_str() == "hubs2/m")
    });
    std::thread::sleep(Duration::from_millis(1000));
    let file = std::fs::read_to_string(root.join("hubs2/m/.permark/local/quiet.json")).unwrap();
    let root_file =
        std::fs::read_to_string(root.join(".permark/local/quiet.json")).unwrap_or_default();
    assert!(
        silenced(&core, "hubs2/m/drafts/d.md"),
        "mark lost after ancestor rename; member file {file}; root file {root_file}"
    );
    assert!(
        file.contains("drafts"),
        "member file lost its mark: {file}; root file {root_file}"
    );
}
