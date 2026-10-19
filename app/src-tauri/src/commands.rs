//! IPC commands: typed adapters over core services. Frontend mirror: `app/src/api/`. `#[tauri::command(async)]`
//! -> runs off the main thread. Every command is listed once in [`commands!`] (handler + name list for the
//! contract test: `src/api/generated/CommandName.ts` must equal the frontend `Commands` keys).

// Tauri hands command arguments (handle, window, deserialized args) over by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use permark_core::conditions::{Context, Platform};
use permark_core::document::{editor, resolve_code_refs, DocumentView, LinkTarget};
use permark_core::events::{OpenFailed, StateRecovered};
use permark_core::fileops::{
    external_target, DeleteOutcome, MoveOutcome, MovePlan, MoveSpec, NewKind, OpenMode, UndoOutcome,
};
use permark_core::format::FormatRegistry;
use permark_core::paths::VaultPath;
use permark_core::session::{ProjectSession, RecentFolder};
use permark_core::settings::{AppSettings, SettingChange};
use permark_core::tree::{TreeFilter, TreeSnapshot};
use permark_core::workspace::{
    ProjectCore, WindowView, Workspace, WorkspaceId, WorkspaceRef, WorkspaceSummary,
};
use permark_core::{AppInfo, ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_opener::OpenerExt;
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
    /// Opens from outside that failed before this window could listen.
    pub failures: Vec<OpenFailed>,
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
        failures: shell.window_booted(&label),
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

/// The project the invoking window shows.
fn project_of(handle: &AppHandle, window: &WebviewWindow) -> Result<Arc<ProjectCore>, IpcError> {
    match state(handle).registry.workspace_of(window.label()) {
        Some((_, Workspace::Project(core))) => Ok(core),
        _ => Err(IpcError::new(ErrorKind::Unsupported).subject("project")),
    }
}

// ---- Explorer: tree ------------------------------------------------------------------------------------------------------

/// The window's tree: its filter (None: the project default) and the whole list. Later changes arrive as
/// `tree-changed` events.
#[tauri::command(async)]
pub fn tree_open(
    handle: AppHandle,
    window: WebviewWindow,
    filter: Option<TreeFilter>,
) -> Result<TreeSnapshot, IpcError> {
    Ok(project_of(&handle, &window)?
        .explorer()
        .open_view(window.label(), filter))
}

#[tauri::command(async)]
pub fn tree_set_filter(
    handle: AppHandle,
    window: WebviewWindow,
    filter: TreeFilter,
) -> Result<TreeSnapshot, IpcError> {
    Ok(project_of(&handle, &window)?
        .explorer()
        .set_filter(window.label(), filter)?)
}

/// Lists a folder left unopened (large | ignored).
#[tauri::command(async)]
pub fn tree_open_folder(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
) -> Result<(), IpcError> {
    Ok(project_of(&handle, &window)?
        .explorer()
        .open_folder(&path)?)
}

/// The open document stays in the tree even when the filter hides it.
#[tauri::command(async)]
pub fn tree_keep(
    handle: AppHandle,
    window: WebviewWindow,
    path: Option<String>,
) -> Result<(), IpcError> {
    Ok(project_of(&handle, &window)?
        .explorer()
        .keep(window.label(), path.as_deref())?)
}

#[tauri::command(async)]
pub fn tree_close(handle: AppHandle, window: WebviewWindow) {
    if let Ok(core) = project_of(&handle, &window) {
        core.explorer().close_view(window.label());
    }
}

// ---- Explorer: file operations -------------------------------------------------------------------------------------------

#[tauri::command(async)]
pub fn fileops_plan_move(
    handle: AppHandle,
    window: WebviewWindow,
    moves: Vec<MoveSpec>,
    update_links: bool,
) -> Result<MovePlan, IpcError> {
    Ok(project_of(&handle, &window)?
        .fileops()
        .plan_move(&moves, update_links)?)
}

#[tauri::command(async)]
pub fn fileops_apply_move(
    handle: AppHandle,
    window: WebviewWindow,
    plan_id: u64,
    update_links: bool,
) -> Result<MoveOutcome, IpcError> {
    Ok(project_of(&handle, &window)?
        .fileops()
        .apply_move(plan_id, update_links)?)
}

#[tauri::command(async)]
pub fn fileops_delete(
    handle: AppHandle,
    window: WebviewWindow,
    paths: Vec<String>,
) -> Result<DeleteOutcome, IpcError> {
    Ok(project_of(&handle, &window)?.fileops().delete(&paths)?)
}

/// A new empty file | folder; returns its project path.
#[tauri::command(async)]
pub fn fileops_new(
    handle: AppHandle,
    window: WebviewWindow,
    dir: String,
    name: String,
    kind: NewKind,
) -> Result<String, IpcError> {
    Ok(project_of(&handle, &window)?
        .fileops()
        .create(&dir, &name, kind)?)
}

/// Takes the newest operation back; `None`: nothing to undo.
#[tauri::command(async)]
pub fn fileops_undo(
    handle: AppHandle,
    window: WebviewWindow,
) -> Result<Option<UndoOutcome>, IpcError> {
    Ok(project_of(&handle, &window)?.fileops().undo()?)
}

// ---- Explorer: files outside PerMark -------------------------------------------------------------------------------------

/// Absolute place of a project path ("copy full path").
#[tauri::command(async)]
pub fn file_abs_path(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
) -> Result<String, IpcError> {
    let core = project_of(&handle, &window)?;
    let parsed = VaultPath::parse(&path).map_err(IpcError::from)?;
    let abs = core.vault().resolve(&parsed).map_err(IpcError::from)?;
    Ok(abs.display().to_string())
}

/// Default program | browser | file manager, for a file of the project: never an executable | script.
#[tauri::command(async)]
pub fn file_open_external(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
    mode: OpenMode,
) -> Result<(), IpcError> {
    let core = project_of(&handle, &window)?;
    // the vault checks the path and the kind of file; the opener only ever sees the validated absolute path
    let abs = external_target(core.vault(), &path, mode)?;
    let opener = handle.opener();
    let done = match mode {
        OpenMode::Reveal => opener.reveal_item_in_dir(&abs),
        OpenMode::DefaultApp | OpenMode::Browser => {
            opener.open_path(abs.to_string_lossy(), None::<&str>)
        }
    };
    done.map_err(|e| {
        IpcError::new(ErrorKind::Io)
            .subject(path)
            .detail(e.to_string())
    })
}

/// The window shows this document (None: none): the hint other features follow.
#[tauri::command(async)]
pub fn show_document(
    handle: AppHandle,
    window: WebviewWindow,
    path: Option<String>,
) -> Result<(), IpcError> {
    let core = project_of(&handle, &window)?;
    let parsed = path
        .as_deref()
        .map(VaultPath::parse)
        .transpose()
        .map_err(IpcError::from)?;
    // without live updates the open document is polled
    let abs = parsed.as_ref().and_then(|p| core.vault().resolve(p).ok());
    core.watcher().poll_document(window.label(), abs.as_deref());
    state(&handle).registry.set_document(window.label(), parsed);
    Ok(())
}

// ---- Reading: documents ---------------------------------------------------------------------------------------------

/// The workspace the invoking window shows, with the formats its plugins give.
fn reading_of(
    handle: &AppHandle,
    window: &WebviewWindow,
) -> Result<(Workspace, Arc<FormatRegistry>), IpcError> {
    let shell = state(handle);
    let (_, workspace) = shell
        .registry
        .workspace_of(window.label())
        .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).subject("document"))?;
    let formats = match &workspace {
        Workspace::Project(core) => core.formats(),
        Workspace::SingleFile(_) => shell.env.formats(),
    };
    Ok((workspace, formats))
}

fn documents_of(workspace: &Workspace) -> &Arc<permark_core::document::Documents> {
    match workspace {
        Workspace::Project(core) => core.documents(),
        Workspace::SingleFile(file) => &file.documents,
    }
}

/// Reads and renders a document for this window and remembers it as the one shown. `known`: ids of the blocks the
/// window holds (their HTML is not sent again).
#[tauri::command(async)]
pub fn document_open(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
    known: Vec<String>,
) -> Result<DocumentView, IpcError> {
    let (workspace, formats) = reading_of(&handle, &window)?;
    let view = documents_of(&workspace).open(window.label(), &path, &known, &formats)?;
    if let Workspace::Project(core) = &workspace {
        // without live updates the open document is polled
        let abs = VaultPath::parse(&path)
            .ok()
            .and_then(|parsed| core.vault().resolve(&parsed).ok());
        core.watcher().poll_document(window.label(), abs.as_deref());
    }
    Ok(view)
}

/// The window shows no document any more.
#[tauri::command(async)]
pub fn document_close(handle: AppHandle, window: WebviewWindow) {
    if let Some((_, workspace)) = state(&handle).registry.workspace_of(window.label()) {
        documents_of(&workspace).close(window.label());
        if let Workspace::Project(core) = &workspace {
            core.watcher().poll_document(window.label(), None);
        }
    }
}

/// The text of a document as read ("copy document").
#[tauri::command(async)]
pub fn document_text(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
) -> Result<String, IpcError> {
    let (workspace, _) = reading_of(&handle, &window)?;
    documents_of(&workspace).text(&path)
}

/// Where each link of a document leads.
#[tauri::command(async)]
pub fn link_resolve(
    handle: AppHandle,
    window: WebviewWindow,
    doc: String,
    hrefs: Vec<String>,
) -> Result<Vec<LinkTarget>, IpcError> {
    let (workspace, _) = reading_of(&handle, &window)?;
    Ok(documents_of(&workspace).resolve_links(&doc, &hrefs))
}

/// The project files each written code reference can mean (single file: none).
#[tauri::command(async)]
pub fn code_refs_resolve(
    handle: AppHandle,
    window: WebviewWindow,
    doc: String,
    written: Vec<String>,
) -> Result<Vec<Vec<String>>, IpcError> {
    let (workspace, _) = reading_of(&handle, &window)?;
    Ok(match workspace {
        Workspace::Project(core) => resolve_code_refs(core.explorer(), &doc, &written),
        Workspace::SingleFile(_) => written.iter().map(|_| Vec::new()).collect(),
    })
}

/// Opens a file of the workspace in the person's editor (setting `editor`) at a line. The path is checked against the
/// workspace; the program and its arguments come from the setting only.
#[tauri::command(async)]
pub fn open_in_editor(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
    line: Option<u32>,
    col: Option<u32>,
) -> Result<(), IpcError> {
    let (workspace, _) = reading_of(&handle, &window)?;
    let (_, abs) = documents_of(&workspace).scope().resolve(&path)?;
    let template = state(&handle).env.settings().editor;
    editor::launch(&template, &abs, line.unwrap_or(1), col.unwrap_or(1))?;
    Ok(())
}

/// Opens a web link in the system browser. Only `http`, `https` and `mailto`.
#[tauri::command(async)]
pub fn open_web(handle: AppHandle, url: String) -> Result<(), IpcError> {
    if !web_url_allowed(&url) {
        return Err(IpcError::new(ErrorKind::Unsupported).subject(url));
    }
    handle
        .opener()
        .open_url(url.clone(), None::<&str>)
        .map_err(|e| {
            IpcError::new(ErrorKind::Io)
                .subject(url)
                .detail(e.to_string())
        })
}

/// Web links the browser may be given: a known scheme, no control characters, a sane length.
fn web_url_allowed(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    url.len() <= 4096
        && !url.chars().any(char::is_control)
        && (lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("mailto:"))
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
    tree_open,
    tree_set_filter,
    tree_open_folder,
    tree_keep,
    tree_close,
    fileops_plan_move,
    fileops_apply_move,
    fileops_delete,
    fileops_new,
    fileops_undo,
    file_abs_path,
    file_open_external,
    show_document,
    document_open,
    document_close,
    document_text,
    link_resolve,
    code_refs_resolve,
    open_in_editor,
    open_web,
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
    fn web_links_are_limited_to_known_schemes() {
        for good in [
            "https://example.com/a?b=c#d",
            "HTTP://x.org",
            "mailto:me@example.com",
        ] {
            assert!(web_url_allowed(good), "{good}");
        }
        for bad in [
            "javascript:alert(1)",
            "file:///C:/tools/run.cmd",
            "custom-handler:/id",
            "C:\\tool.cmd",
            "https://a.com/\n",
            "  vbscript:x",
            "data:text/html,x",
            "",
            "tel:123",
        ] {
            assert!(!web_url_allowed(bad), "{bad}");
        }
        assert!(!web_url_allowed(&format!(
            "https://a.com/{}",
            "x".repeat(5000)
        )));
    }

    #[test]
    fn placement_wire_format() {
        let p: OpenPlacement = serde_json::from_str("\"newWindow\"").unwrap();
        assert_eq!(p, OpenPlacement::NewWindow);
        assert!(serde_json::from_str::<OpenPlacement>("\"elsewhere\"").is_err());
    }
}
