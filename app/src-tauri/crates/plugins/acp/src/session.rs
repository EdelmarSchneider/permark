//! The agent of one project instance, spoken to over the Agent Client Protocol (v1).
//!
//! - started at the first prompt, on a thread of its own (the RPC thread never waits on the agent): the person's
//!   choice read now ([`Agents`]: a new choice applies at the next start), work copy made when absent, the agent
//!   started in it, `initialize` (no file system, no terminal offered: the agent works on the copy with its own
//!   tools; terminal sign-in methods are welcome: the person runs them) -> `session/new` (folder = the copy, no MCP
//!   servers) -> `session/set_mode` `default` when offered (a person's own "auto" default would let the agent skip
//!   every question).
//! - `auth_required` at `session/new` | a prompt -> `NeedsLogin` with the agent's sign-in methods; `acp.login` has the
//!   agent sign in (`authenticate`, agent methods only) or just tries again, then the waiting prompt goes on.
//! - each prompt: the copy takes the person's latest changes first ([`WorkCopy::sync`]), then `session/prompt`; the
//!   turn ends with the agent's stop reason and the count of changed files.
//! - news (`session/update`) -> `acp:update` events: reply text, "thinking", tool lines. Anything else the agent
//!   tells (modes, commands, usage, account status) is dropped unread.
//! - permission questions: a tool that only reads | searches | thinks, or edits | deletes | moves files inside the
//!   copy, is allowed once by rule; anything else is a card the person answers.
//! - states: Idle -> Starting -> Working <-> Ready; Starting | Working -> `NeedsLogin` -> Starting (login); failed start
//!   | agent gone -> Error (the next prompt starts a fresh one); "new conversation" | a new choice -> Idle (the copy
//!   and its pending changes stay). Each run carries the generation it was started for and does nothing once a newer
//!   one replaced it.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol_schema::v1::{
    AuthCapabilities, AuthMethodTerminal, AuthenticateRequest, CancelNotification,
    ClientCapabilities, ContentBlock, EmbeddedResource, EmbeddedResourceResource,
    Error as RpcError, Implementation, InitializeRequest, InitializeResponse, NewSessionRequest,
    NewSessionResponse, PermissionOptionId, PermissionOptionKind, PromptRequest, PromptResponse,
    RequestId, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    ResourceLink, SelectedPermissionOutcome, SessionId, SessionNotification, SessionUpdate,
    SetSessionModeRequest, TextContent, TextResourceContents, ToolCallStatus, ToolCallUpdateFields,
    ToolKind, AGENT_METHOD_NAMES, CLIENT_METHOD_NAMES,
};
use agent_client_protocol_schema::ProtocolVersion;
use permark_core::paths::{Root, Vault, VaultPath};
use permark_core::plugins::PluginEvents;
use permark_core::{ErrorKind, IpcError};
use serde::Serialize;
use serde_json::Value;

use crate::agents::{Agents, Chosen, Launch};
use crate::client::{lock, AgentPipes, CallError, Connection, Inbox};
use crate::dto::{
    AcpState, AcpStatus, AcpUpdate, AgentEntry, AgentNeed, AnswerArgs, ChooseArgs, LoginArgs,
    LoginKind, LoginMethod, PermissionChoice, PromptArgs, PromptSelection,
};
use crate::work::{copy_dir, Project, WorkCopy};
use crate::SharedCopy;

/// Longest wait for each step of the start (the first `npx` run downloads the adapter).
const SETUP_WAIT: Duration = Duration::from_secs(180);
/// Longest wait for an agent's own sign-in (it may wait for the person in the browser).
const LOGIN_WAIT: Duration = Duration::from_secs(600);
/// Characters of the agent's stderr shown when it stopped by itself.
const TAIL_CHARS: usize = 500;
/// Wait for the agent's last stderr lines after its output ended.
const TAIL_SETTLE: Duration = Duration::from_millis(500);
/// The agent mode that asks before changes.
const ASK_MODE: &str = "default";
/// Mime type of an attached passage.
const PASSAGE_MIME: &str = "text/markdown";

/// What a session works with.
pub struct Host {
    /// The catalog + the person's choice (read at every start).
    pub agents: Agents,
    pub launch: Launch,
    pub project: Project,
    /// Folder the copy is made in (`None`: no profile, no copy can be made).
    pub place: Option<PathBuf>,
    pub copy: SharedCopy,
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
}

#[derive(Default)]
struct Inner {
    state: State,
    /// Bumped by every fresh start | stop: runs and listeners of an older one do nothing.
    generation: u64,
    /// Permission questions waiting for the person, by the id the window got.
    asks: HashMap<String, Ask>,
    /// Tool lines of the turn, by tool call id.
    tools: HashMap<String, ToolLine>,
    /// "Thinking" was told since the last other news.
    thinking: bool,
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    /// The start runs; the agent once its program started.
    Starting(Option<Arc<Agent>>),
    Ready(Live),
    Working(Live),
    Error(String),
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

    fn error(&self) -> Option<String> {
        match self {
            Self::Error(message) => Some(message.clone()),
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
}

/// An agent waiting for the person to sign in, with the prompt that goes on afterwards.
struct Login {
    agent: Arc<Agent>,
    /// The session the prompt was refused in (`None`: `session/new` was refused).
    session: Option<SessionId>,
    args: PromptArgs,
    methods: Arc<[LoginMethod]>,
    /// Why the last sign-in did not work (plain English).
    error: Option<String>,
}

/// One running agent program.
struct Agent {
    connection: Arc<Connection>,
    stop: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// The copy's folder, as sessions are opened in it.
    dir: PathBuf,
    /// The copy as a folder of its own: the boundary of "work on the copy's files".
    copy: Vault,
}

impl Agent {
    /// Closes its stdin, then ends it (grace, then kill); every call waiting on it answers `Closed`.
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

/// What the window shows of one tool call.
#[derive(Default, Clone, PartialEq, Eq)]
struct ToolLine {
    title: String,
    kind: ToolKind,
    status: ToolCallStatus,
}

impl ToolLine {
    fn update(&self, id: &str) -> AcpUpdate {
        AcpUpdate::Tool {
            id: id.to_owned(),
            title: self.title.clone(),
            kind: name_of(&self.kind),
            status: name_of(&self.status),
        }
    }

    fn apply(&mut self, fields: &ToolCallUpdateFields) {
        if let Some(title) = &fields.title {
            self.title.clone_from(title);
        }
        if let Some(kind) = fields.kind {
            self.kind = kind;
        }
        if let Some(status) = fields.status {
            self.status = status;
        }
    }
}

/// The wire name of a protocol enum value (`allow_once`, `end_turn`, ...).
fn name_of(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// What a run started by a prompt does.
enum Run {
    Start,
    Turn(Live),
}

/// What a start opened.
enum Opened {
    Live(Live),
    /// The agent wants the person to sign in before a session.
    Login(Arc<Agent>, Arc<[LoginMethod]>),
}

/// Why no session was opened.
enum Opening {
    Login,
    Failed(String),
}

/// What a permission question gets.
enum Route {
    /// Allowed once by rule (this option).
    Allow(PermissionOptionId),
    /// A card for the person.
    Ask(AcpUpdate),
    /// Asked outside a turn of the current agent: answered `cancelled`.
    Drop,
}

impl Session {
    #[must_use]
    pub fn new(host: Host) -> Self {
        Self {
            core: Arc::new(Core {
                host,
                inner: Mutex::new(Inner::default()),
            }),
        }
    }

    #[must_use]
    pub fn status(&self) -> AcpStatus {
        // read before the lock: a file and `PATH` lookups
        let chosen = self.core.host.agents.chosen();
        let inner = lock(&self.core.inner);
        AcpStatus {
            state: inner.state.name(),
            agent: chosen
                .as_ref()
                .map(|c| c.resolved.shown.clone())
                .unwrap_or_default(),
            agent_name: chosen.as_ref().map(|c| c.name.clone()),
            chosen: chosen.is_some(),
            error: inner.state.error(),
            methods: inner.state.methods().unwrap_or_default(),
        }
    }

    /// The agents the person can choose, as each would start on this computer.
    #[must_use]
    pub fn agents(&self) -> Vec<AgentEntry> {
        self.core.host.agents.list()
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
    /// `Unavailable` (`noAgent`): no agent chosen; `Conflict` (`busy`): the agent starts | works; `InvalidData` |
    /// `InvalidPath`: a prompt that cannot be sent.
    pub fn prompt(&self, args: PromptArgs) -> Result<(), IpcError> {
        check(&args)?;
        if self.core.host.agents.chosen().is_none() {
            return Err(IpcError::new(ErrorKind::Unavailable).detail("noAgent"));
        }
        let mut inner = lock(&self.core.inner);
        let mut left = None;
        let run = match &inner.state {
            State::Starting(_) | State::Working(_) => {
                return Err(IpcError::new(ErrorKind::Conflict).detail("busy"));
            }
            // a prompt instead of signing in: a fresh agent for it
            State::Idle | State::Error(_) | State::NeedsLogin(_) => {
                left = inner.state.agent();
                inner.generation += 1;
                inner.state = State::Starting(None);
                Run::Start
            }
            State::Ready(live) => {
                let live = live.clone();
                inner.state = State::Working(live.clone());
                Run::Turn(live)
            }
        };
        let generation = inner.generation;
        let update = inner.state.update();
        drop(inner);
        if let Some(agent) = left {
            stop_aside(&agent);
        }
        self.core.emit(update);
        let core = Arc::clone(&self.core);
        let spawned =
            std::thread::Builder::new()
                .name("acp-prompt".into())
                .spawn(move || match run {
                    Run::Start => core.start(generation, &args),
                    Run::Turn(live) => core.turn(generation, &live, &args),
                });
        if let Err(error) = spawned {
            self.core
                .fail(generation, format!("The prompt could not be sent: {error}"));
            return Err(IpcError::io("acp", &error));
        }
        Ok(())
    }

    /// Stop: the agent ends its turn (questions waiting are answered `cancelled`); a start under way is stopped.
    pub fn cancel(&self) {
        let mut inner = lock(&self.core.inner);
        match &inner.state {
            State::Working(live) => {
                let live = live.clone();
                let asks: Vec<(String, Ask)> = inner.asks.drain().collect();
                drop(inner);
                let connection = &live.agent.connection;
                let _ = connection.notify(
                    AGENT_METHOD_NAMES.session_cancel,
                    CancelNotification::new(live.session.clone()),
                );
                for (key, ask) in asks {
                    let _ = connection.reply(
                        ask.id,
                        Ok(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        )),
                    );
                    self.core
                        .emit(AcpUpdate::PermissionDone { request_id: key });
                }
            }
            State::Starting(_) | State::NeedsLogin(_) => {
                drop(inner);
                self.reset();
            }
            State::Idle | State::Ready(_) | State::Error(_) => {}
        }
    }

    /// The person signs in: an `agent` method -> the agent signs in itself; any other | none -> tried again (signed in
    /// outside the app). Then the waiting prompt goes on. Answers at once; the outcome comes as events.
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
                // a terminal method | variables: the person signs in outside, nothing for the agent to do
                (method.kind == LoginKind::Agent).then(|| method.id.clone())
            }
        };
        inner.state = State::Starting(Some(Arc::clone(&login.agent)));
        let generation = inner.generation;
        let update = inner.state.update();
        drop(inner);
        self.core.emit(update);
        let core = Arc::clone(&self.core);
        let spawned = std::thread::Builder::new()
            .name("acp-login".into())
            .spawn(move || core.sign_in(generation, login, method));
        if let Err(error) = spawned {
            self.core
                .fail(generation, format!("The sign-in could not start: {error}"));
            return Err(IpcError::io("acp", &error));
        }
        Ok(())
    }

    /// New conversation: the agent stops; the next prompt starts a fresh one (the copy and its changes stay).
    pub fn reset(&self) {
        let (agent, updates) = self.core.stop_all();
        for update in updates {
            self.core.emit(update);
        }
        self.core.emit(State::Idle.update());
        if let Some(agent) = agent {
            stop_aside(&agent);
        }
    }

    /// Plugin off | project closed: the agent stops before the copy goes (waits for it).
    pub fn shutdown(&self) {
        if let (Some(agent), _) = self.core.stop_all() {
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
    }
    if let Some(document) = &args.document {
        project_file(document)?;
    }
    Ok(())
}

/// `file://` URI of a file of the copy.
fn file_uri(abs: &Path, path: &str) -> Result<url::Url, IpcError> {
    url::Url::from_file_path(abs).map_err(|()| IpcError::new(ErrorKind::InvalidPath).subject(path))
}

/// The prompt as ACP content: the text; the attached passage as an embedded text resource named by its file in the
/// copy and its line range (`#L<from>:<to>`); the open document as a link to its file in the copy.
///
/// # Errors
/// A path that is no file of the project.
pub fn blocks(args: &PromptArgs, copy: &WorkCopy) -> Result<Vec<ContentBlock>, IpcError> {
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
        out.push(ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::TextResourceContents(
                TextResourceContents::new(text.clone(), uri.to_string()).mime_type(PASSAGE_MIME),
            ),
        )));
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

/// Whether `path` (as the agent wrote it) is the copy's folder or inside it, also on its real location (a link the
/// agent made in the copy does not count as inside).
fn inside(copy: &Vault, path: &Path) -> bool {
    path.is_absolute()
        && !path.components().any(|c| matches!(c, Component::ParentDir))
        && copy
            .relativize(path)
            .is_some_and(|relative| copy.resolve(&relative).is_ok())
}

/// The option allowing a question once when the tool only works on the copy's files: reads | searches | thinks
/// (no paths needed), or edits | deletes | moves where every path is inside the copy.
fn in_copy(asked: &RequestPermissionRequest, copy: &Vault) -> Option<PermissionOptionId> {
    let fields = &asked.tool_call.fields;
    let locations = fields.locations.as_deref().unwrap_or_default();
    let works_on_copy = match fields.kind? {
        ToolKind::Read | ToolKind::Search | ToolKind::Think => true,
        ToolKind::Edit | ToolKind::Delete | ToolKind::Move => !locations.is_empty(),
        _ => false,
    };
    if !works_on_copy || !locations.iter().all(|l| inside(copy, &l.path)) {
        return None;
    }
    asked
        .options
        .iter()
        .find(|o| o.kind == PermissionOptionKind::AllowOnce)
        .map(|o| o.option_id.clone())
}

/// What a card shows of a tool call: its command, else the paths it touches (project paths inside the copy).
fn detail(fields: &ToolCallUpdateFields, copy: &Vault) -> String {
    match fields.raw_input.as_ref().and_then(|raw| raw.get("command")) {
        Some(Value::String(command)) => return command.clone(),
        Some(Value::Array(parts)) => {
            return parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        }
        _ => {}
    }
    fields
        .locations
        .iter()
        .flatten()
        .map(|location| match copy.relativize(&location.path) {
            Some(relative) if inside(copy, &location.path) && !relative.is_root() => {
                relative.as_str().to_owned()
            }
            _ => location.path.display().to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Plain-English text of a work copy refusal.
fn copy_error(error: &IpcError) -> String {
    match (error.kind, error.detail.as_deref()) {
        (ErrorKind::TooLarge, _) => format!(
            "The project is too large for a work copy (at most {} files and {} MiB).",
            crate::work::MAX_FILES,
            crate::work::MAX_TOTAL_BYTES >> 20
        ),
        (ErrorKind::Unavailable, _) => {
            "The projects inside this folder are not known yet; try again in a moment.".to_owned()
        }
        _ => format!("The work copy could not be made ({error})."),
    }
}

/// Plain-English text of the agent ending by itself, with its last stderr lines.
fn stopped_text(connection: &Connection) -> String {
    let tail = connection.stderr_tail(TAIL_CHARS, TAIL_SETTLE);
    if tail.trim().is_empty() {
        "The agent stopped.".to_owned()
    } else {
        format!("The agent stopped. Its last words:\n{tail}")
    }
}

impl Core {
    fn emit(&self, update: AcpUpdate) {
        if let Ok(value) = serde_json::to_value(update) {
            self.host.events.emit("update", value);
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

    /// Ends the current agent's time: state `Idle`, questions dropped; answers the agent to stop + the updates.
    fn stop_all(&self) -> (Option<Arc<Agent>>, Vec<AcpUpdate>) {
        let mut inner = lock(&self.inner);
        inner.generation += 1;
        let agent = std::mem::take(&mut inner.state).agent();
        inner.tools.clear();
        let updates = inner
            .asks
            .drain()
            .map(|(request_id, _)| AcpUpdate::PermissionDone { request_id })
            .collect();
        (agent, updates)
    }

    /// The run of `generation` failed: state `Error` (with `message`), its agent stopped.
    fn fail(&self, generation: u64, message: String) {
        let taken = self.with_current(generation, |inner| {
            if matches!(inner.state, State::Idle | State::Error(_)) {
                return None;
            }
            let agent = inner.state.agent();
            inner.state = State::Error(message);
            let mut updates: Vec<AcpUpdate> = inner
                .asks
                .drain()
                .map(|(request_id, _)| AcpUpdate::PermissionDone { request_id })
                .collect();
            updates.push(inner.state.update());
            Some((agent, updates))
        });
        let Some(Some((agent, updates))) = taken else {
            return;
        };
        for update in updates {
            self.emit(update);
        }
        if let Some(agent) = agent {
            agent.stop();
        }
    }

    /// The start run: copy, program, session; then the first prompt.
    fn start(self: &Arc<Self>, generation: u64, args: &PromptArgs) {
        match self.open(generation) {
            Ok(Some(Opened::Live(live))) => self.work(generation, &live, args),
            Ok(Some(Opened::Login(agent, methods))) => self.need_login(
                generation,
                Login {
                    agent,
                    session: None,
                    args: args.clone(),
                    methods,
                    error: None,
                },
            ),
            // replaced while starting: whoever replaced it stopped the agent
            Ok(None) => {}
            Err(message) => self.fail(generation, message),
        }
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
        let updates = self.with_current(generation, |inner| {
            let mut updates: Vec<AcpUpdate> = inner
                .asks
                .drain()
                .map(|(request_id, _)| AcpUpdate::PermissionDone { request_id })
                .collect();
            inner.state = State::NeedsLogin(login);
            updates.push(inner.state.update());
            updates
        });
        match updates {
            Some(updates) => {
                for update in updates {
                    self.emit(update);
                }
            }
            None => agent.stop(),
        }
    }

    /// The sign-in run: the agent signs in (`method`: an agent method), then the waiting prompt goes on (in a new
    /// session when `session/new` was the refused step).
    fn sign_in(self: &Arc<Self>, generation: u64, login: Login, method: Option<String>) {
        let Login {
            agent,
            session,
            args,
            methods,
            ..
        } = login;
        let again = |error: String| Login {
            agent: Arc::clone(&agent),
            session: session.clone(),
            args: args.clone(),
            methods: Arc::clone(&methods),
            error: Some(error),
        };
        if let Some(method) = method {
            let signed: Result<Value, CallError> = agent.connection.call(
                AGENT_METHOD_NAMES.authenticate,
                AuthenticateRequest::new(method),
                Some(LOGIN_WAIT),
            );
            match signed {
                Ok(_) => {}
                Err(CallError::Closed) => {
                    return self.fail(generation, stopped_text(&agent.connection));
                }
                Err(error) => {
                    let why = call_failed(&agent.connection, "authenticate", error);
                    return self.need_login(generation, again(why));
                }
            }
        }
        let session = match &session {
            Some(session) => session.clone(),
            None => match open_session(&agent.connection, &agent.dir) {
                Ok(session) => session,
                Err(Opening::Login) => {
                    let why = "The agent still needs you to sign in.".to_owned();
                    return self.need_login(generation, again(why));
                }
                Err(Opening::Failed(message)) => return self.fail(generation, message),
            },
        };
        let live = Live {
            agent: Arc::clone(&agent),
            session,
            methods: Arc::clone(&methods),
        };
        self.work(generation, &live, &args);
    }

    /// The copy's folder, made when absent (`None`: replaced meanwhile).
    fn ensure_copy(&self, generation: u64) -> Result<Option<PathBuf>, String> {
        // held while the copy is made: a plugin stop waits for it, then finds it (or nothing) to remove
        let mut copy = lock(&self.host.copy);
        if let Some(existing) = copy.as_ref() {
            return Ok(Some(existing.dir().to_path_buf()));
        }
        let place = self
            .host
            .place
            .as_ref()
            .ok_or_else(|| "There is no profile folder for the work copy.".to_owned())?;
        let dir = copy_dir(place, self.host.project.root());
        let built = WorkCopy::build(self.host.project.clone(), dir).map_err(|e| copy_error(&e))?;
        if !self.is_current(generation) {
            let _ = built.remove();
            return Ok(None);
        }
        let dir = built.dir().to_path_buf();
        *copy = Some(built);
        Ok(Some(dir))
    }

    /// Reads the person's choice, makes the copy, starts the program, opens the session (`None`: replaced meanwhile).
    fn open(self: &Arc<Self>, generation: u64) -> Result<Option<Opened>, String> {
        let chosen = self
            .host
            .agents
            .chosen()
            .ok_or_else(|| "No assistant is chosen.".to_owned())?;
        let plan = chosen
            .resolved
            .plan
            .clone()
            .ok_or_else(|| missing_text(&chosen))?;
        let Some(dir) = self.ensure_copy(generation)? else {
            return Ok(None);
        };
        let copy = Root::open(&dir)
            .map(Vault::new)
            .map_err(|e| format!("The work copy cannot be opened ({e})."))?;
        let AgentPipes {
            to_agent,
            from_agent,
            stderr,
            stop,
        } = (self.host.launch)(&plan, &dir)
            .map_err(|e| format!("The agent could not be started ({}): {e}", plan.shown))?;
        let inbox = Arc::new(Listener {
            core: Arc::clone(self),
            generation,
        });
        let connection = match Connection::start(to_agent, from_agent, stderr, inbox) {
            Ok(connection) => connection,
            Err(error) => {
                stop();
                return Err(format!("The agent could not be started: {error}"));
            }
        };
        let agent = Arc::new(Agent {
            connection,
            stop: Mutex::new(Some(stop)),
            dir,
            copy,
        });
        let kept = self.with_current(generation, |inner| {
            inner.state = State::Starting(Some(Arc::clone(&agent)));
        });
        if kept.is_none() {
            agent.stop();
            return Ok(None);
        }
        let methods: Arc<[LoginMethod]> = initialize(&agent.connection, &plan.shown)?.into();
        match open_session(&agent.connection, &agent.dir) {
            Ok(session) => Ok(Some(Opened::Live(Live {
                agent,
                session,
                methods,
            }))),
            Err(Opening::Login) => Ok(Some(Opened::Login(agent, methods))),
            Err(Opening::Failed(message)) => Err(message),
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
        let blocks = match self.prepare(args) {
            Ok(blocks) => blocks,
            Err(message) => return self.end_turn(generation, "error", Some(message)),
        };
        let asked: Result<PromptResponse, CallError> = live.agent.connection.call(
            AGENT_METHOD_NAMES.session_prompt,
            PromptRequest::new(live.session.clone(), blocks),
            None,
        );
        match asked {
            Ok(answer) => self.end_turn(generation, &name_of(&answer.stop_reason), None),
            Err(CallError::Closed) => {
                if self.is_current(generation) {
                    self.fail(generation, stopped_text(&live.agent.connection));
                }
            }
            Err(error) if error.needs_login() => self.need_login(
                generation,
                Login {
                    agent: Arc::clone(&live.agent),
                    session: Some(live.session.clone()),
                    args: args.clone(),
                    methods: Arc::clone(&live.methods),
                    error: None,
                },
            ),
            Err(CallError::Refused { message, .. }) => self.end_turn(
                generation,
                "error",
                Some(format!("The agent refused the prompt: {message}")),
            ),
            Err(CallError::Unreadable | CallError::TimedOut) => self.end_turn(
                generation,
                "error",
                Some("The agent's answer could not be read.".to_owned()),
            ),
        }
    }

    /// The copy brought up to date with the person's files, then the prompt's content.
    fn prepare(&self, args: &PromptArgs) -> Result<Vec<ContentBlock>, String> {
        let mut guard = lock(&self.host.copy);
        let copy = guard
            .as_mut()
            .ok_or_else(|| "The work copy is gone.".to_owned())?;
        copy.sync().map_err(|e| copy_error(&e))?;
        blocks(args, copy).map_err(|e| format!("The prompt could not be sent ({e})."))
    }

    /// Files whose copy differs now.
    fn changed(&self) -> u32 {
        lock(&self.host.copy)
            .as_mut()
            .and_then(|copy| copy.changes().ok())
            .map_or(0, |list| u32::try_from(list.len()).unwrap_or(u32::MAX))
    }

    /// The turn of `generation` ended: `Ready` again, `turnEnd` told.
    fn end_turn(&self, generation: u64, stop_reason: &str, error: Option<String>) {
        let changed = self.changed();
        let updates = self.with_current(generation, |inner| {
            let State::Working(live) = &inner.state else {
                return Vec::new();
            };
            inner.state = State::Ready(live.clone());
            // the agent no longer waits for them
            let mut updates: Vec<AcpUpdate> = inner
                .asks
                .drain()
                .map(|(request_id, _)| AcpUpdate::PermissionDone { request_id })
                .collect();
            updates.push(AcpUpdate::TurnEnd {
                stop_reason: stop_reason.to_owned(),
                changed,
            });
            updates.push(AcpUpdate::Status {
                state: AcpState::Ready,
                error,
                methods: None,
            });
            updates
        });
        for update in updates.into_iter().flatten() {
            self.emit(update);
        }
    }

    /// Routes a permission question of `generation` (see [`in_copy`]).
    fn route(&self, generation: u64, id: RequestId, asked: &RequestPermissionRequest) -> Route {
        let mut inner = lock(&self.inner);
        if inner.generation != generation {
            return Route::Drop;
        }
        let State::Working(live) = &inner.state else {
            return Route::Drop;
        };
        let copy = &live.agent.copy;
        if let Some(option) = in_copy(asked, copy) {
            return Route::Allow(option);
        }
        let fields = &asked.tool_call.fields;
        let tool = inner.tools.get(&*asked.tool_call.tool_call_id.0);
        let update = AcpUpdate::Permission {
            // the JSON text of the id: unique for numbers and strings alike
            request_id: serde_json::to_string(&id).unwrap_or_default(),
            title: fields
                .title
                .clone()
                .or_else(|| tool.map(|t| t.title.clone()))
                .unwrap_or_default(),
            kind: name_of(&fields.kind.or(tool.map(|t| t.kind)).unwrap_or_default()),
            detail: detail(fields, copy),
            options: asked
                .options
                .iter()
                .map(|o| PermissionChoice {
                    id: o.option_id.0.to_string(),
                    name: o.name.clone(),
                    kind: name_of(&o.kind),
                })
                .collect(),
        };
        if let AcpUpdate::Permission { request_id, .. } = &update {
            inner.asks.insert(
                request_id.clone(),
                Ask {
                    id,
                    options: asked.options.iter().map(|o| o.option_id.clone()).collect(),
                },
            );
        }
        Route::Ask(update)
    }
}

/// Plain-English text of a call of the start that got no answer.
fn call_failed(connection: &Connection, step: &str, error: CallError) -> String {
    match error {
        CallError::Closed => stopped_text(connection),
        CallError::TimedOut => format!("The agent did not answer ({step}) in time."),
        CallError::Refused { message, .. } => format!("The agent refused ({step}): {message}"),
        CallError::Unreadable => format!("The agent's answer ({step}) could not be read."),
    }
}

/// Plain-English text of a chosen agent that cannot start on this computer.
fn missing_text(chosen: &Chosen) -> String {
    let name = &chosen.name;
    match chosen.resolved.needs {
        Some(AgentNeed::Node) => format!("{name} needs Node.js: npx was not found."),
        Some(AgentNeed::Uv) => format!("{name} needs uv: uvx was not found."),
        Some(AgentNeed::Install) | None => {
            let program = chosen
                .resolved
                .shown
                .split_whitespace()
                .next()
                .unwrap_or(name);
            format!("{name} is not installed: {program} was not found.")
        }
    }
}

/// `initialize`: answers the agent's sign-in methods (a terminal method's line = `shown` + its arguments).
fn initialize(connection: &Connection, shown: &str) -> Result<Vec<LoginMethod>, String> {
    // terminal sign-in: the person runs the shown line in a terminal; the app itself never opens one
    let caps = ClientCapabilities::new().auth(AuthCapabilities::new().terminal(true));
    let hello = InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(caps)
        .client_info(Implementation::new("PerMark", env!("CARGO_PKG_VERSION")));
    let raw: Value = connection
        .call(AGENT_METHOD_NAMES.initialize, hello, Some(SETUP_WAIT))
        .map_err(|e| call_failed(connection, "initialize", e))?;
    let init: InitializeResponse = serde_json::from_value(raw.clone())
        .map_err(|_| call_failed(connection, "initialize", CallError::Unreadable))?;
    if init.protocol_version != ProtocolVersion::V1 {
        return Err(format!(
            "The agent speaks protocol version {}; PerMark speaks version 1.",
            init.protocol_version
        ));
    }
    Ok(login_methods(raw.get("authMethods"), shown))
}

/// The sign-in methods of an `initialize` answer, read one by one (a method of a kind this client does not know is
/// left out; a typed decode would read it as an `agent` method and send it to `authenticate`).
fn login_methods(raw: Option<&Value>, shown: &str) -> Vec<LoginMethod> {
    raw.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|method| {
            let id = method.get("id")?.as_str()?.to_owned();
            let text = |key: &str| method.get(key).and_then(Value::as_str).map(str::to_owned);
            let (kind, hint) = match method.get("type").and_then(Value::as_str) {
                None | Some("agent") => (LoginKind::Agent, None),
                Some("terminal") => {
                    let terminal =
                        serde_json::from_value::<AuthMethodTerminal>(method.clone()).ok()?;
                    (LoginKind::Terminal, Some(terminal_line(&terminal, shown)))
                }
                Some("env_var") => (LoginKind::Env, Some(variable_names(method))),
                Some(_) => return None,
            };
            Some(LoginMethod {
                name: text("name").unwrap_or_else(|| id.clone()),
                description: text("description"),
                id,
                kind,
                hint,
            })
        })
        .collect()
}

/// The line a person runs for a terminal sign-in: its variables (`NAME=value`, by name), the agent's command, the
/// method's arguments.
fn terminal_line(method: &AuthMethodTerminal, shown: &str) -> String {
    let mut env: Vec<(&String, &String)> = method.env.iter().collect();
    env.sort();
    env.iter()
        .map(|(name, value)| format!("{name}={value}"))
        .chain(std::iter::once(shown.to_owned()))
        .chain(method.args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The variable names an environment sign-in asks for (`vars[].name` | `varName`), comma separated.
fn variable_names(method: &Value) -> String {
    let listed = method
        .get("vars")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|var| var.get("name").and_then(Value::as_str));
    let single = method.get("varName").and_then(Value::as_str);
    listed.chain(single).collect::<Vec<_>>().join(", ")
}

/// `session/new` -> `session/set_mode` (when the asking mode is offered and not on).
fn open_session(connection: &Connection, dir: &Path) -> Result<SessionId, Opening> {
    let new: NewSessionResponse = match connection.call(
        AGENT_METHOD_NAMES.session_new,
        NewSessionRequest::new(dir),
        Some(SETUP_WAIT),
    ) {
        Ok(new) => new,
        Err(error) if error.needs_login() => return Err(Opening::Login),
        Err(error) => {
            return Err(Opening::Failed(call_failed(
                connection,
                "session/new",
                error,
            )))
        }
    };
    if let Some(modes) = &new.modes {
        let offered = modes.available_modes.iter().any(|m| &*m.id.0 == ASK_MODE);
        if offered && &*modes.current_mode_id.0 != ASK_MODE {
            let _: Value = connection
                .call(
                    AGENT_METHOD_NAMES.session_set_mode,
                    SetSessionModeRequest::new(new.session_id.clone(), ASK_MODE),
                    Some(SETUP_WAIT),
                )
                .map_err(|e| Opening::Failed(call_failed(connection, "session/set_mode", e)))?;
        }
    }
    Ok(new.session_id)
}

/// The reader thread's view of the session, for one agent.
struct Listener {
    core: Arc<Core>,
    generation: u64,
}

impl Inner {
    /// Updates for one piece of the agent's news.
    fn news(&mut self, update: SessionUpdate) -> Vec<AcpUpdate> {
        match update {
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
            SessionUpdate::ToolCall(call) => {
                self.thinking = false;
                let id = call.tool_call_id.0.to_string();
                let line = ToolLine {
                    title: call.title,
                    kind: call.kind,
                    status: call.status,
                };
                let update = line.update(&id);
                self.tools.insert(id, line);
                vec![update]
            }
            SessionUpdate::ToolCallUpdate(call) => {
                self.thinking = false;
                let id = call.tool_call_id.0.to_string();
                let line = self.tools.entry(id.clone()).or_default();
                let before = line.clone();
                line.apply(&call.fields);
                // updates that only stream the tool's input change nothing shown
                if *line == before {
                    Vec::new()
                } else {
                    vec![line.update(&id)]
                }
            }
            // modes, commands, usage, plans, titles: not shown
            _ => Vec::new(),
        }
    }
}

impl Inbox for Listener {
    fn notification(&self, method: &str, params: Option<Value>) {
        // anything but session news (account status among it) is dropped unread
        if method != CLIENT_METHOD_NAMES.session_update {
            return;
        }
        // a kind of news this client does not know does not decode: skipped
        let Some(Ok(note)) = params.map(serde_json::from_value::<SessionNotification>) else {
            return;
        };
        let updates = self
            .core
            .with_current(self.generation, |inner| inner.news(note.update));
        for update in updates.into_iter().flatten() {
            self.core.emit(update);
        }
    }

    fn request(&self, connection: &Connection, id: RequestId, method: &str, params: Option<Value>) {
        // no file system, no terminal, no forms offered: the agent works on the copy with its own tools
        if method != CLIENT_METHOD_NAMES.session_request_permission {
            let _ = connection.reply::<Value>(id, Err(RpcError::method_not_found()));
            return;
        }
        let Some(Ok(asked)) = params.map(serde_json::from_value::<RequestPermissionRequest>) else {
            let _ = connection.reply::<Value>(id, Err(RpcError::invalid_params()));
            return;
        };
        match self.core.route(self.generation, id.clone(), &asked) {
            Route::Allow(option) => {
                let _ = connection.reply(
                    id,
                    Ok(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option)),
                    )),
                );
            }
            Route::Ask(update) => self.core.emit(update),
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
            self.core
                .fail(self.generation, stopped_text(&agent.connection));
        }
    }
}

#[cfg(test)]
mod tests;
