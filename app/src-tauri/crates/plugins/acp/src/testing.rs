//! Fixtures shared by the crate's tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use permark_core::conditions::{Context, Distribution};
use permark_core::document::{DocScope, Documents};
use permark_core::events::{EventSink, NoEvents};
use permark_core::paths::{Root, Vault};
use permark_core::plugins::{CxWorkspace, PluginCx, PluginEvents};
use permark_core::settings::ProjectConfig;
use permark_core::tree::Explorer;
use permark_core::watch::OwnWrites;

use crate::work::Project;
use crate::ID;

/// The project of the folder `root` with its documents service.
pub(crate) fn open(root: &Path) -> (Vault, Arc<Documents>) {
    let vault = Vault::new(Root::open(root).unwrap());
    let documents = Documents::for_project(DocScope::Project(vault.clone()), Arc::new(NoEvents));
    (vault, documents)
}

/// What the work copy needs of `vault` (a fresh tree service, no projects inside).
pub(crate) fn project(vault: &Vault, documents: &Arc<Documents>) -> Project {
    let explorer = Explorer::new(
        vault,
        &ProjectConfig::load(vault.main()),
        Arc::new(NoEvents),
        None,
    );
    Project::new(
        vault.clone(),
        Arc::clone(documents),
        Arc::new(explorer),
        Arc::new(OwnWrites::new()),
    )
}

/// The plugin's event sender into `sink` (payloads without a project wrapper).
pub(crate) fn events(sink: Arc<dyn EventSink>) -> PluginEvents {
    PluginCx::new(
        ID,
        CxWorkspace::App,
        PathBuf::new(),
        None,
        Context::welcome(Distribution::Direct),
        sink,
    )
    .events()
    .clone()
}
