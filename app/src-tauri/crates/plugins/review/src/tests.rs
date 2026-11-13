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

pub(crate) struct Harness {
    _base: tempfile::TempDir,
    pub(crate) project: tempfile::TempDir,
    pub(crate) env: CoreEnv,
    pub(crate) core: Option<Arc<ProjectCore>>,
}

impl Harness {
    pub(crate) fn new(with_git: bool) -> Self {
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

    pub(crate) fn root(&self) -> PathBuf {
        dunce::canonicalize(self.project.path()).unwrap()
    }

    pub(crate) fn write(&self, rel: &str, text: &str) {
        self.write_bytes(rel, text.as_bytes());
    }

    pub(crate) fn write_bytes(&self, rel: &str, bytes: &[u8]) {
        let path = self.root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    pub(crate) fn read(&self, rel: &str) -> Vec<u8> {
        std::fs::read(self.root().join(rel)).unwrap()
    }

    pub(crate) fn open(&mut self) {
        self.core = Some(ProjectCore::open(
            Root::open(&self.root()).unwrap(),
            &self.env,
        ));
    }

    pub(crate) fn call(
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

    pub(crate) fn review(
        &self,
        method: &str,
        args: Value,
    ) -> Result<Value, permark_core::IpcError> {
        self.call("review", method, args)
    }

    pub(crate) fn state(&self, path: &str) -> Value {
        self.review("status", json!({"path": path}))
            .map_or(Value::Null, |v| v["state"].clone())
    }

    pub(crate) fn hash(&self, rel: &str) -> String {
        fsx::hash(&self.read(rel)).hex()
    }

    /// Waits until the unread plugin has looked and the review holds a copy of the document.
    pub(crate) fn ready(&self, path: &str) {
        wait_until("the first look and the copy", || {
            self.state(path)["state"] == "same"
        });
    }
}

pub(crate) fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(15));
    }
}

pub(crate) const BASE_DOC: &str =
    "# Plan\n\nFirst paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n";

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
fn the_persons_own_save_of_a_read_document_is_never_offered_for_review() {
    let mut h = Harness::new(false);
    h.write("own.md", BASE_DOC);
    h.open();
    h.ready("own.md");
    // the person saves in Edit: the editor announces its write, the window tells the documents service
    let saved = BASE_DOC.replace("Second", "2nd");
    let core = h.core.clone().unwrap();
    let hash = fsx::hash(saved.as_bytes());
    core.watcher().own().expect_write("own.md", hash);
    h.write("own.md", &saved);
    core.documents().saved("w", "own.md", &hash.hex());
    // the page reloads at once, before the watch reports the write
    assert_eq!(h.state("own.md")["state"], "same");
    // and once it has: the unread plugin counts the save as read
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(h.state("own.md")["state"], "same");
    // an agent's change after it is news again
    agent_writes(&h, "own.md", &saved.replace("Third", "3rd"));
    wait_until("the change", || h.state("own.md")["state"] == "changes");
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
    h.write_bytes("blob.bin", b"\x00\x01\x02 binary \xff\x00");
    h.open();
    h.ready("a.md");
    // a text that was never looked at has nothing to compare with; binary is not compared
    assert_eq!(h.state("notes.txt")["state"], "noBasis");
    assert_eq!(h.state("blob.bin")["state"], "unsupported");
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
    // a version is a full commit id and nothing else (no name, no short id, no option, no expression), and a page of
    // versions starts at a number: anything else is refused before it is looked at
    let version =
        |id: &str| json!({"path": "a.md", "hash": hash, "index": 0, "basis": {"version": id}});
    for (method, args) in [
        ("status", version("HEAD")),
        ("open", version(&"a".repeat(39))),
        ("revertAll", version("-x")),
        ("revert", version(&format!("{}^", "a".repeat(39)))),
        ("undo", version("--upload-pack=x")),
        ("keep", version("")),
        ("setBasis", json!({"basis": {"version": "HEAD"}})),
        ("setBasis", json!({"basis": {"version": 7}})),
        ("setBasis", json!({"basis": {"version": null}})),
        ("versions", json!({"path": "a.md", "after": -1})),
        ("versions", json!({"path": "a.md", "after": "HEAD"})),
        ("versions", json!({"after": 0})),
    ] {
        let error = h.review(method, args.clone()).unwrap_err();
        assert_eq!(
            error.kind,
            ErrorKind::InvalidData,
            "{method} {args}: {error}"
        );
    }
    for path in ["../outside.md", "/etc/passwd", "a.md\0", "-x/../../y.md"] {
        let error = h.review("versions", json!({"path": path})).unwrap_err();
        assert!(
            matches!(error.kind, ErrorKind::PathEscape | ErrorKind::InvalidPath),
            "{path}: {error}"
        );
    }
    // a file that is there, committed and readable, asked for by a path the open folder does not allow (out of the
    // folder and back in, or the whole path): the list and a version as the basis are refused like every other call
    let mut g = Harness::new(true);
    let repo = init_repo(&g);
    let (first, last) = three_versions(&g, &repo, "v.md");
    g.open();
    wait_git(&g);
    let folder = g.root();
    let name = folder.file_name().unwrap().to_string_lossy().into_owned();
    let whole = folder.join("v.md").to_string_lossy().into_owned();
    let seen = g.hash("v.md");
    for path in [
        format!("../{name}/v.md"),
        format!("sub/../../{name}/v.md"),
        "../v.md".to_owned(),
        whole.clone(),
        whole.replace('\\', "/"),
    ] {
        let version = json!({"path": path, "hash": seen, "index": 0, "basis": {"version": first}});
        for (method, args) in [
            ("versions", json!({"path": path})),
            ("versions", json!({"path": path, "after": 1})),
            ("status", version.clone()),
            ("open", version.clone()),
            ("revert", version.clone()),
            ("revertAll", version.clone()),
        ] {
            let error = g.review(method, args).unwrap_err();
            assert!(
                matches!(error.kind, ErrorKind::PathEscape | ErrorKind::InvalidPath),
                "{method} {path}: {error}"
            );
        }
    }
    assert_eq!(g.read("v.md"), last.as_bytes(), "nothing was written");
    // a full id where there is no repository to ask: nothing to compare with
    for method in ["revert", "revertAll", "open"] {
        let error = h.review(method, version(&"a".repeat(40))).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unavailable, "{method}: {error}");
        assert_eq!(error.subject.as_deref(), Some("noBasis"), "{method}");
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

// ---- an earlier version (a commit) as the basis ------------------------------------------------------------------

/// The time of the first commit of a test (seconds since the epoch); later commits are given later times, so that the
/// order of the versions never rests on the clock.
pub(crate) const T0: i64 = 1_700_000_000;

/// A repository in the project folder that commits the bytes of a file as they are.
pub(crate) fn init_repo(h: &Harness) -> Repository {
    let repo = Repository::init(h.root()).unwrap();
    repo.config()
        .unwrap()
        .set_bool("core.autocrlf", false)
        .unwrap();
    repo
}

/// Commits the folder as it is, by `author`, `secs` after the epoch; the id of the commit.
pub(crate) fn commit_as(repo: &Repository, author: &str, message: &str, secs: i64) -> String {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .unwrap();
    index.update_all(["*"].iter(), None).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = Signature::new(author, "t@example.com", &git2::Time::new(secs, 0)).unwrap();
    let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
        .unwrap()
        .to_string()
}

pub(crate) fn commit(repo: &Repository, message: &str, secs: i64) -> String {
    commit_as(repo, "T", message, secs)
}

/// Waits until the git plugin has found the repository.
pub(crate) fn wait_git(h: &Harness) {
    wait_until("git", || {
        h.call("git", "snapshot", Value::Null)
            .is_ok_and(|v| !v.is_null())
    });
}

fn versions(h: &Harness, path: &str) -> Value {
    h.review("versions", json!({"path": path})).unwrap()
}

fn unavailable(reason: &str) -> Value {
    json!({"state": "unavailable", "reason": reason})
}

fn commits(list: &Value) -> Vec<String> {
    list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["commit"].as_str().unwrap().to_owned())
        .collect()
}

/// Three commits of `rel`: the base document, its first paragraph changed, its third paragraph changed too. The id of
/// the first one and the text of the last one.
fn three_versions(h: &Harness, repo: &Repository, rel: &str) -> (String, String) {
    h.write(rel, BASE_DOC);
    let first = commit(repo, "first", T0);
    h.write(rel, &BASE_DOC.replace("First", "1st"));
    commit(repo, "second", T0 + 60);
    let last = BASE_DOC.replace("First", "1st").replace("Third", "3rd");
    h.write(rel, &last);
    commit(repo, "third", T0 + 120);
    (first, last)
}

#[test]
fn an_earlier_version_is_compared_with_the_text_now_and_reverting_gives_its_text_back() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    let (first, last) = three_versions(&h, &repo, "v.md");
    h.open();
    wait_git(&h);
    let version = json!({"version": first});
    let status = h
        .review("status", json!({"path": "v.md", "basis": version}))
        .unwrap();
    assert_eq!(
        status,
        json!({"state": {"state": "changes", "count": 2}, "basis": version})
    );
    // the changes are the ones between the version two commits back and the text now
    let view = h
        .review("open", json!({"path": "v.md", "basis": version}))
        .unwrap();
    assert_eq!(view["basis"], version);
    assert_eq!(view["asked"], version);
    assert_eq!(view["unit"], "blocks");
    assert_eq!(view["hash"], h.hash("v.md"));
    assert_eq!(kinds(&view), ["edited", "edited"], "{view}");
    let ghosts: Vec<&str> = view["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["ghosts"][0]["html"].as_str().unwrap())
        .collect();
    assert!(ghosts[0].contains("First paragraph."), "{ghosts:?}");
    assert!(ghosts[1].contains("Third paragraph."), "{ghosts:?}");
    // a commit cannot be moved: nothing to keep
    assert_eq!(view["canKeep"], false);
    let hash = view["hash"].as_str().unwrap();
    for (method, args) in [
        (
            "keep",
            json!({"path": "v.md", "hash": hash, "index": 0, "basis": version}),
        ),
        (
            "keepAll",
            json!({"path": "v.md", "hash": hash, "basis": version}),
        ),
    ] {
        let error = h.review(method, args).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidData, "{method}");
        assert_eq!(error.detail.as_deref(), Some("commit"), "{method}");
    }
    assert_eq!(
        h.read("v.md"),
        last.as_bytes(),
        "a refused keep writes nothing"
    );
    // one change reverted: only that part is the old text
    let after = h
        .review(
            "revert",
            json!({"path": "v.md", "hash": hash, "index": 0, "basis": version}),
        )
        .unwrap();
    let partly = BASE_DOC.replace("Third", "3rd");
    assert_eq!(h.read("v.md"), partly.as_bytes());
    assert_eq!(
        (after["count"].as_u64(), &after["basis"]),
        (Some(1), &version)
    );
    // everything reverted: the file holds the text of the version
    let done = h
        .review(
            "revertAll",
            json!({"path": "v.md", "hash": after["hash"], "basis": version}),
        )
        .unwrap();
    assert_eq!(h.read("v.md"), BASE_DOC.as_bytes());
    assert_eq!(
        (done["count"].as_u64(), &done["basis"]),
        (Some(0), &version)
    );
    assert_eq!(
        h.review("status", json!({"path": "v.md", "basis": version}))
            .unwrap(),
        json!({"state": {"state": "same"}, "basis": version})
    );
    // undo brings the text back, step by step
    let back = h
        .review("undo", json!({"path": "v.md", "basis": version}))
        .unwrap();
    assert_eq!(h.read("v.md"), partly.as_bytes());
    assert_eq!(
        (back["count"].as_u64(), &back["basis"]),
        (Some(1), &version)
    );
    let back = h
        .review("undo", json!({"path": "v.md", "basis": version}))
        .unwrap();
    assert_eq!(h.read("v.md"), last.as_bytes());
    assert_eq!(back["count"], 2);
}

#[test]
fn bringing_back_a_version_keeps_the_byte_order_mark_and_the_line_endings_of_the_file() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    // the version was committed as plain UTF-8 with line feeds
    h.write(
        "crlf.md",
        "# Plan\n\nFirst paragraph.\n\nSecond paragraph.\n",
    );
    let first = commit(&repo, "first", T0);
    // since then the file has a byte order mark and CRLF endings
    h.write(
        "crlf.md",
        "\u{feff}# Plan\r\n\r\nFirst paragraph.\r\n\r\nSecond paragraph.\r\n",
    );
    let marked = commit(&repo, "second", T0 + 60);
    h.write(
        "crlf.md",
        "\u{feff}# Plan\r\n\r\nFirst paragraph, edited.\r\n\r\nSecond paragraph.\r\n\r\nAn addition.\r\n",
    );
    commit(&repo, "third", T0 + 120);
    h.open();
    wait_git(&h);
    let version = json!({"version": first});
    let view = h
        .review("open", json!({"path": "crlf.md", "basis": version}))
        .unwrap();
    assert_eq!(kinds(&view), ["edited", "added"], "{view}");
    let after = h
        .review(
            "revert",
            json!({"path": "crlf.md", "hash": view["hash"], "index": 0, "basis": version}),
        )
        .unwrap();
    assert_eq!(kinds(&after), ["added"]);
    assert_eq!(
        h.read("crlf.md"),
        "\u{feff}# Plan\r\n\r\nFirst paragraph.\r\n\r\nSecond paragraph.\r\n\r\nAn addition.\r\n"
            .as_bytes()
    );
    let done = h
        .review(
            "revertAll",
            json!({"path": "crlf.md", "hash": after["hash"], "basis": version}),
        )
        .unwrap();
    assert_eq!(done["count"], 0);
    // the text of the version, in the encoding and with the line endings the file has
    assert_eq!(
        h.read("crlf.md"),
        "\u{feff}# Plan\r\n\r\nFirst paragraph.\r\n\r\nSecond paragraph.\r\n".as_bytes()
    );
    // a version that differs in the mark and the line endings alone is the same text
    assert_eq!(
        h.review(
            "status",
            json!({"path": "crlf.md", "basis": {"version": marked}})
        )
        .unwrap()["state"],
        json!({"state": "same"})
    );
}

#[test]
fn a_file_that_changed_after_a_version_was_drawn_is_never_overwritten() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    let (first, last) = three_versions(&h, &repo, "s.md");
    h.open();
    wait_git(&h);
    let version = json!({"version": first});
    let view = h
        .review("open", json!({"path": "s.md", "basis": version}))
        .unwrap();
    let seen = view["hash"].as_str().unwrap().to_owned();
    // somebody writes again before the person decides
    let newer = last.replace("Second", "2nd");
    h.write("s.md", &newer);
    for (method, args) in [
        (
            "revert",
            json!({"path": "s.md", "hash": seen, "index": 0, "basis": version}),
        ),
        (
            "revertAll",
            json!({"path": "s.md", "hash": seen, "basis": version}),
        ),
        (
            "keep",
            json!({"path": "s.md", "hash": seen, "index": 0, "basis": version}),
        ),
        (
            "keepAll",
            json!({"path": "s.md", "hash": seen, "basis": version}),
        ),
    ] {
        let error = h.review(method, args).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Conflict, "{method}");
        assert_eq!(error.detail.as_deref(), Some("stale"), "{method}");
    }
    assert_eq!(h.read("s.md"), newer.as_bytes(), "nothing was written");
}

/// What a repository holds besides the working files: where HEAD points, every ref (the stash is one) and the index.
fn repository_state(h: &Harness) -> (String, Vec<(String, String)>, Vec<u8>) {
    let repo = Repository::open(h.root()).unwrap();
    let head = repo.find_reference("HEAD").unwrap();
    let head = format!(
        "{:?} -> {:?}",
        head.symbolic_target(),
        repo.head().unwrap().target()
    );
    let mut refs: Vec<(String, String)> = repo
        .references()
        .unwrap()
        .map(|reference| {
            let reference = reference.unwrap();
            (
                reference.name().unwrap().to_owned(),
                reference.target().unwrap().to_string(),
            )
        })
        .collect();
    refs.sort();
    let index = std::fs::read(h.root().join(".git").join("index")).unwrap();
    (head, refs, index)
}

#[test]
fn bringing_back_a_version_changes_the_working_file_and_nothing_in_the_repository() {
    let mut h = Harness::new(true);
    let mut repo = init_repo(&h);
    let (first, last) = three_versions(&h, &repo, "g.md");
    // work put aside before, and text that is in no commit
    h.write("g.md", &last.replace("Second", "aside"));
    let sig = Signature::new("T", "t@example.com", &git2::Time::new(T0 + 180, 0)).unwrap();
    repo.stash_save(&sig, "put aside", None).unwrap();
    assert_eq!(h.read("g.md"), last.as_bytes(), "the stash took the change");
    h.write("g.md", &last.replace("Second", "2nd"));
    h.open();
    wait_git(&h);
    let before = repository_state(&h);
    assert!(
        before.1.iter().any(|(name, _)| name == "refs/stash"),
        "{before:?}"
    );
    let version = json!({"version": first});
    let view = h
        .review("open", json!({"path": "g.md", "basis": version}))
        .unwrap();
    assert_eq!(view["count"], 1, "three paragraphs in a row: one change");
    let done = h
        .review(
            "revertAll",
            json!({"path": "g.md", "hash": view["hash"], "basis": version}),
        )
        .unwrap();
    assert_eq!(done["count"], 0);
    assert_eq!(h.read("g.md"), BASE_DOC.as_bytes());
    // HEAD, every ref (the stash too) and the index are what they were
    let after = repository_state(&h);
    assert_eq!(after.0, before.0, "HEAD");
    assert_eq!(after.1, before.1, "refs");
    assert!(after.2 == before.2, "the index");
    // all git sees is the working file, modified
    let statuses = repo.statuses(None).unwrap();
    let seen: Vec<(String, git2::Status)> = statuses
        .iter()
        .map(|entry| (entry.path().unwrap().to_owned(), entry.status()))
        .collect();
    assert_eq!(seen, [("g.md".to_owned(), git2::Status::WT_MODIFIED)]);
}

#[test]
fn a_version_that_cannot_be_compared_says_why() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    h.write("other.md", "# Other\n");
    let before_the_file = commit(&repo, "another file", T0);
    h.write("s.md", &format!("# Big\n\n{}\n", "word ".repeat(500_000)));
    let big = commit(&repo, "big", T0 + 60);
    h.write_bytes("s.md", b"\x00\x01\x02 binary \xff\x00");
    let binary = commit(&repo, "binary", T0 + 120);
    h.write_bytes("s.md", b"ok \xff\xfe broken");
    let lossy = commit(&repo, "not utf-8", T0 + 180);
    // what Git LFS commits in place of a file it keeps outside the repository
    let pointer_line = "version https://git-lfs.github.com/spec/v1\n";
    let oid = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";
    h.write(
        "s.md",
        &format!("{pointer_line}oid sha256:{oid}\nsize 12345\n"),
    );
    let pointer = commit(&repo, "kept elsewhere", T0 + 240);
    // a document that only starts like one (a pointer is under 1 KB) is a text
    let about = format!("{pointer_line}\n{}\n", "A note on pointers. ".repeat(60));
    h.write("s.md", &about);
    let quoted = commit(&repo, "about pointers", T0 + 270);
    h.write("s.md", BASE_DOC);
    let text = commit(&repo, "text", T0 + 300);
    h.write("s.md", &BASE_DOC.replace("Third", "3rd"));
    let head = commit(&repo, "edited", T0 + 360);
    h.open();
    wait_git(&h);
    let status = |basis: Value| {
        h.review("status", json!({"path": "s.md", "basis": basis}))
            .unwrap()
    };
    // the same text: nothing to review, and no gap
    assert_eq!(
        status(json!({"version": head})),
        json!({"state": {"state": "same"}, "basis": {"version": head}})
    );
    assert_eq!(
        status(json!({"version": text})),
        json!({"state": {"state": "changes", "count": 1}, "basis": {"version": text}})
    );
    let about_pointers = status(json!({"version": quoted}));
    assert_eq!(about_pointers["state"]["state"], "changes");
    assert!(about_pointers.get("gap").is_none(), "{about_pointers}");
    let nowhere = "a".repeat(40);
    for (commit, gap) in [
        (big.as_str(), "tooLarge"),
        (binary.as_str(), "notText"),
        (lossy.as_str(), "notText"),
        (pointer.as_str(), "notText"),
        // no commit has this id; the file was not there yet at that commit
        (nowhere.as_str(), "missing"),
        (before_the_file.as_str(), "missing"),
    ] {
        let version = json!({"version": commit});
        assert_eq!(
            status(version.clone()),
            json!({"state": {"state": "noBasis"}, "basis": version, "gap": gap}),
            "{gap}"
        );
        // no other basis stands in for it
        let error = h
            .review("open", json!({"path": "s.md", "basis": version}))
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unavailable, "{gap}");
        assert_eq!(error.subject.as_deref(), Some("noBasis"), "{gap}");
    }
    // what was last read and the last commit never carry a gap, with or without something to compare with
    h.write("fresh.md", "# Fresh\n");
    for (path, basis) in [("s.md", "read"), ("s.md", "commit"), ("fresh.md", "commit")] {
        let status = h
            .review("status", json!({"path": path, "basis": basis}))
            .unwrap();
        assert!(status.get("gap").is_none(), "{path} {basis}: {status}");
    }
    assert_eq!(
        h.review("status", json!({"path": "fresh.md", "basis": "commit"}))
            .unwrap()["state"],
        json!({"state": "noBasis"})
    );
}

#[test]
fn only_a_version_tells_that_text_in_no_commit_would_be_replaced() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    let (first, last) = three_versions(&h, &repo, "u.md");
    h.write("gone.md", BASE_DOC);
    let kept = commit(&repo, "a file that goes", T0 + 180);
    std::fs::remove_file(h.root().join("gone.md")).unwrap();
    commit(&repo, "gone", T0 + 240);
    h.open();
    h.ready("u.md");
    wait_git(&h);
    let version = json!({"version": first});
    let open = |basis: Value| {
        h.review("open", json!({"path": "u.md", "basis": basis}))
            .unwrap()
    };
    // the file is the last commit: bringing the version back replaces nothing that a commit does not hold
    assert_eq!(open(version.clone())["uncommitted"], false);
    // text that is in no commit
    agent_writes(&h, "u.md", &last.replace("Second", "2nd"));
    wait_until("the change", || h.state("u.md")["state"] == "changes");
    let view = open(version.clone());
    assert_eq!(
        (view["count"].as_u64(), &view["uncommitted"]),
        (Some(1), &json!(true))
    );
    // the other bases have nothing of the kind to tell
    for basis in ["read", "commit"] {
        let view = open(json!(basis));
        assert_eq!(
            (view["basis"].as_str(), &view["uncommitted"]),
            (Some(basis), &json!(false))
        );
    }
    // line endings alone are no other text
    h.write_bytes("u.md", last.replace('\n', "\r\n").as_bytes());
    assert_eq!(open(version.clone())["uncommitted"], false);
    // a file that is gone has nothing to compare, whatever the version: the version is not what is missing
    let gone = json!({"version": kept});
    assert_eq!(
        h.review("status", json!({"path": "gone.md", "basis": gone}))
            .unwrap(),
        json!({"state": {"state": "noBasis"}, "basis": gone})
    );
    // written again, the last commit does not hold it: all of its text is in no commit
    h.write("gone.md", &BASE_DOC.replace("Third", "3rd"));
    let view = h
        .review(
            "open",
            json!({"path": "gone.md", "basis": {"version": kept}}),
        )
        .unwrap();
    assert_eq!(
        (view["count"].as_u64(), &view["uncommitted"]),
        (Some(1), &json!(true))
    );
}

/// The old text of every change a view draws, in order.
fn old_text(view: &Value) -> String {
    view["changes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|change| change["ghosts"].as_array().unwrap().iter())
        .map(|ghost| ghost["html"].as_str().unwrap_or(""))
        .collect()
}

#[test]
fn bringing_back_a_version_does_not_forget_what_was_last_read() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    let (first, last) = three_versions(&h, &repo, "r.md");
    h.open();
    h.ready("r.md");
    wait_git(&h);
    // an agent changes the document and the person reads it: the review keeps the text read before
    let agents = last.replace("Second", "2nd");
    agent_writes(&h, "r.md", &agents);
    wait_until("the change", || h.state("r.md")["state"] == "changes");
    h.call(
        "unread",
        "markRead",
        json!({"path": "r.md", "hash": h.hash("r.md")}),
    )
    .unwrap();
    let read = |h: &Harness| h.review("open", json!({"path": "r.md", "basis": "read"}));
    let before = read(&h).unwrap();
    assert_eq!(before["count"], 1);
    assert!(old_text(&before).contains("Second paragraph."), "{before}");
    // the person looks at the first version and brings it back
    let version = json!({"version": first});
    let view = h
        .review("open", json!({"path": "r.md", "basis": version}))
        .unwrap();
    let done = h
        .review(
            "revertAll",
            json!({"path": "r.md", "hash": view["hash"], "basis": version}),
        )
        .unwrap();
    assert_eq!(done["count"], 0);
    assert_eq!(h.read("r.md"), BASE_DOC.as_bytes());
    // what was last read is still the text from before the agent's change: against it, the version differs in the
    // two paragraphs the commits after it changed, and the agent's paragraph is the one read
    // (asked at once, and again when the watch has reported the write)
    for (moment, wait) in [("at once", 0), ("after the watch", 500)] {
        std::thread::sleep(Duration::from_millis(wait));
        let status = h
            .review("status", json!({"path": "r.md", "basis": "read"}))
            .unwrap();
        assert_eq!(
            status,
            json!({"state": {"state": "changes", "count": 2}, "basis": "read"}),
            "{moment}"
        );
        let after = read(&h).unwrap();
        let old = old_text(&after);
        assert!(
            old.contains("1st paragraph.") && old.contains("3rd paragraph."),
            "{moment}: {after}"
        );
        assert!(!old.contains("2nd paragraph."), "{moment}: {after}");
    }
}

#[test]
fn a_picked_version_is_never_the_choice_remembered() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    let (first, _) = three_versions(&h, &repo, "a.md");
    h.open();
    h.ready("a.md");
    wait_git(&h);
    h.review("setBasis", json!({"basis": "commit"})).unwrap();
    // comparing with a version, and even naming it as the choice, changes nothing of what is remembered
    let version = json!({"version": first});
    let view = h
        .review("open", json!({"path": "a.md", "basis": version}))
        .unwrap();
    assert_eq!(view["count"], 2);
    h.review("setBasis", json!({"basis": version})).unwrap();
    assert_eq!(h.review("basis", Value::Null).unwrap(), json!("commit"));
    let prefs = h
        .core
        .as_ref()
        .unwrap()
        .store()
        .plugin_file::<Prefs>(&ID, "prefs.json");
    assert_eq!(
        prefs.load().into_value(),
        Prefs {
            basis: Basis::Commit
        }
    );
    // a new service over the same state folder answers the choice made before
    drop(h.core.take());
    h.open();
    wait_until("the plugin", || h.review("basis", Value::Null).is_ok());
    assert_eq!(h.review("basis", Value::Null).unwrap(), json!("commit"));
    assert_eq!(
        h.review("status", json!({"path": "a.md"})).unwrap()["basis"],
        "commit"
    );
    // a file that names a version (nothing of PerMark writes one) counts for nothing
    drop(h.core.take());
    let named: Value = json!({"basis": {"version": first}});
    prefs.save(&serde_json::from_value(named).unwrap()).unwrap();
    h.open();
    wait_until("the plugin", || h.review("basis", Value::Null).is_ok());
    assert_eq!(h.review("basis", Value::Null).unwrap(), json!("read"));
}

// ---- the list of versions ----------------------------------------------------------------------------------------

#[test]
fn the_versions_of_a_document_are_listed_with_who_when_and_what() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    h.write("l.md", BASE_DOC);
    let first = commit_as(&repo, "Ana", "Write the plan", T0);
    h.write("l.md", &BASE_DOC.replace("First", "1st"));
    let second = commit_as(
        &repo,
        "Bo",
        "Shorten the first paragraph\n\nAs asked.\n\nCo-authored-by: Helper Bot <bot@example.com>\n",
        T0 + 60,
    );
    // a commit that leaves the file alone is no version of it
    h.write("other.md", "# Other\n");
    commit(&repo, "Another file", T0 + 90);
    let last = BASE_DOC.replace("First", "1st").replace("Third", "3rd");
    h.write("l.md", &last);
    let third = commit_as(&repo, "Ana", "Shorten the third paragraph", T0 + 120);
    h.open();
    wait_git(&h);
    assert_eq!(
        versions(&h, "l.md"),
        json!({
            "state": "list",
            "rows": [
                {
                    "commit": third,
                    "atMs": (T0 + 120) * 1000,
                    "author": "Ana",
                    "coAuthors": [],
                    "subject": "Shorten the third paragraph",
                    "same": true,
                },
                {
                    "commit": second,
                    "atMs": (T0 + 60) * 1000,
                    "author": "Bo",
                    "coAuthors": ["Helper Bot"],
                    "subject": "Shorten the first paragraph",
                    "same": false,
                },
                {
                    "commit": first,
                    "atMs": T0 * 1000,
                    "author": "Ana",
                    "coAuthors": [],
                    "subject": "Write the plan",
                    "same": false,
                },
            ],
            "next": null,
            "renamedFrom": null,
            "cut": false,
        })
    );
    let same = |list: &Value| -> Vec<bool> {
        list["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["same"].as_bool().unwrap())
            .collect()
    };
    // line endings alone are no other text
    h.write_bytes("l.md", last.replace('\n', "\r\n").as_bytes());
    assert_eq!(same(&versions(&h, "l.md")), [true, false, false]);
    // text that is in no commit: no version is the text now
    h.write("l.md", &last.replace("Second", "2nd"));
    assert_eq!(same(&versions(&h, "l.md")), [false, false, false]);
    // not even the version whose text it is again: the last commit alone is told apart without reading every version
    h.write("l.md", BASE_DOC);
    assert_eq!(same(&versions(&h, "l.md")), [false, false, false]);
}

#[test]
fn a_list_of_versions_is_never_empty_without_a_reason() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    h.write("one.md", BASE_DOC);
    h.write("code.rs", "fn main() {}\n");
    h.write_bytes("blob.bin", b"\x00\x01\x02 binary \xff\x00");
    h.write_bytes("lossy.txt", b"ok \xff\xfe broken");
    h.write("big.md", &format!("# Big\n\n{}\n", "word ".repeat(500_000)));
    commit(&repo, "first", T0);
    h.write("new.md", "# Fresh\n");
    h.open();
    wait_git(&h);
    // one commit and the file as it left it: the version is the one the person sees
    assert_eq!(versions(&h, "one.md"), unavailable("onlyCurrent"));
    assert_eq!(versions(&h, "code.rs"), unavailable("onlyCurrent"));
    // never committed
    assert_eq!(versions(&h, "new.md"), unavailable("notCommitted"));
    assert_eq!(versions(&h, "nothing.md"), unavailable("notCommitted"));
    // what the review cannot draw has no versions to compare: not text, over 2 MB
    for path in ["blob.bin", "lossy.txt", "big.md"] {
        assert_eq!(versions(&h, path), unavailable("unsupported"), "{path}");
    }
    // one commit, and text that is in no commit: the version is listed, to compare with and to go back to
    h.write("one.md", &BASE_DOC.replace("Third", "3rd"));
    let list = versions(&h, "one.md");
    assert_eq!(list["state"], "list");
    assert_eq!(list["rows"].as_array().unwrap().len(), 1);
    assert_eq!(list["rows"][0]["same"], false);
}

#[test]
fn without_git_there_are_no_versions_and_the_answer_says_so() {
    let mut h = Harness::new(false);
    h.write("a.md", BASE_DOC);
    h.open();
    h.ready("a.md");
    assert_eq!(versions(&h, "a.md"), unavailable("unavailable"));
    // a version asked for all the same: nothing to compare with
    let version = json!({"version": "a".repeat(40)});
    assert_eq!(
        h.review("status", json!({"path": "a.md", "basis": version}))
            .unwrap(),
        json!({"state": {"state": "noBasis"}, "basis": version, "gap": "missing"})
    );
}

#[test]
fn the_versions_come_in_pages_and_the_next_page_starts_where_the_last_one_ended() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    let mut made = Vec::new();
    for n in 0..53 {
        h.write("p.md", &format!("# Plan\n\nParagraph {n}.\n"));
        made.push(commit(&repo, &format!("change {n}"), T0 + n * 60));
    }
    made.reverse();
    h.open();
    wait_git(&h);
    let first = versions(&h, "p.md");
    assert_eq!(first["state"], "list");
    assert_eq!(commits(&first), made[..50]);
    assert_eq!(first["rows"][0]["same"], true);
    assert!(first["next"].is_u64(), "{}", first["next"]);
    let second = h
        .review("versions", json!({"path": "p.md", "after": first["next"]}))
        .unwrap();
    assert_eq!(commits(&second), made[50..]);
    assert_eq!(second["next"], Value::Null);
    assert_eq!(second["rows"][0]["subject"], "change 2");
    // the first page again, asked for by its number
    assert_eq!(
        h.review("versions", json!({"path": "p.md", "after": 0}))
            .unwrap(),
        first
    );
}

#[test]
fn a_renamed_file_names_the_path_it_had_before() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    h.write("docs/old name.md", BASE_DOC);
    commit(&repo, "first", T0);
    std::fs::rename(
        h.root().join("docs/old name.md"),
        h.root().join("docs/new.md"),
    )
    .unwrap();
    let renamed = commit(&repo, "renamed", T0 + 60);
    h.write("docs/new.md", &BASE_DOC.replace("Third", "3rd"));
    let edited = commit(&repo, "edited", T0 + 120);
    h.open();
    wait_git(&h);
    let list = versions(&h, "docs/new.md");
    assert_eq!(list["state"], "list");
    assert_eq!(commits(&list), [edited, renamed]);
    assert_eq!(list["renamedFrom"], "docs/old name.md");
    assert_eq!(list["next"], Value::Null);
}

#[test]
fn a_file_only_renamed_since_lists_its_one_version_with_the_name_it_had_before() {
    let mut h = Harness::new(true);
    let repo = init_repo(&h);
    h.write("docs/old name.md", BASE_DOC);
    commit(&repo, "first", T0);
    std::fs::rename(
        h.root().join("docs/old name.md"),
        h.root().join("docs/new.md"),
    )
    .unwrap();
    let renamed = commit(&repo, "renamed", T0 + 60);
    h.open();
    wait_git(&h);
    // "only one version" would hide where the earlier ones are
    let list = versions(&h, "docs/new.md");
    assert_eq!(list["state"], "list");
    assert_eq!(commits(&list), [renamed]);
    assert_eq!(list["rows"][0]["same"], true);
    assert_eq!(list["renamedFrom"], "docs/old name.md");
}
