//! The agent connection of one open project: the switch, the socket while it is on, the state file, and what the
//! window publishes. [`AgentBridge`] ties the pieces together; the pieces are tested on their own.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::vault::Vault;

use super::context::AgentContext;
use super::file::{ContextFile, WRITE_INTERVAL};
use super::hub::{AgentEvents, AgentStatus, Backend, Hub};
use super::protocol::Opened;
use super::server::BridgeServer;
use super::socket::{Endpoint, SocketError};
use super::tools::program_path;

/// What exists while the connection is on.
struct Running {
    server: Option<BridgeServer>,
    file: Option<ContextFile>,
}

pub struct AgentBridge {
    hub: Arc<Hub>,
    endpoint: Result<Endpoint, String>,
    /// Where `agent.json` goes: only a project that keeps its state in `.trailmark/` has one.
    agent_path: Mutex<Option<PathBuf>>,
    running: Mutex<Option<Running>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
            endpoint: endpoint.map_err(|error| error.to_string()),
            agent_path: Mutex::new(agent_path),
            running: Mutex::new(None),
        }
    }

    /// Turns the connection on or off. A connection that cannot start (the socket cannot be made) stays "on" as
    /// a setting and reports why in the status, instead of failing the caller.
    pub fn set_enabled(&self, enabled: bool) -> AgentStatus {
        let mut running = lock(&self.running);
        if enabled == running.is_some() {
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

    fn start(&self) -> Result<Running, String> {
        let endpoint = self.endpoint.as_ref().map_err(Clone::clone)?;
        let server =
            BridgeServer::start(endpoint, Arc::clone(&self.hub)).map_err(|e| e.to_string())?;
        let file = lock(&self.agent_path)
            .clone()
            .map(|path| ContextFile::start(path, WRITE_INTERVAL, self.hub.context()));
        Ok(Running {
            server: Some(server),
            file,
        })
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
        let sockets = tempfile::Builder::new()
            .prefix("tm")
            .tempdir_in(std::env::temp_dir())
            .expect("sockets");
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
        if cfg!(windows) {
            // A named pipe name is exclusive; elsewhere the newer listener replaces a leftover socket file.
            assert!(status.enabled && !status.listening, "{status:?}");
            assert!(status.error.is_some());
        }
        fx.bridge.shutdown();
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
    fn a_project_without_a_state_folder_has_no_state_file() {
        let fx = fx("nofile", None);
        fx.bridge.set_enabled(true);
        fx.bridge.publish(AgentContext::default());
        fx.bridge.set_enabled(false);
        assert!(!fx.project.path().join(".trailmark").exists());
    }
}
