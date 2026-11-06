//! Correction runs over `local-ai`'s in-process [`FakeEngine`] (echoes the last user turn word by word, paced for
//! cancel; [`RAMBLE`] in a piece -> answer cut at its cap): dormant status, phase order (engine fetch first when
//! missing), discards (length, thinking,
//! too long), answer cap + sampling, cancel, new run cancels the old one, warm session (idle unload, reassignment),
//! plugin off (no thread, nothing loaded), plugin RPC without the model service.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use permark_core::conditions::{Context, Distribution};
use permark_core::events::{CoreEvent, EventSink};
use permark_core::plugins::{AppServices, CxWorkspace, PluginCx, RpcCall, RpcTable, Services};
use permark_core::settings::AppSettings;
use permark_core::IpcError;
use permark_plugin_local_ai::engine::{
    CancelToken, ChatRequest, Device, Engine, EngineError, FakeEngine, FileRole, Finish, GenEvent,
    LoadOpts, ModelFiles, Role, SessionId, Usage,
};
use permark_plugin_local_ai::{HostError, HostProgress, Task};
use serde_json::{json, Value};

use crate::prompt;
use crate::run::{Corrector, Models, Piece, Session, CONTEXT, THINKING};

/// Piece word that makes the fake answer hit its cap.
const RAMBLE: &str = "ramble";
const SHORT_IDLE: Duration = Duration::from_millis(150);
const PACE: Duration = Duration::from_millis(20);

#[derive(Default)]
struct Events(Mutex<Vec<(String, Value)>>);

impl EventSink for Events {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, payload } = event {
            self.0.lock().unwrap().push((name, payload));
        }
    }
}

impl Events {
    /// Phases of run `run` so far.
    fn phases(&self, run: &str) -> Vec<Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, p)| name == "correction:run" && p["run"] == run)
            .map(|(_, p)| p["phase"].clone())
            .collect()
    }

    /// Phases of `run` once its last one (`done` | `cancelled` | `failed`) went out.
    fn ended(&self, run: &str) -> Vec<Value> {
        wait(|| {
            let phases = self.phases(run);
            phases
                .last()
                .is_some_and(|p| {
                    ["done", "cancelled", "failed"].contains(&p["kind"].as_str().unwrap())
                })
                .then_some(phases)
        })
    }
}

fn wait<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(value) = f() {
            return value;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Model source over one [`FakeEngine`]; counts loads, records requests.
struct Fake {
    engine: Arc<FakeEngine>,
    assigned: AtomicBool,
    /// Sessions keep serving (false = model reassigned).
    serving: Arc<AtomicBool>,
    opens: AtomicUsize,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    /// Engine files missing: bytes the next open fetches first (as `ModelHost`), then installed.
    engine_missing: Mutex<Option<u64>>,
    /// The engine fetch never ends before a cancel.
    engine_held: AtomicBool,
}

impl Fake {
    fn new(engine: FakeEngine) -> Arc<Self> {
        Arc::new(Self {
            engine: Arc::new(engine),
            assigned: AtomicBool::new(true),
            serving: Arc::new(AtomicBool::new(true)),
            opens: AtomicUsize::new(0),
            requests: Arc::default(),
            engine_missing: Mutex::new(None),
            engine_held: AtomicBool::new(false),
        })
    }

    fn opens(&self) -> usize {
        self.opens.load(Ordering::Relaxed)
    }
}

impl Models for Fake {
    fn available(&self) -> bool {
        self.assigned.load(Ordering::Relaxed)
    }

    fn open(
        &self,
        progress: &dyn Fn(HostProgress),
        cancel: &CancelToken,
    ) -> Result<Box<dyn Session>, IpcError> {
        if !self.available() {
            return Err(HostError::NoModel(Task::Correction).into());
        }
        let missing = *self.engine_missing.lock().unwrap();
        if let Some(total) = missing {
            progress(HostProgress::Engine { received: 0, total });
            if self.engine_held.load(Ordering::Relaxed) {
                wait(|| cancel.is_cancelled().then_some(()));
                return Err(permark_plugin_local_ai::engine_ipc(&EngineError::Cancelled));
            }
            progress(HostProgress::Engine {
                received: total,
                total,
            });
            *self.engine_missing.lock().unwrap() = None;
        }
        progress(HostProgress::Loading(0.0));
        self.opens.fetch_add(1, Ordering::Relaxed);
        self.serving.store(true, Ordering::Relaxed);
        let files = ModelFiles {
            model_id: "fake".into(),
            catalog_version: 1,
            files: vec![(FileRole::Weights, PathBuf::from("fake.gguf"))],
            prefix_profile: None,
            prefixes: None,
            prompt: None,
            min_engine: None,
        };
        let opts = LoadOpts {
            context_len: CONTEXT,
            device: Device::Auto,
            threads: None,
        };
        let id = self
            .engine
            .load(
                files,
                opts,
                &|value| progress(HostProgress::Loading(value)),
                cancel,
            )
            .map_err(|e| permark_plugin_local_ai::engine_ipc(&e))?;
        Ok(Box::new(FakeSession {
            engine: Arc::clone(&self.engine),
            id,
            serving: Arc::clone(&self.serving),
            requests: Arc::clone(&self.requests),
        }))
    }
}

struct FakeSession {
    engine: Arc<FakeEngine>,
    id: SessionId,
    serving: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl Drop for FakeSession {
    fn drop(&mut self) {
        self.engine.unload(self.id);
    }
}

impl Session for FakeSession {
    fn serves(&self) -> bool {
        self.serving.load(Ordering::Relaxed)
    }

    fn count_tokens(&self, text: &str) -> Result<usize, EngineError> {
        self.engine.count_tokens(self.id, text)
    }

    fn generate(
        &self,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        self.requests.lock().unwrap().push(request.clone());
        let last = &request.messages[request.messages.len() - 1].content;
        if last.contains(RAMBLE) {
            sink(GenEvent::Delta("and so on".into()));
            sink(GenEvent::Done {
                usage: Usage::default(),
                finish: Finish::Length,
            });
            return Ok(());
        }
        self.engine.generate(self.id, request, sink, cancel)
    }
}

struct Lab {
    fake: Arc<Fake>,
    events: Arc<Events>,
    corrector: Corrector,
}

fn cx(events: &Arc<Events>) -> PluginCx {
    PluginCx::new(
        crate::ID,
        CxWorkspace::App,
        std::env::temp_dir().join("permark-correction-unused"),
        None,
        Context::welcome(Distribution::Direct),
        Arc::clone(events) as Arc<dyn EventSink>,
    )
}

impl Lab {
    fn new(engine: FakeEngine) -> Self {
        let fake = Fake::new(engine);
        let events = Arc::new(Events::default());
        let corrector = Corrector::new(
            Arc::clone(&fake) as Arc<dyn Models>,
            cx(&events).events().clone(),
            SHORT_IDLE,
        );
        Self {
            fake,
            events,
            corrector,
        }
    }

    fn paced() -> Self {
        Self::new(FakeEngine::new().paced(PACE))
    }

    fn run(&self, run: &str, texts: &[&str]) {
        let pieces = (1u32..)
            .zip(texts)
            .map(|(id, text)| Piece {
                id,
                text: (*text).to_owned(),
            })
            .collect();
        self.corrector.start(run.to_owned(), pieces).unwrap();
    }

    fn loaded(&self) -> usize {
        self.fake.engine.loaded()
    }
}

fn kinds(phases: &[Value]) -> Vec<&str> {
    phases.iter().map(|p| p["kind"].as_str().unwrap()).collect()
}

fn piece(phases: &[Value], id: u32) -> &Value {
    phases
        .iter()
        .find(|p| p["kind"] == "piece" && p["id"] == id)
        .unwrap_or_else(|| panic!("piece {id} in {phases:?}"))
}

fn discarded(phases: &[Value], id: u32) -> String {
    let result = &piece(phases, id)["result"];
    assert_eq!(result["kind"], "discarded", "{result}");
    result["reason"].as_str().unwrap().to_owned()
}

/// AC-1 dormancy: status from the state alone; nothing opened before a run.
#[test]
fn status_reads_the_assignment_and_loads_nothing() {
    let lab = Lab::new(FakeEngine::new());
    lab.fake.assigned.store(false, Ordering::Relaxed);
    assert!(!lab.corrector.status().available);
    lab.fake.assigned.store(true, Ordering::Relaxed);
    assert!(lab.corrector.status().available);
    assert_eq!(lab.fake.opens(), 0);
    assert_eq!(lab.loaded(), 0);
    assert!(!lab.corrector.worker_alive());
}

/// AC-6: loading -> pieces in the given order (first visible before the end) -> done.
#[test]
fn a_run_streams_loading_then_each_piece_in_order_then_done() {
    let lab = Lab::new(FakeEngine::new());
    lab.run("r1", &["Second piece ⟦1⟧ here.", "First piece."]);
    let phases = lab.events.ended("r1");
    assert_eq!(
        kinds(&phases),
        ["loading", "loading", "loading", "loading", "loading", "piece", "piece", "done"]
    );
    assert_eq!(phases[0]["progress"], 0.0);
    assert_eq!(phases[4]["progress"], 1.0);
    assert_eq!(phases[5]["id"], 1);
    assert_eq!(
        phases[5]["result"],
        json!({ "kind": "output", "text": "Second piece ⟦1⟧ here." })
    );
    assert_eq!(phases[6]["id"], 2);
    assert_eq!(lab.fake.opens(), 1);
}

/// AC-8 (bugfix-local-ai-unrunnable-models): engine missing -> its download first (bytes), then loading -> pieces ->
/// done; the next open fetches nothing.
#[test]
fn a_run_gets_a_missing_engine_before_loading() {
    let lab = Lab::new(FakeEngine::new());
    *lab.fake.engine_missing.lock().unwrap() = Some(48 << 20);
    lab.run("r1", &["One."]);
    let phases = lab.events.ended("r1");
    let mut seen = kinds(&phases);
    seen.dedup();
    assert_eq!(seen, ["engine", "loading", "piece", "done"]);
    assert_eq!(
        phases[0],
        json!({ "kind": "engine", "receivedBytes": 0, "totalBytes": 48 << 20 })
    );
    assert_eq!(phases[1]["receivedBytes"], 48 << 20);
    assert_eq!(phases[2], json!({ "kind": "loading", "progress": 0.0 }));
    assert_eq!(lab.fake.opens(), 1);
}

/// AC-8: cancel while the engine downloads -> `cancelled`, nothing loaded.
#[test]
fn cancel_during_the_engine_download_ends_cancelled() {
    let lab = Lab::new(FakeEngine::new());
    *lab.fake.engine_missing.lock().unwrap() = Some(1000);
    lab.fake.engine_held.store(true, Ordering::Relaxed);
    lab.run("r1", &["One."]);
    wait(|| (!lab.events.phases("r1").is_empty()).then_some(()));
    lab.corrector.cancel("r1");
    let phases = lab.events.ended("r1");
    assert_eq!(kinds(&phases), ["engine", "cancelled"]);
    assert_eq!(lab.fake.opens(), 0);
    assert_eq!(lab.loaded(), 0);
}

/// D-1 + D-2: system rules + 2 few-shot turns + raw piece; greedy, fixed seed; cap = ceil(1.3 x tokens) + 32.
#[test]
fn requests_carry_the_prompt_greedy_sampling_and_the_answer_cap() {
    let lab = Lab::new(FakeEngine::new());
    let text = "one two three four five six seven eight nine ten";
    lab.run("r1", &[text]);
    lab.events.ended("r1");
    let requests = lab.fake.requests.lock().unwrap();
    let request = &requests[0];
    let roles: Vec<Role> = request.messages.iter().map(|m| m.role).collect();
    let mut want = vec![Role::System];
    for _ in prompt::FEW_SHOT {
        want.extend([Role::User, Role::Assistant]);
    }
    want.push(Role::User);
    assert_eq!(roles, want);
    assert_eq!(request.messages[0].content, prompt::SYSTEM);
    assert_eq!(request.messages[want.len() - 1].content, text);
    // the first two examples keep a marker (D-3)
    assert!(request.messages[1..5]
        .iter()
        .all(|m| m.content.contains("⟦1⟧")));
    assert_eq!(request.max_tokens, 45);
    assert!(request.sampling.temperature.abs() < f32::EPSILON);
    assert!(request.sampling.seed.is_some());
    assert_eq!(request.tools.len(), 0);
}

#[test]
fn the_answer_cap_is_1_3_times_the_piece_plus_32() {
    assert_eq!(prompt::max_tokens(0), 32);
    assert_eq!(prompt::max_tokens(1), 34);
    assert_eq!(prompt::max_tokens(10), 45);
    assert_eq!(prompt::max_tokens(100), 162);
    assert_eq!(prompt::max_tokens(u32::MAX), u32::MAX);
}

/// D-1: an answer cut at its cap is discarded (`length`); the run goes on.
#[test]
fn an_answer_cut_at_its_cap_is_discarded() {
    let lab = Lab::new(FakeEngine::new());
    lab.run("r1", &[&format!("please {RAMBLE} on"), "Fine text."]);
    let phases = lab.events.ended("r1");
    assert_eq!(discarded(&phases, 1), "length");
    assert_eq!(piece(&phases, 2)["result"]["kind"], "output");
    assert_eq!(kinds(&phases).last(), Some(&"done"));
}

/// D-1: thinking markup in the answer -> discarded (`thinking`).
#[test]
fn a_thinking_leak_is_discarded() {
    let lab = Lab::new(FakeEngine::new());
    lab.run("r1", &[&format!("{THINKING}thought Fixed text.")]);
    let phases = lab.events.ended("r1");
    assert_eq!(discarded(&phases, 1), "thinking");
}

/// D-4 token budget: prompt + piece + answer room over the context -> `tooLong`, never sent to the model.
#[test]
fn a_piece_over_the_context_is_too_long_and_not_sent() {
    let lab = Lab::new(FakeEngine::new());
    let long = vec!["word"; 2000].join(" ");
    let fits = vec!["word"; 1000].join(" ");
    lab.run("r1", &[&long, &fits]);
    let phases = lab.events.ended("r1");
    assert_eq!(discarded(&phases, 1), "tooLong");
    assert_eq!(piece(&phases, 2)["result"]["kind"], "output");
    assert_eq!(lab.fake.requests.lock().unwrap().len(), 1);
}

/// AC-6: cancel mid-answer -> `cancelled`, later pieces never answered; session warm, unloaded after the idle time
/// (worker gone).
#[test]
fn cancel_mid_run_ends_cancelled_and_the_session_unloads_when_idle() {
    let lab = Lab::paced();
    let long = vec!["word"; 200].join(" ");
    lab.run("r1", &[&long, "Never answered."]);
    wait(|| (lab.fake.requests.lock().unwrap().len() == 1).then_some(()));
    lab.corrector.cancel("other");
    lab.corrector.cancel("r1");
    let phases = lab.events.ended("r1");
    assert_eq!(kinds(&phases).last(), Some(&"cancelled"));
    assert!(!kinds(&phases).contains(&"piece"), "{phases:?}");
    assert_eq!(lab.fake.requests.lock().unwrap().len(), 1);
    assert_eq!(lab.loaded(), 1, "kept warm");
    wait(|| (lab.loaded() == 0 && !lab.corrector.worker_alive()).then_some(()));
}

/// Warm session: the next run reuses it (no loading phase); a reassigned model -> reopened, old one unloaded.
#[test]
fn the_next_run_reuses_the_warm_session_until_the_model_changes() {
    let lab = Lab::new(FakeEngine::new());
    lab.run("r1", &["One."]);
    lab.events.ended("r1");
    lab.run("r2", &["Two."]);
    let phases = lab.events.ended("r2");
    assert_eq!(kinds(&phases), ["piece", "done"]);
    assert_eq!(lab.fake.opens(), 1);
    lab.fake.serving.store(false, Ordering::Relaxed);
    lab.run("r3", &["Three."]);
    let phases = lab.events.ended("r3");
    assert_eq!(kinds(&phases)[0], "loading");
    assert_eq!(lab.fake.opens(), 2);
    assert_eq!(lab.loaded(), 1);
}

/// Person re-triggered: a new run cancels the running one; both end, the new one done.
#[test]
fn a_new_run_cancels_the_running_one() {
    let lab = Lab::paced();
    let long = vec!["word"; 200].join(" ");
    lab.run("r1", &[&long]);
    wait(|| (lab.fake.requests.lock().unwrap().len() == 1).then_some(()));
    lab.run("r2", &["Short."]);
    assert_eq!(kinds(&lab.events.ended("r1")).last(), Some(&"cancelled"));
    let phases = lab.events.ended("r2");
    assert_eq!(kinds(&phases), ["piece", "done"]);
    assert_eq!(lab.fake.opens(), 1);
}

/// AC-8: no model -> `failed` with the model service code; nothing loaded.
#[test]
fn a_run_without_a_model_fails_no_model() {
    let lab = Lab::new(FakeEngine::new());
    lab.fake.assigned.store(false, Ordering::Relaxed);
    lab.run("r1", &["Text."]);
    let phases = lab.events.ended("r1");
    assert_eq!(kinds(&phases), ["failed"]);
    let error = &phases[0]["error"];
    assert_eq!(
        (&error["kind"], &error["subject"], &error["detail"]),
        (&json!("unavailable"), &json!("model"), &json!("noModel"))
    );
    assert_eq!(lab.loaded(), 0);
}

/// AC-1 teardown: plugin off mid-run -> cancelled, nothing loaded, no thread; later runs refused.
#[test]
fn shutdown_mid_run_leaves_no_thread_and_nothing_loaded() {
    let lab = Lab::paced();
    let long = vec!["word"; 200].join(" ");
    lab.run("r1", &[&long]);
    wait(|| (lab.fake.requests.lock().unwrap().len() == 1).then_some(()));
    lab.corrector.shutdown();
    assert_eq!(kinds(&lab.events.phases("r1")).last(), Some(&"cancelled"));
    assert_eq!(lab.loaded(), 0);
    assert!(!lab.corrector.worker_alive());
    assert!(lab.corrector.start("r2".into(), Vec::new()).is_err());
    lab.corrector.shutdown();
}

/// Plugin over the real service lookup: no `ModelHost` offered -> status unavailable, run fails `noModel`; off
/// withdraws the RPC.
#[test]
fn the_plugin_without_the_model_service_is_unavailable() {
    let (rpc, services) = (RpcTable::new(), Services::new());
    let events = Arc::new(Events::default());
    let cx = cx(&events).with_app(AppServices::new(
        Arc::clone(&rpc),
        services,
        Arc::new(RwLock::new(AppSettings::default())),
    ));
    let running = crate::plugin().start(&cx).unwrap();
    let call = |method: &str, args: Value| {
        rpc.call(
            "correction",
            method,
            &RpcCall {
                window: "main".into(),
                args,
            },
        )
    };
    assert_eq!(
        call("status", json!({})).unwrap(),
        json!({ "available": false })
    );
    let pieces = json!({ "run": "r1", "pieces": [{ "id": 1, "text": "Text." }] });
    assert_eq!(call("run", pieces).unwrap(), Value::Null);
    let phases = events.ended("r1");
    assert_eq!(phases.last().unwrap()["error"]["detail"], "noModel");
    assert_eq!(call("cancel", json!({ "run": "r1" })).unwrap(), Value::Null);
    assert!(call("run", json!({ "run": "r2", "pieces": [], "extra": 1 })).is_err());
    drop(running);
    assert!(call("status", json!({})).is_err(), "RPC gone after off");
}

mod eval;
