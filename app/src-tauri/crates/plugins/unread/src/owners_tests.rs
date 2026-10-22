//! Round 4 (`projects.md` §9.2, §9.3): read state lives with the project that owns the document (its own
//! `read.json`, the file it uses opened alone); the unread list stays root-relative; the 0.6 state of the opened
//! folder is copied once (a document unread there stays unread); a project with no state at all starts read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::events::NoEvents;
use permark_core::fsx;
use permark_core::paths::Root;
use permark_core::plugins::{PluginRegistry, RpcCall};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use serde_json::{json, Value};

use super::*;

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
            "unread",
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

fn unread_now(core: &ProjectCore) -> Vec<String> {
    wait("first look", || {
        call(core, "list", Value::Null)["ready"] == json!(true)
    });
    serde_json::from_value(call(core, "list", Value::Null)["unread"].clone()).unwrap()
}

fn hash(text: &str) -> String {
    fsx::hash(text.as_bytes()).hex()
}

fn envelope(docs: &BTreeMap<&str, String>) -> String {
    serde_json::to_string(&json!({ "version": 1, "data": { "docs": docs } })).unwrap()
}

/// Root (`.permark/`) with 0.6 state: `top.md` + `sub/a.md` read, `sub/b.md` unread (seen at the last visit);
/// members `sub` (own `.permark/`) and `fresh` (no state anywhere, never seen from the root).
fn setup() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("root");
    let local = root.join(".permark/local/plugins/unread");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::create_dir_all(root.join("sub/.permark")).unwrap();
    std::fs::create_dir_all(root.join("fresh/.permark")).unwrap();
    for (rel, text) in [
        ("top.md", "# top\n"),
        ("sub/a.md", "# a\n"),
        ("sub/b.md", "# b\n"),
        ("fresh/f.md", "# f\n"),
    ] {
        std::fs::write(root.join(rel), text).unwrap();
    }
    let read: BTreeMap<&str, String> =
        [("top.md", hash("# top\n")), ("sub/a.md", hash("# a\n"))].into();
    let mut visit = read.clone();
    visit.insert("sub/b.md", hash("# b\n"));
    std::fs::write(local.join("read.json"), envelope(&read)).unwrap();
    std::fs::write(
        local.join("visit.json"),
        serde_json::to_string(&json!({ "version": 1, "data": { "at_ms": 1, "docs": visit } }))
            .unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join(".permark/projects.toml"),
        "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"sub\"\n\n[[project]]\nid = \"bbbbbbbbbb\"\npath = \"fresh\"\n",
    )
    .unwrap();
    (dir, root)
}

fn docs_of(path: &Path) -> Value {
    let v: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    v["data"]["docs"].clone()
}

#[test]
fn read_state_lives_with_its_project_and_unread_stays_unread() {
    let (_dir, root) = setup();
    let root_read = root.join(".permark/local/plugins/unread/read.json");
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    // copied once: b stays unread; the fresh project starts read (no flood)
    assert_eq!(unread_now(&core), ["sub/b.md"]);
    let member_read = root.join("sub/.permark/local/plugins/unread/read.json");
    assert_eq!(docs_of(&member_read), json!({ "a.md": hash("# a\n") }));

    call(
        &core,
        "markRead",
        json!({ "path": "sub/b.md", "hash": hash("# b\n") }),
    );
    wait("member read.json written", || {
        docs_of(&member_read).get("b.md").is_some()
    });
    assert!(unread_now(&core).is_empty());
    wait("root read.json written", || root_read.exists());
    let root_docs = docs_of(&root_read);
    assert_eq!(root_docs.get("sub/b.md"), None, "never in the root's file");
    assert_eq!(
        root_docs.get("sub/a.md"),
        Some(&json!(hash("# a\n"))),
        "the root's own entry inside the project rests as written"
    );
    assert!(root_docs.get("fresh/f.md").is_none());
    let fresh_read = root.join("fresh/.permark/local/plugins/unread/read.json");
    wait("fresh member state", || fresh_read.exists());
    assert_eq!(docs_of(&fresh_read), json!({ "f.md": hash("# f\n") }));

    // the project opened alone: the same read state
    let alone = ProjectCore::open(Root::open(&root.join("sub")).unwrap(), &env);
    assert!(unread_now(&alone).is_empty());
    drop(alone);

    // changed outside: unread in both views, root-relative here
    std::fs::write(root.join("sub/a.md"), "# a changed\n").unwrap();
    wait("change seen", || unread_now(&core) == ["sub/a.md"]);
}

#[test]
fn a_project_included_while_open_keeps_what_the_root_knew() {
    let (_dir, root) = setup();
    std::fs::remove_file(root.join(".permark/projects.toml")).unwrap();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    // not included: the root's state (sub/b unread; fresh/f new since the root's state = unread)
    let before = unread_now(&core);
    assert!(before.contains(&"sub/b.md".to_owned()), "{before:?}");
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    core.projects_include(
        &env,
        &permark_core::projects::ProjectPick::Paths {
            paths: vec!["sub".to_owned()],
        },
    )
    .unwrap();
    wait("owners", || core.projects().owners().slots().len() == 2);
    assert_eq!(
        unread_now(&core),
        before,
        "the same documents unread after the include"
    );
    let member_read = root.join("sub/.permark/local/plugins/unread/read.json");
    assert_eq!(docs_of(&member_read), json!({ "a.md": hash("# a\n") }));
}

/// B1 (D51): the root tab and the project opened alone mark documents read at the same time; when both close, the
/// project's `read.json` holds every mark (neither tab's save drops the other's), and a tab learns the other's marks.
#[test]
fn two_tabs_marking_read_at_once_lose_nothing() {
    let (_dir, root) = setup();
    for i in 0..12 {
        std::fs::write(root.join(format!("sub/d{i}.md")), format!("# d{i}\n")).unwrap();
    }
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    let alone = ProjectCore::open(Root::open(&root.join("sub")).unwrap(), &env);
    let _ = (unread_now(&core), unread_now(&alone));
    let ta = std::thread::spawn({
        let core = Arc::clone(&core);
        move || {
            for i in 0..6 {
                call(
                    &core,
                    "markRead",
                    json!({ "path": format!("sub/d{i}.md"), "hash": hash(&format!("# d{i}\n")) }),
                );
                std::thread::sleep(Duration::from_millis(60));
            }
        }
    });
    for i in 6..12 {
        call(
            &alone,
            "markRead",
            json!({ "path": format!("d{i}.md"), "hash": hash(&format!("# d{i}\n")) }),
        );
        std::thread::sleep(Duration::from_millis(60));
    }
    ta.join().unwrap();
    let member_read = root.join("sub/.permark/local/plugins/unread/read.json");
    // the alone tab saved its last mark; the root tab's next save learns it
    wait("alone tab saved", || {
        docs_of(&member_read).get("d11.md").is_some()
    });
    call(&core, "markUnread", json!({ "path": "sub/d0.md" }));
    wait("root tab saved", || {
        docs_of(&member_read).get("d0.md").is_none()
    });
    call(
        &core,
        "markRead",
        json!({ "path": "sub/d0.md", "hash": hash("# d0\n") }),
    );
    wait("root tab learns", || {
        !unread_now(&core).iter().any(|p| p.starts_with("sub/d"))
    });
    drop(alone);
    drop(core);
    let docs = docs_of(&member_read);
    for i in 0..12 {
        assert_eq!(
            docs.get(format!("d{i}.md")),
            Some(&json!(hash(&format!("# d{i}\n")))),
            "d{i}: {docs}"
        );
    }
    assert!(docs.get("a.md").is_some(), "{docs}");
}
