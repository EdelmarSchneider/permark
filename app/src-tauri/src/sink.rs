//! Core events -> Tauri events (window-targeted ones carry their window label; frontends filter by it).

use permark_core::events::{CoreEvent, EventSink};
use tauri::{AppHandle, Emitter};

pub struct TauriSink {
    handle: AppHandle,
}

impl TauriSink {
    pub fn new(handle: AppHandle) -> Self {
        Self { handle }
    }
}

impl EventSink for TauriSink {
    fn emit(&self, event: CoreEvent) {
        let name = event.name().to_owned();
        let target = event.target().map(str::to_owned);
        let payload = match event {
            CoreEvent::WorkspacesChanged(e) => serde_json::to_value(e),
            CoreEvent::WindowChanged(e) => serde_json::to_value(e),
            CoreEvent::ContextChanged(e) => serde_json::to_value(e),
            CoreEvent::SettingsChanged(e) => serde_json::to_value(e),
            CoreEvent::StateRecovered(e) => serde_json::to_value(e),
            CoreEvent::OpenFailed(e) => serde_json::to_value(e),
            CoreEvent::TreeChanged(e) => serde_json::to_value(e),
            CoreEvent::PluginsChanged => Ok(serde_json::Value::Null),
            CoreEvent::PluginFailed { message } => Ok(serde_json::Value::String(message)),
            CoreEvent::Plugin { payload, .. } => Ok(payload),
        };
        let Ok(payload) = payload else {
            return;
        };
        // emit failures (window gone mid-emit) are not actionable
        let _ = match target {
            Some(label) => self.handle.emit_to(label.as_str(), &name, payload),
            None => self.handle.emit(&name, payload),
        };
    }
}
