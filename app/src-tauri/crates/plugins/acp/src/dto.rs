//! What the window and the plugin exchange: the agent's state and news, prompts, answers, the copy's changes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The copy's changes as last computed (at each turn's end | on `refresh`; a choice updates its own file): at most
/// 500 files, by path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChangeList {
    pub files: Vec<FileChange>,
    /// Changed files past the first 500, not listed.
    pub more: u32,
}

impl ChangeList {
    /// Every changed file, listed or not.
    #[must_use]
    pub fn count(&self) -> u32 {
        u32::try_from(self.files.len())
            .unwrap_or(u32::MAX)
            .saturating_add(self.more)
    }
}

/// One file whose copy differs from the real file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileChange {
    /// Project path (`/` separators).
    pub path: String,
    pub kind: FileChangeKind,
    /// Line runs that differ (`modified`); the whole text as one run (`added` | `deleted` text files); none for
    /// `binary` | `conflict` | `link` (whole-file choices only).
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
    /// A link | special file in the copy (never read through): only Reject, which removes it (a file the copy had
    /// there gets the real one back).
    Link,
}

/// One run of lines that differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HunkView {
    /// Names this run by its file, place and lines (what accept | reject name): a key of an older list that no
    /// longer matches is refused (`outdated`), never applied to another run.
    pub key: String,
    /// 1-based line of the real file where the run starts (no old lines: the line the new ones go before).
    pub old_start: u32,
    /// At most 400 lines per side (`truncated`).
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    /// A side had more than 400 lines: only the first are shown; a choice still applies the whole run.
    pub truncated: bool,
}

/// Arguments of `acp.changes`.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ChangesArgs {
    /// Computes the list again (else the kept one).
    #[serde(default)]
    #[ts(optional)]
    pub refresh: Option<bool>,
}

/// Arguments of `acp.accept` | `acp.reject`.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct DecideArgs {
    /// Project path.
    pub path: String,
    /// `HunkView.key`s; `null`: the whole file as it is now.
    #[serde(default)]
    pub hunks: Option<Vec<String>>,
}

/// Arguments of `acp.undo`: takes back the last accept | reject of the file, once. Kept in memory only: lost when
/// the plugin stops (turned off, project closed, app quit); the oldest are dropped past 64 MiB of kept bytes.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct UndoArgs {
    /// Project path.
    pub path: String,
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

/// What went wrong with the agent: a closed set the window words in the person's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AcpErrorCode {
    /// No agent is chosen.
    NoAgent,
    /// The chosen agent's program was not found (`AcpStatus.needs` says what to install).
    NotFound,
    /// No profile folder: no work copy can be made, nothing can start.
    NoProfile,
    /// The work copy could not be made | opened | cleaned.
    CopyFailed,
    /// The project is too large for a work copy.
    TooLarge,
    /// The projects inside the folder (their rules on what agents may see) are not known yet: try again soon.
    ProjectsNotReady,
    /// The agent's program could not be started.
    StartFailed,
    /// The agent did not answer a step of its start | sign-in in time.
    TimedOut,
    /// The agent refused | garbled `initialize`.
    InitFailed,
    /// The agent speaks another version of the protocol.
    ProtocolVersion,
    /// The agent refused | garbled opening the conversation.
    SessionFailed,
    /// The agent's own sign-in did not work.
    LoginFailed,
    /// Tried again after a sign-in outside the app: the agent still wants one.
    StillNeedsLogin,
    /// The agent refused the message.
    Refused,
    /// The agent's answer to the message could not be read.
    Protocol,
    /// The agent's program ended by itself.
    Exited,
    /// The message could not be handed to the agent.
    SendFailed,
}

/// Why the agent failed | a step did not work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AcpError {
    pub code: AcpErrorCode,
    /// The agent's own words with a refusal (`initFailed` | `sessionFailed` | `loginFailed` | `refused`), shown as
    /// they are (at most 2 KiB).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
    /// The newest lines the agent's program wrote to its error output (at most 2 KiB, terminal color codes removed,
    /// account news left out), when its start | its run failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stderr: Option<String>,
}

impl AcpError {
    #[must_use]
    pub fn new(code: AcpErrorCode) -> Self {
        Self {
            code,
            message: None,
            stderr: None,
        }
    }
}

/// Answer of `acp.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AcpStatus {
    pub state: AcpState,
    /// The chosen agent's command as it would run (`""`: none chosen).
    pub agent: String,
    /// The chosen agent's registry name; `null`: none chosen | the person's own command (the window words it).
    pub agent_name: Option<String>,
    /// An agent is chosen (`false`: a prompt is refused with `noAgent` until `acp.choose`).
    pub chosen: bool,
    /// Why the agent is in `error` | why a sign-in failed.
    pub error: Option<AcpError>,
    /// How the person can sign in (`needsLogin` only; else empty).
    pub methods: Vec<LoginMethod>,
    /// What the person must do before the chosen agent can start (absent: nothing); `snap` also when none is chosen
    /// (no agent can start in this package).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub needs: Option<AgentNeed>,
    /// Files the work copy left out for their size (over 4 MiB) at its last build | update; `0` before the first.
    pub skipped_files: u32,
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
    /// Nothing can start: the app runs inside a strict Snap, which sees none of the computer's programs.
    Snap,
}

/// One agent the person can choose (`acp.agents`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentEntry {
    /// Registry id (`custom`: the person's own command).
    pub id: String,
    /// Registry name (`""` for the person's own command: the window words it).
    pub name: String,
    pub description: String,
    pub version: String,
    pub how: AgentHow,
    /// Set when `how` is `missing` (or a custom program is not found).
    pub needs: Option<AgentNeed>,
    /// The command as it would run: a found own program | own command by its full path (what really starts); else
    /// the program by name.
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

/// What kind of tool the agent runs (the protocol's names; one this client does not know -> `other`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AcpToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    SwitchMode,
    Other,
}

/// Where a tool call stands (the protocol's names; one this client does not know -> `other`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AcpToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Other,
}

/// Why a turn ended: the protocol's stop reasons (one this client does not know -> `other`, a normal end), or
/// `error` (the turn failed on this side: the status news tells why).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AcpStopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    Error,
    Other,
}

/// What an answer to a permission question does (the protocol's names; one this client does not know -> `other`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AcpOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    Other,
}

/// One answer the agent offers for a permission question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PermissionChoice {
    pub id: String,
    /// Label from the agent.
    pub name: String,
    pub kind: AcpOptionKind,
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
        error: Option<AcpError>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        methods: Option<Vec<LoginMethod>>,
    },
    /// A piece of the agent's reply (append).
    Text { text: String },
    /// The agent thinks (its thoughts are not shown); once per stretch of thinking.
    Thinking {},
    /// A tool the agent runs (upsert by `id`).
    Tool {
        id: String,
        title: String,
        kind: AcpToolKind,
        status: AcpToolStatus,
    },
    /// The agent asks before doing something that is not plainly work on the copy's files: the person answers with
    /// `acp.answer`. Title, kind and detail come from everything the agent told of the tool call so far.
    Permission {
        request_id: String,
        title: String,
        kind: AcpToolKind,
        /// The command to run, else the paths it touches (project paths inside the copy), one per line.
        detail: String,
        options: Vec<PermissionChoice>,
    },
    /// The question was answered | dropped (Stop, the agent withdrew it, the turn ended, new conversation): its card
    /// goes.
    PermissionDone { request_id: String },
    /// The prompt ended; `changed`: files whose copy now differs from the real file (`0` when Stop ended a start | a
    /// sign-in: the prompt never reached the agent).
    TurnEnd {
        stop_reason: AcpStopReason,
        changed: u32,
    },
}
