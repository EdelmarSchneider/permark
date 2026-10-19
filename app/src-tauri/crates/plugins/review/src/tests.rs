//! Service + plugin tests against a real project (temp folders, the real watch, the real unread plugin).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use git2::{IndexAddOption, Repository, Signature};
use permark_core::conditions::Distribution;
use permark_core::events::NoEvents;
use permark_core::fsx;
use permark_core::paths::Root;
use permark_core::plugins::{PluginRegistry, RpcCall};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use permark_core::ErrorKind;
use serde_json::{json, Value};

use super::*;

// ---- harness -----------------------------------------------------------------------------------------------------

struct Harness {
    _base: tempfile::TempDir,
    project: tempfile::TempDir,
    env: CoreEnv,
    core: Option<Arc<ProjectCore>>,
}

impl Harness {
    fn new(with_git: bool) -> Self {
        let base = tempfile::tempdir().unwrap();
        let mut plugins = vec![permark_plugin_unread::plugin(), plugin()];
        if with_git {
            plugins.push(permark_plugin_git::plugin());
        }
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(PluginRegistry::new(plugins).unwrap()),
            Arc::new(NoEvents),
            Distribution::Direct,
            AppSettings::default(),
        );
        Self {
            _base: base,
            project: tempfile::tempdir().unwrap(),
            env,
            core: None,
        }
    }

    fn root(&self) -> PathBuf {
        dunce::canonicalize(self.project.path()).unwrap()
    }

    fn write(&self, rel: &str, text: &str) {
        self.write_bytes(rel, text.as_bytes());
    }

    fn write_bytes(&self, rel: &str, bytes: &[u8]) {
        let path = self.root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn read(&self, rel: &str) -> Vec<u8> {
        std::fs::read(self.root().join(rel)).unwrap()
    }

    fn open(&mut self) {
        self.core = Some(ProjectCore::open(
            Root::open(&self.root()).unwrap(),
            &self.env,
        ));
    }

    fn call(
        &self,
        plugin: &str,
        method: &str,
        args: Value,
    ) -> Result<Value, permark_core::IpcError> {
        self.core.as_ref().unwrap().rpc().call(
            plugin,
            method,
            &RpcCall {
                window: "w".to_owned(),
                args,
            },
        )
    }

    fn review(&self, method: &str, args: Value) -> Result<Value, permark_core::IpcError> {
        self.call("review", method, args)
    }

    fn state(&self, path: &str) -> Value {
        self.review("status", json!({"path": path}))
            .map_or(Value::Null, |v| v["state"].clone())
    }

    fn hash(&self, rel: &str) -> String {
        fsx::hash(&self.read(rel)).hex()
    }

    /// Waits until the unread plugin has looked and the review holds a copy of the document.
    fn ready(&self, path: &str) {
        wait_until("the first look and the copy", || {
            self.state(path)["state"] == "same"
        });
    }
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(15));
    }
}

const BASE_DOC: &str = "# Plan\n\nFirst paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n";

fn kinds(view: &Value) -> Vec<String> {
    view["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["kind"].as_str().unwrap().to_owned())
        .collect()
}

fn agent_writes(h: &Harness, rel: &str, text: &str) {
    // the watch is quiet 150 ms before it reports; the unread plugin hashes what it finds
    std::thread::sleep(Duration::from_millis(200));
    h.write(rel, text);
}

// ---- tests -----------------------------------------------------------------------------------------------------------

#[test]
fn what_an_agent_changed_shows_block_by_block_with_the_old_text() {
    let mut h = Harness::new(false);
    h.write("plan.md", BASE_DOC);
    h.open();
    h.ready("plan.md");
    agent_writes(
        &h,
        "plan.md",
        "# Plan\n\nFirst paragraph, rewritten.\n\nA brand new paragraph.\n\nThird paragraph.\n",
    );
    wait_until("the change", || h.state("plan.md")["state"] == "changes");
    let view = h.review("open", json!({"path": "plan.md"})).unwrap();
    // "First" edited; "Second" replaced by the new one (one edited change: the runs are adjacent) or edited + added
    assert_eq!(view["basis"], "read");
    assert_eq!(view["canKeep"], true);
    assert_eq!(view["hash"], h.hash("plan.md"));
    let changes = view["changes"].as_array().unwrap();
    assert!(!changes.is_empty());
    let all_ghost_html: String = changes
        .iter()
        .flat_map(|c| c["ghosts"].as_array().unwrap().iter())
        .map(|g| g["html"].as_str().unwrap_or(""))
        .collect();
    assert!(
        all_ghost_html.contains("First paragraph."),
        "{all_ghost_html}"
    );
    assert!(
        all_ghost_html.contains("Second paragraph."),
        "{all_ghost_html}"
    );
    assert!(
        !all_ghost_html.contains("Third"),
        "an unchanged block is no ghost"
    );
    // the changes cover the new blocks that differ, by position in the page's list of blocks
    let covered: u64 = changes.iter().map(|c| c["len"].as_u64().unwrap()).sum();
    assert_eq!(covered, 2, "{view}");
}

#[test]
fn reading_a_changed_document_keeps_its_review_open_until_released() {
    let mut h = Harness::new(false);
    h.write("a.md", BASE_DOC);
    h.open();
    h.ready("a.md");
    agent_writes(&h, "a.md", &BASE_DOC.replace("Second", "2nd"));
    wait_until("the change", || h.state("a.md")["state"] == "changes");
    // the person reads it: it counts as read, the review stays
    let hash = h.hash("a.md");
    h.call("unread", "markRead", json!({"path": "a.md", "hash": hash}))
        .unwrap();
    assert_eq!(h.state("a.md")["state"], "changes");
    assert_eq!(
        h.review("open", json!({"path": "a.md"})).unwrap()["count"],
        1
    );
    // leaving the document releases it: what was read is what is compared with
    h.review("release", json!({"path": "a.md"})).unwrap();
    assert_eq!(h.state("a.md")["state"], "same");
}

#[test]
fn reverting_a_change_gives_back_the_old_block_and_every_other_byte_stays() {
    let mut h = Harness::new(false);
    // a file with a byte order mark and CRLF endings
    let original = "\u{feff}# Plan\r\n\r\nFirst paragraph.\r\n\r\nSecond paragraph.\r\n";
    h.write("crlf.md", original);
    h.open();
    h.ready("crlf.md");
    agent_writes(
        &h,
        "crlf.md",
        "\u{feff}# Plan\r\n\r\nFirst paragraph, edited.\r\n\r\nSecond paragraph.\r\n\r\nAn addition.\r\n",
    );
    wait_until("the changes", || h.state("crlf.md")["state"] == "changes");
    let view = h.review("open", json!({"path": "crlf.md"})).unwrap();
    assert_eq!(kinds(&view), ["edited", "added"], "{view}");
    // revert the edit: the second change (the addition) is untouched
    let hash = view["hash"].as_str().unwrap();
    let after = h
        .review(
            "revert",
            json!({"path": "crlf.md", "hash": hash, "index": 0}),
        )
        .unwrap();
    assert_eq!(kinds(&after), ["added"]);
    assert_eq!(
        h.read("crlf.md"),
        "\u{feff}# Plan\r\n\r\nFirst paragraph.\r\n\r\nSecond paragraph.\r\n\r\nAn addition.\r\n"
            .as_bytes()
    );
    // revert the rest: the file is byte for byte what was read
    let hash = after["hash"].as_str().unwrap();
    let done = h
        .review("revertAll", json!({"path": "crlf.md", "hash": hash}))
        .unwrap();
    assert_eq!(done["count"], 0);
    assert_eq!(h.read("crlf.md"), original.as_bytes());
}

#[test]
fn keeping_moves_the_base_and_undo_takes_it_back() {
    let mut h = Harness::new(false);
    h.write("k.md", BASE_DOC);
    h.open();
    h.ready("k.md");
    agent_writes(
        &h,
        "k.md",
        "# Plan\n\nFirst changed.\n\nSecond paragraph.\n\nThird changed.\n",
    );
    wait_until("the changes", || h.state("k.md")["state"] == "changes");
    let view = h.review("open", json!({"path": "k.md"})).unwrap();
    assert_eq!(view["count"], 2);
    let hash = view["hash"].as_str().unwrap().to_owned();
    let kept = h
        .review("keep", json!({"path": "k.md", "hash": hash, "index": 0}))
        .unwrap();
    assert_eq!(kept["count"], 1, "{kept}");
    assert!(
        kept["changes"][0]["ghosts"][0]["html"]
            .as_str()
            .unwrap()
            .contains("Third"),
        "the one left is the third paragraph"
    );
    // the file is untouched by a keep
    assert_eq!(h.hash("k.md"), hash);
    let undone = h.review("undo", json!({"path": "k.md"})).unwrap();
    assert_eq!(undone["count"], 2);
    assert!(
        h.review("undo", json!({"path": "k.md"})).is_err(),
        "nothing more to undo"
    );
    // keep all: approved, counts as read, review over
    let all = h
        .review("keepAll", json!({"path": "k.md", "hash": hash}))
        .unwrap();
    assert_eq!(all["count"], 0);
    assert_eq!(h.state("k.md")["state"], "same");
    wait_until("keep all counts as read", || {
        h.call("unread", "list", Value::Null).unwrap()["unread"] == json!([])
    });
}

#[test]
fn undo_of_a_revert_puts_the_agents_text_back() {
    let mut h = Harness::new(false);
    h.write("u.md", BASE_DOC);
    h.open();
    h.ready("u.md");
    let changed = BASE_DOC.replace("Second", "Middle");
    agent_writes(&h, "u.md", &changed);
    wait_until("the change", || h.state("u.md")["state"] == "changes");
    let view = h.review("open", json!({"path": "u.md"})).unwrap();
    let hash = view["hash"].as_str().unwrap();
    h.review("revert", json!({"path": "u.md", "hash": hash, "index": 0}))
        .unwrap();
    assert_eq!(h.read("u.md"), BASE_DOC.as_bytes());
    let back = h.review("undo", json!({"path": "u.md"})).unwrap();
    assert_eq!(h.read("u.md"), changed.as_bytes());
    assert_eq!(back["count"], 1);
}

#[test]
fn a_file_that_changed_meanwhile_is_never_overwritten() {
    let mut h = Harness::new(false);
    h.write("s.md", BASE_DOC);
    h.open();
    h.ready("s.md");
    agent_writes(&h, "s.md", &BASE_DOC.replace("First", "1st"));
    wait_until("the change", || h.state("s.md")["state"] == "changes");
    let view = h.review("open", json!({"path": "s.md"})).unwrap();
    let seen = view["hash"].as_str().unwrap().to_owned();
    // somebody writes again before the person decides
    let newer = BASE_DOC.replace("First", "1st").replace("Third", "3rd");
    h.write("s.md", &newer);
    for (method, args) in [
        ("revert", json!({"path": "s.md", "hash": seen, "index": 0})),
        ("revertAll", json!({"path": "s.md", "hash": seen})),
        ("keep", json!({"path": "s.md", "hash": seen, "index": 0})),
        ("keepAll", json!({"path": "s.md", "hash": seen})),
    ] {
        let error = h.review(method, args).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Conflict, "{method}");
    }
    assert_eq!(h.read("s.md"), newer.as_bytes(), "nothing was written");
}

#[test]
fn front_matter_does_not_shift_the_lines_of_a_revert() {
    let mut h = Harness::new(false);
    let original =
        "---\ntitle: T\ntags: [a]\n---\n# Plan\n\nFirst paragraph.\n\nSecond paragraph.\n";
    h.write("fm.md", original);
    h.open();
    h.ready("fm.md");
    agent_writes(&h, "fm.md", &original.replace("Second", "2nd"));
    wait_until("the change", || h.state("fm.md")["state"] == "changes");
    let view = h.review("open", json!({"path": "fm.md"})).unwrap();
    let hash = view["hash"].as_str().unwrap();
    h.review("revertAll", json!({"path": "fm.md", "hash": hash}))
        .unwrap();
    assert_eq!(h.read("fm.md"), original.as_bytes());
}

#[test]
fn the_states_without_a_review() {
    let mut h = Harness::new(false);
    h.write("a.md", BASE_DOC);
    h.write("notes.txt", "plain text\n");
    h.open();
    h.ready("a.md");
    // not Markdown
    assert_eq!(h.state("notes.txt")["state"], "unsupported");
    // a document that appeared later has no version to compare with (and no repository)
    std::thread::sleep(Duration::from_millis(200));
    h.write("new.md", "# Fresh\n");
    wait_until("the document", || h.state("new.md")["state"] == "noBasis");
    // over 2 MB
    h.write("big.md", &format!("# Big\n\n{}\n", "word ".repeat(500_000)));
    assert_eq!(h.state("big.md")["state"], "tooLarge");
    assert_eq!(
        h.review("open", json!({"path": "big.md"}))
            .unwrap_err()
            .kind,
        ErrorKind::TooLarge
    );
    // a document that is not there
    assert_eq!(
        h.review("open", json!({"path": "nothing.md"}))
            .unwrap_err()
            .kind,
        ErrorKind::NotFound
    );
}

#[test]
fn a_deleted_document_comes_back_as_it_was_last_read() {
    let mut h = Harness::new(false);
    h.write("gone.md", BASE_DOC);
    h.open();
    h.ready("gone.md");
    std::fs::remove_file(h.root().join("gone.md")).unwrap();
    assert!(h.review("restore", json!({"path": "gone.md"})).is_ok());
    assert_eq!(String::from_utf8(h.read("gone.md")).unwrap(), BASE_DOC);
    // there is nothing to restore when the file is there
    assert_eq!(
        h.review("restore", json!({"path": "gone.md"}))
            .unwrap_err()
            .kind,
        ErrorKind::Exists
    );
}

#[test]
fn hostile_calls_are_refused() {
    let mut h = Harness::new(false);
    h.write("a.md", BASE_DOC);
    h.open();
    h.ready("a.md");
    agent_writes(&h, "a.md", &BASE_DOC.replace("Second", "2nd"));
    wait_until("the change", || h.state("a.md")["state"] == "changes");
    let hash = h.hash("a.md");
    for (method, args) in [
        ("open", json!({"path": "../outside.md"})),
        ("status", json!({"path": "/etc/passwd"})),
        ("revert", json!({"path": "a.md", "hash": hash, "index": 99})),
        ("keep", json!({"path": "a.md", "hash": hash, "index": 99})),
        ("revert", json!({"path": "a.md", "hash": "zz", "index": 0})),
        ("revert", json!({"path": "a.md", "hash": hash})),
        ("setBasis", json!({"basis": "sideways"})),
        ("undo", json!({"path": "a.md"})),
    ] {
        let error = h.review(method, args.clone()).unwrap_err();
        assert!(
            matches!(
                error.kind,
                ErrorKind::InvalidData
                    | ErrorKind::PathEscape
                    | ErrorKind::InvalidPath
                    | ErrorKind::Conflict
                    | ErrorKind::Unavailable
            ),
            "{method} {args}: {error}"
        );
    }
    assert_eq!(h.read("a.md"), BASE_DOC.replace("Second", "2nd").as_bytes());
}

#[test]
fn the_last_commit_is_a_basis_too_and_a_commit_cannot_be_kept() {
    let mut h = Harness::new(true);
    let repo = Repository::init(h.root()).unwrap();
    h.write("c.md", BASE_DOC);
    {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("T", "t@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "first", &tree, &[])
            .unwrap();
    }
    h.open();
    h.ready("c.md");
    agent_writes(&h, "c.md", &BASE_DOC.replace("Third", "3rd"));
    wait_until("the change", || h.state("c.md")["state"] == "changes");
    // the person never read this version in the app, so the last read is the base; the commit is the same text here
    let wait_repo = || {
        h.call("git", "snapshot", Value::Null)
            .is_ok_and(|v| !v.is_null())
    };
    wait_until("git", wait_repo);
    h.review("setBasis", json!({"basis": "commit"})).unwrap();
    let view = h.review("open", json!({"path": "c.md"})).unwrap();
    assert_eq!(
        (view["basis"].as_str(), view["canKeep"].as_bool()),
        (Some("commit"), Some(false))
    );
    assert_eq!(view["count"], 1);
    let hash = view["hash"].as_str().unwrap();
    assert!(h
        .review("keep", json!({"path": "c.md", "hash": hash, "index": 0}))
        .is_err());
    h.review("revert", json!({"path": "c.md", "hash": hash, "index": 0}))
        .unwrap();
    assert_eq!(h.read("c.md"), BASE_DOC.as_bytes());
    assert_eq!(h.review("basis", Value::Null).unwrap(), json!("commit"));
}

#[test]
fn off_leaves_nothing_behind_and_the_review_follows_its_dependency() {
    let mut h = Harness::new(false);
    h.write("a.md", BASE_DOC);
    h.open();
    h.ready("a.md");
    let core = h.core.clone().unwrap();
    h.env.set_settings(AppSettings {
        plugins: std::collections::BTreeMap::from([(
            permark_core::plugins::PluginId::of("unread"),
            false,
        )]),
        ..AppSettings::default()
    });
    core.apply_plugins(&h.env);
    assert_eq!(
        core.rpc().served(),
        Vec::<String>::new(),
        "review goes with unread"
    );
    h.env.set_settings(AppSettings::default());
    core.apply_plugins(&h.env);
    wait_until("both on again", || h.state("a.md")["state"] == "same");
    drop(core);
}

#[test]
fn the_choice_of_basis_is_remembered() {
    let mut h = Harness::new(false);
    h.write("a.md", BASE_DOC);
    h.open();
    h.ready("a.md");
    assert_eq!(h.review("basis", Value::Null).unwrap(), json!("read"));
    h.review("setBasis", json!({"basis": "commit"})).unwrap();
    let core = h.core.take().unwrap();
    drop(core);
    h.open();
    wait_until("the plugin", || h.review("basis", Value::Null).is_ok());
    assert_eq!(h.review("basis", Value::Null).unwrap(), json!("commit"));
}
