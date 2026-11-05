//! Model service for PerMark's own features: app service `services.get::<ModelHost>()` while the plugin is on
//! (lease dropped first at plugin off). Features NEVER call [`Runtime::load`] themselves.
//!
//! - [`ModelHost::open`]: model assigned to a [`Task`] ([`Installs::model_for`]) + runs on the pinned worker (else
//!   `engineTooOld` | `unsupported`, nothing started) + engine files installed NOW (never downloaded here: missing
//!   -> [`HostError::NoEngine`], the Local AI page's Test fetches them) -> load through
//!   [`Runtime::load`] (memory guard inside; `Device::Auto`; context capped at the model's maximum).
//! - [`ModelSession`]: RAII, Drop unloads (worker exits). Plugin off | model removed | store change unloads it under
//!   the holder too (its load token cancelled, calls then fail `notLoaded`).
//! - Dormant: building one creates | spawns nothing; [`ModelHost::is_assigned`] + [`ModelHost::serves`] read the
//!   state alone.
//! - Errors toward IPC: [`HostError`] -> [`IpcError`]; engine failures through [`engine_ipc`] (stable codes; engine
//!   text | worker log NEVER sent, may hold paths).
//! - Tests: `src/tests/host.rs`.

use std::sync::Arc;

use permark_core::{ErrorKind, IpcError};

use crate::catalog::{Catalog, EngineNeed, Model};
use crate::engine::files::WorkerFiles;
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
    /// Engine files not installed (no worker for this computer | not fetched yet).
    #[error("the model engine is not installed")]
    NoEngine,
    #[error(transparent)]
    Engine(#[from] EngineError),
}

impl From<HostError> for IpcError {
    /// `noModel`: subject `model`; engine: subject `engine`, `detail` = stable code (see [`engine_ipc`]).
    fn from(error: HostError) -> Self {
        match error {
            HostError::NoModel(_) => IpcError::new(ErrorKind::Unavailable)
                .subject("model")
                .detail("noModel"),
            HostError::NoEngine => engine_code(ErrorKind::Unavailable, "notAvailable"),
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
}

impl ModelHost {
    /// Host over the plugin instance's parts (nothing created | spawned).
    #[must_use]
    pub fn new(
        installs: Arc<Installs>,
        catalog: Arc<Catalog>,
        runtime: Arc<Runtime>,
        files: Arc<WorkerFiles>,
    ) -> Self {
        Self {
            installs,
            catalog,
            runtime,
            files,
        }
    }

    /// `session` still carries the model assigned to `task` (lineage; state alone): a feature keeping a session warm
    /// drops it when the person assigns another model (a version update | removal unloads it already).
    #[must_use]
    pub fn serves(&self, session: &ModelSession, task: Task) -> bool {
        self.installs.assigned_lineage(task).as_deref() == Some(session.lineage.as_str())
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
    /// `progress` gets 0.0..=1.0.
    ///
    /// # Errors
    /// [`HostError::NoModel`] | [`HostError::NoEngine`] (nothing fetched) | [`HostError::Engine`]
    /// (`EngineTooOld` | `Unsupported`: the pinned worker cannot run it; `OutOfMemory` from the guard, `Cancelled`,
    /// load errors).
    pub fn open(
        &self,
        task: Task,
        context_len: u32,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<ModelSession, HostError> {
        let ModelFor::Assigned(installed) = self.installs.model_for(task) else {
            return Err(HostError::NoModel(task));
        };
        let model = self
            .catalog
            .model(&installed.id)
            .ok_or(HostError::NoModel(task))?;
        if let Some(refusal) = self.refusal(model) {
            return Err(refusal.into());
        }
        if !self.files.is_installed() {
            return Err(HostError::NoEngine);
        }
        Ok(self.load(model, &installed, context_len, progress, cancel)?)
    }

    /// Loads `installed` (version of catalog `model`); engine files expected installed (Test fetches them first).
    pub(crate) fn load(
        &self,
        model: &Model,
        installed: &AssignedModel,
        context_len: u32,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<ModelSession, EngineError> {
        let files = engine::model_files(installed, &self.catalog);
        let opts = LoadOpts {
            context_len: model.context_len.min(context_len),
            device: Device::Auto,
            threads: None,
        };
        let memory_bytes = self
            .runtime
            .engine()
            .estimate_memory(&files, &opts)
            .ok()
            .map(|m| m.ram.saturating_add(m.vram));
        let id = self
            .runtime
            .load(
                &installed.lineage,
                &model.needs,
                files,
                opts,
                progress,
                cancel,
            )
            .map_err(|e| match e {
                // verified files gone at spawn time = taken by security software (D-14)
                EngineError::Io(_) if !self.files.is_installed() => EngineError::Blocked,
                e => e,
            })?;
        let on_gpu = self.runtime.engine().on_gpu(id);
        Ok(ModelSession {
            runtime: Arc::clone(&self.runtime),
            id,
            lineage: installed.lineage.clone(),
            on_gpu,
            memory_bytes,
        })
    }
}
