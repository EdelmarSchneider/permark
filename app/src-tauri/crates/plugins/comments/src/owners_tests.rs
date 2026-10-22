//! Round 4 (`projects.md` §9.2): comments live with the project that owns the document (its own `comments.json`,
//! the file it uses opened alone); lists stay root-relative; an id two files share shows as `<uid>:<id>`; two tabs
//! writing the same project's comments (the root tab + the project opened alone) lose nothing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use permark_core::conditions::Distribution;
use permark_core::events::NoEvents;
use permark_core::organizing::CommentsApi;
use permark_core::paths::Root;
use permark_core::plugins::{PluginRegistry, RpcCall};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use serde_json::{json, Value};

use super::*;

const MEMBER: &str = "sub a";
const UID: &str = "aaaaaaaaaa";

fn env(base: &Path) -> CoreEnv {
    CoreEnv::new(
        AppDirs::under(base),
        Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
        Arc::new(NoEvents),
        Distribution::Direct,
        AppSettings::default(),
    )
}

fn call(core: &ProjectCore, method: &str, args: Value) -> Result<Value, permark_core::IpcError> {
    core.rpc().call(
        "comments",
        method,
        &RpcCall {
            window: "w".to_owned(),
            args,
        },
    )
}

fn list(core: &ProjectCore) -> Vec<Value> {
    call(core, "list", json!({ "scope": "all" }))
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

fn comment_json(id: &str, path: &str, text: &str) -> String {
    format!(
        r#"{{"id":"{id}","path":"{path}","text":"{text}","anchor":{{"quote":"alpha","line":1,"endLine":1}},"createdMs":1,"done":null}}"#
    )
}

/// A root with `.permark/` and an included member (own `.permark/`); both with a comment of the same id.
fn setup() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("root");
    std::fs::create_dir_all(root.join(".permark/local")).unwrap();
    std::fs::create_dir_all(root.join(MEMBER).join(".permark/local")).unwrap();
    std::fs::write(root.join("top.md"), "alpha\n").unwrap();
    std::fs::write(root.join(MEMBER).join("doc.md"), "alpha\n").unwrap();
    std::fs::write(
        root.join(".permark/local/comments.json"),
        format!(
            r#"{{"version":1,"comments":[{}]}}"#,
            comment_json("same", "top.md", "root one")
        ),
    )
    .unwrap();
    std::fs::write(
        root.join(MEMBER).join(".permark/local/comments.json"),
        format!(
            r#"{{"version":1,"comments":[{}]}}"#,
            comment_json("same", "doc.md", "member one")
        ),
    )
    .unwrap();
    std::fs::write(
        root.join(".permark/projects.toml"),
        format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"{MEMBER}\"\n"),
    )
    .unwrap();
    (dir, root)
}

#[test]
fn comments_live_with_their_project_and_a_shared_id_is_told_apart() {
    let (_dir, root) = setup();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    let all = list(&core);
    assert_eq!(all.len(), 2);
    let ids: Vec<&str> = all.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"same"));
    let prefixed = format!("{UID}:same");
    assert!(ids.contains(&prefixed.as_str()), "{ids:?}");
    let member = all.iter().find(|c| c["id"] == prefixed.as_str()).unwrap();
    assert_eq!(
        member["path"],
        json!(format!("{MEMBER}/doc.md")),
        "root-relative"
    );

    // done by the agent through the prefixed id: the member's file changes, its own id kept
    let api = core.services().services.get::<CommentsApi>().unwrap();
    api.0.resolve(&prefixed, Some("ok")).unwrap();
    let member_file = root.join(MEMBER).join(".permark/local/comments.json");
    let book: Value = serde_json::from_slice(&std::fs::read(&member_file).unwrap()).unwrap();
    assert_eq!(book["comments"][0]["id"], json!("same"));
    assert_eq!(book["comments"][0]["done"]["reply"], json!("ok"));
    let root_book: Value =
        serde_json::from_slice(&std::fs::read(root.join(".permark/local/comments.json")).unwrap())
            .unwrap();
    assert!(
        root_book["comments"][0]["done"].is_null(),
        "the root's comment untouched"
    );

    // a new comment on a project document goes to the project's file, with its own path
    let added = call(
        &core,
        "add",
        json!({ "path": format!("{MEMBER}/doc.md"), "quote": "alpha", "text": "new" }),
    )
    .unwrap();
    assert_eq!(added["path"], json!(format!("{MEMBER}/doc.md")));
    let book: Value = serde_json::from_slice(&std::fs::read(&member_file).unwrap()).unwrap();
    assert_eq!(book["comments"].as_array().unwrap().len(), 2);
    assert_eq!(book["comments"][1]["path"], json!("doc.md"));
}

#[test]
fn two_tabs_writing_the_same_projects_comments_lose_nothing() {
    let (_dir, root) = setup();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    let alone = ProjectCore::open(Root::open(&root.join(MEMBER)).unwrap(), &env);
    for i in 0..10 {
        call(
            &core,
            "add",
            json!({ "path": format!("{MEMBER}/doc.md"), "quote": "alpha", "text": format!("root tab {i}") }),
        )
        .unwrap();
        call(
            &alone,
            "add",
            json!({ "path": "doc.md", "quote": "alpha", "text": format!("alone {i}") }),
        )
        .unwrap();
    }
    // each change read the file again first (length | time differ): both tabs see all 21
    assert_eq!(list(&alone).len(), 21);
    assert_eq!(
        list(&core)
            .iter()
            .filter(|c| c["path"] == json!(format!("{MEMBER}/doc.md")))
            .count(),
        21
    );
}

/// `permark mcp` with the window open elsewhere (files mode) reads a project's comments while the app writes them:
/// every read sees a whole file (atomic writes), never a torn one, and the reader writes nothing.
#[test]
fn a_reader_with_no_window_never_sees_a_torn_file() {
    let (_dir, root) = setup();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    let reader = ProjectCore::from_files(
        Root::open(&root).unwrap(),
        &env,
        &[permark_core::plugins::PluginId::of("comments")],
    );
    let writer = std::thread::spawn({
        let core = Arc::clone(&core);
        move || {
            for i in 0..30 {
                call(
                    &core,
                    "add",
                    json!({ "path": format!("{MEMBER}/doc.md"), "quote": "alpha", "text": format!("w{i}") }),
                )
                .unwrap();
            }
        }
    });
    let mut last = 0;
    while !writer.is_finished() {
        let now = call(&reader, "list", json!({ "scope": "all" }))
            .expect("a whole file every time")
            .as_array()
            .unwrap()
            .len();
        assert!(now >= last, "never fewer: {now} < {last}");
        last = now;
    }
    writer.join().unwrap();
    assert_eq!(
        call(&reader, "list", json!({ "scope": "all" }))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        32
    );
}

// ---- verifier round 4 (adversarial) -----------------------------------------------------------------------------

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let start = std::time::Instant::now();
    while !ok() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn paths_of(core: &ProjectCore) -> Vec<String> {
    list(core)
        .iter()
        .map(|c| c["path"].as_str().unwrap().to_owned())
        .collect()
}

/// V1: a member folder renamed while open (silent re-attach, §4.4 step 1): its comments stay visible and stay in the
/// member's own file (never land in the root file as dormant entries).
#[test]
fn verify_member_renamed_while_open_keeps_its_comments() {
    for round in 0..5 {
        let (_dir, root) = setup();
        let base = tempfile::tempdir().unwrap();
        let env = env(base.path());
        let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
        assert!(core.projects().wait_ready(Duration::from_secs(10)));
        assert!(paths_of(&core).contains(&format!("{MEMBER}/doc.md")));
        std::fs::rename(root.join(MEMBER), root.join("sub z")).unwrap();
        let uid = permark_core::projects::ProjectUid::parse(UID).unwrap();
        wait_until("member moved", || {
            core.projects()
                .snapshot()
                .member(&uid)
                .is_some_and(|m| m.path.as_str() == "sub z")
        });
        std::thread::sleep(Duration::from_millis(1500));
        let paths = paths_of(&core);
        let root_book = std::fs::read_to_string(root.join(".permark/local/comments.json")).unwrap();
        assert!(
            paths.contains(&"sub z/doc.md".to_owned()),
            "round {round}: member comment gone from the list: {paths:?}; root file: {root_book}"
        );
        assert!(
            !root_book.contains("sub z/doc.md"),
            "round {round}: landed in root file: {root_book}"
        );
    }
}

/// V2: an ancestor of a member renamed while open.
#[test]
fn verify_ancestor_of_member_renamed_while_open_keeps_its_comments() {
    for round in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap().join("root");
        let member = root.join("hubs").join("m");
        std::fs::create_dir_all(root.join(".permark/local")).unwrap();
        std::fs::create_dir_all(member.join(".permark/local")).unwrap();
        std::fs::write(member.join("doc.md"), "alpha\n").unwrap();
        std::fs::write(
            member.join(".permark/local/comments.json"),
            format!(
                r#"{{"version":1,"comments":[{}]}}"#,
                comment_json("m1", "doc.md", "member")
            ),
        )
        .unwrap();
        std::fs::write(
            root.join(".permark/projects.toml"),
            format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"hubs/m\"\n"),
        )
        .unwrap();
        let base = tempfile::tempdir().unwrap();
        let env = env(base.path());
        let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
        assert!(core.projects().wait_ready(Duration::from_secs(10)));
        assert!(paths_of(&core).contains(&"hubs/m/doc.md".to_owned()));
        std::fs::rename(root.join("hubs"), root.join("hubs2")).unwrap();
        let uid = permark_core::projects::ProjectUid::parse(UID).unwrap();
        wait_until("member moved", || {
            core.projects()
                .snapshot()
                .member(&uid)
                .is_some_and(|m| m.path.as_str() == "hubs2/m")
        });
        std::thread::sleep(Duration::from_millis(1500));
        let paths = paths_of(&core);
        let root_book =
            std::fs::read_to_string(root.join(".permark/local/comments.json")).unwrap_or_default();
        assert!(
            paths.contains(&"hubs2/m/doc.md".to_owned()),
            "round {round}: member comment gone from the list: {paths:?}; root file: {root_book}"
        );
    }
}

/// V3: left out, a comment added on a project document (goes to the root file), included again (same path: no
/// second copy): the comment must still show.
#[test]
fn verify_comment_added_while_left_out_survives_include_again() {
    let (_dir, root) = setup();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    let uid = permark_core::projects::ProjectUid::parse(UID).unwrap();
    core.projects()
        .leave_out(std::slice::from_ref(&uid))
        .unwrap();
    wait_until("left out", || core.projects().owners().is_single());
    call(
        &core,
        "add",
        json!({ "path": format!("{MEMBER}/doc.md"), "quote": "alpha", "text": "while left out" }),
    )
    .unwrap();
    assert!(list(&core).iter().any(|c| c["text"] == "while left out"));
    core.projects_include(
        &env,
        &permark_core::projects::ProjectPick::Paths {
            paths: vec![MEMBER.to_owned()],
        },
    )
    .unwrap();
    wait_until("included again", || {
        core.projects().owners().slots().len() == 2
    });
    let texts: Vec<Value> = list(&core).iter().map(|c| c["text"].clone()).collect();
    assert!(
        texts.contains(&json!("while left out")),
        "comment written while left out is hidden after include again: {texts:?}"
    );
}

/// V4: root tab + project opened alone add comments to the same project at the same time: nothing lost.
#[test]
fn verify_two_tabs_adding_concurrently_lose_nothing() {
    let (_dir, root) = setup();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    let alone = ProjectCore::open(Root::open(&root.join(MEMBER)).unwrap(), &env);
    let a = std::thread::spawn({
        let core = Arc::clone(&core);
        move || {
            for i in 0..40 {
                call(&core, "add", json!({ "path": format!("{MEMBER}/doc.md"), "quote": "alpha", "text": format!("r{i}") }))
                    .unwrap();
            }
        }
    });
    let b = std::thread::spawn({
        let alone = Arc::clone(&alone);
        move || {
            for i in 0..40 {
                call(
                    &alone,
                    "add",
                    json!({ "path": "doc.md", "quote": "alpha", "text": format!("a{i}") }),
                )
                .unwrap();
            }
        }
    });
    a.join().unwrap();
    b.join().unwrap();
    let file: Value = serde_json::from_slice(
        &std::fs::read(root.join(MEMBER).join(".permark/local/comments.json")).unwrap(),
    )
    .unwrap();
    let texts: Vec<String> = file["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["text"].as_str().unwrap().to_owned())
        .collect();
    let r = texts.iter().filter(|t| t.starts_with('r')).count();
    let a = texts.iter().filter(|t| t.starts_with('a')).count();
    assert_eq!(
        texts.len(),
        81,
        "lost updates: root tab kept {r}/40, alone tab kept {a}/40"
    );
}
