//! The agent's side, with no window: which project the working folder belongs to, whether the app serves it (then
//! calls go there) or not (then only `open` acts: it starts the app), and `permark context`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use permark_core::ipc_local::{self, Endpoint, IpcLocalError, Stream};
use permark_core::paths::Root;
use permark_core::session::RecentStore;
use permark_core::state::{AppDirs, StateStore};
use serde::Deserialize as _;
use serde_json::{json, Value};

use crate::context::WindowContext;
use crate::perms;
use crate::server::{Request, Response};
use crate::session::SessionId;
use crate::ToolError;

/// How long `open` waits for the app it started to listen.
pub const LAUNCH_WAIT: Duration = Duration::from_secs(25);
/// How often a running `permark mcp` checks its session and opens it again (the app restarted, a place freed).
pub const REPAIR_EVERY: Duration = Duration::from_secs(3);
/// While the person keeps this agent disconnected, every this many checks one hello still asks the app: an Undo there
/// pairs the agent again without waiting for its next tool call (the app answers a refusal at once).
pub const ASK_AGAIN_EVERY: u32 = 5;
const LAUNCH_POLL: Duration = Duration::from_millis(200);

/// Starts the app on a project.
pub type Launcher = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

/// The nearest folder at or above `start` that the person connected (its profile folder has `agent.json`). None when
/// that choice was made for another folder that stood at its path: the search never goes on to a folder above it.
#[must_use]
pub fn discover(dirs: &AppDirs, start: &Path) -> Option<Root> {
    let (root, store) = nearest_known(dirs, start)?;
    (!perms::load(&store).replaced(root.path())).then_some(root)
}

/// The nearest folder at or above `start` the person made a choice for (on, off or replaced since).
fn nearest_known(dirs: &AppDirs, start: &Path) -> Option<(Root, StateStore)> {
    let start = dunce_canonical(start)?;
    start.ancestors().find_map(|folder| {
        let root = Root::open(folder).ok()?;
        let store = StateStore::for_project(dirs, &root);
        perms::known(&store).then_some((root, store))
    })
}

/// Connected projects below `start` among the recent folders, newest first, `/`-separated relative to `start` with
/// a trailing `/`. Only a pointer for the agent's refusal: the switch of each is read fresh and nothing is granted.
fn connected_below(dirs: &AppDirs, start: &Path) -> Vec<String> {
    let Ok(start) = Root::open(start) else {
        return Vec::new();
    };
    // compared as written before any disk access: a recent folder elsewhere (an offline network drive) is never opened
    RecentStore::new(&StateStore::app(dirs))
        .list_where(|path| path != start.path() && path.starts_with(start.path()))
        .into_iter()
        .filter(|recent| !recent.missing)
        .filter_map(|recent| Root::open(Path::new(&recent.path)).ok())
        .filter_map(|root| {
            let rel = folder_in(&start, root.path()).filter(|rel| !rel.is_empty())?;
            perms::load(&StateStore::for_project(dirs, &root))
                .is_on(root.path())
                .then(|| format!("{rel}/"))
        })
        .collect()
}

fn dunce_canonical(path: &Path) -> Option<PathBuf> {
    Root::open(path).ok().map(|root| root.path().to_path_buf())
}

/// `working_folder` relative to `root`, `/`-separated (`""` = the root itself).
fn folder_in(root: &Root, working_folder: &Path) -> Option<String> {
    let start = dunce_canonical(working_folder)?;
    let rest = start.strip_prefix(root.path()).ok()?;
    Some(
        rest.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// The project an agent works in, and how to reach it.
pub struct Agent {
    project: Option<Project>,
    /// No folder at or above the working folder was ever connected: the connected projects below it (found once at
    /// start). None while in a project, and for a folder whose choice was made for another folder at its path (that
    /// one answers `Off`, never pointing elsewhere).
    outside: Option<Vec<String>>,
    launcher: Launcher,
    launch_wait: Duration,
    repair_every: Duration,
    /// The held connection that shows the app an agent is here (dropping it ends the session).
    session: Mutex<Option<Stream>>,
    /// One hello at a time; the others skip instead of waiting (a tool call never waits on an app that is slow).
    opening: Mutex<()>,
    client: Mutex<String>,
    /// Who this process is to the app, drawn once at start.
    id: SessionId,
    /// The working folder relative to the project root, `/`-separated (`""` = the root or no project).
    folder: String,
    /// The app's last answer was a refusal of this session (the person disconnected it): no session is opened
    /// while set, except the pairing loop's occasional [`Agent::ask_again`]. Calls still go to the app, so an Undo
    /// there lets the next call | hello through (and clears this).
    disconnected: AtomicBool,
}

struct Project {
    root: Root,
    store: StateStore,
    endpoint: Result<Endpoint, String>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Agent {
    /// Finds the project of `working_folder` (none: every tool refuses, naming connected projects below it).
    #[must_use]
    pub fn discover(dirs: &AppDirs, working_folder: &Path, launcher: Launcher) -> Self {
        let known = nearest_known(dirs, working_folder);
        let outside = known
            .is_none()
            .then(|| connected_below(dirs, working_folder));
        let project = known
            .filter(|(root, store)| !perms::load(store).replaced(root.path()))
            .map(|(root, store)| Project {
                endpoint: Endpoint::project(&dirs.profile, &root).map_err(|e| e.to_string()),
                store,
                root,
            });
        let folder = project
            .as_ref()
            .and_then(|p| folder_in(&p.root, working_folder))
            .unwrap_or_default();
        Self {
            project,
            outside,
            launcher,
            launch_wait: LAUNCH_WAIT,
            repair_every: REPAIR_EVERY,
            session: Mutex::new(None),
            opening: Mutex::new(()),
            client: Mutex::new("agent".to_owned()),
            id: SessionId::draw(),
            folder,
            disconnected: AtomicBool::new(false),
        }
    }

    /// This process's session id.
    #[must_use]
    pub fn session_id(&self) -> &SessionId {
        &self.id
    }

    /// The working folder relative to the project root (`""` = the root).
    #[must_use]
    pub fn folder(&self) -> &str {
        &self.folder
    }

    /// The app refused this session (the person disconnected it): stop opening sessions.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.disconnected.load(Ordering::Acquire)
    }

    /// Remembers whether the app's answer refused this session (any other answer = not refused any more).
    fn note(&self, answer: Result<(), &ToolError>) {
        let refused = answer.is_err_and(|e| *e == ToolError::Disconnected);
        self.disconnected.store(refused, Ordering::Release);
    }

    #[must_use]
    pub fn with_launch_wait(mut self, wait: Duration) -> Self {
        self.launch_wait = wait;
        self
    }

    /// How often a running `permark mcp` looks at its session (tests: shorter).
    #[must_use]
    pub fn with_repair_every(mut self, every: Duration) -> Self {
        self.repair_every = every;
        self
    }

    #[must_use]
    pub fn repair_every(&self) -> Duration {
        self.repair_every
    }

    /// The project folder (none when the working folder is not a connected project).
    #[must_use]
    pub fn root(&self) -> Option<&Root> {
        self.project.as_ref().map(|p| &p.root)
    }

    /// The name the client gave.
    #[must_use]
    pub fn client(&self) -> String {
        lock(&self.client).clone()
    }

    /// The client said who it is: open a session with the app (it shows the agent as connected) when it listens.
    pub fn hello(&self, client: &str) {
        self.name_client(client);
        self.ensure_session();
    }

    /// Records the client's name only (no I/O). Call it where the name is decided, in order: two writers on blocking
    /// threads finish in any order.
    pub fn name_client(&self, client: &str) {
        client.clone_into(&mut lock(&self.client));
    }

    /// Opens the session with the app when it listens and none is held (blocking I/O). A held one the app closed
    /// (it quit, restarted or let the session go) is dropped first. Nothing while the switch is off (read fresh) or
    /// the person disconnected this agent. Returns at once while another hello is under way.
    pub fn ensure_session(&self) {
        self.open_session(false);
    }

    /// Like [`Agent::ensure_session`], but also while this agent was disconnected: one hello asks the app whether
    /// that still holds (an answer other than the refusal pairs it again).
    pub fn ask_again(&self) {
        self.open_session(true);
    }

    fn open_session(&self, past_refusal: bool) {
        let Some(project) = &self.project else {
            return;
        };
        let Ok(endpoint) = &project.endpoint else {
            return;
        };
        let _opening = match self.opening.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        {
            let mut session = lock(&self.session);
            match session.as_ref() {
                Some(stream) if !ipc_local::peer_gone(stream) => return,
                Some(_) => drop(session.take()),
                None => {}
            }
        }
        if (self.is_disconnected() && !past_refusal)
            || !perms::load(&project.store).is_on(project.root.path())
        {
            return;
        }
        let hello = Request::Hello {
            session: Some(self.id.clone()),
            client: lock(&self.client).clone(),
            folder: self.folder.clone(),
        };
        // the session lock is not held over the I/O: a tool call that ends the session never waits for it
        match ipc_local::open_session::<_, Response>(endpoint, &hello) {
            Ok((Response::Ok { .. }, stream)) => {
                self.note(Ok(()));
                *lock(&self.session) = Some(stream);
            }
            Ok((Response::Error { code, message }, _)) => {
                self.note(Err(&ToolError::from_wire(&code, &message)));
            }
            Err(_) => {}
        }
    }

    fn proxy(
        &self,
        endpoint: &Endpoint,
        tool: &str,
        args: &Value,
    ) -> Result<Result<Value, ToolError>, IpcLocalError> {
        let request = Request::Call {
            tool: tool.to_owned(),
            args: args.clone(),
            session: Some(self.id.clone()),
        };
        match ipc_local::request::<_, Response>(endpoint, &request)? {
            Response::Ok { value } => {
                self.note(Ok(()));
                Ok(Ok(value))
            }
            Response::Error { code, message } => {
                let error = ToolError::from_wire(&code, &message);
                self.note(Err(&error));
                Ok(Err(error))
            }
        }
    }

    /// One tool call: the switch (read fresh), then the app when it serves the project, else a refusal (`open`
    /// starts the app).
    ///
    /// # Errors
    /// What the tool answers, or why nothing could.
    pub fn call(&self, tool: &str, args: &Value) -> Result<Value, ToolError> {
        let Some(project) = &self.project else {
            return Err(self
                .outside
                .clone()
                .map_or(ToolError::Off, |below| ToolError::NotConnected { below }));
        };
        if !perms::load(&project.store).is_on(project.root.path()) {
            return Err(ToolError::Off);
        }
        let endpoint = project
            .endpoint
            .as_ref()
            .map_err(|why| ToolError::Failed(why.clone()))?;
        match self.proxy(endpoint, tool, args) {
            Ok(answer) => {
                if self.is_disconnected() {
                    // the app already closed its end
                    lock(&self.session).take();
                } else {
                    self.ensure_session();
                }
                answer
            }
            Err(IpcLocalError::Unreachable { .. }) => {
                // the app is not serving this project: a session held before is gone
                lock(&self.session).take();
                // only `open` acts while the app is closed, and visibly: it brings the app up
                if tool == "open" {
                    self.start_and_open(project, endpoint, args)
                } else {
                    Err(ToolError::AppClosed)
                }
            }
            Err(IpcLocalError::Untrusted(why)) => Err(ToolError::Failed(format!(
                "the program listening for this project is not this user's PerMark ({why})"
            ))),
            Err(other) => Err(ToolError::Failed(other.to_string())),
        }
    }

    /// Starts the app on the project and hands the place over once it listens (the app checks the place).
    fn start_and_open(
        &self,
        project: &Project,
        endpoint: &Endpoint,
        args: &Value,
    ) -> Result<Value, ToolError> {
        // arguments the tool would refuse start nothing
        crate::tools::check_open_args(args)?;
        (self.launcher)(project.root.path())
            .map_err(|e| ToolError::Failed(format!("PerMark could not be started: {e}")))?;
        let deadline = Instant::now() + self.launch_wait;
        loop {
            match self.proxy(endpoint, "open", args) {
                Ok(Ok(mut value)) => {
                    if let Some(map) = value.as_object_mut() {
                        map.insert("started".to_owned(), json!(true));
                    }
                    self.ensure_session();
                    return Ok(value);
                }
                Ok(Err(error)) => return Err(error),
                Err(_) if Instant::now() >= deadline => {
                    return Err(ToolError::Failed(
                        "PerMark was started but is not ready yet; try again in a few seconds"
                            .to_owned(),
                    ))
                }
                Err(_) => std::thread::sleep(LAUNCH_POLL),
            }
        }
    }

    /// What `permark context` prints, or why there is none.
    ///
    /// # Errors
    /// The reason, for standard error.
    pub fn context_line(&self) -> Result<String, String> {
        let value = self
            .call("get_context", &Value::Null)
            .map_err(|e| e.to_string())?;
        context_text(&value)
    }
}

/// The `permark context` sentence of a `get_context` answer, led by the project's name when it has one.
pub(crate) fn context_text(value: &Value) -> Result<String, String> {
    let open = value
        .get("openComments")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let context = WindowContext::deserialize(value).map_err(|e| e.to_string())?;
    let line = context
        .summary(usize::try_from(open).unwrap_or(usize::MAX))
        .ok_or_else(|| "no document is open in PerMark".to_owned())?;
    Ok(match value.get("project").and_then(Value::as_str) {
        Some(project) => format!("Project {}: {line}", crate::format::value(project)),
        None => line,
    })
}

/// The running program, as it can be started again (an AppImage is named by the file the person started).
#[must_use]
pub fn program_path() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::current_exe().ok())
        .map(|p| {
            let text = p.to_string_lossy().into_owned();
            text.strip_prefix(r"\\?\")
                .filter(|rest| !rest.starts_with("UNC\\"))
                .map_or(p, PathBuf::from)
        })
}

/// Starts this program on a project folder, detached (no shell; the folder is the only argument).
///
/// # Errors
/// The program's own path is unknown, or it cannot be started.
pub fn launch_app(project: &Path) -> io::Result<()> {
    let program =
        program_path().ok_or_else(|| io::Error::other("the program's own path is unknown"))?;
    permark_core::process::spawn_detached(
        &program,
        &[std::ffi::OsStr::new("--"), project.as_os_str()],
    )
}
