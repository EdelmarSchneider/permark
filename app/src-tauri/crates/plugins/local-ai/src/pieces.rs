//! Piece-by-piece model runs for PerMark features (e.g. plugin `correction`): pieces made by the window (segment +
//! markers in TS) -> one model answer each, streamed as event `<plugin>:run` ([`RunNews`]); the window judges them.
//!
//! - Per feature: a [`PieceJob`] (task, context, prompt from the run's parameters, answer cap, sampling) + its own
//!   RPC over a [`PieceRunner`]. Everything below lives here once.
//! - One run at a time: a new run cancels the running one (person re-triggered); it ends `cancelled`.
//! - Flow on one worker thread: session = warm one (still the assigned model, still loaded) | opened through
//!   [`Models::open`] (engine missing | outdated -> fetched first, phase `engine`; then phase `loading`) -> per piece
//!   in order: too long for the context -> `tooLong`; else the job's answer ([`answer`]); `Finish::Length` ->
//!   `length`, [`THINKING`] in the answer (thinking leak) -> `thinking`, else `output`. Cancel between pieces + inside
//!   generate. End: `done` | `cancelled` | `failed` (stable [`IpcError`]).
//! - Warm session: kept after a run, unloaded after [`IDLE`] without a run, at plugin off
//!   ([`PieceRunner::shutdown`]: cancel + join, no thread left) and when it no longer serves (model reassigned |
//!   released).
//! - Dormant: nothing runs | loads before a run; [`Models::available`] reads the state alone.
//! - Model access ONLY through [`ModelHost`] ([`PieceRunner::hosted`]); [`Models`] | [`Session`] = seams for fakes.
//! - Text never logged; nothing written to disk.
//! - Tests: `src/tests/pieces.rs`; their fakes = module `testkit`, offered to the tests of the feature plugins too
//!   (Cargo feature `testkit`, dev-dependencies only).

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

use std::cell::Cell;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use permark_core::plugins::{PluginCx, PluginEvents, PluginId, Services};
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::engine::{
    CancelToken, ChatMessage, ChatRequest, EngineError, Finish, GenEvent, Sampling,
};
use crate::host::{engine_ipc, HostError, HostProgress, ModelHost, ModelSession};
use crate::tasks::Task;

/// Chat markup around the turns (not in the token counts), tokens.
const MARKUP: u32 = 64;
/// Warm session kept this long after a run.
pub const IDLE: Duration = Duration::from_secs(120);
/// Answer containing this = thinking leaked (gemma-4 channel markup) -> discarded.
pub const THINKING: &str = "<|channel>";
/// Smallest load progress step sent.
const PROGRESS_STEP: f32 = 0.01;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What one feature asks of the model per piece; the rest of a run is [`PieceRunner`]'s.
pub trait PieceJob: 'static {
    /// Per-run parameters, given with each run (none: `()`).
    type Params: Send + 'static;

    /// Job whose assigned model answers.
    const TASK: Task;
    /// Context window asked for a session (model maximum when smaller), tokens.
    const CONTEXT: u32;
    /// Decoding of every answer.
    const SAMPLING: Sampling;

    /// Chat turns of one piece: the prompt for `params`, then `text` as the last user turn.
    fn messages(params: &Self::Params, text: &str) -> Vec<ChatMessage>;

    /// The prompt text before the piece, for its token count (does the piece fit the context).
    fn preamble(params: &Self::Params) -> String;

    /// Answer cap for a piece of `piece_tokens` tokens.
    fn max_tokens(piece_tokens: u32) -> u32;
}

/// One piece of text for the model (window-made: one prose block | sentence group, non-prose as `⟦n⟧` markers).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, rename = "PieceRunPiece")]
pub struct Piece {
    pub id: u32,
    pub text: String,
}

/// One run's news (event `<plugin>:run`, app-wide: windows keep their own `run`).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "PieceRun")]
pub struct RunNews {
    /// Id the window gave the run.
    pub run: String,
    pub phase: RunPhase,
}

/// Where a run stands; `done` | `cancelled` | `failed` end it.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "PieceRunPhase")]
pub enum RunPhase {
    /// AI engine downloading (first run only | after an engine update), bytes.
    Engine {
        #[ts(type = "number")]
        received_bytes: u64,
        #[ts(type = "number")]
        total_bytes: u64,
    },
    /// Model loading, 0..=1 (not sent when a warm session serves the run).
    Loading {
        progress: f32,
    },
    /// One piece answered, in the order given.
    Piece {
        id: u32,
        result: PieceResult,
    },
    Done,
    Cancelled,
    /// `error`: model service codes (`unavailable/model/noModel`, `unavailable/engine/notAvailable`, engine codes of
    /// [`engine_ipc`]).
    Failed {
        error: IpcError,
    },
}

/// Model answer for one piece.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "PieceRunResult")]
pub enum PieceResult {
    /// Answer as the model wrote it (the window judges it).
    Output {
        text: String,
    },
    Discarded {
        reason: Discard,
    },
}

/// Why a piece's answer was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "PieceRunDiscard")]
pub enum Discard {
    /// Answer hit its cap (rambling).
    Length,
    /// Thinking markup in the answer.
    Thinking,
    /// Piece + prompt + answer room exceed the context.
    TooLong,
}

/// Model source of the runs (production: [`ModelHost`] through [`PieceRunner::hosted`]; tests: a fake).
pub trait Models: Send + Sync {
    /// A model is assigned to the job (state alone: nothing loaded).
    fn available(&self) -> bool;

    /// Loads the assigned model with the job's context window, the engine fetched first when missing; `progress` as
    /// [`ModelHost::open`] (engine bytes, then loading from 0).
    ///
    /// # Errors
    /// Stable model service codes (no model, no engine for this computer, download | engine failures).
    fn open(
        &self,
        progress: &dyn Fn(HostProgress),
        cancel: &CancelToken,
    ) -> Result<Box<dyn Session>, IpcError>;
}

/// A loaded model (Drop unloads).
pub trait Session: Send {
    /// Still the assigned model + still loaded (else the next run opens a new one).
    fn serves(&self) -> bool;

    /// Tokens of `text`.
    ///
    /// # Errors
    /// Engine failure.
    fn count_tokens(&self, text: &str) -> Result<usize, EngineError>;

    /// Streams the answer to `request`.
    ///
    /// # Errors
    /// Engine failure.
    fn generate(
        &self,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError>;
}

/// [`Models`] over the [`ModelHost`] app service (looked up per call; absent -> unavailable) for one job's task.
struct Hosted {
    services: Option<Arc<Services>>,
    task: Task,
    context: u32,
}

impl Hosted {
    fn host(&self) -> Option<Arc<ModelHost>> {
        self.services.as_ref()?.get::<ModelHost>()
    }
}

impl Models for Hosted {
    fn available(&self) -> bool {
        self.host().is_some_and(|h| h.is_assigned(self.task))
    }

    fn open(
        &self,
        progress: &dyn Fn(HostProgress),
        cancel: &CancelToken,
    ) -> Result<Box<dyn Session>, IpcError> {
        let host = self.host().ok_or(HostError::NoModel(self.task))?;
        let session = host.open(self.task, self.context, progress, cancel)?;
        Ok(Box::new(HostedSession {
            host,
            session,
            task: self.task,
        }))
    }
}

struct HostedSession {
    host: Arc<ModelHost>,
    session: ModelSession,
    task: Task,
}

impl Session for HostedSession {
    fn serves(&self) -> bool {
        self.host.serves(&self.session, self.task)
            && self.session.engine().context_len(self.session.id()).is_ok()
    }

    fn count_tokens(&self, text: &str) -> Result<usize, EngineError> {
        self.session.engine().count_tokens(self.session.id(), text)
    }

    fn generate(
        &self,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        self.session
            .engine()
            .generate(self.session.id(), request, sink, cancel)
    }
}

/// A run asked by a window.
struct Asked<P> {
    run: String,
    params: P,
    pieces: Vec<Piece>,
    cancel: CancelToken,
}

struct State<P> {
    /// Asked, not started yet.
    pending: Option<Asked<P>>,
    /// Id + token of the run on the worker.
    running: Option<(String, CancelToken)>,
    /// Session kept between runs.
    warm: Option<Box<dyn Session>>,
    /// Worker thread alive.
    worker: Option<JoinHandle<()>>,
    /// Worker gave up (idle | closed); its handle still to join.
    worker_done: bool,
    closed: bool,
}

struct Shared<J: PieceJob> {
    /// Owning plugin: worker thread name + subject of the refusal while stopping.
    plugin: PluginId,
    models: Arc<dyn Models>,
    events: PluginEvents,
    idle: Duration,
    state: Mutex<State<J::Params>>,
    wake: Condvar,
}

/// Runs of job `J` for one plugin instance (rules: module doc).
pub struct PieceRunner<J: PieceJob> {
    shared: Arc<Shared<J>>,
}

impl<J: PieceJob> PieceRunner<J> {
    /// Runner of plugin `cx` over the model assigned to the job's task ([`ModelHost`] service; absent ->
    /// unavailable); warm session kept [`IDLE`]. Nothing started | loaded (dormant).
    #[must_use]
    pub fn hosted(cx: &PluginCx) -> Self {
        let models = Hosted {
            services: cx.app().map(|app| Arc::clone(&app.services)),
            task: J::TASK,
            context: J::CONTEXT,
        };
        Self::new(cx, Arc::new(models), IDLE)
    }

    /// Runner of plugin `cx` over `models` (tests: a fake); `idle` = warm session lifetime without a run. Nothing
    /// started | loaded (dormant).
    #[must_use]
    pub fn new(cx: &PluginCx, models: Arc<dyn Models>, idle: Duration) -> Self {
        Self {
            shared: Arc::new(Shared {
                plugin: cx.id.clone(),
                models,
                events: cx.events().clone(),
                idle,
                state: Mutex::new(State {
                    pending: None,
                    running: None,
                    warm: None,
                    worker: None,
                    worker_done: false,
                    closed: false,
                }),
                wake: Condvar::new(),
            }),
        }
    }

    /// A model is assigned to the job (state alone: nothing loaded; it may since be gone -> the run fails
    /// `noModel`).
    #[must_use]
    pub fn available(&self) -> bool {
        self.shared.models.available()
    }

    /// Starts run `run` over `pieces` with `params` (news as events); cancels the run in progress | not started yet.
    ///
    /// # Errors
    /// Plugin stopping; no worker thread.
    pub fn start(
        &self,
        run: String,
        params: J::Params,
        pieces: Vec<Piece>,
    ) -> Result<(), IpcError> {
        let shared = &self.shared;
        let mut state = lock(&shared.state);
        if state.closed {
            return Err(IpcError::new(ErrorKind::Unavailable).subject(shared.plugin.as_str()));
        }
        if let Some((_, token)) = &state.running {
            token.cancel();
        }
        let asked = Asked {
            run,
            params,
            pieces,
            cancel: CancelToken::new(),
        };
        if let Some(replaced) = state.pending.replace(asked) {
            shared.emit(&replaced.run, RunPhase::Cancelled);
        }
        if state.worker.is_none() || state.worker_done {
            let previous = state.worker.take();
            let s = Arc::clone(shared);
            let handle = std::thread::Builder::new()
                .name(shared.plugin.as_str().to_owned())
                .spawn(move || {
                    // the previous worker gave up: its last step was the unlock
                    if let Some(previous) = previous {
                        let _ = previous.join();
                    }
                    s.work();
                })
                .map_err(|e| IpcError::new(ErrorKind::Internal).detail(e.to_string()))?;
            state.worker = Some(handle);
            state.worker_done = false;
        }
        shared.wake.notify_all();
        Ok(())
    }

    /// Cancels run `run` (other | no run: nothing).
    pub fn cancel(&self, run: &str) {
        let shared = &self.shared;
        let mut state = lock(&shared.state);
        if let Some((id, token)) = &state.running {
            if id == run {
                token.cancel();
            }
        }
        if state.pending.as_ref().is_some_and(|asked| asked.run == run) {
            if let Some(asked) = state.pending.take() {
                shared.emit(&asked.run, RunPhase::Cancelled);
            }
        }
    }

    /// Worker thread running (tests: no thread left after idle | off).
    #[cfg(test)]
    pub(crate) fn worker_alive(&self) -> bool {
        lock(&self.shared.state)
            .worker
            .as_ref()
            .is_some_and(|w| !w.is_finished())
    }

    /// Plugin off: cancels the run, unloads the warm session, joins the worker (no thread left). Idempotent.
    pub fn shutdown(&self) {
        let shared = &self.shared;
        let (worker, warm) = {
            let mut state = lock(&shared.state);
            state.closed = true;
            if let Some((_, token)) = &state.running {
                token.cancel();
            }
            if let Some(asked) = state.pending.take() {
                shared.emit(&asked.run, RunPhase::Cancelled);
            }
            (state.worker.take(), state.warm.take())
        };
        shared.wake.notify_all();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
        drop(warm);
    }
}

impl<J: PieceJob> Drop for PieceRunner<J> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl<J: PieceJob> Shared<J> {
    fn emit(&self, run: &str, phase: RunPhase) {
        let news = RunNews {
            run: run.to_owned(),
            phase,
        };
        if let Ok(payload) = serde_json::to_value(news) {
            self.events.emit("run", payload);
        }
    }

    /// Worker loop: runs what was asked; idle past `idle` | closed -> warm session unloaded, thread ends.
    fn work(&self) {
        loop {
            let (asked, warm) = {
                let state = lock(&self.state);
                let (mut state, timeout) = self
                    .wake
                    .wait_timeout_while(state, self.idle, |s| s.pending.is_none() && !s.closed)
                    .unwrap_or_else(PoisonError::into_inner);
                if state.closed || (timeout.timed_out() && state.pending.is_none()) {
                    state.worker_done = true;
                    let warm = state.warm.take();
                    drop(state);
                    drop(warm);
                    return;
                }
                let Some(asked) = state.pending.take() else {
                    continue;
                };
                state.running = Some((asked.run.clone(), asked.cancel.clone()));
                (asked, state.warm.take())
            };
            let (phase, session) = self.run(&asked, warm);
            let mut state = lock(&self.state);
            // closed meanwhile: unloaded here (after the unlock), never kept
            let closing = if state.closed {
                session
            } else {
                state.warm = session;
                None
            };
            state.running = None;
            drop(state);
            // after the unlock: a sink never runs under the state lock
            self.emit(&asked.run, phase);
            drop(closing);
        }
    }

    /// One run; answers the last phase + the session to keep warm.
    fn run(
        &self,
        asked: &Asked<J::Params>,
        warm: Option<Box<dyn Session>>,
    ) -> (RunPhase, Option<Box<dyn Session>>) {
        let cancel = &asked.cancel;
        if asked.pieces.is_empty() {
            return (RunPhase::Done, warm);
        }
        let session = match warm.filter(|s| s.serves()) {
            Some(session) => session,
            None => match self.open(&asked.run, cancel) {
                Ok(session) => session,
                Err(_) if cancel.is_cancelled() => return (RunPhase::Cancelled, None),
                Err(error) => return (RunPhase::Failed { error }, None),
            },
        };
        let phase = match self.pieces(asked, session.as_ref()) {
            _ if cancel.is_cancelled() => RunPhase::Cancelled,
            Ok(()) => RunPhase::Done,
            Err(EngineError::Cancelled) => RunPhase::Cancelled,
            Err(error) => {
                // engine trouble: never keep this session
                return (
                    RunPhase::Failed {
                        error: engine_ipc(&error),
                    },
                    None,
                );
            }
        };
        (phase, Some(session))
    }

    fn open(&self, run: &str, cancel: &CancelToken) -> Result<Box<dyn Session>, IpcError> {
        // load progress: the first value, then steps of at least PROGRESS_STEP
        let sent = Cell::new(None::<f32>);
        let progress = |news: HostProgress| match news {
            HostProgress::Engine { received, total } => self.emit(
                run,
                RunPhase::Engine {
                    received_bytes: received,
                    total_bytes: total,
                },
            ),
            HostProgress::Loading(value) => {
                if value.is_finite() && sent.get().is_none_or(|s| value - s >= PROGRESS_STEP) {
                    sent.set(Some(value));
                    self.emit(run, RunPhase::Loading { progress: value });
                }
            }
        };
        self.models.open(&progress, cancel)
    }

    /// Every piece in order (stops early on cancel).
    fn pieces(&self, asked: &Asked<J::Params>, session: &dyn Session) -> Result<(), EngineError> {
        let preamble = preamble_tokens::<J>(session, &asked.params)?;
        for piece in &asked.pieces {
            if asked.cancel.is_cancelled() {
                return Ok(());
            }
            let Some(result) =
                answer::<J>(session, &asked.params, preamble, &piece.text, &asked.cancel)?
            else {
                return Ok(());
            };
            self.emit(
                &asked.run,
                RunPhase::Piece {
                    id: piece.id,
                    result,
                },
            );
        }
        Ok(())
    }
}

fn tokens(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Tokens of the job's prompt before the piece ([`PieceJob::preamble`]), counted once per run.
///
/// # Errors
/// Engine failure.
pub fn preamble_tokens<J: PieceJob>(
    session: &dyn Session,
    params: &J::Params,
) -> Result<u32, EngineError> {
    Ok(tokens(session.count_tokens(&J::preamble(params))?))
}

/// One piece (the run path, also driven directly by a feature's eval set); `None`: cancelled during the answer.
///
/// # Errors
/// Engine failure.
pub fn answer<J: PieceJob>(
    session: &dyn Session,
    params: &J::Params,
    preamble: u32,
    text: &str,
    cancel: &CancelToken,
) -> Result<Option<PieceResult>, EngineError> {
    let discarded = |reason| Ok(Some(PieceResult::Discarded { reason }));
    let piece = tokens(session.count_tokens(text)?);
    let max_tokens = J::max_tokens(piece);
    let needed = u64::from(preamble) + u64::from(piece) + u64::from(max_tokens) + u64::from(MARKUP);
    if needed > u64::from(J::CONTEXT) {
        return discarded(Discard::TooLong);
    }
    let request = ChatRequest {
        messages: J::messages(params, text),
        sampling: J::SAMPLING,
        max_tokens,
        ..ChatRequest::default()
    };
    let mut output = String::new();
    let mut finish = None;
    session.generate(
        &request,
        &mut |event| match event {
            GenEvent::Delta(delta) => output.push_str(&delta),
            GenEvent::Done { finish: f, .. } => finish = Some(f),
            GenEvent::ToolCall { .. } => {}
        },
        cancel,
    )?;
    match finish {
        Some(Finish::Cancelled) => Ok(None),
        _ if cancel.is_cancelled() => Ok(None),
        Some(Finish::Length) => discarded(Discard::Length),
        _ if output.contains(THINKING) => discarded(Discard::Thinking),
        _ => Ok(Some(PieceResult::Output { text: output })),
    }
}
