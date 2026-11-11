//! Test kit of the piece runner: one scripted model source ([`Fake`] over the in-process [`FakeEngine`], which echoes
//! the last user turn word by word) + the collector of a plugin's run events ([`Events`]). Shared by the runner's own
//! tests and the tests of every plugin that runs a job on it (one definition, no copy per plugin).
//!
//! Compiled for tests only: this crate's own, another crate's through feature `testkit` enabled from its
//! `[dev-dependencies]` (never part of an app build).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use permark_core::conditions::{Context, Distribution};
use permark_core::events::{CoreEvent, EventSink};
use permark_core::plugins::{CxWorkspace, PluginCx, PluginId};
use permark_core::IpcError;
use serde_json::Value;

use super::{lock, Models, Piece, PieceJob, Session};
use crate::engine::{
    CancelToken, ChatRequest, Device, Engine, EngineError, FakeEngine, FileRole, Finish, GenEvent,
    LoadOpts, ModelFiles, SessionId, Usage,
};
use crate::host::{engine_ipc, HostError, HostProgress};
use crate::tasks::Task;

/// Piece word that makes the fake answer hit its cap.
pub const RAMBLE: &str = "ramble";

/// Polls `f` until it answers.
///
/// # Panics
/// No answer within 10 s.
pub fn wait<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(value) = f() {
            return value;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// One piece per text, ids from 1.
#[must_use]
pub fn pieces(texts: &[&str]) -> Vec<Piece> {
    (1u32..)
        .zip(texts)
        .map(|(id, text)| Piece {
            id,
            text: (*text).to_owned(),
        })
        .collect()
}

/// Events of one plugin as emitted, in order; reads its runs (event `<plugin>:run`).
pub struct Events {
    plugin: PluginId,
    seen: Mutex<Vec<(String, Value)>>,
}

impl EventSink for Events {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, payload } = event {
            lock(&self.seen).push((name, payload));
        }
    }
}

impl Events {
    /// Collector of the events of `plugin`.
    #[must_use]
    pub fn new(plugin: PluginId) -> Arc<Self> {
        Arc::new(Self {
            plugin,
            seen: Mutex::default(),
        })
    }

    /// App-scope context of the plugin, its events landing here (no store, no app services, nothing on disk).
    #[must_use]
    pub fn cx(self: &Arc<Self>) -> PluginCx {
        PluginCx::new(
            self.plugin.clone(),
            CxWorkspace::App,
            std::env::temp_dir().join("permark-pieces-unused"),
            None,
            Context::welcome(Distribution::Direct),
            Arc::clone(self) as Arc<dyn EventSink>,
        )
    }

    /// Phases of run `run` so far.
    #[must_use]
    pub fn phases(&self, run: &str) -> Vec<Value> {
        let event = format!("{}:run", self.plugin.as_str());
        lock(&self.seen)
            .iter()
            .filter(|(name, p)| *name == event && p["run"] == run)
            .map(|(_, p)| p["phase"].clone())
            .collect()
    }

    /// Phases of `run` once its last one (`done` | `cancelled` | `failed`) went out ([`wait`]s for it).
    pub fn ended(&self, run: &str) -> Vec<Value> {
        wait(|| {
            let phases = self.phases(run);
            phases
                .last()
                .and_then(|p| p["kind"].as_str())
                .is_some_and(|kind| ["done", "cancelled", "failed"].contains(&kind))
                .then_some(phases)
        })
    }
}

/// Model source over one [`FakeEngine`]; counts loads (+ the most loaded at once), records requests. One source may
/// serve several runners (as the model service does for the features of one app).
pub struct Fake {
    /// Engine behind every session (its `loaded` = sessions open now).
    pub engine: Arc<FakeEngine>,
    /// A model is assigned to the job (false: unavailable, a run fails `noModel`).
    pub assigned: AtomicBool,
    /// Sessions keep serving (false = model reassigned).
    pub serving: Arc<AtomicBool>,
    /// Requests the sessions got, in order.
    pub requests: Arc<Mutex<Vec<ChatRequest>>>,
    /// Engine files missing: bytes the next open fetches first (as `ModelHost`), then installed.
    pub engine_missing: Mutex<Option<u64>>,
    /// The engine fetch never ends before a cancel.
    pub engine_held: AtomicBool,
    opens: AtomicUsize,
    /// Most sessions loaded at once, read after each load.
    peak: AtomicUsize,
    /// Job of the runs (named by `noModel`).
    task: Task,
    /// Context window asked of the engine, tokens.
    context: u32,
}

impl Fake {
    /// Source for job `J` (its task + context window) over `engine`: model assigned, engine installed.
    #[must_use]
    pub fn new<J: PieceJob>(engine: FakeEngine) -> Arc<Self> {
        Arc::new(Self {
            engine: Arc::new(engine),
            assigned: AtomicBool::new(true),
            serving: Arc::new(AtomicBool::new(true)),
            requests: Arc::default(),
            engine_missing: Mutex::new(None),
            engine_held: AtomicBool::new(false),
            opens: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            task: J::TASK,
            context: J::CONTEXT,
        })
    }

    /// Models loaded so far.
    #[must_use]
    pub fn opens(&self) -> usize {
        self.opens.load(Ordering::Relaxed)
    }

    /// Most models loaded at the same time so far (1: every load found the earlier sessions unloaded).
    #[must_use]
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::Relaxed)
    }
}

impl Models for Fake {
    fn available(&self) -> bool {
        self.assigned.load(Ordering::Relaxed)
    }

    fn open(
        &self,
        progress: &dyn Fn(HostProgress),
        cancel: &CancelToken,
    ) -> Result<Box<dyn Session>, IpcError> {
        if !self.available() {
            return Err(HostError::NoModel(self.task).into());
        }
        let missing = *lock(&self.engine_missing);
        if let Some(total) = missing {
            progress(HostProgress::Engine { received: 0, total });
            if self.engine_held.load(Ordering::Relaxed) {
                wait(|| cancel.is_cancelled().then_some(()));
                return Err(engine_ipc(&EngineError::Cancelled));
            }
            progress(HostProgress::Engine {
                received: total,
                total,
            });
            *lock(&self.engine_missing) = None;
        }
        progress(HostProgress::Loading(0.0));
        self.opens.fetch_add(1, Ordering::Relaxed);
        self.serving.store(true, Ordering::Relaxed);
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
            context_len: self.context,
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
        self.peak.fetch_max(self.engine.loaded(), Ordering::Relaxed);
        Ok(Box::new(FakeSession {
            engine: Arc::clone(&self.engine),
            id,
            serving: Arc::clone(&self.serving),
            requests: Arc::clone(&self.requests),
        }))
    }
}

/// Session of a [`Fake`] (Drop unloads it from the engine).
struct FakeSession {
    engine: Arc<FakeEngine>,
    id: SessionId,
    serving: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl Drop for FakeSession {
    fn drop(&mut self) {
        self.engine.unload(self.id);
    }
}

impl Session for FakeSession {
    fn serves(&self) -> bool {
        self.serving.load(Ordering::Relaxed)
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
        lock(&self.requests).push(request.clone());
        let last = request.messages.last();
        if last.is_some_and(|m| m.content.contains(RAMBLE)) {
            sink(GenEvent::Delta("and so on".into()));
            sink(GenEvent::Done {
                usage: Usage::default(),
                finish: Finish::Length,
            });
            return Ok(());
        }
        self.engine.generate(self.id, request, sink, cancel)
    }
}
