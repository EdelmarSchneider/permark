//! Core <-> plugin boundary (deliberate design, see directives §2 exception): identity, manifest, lifecycle.
//! Plugins get core services through [`PluginCx`] only; a running plugin = [`Running`] whose `Drop` tears it down.

mod enable;
mod registry;
mod set;

use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use enable::{on_ids, resolve_enabled, EnableInputs, Enablement, OffReason};
pub use registry::{PluginRegistry, RegistryError};
pub use set::PluginSet;

use crate::conditions::{Context, Requirement};
use crate::events::{CoreEvent, EventSink};
use crate::paths::Vault;

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
}

/// What an instance works on.
#[derive(Debug, Clone)]
pub enum CxWorkspace {
    App,
    Project(Vault),
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
        };
        Self {
            id,
            workspace,
            state_dir,
            settings,
            context,
            events,
            watch: None,
        }
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

/// Event sink of one plugin: every name prefixed `<id>:`.
#[derive(Clone)]
pub struct PluginEvents {
    id: PluginId,
    sink: Arc<dyn EventSink>,
}

impl PluginEvents {
    pub fn emit(&self, name: &str, payload: serde_json::Value) {
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
