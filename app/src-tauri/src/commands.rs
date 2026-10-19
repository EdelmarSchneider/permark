//! IPC commands: typed adapters over core services. Frontend mirror: `app/src/api/`. `#[tauri::command(async)]`
//! -> runs off the main thread. Every command is listed once in [`commands!`] (handler + name list for the
//! contract test: `src/api/generated/CommandName.ts` must equal the frontend `Commands` keys).

// Tauri hands command arguments (handle, window, deserialized args) over by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::PathBuf;
use std::sync::Mutex;

use permark_core::conditions::{Context, Platform};
use permark_core::events::StateRecovered;
use permark_core::session::{ProjectSession, RecentFolder};
use permark_core::settings::{AppSettings, SettingChange};
use permark_core::workspace::{WindowView, Workspace, WorkspaceId, WorkspaceRef, WorkspaceSummary};
use permark_core::{AppInfo, ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, WebviewWindow};
use ts_rs::TS;

use crate::open::{open_in, open_in_new_window};
use crate::shell::state;

/// What a window needs to draw its first frame.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BootInfo {
    pub window: String,
    pub view: WindowView,
    pub context: Context,
    pub settings: AppSettings,
    pub notices: Vec<StateRecovered>,
    pub platform: Platform,
}

/// Where an open from the UI goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OpenPlacement {
    ThisWindow,
    NewWindow,
    /// This window if it shows nothing, else the UI asks first (`NeedsChoice`).
    Ask,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "outcome", rename_all = "camelCase")]
#[ts(export)]
pub enum OpenOutcome {
    Opened { view: WindowView },
    NeedsChoice,
    OpenedElsewhere { window: String },
}

/// Serializes project session writes (two windows on one project).
static PROJECT_SESSION: Mutex<()> = Mutex::new(());

#[tauri::command(async)]
pub fn app_info() -> AppInfo {
    permark_core::app_info()
}

#[tauri::command(async)]
pub fn window_boot(handle: AppHandle, window: WebviewWindow) -> BootInfo {
    let shell = state(&handle);
    let label = window.label().to_owned();
    shell.registry.add_window(&label);
    BootInfo {
        view: shell
            .registry
            .window_view(&label)
            .unwrap_or(WindowView::Welcome),
        context: shell
            .registry
            .context_of(&label)
            .unwrap_or_else(|| Context::welcome(shell.env.distribution)),
        settings: shell.env.settings(),
        notices: shell.take_notices(),
        platform: Platform::current(),
        window: label,
    }
}

#[tauri::command(async)]
pub fn open_path(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
    placement: OpenPlacement,
) -> Result<OpenOutcome, IpcError> {
    let shell = state(&handle);
    let label = window.label().to_owned();
    let reference = WorkspaceRef::classify(&PathBuf::from(&path))?;
    let shown = shell.registry.workspace_of(&label);
    let placement = match placement {
        OpenPlacement::Ask if shown.is_some() => return Ok(OpenOutcome::NeedsChoice),
        OpenPlacement::Ask => OpenPlacement::ThisWindow,
        other => other,
    };
    if placement == OpenPlacement::NewWindow {
        let window = open_in_new_window(&handle, &reference, None)
            .ok_or_else(|| IpcError::new(ErrorKind::Internal).subject("window"))?;
        return Ok(OpenOutcome::OpenedElsewhere { window });
    }
    // a file inside this window's project is shown in it
    if let (WorkspaceRef::SingleFile(file), Some((_, Workspace::Project(core)))) =
        (&reference, &shown)
    {
        if let Some(document) = core.vault().relativize(file) {
            shell.registry.set_document(&label, Some(document));
            return Ok(opened(&shell, &label));
        }
    }
    open_in(&handle, &label, &reference, None, false)?;
    Ok(opened(&shell, &label))
}

fn opened(shell: &crate::Shell, label: &str) -> OpenOutcome {
    OpenOutcome::Opened {
        view: shell
            .registry
            .window_view(label)
            .unwrap_or(WindowView::Welcome),
    }
}

#[tauri::command(async)]
pub fn show_welcome(handle: AppHandle, window: WebviewWindow) {
    let shell = state(&handle);
    shell.registry.show_welcome(window.label());
    shell.saver.poke();
}

#[tauri::command(async)]
pub fn list_workspaces(handle: AppHandle) -> Vec<WorkspaceSummary> {
    state(&handle).registry.list()
}

#[tauri::command(async)]
pub fn focus_workspace(handle: AppHandle, id: WorkspaceId) -> Result<(), IpcError> {
    let window = state(&handle)
        .registry
        .windows_of(id)
        .into_iter()
        .next()
        .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(id.0.to_string()))?;
    crate::windows::focus(&handle, &window);
    Ok(())
}

#[tauri::command(async)]
pub fn recent_list(handle: AppHandle) -> Vec<RecentFolder> {
    state(&handle).recent.list()
}

#[tauri::command(async)]
pub fn recent_remove(handle: AppHandle, path: String) -> Result<(), IpcError> {
    Ok(state(&handle).recent.remove(&path)?)
}

#[tauri::command(async)]
pub fn settings_get(handle: AppHandle) -> AppSettings {
    state(&handle).env.settings()
}

#[tauri::command(async)]
pub fn settings_set(handle: AppHandle, change: SettingChange) -> Result<AppSettings, IpcError> {
    Ok(state(&handle).change_setting(&change)?)
}

fn project_session(handle: &AppHandle, label: &str) -> Result<ProjectSession, IpcError> {
    match state(handle).registry.workspace_of(label) {
        Some((_, Workspace::Project(core))) => Ok(ProjectSession::new(core.store())),
        _ => Err(IpcError::new(ErrorKind::Unsupported).subject("project session")),
    }
}

#[tauri::command(async)]
pub fn project_session_get(
    handle: AppHandle,
    window: WebviewWindow,
    key: String,
) -> Result<Option<serde_json::Value>, IpcError> {
    Ok(project_session(&handle, window.label())?.get(&key)?)
}

#[tauri::command(async)]
pub fn project_session_set(
    handle: AppHandle,
    window: WebviewWindow,
    key: String,
    value: Option<serde_json::Value>,
) -> Result<(), IpcError> {
    let session = project_session(&handle, window.label())?;
    let _guard = PROJECT_SESSION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(session.set(&key, value)?)
}

/// Lists every command once: Tauri handler + names (contract test).
macro_rules! commands {
    ($($name:ident),+ $(,)?) => {
        pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($name),+]
        }

        #[cfg(test)]
        pub const NAMES: &[&str] = &[$(stringify!($name)),+];
    };
}

commands![
    app_info,
    window_boot,
    open_path,
    show_welcome,
    list_workspaces,
    focus_workspace,
    recent_list,
    recent_remove,
    settings_get,
    settings_set,
    project_session_get,
    project_session_set,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_adapts_core() {
        assert_eq!(app_info(), permark_core::app_info());
    }

    /// Writes `CommandName.ts`: the frontend `Commands` map must have exactly these keys (checked by `tsc`).
    #[test]
    fn export_command_names() {
        let union = NAMES
            .iter()
            .map(|n| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(" | ");
        let text = format!(
            "// Generated by `cargo test` (app crate, commands.rs): every registered IPC command. Do not edit.\n\nexport type CommandName = {union};\n"
        );
        let dir = std::path::Path::new(env!("TS_RS_EXPORT_DIR"));
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("CommandName.ts");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(path, text).unwrap();
        }
        let unique: std::collections::BTreeSet<_> = NAMES.iter().collect();
        assert_eq!(unique.len(), NAMES.len(), "duplicate command");
    }

    #[test]
    fn placement_wire_format() {
        let p: OpenPlacement = serde_json::from_str("\"newWindow\"").unwrap();
        assert_eq!(p, OpenPlacement::NewWindow);
        assert!(serde_json::from_str::<OpenPlacement>("\"elsewhere\"").is_err());
    }
}
