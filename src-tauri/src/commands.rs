//! Thin `#[tauri::command]` adapters: validate arguments, move blocking work off the async runtime,
//! and call the Tauri-free core in [`crate::workspace`]. Command names are the `snake_case` forms of the
//! methods in `src/lib/api.ts`; argument names match what `api.ts` sends.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::AppError;
use crate::git::{BaselineKind, GitSnapshot};
use crate::links::LinkTarget;
use crate::render::DocumentView;
use crate::search::SearchHit;
use crate::session::{RecentStore, RecentVault, SessionState, StateLocation};
use crate::vault::TreeNode;
use crate::watch::FsBatch;
use crate::workspace::{ChangedSince, EventSink, GuardedSink, VaultInfo, Workspace};

/// The folder that is currently open, if any.
#[derive(Default)]
pub struct AppState {
    workspace: Mutex<Option<Arc<Workspace>>>,
    /// Identifies the workspace allowed to emit events (see [`GuardedSink`]).
    epoch: Arc<AtomicU64>,
}

impl AppState {
    fn current(&self) -> Result<Arc<Workspace>, AppError> {
        self.workspace
            .lock()
            .map_err(|_| AppError::Internal("application state is unavailable".to_owned()))?
            .clone()
            .ok_or(AppError::NoVault)
    }

    /// Installs a workspace and returns the previous one.
    fn replace(&self, next: Option<Arc<Workspace>>) -> Option<Arc<Workspace>> {
        let mut slot = self.workspace.lock().ok()?;
        std::mem::replace(&mut *slot, next)
    }

    /// Remembers "now" as the last visit without closing anything (window close).
    pub fn save_last_visit(&self) {
        if let Ok(ws) = self.current() {
            ws.save_last_visit();
        }
    }

    /// Window focus: external tools may have changed git state while we were in the background.
    pub fn refresh_git(&self) {
        if let Ok(ws) = self.current() {
            ws.refresh_git();
        }
    }

    /// Application exit: save the last visit and stop everything.
    pub fn shutdown(&self) {
        if let Some(ws) = self.replace(None) {
            ws.close();
        }
    }
}

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
    // From here on only the workspace being opened may emit events.
    let epoch = state.epoch.fetch_add(1, Ordering::SeqCst) + 1;
    let sink: Arc<dyn EventSink> = Arc::new(GuardedSink::new(
        Arc::new(TauriSink(app.clone())),
        epoch,
        Arc::clone(&state.epoch),
    ));
    let opened = blocking(move || {
        Workspace::open(Path::new(&path), &app_data, home.as_deref(), &sink).map(Arc::new)
    })
    .await;
    let workspace = match opened {
        Ok(ws) => ws,
        Err(e) => {
            // The previous folder stays open, so it may emit again.
            let _ =
                state
                    .epoch
                    .compare_exchange(epoch, epoch - 1, Ordering::SeqCst, Ordering::SeqCst);
            return Err(e);
        }
    };

    // Images in the documents are served through the asset protocol, scoped to the open roots.
    let scope = app.asset_protocol_scope();
    for root in workspace.root_paths() {
        scope
            .allow_directory(&root, true)
            .map_err(|e| AppError::Internal(e.to_string()))?;
    }
    let info = workspace.info();
    // Only now stop the previous folder, so a failed open leaves the current one untouched.
    if let Some(previous) = state.replace(Some(workspace)) {
        blocking(move || {
            previous.close();
            Ok(())
        })
        .await?;
    }
    Ok(info)
}

/// The path given on the command line (`trailmark <path>`), if it exists. Relative paths resolve
/// against the current directory.
#[tauri::command]
pub fn initial_path() -> Option<String> {
    let arg = std::env::args_os().nth(1)?;
    initial_path_from(arg, std::env::current_dir().ok())
}

fn initial_path_from(arg: OsString, cwd: Option<PathBuf>) -> Option<String> {
    if arg.to_string_lossy().starts_with('-') {
        return None; // a flag, not a path
    }
    let path = PathBuf::from(arg);
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

#[tauri::command]
pub async fn list_tree(state: State<'_, AppState>) -> Result<TreeNode, AppError> {
    let ws = state.current()?;
    blocking(move || Ok(ws.tree())).await
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

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>, AppError> {
    let ws = state.current()?;
    blocking(move || Ok(ws.search(&query, limit))).await
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn app_state_starts_empty_and_reports_no_vault() {
        let state = AppState::default();
        assert!(matches!(state.current(), Err(AppError::NoVault)));
        state.save_last_visit();
        state.refresh_git();
        state.shutdown();
    }
}
