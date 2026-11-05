//! "Test model" (AC-7): proves an installed model runs on this computer, end to end.
//!
//! - One test at a time per plugin instance: a second start while one runs -> [`TestError::Busy`] (never cancels the
//!   running one). Start answers at once; the test runs on its own thread, phases go out as event `local-ai:test`
//!   ([`ModelTest`]).
//! - Flow: worker files missing -> queued through [`WorkerFiles`] (phase `engine`, download bytes) -> load through
//!   [`Runtime::load`] (memory guard inside; phase `loading`) -> run (phase `running`): chat model = [`PROMPT`]
//!   streamed (<= [`MAX_TOKENS`]), embedding-only model = one vector of [`EMBED_TEXT`] -> `done` with timings.
//! - The session is ALWAYS unloaded (done | failed | cancelled) before the last event goes out.
//! - Cancel ([`Tester::cancel`]): stops the engine download (part kept) | the load (worker killed) | the run.
//! - Device: the contract tells only whether a GPU backend is usable (`gpu`); load uses `Device::Auto` (GPU when the
//!   model fits). Memory = the engine's estimate (lower bound: file bytes).
//! - Dormant: nothing runs | is created before a start; plugin off ([`Tester::shutdown`]) cancels + joins.
//! - Tests: `src/tests/testrun.rs` (shares the installs lab).

use std::cell::Cell;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use permark_core::downloads::{DownloadError, Listener, Progress, Update};
use permark_core::plugins::PluginEvents;
use permark_core::{ErrorKind, IpcError};
use serde::Serialize;
use ts_rs::TS;

use crate::catalog::{Catalog, Model};
use crate::engine::files::{FilesError, WorkerFiles};
use crate::engine::{
    self, CancelToken, Capability, ChatMessage, ChatRequest, Device, EmbedRole, Engine,
    EngineError, Finish, GenEvent, LoadOpts, Role, SessionId,
};
use crate::installs::{AssignedModel, InstallError, Installs};
use crate::runtime::Runtime;

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
/// Cancel check interval while the engine downloads.
const POLL: Duration = Duration::from_millis(50);
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
    /// A GPU backend was usable (load picks it when the model fits).
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
    #[error(transparent)]
    Files(#[from] FilesError),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Engine(#[from] EngineError),
}

impl From<TestError> for IpcError {
    /// Engine | files errors: subject `engine`, `detail` = stable code (`outOfMemory`, `notAvailable`, `crashed`,
    /// `blocked`, `engineTooOld`, `unsupported`, `corrupt`, `notLoaded`, `cancelled`, `badPin`); worker log NEVER
    /// sent (may hold paths).
    fn from(error: TestError) -> Self {
        let engine = |kind, detail: &str| IpcError::new(kind).subject("engine").detail(detail);
        match error {
            TestError::Busy => IpcError::new(ErrorKind::Conflict).detail("testRunning"),
            TestError::NoTest(id) => IpcError::new(ErrorKind::Unsupported)
                .subject(id)
                .detail("noTest"),
            TestError::Install(e) => e.into(),
            TestError::Files(FilesError::NotAvailable) => {
                engine(ErrorKind::Unavailable, "notAvailable")
            }
            TestError::Files(FilesError::NoDownloads) => {
                IpcError::new(ErrorKind::Unavailable).subject("downloads")
            }
            TestError::Files(FilesError::BadPin) => engine(ErrorKind::Internal, "badPin"),
            TestError::Download(e) => IpcError::from(e).subject("engine"),
            TestError::Engine(e) => match e {
                EngineError::OutOfMemory => engine(ErrorKind::Unavailable, "outOfMemory"),
                EngineError::Unsupported { .. } => engine(ErrorKind::Unsupported, "unsupported"),
                EngineError::Corrupt => engine(ErrorKind::InvalidData, "corrupt"),
                EngineError::Cancelled => engine(ErrorKind::Superseded, "cancelled"),
                EngineError::EngineCrashed => engine(ErrorKind::Unavailable, "crashed"),
                EngineError::EngineTooOld { .. } => engine(ErrorKind::Unsupported, "engineTooOld"),
                EngineError::NotLoaded => engine(ErrorKind::Unavailable, "notLoaded"),
                EngineError::Blocked => engine(ErrorKind::Denied, "blocked"),
                EngineError::Io(detail) => IpcError::new(ErrorKind::Io)
                    .subject("engine")
                    .detail(detail),
            },
        }
    }
}

/// Worker-unit download updates -> the running test (the [`WorkerFiles`] listener; nobody listening -> dropped).
#[derive(Default)]
pub struct EngineNews(Mutex<Option<Sender<Update>>>);

impl EngineNews {
    /// Listener for [`WorkerFiles::new`].
    #[must_use]
    pub fn listener(self: &Arc<Self>) -> Listener {
        let news = Arc::clone(self);
        Arc::new(move |_, update| {
            if let Some(tx) = lock(&news.0).as_ref() {
                let _ = tx.send(update.clone());
            }
        })
    }

    fn open(&self) -> Receiver<Update> {
        let (tx, rx) = mpsc::channel();
        *lock(&self.0) = Some(tx);
        rx
    }

    fn close(&self) {
        *lock(&self.0) = None;
    }
}

/// What a test runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chat,
    Embed,
}

/// Session unloaded when dropped (every way out of a test).
struct Loaded<'a> {
    runtime: &'a Runtime,
    session: SessionId,
}

impl Drop for Loaded<'_> {
    fn drop(&mut self) {
        self.runtime.unload(self.session);
    }
}

/// What a test needs of the plugin instance.
pub struct TestParts {
    pub installs: Arc<Installs>,
    pub catalog: Arc<Catalog>,
    pub runtime: Arc<Runtime>,
    pub files: Arc<WorkerFiles>,
    /// Same as the one given to `files` ([`EngineNews::listener`]).
    pub news: Arc<EngineNews>,
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
    /// [`TestError::Busy`], [`TestError::NoTest`], unknown | not installed model ([`TestError::Install`]).
    pub fn start(&self, id: &str) -> Result<(), TestError> {
        let mut running = lock(&self.shared.running);
        if running.is_some() {
            return Err(TestError::Busy);
        }
        let parts = &self.shared.parts;
        let installed = parts.installs.installed_model(id)?;
        let model = parts
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
                | TestError::Download(DownloadError::Cancelled),
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
        let TestParts {
            catalog,
            runtime,
            files,
            ..
        } = &self.parts;
        self.ensure_engine(&model.id, cancel)?;
        let model_files = engine::model_files(installed, catalog);
        let opts = LoadOpts {
            context_len: model.context_len.min(TEST_CONTEXT),
            device: Device::Auto,
            threads: None,
        };
        let memory_bytes = runtime
            .engine()
            .estimate_memory(&model_files, &opts)
            .ok()
            .map(|m| m.ram.saturating_add(m.vram));
        self.emit(&model.id, TestPhase::Loading { progress: 0.0 });
        let sent = Cell::new(0.0f32);
        let progress = |value: f32| {
            if value.is_finite() && value - sent.get() >= PROGRESS_STEP {
                sent.set(value);
                self.emit(&model.id, TestPhase::Loading { progress: value });
            }
        };
        let started = Instant::now();
        let session = runtime
            .load(
                &installed.lineage,
                &model.needs,
                model_files,
                opts,
                &progress,
                cancel,
            )
            .map_err(|e| match e {
                // verified files gone at spawn time = taken by security software (D-14)
                EngineError::Io(_) if !files.is_installed() => EngineError::Blocked,
                e => e,
            })?;
        let loaded = Loaded {
            runtime: runtime.as_ref(),
            session,
        };
        let load_ms = millis(started.elapsed());
        self.emit(&model.id, TestPhase::Running);
        let caps = runtime.engine().caps();
        let started = Instant::now();
        let output = match kind {
            Kind::Chat => chat(runtime.engine(), loaded.session, cancel, started)?,
            Kind::Embed => {
                let embeddings = runtime.engine().embed(
                    loaded.session,
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
        drop(loaded);
        Ok(TestResult {
            load_ms,
            run_ms,
            gpu: caps.gpu,
            memory_bytes,
            engine_version: caps.version,
            output,
        })
    }

    /// Worker files installed now, else downloaded (phase `engine`); cancel stops the download (part kept).
    fn ensure_engine(&self, model: &str, cancel: &CancelToken) -> Result<(), TestError> {
        let TestParts { files, news, .. } = &self.parts;
        if files.is_installed() {
            return Ok(());
        }
        let updates = news.open();
        let result = files
            .enqueue()
            .map_err(TestError::from)
            .and_then(|()| self.follow(model, &updates, cancel));
        news.close();
        result
    }

    fn follow(
        &self,
        model: &str,
        updates: &Receiver<Update>,
        cancel: &CancelToken,
    ) -> Result<(), TestError> {
        loop {
            if cancel.is_cancelled() {
                self.parts.files.cancel();
                return Err(DownloadError::Cancelled.into());
            }
            match updates.recv_timeout(POLL) {
                Ok(Update::Progress(Progress { done, total, .. })) => self.emit(
                    model,
                    TestPhase::Engine {
                        received_bytes: done,
                        total_bytes: total,
                    },
                ),
                Ok(Update::Done) => return Ok(()),
                Ok(Update::Failed(error)) => return Err(error.into()),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err(DownloadError::Cancelled.into()),
            }
        }
    }
}

/// [`PROMPT`] streamed; speed over the time since `started`.
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
    let mut end = None;
    engine.generate(
        session,
        &request,
        &mut |event| match event {
            GenEvent::Delta(text) => {
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
    Ok(TestOutput::Chat {
        tokens,
        tokens_per_second: per_second(tokens, started.elapsed()),
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
