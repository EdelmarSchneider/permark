//! Plugin `local-ai`: small models that run on this computer, for PerMark's own features only (never offered to
//! outside tools).
//!
//! On by default but dormant: starting registers the RPC and nothing else (no thread, no network, no file | folder)
//! until the person acts. The curated catalog ([`catalog`]) is bundled; each PerMark job ([`tasks::Task`]) needs a
//! capability set a model must offer.
//!
//! RPC for the windows: `catalog`, `tasks`, `state`, `get`, `update`, `acceptLicense`, `cancel`, `deletePart`,
//! `remove`, `assign`, `setToken`, `clearToken` (install calls answer the new [`LocalAiState`]). Events (app scope):
//! `state` (whole state after every change), `progress` (one model, throttled by the engine).

pub mod catalog;
pub mod installs;
pub mod state;
pub mod tasks;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::downloads::DownloadKit;
use permark_core::plugins::{
    typed, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running,
};
use permark_core::IpcError;
use serde::Deserialize;

pub use catalog::{Capability, Catalog, CatalogError, CatalogInfo, ModelInfo};
pub use installs::{
    AssignedModel, GetBlock, InstallError, InstallState, Installs, LocalAiState, ModelFor,
    ModelProgress, ModelStatus, TaskAssignment,
};
pub use tasks::{Task, TaskInfo};

const ID: PluginId = PluginId::of("local-ai");

/// The plugin with the bundled catalog.
#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    plugin_of(catalog::bundled().cloned().map_err(Clone::clone))
}

/// The plugin over another catalog (tests).
#[must_use]
pub fn plugin_with(catalog: Catalog) -> Arc<dyn Plugin> {
    plugin_of(Ok(catalog))
}

fn plugin_of(catalog: Result<Catalog, CatalogError>) -> Arc<dyn Plugin> {
    Arc::new(LocalAi {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::App,
            requires: Requirement::NONE,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
        catalog: catalog.map(Arc::new),
    })
}

struct LocalAi {
    manifest: Manifest,
    /// A bad bundled file (a build bug; the tests read the same file): the plugin does not start.
    catalog: Result<Arc<Catalog>, CatalogError>,
}

struct Started {
    _rpc: Vec<RpcRegistration>,
    installs: Arc<Installs>,
}

impl Running for Started {}

impl Drop for Started {
    fn drop(&mut self) {
        // off: downloads stop (parts kept) and the worker is joined
        self.installs.shutdown();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelArgs {
    model: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignArgs {
    task: Task,
    model: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenArgs {
    token: String,
}

/// Install calls (each answers the new state).
fn register(cx: &PluginCx, installs: &Arc<Installs>) -> Vec<RpcRegistration> {
    type ModelCall = fn(&Installs, &str) -> Result<LocalAiState, InstallError>;
    let mut rpc = Vec::new();
    let i = Arc::clone(installs);
    rpc.extend(cx.rpc(
        "state",
        typed(move |_: NoArgs, _| Ok::<_, IpcError>(i.state())),
    ));
    let by_model: [(&str, ModelCall); 6] = [
        ("get", Installs::get),
        ("update", Installs::update),
        ("acceptLicense", Installs::accept_license),
        ("cancel", Installs::cancel),
        ("deletePart", Installs::delete_part),
        ("remove", Installs::remove),
    ];
    for (method, call) in by_model {
        let i = Arc::clone(installs);
        rpc.extend(cx.rpc(
            method,
            typed(move |args: ModelArgs, _| Ok::<_, IpcError>(call(&i, &args.model)?)),
        ));
    }
    let i = Arc::clone(installs);
    rpc.extend(cx.rpc(
        "assign",
        typed(move |args: AssignArgs, _| {
            Ok::<_, IpcError>(i.assign(args.task, args.model.as_deref())?)
        }),
    ));
    let i = Arc::clone(installs);
    rpc.extend(cx.rpc(
        "setToken",
        typed(move |args: TokenArgs, _| Ok::<_, IpcError>(i.set_token(&args.token)?)),
    ));
    let i = Arc::clone(installs);
    rpc.extend(cx.rpc(
        "clearToken",
        typed(move |_: NoArgs, _| Ok::<_, IpcError>(i.clear_token()?)),
    ));
    rpc
}

impl Plugin for LocalAi {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let catalog = self.catalog.as_ref().map_err(|e| PluginError::Start {
            id: ID,
            reason: e.to_string(),
        })?;
        let info = CatalogInfo::from(&**catalog);
        let mut rpc = Vec::new();
        rpc.extend(cx.rpc(
            "catalog",
            typed(move |_: NoArgs, _| Ok::<_, IpcError>(info.clone())),
        ));
        rpc.extend(cx.rpc(
            "tasks",
            typed(|_: NoArgs, _| Ok::<_, IpcError>(tasks::infos())),
        ));
        // no download service in this app: install calls answer `unavailable`
        let kit = cx.app().and_then(|app| app.services.get::<DownloadKit>());
        let installs = Installs::new(
            Arc::clone(catalog),
            kit,
            cx.state_dir.join("models"),
            cx.state_file(state::FILE),
            cx.events().clone(),
        );
        rpc.extend(register(cx, &installs));
        Ok(Box::new(Started {
            _rpc: rpc,
            installs,
        }))
    }
}

#[cfg(test)]
mod tests;
