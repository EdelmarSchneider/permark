//! IPC commands: typed adapters over core services. Frontend mirror: `app/src/api/`. `#[tauri::command(async)]`
//! -> runs off the main thread. Every command is listed once in [`commands!`] (handler + name list for the
//! contract test: `src/api/generated/CommandName.ts` must equal the frontend `Commands` keys).

// Tauri hands command arguments (handle, window, deserialized args) over by value.
#![allow(clippy::needless_pass_by_value)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use permark_core::conditions::{Context, Platform};
use permark_core::document::{editor, resolve_code_refs, DocumentView, LinkTarget};
use permark_core::edit::{ConfigEdited, EditSource, Editor, LineEdit, SaveOutcome, SecretRange};
use permark_core::events::{OpenFailed, StateRecovered};
use permark_core::fileops::{
    external_target, DeleteOutcome, MoveOutcome, MovePlan, MoveSpec, NewKind, OpenMode, UndoOutcome,
};
use permark_core::format::config::IgnoreMatches;
use permark_core::format::{
    Body, ConfigEdit, FormatRegistry, Measure, OutlineItem, Segment, TextKind,
};
use permark_core::paths::VaultPath;
use permark_core::plugins::{Enablement, Manifest, PluginId, PluginScope, RpcCall};
use permark_core::search::{IndexStatus, PathResults, Recent, SearchOptions, SectionResults};
use permark_core::session::{ProjectSession, RecentFolder};
use permark_core::settings::{
    AppSettings, ProjectConfigFile, ProjectSettingChange, ProjectSettings, SettingChange,
};
use permark_core::tree::{TreeFilter, TreeSnapshot};
use permark_core::workspace::{
    ProjectCore, WindowView, Workspace, WorkspaceId, WorkspaceRef, WorkspaceSummary,
};
use permark_core::{AppInfo, ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, WebviewWindow};
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

/// Event `close-requested`: the window was asked to close while it holds unsaved edits (the close waits).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CloseRequested {
    pub window: String,
    /// The document with the unsaved edits.
    pub path: String,
}

/// Event `workspace-close-requested`: a window that shows a workspace is asked to close it (it runs its unsaved-changes
/// prompt, then goes to Welcome; no window is closed).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkspaceCloseRequested {
    pub window: String,
}

/// One plugin as the Plugins page shows it, for the invoking window.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginInfo {
    pub manifest: Manifest,
    /// The app-wide switch (the setting, else the manifest default).
    pub app_on: bool,
    /// On | off for this window: the open project's resolution (overrides included), else the app's.
    pub state: Enablement,
    /// The person's own choice for this project (profile); `None` = follows the app.
    pub project_choice: Option<bool>,
    /// The project's `.permark/config.toml` turns it off.
    pub project_config_off: bool,
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
    let label = window.label().to_owned();
    let reference = WorkspaceRef::classify(&PathBuf::from(&path))?;
    open_reference(&handle, &label, reference, placement)
}

/// Opens `reference` in the window `label`, in a new window, or asks which (`placement`).
fn open_reference(
    handle: &AppHandle,
    label: &str,
    reference: WorkspaceRef,
    placement: OpenPlacement,
) -> Result<OpenOutcome, IpcError> {
    let shell = state(handle);
    let shown = shell.registry.workspace_of(label);
    let placement = match placement {
        OpenPlacement::Ask if shown.is_some() => return Ok(OpenOutcome::NeedsChoice),
        OpenPlacement::Ask => OpenPlacement::ThisWindow,
        other => other,
    };
    if placement == OpenPlacement::NewWindow {
        let window = open_in_new_window(handle, &reference, None)
            .ok_or_else(|| IpcError::new(ErrorKind::Internal).subject("window"))?;
        return Ok(OpenOutcome::OpenedElsewhere { window });
    }
    // a file inside this window's project is shown in it
    if let (WorkspaceRef::SingleFile(file), Some((_, Workspace::Project(core)))) =
        (&reference, &shown)
    {
        if let Some(document) = core.vault().relativize(file) {
            shell.registry.set_document(label, Some(document));
            return Ok(opened(&shell, label));
        }
    }
    open_in(handle, label, &reference, None, false)?;
    Ok(opened(&shell, label))
}

/// Opens the Notes in this window (or a new one, or asks first when this window shows something).
#[tauri::command(async)]
pub fn open_notes(
    handle: AppHandle,
    window: WebviewWindow,
    placement: OpenPlacement,
) -> Result<OpenOutcome, IpcError> {
    let shell = state(&handle);
    let label = window.label().to_owned();
    // already here: nothing to do (unless a window of its own was asked for)
    if placement != OpenPlacement::NewWindow {
        if let Some((_, Workspace::Notes(_))) = shell.registry.workspace_of(&label) {
            return Ok(opened(&shell, &label));
        }
    }
    open_reference(&handle, &label, WorkspaceRef::Notes, placement)
}

/// Opens the project that holds `path` in a window of its own, showing that file: the deepest project opened lately
/// that contains it, else the file's folder.
#[tauri::command(async)]
pub fn open_in_project(handle: AppHandle, path: String) -> Result<(), IpcError> {
    let shell = state(&handle);
    let file = PathBuf::from(&path);
    if !file.is_file() {
        return Err(IpcError::new(ErrorKind::NotFound).subject(path));
    }
    let mut best: Option<(usize, permark_core::paths::Root, VaultPath)> = None;
    for recent in shell.recent.list().into_iter().filter(|r| !r.missing) {
        let Ok(root) = permark_core::paths::Root::open(std::path::Path::new(&recent.path)) else {
            continue;
        };
        let vault = permark_core::paths::Vault::new(root.clone());
        if let Some(inside) = vault.relativize(&file) {
            let depth = root.path().components().count();
            if best.as_ref().is_none_or(|(d, _, _)| depth > *d) {
                best = Some((depth, root, inside));
            }
        }
    }
    let (root, document) = if let Some((_, root, document)) = best {
        (root, document)
    } else {
        let folder = file
            .parent()
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(path.clone()))?;
        let root = permark_core::paths::Root::open(folder)?;
        let document = permark_core::paths::Vault::new(root.clone())
            .relativize(&file)
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(path))?;
        (root, document)
    };
    open_in_new_window(&handle, &WorkspaceRef::Project(root), Some(document))
        .map(|_| ())
        .ok_or_else(|| IpcError::new(ErrorKind::Internal).subject("window"))
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
        Some((_, Workspace::Project(core))) => Ok(ProjectSession::new(&core.store())),
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
    let core = project_of(&handle, &window)?;
    let unsaved = core.documents().unsaved_keys();
    Ok(core
        .fileops()
        .plan_move_with(&moves, update_links, &unsaved)?)
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
    // the Notes: a note in its folder, a loose file where it is
    if let Some((_, Workspace::Notes(core))) = state(&handle).registry.workspace_of(window.label())
    {
        let (_, abs) = core.scope().resolve_new(&path)?;
        return Ok(abs.display().to_string());
    }
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
    let abs = if let Some((_, Workspace::Notes(core))) =
        state(&handle).registry.workspace_of(window.label())
    {
        // the Notes only show a file in the file manager; they never hand a note to another program
        if mode != OpenMode::Reveal {
            return Err(IpcError::new(ErrorKind::Unsupported).subject(path));
        }
        let (_, abs) = core.scope().resolve(&path)?;
        abs
    } else {
        let core = project_of(&handle, &window)?;
        // the vault checks the path and the kind of file; the opener only ever sees the validated absolute path
        external_target(core.vault(), &path, mode)?
    };
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
        Workspace::SingleFile(_) | Workspace::Notes(_) => shell.env.formats(),
    };
    Ok((workspace, formats))
}

fn documents_of(workspace: &Workspace) -> &Arc<permark_core::document::Documents> {
    match workspace {
        Workspace::Project(core) => core.documents(),
        Workspace::SingleFile(file) => &file.documents,
        Workspace::Notes(core) => core.documents(),
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
        Workspace::SingleFile(_) | Workspace::Notes(_) => {
            written.iter().map(|_| Vec::new()).collect()
        }
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

// ---- Editing --------------------------------------------------------------------------------------------------------

/// The editor of the invoking window's workspace.
fn with_editor<R>(
    handle: &AppHandle,
    window: &WebviewWindow,
    run: impl FnOnce(&Editor<'_>, &Workspace) -> Result<R, IpcError>,
) -> Result<R, IpcError> {
    let (workspace, formats) = reading_of(handle, window)?;
    let own = match &workspace {
        Workspace::Project(core) => Some(core.watcher().own().as_ref()),
        Workspace::SingleFile(_) | Workspace::Notes(_) => None,
    };
    // the Notes keep earlier versions of what a save replaces
    let observer = match &workspace {
        Workspace::Notes(core) => core.scope().observer(),
        Workspace::Project(_) | Workspace::SingleFile(_) => None,
    };
    let editor = Editor {
        scope: documents_of(&workspace).scope(),
        formats: &formats,
        own,
        observer: observer.as_deref(),
    };
    run(&editor, &workspace)
}

/// The text an editor starts from (BOM gone, LF breaks) with its hash and block map.
#[tauri::command(async)]
pub fn edit_source(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
) -> Result<EditSource, IpcError> {
    with_editor(&handle, &window, |editor, _| editor.source(&path))
}

/// The block map of an unsaved Markdown text (Source -> Edit).
#[tauri::command(async)]
pub fn edit_segments(
    handle: AppHandle,
    window: WebviewWindow,
    text: String,
) -> Result<Vec<Segment>, IpcError> {
    with_editor(&handle, &window, |editor, _| Ok(editor.segments(&text)))
}

/// Applies line edits to the version `seen_hash`; a different disk version is a conflict.
#[tauri::command(async)]
pub fn edit_save(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
    seen_hash: String,
    edits: Vec<LineEdit>,
) -> Result<SaveOutcome, IpcError> {
    with_editor(&handle, &window, |editor, workspace| {
        let outcome = editor.save(&path, &seen_hash, &edits)?;
        if let SaveOutcome::Saved { hash, .. } = &outcome {
            documents_of(workspace).saved(window.label(), &path, hash);
        }
        Ok(outcome)
    })
}

/// The person's text as a new file (`exact`: at `path` itself, when free). Returns its path.
#[tauri::command(async)]
pub fn edit_save_as(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
    text: String,
    exact: bool,
) -> Result<String, IpcError> {
    with_editor(&handle, &window, |editor, _| {
        editor.save_as(&path, &text, exact)
    })
}

/// Picture bytes (raw body; headers `pm-doc`: document path, percent-encoded; `pm-ext`: type) saved next to the
/// document. Returns the picture's path.
#[tauri::command]
pub async fn edit_write_asset(
    handle: AppHandle,
    window: WebviewWindow,
    request: tauri::ipc::Request<'_>,
) -> Result<String, IpcError> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(IpcError::new(ErrorKind::InvalidData).subject("body"));
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(permark_core::links::percent_decode)
            .ok_or_else(|| IpcError::new(ErrorKind::InvalidData).subject(name.to_owned()))
    };
    let doc = header("pm-doc")?;
    let ext = header("pm-ext")?;
    let bytes = bytes.clone();
    tauri::async_runtime::spawn_blocking(move || {
        with_editor(&handle, &window, |editor, _| {
            editor.write_asset(&doc, &ext, &bytes)
        })
    })
    .await
    .map_err(|e| IpcError::new(ErrorKind::Internal).detail(e.to_string()))?
}

/// This window holds unsaved edits of `path` (`None`: none): close guard, link updates.
#[tauri::command(async)]
pub fn edit_set_unsaved(handle: AppHandle, window: WebviewWindow, path: Option<String>) {
    if let Some((_, workspace)) = state(&handle).registry.workspace_of(window.label()) {
        documents_of(&workspace).set_unsaved(window.label(), path);
    }
}

/// Headings of a Markdown document (link completion after `#`).
#[tauri::command(async)]
pub fn edit_headings(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
) -> Result<Vec<OutlineItem>, IpcError> {
    with_editor(&handle, &window, |editor, _| editor.headings(&path))
}

/// Files a link can point at (project: listed, not ignored; single file: none).
#[tauri::command(async)]
pub fn edit_link_targets(
    handle: AppHandle,
    window: WebviewWindow,
) -> Result<Vec<String>, IpcError> {
    let (workspace, _) = reading_of(&handle, &window)?;
    Ok(match workspace {
        Workspace::Project(core) => core.explorer().files(""),
        Workspace::SingleFile(_) | Workspace::Notes(_) => Vec::new(),
    })
}

/// The sheet of an unsaved text of a config file (Edit mode); text the sheet cannot show comes back as text.
#[tauri::command(async)]
pub fn config_sheet(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
    text: String,
) -> Result<Body, IpcError> {
    with_editor(&handle, &window, |editor, _| {
        editor.config_sheet(&path, &text)
    })
}

/// One value edit on the editor's text of a config file: the new text (only that value changed) and its sheet.
#[tauri::command(async)]
pub fn config_edit(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
    text: String,
    edit: ConfigEdit,
) -> Result<ConfigEdited, IpcError> {
    with_editor(&handle, &window, |editor, _| {
        editor.config_edit(&path, &text, &edit)
    })
}

/// Where the secret values of the editor's text are (UTF-16 units), for the mask in Source.
#[tauri::command(async)]
pub fn config_secrets(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
    text: String,
) -> Result<Vec<SecretRange>, IpcError> {
    with_editor(&handle, &window, |editor, _| {
        editor.config_secrets(&path, &text)
    })
}

/// How many project files each pattern of an ignore file matches (`text`: the editor's version, else the disk's).
#[tauri::command(async)]
pub fn ignore_matches(
    handle: AppHandle,
    window: WebviewWindow,
    path: String,
    text: Option<String>,
) -> Result<IgnoreMatches, IpcError> {
    with_editor(&handle, &window, |editor, _| {
        editor.ignore_matches(&path, text.as_deref())
    })
}

/// Measures of the enabled analyses: of a document (`path`) or of a text.
#[tauri::command(async)]
pub fn analyze(
    handle: AppHandle,
    window: WebviewWindow,
    path: Option<String>,
    text: Option<String>,
    kind: TextKind,
) -> Result<Vec<Measure>, IpcError> {
    let (workspace, formats) = reading_of(&handle, &window)?;
    let text = match (path, text) {
        (_, Some(text)) => text,
        (Some(path), None) => documents_of(&workspace).text(&path)?,
        (None, None) => String::new(),
    };
    Ok(formats.analyze(&text, kind))
}

// ---- Search ----------------------------------------------------------------------------------------------------------------

fn as_limit(limit: u32) -> usize {
    usize::try_from(limit).unwrap_or(usize::MAX)
}

/// Where the project's index is (the first build, then ready).
#[tauri::command(async)]
pub fn search_status(handle: AppHandle, window: WebviewWindow) -> Result<IndexStatus, IpcError> {
    Ok(project_of(&handle, &window)?.search().status())
}

/// Ranked sections for the words (partial while the first build runs).
#[tauri::command(async)]
pub fn search_sections(
    handle: AppHandle,
    window: WebviewWindow,
    query: String,
    limit: u32,
    options: SearchOptions,
) -> Result<SectionResults, IpcError> {
    let core = project_of(&handle, &window)?;
    let filter = core.search_filter(&options)?;
    Ok(core.search().sections(&query, as_limit(limit), &filter))
}

/// Files (and folders) whose path matches the words: Quick open, Search everywhere.
#[tauri::command(async)]
pub fn search_files(
    handle: AppHandle,
    window: WebviewWindow,
    query: String,
    limit: u32,
    folder_limit: u32,
    options: SearchOptions,
) -> Result<PathResults, IpcError> {
    let core = project_of(&handle, &window)?;
    let filter = core.search_filter(&options)?;
    Ok(core
        .search()
        .paths(&query, as_limit(limit), as_limit(folder_limit), &filter))
}

/// What Quick open lists for an empty box: the opened files that still exist, and the newest files.
#[tauri::command(async)]
pub fn search_recent(
    handle: AppHandle,
    window: WebviewWindow,
    opened: Vec<String>,
    limit: u32,
    options: SearchOptions,
) -> Result<Recent, IpcError> {
    let core = project_of(&handle, &window)?;
    let filter = core.search_filter(&options)?;
    Ok(core.search().recent(&opened, as_limit(limit), &filter))
}

// ---- Plugins -----------------------------------------------------------------------------------------------------------

/// Calls a method a running plugin of the window's project serves (data in, data out). A plugin that is off, or a
/// method nobody serves, is `Unavailable`.
#[tauri::command(async)]
pub fn plugin_call(
    handle: AppHandle,
    window: WebviewWindow,
    plugin: String,
    method: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    let call = RpcCall {
        window: window.label().to_owned(),
        args,
    };
    // an app plugin (the notes) answers from anywhere: the welcome screen and the Notes window have no project
    let shell = state(&handle);
    let app_scope = PluginId::parse(&plugin)
        .and_then(|id| shell.env.registry.manifest(&id).map(|m| m.scope))
        == Some(PluginScope::App);
    if app_scope {
        return shell.env.rpc().call(&plugin, &method, &call);
    }
    project_of(&handle, &window)?
        .rpc()
        .call(&plugin, &method, &call)
}

/// Closes this window for real (after the unsaved prompt): the guard is let go first.
#[tauri::command(async)]
pub fn window_close(handle: AppHandle, window: WebviewWindow) -> Result<(), IpcError> {
    if let Some((_, workspace)) = state(&handle).registry.workspace_of(window.label()) {
        documents_of(&workspace).set_unsaved(window.label(), None);
    }
    window
        .close()
        .map_err(|e| IpcError::new(ErrorKind::Internal).detail(e.to_string()))
}

// ---- Default app ----------------------------------------------------------------------------------------------------

/// The default-app rows of the settings, and whether to ask (first run).
#[tauri::command(async)]
pub fn default_app_get(handle: AppHandle) -> permark_core::desktop::DefaultAppView {
    let shell = state(&handle);
    crate::default_app::view(&handle, &shell.env.dirs, shell.env.distribution)
}

/// Makes PerMark the default app of a group of file types, or gives the group back (macOS, an installed Linux package).
#[tauri::command(async)]
pub fn default_app_set(
    handle: AppHandle,
    group: permark_core::desktop::FileGroup,
    on: bool,
) -> Result<permark_core::desktop::DefaultAppView, IpcError> {
    let shell = state(&handle);
    crate::default_app::set(&handle, &shell.env.dirs, shell.env.distribution, group, on)
}

/// The first-run question was answered or put off.
#[tauri::command(async)]
pub fn default_app_asked(handle: AppHandle) -> Result<(), IpcError> {
    crate::default_app::asked(&state(&handle).env.dirs)
}

/// Opens Windows' Default Apps page (Windows only).
#[tauri::command(async)]
pub fn default_app_handoff(handle: AppHandle) -> Result<(), IpcError> {
    let shell = state(&handle);
    crate::default_app::handoff(&handle, &shell.env.dirs, shell.env.distribution)
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

// ---- Shell: initialize, project settings, plugins page, workspaces -------------------------------------------------------

/// Creates `.permark/` in a folder (New project, the Explorer menu, the settings). A project open on that folder reloads
/// its state at once.
#[tauri::command(async)]
pub fn project_init(handle: AppHandle, path: String) -> Result<(), IpcError> {
    state(&handle).init_project(std::path::Path::new(&path))
}

/// The shared settings of the project the window shows.
#[tauri::command(async)]
pub fn project_config_get(
    handle: AppHandle,
    window: WebviewWindow,
) -> Result<ProjectSettings, IpcError> {
    let core = project_of(&handle, &window)?;
    let initialized = ProjectConfigFile::of(core.root()).initialized();
    Ok(ProjectSettings::of(initialized, &core.config()))
}

/// One shared setting of the project: written surgically to `.permark/config.toml`, live at once.
#[tauri::command(async)]
pub fn project_config_set(
    handle: AppHandle,
    window: WebviewWindow,
    change: ProjectSettingChange,
) -> Result<ProjectSettings, IpcError> {
    let core = project_of(&handle, &window)?;
    let config = core.change_config(&change)?;
    Ok(ProjectSettings::of(true, &config))
}

/// Every plugin of this build with its state for the window (the Plugins page).
#[tauri::command(async)]
pub fn plugins_list(handle: AppHandle, window: WebviewWindow) -> Vec<PluginInfo> {
    let shell = state(&handle);
    let settings = shell.env.settings();
    let project = match shell.registry.workspace_of(window.label()) {
        Some((_, Workspace::Project(core))) => Some(core),
        _ => None,
    };
    let (states, choices, config_off) = match &project {
        Some(core) => (
            core.enablement(),
            core.plugin_choices(),
            core.config().plugins_off,
        ),
        None => (shell.env.app_enablement(), BTreeMap::new(), BTreeSet::new()),
    };
    shell
        .env
        .registry
        .ordered()
        .map(|manifest| PluginInfo {
            manifest: manifest.clone(),
            app_on: settings
                .plugins
                .get(&manifest.id)
                .copied()
                .unwrap_or(manifest.default_enabled),
            state: states.get(&manifest.id).cloned().unwrap_or(Enablement::On),
            project_choice: choices.get(&manifest.id).copied(),
            project_config_off: config_off.contains(&manifest.id),
        })
        .collect()
}

/// The person's own choice for a plugin in the project the window shows (`None`: follow the app).
#[tauri::command(async)]
pub fn plugin_set_project(
    handle: AppHandle,
    window: WebviewWindow,
    plugin: String,
    enabled: Option<bool>,
) -> Result<(), IpcError> {
    let shell = state(&handle);
    let id = PluginId::parse(&plugin)
        .filter(|id| shell.env.registry.manifest(id).map(|m| m.scope) == Some(PluginScope::Project))
        .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).subject(plugin.clone()))?;
    let core = project_of(&handle, &window)?;
    core.set_plugin_choice(&shell.env, &id, enabled)?;
    shell
        .env
        .sink
        .emit(permark_core::events::CoreEvent::PluginsChanged);
    shell.registry.refresh();
    Ok(())
}

/// Deletes the data a plugin keeps (the app's and every project's). Refused while the plugin is on anywhere.
#[tauri::command(async)]
pub fn plugin_remove_data(handle: AppHandle, plugin: String) -> Result<(), IpcError> {
    let shell = state(&handle);
    let id = PluginId::parse(&plugin)
        .filter(|id| shell.env.registry.manifest(id).is_some())
        .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).subject(plugin.clone()))?;
    let manifest_on = shell.env.registry.manifest(&id).map(|m| m.default_enabled);
    let app_on = shell
        .env
        .settings()
        .plugins
        .get(&id)
        .copied()
        .or(manifest_on)
        .unwrap_or(true);
    let cores = shell.registry.cores();
    if app_on || cores.iter().any(|core| core.plugins_on().contains(&id)) {
        return Err(IpcError::new(ErrorKind::Unavailable).subject(plugin));
    }
    let roots: Vec<_> = cores.iter().map(|core| core.root().clone()).collect();
    permark_core::state::remove_plugin_data_everywhere(&shell.env.dirs, &id, &roots)?;
    Ok(())
}

/// Asks every window that shows the workspace to close it (each runs its unsaved-changes prompt first).
#[tauri::command(async)]
pub fn workspace_close(handle: AppHandle, id: WorkspaceId) -> Result<(), IpcError> {
    let windows = state(&handle).registry.windows_of(id);
    if windows.is_empty() {
        return Err(IpcError::new(ErrorKind::NotFound).subject(id.0.to_string()));
    }
    for window in windows {
        let _ = handle.emit_to(
            window.as_str(),
            "workspace-close-requested",
            WorkspaceCloseRequested {
                window: window.clone(),
            },
        );
    }
    Ok(())
}

commands![
    app_info,
    window_boot,
    open_path,
    open_notes,
    open_in_project,
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
    edit_source,
    edit_segments,
    edit_save,
    edit_save_as,
    edit_write_asset,
    edit_set_unsaved,
    edit_headings,
    edit_link_targets,
    config_sheet,
    config_edit,
    config_secrets,
    ignore_matches,
    analyze,
    search_status,
    search_sections,
    search_files,
    search_recent,
    plugin_call,
    project_init,
    project_config_get,
    project_config_set,
    plugins_list,
    plugin_set_project,
    plugin_remove_data,
    workspace_close,
    window_close,
    default_app_get,
    default_app_set,
    default_app_asked,
    default_app_handoff,
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
