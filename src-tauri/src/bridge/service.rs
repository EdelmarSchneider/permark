//! The agent connection of one open project: the switch, the socket while it is on, the state file, and what the
//! window publishes. [`AgentBridge`] ties the pieces together; the pieces are tested on their own.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::session::remove_local_state;
use crate::vault::Vault;

use super::context::AgentContext;
use super::file::{ContextFile, WRITE_INTERVAL};
use super::hub::{AgentEvents, AgentStatus, Backend, FailureKind, Hub, StartFailure};
use super::protocol::Opened;
use super::server::{BridgeServer, StartError};
use super::socket::{Endpoint, ListenError, SocketError};
use super::tools::program_path;

/// What exists while the connection is on.
struct Running {
    server: Option<BridgeServer>,
    file: Option<ContextFile>,
}

pub struct AgentBridge {
    hub: Arc<Hub>,
    endpoint: Result<Endpoint, StartFailure>,
    /// Where `agent.json` goes: only a project that keeps its state in `.trailmark/` has one.
    agent_path: Mutex<Option<PathBuf>>,
    running: Mutex<Option<Running>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether a failure to listen means that another program already holds the name. A named pipe's first instance is
/// exclusive on Windows: the second is refused with "access denied" (or "pipe busy" when every instance is in use);
/// a socket file that is in the way is "address in use".
fn name_is_taken(source: &io::Error) -> bool {
    const ACCESS_DENIED: i32 = 5;
    const PIPE_BUSY: i32 = 231;
    source.kind() == io::ErrorKind::AddrInUse
        || (cfg!(windows) && matches!(source.raw_os_error(), Some(ACCESS_DENIED | PIPE_BUSY)))
}

/// A socket that cannot be made, in the terms the interface words.
fn socket_failure(error: &SocketError) -> StartFailure {
    let kind = match error {
        SocketError::PathTooLong(_) => FailureKind::PathTooLong,
        SocketError::Folder { .. } => FailureKind::FolderNotPrivate,
    };
    StartFailure {
        kind,
        message: error.to_string(),
    }
}

/// A listener that cannot start, in the terms the interface words.
fn start_failure(error: &StartError) -> StartFailure {
    let kind = match error {
        StartError::Listen(ListenError::Socket(error)) => return socket_failure(error),
        StartError::Listen(ListenError::InUse(_)) => FailureKind::InUse,
        StartError::Listen(ListenError::Io { source, .. }) if name_is_taken(source) => {
            FailureKind::InUse
        }
        StartError::Listen(ListenError::Io { .. }) | StartError::Thread(_) => FailureKind::Other,
    };
    StartFailure {
        kind,
        message: error.to_string(),
    }
}

impl AgentBridge {
    /// The connection is off until [`AgentBridge::set_enabled`] turns it on.
    pub(crate) fn new(
        vault: Arc<Vault>,
        endpoint: Result<Endpoint, SocketError>,
        agent_path: Option<PathBuf>,
        backend: Arc<dyn Backend>,
        events: Arc<dyn AgentEvents>,
    ) -> Self {
        Self {
            hub: Arc::new(
                Hub::new(vault, backend, events)
                    .with_program(program_path().map(|path| path.display().to_string())),
            ),
            endpoint: endpoint.map_err(|error| socket_failure(&error)),
            agent_path: Mutex::new(agent_path),
            running: Mutex::new(None),
        }
    }

    /// Turns the connection on or off. A connection that cannot start (the socket cannot be made) stays "on" as
    /// a setting and reports why in the status, instead of failing the caller. Off always clears that state (there
    /// is nothing running to stop, but the failure is not kept), and on again tries again.
    pub fn set_enabled(&self, enabled: bool) -> AgentStatus {
        let mut running = lock(&self.running);
        if enabled && running.is_some() {
            return self.hub.status();
        }
        if enabled {
            match self.start() {
                Ok(started) => {
                    *running = Some(started);
                    self.hub.set_lifecycle(true, true, None);
                }
                Err(error) => self.hub.set_lifecycle(true, false, Some(error)),
            }
        } else {
            drop(running.take()); // the socket closes, the connections end, the state file goes
            self.hub.set_lifecycle(false, false, None);
        }
        self.hub.status()
    }

    fn start(&self) -> Result<Running, StartFailure> {
        let endpoint = self.endpoint.as_ref().map_err(Clone::clone)?;
        let server = BridgeServer::start(endpoint, Arc::clone(&self.hub))
            .map_err(|error| start_failure(&error))?;
        let file = lock(&self.agent_path)
            .clone()
            .map(|path| ContextFile::start(path, WRITE_INTERVAL, self.hub.context()));
        Ok(Running {
            server: Some(server),
            file,
        })
    }

    /// Deletes a state file that a window left behind when it was killed while the connection was on. The file is only
    /// meant to exist while the connection is on, so a project that opens with it off has no use for one.
    pub fn remove_stale_file(&self) {
        if let Some(path) = lock(&self.agent_path).as_ref() {
            let _ = remove_local_state(path);
        }
    }

    /// What the window shows now. Kept whether or not the connection is on, so turning it on answers at once.
    pub fn publish(&self, context: AgentContext) {
        let published = self.hub.publish(context);
        if let Some(file) = lock(&self.running).as_ref().and_then(|r| r.file.as_ref()) {
            file.publish(published);
        }
    }

    /// The place an agent asked to be shown, once.
    pub fn take_open(&self) -> Option<Opened> {
        self.hub.take_open()
    }

    pub fn status(&self) -> AgentStatus {
        self.hub.status()
    }

    /// The project's state moved (it got a `.trailmark/` folder): the state file follows, or starts.
    pub fn relocate_file(&self, path: Option<PathBuf>) {
        lock(&self.agent_path).clone_from(&path);
        let mut running = lock(&self.running);
        let Some(running) = running.as_mut() else {
            return;
        };
        match (path, running.file.as_ref()) {
            (Some(path), Some(file)) => file.relocate(path),
            (Some(path), None) => {
                running.file = Some(ContextFile::start(path, WRITE_INTERVAL, self.hub.context()));
            }
            (None, _) => running.file = None,
        }
    }

    /// Writes the state file now instead of at the next interval (a test reads it right after).
    #[cfg(test)]
    fn flush_file(&self) {
        if let Some(file) = lock(&self.running).as_ref().and_then(|r| r.file.as_ref()) {
            file.flush();
        }
    }

    /// Stops everything (the project is closing).
    pub fn shutdown(&self) {
        if let Some(mut running) = lock(&self.running).take() {
            if let Some(mut server) = running.server.take() {
                server.stop();
            }
        }
        self.hub
            .set_lifecycle(self.hub.status().enabled, false, None);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::bridge::context::{Section, ViewMode};
    use crate::bridge::hub::testing::Fake;
    use crate::bridge::link::{Link, LinkError};
    use crate::bridge::protocol::{ErrorKind, Request};
    use crate::search::SearchHit;

    struct Fx {
        project: tempfile::TempDir,
        sockets: tempfile::TempDir,
        fake: Arc<Fake>,
        bridge: AgentBridge,
        endpoint: Endpoint,
    }

    /// A project with one document and a bridge on a socket of its own (every test needs its own name).
    fn fx(name: &str, agent_path: Option<PathBuf>) -> Fx {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("docs")).expect("mkdir");
        std::fs::write(
            project.path().join("docs/plan.md"),
            "# Plan\n\nIntro.\n\n## Rollout\n\nShip it.\n",
        )
        .expect("write");
        let sockets = crate::bridge::socket::socket_folder();
        let vault = Arc::new(Vault::open(project.path()).expect("vault"));
        let fake = Arc::new(Fake::default());
        let endpoint =
            Endpoint::in_folder(sockets.path(), Path::new(name), "tester").expect("endpoint");
        let bridge = AgentBridge::new(
            vault,
            Endpoint::in_folder(sockets.path(), Path::new(name), "tester"),
            agent_path,
            fake.clone(),
            fake.clone(),
        );
        Fx {
            project,
            sockets,
            fake,
            bridge,
            endpoint,
        }
    }

    fn link(fx: &Fx) -> Link {
        Link::new(fx.endpoint.clone())
    }

    #[tokio::test]
    async fn nobody_answers_while_the_connection_is_off() {
        let fx = fx("off", None);
        assert!(matches!(
            link(&fx).call(Request::GetContext).await,
            Err(LinkError::NotRunning)
        ));
        assert!(!fx.bridge.status().enabled);
        // Even off, the status says where the program is, for the setup lines.
        assert!(fx.bridge.status().program.is_some());
    }

    #[tokio::test]
    async fn an_agent_reads_what_the_window_published() {
        let fx = fx("context", None);
        let status = fx.bridge.set_enabled(true);
        assert!(status.enabled && status.listening, "{status:?}");
        fx.bridge.publish(AgentContext {
            document: Some("docs/plan.md".to_owned()),
            mode: Some(ViewMode::Reading),
            section: Some(Section {
                heading_path: vec!["Plan".to_owned(), "Rollout".to_owned()],
                start_line: Some(5),
                end_line: Some(7),
            }),
            ..AgentContext::default()
        });
        let result = link(&fx)
            .call(Request::GetContext)
            .await
            .expect("the window answers");
        assert_eq!(result["document"], "docs/plan.md");
        assert_eq!(result["mode"], "reading");
        assert_eq!(result["section"]["headingPath"], json!(["Plan", "Rollout"]));
        assert_eq!(result["section"]["startLine"], 5);
        assert_eq!(result["unsaved"], false);
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn open_reaches_the_window_and_waits_for_it_to_pick_it_up() {
        let fx = fx("open", None);
        fx.bridge.set_enabled(true);
        let link = link(&fx);
        let result = link
            .call(Request::Open {
                path: "docs/plan.md".to_owned(),
                line: None,
                heading: Some("Rollout".to_owned()),
            })
            .await
            .expect("opened");
        assert_eq!(result, json!({ "path": "docs/plan.md", "line": 5 }));
        assert_eq!(
            fx.fake.opens.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the interface was told"
        );
        let place = fx.bridge.take_open().expect("a place waits");
        assert_eq!((place.path.as_str(), place.line), ("docs/plan.md", Some(5)));
        assert_eq!(fx.bridge.take_open(), None);

        // Something that is not a document of the project is an error the agent can read.
        let error = link
            .call(Request::Open {
                path: "../elsewhere.md".to_owned(),
                line: None,
                heading: None,
            })
            .await
            .expect_err("refused");
        assert!(
            matches!(&error, LinkError::Remote(e) if e.kind == ErrorKind::NotFound),
            "{error:?}"
        );
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn search_and_unread_are_answered_from_the_project() {
        let fx = fx("search", None);
        *fx.fake.corpus.lock().expect("lock") = vec![SearchHit {
            path: "docs/plan.md".to_owned(),
            heading_path: vec!["Plan".to_owned(), "Rollout".to_owned()],
            snippet: "<mark>Ship</mark> it".to_owned(),
            start_line: 5,
            score: 1.0,
        }];
        *fx.fake.unread.lock().expect("lock") = vec!["docs/plan.md".to_owned()];
        fx.bridge.set_enabled(true);
        let link = link(&fx);
        let hits = link
            .call(Request::Search {
                query: "Ship".to_owned(),
                limit: None,
            })
            .await
            .expect("hits");
        assert_eq!(hits["hits"][0]["snippet"], "Ship it");
        assert_eq!(hits["hits"][0]["line"], 5);
        let unread = link.call(Request::ListUnread).await.expect("unread");
        assert_eq!(unread["total"], 1);
        assert_eq!(unread["unread"][0]["path"], "docs/plan.md");
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn the_status_counts_the_connected_agent_and_lists_what_it_asked() {
        let fx = fx("status", None);
        fx.bridge.set_enabled(true);
        let link = link(&fx);
        link.call(Request::GetContext).await.expect("answer");
        let status = fx.bridge.status();
        assert_eq!(status.connections, 1);
        assert_eq!(status.recent[0].tool, "get_context");
        link.call(Request::ListUnread).await.expect("answer");
        assert_eq!(fx.bridge.status().connections, 1, "the connection is kept");
        assert_eq!(fx.bridge.status().recent.len(), 2);
        fx.bridge.shutdown();
    }

    /// One slow answer does not hold up another connection, or the window's own switch.
    #[tokio::test]
    async fn a_slow_answer_does_not_hold_up_the_others() {
        use std::sync::atomic::Ordering;
        use std::sync::mpsc;
        use std::time::Duration;

        let fx = fx("slow", None);
        fx.bridge.set_enabled(true);
        // The unread list waits for a lock that another thread holds until it is told to let go.
        let (locked, is_locked) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let fake = Arc::clone(&fx.fake);
        let holder = std::thread::spawn(move || {
            let held = fake.unread.lock().expect("lock");
            locked.send(()).expect("say so");
            let _ = released.recv();
            drop(held);
        });
        is_locked.recv().expect("the lock is held");

        let slow = {
            let link = link(&fx);
            tokio::spawn(async move { link.call(Request::ListUnread).await })
        };
        // Wait for the slow request to be inside the window's answer, so that what follows is really behind it.
        while fx.fake.unread_asked.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let other =
            tokio::time::timeout(Duration::from_secs(10), link(&fx).call(Request::GetContext))
                .await
                .expect("answered while the slow answer is still being worked out");
        assert!(other.is_ok(), "{other:?}");

        release.send(()).expect("let go");
        let slow = slow
            .await
            .expect("task")
            .expect("the slow one is answered too");
        assert_eq!(slow["total"], 0);
        holder.join().expect("holder");
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn turning_it_off_closes_the_socket_and_a_running_agent_finds_out() {
        let fx = fx("switch", None);
        fx.bridge.set_enabled(true);
        let link = link(&fx);
        link.call(Request::GetContext).await.expect("answer");

        let status = fx.bridge.set_enabled(false);
        assert!(!status.enabled && !status.listening);
        assert_eq!(status.connections, 0);
        assert!(
            matches!(
                link.call(Request::GetContext).await,
                Err(LinkError::NotRunning)
            ),
            "the stale connection is replaced, and there is nothing to connect to"
        );

        // On again: the same agent process reconnects by itself.
        fx.bridge.set_enabled(true);
        assert!(link.call(Request::GetContext).await.is_ok());
        fx.bridge.shutdown();
    }

    #[tokio::test]
    async fn a_second_window_cannot_take_the_socket_of_the_first() {
        let fx = fx("twice", None);
        assert!(fx.bridge.set_enabled(true).listening);
        let vault = Arc::new(Vault::open(fx.project.path()).expect("vault"));
        let other = AgentBridge::new(
            vault,
            Endpoint::in_folder(fx.sockets.path(), Path::new("twice"), "tester"),
            None,
            fx.fake.clone(),
            fx.fake.clone(),
        );
        let status = other.set_enabled(true);
        // the person's choice stands, and the status says why nothing listens
        assert!(status.enabled && !status.listening, "{status:?}");
        let why = status.error.expect("the reason");
        assert_eq!(why.kind, FailureKind::InUse, "{why:?}");
        assert!(!why.message.is_empty(), "the system's own words are kept");
        // the first window still answers, also after the second one closed
        let link = link(&fx);
        assert!(link.call(Request::GetContext).await.is_ok());
        other.shutdown();
        drop(other);
        assert!(link.call(Request::GetContext).await.is_ok());
        assert_eq!(fx.bridge.status().connections, 1);
        fx.bridge.shutdown();
    }

    fn listen_error(source: io::Error) -> StartError {
        StartError::Listen(ListenError::Io {
            place: "the place".to_owned(),
            source,
        })
    }

    /// The interface words each kind in its own language, so the kinds have to tell the causes apart.
    #[test]
    fn a_connection_that_cannot_start_says_why_in_kinds_the_interface_can_word() {
        let kind = |error: &StartError| start_failure(error).kind;
        assert_eq!(
            kind(&StartError::Listen(ListenError::InUse("socket".to_owned()))),
            FailureKind::InUse
        );
        assert_eq!(
            kind(&listen_error(io::Error::from(io::ErrorKind::AddrInUse))),
            FailureKind::InUse
        );
        assert_eq!(
            kind(&StartError::Listen(ListenError::Socket(
                SocketError::PathTooLong("a/long/path".to_owned())
            ))),
            FailureKind::PathTooLong
        );
        let folder = SocketError::Folder {
            path: "/run/user/1000/trailmark".to_owned(),
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        };
        assert_eq!(socket_failure(&folder).kind, FailureKind::FolderNotPrivate);
        assert_eq!(
            kind(&StartError::Listen(ListenError::Socket(folder))),
            FailureKind::FolderNotPrivate
        );
        // anything else keeps the system's words
        let other = start_failure(&listen_error(io::Error::from(io::ErrorKind::NotFound)));
        assert_eq!(other.kind, FailureKind::Other);
        assert!(other.message.contains("the place"), "{}", other.message);
        assert_eq!(
            kind(&StartError::Thread(io::Error::other("no thread"))),
            FailureKind::Other
        );
    }

    /// What Windows says when the name of the pipe is held by another program: "Access is denied. (os error 5)".
    #[cfg(windows)]
    #[test]
    fn a_pipe_that_another_program_holds_is_in_use_not_a_permission_problem() {
        for code in [5, 231] {
            let error = listen_error(io::Error::from_raw_os_error(code));
            assert_eq!(start_failure(&error).kind, FailureKind::InUse, "{code}");
        }
        let other = listen_error(io::Error::from_raw_os_error(2));
        assert_eq!(start_failure(&other).kind, FailureKind::Other);
    }

    /// A connection that could not start is "on, could not start" until the person turns it off; off clears that, and on
    /// again tries again.
    #[test]
    fn turning_it_off_clears_a_connection_that_could_not_start_and_turning_it_on_retries() {
        let fx = fx("failed", None);
        assert!(fx.bridge.set_enabled(true).listening);
        let vault = Arc::new(Vault::open(fx.project.path()).expect("vault"));
        let other = AgentBridge::new(
            vault,
            Endpoint::in_folder(fx.sockets.path(), Path::new("failed"), "tester"),
            None,
            fx.fake.clone(),
            fx.fake.clone(),
        );
        let status = other.set_enabled(true);
        assert!(status.enabled && !status.listening, "{status:?}");
        assert!(status.error.is_some());

        let status = other.set_enabled(false);
        assert!(
            !status.enabled && !status.listening && status.error.is_none(),
            "off leaves nothing of the failure: {status:?}"
        );
        assert_eq!(other.status(), status);

        // Still taken: on says so again. Free: on listens.
        assert!(other.set_enabled(true).error.is_some());
        fx.bridge.set_enabled(false);
        let status = other.set_enabled(true);
        assert!(status.enabled && status.listening, "{status:?}");
        assert!(status.error.is_none());
        other.shutdown();
    }

    #[test]
    fn the_state_file_exists_only_while_the_connection_is_on() {
        let state = tempfile::tempdir().expect("state");
        let path = state.path().join("local").join("agent.json");
        let fx = fx("file", Some(path.clone()));
        fx.bridge.set_enabled(true);
        fx.bridge.publish(AgentContext {
            document: Some("docs/plan.md".to_owned()),
            unsaved: true,
            ..AgentContext::default()
        });
        fx.bridge.flush_file();
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("the file exists")).expect("json");
        assert_eq!(written["document"], "docs/plan.md");
        assert_eq!(written["unsaved"], true);
        assert!(written["updatedAtMs"].as_u64().is_some());
        fx.bridge.set_enabled(false);
        assert!(!path.exists(), "turning it off deletes the file");
    }

    #[test]
    fn a_state_file_left_by_a_killed_window_is_removed_when_the_project_opens_with_the_connection_off(
    ) {
        let state = tempfile::tempdir().expect("state");
        let path = state.path().join("agent.json");
        std::fs::write(&path, "{}").expect("write");
        let fx = fx("stale", Some(path.clone()));
        fx.bridge.remove_stale_file();
        assert!(!path.exists());
        // nothing there is nothing to do
        fx.bridge.remove_stale_file();
        let without = self::fx("stale2", None);
        without.bridge.remove_stale_file();
    }

    #[test]
    fn a_stale_state_file_is_not_deleted_behind_a_link() {
        use crate::util::testing::{dir_link, remove_link};

        let state = tempfile::tempdir().expect("state");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("agent.json"), "not ours").expect("write");
        let trailmark = state.path().join(".trailmark");
        std::fs::create_dir(&trailmark).expect("mkdir");
        dir_link(&trailmark.join("local"), outside.path());
        let fx = fx("linked", Some(trailmark.join("local").join("agent.json")));
        fx.bridge.remove_stale_file();
        assert!(outside.path().join("agent.json").is_file());
        remove_link(&trailmark.join("local"));
    }

    #[test]
    fn a_project_without_a_state_folder_has_no_state_file() {
        let fx = fx("nofile", None);
        fx.bridge.set_enabled(true);
        fx.bridge.publish(AgentContext::default());
        fx.bridge.set_enabled(false);
        assert!(!fx.project.path().join(".trailmark").exists());
    }
}
