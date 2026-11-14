//! The app's side: the project's endpoint, who is connected, the latest requests, and the switch that starts and
//! stops listening. A session (a `permark mcp` that said hello) is held open; while one is, the project is
//! `Connected`.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use permark_core::conditions::AgentCondition;
use permark_core::ipc_local::{Endpoint, IpcLocalError, Reply, Server};
use permark_core::plugins::{PluginEvents, ProjectFacts};
use permark_core::settings::ProjectConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::perms::{self, Access, Group};
use crate::session::{self, SessionId};
use crate::tools::Tools;
use crate::ToolError;

/// How many requests the status popover lists.
pub const RECENT: usize = 8;
/// Longest client name kept.
const MAX_CLIENT_CHARS: usize = 64;
/// Longest working folder kept (display only).
const MAX_FOLDER_CHARS: usize = 260;

/// One line from `permark mcp`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Request {
    /// Opens a session: the connection is held while the client runs.
    Hello {
        /// `None` = missing | malformed on the wire: refused.
        #[serde(default, deserialize_with = "session::lenient")]
        session: Option<SessionId>,
        client: String,
        /// The client's working folder relative to the project root, `/`-separated (`""` = the root).
        #[serde(default)]
        folder: String,
    },
    /// One tool call.
    Call {
        tool: String,
        #[serde(default)]
        args: Value,
        #[serde(default)]
        client: String,
        #[serde(default, deserialize_with = "session::lenient")]
        session: Option<SessionId>,
    },
}

/// The answer line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Response {
    Ok { value: Value },
    Error { code: String, message: String },
}

impl Response {
    #[must_use]
    pub fn of(result: Result<Value, ToolError>) -> Self {
        match result {
            Ok(value) => Self::Ok { value },
            Err(error) => Self::Error {
                code: error.code().to_owned(),
                message: error.to_string(),
            },
        }
    }
}

/// Whether the endpoint listens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export, rename = "AgentServerState")]
pub enum ServerState {
    Off,
    Listening,
    Error { reason: String },
}

/// One request an agent made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AgentRequest")]
pub struct RequestRecord {
    /// The session that asked.
    #[ts(type = "string")]
    pub session: SessionId,
    pub tool: String,
    pub subject: Option<String>,
    #[ts(type = "number")]
    pub at_ms: u64,
    /// `ok | denied | notFound | invalid | error`.
    pub outcome: String,
}

/// One connected agent (a `permark mcp` process that said hello and is still running).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSession {
    #[ts(type = "string")]
    pub id: SessionId,
    /// The name the client gave itself (not verified).
    pub name: String,
    #[ts(type = "number")]
    pub started_ms: u64,
    /// Working folder relative to the project root, `/`-separated; empty = the root.
    pub folder: String,
    #[ts(type = "number | null")]
    pub last_call_ms: Option<u64>,
}

/// What the status item, its popover and the settings show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentStatus {
    /// The switch is on for this folder.
    pub enabled: bool,
    /// The switch was turned on for another folder that stood at this path (shown off, with why).
    pub replaced: bool,
    pub server: ServerState,
    /// Connected agents, the earliest first.
    pub sessions: Vec<AgentSession>,
    /// Group id -> on (the person's choice, default when not chosen).
    pub groups: BTreeMap<String, bool>,
    /// Groups the project's `.permark/config.toml` denies.
    pub denied: Vec<String>,
    pub recent: Vec<RequestRecord>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Printable text of at most `max` chars (a client's self-declared words, shown in the app).
fn clean(text: &str, max: usize) -> String {
    let text: String = text.chars().filter(|c| !c.is_control()).take(max).collect();
    text.trim().to_owned()
}

/// What the hub knows of one session.
struct SessionInfo {
    name: String,
    started_ms: u64,
    folder_rel: String,
    last_call_ms: Option<u64>,
    /// Which held connection keeps the session (a hello again replaces it; only that one's end removes it).
    holder: u64,
    /// Order of the first hello (milliseconds tie).
    order: u64,
}

/// What the server threads and the window's calls share.
pub struct Hub {
    pub tools: Tools,
    facts: Arc<ProjectFacts>,
    events: PluginEvents,
    sessions: Mutex<BTreeMap<SessionId, SessionInfo>>,
    next: AtomicU64,
    recent: Mutex<VecDeque<RequestRecord>>,
    server: Mutex<ServerState>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Ends a session when dropped (peer gone or server stopped).
struct Session {
    hub: Weak<Hub>,
    id: SessionId,
    holder: u64,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            {
                let mut sessions = lock(&hub.sessions);
                if sessions
                    .get(&self.id)
                    .is_some_and(|s| s.holder == self.holder)
                {
                    sessions.remove(&self.id);
                }
            }
            hub.changed();
        }
    }
}

/// The refusal of a request that names no valid session.
fn no_session() -> Reply<Response> {
    Reply::Answer(Response::of(Err(ToolError::Invalid(
        "no valid session id; restart the agent with this version of PerMark".to_owned(),
    ))))
}

impl Hub {
    #[must_use]
    pub fn new(tools: Tools, facts: Arc<ProjectFacts>, events: PluginEvents) -> Arc<Self> {
        Arc::new(Self {
            tools,
            facts,
            events,
            sessions: Mutex::new(BTreeMap::new()),
            next: AtomicU64::new(1),
            recent: Mutex::new(VecDeque::new()),
            server: Mutex::new(ServerState::Off),
        })
    }

    /// One request line.
    fn handle(self: &Arc<Self>, request: Request) -> Reply<Response> {
        match request {
            Request::Hello {
                session: Some(id),
                client,
                folder,
            } => {
                let name = Some(clean(&client, MAX_CLIENT_CHARS))
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| "agent".to_owned());
                let holder = self.next.fetch_add(1, Ordering::Relaxed);
                {
                    // a call made before the hello still counts as the session's last call
                    let last_call = lock(&self.recent)
                        .iter()
                        .find(|r| r.session == id)
                        .map(|r| r.at_ms);
                    let mut sessions = lock(&self.sessions);
                    // the same process again (its held connection was lost): it keeps its start and last call
                    let (started_ms, last_call_ms, order) = sessions
                        .get(&id)
                        .map_or((now_ms(), last_call, holder), |s| {
                            (s.started_ms, s.last_call_ms, s.order)
                        });
                    sessions.insert(
                        id.clone(),
                        SessionInfo {
                            name,
                            started_ms,
                            folder_rel: clean(&folder, MAX_FOLDER_CHARS),
                            last_call_ms,
                            holder,
                            order,
                        },
                    );
                }
                self.changed();
                Reply::Hold(
                    Response::Ok { value: Value::Null },
                    Box::new(Session {
                        hub: Arc::downgrade(self),
                        id,
                        holder,
                    }),
                )
            }
            Request::Call {
                tool,
                args,
                session: Some(id),
                ..
            } => Reply::Answer(Response::of(self.call(&id, &tool, args))),
            Request::Hello { session: None, .. } | Request::Call { session: None, .. } => {
                no_session()
            }
        }
    }

    /// Runs a tool for `session` and notes it for the popover.
    ///
    /// # Errors
    /// What the tool answers.
    pub fn call(&self, session: &SessionId, tool: &str, args: Value) -> Result<Value, ToolError> {
        let subject = ["path", "query", "id", "note", "tag"]
            .iter()
            .find_map(|key| args.get(*key).and_then(Value::as_str))
            .map(|s| s.chars().take(120).collect::<String>());
        let result = self.tools.call(tool, args);
        if let Err(ToolError::Denied {
            group,
            first_ask: true,
        }) = &result
        {
            self.ask(*group);
        }
        let outcome = match &result {
            Ok(_) => "ok",
            Err(error) => error.code(),
        };
        let at_ms = now_ms();
        if let Some(info) = lock(&self.sessions).get_mut(session) {
            info.last_call_ms = Some(at_ms);
        }
        {
            let mut recent = lock(&self.recent);
            recent.push_front(RequestRecord {
                session: session.clone(),
                tool: tool.chars().take(40).collect(),
                subject,
                at_ms,
                outcome: outcome.to_owned(),
            });
            recent.truncate(RECENT);
        }
        self.changed();
        result
    }

    /// The first time resolving is asked for and is off: the person is offered it once.
    fn ask(&self, group: Group) {
        let store = self.tools.store();
        let mut choice = perms::load(store);
        if choice.resolve_asked {
            return;
        }
        choice.resolve_asked = true;
        let _ = store.profile_file(perms::PROJECT_FILE).save(&choice);
        self.events
            .emit("ask", serde_json::json!({ "group": group.id() }));
    }

    #[must_use]
    pub fn status(&self) -> AgentStatus {
        let choice = perms::load(self.tools.store());
        let root = self.tools.vault_root().path();
        let config = ProjectConfig::load(self.tools.vault_root());
        let effective = perms::effective(&choice.groups, &config.agent_deny);
        AgentStatus {
            enabled: choice.is_on(root),
            replaced: choice.replaced(root),
            server: lock(&self.server).clone(),
            sessions: self.sessions(),
            groups: Group::ALL
                .into_iter()
                .map(|g| {
                    let on = choice
                        .groups
                        .get(g.id())
                        .copied()
                        .unwrap_or_else(|| g.default_on());
                    (g.id().to_owned(), on)
                })
                .collect(),
            denied: effective
                .iter()
                .filter(|(_, a)| **a == Access::DeniedByProject)
                .map(|(g, _)| g.id().to_owned())
                .collect(),
            recent: lock(&self.recent).iter().cloned().collect(),
        }
    }

    /// Connected agents, the earliest first.
    fn sessions(&self) -> Vec<AgentSession> {
        let sessions = lock(&self.sessions);
        let mut list: Vec<(&SessionId, &SessionInfo)> = sessions.iter().collect();
        list.sort_by_key(|(_, s)| s.order);
        list.into_iter()
            .map(|(id, s)| AgentSession {
                id: id.clone(),
                name: s.name.clone(),
                started_ms: s.started_ms,
                folder: s.folder_rel.clone(),
                last_call_ms: s.last_call_ms,
            })
            .collect()
    }

    /// Recomputes the project's agent condition and tells the windows.
    pub fn changed(&self) {
        let first = self.sessions().into_iter().next();
        let condition = match (&*lock(&self.server), first) {
            (ServerState::Listening, Some(first)) => {
                AgentCondition::Connected { client: first.name }
            }
            (ServerState::Listening, None) => AgentCondition::On,
            (ServerState::Off | ServerState::Error { .. }, _) => AgentCondition::Off,
        };
        self.facts.set_agent(condition);
        let status = serde_json::to_value(self.status()).unwrap_or(Value::Null);
        self.events.emit("status", status);
    }

    fn set_server(&self, state: ServerState) {
        *lock(&self.server) = state;
        if !matches!(*lock(&self.server), ServerState::Listening) {
            lock(&self.sessions).clear();
        }
        self.changed();
    }
}

/// The endpoint and its server: listening only while the switch is on (teardown: server dropped, sessions end).
pub struct Switch {
    pub hub: Arc<Hub>,
    endpoint: Result<Endpoint, String>,
    server: Mutex<Option<Server>>,
}

impl Switch {
    #[must_use]
    pub fn new(hub: Arc<Hub>, endpoint: Result<Endpoint, IpcLocalError>) -> Self {
        Self {
            hub,
            endpoint: endpoint.map_err(|e| e.to_string()),
            server: Mutex::new(None),
        }
    }

    /// Makes the listener match the switch in the profile.
    pub fn apply(&self) {
        let on = perms::load(self.hub.tools.store()).is_on(self.hub.tools.vault_root().path());
        let mut server = lock(&self.server);
        match (on, server.is_some()) {
            (true, true) | (false, false) => {
                if !on {
                    self.hub.set_server(ServerState::Off);
                }
            }
            (false, true) => {
                server.take();
                self.hub.set_server(ServerState::Off);
            }
            (true, false) => {
                let started = self
                    .endpoint
                    .clone()
                    .and_then(|endpoint| endpoint.listen().map_err(|e| reason_of(&e)))
                    .and_then(|listening| {
                        let hub = Arc::clone(&self.hub);
                        Server::spawn_with(listening, move |request: Request| hub.handle(request))
                            .map_err(|e| e.to_string())
                    });
                match started {
                    Ok(started) => {
                        *server = Some(started);
                        self.hub.set_server(ServerState::Listening);
                    }
                    Err(reason) => self.hub.set_server(ServerState::Error { reason }),
                }
            }
        }
    }

    /// Stops listening (plugin off, project closed).
    pub fn stop(&self) {
        lock(&self.server).take();
        self.hub.set_server(ServerState::Off);
    }
}

fn reason_of(error: &IpcLocalError) -> String {
    match error {
        IpcLocalError::InUse(_) => "inUse".to_owned(),
        other => other.to_string(),
    }
}
