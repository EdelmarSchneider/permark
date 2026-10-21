// Test code: counts and sizes here are small, and plain `assert!(x.is_empty())` reads best.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::items_after_statements,
    clippy::single_match_else
)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use permark_core::conditions::{Context, Distribution};
use permark_core::events::{CoreEvent, EventSink};
use permark_core::plugins::{AppServices, CxWorkspace, RpcCall, RpcTable, Services};
use permark_core::settings::{AppSettings, ProjectConfig};
use permark_core::state::{AppDirs, StateStore};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::*;

#[derive(Default)]
pub(crate) struct Events(pub(crate) Mutex<Vec<(String, Value)>>);

impl EventSink for Events {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, payload } = event {
            self.0.lock().unwrap().push((name, payload));
        }
    }
}

pub(crate) fn rank_file() -> Vec<u8> {
    let mut tokens: Vec<Vec<u8>> = (0..=255_u8).map(|b| vec![b]).collect();
    for extra in [
        "he", "hel", "hell", "hello", " w", " wo", " wor", " worl", " world",
    ] {
        tokens.push(extra.as_bytes().to_vec());
    }
    let mut out = Vec::new();
    for (rank, token) in tokens.iter().enumerate() {
        out.extend_from_slice(STANDARD.encode(token).as_bytes());
        out.extend_from_slice(format!(" {rank}\n").as_bytes());
    }
    out
}

pub(crate) fn pin_of(body: &[u8]) -> Pin {
    Pin {
        url: "test://vocab".to_owned(),
        bytes: body.len() as u64,
        sha256: Sha256::digest(body).into(),
    }
}

/// Serves `body`, counting the calls.
pub(crate) struct Source {
    pub(crate) body: Vec<u8>,
    pub(crate) calls: AtomicUsize,
}

impl Fetch for Source {
    fn get(&self, _url: &str, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<(), FetchError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        for chunk in self.body.chunks(500) {
            if !sink(chunk) {
                break;
            }
        }
        Ok(())
    }
}

struct Harness {
    _tmp: tempfile::TempDir,
    dirs: AppDirs,
    rpc: Arc<RpcTable>,
    services: Arc<Services>,
    settings: Arc<RwLock<AppSettings>>,
    events: Arc<Events>,
    source: Arc<Source>,
    running: Option<Box<dyn Running>>,
}

impl Harness {
    fn start() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(tmp.path());
        let body = rank_file();
        let source = Arc::new(Source {
            body: body.clone(),
            calls: AtomicUsize::new(0),
        });
        let rpc = RpcTable::new();
        let services = Services::new();
        let settings = Arc::new(RwLock::new(AppSettings::default()));
        let events = Arc::new(Events::default());
        let store = StateStore::app(&dirs);
        let cx = PluginCx::new(
            ID,
            CxWorkspace::App,
            store.plugin_dir(&ID),
            None,
            Context::welcome(Distribution::Direct),
            Arc::clone(&events) as Arc<dyn EventSink>,
        )
        .with_store(store)
        .with_app(AppServices::new(
            Arc::clone(&rpc),
            Arc::clone(&services),
            Arc::clone(&settings),
        ));
        let plugin = plugin_with(
            Arc::clone(&source) as Arc<dyn Fetch>,
            [pin_of(&body), pin_of(&body)],
        );
        let running = plugin.start(&cx).unwrap();
        Self {
            _tmp: tmp,
            dirs,
            rpc,
            services,
            settings,
            events,
            source,
            running: Some(running),
        }
    }

    fn call(&self, method: &str, args: Value) -> Result<Value, permark_core::IpcError> {
        self.rpc.call(
            "token-count",
            method,
            &RpcCall {
                window: "main".into(),
                args,
            },
        )
    }

    fn wait_state(&self, encoding: &str, kind: &str) {
        let start = Instant::now();
        loop {
            let state = self.call("state", json!({})).unwrap();
            if state["vocab"][encoding]["state"]["kind"] == kind {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "stuck: {state}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn stop(&mut self) {
        self.running = None;
    }
}

#[test]
fn the_manifest_is_an_analysis_that_starts_on() {
    let manifest = plugin().manifest().clone();
    assert_eq!(manifest.id.as_str(), "token-count");
    assert_eq!(manifest.kind, PluginKind::Analysis);
    assert_eq!(manifest.scope, PluginScope::App);
    assert!(manifest.default_enabled);
    assert!(manifest.depends_on.is_empty());
}

/// On by default: starting (and stopping, and starting again, as the app does when settings change) asks the network
/// for nothing and writes no file. The download stays the person's own act.
#[test]
fn starting_fetches_nothing_and_writes_nothing() {
    fn files_under(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .flat_map(|e| {
                let path = e.path();
                if path.is_dir() {
                    files_under(&path)
                } else {
                    vec![path]
                }
            })
            .collect()
    }
    let mut h = Harness::start();
    h.call("state", json!({})).unwrap();
    h.stop();
    h.running = Some(
        plugin_with(
            Arc::clone(&h.source) as Arc<dyn Fetch>,
            [pin_of(b"x"), pin_of(b"x")],
        )
        .start(
            &PluginCx::new(
                ID,
                CxWorkspace::App,
                StateStore::app(&h.dirs).plugin_dir(&ID),
                None,
                Context::welcome(Distribution::Direct),
                Arc::clone(&h.events) as Arc<dyn EventSink>,
            )
            .with_store(StateStore::app(&h.dirs))
            .with_app(AppServices::new(
                Arc::clone(&h.rpc),
                Arc::clone(&h.services),
                Arc::clone(&h.settings),
            )),
        )
        .unwrap(),
    );
    h.stop();
    assert_eq!(h.source.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        files_under(&h.dirs.profile),
        Vec::<std::path::PathBuf>::new()
    );
    assert_eq!(files_under(&h.dirs.cache), Vec::<std::path::PathBuf>::new());
}

#[test]
fn estimates_count_with_no_vocabulary_and_no_network() {
    let h = Harness::start();
    let out = h
        .call(
            "count",
            json!({ "tokenizers": ["chars4", "claude", "gemini"], "text": "abcd efgh" }),
        )
        .unwrap();
    let out = out.as_array().unwrap();
    assert_eq!(out.len(), 3);
    assert_eq!(out[0]["status"], "ok");
    assert_eq!(out[0]["whole"], 3);
    assert!(out[1]["whole"].as_u64().unwrap() >= 1);
    assert_eq!(h.source.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn an_exact_tokenizer_is_unavailable_until_its_vocabulary_is_downloaded() {
    let h = Harness::start();
    let args = json!({ "tokenizers": ["o200k"], "text": "hello world" });
    let out = h.call("count", args.clone()).unwrap();
    assert_eq!(out[0]["status"], "unavailable");
    assert_eq!(out[0]["reason"], "noVocab");
    assert_eq!(h.source.calls.load(Ordering::SeqCst), 0);

    h.call("download", json!({ "tokenizer": "o200k" })).unwrap();
    h.wait_state("o200k", "ready");
    assert_eq!(h.source.calls.load(Ordering::SeqCst), 1);
    let out = h.call("count", args).unwrap();
    assert_eq!(out[0]["status"], "ok");
    // "hello" and " world" are tokens of the small vocabulary: two
    assert_eq!(out[0]["whole"], 2);
    // the file is in the app cache, in the plugin's folder
    let file = h
        .dirs
        .cache
        .join("plugins")
        .join("token-count")
        .join("o200k_base.tiktoken");
    assert!(file.is_file());
    // events told the windows
    let events = h.events.0.lock().unwrap().clone();
    assert!(events.iter().all(|(name, _)| name == "token-count:state"));
    assert!(events
        .iter()
        .any(|(_, payload)| payload["vocab"]["o200k"]["state"]["kind"] == "downloading"));
    assert_eq!(
        events.last().unwrap().1["vocab"]["o200k"]["state"]["kind"],
        "ready"
    );
}

#[test]
fn sections_are_counted_by_their_lines() {
    let h = Harness::start();
    let text = "hello\nhello world\n\nabcdefgh";
    let out = h
        .call(
            "count",
            json!({ "tokenizers": ["chars4"], "text": text, "parts": [[1, 1], [2, 3], [4, 4], [1, 4], [9, 12]] }),
        )
        .unwrap();
    assert_eq!(out[0]["whole"], 7);
    assert_eq!(out[0]["parts"], json!([2, 4, 2, 7, 0]));
}

#[test]
fn bad_arguments_are_typed_errors() {
    let h = Harness::start();
    for (method, args) in [
        ("count", json!({ "tokenizers": ["nope"], "text": "x" })),
        ("count", json!({ "tokenizers": [1], "text": "x" })),
        ("count", json!({ "text": "x" })),
        (
            "count",
            json!({ "tokenizers": [], "text": "x", "extra": 1 }),
        ),
        ("download", json!({ "tokenizer": "claude" })),
        ("download", json!({ "tokenizer": "nope" })),
        ("cancel", json!({})),
        ("removeVocab", json!({ "tokenizer": "chars4" })),
        ("setOutline", json!({ "on": "yes" })),
    ] {
        let error = h.call(method, args.clone()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidData, "{method} {args}");
    }
    assert_eq!(h.source.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn the_outline_switch_is_kept_in_the_plugins_own_folder() {
    let mut h = Harness::start();
    assert_eq!(h.call("state", json!({})).unwrap()["outline"], false);
    let state = h.call("setOutline", json!({ "on": true })).unwrap();
    assert_eq!(state["outline"], true);
    h.stop();
    // a new start reads it back
    let again = Harness::start_in(&h);
    assert_eq!(again.call("state", json!({})).unwrap()["outline"], true);
    let file = StateStore::app(&h.dirs).plugin_dir(&ID).join("prefs.json");
    assert!(file.is_file());
    // remove data deletes it and the cache
    drop(again);
    StateStore::app(&h.dirs).remove_plugin_data(&ID).unwrap();
    assert!(!file.exists());
}

impl Harness {
    /// A second start over the same folders (the same profile).
    fn start_in(other: &Harness) -> Harness {
        let body = rank_file();
        let source = Arc::new(Source {
            body: body.clone(),
            calls: AtomicUsize::new(0),
        });
        let rpc = RpcTable::new();
        let services = Services::new();
        let settings = Arc::new(RwLock::new(AppSettings::default()));
        let events = Arc::new(Events::default());
        let store = StateStore::app(&other.dirs);
        let cx = PluginCx::new(
            ID,
            CxWorkspace::App,
            store.plugin_dir(&ID),
            None,
            Context::welcome(Distribution::Direct),
            Arc::clone(&events) as Arc<dyn EventSink>,
        )
        .with_store(store)
        .with_app(AppServices::new(
            Arc::clone(&rpc),
            Arc::clone(&services),
            Arc::clone(&settings),
        ));
        let running = plugin_with(
            Arc::clone(&source) as Arc<dyn Fetch>,
            [pin_of(&body), pin_of(&body)],
        )
        .start(&cx)
        .unwrap();
        Harness {
            _tmp: tempfile::tempdir().unwrap(),
            dirs: other.dirs.clone(),
            rpc,
            services,
            settings,
            events,
            source,
            running: Some(running),
        }
    }
}

#[test]
fn the_core_service_counts_for_the_project_s_primary_tokenizer() {
    let h = Harness::start();
    let api = h
        .services
        .get::<TokenCounterApi>()
        .expect("service offered");
    let text = "hello world";
    // nothing chosen
    assert_eq!(api.0.primary(&ProjectConfig::default(), text), None);
    // the app chooses an estimate
    h.settings.write().unwrap().tokenizers = vec!["chars4".to_owned()];
    let counted = api.0.primary(&ProjectConfig::default(), text).unwrap();
    assert_eq!((counted.value, counted.approx), (3, true));
    // the project's choice wins: exact, vocabulary missing -> nothing, not a guess
    let project = ProjectConfig {
        tokenizers: vec!["o200k".to_owned()],
        ..ProjectConfig::default()
    };
    assert_eq!(api.0.primary(&project, text), None);
    h.call("download", json!({ "tokenizer": "o200k" })).unwrap();
    h.wait_state("o200k", "ready");
    let counted = api.0.primary(&project, text).unwrap();
    assert_eq!((counted.value, counted.approx), (2, false));
}

#[test]
fn turning_the_plugin_off_leaves_nothing_running() {
    let mut h = Harness::start();
    h.call("download", json!({ "tokenizer": "o200k" })).unwrap();
    h.stop();
    assert!(h.services.get::<TokenCounterApi>().is_none());
    assert_eq!(
        h.call("state", json!({})).unwrap_err().kind,
        ErrorKind::Unavailable
    );
    assert!(h.rpc.served().is_empty());
}

#[test]
fn the_vocabulary_can_be_removed_and_a_download_cancelled() {
    let h = Harness::start();
    h.call("download", json!({ "tokenizer": "cl100k" }))
        .unwrap();
    h.wait_state("cl100k", "ready");
    let state = h
        .call("removeVocab", json!({ "tokenizer": "cl100k" }))
        .unwrap();
    assert_eq!(state["vocab"]["cl100k"]["state"]["kind"], "absent");
    assert_eq!(state["vocab"]["cl100k"]["bytes"], rank_file().len());
    // cancel with nothing running is quiet
    h.call("cancel", json!({ "tokenizer": "cl100k" })).unwrap();
}
