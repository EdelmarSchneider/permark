//! Piece runs of a small test job ([`Echo`]) over the in-process [`FakeEngine`] (echoes the last user turn word by
//! word, paced for cancel; [`RAMBLE`] in a piece -> answer cut at its cap): dormant availability, phase order (engine
//! fetch first when missing), requests built by the job from the run's parameters, discards (length, thinking, too
//! long), cancel, new run cancels the old one, warm session (idle unload, reassignment), plugin off (no thread,
//! nothing loaded), production model source (`ModelHost` service, the job's own task).

use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use permark_core::plugins::{AppServices, PluginId, RpcTable, Services};
use permark_core::settings::AppSettings;
use serde_json::{json, Value};

use super::installs::E2B;
use super::testrun::Bench;
use crate::engine::{ChatMessage, FakeEngine, Role, Sampling};
use crate::pieces::testkit::{pieces, wait, Events, Fake, RAMBLE};
use crate::pieces::{Models, PieceJob, PieceRunner, THINKING};
use crate::tasks::Task;

/// Plugin the test runs belong to (events `echo:run`).
const PLUGIN: PluginId = PluginId::of("echo");
const SHORT_IDLE: Duration = Duration::from_millis(150);
const PACE: Duration = Duration::from_millis(20);
/// System turn of [`Echo`] before the run's tone.
const RULES: &str = "Echo the text in this tone:";
/// Tone of the runs that do not care.
const TONE: &str = "plain";

/// Small job with a per-run parameter (a tone): system turn = [`RULES`] + tone, user turn = the piece; cap =
/// 2 x piece tokens + 8; a task of its own.
struct Echo;

impl PieceJob for Echo {
    type Params = String;

    const TASK: Task = Task::Translation;
    const CONTEXT: u32 = 4096;
    const SAMPLING: Sampling = Sampling {
        temperature: 0.0,
        top_p: 1.0,
        seed: Some(7),
    };

    fn messages(tone: &String, text: &str) -> Vec<ChatMessage> {
        vec![
            ChatMessage {
                role: Role::System,
                content: Self::preamble(tone),
            },
            ChatMessage {
                role: Role::User,
                content: text.to_owned(),
            },
        ]
    }

    fn preamble(tone: &String) -> String {
        format!("{RULES} {tone}")
    }

    fn max_tokens(piece_tokens: u32) -> u32 {
        piece_tokens.saturating_mul(2).saturating_add(8)
    }
}

struct Lab {
    fake: Arc<Fake>,
    events: Arc<Events>,
    runner: PieceRunner<Echo>,
}

impl Lab {
    fn new(engine: FakeEngine) -> Self {
        let fake = Fake::new::<Echo>(engine);
        let events = Events::new(PLUGIN);
        let runner = PieceRunner::new(
            &events.cx(),
            Arc::clone(&fake) as Arc<dyn Models>,
            SHORT_IDLE,
        );
        Self {
            fake,
            events,
            runner,
        }
    }

    fn paced() -> Self {
        Self::new(FakeEngine::new().paced(PACE))
    }

    fn run(&self, run: &str, texts: &[&str]) {
        self.runner
            .start(run.to_owned(), TONE.to_owned(), pieces(texts))
            .unwrap();
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

/// Dormancy: availability from the state alone; nothing opened before a run.
#[test]
fn availability_reads_the_assignment_and_loads_nothing() {
    let lab = Lab::new(FakeEngine::new());
    lab.fake.assigned.store(false, Ordering::Relaxed);
    assert!(!lab.runner.available());
    lab.fake.assigned.store(true, Ordering::Relaxed);
    assert!(lab.runner.available());
    assert_eq!(lab.fake.opens(), 0);
    assert_eq!(lab.loaded(), 0);
    assert!(!lab.runner.worker_alive());
}

/// Loading -> pieces in the given order (first visible before the end) -> done.
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

/// Engine missing -> its download first (bytes), then loading -> pieces -> done; the next open fetches nothing.
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

/// Cancel while the engine downloads -> `cancelled`, nothing loaded.
#[test]
fn cancel_during_the_engine_download_ends_cancelled() {
    let lab = Lab::new(FakeEngine::new());
    *lab.fake.engine_missing.lock().unwrap() = Some(1000);
    lab.fake.engine_held.store(true, Ordering::Relaxed);
    lab.run("r1", &["One."]);
    wait(|| (!lab.events.phases("r1").is_empty()).then_some(()));
    lab.runner.cancel("r1");
    let phases = lab.events.ended("r1");
    assert_eq!(kinds(&phases), ["engine", "cancelled"]);
    assert_eq!(lab.fake.opens(), 0);
    assert_eq!(lab.loaded(), 0);
}

/// Each request = the job's turns for that run's parameters + the raw piece, the job's sampling and answer cap.
#[test]
fn requests_carry_the_jobs_turns_for_the_runs_parameters_its_sampling_and_cap() {
    let lab = Lab::new(FakeEngine::new());
    let text = "one two three four five six seven eight nine ten";
    for (run, tone) in [("r1", "formal"), ("r2", "casual")] {
        lab.runner
            .start(run.to_owned(), tone.to_owned(), pieces(&[text]))
            .unwrap();
        lab.events.ended(run);
    }
    let requests = lab.fake.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    for (request, tone) in requests.iter().zip(["formal", "casual"]) {
        let turns: Vec<(Role, &str)> = request
            .messages
            .iter()
            .map(|m| (m.role, m.content.as_str()))
            .collect();
        let rules = format!("{RULES} {tone}");
        assert_eq!(turns, [(Role::System, rules.as_str()), (Role::User, text)]);
        assert_eq!(request.max_tokens, 28);
        assert_eq!(request.sampling, Echo::SAMPLING);
        assert_eq!(request.tools.len(), 0);
    }
}

/// An answer cut at its cap is discarded (`length`); the run goes on.
#[test]
fn an_answer_cut_at_its_cap_is_discarded() {
    let lab = Lab::new(FakeEngine::new());
    lab.run("r1", &[&format!("please {RAMBLE} on"), "Fine text."]);
    let phases = lab.events.ended("r1");
    assert_eq!(discarded(&phases, 1), "length");
    assert_eq!(piece(&phases, 2)["result"]["kind"], "output");
    assert_eq!(kinds(&phases).last(), Some(&"done"));
}

/// Thinking markup in the answer -> discarded (`thinking`).
#[test]
fn a_thinking_leak_is_discarded() {
    let lab = Lab::new(FakeEngine::new());
    lab.run("r1", &[&format!("{THINKING}thought Fixed text.")]);
    let phases = lab.events.ended("r1");
    assert_eq!(discarded(&phases, 1), "thinking");
}

/// Token budget: prompt + piece + answer room over the context -> `tooLong`, never sent to the model.
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

/// Cancel mid-answer -> `cancelled`, later pieces never answered; session warm, unloaded after the idle time (worker
/// gone).
#[test]
fn cancel_mid_run_ends_cancelled_and_the_session_unloads_when_idle() {
    let lab = Lab::paced();
    let long = vec!["word"; 200].join(" ");
    lab.run("r1", &[&long, "Never answered."]);
    wait(|| (lab.fake.requests.lock().unwrap().len() == 1).then_some(()));
    lab.runner.cancel("other");
    lab.runner.cancel("r1");
    let phases = lab.events.ended("r1");
    assert_eq!(kinds(&phases).last(), Some(&"cancelled"));
    assert!(!kinds(&phases).contains(&"piece"), "{phases:?}");
    assert_eq!(lab.fake.requests.lock().unwrap().len(), 1);
    assert_eq!(lab.loaded(), 1, "kept warm");
    wait(|| (lab.loaded() == 0 && !lab.runner.worker_alive()).then_some(()));
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

/// No model -> `failed` with the model service code; nothing loaded.
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

/// Teardown: plugin off mid-run -> cancelled, nothing loaded, no thread; later runs refused.
#[test]
fn shutdown_mid_run_leaves_no_thread_and_nothing_loaded() {
    let lab = Lab::paced();
    let long = vec!["word"; 200].join(" ");
    lab.run("r1", &[&long]);
    wait(|| (lab.fake.requests.lock().unwrap().len() == 1).then_some(()));
    lab.runner.shutdown();
    assert_eq!(kinds(&lab.events.phases("r1")).last(), Some(&"cancelled"));
    assert_eq!(lab.loaded(), 0);
    assert!(!lab.runner.worker_alive());
    let refused = lab
        .runner
        .start("r2".into(), TONE.into(), Vec::new())
        .unwrap_err();
    assert_eq!(refused.subject.as_deref(), Some("echo"));
    lab.runner.shutdown();
}

/// Production model source (`ModelHost` service): the job's own task picks the model (another job's assignment never
/// serves it); a run loads through the service and keeps the session until that assignment changes.
#[test]
fn a_hosted_runner_uses_the_model_assigned_to_the_jobs_task() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    let services = Services::new();
    let _offered = services.provide(Arc::clone(&bench.host));
    let events = Events::new(PLUGIN);
    let cx = events.cx().with_app(AppServices::new(
        RpcTable::new(),
        Arc::clone(&services),
        Arc::new(RwLock::new(AppSettings::default())),
    ));
    let runner = PieceRunner::<Echo>::hosted(&cx);
    let run = |run: &str, text: &str| {
        runner
            .start(run.to_owned(), TONE.to_owned(), pieces(&[text]))
            .unwrap();
        events.ended(run)
    };
    assert!(!runner.available(), "another job's model");
    assert_eq!(
        run("r1", "One.").last().unwrap()["error"]["detail"],
        "noModel"
    );
    assert_eq!(bench.workers.launched(), 0);

    bench.installs.assign(Echo::TASK, Some(E2B)).unwrap();
    assert!(runner.available());
    let phases = run("r2", "Two.");
    assert_eq!(
        piece(&phases, 1)["result"],
        json!({ "kind": "output", "text": "Two." })
    );
    assert_eq!(kinds(&phases).last(), Some(&"done"));
    assert_eq!(bench.runtime.loaded().len(), 1, "kept warm");
    assert_eq!(kinds(&run("r3", "Three.")), ["piece", "done"]);
    assert_eq!(bench.workers.launched(), 1);

    bench.installs.assign(Echo::TASK, None).unwrap();
    assert_eq!(
        run("r4", "Four.").last().unwrap()["error"]["detail"],
        "noModel"
    );
    bench.unloaded();
    runner.shutdown();
}
