//! The correction job on `local-ai`'s piece runner, over its in-process [`FakeEngine`] (echoes the last user turn word
//! by word): requests carry the prompt + sampling + answer cap, plugin RPC without the model service (status, run
//! fails `noModel`, off withdraws the RPC). Runner behavior itself (phases, discards, cancel, warm session, plugin
//! off): `local-ai`'s `src/tests/pieces.rs`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use permark_core::conditions::{Context, Distribution};
use permark_core::events::{CoreEvent, EventSink};
use permark_core::plugins::{AppServices, CxWorkspace, PluginCx, RpcCall, RpcTable, Services};
use permark_core::settings::AppSettings;
use permark_core::IpcError;
use permark_plugin_local_ai::engine::{
    CancelToken, ChatRequest, Device, Engine, EngineError, FakeEngine, FileRole, GenEvent,
    LoadOpts, ModelFiles, Role, SessionId,
};
use permark_plugin_local_ai::pieces::{Models, Piece, Session, IDLE};
use permark_plugin_local_ai::{engine_ipc, HostProgress};
use serde_json::{json, Value};

use crate::prompt::{self, CONTEXT};
use crate::Corrector;

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

/// Model source over one [`FakeEngine`]; records requests.
struct Fake {
    engine: Arc<FakeEngine>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl Fake {
    fn new(engine: FakeEngine) -> Arc<Self> {
        Arc::new(Self {
            engine: Arc::new(engine),
            requests: Arc::default(),
        })
    }
}

impl Models for Fake {
    fn available(&self) -> bool {
        true
    }

    fn open(
        &self,
        progress: &dyn Fn(HostProgress),
        cancel: &CancelToken,
    ) -> Result<Box<dyn Session>, IpcError> {
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
            .map_err(|e| engine_ipc(&e))?;
        Ok(Box::new(FakeSession {
            engine: Arc::clone(&self.engine),
            id,
            requests: Arc::clone(&self.requests),
        }))
    }
}

struct FakeSession {
    engine: Arc<FakeEngine>,
    id: SessionId,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl Drop for FakeSession {
    fn drop(&mut self) {
        self.engine.unload(self.id);
    }
}

impl Session for FakeSession {
    fn serves(&self) -> bool {
        true
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
        let corrector = Corrector::new(&cx(&events), Arc::clone(&fake) as Arc<dyn Models>, IDLE);
        Self {
            fake,
            events,
            corrector,
        }
    }

    fn run(&self, run: &str, texts: &[&str]) {
        let pieces = (1u32..)
            .zip(texts)
            .map(|(id, text)| Piece {
                id,
                text: (*text).to_owned(),
            })
            .collect();
        self.corrector.start(run.to_owned(), (), pieces).unwrap();
    }
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
