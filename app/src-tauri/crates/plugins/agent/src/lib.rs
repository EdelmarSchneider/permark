//! Plugin `agent`: the agent connection. Any agent speaking MCP (`permark mcp`) learns where the person is, the
//! unread documents, search results, tags, structure and parts of files, open comments and shared notes, and can show
//! the person a place. Off by default, per project, local only, visible while on; the switch and the groups live in
//! the person's profile only (a project's files can narrow, never widen). The agent never edits files.
//!
//! ```text
//!  agent --stdio--> permark mcp --local socket--> app (server of this project) --> Tools over ProjectServices
//!                        |
//!                        +-- app closed: Tools over ProjectCore::from_files (same composition)
//! ```

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod cli;
pub mod context;
pub mod guard;
pub mod live;
pub mod mcp;
pub mod perms;
pub mod server;
pub mod structure;
#[cfg(test)]
mod tests;
pub mod tools;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::ipc_local::Endpoint;
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, RpcRegistration, Running,
};
use permark_core::state::StateLocation;
use permark_core::{ErrorKind, IpcError};
use serde::Deserialize;

use crate::context::WindowContext;
use crate::live::Live;
use crate::perms::{Group, ProjectChoice};
use crate::server::{Hub, Switch};
use crate::tools::Tools;

const ID: PluginId = PluginId::of("agent");
/// The last published context, for scripts without MCP (only in a project with `.permark/`).
pub const CONTEXT_FILE: &str = "agent-context.json";

/// Why a tool call gave no answer. The words are what the agent reads.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("the agent connection is off for this project: the person can turn it on in PerMark, Settings > Agent")]
    Off,
    #[error("not allowed in this project: {}", group.id())]
    Denied {
        group: Group,
        /// The first time resolving was asked for while off: the person is offered it.
        first_ask: bool,
    },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("PerMark is not open on this project")]
    NotOpen,
    #[error("{0}")]
    Failed(String),
}

impl ToolError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Denied { .. } => "denied",
            Self::NotFound(_) => "notFound",
            Self::Invalid(_) => "invalid",
            Self::NotOpen => "notOpen",
            Self::Failed(_) => "error",
        }
    }

    /// Back from the code and message a server sent.
    #[must_use]
    pub fn from_wire(code: &str, message: &str) -> Self {
        let after = |prefix: &str| message.strip_prefix(prefix).unwrap_or(message).to_owned();
        match code {
            "off" => Self::Off,
            "denied" => Group::parse(&after("not allowed in this project: ")).map_or_else(
                || Self::Failed(message.to_owned()),
                |group| Self::Denied {
                    group,
                    first_ask: false,
                },
            ),
            "notFound" => Self::NotFound(after("not found: ")),
            "invalid" => Self::Invalid(after("invalid: ")),
            "notOpen" => Self::NotOpen,
            _ => Self::Failed(message.to_owned()),
        }
    }
}

struct AgentPlugin {
    manifest: Manifest,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(AgentPlugin {
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
struct EnabledArgs {
    enabled: bool,
}

#[derive(Deserialize)]
struct GroupArgs {
    group: String,
    on: bool,
}

#[derive(Deserialize)]
struct PublishArgs {
    context: WindowContext,
}

fn invalid(subject: &str) -> IpcError {
    IpcError::new(ErrorKind::InvalidData).subject(subject)
}

fn saved(result: Result<(), permark_core::state::StateError>) -> Result<(), IpcError> {
    result.map_err(IpcError::from)
}

/// Turns the switch: the first time on, the project starts from the app's defaults.
fn set_enabled(hub: &Hub, enabled: bool) -> Result<(), IpcError> {
    let store = hub.tools.store();
    let mut choice = perms::load(store);
    if enabled && !choice.enabled && choice.groups.is_empty() {
        choice.groups = perms::clean(&perms::defaults(store));
    }
    choice.enabled = enabled;
    saved(
        store
            .profile_file::<ProjectChoice>(perms::PROJECT_FILE)
            .save(&choice),
    )
}

impl Plugin for AgentPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let (Some(project), CxWorkspace::Project(vault), Some(store)) =
            (cx.project(), &cx.workspace, cx.store())
        else {
            return Err(PluginError::Start {
                id: ID,
                reason: "needs a project".to_owned(),
            });
        };
        let live = Arc::new(Live::new(cx.events().clone()));
        let tools = Tools::new(
            vault.clone(),
            store.clone(),
            project.clone(),
            Some(Arc::clone(&live)),
        );
        let hub = Hub::new(tools, Arc::clone(&project.facts), cx.events().clone());
        let endpoint = Endpoint::project(store.app_dir(), vault.main());
        let switch = Arc::new(Switch::new(Arc::clone(&hub), endpoint));
        switch.apply();
        let mut registrations: Vec<RpcRegistration> = Vec::new();
        let h = Arc::clone(&hub);
        registrations.extend(cx.rpc("status", typed(move |_: NoArgs, _| Ok(h.status()))));
        let s = Arc::clone(&switch);
        registrations.extend(cx.rpc(
            "setEnabled",
            typed(move |args: EnabledArgs, _| {
                set_enabled(&s.hub, args.enabled)?;
                s.apply();
                Ok(s.hub.status())
            }),
        ));
        let h = Arc::clone(&hub);
        registrations.extend(cx.rpc(
            "setGroup",
            typed(move |args: GroupArgs, _| {
                let group = Group::parse(&args.group).ok_or_else(|| invalid("group"))?;
                let store = h.tools.store();
                let mut choice = perms::load(store);
                choice.groups.insert(group.id().to_owned(), args.on);
                saved(
                    store
                        .profile_file::<ProjectChoice>(perms::PROJECT_FILE)
                        .save(&choice),
                )?;
                h.changed();
                Ok(h.status())
            }),
        ));
        let h = Arc::clone(&hub);
        registrations.extend(cx.rpc(
            "setDefaults",
            typed(move |_: NoArgs, _| {
                // the groups of this project become the defaults of new projects
                let store = h.tools.store();
                let groups = h.status().groups;
                saved(
                    store
                        .app_file::<perms::Defaults>(perms::DEFAULTS_FILE)
                        .save(&perms::Defaults { groups }),
                )
            }),
        ));
        let h = Arc::clone(&hub);
        let l = Arc::clone(&live);
        let local = store.clone();
        registrations.extend(cx.rpc(
            "publish",
            typed(move |args: PublishArgs, _| {
                l.publish(args.context);
                if perms::load(h.tools.store()).enabled
                    && local.location() == StateLocation::Project
                {
                    let shared = l.context().sanitize(&h.tools.guard_now());
                    if let Ok(bytes) = serde_json::to_vec_pretty(&shared) {
                        let _ = local.raw_file(CONTEXT_FILE).write(&bytes);
                    }
                }
                Ok(())
            }),
        ));
        registrations.extend(cx.rpc(
            "setup",
            typed(move |_: NoArgs, _| Ok(cli::program_path().map(|p| p.display().to_string()))),
        ));
        Ok(Box::new(AgentRunning {
            registrations,
            switch: Some(switch),
        }))
    }
}

/// A started `agent` plugin. Drop = teardown: nobody is answered, the server stops (sessions end, threads joined) and
/// the project's agent condition goes back to `Off`.
struct AgentRunning {
    registrations: Vec<RpcRegistration>,
    switch: Option<Arc<Switch>>,
}

impl Running for AgentRunning {}

impl Drop for AgentRunning {
    fn drop(&mut self) {
        self.registrations.clear();
        if let Some(switch) = self.switch.take() {
            switch.stop();
        }
    }
}
