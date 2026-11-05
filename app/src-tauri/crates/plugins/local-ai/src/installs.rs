//! Installs: get | cancel | delete part | remove | update of catalog models through core `downloads` (one lazy
//! [`Downloads`] per store), terms acceptance, Hugging Face token, task assignments, [`Installs::model_for`].
//!
//! Lazy: no engine, folder, thread, request until a call needs one. The store folder is made by the first download
//! only; a state query reads (never creates, never removes). Startup of a store that exists: [`Downloads::sweep`]
//! lists the paused parts (NEVER restarted). Tidy ([`crate::store::tidy`] under [`Downloads::while_idle`]) ONLY on a
//! person's action that writes to the store (get | update | delete part | remove | store move): leftover version
//! folders of known lineages that hold no install | part, scratch copies of a stopped move, then old versions whose
//! removal failed (`stale`).
//!
//! Per model (activity first, else what is on disk):
//! `notInstalled -get-> queued -> downloading <-> verifying -> installed`; `downloading -cancel-> paused -get->
//! queued`; `* -fail-> failed -get-> queued`; installed version < catalog -> `updateAvailable -update-> queued -> ...
//! -> installed` (old version removed after the new one verified; a failed update leaves the old one usable).
//!
//! Locks: the engine calls the listener with none of its locks held; this module NEVER holds its own lock while it
//! calls the engine (`cancel` | `delete_part` | `remove` | `enqueue` may call the listener on the caller's thread)
//! and NEVER drops an engine under it (drop joins the engine's worker, whose listener takes the lock). Disk reads
//! (installed checks, free space, project overlap) run on a copy taken under the lock, never under it.
//!
//! Store (AC-7, [`moves`]): default `<profile>/plugins/local-ai/models/` | a folder the person chose. A chosen store
//! that is gone (drive unplugged) | overlaps a project opened since: `storeUnavailable`, never `notInstalled`;
//! nothing created there.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use permark_core::downloads::{
    DownloadError, DownloadKit, Downloads, Phase, Progress, Secret, SecretError, Source, Unit,
    Update,
};
use permark_core::fsx;
use permark_core::plugins::{FolderRevealApi, PluginEvents, ProjectPlacesApi};
use permark_core::state::{StateError, StateFile};
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalog::{Catalog, CatalogError, Model, ModelFile};
use crate::runtime::Runtime;
use crate::state::{Acceptance, Installed, PluginState, Stale};
use crate::store::{prune, tidy, version_dir};
use crate::tasks::Task;

mod moves;

/// Longest Hugging Face token taken, bytes (real ones: < 100).
const TOKEN_LIMIT: usize = 1024;

/// Where one model stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, rename = "InstallState")]
pub enum InstallState {
    NotInstalled,
    /// Waiting for its turn (one download at a time).
    Queued,
    Downloading {
        #[ts(type = "number")]
        done: u64,
        #[ts(type = "number")]
        total: u64,
    },
    /// Rehashing a part before resuming | the final check (no cancel in the UI).
    Verifying {
        #[ts(type = "number")]
        done: u64,
        #[ts(type = "number")]
        total: u64,
    },
    /// Stopped with a part kept (cancel | found at start): Get resumes, Delete part drops it.
    Paused {
        #[ts(type = "number")]
        done: u64,
        #[ts(type = "number")]
        total: u64,
    },
    #[serde(rename_all = "camelCase")]
    Installed {
        catalog_version: u32,
    },
    /// An older version is installed (usable) and the catalog has a newer one.
    #[serde(rename_all = "camelCase")]
    UpdateAvailable {
        installed_version: u32,
    },
    /// Last download stopped; Get tries again (resumes where a part was kept).
    Failed {
        error: DownloadError,
    },
    /// The chosen store folder is gone (drive unplugged | folder removed): what it holds is unknown.
    StoreUnavailable,
}

/// Why Get cannot start now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "GetBlock")]
pub enum GetBlock {
    /// Terms to accept first (`acceptLicense`).
    License,
    /// Gated: needs a Hugging Face token (`setToken`).
    Token,
    /// Taken out of the catalog.
    Withdrawn,
}

impl GetBlock {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::License => "license",
            Self::Token => "token",
            Self::Withdrawn => "withdrawn",
        }
    }
}

/// One catalog model as the windows see its install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ModelStatus")]
pub struct ModelStatus {
    pub model: String,
    pub state: InstallState,
    /// Version usable now (`None`: none installed); stays during an update until the new one is in.
    pub installed_version: Option<u32>,
    /// Why Get is refused now (`None`: Get works).
    pub blocked: Option<GetBlock>,
}

/// Model chosen for one task (`model` = catalog id; `None`: not assigned | assigned model gone).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TaskAssignment")]
pub struct TaskAssignment {
    pub task: Task,
    pub model: Option<String>,
}

/// Everything the settings page shows of installs (RPC `local-ai.state`, event `local-ai:state`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "LocalAiState")]
pub struct LocalAiState {
    /// Catalog order.
    pub models: Vec<ModelStatus>,
    /// Every task, in [`Task::ALL`] order.
    pub assignments: Vec<TaskAssignment>,
    pub store: StoreStatus,
    /// Running | failed store move (`None`: none).
    pub store_move: Option<StoreMove>,
    /// A Hugging Face token is set (the value never leaves the credential store). Asked only when a gated model is
    /// listed.
    pub token_set: bool,
    /// License addresses accepted.
    pub accepted_licenses: Vec<String>,
}

/// Where models are kept (the Storage card).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "StoreStatus")]
pub struct StoreStatus {
    /// Store folder (the default one may not exist yet).
    pub path: String,
    pub is_default: bool,
    /// `false`: a chosen folder that is gone | overlaps a project now (models then `storeUnavailable`).
    pub available: bool,
    /// Why it is not available (`None`: available).
    pub unavailable_reason: Option<UnavailableReason>,
    /// Free bytes on its volume (`None`: unknown | unavailable).
    #[ts(type = "number | null")]
    pub free_bytes: Option<u64>,
    /// Bytes of the installed versions listed.
    #[ts(type = "number")]
    pub used_bytes: u64,
    /// Bytes of downloads kept to resume (paused | running | failed).
    #[ts(type = "number")]
    pub partial_bytes: u64,
}

/// Why the chosen store cannot be used now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "StoreUnavailableReason")]
pub enum UnavailableReason {
    /// The folder is gone (drive unplugged | folder removed).
    Missing,
    /// A project opened since is inside | holds it.
    InsideProject,
}

/// A store move (event `local-ai:move` while it runs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, rename = "StoreMove")]
pub enum StoreMove {
    /// Copying + checking into `to`; the old store stays in use until done.
    Moving {
        to: String,
        #[ts(type = "number")]
        done: u64,
        #[ts(type = "number")]
        total: u64,
    },
    /// Last move stopped (copies removed, old store in use). Cleared by the next store change.
    Failed { to: String, error: DownloadError },
}

/// What happens to installed models when the store changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "StoreMode")]
pub enum StoreMode {
    /// Copied into the new folder, checked, then removed from the old one.
    Move,
    /// Left in the old folder (not listed); the new folder starts empty.
    Keep,
}

/// One model's progress (event `local-ai:progress`, ~10/s while downloading).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ModelProgress")]
pub struct ModelProgress {
    pub model: String,
    pub state: InstallState,
}

/// An installed model a feature can load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignedModel {
    /// Id of the installed version.
    pub id: String,
    pub lineage: String,
    pub catalog_version: u32,
    /// Folder holding its files.
    pub dir: PathBuf,
    /// Pinned files of this installed version (stored under their file names in `dir`).
    pub files: Vec<ModelFile>,
}

/// Answer of [`Installs::model_for`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelFor {
    Assigned(AssignedModel),
    NoModel,
}

/// Why an install call is refused.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("unknown model {0}")]
    UnknownModel(String),
    #[error("model {model}: get refused ({})", block.as_str())]
    Blocked { model: String, block: GetBlock },
    #[error("no download service")]
    NoDownloads,
    #[error("model {0} is downloading")]
    Downloading(String),
    #[error("model {0} is not installed")]
    NotInstalled(String),
    #[error("model {0} cannot take this task")]
    Incompatible(String),
    #[error("model {0} has no update")]
    NoUpdate(String),
    #[error("empty token")]
    EmptyToken,
    #[error("token longer than {TOKEN_LIMIT} bytes")]
    TokenTooLong,
    #[error("the model folder is not available")]
    StoreUnavailable,
    #[error("store {0}: not an absolute path")]
    NotAbsolute(String),
    #[error("store {0}: inside or holding a project folder")]
    InsideProject(String),
    #[error("store {0}: inside or holding a model folder")]
    InsideStore(String),
    #[error("store {0}: inside or holding the app's data folder")]
    InsideAppData(String),
    #[error("store {0}: installed models need move or keep")]
    ModeRequired(String),
    #[error("the model folder is moving")]
    Moving,
    #[error("store {0}: {1}")]
    Folder(String, std::io::Error),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error(transparent)]
    Catalog(#[from] CatalogError),
}

impl From<InstallError> for IpcError {
    fn from(error: InstallError) -> Self {
        let model = |kind, id: String, detail: &str| IpcError::new(kind).subject(id).detail(detail);
        match error {
            InstallError::UnknownModel(id) => IpcError::new(ErrorKind::NotFound).subject(id),
            InstallError::Blocked { model: id, block } => {
                model(ErrorKind::Denied, id, block.as_str())
            }
            InstallError::NoDownloads => IpcError::new(ErrorKind::Unavailable).subject("downloads"),
            InstallError::Downloading(id) => model(ErrorKind::Conflict, id, "downloading"),
            InstallError::NotInstalled(id) => model(ErrorKind::Conflict, id, "notInstalled"),
            InstallError::Incompatible(id) => model(ErrorKind::Unsupported, id, "incompatible"),
            InstallError::NoUpdate(id) => model(ErrorKind::Conflict, id, "noUpdate"),
            InstallError::EmptyToken => IpcError::new(ErrorKind::InvalidData)
                .subject("credentials")
                .detail("empty"),
            InstallError::TokenTooLong => IpcError::new(ErrorKind::InvalidData)
                .subject("credentials")
                .detail("tooLong"),
            InstallError::StoreUnavailable => {
                IpcError::new(ErrorKind::Unavailable).subject("store")
            }
            InstallError::NotAbsolute(path) => model(ErrorKind::InvalidPath, path, "notAbsolute"),
            InstallError::InsideProject(path) => model(ErrorKind::Denied, path, "insideProject"),
            InstallError::InsideStore(path) => model(ErrorKind::Denied, path, "insideStore"),
            InstallError::InsideAppData(path) => model(ErrorKind::Denied, path, "insideAppData"),
            InstallError::ModeRequired(path) => model(ErrorKind::Conflict, path, "modeRequired"),
            InstallError::Moving => model(ErrorKind::Conflict, "store".to_owned(), "moving"),
            InstallError::Folder(path, e) => IpcError::new(ErrorKind::Io)
                .subject(path)
                .detail(e.to_string()),
            InstallError::Download(e) => e.into(),
            InstallError::Secret(e) => e.into(),
            InstallError::State(e) => e.into(),
            InstallError::Catalog(e) => IpcError::new(ErrorKind::Internal).detail(e.to_string()),
        }
    }
}

/// What a unit is doing now (in memory; gone = look at the disk).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Activity {
    Queued,
    Running(Phase),
    Paused,
    Failed(DownloadError),
    /// `deletePart` asked while queued | running: the cancel that follows drops the entry.
    Discarding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Live {
    activity: Activity,
    /// Bytes of the unit known on disk (last progress | paused part).
    done: u64,
}

/// A store move running.
struct Mover {
    to: PathBuf,
    cancel: Arc<AtomicBool>,
    done: u64,
    total: u64,
}

struct Inner {
    state: PluginState,
    live: HashMap<String, Live>,
    /// Engine of the current store (`Downloads::store` = its folder).
    engine: Option<Arc<Downloads>>,
    moving: Option<Mover>,
    /// Last move that failed: target + why.
    move_failed: Option<(PathBuf, DownloadError)>,
    /// A store change is being decided (set together with the download check, under this lock): Get refused.
    changing: bool,
}

/// App services the store choice uses (`None`: the app offers none; tests).
#[derive(Clone, Default)]
pub struct StoreLinks {
    pub places: Option<ProjectPlacesApi>,
    pub reveal: Option<FolderRevealApi>,
    /// The app's profile folder: a chosen store never lands inside | holds it (only the default store lives there).
    pub profile: Option<PathBuf>,
}

/// Installs of one plugin instance.
pub struct Installs {
    catalog: Arc<Catalog>,
    kit: Option<Arc<DownloadKit>>,
    default_store: PathBuf,
    file: Option<StateFile<PluginState>>,
    events: PluginEvents,
    links: StoreLinks,
    /// Engine host: sessions of a model unloaded before its files are removed | moved (D-11; `None`: tests).
    runtime: Option<Arc<Runtime>>,
    inner: Mutex<Inner>,
    /// One engine start (sweep) at a time.
    starting: Mutex<()>,
    /// Thread of the running | last store move.
    mover: Mutex<Option<JoinHandle<()>>>,
    /// Token set, as read once from the credential store (`None`: not read yet); kept by set | clear.
    token: Mutex<Option<bool>>,
    me: Weak<Self>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl Installs {
    /// Installs over `catalog`; state read from `file` (nothing written, nothing created).
    #[must_use]
    pub(crate) fn new(
        catalog: Arc<Catalog>,
        kit: Option<Arc<DownloadKit>>,
        default_store: PathBuf,
        file: Option<StateFile<PluginState>>,
        events: PluginEvents,
        links: StoreLinks,
        runtime: Option<Arc<Runtime>>,
    ) -> Arc<Self> {
        let state = file
            .as_ref()
            .map(|f| f.load().into_value())
            .unwrap_or_default();
        Arc::new_cyclic(|me| Self {
            catalog,
            kit,
            default_store,
            file,
            events,
            links,
            runtime,
            inner: Mutex::new(Inner {
                state,
                live: HashMap::new(),
                engine: None,
                moving: None,
                move_failed: None,
                changing: false,
            }),
            starting: Mutex::new(()),
            mover: Mutex::new(None),
            token: Mutex::new(None),
            me: me.clone(),
        })
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn model(&self, id: &str) -> Result<&Model, InstallError> {
        self.catalog
            .model(id)
            .ok_or_else(|| InstallError::UnknownModel(id.to_owned()))
    }

    fn store_of(&self, state: &PluginState) -> PathBuf {
        state
            .store
            .clone()
            .unwrap_or_else(|| self.default_store.clone())
    }

    /// Why the store of `state` cannot be used now (`None`: usable; the default one is made by the first download).
    /// Disk read: NEVER call under the lock.
    fn unavailable(&self, state: &PluginState) -> Option<UnavailableReason> {
        let store = state.store.as_ref()?;
        if !store.is_dir() {
            Some(UnavailableReason::Missing)
        } else if self
            .links
            .places
            .as_ref()
            .is_some_and(|places| places.0.overlaps(store))
        {
            Some(UnavailableReason::InsideProject)
        } else {
            None
        }
    }

    /// Catalog lineages (the folders the store start may clean).
    fn lineages(&self) -> BTreeSet<String> {
        self.catalog
            .models
            .iter()
            .map(|m| m.lineage.clone())
            .collect()
    }

    /// Version folder `lineage`/`version` is the catalog's | recorded in `state` (kept by the orphan cleanup).
    fn known_version(&self, state: &PluginState, lineage: &str, version: u32) -> bool {
        self.catalog
            .models
            .iter()
            .any(|m| m.lineage == lineage && m.catalog_version == version)
            || state
                .installed
                .get(lineage)
                .is_some_and(|i| i.catalog_version == version)
            || state
                .stale
                .iter()
                .any(|s| s.lineage == lineage && s.installed.catalog_version == version)
    }

    /// The engine of the current store, started on first need (sweep only when the store folder exists; nothing
    /// removed beyond the engine's own part cleanup).
    ///
    /// # Errors
    /// [`InstallError::NoDownloads`]: the app offers no download service; [`InstallError::StoreUnavailable`].
    fn engine(&self) -> Result<Arc<Downloads>, InstallError> {
        let kit = self.kit.as_ref().ok_or(InstallError::NoDownloads)?;
        let _starting = self.starting.lock().unwrap_or_else(PoisonError::into_inner);
        let state = self.inner().state.clone();
        if self.unavailable(&state).is_some() {
            return Err(InstallError::StoreUnavailable);
        }
        let store = self.store_of(&state);
        if let Some(engine) = self.inner().engine.as_ref().filter(|e| e.store() == store) {
            return Ok(Arc::clone(engine));
        }
        let me = self.me.clone();
        let engine = Arc::new(kit.downloads(
            store.clone(),
            Arc::new(move |id, update| {
                if let Some(installs) = me.upgrade() {
                    installs.heard(id, update);
                }
            }),
        ));
        let paused = if store.is_dir() {
            engine.sweep(&self.known_units(&state)).unwrap_or_default()
        } else {
            Vec::new()
        };
        // kept bytes per model: its parts + the files of the unit already complete
        let found: Vec<(String, u64)> = self
            .catalog
            .models
            .iter()
            .filter_map(|model| {
                let mut parts = paused.iter().filter(|p| p.unit == model.id).peekable();
                parts.peek()?;
                let partial: u64 = parts.map(|p| p.done).sum();
                let complete = model.unit().map_or(0, |u| engine.present_bytes(&u));
                Some((model.id.clone(), partial + complete))
            })
            .collect();
        let mut inner = self.inner();
        for (id, done) in found {
            inner.live.entry(id).or_insert(Live {
                activity: Activity::Paused,
                done,
            });
        }
        let previous = inner.engine.replace(Arc::clone(&engine));
        drop(inner);
        drop(previous);
        Ok(engine)
    }

    /// Tidy of the current store before a person's action writes to it (M1 rules, [`crate::store::tidy`]): only
    /// while this engine is idle and no other app instance holds the store; then the old versions whose removal
    /// failed are tried again (in their own store). Best effort.
    fn tidy(&self, engine: &Downloads) {
        let state = self.inner().state.clone();
        let store = engine.store();
        if store.is_dir() {
            let _ = engine.while_idle(|| {
                tidy(store, &self.lineages(), |lineage, version| {
                    self.known_version(&state, lineage, version)
                });
            });
        }
        let mut removed = Vec::new();
        for old in &state.stale {
            let place = old.store.as_deref().unwrap_or(store);
            let Ok(unit) = old.installed.unit(&old.lineage) else {
                continue;
            };
            let gone = if place == store {
                engine.remove(&unit).is_ok()
            } else {
                // another store (a move's old copy): an engine of its own, never started (no worker)
                place.is_dir()
                    && self.kit.as_ref().is_some_and(|kit| {
                        kit.downloads(place.to_path_buf(), Arc::new(|_, _| {}))
                            .remove(&unit)
                            .is_ok()
                    })
            };
            if gone {
                prune(place, &old.lineage, old.installed.catalog_version);
                removed.push(old.clone());
            }
        }
        if !removed.is_empty() {
            let _ = self.change(|s| s.stale.retain(|x| !removed.contains(x)));
        }
    }

    /// Engine already started (listener: never starts one).
    fn current_engine(&self) -> Option<Arc<Downloads>> {
        self.inner().engine.clone()
    }

    /// Units the store may hold: every catalog version + every recorded install + stale ones.
    fn known_units(&self, state: &PluginState) -> Vec<Unit> {
        let catalog = self.catalog.models.iter().filter_map(|m| m.unit().ok());
        let installed = state
            .installed
            .iter()
            .filter_map(|(lineage, i)| i.unit(lineage).ok());
        let leftover = state
            .stale
            .iter()
            .filter_map(|s| s.installed.unit(&s.lineage).ok());
        catalog.chain(installed).chain(leftover).collect()
    }

    fn save(&self, next: &PluginState) -> Result<(), StateError> {
        self.file.as_ref().map_or(Ok(()), |file| file.save(next))
    }

    /// Changes the state: written first, kept in memory once on disk. No change -> nothing written.
    fn change(&self, edit: impl FnOnce(&mut PluginState)) -> Result<(), InstallError> {
        let mut inner = self.inner();
        let mut next = inner.state.clone();
        edit(&mut next);
        if next != inner.state {
            self.save(&next)?;
            inner.state = next;
        }
        Ok(())
    }

    /// Version of `model`'s lineage usable now: the catalog version on disk, else the recorded one on disk.
    fn usable(engine: Option<&Downloads>, state: &PluginState, model: &Model) -> Option<Installed> {
        let engine = engine?;
        if model.unit().is_ok_and(|u| engine.is_installed(&u)) {
            return Some(Installed::of(model));
        }
        state
            .installed
            .get(&model.lineage)
            .filter(|i| {
                i.unit(&model.lineage)
                    .is_ok_and(|u| engine.is_installed(&u))
            })
            .cloned()
    }

    /// A token is set (asked only when a gated model is listed; read once, then kept by set | clear).
    fn token_set(&self) -> bool {
        if !self.catalog.models.iter().any(|m| m.gated) {
            return false;
        }
        let mut token = self.token.lock().unwrap_or_else(PoisonError::into_inner);
        *token.get_or_insert_with(|| {
            self.kit
                .as_ref()
                .is_some_and(|kit| kit.secrets.is_set(&Source::HUGGING_FACE).unwrap_or(false))
        })
    }

    fn token_known(&self, set: bool) {
        *self.token.lock().unwrap_or_else(PoisonError::into_inner) = Some(set);
    }

    fn blocked(model: &Model, state: &PluginState, token_set: bool) -> Option<GetBlock> {
        if model.withdrawn {
            Some(GetBlock::Withdrawn)
        } else if model.gated && !token_set {
            Some(GetBlock::Token)
        } else if model.license.needs_acceptance && !state.accepted(&model.license) {
            Some(GetBlock::License)
        } else {
            None
        }
    }

    fn install_state(
        live: Option<&Live>,
        model: &Model,
        usable: Option<&Installed>,
        available: bool,
    ) -> InstallState {
        let total = model.bytes();
        match live.map(|l| (&l.activity, l.done)) {
            Some((Activity::Queued, _)) => return InstallState::Queued,
            Some((Activity::Running(Phase::Downloading), done)) => {
                return InstallState::Downloading { done, total }
            }
            Some((Activity::Running(Phase::Verifying), done)) => {
                return InstallState::Verifying { done, total }
            }
            Some((Activity::Paused, done)) => return InstallState::Paused { done, total },
            Some((Activity::Failed(error), _)) => {
                return InstallState::Failed {
                    error: error.clone(),
                }
            }
            Some((Activity::Discarding, _)) | None => {}
        }
        match usable {
            Some(i) if i.catalog_version < model.catalog_version => InstallState::UpdateAvailable {
                installed_version: i.catalog_version,
            },
            Some(i) => InstallState::Installed {
                catalog_version: i.catalog_version,
            },
            None if available => InstallState::NotInstalled,
            None => InstallState::StoreUnavailable,
        }
    }

    fn snapshot(&self, engine: Option<&Downloads>) -> LocalAiState {
        let token_set = self.token_set();
        let (state, live, store_move) = {
            let inner = self.inner();
            let store_move = match (&inner.moving, &inner.move_failed) {
                (Some(m), _) => Some(StoreMove::Moving {
                    to: m.to.display().to_string(),
                    done: m.done,
                    total: m.total,
                }),
                (None, Some((to, error))) => Some(StoreMove::Failed {
                    to: to.display().to_string(),
                    error: error.clone(),
                }),
                (None, None) => None,
            };
            (inner.state.clone(), inner.live.clone(), store_move)
        };
        let unavailable = self.unavailable(&state);
        let available = unavailable.is_none();
        let usable: Vec<Option<Installed>> = self
            .catalog
            .models
            .iter()
            .map(|m| Self::usable(engine, &state, m))
            .collect();
        let models = self
            .catalog
            .models
            .iter()
            .zip(&usable)
            .map(|(m, u)| ModelStatus {
                model: m.id.clone(),
                state: Self::install_state(live.get(&m.id), m, u.as_ref(), available),
                installed_version: u.as_ref().map(|i| i.catalog_version),
                blocked: Self::blocked(m, &state, token_set),
            })
            .collect();
        let assignments = Task::ALL
            .iter()
            .map(|&task| TaskAssignment {
                task,
                model: state.assignments.get(&task).and_then(|lineage| {
                    self.catalog
                        .models
                        .iter()
                        .zip(&usable)
                        .find(|(m, u)| &m.lineage == lineage && u.is_some())
                        .map(|(m, _)| m.id.clone())
                }),
            })
            .collect();
        let store = self.store_of(&state);
        let used_bytes = usable
            .iter()
            .flatten()
            .flat_map(|i| &i.files)
            .map(|f| f.bytes)
            .sum();
        let partial_bytes = live
            .values()
            .filter(|l| l.activity != Activity::Discarding)
            .map(|l| l.done)
            .sum();
        LocalAiState {
            models,
            assignments,
            store: StoreStatus {
                path: store.display().to_string(),
                is_default: state.store.is_none(),
                available,
                unavailable_reason: unavailable,
                free_bytes: available.then(|| fsx::free_space(&store).ok()).flatten(),
                used_bytes,
                partial_bytes,
            },
            store_move,
            token_set,
            accepted_licenses: state
                .acceptances
                .iter()
                .map(|a| a.license_url.clone())
                .collect(),
        }
    }

    /// The whole state (starts the engine to read the disk; creates nothing).
    #[must_use]
    pub fn state(&self) -> LocalAiState {
        let engine = self.engine().ok();
        self.snapshot(engine.as_deref())
    }

    fn announce(&self) {
        let engine = self.current_engine();
        if let Ok(payload) = serde_json::to_value(self.snapshot(engine.as_deref())) {
            self.events.emit("state", payload);
        }
    }

    /// Engine news of unit `id` (worker thread).
    fn heard(&self, id: &str, update: &Update) {
        let Some(model) = self.catalog.model(id) else {
            return;
        };
        match update {
            Update::Progress(Progress { done, phase, .. }) => {
                let state = {
                    let mut inner = self.inner();
                    let live = inner.live.entry(id.to_owned()).or_insert(Live {
                        activity: Activity::Queued,
                        done: 0,
                    });
                    if live.activity == Activity::Discarding {
                        return;
                    }
                    *live = Live {
                        activity: Activity::Running(*phase),
                        done: *done,
                    };
                    Self::install_state(Some(live), model, None, true)
                };
                let progress = ModelProgress {
                    model: id.to_owned(),
                    state,
                };
                if let Ok(payload) = serde_json::to_value(progress) {
                    self.events.emit("progress", payload);
                }
            }
            Update::Done => self.finished(model),
            Update::Failed(error) => {
                let mut inner = self.inner();
                let live = inner.live.remove(id);
                let done = live.as_ref().map_or(0, |l| l.done);
                let discarding = live.is_some_and(|l| l.activity == Activity::Discarding);
                let next = match error {
                    _ if discarding => None,
                    DownloadError::Cancelled => (done > 0).then_some(Live {
                        activity: Activity::Paused,
                        done,
                    }),
                    DownloadError::Integrity => Some(Live {
                        activity: Activity::Failed(error.clone()),
                        done: 0,
                    }),
                    other => Some(Live {
                        activity: Activity::Failed(other.clone()),
                        done,
                    }),
                };
                if let Some(next) = next {
                    inner.live.insert(id.to_owned(), next);
                }
                drop(inner);
                self.announce();
            }
        }
    }

    /// `model` verified + installed: recorded; an older version of its lineage removed (failed: kept for later).
    ///
    /// Activity (verifying) kept until the swap is complete: `installed` is only reported once the old version is gone
    /// (else a query between the manifest and the removal reads `installed` beside the old folder).
    fn finished(&self, model: &Model) {
        let previous = {
            let mut inner = self.inner();
            let previous = inner.state.installed.get(&model.lineage).cloned();
            let mut next = inner.state.clone();
            next.installed
                .insert(model.lineage.clone(), Installed::of(model));
            // on disk it is installed anyway (manifest): a failed write only loses the old version's record
            let _ = self.save(&next);
            inner.state = next;
            previous.filter(|p| p.catalog_version != model.catalog_version)
        };
        if let (Some(old), Some(engine)) = (previous, self.current_engine()) {
            let gone = old
                .unit(&model.lineage)
                .is_ok_and(|unit| engine.remove(&unit).is_ok());
            if gone {
                prune(engine.store(), &model.lineage, old.catalog_version);
            } else {
                let _ = self.change(|s| {
                    s.stale.push(Stale {
                        lineage: model.lineage.clone(),
                        installed: old,
                        store: Some(engine.store().to_path_buf()),
                    });
                });
            }
        }
        self.inner().live.remove(&model.id);
        self.announce();
    }

    /// Get (AC-3): queues the catalog version of `id`. Refused: terms not accepted, gated without token, withdrawn.
    /// Already installed | queued | running: nothing.
    ///
    /// # Errors
    /// [`InstallError`].
    pub fn get(&self, id: &str) -> Result<LocalAiState, InstallError> {
        let model = self.model(id)?;
        self.not_moving()?;
        let token_set = self.token_set();
        if let Some(block) = Self::blocked(model, &self.inner().state, token_set) {
            return Err(InstallError::Blocked {
                model: id.to_owned(),
                block,
            });
        }
        let engine = self.engine()?;
        let unit = model.unit()?;
        let installed = engine.is_installed(&unit);
        if !installed {
            self.tidy(&engine);
        }
        let queue = {
            let mut inner = self.inner();
            // a store change began | ended since the engine was taken: never queued into the old store
            let same_store = inner
                .engine
                .as_ref()
                .is_some_and(|e| Arc::ptr_eq(e, &engine));
            if inner.moving.is_some() || inner.changing || !same_store {
                return Err(InstallError::Moving);
            }
            let busy = inner
                .live
                .get(id)
                .is_some_and(|l| matches!(l.activity, Activity::Queued | Activity::Running(_)));
            let queue = !busy && !installed;
            if queue {
                let done = inner.live.get(id).map_or(0, |l| l.done);
                inner.live.insert(
                    id.to_owned(),
                    Live {
                        activity: Activity::Queued,
                        done,
                    },
                );
            }
            queue
        };
        if queue {
            engine.enqueue(unit);
            self.announce();
        }
        Ok(self.snapshot(Some(&engine)))
    }

    /// Update (AC-6): Get of the catalog version while an older one is installed (kept until the new one verified).
    ///
    /// # Errors
    /// [`InstallError::NoUpdate`]: no older version installed; else as [`Installs::get`].
    pub fn update(&self, id: &str) -> Result<LocalAiState, InstallError> {
        let model = self.model(id)?;
        let engine = self.engine()?;
        let state = self.inner().state.clone();
        let older = Self::usable(Some(&engine), &state, model)
            .is_some_and(|i| i.catalog_version < model.catalog_version);
        if !older {
            return Err(InstallError::NoUpdate(id.to_owned()));
        }
        self.get(id)
    }

    /// Records acceptance of `id`'s terms (before any byte). Terms needing none | already accepted: nothing written.
    ///
    /// # Errors
    /// Unknown model, state write.
    pub fn accept_license(&self, id: &str) -> Result<LocalAiState, InstallError> {
        let model = self.model(id)?;
        if model.license.needs_acceptance {
            self.change(|s| {
                if !s.accepted(&model.license) {
                    // an older revision's record of these terms: replaced
                    s.acceptances.retain(|a| a.license_url != model.license.url);
                    s.acceptances.push(Acceptance {
                        model: id.to_owned(),
                        license_url: model.license.url.clone(),
                        revision: model.license.revision.clone(),
                        accepted_at: now_ms(),
                    });
                }
            })?;
        }
        self.announce();
        Ok(self.state())
    }

    /// Stops `id` (part kept -> paused).
    ///
    /// # Errors
    /// Unknown model, no download service.
    pub fn cancel(&self, id: &str) -> Result<LocalAiState, InstallError> {
        self.model(id)?;
        let engine = self.engine()?;
        engine.cancel(id);
        Ok(self.snapshot(Some(&engine)))
    }

    /// Drops the part of `id` (running | queued: stopped first).
    ///
    /// # Errors
    /// Unknown model, no download service, store busy | disk.
    pub fn delete_part(&self, id: &str) -> Result<LocalAiState, InstallError> {
        let model = self.model(id)?;
        self.not_moving()?;
        let engine = self.engine()?;
        let unit = model.unit()?;
        self.tidy(&engine);
        {
            let mut inner = self.inner();
            let active = inner
                .live
                .get(id)
                .is_some_and(|l| matches!(l.activity, Activity::Queued | Activity::Running(_)));
            if active {
                inner.live.insert(
                    id.to_owned(),
                    Live {
                        activity: Activity::Discarding,
                        done: 0,
                    },
                );
            } else {
                inner.live.remove(id);
            }
        }
        engine.delete_part(&unit)?;
        prune(engine.store(), &model.lineage, model.catalog_version);
        self.announce();
        Ok(self.snapshot(Some(&engine)))
    }

    /// Remove (AC-6): every installed version of `id`'s lineage (files + parts; Windows locks retried by the engine);
    /// tasks assigned to it unassigned.
    ///
    /// # Errors
    /// [`InstallError::Downloading`] (cancel first), store busy | disk, state write.
    pub fn remove(&self, id: &str) -> Result<LocalAiState, InstallError> {
        let model = self.model(id)?;
        self.not_moving()?;
        let engine = self.engine()?;
        let unit = model.unit()?;
        let recorded = {
            let inner = self.inner();
            if inner
                .live
                .get(id)
                .is_some_and(|l| matches!(l.activity, Activity::Queued | Activity::Running(_)))
            {
                return Err(InstallError::Downloading(id.to_owned()));
            }
            inner.state.installed.get(&model.lineage).cloned()
        };
        // Windows: a loaded model's files are mapped (locked) -> unloaded first
        if let Some(runtime) = &self.runtime {
            runtime.release(&model.lineage);
        }
        self.tidy(&engine);
        engine.remove(&unit)?;
        prune(engine.store(), &model.lineage, model.catalog_version);
        if let Some(old) = recorded.filter(|r| r.catalog_version != model.catalog_version) {
            engine.remove(&old.unit(&model.lineage)?)?;
            prune(engine.store(), &model.lineage, old.catalog_version);
        }
        self.inner().live.remove(id);
        self.change(|s| {
            s.installed.remove(&model.lineage);
            s.unassign(&model.lineage);
        })?;
        self.announce();
        Ok(self.snapshot(Some(&engine)))
    }

    /// Assigns `model` (installed, compatible) to `task`; `None` unassigns.
    ///
    /// # Errors
    /// Unknown model, [`InstallError::Incompatible`], [`InstallError::NotInstalled`], state write.
    pub fn assign(&self, task: Task, model: Option<&str>) -> Result<LocalAiState, InstallError> {
        let Some(id) = model else {
            self.change(|s| {
                s.assignments.remove(&task);
            })?;
            self.announce();
            return Ok(self.state());
        };
        let model = self.model(id)?;
        if !task.accepts(&model.capabilities) {
            return Err(InstallError::Incompatible(id.to_owned()));
        }
        let engine = self.engine()?;
        let state = self.inner().state.clone();
        let usable = Self::usable(Some(&engine), &state, model).is_some();
        if !usable {
            return Err(InstallError::NotInstalled(id.to_owned()));
        }
        self.change(|s| {
            s.assignments.insert(task, model.lineage.clone());
        })?;
        self.announce();
        Ok(self.snapshot(Some(&engine)))
    }

    /// Stores the Hugging Face token (trimmed; never returned).
    ///
    /// # Errors
    /// [`InstallError::EmptyToken`], no download service, credential store.
    pub fn set_token(&self, token: &str) -> Result<LocalAiState, InstallError> {
        let kit = self.kit.as_ref().ok_or(InstallError::NoDownloads)?;
        let token = token.trim();
        if token.is_empty() {
            return Err(InstallError::EmptyToken);
        }
        if token.len() > TOKEN_LIMIT {
            return Err(InstallError::TokenTooLong);
        }
        kit.secrets.set(&Source::HUGGING_FACE, Secret::new(token))?;
        self.token_known(true);
        self.announce();
        Ok(self.state())
    }

    /// Forgets the Hugging Face token.
    ///
    /// # Errors
    /// No download service, credential store.
    pub fn clear_token(&self) -> Result<LocalAiState, InstallError> {
        let kit = self.kit.as_ref().ok_or(InstallError::NoDownloads)?;
        kit.secrets.clear(&Source::HUGGING_FACE)?;
        self.token_known(false);
        self.announce();
        Ok(self.state())
    }

    /// The installed model assigned to `task` (AC-8), for PerMark's own features.
    #[must_use]
    pub fn model_for(&self, task: Task) -> ModelFor {
        let Some(lineage) = self.inner().state.assignments.get(&task).cloned() else {
            return ModelFor::NoModel;
        };
        self.catalog
            .models
            .iter()
            .filter(|m| m.lineage == lineage)
            .find_map(|m| self.installed_model(&m.id).ok())
            .map_or(ModelFor::NoModel, ModelFor::Assigned)
    }

    /// The version of `id`'s lineage usable now (catalog version on disk, else the recorded one on disk).
    ///
    /// # Errors
    /// Unknown model, [`InstallError::NotInstalled`], no download service | store unavailable.
    pub fn installed_model(&self, id: &str) -> Result<AssignedModel, InstallError> {
        let model = self.model(id)?;
        let engine = self.engine()?;
        let state = self.inner().state.clone();
        let installed = Self::usable(Some(&engine), &state, model)
            .ok_or_else(|| InstallError::NotInstalled(id.to_owned()))?;
        Ok(AssignedModel {
            dir: version_dir(engine.store(), &model.lineage, installed.catalog_version),
            id: installed.model,
            lineage: model.lineage.clone(),
            catalog_version: installed.catalog_version,
            files: installed.files,
        })
    }

    /// Catalog ids of the models assigned to tasks, from the state alone (no engine started, no disk read): the
    /// machine budget.
    #[must_use]
    pub fn assigned_ids(&self) -> Vec<String> {
        let inner = self.inner();
        inner
            .state
            .assignments
            .values()
            .filter_map(|lineage| {
                self.catalog
                    .models
                    .iter()
                    .find(|m| &m.lineage == lineage)
                    .map(|m| m.id.clone())
            })
            .collect()
    }

    /// Stops a store move (copies removed, old store kept) + every download, joins both (plugin off); parts kept.
    pub fn shutdown(&self) {
        if let Some(mover) = &self.inner().moving {
            mover.cancel.store(true, Ordering::Relaxed);
        }
        let mover = self
            .mover
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        // the move thread dropping the last handle: never join oneself
        if let Some(mover) = mover.filter(|h| h.thread().id() != std::thread::current().id()) {
            let _ = mover.join();
        }
        let engine = self.inner().engine.take();
        if let Some(engine) = engine {
            engine.shutdown();
        }
    }

    fn not_moving(&self) -> Result<(), InstallError> {
        let inner = self.inner();
        if inner.moving.is_some() || inner.changing {
            return Err(InstallError::Moving);
        }
        Ok(())
    }
}
