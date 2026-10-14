//! TrailMark: read, navigate and follow Markdown documents written by AI agents.
//!
//! The core modules are plain Rust with no Tauri dependency: [`vault`] (roots and path safety),
//! [`render`] (documents to HTML), [`links`], [`search`], [`watch`] (filesystem events), [`git`] and
//! [`session`] (per-project state). [`workspace`] wires them together for one open folder, and
//! [`commands`] exposes it to the UI.

mod commands;
mod error;
mod git;
mod links;
mod render;
mod search;
mod session;
mod util;
mod vault;
mod watch;
mod workspace;

use tauri::{Manager, RunEvent, WindowEvent};

use commands::AppState;

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        // Images of the open folder: answered from the folder that is open now (see `serve_asset`).
        .register_asynchronous_uri_scheme_protocol(
            commands::ASSET_SCHEME,
            |ctx, request, responder| {
                commands::serve_asset(ctx.app_handle(), request.uri(), responder);
            },
        )
        .invoke_handler(tauri::generate_handler![
            commands::open_vault,
            commands::initial_path,
            commands::recent_vaults,
            commands::list_tree,
            commands::read_document,
            commands::resolve_link,
            commands::git_snapshot,
            commands::set_git_baseline,
            commands::since_last_visit,
            commands::save_session,
            commands::load_session,
            commands::absolute_path,
            commands::open_external,
            commands::reveal_in_folder,
            commands::search,
            commands::init_project_folder,
            commands::state_location,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|handle, event| match event {
        // The last visit is saved when the window closes and again on exit.
        RunEvent::WindowEvent {
            event: WindowEvent::CloseRequested { .. },
            ..
        } => handle.state::<AppState>().save_last_visit(),
        // Back in the foreground: other tools may have changed git state meanwhile.
        RunEvent::WindowEvent {
            event: WindowEvent::Focused(true),
            ..
        } => handle.state::<AppState>().refresh_on_focus(),
        RunEvent::Exit => handle.state::<AppState>().shutdown(),
        _ => {}
    });
}
