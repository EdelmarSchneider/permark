//! The correction job on `local-ai`'s piece runner, over its in-process [`FakeEngine`] (echoes the last user turn word
//! by word): requests carry the prompt + sampling + answer cap, plugin RPC without the model service (status, run
//! fails `noModel`, off withdraws the RPC). Runner behavior itself (phases, discards, cancel, warm session, plugin
//! off): `local-ai`'s `src/tests/pieces.rs`.

use std::sync::{Arc, RwLock};

use permark_core::plugins::{AppServices, RpcCall, RpcTable, Services};
use permark_core::settings::AppSettings;
use permark_plugin_local_ai::engine::{FakeEngine, Role};
use permark_plugin_local_ai::pieces::testkit::{pieces, Events, Fake};
use permark_plugin_local_ai::pieces::{Models, IDLE};
use serde_json::{json, Value};

use crate::prompt::{self, CorrectionJob};
use crate::Corrector;

struct Lab {
    fake: Arc<Fake>,
    events: Arc<Events>,
    corrector: Corrector,
}

impl Lab {
    fn new(engine: FakeEngine) -> Self {
        let fake = Fake::new::<CorrectionJob>(engine);
        let events = Events::new(crate::ID);
        let corrector = Corrector::new(&events.cx(), Arc::clone(&fake) as Arc<dyn Models>, IDLE);
        Self {
            fake,
            events,
            corrector,
        }
    }

    fn run(&self, run: &str, texts: &[&str]) {
        self.corrector
            .start(run.to_owned(), (), pieces(texts))
            .unwrap();
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
    let events = Events::new(crate::ID);
    let cx = events.cx().with_app(AppServices::new(
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
