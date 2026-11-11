//! Piece runs of a small test job ([`Echo`]) over the in-process [`FakeEngine`] (echoes the last user turn word by
//! word, paced for cancel; [`RAMBLE`] in a piece -> answer cut at its cap): dormant availability, phase order (engine
//! fetch first when missing), requests built by the job from the run's parameters, discards (length, thinking, too
//! long), cancel, new run cancels the old one, warm session (idle unload, reassignment), plugin off (no thread,
//! nothing loaded), warm session unloaded under its run (by its source -> `cancelled`; lost by the engine ->
//! `failed`), production model source (`ModelHost` service, the job's own task), two plugins' runners in one
//! app (a load unloads the other's idle session first, a run that cannot load unloads nothing, a running one is left
//! alone, no wait on each other).

use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use permark_core::plugins::{AppServices, PluginCx, PluginId, RpcTable, ServiceLease, Services};
use permark_core::settings::AppSettings;
use serde_json::{json, Value};

use super::installs::E2B;
use super::testrun::Bench;
use crate::engine::{ChatMessage, Engine, FakeEngine, Role, Sampling, SessionId};
use crate::pieces::testkit::{pieces, wait, Events, Fake, RAMBLE};
use crate::pieces::{Models, PieceJob, PieceRunner, IDLE, THINKING};
use crate::tasks::Task;

/// Plugin the test runs belong to (events `echo:run`).
const PLUGIN: PluginId = PluginId::of("echo");
/// Second plugin with a runner of its own in the same app (events `other:run`).
const OTHER: PluginId = PluginId::of("other");
const SHORT_IDLE: Duration = Duration::from_millis(150);
const PACE: Duration = Duration::from_millis(20);
/// Repeats of the tests where two threads meet (a rare interleaving shows up within them).
const ROUNDS: usize = 20;
/// System turn of [`Echo`] before the run's tone.
const RULES: &str = "Echo the text in this tone:";
/// Tone of the runs that do not care.
const TONE: &str = "plain";
/// One-word pieces of a run to be met in progress: paced, seconds pass before its last one, and an engine call
/// that fails ends it at the next piece.
const MANY: [&str; 200] = ["word"; 200];
/// Session of a fake engine's first load.
const FIRST: SessionId = SessionId(1);

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
        let events = Events::new(PLUGIN);
        let cx = events.cx();
        Self::over(Fake::new::<Echo>(engine), events, &cx, SHORT_IDLE)
    }

    fn over(fake: Arc<Fake>, events: Arc<Events>, cx: &PluginCx, idle: Duration) -> Self {
        let runner = PieceRunner::new(cx, Arc::clone(&fake) as Arc<dyn Models>, idle);
        Self {
            fake,
            events,
            runner,
        }
    }

    /// Runner of `plugin` in `app` over `fake`, warm for [`IDLE`]: during a test only a peer's load | plugin off
    /// unloads its session.
    fn peer(plugin: PluginId, fake: &Arc<Fake>, app: &AppServices) -> Self {
        let events = Events::new(plugin);
        let cx = events.cx().with_app(app.clone());
        Self::over(Arc::clone(fake), events, &cx, IDLE)
    }

    /// Runners of two plugins in one app over one model source (its counters: both runners).
    fn pair(engine: FakeEngine) -> (Self, Self) {
        let (fake, app) = (Fake::new::<Echo>(engine), app());
        (
            Self::peer(PLUGIN, &fake, &app),
            Self::peer(OTHER, &fake, &app),
        )
    }

    fn paced() -> Self {
        Self::new(FakeEngine::new().paced(PACE))
    }

    /// Run `r2` in progress ([`MANY`]) on the session run `r1` left warm ([`FIRST`]; kept [`IDLE`]: only the test
    /// unloads it).
    fn mid_warm_run() -> Self {
        let events = Events::new(PLUGIN);
        let cx = events.cx();
        let fake = Fake::new::<Echo>(FakeEngine::new().paced(PACE));
        let lab = Self::over(fake, events, &cx, IDLE);
        lab.run("r1", &["One."]);
        lab.events.ended("r1");
        lab.run("r2", &MANY);
        wait(|| (lab.fake.requests.lock().unwrap().len() == 2).then_some(()));
        lab
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

/// Services of one app (plugins started with clones of it = that app's plugins).
fn app() -> AppServices {
    AppServices::new(
        RpcTable::new(),
        Services::new(),
        Arc::new(RwLock::new(AppSettings::default())),
    )
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

/// The model source takes a warm session from under its run (the person lets models run elsewhere | removes the
/// model: unloaded, no longer serving) -> the run ends `cancelled`, as a run that loaded the model itself does;
/// never `failed`. The session is not kept: a source that serves again never hands the next run the unloaded one.
#[test]
fn a_run_whose_warm_session_its_source_unloads_ends_cancelled() {
    let lab = Lab::mid_warm_run();
    lab.fake.serving.store(false, Ordering::Relaxed);
    lab.fake.engine.unload(FIRST);
    let phases = lab.events.ended("r2");
    assert_eq!(kinds(&phases).last(), Some(&"cancelled"), "{phases:?}");
    assert!(!kinds(&phases).contains(&"loading"), "{phases:?}");
    lab.fake.serving.store(true, Ordering::Relaxed);
    lab.run("r3", &["Three."]);
    let phases = lab.events.ended("r3");
    assert_eq!(kinds(&phases)[0], "loading");
    assert_eq!(kinds(&phases).last(), Some(&"done"));
    assert_eq!(lab.fake.opens(), 2);
}

/// A run on a warm session the engine loses by `lose` ends `failed` with engine code `code`; the session is not
/// kept, the next run loads again.
fn fails_after(lose: fn(&Lab), code: &str) {
    let lab = Lab::mid_warm_run();
    lose(&lab);
    let phases = lab.events.ended("r2");
    assert_eq!(kinds(&phases).last(), Some(&"failed"), "{phases:?}");
    let error = &phases.last().unwrap()["error"];
    assert_eq!(
        (&error["kind"], &error["subject"], &error["detail"]),
        (&json!("unavailable"), &json!("engine"), &json!(code))
    );
    lab.run("r3", &["Three."]);
    assert_eq!(kinds(&lab.events.ended("r3")).last(), Some(&"done"));
    assert_eq!(lab.fake.opens(), 2, "{code}");
}

/// A session the engine loses is the engine's failure, never read as the person's change: `failed` with the
/// engine's code when its source still serves it, and after a crash whatever the source says (in production a
/// crashed session no longer serves: not loaded).
#[test]
fn a_session_the_engine_loses_under_a_run_fails_it() {
    fails_after(|lab| lab.fake.engine.unload(FIRST), "notLoaded");
    fails_after(
        |lab| {
            lab.fake.serving.store(false, Ordering::Relaxed);
            lab.fake.engine.crash();
        },
        "crashed",
    );
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

/// Two features one after the other: the first runner's idle session is unloaded before the second one loads (never
/// two models for one feature at work); the released runner says nothing.
#[test]
fn a_load_unloads_the_idle_session_of_another_runner_first() {
    let (a, b) = Lab::pair(FakeEngine::new());
    a.run("a1", &["One."]);
    let told = a.events.ended("a1");
    assert_eq!(a.loaded(), 1, "kept warm");
    b.run("b1", &["Two."]);
    let phases = b.events.ended("b1");
    assert_eq!(kinds(&phases)[0], "loading");
    assert_eq!(
        piece(&phases, 1)["result"],
        json!({ "kind": "output", "text": "Two." })
    );
    assert_eq!(kinds(&phases).last(), Some(&"done"));
    assert_eq!(b.fake.opens(), 2);
    assert_eq!(b.fake.peak(), 1, "unloaded before the load");
    assert_eq!(b.loaded(), 1);
    assert_eq!(a.events.phases("a1"), told, "no news from the released one");
}

/// The released runner's next run is a cold start: loading again, then its pieces; the other's session, idle by
/// now, goes first in turn.
#[test]
fn a_released_runner_loads_again_on_its_next_run() {
    let (a, b) = Lab::pair(FakeEngine::new());
    a.run("a1", &["One."]);
    a.events.ended("a1");
    b.run("b1", &["Two."]);
    b.events.ended("b1");
    a.run("a2", &["Three."]);
    let phases = a.events.ended("a2");
    assert_eq!(kinds(&phases)[0], "loading");
    assert_eq!(
        piece(&phases, 1)["result"],
        json!({ "kind": "output", "text": "Three." })
    );
    assert_eq!(kinds(&phases).last(), Some(&"done"));
    assert_eq!(a.fake.opens(), 3);
    assert_eq!(a.fake.peak(), 1);
    assert_eq!(a.loaded(), 1);
}

/// A runner answering a piece is never interrupted by another plugin's run: both models loaded, both runs done, the
/// long answer whole.
#[test]
fn a_running_runner_keeps_its_session_while_another_loads() {
    let (a, b) = Lab::pair(FakeEngine::new().paced(PACE));
    let long = vec!["word"; 100].join(" ");
    a.run("a1", &[&long]);
    wait(|| (a.fake.requests.lock().unwrap().len() == 1).then_some(()));
    b.run("b1", &["Short."]);
    assert_eq!(kinds(&b.events.ended("b1")).last(), Some(&"done"));
    assert_eq!(b.fake.peak(), 2, "both loaded");
    let phases = a.events.ended("a1");
    assert_eq!(
        piece(&phases, 1)["result"],
        json!({ "kind": "output", "text": long })
    );
    assert_eq!(kinds(&phases).last(), Some(&"done"));
    assert_eq!(a.fake.opens(), 2);
}

/// A runner's own warm session still serves its next run (no load) after its load made a peer give up its own.
#[test]
fn a_runner_reuses_its_own_warm_session_beside_a_peer() {
    let (a, b) = Lab::pair(FakeEngine::new());
    a.run("a1", &["One."]);
    a.events.ended("a1");
    b.run("b1", &["Two."]);
    b.events.ended("b1");
    b.run("b2", &["Three."]);
    assert_eq!(kinds(&b.events.ended("b2")), ["piece", "done"]);
    assert_eq!(b.fake.opens(), 2);
    assert_eq!(b.loaded(), 1);
}

/// A run that cannot load (no model assigned to its job) makes no room: the other runner's idle session stays loaded
/// and serves its next run without a load.
#[test]
fn a_run_without_a_model_leaves_the_idle_session_of_another_runner() {
    let app = app();
    let a = Lab::peer(PLUGIN, &Fake::new::<Echo>(FakeEngine::new()), &app);
    let b = Lab::peer(OTHER, &Fake::new::<Echo>(FakeEngine::new()), &app);
    b.fake.assigned.store(false, Ordering::Relaxed);
    assert_eq!((a.runner.peers(), b.runner.peers()), (1, 1));
    a.run("a1", &["One."]);
    a.events.ended("a1");
    b.run("b1", &["Two."]);
    let phases = b.events.ended("b1");
    assert_eq!(kinds(&phases), ["failed"]);
    assert_eq!(phases[0]["error"]["detail"], "noModel");
    assert_eq!(a.loaded(), 1, "still warm");
    a.run("a2", &["Three."]);
    assert_eq!(kinds(&a.events.ended("a2")), ["piece", "done"]);
    assert_eq!((a.fake.opens(), b.fake.opens()), (1, 0));
}

/// Plugin off: the dropped runner is no longer a peer (nothing left to call); the other one runs as if alone.
#[test]
fn a_dropped_runner_is_no_longer_a_peer() {
    let (a, b) = Lab::pair(FakeEngine::new());
    assert_eq!(b.runner.peers(), 1);
    a.run("a1", &["One."]);
    a.events.ended("a1");
    drop(a);
    assert_eq!(b.runner.peers(), 0);
    assert_eq!(b.loaded(), 0, "unloaded by its own plugin off");
    b.run("b1", &["Two."]);
    assert_eq!(kinds(&b.events.ended("b1")).last(), Some(&"done"));
    assert_eq!(b.fake.opens(), 2);
    assert_eq!(b.loaded(), 1);
}

/// Peers = runners of the same app only: another app's idle session stays loaded and serves its next run.
#[test]
fn a_runner_of_another_app_keeps_its_idle_session() {
    let fake = Fake::new::<Echo>(FakeEngine::new());
    let a = Lab::peer(PLUGIN, &fake, &app());
    let b = Lab::peer(OTHER, &fake, &app());
    assert_eq!((a.runner.peers(), b.runner.peers()), (0, 0));
    a.run("a1", &["One."]);
    a.events.ended("a1");
    b.run("b1", &["Two."]);
    b.events.ended("b1");
    assert_eq!(fake.peak(), 2);
    a.run("a2", &["Three."]);
    assert_eq!(kinds(&a.events.ended("a2")), ["piece", "done"]);
    assert_eq!(fake.opens(), 2);
}

/// Plugin off of an idle runner at the moment its peer starts a run: neither waits on the other (bounded), the run
/// ends done, only its model stays loaded.
#[test]
fn plugin_off_of_an_idle_runner_while_a_peer_starts_never_blocks() {
    for round in 0..ROUNDS {
        let (a, b) = Lab::pair(FakeEngine::new());
        a.run("a1", &["One."]);
        a.events.ended("a1");
        let off = std::thread::spawn(move || {
            a.runner.shutdown();
            a
        });
        b.run("b1", &["Two."]);
        let phases = b.events.ended("b1");
        assert_eq!(kinds(&phases).last(), Some(&"done"), "round {round}");
        wait(|| off.is_finished().then_some(()));
        let a = off.join().unwrap();
        assert!(!a.runner.worker_alive(), "round {round}");
        assert_eq!(b.loaded(), 1, "round {round}");
    }
}

/// Two runners starting at the same moment, one of them with an idle session: neither waits on the other, both runs
/// end done.
#[test]
fn two_runners_starting_together_both_finish() {
    for round in 0..ROUNDS {
        let (a, b) = Lab::pair(FakeEngine::new());
        a.run("a1", &["One."]);
        a.events.ended("a1");
        std::thread::scope(|scope| {
            scope.spawn(|| a.run("a2", &["Two."]));
            b.run("b1", &["Three."]);
        });
        for (lab, run) in [(&a, "a2"), (&b, "b1")] {
            let phases = lab.events.ended(run);
            assert_eq!(kinds(&phases).last(), Some(&"done"), "round {round}");
        }
        assert!(b.fake.peak() <= 2, "round {round}");
    }
}

/// Runner of [`Echo`] over the model service of `bench` (the production source) + its events + the lease that keeps
/// the service offered.
fn hosted(bench: &Bench) -> (PieceRunner<Echo>, Arc<Events>, ServiceLease) {
    let services = Services::new();
    let offered = services.provide(Arc::clone(&bench.host));
    let events = Events::new(PLUGIN);
    let cx = events.cx().with_app(AppServices::new(
        RpcTable::new(),
        services,
        Arc::new(RwLock::new(AppSettings::default())),
    ));
    (PieceRunner::hosted(&cx), events, offered)
}

/// Production model source: the person chooses another place for models to run | removes the model while a run
/// works on its warm session -> the model service unloads it under the run, which ends `cancelled` (never
/// `failed`); nothing stays loaded.
#[test]
fn a_hosted_run_on_a_warm_session_the_model_service_unloads_ends_cancelled() {
    let changes: [fn(&Bench); 2] = [
        |bench| drop(bench.installs.set_cpu_only(true).unwrap()),
        |bench| drop(bench.installs.remove(E2B).unwrap()),
    ];
    for change in changes {
        let bench = Bench::fake(|| FakeEngine::new().paced(PACE));
        bench.install(E2B);
        bench.installs.assign(Echo::TASK, Some(E2B)).unwrap();
        let (runner, events, _offered) = hosted(&bench);
        let start = |run: &str, texts: &[&str]| {
            runner
                .start(run.to_owned(), TONE.to_owned(), pieces(texts))
                .unwrap();
        };
        start("r1", &["One."]);
        events.ended("r1");
        start("r2", &MANY);
        wait(|| (!events.phases("r2").is_empty()).then_some(()));
        change(&bench);
        let phases = events.ended("r2");
        assert_eq!(kinds(&phases).last(), Some(&"cancelled"), "{phases:?}");
        assert!(!kinds(&phases).contains(&"loading"), "{phases:?}");
        assert_eq!(bench.workers.launched(), 1);
        bench.unloaded();
        runner.shutdown();
    }
}

/// Production model source (`ModelHost` service): the job's own task picks the model (another job's assignment never
/// serves it); a run loads through the service and keeps the session until that assignment changes.
#[test]
fn a_hosted_runner_uses_the_model_assigned_to_the_jobs_task() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    let (runner, events, _offered) = hosted(&bench);
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
