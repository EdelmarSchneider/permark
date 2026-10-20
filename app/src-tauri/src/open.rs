//! Opening: placement of opens from the app (tab | this tab | new window), opens from outside (second launch, CLI,
//! macOS open events), first launch (lazy session restore), the loads a registry change starts, moving a tab.

use std::path::{Path, PathBuf};

use permark_core::events::OpenFailed;
use permark_core::open::{
    create_untitled_markdown, place_open, route, OpenPlacement, OpenRequest, Placed, Route,
};
use permark_core::paths::{Root, VaultPath};
use permark_core::session::{restore_plan, AppSession};
use permark_core::settings::StartIn;
use permark_core::state::older::older_window;
use permark_core::workspace::{
    Begin, MoveTo, OpenError, Outcome, Place, TabId, TabOpen, Ticket, WindowView, WorkspaceRef,
};
use permark_core::{ErrorKind, IpcError};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use ts_rs::TS;

use crate::shell::state;
use crate::windows::{self, MAIN};

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "outcome", rename_all = "camelCase")]
#[ts(export)]
pub enum OpenOutcome {
    /// Opened in the asking view (this tab).
    Opened {
        view: WindowView,
    },
    /// A new tab of this window (it loads on its own).
    NewTab {
        tab: TabId,
    },
    /// It was open already: that window + tab came forward.
    Focused {
        window: String,
        tab: TabId,
    },
    OpenedElsewhere {
        window: String,
    },
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn path_of(reference: &WorkspaceRef) -> PathBuf {
    match reference {
        WorkspaceRef::Project(root) => root.path().to_path_buf(),
        WorkspaceRef::SingleFile(path) => path.clone(),
        WorkspaceRef::Notes => "notes".into(),
    }
}

/// Tells `view` that opening `path` failed (it shows a notice; kept until it boots).
fn report(handle: &AppHandle, view: &str, path: &Path, error: IpcError) {
    state(handle).report_open_failed(OpenFailed {
        window: view.to_owned(),
        path: path.display().to_string(),
        error,
    });
}

/// Tells the view `window` shows (or would show) that opening `path` failed.
fn report_in_window(handle: &AppHandle, window: &str, path: &Path, error: IpcError) {
    let shell = state(handle);
    if shell.registry.shown_view(window).is_none() {
        run(handle, shell.registry.show_welcome_view(window));
    }
    if let Some(view) = shell.registry.shown_view(window) {
        report(handle, &view, path, error);
    }
}

/// Runs what a registry change asks: webview changes, then the opens it started (each on its own thread).
pub fn run(handle: &AppHandle, outcome: Outcome) {
    crate::views::apply(handle, outcome.changes);
    for (ticket, reference) in outcome.loads {
        let handle = handle.clone();
        std::thread::spawn(move || {
            let _ = load(&handle, ticket, &reference);
        });
    }
    state(handle).saver.poke();
}

/// Builds + binds a reserved open. A tab whose first open failed closes (its view would show the Welcome) and the
/// window's view on screen tells why; a newer open winning stays quiet.
fn load(handle: &AppHandle, ticket: Ticket, reference: &WorkspaceRef) -> Result<(), IpcError> {
    let shell = state(handle);
    let view = ticket.view().to_owned();
    match shell.registry.load(ticket, reference) {
        Ok(_) => {
            if let WorkspaceRef::Project(root) = reference {
                let _ = shell.recent.touch(root, now_ms());
            }
            shell.saver.poke();
            Ok(())
        }
        Err(OpenError::Superseded) => Err(OpenError::Superseded.into()),
        Err(error) => {
            let error = IpcError::from(error);
            let path = path_of(reference);
            let place = shell.registry.place_of_view(&view);
            let empty = shell.registry.window_view(&view) == Some(WindowView::Welcome);
            match place {
                Some(place) if empty => {
                    run(handle, shell.registry.close_tab(place.tab));
                    report_in_window(handle, &place.window, &path, error.clone());
                }
                _ => report(handle, &view, &path, error.clone()),
            }
            Err(error)
        }
    }
}

/// The window comes forward on that tab (a pending tab opens, on `document` when given).
pub fn focus_place(handle: &AppHandle, place: &Place, document: Option<VaultPath>) {
    let shell = state(handle);
    run(handle, shell.registry.activate_with(place, document));
    windows::focus(handle, &place.window);
}

fn focused(place: &Place) -> OpenOutcome {
    OpenOutcome::Focused {
        window: place.window.clone(),
        tab: place.tab,
    }
}

/// A new tab in `window` (or where the workspace already is).
///
/// # Errors
/// The window is closing | asking to close, unknown window.
pub fn open_tab(
    handle: &AppHandle,
    window: &str,
    reference: &WorkspaceRef,
    document: Option<VaultPath>,
) -> Result<OpenOutcome, IpcError> {
    let shell = state(handle);
    match shell
        .registry
        .open_tab(window, reference, document.clone())?
    {
        TabOpen::Opening { tab, outcome, .. } => {
            run(handle, outcome);
            Ok(OpenOutcome::NewTab { tab })
        }
        TabOpen::Existing(place) => {
            focus_place(handle, &place, document);
            Ok(focused(&place))
        }
    }
}

/// A new work window with `reference` as its one tab.
///
/// # Errors
/// The window cannot be made.
pub fn open_in_new_window(
    handle: &AppHandle,
    reference: &WorkspaceRef,
    document: Option<VaultPath>,
) -> Result<String, IpcError> {
    let window = windows::create(handle)
        .map_err(|e| IpcError::new(ErrorKind::Internal).detail(e.to_string()))?;
    let label = window.label().to_owned();
    open_tab(handle, &label, reference, document)?;
    Ok(label)
}

/// An open from the app's own UI in `view`: where it is already (one place), else by `placement` (`place_open`).
///
/// # Errors
/// The open error (this-tab opens), unknown view, the window cannot be made.
pub fn open_placed(
    handle: &AppHandle,
    view: &str,
    reference: &WorkspaceRef,
    document: Option<VaultPath>,
    placement: OpenPlacement,
) -> Result<OpenOutcome, IpcError> {
    let shell = state(handle);
    let placed = place_open(&shell.registry, view, reference, placement)
        .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(view.to_owned()))?;
    match placed {
        Placed::Focus {
            place,
            document: inside,
        } => {
            focus_place(handle, &place, inside.or(document));
            Ok(focused(&place))
        }
        Placed::NewWindow => {
            let window = open_in_new_window(handle, reference, document)?;
            Ok(OpenOutcome::OpenedElsewhere { window })
        }
        Placed::Tab { window } => open_tab(handle, &window, reference, document),
        Placed::ThisTab => match shell
            .registry
            .begin_open(view, reference, document.clone())?
        {
            Begin::Existing(place) => {
                focus_place(handle, &place, document);
                Ok(focused(&place))
            }
            Begin::Ticket(ticket) => {
                load(handle, ticket, reference)?;
                Ok(OpenOutcome::Opened {
                    view: shell
                        .registry
                        .window_view(view)
                        .unwrap_or(WindowView::Welcome),
                })
            }
        },
    }
}

/// Moves a tab into a new window (cascaded from its window); its view goes along, workspace + unsaved edits kept.
///
/// # Errors
/// Unknown tab, the window cannot be made.
pub fn move_to_new_window(handle: &AppHandle, tab: TabId) -> Result<String, IpcError> {
    let shell = state(handle);
    let not_found = || IpcError::new(ErrorKind::NotFound).subject(tab.0.to_string());
    let from = shell.registry.window_of_tab(tab).ok_or_else(not_found)?;
    let target = windows::create(handle)
        .map_err(|e| IpcError::new(ErrorKind::Internal).detail(e.to_string()))?;
    let (outcome, moved) = shell.registry.move_out(tab, MoveTo::NewWindow);
    let Some(moved) = moved else {
        let _ = target.destroy();
        return Err(IpcError::new(ErrorKind::Unavailable).subject(from));
    };
    if let Some(view) = moved.view() {
        if let Some(webview) = handle.get_webview(view) {
            webview
                .reparent(&target)
                .map_err(|e| IpcError::new(ErrorKind::Internal).detail(e.to_string()))?;
        }
    }
    let label = target.label().to_owned();
    let arrived = shell.registry.take_in(&label, moved);
    run(handle, outcome);
    run(handle, arrived);
    // a moved view has painted already: the new window shows at once
    crate::views::show_window(handle, &label);
    Ok(label)
}

// ---- first launch -----------------------------------------------------------------------------------------------

/// First launch: the saved session comes back (lazy: each window loads its active tab only), then the request's
/// paths open as outside opens. `main` takes the first saved window's place (else 0.5's last window's). Runs on its
/// own thread (views are made with a round trip to the main thread).
pub fn first_launch(handle: &AppHandle, request: OpenRequest) {
    let shell = state(handle);
    let session = shell.load_session();
    let geometry = session.first_geometry(|| older_window(&shell.env.dirs));
    if let (Some(main), Some(geometry)) = (handle.get_window(MAIN), geometry) {
        windows::restore(&main, &geometry);
    }
    let handle = handle.clone();
    std::thread::spawn(move || {
        restore_session(&handle, &session, request.is_empty());
        if !request.is_empty() {
            handle_request(&handle, &request);
        }
    });
}

fn notes_on(handle: &AppHandle) -> bool {
    state(handle)
        .env
        .app_plugins_on()
        .contains(&permark_core::plugins::PluginId::of("notes"))
}

/// The windows of the last session: tabs restored as pending, each window's active tab loaded; missing paths ->
/// a notice in that window. Nothing saved -> `main` on Welcome (or the Notes, `start_in = notes`, nothing to open).
fn restore_session(handle: &AppHandle, session: &AppSession, nothing_asked: bool) {
    let shell = state(handle);
    let plan = restore_plan(session, |dto| WorkspaceRef::parse(dto).ok());
    // stage A: the Notes are a tab (of the first window)
    let notes_tab = notes_on(handle)
        && (session.notes.open
            || (plan.windows.is_empty()
                && nothing_asked
                && shell.env.settings().start_in == StartIn::Notes));
    if plan.windows.is_empty() {
        if notes_tab {
            let _ = open_tab(handle, MAIN, &WorkspaceRef::Notes, None);
        } else {
            run(handle, shell.registry.show_welcome_view(MAIN));
        }
        return;
    }
    for (index, planned) in plan.windows.iter().enumerate() {
        let label = if index == 0 {
            MAIN.to_owned()
        } else {
            match windows::create_at(handle, planned.geometry.as_ref()) {
                Ok(window) => window.label().to_owned(),
                Err(_) => continue,
            }
        };
        let tabs: Vec<Option<TabId>> = planned
            .tabs
            .iter()
            .map(|reference| shell.registry.restore_tab(&label, reference, None))
            .collect();
        if index == 0 && notes_tab {
            shell
                .registry
                .restore_tab(&label, &WorkspaceRef::Notes, None);
        }
        match planned.active.and_then(|a| tabs.get(a).copied().flatten()) {
            Some(active) => run(handle, shell.registry.activate(active)),
            None => run(handle, shell.registry.show_welcome_view(&label)),
        }
        for path in &planned.missing {
            report_in_window(
                handle,
                &label,
                Path::new(path),
                IpcError::new(ErrorKind::NotFound).subject(path.clone()),
            );
        }
    }
}

// ---- opens from outside -----------------------------------------------------------------------------------------

/// A second launch forwarded its request.
pub fn forwarded(handle: &AppHandle, request: &OpenRequest) {
    if request.is_empty() && !request.flags.new_window {
        // plain second start: bring the app forward
        if let Some(window) = state(handle).registry.last_focused_work() {
            windows::focus(handle, &window);
        }
        return;
    }
    handle_request(handle, request);
}

/// One thing a request asks for: what (or why not) + the document to show.
type Item = (Result<WorkspaceRef, (PathBuf, IpcError)>, Option<VaultPath>);

fn items_of(request: &OpenRequest) -> Vec<Item> {
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
    items
}

/// Routes each path of a request in order: open already -> its window + tab forward; else a tab in the work window
/// focused last (`open_in = window` | `--new-window` -> one new window the later paths join); no window -> a new one.
/// The last path's tab ends active. `--new-window` alone -> an empty new window.
fn handle_request(handle: &AppHandle, request: &OpenRequest) {
    let shell = state(handle);
    let open_in = shell.env.settings().open_in;
    let mut joined: Option<String> = None;
    let mut last: Option<String> = None;
    let items = items_of(request);
    if items.is_empty() && request.flags.new_window {
        if let Ok(window) = windows::create(handle) {
            run(handle, shell.registry.show_welcome_view(window.label()));
            last = Some(window.label().to_owned());
        }
    }
    for (reference, document) in items {
        let reference = match reference {
            Ok(reference) => reference,
            Err((path, error)) => {
                let window = shell
                    .registry
                    .last_focused_work()
                    .unwrap_or_else(|| MAIN.to_owned());
                report_in_window(handle, &window, &path, error);
                last = Some(window);
                continue;
            }
        };
        let routed = route(
            &shell.registry,
            &reference,
            request.flags.new_window,
            open_in,
            joined.as_deref(),
        );
        match routed {
            Route::Focus {
                place,
                document: inside,
            } => {
                run(
                    handle,
                    shell.registry.activate_with(&place, inside.or(document)),
                );
                last = Some(place.window);
            }
            Route::Tab { window } => {
                if open_tab(handle, &window, &reference, document.clone()).is_ok() {
                    last = Some(window);
                } else if let Ok(window) = open_in_new_window(handle, &reference, document) {
                    // the window started closing meanwhile: a new one
                    last = Some(window);
                }
            }
            Route::NewWindow => {
                if let Ok(window) = open_in_new_window(handle, &reference, document) {
                    joined = Some(window.clone());
                    last = Some(window);
                }
            }
        }
    }
    if let Some(window) = last {
        windows::focus(handle, &window);
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
