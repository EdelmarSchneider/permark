//! Events core emits; the shell forwards them to windows. A sink MUST NOT call back into the emitter (emits happen
//! under the emitter's lock, which keeps them in order).

use serde::Serialize;
use ts_rs::TS;

use crate::conditions::Context;
use crate::document::DocumentChanged;
use crate::settings::{AppSettings, SettingsScope};
use crate::tree::TreeChanged;
use crate::workspace::{WindowSummary, WindowView};

/// Every window with its tabs (each view shows its own window's).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WindowsChanged {
    pub windows: Vec<WindowSummary>,
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

/// An open that came from outside (command line, OS) failed; shown in `window`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OpenFailed {
    pub window: String,
    pub path: String,
    pub error: crate::error::IpcError,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum CoreEvent {
    WindowsChanged(WindowsChanged),
    /// To one window.
    WindowChanged(WindowChanged),
    /// To one window.
    ContextChanged(ContextChanged),
    SettingsChanged(SettingsChanged),
    StateRecovered(StateRecovered),
    /// To one window.
    OpenFailed(OpenFailed),
    /// To one window: its Explorer list changed.
    TreeChanged(TreeChanged),
    /// To one window: the document it shows changed on disk.
    DocumentChanged(DocumentChanged),
    /// The search index of a project made progress (all windows; the payload names the project).
    IndexProgress(crate::search::IndexProgress),
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
            Self::WindowsChanged(_) => "windows-changed",
            Self::WindowChanged(_) => "window-changed",
            Self::ContextChanged(_) => "context-changed",
            Self::SettingsChanged(_) => "settings-changed",
            Self::StateRecovered(_) => "state-recovered",
            Self::OpenFailed(_) => "open-failed",
            Self::TreeChanged(_) => "tree-changed",
            Self::DocumentChanged(_) => "document-changed",
            Self::IndexProgress(_) => "index-progress",
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
            Self::OpenFailed(e) => Some(&e.window),
            Self::TreeChanged(e) => Some(&e.window),
            Self::DocumentChanged(e) => Some(&e.window),
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
