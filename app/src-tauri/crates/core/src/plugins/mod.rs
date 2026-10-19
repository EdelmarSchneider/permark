//! Core <-> plugin boundary (deliberate design, see directives §2 exception): identity, manifest, lifecycle.
//! Plugins get core services through [`PluginCx`] only; a running plugin = [`Running`] whose `Drop` tears it down.

mod enable;
mod facts;
mod registry;
mod rpc;
mod services;
mod set;

use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use enable::{on_ids, resolve_enabled, EnableInputs, Enablement, OffReason};
pub use facts::{ProjectFacts, RepoHandle, RepoReader};
pub use registry::{PluginRegistry, RegistryError};
pub use rpc::{typed, NoArgs, RpcCall, RpcHandler, RpcRegistration, RpcTable};
pub use services::{ServiceLease, Services};
pub use set::PluginSet;

use crate::conditions::{Context, Requirement};
use crate::document::Documents;
use crate::edit::Editor;
use crate::events::{CoreEvent, EventSink};
use crate::format::{Contributions, FormatRegistry};
use crate::paths::{Root, Vault};
use crate::state::{StateData, StateFile, StateStore};
use crate::tree::Explorer;
use crate::watch::OwnWrites;

/// Kebab-case plugin id: `[a-z0-9]+(-[a-z0-9]+)*`, <= 32 chars.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
#[ts(type = "string")]
pub struct PluginId(Cow<'static, str>);

/// Parse, don't validate: an invalid id never deserializes.
impl<'de> Deserialize<'de> for PluginId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if is_valid_id(&text) {
            Ok(Self(Cow::Owned(text)))
        } else {
            Err(serde::de::Error::custom(format!(
                "invalid plugin id {text:?}"
            )))
        }
    }
}

impl PluginId {
    /// Const id for manifests (validated by `PluginRegistry::new`).
    #[must_use]
    pub const fn of(id: &'static str) -> Self {
        Self(Cow::Borrowed(id))
    }

    /// Parses an id from outside (settings, IPC).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        is_valid_id(text).then(|| Self(Cow::Owned(text.to_owned())))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub(crate) fn is_valid_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 32
        && !text.starts_with('-')
        && !text.ends_with('-')
        && !text.contains("--")
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PluginKind {
    Format,
    Role,
    Analysis,
    Checker,
    Feature,
    Embed,
}

/// App: one instance per process. Project: one instance per open project (`ProjectCore`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PluginScope {
    App,
    Project,
}

/// Static description of a plugin (const per crate).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Manifest {
    pub id: PluginId,
    pub kind: PluginKind,
    pub scope: PluginScope,
    pub requires: Requirement,
    pub depends_on: Cow<'static, [PluginId]>,
    pub default_enabled: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin {id} failed to start: {reason}")]
    Start { id: PluginId, reason: String },
}

/// A started plugin. `Drop` = teardown: threads joined, watchers removed, sockets closed (RAII).
pub trait Running: Send {}

/// A plugin implementation (one per plugin crate).
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &Manifest;
    /// Starts one instance (scope Project: per `ProjectCore`; App: once).
    ///
    /// # Errors
    /// The plugin cannot run (it + its dependents stay off; others run).
    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError>;
    /// Data this plugin adds to core registries while it is on (formats, roles, Markdown features).
    fn contributions(&self) -> Contributions {
        Contributions::default()
    }
    /// Extra roots the plugin offers for the project at `root` (`(name, folder)`, like linked worktrees), asked
    /// before the project is built and before `start`. Must be quick (file reads only).
    fn roots(&self, _root: &Root) -> Vec<(String, PathBuf)> {
        Vec::new()
    }
}

/// A running instance that does nothing: the plugin's work happens through its contributions.
struct Idle;
impl Running for Idle {}

/// A plugin with no process of its own: a manifest and the data it contributes (viewers, roles, embeds that live
/// in the UI). Dropping its instance has nothing to tear down.
pub struct StaticPlugin {
    manifest: Manifest,
    contributions: Contributions,
}

impl StaticPlugin {
    #[must_use]
    pub fn new(manifest: Manifest, contributions: Contributions) -> Arc<Self> {
        Arc::new(Self {
            manifest,
            contributions,
        })
    }

    /// An app-scope plugin that is on by default, with no requirement and no dependency.
    #[must_use]
    pub fn app(
        id: &'static str,
        kind: PluginKind,
        contributions: Contributions,
    ) -> Arc<dyn Plugin> {
        Self::new(
            Manifest {
                id: PluginId::of(id),
                kind,
                scope: PluginScope::App,
                requires: Requirement::NONE,
                depends_on: Cow::Borrowed(&[]),
                default_enabled: true,
            },
            contributions,
        )
    }
}

impl Plugin for StaticPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, _cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        Ok(Box::new(Idle))
    }

    fn contributions(&self) -> Contributions {
        self.contributions.clone()
    }
}

/// What an instance works on.
#[derive(Debug, Clone)]
pub enum CxWorkspace {
    App,
    Project(Vault),
}

/// The project services a Project-scope plugin may use besides the file watch.
#[derive(Clone)]
pub struct ProjectServices {
    /// Reads and renders documents of the project.
    pub documents: Arc<Documents>,
    /// The tree service (document scans).
    pub explorer: Arc<Explorer>,
    /// Announcements of the app's own writes (the watch marks them `own`).
    pub own: Arc<OwnWrites>,
    /// What plugins publish about the project (git condition).
    pub facts: Arc<ProjectFacts>,
    /// Typed APIs plugins hand to each other (one registry per project).
    pub services: Arc<Services>,
    rpc: Arc<RpcTable>,
    formats: Arc<RwLock<Arc<FormatRegistry>>>,
}

impl ProjectServices {
    #[must_use]
    pub fn new(
        documents: Arc<Documents>,
        explorer: Arc<Explorer>,
        own: Arc<OwnWrites>,
        facts: Arc<ProjectFacts>,
        services: Arc<Services>,
        rpc: Arc<RpcTable>,
        formats: Arc<RwLock<Arc<FormatRegistry>>>,
    ) -> Self {
        Self {
            documents,
            explorer,
            own,
            facts,
            services,
            rpc,
            formats,
        }
    }

    /// The formats of the plugins that are on for the project, now.
    #[must_use]
    pub fn formats(&self) -> Arc<FormatRegistry> {
        Arc::clone(&self.formats.read().unwrap_or_else(PoisonError::into_inner))
    }

    #[must_use]
    pub fn rpc(&self) -> &Arc<RpcTable> {
        &self.rpc
    }

    /// Runs `f` with an editor over the project (surgical saves; writes announced as the app's own).
    pub fn with_editor<R>(&self, f: impl FnOnce(&Editor<'_>) -> R) -> R {
        let formats = self.formats();
        let editor = Editor {
            scope: self.documents.scope(),
            formats: &formats,
            own: Some(&self.own),
        };
        f(&editor)
    }
}

/// Core services handed to a plugin instance; nothing else of core is reachable from a plugin.
pub struct PluginCx {
    pub id: PluginId,
    pub workspace: CxWorkspace,
    /// Own namespace folder (created on first write through `StateStore::plugin_file`).
    pub state_dir: PathBuf,
    /// Own raw `[plugins.<id>]` table (app settings, project config overrides keys).
    pub settings: Option<toml_edit::Table>,
    /// Context when the instance started.
    pub context: Context,
    events: PluginEvents,
    watch: Option<Arc<crate::watch::WatchHub>>,
    project: Option<ProjectServices>,
    store: Option<StateStore>,
}

impl PluginCx {
    #[must_use]
    pub fn new(
        id: PluginId,
        workspace: CxWorkspace,
        state_dir: PathBuf,
        settings: Option<toml_edit::Table>,
        context: Context,
        sink: Arc<dyn EventSink>,
    ) -> Self {
        let events = PluginEvents {
            id: id.clone(),
            sink,
            root: None,
        };
        Self {
            id,
            workspace,
            state_dir,
            settings,
            context,
            events,
            watch: None,
            project: None,
            store: None,
        }
    }

    /// Gives the instance the state store its files live in (own namespace: `state_file`).
    #[must_use]
    pub fn with_store(mut self, store: StateStore) -> Self {
        self.store = Some(store);
        self
    }

    /// One typed state file in the plugin's own namespace (`None`: no store was given).
    #[must_use]
    pub fn state_file<T: StateData>(&self, name: &str) -> Option<StateFile<T>> {
        self.store
            .as_ref()
            .map(|store| store.plugin_file(&self.id, name))
    }

    /// Gives a Project-scope instance its project services; its events then carry the project folder.
    #[must_use]
    pub fn with_project(mut self, services: ProjectServices) -> Self {
        if let CxWorkspace::Project(vault) = &self.workspace {
            self.events.root = Some(vault.main().path().display().to_string());
        }
        self.project = Some(services);
        self
    }

    /// The project services (Project scope).
    #[must_use]
    pub fn project(&self) -> Option<&ProjectServices> {
        self.project.as_ref()
    }

    /// Serves `method` of this plugin until the registration is dropped (Project scope; `None` otherwise).
    #[must_use]
    pub fn rpc(&self, method: &str, handler: RpcHandler) -> Option<RpcRegistration> {
        self.project
            .as_ref()
            .map(|project| project.rpc.register(&self.id, method, handler))
    }

    /// Gives a Project-scope instance the file watch of its project.
    #[must_use]
    pub fn with_watch(mut self, hub: Arc<crate::watch::WatchHub>) -> Self {
        self.watch = Some(hub);
        self
    }

    /// The file watch of the project (Project scope): `subscribe` to hear verified changes; drop the subscription
    /// to stop.
    #[must_use]
    pub fn watch(&self) -> Option<&Arc<crate::watch::WatchHub>> {
        self.watch.as_ref()
    }

    #[must_use]
    pub fn events(&self) -> &PluginEvents {
        &self.events
    }
}

/// Event sink of one plugin: every name prefixed `<id>:`. A Project plugin's payload is `{ root, data }`: windows
/// keep the events of the folder they show.
#[derive(Clone)]
pub struct PluginEvents {
    id: PluginId,
    sink: Arc<dyn EventSink>,
    root: Option<String>,
}

impl PluginEvents {
    pub fn emit(&self, name: &str, payload: serde_json::Value) {
        let payload = match &self.root {
            Some(root) => serde_json::json!({ "root": root, "data": payload }),
            None => payload,
        };
        self.sink.emit(CoreEvent::Plugin {
            name: format!("{}:{name}", self.id),
            payload,
        });
    }
}

#[cfg(test)]
pub(crate) mod fake;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_syntax() {
        for good in ["git", "broken-links", "fmt-toml", "a1", "x"] {
            assert!(PluginId::parse(good).is_some(), "{good}");
        }
        for bad in [
            "",
            "Git",
            "-git",
            "git-",
            "a--b",
            "a_b",
            "a b",
            "ä",
            &"a".repeat(33),
        ] {
            assert!(PluginId::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn plugin_events_are_prefixed() {
        let sink = Arc::new(crate::events::Recorder::default());
        let cx = PluginCx::new(
            PluginId::of("git"),
            CxWorkspace::App,
            PathBuf::new(),
            None,
            Context::welcome(crate::conditions::Distribution::Direct),
            sink.clone(),
        );
        cx.events().emit("changed", serde_json::json!(1));
        let events = sink.take();
        assert!(matches!(&events[..], [CoreEvent::Plugin { name, .. }] if name == "git:changed"));
    }
}
