//! PerMark: read, navigate and follow Markdown documents written by AI agents.
//!
//! The core modules are plain Rust with no Tauri dependency: [`vault`] (roots and path safety),
//! [`render`] (documents to HTML), [`links`], [`search`], [`watch`] (filesystem events), [`git`] and
//! [`session`] (per-project state), [`edit`] (safe saves, new files, block ranges for the editor),
//! [`fileops`] (rename, move, delete and new folders with the links kept working), [`external_editor`] (the command that
//! opens a file in the person's editor).
//! [`media`] knows the pictures, sound and video of a folder, serves them in parts and decides which files the system may open.
//! [`refs`] knows which images and media the documents use (for the Explorer filter); [`unread`] knows which documents
//! changed since they were last read; [`comments`] keeps the comments a person leaves on passages.
//! [`shell_integration`] adds "Open with PerMark" to the file manager (Windows); [`open_request`] receives what the
//! macOS Finder opens with the app.
//! [`nav`] keeps the window on the app's own page, and [`window`] remembers where it is. [`workspace`] wires the
//! rest together for one open folder, and [`commands`] exposes it to the UI.
//! [`bridge`] is the agent connection: `permark mcp` runs without a window and speaks the Model Context Protocol.
//! [`distribution`] tells the build the project publishes from the one in the Microsoft Store, which updates differently.

mod bridge;
mod commands;
mod comments;
mod distribution;
mod edit;
mod error;
mod external_editor;
mod fileops;
mod git;
mod legacy_data;
mod links;
mod media;
mod meta;
mod nav;
mod notes;
mod notes_commands;
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

/// What the integration tests need to run the real program without a window against a data folder of their own.
/// Not part of the program's interface.
#[doc(hidden)]
pub mod test_support {
    use std::path::Path;

    /// The environment variable that points a debug build of the program at another data folder.
    pub const DATA_DIR_VARIABLE: &str = crate::bridge::project::TEST_DATA_DIR_VARIABLE;

    /// Remembers, in the data folder `app_data_dir`, that the person turned the agent connection of `project` on or off.
    ///
    /// # Errors
    ///
    /// The reason the switch could not be written.
    pub fn set_agent_connection(
        app_data_dir: &Path,
        project: &Path,
        on: bool,
    ) -> Result<(), String> {
        crate::session::StateStore::open(app_data_dir, project)
            .set_agent_connection(on)
            .map_err(|error| error.to_string())
    }
}

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

/// What this program is, once the Notes window has been looked for. One of these is made when the program starts.
#[allow(clippy::large_enum_variant)]
enum NotesRole {
    /// An ordinary window for a project (or the welcome screen).
    Project,
    /// The Notes window: what it was asked to show when it started (a file the file manager opened), and the socket other
    /// windows ask it over, when it could be made.
    Window(notes::NotesRequest, Option<bridge::socket::Listening>),
}

/// Decides whether this program is the Notes window (started with `--notes`, with a file to open, or with nothing to open
/// when the person chose to start in Notes) and, if so, whether one is running already: then that one is asked to come
/// forward (and to show the file) and this program ends. Otherwise this one listens, so that the next launch finds it.
fn claim_notes_window(identifier: &str) -> NotesRole {
    use std::ffi::OsString;

    let Some(app_data) = bridge::project::app_data_dir(identifier) else {
        return NotesRole::Project;
    };
    let args: Vec<OsString> = std::env::args_os().collect();
    let start_in = session::AppSettingsStore::new(&app_data)
        .load()
        .start_in
        .unwrap_or_default();
    let cwd = std::env::current_dir().ok();
    let Some(request) = notes::notes_launch(&args, start_in, cwd.as_deref()) else {
        return NotesRole::Project;
    };
    let Ok(endpoint) = bridge::socket::Endpoint::for_notes(&app_data) else {
        return NotesRole::Window(request, None);
    };
    let claimed = tauri::async_runtime::block_on(notes::claim(&endpoint, &request));
    match claimed {
        notes::Claim::Forwarded => std::process::exit(0),
        notes::Claim::Listening(listening) => NotesRole::Window(request, Some(listening)),
        // no socket: the window runs anyway, and other windows will start another one when they ask
        notes::Claim::Unavailable => NotesRole::Window(request, None),
    }
}

/// Builds and runs the Tauri application. Started as `permark mcp` or `permark context`, it instead runs that command
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
    // The folders of the identifier the app had before are copied to the new ones before anything reads or makes them
    // (a debug build keeps its data apart and never touches the person's).
    if !cfg!(debug_assertions) {
        legacy_data::carry_over_system(&context.config().identifier);
    }
    if let Some(command) = bridge::subcommand(std::env::args_os()) {
        // The app's identifier names its data folder, where the project's saved settings are.
        std::process::exit(bridge::cli::run(command, &context.config().identifier));
    }
    // what this program was started to show is read now, and is not handed on to the programs it starts
    notes_commands::read_launch_show();
    let dev_url = if cfg!(debug_assertions) {
        context.config().build.dev_url.clone()
    } else {
        None
    };
    // The Notes window is one program for each app data folder: a second launch asks it to come forward and ends.
    let notes_socket = claim_notes_window(&context.config().identifier);
    let (notes_window, notes_request, notes_listener) = match notes_socket {
        NotesRole::Window(request, listener) => (true, request, listener),
        NotesRole::Project => (false, notes::NotesRequest::default(), None),
    };
    let builder = tauri::Builder::default()
        .plugin(navigation_guard(dev_url))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    // In-app updates: the updater checks the GitHub release endpoint from `tauri.conf.json`. The Microsoft Store build
    // leaves it out: the Store updates that app, and its package cannot replace itself (see `distribution`).
    #[cfg(not(feature = "store"))]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    let app = builder
        // The process plugin restarts the app once an update is installed.
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .manage(open_request::OpenInbox::default())
        .manage(notes_commands::NotesLaunch::new(
            notes_window,
            notes_request,
        ))
        // The window starts hidden; this puts it where it was and shows it.
        .setup(move |app| {
            window::setup(app);
            if let Some(listening) = notes_listener {
                // Other windows and launches ask this one to come forward over the socket.
                notes_commands::serve_requests(app.handle(), listening);
            }
            Ok(())
        })
        // Pictures, sound and video of the open folder: answered from the folder that is open now (see `serve_asset`).
        .register_asynchronous_uri_scheme_protocol(
            commands::ASSET_SCHEME,
            |ctx, request, responder| {
                commands::serve_asset(ctx.app_handle(), &request, responder);
            },
        )
        .invoke_handler(tauri::generate_handler![
            commands::open_vault,
            commands::close_vault,
            commands::initial_path,
            commands::recent_vaults,
            commands::app_language,
            commands::app_distribution,
            commands::set_app_language,
            commands::app_tone,
            commands::set_app_tone,
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
            commands::open_with_default_app,
            commands::open_in_browser,
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
            commands::tag_styles,
            commands::set_tag_style,
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
            commands::add_comment,
            commands::list_comments,
            commands::set_comment_done,
            commands::delete_comment,
            notes_commands::launch_mode,
            notes_commands::notes_folder,
            notes_commands::set_notes_folder,
            notes_commands::create_notes_folder,
            notes_commands::notes_start_in,
            notes_commands::set_notes_start_in,
            notes_commands::notes_overview,
            notes_commands::open_notes,
            notes_commands::list_notes,
            notes_commands::new_note,
            notes_commands::save_note,
            notes_commands::note_opened,
            notes_commands::close_note,
            notes_commands::set_note_view,
            notes_commands::set_note_pinned,
            notes_commands::set_pin_order,
            notes_commands::rename_note,
            notes_commands::delete_notes,
            notes_commands::undo_delete_notes,
            notes_commands::delete_copied_notes,
            notes_commands::mark_note_copied,
            notes_commands::note_versions,
            notes_commands::note_version_text,
            notes_commands::restore_note_version,
            notes_commands::open_notes_folder,
            notes_commands::set_note_shared,
            notes_commands::move_suggestion,
            notes_commands::move_note_to_project,
            notes_commands::undo_move_note,
            notes_commands::open_in_project,
            notes_commands::launch_show,
            notes_commands::classify_path,
            notes_commands::open_loose_file,
            notes_commands::loose_status,
            notes_commands::remove_loose_file,
            notes_commands::locate_loose_file,
            notes_commands::save_pending,
            notes_commands::load_pending,
            notes_commands::clear_pending,
            notes_commands::keep_edits_as_note,
            notes_commands::show_notes,
            notes_commands::become_notes_window,
            notes_commands::take_notes_request,
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
