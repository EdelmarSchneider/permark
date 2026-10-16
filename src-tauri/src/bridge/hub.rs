//! The brain of the connection, with no socket and no window in it: what the window last published, who is
//! connected and what they asked, a place waiting to be shown, and the answer to every request.
//!
//! The socket server hands each request line to [`Hub::handle`]. The window publishes through [`Hub::publish`] and
//! picks up a place an agent asked to be shown with [`Hub::take_open`]. What only an open project can do (the live
//! search index, the live unread markers) comes through [`Backend`]; what the interface must hear about goes
//! out through [`AgentEvents`].

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;

use crate::comments::{CommentError, CommentView};
use crate::notes::share::{self, ShareError};
use crate::search::SearchHit;
use crate::util::now_ms;
use crate::vault::Vault;

use super::answers::{self, PrivateTree};
use super::context::{AgentContext, Published, ViewMode};
use super::protocol::{
    Call, ErrorKind, Hits, NoteList, Opened, ProtocolError, Reply, Request, UnreadList,
    DEFAULT_SEARCH_HITS, EMPTY_QUERY, MAX_SEARCH_HITS,
};
use super::tags::{names_a_tag, DocIndex};

/// How many requests the popover can list. One constant to change.
pub const RECENT_REQUESTS: usize = 8;

/// What only the open project can answer.
pub trait Backend: Send + Sync + 'static {
    /// The documents that are unread now.
    fn unread(&self) -> Vec<String>;
    /// The best sections for a query, among the documents `allow` lets through (it is given a document's path).
    fn search(&self, query: &str, limit: usize, allow: &dyn Fn(&str) -> bool) -> Vec<SearchHit>;
    /// The Markdown documents with their tags and descriptions, as the open project keeps them now.
    fn docs(&self) -> DocIndex;
    /// Every comment, with its passage found again in the document as it is now.
    fn comments(&self) -> Vec<CommentView>;
    /// Marks a comment done by an agent.
    fn resolve_comment(&self, id: &str, reply: Option<&str>) -> Result<CommentView, CommentError>;
    /// The folder of the notes, where the notes the person shared are read from (not part of any project).
    fn notes_folder(&self) -> Option<PathBuf>;
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

/// Why the connection could not start, as far as the interface words it in its own language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    /// Another program holds the name the connection listens on.
    InUse,
    /// The folder of the connection's socket is not one only the user can enter.
    FolderNotPrivate,
    /// The socket's path is longer than the system allows.
    PathTooLong,
    /// Something else; only the system's own words say what.
    Other,
}

/// A connection that could not start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartFailure {
    pub kind: FailureKind,
    /// The system's own words, in English: the place and the error, for a tooltip and for logs.
    pub message: String,
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
    /// Why it is not listening, when it was meant to (see [`FailureKind`]).
    pub error: Option<StartFailure>,
    /// The program an agent starts (`<program> mcp`): where this one is, for the setup snippets.
    pub program: Option<String>,
}

#[derive(Debug, Default)]
struct State {
    enabled: bool,
    listening: bool,
    error: Option<StartFailure>,
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
    /// The folder of the notes, when it is inside the project: nothing in it is shown, opened, found or commented on.
    private: Mutex<PrivateTree>,
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
            private: Mutex::new(PrivateTree::default()),
        }
    }

    /// Says which folder of the project is the person's private notes (see [`PrivateTree`]).
    pub fn set_private(&self, private: PrivateTree) {
        *lock(&self.private) = private;
    }

    /// Whether the document `rel` of the project is in the private notes folder.
    fn is_private(&self, rel: &str) -> bool {
        lock(&self.private).covers_path(&self.vault, rel)
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
        let mut context = context.sanitize(&self.vault);
        // a private note the person has open is shown to nobody: it is as if no document were open
        if context
            .document
            .as_deref()
            .is_some_and(|document| self.is_private(document))
        {
            context = AgentContext::default();
        }
        let published = Published {
            context,
            updated_at_ms: now_ms(),
        };
        *lock(&self.latest) = published.clone();
        published
    }

    /// What the window showed last.
    pub fn context(&self) -> Published {
        lock(&self.latest).clone()
    }

    /// The window last said the person is in an editor (Visual or Source) rather than reading.
    fn person_is_editing(&self) -> bool {
        matches!(
            lock(&self.latest).context.mode,
            Some(ViewMode::Visual | ViewMode::Source)
        )
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
    pub fn set_lifecycle(&self, enabled: bool, listening: bool, error: Option<StartFailure>) {
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

    /// Remembers that an agent asked this, for the popover. `subject` says what it was about when that is not what the request
    /// names (a note is told by its title, not by its file name).
    fn note(&self, request: &Request, subject: Option<String>) {
        self.note_tool(request.tool(), subject.or_else(|| request.subject()));
    }

    fn note_tool(&self, tool: &str, subject: Option<String>) {
        let record = RequestRecord {
            tool: tool.to_owned(),
            subject,
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
        // a read of a note is remembered by the note's title, which is known once it was read
        if !matches!(request, Request::ReadNote { .. }) {
            self.note(&request, None);
        }
        match request {
            Request::GetContext => Reply::success(id, json(&self.context())),
            Request::ListUnread => {
                let unread: Vec<String> = self
                    .backend
                    .unread()
                    .into_iter()
                    .filter(|path| !self.is_private(path))
                    .collect();
                let docs = answers::unread_docs(&self.vault, &unread);
                Reply::success(id, json(&UnreadList::new(docs)))
            }
            Request::Search { query, limit, tags } => {
                let query = query.trim();
                if query.is_empty() && !names_a_tag(&tags) {
                    return Reply::failure(
                        id,
                        ProtocolError::new(ErrorKind::BadRequest, EMPTY_QUERY),
                    );
                }
                let docs = self.visible_docs();
                // no words: the documents that have the tags
                if query.is_empty() {
                    return Reply::success(id, json(&docs.tagged(&tags, limit)));
                }
                let limit = limit
                    .unwrap_or(DEFAULT_SEARCH_HITS)
                    .clamp(1, MAX_SEARCH_HITS);
                let found: Vec<SearchHit> = self
                    .backend
                    .search(query, limit, &*docs.allowing(&tags))
                    .into_iter()
                    .filter(|hit| !self.is_private(&hit.path))
                    .collect();
                let hits = docs.describe(answers::plain_hits(found));
                Reply::success(id, json(&Hits { hits }))
            }
            Request::ListTags { tag, limit } => Reply::success(
                id,
                json(&self.visible_docs().list_tags(tag.as_deref(), limit)),
            ),
            Request::ListComments { which } => {
                let list =
                    answers::comment_list(self.visible_comments(), which.unwrap_or_default());
                Reply::success(id, json(&list))
            }
            Request::ResolveComment { comment, reply } => {
                // a comment on a private note is a comment that is not there
                if !self
                    .visible_comments()
                    .iter()
                    .any(|known| known.id == comment)
                {
                    return Reply::failure(
                        id,
                        ProtocolError::new(
                            ErrorKind::NotFound,
                            CommentError::NotFound(comment).to_string(),
                        ),
                    );
                }
                match self.backend.resolve_comment(&comment, reply.as_deref()) {
                    Ok(view) => Reply::success(id, json(&view)),
                    Err(error @ CommentError::NotFound(_)) => Reply::failure(
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
            } => self.open(id, &path, line, heading.as_deref()),
            Request::ListNotes => {
                let shared = self
                    .backend
                    .notes_folder()
                    .map(|root| share::list_shared(&root))
                    .unwrap_or_default();
                Reply::success(id, json(&NoteList::new(shared)))
            }
            Request::ReadNote { note } => self.read_note(id, &note),
        }
    }

    /// An agent reads a note the person shared. The popover tells the note by its title when it was read, and by the name asked for
    /// when it was not; a note that is not shared is a note that is not there, and nothing is said of what exists.
    fn read_note(&self, id: u64, note: &str) -> Reply {
        let read = self
            .backend
            .notes_folder()
            .map(|root| share::read_shared(&root, note));
        let subject = match &read {
            Some(Ok(text)) if !text.title.is_empty() => text.title.clone(),
            _ => note.trim().to_owned(),
        };
        self.note_tool("read_note", Some(subject.chars().take(80).collect()));
        match read {
            Some(Ok(text)) => Reply::success(id, json(&text)),
            Some(Err(error @ (ShareError::TooLarge | ShareError::NotText))) => Reply::failure(
                id,
                ProtocolError::new(ErrorKind::Unavailable, error.to_string()),
            ),
            Some(Err(ShareError::NotShared)) | None => Reply::failure(
                id,
                ProtocolError::new(ErrorKind::NotFound, ShareError::NotShared.to_string()),
            ),
        }
    }

    /// An agent asks to be shown a place: it is checked (a private note is a path that does not exist), and the window picks it up.
    fn open(&self, id: u64, path: &str, line: Option<u32>, heading: Option<&str>) -> Reply {
        let checked =
            answers::prepare_open_hiding(&self.vault, &lock(&self.private), path, line, heading);
        match checked {
            Ok(opened) => {
                *lock(&self.pending_open) = Some(opened.clone());
                self.events.open_requested();
                let mut shown = json(&opened);
                // The window does not take a person away from what they are writing: it shows a notice with a way
                // to the place instead, and the agent should know that nothing moved on the screen.
                if self.person_is_editing() {
                    if let Some(fields) = shown.as_object_mut() {
                        fields.insert("editing".to_owned(), serde_json::Value::Bool(true));
                    }
                }
                Reply::success(id, shown)
            }
            Err(error) => Reply::failure(id, error),
        }
    }

    /// The documents of the project that are not private notes, with their tags and descriptions.
    fn visible_docs(&self) -> DocIndex {
        let mut docs = self.backend.docs();
        docs.retain(|doc| !self.is_private(&doc.path));
        docs
    }

    /// The comments of the project that are not on a private note.
    fn visible_comments(&self) -> Vec<CommentView> {
        self.backend
            .comments()
            .into_iter()
            .filter(|comment| !self.is_private(&comment.path))
            .collect()
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
    use crate::bridge::tags::Doc;

    /// A project's answers from lists, and a record of what the interface was told.
    #[derive(Default)]
    pub struct Fake {
        pub unread: Mutex<Vec<String>>,
        pub corpus: Mutex<Vec<SearchHit>>,
        pub comments: Mutex<Vec<CommentView>>,
        pub resolved: Mutex<Vec<(String, Option<String>)>>,
        pub statuses: Mutex<Vec<AgentStatus>>,
        pub opens: AtomicUsize,
        /// How many times the unread list was asked for (counted before it waits for its lock).
        pub unread_asked: AtomicUsize,
        /// Where the notes are (none until a test makes a folder of notes).
        pub notes: Mutex<Option<PathBuf>>,
        /// The documents with their tags.
        pub docs: Mutex<Vec<Doc>>,
    }

    impl Backend for Fake {
        fn notes_folder(&self) -> Option<PathBuf> {
            lock(&self.notes).clone()
        }

        fn unread(&self) -> Vec<String> {
            self.unread_asked.fetch_add(1, Ordering::SeqCst);
            lock(&self.unread).clone()
        }

        fn search(
            &self,
            query: &str,
            limit: usize,
            allow: &dyn Fn(&str) -> bool,
        ) -> Vec<SearchHit> {
            lock(&self.corpus)
                .iter()
                .filter(|hit| hit.snippet.contains(query) && allow(&hit.path))
                .take(limit)
                .cloned()
                .collect()
        }

        fn docs(&self) -> DocIndex {
            DocIndex::new(lock(&self.docs).clone(), None)
        }

        fn comments(&self) -> Vec<CommentView> {
            lock(&self.comments).clone()
        }

        fn resolve_comment(
            &self,
            id: &str,
            reply: Option<&str>,
        ) -> Result<CommentView, CommentError> {
            let mut comments = lock(&self.comments);
            let comment = comments
                .iter_mut()
                .find(|comment| comment.id == id)
                .ok_or_else(|| CommentError::NotFound(id.to_owned()))?;
            comment.done = true;
            comment.reply = reply.map(str::to_owned);
            lock(&self.resolved).push((id.to_owned(), reply.map(str::to_owned)));
            Ok(comment.clone())
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

    /// An open comment on a passage of `path`, as the window lists it.
    pub fn comment(id: &str, path: &str, line: u32) -> CommentView {
        CommentView {
            id: id.to_owned(),
            path: path.to_owned(),
            text: format!("comment {id}"),
            quote: "Ship it".to_owned(),
            heading_path: vec!["Plan".to_owned()],
            line,
            end_line: line,
            created_ms: 1_760_520_600_000,
            state: crate::comments::PassageState::Anchored,
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

    use super::testing::{comment, hub};
    use super::*;
    use crate::bridge::context::ViewMode;
    use crate::bridge::tags::Doc;
    use crate::comments::CommentFilter;

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
                    tags: Vec::new(),
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
                tags: s(&[" "]),
            },
        ));
        assert!(!empty.ok);
        assert_eq!(empty.error.expect("error").kind, ErrorKind::BadRequest);
    }

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_owned()).collect()
    }

    fn doc(path: &str, file: &[&str], app: &[&str], description: &str) -> Doc {
        Doc {
            path: path.to_owned(),
            file_tags: s(file),
            app_tags: s(app),
            description: description.to_owned(),
        }
    }

    fn search(query: &str, tags: &[&str]) -> Request {
        Request::Search {
            query: query.to_owned(),
            limit: None,
            tags: s(tags),
        }
    }

    /// Three documents that all mention "retry", two of them tagged.
    fn tagged_project() -> (tempfile::TempDir, Arc<Hub>, Arc<testing::Fake>) {
        let (dir, vault) = project();
        let (hub, fake) = hub(vault);
        *lock(&fake.corpus) = vec![
            hit("docs/loud.md", "retry retry retry"),
            hit("docs/adr-1.md", "we agreed on a retry policy"),
            hit("docs/adr-2.md", "retry with backoff"),
        ];
        *lock(&fake.docs) = vec![
            doc("docs/loud.md", &[], &[], ""),
            doc("docs/adr-1.md", &["adr"], &["api"], "Retry policy."),
            doc("docs/adr-2.md", &["adr"], &[], ""),
        ];
        (dir, hub, fake)
    }

    #[test]
    fn a_search_names_the_tags_and_description_of_the_document_each_hit_is_in() {
        let (_dir, hub, _fake) = tagged_project();
        let result = hub
            .handle(call(1, search("retry", &[])))
            .into_result()
            .expect("a result");
        let hits = result["hits"].as_array().expect("hits");
        assert_eq!(hits.len(), 3);
        let of = |path: &str| {
            hits.iter()
                .find(|h| h["path"] == path)
                .expect("a hit")
                .clone()
        };
        assert_eq!(of("docs/adr-1.md")["tags"], json!(["adr", "api"]));
        assert_eq!(of("docs/adr-1.md")["description"], "Retry policy.");
        assert_eq!(of("docs/adr-2.md")["tags"], json!(["adr"]));
        // what a document does not have is not said
        assert!(of("docs/adr-2.md").get("description").is_none());
        assert!(of("docs/loud.md").get("tags").is_none());
        assert!(of("docs/loud.md").get("description").is_none());
    }

    #[test]
    fn a_search_with_tags_looks_only_in_the_documents_that_have_all_of_them() {
        let (_dir, hub, _fake) = tagged_project();
        let paths = |tags: &[&str]| -> Vec<String> {
            hub.handle(call(2, search("retry", tags)))
                .into_result()
                .expect("a result")["hits"]
                .as_array()
                .expect("hits")
                .iter()
                .map(|h| h["path"].as_str().expect("path").to_owned())
                .collect()
        };
        assert_eq!(paths(&["adr"]), ["docs/adr-1.md", "docs/adr-2.md"]);
        assert_eq!(paths(&["ADR", "api"]), ["docs/adr-1.md"]);
        assert_eq!(paths(&["adr", "nope"]), Vec::<String>::new());
        // a tag that says nothing narrows nothing
        assert_eq!(paths(&["  "]).len(), 3);
    }

    #[test]
    fn tags_without_words_list_the_documents_that_have_them() {
        let (_dir, hub, _fake) = tagged_project();
        let result = hub
            .handle(call(3, search("  ", &["adr"])))
            .into_result()
            .expect("a result");
        assert!(result.get("hits").is_none(), "{result}");
        assert_eq!(result["total"], 2);
        assert_eq!(
            result["docs"][0],
            json!({"path": "docs/adr-1.md", "description": "Retry policy.", "tags": ["adr", "api"]})
        );
        assert_eq!(result["docs"][1]["path"], "docs/adr-2.md");
        // the popover tells it the way the search box takes tags
        let status = hub.status();
        assert_eq!(status.recent[0].tool, "search");
        assert_eq!(status.recent[0].subject.as_deref(), Some("tag:adr"));
    }

    #[test]
    fn a_search_with_neither_words_nor_tags_is_refused_and_says_what_to_give() {
        let (_dir, hub, _fake) = tagged_project();
        let refused = hub.handle(call(4, search("", &[])));
        assert!(!refused.ok);
        let error = refused.error.expect("error");
        assert_eq!(error.kind, ErrorKind::BadRequest);
        assert!(error.message.contains("tags"), "{}", error.message);
    }

    #[test]
    fn list_tags_answers_the_distribution_then_the_documents_of_a_tag() {
        let (_dir, hub, _fake) = tagged_project();
        let overview = hub
            .handle(call(
                5,
                Request::ListTags {
                    tag: None,
                    limit: None,
                },
            ))
            .into_result()
            .expect("a result");
        assert_eq!(overview["documents"], 3);
        assert_eq!(overview["untagged"], 1);
        assert_eq!(
            overview["tags"],
            json!([
                {"name": "adr", "count": 2, "from": "frontMatter"},
                {"name": "api", "count": 1, "from": "trailmark"},
            ])
        );
        let adr = hub
            .handle(call(
                6,
                Request::ListTags {
                    tag: Some("ADR".to_owned()),
                    limit: None,
                },
            ))
            .into_result()
            .expect("a result");
        assert_eq!(adr["total"], 2);
        assert_eq!(adr["docs"][1]["path"], "docs/adr-2.md");
        let status = hub.status();
        assert_eq!(status.recent[0].tool, "list_tags");
        assert_eq!(status.recent[0].subject.as_deref(), Some("ADR"));
        assert_eq!(status.recent[1].subject, None);
    }

    #[test]
    fn the_private_notes_folder_is_not_in_the_tags_nor_in_the_filtered_search() {
        let (dir, vault) = project();
        std::fs::create_dir_all(dir.path().join("notes")).expect("mkdir");
        std::fs::write(dir.path().join("notes/secret.md"), "# Secret\n").expect("write");
        let (hub, fake) = hub(vault);
        hub.set_private(PrivateTree::at(
            dunce::canonicalize(dir.path().join("notes")).expect("canonical"),
        ));
        *lock(&fake.corpus) = vec![
            hit("notes/secret.md", "retry in secret"),
            hit("docs/plan.md", "retry in the plan"),
        ];
        *lock(&fake.docs) = vec![
            doc("notes/secret.md", &["adr"], &["pelican"], "Secret."),
            doc("docs/plan.md", &["adr"], &[], ""),
        ];
        let overview = hub
            .handle(call(
                7,
                Request::ListTags {
                    tag: None,
                    limit: None,
                },
            ))
            .into_result()
            .expect("a result");
        assert_eq!(overview["documents"], 1);
        assert_eq!(
            overview["tags"],
            json!([{"name": "adr", "count": 1, "from": "frontMatter"}])
        );
        let pelican = hub
            .handle(call(
                8,
                Request::ListTags {
                    tag: Some("pelican".to_owned()),
                    limit: None,
                },
            ))
            .into_result()
            .expect("a result");
        assert_eq!(pelican["total"], 0);
        let listed = hub
            .handle(call(9, search("", &["adr"])))
            .into_result()
            .expect("a result");
        assert_eq!(listed["total"], 1);
        assert_eq!(listed["docs"][0]["path"], "docs/plan.md");
        let found = hub
            .handle(call(10, search("retry", &["adr"])))
            .into_result()
            .expect("a result");
        assert_eq!(found["hits"].as_array().expect("hits").len(), 1);
        assert_eq!(found["hits"][0]["path"], "docs/plan.md");
    }

    #[test]
    fn comments_come_from_the_backend_open_ones_unless_more_is_asked() {
        let (_dir, vault) = project();
        let (hub, fake) = hub(vault);
        let mut closed = comment("c3", "docs/plan.md", 7);
        closed.done = true;
        *lock(&fake.comments) = vec![
            comment("c2", "docs/plan.md", 7),
            comment("c1", "docs/plan.md", 1),
            closed,
        ];
        let ids = |which| {
            let result = hub
                .handle(call(8, Request::ListComments { which }))
                .into_result()
                .expect("a result");
            result["comments"]
                .as_array()
                .expect("a list")
                .iter()
                .map(|n| n["id"].as_str().expect("id").to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(None), ["c1", "c2"]);
        assert_eq!(ids(Some(CommentFilter::Done)), ["c3"]);
        assert_eq!(ids(Some(CommentFilter::All)), ["c1", "c2", "c3"]);
        let result = hub
            .handle(call(9, Request::ListComments { which: None }))
            .into_result()
            .expect("a result");
        assert_eq!(result["total"], 2);
        assert_eq!(result["comments"][0]["state"], "anchored");
        assert_eq!(result["comments"][0]["headingPath"], json!(["Plan"]));
    }

    #[test]
    fn a_comment_is_resolved_through_the_backend_and_an_unknown_one_is_not_found() {
        let (_dir, vault) = project();
        let (hub, fake) = hub(vault);
        *lock(&fake.comments) = vec![comment("c1", "docs/plan.md", 1)];
        let result = hub
            .handle(call(
                10,
                Request::ResolveComment {
                    comment: "c1".to_owned(),
                    reply: Some("Rewritten".to_owned()),
                },
            ))
            .into_result()
            .expect("a result");
        assert_eq!(result["done"], true);
        assert_eq!(result["reply"], "Rewritten");
        assert_eq!(
            *lock(&fake.resolved),
            [("c1".to_owned(), Some("Rewritten".to_owned()))]
        );
        let missing = hub.handle(call(
            11,
            Request::ResolveComment {
                comment: "nope".to_owned(),
                reply: None,
            },
        ));
        assert_eq!(missing.error.expect("error").kind, ErrorKind::NotFound);
    }

    /// A folder of notes: `shared one.md` is shared, `private.md` is not.
    fn notes() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join(".trailmark")).expect("mkdir");
        std::fs::write(
            dir.path().join("shared one.md"),
            "# Server IPs\napi 10.0.0.4\n",
        )
        .expect("write");
        std::fs::write(dir.path().join("private.md"), "# Private\n").expect("write");
        std::fs::write(
            dir.path().join(".trailmark").join("notes.json"),
            r#"{"shared": ["shared one.md"]}"#,
        )
        .expect("notes.json");
        dir
    }

    #[test]
    fn shared_notes_are_listed_and_read_and_the_popover_tells_the_note_by_its_title() {
        let (_dir, vault) = project();
        let (hub, fake) = hub(vault);
        let notes = notes();
        *lock(&fake.notes) = Some(notes.path().to_path_buf());

        let reply = hub.handle(call(1, Request::ListNotes));
        assert!(reply.ok, "{reply:?}");
        let list = reply.result.expect("a list");
        assert_eq!(list["total"], 1);
        assert_eq!(list["notes"][0]["note"], "shared one.md");
        assert_eq!(list["notes"][0]["title"], "Server IPs");

        let reply = hub.handle(call(
            2,
            Request::ReadNote {
                note: "shared one.md".to_owned(),
            },
        ));
        assert_eq!(
            reply.result.expect("text")["text"],
            "# Server IPs\napi 10.0.0.4\n"
        );
        let recent = hub.status().recent;
        assert_eq!(recent[0].tool, "read_note");
        assert_eq!(recent[0].subject.as_deref(), Some("Server IPs"));
        assert_eq!(recent[1].tool, "list_notes");
        assert_eq!(recent[1].subject, None);
    }

    #[test]
    fn a_note_that_is_not_shared_is_not_found_whatever_is_true_of_it() {
        let (_dir, vault) = project();
        let (hub, fake) = hub(vault);
        let notes = notes();
        *lock(&fake.notes) = Some(notes.path().to_path_buf());
        for name in [
            "private.md",
            "nothing.md",
            "../private.md",
            ".trailmark/notes.json",
        ] {
            let reply = hub.handle(call(
                1,
                Request::ReadNote {
                    note: name.to_owned(),
                },
            ));
            assert!(!reply.ok, "{name}");
            let error = reply.error.expect("an error");
            assert_eq!(error.kind, ErrorKind::NotFound, "{name}");
            assert!(
                !error.message.contains("private"),
                "{name}: {}",
                error.message
            );
        }
        // the popover still says what was asked
        assert_eq!(
            hub.status().recent[0].subject.as_deref(),
            Some(".trailmark/notes.json")
        );
    }

    #[test]
    fn without_a_notes_folder_nothing_is_shared() {
        let (_dir, vault) = project();
        let (hub, _fake) = hub(vault);
        let reply = hub.handle(call(1, Request::ListNotes));
        assert_eq!(reply.result.expect("a list")["total"], 0);
        let reply = hub.handle(call(
            2,
            Request::ReadNote {
                note: "shared one.md".to_owned(),
            },
        ));
        assert_eq!(reply.error.expect("an error").kind, ErrorKind::NotFound);
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

    /// A person who is editing is not taken away from the work: the window shows a notice, and the agent is told so.
    #[test]
    fn open_tells_the_agent_when_the_person_is_editing() {
        use crate::bridge::context::ViewMode;

        let (_dir, vault) = project();
        let (hub, _) = hub(vault);
        let open = || {
            hub.handle(call(
                8,
                Request::Open {
                    path: "docs/plan.md".to_owned(),
                    line: Some(3),
                    heading: None,
                },
            ))
            .into_result()
            .expect("a result")
        };
        // nothing published yet, and reading: the window goes there
        assert_eq!(open(), json!({ "path": "docs/plan.md", "line": 3 }));
        hub.publish(AgentContext {
            document: Some("docs/plan.md".to_owned()),
            mode: Some(ViewMode::Reading),
            ..AgentContext::default()
        });
        assert_eq!(open(), json!({ "path": "docs/plan.md", "line": 3 }));
        for mode in [ViewMode::Visual, ViewMode::Source] {
            hub.publish(AgentContext {
                document: Some("docs/plan.md".to_owned()),
                mode: Some(mode),
                ..AgentContext::default()
            });
            assert_eq!(
                open(),
                json!({ "path": "docs/plan.md", "line": 3, "editing": true }),
                "{mode:?}"
            );
        }
        // the place that waits for the window is the same either way
        assert_eq!(hub.take_open().expect("a place").line, Some(3));
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

    /// The notes folder can lie inside a project. What an agent asks about it is answered as if it were not there: not opened (and
    /// no word on its headings), not the open document, not commented on, not listed and not found.
    #[test]
    fn the_notes_folder_inside_the_project_is_answered_as_a_path_that_does_not_exist() {
        use super::super::answers::PrivateTree;
        let (dir, vault) = project();
        std::fs::create_dir_all(dir.path().join("notes")).expect("mkdir");
        std::fs::write(
            dir.path().join("notes/n1.md"),
            "# Private\n\n## Secret heading\n\nthe pelican\n",
        )
        .expect("write");
        let (hub, fake) = hub(vault);
        let notes = dunce::canonicalize(dir.path().join("notes")).expect("canonical");
        hub.set_private(PrivateTree::at(notes));

        let asked = |hub: &Hub, path: &str, heading: Option<&str>| {
            let reply = hub.handle(call(
                7,
                Request::Open {
                    path: path.to_owned(),
                    line: Some(2),
                    heading: heading.map(str::to_owned),
                },
            ));
            let error = reply.error.expect("it is refused");
            (error.kind, error.message.replace(path, "<path>"))
        };
        // the same answer as for a note that is not there, with or without a heading to find out about
        let missing = asked(&hub, "notes/zzz.md", Some("Secret heading"));
        assert_eq!(missing.0, ErrorKind::NotFound);
        for path in [
            "notes/n1.md",
            "NOTES\\n1.md",
            "./notes/n1.md",
            "notes/../notes/n1.md",
        ] {
            assert_eq!(asked(&hub, path, Some("Secret heading")), missing, "{path}");
            assert_eq!(asked(&hub, path, Some("Not a heading")), missing, "{path}");
            assert_eq!(asked(&hub, path, None), missing, "{path}");
        }
        assert_eq!(fake.opens.load(Ordering::SeqCst), 0);
        assert_eq!(hub.take_open(), None);
        // the rest of the project still opens
        assert!(
            hub.handle(call(
                8,
                Request::Open {
                    path: "docs/plan.md".to_owned(),
                    line: None,
                    heading: None
                }
            ))
            .ok
        );

        // the person looking at a note shows an agent nothing at all
        hub.publish(AgentContext {
            document: Some("notes/n1.md".to_owned()),
            mode: Some(ViewMode::Source),
            unsaved: true,
            ..AgentContext::default()
        });
        assert_eq!(hub.context().context, AgentContext::default());

        // a comment on a note is not listed, and cannot be resolved by its id
        *lock(&fake.comments) = vec![
            comment("c1", "notes/n1.md", 3),
            comment("c2", "docs/plan.md", 5),
        ];
        let listed = hub
            .handle(call(9, Request::ListComments { which: None }))
            .into_result()
            .expect("a result");
        let ids: Vec<_> = listed["comments"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|c| c["id"].as_str().expect("id").to_owned())
            .collect();
        assert_eq!(ids, ["c2"]);
        let refused = hub.handle(call(
            10,
            Request::ResolveComment {
                comment: "c1".to_owned(),
                reply: None,
            },
        ));
        assert_eq!(refused.error.expect("refused").kind, ErrorKind::NotFound);
        assert!(lock(&fake.resolved).is_empty());

        // and what the backend lists as unread or finds is held back too
        *lock(&fake.unread) = vec!["notes/n1.md".to_owned(), "docs/plan.md".to_owned()];
        let unread = hub
            .handle(call(11, Request::ListUnread))
            .into_result()
            .expect("a result");
        assert_eq!(unread["total"], 1);
        *lock(&fake.corpus) = vec![
            hit("notes/n1.md", "the pelican"),
            hit("docs/plan.md", "the pelican"),
        ];
        let found = hub
            .handle(call(12, search("pelican", &[])))
            .into_result()
            .expect("a result");
        assert_eq!(found["hits"].as_array().expect("hits").len(), 1);
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
                tags: Vec::new(),
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
        let failure = StartFailure {
            kind: FailureKind::InUse,
            message: "no socket".to_owned(),
        };
        hub.set_lifecycle(true, false, Some(failure.clone()));
        let status = hub.status();
        assert_eq!(status.connections, 0);
        assert_eq!(status.error, Some(failure));
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
