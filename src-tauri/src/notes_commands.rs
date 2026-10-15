//! Thin `#[tauri::command]` adapters for the Notes window and for the places that lead to it (the welcome screen, the
//! shortcuts of a project window). The work is in [`crate::notes`]; like the commands of [`crate::commands`], these
//! validate arguments and move blocking work off the async runtime. Their names are listed in `ipc-contract.json`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::bridge::socket::Endpoint;
use crate::commands::{app_data_dir, blocking, AppState, TauriSink};
use crate::error::AppError;
use crate::notes::{
    self, folder, FolderStatus, Moment, NoteDeleted, NoteList, NoteSaved, NoteVersion, NoteView,
    NotesFolder, NotesRequest, NotesSummary, ReadOnlyReason, Restored,
};
use crate::session::{AppSettingsStore, StartIn};
use crate::vault::VaultError;
use crate::workspace::{EventSink, VaultInfo, Workspace};

/// What the program was started as, and the requests to the Notes window that it has not shown yet. Managed state.
pub struct NotesLaunch {
    is_notes_window: bool,
    pending: Mutex<Option<NotesRequest>>,
}

impl NotesLaunch {
    pub fn new(is_notes_window: bool) -> Self {
        Self {
            is_notes_window,
            pending: Mutex::new(None),
        }
    }

    /// Whether this program is the Notes window.
    pub fn is_notes_window(&self) -> bool {
        self.is_notes_window
    }

    /// A request came in. Requests that pile up before the window picks them up are one: a new note wanted by any of them
    /// is wanted, and the note to show is the last one asked for.
    pub fn push(&self, request: NotesRequest) {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let merged = match pending.take() {
            Some(before) => NotesRequest {
                new_note: before.new_note || request.new_note,
                open: request.open.or(before.open),
            },
            None => request,
        };
        *pending = Some(merged);
    }

    fn take(&self) -> Option<NotesRequest> {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

/// A request reached the Notes window: the window comes forward and the UI is told to pick the request up.
pub fn deliver(app: &AppHandle, request: NotesRequest) {
    if let Some(launch) = app.try_state::<NotesLaunch>() {
        launch.push(request);
    }
    crate::window::raise(app);
    let _ = app.emit("notes-request", ());
}

/// What the program is. Matches the UI's `LaunchMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchMode {
    /// This program is the Notes window.
    pub notes: bool,
}

/// What opening the notes found. Matches the UI's `NotesOpened`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesOpened {
    pub info: VaultInfo,
    pub read_only: Option<ReadOnlyReason>,
    pub recovered: Vec<String>,
}

/// The notes as the welcome screen shows them. Matches the UI's `NotesOverview`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesOverview {
    pub folder: NotesFolder,
    pub summary: NotesSummary,
}

/// The folder the person chose for the notes, if they chose one.
fn chosen_folder(app_data: &std::path::Path) -> Option<String> {
    AppSettingsStore::new(app_data).load().notes_folder
}

/// Where the notes are.
fn folder_path(app_data: &std::path::Path) -> PathBuf {
    folder::configured(app_data, chosen_folder(app_data).as_deref())
}

fn folder_info(app_data: &std::path::Path) -> NotesFolder {
    folder::describe(&folder_path(app_data), chosen_folder(app_data).is_none())
}

/// The place of the notes of the open Notes window, as the commands that need a workspace see it.
fn notes_of(state: &State<'_, AppState>) -> Result<Arc<Workspace>, AppError> {
    let ws = state.current()?;
    ws.notes()?;
    Ok(ws)
}

/// Whether this program is the Notes window.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands injected state over by value
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub fn launch_mode(app: AppHandle, launch: State<'_, NotesLaunch>) -> LaunchMode {
    // Finder opened something while the program started as the Notes window: that gets a window of its own
    #[cfg(target_os = "macos")]
    if launch.is_notes_window {
        if let Some(path) = app
            .state::<crate::open_request::OpenInbox>()
            .take_launch_path()
        {
            let _ = crate::open_request::spawn_instance(&path);
        }
    }
    LaunchMode {
        notes: launch.is_notes_window,
    }
}

/// The notes folder: where it is, and whether it is there.
#[tauri::command]
pub async fn notes_folder(app: AppHandle) -> Result<NotesFolder, AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || Ok(folder_info(&dir))).await
}

/// Points the notes at another folder, one that exists ("Locate...", and Settings).
#[tauri::command]
pub async fn set_notes_folder(app: AppHandle, path: String) -> Result<NotesFolder, AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || {
        let chosen = PathBuf::from(path.trim());
        if !chosen.is_absolute() || !chosen.is_dir() {
            return Err(VaultError::NotFound(path).into());
        }
        let chosen = dunce::canonicalize(&chosen).unwrap_or(chosen);
        AppSettingsStore::new(&dir).set_notes_folder(&chosen.display().to_string())?;
        Ok(folder_info(&dir))
    })
    .await
}

/// Makes the notes folder that is not there ("Create it"). Nothing else makes it.
#[tauri::command]
pub async fn create_notes_folder(app: AppHandle) -> Result<NotesFolder, AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || {
        let path = folder_path(&dir);
        folder::create(&path).map_err(|source| VaultError::io(&path, source))?;
        Ok(folder_info(&dir))
    })
    .await
}

/// Where TrailMark opens when it is started with nothing to open.
#[tauri::command]
pub async fn notes_start_in(app: AppHandle) -> Result<StartIn, AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || {
        Ok(AppSettingsStore::new(&dir)
            .load()
            .start_in
            .unwrap_or_default())
    })
    .await
}

/// Keeps where TrailMark opens when it is started with nothing to open.
#[tauri::command]
pub async fn set_notes_start_in(app: AppHandle, start_in: StartIn) -> Result<(), AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || Ok(AppSettingsStore::new(&dir).set_start_in(start_in)?)).await
}

/// The notes folder and what the welcome screen shows of it: the pinned notes and the latest. Any window can ask; the
/// folder is only read, and never made.
#[tauri::command]
pub async fn notes_overview(app: AppHandle) -> Result<NotesOverview, AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || {
        let path = folder_path(&dir);
        Ok(NotesOverview {
            folder: folder::describe(&path, chosen_folder(&dir).is_none()),
            summary: notes::summary(&path),
        })
    })
    .await
}

/// Opens the notes folder as this window's workspace (the Notes window only). The folder must be there. Asking again for
/// the folder that is open answers with it, so the lock it holds is never taken twice.
#[tauri::command]
pub async fn open_notes(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<NotesOpened, AppError> {
    let app_data = app_data_dir(&app)?;
    let host = state.inner().clone();
    let request = host.request();
    let sink: Arc<dyn EventSink> = Arc::new(TauriSink::new(app));
    blocking(move || {
        let path = folder_path(&app_data);
        if let Ok(ws) = host.current() {
            if ws.shows_notes_of(&path) {
                return Ok(opened(&ws));
            }
        }
        let status = folder::describe(&path, false).status;
        if status == FolderStatus::Missing {
            return Err(VaultError::NotFound(path.display().to_string()).into());
        }
        host.open(request, &sink, |sink| {
            Workspace::open_notes(&path, &app_data, sink)
        })?;
        let ws = host.current()?;
        Ok(opened(&ws))
    })
    .await
}

fn opened(ws: &Workspace) -> NotesOpened {
    NotesOpened {
        info: ws.info(),
        read_only: ws.notes_read_only(),
        recovered: ws
            .notes_opened()
            .map(|report| report.recovered.clone())
            .unwrap_or_default(),
    }
}

/// The notes, pinned ones first.
#[tauri::command]
pub async fn list_notes(state: State<'_, AppState>) -> Result<NoteList, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.list())).await
}

/// The name of a new note. No file is made until its text has something in it.
#[tauri::command]
pub async fn new_note(state: State<'_, AppState>) -> Result<String, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.reserve_new(Moment::now())?)).await
}

/// A new note that was left without being written is forgotten.
#[tauri::command]
pub async fn forget_new_note(state: State<'_, AppState>, path: String) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || {
        ws.notes()?.release_new(&path);
        Ok(())
    })
    .await
}

/// Saves the text of a note, unless the note changed since `expected_hash` (then the text is kept as a copy). An empty
/// `expected_hash` is a new note, whose file is made by the first text that is not blank.
#[tauri::command]
pub async fn save_note(
    state: State<'_, AppState>,
    path: String,
    text: String,
    expected_hash: String,
) -> Result<NoteSaved, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || ws.save_note(&path, &text, &expected_hash)).await
}

/// A note was opened: it is the one to come back to, and its next save keeps a version.
#[tauri::command]
pub async fn note_opened(state: State<'_, AppState>, path: String) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || {
        ws.notes()?.opened(&path);
        Ok(())
    })
    .await
}

/// No note is open any more.
#[tauri::command]
pub async fn close_note(state: State<'_, AppState>) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || {
        ws.notes()?.closed();
        Ok(())
    })
    .await
}

/// Where a note was left: the editor, the caret and the scroll position.
#[tauri::command]
pub async fn set_note_view(
    state: State<'_, AppState>,
    path: String,
    view: NoteView,
) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || {
        ws.notes()?.set_view(&path, view);
        Ok(())
    })
    .await
}

/// Pins a note or unpins it.
#[tauri::command]
pub async fn set_note_pinned(
    state: State<'_, AppState>,
    path: String,
    pinned: bool,
) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.set_pinned(&path, pinned)?)).await
}

/// The order of the pinned notes.
#[tauri::command]
pub async fn set_pin_order(state: State<'_, AppState>, order: Vec<String>) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.set_pin_order(&order)?)).await
}

/// Gives a note a real file name. Resolves to the name it has now.
#[tauri::command]
pub async fn rename_note(
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> Result<String, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.rename(&path, &name)?)).await
}

/// Sends notes to the recycle bin; they come back with `undo_delete_notes` for as long as the program runs.
#[tauri::command]
pub async fn delete_notes(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<NoteDeleted, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.delete(&paths)?)).await
}

/// Puts back the notes of a deletion. Resolves to the names they have now.
#[tauri::command]
pub async fn undo_delete_notes(
    state: State<'_, AppState>,
    undo_id: u64,
) -> Result<Vec<String>, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.undo_delete(undo_id)?)).await
}

/// Sends the notes that were copied and not edited since to the recycle bin; pinned notes stay.
#[tauri::command]
pub async fn delete_copied_notes(state: State<'_, AppState>) -> Result<NoteDeleted, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.delete_copied()?)).await
}

/// A note was copied: it shows as copied until it is edited.
#[tauri::command]
pub async fn mark_note_copied(state: State<'_, AppState>, path: String) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.mark_copied(&path)?)).await
}

/// The earlier versions of a note, the newest first.
#[tauri::command]
pub async fn note_versions(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<NoteVersion>, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.versions(&path)?)).await
}

/// The text of an earlier version.
#[tauri::command]
pub async fn note_version_text(
    state: State<'_, AppState>,
    path: String,
    id: String,
) -> Result<String, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.version_text(&path, &id)?)).await
}

/// Puts an earlier version back; the text the note has now becomes a version first.
#[tauri::command]
pub async fn restore_note_version(
    state: State<'_, AppState>,
    path: String,
    id: String,
) -> Result<Restored, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || {
        Ok(ws
            .notes()?
            .restore_version(&path, &id, crate::util::now_ms())?)
    })
    .await
}

/// Removes a note that was never written in (blank, and not pinned). Resolves to whether it was removed.
#[tauri::command]
pub async fn discard_blank_note(
    state: State<'_, AppState>,
    path: String,
) -> Result<bool, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.discard_blank(&path)?)).await
}

/// Shows the notes folder in the system's file manager.
#[tauri::command]
pub async fn open_notes_folder(app: AppHandle) -> Result<(), AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || {
        let path = folder_path(&dir);
        if !path.is_dir() {
            return Err(VaultError::NotFound(path.display().to_string()).into());
        }
        app.opener()
            .open_path(path.display().to_string(), None::<&str>)
            .map_err(|e| AppError::Internal(e.to_string()))
    })
    .await
}

/// Brings the Notes window forward, with a new note ready or a given note shown. Starts the window when it is not
/// running. This is what `Ctrl+Shift+N` and `Ctrl+Shift+D` do from a project window.
#[tauri::command]
pub async fn show_notes(
    app: AppHandle,
    new_note: bool,
    open: Option<String>,
) -> Result<(), AppError> {
    let dir = app_data_dir(&app)?;
    let endpoint = Endpoint::for_notes(&dir).map_err(|e| AppError::Internal(e.to_string()))?;
    let request = NotesRequest { new_note, open };
    notes::show_or_start(&endpoint, &request, start_notes_window)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))
}

/// The request that came in while the window was not looking, once.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands injected state over by value
pub fn take_notes_request(launch: State<'_, NotesLaunch>) -> Option<NotesRequest> {
    launch.take()
}

/// Starts another TrailMark as the Notes window, and lets it go its own way.
fn start_notes_window() -> std::io::Result<()> {
    use std::process::{Command, Stdio};

    let mut child = Command::new(std::env::current_exe()?)
        .arg(notes::NOTES_FLAG)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Collects the exit status when it ends, so the finished process is not left behind as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
