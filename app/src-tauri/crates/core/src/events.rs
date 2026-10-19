//! Events core emits; the shell forwards them to windows. A sink MUST NOT call back into the emitter (emits happen
//! under the emitter's lock, which keeps them in order).

use serde::Serialize;
use ts_rs::TS;

use crate::conditions::Context;
use crate::settings::{AppSettings, SettingsScope};
use crate::workspace::{WindowView, WorkspaceSummary};

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkspacesChanged {
    pub workspaces: Vec<WorkspaceSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WindowChanged {
    pub window: String,
    pub view: WindowView,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContextChanged {
    pub window: String,
    pub context: Context,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SettingsChanged {
    pub scope: SettingsScope,
    pub keys: Vec<String>,
    pub settings: AppSettings,
}

/// A state file was unreadable and moved aside; the UI shows a quiet notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StateRecovered {
    pub file: String,
    pub backup: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum CoreEvent {
    WorkspacesChanged(WorkspacesChanged),
    /// To one window.
    WindowChanged(WindowChanged),
    /// To one window.
    ContextChanged(ContextChanged),
    SettingsChanged(SettingsChanged),
    StateRecovered(StateRecovered),
    PluginsChanged,
    PluginFailed {
        message: String,
    },
    /// Plugin event: name already prefixed `<id>:`.
    Plugin {
        name: String,
        payload: serde_json::Value,
    },
}

impl CoreEvent {
    /// Event name on the wire (kebab-case).
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::WorkspacesChanged(_) => "workspaces-changed",
            Self::WindowChanged(_) => "window-changed",
            Self::ContextChanged(_) => "context-changed",
            Self::SettingsChanged(_) => "settings-changed",
            Self::StateRecovered(_) => "state-recovered",
            Self::PluginsChanged => "plugins-changed",
            Self::PluginFailed { .. } => "plugin-failed",
            Self::Plugin { name, .. } => name,
        }
    }

    /// Window label when the event is for one window only.
    #[must_use]
    pub fn target(&self) -> Option<&str> {
        match self {
            Self::WindowChanged(e) => Some(&e.window),
            Self::ContextChanged(e) => Some(&e.window),
            _ => None,
        }
    }
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: CoreEvent);
}

/// Sink that drops everything (tools, tests that do not look).
#[derive(Debug, Default)]
pub struct NoEvents;

impl EventSink for NoEvents {
    fn emit(&self, _: CoreEvent) {}
}

/// Test sink: records events.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct Recorder(std::sync::Mutex<Vec<CoreEvent>>);

#[cfg(test)]
impl Recorder {
    pub(crate) fn take(&self) -> Vec<CoreEvent> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

#[cfg(test)]
impl EventSink for Recorder {
    fn emit(&self, event: CoreEvent) {
        self.0.lock().unwrap().push(event);
    }
}
