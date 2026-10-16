//! Remembering the size and position of the window.
//!
//! The window is created hidden (`visible: false` in `tauri.conf.json`). [`setup`] puts it where it was
//! the last time and then shows it, so it never appears in one place and jumps to another. Afterwards
//! [`on_event`] notices that it is moved and resized, looks at it once it has been still for a moment, and
//! saves what it sees; it looks and saves once more when the window closes.
//!
//! The state is saved twice: with the open project (see [`crate::session`]) and app-wide. The project's
//! own copy is used when the app is started with that folder; the app-wide copy is for everything else
//! (the welcome screen, a folder that was never open before, a single file). [`place`] decides where a
//! saved window goes on the screens there are now, and [`track`] what is worth remembering and when to
//! look; this file only talks to the window system.

mod place;
mod track;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{
    App, AppHandle, Manager, PhysicalPosition, PhysicalSize, RunEvent, WebviewWindow, WindowEvent,
};

use crate::commands::{self, AppState};
use crate::notes_commands::NotesLaunch;
use crate::session::{saved_for_launch, WindowState};
use place::{place, Limits, Rect, Screen};
use track::{Debounce, Snapshot, Tracker};

/// The label of the window in `tauri.conf.json`.
const MAIN: &str = "main";
/// The folder in the app's data where the Notes window keeps its size and position.
const NOTES_WINDOW_DIR: &str = "notes-window";
/// The window is looked at once it has been still this long, and at least this often while it is not.
const STILL_FOR: Duration = Duration::from_millis(400);
const AT_LEAST_EVERY: Duration = Duration::from_secs(3);
/// The smallest content area, in logical pixels, when the window config does not say.
const DEFAULT_MIN: (f64, f64) = (900.0, 600.0);

/// What the app knows about the window, and where it writes it.
struct Remembered {
    tracker: Mutex<Tracker>,
    write: Box<dyn Fn(&WindowState) + Send + Sync>,
}

impl Remembered {
    /// Looks at the window and returns what is new about it.
    fn observe(&self, window: &WebviewWindow) -> Option<WindowState> {
        let snapshot = snapshot(window)?;
        self.tracker.lock().ok()?.observe(&snapshot)
    }

    /// The window has been still for a moment: writes it if it is not what was written. Without a window
    /// (the app is ending) writes what is known.
    fn settled(&self, window: Option<&WebviewWindow>) {
        let state = match window {
            Some(window) => self.observe(window),
            None => self.known(),
        };
        if let Some(state) = state {
            (self.write)(&state);
        }
    }

    /// The window is closing: writes it as it is, changed or not (a project opened in this window for the
    /// first time gets its entry here).
    fn closing(&self, window: &WebviewWindow) {
        self.observe(window);
        if let Some(state) = self.known() {
            (self.write)(&state);
        }
    }

    fn known(&self) -> Option<WindowState> {
        self.tracker.lock().ok()?.current()
    }
}

/// What follows the window while the app runs.
struct Keeper {
    remembered: Arc<Remembered>,
    /// Pokes the app to look at the window once it stops moving.
    debounce: Debounce,
}

/// Puts the window where it was and shows it, and starts following it. Called once the app is built and
/// before its event loop runs. Nothing here may stop the app from starting, so a failure only means the
/// window opens where the system puts it.
pub fn setup(app: &App) {
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    if let Ok(app_data) = commands::app_data_dir(app.handle()) {
        // The Notes window has a place of its own: it is not the project window's, and not another project's.
        let notes_window = app
            .try_state::<NotesLaunch>()
            .is_some_and(|launch| launch.is_notes_window());
        let place_dir = if notes_window {
            app_data.join(NOTES_WINDOW_DIR)
        } else {
            app_data.clone()
        };
        let opening = commands::command_line_path();
        let saved = if notes_window {
            WindowState::load(&place_dir)
        } else {
            saved_for_launch(&app_data, opening.as_deref().map(Path::new))
        };
        let placed = saved.and_then(|saved| restore(&window, &saved, min_size(app)));

        let host = app.state::<AppState>().inner().clone();
        let remembered = Arc::new(Remembered {
            tracker: Mutex::new(Tracker::new(placed)),
            write: Box::new(move |state| {
                // Best effort: a window that cannot be remembered is not worth an error.
                let _ = state.save(&place_dir);
                if !notes_window {
                    host.save_window(state);
                }
            }),
        });
        let (handle, looking) = (app.handle().clone(), Arc::clone(&remembered));
        let debounce = Debounce::new(STILL_FOR, AT_LEAST_EVERY, move || {
            looking.settled(handle.get_webview_window(MAIN).as_ref());
        });
        app.manage(Keeper {
            remembered,
            debounce,
        });
    }
    let _ = window.show();
}

/// Brings the window to the front, for a path that was opened from outside the app (macOS) and for a request to the Notes
/// window from another TrailMark window.
pub fn raise(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Follows the window. Called with every event of the app's event loop, before the app handles it.
pub fn on_event(app: &AppHandle, event: &RunEvent) {
    let Some(keeper) = app.try_state::<Keeper>() else {
        return;
    };
    match event {
        RunEvent::WindowEvent { label, event, .. } if label == MAIN => match event {
            WindowEvent::Moved(_)
            | WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. } => keeper.debounce.poke(),
            WindowEvent::CloseRequested { .. } => {
                if let Some(window) = app.get_webview_window(MAIN) {
                    keeper.debounce.cancel();
                    keeper.remembered.closing(&window);
                }
            }
            WindowEvent::Destroyed => keeper.debounce.flush(),
            _ => {}
        },
        // The app can end without a close request (a restart to update): write what is waiting.
        RunEvent::ExitRequested { .. } | RunEvent::Exit => keeper.debounce.flush(),
        _ => {}
    }
}

/// The smallest content area the window config asks for, in logical pixels.
fn min_size(app: &App) -> (f64, f64) {
    app.config()
        .app
        .windows
        .iter()
        .find(|window| window.label == MAIN)
        .map_or(DEFAULT_MIN, |window| {
            (
                window.min_width.unwrap_or(DEFAULT_MIN.0),
                window.min_height.unwrap_or(DEFAULT_MIN.1),
            )
        })
}

/// How the window looks now, or `None` when the system will not say.
fn snapshot(window: &WebviewWindow) -> Option<Snapshot> {
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    Some(Snapshot {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        scale: window.scale_factor().ok()?,
        maximized: window.is_maximized().ok()?,
        minimized: window.is_minimized().ok()?,
        fullscreen: window.is_fullscreen().ok()?,
    })
}

/// The screens there are now.
fn screens(window: &WebviewWindow) -> Option<Vec<Screen>> {
    let primary = window.primary_monitor().ok()?;
    let monitors = window.available_monitors().ok()?;
    Some(
        monitors
            .iter()
            .map(|monitor| {
                let work = monitor.work_area();
                Screen {
                    work: Rect {
                        x: work.position.x,
                        y: work.position.y,
                        width: work.size.width,
                        height: work.size.height,
                    },
                    scale: monitor.scale_factor(),
                    primary: primary.as_ref().is_some_and(|primary| {
                        primary.position() == monitor.position() && primary.size() == monitor.size()
                    }),
                }
            })
            .collect(),
    )
}

/// What the title bar and borders add to the content area, in logical pixels.
fn frame(window: &WebviewWindow) -> Option<(f64, f64)> {
    let outer = window.outer_size().ok()?;
    let inner = window.inner_size().ok()?;
    let scale = window.scale_factor().ok()?;
    Some((
        f64::from(outer.width.saturating_sub(inner.width)) / scale,
        f64::from(outer.height.saturating_sub(inner.height)) / scale,
    ))
}

/// Moves and sizes the hidden window to where `saved` goes now. Returns the state the window has then.
fn restore(window: &WebviewWindow, saved: &WindowState, min: (f64, f64)) -> Option<WindowState> {
    let limits = Limits {
        min,
        frame: frame(window)?,
    };
    let at = place(saved, &screens(window)?, limits)?;
    // The position first: moving to a screen with another scale changes what a size means.
    window
        .set_position(PhysicalPosition::new(at.x, at.y))
        .ok()?;
    window
        .set_size(PhysicalSize::new(at.width, at.height))
        .ok()?;
    // What the window has as a normal window, before it is maximized.
    let normal = snapshot(window)?;
    if at.maximized {
        window.maximize().ok()?;
    }
    Some(WindowState {
        x: normal.x,
        y: normal.y,
        width: normal.width,
        height: normal.height,
        scale: normal.scale,
        maximized: at.maximized,
    })
}
