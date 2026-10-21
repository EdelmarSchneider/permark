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

#[cfg(test)]
mod aids_tests;
#[cfg(test)]
mod budget_tests;
pub mod cli;
pub mod context;
pub mod format;
pub mod guard;
pub mod handoff;
pub mod live;
pub mod map;
pub mod mcp;
pub mod perms;
#[cfg(test)]
mod projects_tests;
pub mod render;
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
    /// A project inside the folder takes the group away below it (its `[agent] deny`).
    #[error("not allowed below {member}: {}", group.id())]
    DeniedBelow { group: Group, member: String },
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
            Self::Denied { .. } | Self::DeniedBelow { .. } => "denied",
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

    #[allow(clippy::too_many_lines)] // one registration per method the window calls
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
                    // compact, no empty fields, versioned like the agent's text answers
                    let guard = h.tools.guard_now();
                    if let Ok((shared, open)) = h.tools.context(&guard) {
                        let mut value = serde_json::to_value(&shared).unwrap_or_default();
                        if let Some(map) = value.as_object_mut() {
                            map.insert("v".to_owned(), serde_json::json!(1));
                            if open > 0 {
                                map.insert("openComments".to_owned(), serde_json::json!(open));
                            }
                        }
                        if let Ok(bytes) = serde_json::to_vec(&value) {
                            let _ = local.raw_file(CONTEXT_FILE).write(&bytes);
                        }
                    }
                }
                Ok(())
            }),
        ));
        // the hand-off: the dialog's content, then the confirm that freezes the package and sets the checkpoint
        let h = Arc::clone(&hub);
        registrations.extend(cx.rpc(
            "handOffPreview",
            typed(move |args: handoff::PreviewArgs, _| {
                let guard = h.tools.guard_now();
                Ok(handoff::preview(&h.tools, &guard, args.checked.as_deref()))
            }),
        ));
        let h = Arc::clone(&hub);
        registrations.extend(cx.rpc(
            "handOffConfirm",
            typed(move |args: handoff::ConfirmArgs, _| {
                let guard = h.tools.guard_now();
                handoff::confirm(&h.tools, &guard, args.upto, &args.checked)
                    .map_err(|e| IpcError::new(ErrorKind::Unavailable).detail(e.to_string()))
            }),
        ));
        // a store copy (Microsoft Store alias, snap command) is started as `permark`: its own path changes per version
        let by_name = cx.context.distribution == permark_core::conditions::Distribution::Store;
        registrations.extend(cx.rpc(
            "setup",
            typed(move |_: NoArgs, _| Ok(setup_program(by_name, cli::program_path()))),
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

/// The program the setup lines name: `permark` for a store copy, else the running program's path.
fn setup_program(by_name: bool, running: Option<std::path::PathBuf>) -> Option<String> {
    if by_name {
        Some("permark".to_owned())
    } else {
        running.map(|p| p.display().to_string())
    }
}

#[cfg(test)]
mod setup_tests {
    use super::setup_program;

    #[test]
    fn a_store_copy_is_named_by_its_alias() {
        let path = std::path::PathBuf::from("/opt/Per Mark/permark");
        assert_eq!(
            setup_program(true, Some(path.clone())).as_deref(),
            Some("permark")
        );
        assert_eq!(setup_program(true, None).as_deref(), Some("permark"));
        assert_eq!(
            setup_program(false, Some(path.clone())),
            Some(path.display().to_string())
        );
        assert_eq!(setup_program(false, None), None);
    }
}
