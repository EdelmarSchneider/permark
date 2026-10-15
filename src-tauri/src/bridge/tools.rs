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

use super::link::{Link, LinkError};
use super::project::{connection_is_on, find_project, Project};
use super::protocol::{Request, DEFAULT_SEARCH_HITS, MAX_SEARCH_HITS};
use super::socket::Endpoint;

/// Shown to the agent (and through it to the person) when the connection is off. One place to change the words.
const OFF_MESSAGE: &str = "The agent connection is off for this project, so TrailMark shares nothing. The person \
can turn it on in TrailMark: Settings > Agent connection (or the command palette: \"Agent connection: turn on\").";
/// The project is not one TrailMark has been used on.
const UNKNOWN_MESSAGE: &str =
    "TrailMark has not been used on this folder. The person can open the project in \
TrailMark and turn on Settings > Agent connection.";
const NOT_OPEN_MESSAGE: &str = "TrailMark is not open on this project.";
const SHOWN_MESSAGE: &str =
    "Shown in TrailMark. It does not take focus; the person sees a notice with a way back \
and looks when ready.";

/// How long `open` waits for TrailMark to start and listen.
pub const LAUNCH_WAIT: Duration = Duration::from_secs(25);
/// How often it looks while waiting.
const LAUNCH_POLL: Duration = Duration::from_millis(200);

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

    fn failed(message: impl Into<String>) -> Self {
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
        }
    }

    #[cfg(test)]
    pub(crate) fn with_launch_wait(mut self, wait: Duration) -> Self {
        self.launch_wait = wait;
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
        match self.ask(Request::GetContext).await {
            Ok(context) => Answer::ok(object(context)),
            Err(LinkError::NotRunning) => Answer::notice("notOpen", NOT_OPEN_MESSAGE),
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

    /// Where something is described.
    pub async fn search(&self, query: &str, limit: Option<usize>) -> Answer {
        if !self.is_on() {
            return Answer::off(self.known);
        }
        let query = query.trim().to_owned();
        if query.is_empty() {
            return Answer::failed("the query is empty");
        }
        let limit = limit
            .unwrap_or(DEFAULT_SEARCH_HITS)
            .clamp(1, MAX_SEARCH_HITS);
        let request = Request::Search {
            query: query.clone(),
            limit: Some(limit),
        };
        let (mut hits, source) = match self.ask(request).await {
            Ok(hits) => (object(hits), "window"),
            Err(LinkError::Remote(error)) => return Answer::failed(error.message),
            Err(_) => {
                match self
                    .offline(move |project| project.search(&query, limit))
                    .await
                {
                    Ok(hits) => (Map::from_iter([("hits".to_owned(), json!(hits))]), "files"),
                    Err(error) => return Answer::failed(error),
                }
            }
        };
        hits.insert("source".to_owned(), json!(source));
        Answer::ok(hits)
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
            Ok(opened) => Answer::ok(shown(opened, false)),
            Err(LinkError::Remote(error)) => Answer::failed(error.message),
            Err(LinkError::NotRunning) => self.start_and_open(request).await,
            Err(error) => Answer::failed(error.to_string()),
        }
    }

    /// The window is closed: check the place first (nothing is started for a path that is not a document), start
    /// TrailMark on the project, and hand the place over once it listens.
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
        if let Err(error) = (self.launcher)(&self.root) {
            return Answer::failed(format!("TrailMark could not be started: {error}"));
        }
        let deadline = tokio::time::Instant::now() + self.launch_wait;
        loop {
            match self.ask(request.clone()).await {
                Ok(opened) => return Answer::ok(shown(opened, true)),
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

/// The `open` answer: where the person was taken, and what to expect.
fn shown(opened: Value, started: bool) -> Map<String, Value> {
    let mut fields = object(opened);
    let heading_missing = fields
        .get("headingNotFound")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = if heading_missing {
        format!("{SHOWN_MESSAGE} The document has no such heading, so it opened at its top.")
    } else {
        SHOWN_MESSAGE.to_owned()
    };
    fields.insert("message".to_owned(), json!(message));
    if started {
        fields.insert("started".to_owned(), json!(true));
    }
    fields
}

/// Starts TrailMark on a project and lets it go its own way.
pub(crate) fn launch_app(project: &Path) -> io::Result<()> {
    use std::process::{Command, Stdio};

    let mut command = Command::new(std::env::current_exe()?);
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
    use crate::bridge::hub::testing::Fake;
    use crate::bridge::service::AgentBridge;
    use crate::search::SearchHit;
    use crate::session::{SessionState, StateStore};
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
        let sockets = tempfile::Builder::new()
            .prefix("tm")
            .tempdir_in(std::env::temp_dir())
            .expect("sockets");
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
                .save_session(&SessionState {
                    agent_connection: on,
                    ..SessionState::default()
                })
                .expect("save the session");
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
            core.search("lighthouse", None).await,
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
        assert_eq!(
            answer.value["message"],
            "TrailMark is not open on this project."
        );
        assert!(!answer.is_error);
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

        let found = core.search("beacon", Some(3)).await;
        assert_eq!(status(&found), "ok", "{found:?}");
        assert_eq!(found.value["source"], "files");
        assert_eq!(found.value["hits"][0]["path"], "docs/plan.md");
        assert_eq!(found.value["hits"][0]["headingPath"][1], "Rollout");
        assert_eq!(found.value["hits"][0]["line"], 5);
        assert!(found.value["hits"][0]["snippet"]
            .as_str()
            .expect("snippet")
            .contains("beacon"));

        let empty = core.search("  ", None).await;
        assert!(empty.is_error);
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
        let found = core.search("lighthouse", None).await;
        assert_eq!(found.value["source"], "window");
        assert_eq!(found.value["hits"][0]["snippet"], "The lighthouse");
        fx.bridge.shutdown();
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
    fn times_are_written_as_utc() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601_utc(1_760_520_600_000), "2025-10-15T09:30:00Z");
        assert_eq!(iso8601_utc(4_102_444_799_999), "2099-12-31T23:59:59Z");
    }
}
