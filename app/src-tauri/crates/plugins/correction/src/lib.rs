//! Plugin `correction`: fixes spelling, grammar and punctuation of Markdown text with the local model assigned to job
//! "correction" (plugin `local-ai`); the window reviews every change before it lands (TS side: segment, markers, diff,
//! guards, review bar).
//!
//! Rust side is thin: prompt data ([`prompt`]), model call + piece results stream ([`run`]). Depends on `local-ai`
//! (model service `ModelHost`, looked up per call: never loads | downloads by itself).
//!
//! On by default but dormant: start registers the RPC only (no thread, no load); the window hides the command until
//! `status` says a model is assigned.
//!
//! RPC (app scope): `status {}` -> [`CorrectionStatus`]; `run { run, pieces: [{ id, text }] }` -> `null` (news as
//! event `correction:run` = [`RunNews`]); `cancel { run }` -> `null`.

pub mod prompt;
pub mod run;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running, Services,
};
use permark_core::IpcError;
use permark_plugin_local_ai::engine::{CancelToken, ChatRequest, EngineError, GenEvent};
use permark_plugin_local_ai::{HostError, ModelHost, ModelSession, Task};
use serde::Deserialize;

pub use run::{
    CorrectionStatus, Corrector, Discard, Models, Piece, PieceResult, RunNews, RunPhase, Session,
};

const ID: PluginId = PluginId::of("correction");

/// The plugin.
#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(Correction {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::App,
            requires: Requirement::NONE,
            depends_on: Cow::Owned(vec![PluginId::of("local-ai")]),
            default_enabled: true,
        },
    })
}

struct Correction {
    manifest: Manifest,
}

struct Started {
    _rpc: Vec<RpcRegistration>,
    corrector: Arc<Corrector>,
}

impl Running for Started {}

impl Drop for Started {
    fn drop(&mut self) {
        // off: run cancelled, warm session unloaded, worker joined (later calls refused); RPC dropped next
        self.corrector.shutdown();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArgs {
    run: String,
    pieces: Vec<Piece>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelArgs {
    run: String,
}

/// RPC of one instance over `corrector`.
fn register(cx: &PluginCx, corrector: &Arc<Corrector>) -> Vec<RpcRegistration> {
    let mut rpc = Vec::new();
    let c = Arc::clone(corrector);
    rpc.extend(cx.rpc(
        "status",
        typed(move |_: NoArgs, _| Ok::<_, IpcError>(c.status())),
    ));
    let c = Arc::clone(corrector);
    rpc.extend(cx.rpc(
        "run",
        typed(move |args: RunArgs, _| c.start(args.run, args.pieces)),
    ));
    let c = Arc::clone(corrector);
    rpc.extend(cx.rpc(
        "cancel",
        typed(move |args: CancelArgs, _| {
            c.cancel(&args.run);
            Ok::<_, IpcError>(())
        }),
    ));
    rpc
}

impl Plugin for Correction {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let models = Arc::new(Hosted {
            services: cx.app().map(|app| Arc::clone(&app.services)),
        });
        let corrector = Arc::new(Corrector::new(models, cx.events().clone(), run::IDLE));
        Ok(Box::new(Started {
            _rpc: register(cx, &corrector),
            corrector,
        }))
    }
}

/// [`Models`] over `local-ai`'s `ModelHost` (app service; absent -> unavailable).
struct Hosted {
    services: Option<Arc<Services>>,
}

impl Hosted {
    fn host(&self) -> Option<Arc<ModelHost>> {
        self.services.as_ref()?.get::<ModelHost>()
    }
}

impl Models for Hosted {
    fn available(&self) -> bool {
        self.host().is_some_and(|h| h.is_assigned(Task::Correction))
    }

    fn open(
        &self,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<Box<dyn Session>, IpcError> {
        let host = self.host().ok_or(HostError::NoModel(Task::Correction))?;
        let session = host.open(Task::Correction, run::CONTEXT, progress, cancel)?;
        Ok(Box::new(HostedSession { host, session }))
    }
}

struct HostedSession {
    host: Arc<ModelHost>,
    session: ModelSession,
}

impl Session for HostedSession {
    fn serves(&self) -> bool {
        self.host.serves(&self.session, Task::Correction)
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

#[cfg(test)]
mod tests;
