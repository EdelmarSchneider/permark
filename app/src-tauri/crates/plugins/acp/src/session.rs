//! The agent of one project instance, spoken to over the Agent Client Protocol (v1).
//!
//! - started at the first prompt, on a thread of its own (the RPC thread never waits on the agent): the person's
//!   choice read now ([`Agents`]: a new choice applies at the next start), work copy made when absent and cleared of
//!   agent settings, the agent's program started in an empty folder of the plugin's own ([`Host::run`]: never the
//!   copy, whose project files a runner such as `npx` would read as its settings), `initialize` (no file system, no
//!   terminal offered: the agent works on the copy with its own tools; terminal sign-in methods are welcome: the
//!   person runs them) -> `session/new` (folder = the copy, no MCP servers) -> the asking mode `default` when offered:
//!   `session/set_mode`, else through a mode config option (`session/set_config_option`): a person's own "auto"
//!   default would let the agent skip every question.
//! - `auth_required` at `session/new` | a prompt -> `NeedsLogin` with the agent's sign-in methods; `acp.login` with an
//!   agent method has the agent sign in (`authenticate`) and the waiting prompt goes on; any other (the person signed
//!   in outside the app) starts a fresh agent, which reads the new sign-in at its start.
//! - each prompt: agent settings the agent wrote into the copy go ([`WorkCopy::purge`]: they would load at its next
//!   start and could allow anything), the copy takes the person's latest changes ([`WorkCopy::sync`]), then
//!   `session/prompt`; the turn ends with the agent's stop reason and the count of changed files (the change list is
//!   computed then, once, and kept for the windows: [`WorkCopy::refresh`]). A passage of a file
//!   agents may not see is refused; such an open document is not named.
//! - Stop: before the prompt is on its way it is never sent (the turn ends `cancelled`); after, `session/cancel`; a
//!   question asked after Stop is answered `cancelled` at once. Stop during a start stops the agent.
//! - news (`session/update`) -> `acp:update` events: reply text, "thinking", tool lines, the agent's settings the
//!   person may change ([`options`]); a title is kept with the conversation. Anything else the agent tells (modes,
//!   commands, usage, account status) is dropped unread.
//! - permission questions ([`permission`]): plainly work on the copy's files -> allowed once by rule; anything else
//!   is a card the person answers, told while the question is held under the session's lock (whatever answers it
//!   takes that lock: its card always comes before its end). Every question gets exactly one answer: the person's,
//!   `cancelled` (Stop, the turn ended, the agent left), or `-32800` when the agent withdrew it (`$/cancel_request`).
//!   A card's id names the agent's run too: a fresh agent numbers its questions from the start again.
//! - errors reach the window as codes ([`AcpErrorCode`]) with the agent's own words: its refusal, its last stderr.
//! - continue ([`resume`]): the project's last conversation in a fresh agent: `session/load` (the agent replays it
//!   as news: the person's messages, its replies, tool lines; no question of the replay reaches the person) else
//!   `session/resume`, in the copy's folder, then the asking mode again; it ends `Ready` with no prompt sent.
//! - every session opened | continued: after the asking mode, the person's picks of the agent's settings
//!   ([`options`]: its model, reasoning level) apply where the agent still offers them.
//! - states: Idle -> Starting -> Working <-> Ready; Starting (continue) -> Ready; Starting | Working -> `NeedsLogin`
//!   -> Starting (login); failed start | agent gone -> Error (the next prompt starts a fresh one); "new conversation" |
//!   a new choice -> Idle (the copy and its pending changes stay). Each run carries the generation it was started for
//!   and does nothing once a newer one replaced it.

mod login;
mod options;
mod permission;
mod resume;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use agent_client_protocol_schema::v1::{
    AuthCapabilities, AuthenticateRequest, CancelNotification, CancelRequestNotification,
    ClientCapabilities, ContentBlock, EmbeddedResource, EmbeddedResourceResource,
    Error as RpcError, Implementation, InitializeRequest, InitializeResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, PermissionOptionId,
    PermissionOptionKind, PromptRequest, RequestId, RequestPermissionOutcome,
    RequestPermissionResponse, ResourceLink, ResumeSessionRequest, ResumeSessionResponse,
    SelectedPermissionOutcome, SessionConfigId, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory, SessionId, SessionModeState, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionModeRequest, StopReason, TextContent,
    TextResourceContents, ToolCallStatus, ToolKind, AGENT_METHOD_NAMES, CLIENT_METHOD_NAMES,
    PROTOCOL_LEVEL_METHOD_NAMES,
};
use agent_client_protocol_schema::MaybeUndefined;
use agent_client_protocol_schema::ProtocolVersion;
use permark_core::paths::{self, Root, Vault, VaultPath};
use permark_core::plugins::PluginEvents;
use permark_core::state::StateFile;
use permark_core::{ErrorKind, IpcError};
use serde_json::Value;

use crate::agents::{Agents, Choice, Plan};
use crate::client::{lock, AgentPipes, CallError, Connection, Inbox};
use crate::dto::{
    AcpConfigOption, AcpError, AcpErrorCode, AcpOptionKind, AcpState, AcpStatus, AcpStopReason,
    AcpToolKind, AcpToolStatus, AcpUpdate, AgentEntry, AnswerArgs, ChooseArgs, ContinueWay,
    LoginArgs, LoginKind, LoginMethod, PermissionChoice, PromptArgs, PromptSelection, ResolveArgs,
    ResolvedCommand, SetOptionArgs,
};
use crate::reply;
use crate::work::{copy_dir, kept_out, start_of, Project, WorkCopy};
use crate::SharedCopy;
use login::login_methods;
pub use options::{Picks, OPTIONS_FILE};
use permission::{allow_once, detail, diff, plain_work, reach, summary, Question, ToolLine};
pub use resume::{Remembered, Saved, SAVED_FILE};

/// Longest wait for each step of the start (the first `npx` run downloads the adapter).
const SETUP_WAIT: Duration = Duration::from_secs(180);
/// Longest wait for an agent's own sign-in (it may wait for the person in the browser).
const LOGIN_WAIT: Duration = Duration::from_secs(600);
/// Most bytes of the agent's own words kept with an error (its last stderr lines, a refusal's message).
const WORDS_CAP: usize = 2 << 10;
/// Wait for the agent's last stderr lines after its output ended.
const TAIL_SETTLE: Duration = Duration::from_millis(500);
/// The agent mode that asks before changes.
const ASK_MODE: &str = "default";
/// Longest passage a prompt attaches: it rides along in one line to the agent and stays queued while the agent reads.
pub const MAX_PASSAGE_BYTES: usize = 256 << 10;

/// What a session works with.
pub struct Host {
    /// The catalog + the person's choice (read at every start).
    pub agents: Agents,
    pub launch: crate::agents::Launch,
    pub project: Project,
    /// Folder the copy is made in (`None`: no profile, no copy can be made).
    pub place: Option<PathBuf>,
    /// Folder the agent's program starts in, emptied before each start, outside the project and the copy: a runner
    /// (`npx`, `uvx`) reads its settings from the folder it starts in, and a project's must never choose what it runs
    /// (`None`: no profile, nothing starts).
    pub run: Option<PathBuf>,
    pub copy: SharedCopy,
    /// The project's last conversation, in the plugin's profile folder for the project ([`resume`]; `None`: no
    /// profile, nothing is kept).
    pub saved: Option<StateFile<Remembered>>,
    /// The person's picks of agent settings, in the profile folder itself ([`options`]; `None`: no profile, nothing
    /// is kept or applied).
    pub picks: Option<StateFile<Picks>>,
    pub events: PluginEvents,
}

/// The agent of one project instance (cheap to clone: one shared state).
#[derive(Clone)]
pub struct Session {
    core: Arc<Core>,
}

struct Core {
    host: Host,
    inner: Mutex<Inner>,
    /// Files the copy left out for their size at its last build | update (read by the status without the copy's lock,
    /// which a build | an update holds for long).
    skipped: AtomicU32,
    /// The places the copy is named by ([`reply::copy_spellings`]): never shown in the person's replayed words.
    copy_names: Vec<PathBuf>,
}

#[derive(Default)]
struct Inner {
    state: State,
    /// Bumped by every fresh start | stop: runs and listeners of an older one do nothing.
    generation: u64,
    /// Permission questions waiting for the person, by the id the window got.
    asks: HashMap<String, Ask>,
    /// What the agent told of each tool call of the turn, by its id.
    tools: HashMap<String, ToolLine>,
    /// "Thinking" was told since the last other news.
    thinking: bool,
    /// Stop was pressed during the current prompt.
    stopping: bool,
    /// The current prompt is on its way to the agent (queued under this lock: a Stop either finds it unsent and it
    /// never goes, or comes after it).
    sent: bool,
    /// What the agent's news is now.
    telling: Telling,
    /// The config options of the agent's session as last told (empty: no session).
    options: Vec<SessionConfigOption>,
    /// Of `options`, those the window shows, as it was last told ([`options::exposed`]).
    shown: Vec<AcpConfigOption>,
}

/// What the agent's news is: told as it happens | a conversation replayed by `session/load` (the person's messages
/// are shown again, thoughts are not).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Telling {
    #[default]
    Live,
    Replayed,
}

impl Inner {
    /// The questions still waiting, all of them taken (their answers are the caller's: [`Core::close_asks`]).
    fn take_asks(&mut self) -> Vec<(String, Ask)> {
        self.asks.drain().collect()
    }

    /// A new prompt begins: no Stop for it yet.
    fn begin_prompt(&mut self) {
        self.stopping = false;
        self.sent = false;
    }

    /// Ends the current agent's time: state `Idle` (the window drops its settings with it), generation bumped;
    /// answers the agent to stop + its questions.
    fn end_agent(&mut self) -> (Option<Arc<Agent>>, Vec<(String, Ask)>) {
        self.generation += 1;
        let agent = std::mem::take(&mut self.state).agent();
        self.tools.clear();
        self.drop_options();
        (agent, self.take_asks())
    }

    /// A fresh agent takes over: generation bumped, `Starting`; the settings of the one before go (`Some`: the window
    /// still shows some, the news that empties them). Answers the agent that ran, to stop.
    fn restart(&mut self) -> (Option<Arc<Agent>>, Option<AcpUpdate>) {
        let left = self.state.agent();
        self.generation += 1;
        self.state = State::Starting(None);
        let cleared = self.drop_options().then(|| AcpUpdate::Options {
            options: Vec::new(),
        });
        (left, cleared)
    }
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    /// The start runs; the agent once its program started.
    Starting(Option<Arc<Agent>>),
    Ready(Live),
    Working(Live),
    Error(AcpError),
    /// The agent waits for the person to sign in.
    NeedsLogin(Login),
}

impl State {
    fn name(&self) -> AcpState {
        match self {
            Self::Idle => AcpState::Idle,
            Self::Starting(_) => AcpState::Starting,
            Self::Ready(_) => AcpState::Ready,
            Self::Working(_) => AcpState::Working,
            Self::Error(_) => AcpState::Error,
            Self::NeedsLogin(_) => AcpState::NeedsLogin,
        }
    }

    fn agent(&self) -> Option<Arc<Agent>> {
        match self {
            Self::Starting(agent) => agent.clone(),
            Self::Ready(live) | Self::Working(live) => Some(Arc::clone(&live.agent)),
            Self::NeedsLogin(login) => Some(Arc::clone(&login.agent)),
            Self::Idle | Self::Error(_) => None,
        }
    }

    fn error(&self) -> Option<AcpError> {
        match self {
            Self::Error(error) => Some(error.clone()),
            Self::NeedsLogin(login) => login.error.clone(),
            _ => None,
        }
    }

    fn methods(&self) -> Option<Vec<LoginMethod>> {
        match self {
            Self::NeedsLogin(login) => Some(login.methods.to_vec()),
            _ => None,
        }
    }

    fn update(&self) -> AcpUpdate {
        AcpUpdate::Status {
            state: self.name(),
            error: self.error(),
            methods: self.methods(),
        }
    }
}

/// A started agent with an open session.
#[derive(Clone)]
struct Live {
    agent: Arc<Agent>,
    session: SessionId,
    /// The agent's sign-in methods (from `initialize`).
    methods: Arc<[LoginMethod]>,
    /// The agent takes embedded resources in prompts (`promptCapabilities.embeddedContext`).
    embedded: bool,
}

/// An agent waiting for the person to sign in, with what goes on afterwards.
struct Login {
    agent: Arc<Agent>,
    /// The session the prompt was refused in (`None`: opening the session was refused).
    session: Option<SessionId>,
    then: Then,
    methods: Arc<[LoginMethod]>,
    embedded: bool,
    /// Why the last sign-in did not work.
    error: Option<AcpError>,
}

/// One running agent program.
struct Agent {
    connection: Arc<Connection>,
    stop: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// The copy's folder, as sessions are opened in it.
    dir: PathBuf,
    /// The copy as a folder of its own: the boundary of "work on the copy's files".
    copy: Vault,
    /// The person's choice it was started for.
    choice: Choice,
    /// The copy was made for this start (changes an earlier run left in it are gone).
    fresh: bool,
    /// How it offers to continue a conversation (set by `initialize`).
    way: OnceLock<Option<ContinueWay>>,
}

impl Agent {
    fn way(&self) -> Option<ContinueWay> {
        self.way.get().copied().flatten()
    }

    /// Closes its stdin (never waits on a write the agent does not read), then ends it (grace, then its whole process
    /// tree killed); every call waiting on it answers `Closed`.
    fn stop(&self) {
        self.connection.close();
        let stop = lock(&self.stop).take();
        if let Some(stop) = stop {
            stop();
        }
    }
}

/// Stops `agent` on a thread of its own (the grace must not hold the caller).
fn stop_aside(agent: &Arc<Agent>) {
    let spare = Arc::clone(agent);
    let spawned = std::thread::Builder::new()
        .name("acp-agent-stop".into())
        .spawn(move || spare.stop());
    if spawned.is_err() {
        agent.stop();
    }
}

/// A permission question waiting for the person.
struct Ask {
    id: RequestId,
    options: Vec<PermissionOptionId>,
}

/// A tool kind as the window's closed set (one of a later protocol release -> `other`).
fn tool_kind(kind: ToolKind) -> AcpToolKind {
    match kind {
        ToolKind::Read => AcpToolKind::Read,
        ToolKind::Edit => AcpToolKind::Edit,
        ToolKind::Delete => AcpToolKind::Delete,
        ToolKind::Move => AcpToolKind::Move,
        ToolKind::Search => AcpToolKind::Search,
        ToolKind::Execute => AcpToolKind::Execute,
        ToolKind::Think => AcpToolKind::Think,
        ToolKind::Fetch => AcpToolKind::Fetch,
        ToolKind::SwitchMode => AcpToolKind::SwitchMode,
        _ => AcpToolKind::Other,
    }
}

/// A tool call status as the window's closed set (one of a later protocol release -> `other`).
fn tool_status(status: ToolCallStatus) -> AcpToolStatus {
    match status {
        ToolCallStatus::Pending => AcpToolStatus::Pending,
        ToolCallStatus::InProgress => AcpToolStatus::InProgress,
        ToolCallStatus::Completed => AcpToolStatus::Completed,
        ToolCallStatus::Failed => AcpToolStatus::Failed,
        _ => AcpToolStatus::Other,
    }
}

/// A permission option kind as the window's closed set (one of a later protocol release -> `other`).
fn option_kind(kind: PermissionOptionKind) -> AcpOptionKind {
    match kind {
        PermissionOptionKind::AllowOnce => AcpOptionKind::AllowOnce,
        PermissionOptionKind::AllowAlways => AcpOptionKind::AllowAlways,
        PermissionOptionKind::RejectOnce => AcpOptionKind::RejectOnce,
        PermissionOptionKind::RejectAlways => AcpOptionKind::RejectAlways,
        _ => AcpOptionKind::Other,
    }
}

/// What a run started by a prompt does.
enum Run {
    Start,
    Turn(Live),
}

/// What a start goes on with once its session is open.
#[derive(Clone)]
enum Then {
    /// A new conversation takes this prompt.
    Prompt(PromptArgs),
    /// The kept conversation is continued; no prompt is sent.
    Continue(Saved),
}

/// What a start opened.
enum Opened {
    Live(Live),
    /// The agent wants the person to sign in before a session.
    Login(Arc<Agent>, Arc<[LoginMethod]>, bool),
}

/// Why no session was opened.
enum Opening {
    Login,
    Failed(AcpError),
}

/// What a permission question gets.
enum Route {
    /// Allowed once by rule (this option).
    Allow(PermissionOptionId),
    /// A card for the person (told already).
    Asked,
    /// Asked outside a turn of the current agent | after Stop: answered `cancelled`.
    Drop,
}

/// The window's id of question `id` of the agent run `generation`: the run, then the JSON text of the agent's id
/// (unique for numbers and strings alike); a fresh agent numbers its questions from the start again.
fn question_key(generation: u64, id: &RequestId) -> String {
    format!(
        "{generation}:{}",
        serde_json::to_string(id).unwrap_or_default()
    )
}

/// The step of a start | sign-in a failed call belongs to.
#[derive(Clone, Copy)]
enum Step {
    Init,
    Session,
    Login,
}

impl Step {
    fn code(self) -> AcpErrorCode {
        match self {
            Self::Init => AcpErrorCode::InitFailed,
            Self::Session => AcpErrorCode::SessionFailed,
            Self::Login => AcpErrorCode::LoginFailed,
        }
    }
}

impl Session {
    #[must_use]
    pub fn new(host: Host) -> Self {
        let copy_names = reply::copy_spellings(host.place.as_deref(), host.project.root());
        Self {
            core: Arc::new(Core {
                host,
                inner: Mutex::new(Inner::default()),
                skipped: AtomicU32::new(0),
                copy_names,
            }),
        }
    }

    /// The places the copy is named by ([`reply::copy_spellings`]).
    #[must_use]
    pub fn copy_names(&self) -> &[PathBuf] {
        &self.core.copy_names
    }

    #[must_use]
    pub fn status(&self) -> AcpStatus {
        // read before the lock: files and `PATH` lookups
        let chosen = self.core.host.agents.chosen();
        let last = chosen
            .as_ref()
            .and_then(|c| self.core.saved()?.offer(&c.choice));
        let inner = lock(&self.core.inner);
        AcpStatus {
            state: inner.state.name(),
            agent: chosen
                .as_ref()
                .map(|c| c.resolved.shown.clone())
                .unwrap_or_default(),
            agent_name: chosen.as_ref().and_then(|c| c.name.clone()),
            chosen: chosen.is_some(),
            error: inner.state.error(),
            methods: inner.state.methods().unwrap_or_default(),
            needs: chosen
                .as_ref()
                .map_or_else(|| self.core.host.agents.blocked(), |c| c.resolved.needs),
            skipped_files: self.core.skipped.load(Ordering::Relaxed),
            last,
            options: inner.shown.clone(),
        }
    }

    /// The agents the person can choose, as each would start on this computer.
    #[must_use]
    pub fn agents(&self) -> Vec<AgentEntry> {
        self.core.host.agents.list()
    }

    /// What a choice of the own command `args` would do, nothing kept and nothing stopped ([`Agents::preview`]).
    #[must_use]
    pub fn resolve(&self, args: &ResolveArgs) -> ResolvedCommand {
        self.core.host.agents.preview(&args.command)
    }

    /// Keeps the person's choice and stops the agent (the copy and its pending changes stay): the next prompt starts
    /// the chosen one. Answers the list with the new choice.
    ///
    /// # Errors
    /// `Conflict` (`busy`): the agent starts | works; the refusals of [`Agents::choose`].
    pub fn choose(&self, args: &ChooseArgs) -> Result<Vec<AgentEntry>, IpcError> {
        {
            let inner = lock(&self.core.inner);
            if matches!(inner.state, State::Starting(_) | State::Working(_)) {
                return Err(IpcError::new(ErrorKind::Conflict).detail("busy"));
            }
            // written under the lock: a start begun meanwhile reads the new choice
            self.core.host.agents.choose(args)?;
        }
        self.reset();
        Ok(self.core.host.agents.list())
    }

    /// Sends a prompt: starts the agent first when none runs. Answers at once; the reply comes as events.
    ///
    /// # Errors
    /// `Unavailable` (`noAgent`): no agent chosen; `Conflict` (`busy`): the agent starts | works; `Denied`
    /// (`hiddenFromAgents`): the passage is of a file agents may not see; `Unavailable` (`projectsNotReady`): what
    /// agents may see is not known yet; `InvalidData` | `InvalidPath`: a prompt that cannot be sent.
    pub fn prompt(&self, args: PromptArgs) -> Result<(), IpcError> {
        check(&args)?;
        if self.core.host.agents.chosen().is_none() {
            return Err(IpcError::new(ErrorKind::Unavailable).detail("noAgent"));
        }
        let args = self.core.screen(args)?;
        let mut inner = lock(&self.core.inner);
        let (mut left, mut cleared) = (None, None);
        let run = match &inner.state {
            State::Starting(_) | State::Working(_) => {
                return Err(IpcError::new(ErrorKind::Conflict).detail("busy"));
            }
            // a prompt instead of signing in: a fresh agent for it
            State::Idle | State::Error(_) | State::NeedsLogin(_) => {
                (left, cleared) = inner.restart();
                Run::Start
            }
            State::Ready(live) => {
                let live = live.clone();
                inner.state = State::Working(live.clone());
                Run::Turn(live)
            }
        };
        inner.begin_prompt();
        let generation = inner.generation;
        let update = inner.state.update();
        drop(inner);
        if let Some(agent) = left {
            stop_aside(&agent);
        }
        self.core.emit_all(cleared.into_iter().chain([update]));
        self.run_aside(
            "acp-prompt",
            generation,
            AcpErrorCode::SendFailed,
            move |core| match run {
                Run::Start => core.start(generation, &Then::Prompt(args), false),
                Run::Turn(live) => core.turn(generation, &live, &args),
            },
        )
    }

    /// Continues the project's last conversation in a fresh agent (one running stops first): the agent replays it |
    /// remembers it, it is set to ask again, no prompt is sent. Answers at once; the outcome comes as events.
    ///
    /// # Errors
    /// `Unavailable` (`noAgent`): no agent chosen; `NotFound` (`noSaved`): no conversation kept that the chosen agent
    /// can continue; `Conflict` (`busy`): the agent starts | works.
    pub fn continue_last(&self) -> Result<(), IpcError> {
        let chosen = self
            .core
            .host
            .agents
            .chosen()
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).detail("noAgent"))?;
        let saved = self
            .core
            .saved()
            .filter(|saved| saved.offer(&chosen.choice).is_some())
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).detail("noSaved"))?;
        let mut inner = lock(&self.core.inner);
        if matches!(inner.state, State::Starting(_) | State::Working(_)) {
            return Err(IpcError::new(ErrorKind::Conflict).detail("busy"));
        }
        let (left, cleared) = inner.restart();
        inner.begin_prompt();
        let generation = inner.generation;
        let update = inner.state.update();
        drop(inner);
        if let Some(agent) = left {
            stop_aside(&agent);
        }
        self.core.emit_all(cleared.into_iter().chain([update]));
        self.run_aside(
            "acp-continue",
            generation,
            AcpErrorCode::StartFailed,
            move |core| core.start(generation, &Then::Continue(saved), false),
        )
    }

    /// Runs `job` for `generation` on a thread of its own (the RPC thread never waits on the agent); a thread that
    /// cannot start fails the run with `failure`.
    fn run_aside(
        &self,
        name: &str,
        generation: u64,
        failure: AcpErrorCode,
        job: impl FnOnce(&Arc<Core>) + Send + 'static,
    ) -> Result<(), IpcError> {
        let core = Arc::clone(&self.core);
        let spawned = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || job(&core));
        if let Err(error) = spawned {
            self.core.fail(generation, AcpError::new(failure));
            return Err(IpcError::io("acp", &error));
        }
        Ok(())
    }

    /// The copy now holds `changed` files waiting for the person (after an accept | reject | undo).
    pub fn changes_left(&self, changed: u32) {
        self.core.changes_left(changed);
    }

    /// Stop. During a prompt: a prompt not yet on its way is never sent, one on its way is cancelled
    /// (`session/cancel`); questions waiting are answered `cancelled`, later ones too. During a start | a sign-in: the
    /// agent stops (the waiting prompt never reaches it) and the turn ends `cancelled`.
    pub fn cancel(&self) {
        let mut inner = lock(&self.core.inner);
        match &inner.state {
            State::Working(live) => {
                let live = live.clone();
                inner.stopping = true;
                let sent = inner.sent;
                let asks = inner.take_asks();
                drop(inner);
                let connection = &live.agent.connection;
                if sent {
                    let _ = connection.notify(
                        AGENT_METHOD_NAMES.session_cancel,
                        CancelNotification::new(live.session.clone()),
                    );
                }
                self.core.close_asks(connection, asks);
            }
            State::Starting(_) | State::NeedsLogin(_) => {
                let (agent, asks) = inner.end_agent();
                drop(inner);
                self.core.leave(
                    agent,
                    asks,
                    Some(AcpUpdate::TurnEnd {
                        stop_reason: AcpStopReason::Cancelled,
                        changed: 0,
                    }),
                );
            }
            State::Idle | State::Ready(_) | State::Error(_) => {}
        }
    }

    /// The person signs in: an `agent` method -> the agent signs in itself, then the waiting prompt goes on. Any other
    /// | none (the person signed in outside the app: a terminal, variables) -> a fresh agent starts for the prompt and
    /// reads the new sign-in at its start. Answers at once; the outcome comes as events.
    ///
    /// # Errors
    /// `NotFound` (`noLogin`): no sign-in waits; `InvalidData` (`unknownMethod`).
    pub fn login(&self, args: &LoginArgs) -> Result<(), IpcError> {
        let mut inner = lock(&self.core.inner);
        let login = match std::mem::take(&mut inner.state) {
            State::NeedsLogin(login) => login,
            other => {
                inner.state = other;
                return Err(IpcError::new(ErrorKind::NotFound).detail("noLogin"));
            }
        };
        let method = match &args.method_id {
            None => None,
            Some(id) => {
                let Some(method) = login.methods.iter().find(|m| &m.id == id) else {
                    inner.state = State::NeedsLogin(login);
                    return Err(IpcError::new(ErrorKind::InvalidData)
                        .subject(id.clone())
                        .detail("unknownMethod"));
                };
                (method.kind == LoginKind::Agent).then(|| method.id.clone())
            }
        };
        // the person's go-ahead for the waiting prompt: an earlier Stop no longer holds it back
        inner.begin_prompt();
        let (left, cleared) = if method.is_some() {
            inner.state = State::Starting(Some(Arc::clone(&login.agent)));
            (None, None)
        } else {
            // the state was taken out above: the agent that waited is the one to stop
            let (_, cleared) = inner.restart();
            (Some(Arc::clone(&login.agent)), cleared)
        };
        let generation = inner.generation;
        let update = inner.state.update();
        drop(inner);
        if let Some(agent) = left {
            stop_aside(&agent);
        }
        self.core.emit_all(cleared.into_iter().chain([update]));
        self.run_aside(
            "acp-login",
            generation,
            AcpErrorCode::LoginFailed,
            move |core| match method {
                Some(method) => core.sign_in(generation, login, method),
                None => core.start(generation, &login.then, true),
            },
        )
    }

    /// New conversation: the agent stops; the next prompt starts a fresh one (the copy and its changes stay).
    pub fn reset(&self) {
        let (agent, asks) = lock(&self.core.inner).end_agent();
        self.core.leave(agent, asks, None);
    }

    /// Plugin off | project closed: the agent stops before the copy goes (waits for it).
    pub fn shutdown(&self) {
        let (agent, asks) = lock(&self.core.inner).end_agent();
        if let Some(agent) = agent {
            self.core.close_asks(&agent.connection, asks);
            agent.stop();
        }
    }

    /// The person's answer to a permission card (`None`: dismissed, the agent hears `cancelled`).
    ///
    /// # Errors
    /// `NotFound` (`noQuestion`): no such question waits; `InvalidData` (`unknownOption`); IO toward the agent.
    pub fn answer(&self, args: &AnswerArgs) -> Result<(), IpcError> {
        let mut inner = lock(&self.core.inner);
        let no_question = || {
            IpcError::new(ErrorKind::NotFound)
                .subject(args.request_id.clone())
                .detail("noQuestion")
        };
        let State::Working(live) = &inner.state else {
            return Err(no_question());
        };
        let connection = Arc::clone(&live.agent.connection);
        let ask = inner.asks.get(&args.request_id).ok_or_else(no_question)?;
        let outcome = match &args.option_id {
            None => RequestPermissionOutcome::Cancelled,
            Some(option) => {
                let id = ask
                    .options
                    .iter()
                    .find(|id| &*id.0 == option.as_str())
                    .cloned()
                    .ok_or_else(|| {
                        IpcError::new(ErrorKind::InvalidData)
                            .subject(option.clone())
                            .detail("unknownOption")
                    })?;
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id))
            }
        };
        let ask = inner
            .asks
            .remove(&args.request_id)
            .ok_or_else(no_question)?;
        drop(inner);
        connection
            .reply(ask.id, Ok(RequestPermissionResponse::new(outcome)))
            .map_err(|e| IpcError::io("agent", &e))?;
        self.core.emit(AcpUpdate::PermissionDone {
            request_id: args.request_id.clone(),
        });
        Ok(())
    }

    /// The person changes one of the agent's settings ([`options`]): sent aside (the RPC thread never waits on the
    /// agent); the new values come as `options` news, a refusal as a status news with `optionRefused`. Kept as the
    /// person's pick for this agent once the agent took it.
    ///
    /// # Errors
    /// `Conflict` (`starting`): the agent starts; `Unavailable` (`notRunning`): no agent runs | it waits for a
    /// sign-in; `NotFound` (`unknownSetting`): no such setting the window may change; `InvalidData` (`notOffered`): a
    /// value it does not offer; IO: no thread for it.
    pub fn set_option(&self, args: &SetOptionArgs) -> Result<(), IpcError> {
        let inner = lock(&self.core.inner);
        let live = match &inner.state {
            State::Ready(live) | State::Working(live) => live.clone(),
            State::Starting(_) => {
                return Err(IpcError::new(ErrorKind::Conflict).detail("starting"));
            }
            State::Idle | State::Error(_) | State::NeedsLogin(_) => {
                return Err(IpcError::new(ErrorKind::Unavailable).detail("notRunning"));
            }
        };
        let value = options::value_for(&inner.options, &args.id, &args.value)?;
        let generation = inner.generation;
        drop(inner);
        let core = Arc::clone(&self.core);
        let (id, picked) = (args.id.clone(), args.value.clone());
        std::thread::Builder::new()
            .name("acp-option".into())
            .spawn(move || core.set_option(generation, &live, &id, value, &picked))
            .map(|_| ())
            .map_err(|error| IpcError::io("acp", &error))
    }
}

/// A project path of a file (not the folder itself).
fn project_file(path: &str) -> Result<(), IpcError> {
    if VaultPath::parse(path)?.is_root() {
        return Err(IpcError::new(ErrorKind::InvalidPath).subject(path));
    }
    Ok(())
}

/// Refuses a prompt that cannot be sent, before anything starts.
fn check(args: &PromptArgs) -> Result<(), IpcError> {
    if args.text.trim().is_empty() && args.selection.is_none() {
        return Err(IpcError::new(ErrorKind::InvalidData).detail("emptyPrompt"));
    }
    if let Some(selection) = &args.selection {
        project_file(&selection.path)?;
        if selection.from_line == 0 || selection.to_line < selection.from_line {
            return Err(IpcError::new(ErrorKind::InvalidData)
                .subject(selection.path.clone())
                .detail("lineRange"));
        }
        if selection.text.len() > MAX_PASSAGE_BYTES {
            return Err(IpcError::new(ErrorKind::TooLarge)
                .subject(selection.path.clone())
                .detail("passageTooLarge"));
        }
    }
    if let Some(document) = &args.document {
        project_file(document)?;
    }
    Ok(())
}

/// Mime type of a passage of the file `path`: Markdown for a Markdown file, else plain text (a JSON | YAML | code
/// passage read as Markdown would be misread).
fn passage_mime(path: &str) -> &'static str {
    let markdown = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"));
    if markdown {
        "text/markdown"
    } else {
        "text/plain"
    }
}

/// `file://` URI of a file of the copy.
fn file_uri(abs: &Path, path: &str) -> Result<url::Url, IpcError> {
    url::Url::from_file_path(abs).map_err(|()| IpcError::new(ErrorKind::InvalidPath).subject(path))
}

/// The passage as plain text: where it is from, then its lines in a fence longer than any run of backticks inside.
fn passage_text(path: &str, from_line: u32, to_line: u32, text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest.max(2) + 1);
    let body = text.strip_suffix('\n').unwrap_or(text);
    format!("Passage from {path}, lines {from_line}-{to_line}:\n{fence}\n{body}\n{fence}")
}

/// The prompt as ACP content: the text; the attached passage named by its file in the copy and its line range
/// (`#L<from>:<to>`): an embedded text resource for an agent that takes them (`embedded`), else the passage as text
/// plus a link to its lines (content every agent takes); the open document as a link to its file in the copy.
///
/// # Errors
/// A path that is no file of the project.
pub fn blocks(
    args: &PromptArgs,
    copy: &WorkCopy,
    embedded: bool,
) -> Result<Vec<ContentBlock>, IpcError> {
    let mut out = vec![ContentBlock::Text(TextContent::new(args.text.clone()))];
    if let Some(PromptSelection {
        path,
        from_line,
        to_line,
        text,
    }) = &args.selection
    {
        let mut uri = file_uri(&copy.copy_path(path)?, path)?;
        uri.set_fragment(Some(&format!("L{from_line}:{to_line}")));
        if embedded {
            out.push(ContentBlock::Resource(EmbeddedResource::new(
                EmbeddedResourceResource::TextResourceContents(
                    TextResourceContents::new(text.clone(), uri.to_string())
                        .mime_type(passage_mime(path)),
                ),
            )));
        } else {
            out.push(ContentBlock::Text(TextContent::new(passage_text(
                path, *from_line, *to_line, text,
            ))));
            out.push(ContentBlock::ResourceLink(ResourceLink::new(
                path.clone(),
                uri.to_string(),
            )));
        }
    }
    if let Some(path) = &args.document {
        let uri = file_uri(&copy.copy_path(path)?, path)?;
        out.push(ContentBlock::ResourceLink(ResourceLink::new(
            path.clone(),
            uri.to_string(),
        )));
    }
    Ok(out)
}

/// At most [`WORDS_CAP`] bytes of the agent's own words, from their start (`None`: nothing said).
fn said(message: &str) -> Option<String> {
    let message = message.trim();
    if message.is_empty() {
        return None;
    }
    Some(start_of(message, WORDS_CAP).to_owned())
}

/// `error` with the newest lines of the agent's stderr (`settle`: waits that long for the stream to end).
fn with_stderr(mut error: AcpError, connection: &Connection, settle: Duration) -> AcpError {
    let tail = connection.stderr_tail(WORDS_CAP, settle);
    error.stderr = (!tail.trim().is_empty()).then_some(tail);
    error
}

/// The agent ended by itself, with its last stderr lines.
fn exited(connection: &Connection) -> AcpError {
    with_stderr(AcpError::new(AcpErrorCode::Exited), connection, TAIL_SETTLE)
}

/// A call of a start | sign-in that got no answer.
fn call_failed(connection: &Connection, step: Step, error: CallError) -> AcpError {
    match error {
        CallError::Closed => exited(connection),
        CallError::TimedOut => with_stderr(
            AcpError::new(AcpErrorCode::TimedOut),
            connection,
            Duration::ZERO,
        ),
        CallError::Refused { message, .. } => AcpError {
            message: said(&message),
            ..AcpError::new(step.code())
        },
        CallError::Unreadable => {
            with_stderr(AcpError::new(step.code()), connection, Duration::ZERO)
        }
    }
}

/// A work copy refusal.
fn copy_error(error: &IpcError) -> AcpError {
    AcpError::new(match error.kind {
        ErrorKind::TooLarge => AcpErrorCode::TooLarge,
        ErrorKind::Unavailable => AcpErrorCode::ProjectsNotReady,
        _ => AcpErrorCode::CopyFailed,
    })
}

/// The stop reason of a `session/prompt` answer (`None`: none given); one this client does not know ends the turn as
/// `other`, never as an error (read on its own: a typed answer would refuse a stop reason of a later release).
fn stop_reason(answer: &Value) -> Option<AcpStopReason> {
    let named = answer.get("stopReason").filter(|v| v.is_string())?;
    Some(match serde_json::from_value::<StopReason>(named.clone()) {
        Ok(StopReason::EndTurn) => AcpStopReason::EndTurn,
        Ok(StopReason::MaxTokens) => AcpStopReason::MaxTokens,
        Ok(StopReason::MaxTurnRequests) => AcpStopReason::MaxTurnRequests,
        Ok(StopReason::Refusal) => AcpStopReason::Refusal,
        Ok(StopReason::Cancelled) => AcpStopReason::Cancelled,
        Ok(_) | Err(_) => AcpStopReason::Other,
    })
}

impl Core {
    fn emit(&self, update: AcpUpdate) {
        if let Ok(value) = serde_json::to_value(update) {
            self.host.events.emit("update", value);
        }
    }

    fn emit_all(&self, updates: impl IntoIterator<Item = AcpUpdate>) {
        for update in updates {
            self.emit(update);
        }
    }

    /// Runs `f` while `generation` is still the current one.
    fn with_current<R>(&self, generation: u64, f: impl FnOnce(&mut Inner) -> R) -> Option<R> {
        let mut inner = lock(&self.inner);
        (inner.generation == generation).then(|| f(&mut inner))
    }

    fn is_current(&self, generation: u64) -> bool {
        lock(&self.inner).generation == generation
    }

    /// Answers each dropped question `cancelled` (every request of the agent gets one answer) and closes its card.
    fn close_asks(&self, connection: &Connection, asks: Vec<(String, Ask)>) {
        for (request_id, ask) in asks {
            let _ = connection.reply(
                ask.id,
                Ok(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Cancelled,
                )),
            );
            self.emit(AcpUpdate::PermissionDone { request_id });
        }
    }

    /// After [`Inner::end_agent`]: its questions closed, `ended` told (a turn that will not go on), `Idle` told, the
    /// agent stopped aside.
    fn leave(&self, agent: Option<Arc<Agent>>, asks: Vec<(String, Ask)>, ended: Option<AcpUpdate>) {
        if let Some(agent) = &agent {
            self.close_asks(&agent.connection, asks);
        }
        if let Some(update) = ended {
            self.emit(update);
        }
        self.emit(State::Idle.update());
        if let Some(agent) = agent {
            stop_aside(&agent);
        }
    }

    /// Keeps what agents may not see out of a prompt: a passage of a file agents may not see is refused (its text
    /// would reach the agent), such an open document | one the copy leaves out is not named.
    fn screen(&self, mut args: PromptArgs) -> Result<PromptArgs, IpcError> {
        let project = &self.host.project;
        if let Some(selection) = &args.selection {
            if project.hidden(&selection.path)? {
                return Err(IpcError::new(ErrorKind::Denied)
                    .subject(selection.path.clone())
                    .detail("hiddenFromAgents"));
            }
        }
        if let Some(document) = &args.document {
            if kept_out(document) || project.hidden(document)? {
                args.document = None;
            }
        }
        Ok(args)
    }

    /// The run of `generation` failed: state `Error` (with `error`), its agent stopped.
    fn fail(&self, generation: u64, error: AcpError) {
        let taken = self.with_current(generation, |inner| {
            if matches!(inner.state, State::Idle | State::Error(_)) {
                return None;
            }
            let agent = inner.state.agent();
            inner.state = State::Error(error);
            // the window drops its settings with `error`
            inner.drop_options();
            Some((agent, inner.take_asks(), inner.state.update()))
        });
        let Some(Some((agent, asks, update))) = taken else {
            return;
        };
        if let Some(agent) = &agent {
            self.close_asks(&agent.connection, asks);
        }
        self.emit(update);
        if let Some(agent) = agent {
            agent.stop();
        }
    }

    /// The start run: copy, program, session; then the first prompt | the conversation continued. `again`: started
    /// after a sign-in outside the app (an agent still asking for one says so).
    fn start(self: &Arc<Self>, generation: u64, then: &Then, again: bool) {
        match self.open(generation, then) {
            Ok(Some(Opened::Live(live))) => self.go_on(generation, &live, then),
            Ok(Some(Opened::Login(agent, methods, embedded))) => self.need_login(
                generation,
                Login {
                    agent,
                    session: None,
                    then: then.clone(),
                    methods,
                    embedded,
                    error: again.then(|| AcpError::new(AcpErrorCode::StillNeedsLogin)),
                },
            ),
            // replaced while starting: whoever replaced it stopped the agent
            Ok(None) => {}
            Err(error) => self.fail(generation, error),
        }
    }

    /// The session of `generation` is open: the prompt | the conversation continued.
    fn go_on(&self, generation: u64, live: &Live, then: &Then) {
        match then {
            Then::Prompt(args) => self.work(generation, live, args),
            Then::Continue(_) => self.restored(generation, live),
        }
    }

    /// The conversation of `generation` was continued: `restored` told (whether it was replayed, whether changes the
    /// person had not accepted are gone with an earlier copy), then `Ready`; no prompt is sent.
    fn restored(&self, generation: u64, live: &Live) {
        let agent = &live.agent;
        let changes_lost = agent.fresh && self.saved().is_some_and(|saved| saved.had_changes);
        let moved = self.with_current(generation, |inner| {
            inner.state = State::Ready(live.clone());
            inner.state.update()
        });
        let Some(update) = moved else {
            return agent.stop();
        };
        self.continued(agent, &live.session);
        self.emit(AcpUpdate::Restored {
            replayed: agent.way() == Some(ContinueWay::Load),
            changes_lost,
        });
        self.emit(update);
    }

    /// The started agent of `generation` takes the prompt: `Working`, then the turn.
    fn work(&self, generation: u64, live: &Live, args: &PromptArgs) {
        let moved = self.with_current(generation, |inner| {
            inner.state = State::Working(live.clone());
            inner.state.update()
        });
        match moved {
            Some(update) => {
                self.emit(update);
                self.turn(generation, live, args);
            }
            None => live.agent.stop(),
        }
    }

    /// The agent of `generation` wants the person to sign in: `NeedsLogin` with its methods.
    fn need_login(&self, generation: u64, login: Login) {
        let agent = Arc::clone(&login.agent);
        let taken = self.with_current(generation, |inner| {
            let asks = inner.take_asks();
            inner.state = State::NeedsLogin(login);
            (asks, inner.state.update())
        });
        match taken {
            Some((asks, update)) => {
                self.close_asks(&agent.connection, asks);
                self.emit(update);
            }
            None => agent.stop(),
        }
    }

    /// The sign-in run with an agent method: the agent signs in, then what waited goes on (in a session opened now
    /// when opening it was the refused step).
    fn sign_in(self: &Arc<Self>, generation: u64, login: Login, method: String) {
        let Login {
            agent,
            session,
            then,
            methods,
            embedded,
            ..
        } = login;
        let again = |error: AcpError| Login {
            agent: Arc::clone(&agent),
            session: session.clone(),
            then: then.clone(),
            methods: Arc::clone(&methods),
            embedded,
            error: Some(error),
        };
        let signed: Result<Value, CallError> = agent.connection.call(
            AGENT_METHOD_NAMES.authenticate,
            AuthenticateRequest::new(method),
            Some(LOGIN_WAIT),
        );
        match signed {
            Ok(_) => {}
            Err(CallError::Closed) => return self.fail(generation, exited(&agent.connection)),
            Err(error) => {
                let why = call_failed(&agent.connection, Step::Login, error);
                return self.need_login(generation, again(why));
            }
        }
        let session = match &session {
            Some(session) => session.clone(),
            None => match self.open_in(generation, &agent, &then) {
                Ok(session) => session,
                Err(Opening::Login) => {
                    let why = AcpError::new(AcpErrorCode::StillNeedsLogin);
                    return self.need_login(generation, again(why));
                }
                Err(Opening::Failed(error)) => return self.fail(generation, error),
            },
        };
        let live = Live {
            agent: Arc::clone(&agent),
            session,
            methods: Arc::clone(&methods),
            embedded,
        };
        self.go_on(generation, &live, &then);
    }

    /// Opens the session `then` needs in `agent`: a new one (kept as the project's last conversation) | the kept one
    /// continued; then the person's picks of its settings apply, and the window is told them.
    fn open_in(&self, generation: u64, agent: &Agent, then: &Then) -> Result<SessionId, Opening> {
        let (session, options) = match then {
            Then::Prompt(_) => {
                let (session, options) = open_session(&agent.connection, &agent.dir)?;
                self.remember(agent, &session);
                (session, options)
            }
            Then::Continue(saved) => self.reopen(generation, agent, saved)?,
        };
        let options = self.apply_picks(generation, agent, &session, options);
        self.keep_options(generation, Some(options), false);
        Ok(session)
    }

    /// `session/load` (the agent replays the conversation as news before it answers) | `session/resume`, as the
    /// agent offers, in the copy's folder the conversation was opened in; then the asking mode again. A refusal: the
    /// agent no longer has it, so it is forgotten.
    fn reopen(
        &self,
        generation: u64,
        agent: &Agent,
        saved: &Saved,
    ) -> Result<(SessionId, Vec<SessionConfigOption>), Opening> {
        let session = SessionId::new(saved.session_id.clone());
        let connection = &agent.connection;
        let opened: Result<SessionSetup, CallError> = match agent.way() {
            Some(ContinueWay::Load) => {
                self.with_current(generation, |inner| inner.telling = Telling::Replayed);
                let loaded = connection.call::<_, LoadSessionResponse>(
                    AGENT_METHOD_NAMES.session_load,
                    LoadSessionRequest::new(session.clone(), &agent.dir)
                        .meta(never_skip_questions()),
                    Some(SETUP_WAIT),
                );
                self.with_current(generation, |inner| inner.telling = Telling::Live);
                loaded.map(|done| (done.modes, done.config_options))
            }
            Some(ContinueWay::Resume) => connection
                .call::<_, ResumeSessionResponse>(
                    AGENT_METHOD_NAMES.session_resume,
                    ResumeSessionRequest::new(session.clone(), &agent.dir)
                        .meta(never_skip_questions()),
                    Some(SETUP_WAIT),
                )
                .map(|done| (done.modes, done.config_options)),
            None => {
                self.forget(&session);
                return Err(Opening::Failed(AcpError::new(AcpErrorCode::SessionGone)));
            }
        };
        match opened {
            Ok((modes, options)) => {
                let options = set_ask_mode(connection, &session, modes.as_ref(), options)?;
                Ok((session, options))
            }
            Err(error) if error.needs_login() => Err(Opening::Login),
            Err(CallError::Refused { message, .. }) => {
                self.forget(&session);
                Err(Opening::Failed(AcpError {
                    message: said(&message),
                    ..AcpError::new(AcpErrorCode::SessionGone)
                }))
            }
            Err(error) => Err(Opening::Failed(call_failed(
                connection,
                Step::Session,
                error,
            ))),
        }
    }

    /// The copy's folder, made when absent (`true`: made now), cleared of agent settings (`None`: replaced meanwhile).
    fn ensure_copy(&self, generation: u64) -> Result<Option<(PathBuf, bool)>, AcpError> {
        // held while the copy is made: a plugin stop waits for it, then finds it (or nothing) to remove
        let mut copy = lock(&self.host.copy);
        if let Some(existing) = copy.as_ref() {
            // settings an agent wrote would load at this start
            existing
                .purge()
                .map_err(|_| AcpError::new(AcpErrorCode::CopyFailed))?;
            return Ok(Some((existing.dir().to_path_buf(), false)));
        }
        let place = self
            .host
            .place
            .as_ref()
            .ok_or_else(|| AcpError::new(AcpErrorCode::NoProfile))?;
        // the real spelling (links, 8.3 names): the agent's cwd, and so every place it names, matches the copy's
        // canonical root without a second lookup (which only runs on the same volume)
        let dir = copy_dir(place, self.host.project.root());
        let dir = paths::canonicalize_lenient(&dir).unwrap_or(dir);
        let built = WorkCopy::build(self.host.project.clone(), dir).map_err(|e| copy_error(&e))?;
        if !self.is_current(generation) {
            let _ = built.remove();
            return Ok(None);
        }
        self.count_skipped(&built);
        let dir = built.dir().to_path_buf();
        *copy = Some(built);
        Ok(Some((dir, true)))
    }

    /// The agent's start folder, empty: anything an earlier run left there would be read as a runner's settings.
    fn start_folder(&self) -> Result<PathBuf, AcpError> {
        let run = self
            .host
            .run
            .as_ref()
            .ok_or_else(|| AcpError::new(AcpErrorCode::NoProfile))?;
        empty_folder(run).map_err(|_| AcpError::new(AcpErrorCode::StartFailed))?;
        Ok(run.clone())
    }

    /// Reads the person's choice, makes the copy, starts the program, opens the session `then` needs (`None`:
    /// replaced meanwhile).
    fn open(self: &Arc<Self>, generation: u64, then: &Then) -> Result<Option<Opened>, AcpError> {
        let chosen = self
            .host
            .agents
            .chosen()
            .ok_or_else(|| AcpError::new(AcpErrorCode::NoAgent))?;
        // another agent was chosen since: it never continues a conversation of the one before
        if matches!(then, Then::Continue(saved) if !saved.started_by(&chosen.choice)) {
            return Err(AcpError::new(AcpErrorCode::SessionGone));
        }
        // nothing to start it with: the status's `needs` says what
        let plan = chosen
            .resolved
            .plan
            .clone()
            .ok_or_else(|| AcpError::new(AcpErrorCode::NotFound))?;
        let Some((dir, fresh)) = self.ensure_copy(generation)? else {
            return Ok(None);
        };
        let copy = Root::open(&dir)
            .map(Vault::new)
            .map_err(|_| AcpError::new(AcpErrorCode::CopyFailed))?;
        let run = self.start_folder()?;
        let AgentPipes {
            to_agent,
            from_agent,
            stderr,
            stop,
        } = (self.host.launch)(&plan, &run)
            .map_err(|_| AcpError::new(AcpErrorCode::StartFailed))?;
        let inbox = Arc::new(Listener {
            core: Arc::clone(self),
            generation,
        });
        let Ok(connection) = Connection::start(to_agent, from_agent, stderr, inbox) else {
            stop();
            return Err(AcpError::new(AcpErrorCode::StartFailed));
        };
        let agent = Arc::new(Agent {
            connection,
            stop: Mutex::new(Some(stop)),
            dir,
            copy,
            choice: chosen.choice,
            fresh,
            way: OnceLock::new(),
        });
        let kept = self.with_current(generation, |inner| {
            inner.state = State::Starting(Some(Arc::clone(&agent)));
        });
        if kept.is_none() {
            agent.stop();
            return Ok(None);
        }
        let (methods, embedded, way) = initialize(&agent.connection, &plan)?;
        let _ = agent.way.set(way);
        let methods: Arc<[LoginMethod]> = methods.into();
        match self.open_in(generation, &agent, then) {
            Ok(session) => Ok(Some(Opened::Live(Live {
                agent,
                session,
                methods,
                embedded,
            }))),
            Err(Opening::Login) => Ok(Some(Opened::Login(agent, methods, embedded))),
            Err(Opening::Failed(error)) => Err(error),
        }
    }

    /// One prompt of `live`, through to its end.
    fn turn(&self, generation: u64, live: &Live, args: &PromptArgs) {
        let fresh = self.with_current(generation, |inner| {
            inner.tools.clear();
            inner.thinking = false;
        });
        if fresh.is_none() {
            return;
        }
        let blocks = match self.prepare(args, live.embedded) {
            Ok(blocks) => blocks,
            Err(error) => return self.end_turn(generation, AcpStopReason::Error, Some(error)),
        };
        let connection = &live.agent.connection;
        let pending = {
            let mut inner = lock(&self.inner);
            if inner.generation != generation {
                return;
            }
            if inner.stopping {
                drop(inner);
                return self.end_turn(generation, AcpStopReason::Cancelled, None);
            }
            // queued under the lock: a Stop from now on finds it sent
            let pending = connection.request(
                AGENT_METHOD_NAMES.session_prompt,
                PromptRequest::new(live.session.clone(), blocks),
            );
            inner.sent = pending.is_ok();
            pending
        };
        let asked: Result<Value, CallError> = pending.and_then(|pending| pending.wait(None));
        let stopping = || lock(&self.inner).stopping;
        match asked {
            Ok(answer) => match stop_reason(&answer) {
                Some(reason) => self.end_turn(generation, reason, None),
                None => self.end_turn(
                    generation,
                    AcpStopReason::Error,
                    Some(AcpError::new(AcpErrorCode::Protocol)),
                ),
            },
            Err(CallError::Closed) => {
                if self.is_current(generation) {
                    self.fail(generation, exited(connection));
                }
            }
            Err(error) if error.needs_login() => self.need_login(
                generation,
                Login {
                    agent: Arc::clone(&live.agent),
                    session: Some(live.session.clone()),
                    then: Then::Prompt(args.clone()),
                    methods: Arc::clone(&live.methods),
                    embedded: live.embedded,
                    error: None,
                },
            ),
            // an agent may answer a cancelled prompt with an error instead of its stop reason
            Err(_) if stopping() => self.end_turn(generation, AcpStopReason::Cancelled, None),
            Err(CallError::Refused { message, .. }) => self.end_turn(
                generation,
                AcpStopReason::Error,
                Some(AcpError {
                    message: said(&message),
                    ..AcpError::new(AcpErrorCode::Refused)
                }),
            ),
            Err(CallError::Unreadable | CallError::TimedOut) => self.end_turn(
                generation,
                AcpStopReason::Error,
                Some(AcpError::new(AcpErrorCode::Protocol)),
            ),
        }
    }

    /// The copy cleared of agent settings and brought up to date with the person's files, then the prompt's content.
    fn prepare(&self, args: &PromptArgs, embedded: bool) -> Result<Vec<ContentBlock>, AcpError> {
        let mut guard = lock(&self.host.copy);
        let copy = guard
            .as_mut()
            .ok_or_else(|| AcpError::new(AcpErrorCode::CopyFailed))?;
        copy.purge()
            .map_err(|_| AcpError::new(AcpErrorCode::CopyFailed))?;
        copy.sync().map_err(|e| copy_error(&e))?;
        self.count_skipped(copy);
        blocks(args, copy, embedded).map_err(|_| AcpError::new(AcpErrorCode::SendFailed))
    }

    fn count_skipped(&self, copy: &WorkCopy) {
        let skipped = u32::try_from(copy.skipped()).unwrap_or(u32::MAX);
        self.skipped.store(skipped, Ordering::Relaxed);
    }

    /// Files whose copy differs now: the change list computed once for the turn, kept for the windows.
    fn changed(&self) -> u32 {
        lock(&self.host.copy)
            .as_mut()
            .and_then(|copy| copy.refresh().ok())
            .map_or(0, |list| list.count())
    }

    /// Files left for the person in the copy as last listed (`0`: no copy | it cannot be read).
    fn left_in_copy(&self) -> u32 {
        lock(&self.host.copy)
            .as_mut()
            .and_then(|copy| copy.changes().ok())
            .map_or(0, |list| list.count())
    }

    /// The turn of `generation` ended: `Ready` again (questions still open answered `cancelled`), `turnEnd` told.
    fn end_turn(&self, generation: u64, stop_reason: AcpStopReason, error: Option<AcpError>) {
        let changed = self.changed();
        let taken = self.with_current(generation, |inner| {
            let State::Working(live) = &inner.state else {
                return None;
            };
            let connection = Arc::clone(&live.agent.connection);
            let session = live.session.clone();
            inner.state = State::Ready(live.clone());
            Some((connection, session, inner.take_asks()))
        });
        let Some(Some((connection, session, asks))) = taken else {
            return;
        };
        self.close_asks(&connection, asks);
        self.turn_kept(&session, changed);
        self.emit(AcpUpdate::TurnEnd {
            stop_reason,
            changed,
        });
        self.emit(AcpUpdate::Status {
            state: AcpState::Ready,
            error,
            methods: None,
        });
    }

    /// Routes a permission question of `generation`: plainly work on the copy's files ([`plain_work`], on what the
    /// agent told of the tool call so far) -> allowed once; else a card, told before the lock is let go.
    fn route(&self, generation: u64, id: RequestId, asked: Question) -> Route {
        let mut inner = lock(&self.inner);
        if inner.generation != generation || inner.stopping {
            return Route::Drop;
        }
        let State::Working(live) = &inner.state else {
            return Route::Drop;
        };
        let copy = &live.agent.copy;
        let mut tool = inner
            .tools
            .get(&*asked.tool_call.tool_call_id.0)
            .cloned()
            .unwrap_or_default();
        tool.apply(&asked.tool_call.fields);
        if plain_work(&tool, copy) {
            if let Some(option) = allow_once(&asked.options) {
                return Route::Allow(option);
            }
        }
        let request_id = question_key(generation, &id);
        let update = AcpUpdate::Permission {
            request_id: request_id.clone(),
            kind: tool.kind(),
            reach: reach(&tool, copy),
            detail: detail(&tool, copy),
            diff: diff(&tool, copy),
            summary: summary(&tool),
            title: tool.title,
            options: asked
                .options
                .iter()
                .map(|o| PermissionChoice {
                    id: o.id.0.to_string(),
                    name: o.name.clone(),
                    kind: o.kind,
                })
                .collect(),
        };
        inner.asks.insert(
            request_id,
            Ask {
                id,
                options: asked.options.into_iter().map(|o| o.id).collect(),
            },
        );
        // told under the lock: a Stop | a turn's end takes this question only after its card went out
        self.emit(update);
        Route::Asked
    }
}

/// Makes `dir` an empty folder: what it holds is removed (a link in its place is removed itself, never followed). An
/// empty one stays as it is: an agent still stopping may have it as its working folder, which Windows will not remove.
fn empty_folder(dir: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => {
            if std::fs::read_dir(dir)?.next().is_none() {
                return Ok(());
            }
            std::fs::remove_dir_all(dir)?;
        }
        // a file | a link (Windows: a folder link goes with `remove_dir`)
        Ok(_) => std::fs::remove_file(dir).or_else(|_| std::fs::remove_dir(dir))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    std::fs::create_dir_all(dir)
}

/// `initialize`: answers the agent's sign-in methods (a terminal method's line: `plan`'s variables + command, then the
/// method's own), whether it takes embedded resources in prompts and how it can continue a conversation.
fn initialize(
    connection: &Connection,
    plan: &Plan,
) -> Result<(Vec<LoginMethod>, bool, Option<ContinueWay>), AcpError> {
    // terminal sign-in: the person runs the shown line in a terminal; the app itself never opens one
    let caps = ClientCapabilities::new().auth(AuthCapabilities::new().terminal(true));
    let hello = InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(caps)
        .client_info(Implementation::new("PerMark", env!("CARGO_PKG_VERSION")));
    let raw: Value = connection
        .call(AGENT_METHOD_NAMES.initialize, hello, Some(SETUP_WAIT))
        .map_err(|e| call_failed(connection, Step::Init, e))?;
    let init: InitializeResponse = serde_json::from_value(raw.clone())
        .map_err(|_| call_failed(connection, Step::Init, CallError::Unreadable))?;
    if init.protocol_version != ProtocolVersion::V1 {
        return Err(AcpError::new(AcpErrorCode::ProtocolVersion));
    }
    let embedded = init.agent_capabilities.prompt_capabilities.embedded_context;
    Ok((
        login_methods(raw.get("authMethods"), plan),
        embedded,
        resume::way_of(&init.agent_capabilities),
    ))
}

/// `session/new` -> the asking mode when offered and not on ([`ask_mode`]). Answers the session with its config
/// options as they are then.
fn open_session(
    connection: &Connection,
    dir: &Path,
) -> Result<(SessionId, Vec<SessionConfigOption>), Opening> {
    let new: NewSessionResponse = match connection.call(
        AGENT_METHOD_NAMES.session_new,
        NewSessionRequest::new(dir).meta(never_skip_questions()),
        Some(SETUP_WAIT),
    ) {
        Ok(new) => new,
        Err(error) if error.needs_login() => return Err(Opening::Login),
        Err(error) => {
            return Err(Opening::Failed(call_failed(
                connection,
                Step::Session,
                error,
            )))
        }
    };
    let options = set_ask_mode(
        connection,
        &new.session_id,
        new.modes.as_ref(),
        new.config_options,
    )?;
    Ok((new.session_id, options))
}

/// The modes and config options a session was opened | continued with.
type SessionSetup = (Option<SessionModeState>, Option<Vec<SessionConfigOption>>);

/// Sets `session` to the asking mode when offered and not on ([`ask_mode`]): every open | continue, since a
/// conversation continued may come back in whatever mode its agent keeps. Answers the config options afterwards (a
/// config option answer carries them all; else `options` as they were).
fn set_ask_mode(
    connection: &Connection,
    session: &SessionId,
    modes: Option<&SessionModeState>,
    options: Option<Vec<SessionConfigOption>>,
) -> Result<Vec<SessionConfigOption>, Opening> {
    let options = options.unwrap_or_default();
    let set: Result<Value, CallError> = match ask_mode(modes, &options) {
        Some(AskMode::Mode) => connection.call(
            AGENT_METHOD_NAMES.session_set_mode,
            SetSessionModeRequest::new(session.clone(), ASK_MODE),
            Some(SETUP_WAIT),
        ),
        Some(AskMode::Option(config)) => connection.call(
            AGENT_METHOD_NAMES.session_set_config_option,
            SetSessionConfigOptionRequest::new(session.clone(), config, ASK_MODE),
            Some(SETUP_WAIT),
        ),
        None => return Ok(options),
    };
    let answer = set.map_err(|e| Opening::Failed(call_failed(connection, Step::Session, e)))?;
    Ok(options::answered(answer).unwrap_or(options))
}

/// How a session is set to the asking mode.
enum AskMode {
    /// `session/set_mode`.
    Mode,
    /// `session/set_config_option` on this mode option.
    Option(SessionConfigId),
}

/// How to set a session with `modes` | `options` to the asking mode (`None`: not offered | on already): the session
/// modes when they offer it, else a config option of category `mode` that offers it (agents move from modes to
/// config options).
fn ask_mode(modes: Option<&SessionModeState>, options: &[SessionConfigOption]) -> Option<AskMode> {
    if let Some(modes) = modes {
        if modes.available_modes.iter().any(|m| &*m.id.0 == ASK_MODE) {
            return (&*modes.current_mode_id.0 != ASK_MODE).then_some(AskMode::Mode);
        }
    }
    options.iter().find_map(|option| {
        let SessionConfigKind::Select(select) = &option.kind else {
            return None;
        };
        if option.category != Some(SessionConfigOptionCategory::Mode) {
            return None;
        }
        let offered = options::values(&select.options)
            .into_iter()
            .any(|v| &*v.value.0 == ASK_MODE);
        (offered && &*select.current_value.0 != ASK_MODE)
            .then(|| AskMode::Option(option.id.clone()))
    })
}

/// `_meta` of `session/new` | `load` | `resume`: Claude's adapter offers (and a settings file | a subagent header may pick) a mode that
/// runs every tool without asking unless the client opts out; other agents ignore the key.
fn never_skip_questions() -> serde_json::Map<String, Value> {
    let mut meta = serde_json::Map::new();
    meta.insert(
        "claudeCode".to_owned(),
        serde_json::json!({ "options": { "allowDangerouslySkipPermissions": false } }),
    );
    meta
}

/// The reader thread's view of the session, for one agent.
struct Listener {
    core: Arc<Core>,
    generation: u64,
}

impl Inner {
    /// Updates for one piece of the agent's news; `copy`: the places the copy is named by.
    fn news(&mut self, update: SessionUpdate, copy: &[PathBuf]) -> Vec<AcpUpdate> {
        match update {
            // a replayed thought is no thinking now
            SessionUpdate::AgentThoughtChunk(_) if self.telling == Telling::Replayed => Vec::new(),
            SessionUpdate::AgentThoughtChunk(_) => {
                if std::mem::replace(&mut self.thinking, true) {
                    Vec::new()
                } else {
                    vec![AcpUpdate::Thinking {}]
                }
            }
            SessionUpdate::AgentMessageChunk(chunk) => {
                self.thinking = false;
                match chunk.content {
                    ContentBlock::Text(text) => vec![AcpUpdate::Text { text: text.text }],
                    _ => Vec::new(),
                }
            }
            // the person's earlier messages, shown again: their words only (attachments come back as other blocks, or
            // flattened into the text by Claude's adapter)
            SessionUpdate::UserMessageChunk(chunk) if self.telling == Telling::Replayed => {
                match chunk.content {
                    ContentBlock::Text(text) => reply::replayed_words(&text.text, copy)
                        .map(|text| AcpUpdate::You { text })
                        .into_iter()
                        .collect(),
                    _ => Vec::new(),
                }
            }
            SessionUpdate::ToolCall(call) => {
                self.thinking = false;
                let id = call.tool_call_id.0.to_string();
                let line = ToolLine::of(call);
                let update = line.update(&id);
                self.tools.insert(id, line);
                vec![update]
            }
            SessionUpdate::ToolCallUpdate(call) => {
                self.thinking = false;
                let id = call.tool_call_id.0.to_string();
                let line = self.tools.entry(id.clone()).or_default();
                let before = line.update(&id);
                line.apply(&call.fields);
                let after = line.update(&id);
                // updates that only stream the tool's input change nothing shown
                if after == before {
                    Vec::new()
                } else {
                    vec![after]
                }
            }
            // modes, commands, usage, plans, titles (kept by the listener), config options (kept by the listener):
            // not shown here
            _ => Vec::new(),
        }
    }
}

impl Listener {
    /// The agent withdrew one of its questions: answered `-32800` (request cancelled), its card closed.
    fn withdrawn(&self, connection: &Connection, params: Option<Value>) {
        let Some(Ok(note)) = params.map(serde_json::from_value::<CancelRequestNotification>) else {
            return;
        };
        let key = question_key(self.generation, &note.request_id);
        let Some(Some(ask)) = self
            .core
            .with_current(self.generation, |inner| inner.asks.remove(&key))
        else {
            return;
        };
        let _ = connection.reply::<Value>(ask.id, Err(RpcError::request_cancelled()));
        self.core
            .emit(AcpUpdate::PermissionDone { request_id: key });
    }
}

impl Inbox for Listener {
    fn notification(&self, connection: &Connection, method: &str, params: Option<Value>) {
        if method == PROTOCOL_LEVEL_METHOD_NAMES.cancel_request {
            return self.withdrawn(connection, params);
        }
        // anything but session news (account status among it) is dropped unread
        if method != CLIENT_METHOD_NAMES.session_update {
            return;
        }
        // a kind of news this client does not know does not decode: skipped
        let Some(Ok(note)) = params.map(serde_json::from_value::<SessionNotification>) else {
            return;
        };
        if let SessionUpdate::ConfigOptionUpdate(update) = note.update {
            return self
                .core
                .keep_options(self.generation, Some(update.config_options), false);
        }
        let title = match &note.update {
            SessionUpdate::SessionInfoUpdate(info) => match &info.title {
                MaybeUndefined::Value(title) => Some(Some(title.clone())),
                MaybeUndefined::Null => Some(None),
                MaybeUndefined::Undefined => None,
            },
            _ => None,
        };
        let Some(updates) = self.core.with_current(self.generation, |inner| {
            inner.news(note.update, &self.core.copy_names)
        }) else {
            return;
        };
        if let Some(title) = title {
            self.core.retitle(&note.session_id, title.as_deref());
        }
        for update in updates {
            self.core.emit(update);
        }
    }

    fn request(&self, connection: &Connection, id: RequestId, method: &str, params: Option<Value>) {
        // no file system, no terminal, no forms offered: the agent works on the copy with its own tools
        if method != CLIENT_METHOD_NAMES.session_request_permission {
            let _ = connection.reply::<Value>(id, Err(RpcError::method_not_found()));
            return;
        }
        let Some(asked) = params.and_then(Question::read) else {
            let _ = connection.reply::<Value>(id, Err(RpcError::invalid_params()));
            return;
        };
        match self.core.route(self.generation, id.clone(), asked) {
            Route::Allow(option) => {
                let _ = connection.reply(
                    id,
                    Ok(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option)),
                    )),
                );
            }
            Route::Asked => {}
            Route::Drop => {
                let _ = connection.reply(
                    id,
                    Ok(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    )),
                );
            }
        }
    }

    fn closed(&self) {
        // during a start | a prompt, that run reports the end; here only an agent that left while waiting
        let waiting = self
            .core
            .with_current(self.generation, |inner| match &inner.state {
                State::Ready(live) => Some(Arc::clone(&live.agent)),
                State::NeedsLogin(login) => Some(Arc::clone(&login.agent)),
                _ => None,
            });
        if let Some(Some(agent)) = waiting {
            self.core.fail(self.generation, exited(&agent.connection));
        }
    }
}

#[cfg(test)]
mod tests;
