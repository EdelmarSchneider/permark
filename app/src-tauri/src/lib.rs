//! `PerMark` app crate: Tauri shell. Wires core services to windows + IPC; holds no domain logic.

#[cfg(feature = "plugin-agent")]
pub mod agent;
mod assets;
mod commands;
pub mod crash;
mod default_app;
mod downloads_http;
mod files;
mod menu;
mod nav;
mod open;
mod plugins;
mod print;
mod renderer;
mod saver;
mod shell;
mod sink;
#[cfg(feature = "plugin-updates")]
mod updater;
mod views;
mod web_pictures;
mod windows;

use std::time::Duration;

use permark_core::ipc_local::{self, Endpoint, IpcLocalError, Listening};
use permark_core::open::{OpenAck, OpenRequest};
use permark_core::state::AppDirs;
use tauri::{Manager, RunEvent, WindowEvent};

pub use shell::Shell;

/// Why the app did not start; each text names the step that failed (shown to the person as is).
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("finding the folder for its settings failed: this system names no profile folder")]
    NoProfile,
    #[error("setting up the app window failed: {0}")]
    Tauri(#[from] tauri::Error),
}

/// What the single-instance check decided.
enum Instance {
    /// Another PerMark of this user took the request.
    Forwarded,
    /// This process serves (listening may be absent: socket unavailable -> app still runs).
    Primary(Option<Listening>),
}

/// Forwards to a running instance, else becomes the one (retrying when two start at the same moment).
fn claim(endpoint: Option<&Endpoint>, request: &OpenRequest) -> Instance {
    let Some(endpoint) = endpoint else {
        return Instance::Primary(None);
    };
    for _ in 0..5 {
        match ipc_local::request::<OpenRequest, OpenAck>(endpoint, request) {
            Ok(ack) if ack.accepted => return Instance::Forwarded,
            Ok(_)
            | Err(
                IpcLocalError::Untrusted(_)
                | IpcLocalError::NoAnswer(_)
                | IpcLocalError::BadMessage(_),
            ) => {
                // something answers but not as our PerMark: run on our own, no socket
                return Instance::Primary(None);
            }
            Err(_) => {}
        }
        match endpoint.listen() {
            Ok(listening) => return Instance::Primary(Some(listening)),
            Err(IpcLocalError::InUse(_)) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return Instance::Primary(None),
        }
    }
    Instance::Primary(None)
}

/// Builds + runs the app until the last window closes (or forwards `request` to the running app).
///
/// # Errors
/// No profile folder; Tauri fails to start.
pub fn run(request: OpenRequest) -> Result<(), StartError> {
    let dirs = AppDirs::system().ok_or(StartError::NoProfile)?;
    let endpoint = Endpoint::app(&dirs).ok();
    let listening = match claim(endpoint.as_ref(), &request) {
        Instance::Forwarded => return Ok(()),
        Instance::Primary(listening) => listening,
    };
    let app = tauri::Builder::default()
        .plugin(nav::guard(nav::dev_url()))
        .plugin(tauri_plugin_dialog::init())
        // used from Rust only (validated paths): no permission is granted to the windows
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol(assets::SCHEME, assets::handle)
        // export pages: each served only to its own hidden page window
        .manage(print::Pages::default())
        .register_asynchronous_uri_scheme_protocol(print::SCHEME, print::handle);
    // signed self-updates (direct builds; the plugin `updates` drives it, from Rust only: no window permission)
    #[cfg(feature = "plugin-updates")]
    let app = app.plugin(tauri_plugin_updater::Builder::new().build());
    // macOS: the app menu's Quit asks every window to close (the system's Quit ends the process with no question)
    #[cfg(target_os = "macos")]
    let app = app
        .menu(menu::build)
        .on_menu_event(|handle, event| menu::on_event(handle, &event));
    let app = app
        .invoke_handler(commands::handler())
        .setup(move |app| {
            let shell = Shell::new(app.handle().clone(), &dirs);
            app.manage(shell);
            let handle = app.handle().clone();
            if let Some(listening) = listening {
                shell::state(&handle).serve(listening);
            }
            windows::create_main(&handle)?;
            open::first_launch(&handle, request);
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|handle, event| match event {
        RunEvent::WindowEvent { label, event, .. } => on_window_event(handle, &label, &event),
        RunEvent::Exit => shell::state(handle).shutdown(handle),
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => open::opened_urls(handle, &urls),
        _ => {}
    });
    Ok(())
}

fn on_window_event(handle: &tauri::AppHandle, label: &str, event: &WindowEvent) {
    // an export page window is none of the person's windows (no geometry, no session, no close prompt)
    if print::is_sheet(label) {
        return;
    }
    let shell = shell::state(handle);
    match event {
        WindowEvent::Moved(_) => {
            windows::remember_geometry(handle, label);
            shell.saver.poke();
        }
        WindowEvent::Resized(size) => {
            views::fit(handle, label, *size);
            windows::remember_geometry(handle, label);
            shell.saver.poke();
        }
        WindowEvent::Focused(true) => shell.registry.focused(label, open::now_ms()),
        WindowEvent::CloseRequested { api, .. } => {
            if !request_close(handle, label) {
                api.prevent_close();
            }
        }
        WindowEvent::Destroyed => {
            let views: Vec<String> = shell
                .registry
                .views()
                .into_iter()
                .filter(|v| shell.registry.window_of(v).as_deref() == Some(label))
                .collect();
            shell.registry.forget_window(label);
            for view in views {
                views::forget(handle, &view);
            }
            views::forget_window(handle, label);
            shell.saver.poke();
        }
        _ => {}
    }
}

/// Asks the work window `label` to close, as its close button does. Unsaved edits in any tab -> one prompt in the view
/// on screen (the window closes once it is answered) and `false`. `true`: it may close now (the last window on screen
/// saved the session first, frozen until exit); the caller lets the system's close go through, or closes it.
pub(crate) fn request_close(handle: &tauri::AppHandle, label: &str) -> bool {
    let shell = shell::state(handle);
    if shell.registry.is_closing(label) {
        return true;
    }
    let outcome = shell.registry.request_close(label);
    let closes = outcome
        .changes
        .iter()
        .any(|c| matches!(c, permark_core::workspace::ViewChange::CloseWindow { .. }));
    if closes {
        // the last window: the session keeps it (saved now, frozen until exit)
        if shell.registry.is_last_on_screen(label) {
            shell.save_session_now(handle);
            shell.saver.freeze();
        }
        return true;
    }
    open::run(handle, outcome);
    false
}

#[cfg(test)]
mod packaging_tests;
