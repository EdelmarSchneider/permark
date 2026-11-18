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
    /// The agent runs but wants the person to sign in first (`methods`): `acp.login`, then the prompt goes on.
    NeedsLogin,
}

/// Answer of `acp.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AcpStatus {
    pub state: AcpState,
    /// The chosen agent's command as it would run (`""`: none chosen).
    pub agent: String,
    /// The chosen agent's name (an own command: its program); `null`: none chosen.
    pub agent_name: Option<String>,
    /// An agent is chosen (`false`: a prompt is refused with `noAgent` until `acp.choose`).
    pub chosen: bool,
    /// Why the agent is in `error` | why a sign-in failed (plain English).
    pub error: Option<String>,
    /// How the person can sign in (`needsLogin` only; else empty).
    pub methods: Vec<LoginMethod>,
}

/// How a sign-in works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LoginKind {
    /// The agent signs in by itself (it may open the browser): `acp.login { methodId }`.
    Agent,
    /// The person runs `hint` in a terminal, then `acp.login {}` tries again.
    Terminal,
    /// The person sets the variables named in `hint`, restarts the app, then tries again.
    Env,
}

/// One way to sign in that the agent offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoginMethod {
    pub id: String,
    /// Label from the agent.
    pub name: String,
    pub description: Option<String>,
    pub kind: LoginKind,
    /// `terminal`: the whole command line to run (the agent's command + the method's arguments; variables first as
    /// `NAME=value`); `env`: the variable names, comma separated; `agent`: `null`.
    pub hint: Option<String>,
}

/// How a listed agent would start on this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentHow {
    /// Its own program is installed (found on `PATH`).
    Native,
    /// Through Node's `npx` (downloads the agent's package on first use).
    Npx,
    /// Through uv's `uvx` (downloads the agent's package on first use).
    Uvx,
    /// Nothing to start it with: see `needs`.
    Missing,
    /// The person's own command.
    Custom,
}

/// What the person must install before an agent can start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentNeed {
    /// Node.js (`npx` was not found).
    Node,
    /// uv (`uvx` was not found).
    Uv,
    /// The agent's own program (see `website`) | the own command's program was not found.
    Install,
}

/// One agent the person can choose (`acp.agents`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentEntry {
    /// Registry id (`custom`: the person's own command).
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub how: AgentHow,
    /// Set when `how` is `missing` (or a custom program is not found).
    pub needs: Option<AgentNeed>,
    /// The command as it would run (program by name).
    pub command: String,
    /// The agent's website, else its repository.
    pub website: Option<String>,
    pub chosen: bool,
}

/// Arguments of `acp.choose`: exactly one of `agent` (a registry id of `acp.agents`) | `command` (the person's own).
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ChooseArgs {
    #[serde(default)]
    #[ts(optional)]
    pub agent: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub command: Option<String>,
}

/// Arguments of `acp.login`: `methodId` of an `agent` method -> the agent signs in, then the prompt goes on; any
/// other method | none -> tries again (the person signed in outside the app).
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct LoginArgs {
    #[serde(default)]
    #[ts(optional)]
    pub method_id: Option<String>,
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
    /// The state changed; `error` with `error` | a failed sign-in (or a prompt that could not be sent: state stays
    /// `ready`); `methods` only with `needsLogin`.
    Status {
        state: AcpState,
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        methods: Option<Vec<LoginMethod>>,
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
