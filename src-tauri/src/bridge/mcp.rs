//! `trailmark mcp`: TrailMark as a Model Context Protocol server over standard input and output.
//!
//! An agent starts the program with the `mcp` argument and talks to it in newline-delimited JSON-RPC; no window opens.
//! The tools only translate: what they do is in [`Core`](super::tools::Core). The server is generic over the
//! transport, so the tests drive it over an in-memory pipe and the program gives it the real standard streams
//! ([`run_stdio`]).

use std::io::Write;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::{NotificationContext, ServerInitializeError};
use rmcp::transport::{stdio, IntoTransport};
use rmcp::{tool, tool_handler, tool_router, ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde::Deserialize;

use crate::comments::CommentFilter;

use super::tools::{launch_app, Answer, Core};

/// Why the MCP server stopped with an error.
#[derive(Debug, thiserror::Error)]
pub(crate) enum McpError {
    #[error("could not read the working folder: {0}")]
    WorkingFolder(#[source] std::io::Error),
    #[error("could not start the server: {0}")]
    Runtime(#[source] std::io::Error),
    // Boxed: the handshake error is large, and this enum is returned by value.
    #[error("the client did not complete the MCP handshake: {0}")]
    Handshake(#[source] Box<ServerInitializeError>),
    #[error("the server stopped unexpectedly: {0}")]
    Stopped(#[from] tokio::task::JoinError),
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct SearchArgs {
    /// What to look for: keywords or a short phrase, as you would type them into a search box.
    query: String,
    /// How many sections to return (1 to 25). Ten when left out.
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ListCommentsArgs {
    /// Which comments: `open` (the default), `done` or `all`.
    which: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ResolveCommentArgs {
    /// The comment's `id`, as `list_comments` gives it.
    id: String,
    /// A short message to the person saying what you did, such as `Rewritten in 3f2a1c`. They read it under the comment.
    reply: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct OpenArgs {
    /// The document, relative to the project folder, with `/` separators (for example `docs/plan.md`).
    path: String,
    /// A line of the document to show (from 1). Optional.
    line: Option<u32>,
    /// A heading of the document to show, such as `Rollout`, or a path of headings such as `Plan > Rollout`.
    /// Wins over `line` when the document has it. Optional.
    heading: Option<String>,
}

/// What an agent can ask TrailMark. The project is the folder the agent works in.
#[derive(Clone)]
struct AgentServer {
    core: Arc<Core>,
    tool_router: ToolRouter<Self>,
}

fn result(answer: Answer) -> CallToolResult {
    if answer.is_error {
        CallToolResult::structured_error(answer.value)
    } else {
        CallToolResult::structured(answer.value)
    }
}

// The tool macro requires async methods; a tool that only asks the core still awaits it.
#[tool_router]
impl AgentServer {
    fn new(core: Arc<Core>) -> Self {
        Self {
            core,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "What the person is looking at in TrailMark right now: the open document (path relative to \
the project), whether they are reading or editing it, the section on screen (heading path and line range), the \
text they selected (with its section and lines) and whether it has unsaved edits. Call this when the person \
says \"this\", \"here\", \"the selected text\", \"this section\", \"what I'm reading\" or otherwise points at \
something on their screen without naming it. Needs TrailMark open on the project with its agent connection on; \
otherwise it says so.",
        annotations(read_only_hint = true)
    )]
    async fn get_context(&self) -> Result<CallToolResult, ErrorData> {
        Ok(result(self.core.get_context().await))
    }

    #[tool(
        description = "The Markdown documents that changed since the person last read them in TrailMark, newest \
change first, with the time of the change. Call this when the person asks what changed, what they have not read \
yet, or to catch up (for example after you or another agent edited documents). Also works when TrailMark is \
closed, from the project's files.",
        annotations(read_only_hint = true)
    )]
    async fn list_unread(&self) -> Result<CallToolResult, ErrorData> {
        Ok(result(self.core.list_unread().await))
    }

    #[tool(
        description = "Find where something is described in the project's Markdown documents. Returns the best \
matching sections ranked, each with its path, heading path, line and a short snippet (the same index as \
TrailMark's Search everywhere). Use it to locate documentation before reading whole files, for example to find \
the section that defines a term or a decision. Also works when TrailMark is closed, from the project's files.",
        annotations(read_only_hint = true)
    )]
    async fn search(
        &self,
        Parameters(SearchArgs { query, limit }): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(result(self.core.search(&query, limit).await))
    }

    #[tool(
        description = "The comments the person left in TrailMark on passages of the project's Markdown \
documents (\"this contradicts section 2\", \"simplify\"). Each has an id, the document path, the quoted \
passage, its heading path and line, the comment text, when it was made, and `state`: `anchored` when the quote is \
still in the document (the line is where it is now), `passageChanged` when the passage was rewritten (the line \
is the best guess) or `documentGone`. Open comments by default; pass `which` = `done` or `all` for others. Call \
this when the person asks you to go through, address or apply their comments on the documents. Also \
works when TrailMark is closed, from the project's files.",
        annotations(read_only_hint = true)
    )]
    async fn list_comments(
        &self,
        Parameters(ListCommentsArgs { which }): Parameters<ListCommentsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let filter = match which.as_deref().map(str::trim) {
            None | Some("" | "open") => Ok(CommentFilter::Open),
            Some("done") => Ok(CommentFilter::Done),
            Some("all") => Ok(CommentFilter::All),
            Some(other) => Err(format!("`which` is open, done or all, not {other:?}")),
        };
        Ok(result(match filter {
            Ok(filter) => self.core.list_comments(Some(filter)).await,
            Err(message) => Answer::failed(message),
        }))
    }

    #[tool(
        description = "Mark one comment done after you addressed it, with an optional short `reply` saying \
what you did (\"Rewritten in 3f2a1c\"), which TrailMark shows under the comment. Use the `id` from list_comments. This \
is the only change an agent can make through TrailMark: it cannot write comments or edit documents. Also works \
when TrailMark is closed, on the project's files.",
        annotations(read_only_hint = false, destructive_hint = false)
    )]
    async fn resolve_comment(
        &self,
        Parameters(ResolveCommentArgs { id, reply }): Parameters<ResolveCommentArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(result(
            self.core.resolve_comment(&id, reply.as_deref()).await,
        ))
    }

    #[tool(
        description = "Show the person a place in TrailMark: a document, optionally at a line or a heading. \
TrailMark opens it in its window without taking focus from your terminal, highlights the section and shows a \
notice with a way back; the person looks when ready. Use it after you wrote or found something they should see \
(\"see the new section in docs/plan.md\"). It starts TrailMark on the project if it is closed. It only shows; \
it never changes a document.",
        annotations(read_only_hint = false, destructive_hint = false)
    )]
    async fn open(
        &self,
        Parameters(OpenArgs {
            path,
            line,
            heading,
        }): Parameters<OpenArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(result(
            self.core.open(&path, line, heading.as_deref()).await,
        ))
    }
}

#[allow(clippy::unused_async_trait_impl)] // The tool macro writes async handlers that do not wait.
#[tool_handler(
    router = self.tool_router,
    name = "trailmark",
    instructions = "TrailMark reads and follows the Markdown documents of a project. These tools let you see what \
the person is looking at in TrailMark, what changed since they last read, where something is described, the \
comments they left on passages (and mark one done), and show them a place. They are available only while the person \
has turned on the agent connection for the project."
)]
impl ServerHandler for AgentServer {
    // The client is ready: connect to the window now, so it shows that an agent is here before the first question.
    async fn on_initialized(&self, _context: NotificationContext<RoleServer>) {
        self.core.keep_connected().await;
    }
}

/// Serves MCP for the project of `core` over `transport` until the client closes it.
pub(crate) async fn serve<T, E, A>(core: Arc<Core>, transport: T) -> Result<(), McpError>
where
    T: IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let running = AgentServer::new(core)
        .serve(transport)
        .await
        .map_err(|error| McpError::Handshake(Box::new(error)))?;
    running.waiting().await?;
    Ok(())
}

/// Serves MCP over the program's standard input and output until the input closes.
pub(crate) fn run_stdio(core: Core) -> Result<(), McpError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(McpError::Runtime)?;
    let outcome = runtime.block_on(serve(Arc::new(core), stdio()));
    // A read of the standard input can still be pending on a helper thread; do not wait for it.
    runtime.shutdown_background();
    outcome
}

/// The `trailmark mcp` command: serves the project of the working folder and returns the exit code. `identifier` is
/// the app's, which names its data folder. Standard output carries only protocol messages, so a failure is
/// reported on standard error.
pub(crate) fn run_cli(identifier: &str) -> i32 {
    let outcome = std::env::current_dir()
        .map_err(McpError::WorkingFolder)
        .and_then(|folder| {
            let core = Core::discover(identifier, &folder, Arc::new(launch_app));
            run_stdio(core)
        });
    match outcome {
        Ok(()) => 0,
        Err(error) => {
            // The streams may not exist (a program without a console); there is nothing else to do then.
            let _ = writeln!(std::io::stderr(), "trailmark mcp: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use serde_json::{json, Value};
    use tokio::io::{
        duplex, split, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf,
    };
    use tokio::task::JoinHandle;
    use tokio::time::timeout;

    use super::*;
    use crate::bridge::context::{AgentContext, ViewMode};
    use crate::bridge::hub::testing::{comment, Fake};
    use crate::bridge::service::AgentBridge;
    use crate::bridge::socket::Endpoint;
    use crate::bridge::tools::Launcher;
    use crate::session::StateStore;
    use crate::vault::Vault;

    const WAIT: Duration = Duration::from_secs(10);

    /// A project, the window of that project (a bridge), and what an agent's program needs to reach both.
    struct World {
        app: tempfile::TempDir,
        _project: tempfile::TempDir,
        sockets: tempfile::TempDir,
        root: std::path::PathBuf,
        fake: Arc<Fake>,
        bridge: Arc<AgentBridge>,
    }

    impl World {
        fn new() -> Self {
            let app = tempfile::tempdir().expect("app data");
            let project = tempfile::tempdir().expect("project");
            std::fs::create_dir_all(project.path().join("docs")).expect("mkdir");
            std::fs::write(
                project.path().join("docs/plan.md"),
                "# Plan\n\nIntro.\n\n## Rollout\n\nShip it.\n",
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
            Self {
                app,
                _project: project,
                sockets,
                root,
                fake,
                bridge,
            }
        }

        fn connection(&self, on: bool) {
            StateStore::open(self.app.path(), &self.root)
                .set_agent_connection(on)
                .expect("remember the switch");
        }

        fn core(&self) -> Arc<Core> {
            let sockets = self.sockets.path().to_path_buf();
            let launcher: Launcher = Arc::new(|_| panic!("TrailMark must not be started"));
            Arc::new(Core::new(
                self.root.clone(),
                Some(self.app.path().to_path_buf()),
                true,
                move |root: &Path| Endpoint::in_folder(&sockets, root, "tester"),
                launcher,
            ))
        }
    }

    /// A raw MCP client: newline-delimited JSON-RPC over an in-memory pipe, as an agent's MCP client speaks it.
    struct Client {
        reader: BufReader<ReadHalf<DuplexStream>>,
        writer: WriteHalf<DuplexStream>,
        server: JoinHandle<Result<(), McpError>>,
    }

    impl Client {
        fn connect(core: Arc<Core>) -> Self {
            let (server_side, client_side) = duplex(64 * 1024);
            let server = tokio::spawn(serve(core, server_side));
            let (read, writer) = split(client_side);
            Self {
                reader: BufReader::new(read),
                writer,
                server,
            }
        }

        async fn send(&mut self, message: &Value) {
            let mut line = message.to_string();
            line.push('\n');
            self.writer
                .write_all(line.as_bytes())
                .await
                .expect("the pipe is open");
            self.writer.flush().await.expect("the pipe is open");
        }

        /// The next message from the server; `None` when the server closed its side.
        async fn receive(&mut self) -> Option<Value> {
            let mut line = String::new();
            let read = timeout(WAIT, self.reader.read_line(&mut line))
                .await
                .expect("the server answered in time")
                .expect("a line was read");
            (read > 0).then(|| serde_json::from_str(&line).expect("the server sends JSON"))
        }

        async fn request(&mut self, id: u64, method: &str, params: &Value) -> Value {
            self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
                .await;
            loop {
                let message = self.receive().await.expect("the server stayed open");
                if message["id"] == id {
                    return message;
                }
            }
        }

        async fn handshake(&mut self) -> Value {
            let reply = self
                .request(
                    1,
                    "initialize",
                    &json!({
                        "protocolVersion": "2025-11-25",
                        "capabilities": {},
                        "clientInfo": { "name": "test-client", "version": "0.0.0" }
                    }),
                )
                .await;
            self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
                .await;
            reply
        }

        async fn call(&mut self, id: u64, tool: &str, arguments: &Value) -> Value {
            self.request(
                id,
                "tools/call",
                &json!({ "name": tool, "arguments": arguments }),
            )
            .await
        }

        /// Closes the client's side (as an agent does when it exits) and returns how the server ended.
        async fn close(mut self) -> Result<(), McpError> {
            // Shutting the write half down is what closing the pipe's end does; the read half keeps the stream itself.
            self.writer.shutdown().await.expect("the pipe is open");
            timeout(WAIT, self.server)
                .await
                .expect("the server stopped when its input closed")
                .expect("the server task did not panic")
        }
    }

    #[tokio::test]
    async fn handshake_announces_trailmark_and_its_tools() {
        let world = World::new();
        let mut client = Client::connect(world.core());
        let reply = client.handshake().await;
        let result = &reply["result"];
        assert_eq!(result["serverInfo"]["name"], "trailmark");
        assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
        assert!(result["capabilities"]["tools"].is_object());
        client.close().await.expect("the server ended cleanly");
    }

    /// A client that asks before running a tool that changes something can run the others without asking.
    #[tokio::test]
    async fn the_tools_say_which_of_them_change_anything() {
        let world = World::new();
        let mut client = Client::connect(world.core());
        client.handshake().await;
        let reply = client.request(2, "tools/list", &json!({})).await;
        let tools = reply["result"]["tools"].as_array().expect("a list");
        let annotations = |name: &str| {
            tools
                .iter()
                .find(|t| t["name"] == name)
                .map(|t| t["annotations"].clone())
                .expect("a tool")
        };
        for name in ["get_context", "list_unread", "search", "list_comments"] {
            assert_eq!(annotations(name)["readOnlyHint"], json!(true), "{name}");
        }
        // Showing a place and marking a comment done are changes, but they destroy nothing.
        for name in ["open", "resolve_comment"] {
            let hints = annotations(name);
            assert_eq!(hints["readOnlyHint"], json!(false), "{name}");
            assert_eq!(hints["destructiveHint"], json!(false), "{name}");
        }
    }

    #[tokio::test]
    async fn the_tools_say_when_an_agent_should_call_them() {
        let world = World::new();
        let mut client = Client::connect(world.core());
        client.handshake().await;
        let reply = client.request(2, "tools/list", &json!({})).await;
        let tools = reply["result"]["tools"].as_array().expect("a list");
        let names: Vec<_> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("name"))
            .collect();
        assert_eq!(names.len(), 6, "{names:?}");
        for name in [
            "get_context",
            "list_unread",
            "search",
            "list_comments",
            "resolve_comment",
            "open",
        ] {
            assert!(names.contains(&name), "{name} is missing from {names:?}");
        }
        let description = |name: &str| {
            tools
                .iter()
                .find(|t| t["name"] == name)
                .and_then(|t| t["description"].as_str())
                .expect("a description")
                .to_owned()
        };
        // The wording that makes an agent reach for the tool at the right moment.
        assert!(description("get_context").contains("\"this\""));
        assert!(description("get_context").contains("selected text"));
        assert!(description("list_unread").contains("what changed"));
        assert!(description("search").contains("locate documentation"));
        assert!(description("open").contains("without taking focus"));
        assert!(description("list_comments").contains("go through"));
        assert!(description("list_comments").contains("passageChanged"));
        assert!(description("resolve_comment").contains("only change an agent can make"));
        // Arguments are described to the agent.
        let search = tools
            .iter()
            .find(|t| t["name"] == "search")
            .expect("search");
        assert_eq!(search["inputSchema"]["required"], json!(["query"]));
        assert!(search["inputSchema"]["properties"]["query"]["description"]
            .as_str()
            .is_some());
        let open = tools.iter().find(|t| t["name"] == "open").expect("open");
        assert_eq!(open["inputSchema"]["required"], json!(["path"]));
        let resolve = tools
            .iter()
            .find(|t| t["name"] == "resolve_comment")
            .expect("resolve_comment");
        assert_eq!(resolve["inputSchema"]["required"], json!(["id"]));
        assert!(resolve["inputSchema"]["properties"]["reply"]["description"]
            .as_str()
            .is_some());
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn every_tool_answers_that_the_connection_is_off() {
        let world = World::new();
        world.connection(false);
        let mut client = Client::connect(world.core());
        client.handshake().await;
        for (id, tool, arguments) in [
            (3, "get_context", json!({})),
            (4, "list_unread", json!({})),
            (5, "search", json!({ "query": "plan" })),
            (6, "open", json!({ "path": "docs/plan.md" })),
            (7, "list_comments", json!({})),
            (8, "resolve_comment", json!({ "id": "c1", "reply": "Done" })),
        ] {
            let reply = client.call(id, tool, &arguments).await;
            let answer = &reply["result"];
            assert_eq!(answer["isError"], false, "{tool}: {reply}");
            assert_eq!(answer["structuredContent"]["status"], "off", "{tool}");
            assert!(answer["structuredContent"]["message"]
                .as_str()
                .expect("message")
                .contains("Agent connection"));
        }
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn get_context_over_mcp_reaches_the_window_through_the_socket() {
        let world = World::new();
        world.connection(true);
        world.bridge.set_enabled(true);
        world.bridge.publish(AgentContext {
            document: Some("docs/plan.md".to_owned()),
            mode: Some(ViewMode::Source),
            unsaved: true,
            ..AgentContext::default()
        });
        let mut client = Client::connect(world.core());
        client.handshake().await;
        let reply = client.call(3, "get_context", &json!({})).await;
        let answer = &reply["result"];
        assert_eq!(answer["isError"], false);
        let context = &answer["structuredContent"];
        assert_eq!(context["status"], "ok");
        assert_eq!(context["document"], "docs/plan.md");
        assert_eq!(context["mode"], "source");
        assert_eq!(context["unsaved"], true);
        // The same answer as text, for clients that read only text.
        let text = answer["content"][0]["text"].as_str().expect("text content");
        assert_eq!(
            serde_json::from_str::<Value>(text).expect("the text is JSON"),
            *context
        );
        client.close().await.expect("the server ended cleanly");
        world.bridge.shutdown();
    }

    #[tokio::test]
    async fn with_the_window_closed_get_context_says_trailmark_is_not_open() {
        let world = World::new();
        world.connection(true);
        let mut client = Client::connect(world.core());
        client.handshake().await;
        let reply = client.call(3, "get_context", &json!({})).await;
        let answer = &reply["result"]["structuredContent"];
        assert_eq!(answer["status"], "notOpen");
        assert!(answer["message"]
            .as_str()
            .expect("a message")
            .starts_with("TrailMark is not open on this project."));
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn open_over_mcp_is_delivered_to_the_window() {
        let world = World::new();
        world.connection(true);
        world.bridge.set_enabled(true);
        let mut client = Client::connect(world.core());
        client.handshake().await;
        let reply = client
            .call(
                3,
                "open",
                &json!({ "path": "docs/plan.md", "heading": "Rollout" }),
            )
            .await;
        let answer = &reply["result"];
        assert_eq!(answer["isError"], false, "{reply}");
        assert_eq!(answer["structuredContent"]["status"], "ok");
        assert_eq!(answer["structuredContent"]["line"], 5);
        let place = world
            .bridge
            .take_open()
            .expect("the window was asked to open it");
        assert_eq!((place.path.as_str(), place.line), ("docs/plan.md", Some(5)));
        assert_eq!(
            world.fake.opens.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the interface was told once"
        );

        // A place outside the project is an error the agent can read.
        let reply = client
            .call(4, "open", &json!({ "path": "../elsewhere.md" }))
            .await;
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(reply["result"]["structuredContent"]["status"], "error");
        client.close().await.expect("the server ended cleanly");
        world.bridge.shutdown();
    }

    #[tokio::test]
    async fn comments_are_listed_and_resolved_over_mcp() {
        let world = World::new();
        world.connection(true);
        world.bridge.set_enabled(true);
        *world.fake.comments.lock().expect("lock") = vec![comment("c1", "docs/plan.md", 5)];
        let mut client = Client::connect(world.core());
        client.handshake().await;

        let reply = client.call(3, "list_comments", &json!({})).await;
        let answer = &reply["result"]["structuredContent"];
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert_eq!(answer["status"], "ok");
        assert_eq!(answer["total"], 1);
        assert_eq!(answer["comments"][0]["id"], "c1");
        assert_eq!(answer["comments"][0]["state"], "anchored");

        let reply = client
            .call(
                4,
                "resolve_comment",
                &json!({ "id": "c1", "reply": "Fixed" }),
            )
            .await;
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert_eq!(
            reply["result"]["structuredContent"]["comment"]["done"],
            true
        );
        assert_eq!(
            reply["result"]["structuredContent"]["comment"]["reply"],
            "Fixed"
        );

        // done comments are listed when asked for
        let reply = client
            .call(5, "list_comments", &json!({ "which": "done" }))
            .await;
        assert_eq!(reply["result"]["structuredContent"]["total"], 1);
        let reply = client.call(6, "list_comments", &json!({})).await;
        assert_eq!(reply["result"]["structuredContent"]["total"], 0);

        // arguments that no comment fits are errors an agent can read
        let reply = client
            .call(7, "list_comments", &json!({ "which": "bogus" }))
            .await;
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        let reply = client
            .call(8, "resolve_comment", &json!({ "id": "nope" }))
            .await;
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        let reply = client.call(9, "resolve_comment", &json!({})).await;
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        client.close().await.expect("the server ended cleanly");
        world.bridge.shutdown();
    }

    #[tokio::test]
    async fn the_window_sees_the_agent_connect_when_the_client_is_ready() {
        let world = World::new();
        world.connection(true);
        world.bridge.set_enabled(true);
        let mut client = Client::connect(world.core());
        client.handshake().await;
        // Whether the connection was made when the client said it was ready or by the first question, one
        // agent is one connection: the window counts it once.
        client.call(2, "get_context", &json!({})).await;
        assert_eq!(world.bridge.status().connections, 1);
        client.close().await.expect("the server ended cleanly");
        world.bridge.shutdown();
    }

    #[tokio::test]
    async fn an_unknown_tool_is_an_error_reply() {
        let world = World::new();
        let mut client = Client::connect(world.core());
        client.handshake().await;
        let reply = client.call(4, "nope", &json!({})).await;
        assert!(reply["error"].is_object(), "got {reply}");
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn arguments_that_do_not_fit_are_an_error_reply() {
        let world = World::new();
        let mut client = Client::connect(world.core());
        client.handshake().await;
        let reply = client.call(4, "search", &json!({ "limit": 3 })).await;
        assert_eq!(reply["result"]["isError"], true, "got {reply}");
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert!(
            text.contains("query"),
            "the missing argument is named: {text}"
        );
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn the_server_ends_when_the_client_closes_the_connection() {
        let world = World::new();
        let mut client = Client::connect(world.core());
        client.handshake().await;
        assert!(client.close().await.is_ok());
    }

    #[tokio::test]
    async fn closing_before_the_handshake_is_reported() {
        let world = World::new();
        let client = Client::connect(world.core());
        let outcome = client.close().await;
        assert!(
            matches!(outcome, Err(McpError::Handshake(_))),
            "got {outcome:?}"
        );
    }
}
