//! Plugin `tags`: tags and a one-line description for any file, without touching it. Tags in a document's front matter
//! count too (read-only). The app's tags live in `.permark/meta.toml` (shared) when the project has `.permark/`, else in
//! the person's profile; the layout is the one of 0.5 and writes are surgical (`meta`).

#![cfg_attr(test, allow(clippy::assert_is_empty))]

pub mod catalog;
pub mod frontmatter;
pub mod meta;
pub mod normalize;
#[cfg(test)]
mod owners_tests;
mod service;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;
mod types;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::activity::Fact;
use permark_core::conditions::Requirement;
use permark_core::organizing::TagsApi;
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, RpcRegistration, Running, ServiceLease,
};
use permark_core::projects::OwnersChange;
use permark_core::watch::Subscription;
use serde::Deserialize;

pub use service::{clean_tags, ServiceArgs, TagsService, Timing};
pub use types::{
    FileMeta, IndexEntry, MetaProblem, TagCountView, TagRefView, TagStyleView, TaggedFile,
    TagsIndex, TagsLocation, TagsOverview, TagsStatus,
};

const ID: PluginId = PluginId::of("tags");

/// Project path of the shared file, as the watch reports it.
pub const META_PATH: &str = ".permark/meta.toml";
/// The file's name in its folder.
pub const META_FILE: &str = "meta.toml";

struct TagsPlugin {
    manifest: Manifest,
    timing: Timing,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    plugin_with(Timing::default())
}

/// The plugin with its own writing times (tests).
#[must_use]
pub fn plugin_with(timing: Timing) -> Arc<dyn Plugin> {
    Arc::new(TagsPlugin {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::Project,
            requires: Requirement::PROJECT,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
        timing,
    })
}

#[derive(Deserialize)]
struct PathArg {
    path: String,
}

#[derive(Deserialize)]
struct SetTagsArgs {
    path: String,
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct SetDescriptionArgs {
    path: String,
    description: String,
}

#[derive(Deserialize)]
struct SetStyleArgs {
    tag: String,
    color: Option<String>,
    icon: Option<String>,
}

#[derive(Deserialize)]
struct FilesWithArgs {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    untagged: bool,
}

impl Plugin for TagsPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    #[allow(clippy::too_many_lines)] // one registration per method the window calls
    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let (Some(project), CxWorkspace::Project(_), Some(hub), Some(store)) =
            (cx.project(), &cx.workspace, cx.watch(), cx.store())
        else {
            return Err(PluginError::Start {
                id: ID,
                reason: "needs a project".to_owned(),
            });
        };
        let events = cx.events().clone();
        let started = project.owners(store);
        // S3: before the worker knows the owners, nothing is written (it could land in the outer folder's file)
        let early = !project.owners_known();
        let service = Arc::new(TagsService::start(ServiceArgs {
            owners: started.clone(),
            // a reader with no window does not carry the profile copy over (the app does, on its next open)
            legacy: !cx.files_only(),
            own: Arc::clone(&project.own),
            scope: project.documents.scope().clone(),
            harvest: Arc::clone(project.search.harvest()),
            projects: project.projects().cloned(),
            announce: Arc::new(move || events.emit("changed", serde_json::Value::Null)),
            timing: self.timing,
        }));
        if early {
            service.start_paused();
        }
        let subscription = hub.subscribe({
            let service = Arc::clone(&service);
            move |batch| service.feed(batch)
        });
        // each included project's tags live in its own meta.toml (`projects.md` §9.2)
        let owners = project.follow_owners({
            let service = Arc::downgrade(&service);
            move |change| {
                if let Some(service) = service.upgrade() {
                    service.follow_owners(&change);
                }
            }
        });
        // an owners change between the read above and the subscription (or owners known meanwhile): caught up here
        let now = project.owners(store);
        if !now.same(&started) || (early && project.owners_known()) {
            service.follow_owners(&OwnersChange::Resume(&now));
        }
        let mut registrations = Vec::new();
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc("index", typed(move |_: NoArgs, _| Ok(s.index()))));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc("overview", typed(move |_: NoArgs, _| Ok(s.overview()))));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc("status", typed(move |_: NoArgs, _| Ok(s.status()))));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "filesWith",
            typed(move |args: FilesWithArgs, _| Ok(s.files_with(&args.tags, args.untagged))),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "fileMeta",
            typed(move |args: PathArg, _| s.file_meta(&args.path)),
        ));
        let (s, journal) = (Arc::clone(&service), Arc::clone(&project.activity));
        registrations.extend(cx.rpc(
            "setTags",
            typed(move |args: SetTagsArgs, _| {
                let app_tags = |meta: &FileMeta| -> Vec<String> {
                    meta.tags
                        .iter()
                        .filter(|t| t.app)
                        .map(|t| t.name.clone())
                        .collect()
                };
                let before = s.file_meta(&args.path).map(|m| app_tags(&m)).ok();
                let after = s.set_tags(&args.path, &args.tags)?;
                // what the person added and removed, in the project's activity journal
                if let Some(before) = before {
                    let now = app_tags(&after);
                    for tag in now.iter().filter(|t| !before.contains(t)) {
                        journal.person(Fact::Tag {
                            added: true,
                            path: after.path.clone(),
                            tag: tag.clone(),
                        });
                    }
                    for tag in before.iter().filter(|t| !now.contains(t)) {
                        journal.person(Fact::Tag {
                            added: false,
                            path: after.path.clone(),
                            tag: tag.clone(),
                        });
                    }
                }
                Ok(after)
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "setDescription",
            typed(move |args: SetDescriptionArgs, _| {
                s.set_description(&args.path, &args.description)
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "setStyle",
            typed(move |args: SetStyleArgs, _| {
                s.set_style(&args.tag, args.color.as_deref(), args.icon.as_deref())
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc("orphans", typed(move |_: NoArgs, _| Ok(s.orphans()))));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "cleanUp",
            typed(move |_: NoArgs, _| s.clean_up().map(|n| u32::try_from(n).unwrap_or(u32::MAX))),
        ));
        let lease = project
            .services
            .provide(Arc::new(TagsApi(service.lookup())));
        Ok(Box::new(TagsRunning {
            subscription: Some(subscription),
            owners,
            registrations,
            lease: Some(lease),
            service: Some(service),
        }))
    }
}

/// A started `tags` plugin. Drop = teardown: the watch is not heard, nobody is answered, the service is withdrawn, and
/// the threads join (what was changed is written first).
struct TagsRunning {
    subscription: Option<Subscription>,
    owners: Option<permark_core::projects::ProjectsSubscription>,
    registrations: Vec<RpcRegistration>,
    lease: Option<ServiceLease>,
    service: Option<Arc<TagsService>>,
}

impl Running for TagsRunning {}

impl Drop for TagsRunning {
    fn drop(&mut self) {
        self.subscription.take();
        self.owners.take();
        self.registrations.clear();
        self.lease.take();
        self.service.take();
    }
}
