//! Correction runs (D-1, D-4): pieces made by the window (segment + markers in TS) -> one model answer each, streamed
//! as event `correction:run` ([`RunNews`]); the window diffs + guards them.
//!
//! - One run at a time: a new run cancels the running one (person re-triggered); it ends `cancelled`.
//! - Flow on one worker thread: session = warm one (still the assigned model, still loaded) | opened through
//!   [`Models::open`] (phase `loading`) -> per piece in order: too long for the context -> `tooLong`; else greedy
//!   answer ([`prompt`]); `Finish::Length` -> `length`, `<|channel>` in the answer (thinking leak) -> `thinking`, else
//!   `output`. Cancel between pieces + inside generate. End: `done` | `cancelled` | `failed` (stable [`IpcError`]).
//! - Warm session: kept after a run, unloaded after [`IDLE`] without a run, at plugin off ([`Corrector::shutdown`]:
//!   cancel + join, no thread left) and when it no longer serves (model reassigned | released by `local-ai`).
//! - Dormant: nothing runs | loads before a run; [`Models::available`] reads the state alone.
//! - Text never logged; nothing written to disk.
//! - Tests: `src/tests.rs`.

use std::cell::Cell;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use permark_core::plugins::PluginEvents;
use permark_core::{ErrorKind, IpcError};
use permark_plugin_local_ai::engine::{CancelToken, ChatRequest, EngineError, Finish, GenEvent};
use permark_plugin_local_ai::engine_ipc;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::prompt;

/// Context window asked for a correction session (D-4), tokens.
pub const CONTEXT: u32 = 4096;
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

/// Answer of `correction.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CorrectionStatus {
    /// A model is assigned to job "correction" (state alone; it may since be gone -> the run fails `noModel`).
    pub available: bool,
}

/// One piece of text to correct (window-made: one prose block | sentence group, non-prose as `⟦n⟧` markers).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, rename = "CorrectionPiece")]
pub struct Piece {
    pub id: u32,
    pub text: String,
}

/// One run's news (event `correction:run`, app-wide: windows keep their own `run`).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "CorrectionRun")]
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
#[ts(export, rename = "CorrectionPhase")]
pub enum RunPhase {
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
    /// `error`: `local-ai` model service codes (`unavailable/model/noModel`, `unavailable/engine/notAvailable`,
    /// engine codes of `engine_ipc`).
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
#[ts(export, rename = "CorrectionPieceResult")]
pub enum PieceResult {
    /// Corrected text as the model wrote it (window diffs + guards it).
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
#[ts(export, rename = "CorrectionDiscard")]
pub enum Discard {
    /// Answer hit its cap (rambling).
    Length,
    /// Thinking markup in the answer.
    Thinking,
    /// Piece + prompt + answer room exceed the context.
    TooLong,
}

/// Model source of the runs (production: `local-ai`'s `ModelHost`, adapter in `lib.rs`; tests: a fake).
pub trait Models: Send + Sync {
    /// A model is assigned to correction (state alone: nothing loaded).
    fn available(&self) -> bool;

    /// Loads the assigned model with a [`CONTEXT`] window; `progress` 0..=1.
    ///
    /// # Errors
    /// Stable model service codes (no model, engine not installed, engine failures).
    fn open(
        &self,
        progress: &dyn Fn(f32),
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

/// A run asked by a window.
struct Job {
    run: String,
    pieces: Vec<Piece>,
    cancel: CancelToken,
}

#[derive(Default)]
struct State {
    /// Asked, not started yet.
    pending: Option<Job>,
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

struct Shared {
    models: Arc<dyn Models>,
    events: PluginEvents,
    idle: Duration,
    state: Mutex<State>,
    wake: Condvar,
}

/// Correction runs of one plugin instance (rules: module doc).
pub struct Corrector {
    shared: Arc<Shared>,
}

impl Corrector {
    /// Nothing started | loaded (dormant); `idle` = warm session lifetime without a run ([`IDLE`]).
    #[must_use]
    pub fn new(models: Arc<dyn Models>, events: PluginEvents, idle: Duration) -> Self {
        Self {
            shared: Arc::new(Shared {
                models,
                events,
                idle,
                state: Mutex::new(State::default()),
                wake: Condvar::new(),
            }),
        }
    }

    /// Answer of `correction.status` (state alone).
    #[must_use]
    pub fn status(&self) -> CorrectionStatus {
        CorrectionStatus {
            available: self.shared.models.available(),
        }
    }

    /// Starts run `run` over `pieces` (news as events); cancels the run in progress | not started yet.
    ///
    /// # Errors
    /// Plugin stopping; no worker thread.
    pub fn start(&self, run: String, pieces: Vec<Piece>) -> Result<(), IpcError> {
        let shared = &self.shared;
        let mut state = lock(&shared.state);
        if state.closed {
            return Err(IpcError::new(ErrorKind::Unavailable).subject("correction"));
        }
        if let Some((_, token)) = &state.running {
            token.cancel();
        }
        let job = Job {
            run,
            pieces,
            cancel: CancelToken::new(),
        };
        if let Some(replaced) = state.pending.replace(job) {
            shared.emit(&replaced.run, RunPhase::Cancelled);
        }
        if state.worker.is_none() || state.worker_done {
            let previous = state.worker.take();
            let s = Arc::clone(shared);
            let handle = std::thread::Builder::new()
                .name("correction".into())
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
        if state.pending.as_ref().is_some_and(|job| job.run == run) {
            if let Some(job) = state.pending.take() {
                shared.emit(&job.run, RunPhase::Cancelled);
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
            if let Some(job) = state.pending.take() {
                shared.emit(&job.run, RunPhase::Cancelled);
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

impl Drop for Corrector {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Shared {
    fn emit(&self, run: &str, phase: RunPhase) {
        let news = RunNews {
            run: run.to_owned(),
            phase,
        };
        if let Ok(payload) = serde_json::to_value(news) {
            self.events.emit("run", payload);
        }
    }

    /// Worker loop: runs jobs; idle past `idle` | closed -> warm session unloaded, thread ends.
    fn work(&self) {
        loop {
            let (job, warm) = {
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
                let Some(job) = state.pending.take() else {
                    continue;
                };
                state.running = Some((job.run.clone(), job.cancel.clone()));
                (job, state.warm.take())
            };
            let (phase, session) = self.run(&job, warm);
            let mut state = lock(&self.state);
            // closed meanwhile: unloaded here (after the unlock), never kept
            let closing = if state.closed {
                session
            } else {
                state.warm = session;
                None
            };
            self.emit(&job.run, phase);
            state.running = None;
            drop(state);
            drop(closing);
        }
    }

    /// One run; answers the last phase + the session to keep warm.
    fn run(
        &self,
        job: &Job,
        warm: Option<Box<dyn Session>>,
    ) -> (RunPhase, Option<Box<dyn Session>>) {
        let cancel = &job.cancel;
        if job.pieces.is_empty() {
            return (RunPhase::Done, warm);
        }
        let session = match warm.filter(|s| s.serves()) {
            Some(session) => session,
            None => match self.open(&job.run, cancel) {
                Ok(session) => session,
                Err(_) if cancel.is_cancelled() => return (RunPhase::Cancelled, None),
                Err(error) => return (RunPhase::Failed { error }, None),
            },
        };
        let phase = match self.pieces(job, session.as_ref()) {
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
        self.emit(run, RunPhase::Loading { progress: 0.0 });
        let sent = Cell::new(0.0f32);
        let progress = |value: f32| {
            if value.is_finite() && value - sent.get() >= PROGRESS_STEP {
                sent.set(value);
                self.emit(run, RunPhase::Loading { progress: value });
            }
        };
        self.models.open(&progress, cancel)
    }

    /// Every piece in order (stops early on cancel).
    fn pieces(&self, job: &Job, session: &dyn Session) -> Result<(), EngineError> {
        let preamble = tokens(session.count_tokens(&prompt::preamble())?);
        for piece in &job.pieces {
            if job.cancel.is_cancelled() {
                return Ok(());
            }
            let Some(result) = correct(session, preamble, &piece.text, &job.cancel)? else {
                return Ok(());
            };
            self.emit(
                &job.run,
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

/// One piece; `None`: cancelled during the answer.
fn correct(
    session: &dyn Session,
    preamble: u32,
    text: &str,
    cancel: &CancelToken,
) -> Result<Option<PieceResult>, EngineError> {
    let discarded = |reason| Ok(Some(PieceResult::Discarded { reason }));
    let piece = tokens(session.count_tokens(text)?);
    let max_tokens = prompt::max_tokens(piece);
    let needed = u64::from(preamble) + u64::from(piece) + u64::from(max_tokens) + u64::from(MARKUP);
    if needed > u64::from(CONTEXT) {
        return discarded(Discard::TooLong);
    }
    let request = ChatRequest {
        messages: prompt::messages(text),
        sampling: prompt::SAMPLING,
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
