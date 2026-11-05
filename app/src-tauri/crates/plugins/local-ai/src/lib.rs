//! Plugin `local-ai`: small models that run on this computer, for PerMark's own features only (never offered to
//! outside tools).
//!
//! On by default but dormant: starting registers the RPC and nothing else (no thread, no network, no file | folder)
//! until the person acts. The curated catalog ([`catalog`]) is bundled; each PerMark job ([`tasks::Task`]) needs a
//! capability set a model must offer.
//!
//! RPC for the windows: `catalog`, `tasks`, `state`, `get`, `update`, `acceptLicense`, `cancel`, `deletePart`,
//! `remove`, `assign`, `setToken`, `clearToken`, `setStore`, `cancelMove` (install calls answer the new
//! [`LocalAiState`]), `revealStore`, `machine` ([`fit::MachineInfo`]: profile read on first call, fit hints,
//! budget), `testModel` + `cancelTest` ([`testrun`], answer `null`). Events (app scope): `state` (whole state after
//! every change), `progress` (one model, throttled by the engine), `move` (store move, ~10/s), `test`
//! ([`testrun::ModelTest`]: one test's phases).
//!
//! "Remove data" (plugin off): core deletes the plugin's namespace (state + default store); [`Plugin::remove_data`]
//! first removes the model folders of a chosen store (ONLY `<lineage>/<version>/` of known lineages there),
//! [`Plugin::data_bytes`] tells the bytes to be freed (models + parts of the current store + engine files).
//!
//! Engine ([`runtime`]): worker files in `<plugin data>/engine/` ([`engine::files`]), one [`runtime::Runtime`]
//! (sessions unloaded before a model's files are removed | moved; every worker ended at plugin off).
//!
//! Model service ([`host`]): app service [`ModelHost`] (`services.get::<ModelHost>()`) while on, for PerMark
//! features (e.g. plugin `correction`); withdrawn first at plugin off.

pub mod catalog;
pub mod engine;
pub mod fit;
pub mod guard;
pub mod host;
pub mod installs;
pub mod machine;
mod relocate;
pub mod runtime;
pub(crate) mod state;
mod store;
pub mod tasks;
pub mod testrun;

use std::collections::BTreeSet;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::downloads::DownloadKit;
use permark_core::plugins::{
    typed, FolderRevealApi, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, ProjectPlacesApi, RpcRegistration, Running, ServiceLease,
};
use permark_core::state::StateStore;
use permark_core::{ErrorKind, IpcError};
use serde::Deserialize;

use crate::engine::files::{self, WorkerFiles};
use crate::host::EngineNews;
use crate::runtime::{Runtime, WorkerLog};
use crate::testrun::{TestParts, Tester};

pub use catalog::{Capability, Catalog, CatalogError, CatalogInfo, EngineNeed, ModelInfo};
pub use host::{engine_ipc, HostError, HostProgress, ModelHost, ModelSession};
pub use installs::{
    AssignedModel, GetBlock, InstallError, InstallState, Installs, LocalAiState, ModelFor,
    ModelProgress, ModelStatus, StoreLinks, StoreMode, StoreMove, StoreStatus, TaskAssignment,
    UnavailableReason,
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
    /// [`ModelHost`] offered to the app (`None`: no app services).
    service: Option<ServiceLease>,
    _rpc: Vec<RpcRegistration>,
    installs: Arc<Installs>,
    runtime: Arc<Runtime>,
    worker_files: Arc<WorkerFiles>,
    tester: Arc<Tester>,
}

impl Running for Started {}

impl Drop for Started {
    fn drop(&mut self) {
        // off (before any data removal): model service withdrawn, the test ends, every engine worker ends,
        // downloads stop (parts kept), threads joined
        drop(self.service.take());
        self.tester.shutdown();
        self.runtime.release_all();
        self.worker_files.shutdown();
        self.installs.shutdown();
    }
}

/// Engine worker log in the plugin's data folder.
const WORKER_LOG: &str = "worker.log";

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreArgs {
    path: Option<String>,
    #[serde(default)]
    mode: Option<StoreMode>,
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
    let i = Arc::clone(installs);
    rpc.extend(cx.rpc(
        "setStore",
        typed(move |args: StoreArgs, _| {
            Ok::<_, IpcError>(i.set_store(args.path.as_deref(), args.mode)?)
        }),
    ));
    let i = Arc::clone(installs);
    rpc.extend(cx.rpc(
        "cancelMove",
        typed(move |_: NoArgs, _| Ok::<_, IpcError>(i.cancel_move())),
    ));
    let i = Arc::clone(installs);
    rpc.extend(cx.rpc("revealStore", typed(move |_: NoArgs, _| i.reveal_store())));
    rpc
}

impl LocalAi {
    /// State + store as kept in the profile (plugin off: read without an instance).
    fn kept(app: &StateStore) -> (state::PluginState, std::path::PathBuf) {
        let kept = app
            .plugin_file::<state::PluginState>(&ID, state::FILE)
            .load()
            .into_value();
        let store = kept
            .store
            .clone()
            .unwrap_or_else(|| app.plugin_dir(&ID).join("models"));
        (kept, store)
    }

    /// Lineages whose folders are the plugin's: the catalog's + the recorded ones that make valid folders.
    fn own_lineages(&self, kept: &state::PluginState) -> BTreeSet<String> {
        let catalog = self.catalog.as_ref().map_or_else(
            |_| Vec::new(),
            |c| c.models.iter().map(|m| m.lineage.clone()).collect(),
        );
        let recorded = kept
            .installed
            .iter()
            .chain(kept.stale.iter().map(|s| (&s.lineage, &s.installed)))
            .filter(|(lineage, i)| i.unit(lineage).is_ok())
            .map(|(lineage, _)| lineage.clone());
        catalog.into_iter().chain(recorded).collect()
    }
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
        // nothing created | spawned until a load (dormant)
        let news = Arc::new(EngineNews::default());
        let worker_files = Arc::new(WorkerFiles::new(
            kit.as_deref(),
            cx.state_dir.join(files::DIR),
            news.listener(),
        ));
        let runtime = Arc::new(Runtime::new(
            Arc::clone(&worker_files),
            Arc::new(WorkerLog::new(cx.state_dir.join(WORKER_LOG))),
        ));
        let installs = Installs::new(
            Arc::clone(catalog),
            kit,
            cx.state_dir.join("models"),
            cx.state_file(state::FILE),
            cx.events().clone(),
            StoreLinks {
                places: cx
                    .app()
                    .and_then(|app| app.services.get::<ProjectPlacesApi>())
                    .map(|api| (*api).clone()),
                reveal: cx
                    .app()
                    .and_then(|app| app.services.get::<FolderRevealApi>())
                    .map(|api| (*api).clone()),
                profile: cx.store().map(|store| store.app_dir().to_path_buf()),
            },
            worker_files.pin(),
            Some(Arc::clone(&runtime)),
        );
        rpc.extend(register(cx, &installs));
        // read on the first call only (dormant start)
        let machine = machine::Machine::new(machine::detect);
        let (c, i) = (Arc::clone(catalog), Arc::clone(&installs));
        rpc.extend(cx.rpc(
            "machine",
            typed(move |_: NoArgs, _| {
                // state alone: no engine started, no store read
                let assigned = i.assigned_ids();
                Ok::<_, IpcError>(fit::advise(&c, machine.profile(), &assigned))
            }),
        ));
        let host = Arc::new(ModelHost::new(
            Arc::clone(&installs),
            Arc::clone(catalog),
            Arc::clone(&runtime),
            Arc::clone(&worker_files),
            news,
        ));
        let service = cx.app().map(|app| app.services.provide(Arc::clone(&host)));
        let tester = Arc::new(Tester::new(TestParts {
            host,
            events: cx.events().clone(),
        }));
        let t = Arc::clone(&tester);
        rpc.extend(cx.rpc(
            "testModel",
            typed(move |args: ModelArgs, _| {
                t.start(&args.model)?;
                Ok::<_, IpcError>(())
            }),
        ));
        let t = Arc::clone(&tester);
        rpc.extend(cx.rpc(
            "cancelTest",
            typed(move |_: NoArgs, _| {
                t.cancel();
                Ok::<_, IpcError>(())
            }),
        ));
        Ok(Box::new(Started {
            service,
            _rpc: rpc,
            installs,
            runtime,
            worker_files,
            tester,
        }))
    }

    fn data_bytes(&self, app: &StateStore) -> Option<u64> {
        let (kept, store) = Self::kept(app);
        let engine = files::bytes(&app.plugin_dir(&ID).join(files::DIR));
        Some(store::bytes(&store, &self.own_lineages(&kept)) + engine)
    }

    /// A chosen store: its model folders (the default one lives in the namespace core deletes next). A chosen store
    /// that is gone: nothing reachable to remove.
    fn remove_data(&self, app: &StateStore) -> Result<(), IpcError> {
        let (kept, store) = Self::kept(app);
        if kept.store.is_none() || !store.is_dir() {
            return Ok(());
        }
        store::remove_all(&store, &self.own_lineages(&kept)).map_err(|e| {
            IpcError::new(ErrorKind::Io)
                .subject(store.display().to_string())
                .detail(e.to_string())
        })
    }
}

#[cfg(test)]
mod tests;
