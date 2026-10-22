//! Round 4 (`projects.md` §9.2, §7): tags + descriptions live with the project that owns the file (its own
//! `meta.toml`); the views stay root-relative; a file moved into a project takes its entry to that project's file;
//! official | local | unlisted marks from the vocabularies (dormant without an official list); promote; front matter
//! tags come from the index pass (harvest).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::events::NoEvents;
use permark_core::paths::Root;
use permark_core::plugins::{PluginRegistry, RpcCall};
use permark_core::projects::VocabKind;
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use serde_json::{json, Value};

use super::*;

const UID: &str = "aaaaaaaaaa";

fn fast() -> Timing {
    Timing {
        quiet: Duration::from_millis(40),
        max_wait: Duration::from_millis(200),
        retry: [Duration::from_millis(40); 3],
    }
}

fn env(base: &Path) -> CoreEnv {
    CoreEnv::new(
        AppDirs::under(base),
        Arc::new(PluginRegistry::new(vec![plugin_with(fast())]).unwrap()),
        Arc::new(NoEvents),
        Distribution::Direct,
        AppSettings::default(),
    )
}

fn call(core: &ProjectCore, method: &str, args: Value) -> Value {
    core.rpc()
        .call(
            "tags",
            method,
            &RpcCall {
                window: "w".to_owned(),
                args,
            },
        )
        .unwrap()
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

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

const ROOT_META: &str = "# root tags\r\n[files.\"top.md\"]\r\ntags = [\"spec\"]\r\n\r\n[files.\"sub/plan.md\"]\r\ntags = [\"draft\"]\r\ndescription = \"The plan.\"\r\n";

/// Root (`.permark/`, a 0.6 `meta.toml` (CRLF), official list `spec`) + included `sub` (own `.permark/`, local list
/// `draft`).
fn setup() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("root");
    std::fs::create_dir_all(root.join(".permark")).unwrap();
    std::fs::create_dir_all(root.join("sub/.permark")).unwrap();
    for (rel, text) in [
        ("top.md", "# top\n"),
        ("sub/plan.md", "# plan\n"),
        ("sub/x.md", "# x\n"),
        ("sub/fm.md", "---\ntags: [lab, draft]\n---\n# fm\n"),
    ] {
        std::fs::write(root.join(rel), text).unwrap();
    }
    std::fs::write(root.join(".permark/meta.toml"), ROOT_META).unwrap();
    std::fs::write(
        root.join(".permark/vocabulary.toml"),
        "version = 1\n\n[tags.spec]\ncolor = \"blue\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("sub/.permark/vocabulary.toml"),
        "[tags.draft]\ncolor = \"gray\"\nicon = \"flag\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".permark/projects.toml"),
        format!("version = 1\n\n[[project]]\nid = \"{UID}\"\npath = \"sub\"\n"),
    )
    .unwrap();
    (dir, root)
}

fn status_of(overview: &Value, tag: &str) -> (Value, Value) {
    let row = overview["tags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == tag)
        .unwrap_or_else(|| panic!("{tag} in {overview}"));
    (row["status"].clone(), row["owner"].clone())
}

#[test]
fn tags_live_with_their_project_and_wear_their_status() {
    let (_dir, root) = setup();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    // copied once into the project's own meta.toml (its own paths)
    let member_meta = root.join("sub/.permark/meta.toml");
    assert!(read(&member_meta)
        .contains("[files.\"plan.md\"]\ntags = [\"draft\"]\ndescription = \"The plan.\""));
    wait("harvest", || {
        call(&core, "fileMeta", json!({ "path": "sub/fm.md" }))["tags"]
            .as_array()
            .is_some_and(|t| t.len() == 2)
    });
    let index = call(&core, "index", Value::Null);
    let paths: Vec<&str> = index["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        ["sub/fm.md", "sub/plan.md", "top.md"],
        "root-relative, one each"
    );
    assert_eq!(
        index["styles"]["spec"]["color"],
        json!("blue"),
        "official look"
    );

    // marks: official | local (the project listing it) | unlisted
    let overview = call(&core, "overview", Value::Null);
    assert_eq!(overview["marks"], json!(true));
    assert_eq!(
        status_of(&overview, "spec"),
        (json!("official"), Value::Null)
    );
    assert_eq!(status_of(&overview, "draft"), (json!("local"), json!(UID)));
    assert_eq!(
        status_of(&overview, "lab"),
        (json!("unlisted"), Value::Null)
    );
    let meta = call(&core, "fileMeta", json!({ "path": "sub/fm.md" }));
    assert_eq!(meta["tags"][1]["name"], json!("draft"));
    assert_eq!(meta["tags"][1]["status"], json!("local"));

    // an edit on a project file goes to the project's file, the root's stays as it was
    call(
        &core,
        "setTags",
        json!({ "path": "sub/x.md", "tags": ["draft", "new one"] }),
    );
    wait("member write", || {
        read(&member_meta).contains("[files.\"x.md\"]")
    });
    assert!(read(&member_meta).contains("tags = [\"draft\", \"new-one\"]"));
    assert_eq!(read(&root.join(".permark/meta.toml")), ROOT_META);

    // a file moved into the project (Explorer): its entry goes to the project's file
    let ops = core.fileops();
    let plan = ops
        .plan_move(
            &[permark_core::fileops::MoveSpec {
                from: "top.md".to_owned(),
                to: "sub/top.md".to_owned(),
            }],
            false,
        )
        .unwrap();
    ops.apply_move(plan.plan_id, false).unwrap();
    wait("moved entry", || {
        read(&member_meta).contains("[files.\"top.md\"]")
    });
    wait("root entry gone", || {
        !read(&root.join(".permark/meta.toml")).contains("\"top.md\"")
    });
    assert!(
        read(&root.join(".permark/meta.toml"))
            .contains("[files.\"sub/plan.md\"]\r\ntags = [\"draft\"]"),
        "the root's resting entry inside the project stays, CRLF kept"
    );

    // promote: the local tag becomes official (its look copied), nothing else of the file changes
    core.projects().promote(VocabKind::Tag, "draft").unwrap();
    let vocab = read(&root.join(".permark/vocabulary.toml"));
    assert_eq!(
        vocab,
        "version = 1\n\n[tags.spec]\ncolor = \"blue\"\n\n[tags.draft]\ncolor = \"gray\"\nicon = \"flag\"\n"
    );
    wait("official now", || {
        status_of(&call(&core, "overview", Value::Null), "draft").0 == json!("official")
    });

    // the project opened alone: the same entries, its own list official there
    let alone = ProjectCore::open(Root::open(&root.join("sub")).unwrap(), &env);
    let meta = call(&alone, "fileMeta", json!({ "path": "x.md" }));
    assert_eq!(meta["tags"][0]["name"], json!("draft"));
    assert_eq!(
        meta["tags"][0]["status"],
        json!("official"),
        "opened alone: its own list is official"
    );
}

#[test]
fn without_an_official_list_marks_stay_dormant() {
    let (_dir, root) = setup();
    std::fs::remove_file(root.join(".permark/vocabulary.toml")).unwrap();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    wait("harvest", || {
        call(&core, "overview", Value::Null)["tags"]
            .as_array()
            .is_some_and(|t| t.len() >= 3)
    });
    let overview = call(&core, "overview", Value::Null);
    assert_eq!(overview["marks"], json!(false));
    assert!(overview["tags"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["status"].is_null() && t["owner"].is_null()));
    assert!(
        !root.join(".permark/vocabulary.toml").exists(),
        "nothing written"
    );
}
