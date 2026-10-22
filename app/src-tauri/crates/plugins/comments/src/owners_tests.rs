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
