//! "Test model" (AC-7): proves an installed model runs on this computer, end to end.
//!
//! - One test at a time per plugin instance: a second start while one runs -> [`TestError::Busy`] (never cancels the
//!   running one). Start answers at once; the test runs on its own thread, phases go out as event `local-ai:test`
//!   ([`ModelTest`]).
//! - Flow: [`ModelHost`] open (same path as features): worker files missing -> fetched (phase `engine`, download
//!   bytes) -> load (memory guard inside; phase `loading`) -> run (phase `running`): chat model = [`PROMPT`]
//!   streamed (<= [`MAX_TOKENS`]), embedding-only model = one vector of [`EMBED_TEXT`] -> `done` with timings.
//! - The session is ALWAYS unloaded (done | failed | cancelled) before the last event goes out.
//! - Cancel ([`Tester::cancel`]): stops the engine download (part kept) | the load (worker killed) | the run.
//! - Device: the load runs where the person chose ([`ModelHost`]: GPU when the model fits | processor only); result
//!   `gpu` = the session really runs on it ([`crate::host::ModelSession::on_gpu`]; processor only: never). Memory =
//!   the engine's estimate (lower bound: file bytes). The choice changed mid-test: session unloaded, test cancelled.
//! - Load, unload, `Blocked` (D-14) + engine error codes: [`crate::host`].
//! - Dormant: nothing runs | is created before a start; plugin off ([`Tester::shutdown`]) cancels + joins.
//! - Tests: `src/tests/testrun.rs` (shares the installs lab).

use std::cell::Cell;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use permark_core::downloads::DownloadError;
use permark_core::plugins::PluginEvents;
use permark_core::{ErrorKind, IpcError};
use serde::Serialize;
use ts_rs::TS;

use crate::catalog::Model;
use crate::engine::{
    CancelToken, Capability, ChatMessage, ChatRequest, EmbedRole, Engine, EngineError, Finish,
    GenEvent, Role, SessionId,
};
use crate::host::{engine_ipc, HostError, HostProgress, ModelHost};
use crate::installs::{AssignedModel, InstallError};

/// Chat test prompt.
pub const PROMPT: &str = "Say hello in one short sentence.";
/// Answer cap of the chat test, tokens.
pub const MAX_TOKENS: u32 = 32;
/// Embedding test text.
pub const EMBED_TEXT: &str = "PerMark checks that this model works.";
/// Context window of a test session (model maximum when smaller), tokens.
const TEST_CONTEXT: u32 = 2048;
/// Answer kept for the result, chars.
const ANSWER_CAP: usize = 400;
/// Smallest load progress step sent.
const PROGRESS_STEP: f32 = 0.01;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One test's news (event `local-ai:test`).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ModelTest")]
pub struct ModelTest {
    /// Catalog id the test was started for.
    pub model: String,
    pub phase: TestPhase,
}

/// Where a test stands; `done` | `failed` | `cancelled` end it.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "ModelTestPhase")]
pub enum TestPhase {
    /// Model engine downloading (first test only).
    Engine {
        #[ts(type = "number")]
        received_bytes: u64,
        #[ts(type = "number")]
        total_bytes: u64,
    },
    /// Model loading, 0..=1.
    Loading {
        progress: f32,
    },
    Running,
    Done {
        result: TestResult,
    },
    /// `error`: see [`TestError`] -> [`IpcError`] (stable `kind` + `detail` codes).
    Failed {
        error: IpcError,
    },
    Cancelled,
}

/// What a finished test measured.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ModelTestResult")]
pub struct TestResult {
    #[ts(type = "number")]
    pub load_ms: u64,
    #[ts(type = "number")]
    pub run_ms: u64,
    /// The model ran (partly) on the graphics card (layers placed there at load).
    pub gpu: bool,
    /// Engine estimate of the loaded model, bytes (lower bound); `None`: unknown.
    #[ts(type = "number | null")]
    pub memory_bytes: Option<u64>,
    /// Engine release that ran it.
    pub engine_version: String,
    pub output: TestOutput,
}

/// What the run produced.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "ModelTestOutput")]
pub enum TestOutput {
    Chat {
        tokens: u32,
        tokens_per_second: f64,
        /// Model's answer (trimmed, capped).
        answer: String,
    },
    Embed {
        dims: u32,
    },
}

/// Why a test is refused | failed.
#[derive(Debug, thiserror::Error)]
pub enum TestError {
    #[error("a model test is running")]
    Busy,
    /// Model offers neither chat nor embedding (nothing this build can test).
    #[error("model {0} has nothing to test")]
    NoTest(String),
    #[error(transparent)]
    Install(#[from] InstallError),
    /// Engine fetch | load ([`ModelHost`] codes).
    #[error(transparent)]
    Host(#[from] HostError),
    #[error(transparent)]
    Engine(#[from] EngineError),
}

impl From<TestError> for IpcError {
    /// Engine | files errors: [`HostError`] codes + [`engine_ipc`]; engine text | worker log NEVER sent (may hold
    /// paths).
    fn from(error: TestError) -> Self {
        match error {
            TestError::Busy => IpcError::new(ErrorKind::Conflict).detail("testRunning"),
            TestError::NoTest(id) => IpcError::new(ErrorKind::Unsupported)
                .subject(id)
                .detail("noTest"),
            TestError::Install(e) => e.into(),
            TestError::Host(e) => e.into(),
            TestError::Engine(e) => engine_ipc(&e),
        }
    }
}

/// What a test runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chat,
    Embed,
}

/// What a test needs of the plugin instance.
pub struct TestParts {
    /// Installs, catalog, runtime, worker files; open -> [`crate::host::ModelSession`] (unloaded on every way out).
    pub host: Arc<ModelHost>,
    pub events: PluginEvents,
}

struct Shared {
    parts: TestParts,
    /// Token of the running test (`None`: idle).
    running: Mutex<Option<CancelToken>>,
}

/// Test runner of one plugin instance (rules: module doc).
pub struct Tester {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Tester {
    #[must_use]
    pub fn new(parts: TestParts) -> Self {
        Self {
            shared: Arc::new(Shared {
                parts,
                running: Mutex::new(None),
            }),
            thread: Mutex::new(None),
        }
    }

    /// Starts a test of installed model `id` (phases as events).
    ///
    /// # Errors
    /// [`TestError::Busy`], [`TestError::NoTest`], unknown | not installed model ([`TestError::Install`]), the pinned
    /// worker cannot run it ([`TestError::Engine`]: `EngineTooOld` | `Unsupported`).
    pub fn start(&self, id: &str) -> Result<(), TestError> {
        let mut running = lock(&self.shared.running);
        if running.is_some() {
            return Err(TestError::Busy);
        }
        let host = &self.shared.parts.host;
        let installed = host.installs.installed_model(id)?;
        let model = host
            .catalog
            .model(id)
            .ok_or_else(|| InstallError::UnknownModel(id.to_owned()))?
            .clone();
        let kind = if model.capabilities.contains(&Capability::Chat) {
            Kind::Chat
        } else if model.capabilities.contains(&Capability::Embed) {
            Kind::Embed
        } else {
            return Err(TestError::NoTest(id.to_owned()));
        };
        if let Some(refusal) = host.refusal(&model) {
            return Err(refusal.into());
        }
        let token = CancelToken::new();
        // the last test released `running` as its final step: joining it waits for nothing
        if let Some(done) = lock(&self.thread).take() {
            let _ = done.join();
        }
        let shared = Arc::clone(&self.shared);
        let cancel = token.clone();
        let id = id.to_owned();
        let handle = std::thread::Builder::new()
            .name("local-ai-test".into())
            .spawn(move || {
                let phase = shared.run(&model, &installed, kind, &cancel);
                shared.finish(&id, phase);
            })
            .map_err(|e| EngineError::Io(e.to_string()))?;
        *running = Some(token);
        *lock(&self.thread) = Some(handle);
        Ok(())
    }

    /// Cancels the running test (none: nothing).
    pub fn cancel(&self) {
        if let Some(token) = lock(&self.shared.running).as_ref() {
            token.cancel();
        }
    }

    /// Plugin off: cancels the running test and waits for its end (session unloaded). Idempotent.
    pub fn shutdown(&self) {
        self.cancel();
        if let Some(thread) = lock(&self.thread).take() {
            let _ = thread.join();
        }
    }
}

impl Shared {
    fn emit(&self, model: &str, phase: TestPhase) {
        let news = ModelTest {
            model: model.to_owned(),
            phase,
        };
        if let Ok(payload) = serde_json::to_value(news) {
            self.parts.events.emit("test", payload);
        }
    }

    /// Last event, then idle (one lock: a new start sees idle only after the end went out).
    fn finish(&self, model: &str, phase: TestPhase) {
        let mut running = lock(&self.running);
        self.emit(model, phase);
        *running = None;
    }

    fn run(
        &self,
        model: &Model,
        installed: &AssignedModel,
        kind: Kind,
        cancel: &CancelToken,
    ) -> TestPhase {
        match self.steps(model, installed, kind, cancel) {
            Ok(result) => TestPhase::Done { result },
            Err(_) if cancel.is_cancelled() => TestPhase::Cancelled,
            Err(
                TestError::Engine(EngineError::Cancelled)
                | TestError::Host(
                    HostError::Engine(EngineError::Cancelled)
                    | HostError::Download(DownloadError::Cancelled),
                ),
            ) => TestPhase::Cancelled,
            Err(e) => TestPhase::Failed { error: e.into() },
        }
    }

    fn steps(
        &self,
        model: &Model,
        installed: &AssignedModel,
        kind: Kind,
        cancel: &CancelToken,
    ) -> Result<TestResult, TestError> {
        // load progress: the first value, then steps of at least PROGRESS_STEP
        let sent = Cell::new(None::<f32>);
        let started = Cell::new(Instant::now());
        let progress = |news: HostProgress| match news {
            HostProgress::Engine { received, total } => self.emit(
                &model.id,
                TestPhase::Engine {
                    received_bytes: received,
                    total_bytes: total,
                },
            ),
            HostProgress::Loading(value) => {
                if sent.get().is_none() {
                    started.set(Instant::now());
                }
                if value.is_finite() && sent.get().is_none_or(|s| value - s >= PROGRESS_STEP) {
                    sent.set(Some(value));
                    self.emit(&model.id, TestPhase::Loading { progress: value });
                }
            }
        };
        let loaded =
            self.parts
                .host
                .open_model(model, installed, TEST_CONTEXT, &progress, cancel)?;
        let load_ms = millis(started.get().elapsed());
        self.emit(&model.id, TestPhase::Running);
        let caps = loaded.engine().caps();
        let started = Instant::now();
        let output = match kind {
            Kind::Chat => chat(loaded.engine(), loaded.id(), cancel, started)?,
            Kind::Embed => {
                let embeddings = loaded.engine().embed(
                    loaded.id(),
                    &[EMBED_TEXT],
                    EmbedRole::Document,
                    None,
                    true,
                    cancel,
                )?;
                TestOutput::Embed {
                    dims: embeddings.fingerprint.dims,
                }
            }
        };
        let run_ms = millis(started.elapsed());
        let (gpu, memory_bytes) = (loaded.on_gpu(), loaded.memory_bytes());
        drop(loaded);
        Ok(TestResult {
            load_ms,
            run_ms,
            gpu,
            memory_bytes,
            engine_version: caps.version,
            output,
        })
    }
}

/// [`PROMPT`] streamed; speed = writing speed after the first token (see below).
fn chat(
    engine: &dyn Engine,
    session: SessionId,
    cancel: &CancelToken,
    started: Instant,
) -> Result<TestOutput, TestError> {
    let request = ChatRequest {
        messages: vec![ChatMessage {
            role: Role::User,
            content: PROMPT.to_owned(),
        }],
        max_tokens: MAX_TOKENS,
        ..ChatRequest::default()
    };
    let mut answer = String::new();
    let mut deltas = 0u32;
    let mut first = None;
    let mut end = None;
    engine.generate(
        session,
        &request,
        &mut |event| match event {
            GenEvent::Delta(text) => {
                first.get_or_insert_with(Instant::now);
                deltas = deltas.saturating_add(1);
                answer.push_str(&text);
            }
            GenEvent::Done { usage, finish } => end = Some((usage, finish)),
            GenEvent::ToolCall { .. } => {}
        },
        cancel,
    )?;
    let tokens = match end {
        Some((_, Finish::Cancelled)) => return Err(EngineError::Cancelled.into()),
        Some((usage, _)) if usage.completion > 0 => usage.completion,
        _ => deltas,
    };
    // writing speed: tokens after the first one over the time since it (prompt reading + first-run warm-up, e.g.
    // GPU shader setup, would make a short test look slow); fewer than 2 tokens -> overall time
    let tokens_per_second = match first {
        Some(at) if deltas > 1 => per_second(deltas - 1, at.elapsed()),
        _ => per_second(tokens, started.elapsed()),
    };
    Ok(TestOutput::Chat {
        tokens,
        tokens_per_second,
        answer: answer.trim().chars().take(ANSWER_CAP).collect(),
    })
}

fn millis(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

/// Tokens per second (finite: no time -> 0).
fn per_second(tokens: u32, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    let rate = f64::from(tokens) / secs;
    if secs > 0.0 && rate.is_finite() {
        rate
    } else {
        0.0
    }
}
