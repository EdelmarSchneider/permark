//! Service + plugin tests against a real project (temp folders, the real watch).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::events::{CoreEvent, EventSink};
use permark_core::fileops::MoveSpec;
use permark_core::paths::Root;
use permark_core::plugins::{PluginId, PluginRegistry, RpcCall};
use permark_core::search::SearchOptions;
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use permark_core::ErrorKind;
use serde_json::{json, Value};

use super::*;

/// A byte copy of a real `.permark/meta.toml` written by version 0.5 (CRLF).
const REAL: &str = include_str!("fixtures/real-meta-0.5.toml");
const REAL_PATH: &str = "design/2-concept/sync/concept.md";

#[derive(Default)]
struct Sink(Mutex<Vec<String>>);

impl EventSink for Sink {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, .. } = event {
            self.0.lock().unwrap().push(name);
        }
    }
}

fn fast() -> Timing {
    Timing {
        quiet: Duration::from_millis(60),
        max_wait: Duration::from_millis(300),
        retry: [Duration::from_millis(50); 3],
    }
}

struct Harness {
    base: tempfile::TempDir,
    project: tempfile::TempDir,
    sink: Arc<Sink>,
    env: CoreEnv,
    core: Option<Arc<ProjectCore>>,
}

impl Harness {
    fn new() -> Self {
        let sink = Arc::new(Sink::default());
        let base = tempfile::tempdir().unwrap();
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(PluginRegistry::new(vec![plugin_with(fast())]).unwrap()),
            sink.clone(),
            Distribution::Direct,
            AppSettings::default(),
        );
        Self {
            base,
            project: tempfile::tempdir().unwrap(),
            sink,
            env,
            core: None,
        }
    }

    /// A project that has `.permark/` (the tags are shared there).
    fn shared() -> Self {
        let h = Self::new();
        std::fs::create_dir(h.root().join(".permark")).unwrap();
        h
    }

    fn root(&self) -> PathBuf {
        dunce::canonicalize(self.project.path()).unwrap()
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn open(&mut self) {
        self.core = Some(ProjectCore::open(
            Root::open(&self.root()).unwrap(),
            &self.env,
        ));
    }

    fn close(&mut self) {
        self.core = None;
    }

    fn core(&self) -> &Arc<ProjectCore> {
        self.core.as_ref().unwrap()
    }

    fn call(&self, method: &str, args: Value) -> Result<Value, permark_core::IpcError> {
        self.core().rpc().call(
            "tags",
            method,
            &RpcCall {
                window: "w".to_owned(),
                args,
            },
        )
    }

    fn meta(&self) -> PathBuf {
        self.root().join(".permark").join("meta.toml")
    }

    fn meta_text(&self) -> String {
        std::fs::read_to_string(self.meta()).unwrap_or_default()
    }

    fn index(&self) -> Value {
        self.call("index", Value::Null).unwrap()
    }

    fn tags_of(&self, path: &str) -> Vec<String> {
        self.index()["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["path"] == path)
            .map(|e| {
                e["tags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t.as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn set_tags(&self, path: &str, tags: &[&str]) {
        self.call("setTags", json!({ "path": path, "tags": tags }))
            .unwrap();
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

#[test]
fn the_real_file_is_read_and_an_edit_adds_only_its_own_bytes() {
    let mut h = Harness::shared();
    std::fs::write(h.meta(), REAL).unwrap();
    h.write("README.md", "# Readme\n");
    h.open();
    assert_eq!(h.tags_of(REAL_PATH), ["approved"], "read as it is");
    assert_eq!(h.index()["status"]["location"], "project");
    h.set_tags("README.md", &["Spec", "needs review"]);
    wait_until("the write", || h.meta_text().contains("README.md"));
    let text = h.meta_text();
    assert!(text.starts_with(REAL.trim_end()), "{text:?}");
    assert!(text.contains("tags = [\"spec\", \"needs-review\"]"));
    assert!(!text.replace("\r\n", "").contains('\n'), "CRLF kept");
    // closing writes what is pending
    h.call(
        "setDescription",
        json!({ "path": "README.md", "description": "  The\nstart " }),
    )
    .unwrap();
    h.close();
    assert!(h.meta_text().contains("description = \"The start\""));
    // nothing else was written into the project besides .permark/meta.toml
    let mut names: Vec<String> = std::fs::read_dir(h.root())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [".permark", "README.md"]);
}

#[test]
fn without_permark_the_tags_live_in_the_profile_and_the_project_is_untouched() {
    let mut h = Harness::new();
    h.write("a.md", "a\n");
    h.open();
    h.set_tags("a.md", &["x"]);
    assert_eq!(h.index()["status"]["location"], "profile");
    let profile = h.core().store().shared_file(META_FILE).path().to_path_buf();
    wait_until("the write", || profile.exists());
    assert!(profile.starts_with(h.base.path()));
    assert!(!h.root().join(".permark").exists());
}

#[test]
fn files_get_tags_in_front_matter_and_in_the_app_and_are_found_by_both() {
    let mut h = Harness::shared();
    h.write("a.md", "---\ntags: [fm, shared]\n---\n# A\n");
    h.write("b.md", "# B\n");
    h.write("c.png", "not really a picture");
    h.open();
    wait_until("the catalog", || {
        h.call("overview", Value::Null).unwrap()["tags"]
            .as_array()
            .is_some_and(|t| t.len() == 2)
    });
    h.set_tags("b.md", &["shared"]);
    h.set_tags("c.png", &["shared", "asset"]);
    h.set_tags("a.md", &["app"]);
    wait_until("the counts", || {
        h.call("overview", Value::Null).unwrap()["tags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "asset")
    });
    let overview = h.call("overview", Value::Null).unwrap();
    assert_eq!(
        overview["tags"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "shared")
            .unwrap()["count"],
        3
    );
    let both = h
        .call("filesWith", json!({ "tags": ["shared", "fm"] }))
        .unwrap();
    assert_eq!(both.as_array().unwrap().len(), 1);
    assert_eq!(both[0]["path"], "a.md");
    let meta = h.call("fileMeta", json!({ "path": "a.md" })).unwrap();
    assert_eq!(
        meta["tags"][0],
        json!({ "name": "fm", "file": true, "app": false })
    );
    assert_eq!(
        meta["tags"][2],
        json!({ "name": "app", "file": false, "app": true })
    );
    // search asks the same questions through the core seam
    let only = h
        .core()
        .search_filter(&SearchOptions {
            include_silenced: true,
            tags: vec!["asset".to_owned()],
            hidden_last: false,
        })
        .unwrap()
        .only
        .unwrap();
    assert_eq!(only.into_iter().collect::<Vec<_>>(), ["c.png"]);
}

#[test]
fn a_file_that_cannot_be_read_is_never_written_over_and_changes_wait() {
    let mut h = Harness::shared();
    h.write("a.md", "a\n");
    h.write("b.md", "b\n");
    h.open();
    h.set_tags("a.md", &["one"]);
    wait_until("the first write", || h.meta_text().contains("one"));
    // someone leaves a merge conflict in the file
    let broken = "<<<<<<< HEAD\n[files.\"a.md\"]\ntags = [\"one\"]\n=======\n[files.\"a.md\"]\ntags = [\"two\"]\n>>>>>>> theirs\n";
    std::fs::write(h.meta(), broken).unwrap();
    wait_until("the problem", || {
        h.index()["status"]["problem"]["line"] == 1
    });
    // read-only now: the edit is refused, the file is as it was
    let refused = h
        .call("setTags", json!({ "path": "b.md", "tags": ["x"] }))
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Unavailable);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(h.meta_text(), broken, "never written over");
    // fixed by hand: readable again, the app writes again
    std::fs::write(h.meta(), "[files.\"a.md\"]\ntags = [\"fixed\"]\n").unwrap();
    wait_until("readable", || h.index()["status"]["problem"].is_null());
    assert_eq!(h.tags_of("a.md"), ["fixed"]);
    h.set_tags("b.md", &["x"]);
    wait_until("written", || h.meta_text().contains("b.md"));
    assert!(h.meta_text().contains("fixed"));
}

#[test]
fn changes_made_before_the_file_broke_are_parked_and_written_when_it_reads_again() {
    // a slow writer: the change is still waiting when the file breaks
    let mut h = Harness::shared_with(Timing {
        quiet: Duration::from_millis(600),
        max_wait: Duration::from_secs(2),
        retry: [Duration::from_millis(50); 3],
    });
    h.write("a.md", "a\n");
    h.open();
    h.set_tags("a.md", &["mine"]);
    std::fs::write(h.meta(), "[files\n").unwrap();
    let started = Instant::now();
    while h.index()["status"]["problem"].is_null() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for the problem: {} / file {:?}",
            h.index()["status"],
            h.meta_text()
        );
        std::thread::sleep(Duration::from_millis(15));
    }
    assert_eq!(h.index()["status"]["parked"], 1);
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(h.meta_text(), "[files\n", "parked, not written");
    std::fs::write(h.meta(), "[files.\"z.md\"]\ntags = [\"theirs\"]\n").unwrap();
    wait_until("both", || {
        let text = h.meta_text();
        text.contains("mine") && text.contains("theirs")
    });
    // the count drops once the writer is back from the disk
    wait_until("nothing parked", || h.index()["status"]["parked"] == 0);
}

impl Harness {
    fn shared_with(timing: Timing) -> Self {
        let mut h = Self::shared();
        h.env = CoreEnv::new(
            AppDirs::under(h.base.path()),
            Arc::new(PluginRegistry::new(vec![plugin_with(timing)]).unwrap()),
            h.sink.clone(),
            Distribution::Direct,
            AppSettings::default(),
        );
        h
    }
}

#[test]
fn an_outside_edit_is_read_in_with_the_pending_changes_on_top() {
    let mut h = Harness::shared_with(Timing {
        quiet: Duration::from_millis(500),
        max_wait: Duration::from_secs(2),
        retry: [Duration::from_millis(50); 3],
    });
    h.write("a.md", "a\n");
    h.write("b.md", "b\n");
    h.open();
    h.set_tags("a.md", &["mine"]);
    std::fs::write(
        h.meta(),
        "# their note\n[files.\"b.md\"]\ntags = [\"theirs\"]\nfuture = 1\n",
    )
    .unwrap();
    wait_until("their edit", || h.tags_of("b.md") == ["theirs"]);
    assert_eq!(h.tags_of("a.md"), ["mine"], "mine stays on top");
    wait_until("the merged write", || h.meta_text().contains("mine"));
    let text = h.meta_text();
    assert!(
        text.contains("# their note") && text.contains("future = 1") && text.contains("theirs")
    );
}

#[test]
fn moving_a_file_or_a_folder_takes_the_tags_along() {
    let mut h = Harness::shared();
    h.write("old.md", "x\n");
    h.write("docs/sub/deep.md", "y\n");
    h.write("docs/top.md", "z\n");
    h.open();
    h.set_tags("old.md", &["a"]);
    h.set_tags("docs/sub/deep.md", &["b"]);
    h.set_tags("docs/top.md", &["c"]);
    wait_until("written", || h.meta_text().contains("docs/top.md"));
    // moves the app makes itself (Explorer)
    let mv = |from: &str, to: &str| {
        let ops = h.core().fileops();
        let plan = ops
            .plan_move(
                &[MoveSpec {
                    from: from.to_owned(),
                    to: to.to_owned(),
                }],
                false,
            )
            .unwrap();
        ops.apply_move(plan.plan_id, false).unwrap();
    };
    mv("old.md", "docs/new.md");
    wait_until("the file moved", || h.tags_of("docs/new.md") == ["a"]);
    mv("docs", "guide");
    wait_until("the folder moved", || {
        h.tags_of("guide/sub/deep.md") == ["b"]
            && h.tags_of("guide/top.md") == ["c"]
            && h.tags_of("guide/new.md") == ["a"]
    });
    wait_until("written", || h.meta_text().contains("guide/top.md"));
    assert!(!h.meta_text().contains("docs/"));
    assert!(h.tags_of("docs/top.md").is_empty());
    // a move made outside the app, of a file the watch has seen: found as a move by its content
    h.write("late.md", "a file written while the project is open\n");
    h.set_tags("late.md", &["l"]);
    wait_until("seen", || h.meta_text().contains("late.md"));
    std::thread::sleep(Duration::from_millis(400));
    std::fs::rename(h.root().join("late.md"), h.root().join("later.md")).unwrap();
    wait_until("the outside move", || h.tags_of("later.md") == ["l"]);
}

#[test]
fn clean_up_removes_the_entries_of_files_that_are_gone_and_only_those() {
    let mut h = Harness::shared();
    h.write("keep.md", "k\n");
    h.write("gone.md", "g\n");
    h.open();
    h.set_tags("keep.md", &["k"]);
    h.set_tags("gone.md", &["g"]);
    wait_until("written", || h.meta_text().contains("gone.md"));
    std::fs::remove_file(h.root().join("gone.md")).unwrap();
    // an entry that is no project path is left alone
    let text = format!(
        "{}\n[files.\"../outside.md\"]\ntags = [\"odd\"]\n",
        h.meta_text()
    );
    std::fs::write(h.meta(), text).unwrap();
    wait_until("reloaded", || !h.tags_of("../outside.md").is_empty());
    assert_eq!(h.call("orphans", Value::Null).unwrap(), json!(["gone.md"]));
    assert_eq!(h.call("overview", Value::Null).unwrap()["orphans"], 1);
    assert_eq!(h.call("cleanUp", Value::Null).unwrap(), 1);
    wait_until("cleaned", || !h.meta_text().contains("gone.md"));
    let text = h.meta_text();
    assert!(text.contains("keep.md") && text.contains("outside.md"));
}

#[test]
fn a_profile_copy_from_before_permark_is_merged_into_the_shared_file_and_retired() {
    let mut h = Harness::new();
    h.write("a.md", "a\n");
    h.write("b.md", "b\n");
    // the profile holds what the person tagged before the project had `.permark/`
    let store = permark_core::state::StateStore::for_project(
        &AppDirs::under(h.base.path()),
        &Root::open(&h.root()).unwrap(),
    );
    let legacy = store.shared_file(META_FILE);
    legacy
        .write(
            b"[files.\"a.md\"]\ntags = [\"old\", \"both\"]\ndescription = \"from the profile\"\n\n[tag_styles.old]\ncolor = \"red\"\n",
        )
        .unwrap();
    // the project has since got `.permark/` with a file of its own
    std::fs::create_dir(h.root().join(".permark")).unwrap();
    std::fs::write(
        h.meta(),
        "[files.\"a.md\"]\ntags = [\"both\", \"team\"]\ndescription = \"shared wins\"\n",
    )
    .unwrap();
    h.open();
    wait_until("the merge", || !legacy.path().exists());
    assert_eq!(h.tags_of("a.md"), ["both", "team", "old"]);
    let text = h.meta_text();
    assert!(text.contains("shared wins") && !text.contains("from the profile"));
    assert!(text.contains("[tag_styles.old]") && text.contains("color = \"red\""));
    assert!(
        !h.core().store().dir().join(META_FILE).exists(),
        "not in local/"
    );
}

#[test]
fn styles_are_kept_in_tag_styles_and_unknown_names_mean_none() {
    let mut h = Harness::shared();
    h.write("a.md", "a\n");
    h.open();
    h.call(
        "setStyle",
        json!({ "tag": "Spec", "color": "blue", "icon": "book-open" }),
    )
    .unwrap();
    wait_until("written", || h.meta_text().contains("[tag_styles.spec]"));
    assert_eq!(
        h.index()["styles"]["spec"],
        json!({ "color": "blue", "icon": "book-open" })
    );
    h.call(
        "setStyle",
        json!({ "tag": "spec", "color": "magenta", "icon": null }),
    )
    .unwrap();
    wait_until("removed", || !h.meta_text().contains("tag_styles"));
    assert_eq!(
        h.call("setStyle", json!({ "tag": "  " })).unwrap_err().kind,
        ErrorKind::InvalidData
    );
}

#[test]
fn hostile_calls_are_refused_and_nothing_else_is_written() {
    let mut h = Harness::shared();
    h.write("a.md", "a\n");
    h.open();
    for (path, kind) in [
        ("../etc/passwd", ErrorKind::PathEscape),
        ("/abs.md", ErrorKind::InvalidPath),
        ("missing.md", ErrorKind::NotFound),
        ("", ErrorKind::NotFound),
    ] {
        let error = h
            .call("setTags", json!({ "path": path, "tags": ["x"] }))
            .unwrap_err();
        assert_eq!(error.kind, kind, "{path}");
    }
    assert_eq!(
        h.call("setTags", json!({ "path": "a.md" }))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    // a folder is no file
    h.write("dir/x.md", "x\n");
    assert_eq!(
        h.call("setTags", json!({ "path": "dir", "tags": ["x"] }))
            .unwrap_err()
            .kind,
        ErrorKind::NotFound
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(!h.meta().exists());
    // a wall of tags and a very long description are cut, not refused
    let many: Vec<String> = (0..500).map(|n| format!("t{n}")).collect();
    let meta = h
        .call("setTags", json!({ "path": "a.md", "tags": many }))
        .unwrap();
    assert_eq!(meta["tags"].as_array().unwrap().len(), 100);
}

#[test]
fn off_leaves_nothing_and_on_again_reads_the_same_file() {
    let mut h = Harness::shared();
    h.write("a.md", "a\n");
    h.open();
    h.set_tags("a.md", &["x"]);
    wait_until("written", || h.meta_text().contains("a.md"));
    let before = h.meta_text();
    let core = h.core().clone();
    h.env.set_settings(AppSettings {
        plugins: BTreeMap::from([(PluginId::of("tags"), false)]),
        ..AppSettings::default()
    });
    core.apply_plugins(&h.env);
    assert_eq!(core.rpc().served(), Vec::<String>::new());
    assert_eq!(
        h.call("index", Value::Null).unwrap_err().kind,
        ErrorKind::Unavailable
    );
    // search cannot ask for tags any more
    let error = core
        .search_filter(&SearchOptions {
            include_silenced: true,
            tags: vec!["x".to_owned()],
            hidden_last: false,
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert_eq!(h.meta_text(), before, "the person's data is not touched");
    h.env.set_settings(AppSettings::default());
    core.apply_plugins(&h.env);
    assert_eq!(h.tags_of("a.md"), ["x"]);
    drop(core);
}

#[test]
fn rapid_edits_become_one_write_and_all_of_them_are_in_it() {
    let mut h = Harness::shared_with(Timing {
        quiet: Duration::from_millis(150),
        max_wait: Duration::from_secs(2),
        retry: [Duration::from_millis(50); 3],
    });
    for n in 0..20 {
        h.write(&format!("f{n}.md"), "x\n");
    }
    h.open();
    for n in 0..20 {
        h.set_tags(&format!("f{n}.md"), &[&format!("t{n}"), "common"]);
        h.call(
            "setDescription",
            json!({ "path": format!("f{n}.md"), "description": format!("d{n}") }),
        )
        .unwrap();
    }
    wait_until("all written", || h.meta_text().contains("f19.md"));
    h.close();
    let text = h.meta_text();
    for n in 0..20 {
        assert!(text.contains(&format!("[files.\"f{n}.md\"]")), "f{n}");
    }
    assert_eq!(text.matches("common").count(), 20);
    // the events told the window (at least one)
    assert!(h.sink.0.lock().unwrap().iter().any(|n| n == "tags:changed"));
}
