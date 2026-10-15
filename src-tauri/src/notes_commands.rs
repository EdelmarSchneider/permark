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
use crate::session::{AppSettingsStore, RecentStore, StartIn};
use crate::util::canonicalize_lenient;
use crate::vault::{strip_root, VaultError};
use crate::workspace::{EventSink, VaultInfo, Workspace};

/// What the program was started as, and the requests to the Notes window that it has not shown yet. Managed state.
pub struct NotesLaunch {
    is_notes_window: bool,
    pending: Mutex<Option<NotesRequest>>,
}

impl NotesLaunch {
    /// `first` is what the window was started to show (the file a launch carried); nothing when it is the default.
    pub fn new(is_notes_window: bool, first: NotesRequest) -> Self {
        Self {
            is_notes_window,
            pending: Mutex::new((first != NotesRequest::default()).then_some(first)),
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
                open_file: request.open_file.or(before.open_file),
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
    // Finder opened something while the program started as the Notes window: a file is shown here, a folder gets a window of
    // its own
    #[cfg(target_os = "macos")]
    if launch.is_notes_window {
        if let Some(path) = app
            .state::<crate::open_request::OpenInbox>()
            .take_launch_path()
        {
            if std::path::Path::new(&path).is_file() {
                launch.push(NotesRequest {
                    open_file: Some(path),
                    ..NotesRequest::default()
                });
            } else {
                let _ = crate::open_request::spawn_instance(&path);
            }
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
    let home = app.path().home_dir().ok();
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
            Workspace::open_notes(&path, &app_data, home.as_deref(), sink)
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

// ---- sharing and moving ------------------------------------------------------------------------------------

/// Shares a note with agents, or stops sharing it.
#[tauri::command]
pub async fn set_note_shared(
    state: State<'_, AppState>,
    path: String,
    shared: bool,
) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.set_shared(&path, shared)?)).await
}

/// What the Save dialog starts with when a note is moved into a project. Matches the UI's `MoveSuggestion`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveSuggestion {
    /// Where the dialog opens, with the name: in the project that was opened last when it is still there.
    pub default_path: String,
    /// The project folder it opens in, when there is one.
    pub folder: Option<String>,
}

/// The place and the name the Save dialog starts with for a note: the project opened last, and a name made from the title.
#[tauri::command]
pub async fn move_suggestion(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<MoveSuggestion, AppError> {
    let dir = app_data_dir(&app)?;
    let ws = notes_of(&state)?;
    blocking(move || {
        let folder = RecentStore::new(&dir)
            .load_existing()
            .into_iter()
            .map(|recent| PathBuf::from(recent.root))
            .find(|root| root.is_dir());
        let name = ws.notes()?.suggest_name(&path, folder.as_deref())?;
        Ok(MoveSuggestion {
            default_path: folder.as_ref().map_or_else(
                || name.clone(),
                |folder| folder.join(&name).display().to_string(),
            ),
            folder: folder.map(|folder| folder.display().to_string()),
        })
    })
    .await
}

/// Moves a note into a project: its text is written to `destination` (a file that must not be there), the note goes to the
/// recycle bin, and the copy is listed under Files.
#[tauri::command]
pub async fn move_note_to_project(
    state: State<'_, AppState>,
    path: String,
    destination: String,
) -> Result<notes::NoteMoved, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || {
        Ok(ws.notes()?.move_to(
            &path,
            std::path::Path::new(&destination),
            crate::util::now_ms(),
        )?)
    })
    .await
}

/// Undoes a move: the note is back, and the copy in the project goes unless it was changed since.
#[tauri::command]
pub async fn undo_move_note(
    state: State<'_, AppState>,
    undo_id: u64,
) -> Result<notes::MoveUndone, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.undo_move(undo_id)?)).await
}

/// The environment variable that tells a program just started on a project which of its files to show first.
const SHOW_VARIABLE: &str = "TRAILMARK_SHOW";

/// "Open in project": starts TrailMark on the project that holds `path` (the project opened lately that contains it, else its
/// folder), showing that file. A project that is open in a window already is opened once more, as a second launch does.
#[tauri::command]
pub async fn open_in_project(app: AppHandle, path: String) -> Result<(), AppError> {
    let dir = app_data_dir(&app)?;
    blocking(move || {
        let file = PathBuf::from(&path);
        if !file.is_file() {
            return Err(VaultError::NotFound(path).into());
        }
        let roots: Vec<PathBuf> = RecentStore::new(&dir)
            .load_existing()
            .into_iter()
            .map(|recent| PathBuf::from(recent.root))
            .collect();
        let folder = project_holding(&file, &roots)
            .or_else(|| file.parent().map(std::path::Path::to_path_buf))
            .ok_or_else(|| VaultError::NotFound(file.display().to_string()))?;
        start_project_window(&folder, &file).map_err(|e| AppError::Internal(e.to_string()))
    })
    .await
}

/// The deepest of `roots` that holds `file`. The case of a path and the way a folder is spelled (a short name, a link) are not
/// told apart where the system does not tell them apart, as the rest of the program does not.
fn project_holding(file: &std::path::Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let canonical =
        |path: &std::path::Path| canonicalize_lenient(path).unwrap_or_else(|| path.to_path_buf());
    let file = canonical(file);
    roots
        .iter()
        .filter(|root| strip_root(&file, &canonical(root)).is_some())
        .max_by_key(|root| root.components().count())
        .cloned()
}

/// Starts another TrailMark on a project, with the file to show first, and lets it go its own way.
fn start_project_window(folder: &std::path::Path, file: &std::path::Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};

    let mut child = Command::new(std::env::current_exe()?)
        .arg(folder)
        .env(SHOW_VARIABLE, file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// The file this program was started to show, read once, at start, and taken out of the environment: a program this one starts
/// later (another window, the Notes window, an editor, an agent's launch) would inherit it and show the file too.
pub(crate) fn take_show_variable() -> Option<String> {
    let value = std::env::var_os(SHOW_VARIABLE);
    std::env::remove_var(SHOW_VARIABLE);
    value
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
}

/// The file this program was started to show.
static LAUNCH_SHOW: Mutex<Option<String>> = Mutex::new(None);

/// Reads what the program was started to show (called once, before anything else is started).
pub(crate) fn read_launch_show() {
    *LAUNCH_SHOW.lock().unwrap_or_else(PoisonError::into_inner) = take_show_variable();
}

/// The file this program was started to show once its project is open (see [`open_in_project`]), asked once.
#[tauri::command]
pub fn launch_show() -> Option<String> {
    LAUNCH_SHOW
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

// ---- loose files -----------------------------------------------------------------------------------------

/// What kind of thing a path of the disk is, and where it is in the project this window shows. Matches the UI's `PathClass`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathClass {
    pub kind: PathKind,
    /// The path in the open project, for a file inside the folder this window shows.
    pub in_project: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PathKind {
    Folder,
    File,
    Missing,
}

/// What a path the person picked, dropped or was handed is: a folder opens as a project, a file inside the open project
/// is shown there, and any other file is a loose file of the Notes window.
#[tauri::command]
pub async fn classify_path(
    state: State<'_, AppState>,
    path: String,
) -> Result<PathClass, AppError> {
    let ws = state.current().ok();
    blocking(move || {
        let path = std::path::Path::new(&path);
        let kind = if path.is_dir() {
            PathKind::Folder
        } else if path.is_file() {
            PathKind::File
        } else {
            PathKind::Missing
        };
        let in_project = ws.and_then(|ws| ws.locate_in_project(path));
        Ok(PathClass { kind, in_project })
    })
    .await
}

/// Opens a file in the Notes window: a note when it is one of the notes folder, else a loose file, listed under Files.
/// Resolves to what to show.
#[tauri::command]
pub async fn open_loose_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<notes::OpenedFile, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || {
        Ok(ws
            .notes()?
            .open_file(std::path::Path::new(&path), crate::util::now_ms())?)
    })
    .await
}

/// Looks at every loose file: as it was read, changed since, or gone (and found again under another name).
#[tauri::command]
pub async fn loose_status(state: State<'_, AppState>) -> Result<Vec<notes::LooseStatus>, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.loose_status())).await
}

/// Takes a loose file out of the list. The file itself is not touched.
#[tauri::command]
pub async fn remove_loose_file(state: State<'_, AppState>, path: String) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.remove_file(&path)?)).await
}

/// Points a listed file at another file ("Locate..."). Resolves to what to show.
#[tauri::command]
pub async fn locate_loose_file(
    state: State<'_, AppState>,
    path: String,
    new_path: String,
) -> Result<notes::OpenedFile, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || {
        Ok(ws.notes()?.locate_file(
            &path,
            std::path::Path::new(&new_path),
            crate::util::now_ms(),
        )?)
    })
    .await
}

/// Keeps the text typed into a loose file, which is never written until the person saves.
#[tauri::command]
pub async fn save_pending(
    state: State<'_, AppState>,
    path: String,
    text: String,
    base_hash: String,
) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || {
        Ok(ws
            .notes()?
            .save_pending(&path, &text, &base_hash, crate::util::now_ms())?)
    })
    .await
}

/// The text kept for a loose file, if any.
#[tauri::command]
pub async fn load_pending(
    state: State<'_, AppState>,
    path: String,
) -> Result<Option<notes::PendingEdits>, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.load_pending(&path)?)).await
}

/// Forgets the text kept for a loose file.
#[tauri::command]
pub async fn clear_pending(state: State<'_, AppState>, path: String) -> Result<(), AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.clear_pending(&path)?)).await
}

/// Makes a note of the text typed into a loose file, "{name} (my edits)". Resolves to the note's name.
#[tauri::command]
pub async fn keep_edits_as_note(
    state: State<'_, AppState>,
    path: String,
    text: String,
) -> Result<String, AppError> {
    let ws = notes_of(&state)?;
    blocking(move || Ok(ws.notes()?.keep_edits_as_note(&path, &text)?)).await
}

/// Brings the Notes window forward, with a new note ready, a given note shown, or a file of the disk opened. Starts the
/// window when it is not running. This is what `Ctrl+Shift+N` and `Ctrl+Shift+D` do from a project window.
#[tauri::command]
pub async fn show_notes(
    app: AppHandle,
    new_note: bool,
    open: Option<String>,
    open_file: Option<String>,
) -> Result<(), AppError> {
    let dir = app_data_dir(&app)?;
    let endpoint = Endpoint::for_notes(&dir).map_err(|e| AppError::Internal(e.to_string()))?;
    let request = NotesRequest {
        new_note,
        open,
        open_file,
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The file to show first is read once, at start, and taken out of the environment: no program this one starts later (another
    /// window, the Notes window, an editor, an agent's launch) is told to show it too.
    #[test]
    fn the_file_to_show_is_read_once_and_not_handed_on_to_other_programs() {
        std::env::set_var(SHOW_VARIABLE, "C:/work/plan/ask.md");
        assert_eq!(take_show_variable().as_deref(), Some("C:/work/plan/ask.md"));
        assert!(
            std::env::var_os(SHOW_VARIABLE).is_none(),
            "a program started now does not inherit it"
        );
        assert_eq!(take_show_variable(), None, "and it is read once");
        // an empty value is no file
        std::env::set_var(SHOW_VARIABLE, "");
        assert_eq!(take_show_variable(), None);
        assert!(std::env::var_os(SHOW_VARIABLE).is_none());
    }

    /// "Open in project" starts the project that holds the file: the deepest of the recent ones, however the case of the path
    /// differs where the system does not tell it.
    #[test]
    fn the_project_that_holds_a_file_is_the_deepest_recent_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outer = dunce::canonicalize(dir.path()).expect("canonical");
        let inner = outer.join("plan");
        std::fs::create_dir_all(&inner).expect("mkdir");
        let file = inner.join("ask.md");
        std::fs::write(&file, "x").expect("write");
        let roots = [outer.clone(), inner.clone(), outer.join("elsewhere")];
        assert_eq!(project_holding(&file, &roots), Some(inner.clone()));
        assert_eq!(project_holding(&outer.join("a.md"), &roots), Some(outer));
        assert_eq!(
            project_holding(std::path::Path::new("/nowhere/x.md"), &roots),
            None
        );
        // a dialog may spell the path in another case than the recent folder was opened with
        #[cfg(windows)]
        {
            let shouting = PathBuf::from(file.display().to_string().to_uppercase());
            assert_eq!(project_holding(&shouting, &roots), Some(inner));
        }
    }
}
