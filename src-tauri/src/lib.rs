//! TrailMark: read, navigate and follow Markdown documents written by AI agents.
//!
//! The core modules are plain Rust with no Tauri dependency: [`vault`] (roots and path safety),
//! [`render`] (documents to HTML), [`links`], [`search`], [`watch`] (filesystem events), [`git`] and
//! [`session`] (per-project state), [`edit`] (safe saves, new files, block ranges for the editor),
//! [`fileops`] (rename, move, delete and new folders with the links kept working).
//! [`workspace`] wires them together for one open folder, and
//! [`commands`] exposes it to the UI.

mod commands;
mod edit;
mod error;
mod fileops;
mod git;
mod links;
mod meta;
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
        // In-app updates: the updater checks the GitHub release endpoint from `tauri.conf.json`,
        // the process plugin restarts the app once an update is installed.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
            commands::file_meta,
            commands::set_file_tags,
            commands::set_file_description,
            commands::list_tags,
            commands::files_with_tags,
            commands::meta_index,
            commands::clean_up_meta,
            commands::document_source,
            commands::parse_blocks,
            commands::save_document,
            commands::create_document,
            commands::write_asset,
            commands::preview_move,
            commands::apply_move,
            commands::delete_entries,
            commands::new_folder,
            commands::undo_file_op,
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
