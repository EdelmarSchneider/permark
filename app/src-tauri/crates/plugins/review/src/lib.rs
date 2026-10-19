//! Plugin `review`: what changed in a document since the person last read it, shown in the page, block by block, with
//! Keep and Revert for each change (or all), and Undo. Needs the `unread` plugin (it follows what was read). Revert
//! is a surgical line save through the editor: every other byte of the file stays; a file that changed meanwhile is
//! never overwritten.

#![cfg_attr(test, allow(clippy::assert_is_empty))]

mod diff;
mod service;
mod splice;
mod store;
#[cfg(test)]
mod tests;
mod types;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::error::IpcError;
use permark_core::paths::VaultPath;
use permark_core::plugins::{
    typed, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running,
};
use permark_plugin_unread::UnreadHub;
use serde::Deserialize;

pub use diff::{changes, content_of, Change, Kind};
pub use service::{Files, HeldFile, Prefs, ReviewService};
pub use splice::{edit_for, invert, CannotSplice, Span, Text};
pub use store::{CopyStore, MAX_COPY_BYTES};
pub use types::{Basis, ReviewChange, ReviewChangeKind, ReviewState, ReviewStatus, ReviewView};

const ID: PluginId = PluginId::of("review");

struct ReviewPlugin {
    manifest: Manifest,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(ReviewPlugin {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::Project,
            requires: Requirement::PROJECT,
            depends_on: Cow::Owned(vec![PluginId::of("unread")]),
            default_enabled: true,
        },
    })
}

#[derive(Deserialize)]
struct PathArg {
    path: String,
}

#[derive(Deserialize)]
struct OpenArgs {
    path: String,
    basis: Option<Basis>,
}

#[derive(Deserialize)]
struct ChangeArgs {
    path: String,
    /// The version of the file the change was drawn from.
    hash: String,
    index: usize,
    basis: Option<Basis>,
}

#[derive(Deserialize)]
struct AllArgs {
    path: String,
    hash: String,
    basis: Option<Basis>,
}

#[derive(Deserialize)]
struct UndoArgs {
    path: String,
    basis: Option<Basis>,
}

#[derive(Deserialize)]
struct BasisArg {
    basis: Basis,
}

/// A project path from outside, checked.
fn checked(path: &str) -> Result<String, IpcError> {
    Ok(VaultPath::parse(path)?.to_string())
}

impl Plugin for ReviewPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let fail = |reason: &str| PluginError::Start {
            id: ID,
            reason: reason.to_owned(),
        };
        let project = cx.project().ok_or_else(|| fail("needs a project"))?;
        let hub = project
            .services
            .get::<UnreadHub>()
            .ok_or_else(|| fail("needs the unread plugin"))?;
        let store = cx.store().cloned().ok_or_else(|| fail("no state folder"))?;
        let service = Arc::new(ReviewService::start(
            project.clone(),
            &hub,
            store,
            ID,
            Files {
                held: cx.state_file("held.json"),
                prefs: cx.state_file("prefs.json"),
            },
        ));
        let mut registrations: Vec<RpcRegistration> = Vec::new();
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "status",
            typed(move |args: PathArg, _| s.status(&checked(&args.path)?)),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "open",
            typed(move |args: OpenArgs, _| s.open(&checked(&args.path)?, args.basis)),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "keep",
            typed(move |args: ChangeArgs, _| {
                s.keep(&checked(&args.path)?, &args.hash, args.index, args.basis)
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "keepAll",
            typed(move |args: AllArgs, _| {
                s.keep_all(&checked(&args.path)?, &args.hash, args.basis)
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "revert",
            typed(move |args: ChangeArgs, _| {
                s.revert(&checked(&args.path)?, &args.hash, args.index, args.basis)
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "revertAll",
            typed(move |args: AllArgs, _| {
                s.revert_all(&checked(&args.path)?, &args.hash, args.basis)
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "undo",
            typed(move |args: UndoArgs, _| s.undo(&checked(&args.path)?, args.basis)),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "release",
            typed(move |args: PathArg, _| {
                s.release(&checked(&args.path)?);
                Ok(())
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "restore",
            typed(move |args: PathArg, _| s.restore(&checked(&args.path)?)),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "setBasis",
            typed(move |args: BasisArg, _| {
                s.set_basis(args.basis);
                Ok(())
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc("basis", typed(move |_: NoArgs, _| Ok(s.basis()))));
        Ok(Box::new(ReviewRunning {
            registrations,
            service: Some(service),
        }))
    }
}

/// A started `review` plugin. Drop = teardown: nobody is answered, the observer leaves the unread hub, the copier
/// stops.
struct ReviewRunning {
    registrations: Vec<RpcRegistration>,
    service: Option<Arc<ReviewService>>,
}

impl Running for ReviewRunning {}

impl Drop for ReviewRunning {
    fn drop(&mut self) {
        self.registrations.clear();
        self.service.take();
    }
}
