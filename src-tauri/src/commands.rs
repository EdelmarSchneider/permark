//! Thin `#[tauri::command]` adapters: validate arguments, move blocking work off the async runtime,
//! and call the Tauri-free core in [`crate::workspace`]. Command names are the `snake_case` forms of the
//! methods in `src/lib/api.ts`; argument names match what `api.ts` sends.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::http::{self, header, StatusCode};
use tauri::{AppHandle, Emitter, Manager, State, UriSchemeResponder};
use tauri_plugin_opener::OpenerExt;

use crate::edit::{self, AssetWritten, CreatedDoc, ParsedBlocks, SaveOutcome, SourceDoc};
use crate::error::AppError;
use crate::external_editor::{self, EditorError, EditorTemplate};
use crate::fileops::{
    BufferText, CreatedFolder, DeleteOutcome, Move, MoveOutcome, MovePreview, UndoOutcome,
};
use crate::git::{BaselineKind, GitSnapshot};
use crate::links::{self, LinkTarget};
use crate::meta::catalog::{IndexEntry, TaggedFile};
use crate::meta::service::{FileMeta, TagOverview};
use crate::open_request::OpenInbox;
#[cfg(target_os = "macos")]
use crate::open_request::{self, Action};
use crate::render::DocumentView;
use crate::search::SearchHit;
use crate::session::{
    effective_language, installer_language, AppSettingsStore, Language, RecentStore, RecentVault,
    SessionState, StateLocation,
};
use crate::shell_integration::{self, ShellError, ShellState};
use crate::vault::{TreeNode, TreeOptions};
use crate::watch::FsBatch;
use crate::workspace::{Asset, ChangedSince, EventSink, VaultInfo, Workspace, WorkspaceHost};

/// The folder that is currently open, if any, and the rules for switching folders.
pub type AppState = WorkspaceHost;

/// Emits the contract's events to the webview.
struct TauriSink(AppHandle);

impl EventSink for TauriSink {
    fn fs_change(&self, batch: &FsBatch) {
        let _ = self.0.emit("fs-change", batch);
    }

    fn tree_changed(&self) {
        let _ = self.0.emit("tree-changed", ());
    }

    fn git_status(&self, snapshot: &GitSnapshot) {
        let _ = self.0.emit("git-status-changed", snapshot);
    }

    fn index_progress(&self, indexed: usize, total: usize) {
        let _ = self.0.emit(
            "index-progress",
            serde_json::json!({ "indexed": indexed, "total": total }),
        );
    }

    fn meta_changed(&self) {
        let _ = self.0.emit("meta-changed", ());
    }

    fn image_refs_changed(&self) {
        let _ = self.0.emit("image-refs-changed", ());
    }
}

/// Runs blocking work on the blocking thread pool.
async fn blocking<T, F>(f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn open_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<VaultInfo, AppError> {
    let app_data = app_data_dir(&app)?;
    let home = app.path().home_dir().ok();
    let host = state.inner().clone();
    // Taken now, in the order requests arrive: when several wait for their turn only the newest opens,
    // so the folder asked for last is the one that stays open.
    let request = host.request();
    let sink: Arc<dyn EventSink> = Arc::new(TauriSink(app));
    blocking(move || {
        host.open(request, &sink, |sink| {
            Workspace::open(Path::new(&path), &app_data, home.as_deref(), sink)
        })
    })
    .await
}

/// The path the app starts with: the one the system delivered while the app was starting (macOS hands over what
/// Finder opens that way, see [`crate::open_request`]), else the one given on the command line.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands injected state over by value
pub fn initial_path(inbox: State<'_, OpenInbox>) -> Option<String> {
    inbox.take_launch_path().or_else(command_line_path)
}

/// The path given on the command line (`trailmark <path>`), if it exists. Relative paths resolve
/// against the current directory.
pub fn command_line_path() -> Option<String> {
    let arg = std::env::args_os().nth(1)?;
    initial_path_from(arg, std::env::current_dir().ok())
}

/// A path was opened from outside the app (macOS: Finder, the Dock icon). Delivered by `RunEvent::Opened`, both while
/// the app starts and while it runs.
#[cfg(target_os = "macos")]
pub fn opened(app: &AppHandle, urls: &[tauri::Url]) {
    let project_open = app.state::<AppState>().current().is_ok();
    let actions = app
        .state::<OpenInbox>()
        .receive(open_request::paths_from_urls(urls), project_open);
    for action in actions {
        match action {
            Action::OpenHere(path) => {
                crate::window::raise(app);
                let _ = app.emit("open-path", path);
            }
            // Best effort: a process that cannot start leaves nothing to report to.
            Action::NewProcess(path) => {
                let _ = open_request::spawn_instance(&path);
            }
        }
    }
}

/// A drive root passed from the file manager's "Open with TrailMark" entry is `"C:\"` on the command line, and
/// the quote after the backslash reads as an escaped quote: the program receives `C:"`. Gives the root back.
fn unquote_drive_root(arg: OsString) -> OsString {
    let text = arg.to_string_lossy();
    match text.as_bytes() {
        [letter, b':', b'"'] if letter.is_ascii_alphabetic() => {
            OsString::from(format!("{}:\\", char::from(*letter)))
        }
        _ => arg,
    }
}

fn initial_path_from(arg: OsString, cwd: Option<PathBuf>) -> Option<String> {
    if arg.to_string_lossy().starts_with('-') {
        return None; // a flag, not a path
    }
    let path = PathBuf::from(unquote_drive_root(arg));
    let absolute = if path.is_absolute() {
        path
    } else {
        cwd?.join(path)
    };
    dunce::canonicalize(absolute)
        .ok()
        .map(|p| p.display().to_string())
}

#[tauri::command]
pub async fn recent_vaults(app: AppHandle) -> Result<Vec<RecentVault>, AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || Ok(RecentStore::new(&dir).load_existing())).await
}

/// The language the UI starts in: the one picked in the app, else the one the installer recorded, else the
/// system's.
#[tauri::command]
pub async fn app_language(app: AppHandle) -> Result<Language, AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || {
        let stored = AppSettingsStore::new(&dir).load().language;
        Ok(effective_language(stored, installer_language()))
    })
    .await
}

/// Keeps the language picked in Settings (the app-wide choice that outranks the installer's).
#[tauri::command]
pub async fn set_app_language(app: AppHandle, language: Language) -> Result<(), AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || Ok(AppSettingsStore::new(&dir).set_language(language)?)).await
}

/// What "Open with TrailMark" has registered with the file manager, read from the system each time.
#[tauri::command]
pub async fn shell_integration_state() -> Result<ShellState, AppError> {
    blocking(|| Ok(shell_integration::state())).await
}

/// Adds or removes "Open with TrailMark" in the right-click menu. `locale` is the language of the interface
/// (`en`, `pt-BR`, `es`, `de`): the entry is worded in it.
#[tauri::command]
pub async fn set_shell_context_menu(enabled: bool, locale: String) -> Result<ShellState, AppError> {
    blocking(move || Ok(shell_integration::set_context_menu(enabled, &locale)?)).await
}

/// Adds or removes TrailMark in the "Open with" list of Markdown files.
#[tauri::command]
pub async fn set_shell_file_association(
    enabled: bool,
    locale: String,
) -> Result<ShellState, AppError> {
    blocking(move || Ok(shell_integration::set_association(enabled, &locale)?)).await
}

/// Rewords the entries that exist after the interface language changed.
#[tauri::command]
pub async fn refresh_shell_labels(locale: String) -> Result<(), AppError> {
    blocking(move || Ok(shell_integration::refresh_labels(&locale)?)).await
}

/// Opens the page of Windows Settings where the default app of a file type is chosen: Windows does not let a
/// program pick itself.
#[tauri::command]
pub async fn open_default_apps(app: AppHandle) -> Result<(), AppError> {
    if !cfg!(windows) {
        return Err(ShellError::Unsupported.into());
    }
    blocking(move || {
        app.opener()
            .open_url(shell_integration::DEFAULT_APPS_URI, None::<&str>)
            .map_err(|e| AppError::Internal(e.to_string()))
    })
    .await
}

#[tauri::command]
pub async fn list_tree(
    state: State<'_, AppState>,
    show_ignored: bool,
    show_hidden: bool,
) -> Result<TreeNode, AppError> {
    let ws = state.current()?;
    let opts = TreeOptions {
        show_ignored,
        show_state_dir: show_hidden,
    };
    blocking(move || Ok(ws.tree(opts))).await
}

#[tauri::command]
pub async fn read_document(
    state: State<'_, AppState>,
    path: String,
) -> Result<DocumentView, AppError> {
    let ws = state.current()?;
    blocking(move || ws.read_document(&path)).await
}

#[tauri::command]
pub async fn resolve_link(
    state: State<'_, AppState>,
    from: String,
    href: String,
) -> Result<LinkTarget, AppError> {
    let ws = state.current()?;
    blocking(move || Ok(ws.resolve_link(&from, &href))).await
}

#[tauri::command]
pub async fn git_snapshot(state: State<'_, AppState>) -> Result<GitSnapshot, AppError> {
    let ws = state.current()?;
    blocking(move || Ok(ws.git_snapshot())).await
}

#[tauri::command]
pub async fn set_git_baseline(
    state: State<'_, AppState>,
    kind: BaselineKind,
) -> Result<GitSnapshot, AppError> {
    let ws = state.current()?;
    blocking(move || Ok(ws.set_git_baseline(kind))).await
}

#[tauri::command]
pub async fn since_last_visit(state: State<'_, AppState>) -> Result<ChangedSince, AppError> {
    let ws = state.current()?;
    blocking(move || ws.since_last_visit()).await
}

#[tauri::command]
pub async fn save_session(state: State<'_, AppState>, s: SessionState) -> Result<(), AppError> {
    let ws = state.current()?;
    blocking(move || ws.save_session(&s)).await
}

#[tauri::command]
pub async fn load_session(state: State<'_, AppState>) -> Result<Option<SessionState>, AppError> {
    let ws = state.current()?;
    blocking(move || ws.load_session()).await
}

#[tauri::command]
pub async fn absolute_path(state: State<'_, AppState>, path: String) -> Result<String, AppError> {
    let ws = state.current()?;
    blocking(move || ws.absolute_path(&path)).await
}

/// Hands a file of the folder to the system's default application. Only documents and images go: a program or script
/// in the folder is never started from here.
fn open_with_system(app: &AppHandle, ws: &Workspace, path: &str) -> Result<(), AppError> {
    let target = ws.external_target(path)?;
    app.opener()
        .open_path(target.display().to_string(), None::<&str>)
        .map_err(|e| AppError::Internal(e.to_string()))
}

/// Opens a file of the folder with the system's default application.
#[tauri::command]
pub async fn open_external(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), AppError> {
    let ws = state.current()?;
    blocking(move || open_with_system(&app, &ws, &path)).await
}

/// Opens a file of the folder in the person's editor, at a line and column (both from 1): runs the editor command of
/// the settings, or the system's default application when there is none (the line is then ignored).
#[tauri::command]
pub async fn open_in_editor(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    line: Option<u32>,
    col: Option<u32>,
) -> Result<(), AppError> {
    let ws = state.current()?;
    let dir = app_data_dir(&app)?;
    blocking(move || {
        let command = AppSettingsStore::new(&dir)
            .load()
            .editor_command
            .unwrap_or_default();
        match EditorTemplate::parse(&command).map_err(EditorError::Template)? {
            None => open_with_system(&app, &ws, &path),
            Some(template) => {
                let target = ws.editor_target(&path)?;
                Ok(external_editor::run(
                    &template,
                    &target,
                    line.unwrap_or(1),
                    col.unwrap_or(1),
                )?)
            }
        }
    })
    .await
}

/// The editor command of the settings; blank means the system's default application.
#[tauri::command]
pub async fn editor_command(app: AppHandle) -> Result<String, AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || {
        Ok(AppSettingsStore::new(&dir)
            .load()
            .editor_command
            .unwrap_or_default())
    })
    .await
}

/// Keeps the editor command. A command that can't be parsed (an unknown placeholder, an open quote) is refused, so what
/// is saved always runs.
#[tauri::command]
pub async fn set_editor_command(app: AppHandle, command: String) -> Result<(), AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || {
        EditorTemplate::parse(&command).map_err(EditorError::Template)?;
        Ok(AppSettingsStore::new(&dir).set_editor_command(&command)?)
    })
    .await
}

/// Shows a file or folder of the open folder in the system's file manager.
#[tauri::command]
pub async fn reveal_in_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), AppError> {
    let ws = state.current()?;
    blocking(move || {
        let target = ws.reveal_target(&path)?;
        app.opener()
            .reveal_item_in_dir(target)
            .map_err(|e| AppError::Internal(e.to_string()))
    })
    .await
}

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>, AppError> {
    let ws = state.current()?;
    blocking(move || Ok(ws.search(&query, limit))).await
}

/// The images that some document of the folder links to, as folder paths (for the Explorer filter).
#[tauri::command]
pub async fn referenced_images(state: State<'_, AppState>) -> Result<Vec<String>, AppError> {
    let ws = state.current()?;
    blocking(move || Ok(ws.referenced_images())).await
}

#[tauri::command]
pub async fn init_project_folder(state: State<'_, AppState>) -> Result<(), AppError> {
    let ws = state.current()?;
    blocking(move || ws.init_project_folder()).await
}

#[tauri::command]
pub async fn state_location(state: State<'_, AppState>) -> Result<StateLocation, AppError> {
    let ws = state.current()?;
    blocking(move || ws.state_location()).await
}

/// The tags (from the file's front matter and from the app) and the description of a file.
#[tauri::command]
pub async fn file_meta(state: State<'_, AppState>, path: String) -> Result<FileMeta, AppError> {
    let ws = state.current()?;
    blocking(move || ws.file_meta(&path)).await
}

/// Replaces the tags the app keeps for a file.
#[tauri::command]
pub async fn set_file_tags(
    state: State<'_, AppState>,
    path: String,
    tags: Vec<String>,
) -> Result<FileMeta, AppError> {
    let ws = state.current()?;
    blocking(move || ws.set_file_tags(&path, &tags)).await
}

/// Replaces the description of a file.
#[tauri::command]
pub async fn set_file_description(
    state: State<'_, AppState>,
    path: String,
    description: String,
) -> Result<FileMeta, AppError> {
    let ws = state.current()?;
    blocking(move || ws.set_file_description(&path, &description)).await
}

/// Every tag with its number of files, the number of untagged files, and the state of `meta.toml`.
#[tauri::command]
pub async fn list_tags(state: State<'_, AppState>) -> Result<TagOverview, AppError> {
    let ws = state.current()?;
    blocking(move || Ok(ws.list_tags())).await
}

/// The files that have all of the tags, or (with `untagged`) the files that have none.
#[tauri::command]
pub async fn files_with_tags(
    state: State<'_, AppState>,
    tags: Vec<String>,
    untagged: bool,
) -> Result<Vec<TaggedFile>, AppError> {
    let ws = state.current()?;
    blocking(move || Ok(ws.files_with_tags(&tags, untagged))).await
}

/// The tags and description of every file that has some (for filters and tooltips).
#[tauri::command]
pub async fn meta_index(state: State<'_, AppState>) -> Result<Vec<IndexEntry>, AppError> {
    let ws = state.current()?;
    blocking(move || Ok(ws.meta_index())).await
}

/// Removes the entries kept for files that no longer exist. Resolves to how many.
#[tauri::command]
pub async fn clean_up_meta(state: State<'_, AppState>) -> Result<usize, AppError> {
    let ws = state.current()?;
    blocking(move || ws.clean_up_meta()).await
}

/// A Markdown file as the editor needs it: the exact text, its hash and the byte ranges of its blocks.
#[tauri::command]
pub async fn document_source(
    state: State<'_, AppState>,
    path: String,
) -> Result<SourceDoc, AppError> {
    let ws = state.current()?;
    blocking(move || ws.document_source(&path)).await
}

/// The top-level blocks of some Markdown text (the editor's buffer).
#[tauri::command]
pub async fn parse_blocks(text: String) -> Result<ParsedBlocks, AppError> {
    blocking(move || Ok(edit::parse_blocks(&text))).await
}

/// The reading view's HTML for pieces of Markdown (Visual mode draws its locked blocks with it).
#[tauri::command]
pub async fn render_fragments(texts: Vec<String>) -> Result<Vec<String>, AppError> {
    blocking(move || Ok(edit::render_fragments(&texts))).await
}

/// Saves the editor's text over a file, unless the file is no longer the version with `expected_hash`.
#[tauri::command]
pub async fn save_document(
    state: State<'_, AppState>,
    path: String,
    text: String,
    expected_hash: String,
) -> Result<SaveOutcome, AppError> {
    let ws = state.current()?;
    blocking(move || ws.save_document(&path, &text, &expected_hash)).await
}

/// Creates a new Markdown file; fails when the name is not usable or the file exists.
#[tauri::command]
pub async fn create_document(
    state: State<'_, AppState>,
    path: String,
    text: String,
) -> Result<CreatedDoc, AppError> {
    let ws = state.current()?;
    blocking(move || ws.create_document(&path, &text)).await
}

/// Writes an image (`data` is base64) into the `assets` folder next to a document.
#[tauri::command]
pub async fn write_asset(
    state: State<'_, AppState>,
    doc_path: String,
    name_hint: String,
    data: String,
) -> Result<AssetWritten, AppError> {
    let ws = state.current()?;
    blocking(move || {
        let bytes = edit::decode_image(&data, &name_hint)?;
        ws.write_asset(&doc_path, &name_hint, &bytes)
    })
    .await
}

/// Checks a rename or move (nothing is overwritten, nothing leaves the project) and, with `scan_links`,
/// lists the links in the documents that would have to change. The documents in `held_back` (open with
/// unsaved edits that cannot be patched) are left out and listed as skipped.
#[tauri::command]
pub async fn preview_move(
    state: State<'_, AppState>,
    moves: Vec<Move>,
    scan_links: bool,
    held_back: Vec<String>,
) -> Result<MovePreview, AppError> {
    let ws = state.current()?;
    blocking(move || ws.preview_move(&moves, scan_links, &held_back)).await
}

/// Makes the move of a preview. With `update_links` the links it listed are rewritten; the documents in
/// `buffers` (open with unsaved edits) are not written, the changes for their text come back instead.
#[tauri::command]
pub async fn apply_move(
    state: State<'_, AppState>,
    plan_id: u64,
    update_links: bool,
    buffers: Vec<BufferText>,
) -> Result<MoveOutcome, AppError> {
    let ws = state.current()?;
    blocking(move || ws.apply_move(plan_id, update_links, &buffers)).await
}

/// Sends files and folders of the open folder to the recycle bin.
#[tauri::command]
pub async fn delete_entries(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<DeleteOutcome, AppError> {
    let ws = state.current()?;
    blocking(move || ws.delete_entries(&paths)).await
}

/// Creates a folder; fails when the name is not usable or the folder exists.
#[tauri::command]
pub async fn new_folder(
    state: State<'_, AppState>,
    path: String,
) -> Result<CreatedFolder, AppError> {
    let ws = state.current()?;
    blocking(move || ws.new_folder(&path)).await
}

/// Reverts an operation of the Explorer (the moves and every rewritten link) as far as the files are unchanged.
#[tauri::command]
pub async fn undo_file_op(state: State<'_, AppState>, op_id: u64) -> Result<UndoOutcome, AppError> {
    let ws = state.current()?;
    blocking(move || ws.undo_file_op(op_id)).await
}

/// The URI scheme that serves the images of the open folder to the document view. Requests are
/// answered from the folder that is open now, through the same path checks as every other file, so
/// nothing outside it (or from a folder that has been closed) can be read.
pub const ASSET_SCHEME: &str = "tmasset";

/// The vault path a request for [`ASSET_SCHEME`] asks for: the URL path, decoded, without its leading slash.
fn asset_request_path(uri: &http::Uri) -> String {
    links::percent_decode(uri.path().trim_start_matches('/'))
}

/// The HTTP answer for an image lookup.
fn asset_response(result: Result<Asset, AppError>) -> http::Response<Vec<u8>> {
    let (status, content_type, body) = match result {
        Ok(asset) => (StatusCode::OK, asset.mime, asset.bytes),
        Err(e) => {
            let status = match e.kind() {
                "notFound" => StatusCode::NOT_FOUND,
                "pathEscape" | "invalidPath" | "noVault" => StatusCode::FORBIDDEN,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, "text/plain", Vec::new())
        }
    };
    http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Content-Type-Options", "nosniff")
        // An SVG shown as a document of its own could not run scripts or load anything.
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; style-src 'unsafe-inline'; sandbox",
        )
        .body(body)
        .unwrap_or_default()
}

/// Answers a request for [`ASSET_SCHEME`] on the blocking pool (reading a file must not stall the UI).
pub fn serve_asset(app: &AppHandle, uri: &http::Uri, responder: UriSchemeResponder) {
    let host = app.state::<AppState>().inner().clone();
    let path = asset_request_path(uri);
    tauri::async_runtime::spawn_blocking(move || {
        responder.respond(asset_response(host.read_asset(&path)));
    });
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    const CONTRACT: &str = include_str!("../ipc-contract.json");
    const LIB_SRC: &str = include_str!("lib.rs");
    const COMMANDS_SRC: &str = include_str!("commands.rs");

    /// This file without its test module (whose own text would match the scans below).
    fn production_source() -> &'static str {
        COMMANDS_SRC
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default()
    }

    /// Splits a parameter list at the commas that are not inside `<...>` or `(...)`.
    fn split_params(params: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let (mut depth, mut start) = (0_i32, 0);
        for (i, c) in params.char_indices() {
            match c {
                '<' | '(' => depth += 1,
                '>' | ')' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(&params[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        parts.push(&params[start..]);
        parts
    }

    /// Every `#[tauri::command]` function in this file with the names of the arguments the UI sends
    /// (everything except the injected `AppHandle` and `State`).
    fn declared_commands() -> BTreeMap<String, Vec<String>> {
        let mut out = BTreeMap::new();
        for chunk in production_source()
            .split(
                "
#[tauri::command]",
            )
            .skip(1)
        {
            let (_, after_fn) = chunk.split_once("fn ").expect("a fn after the attribute");
            let (name, rest) = after_fn.split_once('(').expect("a parameter list");
            let mut depth = 1;
            let mut end = rest.len();
            for (i, c) in rest.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            let args = split_params(&rest[..end])
                .into_iter()
                .filter_map(|p| p.split_once(':'))
                .filter(|(_, ty)| {
                    !ty.trim().starts_with("AppHandle") && !ty.trim().starts_with("State<")
                })
                .map(|(arg, _)| arg.trim().to_owned())
                .collect();
            out.insert(name.trim().to_owned(), args);
        }
        out
    }

    /// The commands listed in `tauri::generate_handler![...]` in `lib.rs`.
    fn registered_commands() -> BTreeSet<String> {
        let list = LIB_SRC
            .split("generate_handler![")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("generate_handler! in lib.rs");
        list.split(',')
            .filter_map(|item| item.trim().strip_prefix("commands::"))
            .map(str::to_owned)
            .collect()
    }

    /// The event names passed to `.emit("...")` in this file.
    fn emitted_events() -> BTreeSet<String> {
        production_source()
            .split(".emit(")
            .skip(1)
            .filter_map(|rest| {
                let rest = rest.trim_start().strip_prefix('"')?;
                rest.split_once('"').map(|(name, _)| name.to_owned())
            })
            .collect()
    }

    /// `src-tauri/ipc-contract.json` is also checked against `tauriApi` by `src/lib/api.test.ts`, so a
    /// renamed command, argument or event fails on one side or the other.
    #[test]
    fn commands_and_events_match_the_ui_contract() {
        let contract: serde_json::Value = serde_json::from_str(CONTRACT).expect("contract json");
        let commands: BTreeMap<String, Vec<String>> =
            serde_json::from_value(contract["commands"].clone()).expect("commands in contract");
        let events: BTreeSet<String> =
            serde_json::from_value(contract["events"].clone()).expect("events in contract");

        assert_eq!(declared_commands(), commands, "#[tauri::command] fns");
        assert_eq!(
            registered_commands(),
            commands.keys().cloned().collect::<BTreeSet<_>>(),
            "generate_handler! in lib.rs"
        );
        assert_eq!(emitted_events(), events, "events emitted to the webview");
    }

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn flags_are_not_paths() {
        let d = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            initial_path_from(os("--help"), Some(d.path().to_path_buf())),
            None
        );
        assert_eq!(
            initial_path_from(os("-x"), Some(d.path().to_path_buf())),
            None
        );
    }

    #[test]
    fn relative_paths_resolve_against_the_working_directory() {
        let d = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(d.path().join("docs")).expect("mkdir");
        let canon = dunce::canonicalize(d.path().join("docs")).expect("canon");
        let got = initial_path_from(os("docs"), Some(d.path().to_path_buf()));
        assert_eq!(got, Some(canon.display().to_string()));
        let got = initial_path_from(os("./docs/../docs"), Some(d.path().to_path_buf()));
        assert_eq!(got, Some(canon.display().to_string()));
    }

    #[test]
    fn absolute_paths_and_files_are_accepted_but_missing_ones_are_not() {
        let d = tempfile::tempdir().expect("tempdir");
        let file = d.path().join("a.md");
        std::fs::write(&file, "x").expect("write");
        let got = initial_path_from(os(&file.display().to_string()), None);
        assert_eq!(
            got,
            Some(
                dunce::canonicalize(&file)
                    .expect("canon")
                    .display()
                    .to_string()
            )
        );
        assert_eq!(
            initial_path_from(os("nope"), Some(d.path().to_path_buf())),
            None
        );
        assert_eq!(initial_path_from(os("relative"), None), None);
    }

    /// "Open with TrailMark" on a drive passes `"C:\"`, which arrives as `C:"` (see `unquote_drive_root`).
    #[test]
    fn a_drive_root_from_the_file_manager_loses_its_stray_quote() {
        assert_eq!(unquote_drive_root(os("C:\"")), os("C:\\"));
        assert_eq!(unquote_drive_root(os("d:\"")), os("d:\\"));
        for untouched in [
            "C:\\",
            "C:",
            "C:\\docs",
            "C:\\docs\"",
            "\"C:\"",
            "docs",
            "",
            "1:\"",
        ] {
            assert_eq!(
                unquote_drive_root(os(untouched)),
                os(untouched),
                "{untouched}"
            );
        }
    }

    #[test]
    fn a_folder_with_spaces_and_a_markdown_file_open_from_the_entry_the_way_the_shell_passes_them()
    {
        let d = tempfile::tempdir().expect("tempdir");
        let folder = d.path().join("my notes");
        std::fs::create_dir(&folder).expect("mkdir");
        let file = folder.join("read me.md");
        std::fs::write(&file, "x").expect("write");
        for path in [&folder, &file] {
            let got = initial_path_from(os(&path.display().to_string()), None);
            let expected = dunce::canonicalize(path)
                .expect("canon")
                .display()
                .to_string();
            assert_eq!(got, Some(expected));
        }
    }

    #[test]
    fn app_state_starts_empty_and_reports_no_vault() {
        let state = AppState::default();
        assert!(matches!(state.current(), Err(AppError::NoVault)));
        assert!(matches!(state.read_asset("a.png"), Err(AppError::NoVault)));
        state.save_last_visit();
        state.refresh_on_focus();
        state.shutdown();
    }

    #[test]
    fn asset_requests_decode_the_vault_path_from_the_url() {
        let path = |uri: &str| asset_request_path(&uri.parse::<http::Uri>().expect("uri"));
        assert_eq!(
            path("http://tmasset.localhost/docs%2Fimg%2Fa%20b.png"),
            "docs/img/a b.png"
        );
        assert_eq!(path("tmasset://localhost/a.png"), "a.png");
        assert_eq!(
            path("http://tmasset.localhost/%40wt%2Ffeat%2Fp.png"),
            "@wt/feat/p.png"
        );
        assert_eq!(path("http://tmasset.localhost/"), "");
    }

    #[test]
    fn asset_responses_carry_the_image_or_a_status_without_details() {
        let ok = asset_response(Ok(Asset {
            bytes: vec![1, 2, 3],
            mime: "image/png",
        }));
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(ok.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(ok.headers()["x-content-type-options"], "nosniff");
        assert!(ok.headers().contains_key(header::CONTENT_SECURITY_POLICY));
        assert_eq!(ok.body(), &vec![1, 2, 3]);

        for (error, status) in [
            (AppError::NoVault, StatusCode::FORBIDDEN),
            (
                crate::vault::VaultError::Escapes("../x".to_owned()).into(),
                StatusCode::FORBIDDEN,
            ),
            (
                crate::vault::VaultError::NotFound("x".to_owned()).into(),
                StatusCode::NOT_FOUND,
            ),
            (
                AppError::Internal("boom".to_owned()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ] {
            let r = asset_response(Err(error));
            assert_eq!(r.status(), status);
            assert!(r.body().is_empty(), "no details leak into the body");
        }
    }
}
