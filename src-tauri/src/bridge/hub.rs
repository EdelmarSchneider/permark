//! The brain of the connection, with no socket and no window in it: what the window last published, who is
//! connected and what they asked, a place waiting to be shown, and the answer to every request.
//!
//! The socket server hands each request line to [`Hub::handle`]. The window publishes through [`Hub::publish`] and
//! picks up a place an agent asked to be shown with [`Hub::take_open`]. What only an open project can do (the live
//! search index, the live unread markers) comes through [`Backend`]; what the interface must hear about goes
//! out through [`AgentEvents`].

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;

use crate::notes::{NoteError, NoteView};
use crate::search::SearchHit;
use crate::util::now_ms;
use crate::vault::Vault;

use super::answers;
use super::context::{AgentContext, Published};
use super::protocol::{
    Call, ErrorKind, Hits, Opened, ProtocolError, Reply, Request, UnreadList, DEFAULT_SEARCH_HITS,
    MAX_SEARCH_HITS,
};

/// How many requests the popover can list. One constant to change.
pub const RECENT_REQUESTS: usize = 8;

/// What only the open project can answer.
pub trait Backend: Send + Sync + 'static {
    /// The documents that are unread now.
    fn unread(&self) -> Vec<String>;
    /// The best sections for a query.
    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit>;
    /// Every review note, with its passage found again in the document as it is now.
    fn notes(&self) -> Vec<NoteView>;
    /// Marks a review note done by an agent.
    fn resolve_note(&self, id: &str, reply: Option<&str>) -> Result<NoteView, NoteError>;
}

/// What the interface hears about.
pub trait AgentEvents: Send + Sync + 'static {
    /// The status changed (a connection came or went, a request was served, the connection was turned on or off).
    fn status_changed(&self, status: &AgentStatus);
    /// An agent asked to show a place; [`Hub::take_open`] has it.
    fn open_requested(&self);
}

/// One request an agent made, for the status popover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRecord {
    /// The tool, as an agent calls it (`get_context`, `search`, ...).
    pub tool: String,
    /// What it was about (the query, the path), when it was about something.
    pub subject: Option<String>,
    /// When it was made (milliseconds since the Unix epoch).
    pub at_ms: u64,
}

/// What the status bar and its popover show. Matches the UI's `AgentStatus`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    /// The agent connection is on for this project.
    pub enabled: bool,
    /// The program is listening (it is not when the socket could not be opened; `error` says why).
    pub listening: bool,
    /// How many programs are connected now (an agent's `trailmark mcp`, a hook's `trailmark context`).
    pub connections: usize,
    /// The latest requests, newest first.
    pub recent: Vec<RequestRecord>,
    pub error: Option<String>,
    /// The program an agent starts (`<program> mcp`): where this one is, for the setup snippets.
    pub program: Option<String>,
}

#[derive(Debug, Default)]
struct State {
    enabled: bool,
    listening: bool,
    error: Option<String>,
    connections: usize,
    recent: VecDeque<RequestRecord>,
    program: Option<String>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn json<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

pub struct Hub {
    vault: Arc<Vault>,
    backend: Arc<dyn Backend>,
    events: Arc<dyn AgentEvents>,
    latest: Mutex<Published>,
    pending_open: Mutex<Option<Opened>>,
    state: Mutex<State>,
}

impl Hub {
    pub fn new(vault: Arc<Vault>, backend: Arc<dyn Backend>, events: Arc<dyn AgentEvents>) -> Self {
        Self {
            vault,
            backend,
            events,
            latest: Mutex::new(Published {
                context: AgentContext::default(),
                updated_at_ms: now_ms(),
            }),
            pending_open: Mutex::new(None),
            state: Mutex::new(State::default()),
        }
    }

    /// Says where the program is, so the setup snippets can name it.
    #[must_use]
    pub fn with_program(self, program: Option<String>) -> Self {
        lock(&self.state).program = program;
        self
    }

    // ---- what the window publishes -----------------------------------------------------------------

    /// Keeps what the window shows (after deciding what may be shared) and returns it with its time.
    pub fn publish(&self, context: AgentContext) -> Published {
        let published = Published {
            context: context.sanitize(&self.vault),
            updated_at_ms: now_ms(),
        };
        *lock(&self.latest) = published.clone();
        published
    }

    /// What the window showed last.
    pub fn context(&self) -> Published {
        lock(&self.latest).clone()
    }

    /// The place an agent asked to be shown, if the window has not picked it up yet.
    pub fn take_open(&self) -> Option<Opened> {
        lock(&self.pending_open).take()
    }

    // ---- status ---------------------------------------------------------------------------------------

    pub fn status(&self) -> AgentStatus {
        Self::snapshot(&lock(&self.state))
    }

    fn snapshot(state: &State) -> AgentStatus {
        AgentStatus {
            enabled: state.enabled,
            listening: state.listening,
            connections: state.connections,
            recent: state.recent.iter().cloned().collect(),
            error: state.error.clone(),
            program: state.program.clone(),
        }
    }

    /// Changes the state and tells the interface, outside the lock.
    fn update(&self, change: impl FnOnce(&mut State)) {
        let status = {
            let mut state = lock(&self.state);
            change(&mut state);
            Self::snapshot(&state)
        };
        self.events.status_changed(&status);
    }

    /// The connection was turned on or off, and whether the program is listening now.
    pub fn set_lifecycle(&self, enabled: bool, listening: bool, error: Option<String>) {
        self.update(|state| {
            state.enabled = enabled;
            state.listening = listening;
            state.error = error;
            if !listening {
                state.connections = 0;
            }
        });
    }

    pub fn connection_opened(&self) {
        self.update(|state| state.connections += 1);
    }

    pub fn connection_closed(&self) {
        self.update(|state| state.connections = state.connections.saturating_sub(1));
    }

    pub fn connections(&self) -> usize {
        lock(&self.state).connections
    }

    fn note(&self, request: &Request) {
        let record = RequestRecord {
            tool: request.tool().to_owned(),
            subject: request.subject(),
            at_ms: now_ms(),
        };
        self.update(|state| {
            state.recent.push_front(record);
            state.recent.truncate(RECENT_REQUESTS);
        });
    }

    // ---- answering -------------------------------------------------------------------------------------

    /// The reply to one request.
    pub fn handle(&self, call: Call) -> Reply {
        let Call { id, request } = call;
        self.note(&request);
        match request {
            Request::GetContext => Reply::success(id, json(&self.context())),
            Request::ListUnread => {
                let docs = answers::unread_docs(&self.vault, &self.backend.unread());
                Reply::success(id, json(&UnreadList::new(docs)))
            }
            Request::Search { query, limit } => {
                let query = query.trim();
                if query.is_empty() {
                    return Reply::failure(
                        id,
                        ProtocolError::new(ErrorKind::BadRequest, "the query is empty"),
                    );
                }
                let limit = limit
                    .unwrap_or(DEFAULT_SEARCH_HITS)
                    .clamp(1, MAX_SEARCH_HITS);
                let hits = answers::plain_hits(self.backend.search(query, limit));
                Reply::success(id, json(&Hits { hits }))
            }
            Request::ListNotes { which } => {
                let list = answers::note_list(self.backend.notes(), which.unwrap_or_default());
                Reply::success(id, json(&list))
            }
            Request::ResolveNote { note, reply } => {
                match self.backend.resolve_note(&note, reply.as_deref()) {
                    Ok(view) => Reply::success(id, json(&view)),
                    Err(error @ NoteError::NotFound(_)) => Reply::failure(
                        id,
                        ProtocolError::new(ErrorKind::NotFound, error.to_string()),
                    ),
                    Err(error) => Reply::failure(
                        id,
                        ProtocolError::new(ErrorKind::Unavailable, error.to_string()),
                    ),
                }
            }
            Request::Open {
                path,
                line,
                heading,
            } => match answers::prepare_open(&self.vault, &path, line, heading.as_deref()) {
                Ok(opened) => {
                    *lock(&self.pending_open) = Some(opened.clone());
                    self.events.open_requested();
                    Reply::success(id, json(&opened))
                }
                Err(error) => Reply::failure(id, error),
            },
        }
    }

    /// The line of a client that is not a request: the id it carried, if any, is answered with the error.
    pub fn reject(line: &str) -> Reply {
        let id = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| value.get("id")?.as_u64())
            .unwrap_or(0);
        Reply::failure(
            id,
            ProtocolError::new(ErrorKind::BadRequest, "not a request this program knows"),
        )
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A project's answers from lists, and a record of what the interface was told.
    #[derive(Default)]
    pub struct Fake {
        pub unread: Mutex<Vec<String>>,
        pub corpus: Mutex<Vec<SearchHit>>,
        pub notes: Mutex<Vec<NoteView>>,
        pub resolved: Mutex<Vec<(String, Option<String>)>>,
        pub statuses: Mutex<Vec<AgentStatus>>,
        pub opens: AtomicUsize,
        /// How many times the unread list was asked for (counted before it waits for its lock).
        pub unread_asked: AtomicUsize,
    }

    impl Backend for Fake {
        fn unread(&self) -> Vec<String> {
            self.unread_asked.fetch_add(1, Ordering::SeqCst);
            lock(&self.unread).clone()
        }

        fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
            lock(&self.corpus)
                .iter()
                .filter(|hit| hit.snippet.contains(query))
                .take(limit)
                .cloned()
                .collect()
        }

        fn notes(&self) -> Vec<NoteView> {
            lock(&self.notes).clone()
        }

        fn resolve_note(&self, id: &str, reply: Option<&str>) -> Result<NoteView, NoteError> {
            let mut notes = lock(&self.notes);
            let note = notes
                .iter_mut()
                .find(|note| note.id == id)
                .ok_or_else(|| NoteError::NotFound(id.to_owned()))?;
            note.done = true;
            note.reply = reply.map(str::to_owned);
            lock(&self.resolved).push((id.to_owned(), reply.map(str::to_owned)));
            Ok(note.clone())
        }
    }

    impl AgentEvents for Fake {
        fn status_changed(&self, status: &AgentStatus) {
            lock(&self.statuses).push(status.clone());
        }

        fn open_requested(&self) {
            self.opens.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// An open note on a passage of `path`, as the window lists it.
    pub fn note(id: &str, path: &str, line: u32) -> NoteView {
        NoteView {
            id: id.to_owned(),
            path: path.to_owned(),
            text: format!("note {id}"),
            quote: "Ship it".to_owned(),
            heading_path: vec!["Plan".to_owned()],
            line,
            end_line: line,
            created_ms: 1_760_520_600_000,
            state: crate::notes::PassageState::Anchored,
            done: false,
            done_ms: None,
            done_by: None,
            reply: None,
        }
    }

    pub fn hub(vault: Arc<Vault>) -> (Arc<Hub>, Arc<Fake>) {
        let fake = Arc::new(Fake::default());
        let hub = Arc::new(Hub::new(vault, fake.clone(), fake.clone()));
        (hub, fake)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use serde_json::json;

    use super::testing::{hub, note};
    use super::*;
    use crate::bridge::context::ViewMode;
    use crate::notes::NoteFilter;

    fn project() -> (tempfile::TempDir, Arc<Vault>) {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("docs")).expect("mkdir");
        std::fs::write(
            dir.path().join("docs/plan.md"),
            "# Plan\n\nIntro.\n\n## Rollout\n\nShip it.\n",
        )
        .expect("write");
        let vault = Arc::new(Vault::open(dir.path()).expect("vault"));
        (dir, vault)
    }

    fn call(id: u64, request: Request) -> Call {
        Call { id, request }
    }

    fn hit(path: &str, snippet: &str) -> SearchHit {
        SearchHit {
            path: path.to_owned(),
            heading_path: vec!["Plan".to_owned()],
            snippet: snippet.to_owned(),
            start_line: 5,
            score: 1.0,
        }
    }

    #[test]
    fn before_anything_is_published_nothing_is_open() {
        let (_dir, vault) = project();
        let (hub, _) = hub(vault);
        let reply = hub.handle(call(1, Request::GetContext));
        assert!(reply.ok);
        let result = reply.into_result().expect("a result");
        assert_eq!(result["document"], serde_json::Value::Null);
        assert_eq!(result["unsaved"], false);
        assert!(result["updatedAtMs"].as_u64().is_some());
    }

    #[test]
    fn a_published_context_is_what_get_context_answers_once_sanitized() {
        let (_dir, vault) = project();
        let (hub, _) = hub(vault);
        hub.publish(AgentContext {
            document: Some("docs\\plan.md".to_owned()),
            mode: Some(ViewMode::Source),
            unsaved: true,
            ..AgentContext::default()
        });
        let result = hub
            .handle(call(2, Request::GetContext))
            .into_result()
            .expect("a result");
        assert_eq!(result["document"], "docs/plan.md");
        assert_eq!(result["mode"], "source");
        assert_eq!(result["unsaved"], true);
        // A path outside the project is never kept.
        hub.publish(AgentContext {
            document: Some("../secret.md".to_owned()),
            ..AgentContext::default()
        });
        assert_eq!(hub.context().context, AgentContext::default());
    }

    #[test]
    fn unread_documents_come_from_the_backend_newest_first_without_what_is_gone() {
        let (dir, vault) = project();
        std::fs::write(dir.path().join("fresh.md"), "x").expect("write");
        let (hub, fake) = hub(vault);
        *lock(&fake.unread) = ["docs/plan.md", "fresh.md", "deleted.md", "@wt/x/y.md"]
            .map(str::to_owned)
            .to_vec();
        let result = hub
            .handle(call(3, Request::ListUnread))
            .into_result()
            .expect("a result");
        assert_eq!(result["total"], 2);
        let paths: Vec<_> = result["unread"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|doc| doc["path"].as_str().expect("path").to_owned())
            .collect();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"fresh.md".to_owned()));
        assert!(paths.contains(&"docs/plan.md".to_owned()));
    }

    #[test]
    fn search_answers_in_plain_text_and_inside_its_limits() {
        let (_dir, vault) = project();
        let (hub, fake) = hub(vault);
        *lock(&fake.corpus) = vec![
            hit("docs/plan.md", "<mark>Ship</mark> it &amp; go"),
            hit("@wt/f/docs/plan.md", "Ship elsewhere"),
        ];
        let result = hub
            .handle(call(
                4,
                Request::Search {
                    query: "  Ship ".to_owned(),
                    limit: Some(1_000),
                },
            ))
            .into_result()
            .expect("a result");
        assert_eq!(
            result,
            json!({ "hits": [{
                "path": "docs/plan.md", "headingPath": ["Plan"], "line": 5, "snippet": "Ship it & go"
            }]})
        );
        let empty = hub.handle(call(
            5,
            Request::Search {
                query: "   ".to_owned(),
                limit: None,
            },
        ));
        assert!(!empty.ok);
        assert_eq!(empty.error.expect("error").kind, ErrorKind::BadRequest);
    }

    #[test]
    fn notes_come_from_the_backend_open_ones_unless_more_is_asked() {
        let (_dir, vault) = project();
        let (hub, fake) = hub(vault);
        let mut closed = note("n3", "docs/plan.md", 7);
        closed.done = true;
        *lock(&fake.notes) = vec![
            note("n2", "docs/plan.md", 7),
            note("n1", "docs/plan.md", 1),
            closed,
        ];
        let ids = |which| {
            let result = hub
                .handle(call(8, Request::ListNotes { which }))
                .into_result()
                .expect("a result");
            result["notes"]
                .as_array()
                .expect("a list")
                .iter()
                .map(|n| n["id"].as_str().expect("id").to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(None), ["n1", "n2"]);
        assert_eq!(ids(Some(NoteFilter::Done)), ["n3"]);
        assert_eq!(ids(Some(NoteFilter::All)), ["n1", "n2", "n3"]);
        let result = hub
            .handle(call(9, Request::ListNotes { which: None }))
            .into_result()
            .expect("a result");
        assert_eq!(result["total"], 2);
        assert_eq!(result["notes"][0]["state"], "anchored");
        assert_eq!(result["notes"][0]["headingPath"], json!(["Plan"]));
    }

    #[test]
    fn a_note_is_resolved_through_the_backend_and_an_unknown_one_is_not_found() {
        let (_dir, vault) = project();
        let (hub, fake) = hub(vault);
        *lock(&fake.notes) = vec![note("n1", "docs/plan.md", 1)];
        let result = hub
            .handle(call(
                10,
                Request::ResolveNote {
                    note: "n1".to_owned(),
                    reply: Some("Rewritten".to_owned()),
                },
            ))
            .into_result()
            .expect("a result");
        assert_eq!(result["done"], true);
        assert_eq!(result["reply"], "Rewritten");
        assert_eq!(
            *lock(&fake.resolved),
            [("n1".to_owned(), Some("Rewritten".to_owned()))]
        );
        let missing = hub.handle(call(
            11,
            Request::ResolveNote {
                note: "nope".to_owned(),
                reply: None,
            },
        ));
        assert_eq!(missing.error.expect("error").kind, ErrorKind::NotFound);
    }

    #[test]
    fn open_waits_for_the_window_and_tells_it_once() {
        let (_dir, vault) = project();
        let (hub, fake) = hub(vault);
        let reply = hub.handle(call(
            6,
            Request::Open {
                path: "docs/plan.md".to_owned(),
                line: None,
                heading: Some("rollout".to_owned()),
            },
        ));
        assert_eq!(
            reply.into_result().expect("a result"),
            json!({ "path": "docs/plan.md", "line": 5 })
        );
        assert_eq!(fake.opens.load(Ordering::SeqCst), 1);
        let place = hub.take_open().expect("a place waits");
        assert_eq!((place.path.as_str(), place.line), ("docs/plan.md", Some(5)));
        assert_eq!(hub.take_open(), None, "it is handed over once");
    }

    #[test]
    fn open_refuses_what_is_not_a_document_of_the_project() {
        let (_dir, vault) = project();
        let (hub, fake) = hub(vault);
        for path in ["../x.md", "docs/nope.md", "/etc/hosts"] {
            let reply = hub.handle(call(
                7,
                Request::Open {
                    path: path.to_owned(),
                    line: None,
                    heading: None,
                },
            ));
            assert_eq!(reply.error.expect("error").kind, ErrorKind::NotFound);
        }
        assert_eq!(fake.opens.load(Ordering::SeqCst), 0);
        assert_eq!(hub.take_open(), None);
    }

    #[test]
    fn the_status_follows_connections_requests_and_the_switch() {
        let (_dir, vault) = project();
        let (hub, fake) = hub(vault);
        assert_eq!(hub.status(), AgentStatus::default());
        hub.set_lifecycle(true, true, None);
        hub.connection_opened();
        hub.handle(call(1, Request::GetContext));
        hub.handle(call(
            2,
            Request::Search {
                query: "plan".to_owned(),
                limit: None,
            },
        ));
        let status = hub.status();
        assert!(status.enabled && status.listening);
        assert_eq!(status.connections, 1);
        let tools: Vec<_> = status.recent.iter().map(|r| r.tool.as_str()).collect();
        assert_eq!(tools, ["search", "get_context"], "newest first");
        assert_eq!(status.recent[0].subject.as_deref(), Some("plan"));
        // Every change reached the interface.
        assert_eq!(lock(&fake.statuses).len(), 4);
        hub.connection_closed();
        hub.connection_closed();
        assert_eq!(hub.connections(), 0, "never below zero");
        hub.set_lifecycle(false, false, None);
        assert!(!hub.status().enabled);
    }

    #[test]
    fn turning_it_off_forgets_who_was_connected() {
        let (_dir, vault) = project();
        let (hub, _) = hub(vault);
        hub.set_lifecycle(true, true, None);
        hub.connection_opened();
        hub.set_lifecycle(true, false, Some("no socket".to_owned()));
        let status = hub.status();
        assert_eq!(status.connections, 0);
        assert_eq!(status.error.as_deref(), Some("no socket"));
    }

    #[test]
    fn only_the_latest_requests_are_kept() {
        let (_dir, vault) = project();
        let (hub, _) = hub(vault);
        for id in 0..(RECENT_REQUESTS as u64 + 3) {
            hub.handle(call(id, Request::GetContext));
        }
        assert_eq!(hub.status().recent.len(), RECENT_REQUESTS);
    }

    #[test]
    fn a_line_that_is_not_a_request_is_answered_with_its_own_id_when_it_has_one() {
        assert_eq!(Hub::reject(r#"{"id":12,"op":"rm"}"#).id, 12);
        assert_eq!(Hub::reject("garbage").id, 0);
        let reply = Hub::reject("[]");
        assert!(!reply.ok);
        assert_eq!(reply.error.expect("error").kind, ErrorKind::BadRequest);
    }
}
