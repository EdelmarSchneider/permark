//! Installs (AC-3..AC-6, AC-8, AC-1 dormant) over scripted downloads: tiny files pinned in a test catalog.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use permark_core::conditions::{Context, Distribution};
use permark_core::downloads::{
    DownloadError, DownloadKit, MemorySecrets, Request, Response, Scripted, ScriptedTransport,
    Transport,
};
use permark_core::events::{CoreEvent, EventSink};
use permark_core::plugins::{
    AppServices, CxWorkspace, PluginCx, RpcCall, RpcTable, Running, ServiceLease, Services,
};
use permark_core::settings::AppSettings;
use permark_core::state::{AppDirs, StateStore};
use permark_core::{ErrorKind, IpcError};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::entries;
use crate::catalog::Catalog;
use crate::engine::files::WorkerPin;
use crate::installs::{Installs, ModelFor};
use crate::tasks::Task;
use crate::{plugin_with, state, ID};

pub(super) const EMBED: &str = "embeddinggemma-2-q8_0";
pub(super) const LFM: &str = "lfm3-1.7b-q4_k_m";
pub(super) const E2B: &str = "gemma-4-e2b-it-q4_0";
pub(super) const WHISPER: &str = "whisper-large-v3-turbo-q5_0";

/// Bytes of one test file (differ per model, file, version).
pub(super) fn body(model: &str, path: &str, version: u32) -> Vec<u8> {
    format!("{model}|{path}|v{version};")
        .repeat(40)
        .into_bytes()
}

pub(super) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The bundled catalog at `version` with tiny files, every model on any llama.cpp build (install tests; real floors:
/// [`shipped_engines`]) (+ `edit` on its JSON).
pub(super) fn catalog(version: u32, edit: impl FnOnce(&mut Value)) -> Catalog {
    let mut value: Value = serde_json::from_str(include_str!("../catalog.json")).unwrap();
    for model in value["models"].as_array_mut().unwrap() {
        let id = model["id"].as_str().unwrap().to_owned();
        model["catalogVersion"] = json!(version);
        model["minEngine"] = json!({ "engine": "llama.cpp", "version": null });
        model["commit"] = json!(format!("{version:040x}"));
        for file in model["files"].as_array_mut().unwrap() {
            let bytes = body(&id, file["path"].as_str().unwrap(), version);
            file["bytes"] = json!(bytes.len());
            file["sha256"] = json!(hex(&Sha256::digest(&bytes)));
        }
    }
    edit(&mut value);
    Catalog::parse(&value.to_string()).unwrap()
}

/// `edit` for [`catalog`]: the bundled engine floors back.
pub(super) fn shipped_engines(value: &mut Value) {
    let bundled: Value = serde_json::from_str(include_str!("../catalog.json")).unwrap();
    for (model, real) in value["models"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(bundled["models"].as_array().unwrap())
    {
        model["minEngine"] = real["minEngine"].clone();
    }
}

pub(super) fn url(catalog: &Catalog, model: &str, file: usize) -> String {
    let m = catalog.model(model).unwrap();
    format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        m.repo, m.commit, m.files[file].path
    )
}

/// Answers every file of `catalog` with its body.
pub(super) fn serving(catalog: &Catalog) -> ScriptedTransport {
    let mut transport = ScriptedTransport::new();
    for m in &catalog.models {
        for (at, f) in m.files.iter().enumerate() {
            let bytes = body(&m.id, &f.path, m.catalog_version);
            transport = transport.answer(&url(catalog, &m.id, at), Scripted::ok(&bytes));
        }
    }
    transport
}

/// Transport that holds requests for one address until opened.
pub(super) struct Held {
    pub(super) inner: ScriptedTransport,
    pub(super) url: String,
    pub(super) open: Mutex<bool>,
    pub(super) wake: Condvar,
    pub(super) reached: AtomicBool,
}

impl Held {
    pub(super) fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.wake.notify_all();
    }
}

impl Transport for Held {
    fn get(&self, request: &Request<'_>) -> Result<Response, DownloadError> {
        if request.url == self.url {
            self.reached.store(true, Ordering::SeqCst);
            let mut open = self.open.lock().unwrap();
            while !*open {
                open = self.wake.wait(open).unwrap();
            }
        }
        self.inner.get(request)
    }
}

#[derive(Default)]
pub(super) struct Events(
    pub(super) Mutex<Vec<(String, Value)>>,
    /// Closed: a `local-ai:move` event waits on the emitting thread until opened (a move held mid-copy).
    pub(super) Gate,
);

#[derive(Default)]
pub(super) struct Gate {
    closed: Mutex<bool>,
    wake: Condvar,
    pub(super) reached: AtomicBool,
}

impl Gate {
    pub(super) fn close(&self) {
        *self.closed.lock().unwrap() = true;
    }

    pub(super) fn open(&self) {
        *self.closed.lock().unwrap() = false;
        self.wake.notify_all();
    }

    fn pass(&self) {
        self.reached.store(true, Ordering::SeqCst);
        let mut closed = self.closed.lock().unwrap();
        while *closed {
            closed = self.wake.wait(closed).unwrap();
        }
    }
}

impl EventSink for Events {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, payload } = event {
            if name == "local-ai:move" {
                self.1.pass();
            }
            self.0.lock().unwrap().push((name, payload));
        }
    }
}

/// One profile across plugin restarts.
pub(super) struct Lab {
    pub(super) tmp: tempfile::TempDir,
    secrets: Arc<MemorySecrets>,
}

pub(super) struct Run {
    rpc: Arc<RpcTable>,
    pub(super) events: Arc<Events>,
    _kit: ServiceLease,
    _leases: Vec<ServiceLease>,
    running: Option<Box<dyn Running>>,
}

impl Lab {
    pub(super) fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
            secrets: Arc::new(MemorySecrets::default()),
        }
    }

    pub(super) fn store(&self) -> StateStore {
        StateStore::app(&AppDirs::under(self.tmp.path()))
    }

    pub(super) fn models_dir(&self) -> PathBuf {
        self.store().plugin_dir(&ID).join("models")
    }

    pub(super) fn cx(
        &self,
        rpc: &Arc<RpcTable>,
        services: &Arc<Services>,
        events: &Arc<Events>,
    ) -> PluginCx {
        PluginCx::new(
            ID,
            CxWorkspace::App,
            self.store().plugin_dir(&ID),
            None,
            Context::welcome(Distribution::Direct),
            Arc::clone(events) as Arc<dyn EventSink>,
        )
        .with_store(self.store())
        .with_app(AppServices::new(
            Arc::clone(rpc),
            Arc::clone(services),
            Arc::new(RwLock::new(AppSettings::default())),
        ))
    }

    pub(super) fn kit(&self, transport: Arc<dyn Transport>) -> DownloadKit {
        DownloadKit {
            transport,
            secrets: Arc::clone(&self.secrets) as _,
        }
    }

    pub(super) fn start(&self, catalog: Catalog, transport: Arc<dyn Transport>) -> Run {
        self.start_with(catalog, transport, |_| Vec::new())
    }

    /// [`Lab::start`] with more app services (`provide` keeps their leases for the run).
    pub(super) fn start_with(
        &self,
        catalog: Catalog,
        transport: Arc<dyn Transport>,
        provide: impl FnOnce(&Arc<Services>) -> Vec<ServiceLease>,
    ) -> Run {
        let rpc = RpcTable::new();
        let services = Services::new();
        let leases = provide(&services);
        let kit = services.provide(Arc::new(self.kit(transport)));
        let events = Arc::new(Events::default());
        let running = plugin_with(catalog)
            .start(&self.cx(&rpc, &services, &events))
            .unwrap();
        Run {
            rpc,
            events,
            _kit: kit,
            _leases: leases,
            running: Some(running),
        }
    }
}

impl Run {
    pub(super) fn call(&self, method: &str, args: Value) -> Result<Value, IpcError> {
        self.rpc.call(
            "local-ai",
            method,
            &RpcCall {
                window: "main".into(),
                args,
            },
        )
    }

    pub(super) fn state(&self) -> Value {
        self.call("state", json!({})).unwrap()
    }

    pub(super) fn model(&self, id: &str) -> Value {
        let state = self.state();
        state["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["model"] == id)
            .cloned()
            .unwrap()
    }

    pub(super) fn wait(&self, id: &str, kind: &str) -> Value {
        let start = Instant::now();
        loop {
            let model = self.model(id);
            if model["state"]["kind"] == kind {
                return model;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "stuck: {model}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub(super) fn assigned(&self, task: &str) -> Value {
        let state = self.state();
        state["assignments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["task"] == task)
            .unwrap()["model"]
            .clone()
    }

    pub(super) fn event_names(&self) -> Vec<String> {
        self.events
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|(n, _)| n.clone())
            .collect()
    }

    pub(super) fn stop(&mut self) {
        self.running = None;
    }
}

pub(super) fn model_args(id: &str) -> Value {
    json!({ "model": id })
}

pub(super) fn refused(result: Result<Value, IpcError>, kind: ErrorKind, detail: &str) {
    let error = result.unwrap_err();
    assert_eq!(error.kind, kind, "{error:?}");
    assert_eq!(error.detail.as_deref(), Some(detail), "{error:?}");
}

pub(super) fn version_dir(lab: &Lab, lineage: &str, version: u32) -> PathBuf {
    lab.models_dir().join(lineage).join(version.to_string())
}

pub(super) fn files_in(dir: &Path) -> Vec<String> {
    let mut names = entries(dir);
    names.sort();
    names
}

/// AC-1: the state query on a fresh profile reads; nothing created, nothing asked.
#[test]
fn the_state_query_on_a_fresh_profile_creates_nothing() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let transport = Arc::new(serving(&cat));
    let mut run = lab.start(cat, Arc::clone(&transport) as _);
    let state = run.state();
    assert_eq!(state["models"].as_array().unwrap().len(), 5);
    assert!(state["models"]
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["state"]["kind"] == "notInstalled" && m["installedVersion"].is_null()));
    assert_eq!(state["store"]["isDefault"], true);
    assert_eq!(state["store"]["available"], true);
    assert_eq!(state["storeMove"], Value::Null);
    assert_eq!(state["tokenSet"], false);
    assert_eq!(
        state["assignments"].as_array().unwrap().len(),
        Task::ALL.len()
    );
    run.call("cancel", model_args(EMBED)).unwrap();
    run.call("assign", json!({"task": "correction", "model": null}))
        .unwrap();
    run.stop();
    assert_eq!(transport.asked().len(), 0);
    assert_eq!(entries(lab.tmp.path()), Vec::<String>::new());
}

/// AC-3: Get -> files in `<store>/<lineage>/<version>/` + engine manifest; installed; told by events.
#[test]
fn get_downloads_every_file_into_the_version_folder() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    let after = run.call("get", model_args(E2B)).unwrap();
    let state = &after["models"].as_array().unwrap()[1];
    assert_eq!(state["model"], E2B);
    let model = run.wait(E2B, "installed");
    assert_eq!(model["state"]["catalogVersion"], 1);
    assert_eq!(model["installedVersion"], 1);
    let dir = version_dir(&lab, "gemma-4-e2b-it", 1);
    let mut expected = vec![
        format!("{E2B}.installed.json"),
        "gemma-4-E2B-it-Q4_0.gguf".to_owned(),
        "mmproj-gemma-4-E2B-it-Q8_0.gguf".to_owned(),
    ];
    expected.sort();
    assert_eq!(files_in(&dir), expected);
    assert_eq!(
        std::fs::read(dir.join("gemma-4-E2B-it-Q4_0.gguf")).unwrap(),
        body(E2B, "gemma-4-E2B-it-Q4_0.gguf", 1)
    );
    let names = run.event_names();
    assert!(names.iter().any(|n| n == "local-ai:state"));
    assert!(names.iter().any(|n| n == "local-ai:progress"));
    // a second Get of an installed model asks nothing
    run.call("get", model_args(E2B)).unwrap();
    assert_eq!(run.model(E2B)["state"]["kind"], "installed");
}

/// AC-4: terms needing acceptance -> refused before any byte; accepted once (recorded), then Get works.
#[test]
fn terms_are_accepted_before_the_first_byte_and_remembered() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let transport = Arc::new(serving(&cat));
    let mut run = lab.start(cat.clone(), Arc::clone(&transport) as _);
    assert_eq!(run.model(LFM)["blocked"], "license");
    refused(
        run.call("get", model_args(LFM)),
        ErrorKind::Denied,
        "license",
    );
    assert_eq!(transport.asked().len(), 0);
    assert_eq!(entries(lab.tmp.path()), Vec::<String>::new());

    let after = run.call("acceptLicense", model_args(LFM)).unwrap();
    let url = cat.model(LFM).unwrap().license.url.clone();
    assert_eq!(after["acceptedLicenses"], json!([url]));
    let saved: Value = serde_json::from_slice(
        &std::fs::read(lab.store().plugin_dir(&ID).join(state::FILE)).unwrap(),
    )
    .unwrap();
    let record = &saved["data"]["acceptances"][0];
    assert_eq!(record["model"], LFM);
    assert_eq!(record["licenseUrl"], url.as_str());
    assert!(record["acceptedAt"].as_u64().unwrap() > 0);
    assert!(run.model(LFM)["blocked"].is_null());
    run.call("get", model_args(LFM)).unwrap();
    run.wait(LFM, "installed");
    run.stop();

    // same terms: not asked again after a restart
    let run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    assert!(run.model(LFM)["blocked"].is_null());
    assert_eq!(run.state()["acceptedLicenses"], json!([url]));
}

/// AC-5: gated -> refused without a token; token set (never shown) -> Get sends it to Hugging Face.
#[test]
fn gated_models_need_a_token_that_is_never_shown() {
    let lab = Lab::new();
    let cat = catalog(1, |v| v["models"][0]["gated"] = json!(true));
    let transport = Arc::new(serving(&cat));
    let run = lab.start(cat, Arc::clone(&transport) as _);
    assert_eq!(run.state()["tokenSet"], false);
    assert_eq!(run.model(EMBED)["blocked"], "token");
    refused(
        run.call("get", model_args(EMBED)),
        ErrorKind::Denied,
        "token",
    );
    refused(
        run.call("setToken", json!({"token": "  \n"})),
        ErrorKind::InvalidData,
        "empty",
    );
    assert_eq!(transport.asked().len(), 0);

    let after = run
        .call("setToken", json!({"token": " hf_secret_value\n"}))
        .unwrap();
    assert_eq!(after["tokenSet"], true);
    assert!(!after.to_string().contains("hf_secret_value"));
    assert!(run.model(EMBED)["blocked"].is_null());
    run.call("get", model_args(EMBED)).unwrap();
    run.wait(EMBED, "installed");
    let asked = transport.asked();
    assert!(asked[0].authorized);
    assert!(asked[0].url.starts_with("https://huggingface.co/"));

    let after = run.call("clearToken", json!({})).unwrap();
    assert_eq!(after["tokenSet"], false);
}

/// AC-3: a stopped download keeps its part -> listed paused at the next start (never restarted), Get resumes.
#[test]
fn a_part_is_listed_paused_at_start_and_resumed_by_get() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let whole = body(EMBED, &cat.model(EMBED).unwrap().files[0].path, 1);
    let at = url(&cat, EMBED, 0);
    let transport = Arc::new(
        ScriptedTransport::new()
            .answer(&at, Scripted::ok(&whole).cut_after(100))
            .answer(
                &at,
                Scripted::partial(100, whole.len() as u64, &whole[100..]),
            ),
    );
    let mut run = lab.start(cat.clone(), Arc::clone(&transport) as _);
    run.call("get", model_args(EMBED)).unwrap();
    let failed = run.wait(EMBED, "failed");
    assert_eq!(failed["state"]["error"]["kind"], "offline");
    run.stop();

    let run = lab.start(cat, Arc::clone(&transport) as _);
    let paused = run.model(EMBED);
    assert_eq!(paused["state"]["kind"], "paused");
    assert_eq!(paused["state"]["done"], 100);
    assert_eq!(paused["state"]["total"], whole.len());
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(transport.asked().len(), 1, "never restarted by itself");
    run.call("get", model_args(EMBED)).unwrap();
    run.wait(EMBED, "installed");
    assert_eq!(transport.asked().last().unwrap().from, Some(100));
}

/// AC-3: cancel keeps the part (paused, also after a restart); delete part drops it.
#[test]
fn cancel_keeps_the_part_and_delete_part_drops_it() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let held = Arc::new(Held {
        inner: serving(&cat),
        url: url(&cat, E2B, 1),
        open: Mutex::new(false),
        wake: Condvar::new(),
        reached: AtomicBool::new(false),
    });
    let mut run = lab.start(cat.clone(), Arc::clone(&held) as _);
    run.call("get", model_args(E2B)).unwrap();
    let start = Instant::now();
    while !held.reached.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    run.call("cancel", model_args(E2B)).unwrap();
    held.release();
    let first = cat.model(E2B).unwrap().files[0].bytes;
    let paused = run.wait(E2B, "paused");
    assert_eq!(paused["state"]["done"], first);
    run.stop();

    let run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    let paused = run.model(E2B);
    assert_eq!(paused["state"]["kind"], "paused");
    assert_eq!(paused["state"]["done"], first);
    let after = run.call("deletePart", model_args(E2B)).unwrap();
    assert_eq!(after["models"][1]["state"]["kind"], "notInstalled");
    let left = files_in(&version_dir(&lab, "gemma-4-e2b-it", 1));
    assert!(left.iter().all(|n| !n.contains(".part")), "{left:?}");
}

/// AC-6 + AC-8: remove deletes the files and unassigns the tasks that used it.
#[test]
fn remove_deletes_the_files_and_unassigns_its_tasks() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    run.call("get", model_args(E2B)).unwrap();
    run.wait(E2B, "installed");
    run.call("assign", json!({"task": "correction", "model": E2B}))
        .unwrap();
    run.call("assign", json!({"task": "assistant", "model": E2B}))
        .unwrap();
    assert_eq!(run.assigned("correction"), E2B);
    let after = run.call("remove", model_args(E2B)).unwrap();
    assert_eq!(after["models"][1]["state"]["kind"], "notInstalled");
    assert!(after["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["model"].is_null()));
    assert!(!lab.models_dir().join("gemma-4-e2b-it").exists());
    let saved = std::fs::read_to_string(lab.store().plugin_dir(&ID).join(state::FILE)).unwrap();
    assert!(!saved.contains("gemma-4-e2b-it"), "{saved}");
}

/// AC-8: only installed + compatible models are assigned.
#[test]
fn assign_refuses_incompatible_or_missing_models() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    refused(
        run.call("assign", json!({"task": "correction", "model": EMBED})),
        ErrorKind::Unsupported,
        "incompatible",
    );
    refused(
        run.call("assign", json!({"task": "semanticIndex", "model": EMBED})),
        ErrorKind::Conflict,
        "notInstalled",
    );
    let unknown = run
        .call("assign", json!({"task": "correction", "model": "nope"}))
        .unwrap_err();
    assert_eq!(unknown.kind, ErrorKind::NotFound);
    run.call("get", model_args(EMBED)).unwrap();
    run.wait(EMBED, "installed");
    run.call("assign", json!({"task": "semanticIndex", "model": EMBED}))
        .unwrap();
    assert_eq!(run.assigned("semanticIndex"), EMBED);
    refused(
        run.call("assign", json!({"task": "correction", "model": EMBED})),
        ErrorKind::Unsupported,
        "incompatible",
    );
    run.call("assign", json!({"task": "semanticIndex", "model": null}))
        .unwrap();
    assert!(run.assigned("semanticIndex").is_null());
}

/// AC-6: newer catalog version -> update available; update downloads beside the old one, swaps after verify, old
/// removed, assignment follows the lineage. A failed update leaves the old one usable.
#[test]
fn update_swaps_versions_and_assignments_follow_the_lineage() {
    let lab = Lab::new();
    let v1 = catalog(1, |_| {});
    let mut run = lab.start(v1.clone(), Arc::new(serving(&v1)));
    refused(
        run.call("update", model_args(EMBED)),
        ErrorKind::Conflict,
        "noUpdate",
    );
    run.call("get", model_args(EMBED)).unwrap();
    run.wait(EMBED, "installed");
    run.call("assign", json!({"task": "semanticIndex", "model": EMBED}))
        .unwrap();
    run.stop();

    // v2 whose file arrives wrong: the update fails, v1 stays
    let v2 = catalog(2, |_| {});
    let wrong = vec![b'x'; usize::try_from(v2.model(EMBED).unwrap().files[0].bytes).unwrap()];
    let broken =
        Arc::new(ScriptedTransport::new().answer(&url(&v2, EMBED, 0), Scripted::ok(&wrong)));
    let mut run = lab.start(v2.clone(), broken);
    let model = run.model(EMBED);
    assert_eq!(
        model["state"],
        json!({"kind": "updateAvailable", "installedVersion": 1})
    );
    assert_eq!(run.assigned("semanticIndex"), EMBED);
    run.call("update", model_args(EMBED)).unwrap();
    let failed = run.wait(EMBED, "failed");
    assert_eq!(failed["state"]["error"]["kind"], "integrity");
    assert_eq!(failed["installedVersion"], 1);
    assert_eq!(run.assigned("semanticIndex"), EMBED);
    assert!(version_dir(&lab, "embeddinggemma-2", 1).is_dir());
    run.stop();

    let mut run = lab.start(v2.clone(), Arc::new(serving(&v2)));
    run.call("update", model_args(EMBED)).unwrap();
    let model = run.wait(EMBED, "installed");
    assert_eq!(model["state"]["catalogVersion"], 2);
    assert_eq!(model["installedVersion"], 2);
    assert!(!version_dir(&lab, "embeddinggemma-2", 1).exists());
    assert!(version_dir(&lab, "embeddinggemma-2", 2).is_dir());
    assert_eq!(run.assigned("semanticIndex"), EMBED);
    run.stop();
}

/// AC-8: `model_for(task)` -> the installed model assigned to it, else none.
#[test]
fn model_for_answers_the_assigned_installed_model() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let rpc = RpcTable::new();
    let services = Services::new();
    let events = Arc::new(Events::default());
    let cx = lab.cx(&rpc, &services, &events);
    let installs = Installs::new(
        Arc::new(cat.clone()),
        Some(Arc::new(lab.kit(Arc::new(serving(&cat))))),
        lab.models_dir(),
        cx.state_file(state::FILE),
        cx.events().clone(),
        crate::StoreLinks::default(),
        Some(crate::engine::files::tests::pin()),
        None,
    );
    assert_eq!(installs.model_for(Task::Dictation), ModelFor::NoModel);
    installs.get("whisper-large-v3-turbo-q5_0").unwrap();
    let start = Instant::now();
    while installs
        .assign(Task::Dictation, Some("whisper-large-v3-turbo-q5_0"))
        .is_err()
    {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    let ModelFor::Assigned(model) = installs.model_for(Task::Dictation) else {
        panic!("not assigned");
    };
    assert_eq!(model.id, "whisper-large-v3-turbo-q5_0");
    assert_eq!(model.catalog_version, 1);
    assert_eq!(model.dir, version_dir(&lab, &model.lineage, 1));
    assert!(model.dir.is_dir());
    // AC-10: the engine finds every file of the installed version
    let files = crate::engine::model_files(&model, &cat);
    assert_eq!(files.files.len(), model.files.len());
    assert!(files.files.iter().all(|(_, path)| path.is_file()));
    assert_eq!(installs.model_for(Task::Correction), ModelFor::NoModel);
    installs.shutdown();
}

/// D-11 (Windows mmap lock): remove unloads the sessions of that model's lineage first; a store change unloads
/// every session. `installed_model` answers the usable version by id.
#[test]
fn remove_and_store_change_unload_the_engine_sessions_first() {
    use crate::catalog::Needs;
    use crate::engine::tests::Workers;
    use crate::engine::{CancelToken, Device, FakeEngine, LoadOpts};
    use crate::machine::MachineProfile;
    use crate::runtime::Runtime;

    fn plenty() -> MachineProfile {
        MachineProfile {
            ram_available: Some(64 << 30),
            ..MachineProfile::unknown()
        }
    }
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let rpc = RpcTable::new();
    let services = Services::new();
    let events = Arc::new(Events::default());
    let cx = lab.cx(&rpc, &services, &events);
    let workers = Workers::default();
    let runtime = Arc::new(Runtime::with_launch(
        workers.launch(FakeEngine::new),
        plenty,
    ));
    let installs = Installs::new(
        Arc::new(cat.clone()),
        Some(Arc::new(lab.kit(Arc::new(serving(&cat))))),
        lab.models_dir(),
        cx.state_file(state::FILE),
        cx.events().clone(),
        crate::StoreLinks::default(),
        Some(crate::engine::files::tests::pin()),
        Some(Arc::clone(&runtime)),
    );
    let id = "whisper-large-v3-turbo-q5_0";
    assert!(matches!(
        installs.installed_model(id),
        Err(crate::InstallError::NotInstalled(_))
    ));
    installs.get(id).unwrap();
    let start = Instant::now();
    // installed + the download no longer running (remove refuses a running one)
    let installed = || {
        installs
            .state()
            .models
            .iter()
            .any(|m| m.model == id && matches!(m.state, crate::InstallState::Installed { .. }))
    };
    while !installed() {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    let model = installs.installed_model(id).unwrap();
    assert_eq!(model.id, id);
    assert_eq!(model.dir, version_dir(&lab, &model.lineage, 1));
    let needs = Needs {
        ram_min: 1,
        ram_rec: 1,
        vram_rec: 1,
    };
    let opts = LoadOpts {
        context_len: 512,
        device: Device::Cpu,
        threads: Some(1),
    };
    let load = |lineage: &str| {
        runtime
            .load(
                lineage,
                &needs,
                crate::engine::model_files(&model, &cat),
                opts,
                &|_| {},
                &CancelToken::new(),
            )
            .unwrap()
    };
    load(&model.lineage);
    load("other");
    installs.remove(id).unwrap();
    assert_eq!(workers.kills(), 1, "this model's worker ended");
    assert_eq!(runtime.loaded(), ["other"]);
    assert!(!model.dir.exists());
    let elsewhere = lab.tmp.path().join("elsewhere");
    installs
        .set_store(Some(&elsewhere.display().to_string()), None)
        .unwrap();
    assert_eq!(workers.kills(), 2, "store change: every worker ended");
    assert_eq!(runtime.loaded(), Vec::<String>::new());
    installs.shutdown();
}

/// D-6: acceptances + assignments survive a restart (profile state file).
#[test]
fn the_state_survives_a_restart() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let mut run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    run.call("acceptLicense", model_args(LFM)).unwrap();
    run.call("get", model_args(LFM)).unwrap();
    run.wait(LFM, "installed");
    run.call("assign", json!({"task": "assistant", "model": LFM}))
        .unwrap();
    // free space moves with the rest of the disk
    let without_free = |mut state: Value| {
        state["store"]["freeBytes"] = Value::Null;
        state
    };
    let before = without_free(run.state());
    run.stop();
    let run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    assert_eq!(without_free(run.state()), before);
    assert_eq!(run.assigned("assistant"), LFM);
}

/// Off: running downloads stop (part kept) and the RPC goes.
#[test]
fn turning_off_stops_downloads() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let held = Arc::new(Held {
        inner: serving(&cat),
        url: url(&cat, E2B, 1),
        open: Mutex::new(false),
        wake: Condvar::new(),
        reached: AtomicBool::new(false),
    });
    let mut run = lab.start(cat, Arc::clone(&held) as _);
    run.call("get", model_args(E2B)).unwrap();
    let start = Instant::now();
    while !held.reached.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    let releaser = {
        let held = Arc::clone(&held);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            held.release();
        })
    };
    run.stop();
    releaser.join().unwrap();
    assert!(run.call("state", json!({})).is_err());
    assert!(!version_dir(&lab, "gemma-4-e2b-it", 1)
        .join(format!("{E2B}.installed.json"))
        .exists());
}

/// S5: acceptance is per terms revision: a record of no | another revision asks again; accepting replaces it.
#[test]
fn a_new_terms_revision_is_asked_again() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let url = cat.model(LFM).unwrap().license.url.clone();
    lab.store()
        .plugin_file::<state::PluginState>(&ID, state::FILE)
        .save(&state::PluginState {
            acceptances: vec![state::Acceptance {
                model: LFM.to_owned(),
                license_url: url.clone(),
                revision: None,
                accepted_at: 1,
            }],
            ..state::PluginState::default()
        })
        .unwrap();
    let mut run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    assert_eq!(run.model(LFM)["blocked"], "license");
    run.call("acceptLicense", model_args(LFM)).unwrap();
    assert!(run.model(LFM)["blocked"].is_null());
    run.stop();

    let v2 = catalog(1, |v| {
        for m in v["models"].as_array_mut().unwrap() {
            if m["id"] == LFM {
                m["license"]["revision"] = json!("v2.0");
            }
        }
    });
    let run = lab.start(v2.clone(), Arc::new(serving(&v2)));
    assert_eq!(run.model(LFM)["blocked"], "license");
    run.call("acceptLicense", model_args(LFM)).unwrap();
    assert!(run.model(LFM)["blocked"].is_null());
    let saved: Value = serde_json::from_slice(
        &std::fs::read(lab.store().plugin_dir(&ID).join(state::FILE)).unwrap(),
    )
    .unwrap();
    let records = saved["data"]["acceptances"].as_array().unwrap();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["licenseUrl"], url.as_str());
    assert_eq!(records[0]["revision"], "v2.0");
}

/// S6: a token longer than 1 KiB is refused (never stored).
#[test]
fn an_overlong_token_is_refused() {
    let lab = Lab::new();
    let cat = catalog(1, |v| v["models"][0]["gated"] = json!(true));
    let run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    refused(
        run.call("setToken", json!({ "token": "x".repeat(1025) })),
        ErrorKind::InvalidData,
        "tooLong",
    );
    assert_eq!(run.state()["tokenSet"], false);
    let after = run
        .call("setToken", json!({ "token": "x".repeat(1024) }))
        .unwrap();
    assert_eq!(after["tokenSet"], true);
}

/// Stale: an old version whose removal fails (file held open) after an update is recorded, kept by queries, and
/// removed by the next action that writes to the store.
#[cfg(windows)]
#[test]
fn an_old_version_that_cannot_be_removed_goes_at_the_next_action() {
    use std::os::windows::fs::OpenOptionsExt;
    let lab = Lab::new();
    let v1 = catalog(1, |_| {});
    let mut run = lab.start(v1.clone(), Arc::new(serving(&v1)));
    run.call("get", model_args(EMBED)).unwrap();
    run.wait(EMBED, "installed");
    run.stop();
    let old = version_dir(&lab, "embeddinggemma-2", 1);
    let path = &v1.model(EMBED).unwrap().files[0].path;
    let name = path.rsplit('/').next().unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(old.join(name))
        .unwrap();
    let v2 = catalog(2, |_| {});
    let run = lab.start(v2.clone(), Arc::new(serving(&v2)));
    run.call("update", model_args(EMBED)).unwrap();
    let model = run.wait(EMBED, "installed");
    assert_eq!(model["installedVersion"], 2);
    let stale = || -> usize {
        let saved: Value = serde_json::from_slice(
            &std::fs::read(lab.store().plugin_dir(&ID).join(state::FILE)).unwrap(),
        )
        .unwrap();
        saved["data"]["stale"].as_array().map_or(0, Vec::len)
    };
    assert_eq!(stale(), 1);
    drop(held);
    run.state();
    assert!(old.join(name).is_file(), "a query removes nothing");
    run.call("get", model_args("whisper-large-v3-turbo-q5_0"))
        .unwrap();
    run.wait("whisper-large-v3-turbo-q5_0", "installed");
    assert!(!old.exists());
    assert_eq!(stale(), 0);
}

/// Bugfix unrunnable models AC-2/AC-3: the pinned engine (llama.cpp b11429) runs Gemma 4 only; the others are listed
/// with what the engine lacks, Get + assign refused (typed), nothing created.
#[test]
fn models_the_pinned_engine_cannot_run_are_listed_with_what_it_lacks() {
    let lab = Lab::new();
    let cat = catalog(1, shipped_engines);
    let run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    let need = |id: &str| run.model(id)["engineNeed"].clone();
    assert_eq!(need(E2B), Value::Null);
    assert_eq!(need("gemma-4-e4b-it-q4_0"), Value::Null);
    assert_eq!(need(EMBED), "newer");
    assert_eq!(need(LFM), "newer");
    assert_eq!(need(WHISPER), "missing");
    run.call("acceptLicense", model_args(LFM)).unwrap();
    refused(
        run.call("get", model_args(LFM)),
        ErrorKind::Unsupported,
        "newerEngine",
    );
    refused(
        run.call("get", model_args(WHISPER)),
        ErrorKind::Unsupported,
        "missingEngine",
    );
    refused(
        run.call("assign", json!({"task": "semanticIndex", "model": EMBED})),
        ErrorKind::Unsupported,
        "newerEngine",
    );
    assert_eq!(run.model(LFM)["state"]["kind"], "notInstalled");
    assert!(!lab.models_dir().exists(), "nothing created");
}

fn plenty() -> crate::machine::MachineProfile {
    crate::machine::MachineProfile {
        ram_available: Some(64 << 30),
        ..crate::machine::MachineProfile::unknown()
    }
}

/// Bugfix AC-6: a newer pin runs the same catalog with no other change. AC-4: a model installed under it stays
/// listed (installed, assignment kept) once the pin is older again, but is not assignable, not loaded, not tested.
#[test]
fn a_newer_pin_runs_them_and_an_earlier_install_stays_but_never_loads() {
    use crate::catalog::EngineNeed;
    use crate::engine::files::PINS;

    static NEWER: WorkerPin = WorkerPin {
        build: "b11960",
        ..PINS[0]
    };
    let lab = Lab::new();
    let cat = catalog(1, shipped_engines);
    let rpc = RpcTable::new();
    let services = Services::new();
    let events = Arc::new(Events::default());
    let cx = lab.cx(&rpc, &services, &events);
    let installs_on = |pin: &'static WorkerPin| {
        Installs::new(
            Arc::new(cat.clone()),
            Some(Arc::new(lab.kit(Arc::new(serving(&cat))))),
            lab.models_dir(),
            cx.state_file(state::FILE),
            cx.events().clone(),
            crate::StoreLinks::default(),
            Some(pin),
            None,
        )
    };
    let need = |installs: &Installs, id: &str| {
        installs
            .state()
            .models
            .into_iter()
            .find(|m| m.model == id)
            .unwrap()
            .engine_need
    };
    let newer = installs_on(&NEWER);
    assert_eq!(need(&newer, LFM), None);
    assert_eq!(need(&newer, EMBED), None);
    assert_eq!(need(&newer, WHISPER), Some(EngineNeed::Missing));
    newer.accept_license(LFM).unwrap();
    newer.get(LFM).unwrap();
    let start = Instant::now();
    while newer.assign(Task::Correction, Some(LFM)).is_err() {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    newer.shutdown();
    drop(newer);

    let pinned = installs_on(&PINS[0]);
    let state = pinned.state();
    let lfm = state.models.iter().find(|m| m.model == LFM).unwrap();
    assert!(matches!(lfm.state, crate::InstallState::Installed { .. }));
    assert_eq!(lfm.engine_need, Some(EngineNeed::Newer));
    let correction = state
        .assignments
        .iter()
        .find(|a| a.task == Task::Correction)
        .unwrap();
    assert_eq!(correction.model.as_deref(), Some(LFM), "choice kept");
    assert!(matches!(
        pinned.assign(Task::Assistant, Some(LFM)),
        Err(crate::InstallError::EngineNeeded {
            need: EngineNeed::Newer,
            ..
        })
    ));

    never_loads(&pinned, &cat, &cx);
    pinned.shutdown();
}

/// AC-4: `pinned`'s assigned model needs a newer engine: open + test refuse it typed, no worker started.
fn never_loads(pinned: &Arc<Installs>, cat: &Catalog, cx: &PluginCx) {
    use crate::engine::files::{WorkerFiles, PINS};
    use crate::engine::tests::Workers;
    use crate::engine::{CancelToken, EngineError, FakeEngine};
    use crate::host::{HostError, ModelHost};
    use crate::runtime::Runtime;
    use crate::testrun::{EngineNews, TestError, TestParts, Tester};

    let workers = Workers::default();
    let runtime = Arc::new(Runtime::with_launch(
        workers.launch(FakeEngine::new),
        plenty,
    ));
    let news = Arc::new(EngineNews::default());
    let files = Arc::new(WorkerFiles::with_pin(
        Some(&PINS[0]),
        None,
        cx.state_dir.join(crate::engine::files::DIR),
        news.listener(),
    ));
    let host = Arc::new(ModelHost::new(
        Arc::clone(pinned),
        Arc::new(cat.clone()),
        runtime,
        files,
    ));
    let error = host
        .open(Task::Correction, 4096, &|_| {}, &CancelToken::new())
        .unwrap_err();
    assert!(
        matches!(&error, HostError::Engine(EngineError::EngineTooOld { needed }) if needed == "b11960"),
        "{error:?}"
    );
    let ipc = IpcError::from(error);
    assert_eq!(
        (ipc.kind, ipc.detail.as_deref()),
        (ErrorKind::Unsupported, Some("engineTooOld"))
    );
    let tester = Tester::new(TestParts {
        host,
        news,
        events: cx.events().clone(),
    });
    assert!(matches!(
        tester.start(LFM),
        Err(TestError::Engine(EngineError::EngineTooOld { .. }))
    ));
    assert_eq!(workers.launched(), 0, "never loaded");
}
