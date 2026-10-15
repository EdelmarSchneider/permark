//! TrailMark: read, navigate and follow Markdown documents written by AI agents.
//!
//! The core modules are plain Rust with no Tauri dependency: [`vault`] (roots and path safety),
//! [`render`] (documents to HTML), [`links`], [`search`], [`watch`] (filesystem events), [`git`] and
//! [`session`] (per-project state), [`edit`] (safe saves, new files, block ranges for the editor),
//! [`fileops`] (rename, move, delete and new folders with the links kept working), [`external_editor`] (the command that
//! opens a file in the person's editor).
//! [`refs`] knows which images the documents use (for the Explorer filter); [`unread`] knows which documents
//! changed since they were last read.
//! [`shell_integration`] adds "Open with TrailMark" to the file manager (Windows); [`open_request`] receives what the
//! macOS Finder opens with the app.
//! [`nav`] keeps the window on the app's own page, and [`window`] remembers where it is. [`workspace`] wires the
//! rest together for one open folder, and [`commands`] exposes it to the UI.
//! [`bridge`] is the agent connection: `trailmark mcp` runs without a window and speaks the Model Context Protocol.

mod bridge;
mod commands;
mod edit;
mod error;
mod external_editor;
mod fileops;
mod git;
mod links;
mod meta;
mod nav;
mod open_request;
mod refs;
mod render;
mod search;
mod session;
mod shell_integration;
mod unread;
mod util;
mod vault;
mod watch;
mod window;
mod workspace;

use tauri::plugin::{Builder as PluginBuilder, TauriPlugin};
use tauri::{Manager, RunEvent, Url, WindowEvent, Wry};

use commands::AppState;

/// A plugin that stops the window from navigating away from the app (see [`nav`]). The development server counts as the app in a
/// debug build.
fn navigation_guard(dev_url: Option<Url>) -> TauriPlugin<Wry> {
    PluginBuilder::new("navigation-guard")
        .on_navigation(move |_webview, url| nav::is_app_url(url, dev_url.as_ref()))
        .build()
}

/// Builds and runs the Tauri application. Started as `trailmark mcp` or `trailmark context`, it instead runs that command
/// without a window (see [`bridge`]): `mcp` serves MCP over standard input and output until the input closes, `context`
/// prints what the person is looking at and exits.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
// The app's command list is one expression, and it grows with the app.
#[allow(clippy::too_many_lines)]
pub fn run() {
    let context = tauri::generate_context!();
    if let Some(command) = bridge::subcommand(std::env::args_os()) {
        // The app's identifier names its data folder, where the project's saved settings are.
        std::process::exit(bridge::cli::run(command, &context.config().identifier));
    }
    let dev_url = if cfg!(debug_assertions) {
        context.config().build.dev_url.clone()
    } else {
        None
    };
    let app = tauri::Builder::default()
        .plugin(navigation_guard(dev_url))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // In-app updates: the updater checks the GitHub release endpoint from `tauri.conf.json`,
        // the process plugin restarts the app once an update is installed.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .manage(open_request::OpenInbox::default())
        // The window starts hidden; this puts it where it was and shows it.
        .setup(|app| {
            window::setup(app);
            Ok(())
        })
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
            commands::app_language,
            commands::set_app_language,
            commands::shell_integration_state,
            commands::set_shell_context_menu,
            commands::set_shell_file_association,
            commands::refresh_shell_labels,
            commands::open_default_apps,
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
            commands::open_in_editor,
            commands::editor_command,
            commands::set_editor_command,
            commands::reveal_in_folder,
            commands::search,
            commands::referenced_images,
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
            commands::render_fragments,
            commands::save_document,
            commands::create_document,
            commands::write_asset,
            commands::preview_move,
            commands::apply_move,
            commands::delete_entries,
            commands::new_folder,
            commands::undo_file_op,
            commands::get_unread,
            commands::mark_read,
            commands::mark_unread,
            commands::mark_all_read,
            commands::mark_folder_read,
            commands::set_agent_connection,
            commands::agent_status,
            commands::publish_agent_context,
            commands::take_agent_open,
        ])
        .build(context)
        .expect("error while building tauri application");

    app.run(|handle, event| {
        window::on_event(handle, &event);
        match event {
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
            // Finder opened a file or folder with the app (double-click, "Open With", a drop on the Dock icon).
            #[cfg(target_os = "macos")]
            RunEvent::Opened { urls } => commands::opened(handle, &urls),
            _ => {}
        }
    });
}
