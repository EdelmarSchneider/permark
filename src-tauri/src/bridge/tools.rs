//! What an agent's tools do, with no MCP in it: [`Core`] answers `get_context`, `list_unread`, `search` and `open`
//! for the project an agent works in. It asks the window when there is one, falls back to the project's files for
//! what does not need the window, and says plainly when the connection is off.
//!
//! The MCP server (`mcp`) only translates; so does `trailmark context`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::comments::{CommentFilter, FileProblem};
use crate::notes::share;

use super::link::{Link, LinkError};
use super::project::{connection_is_on, find_project, Project};
use super::protocol::{Hits, NoteList, Request, DEFAULT_SEARCH_HITS, EMPTY_QUERY, MAX_SEARCH_HITS};
use super::socket::Endpoint;
use super::tags::names_a_tag;

/// Shown to the agent (and through it to the person) when the connection is off. One place to change the words.
const OFF_MESSAGE: &str = "The agent connection is off for this project, so TrailMark shares nothing. The person \
can turn it on in TrailMark: Settings > Agent connection (or the command palette: \"Agent connection: turn on\").";
/// The project is not one TrailMark has been used on.
const UNKNOWN_MESSAGE: &str =
    "TrailMark has not been used on this folder. The person can open the project in \
TrailMark and turn on Settings > Agent connection.";
const NOT_OPEN_MESSAGE: &str = "TrailMark is not open on this project.";
const RESOLVED_MESSAGE: &str =
    "Marked done. The person sees it checked off in TrailMark, with your reply under the comment.";
const SHOWN_MESSAGE: &str =
    "Shown in TrailMark. It does not take focus; the person sees a notice with a way back \
and looks when ready.";

/// What `open` says when the person is editing: nothing moved on their screen.
const EDITING_MESSAGE: &str =
    "The person is editing, so TrailMark did not move them to it. It showed a notice with a button \
to the place instead, and they look when they are done or ready.";

/// How long `open` waits for TrailMark to start and listen.
pub const LAUNCH_WAIT: Duration = Duration::from_secs(25);
/// How often it looks while waiting.
const LAUNCH_POLL: Duration = Duration::from_millis(200);
/// After TrailMark was started and did not come up listening, it is not started again for this long. The window may
/// be slow to start, or open and unable to listen: either way, another copy would not help.
const LAUNCH_PAUSE: Duration = Duration::from_secs(120);

/// Starts TrailMark on a project.
pub type Launcher = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

/// An answer to a tool call: a JSON object with a `status` (`ok`, `off`, `notOpen`, `starting`, or `error`).
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub value: Value,
    /// The call failed (as opposed to answering that there is nothing to share).
    pub is_error: bool,
}

impl Answer {
    fn ok(mut fields: Map<String, Value>) -> Self {
        fields.insert("status".to_owned(), json!("ok"));
        Self {
            value: Value::Object(fields),
            is_error: false,
        }
    }

    /// An answer that is not an error: nothing to share, and why.
    fn notice(status: &str, message: &str) -> Self {
        Self {
            value: json!({ "status": status, "message": message }),
            is_error: false,
        }
    }

    pub(crate) fn failed(message: impl Into<String>) -> Self {
        Self {
            value: json!({ "status": "error", "message": message.into() }),
            is_error: true,
        }
    }

    fn off(known: bool) -> Self {
        Self::notice("off", if known { OFF_MESSAGE } else { UNKNOWN_MESSAGE })
    }
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => Map::from_iter([("result".to_owned(), other)]),
    }
}

/// `2026-10-15T09:30:00Z` for a time in milliseconds since the Unix epoch.
pub(crate) fn iso8601_utc(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Adds a readable `modified` time beside each `modifiedMs` of an unread list.
fn with_readable_times(mut list: Map<String, Value>) -> Map<String, Value> {
    if let Some(Value::Array(docs)) = list.get_mut("unread") {
        for doc in docs {
            let ms = doc.get("modifiedMs").and_then(Value::as_u64);
            if let (Some(ms), Some(doc)) = (ms, doc.as_object_mut()) {
                doc.insert("modified".to_owned(), json!(iso8601_utc(ms)));
            }
        }
    }
    list
}

/// Adds a readable `modified` time beside each `modifiedMs` of a list of notes, and beside the `modifiedMs` of a single note.
fn with_readable_note_times(mut value: Map<String, Value>) -> Map<String, Value> {
    fn stamp(note: &mut Value) {
        let ms = note.get("modifiedMs").and_then(Value::as_u64);
        if let (Some(ms), Some(note)) = (ms, note.as_object_mut()) {
            note.insert("modified".to_owned(), json!(iso8601_utc(ms)));
        }
    }
    if let Some(Value::Array(notes)) = value.get_mut("notes") {
        notes.iter_mut().for_each(stamp);
    }
    if let Some(ms) = value.get("modifiedMs").and_then(Value::as_u64) {
        value.insert("modified".to_owned(), json!(iso8601_utc(ms)));
    }
    value
}

/// Adds a readable `created` time beside each `createdMs` of a comment list, and beside the `createdMs` of a single comment.
fn with_readable_comment_times(mut value: Map<String, Value>) -> Map<String, Value> {
    fn stamp(comment: &mut Value) {
        let ms = comment.get("createdMs").and_then(Value::as_u64);
        if let (Some(ms), Some(comment)) = (ms, comment.as_object_mut()) {
            comment.insert("created".to_owned(), json!(iso8601_utc(ms)));
        }
    }
    if let Some(Value::Array(comments)) = value.get_mut("comments") {
        comments.iter_mut().for_each(stamp);
    }
    if let Some(comment) = value.get_mut("comment") {
        stamp(comment);
    }
    value
}

/// Opens the project's files for reading, once, when something needs them.
struct Offline {
    root: PathBuf,
    app_data: Option<PathBuf>,
    project: Mutex<Option<Arc<Project>>>,
}

impl Offline {
    fn get(&self) -> Result<Arc<Project>, String> {
        let mut slot = self.project.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(project) = slot.as_ref() {
            return Ok(Arc::clone(project));
        }
        let app_data = self
            .app_data
            .as_deref()
            .ok_or("the data folder of TrailMark cannot be found")?;
        let project = Arc::new(
            Project::open(&self.root, app_data, dirs::home_dir().as_deref())
                .map_err(|error| error.to_string())?,
        );
        *slot = Some(Arc::clone(&project));
        Ok(project)
    }
}

/// The project an agent works in, and how to reach it.
pub struct Core {
    root: PathBuf,
    app_data: Option<PathBuf>,
    /// The folder is inside a project TrailMark has been used on.
    known: bool,
    link: Result<Link, String>,
    offline: Arc<Offline>,
    launcher: Launcher,
    launch_wait: Duration,
    launch_pause: Duration,
    /// Held while TrailMark is being started and waited for, so that calls made meanwhile wait for that start instead
    /// of making their own.
    launching: tokio::sync::Mutex<()>,
    /// When TrailMark was last started without coming up listening (cleared once it answers).
    failed_launch: Mutex<Option<tokio::time::Instant>>,
}

impl Core {
    /// Finds the project from the folder the agent runs in (the nearest folder above it that TrailMark knows).
    /// `identifier` is the app's, which names its data folder.
    pub(crate) fn discover(identifier: &str, working_folder: &Path, launcher: Launcher) -> Self {
        let app_data = super::project::app_data_dir(identifier);
        let found = app_data
            .as_deref()
            .and_then(|dir| find_project(dir, working_folder));
        let known = found.is_some();
        let root = found.unwrap_or_else(|| {
            dunce::canonicalize(working_folder)
                .unwrap_or_else(|_| dunce::simplified(working_folder).to_path_buf())
        });
        Self::new(root, app_data, known, Endpoint::for_project, launcher)
    }

    pub(crate) fn new<E: std::fmt::Display>(
        root: PathBuf,
        app_data: Option<PathBuf>,
        known: bool,
        endpoint: impl FnOnce(&Path) -> Result<Endpoint, E>,
        launcher: Launcher,
    ) -> Self {
        let link = endpoint(&root)
            .map(Link::new)
            .map_err(|error| error.to_string());
        Self {
            offline: Arc::new(Offline {
                root: root.clone(),
                app_data: app_data.clone(),
                project: Mutex::new(None),
            }),
            root,
            app_data,
            known,
            link,
            launcher,
            launch_wait: LAUNCH_WAIT,
            launch_pause: LAUNCH_PAUSE,
            launching: tokio::sync::Mutex::new(()),
            failed_launch: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_launch_wait(mut self, wait: Duration) -> Self {
        self.launch_wait = wait;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_launch_pause(mut self, pause: Duration) -> Self {
        self.launch_pause = pause;
        self
    }

    /// Whether the person turned the connection on, looked at every time: it can change while an agent runs.
    fn is_on(&self) -> bool {
        self.known
            && self
                .app_data
                .as_deref()
                .is_some_and(|dir| connection_is_on(dir, &self.root))
    }

    /// Connects to the window now, if there is one (so it shows that an agent is connected).
    pub async fn keep_connected(&self) {
        if self.is_on() {
            if let Ok(link) = &self.link {
                link.connect_only().await;
            }
        }
    }

    async fn ask(&self, request: Request) -> Result<Value, LinkError> {
        match &self.link {
            Ok(link) => link.call(request).await,
            Err(_) => Err(LinkError::NotRunning),
        }
    }

    /// Runs work on the project's files off the async thread.
    async fn offline<T, F>(&self, work: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&Project) -> T + Send + 'static,
    {
        let offline = Arc::clone(&self.offline);
        tokio::task::spawn_blocking(move || offline.get().map(|project| work(&project)))
            .await
            .map_err(|error| error.to_string())?
    }

    /// What the person is looking at. Needs the window.
    pub async fn get_context(&self) -> Answer {
        if !self.is_on() {
            return Answer::off(self.known);
        }
        let link = match &self.link {
            Ok(link) => link,
            // No place to look: that is not the same as a window that is closed.
            Err(why) => {
                return Answer::failed(format!("the connection to TrailMark cannot be used: {why}"))
            }
        };
        match self.ask(Request::GetContext).await {
            Ok(context) => Answer::ok(object(context)),
            // Where it looked is said, because a program started with another environment (no runtime folder, a
            // different temporary folder) looks somewhere else than the window listens.
            Err(LinkError::NotRunning) => Answer::notice(
                "notOpen",
                &format!(
                    "{NOT_OPEN_MESSAGE} Its connection was looked for at {}.",
                    link.place()
                ),
            ),
            Err(error) => Answer::failed(error.to_string()),
        }
    }

    /// The documents that changed since the person last read them.
    pub async fn list_unread(&self) -> Answer {
        if !self.is_on() {
            return Answer::off(self.known);
        }
        let live = match self.ask(Request::ListUnread).await {
            Ok(list) => Some(object(list)),
            Err(LinkError::Remote(error)) => return Answer::failed(error.message),
            Err(_) => None,
        };
        let (mut list, source) = if let Some(list) = live {
            (list, "window")
        } else {
            match self
                .offline(|project| serde_json::to_value(project.unread()))
                .await
            {
                Ok(Ok(list)) => (object(list), "files"),
                Ok(Err(error)) => return Answer::failed(error.to_string()),
                Err(error) => return Answer::failed(error),
            }
        };
        list.insert("source".to_owned(), json!(source));
        Answer::ok(with_readable_times(list))
    }

    /// Where something is described: in the documents that have all of `tags` (every document when there are none).
    /// Tags alone, without words, list the documents that have them.
    pub async fn search(&self, query: &str, limit: Option<usize>, tags: &[String]) -> Answer {
        if !self.is_on() {
            return Answer::off(self.known);
        }
        let query = query.trim().to_owned();
        if query.is_empty() && !names_a_tag(tags) {
            return Answer::failed(EMPTY_QUERY);
        }
        // what the number counts depends on the question: sections for words, documents for tags alone
        let hits = limit
            .unwrap_or(DEFAULT_SEARCH_HITS)
            .clamp(1, MAX_SEARCH_HITS);
        let tags = tags.to_vec();
        let request = Request::Search {
            query: query.clone(),
            limit,
            tags: tags.clone(),
        };
        let (mut found, source) = match self.ask(request).await {
            Ok(found) => (object(found), "window"),
            Err(LinkError::Remote(error)) => return Answer::failed(error.message),
            Err(_) => match self
                .offline(move |project| {
                    if query.is_empty() {
                        serde_json::to_value(project.docs().tagged(&tags, limit))
                    } else {
                        serde_json::to_value(Hits {
                            hits: project.search(&query, hits, &tags),
                        })
                    }
                })
                .await
            {
                Ok(Ok(found)) => (object(found), "files"),
                Ok(Err(error)) => return Answer::failed(error.to_string()),
                Err(error) => return Answer::failed(error),
            },
        };
        found.insert("source".to_owned(), json!(source));
        Answer::ok(found)
    }

    /// The tags in use, with how many documents have each; or, with `tag`, the documents that have it. `limit` is how
    /// many of either to list at most.
    pub async fn list_tags(&self, tag: Option<&str>, limit: Option<usize>) -> Answer {
        if !self.is_on() {
            return Answer::off(self.known);
        }
        let tag = tag.map(str::trim).filter(|tag| !tag.is_empty());
        let request = Request::ListTags {
            tag: tag.map(str::to_owned),
            limit,
        };
        let (mut list, source) = match self.ask(request).await {
            Ok(list) => (object(list), "window"),
            Err(LinkError::Remote(error)) => return Answer::failed(error.message),
            Err(_) => {
                let tag = tag.map(str::to_owned);
                match self
                    .offline(move |project| {
                        serde_json::to_value(project.docs().list_tags(tag.as_deref(), limit))
                    })
                    .await
                {
                    Ok(Ok(list)) => (object(list), "files"),
                    Ok(Err(error)) => return Answer::failed(error.to_string()),
                    Err(error) => return Answer::failed(error),
                }
            }
        };
        list.insert("source".to_owned(), json!(source));
        Answer::ok(list)
    }

    /// The comments the person left. The open ones unless asked for more.
    pub async fn list_comments(&self, which: Option<CommentFilter>) -> Answer {
        if !self.is_on() {
            return Answer::off(self.known);
        }
        let which = which.unwrap_or_default();
        let (mut list, source) = match self.ask(Request::ListComments { which: Some(which) }).await
        {
            Ok(list) => (object(list), "window"),
            Err(LinkError::Remote(error)) => return Answer::failed(error.message),
            Err(_) => match self
                .offline(move |project| serde_json::to_value(project.comments(which)))
                .await
            {
                Ok(Ok(list)) => (object(list), "files"),
                Ok(Err(error)) => return Answer::failed(error.to_string()),
                Err(error) => return Answer::failed(error),
            },
        };
        list.insert("source".to_owned(), json!(source));
        // An empty list can be the file's doing; say so rather than let it read as "no comments".
        if list.get("total").and_then(Value::as_u64) == Some(0) {
            if let Ok(Some(problem)) = self.offline(Project::comments_problem).await {
                list.insert("warning".to_owned(), json!(comments_warning(problem)));
            }
        }
        Answer::ok(with_readable_comment_times(list))
    }

    /// Runs work on the notes folder off the async thread: where the person keeps their notes, which no project holds.
    async fn offline_notes<T, F>(&self, work: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(PathBuf) -> T + Send + 'static,
    {
        let app_data = self
            .app_data
            .clone()
            .ok_or("the data folder of TrailMark cannot be found")?;
        tokio::task::spawn_blocking(move || work(share::notes_root(&app_data)))
            .await
            .map_err(|error| error.to_string())
    }

    /// The notes the person shared with agents. Notes are the person's own, not a project's: an agent of any project whose
    /// connection is on may ask, and sees only the notes that were shared. It works from the files when TrailMark is closed.
    pub async fn list_notes(&self) -> Answer {
        if !self.is_on() {
            return Answer::off(self.known);
        }
        let (mut list, source) = match self.ask(Request::ListNotes).await {
            Ok(list) => (object(list), "window"),
            Err(LinkError::Remote(error)) => return Answer::failed(error.message),
            Err(_) => match self
                .offline_notes(|root| {
                    serde_json::to_value(NoteList::new(share::list_shared(&root)))
                })
                .await
            {
                Ok(Ok(list)) => (object(list), "files"),
                Ok(Err(error)) => return Answer::failed(error.to_string()),
                Err(error) => return Answer::failed(error),
            },
        };
        list.insert("source".to_owned(), json!(source));
        Answer::ok(with_readable_note_times(list))
    }

    /// The text of one shared note, by the name `list_notes` gave. A note that is not shared is said not to be there, whether it
    /// exists or not.
    pub async fn read_note(&self, note: &str) -> Answer {
        if !self.is_on() {
            return Answer::off(self.known);
        }
        let note = note.trim().to_owned();
        if note.is_empty() {
            return Answer::failed("the note name is empty");
        }
        let (mut read, source) = match self.ask(Request::ReadNote { note: note.clone() }).await {
            Ok(read) => (object(read), "window"),
            Err(LinkError::Remote(error)) => return Answer::failed(error.message),
            Err(_) => match self
                .offline_notes(move |root| share::read_shared(&root, &note))
                .await
            {
                Ok(Ok(text)) => match serde_json::to_value(text) {
                    Ok(text) => (object(text), "files"),
                    Err(error) => return Answer::failed(error.to_string()),
                },
                Ok(Err(error)) => return Answer::failed(error.to_string()),
                Err(error) => return Answer::failed(error),
            },
        };
        read.insert("source".to_owned(), json!(source));
        Answer::ok(with_readable_note_times(read))
    }

    /// Marks a comment done, with an optional short reply the person sees under it. The only change an agent
    /// can make; it works from the project's files when TrailMark is closed.
    pub async fn resolve_comment(&self, id: &str, reply: Option<&str>) -> Answer {
        if !self.is_on() {
            return Answer::off(self.known);
        }
        let id = id.trim().to_owned();
        if id.is_empty() {
            return Answer::failed("the comment id is empty");
        }
        let reply = reply.map(str::to_owned);
        let request = Request::ResolveComment {
            comment: id.clone(),
            reply: reply.clone(),
        };
        let (comment, source) = match self.ask(request).await {
            Ok(comment) => (comment, "window"),
            Err(LinkError::Remote(error)) => return Answer::failed(error.message),
            // The file is written directly only when there is no window to write it. A window that is there but
            // slow, or that dropped the connection, may be changing the same comments: its answer is what counts.
            Err(LinkError::NotRunning) => {
                match self
                    .offline(move |project| project.resolve_comment(&id, reply.as_deref()))
                    .await
                {
                    Ok(Ok(comment)) => (json!(comment), "files"),
                    Ok(Err(error)) => return Answer::failed(error.to_string()),
                    Err(error) => return Answer::failed(error),
                }
            }
            Err(error) => return Answer::failed(error.to_string()),
        };
        let fields = Map::from_iter([
            ("comment".to_owned(), comment),
            ("source".to_owned(), json!(source)),
            ("message".to_owned(), json!(RESOLVED_MESSAGE)),
        ]);
        Answer::ok(with_readable_comment_times(fields))
    }

    /// Shows the person a place. Starts TrailMark when it is closed.
    pub async fn open(&self, path: &str, line: Option<u32>, heading: Option<&str>) -> Answer {
        if !self.is_on() {
            return Answer::off(self.known);
        }
        let request = Request::Open {
            path: path.to_owned(),
            line,
            heading: heading.map(str::to_owned),
        };
        match self.ask(request.clone()).await {
            Ok(opened) => {
                self.window_answered();
                Answer::ok(shown(opened, false))
            }
            Err(LinkError::Remote(error)) => Answer::failed(error.message),
            Err(LinkError::NotRunning) => self.start_and_open(request).await,
            Err(error) => Answer::failed(error.to_string()),
        }
    }

    /// The window answered: a start that did not work before is forgotten.
    fn window_answered(&self) {
        self.failed_launch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    /// Whether TrailMark was started a short while ago and has not come up since.
    fn launch_failed_recently(&self) -> bool {
        self.failed_launch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|at| at.elapsed() < self.launch_pause)
    }

    /// The window is closed: check the place first (nothing is started for a path that is not a document), start
    /// TrailMark on the project, and hand the place over once it listens.
    ///
    /// One start at a time, also across programs (two `trailmark mcp` at once take turns on a lock file): calls made
    /// while TrailMark is starting wait for that start and use its window, and after a start that did not come up
    /// listening none is made again for a while (see [`LAUNCH_PAUSE`]).
    async fn start_and_open(&self, request: Request) -> Answer {
        let Request::Open {
            path,
            line,
            heading,
        } = &request
        else {
            return Answer::failed("not an open request");
        };
        let (check_path, check_line, check_heading) = (path.clone(), *line, heading.clone());
        let checked = self
            .offline(move |project| {
                project.check_open(&check_path, check_line, check_heading.as_deref())
            })
            .await;
        match checked {
            Err(error) => return Answer::failed(error),
            Ok(Err(error)) => return Answer::failed(error.message),
            Ok(Ok(_)) => {}
        }
        let _launching = self.launching.lock().await;
        // Another call may have started TrailMark while this one waited for its turn.
        match self.ask(request.clone()).await {
            Ok(opened) => {
                self.window_answered();
                return Answer::ok(shown(opened, false));
            }
            Err(LinkError::Remote(error)) => return Answer::failed(error.message),
            Err(_) => {}
        }
        if self.launch_failed_recently() {
            return Answer::notice(
                "starting",
                "TrailMark was started on this project a moment ago, but its agent connection is not up yet. \
Try again in a few seconds. If it stays like this, look at the agent connection in TrailMark's status bar.",
            );
        }
        // The calls of this program are one at a time by now; another program (a second agent, a second `trailmark
        // mcp`) may be starting the window at this moment. It holds the turn until the window listens, and this one
        // then waits for that window instead of starting another.
        let turn = match self.link.as_ref().map(Link::try_launch_lock) {
            Ok(Ok(Some(turn))) => Some(turn),
            Ok(Ok(None)) => return self.wait_for_window(&request).await,
            // The turn cannot be had (no place for the file): start the window as before.
            Ok(Err(_)) | Err(_) => None,
        };
        if turn.is_some() {
            // The program that had the turn before may have finished just before this one took it.
            match self.ask(request.clone()).await {
                Ok(opened) => {
                    self.window_answered();
                    return Answer::ok(shown(opened, false));
                }
                Err(LinkError::Remote(error)) => return Answer::failed(error.message),
                Err(_) => {}
            }
        }
        if let Err(error) = (self.launcher)(&self.root) {
            return Answer::failed(format!("TrailMark could not be started: {error}"));
        }
        *self
            .failed_launch
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(tokio::time::Instant::now());
        let answer = self.wait_for_window(&request).await;
        // The turn is kept until the window answers (or the wait is over).
        drop(turn);
        answer
    }

    /// Hands the place over once the window listens, looking every [`LAUNCH_POLL`] until the launch wait is over.
    async fn wait_for_window(&self, request: &Request) -> Answer {
        let deadline = tokio::time::Instant::now() + self.launch_wait;
        loop {
            match self.ask(request.clone()).await {
                Ok(opened) => {
                    self.window_answered();
                    return Answer::ok(shown(opened, true));
                }
                Err(LinkError::Remote(error)) => return Answer::failed(error.message),
                Err(_) if tokio::time::Instant::now() >= deadline => {
                    return Answer::notice(
                        "starting",
                        "TrailMark was started but is not ready yet. Try again in a few seconds.",
                    )
                }
                Err(_) => tokio::time::sleep(LAUNCH_POLL).await,
            }
        }
    }
}

/// What a list of comments that is empty because of the comments file says about it.
fn comments_warning(problem: FileProblem) -> &'static str {
    match problem {
        FileProblem::Damaged => {
            "The comments file could not be read, so no comments are listed. TrailMark keeps it as it is and \
sets it aside, as comments.unreadable-<time>.json next to it, when the person next leaves a comment."
        }
        FileProblem::SetAside => {
            "The comments file could not be read and was set aside (as comments.unreadable-<time>.json next to \
it), so there are no comments in the new one yet. The person's earlier comments are in the file that was set aside."
        }
        FileProblem::Unreadable => {
            "The comments file could not be read just now, so no comments are listed. Try again in a moment."
        }
    }
}

/// The `open` answer: where the person was taken, and what to expect.
fn shown(opened: Value, started: bool) -> Map<String, Value> {
    let mut fields = object(opened);
    let heading_missing = fields
        .get("headingNotFound")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // The window does not move a person who is editing: it only showed a notice.
    let editing = fields
        .get("editing")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = match (editing, heading_missing) {
        (false, false) => SHOWN_MESSAGE.to_owned(),
        (false, true) => {
            format!("{SHOWN_MESSAGE} The document has no such heading, so it opened at its top.")
        }
        (true, false) => EDITING_MESSAGE.to_owned(),
        (true, true) => {
            format!("{EDITING_MESSAGE} The document has no such heading, so it opens at its top.")
        }
    };
    fields.insert("message".to_owned(), json!(message));
    if started {
        fields.insert("started".to_owned(), json!(true));
    }
    fields
}

/// Where the program is: what an agent's configuration starts (`<program> mcp`). An AppImage runs from a folder that is
/// mounted for that run only; the file the person started is what lasts, and the AppImage runtime names it.
fn program_from(
    appimage: Option<std::ffi::OsString>,
    running: io::Result<PathBuf>,
) -> Option<PathBuf> {
    appimage
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| running.ok())
        .map(|path| dunce::simplified(&path).to_path_buf())
}

/// The program that is running, in the form that can be started again later.
pub(crate) fn program_path() -> Option<PathBuf> {
    program_from(std::env::var_os("APPIMAGE"), std::env::current_exe())
}

/// Starts TrailMark on a project and lets it go its own way.
///
/// In the Microsoft Store build the program is a file inside the package's folder (under `WindowsApps`), which only
/// the package's own processes may start. That holds here: `trailmark mcp` and `trailmark context` are started through
/// the package's alias, so they are packaged processes, and a process started by a packaged process is part of the same
/// package. Starting the program from a process outside the package by that path would be refused, which is why the
/// agent setup names the alias instead (see `distribution::agent_program`).
pub(crate) fn launch_app(project: &Path) -> io::Result<()> {
    use std::process::{Command, Stdio};

    let program =
        program_path().ok_or_else(|| io::Error::other("the program's own path is unknown"))?;
    let mut command = Command::new(program);
    command
        .arg(project)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    // Collects the exit status when it ends, so the finished process is not left behind as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::bridge::context::{AgentContext, ViewMode};
    use crate::bridge::hub::testing::{comment, Fake};
    use crate::bridge::service::AgentBridge;
    use crate::bridge::tags::Doc;
    use crate::comments::{CommentBook, CommentFilter, NewComment};
    use crate::meta::store::Entries;
    use crate::search::SearchHit;
    use crate::session::StateStore;
    use crate::vault::Vault;

    struct Fx {
        app: tempfile::TempDir,
        /// Keeps the project folder alive for the length of the test.
        _project: tempfile::TempDir,
        sockets: tempfile::TempDir,
        root: PathBuf,
        fake: Arc<Fake>,
        bridge: Arc<AgentBridge>,
    }

    /// A project with two documents, an app data folder, and a socket of its own per test (`name`).
    fn fx() -> Fx {
        let app = tempfile::tempdir().expect("app data");
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("docs")).expect("mkdir");
        std::fs::write(
            project.path().join("README.md"),
            "# Home\n\nThe lighthouse.\n",
        )
        .expect("write");
        std::fs::write(
            project.path().join("docs/plan.md"),
            "# Plan\n\nIntro.\n\n## Rollout\n\nShip the beacon.\n",
        )
        .expect("write");
        let sockets = crate::bridge::socket::socket_folder();
        let root = dunce::canonicalize(project.path()).expect("canonical");
        let fake = Arc::new(Fake::default());
        let bridge = Arc::new(AgentBridge::new(
            Arc::new(Vault::open(&root).expect("vault")),
            Endpoint::in_folder(sockets.path(), &root, "tester"),
            None,
            fake.clone(),
            fake.clone(),
        ));
        Fx {
            app,
            _project: project,
            sockets,
            root,
            fake,
            bridge,
        }
    }

    impl Fx {
        fn set_connection(&self, on: bool) {
            StateStore::open(self.app.path(), &self.root)
                .set_agent_connection(on)
                .expect("remember the switch");
        }

        fn core(&self, launcher: Launcher) -> Core {
            let sockets = self.sockets.path().to_path_buf();
            Core::new(
                self.root.clone(),
                Some(self.app.path().to_path_buf()),
                true,
                move |root| Endpoint::in_folder(&sockets, root, "tester"),
                launcher,
            )
            .with_launch_wait(Duration::from_secs(10))
        }
    }

    impl Fx {
        /// The project's endpoint, as another program of the same user finds it.
        fn endpoint(&self) -> Endpoint {
            Endpoint::in_folder(self.sockets.path(), &self.root, "tester").expect("endpoint")
        }
    }

    /// A launcher that must not be used.
    fn no_launch() -> Launcher {
        Arc::new(|_| panic!("TrailMark must not be started"))
    }

    fn status(answer: &Answer) -> &str {
        answer.value["status"].as_str().expect("a status")
    }

    #[tokio::test]
    async fn every_tool_says_the_connection_is_off_and_how_to_turn_it_on() {
        let fx = fx();
        fx.set_connection(false);
        let core = fx.core(no_launch());
        for answer in [
            core.get_context().await,
            core.list_unread().await,
            core.search("lighthouse", None, &[]).await,
            core.list_tags(None, None).await,
            core.list_comments(None).await,
            core.resolve_comment("c1", Some("done")).await,
            core.open("README.md", None, None).await,
        ] {
            assert_eq!(status(&answer), "off", "{answer:?}");
            assert!(!answer.is_error);
            let message = answer.value["message"].as_str().expect("a message");
            assert!(message.contains("Agent connection"), "{message}");
            assert!(message.contains("turn it on"), "{message}");
        }
    }

    #[tokio::test]
    async fn a_folder_trailmark_has_not_been_used_on_is_off_too() {
        let fx = fx();
        let sockets = fx.sockets.path().to_path_buf();
        let core = Core::new(
            fx.root.clone(),
            Some(fx.app.path().to_path_buf()),
            false,
            move |root| Endpoint::in_folder(&sockets, root, "tester"),
            no_launch(),
        );
        let answer = core.get_context().await;
        assert_eq!(status(&answer), "off");
        assert!(answer.value["message"]
            .as_str()
            .expect("a message")
            .contains("not been used"));
    }

    /// A folder that was never opened in TrailMark, with a `.trailmark/` folder that says the connection is on (a
    /// repository can carry one): nothing is shared, and the answer is the same as for any project that is off.
    #[tokio::test]
    async fn what_a_project_folder_says_about_the_connection_does_not_count() {
        let fx = fx();
        let local = fx.root.join(".trailmark").join("local");
        std::fs::create_dir_all(&local).expect("mkdir");
        std::fs::write(local.join("session.json"), r#"{"agentConnection": true}"#).expect("write");
        std::fs::write(local.join("agent-connection.json"), r#"{"enabled": true}"#).expect("write");
        let core = fx.core(no_launch());
        for answer in [
            core.get_context().await,
            core.list_unread().await,
            core.search("beacon", None, &[]).await,
            core.list_tags(Some("adr"), None).await,
            core.list_comments(None).await,
            core.resolve_comment("c1", None).await,
            core.open("README.md", None, None).await,
        ] {
            assert_eq!(status(&answer), "off", "{answer:?}");
        }
        // the person's own switch is what turns it on
        fx.set_connection(true);
        assert_eq!(status(&core.get_context().await), "notOpen");
    }

    #[tokio::test]
    async fn the_setting_is_read_again_on_every_call() {
        let fx = fx();
        let core = fx.core(no_launch());
        fx.set_connection(false);
        assert_eq!(status(&core.get_context().await), "off");
        fx.set_connection(true);
        assert_eq!(status(&core.get_context().await), "notOpen");
        fx.set_connection(false);
        assert_eq!(status(&core.get_context().await), "off");
    }

    #[tokio::test]
    async fn with_the_window_closed_get_context_says_so() {
        let fx = fx();
        fx.set_connection(true);
        let answer = fx.core(no_launch()).get_context().await;
        assert_eq!(status(&answer), "notOpen");
        let message = answer.value["message"].as_str().expect("a message");
        assert!(
            message.starts_with("TrailMark is not open on this project."),
            "{message}"
        );
        // it says where it looked, to tell a closed window from a program that looks somewhere else
        let place = Endpoint::in_folder(fx.sockets.path(), &fx.root, "tester")
            .expect("endpoint")
            .describe();
        assert!(message.contains(&place), "{message}");
        assert!(!answer.is_error);
    }

    #[tokio::test]
    async fn a_socket_that_cannot_be_named_is_not_a_closed_window() {
        let fx = fx();
        fx.set_connection(true);
        let core = Core::new(
            fx.root.clone(),
            Some(fx.app.path().to_path_buf()),
            true,
            |_: &Path| Err::<Endpoint, _>("the path is too long"),
            no_launch(),
        );
        let answer = core.get_context().await;
        assert!(answer.is_error, "{answer:?}");
        assert!(answer.value["message"]
            .as_str()
            .expect("a message")
            .contains("the path is too long"));
    }

    #[tokio::test]
    async fn get_context_is_what_the_window_shows() {
        let fx = fx();
        fx.set_connection(true);
        fx.bridge.set_enabled(true);
        fx.bridge.publish(AgentContext {
            document: Some("docs/plan.md".to_owned()),
            mode: Some(ViewMode::Reading),
            ..AgentContext::default()
        });
        let answer = fx.core(no_launch()).get_context().await;
        assert_eq!(status(&answer), "ok");
        assert_eq!(answer.value["document"], "docs/plan.md");
        assert_eq!(answer.value["mode"], "reading");
        assert_eq!(answer.value["unsaved"], false);
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn with_the_window_closed_unread_and_search_work_from_the_files() {
        let fx = fx();
        fx.set_connection(true);
        // Nothing was read yet but the app saved a baseline in which README.md is another text.
        let state = StateStore::open(fx.app.path(), &fx.root);
        let hash = format!("{:016x}", crate::util::hash_bytes(b"older readme"));
        std::fs::write(state.read_path(), format!("{{\"README.md\": \"{hash}\"}}")).expect("write");
        let core = fx.core(no_launch());

        let unread = core.list_unread().await;
        assert_eq!(status(&unread), "ok", "{unread:?}");
        assert_eq!(unread.value["source"], "files");
        assert_eq!(unread.value["total"], 2);
        let first = &unread.value["unread"][0];
        assert!(first["path"].as_str().is_some());
        assert!(first["modified"].as_str().expect("a time").ends_with('Z'));

        let found = core.search("beacon", Some(3), &[]).await;
        assert_eq!(status(&found), "ok", "{found:?}");
        assert_eq!(found.value["source"], "files");
        assert_eq!(found.value["hits"][0]["path"], "docs/plan.md");
        assert_eq!(found.value["hits"][0]["headingPath"][1], "Rollout");
        assert_eq!(found.value["hits"][0]["line"], 5);
        assert!(found.value["hits"][0]["snippet"]
            .as_str()
            .expect("snippet")
            .contains("beacon"));

        let empty = core.search("  ", None, &[]).await;
        assert!(empty.is_error);
        assert!(empty.value["message"]
            .as_str()
            .expect("a message")
            .contains("tags"));
    }

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_owned()).collect()
    }

    /// README.md is tagged in its front matter; docs/plan.md in TrailMark, with a description.
    fn tag_the_project(fx: &Fx, meta: &Path) -> Vec<u8> {
        std::fs::write(
            fx.root.join("README.md"),
            "---\ntags: [Home, adr]\n---\n# Home\n\nThe lighthouse.\n",
        )
        .expect("write");
        let mut entries = Entries::default();
        entries.set_tags("docs/plan.md", &names(&["adr", "Rollout Plan"]));
        entries.set_description("docs/plan.md", "How the beacon ships.");
        std::fs::create_dir_all(meta.parent().expect("parent")).expect("mkdir");
        std::fs::write(meta, entries.to_toml()).expect("write");
        std::fs::read(meta).expect("read")
    }

    #[tokio::test]
    async fn with_the_window_closed_the_tags_come_from_the_documents_and_the_apps_meta_file() {
        let fx = fx();
        fx.set_connection(true);
        // the project has no `.trailmark/` folder, so the app keeps its tags in its own data
        let meta = StateStore::open(fx.app.path(), &fx.root).meta_path();
        assert!(meta.starts_with(fx.app.path()));
        let before = tag_the_project(&fx, &meta);
        let core = fx.core(no_launch());

        let tags = core.list_tags(None, None).await;
        assert_eq!(status(&tags), "ok", "{tags:?}");
        assert!(!tags.is_error);
        assert_eq!(tags.value["source"], "files");
        assert_eq!(tags.value["documents"], 2);
        assert_eq!(tags.value["untagged"], 0);
        assert!(tags.value.get("more").is_none());
        assert_eq!(
            tags.value["tags"],
            json!([
                {"name": "adr", "count": 2, "from": "both"},
                {"name": "home", "count": 1, "from": "frontMatter"},
                {"name": "rollout-plan", "count": 1, "from": "trailmark"},
            ])
        );
        assert!(tags.value.get("warning").is_none());

        let adr = core.list_tags(Some(" ADR "), None).await;
        assert_eq!(adr.value["source"], "files");
        assert_eq!(adr.value["total"], 2);
        assert!(adr.value.get("more").is_none());
        assert_eq!(
            adr.value["docs"],
            json!([
                {"path": "README.md", "tags": ["adr", "home"]},
                {"path": "docs/plan.md", "description": "How the beacon ships.", "tags": ["adr", "rollout-plan"]},
            ])
        );
        let none = core.list_tags(Some("nothing"), None).await;
        assert!(!none.is_error);
        assert_eq!(none.value["total"], 0);
        assert!(none.value["message"]
            .as_str()
            .expect("a message")
            .contains("list_tags"));
        // a blank tag is no tag: the distribution
        assert_eq!(core.list_tags(Some("  "), None).await.value["documents"], 2);

        // a search names the tags and description of each hit's document, and can be narrowed by them
        let found = core.search("beacon", None, &[]).await;
        assert_eq!(found.value["hits"][0]["path"], "docs/plan.md");
        assert_eq!(
            found.value["hits"][0]["tags"],
            json!(["adr", "rollout-plan"])
        );
        assert_eq!(
            found.value["hits"][0]["description"],
            "How the beacon ships."
        );
        let narrowed = core.search("beacon", None, &names(&["Rollout Plan"])).await;
        assert_eq!(narrowed.value["source"], "files");
        assert_eq!(narrowed.value["hits"].as_array().expect("hits").len(), 1);
        let others = core.search("beacon", None, &names(&["home"])).await;
        assert_eq!(others.value["hits"], json!([]));
        // all of the tags, not any of them
        let both = core
            .search("lighthouse", None, &names(&["adr", "home"]))
            .await;
        assert_eq!(both.value["hits"][0]["path"], "README.md");
        let one_short = core
            .search("lighthouse", None, &names(&["adr", "rollout-plan"]))
            .await;
        assert_eq!(one_short.value["hits"], json!([]));

        // tags without words list the documents that have them
        let listed = core.search("", None, &names(&["adr"])).await;
        assert!(!listed.is_error, "{listed:?}");
        assert_eq!(listed.value["source"], "files");
        assert_eq!(listed.value["total"], 2);
        assert_eq!(listed.value["docs"][1]["path"], "docs/plan.md");
        assert!(listed.value.get("hits").is_none());
        // and neither words nor tags is an error that says what to give
        assert!(core.search("", None, &[]).await.is_error);
        assert!(core.search(" ", None, &names(&[" "])).await.is_error);

        // looking changed nothing
        assert_eq!(std::fs::read(&meta).expect("read"), before);
        assert!(!fx.root.join(".trailmark").exists());
    }

    #[tokio::test]
    async fn the_tags_of_a_project_with_a_trailmark_folder_are_read_from_it_and_from_the_older_file(
    ) {
        let fx = fx();
        fx.set_connection(true);
        std::fs::create_dir_all(fx.root.join(".trailmark")).expect("mkdir");
        let store = StateStore::open(fx.app.path(), &fx.root);
        let meta = store.meta_path();
        assert_eq!(meta, fx.root.join(".trailmark").join("meta.toml"));
        tag_the_project(&fx, &meta);
        // tags added before the folder existed: the window merges them into the project's file when it next opens it
        let older = store.legacy_meta_path().expect("an older file");
        let mut entries = Entries::default();
        entries.set_tags("README.md", &names(&["kept-from-before"]));
        entries.set_description("README.md", "Before the folder.");
        std::fs::create_dir_all(older.parent().expect("parent")).expect("mkdir");
        std::fs::write(&older, entries.to_toml()).expect("write");
        let core = fx.core(no_launch());

        let tags = core.list_tags(Some("kept-from-before"), None).await;
        assert_eq!(tags.value["total"], 1, "{tags:?}");
        assert_eq!(tags.value["docs"][0]["path"], "README.md");
        assert_eq!(tags.value["docs"][0]["description"], "Before the folder.");
        assert_eq!(
            core.list_tags(None, None).await.value["tags"]
                .as_array()
                .map(Vec::len),
            Some(4)
        );
        // the older file is only read
        assert!(older.is_file());
    }

    #[tokio::test]
    async fn a_meta_file_that_cannot_be_read_is_said_beside_the_tags_that_are_left() {
        let fx = fx();
        fx.set_connection(true);
        let meta = StateStore::open(fx.app.path(), &fx.root).meta_path();
        tag_the_project(&fx, &meta);
        std::fs::write(&meta, "<<<<<<< HEAD\n[files.\"a.md\"]\n").expect("a merge conflict");
        let core = fx.core(no_launch());

        let tags = core.list_tags(None, None).await;
        assert_eq!(status(&tags), "ok", "{tags:?}");
        let warning = tags.value["warning"].as_str().expect("a warning");
        assert!(warning.contains("meta.toml can't be read"), "{warning}");
        assert!(warning.contains("front matter"), "{warning}");
        // the front matter's tags are still there
        assert_eq!(
            tags.value["tags"],
            json!([
                {"name": "adr", "count": 1, "from": "frontMatter"},
                {"name": "home", "count": 1, "from": "frontMatter"},
            ])
        );
        let listed = core.search("", None, &names(&["adr"])).await;
        assert!(listed.value["warning"].is_string());
        assert_eq!(listed.value["total"], 1);
    }

    /// Only the Markdown the project keeps counts as a document: the rest is not in the numbers.
    #[tokio::test]
    async fn only_the_documents_search_can_find_are_counted() {
        let fx = fx();
        fx.set_connection(true);
        std::fs::write(fx.root.join("notes.txt"), "---\ntags: [text]\n---\n").expect("write");
        std::fs::write(fx.root.join(".gitignore"), "build/\n").expect("write");
        std::fs::create_dir_all(fx.root.join("build")).expect("mkdir");
        std::fs::write(
            fx.root.join("build/out.md"),
            "---\ntags: [generated]\n---\n# Out\n",
        )
        .expect("write");
        std::fs::create_dir_all(fx.root.join("node_modules/pkg")).expect("mkdir");
        std::fs::write(
            fx.root.join("node_modules/pkg/README.md"),
            "---\ntags: [vendor]\n---\n# Pkg\n",
        )
        .expect("write");
        let core = fx.core(no_launch());
        let tags = core.list_tags(None, None).await;
        assert_eq!(tags.value["documents"], 2, "{tags:?}");
        assert_eq!(tags.value["untagged"], 2);
        assert_eq!(tags.value["tags"], json!([]));
    }

    #[tokio::test]
    async fn with_the_window_open_the_tags_come_from_it() {
        let fx = fx();
        fx.set_connection(true);
        fx.bridge.set_enabled(true);
        *fx.fake.docs.lock().expect("lock") = vec![
            Doc {
                path: "README.md".to_owned(),
                file_tags: names(&["adr"]),
                app_tags: Vec::new(),
                description: "The home.".to_owned(),
            },
            Doc {
                path: "docs/plan.md".to_owned(),
                file_tags: Vec::new(),
                app_tags: names(&["draft"]),
                description: String::new(),
            },
        ];
        *fx.fake.corpus.lock().expect("lock") = vec![SearchHit {
            path: "README.md".to_owned(),
            heading_path: vec!["Home".to_owned()],
            snippet: "The <mark>lighthouse</mark>".to_owned(),
            start_line: 1,
            score: 1.0,
        }];
        let core = fx.core(no_launch());
        let tags = core.list_tags(None, None).await;
        assert_eq!(tags.value["source"], "window", "{tags:?}");
        assert_eq!(tags.value["documents"], 2);
        assert_eq!(tags.value["tags"][0]["name"], "adr");
        let draft = core.list_tags(Some("draft"), None).await;
        assert_eq!(draft.value["source"], "window");
        assert_eq!(draft.value["docs"][0]["path"], "docs/plan.md");
        let found = core.search("lighthouse", None, &names(&["adr"])).await;
        assert_eq!(found.value["source"], "window");
        assert_eq!(found.value["hits"][0]["tags"], json!(["adr"]));
        assert_eq!(found.value["hits"][0]["description"], "The home.");
        let none = core.search("lighthouse", None, &names(&["draft"])).await;
        assert_eq!(none.value["hits"], json!([]));
        let listed = core.search("", None, &names(&["draft"])).await;
        assert_eq!(listed.value["source"], "window");
        assert_eq!(listed.value["total"], 1);
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn with_the_window_open_unread_and_search_come_from_it() {
        let fx = fx();
        fx.set_connection(true);
        fx.bridge.set_enabled(true);
        *fx.fake.unread.lock().expect("lock") = vec!["README.md".to_owned()];
        *fx.fake.corpus.lock().expect("lock") = vec![SearchHit {
            path: "README.md".to_owned(),
            heading_path: vec!["Home".to_owned()],
            snippet: "The <mark>lighthouse</mark>".to_owned(),
            start_line: 1,
            score: 1.0,
        }];
        let core = fx.core(no_launch());
        let unread = core.list_unread().await;
        assert_eq!(unread.value["source"], "window");
        assert_eq!(unread.value["unread"][0]["path"], "README.md");
        let found = core.search("lighthouse", None, &[]).await;
        assert_eq!(found.value["source"], "window");
        assert_eq!(found.value["hits"][0]["snippet"], "The lighthouse");
        fx.bridge.shutdown();
    }

    /// The comments of the project's files, as the window would have written them.
    fn comment_book(fx: &Fx) -> CommentBook {
        let state = StateStore::open(fx.app.path(), &fx.root);
        CommentBook::new(
            Arc::new(Vault::open(&fx.root).expect("vault")),
            state.comments_path(),
        )
    }

    fn new_comment(path: &str, quote: &str, text: &str) -> NewComment {
        NewComment {
            path: path.to_owned(),
            quote: quote.to_owned(),
            text: text.to_owned(),
            heading_path: Vec::new(),
            line: 0,
            end_line: 0,
        }
    }

    /// An empty list that is empty because the file is no good must not read as "the person left no comments".
    #[tokio::test]
    async fn a_comments_file_that_cannot_be_read_is_said_in_the_empty_list() {
        let fx = fx();
        fx.set_connection(true);
        let core = fx.core(no_launch());
        let warning = |answer: &Answer| {
            answer
                .value
                .get("warning")
                .and_then(Value::as_str)
                .map(str::to_owned)
        };

        // no comments and no problem
        let none = core.list_comments(None).await;
        assert_eq!(status(&none), "ok", "{none:?}");
        assert_eq!(none.value["total"], 0);
        assert_eq!(warning(&none), None);

        // a file that is no good
        let path = StateStore::open(fx.app.path(), &fx.root).comments_path();
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "{ not json").expect("write");
        let damaged = core.list_comments(None).await;
        assert_eq!(status(&damaged), "ok", "{damaged:?}");
        assert_eq!(damaged.value["total"], 0);
        assert!(!damaged.is_error);
        let text = warning(&damaged).expect("a warning");
        assert!(text.contains("could not be read"), "{text}");
        assert!(text.contains("unreadable"), "{text}");

        // the window answers the same way: the file is the same
        fx.bridge.set_enabled(true);
        let live = core.list_comments(None).await;
        assert_eq!(live.value["source"], "window");
        assert_eq!(warning(&live), warning(&damaged));
        fx.bridge.shutdown();

        // once the file is set aside (the person's next comment, here a change that writes nothing), the answer says so
        let book = comment_book(&fx);
        assert!(book.resolve("nope", None).is_err());
        assert!(!path.exists());
        let aside = warning(&core.list_comments(None).await).expect("a warning");
        assert!(aside.contains("was set aside"), "{aside}");

        // and a comment saved since is a file again, with comments in it and nothing to warn about
        book.add(new_comment(
            "docs/plan.md",
            "Ship the beacon",
            "Which beacon?",
        ))
        .expect("a comment");
        let healthy = core.list_comments(None).await;
        assert_eq!(healthy.value["total"], 1);
        assert_eq!(warning(&healthy), None);
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn with_the_window_open_comments_come_from_it_and_resolving_reaches_it() {
        let fx = fx();
        fx.set_connection(true);
        fx.bridge.set_enabled(true);
        *fx.fake.comments.lock().expect("lock") = vec![comment("c1", "docs/plan.md", 5)];
        let core = fx.core(no_launch());
        let listed = core.list_comments(None).await;
        assert_eq!(status(&listed), "ok", "{listed:?}");
        assert_eq!(listed.value["source"], "window");
        assert_eq!(listed.value["total"], 1);
        assert_eq!(listed.value["comments"][0]["id"], "c1");
        let created = listed.value["comments"][0]["created"]
            .as_str()
            .expect("a time");
        assert_eq!(created, "2025-10-15T09:30:00Z");

        let resolved = core.resolve_comment(" c1 ", Some("Rewritten")).await;
        assert_eq!(status(&resolved), "ok", "{resolved:?}");
        assert_eq!(resolved.value["source"], "window");
        assert_eq!(resolved.value["comment"]["done"], true);
        assert_eq!(resolved.value["comment"]["reply"], "Rewritten");
        assert!(resolved.value["comment"]["created"].as_str().is_some());
        assert_eq!(fx.fake.resolved.lock().expect("lock").len(), 1);

        // a comment that is done is not in the open list any more
        assert_eq!(core.list_comments(None).await.value["total"], 0);
        let all = core.list_comments(Some(CommentFilter::All)).await;
        assert_eq!(all.value["total"], 1);

        let missing = core.resolve_comment("nope", None).await;
        assert!(missing.is_error, "{missing:?}");
        assert!(core.resolve_comment("   ", None).await.is_error);
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn with_the_window_closed_comments_are_read_and_resolved_in_the_files() {
        let fx = fx();
        fx.set_connection(true);
        let book = comment_book(&fx);
        let added = book
            .add(new_comment(
                "docs/plan.md",
                "Ship the beacon",
                "Which beacon?",
            ))
            .expect("a comment");
        let core = fx.core(no_launch());

        let listed = core.list_comments(None).await;
        assert_eq!(status(&listed), "ok", "{listed:?}");
        assert_eq!(listed.value["source"], "files");
        let first = &listed.value["comments"][0];
        assert_eq!(first["id"], added.id);
        assert_eq!(first["path"], "docs/plan.md");
        assert_eq!(first["quote"], "Ship the beacon");
        assert_eq!(first["text"], "Which beacon?");
        assert_eq!(first["headingPath"], serde_json::json!(["Plan", "Rollout"]));
        assert_eq!(first["line"], 7);
        assert_eq!(first["state"], "anchored");
        assert!(first["created"].as_str().expect("a time").ends_with('Z'));

        let resolved = core.resolve_comment(&added.id, Some("Named it")).await;
        assert_eq!(status(&resolved), "ok", "{resolved:?}");
        assert_eq!(resolved.value["source"], "files");
        assert_eq!(core.list_comments(None).await.value["total"], 0);
        let done = core.list_comments(Some(CommentFilter::Done)).await;
        assert_eq!(done.value["comments"][0]["reply"], "Named it");
        assert_eq!(done.value["comments"][0]["doneBy"], "agent");
        // the window, opened later, finds it checked off
        assert_eq!(book.list(false)[0].reply.as_deref(), Some("Named it"));

        assert!(core.resolve_comment("nope", None).await.is_error);
    }

    /// A window that is there but does not answer (busy, or its connection broke) may be changing the comments itself:
    /// the file is not written around it.
    #[tokio::test]
    async fn a_window_that_does_not_answer_is_not_written_around() {
        let fx = fx();
        fx.set_connection(true);
        let book = comment_book(&fx);
        let added = book
            .add(new_comment(
                "docs/plan.md",
                "Ship the beacon",
                "Which beacon?",
            ))
            .expect("a comment");
        // something listens and hangs up on whoever connects
        let endpoint =
            Endpoint::in_folder(fx.sockets.path(), &fx.root, "tester").expect("endpoint");
        let listener = endpoint.listen().expect("listen");
        let hanging_up = tokio::spawn(async move {
            loop {
                drop(listener.accept().await);
            }
        });
        let answer = fx
            .core(no_launch())
            .resolve_comment(&added.id, Some("Named it"))
            .await;
        hanging_up.abort();
        assert!(answer.is_error, "{answer:?}");
        assert!(
            !book.list(false)[0].done,
            "the comments file was not written around the window"
        );
    }

    #[tokio::test]
    async fn open_is_delivered_to_the_window_without_starting_anything() {
        let fx = fx();
        fx.set_connection(true);
        fx.bridge.set_enabled(true);
        let answer = fx
            .core(no_launch())
            .open("docs/plan.md", None, Some("Rollout"))
            .await;
        assert_eq!(status(&answer), "ok", "{answer:?}");
        assert_eq!(answer.value["path"], "docs/plan.md");
        assert_eq!(answer.value["line"], 5);
        assert!(answer.value.get("started").is_none());
        let place = fx.bridge.take_open().expect("the window was asked");
        assert_eq!((place.path.as_str(), place.line), ("docs/plan.md", Some(5)));
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn open_reports_a_heading_the_document_does_not_have() {
        let fx = fx();
        fx.set_connection(true);
        fx.bridge.set_enabled(true);
        let answer = fx
            .core(no_launch())
            .open("docs/plan.md", None, Some("Nonexistent"))
            .await;
        assert_eq!(status(&answer), "ok");
        assert_eq!(answer.value["headingNotFound"], true);
        assert!(answer.value["message"]
            .as_str()
            .expect("message")
            .contains("no such heading"));
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn open_never_goes_outside_the_project() {
        let fx = fx();
        fx.set_connection(true);
        fx.bridge.set_enabled(true);
        let core = fx.core(no_launch());
        for path in [
            "../elsewhere.md",
            "/etc/hosts",
            "docs/missing.md",
            "C:/Windows/win.ini",
        ] {
            let answer = core.open(path, None, None).await;
            assert!(answer.is_error, "{path}: {answer:?}");
            assert_eq!(status(&answer), "error");
        }
        assert_eq!(fx.bridge.take_open(), None);
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn open_starts_trailmark_when_it_is_closed_and_hands_the_place_over() {
        let fx = fx();
        fx.set_connection(true);
        let started = Arc::new(AtomicUsize::new(0));
        // "Starting TrailMark" brings the window up: its connection listens.
        let launcher: Launcher = {
            let (bridge, started) = (Arc::clone(&fx.bridge), Arc::clone(&started));
            let root = fx.root.clone();
            Arc::new(move |project| {
                assert_eq!(project, root, "TrailMark is started on the project");
                started.fetch_add(1, Ordering::SeqCst);
                bridge.set_enabled(true);
                Ok(())
            })
        };
        let answer = fx.core(launcher).open("docs/plan.md", Some(3), None).await;
        assert_eq!(status(&answer), "ok", "{answer:?}");
        assert_eq!(answer.value["started"], true);
        assert_eq!(started.load(Ordering::SeqCst), 1);
        let place = fx.bridge.take_open().expect("the place was handed over");
        assert_eq!((place.path.as_str(), place.line), ("docs/plan.md", Some(3)));
        fx.bridge.shutdown();
    }

    /// The window only shows a notice to a person who is editing; the agent is told, so it does not report that the
    /// person is looking at the place.
    #[tokio::test]
    async fn open_says_that_only_a_notice_was_shown_when_the_person_is_editing() {
        let fx = fx();
        fx.set_connection(true);
        fx.bridge.set_enabled(true);
        let core = fx.core(no_launch());
        let publish = |mode| {
            fx.bridge.publish(AgentContext {
                document: Some("README.md".to_owned()),
                mode: Some(mode),
                ..AgentContext::default()
            });
        };
        publish(ViewMode::Reading);
        let reading = core.open("docs/plan.md", Some(3), None).await;
        assert_eq!(status(&reading), "ok", "{reading:?}");
        assert!(reading.value.get("editing").is_none());
        assert!(reading.value["message"]
            .as_str()
            .expect("message")
            .starts_with("Shown in TrailMark"));

        for mode in [ViewMode::Visual, ViewMode::Source] {
            publish(mode);
            let editing = core
                .open("docs/plan.md", Some(3), Some("Nonexistent"))
                .await;
            // the status values stay what clients know
            assert_eq!(status(&editing), "ok", "{editing:?}");
            assert!(!editing.is_error);
            assert_eq!(editing.value["editing"], true);
            assert_eq!(editing.value["path"], "docs/plan.md");
            let message = editing.value["message"].as_str().expect("message");
            assert!(message.contains("is editing"), "{message}");
            assert!(message.contains("notice"), "{message}");
            assert!(message.contains("no such heading"), "{message}");
        }
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn nothing_is_started_for_a_path_that_is_not_a_document() {
        let fx = fx();
        fx.set_connection(true);
        let answer = fx
            .core(no_launch())
            .open("../elsewhere.md", None, None)
            .await;
        assert!(answer.is_error);
    }

    #[tokio::test]
    async fn trailmark_that_never_comes_up_is_reported_not_waited_for_forever() {
        let fx = fx();
        fx.set_connection(true);
        let launcher: Launcher = Arc::new(|_| Ok(()));
        let answer = fx
            .core(launcher)
            .with_launch_wait(Duration::ZERO)
            .open("README.md", None, None)
            .await;
        assert_eq!(status(&answer), "starting");
        assert!(!answer.is_error);
    }

    /// A launcher that counts how often it is used, and brings the window up like the real one does.
    fn counting_launcher(fx: &Fx, comes_up: bool) -> (Launcher, Arc<AtomicUsize>) {
        let started = Arc::new(AtomicUsize::new(0));
        let launcher: Launcher = {
            let (bridge, started) = (Arc::clone(&fx.bridge), Arc::clone(&started));
            Arc::new(move |_| {
                started.fetch_add(1, Ordering::SeqCst);
                if comes_up {
                    bridge.set_enabled(true);
                }
                Ok(())
            })
        };
        (launcher, started)
    }

    /// An agent can call tools in parallel (two at once, or two agents in one project).
    #[tokio::test]
    async fn parallel_opens_with_the_window_closed_start_trailmark_once() {
        let fx = fx();
        fx.set_connection(true);
        let (launcher, started) = counting_launcher(&fx, true);
        let core = fx.core(launcher);
        let (a, b, c) = tokio::join!(
            core.open("README.md", None, None),
            core.open("docs/plan.md", Some(3), None),
            core.open("docs/plan.md", None, Some("Rollout")),
        );
        for answer in [&a, &b, &c] {
            assert_eq!(status(answer), "ok", "{answer:?}");
        }
        assert_eq!(started.load(Ordering::SeqCst), 1, "one start for all three");
        fx.bridge.shutdown();
    }

    /// Two agents, or two `trailmark mcp` programs, ask at the same moment while the window is closed: each has its own
    /// `Core`, so only the turn on the file keeps them from starting two windows.
    #[tokio::test]
    async fn parallel_opens_from_separate_programs_start_trailmark_once() {
        let fx = fx();
        fx.set_connection(true);
        let (launcher, started) = counting_launcher(&fx, true);
        let (first, second) = (fx.core(launcher.clone()), fx.core(launcher));
        let (a, b) = tokio::join!(
            first.open("README.md", None, None),
            second.open("docs/plan.md", Some(3), None),
        );
        for answer in [&a, &b] {
            assert_eq!(status(answer), "ok", "{answer:?}");
        }
        assert_eq!(started.load(Ordering::SeqCst), 1, "one start for both");
        fx.bridge.shutdown();
    }

    /// The window is started by the program that has the turn, and the turn is kept until that is over.
    #[tokio::test]
    async fn the_turn_to_start_the_window_is_held_while_it_starts() {
        let fx = fx();
        fx.set_connection(true);
        let held = Arc::new(AtomicUsize::new(0));
        let launcher: Launcher = {
            let (bridge, held, endpoint) =
                (Arc::clone(&fx.bridge), Arc::clone(&held), fx.endpoint());
            Arc::new(move |_| {
                // another program asking now finds the turn taken
                if endpoint.try_launch_lock()?.is_none() {
                    held.fetch_add(1, Ordering::SeqCst);
                }
                bridge.set_enabled(true);
                Ok(())
            })
        };
        let answer = fx.core(launcher).open("README.md", None, None).await;
        assert_eq!(status(&answer), "ok", "{answer:?}");
        assert_eq!(held.load(Ordering::SeqCst), 1);
        // and it is given up afterwards
        assert!(fx.endpoint().try_launch_lock().expect("lock").is_some());
        fx.bridge.shutdown();
    }

    /// Another program is starting the window right now: this one does not start a second, and says that the window is
    /// on its way.
    #[tokio::test]
    async fn a_program_that_finds_another_one_starting_the_window_does_not_start_one() {
        let fx = fx();
        fx.set_connection(true);
        let _other_program = fx
            .endpoint()
            .try_launch_lock()
            .expect("lock")
            .expect("turn");
        let answer = fx
            .core(no_launch())
            .with_launch_wait(Duration::ZERO)
            .open("README.md", None, None)
            .await;
        assert_eq!(status(&answer), "starting", "{answer:?}");
        assert!(!answer.is_error);
    }

    /// ... and when the window of the other program comes up, the place is handed over to it.
    #[tokio::test]
    async fn the_place_is_handed_to_the_window_another_program_started() {
        let fx = fx();
        fx.set_connection(true);
        let other_program = fx
            .endpoint()
            .try_launch_lock()
            .expect("lock")
            .expect("turn");
        let core = fx.core(no_launch());
        let waiting = core.open("docs/plan.md", Some(3), None);
        let window = async {
            // the other program's window comes up and it gives the turn up
            fx.bridge.set_enabled(true);
            drop(other_program);
        };
        let (answer, ()) = tokio::join!(waiting, window);
        assert_eq!(status(&answer), "ok", "{answer:?}");
        let place = fx.bridge.take_open().expect("the place was handed over");
        assert_eq!((place.path.as_str(), place.line), ("docs/plan.md", Some(3)));
        fx.bridge.shutdown();
    }

    /// A TrailMark that was started and does not listen (it failed to, or is slow) is not started again by every call.
    #[tokio::test]
    async fn a_window_that_never_listens_is_not_started_again_by_every_call() {
        let fx = fx();
        fx.set_connection(true);
        let (launcher, started) = counting_launcher(&fx, false);
        let core = fx.core(launcher).with_launch_wait(Duration::ZERO);
        for call in 1..=4 {
            let answer = core.open("README.md", None, None).await;
            assert_eq!(status(&answer), "starting", "call {call}: {answer:?}");
            assert!(!answer.is_error);
            assert_eq!(started.load(Ordering::SeqCst), 1, "call {call}");
        }
        let (a, b) = tokio::join!(
            core.open("README.md", None, None),
            core.open("README.md", None, None)
        );
        assert_eq!((status(&a), status(&b)), ("starting", "starting"));
        assert_eq!(started.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn trailmark_is_started_again_once_the_pause_is_over() {
        let fx = fx();
        fx.set_connection(true);
        let (launcher, started) = counting_launcher(&fx, false);
        let core = fx
            .core(launcher)
            .with_launch_wait(Duration::ZERO)
            .with_launch_pause(Duration::ZERO);
        core.open("README.md", None, None).await;
        core.open("README.md", None, None).await;
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }

    /// The pause is only for a start that did not work: a window that came up and was closed later is started again.
    #[tokio::test]
    async fn a_window_that_worked_and_was_closed_is_started_again_when_asked() {
        let fx = fx();
        fx.set_connection(true);
        let (launcher, started) = counting_launcher(&fx, true);
        let core = fx.core(launcher);
        assert_eq!(status(&core.open("README.md", None, None).await), "ok");
        assert_eq!(started.load(Ordering::SeqCst), 1);
        // the window is delivered to again without starting anything
        assert_eq!(status(&core.open("README.md", None, None).await), "ok");
        assert_eq!(started.load(Ordering::SeqCst), 1);
        // the person closes it
        fx.bridge.shutdown();
        assert_eq!(status(&core.open("README.md", None, None).await), "ok");
        assert_eq!(started.load(Ordering::SeqCst), 2);
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn a_failure_to_start_trailmark_is_an_error_with_the_reason() {
        let fx = fx();
        fx.set_connection(true);
        let launcher: Launcher = Arc::new(|_| Err(io::Error::other("no such program")));
        let answer = fx.core(launcher).open("README.md", None, None).await;
        assert!(answer.is_error);
        assert!(answer.value["message"]
            .as_str()
            .expect("message")
            .contains("no such program"));
    }

    #[tokio::test]
    async fn an_agent_that_stays_connected_shows_in_the_window() {
        let fx = fx();
        fx.set_connection(true);
        fx.bridge.set_enabled(true);
        let core = fx.core(no_launch());
        core.keep_connected().await;
        // The same connection serves the next call (the window has counted it once its first request is answered).
        core.get_context().await;
        assert_eq!(fx.bridge.status().connections, 1);
        core.get_context().await;
        assert_eq!(fx.bridge.status().connections, 1);
        fx.bridge.shutdown();
    }

    #[test]
    fn the_program_is_the_running_file_except_in_an_appimage() {
        let running = || Ok(PathBuf::from("/tmp/.mount_abc/usr/bin/trailmark"));
        assert_eq!(
            program_from(None, running()),
            Some(PathBuf::from("/tmp/.mount_abc/usr/bin/trailmark"))
        );
        // The AppImage runtime names the file that was started; its mount is gone after the run.
        let appimage = if cfg!(windows) {
            "C:\\Apps\\TrailMark.AppImage"
        } else {
            "/home/ada/Apps/TrailMark.AppImage"
        };
        assert_eq!(
            program_from(Some(appimage.into()), running()),
            Some(PathBuf::from(appimage))
        );
        // Something that is not a path to a file is not trusted over the running program.
        assert_eq!(
            program_from(Some("TrailMark.AppImage".into()), running()),
            Some(PathBuf::from("/tmp/.mount_abc/usr/bin/trailmark"))
        );
        assert_eq!(
            program_from(Some("".into()), running()),
            Some(PathBuf::from("/tmp/.mount_abc/usr/bin/trailmark"))
        );
        assert_eq!(program_from(None, Err(io::Error::other("unknown"))), None);
    }

    #[test]
    fn times_are_written_as_utc() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601_utc(1_760_520_600_000), "2025-10-15T09:30:00Z");
        assert_eq!(iso8601_utc(4_102_444_799_999), "2099-12-31T23:59:59Z");
    }
}
