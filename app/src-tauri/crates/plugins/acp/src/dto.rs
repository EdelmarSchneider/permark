//! What the window and the plugin exchange: the agent's state and news, prompts, answers, the copy's changes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One file whose copy differs from the real file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileChange {
    /// Project path (`/` separators).
    pub path: String,
    pub kind: FileChangeKind,
    /// Line runs that differ (`modified`); the whole text as one run (`added` | `deleted` text files); none for
    /// `binary` | `conflict` (whole-file choices only).
    pub hunks: Vec<HunkView>,
}

/// How a file differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FileChangeKind {
    /// Text changed in the copy only: each hunk can be accepted | rejected alone.
    Modified,
    /// New in the copy.
    Added,
    /// Gone from the copy.
    Deleted,
    /// Changed in the copy, not text the app can splice (binary, undecodable, too large).
    Binary,
    /// Changed in the copy and in the real folder since the copy was taken.
    Conflict,
}

/// One run of lines that differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HunkView {
    /// Index in this list (what accept | reject name).
    pub id: u32,
    /// 1-based line of the real file where the run starts (no old lines: the line the new ones go before).
    pub old_start: u32,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
}

/// Arguments of `acp.accept` | `acp.reject`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecideArgs {
    /// Project path.
    pub path: String,
    /// Hunk ids of the last list; `null`: the whole file.
    #[serde(default)]
    pub hunks: Option<Vec<u32>>,
}

/// Where the agent of the project stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AcpState {
    /// No agent running (none started yet | stopped by "new conversation").
    Idle,
    /// The agent is being started (first prompt: copy made, program started, session opened).
    Starting,
    /// The agent waits for a prompt.
    Ready,
    /// The agent works on a prompt.
    Working,
    /// The agent could not start | stopped by itself; the next prompt starts a fresh one.
    Error,
}

/// Answer of `acp.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AcpStatus {
    pub state: AcpState,
    /// The agent command as the person typed it (or the default).
    pub agent: String,
    /// Why the agent is in `error` (plain English).
    pub error: Option<String>,
}

/// The passage the person attached to a prompt.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct PromptSelection {
    /// Project path of the document.
    pub path: String,
    /// First line, 1-based.
    pub from_line: u32,
    /// Last line, 1-based (inclusive).
    pub to_line: u32,
    pub text: String,
}

/// Arguments of `acp.prompt`.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct PromptArgs {
    pub text: String,
    #[serde(default)]
    #[ts(optional)]
    pub selection: Option<PromptSelection>,
    /// Project path of the open document.
    #[serde(default)]
    #[ts(optional)]
    pub document: Option<String>,
}

/// Arguments of `acp.answer`.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct AnswerArgs {
    /// `requestId` of the `permission` update.
    pub request_id: String,
    /// One of the offered option ids; `null`: the question is dismissed (the agent hears "cancelled").
    pub option_id: Option<String>,
}

/// One answer the agent offers for a permission question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PermissionChoice {
    pub id: String,
    /// Label from the agent.
    pub name: String,
    /// `allow_once` | `allow_always` | `reject_once` | `reject_always`.
    pub kind: String,
}

/// Event `acp:update`: what the agent of the project does, as it happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum AcpUpdate {
    /// The state changed; `error` only with `error` (or a prompt that could not be sent: state stays `ready`).
    Status {
        state: AcpState,
        error: Option<String>,
    },
    /// A piece of the agent's reply (append).
    Text { text: String },
    /// The agent thinks (its thoughts are not shown); once per stretch of thinking.
    Thinking {},
    /// A tool the agent runs (upsert by `id`); `kind`: `read` | `edit` | `delete` | `move` | `search` | `execute`
    /// | `think` | `fetch` | `switch_mode` | `other`; `status`: `pending` | `in_progress` | `completed` | `failed`.
    Tool {
        id: String,
        title: String,
        kind: String,
        status: String,
    },
    /// The agent asks before doing something outside the copy's files: the person answers with `acp.answer`.
    Permission {
        request_id: String,
        title: String,
        kind: String,
        /// The command to run, else the paths it touches (project paths inside the copy).
        detail: String,
        options: Vec<PermissionChoice>,
    },
    /// The question was answered | dropped (cancel, new conversation): its card goes.
    PermissionDone { request_id: String },
    /// The prompt ended; `stopReason`: `end_turn` | `max_tokens` | `max_turn_requests` | `refusal` | `cancelled` |
    /// `error`; `changed`: files whose copy now differs from the real file.
    TurnEnd { stop_reason: String, changed: u32 },
}
