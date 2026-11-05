//! "Test model" (AC-7) over in-process `serve(FakeEngine)` workers + scripted downloads: phase order (chat, embed),
//! engine fetched on the first test only, cancel (run, engine download) -> cancelled + unloaded, guard refusal ->
//! failed `outOfMemory`, no pin -> failed `notAvailable`, one test at a time, error codes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use permark_core::downloads::{Scripted, ScriptedTransport, Transport};
use permark_core::plugins::{RpcTable, Services};
use permark_core::{ErrorKind, IpcError};
use serde_json::{json, Value};

use super::installs::{catalog, serving, Events, Held, Lab, E2B, EMBED};
use crate::catalog::Catalog;
use crate::engine::files::tests::{pin, url, EXE, LIB};
use crate::engine::files::{self, WorkerFiles, WorkerPin};
use crate::engine::tests::Workers;
use crate::engine::{EngineError, FakeEngine};
use crate::installs::Installs;
use crate::machine::MachineProfile;
use crate::runtime::Runtime;
use crate::testrun::{EngineNews, TestError, TestParts, Tester, PROMPT};
use crate::{state, InstallState};

const WHISPER: &str = "whisper-large-v3-turbo-q5_0";

fn plenty() -> MachineProfile {
    MachineProfile {
        ram_available: Some(64 << 30),
        ..MachineProfile::unknown()
    }
}

fn scarce() -> MachineProfile {
    MachineProfile {
        ram_available: Some(1 << 20),
        ..MachineProfile::unknown()
    }
}

/// Worker unit of [`pin`] served.
fn engine_files() -> ScriptedTransport {
    let pin = pin();
    ScriptedTransport::new()
        .answer(&url(&pin.files[0]), Scripted::ok(EXE))
        .answer(&url(&pin.files[1]), Scripted::ok(LIB))
}

struct Bench {
    lab: Lab,
    events: Arc<Events>,
    installs: Arc<Installs>,
    runtime: Arc<Runtime>,
    workers: Workers,
    tester: Tester,
}

impl Bench {
    fn new(
        make: fn() -> FakeEngine,
        machine: fn() -> MachineProfile,
        pin: Option<&'static WorkerPin>,
        engine: Arc<dyn Transport>,
    ) -> Self {
        let lab = Lab::new();
        let cat: Catalog = catalog(1, |_| {});
        let (rpc, services) = (RpcTable::new(), Services::new());
        let events = Arc::new(Events::default());
        let cx = lab.cx(&rpc, &services, &events);
        let workers = Workers::default();
        let runtime = Arc::new(Runtime::with_launch(workers.launch(make), machine));
        let installs = Installs::new(
            Arc::new(cat.clone()),
            Some(Arc::new(lab.kit(Arc::new(serving(&cat))))),
            lab.models_dir(),
            cx.state_file(state::FILE),
            cx.events().clone(),
            crate::StoreLinks::default(),
            Some(Arc::clone(&runtime)),
        );
        let news = Arc::new(EngineNews::default());
        let files = Arc::new(WorkerFiles::with_pin(
            pin,
            Some(&lab.kit(engine)),
            cx.state_dir.join(files::DIR),
            news.listener(),
        ));
        let tester = Tester::new(TestParts {
            installs: Arc::clone(&installs),
            catalog: Arc::new(cat),
            runtime: Arc::clone(&runtime),
            files,
            news,
            events: cx.events().clone(),
        });
        Self {
            lab,
            events,
            installs,
            runtime,
            workers,
            tester,
        }
    }

    fn fake(make: fn() -> FakeEngine) -> Self {
        Self::new(make, plenty, Some(pin()), Arc::new(engine_files()))
    }

    /// `id` downloaded + installed (download no longer running).
    fn install(&self, id: &str) {
        self.installs.get(id).unwrap();
        let start = Instant::now();
        while !self
            .installs
            .state()
            .models
            .iter()
            .any(|m| m.model == id && matches!(m.state, InstallState::Installed { .. }))
        {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{id} not installed"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// `local-ai:test` payloads so far.
    fn tests(&self) -> Vec<Value> {
        self.events
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == "local-ai:test")
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    /// Waits for a test event of phase `kind`.
    fn wait(&self, kind: &str) -> Value {
        let start = Instant::now();
        loop {
            if let Some(found) = self
                .tests()
                .into_iter()
                .find(|t| t["phase"]["kind"] == kind)
            {
                return found;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "no {kind}: {:?}",
                self.tests()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits for the end; answers the phase kinds in order (consecutive repeats once) + the last phase.
    fn end(&self) -> (Vec<String>, Value) {
        let start = Instant::now();
        loop {
            let tests = self.tests();
            if let Some(last) = tests.last().filter(|t| {
                matches!(
                    t["phase"]["kind"].as_str(),
                    Some("done" | "failed" | "cancelled")
                )
            }) {
                let mut kinds: Vec<String> = tests
                    .iter()
                    .map(|t| t["phase"]["kind"].as_str().unwrap().to_owned())
                    .collect();
                kinds.dedup();
                return (kinds, last["phase"].clone());
            }
            assert!(start.elapsed() < Duration::from_secs(10), "{tests:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn clear(&self) {
        self.events.0.lock().unwrap().clear();
    }

    /// Every worker that started is gone, no session left.
    fn unloaded(&self) {
        assert_eq!(self.workers.kills(), self.workers.launched());
        assert_eq!(self.runtime.loaded(), Vec::<String>::new());
    }

    fn failed(phase: &Value, kind: &str, detail: &str) {
        assert_eq!(phase["kind"], "failed", "{phase}");
        assert_eq!(phase["error"]["kind"], kind, "{phase}");
        assert_eq!(phase["error"]["detail"], detail, "{phase}");
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        self.tester.shutdown();
        self.installs.shutdown();
    }
}

/// AC-7: chat model, first test fetches the engine: engine -> loading -> running -> done; unloaded after; the second
/// test skips the engine phase.
#[test]
fn a_chat_model_test_gets_the_engine_loads_runs_and_unloads() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    bench.tester.start(E2B).unwrap();
    let (kinds, done) = bench.end();
    assert_eq!(kinds, ["engine", "loading", "running", "done"]);
    let engine = bench.wait("engine");
    assert_eq!(engine["model"], E2B);
    let total = (EXE.len() + LIB.len()) as u64;
    assert_eq!(engine["phase"]["totalBytes"], total);
    let loading: Vec<f64> = bench
        .tests()
        .iter()
        .filter(|t| t["phase"]["kind"] == "loading")
        .map(|t| t["phase"]["progress"].as_f64().unwrap())
        .collect();
    assert_eq!(loading.first(), Some(&0.0));
    assert_eq!(loading.last(), Some(&1.0));
    let result = &done["result"];
    assert_eq!(result["gpu"], true);
    assert_eq!(result["engineVersion"], "fake-1");
    assert!(result["memoryBytes"].as_u64().unwrap() > 0, "{result}");
    assert!(result["loadMs"].is_u64() && result["runMs"].is_u64());
    let output = &result["output"];
    assert_eq!(output["kind"], "chat");
    assert_eq!(output["answer"], PROMPT, "fake echoes the prompt");
    assert_eq!(output["tokens"], 6);
    assert!(output["tokensPerSecond"].as_f64().unwrap() > 0.0);
    bench.unloaded();
    assert_eq!(bench.workers.launched(), 1);

    bench.clear();
    bench.tester.start(E2B).unwrap();
    let (kinds, _) = bench.end();
    assert_eq!(kinds, ["loading", "running", "done"], "engine kept");
    bench.unloaded();
}

/// AC-7: embedding-only model -> one vector (dims), unloaded after.
#[test]
fn an_embedding_model_test_embeds_one_text() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(EMBED);
    bench.tester.start(EMBED).unwrap();
    let (kinds, done) = bench.end();
    assert_eq!(kinds, ["engine", "loading", "running", "done"]);
    assert_eq!(
        done["result"]["output"],
        json!({ "kind": "embed", "dims": FakeEngine::NATIVE_DIMS })
    );
    bench.unloaded();
}

/// One test at a time: a second start is refused (busy), the running one goes on; cancel mid-run -> cancelled +
/// unloaded; idle again after.
#[test]
fn a_running_test_refuses_another_and_cancel_unloads() {
    let bench = Bench::fake(|| FakeEngine::new().paced(Duration::from_secs(10)));
    bench.install(E2B);
    bench.install(EMBED);
    bench.tester.start(E2B).unwrap();
    bench.wait("running");
    for other in [E2B, EMBED] {
        assert!(matches!(bench.tester.start(other), Err(TestError::Busy)));
    }
    let busy = IpcError::from(TestError::Busy);
    assert_eq!(
        (busy.kind, busy.detail.as_deref()),
        (ErrorKind::Conflict, Some("testRunning"))
    );
    let started = Instant::now();
    bench.tester.cancel();
    let (kinds, last) = bench.end();
    assert!(started.elapsed() < Duration::from_secs(5), "cancel honored");
    assert_eq!(kinds.last().map(String::as_str), Some("cancelled"));
    assert_eq!(last, json!({ "kind": "cancelled" }));
    bench.unloaded();
    assert_eq!(bench.workers.launched(), 1);
    // idle again
    bench.clear();
    bench.tester.cancel();
    bench.tester.start(EMBED).unwrap();
    bench.tester.cancel();
    bench.end();
    bench.unloaded();
}

/// Cancel while the engine downloads: download stopped, cancelled, no worker started.
#[test]
fn cancel_during_the_engine_download_stops_it() {
    let held = Arc::new(Held {
        inner: engine_files(),
        url: url(&pin().files[0]),
        open: Mutex::new(false),
        wake: Condvar::new(),
        reached: AtomicBool::new(false),
    });
    let bench = Bench::new(
        FakeEngine::new,
        plenty,
        Some(pin()),
        Arc::clone(&held) as Arc<dyn Transport>,
    );
    bench.install(E2B);
    bench.tester.start(E2B).unwrap();
    let start = Instant::now();
    while !held.reached.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    bench.tester.cancel();
    let (_, last) = bench.end();
    held.release();
    assert_eq!(last, json!({ "kind": "cancelled" }));
    assert_eq!(bench.workers.launched(), 0);
}

/// AC-5: too little memory -> failed `outOfMemory`, no worker started.
#[test]
fn the_memory_guard_fails_the_test_before_a_worker_starts() {
    let bench = Bench::new(
        FakeEngine::new,
        scarce,
        Some(pin()),
        Arc::new(engine_files()),
    );
    bench.install(E2B);
    bench.tester.start(E2B).unwrap();
    let (kinds, last) = bench.end();
    assert_eq!(kinds, ["engine", "loading", "failed"]);
    Bench::failed(&last, "unavailable", "outOfMemory");
    assert_eq!(last["error"]["subject"], "engine");
    assert_eq!(bench.workers.launched(), 0);
}

/// No worker built for this computer: failed `notAvailable`, nothing fetched | started.
#[test]
fn no_engine_for_this_computer_fails_the_test() {
    let bench = Bench::new(
        FakeEngine::new,
        plenty,
        None,
        Arc::new(ScriptedTransport::new()),
    );
    bench.install(E2B);
    bench.tester.start(E2B).unwrap();
    let (kinds, last) = bench.end();
    assert_eq!(kinds, ["failed"]);
    Bench::failed(&last, "unavailable", "notAvailable");
    assert_eq!(bench.workers.launched(), 0);
    assert!(!bench
        .lab
        .store()
        .plugin_dir(&crate::ID)
        .join(files::DIR)
        .exists());
}

/// Refused at start: unknown | not installed model, a model with nothing to test (speech only).
#[test]
fn start_refuses_what_cannot_be_tested() {
    let bench = Bench::fake(FakeEngine::new);
    let refused = |id: &str| IpcError::from(bench.tester.start(id).unwrap_err());
    assert_eq!(refused("nope").kind, ErrorKind::NotFound);
    let missing = refused(E2B);
    assert_eq!(
        (missing.kind, missing.detail.as_deref()),
        (ErrorKind::Conflict, Some("notInstalled"))
    );
    bench.install(WHISPER);
    let speech = refused(WHISPER);
    assert_eq!(
        (speech.kind, speech.detail.as_deref()),
        (ErrorKind::Unsupported, Some("noTest"))
    );
    assert!(bench.tests().is_empty(), "nothing started");
    assert_eq!(bench.workers.launched(), 0);
}

/// Stable codes the UI maps to its texts (worker log never sent).
#[test]
fn engine_errors_map_to_stable_codes() {
    let cases = [
        (
            EngineError::OutOfMemory,
            ErrorKind::Unavailable,
            "outOfMemory",
        ),
        (
            EngineError::EngineCrashed,
            ErrorKind::Unavailable,
            "crashed",
        ),
        (EngineError::Blocked, ErrorKind::Denied, "blocked"),
        (
            EngineError::EngineTooOld {
                needed: "protocol 2".into(),
            },
            ErrorKind::Unsupported,
            "engineTooOld",
        ),
        (
            EngineError::Unsupported {
                what: "tools".into(),
            },
            ErrorKind::Unsupported,
            "unsupported",
        ),
        (EngineError::Corrupt, ErrorKind::InvalidData, "corrupt"),
        (EngineError::NotLoaded, ErrorKind::Unavailable, "notLoaded"),
        (EngineError::Cancelled, ErrorKind::Superseded, "cancelled"),
        // engine text may hold profile paths: never sent
        (
            EngineError::Io(r"C:\Users\someone\AppData\model.gguf not found".into()),
            ErrorKind::Io,
            "io",
        ),
    ];
    for (error, kind, detail) in cases {
        let ipc = IpcError::from(TestError::Engine(error));
        assert_eq!(ipc.kind, kind);
        assert_eq!(ipc.subject.as_deref(), Some("engine"));
        assert_eq!(ipc.detail.as_deref(), Some(detail));
        let sent = serde_json::to_string(&ipc).unwrap();
        assert!(
            !sent.contains("someone") && !sent.contains("gguf"),
            "{sent}"
        );
    }
    let not_available = IpcError::from(TestError::Files(files::FilesError::NotAvailable));
    assert_eq!(
        (not_available.kind, not_available.detail.as_deref()),
        (ErrorKind::Unavailable, Some("notAvailable"))
    );
}

/// The plugin answers `testModel` + `cancelTest` (null) without starting anything.
#[test]
fn the_plugin_registers_the_test_calls() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let mut run = lab.start(cat.clone(), Arc::new(serving(&cat)));
    assert_eq!(run.call("cancelTest", json!({})).unwrap(), Value::Null);
    let unknown = run
        .call("testModel", json!({ "model": "nope" }))
        .unwrap_err();
    assert_eq!(unknown.kind, ErrorKind::NotFound);
    let missing = run.call("testModel", json!({ "model": E2B })).unwrap_err();
    assert_eq!(missing.detail.as_deref(), Some("notInstalled"));
    assert!(!run.event_names().iter().any(|n| n == "local-ai:test"));
    run.stop();
}
