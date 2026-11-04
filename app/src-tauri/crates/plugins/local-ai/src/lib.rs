//! Plugin `local-ai`: small models that run on this computer, for PerMark's own features only (never offered to
//! outside tools).
//!
//! On by default but dormant: starting registers the RPC and nothing else (no thread, no network, no file | folder)
//! until the person acts. The curated catalog ([`catalog`]) is bundled; each PerMark job ([`tasks::Task`]) needs a
//! capability set a model must offer.
//!
//! RPC for the windows: `catalog`, `tasks`.

pub mod catalog;
pub mod tasks;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running,
};
use permark_core::{ErrorKind, IpcError};

pub use catalog::{Capability, Catalog, CatalogError, CatalogInfo, ModelInfo};
pub use tasks::{Task, TaskInfo};

const ID: PluginId = PluginId::of("local-ai");

/// The plugin.
#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(LocalAi {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::App,
            requires: Requirement::NONE,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
    })
}

struct LocalAi {
    manifest: Manifest,
}

struct Started {
    _rpc: Vec<RpcRegistration>,
}

impl Running for Started {}

/// The bundled catalog toward IPC (a bad bundled file = `internal`).
fn catalog_info() -> Result<CatalogInfo, IpcError> {
    catalog::bundled()
        .map(CatalogInfo::from)
        .map_err(|e| IpcError::new(ErrorKind::Internal).detail(e.to_string()))
}

impl Plugin for LocalAi {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let mut rpc = Vec::new();
        rpc.extend(cx.rpc("catalog", typed(|_: NoArgs, _| catalog_info())));
        rpc.extend(cx.rpc(
            "tasks",
            typed(|_: NoArgs, _| Ok::<_, IpcError>(tasks::infos())),
        ));
        Ok(Box::new(Started { _rpc: rpc }))
    }
}

#[cfg(test)]
mod tests;
