//! Plugin `notes`: the Notes window. Plain Markdown notes in a folder the person chooses, saved as typed with no name or
//! folder asked; earlier versions kept and restorable; text a crash would lose kept; loose files opened next to the
//! notes; sharing with agents note by note; moving a note into a project.
//!
//! The documents themselves are read, edited and saved by core like any workspace's (`NotesScope`); this side keeps
//! what only the notes need: the list, the pins, the versions, the journal, the loose files. App scope: one instance
//! for the whole process, serving the app's RPC table (no project needed).

#![cfg_attr(test, allow(clippy::assert_is_empty))]

pub mod folder;
pub mod journal;
mod loose;
mod moving;
pub mod names;
mod service;
pub mod state;
pub mod text;
mod types;
pub mod versions;

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::error::IpcError;
use permark_core::notes::{NotesScope, ObserverLease, SharedNote, SharedNotes, SharedNotesApi};
use permark_core::plugins::{
    typed, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running, ServiceLease,
};
use permark_core::settings::AppSettings;
use serde::Deserialize;

pub use folder::{FolderInfo, FolderStatus, Places};
pub use loose::{find_renamed, folder_label, new_id, prune as prune_loose, MAX_LISTED};
pub use names::Moment;
pub use service::{now_ms, NotesService};
pub use types::{
    Deleted, EntryKind, LooseState, MoveUndone, Moved, NoteEntry, NoteList, NotesSummary,
};
pub use versions::Version;

const ID: PluginId = PluginId::of("notes");

struct NotesPlugin {
    manifest: Manifest,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(NotesPlugin {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::App,
            requires: Requirement::NONE,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
    })
}

#[derive(Deserialize)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
struct PathsArgs {
    paths: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UndoArgs {
    undo_id: u64,
}

#[derive(Deserialize)]
struct RenameArgs {
    path: String,
    name: String,
}

#[derive(Deserialize)]
struct PinArgs {
    path: String,
    pinned: bool,
}

#[derive(Deserialize)]
struct OrderArgs {
    order: Vec<String>,
}

#[derive(Deserialize)]
struct ShareArgs {
    path: String,
    shared: bool,
}

#[derive(Deserialize)]
struct OpenedArgs {
    path: Option<String>,
}

#[derive(Deserialize)]
struct VersionArgs {
    path: String,
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BufferArgs {
    path: String,
    base_hash: String,
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocateArgs {
    path: String,
    new_path: String,
}

#[derive(Deserialize)]
struct SuggestArgs {
    path: String,
    folder: Option<String>,
}

#[derive(Deserialize)]
struct MoveArgs {
    path: String,
    folder: String,
    name: String,
}

/// What an agent may ask of the notes: the ones the person shared.
struct Shared(Arc<NotesService>);

impl SharedNotes for Shared {
    fn list(&self) -> Vec<SharedNote> {
        self.0.shared_notes()
    }

    fn read(&self, id: &str) -> Result<String, IpcError> {
        self.0.read_shared(id)
    }
}

/// Hears the saves of the Notes window: the text a save replaces becomes a version when one is due.
struct Observer(Arc<NotesService>);

impl permark_core::edit::SaveObserver for Observer {
    fn replacing(&self, path: &str, old: &[u8]) {
        self.0.before_replace(path, old, now_ms());
    }

    fn replaced(&self, path: &str) {
        self.0.after_replace(path);
    }
}

impl Plugin for NotesPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let fail = |reason: &str| PluginError::Start {
            id: ID,
            reason: reason.to_owned(),
        };
        let (Some(app), Some(store)) = (cx.app(), cx.store()) else {
            return Err(fail("needs the app services"));
        };
        let Some(scope) = app.services.get::<NotesScope>() else {
            return Err(fail("the notes scope is missing"));
        };
        let places = Places::current(store.dir());
        let settings = app.clone();
        let service = NotesService::open(
            places,
            store.clone(),
            Arc::clone(&scope),
            Box::new(move || settings.settings().notes_folder),
        );
        let mut registrations = Vec::new();
        {
            let mut serve = |method: &str, handler| registrations.extend(cx.rpc(method, handler));
            register(&mut serve, &service);
        }
        let lease = app
            .services
            .provide(Arc::new(SharedNotesApi(Arc::new(Shared(Arc::clone(
                &service,
            ))))));
        let observer = scope.install_observer(Arc::new(Observer(Arc::clone(&service))));
        Ok(Box::new(NotesRunning {
            service,
            registrations,
            lease: Some(lease),
            observer: Some(observer),
        }))
    }
}

/// A started `notes` plugin. Drop = teardown: nobody is answered, the save hook and the shared-notes service go.
struct NotesRunning {
    service: Arc<NotesService>,
    registrations: Vec<RpcRegistration>,
    lease: Option<ServiceLease>,
    observer: Option<ObserverLease>,
}

impl Running for NotesRunning {
    fn settings_changed(&self, _settings: &AppSettings) {
        self.service.settings_changed();
    }
}

impl Drop for NotesRunning {
    fn drop(&mut self) {
        self.observer.take();
        self.registrations.clear();
        self.lease.take();
    }
}

type Serve<'a> = dyn FnMut(&str, permark_core::plugins::RpcHandler) + 'a;

/// Every method the window can call: a flat list, one line of meaning each.
#[allow(clippy::too_many_lines)]
fn register(serve: &mut Serve<'_>, service: &Arc<NotesService>) {
    let s = Arc::clone(service);
    serve("folder", typed(move |_: NoArgs, _| Ok(s.folder())));
    let s = Arc::clone(service);
    serve("createFolder", typed(move |_: NoArgs, _| s.create_folder()));
    let s = Arc::clone(service);
    serve("list", typed(move |_: NoArgs, _| Ok(s.list(now_ms()))));
    let s = Arc::clone(service);
    serve(
        "summary",
        typed(move |_: NoArgs, _| Ok(s.summary(now_ms()))),
    );
    let s = Arc::clone(service);
    serve("create", typed(move |_: NoArgs, _| s.create(Moment::now())));
    let s = Arc::clone(service);
    serve(
        "delete",
        typed(move |args: PathsArgs, _| s.delete(&args.paths)),
    );
    let s = Arc::clone(service);
    serve(
        "undoDelete",
        typed(move |args: UndoArgs, _| s.undo_delete(args.undo_id)),
    );
    let s = Arc::clone(service);
    serve(
        "rename",
        typed(move |args: RenameArgs, _| s.rename(&args.path, &args.name)),
    );
    let s = Arc::clone(service);
    serve(
        "setPinned",
        typed(move |args: PinArgs, _| s.set_pinned(&args.path, args.pinned)),
    );
    let s = Arc::clone(service);
    serve(
        "setPinOrder",
        typed(move |args: OrderArgs, _| {
            s.set_pin_order(&args.order);
            Ok(())
        }),
    );
    let s = Arc::clone(service);
    serve(
        "setShared",
        typed(move |args: ShareArgs, _| s.set_shared(&args.path, args.shared)),
    );
    let s = Arc::clone(service);
    serve(
        "markCopied",
        typed(move |args: PathArgs, _| s.mark_copied(&args.path)),
    );
    let s = Arc::clone(service);
    serve("deleteCopied", typed(move |_: NoArgs, _| s.delete_copied()));
    let s = Arc::clone(service);
    serve(
        "opened",
        typed(move |args: OpenedArgs, _| {
            s.opened(args.path.as_deref(), now_ms());
            Ok(())
        }),
    );
    let s = Arc::clone(service);
    serve(
        "versions",
        typed(move |args: PathArgs, _| s.versions(&args.path)),
    );
    let s = Arc::clone(service);
    serve(
        "versionText",
        typed(move |args: VersionArgs, _| s.version_text(&args.path, &args.id)),
    );
    let s = Arc::clone(service);
    serve(
        "restoreVersion",
        typed(move |args: VersionArgs, _| s.restore_version(&args.path, &args.id, now_ms())),
    );
    let s = Arc::clone(service);
    serve(
        "buffer",
        typed(move |args: BufferArgs, _| {
            s.buffer(&args.path, &args.base_hash, args.text.as_deref(), now_ms())
        }),
    );
    let s = Arc::clone(service);
    serve(
        "recoveredText",
        typed(move |args: PathArgs, _| Ok(s.recovered_text(&args.path))),
    );
    let s = Arc::clone(service);
    serve(
        "keepRecovered",
        typed(move |args: PathArgs, _| s.keep_recovered_text(&args.path, now_ms())),
    );
    let s = Arc::clone(service);
    serve(
        "discardRecovered",
        typed(move |args: PathArgs, _| {
            s.discard_recovered(&args.path);
            Ok(())
        }),
    );
    let s = Arc::clone(service);
    serve(
        "openFile",
        typed(move |args: PathArgs, _| s.open_file(&args.path, now_ms())),
    );
    let s = Arc::clone(service);
    serve(
        "removeFile",
        typed(move |args: PathArgs, _| s.remove_file(&args.path)),
    );
    let s = Arc::clone(service);
    serve(
        "locateFile",
        typed(move |args: LocateArgs, _| s.locate_file(&args.path, &args.new_path, now_ms())),
    );
    let s = Arc::clone(service);
    serve(
        "reloadFile",
        typed(move |args: PathArgs, _| s.reload_file(&args.path)),
    );
    let s = Arc::clone(service);
    serve(
        "keepMine",
        typed(move |args: PathArgs, _| s.keep_mine(&args.path)),
    );
    let s = Arc::clone(service);
    serve(
        "saveAsNote",
        typed(move |args: PathArgs, _| s.save_as_note(&args.path)),
    );
    let s = Arc::clone(service);
    serve(
        "suggestName",
        typed(move |args: SuggestArgs, _| s.suggest_name(&args.path, args.folder.as_deref())),
    );
    let s = Arc::clone(service);
    serve(
        "moveToProject",
        typed(move |args: MoveArgs, _| s.move_to_project(&args.path, &args.folder, &args.name)),
    );
    let s = Arc::clone(service);
    serve(
        "undoMove",
        typed(move |args: UndoArgs, _| s.undo_move(args.undo_id)),
    );
}
