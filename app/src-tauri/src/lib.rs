//! `PerMark` app crate: Tauri shell. Wires core services to windows + IPC; holds no domain logic.

#[cfg(feature = "plugin-agent")]
pub mod agent;
mod assets;
mod commands;
mod default_app;
mod files;
mod nav;
mod open;
mod plugins;
mod saver;
mod shell;
mod sink;
#[cfg(feature = "plugin-updates")]
mod updater;
mod web_pictures;
mod windows;

use std::time::Duration;

use permark_core::ipc_local::{self, Endpoint, IpcLocalError, Listening};
use permark_core::open::{OpenAck, OpenRequest};
use permark_core::state::AppDirs;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

pub use shell::Shell;

/// Why the app did not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("no profile folder for PerMark's settings on this system")]
    NoProfile,
    #[error(transparent)]
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
    let dev_url = if cfg!(debug_assertions) {
        tauri::Url::parse("http://localhost:1420").ok()
    } else {
        None
    };
    let app = tauri::Builder::default()
        .plugin(nav::guard(dev_url))
        .plugin(tauri_plugin_dialog::init())
        // used from Rust only (validated paths): no permission is granted to the windows
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol(assets::SCHEME, assets::handle);
    // signed self-updates (direct builds; the plugin `updates` drives it, from Rust only: no window permission)
    #[cfg(feature = "plugin-updates")]
    let app = app.plugin(tauri_plugin_updater::Builder::new().build());
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
        RunEvent::Exit => shell::state(handle).shutdown(),
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => open::opened_urls(handle, &urls),
        _ => {}
    });
    Ok(())
}

fn on_window_event(handle: &tauri::AppHandle, label: &str, event: &WindowEvent) {
    let shell = shell::state(handle);
    match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            windows::remember_geometry(handle, label);
            shell.saver.poke();
        }
        WindowEvent::CloseRequested { api, .. } => {
            // unsaved edits: the window asks the person first (`window_close` closes it for real)
            if let Some(path) = shell
                .registry
                .workspace_of(label)
                .and_then(|(_, workspace)| match &workspace {
                    permark_core::workspace::Workspace::Project(core) => {
                        core.documents().unsaved(label)
                    }
                    permark_core::workspace::Workspace::SingleFile(file) => {
                        file.documents.unsaved(label)
                    }
                    permark_core::workspace::Workspace::Notes(core) => {
                        core.documents().unsaved(label)
                    }
                })
            {
                api.prevent_close();
                let _ = handle.emit_to(
                    label,
                    "close-requested",
                    commands::CloseRequested {
                        window: label.to_owned(),
                        path,
                    },
                );
                return;
            }
            // last window: the session keeps it (saved now, frozen until exit)
            if handle.webview_windows().len() <= 1 {
                shell.save_session_now(handle);
                shell.saver.freeze();
            }
        }
        WindowEvent::Destroyed => {
            shell.registry.forget_window(label);
            shell.window_gone(label);
            shell.saver.poke();
        }
        _ => {}
    }
}

#[cfg(test)]
mod packaging_tests;
