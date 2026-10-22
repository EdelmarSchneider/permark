//! Plugin `unread`: which Markdown documents changed since the person last read them, like new mail. What a document
//! held when it was last read is a hash per document in `read.json`; the rules are in [`Tracker`], the worker that
//! keeps them true in [`UnreadService`]. Also what changed since the project was last closed, and the public API
//! (`UnreadHub`) the `review` plugin follows.

#![cfg_attr(test, allow(clippy::assert_is_empty))]

#[cfg(test)]
mod owners_tests;
mod service;
#[cfg(test)]
mod tests;
mod tracker;
mod types;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::error::{ErrorKind, IpcError};
use permark_core::organizing::UnreadApi;
use permark_core::paths::{VaultPath, WORKTREE_PREFIX};
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, RpcRegistration, Running, ServiceLease,
};
use permark_core::projects::OwnersChange;
use permark_core::watch::Subscription;
use serde::Deserialize;

pub use service::{
    hash_from_hex, Announce, ObserverLease, ReadFile, ReadObserver, ServiceArgs, Timing, UnreadHub,
    UnreadService, VisitFile, MAX_BYTES,
};
pub use tracker::{is_tracked, key_of, Doc, Tracker};
pub use types::{SinceVisit, UnreadList, VisitItem, VisitKind};

const ID: PluginId = PluginId::of("unread");

struct UnreadPlugin {
    manifest: Manifest,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(UnreadPlugin {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::Project,
            requires: Requirement::PROJECT,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
    })
}

#[derive(Deserialize)]
struct MarkRead {
    path: String,
    /// The version the person saw (16 hex digits).
    hash: String,
}

#[derive(Deserialize)]
struct PathArg {
    path: String,
}

#[derive(Deserialize)]
struct FolderArg {
    folder: String,
}

/// A project path from outside, checked.
fn checked(path: &str) -> Result<String, IpcError> {
    Ok(VaultPath::parse(path)?.to_string())
}

impl Plugin for UnreadPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    #[allow(clippy::too_many_lines)] // one registration per method the window calls
    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let (Some(project), CxWorkspace::Project(vault), Some(hub)) =
            (cx.project(), &cx.workspace, cx.watch())
        else {
            return Err(PluginError::Start {
                id: ID,
                reason: "needs a project".to_owned(),
            });
        };
        let events = cx.events().clone();
        let mut roots = vec![String::new()];
        roots.extend(
            vault
                .worktrees()
                .iter()
                .map(|w| format!("{WORKTREE_PREFIX}{}", w.name)),
        );
        let Some(store) = cx.store() else {
            return Err(PluginError::Start {
                id: ID,
                reason: "needs a project".to_owned(),
            });
        };
        let started = project.owners(store);
        let service = Arc::new(UnreadService::start(ServiceArgs {
            scope: project.documents.scope().clone(),
            explorer: Arc::clone(&project.explorer),
            roots,
            owners: started.clone(),
            // a reader with no window leaves the picture of the last visit alone
            visit: !cx.files_only(),
            announce: Arc::new(move |list| {
                events.emit(
                    "changed",
                    serde_json::to_value(list).unwrap_or(serde_json::Value::Null),
                );
            }),
            timing: Timing::default(),
            persist: !cx.files_only(),
            owners_known: {
                let projects = project.projects().map(Arc::downgrade);
                Arc::new(move || {
                    if let Some(projects) = projects.as_ref().and_then(std::sync::Weak::upgrade) {
                        let _ = projects.wait_owners(std::time::Duration::from_secs(5));
                    }
                })
            },
        }));
        let subscription = hub.subscribe({
            let service = Arc::clone(&service);
            move |batch| service.feed(batch)
        });
        // the read state follows where each included project keeps it (`projects.md` §9.2)
        let owners = project.follow_owners({
            let service = Arc::downgrade(&service);
            move |change| {
                if let Some(service) = service.upgrade() {
                    service.follow_owners(&change);
                }
            }
        });
        // an owners change between the read above and the subscription: caught up here
        let now = project.owners(store);
        if !now.same(&started) {
            service.follow_owners(&OwnersChange::Resume(&now));
        }
        let mut registrations = Vec::new();
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc("list", typed(move |_: NoArgs, _| Ok(s.list()))));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "sinceLastVisit",
            typed(move |_: NoArgs, _| Ok(s.since_last_visit())),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "markRead",
            typed(move |args: MarkRead, _| {
                let hash = hash_from_hex(&args.hash)
                    .ok_or_else(|| IpcError::new(ErrorKind::InvalidData).subject("hash"))?;
                s.mark_read(&checked(&args.path)?, hash);
                Ok(())
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "markUnread",
            typed(move |args: PathArg, _| {
                s.mark_unread(&checked(&args.path)?);
                Ok(())
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "markFolderRead",
            typed(move |args: FolderArg, _| {
                let folder = if args.folder.is_empty() {
                    String::new()
                } else {
                    checked(&args.folder)?
                };
                s.mark_folder_read(&folder);
                Ok(())
            }),
        ));
        let lease = project.services.provide(Arc::clone(service.hub()));
        let api = project
            .services
            .provide(Arc::new(UnreadApi(service.lookup())));
        Ok(Box::new(UnreadRunning {
            subscription: Some(subscription),
            owners,
            registrations,
            lease: Some(lease),
            api: Some(api),
            service: Some(service),
        }))
    }
}

/// A started `unread` plugin. Drop = teardown: the watch is not heard, nobody is answered, the hub is withdrawn, and
/// the worker joins (saving `read.json` and the picture of this visit).
struct UnreadRunning {
    subscription: Option<Subscription>,
    owners: Option<permark_core::projects::ProjectsSubscription>,
    registrations: Vec<RpcRegistration>,
    lease: Option<ServiceLease>,
    api: Option<ServiceLease>,
    service: Option<Arc<UnreadService>>,
}

impl Running for UnreadRunning {}

impl Drop for UnreadRunning {
    fn drop(&mut self) {
        self.subscription.take();
        self.owners.take();
        self.registrations.clear();
        self.api.take();
        self.lease.take();
        self.service.take();
    }
}
