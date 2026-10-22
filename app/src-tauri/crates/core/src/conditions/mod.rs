//! Conditions: typed facts about a window's situation ([`Context`]), what a feature needs ([`Requirement`]) and the
//! one pure function deciding availability ([`available`]). TS mirror: `app/src/core/conditions/available.ts`.

use std::borrow::Cow;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::plugins::PluginId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum WorkspaceKind {
    Welcome,
    Project,
    SingleFile,
    Notes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RepoState {
    Clean,
    Merging,
    Rebasing,
    CherryPicking,
    Reverting,
    Bisecting,
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum GitCondition {
    /// Chosen git backend missing.
    Unavailable,
    NoRepo,
    Repo {
        state: RepoState,
    },
    Error {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PermarkCondition {
    NotInitialized,
    Initialized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum AgentCondition {
    Off,
    On,
    Connected { client: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum LiveUpdates {
    Available,
    Unavailable { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Distribution {
    Direct,
    Store,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Platform {
    Windows,
    Mac,
    Linux,
}

impl Platform {
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Linux
        }
    }
}

/// Facts of one window: workspace part + app part. Fields not meaningful for a kind hold neutral values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Context {
    pub workspace: WorkspaceKind,
    pub git: GitCondition,
    pub permark: PermarkCondition,
    pub agent: AgentCondition,
    pub live_updates: LiveUpdates,
    pub distribution: Distribution,
    pub platform: Platform,
    /// Plugins on for this workspace (app plugins + this project's).
    pub plugins: BTreeSet<PluginId>,
    /// Agent files at the project root (`workspace::AGENT_FILES` order; a `/` ends a folder); `[]` elsewhere.
    pub agent_files: Vec<String>,
}

impl Context {
    /// Neutral context of a window on the welcome screen.
    #[must_use]
    pub fn welcome(distribution: Distribution) -> Self {
        Self {
            workspace: WorkspaceKind::Welcome,
            git: GitCondition::NoRepo,
            permark: PermarkCondition::NotInitialized,
            agent: AgentCondition::Off,
            live_updates: LiveUpdates::Unavailable {
                reason: "noWorkspace".to_owned(),
            },
            distribution,
            platform: Platform::current(),
            plugins: BTreeSet::new(),
            agent_files: Vec::new(),
        }
    }
}

/// One condition a feature depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "need", rename_all = "camelCase")]
#[ts(export)]
pub enum Need {
    Workspace {
        kinds: Cow<'static, [WorkspaceKind]>,
    },
    /// `git = Repo`.
    GitRepo,
    /// `git != Unavailable`.
    GitAvailable,
    /// `permark = Initialized`.
    Permark,
    /// `agent = On | Connected`.
    AgentOn,
    AgentConnected,
    LiveUpdates,
    Distribution {
        distribution: Distribution,
    },
    Platform {
        platforms: Cow<'static, [Platform]>,
    },
    Plugin {
        plugin: PluginId,
    },
}

/// `needs` unmet -> unavailable (first one, declaration order); `prefers` unmet -> available, degraded (fallback).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Requirement {
    pub needs: Cow<'static, [Need]>,
    pub prefers: Cow<'static, [Need]>,
}

impl Requirement {
    pub const NONE: Self = Self {
        needs: Cow::Borrowed(&[]),
        prefers: Cow::Borrowed(&[]),
    };

    /// A project window (not a single file, not Notes, not Welcome): what project-only features need.
    pub const PROJECT: Self = Self {
        needs: Cow::Borrowed(&[Need::Workspace {
            kinds: Cow::Borrowed(&[WorkspaceKind::Project]),
        }]),
        prefers: Cow::Borrowed(&[]),
    };
}

/// Why something is unavailable | degraded, with the command that fixes it when there is one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Reason {
    #[serde(flatten)]
    pub need: Need,
    pub remedy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum Availability {
    Available { degraded: Vec<Reason> },
    Unavailable { reason: Reason },
}

/// Whether `need` holds in `context`.
#[must_use]
pub fn met(need: &Need, context: &Context) -> bool {
    match need {
        Need::Workspace { kinds } => kinds.contains(&context.workspace),
        Need::GitRepo => matches!(context.git, GitCondition::Repo { .. }),
        Need::GitAvailable => context.git != GitCondition::Unavailable,
        Need::Permark => context.permark == PermarkCondition::Initialized,
        Need::AgentOn => matches!(
            context.agent,
            AgentCondition::On | AgentCondition::Connected { .. }
        ),
        Need::AgentConnected => matches!(context.agent, AgentCondition::Connected { .. }),
        Need::LiveUpdates => context.live_updates == LiveUpdates::Available,
        Need::Distribution { distribution } => context.distribution == *distribution,
        Need::Platform { platforms } => platforms.contains(&context.platform),
        Need::Plugin { plugin } => context.plugins.contains(plugin),
    }
}

/// Default remedy command of a need.
#[must_use]
pub fn remedy(need: &Need) -> Option<String> {
    match need {
        Need::Permark => Some("project.init".to_owned()),
        Need::AgentOn => Some("agent.turnOn".to_owned()),
        Need::Plugin { plugin } => Some(format!("plugins.enable:{plugin}")),
        Need::GitAvailable => Some("settings.git".to_owned()),
        Need::Workspace { .. }
        | Need::GitRepo
        | Need::AgentConnected
        | Need::LiveUpdates
        | Need::Distribution { .. }
        | Need::Platform { .. } => None,
    }
}

fn reason(need: &Need) -> Reason {
    Reason {
        need: need.clone(),
        remedy: remedy(need),
    }
}

/// Availability of a requirement in a context. Pure.
#[must_use]
pub fn available(requirement: &Requirement, context: &Context) -> Availability {
    if let Some(unmet) = requirement.needs.iter().find(|need| !met(need, context)) {
        return Availability::Unavailable {
            reason: reason(unmet),
        };
    }
    Availability::Available {
        degraded: requirement
            .prefers
            .iter()
            .filter(|need| !met(need, context))
            .map(reason)
            .collect(),
    }
}

#[cfg(test)]
mod tests;
