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
use crate::tools::Tools;
use crate::ToolError;

/// How many requests the status popover lists.
pub const RECENT: usize = 8;
/// Longest client name kept.
const MAX_CLIENT_CHARS: usize = 64;

/// One line from `permark mcp`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Request {
    /// Opens a session: the connection is held while the client runs.
    Hello { client: String },
    /// One tool call.
    Call {
        tool: String,
        #[serde(default)]
        args: Value,
        #[serde(default)]
        client: String,
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
    pub tool: String,
    pub subject: Option<String>,
    #[ts(type = "number")]
    pub at_ms: u64,
    /// `ok | denied | notFound | invalid | error`.
    pub outcome: String,
}

/// What the status item, its popover and the settings show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentStatus {
    pub enabled: bool,
    pub server: ServerState,
    pub clients: Vec<String>,
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

/// What the server threads and the window's calls share.
pub struct Hub {
    pub tools: Tools,
    facts: Arc<ProjectFacts>,
    events: PluginEvents,
    sessions: Mutex<BTreeMap<u64, String>>,
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
    id: u64,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            lock(&hub.sessions).remove(&self.id);
            hub.changed();
        }
    }
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
            Request::Hello { client } => {
                let client: String = client
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(MAX_CLIENT_CHARS)
                    .collect();
                let client = if client.trim().is_empty() {
                    "agent".to_owned()
                } else {
                    client.trim().to_owned()
                };
                let id = self.next.fetch_add(1, Ordering::Relaxed);
                lock(&self.sessions).insert(id, client);
                self.changed();
                Reply::Hold(
                    Response::Ok { value: Value::Null },
                    Box::new(Session {
                        hub: Arc::downgrade(self),
                        id,
                    }),
                )
            }
            Request::Call { tool, args, .. } => Reply::Answer(Response::of(self.call(&tool, args))),
        }
    }

    /// Runs a tool and notes it for the popover.
    ///
    /// # Errors
    /// What the tool answers.
    pub fn call(&self, tool: &str, args: Value) -> Result<Value, ToolError> {
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
        {
            let mut recent = lock(&self.recent);
            recent.push_front(RequestRecord {
                tool: tool.chars().take(40).collect(),
                subject,
                at_ms: now_ms(),
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
        let config = ProjectConfig::load(self.tools.vault_root());
        let effective = perms::effective(&choice.groups, &config.agent_deny);
        AgentStatus {
            enabled: choice.enabled,
            server: lock(&self.server).clone(),
            clients: lock(&self.sessions).values().cloned().collect(),
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

    /// Recomputes the project's agent condition and tells the windows.
    pub fn changed(&self) {
        let condition = match (&*lock(&self.server), lock(&self.sessions).values().next()) {
            (ServerState::Listening, Some(client)) => AgentCondition::Connected {
                client: client.clone(),
            },
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
        let on = perms::load(self.hub.tools.store()).enabled;
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
