//! Plugin `comments`: comments on passages of a document, anchored so they follow the text when it changes. They live in
//! the project's local state (`comments.json`, the layout of 0.5), never in the project's files.
//!
//! The quote, the words around it and the headings above it find the passage again after an edit (`anchor`); a
//! worker looks again whenever a document with comments changes. The window draws the bars, the list and the field.

#![cfg_attr(test, allow(clippy::assert_is_empty))]

pub mod anchor;
pub mod model;
#[cfg(test)]
mod owners_tests;
mod service;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::activity::{ActivityLog, CommentOp, Fact};
use permark_core::conditions::Requirement;
use permark_core::organizing::{CommentBook, CommentFilter, CommentItem, CommentsApi};
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running, ServiceLease,
};
use permark_core::projects::OwnersChange;
use permark_core::watch::Subscription;
use permark_core::IpcError;
use serde::Deserialize;

pub use model::{Comment, CommentScope, CommentView, PassageState};
pub use service::{CommentsService, NewComment};

const ID: PluginId = PluginId::of("comments");
/// The file in the project's local state.
pub const FILE: &str = "comments.json";

struct CommentsPlugin {
    manifest: Manifest,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(CommentsPlugin {
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
struct ListArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    scope: CommentScope,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddArgs {
    path: String,
    quote: String,
    text: String,
    #[serde(default)]
    heading_path: Vec<String>,
    #[serde(default)]
    line: u32,
    #[serde(default)]
    end_line: u32,
}

#[derive(Deserialize)]
struct DoneArgs {
    id: String,
    done: bool,
}

#[derive(Deserialize)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
struct ReplyArgs {
    id: String,
    text: String,
}

impl Plugin for CommentsPlugin {
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
        let service = Arc::new(CommentsService::start_owners(
            started.clone(),
            project.documents.scope().clone(),
            Arc::new(move || events.emit("changed", serde_json::Value::Null)),
        ));
        // comments follow where each included project keeps its state (`projects.md` §9.2)
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
        let subscription = hub.subscribe({
            let service = Arc::clone(&service);
            move |batch| service.feed(batch)
        });
        let mut registrations = Vec::new();
        let journal = Arc::clone(&project.activity);
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "list",
            typed(move |args: ListArgs, _| s.list(args.path.as_deref(), args.scope)),
        ));
        let (s, j) = (Arc::clone(&service), Arc::clone(&journal));
        registrations.extend(cx.rpc(
            "add",
            typed(move |args: AddArgs, _| {
                let added = s.add(&NewComment {
                    path: args.path,
                    quote: args.quote,
                    text: args.text,
                    heading_path: args.heading_path,
                    line: args.line,
                    end_line: args.end_line,
                })?;
                note(
                    &j,
                    CommentOp::Add,
                    &added.id,
                    &added.path,
                    &added.heading_path,
                );
                Ok(added)
            }),
        ));
        let (s, j) = (Arc::clone(&service), Arc::clone(&journal));
        registrations.extend(cx.rpc(
            "setDone",
            typed(move |args: DoneArgs, _| {
                let about = about(&s, &args.id);
                s.set_done(&args.id, args.done)?;
                if let Some((path, heading)) = about {
                    let op = if args.done {
                        CommentOp::Done
                    } else {
                        CommentOp::Reopen
                    };
                    note(&j, op, &args.id, &path, &heading);
                }
                Ok(())
            }),
        ));
        let (s, j) = (Arc::clone(&service), Arc::clone(&journal));
        registrations.extend(cx.rpc(
            "remove",
            typed(move |args: IdArgs, _| {
                let about = about(&s, &args.id);
                s.remove(&args.id)?;
                if let Some((path, heading)) = about {
                    note(&j, CommentOp::Remove, &args.id, &path, &heading);
                }
                Ok(())
            }),
        ));
        let (s, j) = (Arc::clone(&service), Arc::clone(&journal));
        registrations.extend(cx.rpc(
            "reply",
            typed(move |args: ReplyArgs, _| {
                let about = about(&s, &args.id);
                s.reply(&args.id, &args.text)?;
                if let Some((path, heading)) = about {
                    note(&j, CommentOp::Reply, &args.id, &path, &heading);
                }
                Ok(())
            }),
        ));
        let lease = project
            .services
            .provide(Arc::new(CommentsApi(Arc::new(Book(Arc::clone(&service))))));
        Ok(Box::new(CommentsRunning {
            subscription: Some(subscription),
            owners,
            registrations,
            lease: Some(lease),
            service: Some(service),
        }))
    }
}

/// A started `comments` plugin. Drop = teardown: the watch is not heard, nobody is answered, the worker joins.
struct CommentsRunning {
    subscription: Option<Subscription>,
    owners: Option<permark_core::projects::ProjectsSubscription>,
    registrations: Vec<RpcRegistration>,
    lease: Option<ServiceLease>,
    service: Option<Arc<CommentsService>>,
}

impl Running for CommentsRunning {}

impl Drop for CommentsRunning {
    fn drop(&mut self) {
        self.subscription.take();
        self.owners.take();
        self.registrations.clear();
        self.lease.take();
        self.service.take();
    }
}

/// What the agent connection asks of the comments.
struct Book(Arc<CommentsService>);

impl CommentBook for Book {
    fn list(&self, which: CommentFilter) -> Result<Vec<CommentItem>, IpcError> {
        let scope = match which {
            CommentFilter::Open => CommentScope::Open,
            CommentFilter::Done => CommentScope::Done,
            CommentFilter::All => CommentScope::All,
        };
        Ok(self
            .0
            .list(None, scope)?
            .into_iter()
            .map(|view| CommentItem {
                state: match view.state {
                    PassageState::Anchored => "anchored",
                    PassageState::Moved => "moved",
                    PassageState::PassageChanged => "passageChanged",
                    PassageState::DocumentMissing => "documentMissing",
                },
                id: view.id,
                path: view.path,
                quote: view.quote,
                text: view.text,
                heading_path: view.heading_path,
                line: view.line,
                created_ms: view.created_ms,
                done: view.done,
                reply: view.reply,
            })
            .collect())
    }

    fn resolve(&self, id: &str, reply: Option<&str>) -> Result<(), IpcError> {
        self.0.resolve_by_agent(id, reply)
    }
}

/// The document and heading of a comment (before an operation changes it).
fn about(service: &CommentsService, id: &str) -> Option<(String, Vec<String>)> {
    service
        .list(None, CommentScope::All)
        .ok()?
        .into_iter()
        .find(|c| c.id == id)
        .map(|c| (c.path, c.heading_path))
}

/// The person's comment action, in the project's activity journal.
fn note(journal: &ActivityLog, op: CommentOp, id: &str, path: &str, heading: &[String]) {
    journal.person(Fact::Comment {
        op,
        path: path.to_owned(),
        id: id.to_owned(),
        heading: (!heading.is_empty()).then(|| heading.join(" > ")),
    });
}
