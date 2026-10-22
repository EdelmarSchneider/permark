//! Service + plugin tests against a real project (temp folders, the real watch).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use permark_core::conditions::{Distribution, Requirement};
use permark_core::events::{CoreEvent, EventSink};
use permark_core::fsx;
use permark_core::paths::Root;
use permark_core::plugins::{
    Manifest, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginRegistry, PluginScope,
    RpcCall, Running,
};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use permark_core::ErrorKind;
use serde_json::{json, Value};

use super::*;

// ---- harness -----------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Sink(Mutex<Vec<(String, Value)>>);

impl EventSink for Sink {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, payload } = event {
            self.0.lock().unwrap().push((name, payload));
        }
    }
}

/// A plugin that depends on `unread` and records what the hub tells it.
struct Probe {
    manifest: Manifest,
    calls: Arc<Mutex<Vec<String>>>,
}

struct Recorder(Arc<Mutex<Vec<String>>>);

impl ReadObserver for Recorder {
    fn at_rest(&self, path: &str, hash: u64, bytes: &[u8]) {
        let ok = fsx::hash(bytes).0 == hash;
        self.0.lock().unwrap().push(format!("at_rest:{path}:{ok}"));
    }
    fn read(&self, path: &str, before: Option<u64>, now: u64) {
        self.0.lock().unwrap().push(format!(
            "read:{path}:{}->{}",
            before.map_or("never".to_owned(), |h| fsx::ContentHash(h).hex()),
            fsx::ContentHash(now).hex()
        ));
    }
    fn moved(&self, from: &str, to: &str) {
        self.0.lock().unwrap().push(format!("moved:{from}->{to}"));
    }
    fn settled(&self, paths: &[String]) {
        self.0
            .lock()
            .unwrap()
            .push(format!("settled:{}", paths.join(",")));
    }
}

struct ProbeRunning {
    _lease: ObserverLease,
}

impl Running for ProbeRunning {}

impl Plugin for Probe {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let hub = cx
            .project()
            .and_then(|p| p.services.get::<UnreadHub>())
            .ok_or_else(|| PluginError::Start {
                id: PluginId::of("probe"),
                reason: "no unread hub".to_owned(),
            })?;
        Ok(Box::new(ProbeRunning {
            _lease: hub.subscribe(Arc::new(Recorder(Arc::clone(&self.calls)))),
        }))
    }
}

struct Harness {
    _base: tempfile::TempDir,
    project: tempfile::TempDir,
    sink: Arc<Sink>,
    calls: Arc<Mutex<Vec<String>>>,
    env: CoreEnv,
    core: Option<Arc<ProjectCore>>,
}

impl Harness {
    fn new() -> Self {
        let calls: Arc<Mutex<Vec<String>>> = Arc::default();
        let probe = Probe {
            manifest: Manifest {
                id: PluginId::of("probe"),
                kind: PluginKind::Feature,
                scope: PluginScope::Project,
                requires: Requirement::NONE,
                depends_on: Cow::Owned(vec![PluginId::of("unread")]),
                default_enabled: true,
            },
            calls: Arc::clone(&calls),
        };
        let sink = Arc::new(Sink::default());
        let base = tempfile::tempdir().unwrap();
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(
                PluginRegistry::new(vec![plugin(), Arc::new(probe) as Arc<dyn Plugin>]).unwrap(),
            ),
            sink.clone(),
            Distribution::Direct,
            AppSettings::default(),
        );
        Self {
            _base: base,
            project: tempfile::tempdir().unwrap(),
            sink,
            calls,
            env,
            core: None,
        }
    }

    fn root(&self) -> std::path::PathBuf {
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

    fn call(&self, method: &str, args: Value) -> Result<Value, permark_core::IpcError> {
        self.core.as_ref().unwrap().rpc().call(
            "unread",
            method,
            &RpcCall {
                window: "w".to_owned(),
                args,
            },
        )
    }

    fn unread(&self) -> Vec<String> {
        self.call("list", Value::Null)
            .map(|v| {
                v["unread"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|p| p.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    fn ready(&self) -> bool {
        self.call("list", Value::Null)
            .is_ok_and(|v| v["ready"] == true)
    }

    fn hash_of(&self, rel: &str) -> String {
        fsx::hash(&std::fs::read(self.root().join(rel)).unwrap()).hex()
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn read_json(&self) -> Option<Value> {
        let path = self.state_file("read.json");
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
    }

    fn state_file(&self, name: &str) -> std::path::PathBuf {
        self.core
            .as_ref()
            .unwrap()
            .store()
            .plugin_dir(&PluginId::of("unread"))
            .join(name)
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

fn paths(items: &Value) -> Vec<(String, String)> {
    items["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["path"].as_str().unwrap().to_owned(),
                i["kind"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

// ---- tests -----------------------------------------------------------------------------------------------------------

#[test]
fn the_first_visit_reads_everything_then_news_is_unread_until_read() {
    let mut h = Harness::new();
    h.write("a.md", "a\n");
    h.write("docs/b ü.md", "b\n");
    h.write("notes.txt", "not a document\n");
    h.open();
    wait_until("the first look", || h.ready());
    assert_eq!(h.unread(), Vec::<String>::new(), "the baseline: all read");
    // the probe was handed every document as read, with its bytes: live while the first look ran, else (it
    // subscribed after that look, run 39717402281) from the hub's list once ready, as the review plugin catches up
    let hub = h
        .core
        .as_ref()
        .unwrap()
        .services()
        .services
        .get::<UnreadHub>()
        .unwrap();
    let mut offered = h.calls();
    for (path, hash) in hub.at_rest() {
        let bytes = std::fs::read(h.root().join(&path)).unwrap();
        offered.push(format!("at_rest:{path}:{}", fsx::hash(&bytes).0 == hash));
    }
    assert!(
        offered.contains(&"at_rest:a.md:true".to_owned()),
        "{offered:?}"
    );
    assert!(
        offered.contains(&"at_rest:docs/b ü.md:true".to_owned()),
        "{offered:?}"
    );
    // the state is written once the baseline is taken
    wait_until("read.json", || h.read_json().is_some());
    assert_eq!(
        h.read_json().unwrap()["data"]["docs"]
            .as_object()
            .unwrap()
            .len(),
        2
    );

    // an agent writes: unread, announced to the windows of the project
    std::thread::sleep(Duration::from_millis(250));
    h.write("a.md", "a changed by an agent\n");
    h.write("fresh.md", "new\n");
    wait_until("news", || h.unread() == ["a.md", "fresh.md"]);
    wait_until("the announcement", || {
        h.sink.0.lock().unwrap().iter().any(|(n, p)| {
            n == "unread:changed"
                && p["data"]["unread"] == json!(["a.md", "fresh.md"])
                && p["root"].is_string()
        })
    });

    // reading the version that is on disk settles it, and the hub hears it
    let hash = h.hash_of("a.md");
    let a_at_rest = |calls: &[String]| {
        calls
            .iter()
            .filter(|c| c.as_str() == "at_rest:a.md:true")
            .count()
    };
    let before = a_at_rest(&h.calls());
    h.call("markRead", json!({"path": "a.md", "hash": hash}))
        .unwrap();
    assert_eq!(h.unread(), ["fresh.md"]);
    let calls = h.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("read:a.md:") && c.ends_with(&hash)),
        "{calls:?}"
    );
    assert!(a_at_rest(&calls) > before, "{calls:?}");
    // reading a version that is not the one on disk leaves it unread
    h.call(
        "markRead",
        json!({"path": "fresh.md", "hash": "0000000000000001"}),
    )
    .unwrap();
    assert_eq!(h.unread(), ["fresh.md"]);
    h.call("markUnread", json!({"path": "a.md"})).unwrap();
    assert_eq!(h.unread(), ["a.md", "fresh.md"]);
    h.call("markFolderRead", json!({"folder": ""})).unwrap();
    assert_eq!(h.unread(), Vec::<String>::new());
    wait_until("settled", || {
        h.calls().iter().any(|c| c.starts_with("settled:"))
    });
}

#[test]
fn the_next_visit_knows_what_changed_while_the_project_was_closed() {
    let mut h = Harness::new();
    h.write("keep.md", "keep\n");
    h.write("edit.md", "one\n");
    h.write("gone.md", "bye\n");
    h.open();
    wait_until("visit 1", || h.ready());
    let first = h.call("sinceLastVisit", Value::Null).unwrap();
    assert_eq!(first["first"], true, "{first}");
    assert!(first["items"].as_array().unwrap().is_empty());
    h.close();

    h.write("edit.md", "two\n");
    h.write("new.md", "new\n");
    std::fs::remove_file(h.root().join("gone.md")).unwrap();
    h.open();
    wait_until("visit 2", || h.ready());
    let since = h.call("sinceLastVisit", Value::Null).unwrap();
    assert_eq!(since["first"], false);
    assert_eq!(
        paths(&since),
        [
            ("edit.md".to_owned(), "changed".to_owned()),
            ("gone.md".to_owned(), "removed".to_owned()),
            ("new.md".to_owned(), "new".to_owned()),
        ]
    );
    // and what is unread: the read state survived the restart
    assert_eq!(h.unread(), ["edit.md", "new.md"]);
}

#[test]
fn moves_keep_the_read_state_and_the_hub_hears_them() {
    let mut h = Harness::new();
    h.write("old/a.md", "a\n");
    h.write("old/b.md", "b\n");
    h.open();
    wait_until("the first look", || h.ready());
    std::thread::sleep(Duration::from_millis(250));
    h.write("old/b.md", "b changed\n");
    wait_until("b is news", || h.unread() == ["old/b.md"]);
    std::fs::rename(h.root().join("old"), h.root().join("new ü")).unwrap();
    wait_until("moved", || h.unread() == ["new ü/b.md"]);
    let calls = h.calls();
    assert!(
        calls.iter().any(|c| c.starts_with("moved:old/")),
        "{calls:?}"
    );
    assert!(h.call("markFolderRead", json!({"folder": "new ü"})).is_ok());
    assert_eq!(h.unread(), Vec::<String>::new());
}

#[test]
fn hostile_calls_are_refused_and_never_touch_other_state() {
    let mut h = Harness::new();
    h.write("a.md", "a\n");
    h.open();
    wait_until("the first look", || h.ready());
    for (method, args) in [
        (
            "markRead",
            json!({"path": "../x.md", "hash": "0000000000000001"}),
        ),
        ("markRead", json!({"path": "a.md", "hash": "xyz"})),
        (
            "markRead",
            json!({"path": "a.md", "hash": "00000000000000011"}),
        ),
        ("markRead", json!({"path": "a.md"})),
        ("markUnread", json!({"path": "/etc/passwd"})),
        ("markFolderRead", json!({"folder": "../.."})),
        ("markFolderRead", json!({"folder": 3})),
    ] {
        let error = h.call(method, args.clone()).unwrap_err();
        assert!(
            matches!(
                error.kind,
                ErrorKind::InvalidData | ErrorKind::PathEscape | ErrorKind::InvalidPath
            ),
            "{method} {args}: {error}"
        );
    }
    assert_eq!(
        h.call("nonsense", Value::Null).unwrap_err().kind,
        ErrorKind::Unavailable
    );
    assert_eq!(h.unread(), Vec::<String>::new());
}

#[test]
fn off_leaves_nothing_and_on_again_starts_from_the_saved_state() {
    let mut h = Harness::new();
    h.write("a.md", "a\n");
    h.open();
    wait_until("the first look", || h.ready());
    wait_until("read.json", || h.read_json().is_some());
    let core = h.core.clone().unwrap();
    h.env.set_settings(AppSettings {
        plugins: BTreeMap::from([(PluginId::of("unread"), false)]),
        ..AppSettings::default()
    });
    core.apply_plugins(&h.env);
    assert_eq!(core.rpc().served(), Vec::<String>::new());
    assert_eq!(
        h.call("list", Value::Null).unwrap_err().kind,
        ErrorKind::Unavailable
    );
    // the dependent probe went with it
    assert!(core.running_plugins().is_empty());
    h.env.set_settings(AppSettings::default());
    core.apply_plugins(&h.env);
    wait_until("on again", || h.ready());
    assert_eq!(h.unread(), Vec::<String>::new());
    drop(core);
}

#[test]
fn a_damaged_read_file_is_a_project_with_no_read_state() {
    let mut h = Harness::new();
    h.write("a.md", "a\n");
    h.open();
    wait_until("the first look", || h.ready());
    wait_until("read.json", || h.read_json().is_some());
    let path = h.state_file("read.json");
    h.close();
    std::fs::write(&path, "{ not json").unwrap();
    h.open();
    wait_until("the second look", || h.ready());
    assert_eq!(
        h.unread(),
        Vec::<String>::new(),
        "a baseline again, nothing flooded"
    );
    wait_until("the state is written again", || Path::new(&path).exists());
}

#[test]
fn a_document_in_a_nested_repo_below_an_ignored_folder_becomes_unread_when_an_agent_writes() {
    let mut h = Harness::new();
    // the master repo ignores `hubs/`; `hubs/a` is a repo of its own
    h.write(".git/HEAD", "ref: refs/heads/main\n");
    h.write(".gitignore", "hubs/\n");
    h.write("hubs/a/.git/HEAD", "ref: refs/heads/main\n");
    h.write("hubs/a/plan.md", "plan\n");
    h.write("hubs/loose.md", "the container's own file\n");
    h.open();
    wait_until("the first look", || h.ready());
    assert_eq!(h.unread(), Vec::<String>::new());
    std::thread::sleep(Duration::from_millis(250));
    h.write("hubs/loose.md", "changed, still ignored\n");
    h.write("hubs/a/plan.md", "plan changed by an agent\n");
    wait_until("the nested document is unread", || {
        h.unread() == ["hubs/a/plan.md"]
    });
}
