//! Plugin `comments`: comments on passages of a document, anchored so they follow the text when it changes. They live in
//! the project's local state (`comments.json`, the layout of 0.5), never in the project's files.
//!
//! The quote, the words around it and the headings above it find the passage again after an edit (`anchor`); a
//! worker looks again whenever a document with comments changes. The window draws the bars, the list and the field.

#![cfg_attr(test, allow(clippy::assert_is_empty))]

pub mod anchor;
pub mod model;
mod service;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running,
};
use permark_core::watch::Subscription;
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
        let service = Arc::new(CommentsService::start(
            store.raw_file(FILE),
            project.documents.scope().clone(),
            Arc::new(move || events.emit("changed", serde_json::Value::Null)),
        ));
        let subscription = hub.subscribe({
            let service = Arc::clone(&service);
            move |batch| service.feed(batch)
        });
        let mut registrations = Vec::new();
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "list",
            typed(move |args: ListArgs, _| s.list(args.path.as_deref(), args.scope)),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "add",
            typed(move |args: AddArgs, _| {
                s.add(&NewComment {
                    path: args.path,
                    quote: args.quote,
                    text: args.text,
                    heading_path: args.heading_path,
                    line: args.line,
                    end_line: args.end_line,
                })
            }),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "setDone",
            typed(move |args: DoneArgs, _| s.set_done(&args.id, args.done)),
        ));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc("remove", typed(move |args: IdArgs, _| s.remove(&args.id))));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "reply",
            typed(move |args: ReplyArgs, _| s.reply(&args.id, &args.text)),
        ));
        Ok(Box::new(CommentsRunning {
            subscription: Some(subscription),
            registrations,
            service: Some(service),
        }))
    }
}

/// A started `comments` plugin. Drop = teardown: the watch is not heard, nobody is answered, the worker joins.
struct CommentsRunning {
    subscription: Option<Subscription>,
    registrations: Vec<RpcRegistration>,
    service: Option<Arc<CommentsService>>,
}

impl Running for CommentsRunning {}

impl Drop for CommentsRunning {
    fn drop(&mut self) {
        self.subscription.take();
        self.registrations.clear();
        self.service.take();
    }
}
