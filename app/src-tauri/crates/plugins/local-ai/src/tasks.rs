//! The jobs PerMark gives a local model, each with the capabilities a model needs to take it.
//!
//! Source of truth; TS mirror `app/src/plugins/local-ai/tasks.ts`; both checked against the shared fixture
//! `app/src/plugins/local-ai/tasks.json`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalog::Capability;

/// One job of PerMark that a local model can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AiTask")]
pub enum Task {
    Correction,
    Translation,
    SemanticIndex,
    ReadAloud,
    Dictation,
    Assistant,
}

use Capability::{Chat, Embed, Speak, Tools, Transcribe, Translate};

impl Task {
    /// Every task, in the order the settings page lists them.
    pub const ALL: [Self; 6] = [
        Self::Correction,
        Self::Translation,
        Self::SemanticIndex,
        Self::ReadAloud,
        Self::Dictation,
        Self::Assistant,
    ];

    /// What a model needs for this task: alternatives (any one), each a set of capabilities (all of them).
    /// Translation: a dedicated translator | any chat model.
    #[must_use]
    pub const fn needs(self) -> &'static [&'static [Capability]] {
        match self {
            Self::Correction => &[&[Chat]],
            Self::Translation => &[&[Translate], &[Chat]],
            Self::SemanticIndex => &[&[Embed]],
            Self::ReadAloud => &[&[Speak]],
            Self::Dictation => &[&[Transcribe]],
            Self::Assistant => &[&[Chat, Tools]],
        }
    }

    /// Whether a model with `capabilities` can take this task.
    #[must_use]
    pub fn accepts(self, capabilities: &[Capability]) -> bool {
        self.needs()
            .iter()
            .any(|set| set.iter().all(|need| capabilities.contains(need)))
    }
}

/// One task as the windows see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AiTaskInfo")]
pub struct TaskInfo {
    pub task: Task,
    /// Alternatives (any one), each a set of capabilities (all of them).
    pub needs: Vec<Vec<Capability>>,
}

/// Every task with its needs (RPC `local-ai.tasks`).
#[must_use]
pub fn infos() -> Vec<TaskInfo> {
    Task::ALL
        .iter()
        .map(|&task| TaskInfo {
            task,
            needs: task.needs().iter().map(|set| set.to_vec()).collect(),
        })
        .collect()
}
