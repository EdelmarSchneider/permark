//! Model service for PerMark's own features: app service `services.get::<ModelHost>()` while the plugin is on
//! (lease dropped first at plugin off). Features NEVER call [`Runtime::load`] themselves.
//!
//! - [`ModelHost::open`]: model assigned to a [`Task`] ([`Installs::model_for`]) + runs on the pinned worker (else
//!   `engineTooOld` | `unsupported`, nothing started) -> engine files of the current pin installed, else fetched
//!   first through [`WorkerFiles`] ([`HostProgress::Engine`]; cancel stops the download, part kept; no pin ->
//!   `notAvailable`) -> [`HostProgress::Loading`] from 0 -> load through [`Runtime::load`] (memory guard inside;
//!   device = the person's choice, [`Installs::device`]: graphics card when there is one | processor only, read once
//!   the runtime knows the load -> a choice changed meanwhile cancels the load | is the one read; context capped at
//!   the model's maximum). Tester ("Test model") takes the same path (`ModelHost::open_model`): ONE engine fetch.
//! - [`ModelSession`]: RAII, Drop unloads (worker exits). Plugin off | model removed | store change | another device
//!   chosen unloads it under the holder too (its load token cancelled, calls then fail `notLoaded`).
//! - Dormant: building one creates | spawns nothing; [`ModelHost::is_assigned`] + [`ModelHost::serves`] read the
//!   state alone.
//! - Errors toward IPC: [`HostError`] -> [`IpcError`]; engine failures through [`engine_ipc`] (stable codes; engine
//!   text | worker log NEVER sent, may hold paths).
//! - Tests: `src/tests/host.rs`.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use permark_core::downloads::{DownloadError, Listener, Progress, Update};
use permark_core::{ErrorKind, IpcError};

use crate::catalog::{Catalog, EngineNeed, Model};
use crate::engine::files::{FilesError, WorkerFiles};
use crate::engine::{self, CancelToken, Device, Engine, EngineError, LoadOpts, SessionId};
use crate::installs::{AssignedModel, Installs, ModelFor};
use crate::runtime::Runtime;
use crate::tasks::Task;

/// Why [`ModelHost::open`] gave no session.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// No installed model assigned to the task.
    #[error("no model assigned to {0:?}")]
    NoModel(Task),
    /// Engine files cannot be fetched (no worker for this computer, bad pin, no download service).
    #[error(transparent)]
    Files(#[from] FilesError),
    /// Engine download failed | was cancelled.
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Engine(#[from] EngineError),
}

/// What an open reports while it works.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HostProgress {
    /// Engine files downloading (first open only | after a pin change), bytes.
    Engine { received: u64, total: u64 },
    /// Model loading, 0..=1 (starts with 0 once the engine is there).
    Loading(f32),
}

/// Cancel check interval while the engine downloads.
const POLL: Duration = Duration::from_millis(50);

/// Worker-unit download updates -> every open waiting on them (the [`WorkerFiles`] listener; nobody waiting ->
/// dropped).
#[derive(Default)]
pub struct EngineNews(Mutex<Vec<Sender<Update>>>);

impl EngineNews {
    /// Listener for [`WorkerFiles::new`].
    #[must_use]
    pub fn listener(self: &Arc<Self>) -> Listener {
        let news = Arc::clone(self);
        // a waiter gone (its open ended) -> its sender dropped here
        Arc::new(move |_, update| news.waiters().retain(|tx| tx.send(update.clone()).is_ok()))
    }

    fn waiters(&self) -> MutexGuard<'_, Vec<Sender<Update>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn open(&self) -> Receiver<Update> {
        let (tx, rx) = mpsc::channel();
        self.waiters().push(tx);
        rx
    }
}

impl From<HostError> for IpcError {
    /// `noModel`: subject `model`; engine | files: subject `engine`, `detail` = stable code (`notAvailable`, `badPin`
    /// + [`engine_ipc`]; download cancelled = `cancelled`); other download errors: own codes, subject `engine`.
    fn from(error: HostError) -> Self {
        match error {
            HostError::NoModel(_) => IpcError::new(ErrorKind::Unavailable)
                .subject("model")
                .detail("noModel"),
            HostError::Files(FilesError::NotAvailable) => {
                engine_code(ErrorKind::Unavailable, "notAvailable")
            }
            HostError::Files(FilesError::NoDownloads) => {
                IpcError::new(ErrorKind::Unavailable).subject("downloads")
            }
            HostError::Files(FilesError::BadPin) => engine_code(ErrorKind::Internal, "badPin"),
            HostError::Download(DownloadError::Cancelled) => engine_ipc(&EngineError::Cancelled),
            HostError::Download(e) => IpcError::from(e).subject("engine"),
            HostError::Engine(e) => engine_ipc(&e),
        }
    }
}

fn engine_code(kind: ErrorKind, detail: &str) -> IpcError {
    IpcError::new(kind).subject("engine").detail(detail)
}

/// Engine failure toward IPC: subject `engine`, `detail` = `outOfMemory` | `unsupported` | `corrupt` | `cancelled`
/// | `crashed` | `engineTooOld` | `notLoaded` | `blocked` | `io`; engine text NEVER sent.
#[must_use]
pub fn engine_ipc(error: &EngineError) -> IpcError {
    match error {
        EngineError::OutOfMemory => engine_code(ErrorKind::Unavailable, "outOfMemory"),
        EngineError::Unsupported { .. } => engine_code(ErrorKind::Unsupported, "unsupported"),
        EngineError::Corrupt => engine_code(ErrorKind::InvalidData, "corrupt"),
        EngineError::Cancelled => engine_code(ErrorKind::Superseded, "cancelled"),
        EngineError::EngineCrashed => engine_code(ErrorKind::Unavailable, "crashed"),
        EngineError::EngineTooOld { .. } => engine_code(ErrorKind::Unsupported, "engineTooOld"),
        EngineError::NotLoaded => engine_code(ErrorKind::Unavailable, "notLoaded"),
        EngineError::Blocked => engine_code(ErrorKind::Denied, "blocked"),
        EngineError::Io(_) => engine_code(ErrorKind::Io, "io"),
    }
}

/// Loaded model; Drop unloads it.
pub struct ModelSession {
    runtime: Arc<Runtime>,
    id: SessionId,
    /// Lineage of the loaded model ([`ModelHost::serves`]).
    lineage: String,
    /// Device it was loaded for ([`ModelHost::serves`]).
    device: Device,
    on_gpu: bool,
    memory_bytes: Option<u64>,
}

impl ModelSession {
    /// Session id for the [`ModelSession::engine`] calls.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// The engine (generate, count tokens, ...) on [`ModelSession::id`].
    #[must_use]
    pub fn engine(&self) -> &dyn Engine {
        self.runtime.engine()
    }

    /// Runs (partly) on the graphics card (layers placed there at load).
    #[must_use]
    pub fn on_gpu(&self) -> bool {
        self.on_gpu
    }

    /// Engine estimate of the loaded model, bytes (lower bound: file bytes); `None`: unknown.
    #[must_use]
    pub fn memory_bytes(&self) -> Option<u64> {
        self.memory_bytes
    }
}

impl std::fmt::Debug for ModelSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelSession")
            .field("id", &self.id)
            .field("on_gpu", &self.on_gpu)
            .finish_non_exhaustive()
    }
}

impl Drop for ModelSession {
    fn drop(&mut self) {
        self.runtime.unload(self.id);
    }
}

/// Model service of one plugin instance (rules: module doc).
pub struct ModelHost {
    pub(crate) installs: Arc<Installs>,
    pub(crate) catalog: Arc<Catalog>,
    pub(crate) runtime: Arc<Runtime>,
    pub(crate) files: Arc<WorkerFiles>,
    /// Same as the one given to `files` ([`EngineNews::listener`]).
    news: Arc<EngineNews>,
}

impl ModelHost {
    /// Host over the plugin instance's parts (nothing created | spawned).
    #[must_use]
    pub fn new(
        installs: Arc<Installs>,
        catalog: Arc<Catalog>,
        runtime: Arc<Runtime>,
        files: Arc<WorkerFiles>,
        news: Arc<EngineNews>,
    ) -> Self {
        Self {
            installs,
            catalog,
            runtime,
            files,
            news,
        }
    }

    /// `session` still carries the model assigned to `task` (lineage), loaded for the device the person chooses now
    /// (state alone): a feature keeping a session warm drops it when the person assigns another model | lets models
    /// run elsewhere (a version update | removal | device change unloads it too).
    #[must_use]
    pub fn serves(&self, session: &ModelSession, task: Task) -> bool {
        self.installs.assigned_lineage(task).as_deref() == Some(session.lineage.as_str())
            && self.installs.device() == session.device
    }

    /// A model is assigned to `task` (state alone: no engine started, no disk read; the model may since be gone ->
    /// [`ModelHost::open`] answers [`HostError::NoModel`]).
    #[must_use]
    pub fn is_assigned(&self, task: Task) -> bool {
        self.installs.is_assigned(task)
    }

    /// The pinned worker cannot run `model`: the engine error a load would end with (no pin: `None`, the engine path
    /// answers `notAvailable`).
    pub(crate) fn refusal(&self, model: &Model) -> Option<EngineError> {
        let pin = self.files.pin()?;
        pin.lacks(&model.min_engine).map(|need| match need {
            EngineNeed::Newer => EngineError::EngineTooOld {
                needed: model.min_engine.version.clone().unwrap_or_default(),
            },
            EngineNeed::Missing => EngineError::Unsupported {
                what: "engine".to_owned(),
            },
        })
    }

    /// Loads the model assigned to `task` with a context of `context_len` tokens (model maximum when smaller);
    /// engine fetched first when missing (rules: module doc).
    ///
    /// # Errors
    /// [`HostError::NoModel`] | [`HostError::Files`] (`NotAvailable`: no worker for this computer) |
    /// [`HostError::Download`] (`Cancelled` on cancel) | [`HostError::Engine`] (`EngineTooOld` | `Unsupported`: the
    /// pinned worker cannot run it; `OutOfMemory` from the guard, `Cancelled`, load errors).
    pub fn open(
        &self,
        task: Task,
        context_len: u32,
        progress: &dyn Fn(HostProgress),
        cancel: &CancelToken,
    ) -> Result<ModelSession, HostError> {
        let ModelFor::Assigned(installed) = self.installs.model_for(task) else {
            return Err(HostError::NoModel(task));
        };
        let model = self
            .catalog
            .model(&installed.id)
            .ok_or(HostError::NoModel(task))?;
        self.open_model(model, &installed, context_len, progress, cancel)
    }

    /// [`ModelHost::open`] of `installed` (version of catalog `model`), whatever the assignments.
    pub(crate) fn open_model(
        &self,
        model: &Model,
        installed: &AssignedModel,
        context_len: u32,
        progress: &dyn Fn(HostProgress),
        cancel: &CancelToken,
    ) -> Result<ModelSession, HostError> {
        if let Some(refusal) = self.refusal(model) {
            return Err(refusal.into());
        }
        self.ensure_engine(progress, cancel)?;
        progress(HostProgress::Loading(0.0));
        let loading = |value| progress(HostProgress::Loading(value));
        Ok(self.load(model, installed, context_len, &loading, cancel)?)
    }

    /// Worker files of the current pin installed now, else queued + followed to the end (`Engine` progress).
    fn ensure_engine(
        &self,
        progress: &dyn Fn(HostProgress),
        cancel: &CancelToken,
    ) -> Result<(), HostError> {
        if self.files.is_installed() {
            return Ok(());
        }
        // listening before the enqueue: an install finished meanwhile still answers `Done`
        let updates = self.news.open();
        self.files.enqueue()?;
        loop {
            if cancel.is_cancelled() {
                self.files.cancel();
                return Err(DownloadError::Cancelled.into());
            }
            match updates.recv_timeout(POLL) {
                Ok(Update::Progress(Progress { done, total, .. })) => {
                    progress(HostProgress::Engine {
                        received: done,
                        total,
                    });
                }
                Ok(Update::Done) => return Ok(()),
                Ok(Update::Failed(error)) => return Err(error.into()),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err(DownloadError::Cancelled.into()),
            }
        }
    }

    /// Loads `installed` (version of catalog `model`); engine files installed ([`ModelHost::open_model`]).
    fn load(
        &self,
        model: &Model,
        installed: &AssignedModel,
        context_len: u32,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<ModelSession, EngineError> {
        let files = engine::model_files(installed, &self.catalog);
        // the person's choice, read by the runtime once it knows this load: `Installs::set_cpu_only` writes the
        // choice, then releases -> a change before the read is the one read, a later one cancels this load |
        // unloads its session. Read here instead, a change before the runtime knew the load would cancel nothing
        // and the model would load where the person no longer lets models run
        let opts = || LoadOpts {
            context_len: model.context_len.min(context_len),
            device: self.installs.device(),
            threads: None,
        };
        let (id, opts) = self
            .runtime
            .load(
                &installed.lineage,
                &model.needs,
                files.clone(),
                opts,
                progress,
                cancel,
            )
            .map_err(|e| match e {
                // verified files gone at spawn time = taken by security software (D-14)
                EngineError::Io(_) if !self.files.is_installed() => EngineError::Blocked,
                e => e,
            })?;
        let memory_bytes = self
            .runtime
            .engine()
            .estimate_memory(&files, &opts)
            .ok()
            .map(|m| m.ram.saturating_add(m.vram));
        let on_gpu = self.runtime.engine().on_gpu(id);
        Ok(ModelSession {
            runtime: Arc::clone(&self.runtime),
            id,
            lineage: installed.lineage.clone(),
            device: opts.device,
            on_gpu,
            memory_bytes,
        })
    }
}
