//! Plugin `correction`: fixes spelling, grammar and punctuation of Markdown text with the local model assigned to job
//! "correction" (plugin `local-ai`); the window reviews every change before it lands (TS side: segment, markers, diff,
//! guards, review bar).
//!
//! Rust side is thin: prompt data + the correction job ([`prompt`]) run by `local-ai`'s piece runner
//! ([`PieceRunner`]: model call + piece results stream, warm session, cancel). Depends on `local-ai` (model service
//! `ModelHost`, looked up per call: loads only through it; a run fetches a missing engine through it, phase
//! `engine`).
//!
//! On by default but dormant: start registers the RPC only (no thread, no load); the window hides the command until
//! `status` says a model is assigned.
//!
//! RPC (app scope): `status {}` -> [`CorrectionStatus`]; `run { run, pieces: [{ id, text }] }` -> `null` (news as
//! event `correction:run` = the runner's [`RunNews`](permark_plugin_local_ai::pieces::RunNews)); `cancel { run }` ->
//! `null`.

pub mod prompt;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running,
};
use permark_core::IpcError;
use permark_plugin_local_ai::pieces::{Piece, PieceRunner};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use prompt::CorrectionJob;

const ID: PluginId = PluginId::of("correction");

/// Correction runs of one plugin instance.
type Corrector = PieceRunner<CorrectionJob>;

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

/// Answer of `correction.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CorrectionStatus {
    /// A model is assigned to job "correction" (state alone; it may since be gone -> the run fails `noModel`).
    pub available: bool,
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
        typed(move |_: NoArgs, _| {
            Ok::<_, IpcError>(CorrectionStatus {
                available: c.available(),
            })
        }),
    ));
    let c = Arc::clone(corrector);
    rpc.extend(cx.rpc(
        "run",
        typed(move |args: RunArgs, _| c.start(args.run, (), args.pieces)),
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
        let corrector = Arc::new(Corrector::hosted(cx));
        Ok(Box::new(Started {
            _rpc: register(cx, &corrector),
            corrector,
        }))
    }
}

#[cfg(test)]
mod tests;
