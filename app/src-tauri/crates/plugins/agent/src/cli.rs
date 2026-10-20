//! The agent's side, with no window: which project the working folder belongs to, whether the app serves it (then
//! calls go there) or not (then the same tools answer from the files), and `permark context`.

use std::any::Any;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use permark_core::ipc_local::{self, Endpoint, IpcLocalError};
use permark_core::paths::Root;
use permark_core::state::{AppDirs, ProjectId, StateStore};
use serde_json::{json, Value};

use crate::context::WindowContext;
use crate::perms;
use crate::server::{Request, Response};
use crate::tools::Tools;
use crate::ToolError;

/// How long `open` waits for the app it started to listen.
pub const LAUNCH_WAIT: Duration = Duration::from_secs(25);
const LAUNCH_POLL: Duration = Duration::from_millis(200);

/// The tools over the project's files, and what must live as long as they do (core env, project core).
pub struct Composed {
    pub tools: Tools,
    pub keep: Box<dyn Any + Send + Sync>,
}

/// Builds the files-mode composition for a project (the app crate knows the plugins of the build).
pub type Compose = Arc<dyn Fn(&Root) -> Result<Composed, String> + Send + Sync>;
/// Starts the app on a project.
pub type Launcher = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

/// The nearest folder at or above `start` that the person connected (its profile folder has `agent.json`).
#[must_use]
pub fn discover(profile: &Path, start: &Path) -> Option<Root> {
    let start = dunce_canonical(start)?;
    start.ancestors().find_map(|folder| {
        let root = Root::open(folder).ok()?;
        let id = ProjectId::of(&root);
        profile
            .join("projects")
            .join(id.as_str())
            .join(perms::PROJECT_FILE)
            .is_file()
            .then_some(root)
    })
}

fn dunce_canonical(path: &Path) -> Option<PathBuf> {
    Root::open(path).ok().map(|root| root.path().to_path_buf())
}

/// The project an agent works in, and how to reach it.
pub struct Agent {
    project: Option<Project>,
    compose: Compose,
    launcher: Launcher,
    launch_wait: Duration,
    files: Mutex<Option<Arc<Composed>>>,
    /// The held connection that shows the app an agent is here (dropping it ends the session).
    session: Mutex<Option<Box<dyn Send>>>,
    client: Mutex<String>,
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
    /// Finds the project of `working_folder` (none: every tool says the connection is off).
    #[must_use]
    pub fn discover(
        dirs: &AppDirs,
        working_folder: &Path,
        compose: Compose,
        launcher: Launcher,
    ) -> Self {
        let project = discover(&dirs.profile, working_folder).map(|root| Project {
            store: StateStore::for_project(dirs, &root),
            endpoint: Endpoint::project(&dirs.profile, &root).map_err(|e| e.to_string()),
            root,
        });
        Self {
            project,
            compose,
            launcher,
            launch_wait: LAUNCH_WAIT,
            files: Mutex::new(None),
            session: Mutex::new(None),
            client: Mutex::new("agent".to_owned()),
        }
    }

    #[must_use]
    pub fn with_launch_wait(mut self, wait: Duration) -> Self {
        self.launch_wait = wait;
        self
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
        client.clone_into(&mut lock(&self.client));
        self.ensure_session();
    }

    fn ensure_session(&self) {
        let Some(Ok(endpoint)) = self.project.as_ref().map(|p| p.endpoint.as_ref()) else {
            return;
        };
        let mut session = lock(&self.session);
        if session.is_some() {
            return;
        }
        let hello = Request::Hello {
            client: lock(&self.client).clone(),
        };
        if let Ok((Response::Ok { .. }, stream)) =
            ipc_local::open_session::<_, Response>(endpoint, &hello)
        {
            *session = Some(Box::new(stream));
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
            client: lock(&self.client).clone(),
        };
        match ipc_local::request::<_, Response>(endpoint, &request)? {
            Response::Ok { value } => Ok(Ok(value)),
            Response::Error { code, message } => Ok(Err(ToolError::from_wire(&code, &message))),
        }
    }

    fn files(&self, project: &Project) -> Result<Arc<Composed>, ToolError> {
        let mut files = lock(&self.files);
        if let Some(composed) = files.as_ref() {
            return Ok(Arc::clone(composed));
        }
        let composed = Arc::new((self.compose)(&project.root).map_err(ToolError::Failed)?);
        *files = Some(Arc::clone(&composed));
        Ok(composed)
    }

    /// One tool call: the switch (read fresh), then the app when it serves the project, else the files.
    ///
    /// # Errors
    /// What the tool answers, or why nothing could.
    pub fn call(&self, tool: &str, args: &Value) -> Result<Value, ToolError> {
        let Some(project) = &self.project else {
            return Err(ToolError::Off);
        };
        if !perms::load(&project.store).enabled {
            return Err(ToolError::Off);
        }
        let endpoint = project
            .endpoint
            .as_ref()
            .map_err(|why| ToolError::Failed(why.clone()))?;
        match self.proxy(endpoint, tool, args) {
            Ok(answer) => {
                self.ensure_session();
                answer
            }
            Err(IpcLocalError::Unreachable { .. }) => {
                // the app is not serving this project: a session held before is gone
                lock(&self.session).take();
                let composed = self.files(project)?;
                match composed.tools.call(tool, args.clone()) {
                    Err(ToolError::NotOpen) if tool == "open" => {
                        self.start_and_open(project, endpoint, args)
                    }
                    other => other,
                }
            }
            Err(IpcLocalError::Untrusted(why)) => Err(ToolError::Failed(format!(
                "the program listening for this project is not this user's PerMark ({why})"
            ))),
            Err(other) => Err(ToolError::Failed(other.to_string())),
        }
    }

    /// The place was checked against the files; start the app on the project and hand the place over once it listens.
    fn start_and_open(
        &self,
        project: &Project,
        endpoint: &Endpoint,
        args: &Value,
    ) -> Result<Value, ToolError> {
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

    /// The line `permark context` prints, or why there is none.
    ///
    /// # Errors
    /// The reason, for standard error.
    pub fn context_line(&self) -> Result<String, String> {
        let value = self
            .call("get_context", &Value::Null)
            .map_err(|e| e.to_string())?;
        let open = value
            .get("openComments")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let context: WindowContext = serde_json::from_value(value).map_err(|e| e.to_string())?;
        context
            .summary(usize::try_from(open).unwrap_or(usize::MAX))
            .ok_or_else(|| "no document is open in PerMark".to_owned())
    }
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
