//! Opening: first launch (request | session restore), forwarded requests, macOS open events, opens in a window.

use std::collections::BTreeSet;
use std::path::Path;

use permark_core::events::OpenFailed;
use permark_core::open::{create_untitled_markdown, route, OpenRequest, Route};
use permark_core::paths::{Root, VaultPath};
use permark_core::settings::StartIn;
use permark_core::workspace::{OpenError, WorkspaceId, WorkspaceRef};
use permark_core::IpcError;
use tauri::{AppHandle, Manager};

use crate::shell::state;
use crate::windows::{self, MAIN};

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Tells `window` that opening `path` failed (it shows a notice).
fn report(handle: &AppHandle, window: &str, path: &Path, error: IpcError) {
    state(handle).report_open_failed(OpenFailed {
        window: window.to_owned(),
        path: path.display().to_string(),
        error,
    });
}

/// Opens `reference` in window `label` (any thread). Projects go to the recent list; failures are reported to the
/// window (except `Superseded`: a newer open in that window wins quietly).
///
/// # Errors
/// The open error, in IPC form.
pub fn open_in(
    handle: &AppHandle,
    label: &str,
    reference: &WorkspaceRef,
    document: Option<VaultPath>,
    report_failure: bool,
) -> Result<WorkspaceId, IpcError> {
    let shell = state(handle);
    match shell.registry.open(label, reference, document) {
        Ok(id) => {
            if let WorkspaceRef::Project(root) = reference {
                let _ = shell.recent.touch(root, now_ms());
            }
            shell.saver.poke();
            Ok(id)
        }
        Err(OpenError::Superseded) => Err(OpenError::Superseded.into()),
        Err(error) => {
            let error = IpcError::from(error);
            let path = match reference {
                WorkspaceRef::Project(root) => root.path().to_path_buf(),
                WorkspaceRef::SingleFile(path) => path.clone(),
                WorkspaceRef::Notes => "notes".into(),
            };
            if report_failure {
                report(handle, label, &path, error.clone());
            }
            Err(error)
        }
    }
}

/// Opens in a new window.
pub fn open_in_new_window(
    handle: &AppHandle,
    reference: &WorkspaceRef,
    document: Option<VaultPath>,
) -> Option<String> {
    let window = windows::create(handle).ok()?;
    let label = window.label().to_owned();
    let _ = open_in(handle, &label, reference, document, true);
    Some(label)
}

/// First launch: the request's paths, else the saved session.
pub fn first_launch(handle: &AppHandle, request: OpenRequest) {
    if request.is_empty() {
        restore_session(handle);
    } else {
        let handle = handle.clone();
        std::thread::spawn(move || handle_request(&handle, &request));
    }
}

/// Windows of the last session come back (main = first); missing paths -> that window stays on Welcome.
fn restore_session(handle: &AppHandle) {
    let shell = state(handle);
    let session = shell.load_session();
    // the person chose to start on the Notes: the first window that shows nothing opens them
    let notes_first = shell.env.settings().start_in == StartIn::Notes;
    if session.windows.is_empty() && notes_first {
        let (handle, label) = (handle.clone(), MAIN.to_owned());
        std::thread::spawn(move || {
            let _ = open_in(&handle, &label, &WorkspaceRef::Notes, None, true);
        });
        return;
    }
    for (index, saved) in session.windows.into_iter().enumerate() {
        let window = if index == 0 {
            handle.get_webview_window(MAIN)
        } else {
            windows::create(handle).ok()
        };
        let Some(window) = window else {
            continue;
        };
        if let Some(geometry) = &saved.geometry {
            windows::restore(&window, geometry);
        }
        let saved_reference = saved
            .workspace
            .and_then(|dto| WorkspaceRef::parse(&dto).ok());
        if let Some(reference) =
            saved_reference.or_else(|| (index == 0 && notes_first).then_some(WorkspaceRef::Notes))
        {
            let (handle, label) = (handle.clone(), window.label().to_owned());
            std::thread::spawn(move || {
                let _ = open_in(&handle, &label, &reference, None, true);
            });
        }
    }
}

/// A second launch forwarded its request.
pub fn forwarded(handle: &AppHandle, request: &OpenRequest) {
    handle_request(handle, request);
    if request.is_empty() {
        // plain second start: bring the app forward
        if let Some(label) = state(handle)
            .registry
            .session_windows()
            .first()
            .map(|(l, _)| l.clone())
        {
            windows::focus(handle, &label);
        }
    }
}

/// One thing a request asks for: what (or why not) + the document to show.
type Item = (
    Result<WorkspaceRef, (std::path::PathBuf, IpcError)>,
    Option<VaultPath>,
);

/// Routes each item of a request (focus | free Welcome window | new window).
fn handle_request(handle: &AppHandle, request: &OpenRequest) {
    let shell = state(handle);
    let mut items: Vec<Item> = Vec::new();
    if let Some(folder) = &request.flags.new_markdown_in {
        let made = create_untitled_markdown(folder)
            .map_err(|e| IpcError::io(folder.display().to_string(), &e))
            .and_then(|file| {
                let root = Root::open(folder).map_err(IpcError::from)?;
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Ok((root, VaultPath::parse(&name).ok()))
            });
        match made {
            Ok((root, document)) => items.push((Ok(WorkspaceRef::Project(root)), document)),
            Err(error) => items.push((Err((folder.clone(), error)), None)),
        }
    }
    if request.flags.notes {
        items.push((Ok(WorkspaceRef::Notes), None));
    }
    for path in &request.paths {
        let reference = WorkspaceRef::classify(path).map_err(|e| (path.clone(), IpcError::from(e)));
        items.push((reference, None));
    }
    let mut claimed = BTreeSet::new();
    for (reference, document) in items {
        let reference = match reference {
            Ok(reference) => reference,
            Err((path, error)) => {
                let window = shell
                    .registry
                    .welcome_windows()
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| MAIN.to_owned());
                report(handle, &window, &path, error);
                windows::focus(handle, &window);
                continue;
            }
        };
        match route(
            &shell.registry,
            &reference,
            request.flags.new_window,
            &mut claimed,
        ) {
            Route::Focus {
                window,
                document: inside,
            } => {
                if let Some(document) = inside.or(document) {
                    shell.registry.set_document(&window, Some(document));
                }
                windows::focus(handle, &window);
            }
            Route::OpenIn { window, .. } => {
                let _ = open_in(handle, &window, &reference, document, true);
                windows::focus(handle, &window);
            }
            Route::NewWindow { .. } => {
                if let Some(window) = open_in_new_window(handle, &reference, document) {
                    windows::focus(handle, &window);
                }
            }
        }
    }
}

/// macOS Finder | Dock | "Open With": same path as a forwarded request.
#[cfg(target_os = "macos")]
pub fn opened_urls(handle: &AppHandle, urls: &[tauri::Url]) {
    let paths = urls
        .iter()
        .filter(|url| url.scheme() == "file")
        .filter_map(|url| url.to_file_path().ok())
        .collect();
    let request = OpenRequest {
        paths,
        ..OpenRequest::default()
    };
    let handle = handle.clone();
    std::thread::spawn(move || handle_request(&handle, &request));
}
