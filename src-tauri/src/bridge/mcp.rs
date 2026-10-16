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
    /// What to look for: keywords or a short phrase, as you would type them into a search box. Leave it out when you
    /// pass `tags`, to get the documents that have those tags instead of sections.
    query: Option<String>,
    /// How many sections to return (1 to 25). Ten when left out. With `tags` and no `query` it is how many documents
    /// to list instead (up to 100, twenty when left out).
    limit: Option<usize>,
    /// Look only in the documents that have all of these tags, as `list_tags` names them (for example `["adr"]`).
    /// Optional.
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ListTagsArgs {
    /// A tag, as `list_tags` names it without this argument. With it, the answer lists the documents that have that
    /// tag (path, description and all of their tags) instead of the tags. Optional.
    tag: Option<String>,
    /// How many tags (or, with `tag`, documents) to list at most: 50 tags or 20 documents when left out, up to 200
    /// tags or 100 documents. `more` in the answer says how many were left out.
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
struct ReadNoteArgs {
    /// The note's name, exactly as `list_notes` gives it in `note` (for example `2026-10-15 1432.md`).
    note: String,
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
TrailMark's Search everywhere), and the `tags` and `description` of its document when it has them. Use it to \
locate documentation before reading whole files, for example to find the section that defines a term or a \
decision. In a large project, narrow first: pass `tags` (see list_tags) to look only in the documents that have \
all of them, then read just the few that fit. Leave `query` out when you pass `tags` to get those documents \
(path, description, tags) instead of sections. Also works when TrailMark is closed, from the project's files.",
        annotations(read_only_hint = true)
    )]
    async fn search(
        &self,
        Parameters(SearchArgs { query, limit, tags }): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(result(
            self.core
                .search(
                    query.as_deref().unwrap_or_default(),
                    limit,
                    tags.as_deref().unwrap_or_default(),
                )
                .await,
        ))
    }

    #[tool(
        description = "The tags in use in the project's Markdown documents: each tag with how many documents have it \
(`count`) and where it is written (`from`: `frontMatter` in the documents, `trailmark` added by the person in \
TrailMark, or `both`), most used first (the first 50; `more` says how many were left out), plus `documents` (how \
many Markdown documents there are) and `untagged` (how many have no tag), so you can tell whether tags are worth \
using here. Pass `tag` to list the documents that have it instead: each with its path, its one-line `description` \
(left out when it has none) and all of its tags (the first 20; `total` and `more` count the rest). `limit` asks for \
more or fewer. Use it to find the right documents in two calls in a large project: call list_tags (or search with \
`tags`) to narrow down, then read only the few documents that fit, instead of going through many search hits. \
Also works when TrailMark is closed, from the project's files.",
        annotations(read_only_hint = true)
    )]
    async fn list_tags(
        &self,
        Parameters(ListTagsArgs { tag, limit }): Parameters<ListTagsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(result(self.core.list_tags(tag.as_deref(), limit).await))
    }

    #[tool(
        description = "The comments the person left in TrailMark on passages of the project's Markdown \
documents (\"this contradicts section 2\", \"simplify\"). Each has an id, the document path, the quoted \
passage, its heading path and line, the comment text, when it was made, and `state`: `anchored` when the quote is \
still in the document (the line is where it is now), `passageChanged` when the passage was rewritten (the line \
is the best guess) or `documentGone`. Open comments by default; pass `which` = `done` or `all` for others. Call \
this when the person asks you to go through, address or apply their comments on the documents. A `warning` \
says when the comments file could not be read, which is why the list is empty. Also works when TrailMark is \
closed, from the project's files.",
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
        description = "The notes the person chose to share with agents in TrailMark (its Notes window keeps their \
quick notes, drafts and prompts, and nothing in it is visible to you unless they shared that note). Each has `note` \
(its name, which read_note takes), `title` (its first line), when it was last changed and its size, newest first. \
Call this when the person points at their notes (\"my note about the deploy\", \"the prompt I wrote\", \"what I \
jotted down\"). A note that is not shared is not listed and cannot be read; you cannot list the notes folder. Also \
works when TrailMark is closed.",
        annotations(read_only_hint = true)
    )]
    async fn list_notes(&self) -> Result<CallToolResult, ErrorData> {
        Ok(result(self.core.list_notes().await))
    }

    #[tool(
        description = "Read one of the notes the person shared with agents, by the `note` name that list_notes gives. \
Returns its title and its whole text. It only reads: it cannot change a note, and a note that is not shared is said \
not to exist. Also works when TrailMark is closed.",
        annotations(read_only_hint = true)
    )]
    async fn read_note(
        &self,
        Parameters(ReadNoteArgs { note }): Parameters<ReadNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(result(self.core.read_note(&note).await))
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
the person is looking at in TrailMark, what changed since they last read, where something is described, which \
tags the documents carry (to narrow a large project down before you read), the comments they left on passages \
(and mark one done), the notes they chose to share with agents, and show them a place. They are available only \
while the person has turned on the agent connection for the project."
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
    use std::fmt::Write as _;
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
    use crate::bridge::tags::Doc;
    use crate::bridge::tools::Launcher;
    use crate::meta::store::Entries;
    use crate::search::SearchHit;
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
        for name in [
            "get_context",
            "list_unread",
            "search",
            "list_tags",
            "list_comments",
            "list_notes",
            "read_note",
        ] {
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
        assert_eq!(names.len(), 9, "{names:?}");
        for name in [
            "get_context",
            "list_unread",
            "search",
            "list_tags",
            "list_comments",
            "resolve_comment",
            "open",
            "list_notes",
            "read_note",
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
        // the two steps that narrow a large project down are taught by both tools that take part in them
        for name in ["search", "list_tags"] {
            assert!(description(name).contains("list_tags"), "{name}");
            assert!(description(name).contains("`tags`"), "{name}");
            assert!(description(name).contains("read"), "{name}");
        }
        assert!(description("list_tags").contains("two calls"));
        assert!(description("list_tags").contains("`untagged`"));
        assert!(description("search").contains("Leave `query` out"));
        assert!(description("open").contains("without taking focus"));
        assert!(description("list_comments").contains("go through"));
        assert!(description("list_comments").contains("passageChanged"));
        assert!(description("resolve_comment").contains("only change an agent can make"));
        // the notes are private, and the tools say so to the agent
        assert!(description("list_notes").contains("chose to share"));
        assert!(description("list_notes").contains("not shared is not listed"));
        assert!(description("read_note").contains("cannot change a note"));
        let read_note = tools
            .iter()
            .find(|t| t["name"] == "read_note")
            .expect("read_note");
        assert_eq!(read_note["inputSchema"]["required"], json!(["note"]));
        // Arguments are described to the agent.
        let search = tools
            .iter()
            .find(|t| t["name"] == "search")
            .expect("search");
        // the words are optional now: tags alone list the documents that have them
        assert!(search["inputSchema"].get("required").is_none());
        for argument in ["query", "limit", "tags"] {
            assert!(
                search["inputSchema"]["properties"][argument]["description"]
                    .as_str()
                    .is_some(),
                "{argument}"
            );
        }
        assert_eq!(
            search["inputSchema"]["properties"]["tags"]["items"]["type"],
            "string"
        );
        let list_tags = tools
            .iter()
            .find(|t| t["name"] == "list_tags")
            .expect("list_tags");
        assert!(list_tags["inputSchema"].get("required").is_none());
        for argument in ["tag", "limit"] {
            assert!(
                list_tags["inputSchema"]["properties"][argument]["description"]
                    .as_str()
                    .is_some(),
                "{argument}"
            );
        }
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
            (11, "list_tags", json!({})),
            (12, "search", json!({ "tags": ["adr"] })),
            (6, "open", json!({ "path": "docs/plan.md" })),
            (7, "list_comments", json!({})),
            (8, "resolve_comment", json!({ "id": "c1", "reply": "Done" })),
            (9, "list_notes", json!({})),
            (10, "read_note", json!({ "note": "a.md" })),
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

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_owned()).collect()
    }

    fn doc(path: &str, file: &[&str], app: &[&str], description: &str) -> Doc {
        Doc {
            path: path.to_owned(),
            file_tags: names(file),
            app_tags: names(app),
            description: description.to_owned(),
        }
    }

    /// The answer of a tool as the text an agent reads, which is also the structured answer written without spaces.
    fn answer_text(reply: &Value) -> &str {
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .expect("text content");
        assert_eq!(
            text,
            reply["result"]["structuredContent"].to_string(),
            "the text is the structured answer, compact"
        );
        text
    }

    /// A project whose window is open and knows four documents, two of them tagged `adr`, and two sections that say "retry".
    fn world_with_tagged_documents() -> World {
        let world = World::new();
        world.connection(true);
        world.bridge.set_enabled(true);
        *world.fake.docs.lock().expect("lock") = vec![
            doc("docs/loud.md", &[], &[], ""),
            doc("docs/adr-1.md", &["adr"], &["api"], "Why retries back off."),
            doc("docs/adr-2.md", &["adr"], &[], ""),
            doc("docs/plan.md", &[], &["draft"], "The rollout plan."),
        ];
        *world.fake.corpus.lock().expect("lock") = vec![
            SearchHit {
                path: "docs/loud.md".to_owned(),
                heading_path: vec!["Retry".to_owned()],
                snippet: "retry retry retry".to_owned(),
                start_line: 1,
                score: 3.0,
            },
            SearchHit {
                path: "docs/adr-1.md".to_owned(),
                heading_path: vec!["Decision".to_owned()],
                snippet: "a <mark>retry</mark> policy".to_owned(),
                start_line: 7,
                score: 2.0,
            },
        ];
        world
    }

    #[tokio::test]
    async fn tags_narrow_what_an_agent_searches_over_mcp() {
        let world = world_with_tagged_documents();
        let mut client = Client::connect(world.core());
        client.handshake().await;

        // call one: how are the documents tagged?
        let reply = client.call(3, "list_tags", &json!({})).await;
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        let tags = &reply["result"]["structuredContent"];
        assert_eq!(tags["status"], "ok");
        assert_eq!(tags["source"], "window");
        assert_eq!(
            (&tags["documents"], &tags["untagged"]),
            (&json!(4), &json!(1))
        );
        assert_eq!(
            tags["tags"],
            json!([
                {"name": "adr", "count": 2, "from": "frontMatter"},
                {"name": "api", "count": 1, "from": "trailmark"},
                {"name": "draft", "count": 1, "from": "trailmark"},
            ])
        );
        let reply = client
            .call(4, "list_tags", &json!({ "tag": "ADR", "limit": 1 }))
            .await;
        let adr = &reply["result"]["structuredContent"];
        assert_eq!((&adr["total"], &adr["more"]), (&json!(2), &json!(1)));
        assert_eq!(
            adr["docs"],
            json!([{"path": "docs/adr-1.md", "description": "Why retries back off.", "tags": ["adr", "api"]}])
        );

        // call two: search where the tags point, and read the few documents it finds
        let reply = client
            .call(5, "search", &json!({ "query": "retry", "tags": ["adr"] }))
            .await;
        let found = &reply["result"]["structuredContent"];
        assert_eq!(found["source"], "window");
        let hits = found["hits"].as_array().expect("hits");
        assert_eq!(
            hits.len(),
            1,
            "the louder document has no such tag: {found}"
        );
        assert_eq!(hits[0]["path"], "docs/adr-1.md");
        assert_eq!(hits[0]["tags"], json!(["adr", "api"]));
        assert_eq!(hits[0]["description"], "Why retries back off.");
        assert_eq!(hits[0]["snippet"], "a retry policy");
        // without tags the same search finds both, and says what it knows of each
        let reply = client.call(6, "search", &json!({ "query": "retry" })).await;
        let both = reply["result"]["structuredContent"]["hits"]
            .as_array()
            .expect("hits")
            .clone();
        assert_eq!(both.len(), 2);
        assert!(both[0].get("tags").is_none() && both[0].get("description").is_none());

        // the window told its popover
        let recent = world.bridge.status().recent;
        assert!(recent.iter().any(|r| r.tool == "list_tags"));
        client.close().await.expect("the server ended cleanly");
        world.bridge.shutdown();
    }

    #[tokio::test]
    async fn tags_alone_list_their_documents_and_nothing_to_go_by_is_said_over_mcp() {
        let world = world_with_tagged_documents();
        let mut client = Client::connect(world.core());
        client.handshake().await;

        // tags without words list the documents that have them
        let reply = client.call(7, "search", &json!({ "tags": ["adr"] })).await;
        let listed = &reply["result"]["structuredContent"];
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert_eq!(listed["total"], 2);
        assert!(listed.get("hits").is_none());
        assert_eq!(listed["docs"][1]["path"], "docs/adr-2.md");

        // nothing to go by is an error that says what to give; a tag nobody has is an answer that says so
        let reply = client.call(8, "search", &json!({})).await;
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert!(reply["result"]["structuredContent"]["message"]
            .as_str()
            .expect("a message")
            .contains("tags"));
        let reply = client
            .call(9, "search", &json!({ "tags": ["nothing"] }))
            .await;
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert_eq!(reply["result"]["structuredContent"]["total"], 0);
        assert!(reply["result"]["structuredContent"]["message"]
            .as_str()
            .expect("a message")
            .contains("list_tags"));
        let reply = client
            .call(10, "search", &json!({ "query": "retry", "tags": "adr" }))
            .await;
        assert_eq!(reply["result"]["isError"], true, "tags are a list: {reply}");

        // the window told its popover, the way the search box takes tags
        let recent = world.bridge.status().recent;
        assert!(recent
            .iter()
            .any(|r| r.tool == "search" && r.subject.as_deref() == Some("tag:adr")));
        client.close().await.expect("the server ended cleanly");
        world.bridge.shutdown();
    }

    /// Tags written in two places, and the project's files only: no window, nothing written.
    #[tokio::test]
    async fn with_the_window_closed_the_tags_are_read_from_the_projects_files() {
        let world = World::new();
        world.connection(true);
        std::fs::write(
            world.root.join("docs/plan.md"),
            "---\ntags: [plan, Draft]\n---\n# Plan\n\nIntro.\n\n## Rollout\n\nShip it.\n",
        )
        .expect("write");
        std::fs::write(
            world.root.join("README.md"),
            "# Home\n\nShip the lighthouse.\n",
        )
        .expect("write");
        let mut entries = Entries::default();
        entries.set_tags("README.md", &names(&["draft", "Start Here"]));
        entries.set_description("README.md", "Where to begin.");
        // the project has no `.trailmark/` folder, so the app keeps what it adds in its own data
        let meta = StateStore::open(world.app.path(), &world.root).meta_path();
        std::fs::create_dir_all(meta.parent().expect("parent")).expect("mkdir");
        std::fs::write(&meta, entries.to_toml()).expect("write");
        let mut client = Client::connect(world.core());
        client.handshake().await;

        let reply = client.call(3, "list_tags", &json!({})).await;
        let tags = &reply["result"]["structuredContent"];
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert_eq!(tags["source"], "files");
        assert_eq!(
            (&tags["documents"], &tags["untagged"]),
            (&json!(2), &json!(0))
        );
        assert_eq!(
            tags["tags"],
            json!([
                {"name": "draft", "count": 2, "from": "both"},
                {"name": "plan", "count": 1, "from": "frontMatter"},
                {"name": "start-here", "count": 1, "from": "trailmark"},
            ])
        );

        let reply = client
            .call(4, "search", &json!({ "query": "ship", "tags": ["draft"] }))
            .await;
        let found = &reply["result"]["structuredContent"];
        assert_eq!(found["source"], "files");
        assert_eq!(found["hits"].as_array().expect("hits").len(), 2);
        let readme = found["hits"]
            .as_array()
            .expect("hits")
            .iter()
            .find(|hit| hit["path"] == "README.md")
            .expect("README.md");
        assert_eq!(readme["description"], "Where to begin.");
        assert_eq!(readme["tags"], json!(["draft", "start-here"]));
        let reply = client
            .call(
                5,
                "search",
                &json!({ "query": "ship", "tags": ["plan", "start here"] }),
            )
            .await;
        assert_eq!(reply["result"]["structuredContent"]["hits"], json!([]));

        let reply = client
            .call(6, "list_tags", &json!({ "tag": "Start Here" }))
            .await;
        assert_eq!(
            reply["result"]["structuredContent"]["docs"],
            json!([{"path": "README.md", "description": "Where to begin.", "tags": ["draft", "start-here"]}])
        );
        // looking changed nothing
        assert!(!world.root.join(".trailmark").exists());
        assert_eq!(
            std::fs::read_to_string(&meta).expect("read"),
            entries.to_toml()
        );
        client.close().await.expect("the server ended cleanly");
    }

    /// A project of a thousand documents with sixty tags and full descriptions: what one narrowing step costs an agent in
    /// text. The ceiling is generous; it is here to catch an answer that grows without anyone meaning it to.
    #[tokio::test]
    async fn the_answers_to_a_large_project_stay_compact() {
        const CEILING: usize = 10_000;
        let world = World::new();
        world.connection(true);
        let tag = |n: usize| format!("topic-{:02}", n % 60);
        let mut entries = Entries::default();
        for n in 0..1_000_usize {
            let path = format!(
                "docs/architecture/decisions/area-{:02}/adr-{n:04}-use-event-sourcing-for-the-ledger-service.md",
                n % 24
            );
            let file = world.root.join(&path);
            std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
            let mut body = String::new();
            for section in 0..4 {
                write!(
                    body,
                    "## Section {section}\n\nThe retry policy for the ledger service is described here, with the backoff \
and the limits that were agreed after the incident review, and what to do when a retry still fails. {}\n\n",
                    "More words about the decision and its consequences. ".repeat(3)
                )
                .expect("a string can be written to");
            }
            std::fs::write(
                &file,
                format!(
                    "---\ntags: [{}, {}]\n---\n# Decision {n}\n\n{body}",
                    tag(n),
                    tag(n * 7 + 3)
                ),
            )
            .expect("write");
            entries.set_tags(&path, &[tag(n * 13 + 5)]);
            entries.set_description(
                &path,
                &format!("Decision {n}: why the ledger service keeps its history as events, what that costs, what was rejected and who agreed"),
            );
        }
        let meta = StateStore::open(world.app.path(), &world.root).meta_path();
        std::fs::create_dir_all(meta.parent().expect("parent")).expect("mkdir");
        std::fs::write(&meta, entries.to_toml()).expect("write");
        let mut client = Client::connect(world.core());
        client.handshake().await;

        for (id, tool, arguments) in [
            (3, "list_tags", json!({})),
            (4, "list_tags", json!({ "tag": "topic-07" })),
            (5, "search", json!({ "query": "retry policy" })),
            (
                6,
                "search",
                json!({ "query": "retry policy", "tags": ["topic-07"] }),
            ),
            (7, "search", json!({ "tags": ["topic-07"] })),
        ] {
            let started = std::time::Instant::now();
            let reply = client.call(id, tool, &arguments).await;
            let took = started.elapsed();
            assert_eq!(reply["result"]["isError"], false, "{tool}: {reply}");
            let size = answer_text(&reply).chars().count();
            eprintln!("{tool} {arguments}: {size} characters, {took:?}");
            assert!(size <= CEILING, "{tool} {arguments} is {size} characters");
        }
        // the answers are the ones the sizes are about
        let reply = client.call(8, "list_tags", &json!({})).await;
        let tags = &reply["result"]["structuredContent"];
        assert_eq!(
            tags["documents"], 1_001,
            "the thousand and the plan of the fixture"
        );
        assert_eq!(tags["tags"].as_array().expect("tags").len(), 50);
        assert_eq!(tags["more"], 10);
        let reply = client
            .call(9, "search", &json!({ "query": "retry policy" }))
            .await;
        assert_eq!(
            reply["result"]["structuredContent"]["hits"]
                .as_array()
                .expect("hits")
                .len(),
            10
        );
        client.close().await.expect("the server ended cleanly");
    }

    /// The notes of the person: three notes, two of them shared, in the folder the program keeps notes in (the data folder's).
    fn notes_of(world: &World) -> std::path::PathBuf {
        let notes = world.app.path().join("notes");
        std::fs::create_dir_all(notes.join(".trailmark")).expect("mkdir");
        std::fs::write(
            notes.join("shared one.md"),
            "# Server IPs\n\napi 10.0.0.4\n",
        )
        .expect("write");
        std::fs::write(notes.join("shared two.md"), "Weekly checklist\n").expect("write");
        std::fs::write(notes.join("private.md"), "# My private note\n").expect("write");
        std::fs::write(
            notes.join(".trailmark").join("notes.json"),
            r#"{"shared": ["shared one.md", "shared two.md"]}"#,
        )
        .expect("notes.json");
        notes
    }

    #[tokio::test]
    async fn the_shared_notes_are_listed_and_read_over_mcp_and_nothing_else_of_the_notes() {
        let world = World::new();
        world.connection(true);
        world.bridge.set_enabled(true);
        *world.fake.notes.lock().expect("lock") = Some(notes_of(&world));
        let mut client = Client::connect(world.core());
        client.handshake().await;

        let reply = client.call(3, "list_notes", &json!({})).await;
        let list = &reply["result"]["structuredContent"];
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert_eq!(
            (list["status"].as_str(), list["source"].as_str()),
            (Some("ok"), Some("window"))
        );
        assert_eq!(list["total"], 2);
        let names: Vec<&str> = list["notes"]
            .as_array()
            .expect("notes")
            .iter()
            .map(|n| n["note"].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"shared one.md") && names.contains(&"shared two.md"));
        assert!(
            !names.contains(&"private.md"),
            "a note that is not shared is not listed"
        );
        let one = list["notes"]
            .as_array()
            .expect("notes")
            .iter()
            .find(|n| n["note"] == "shared one.md")
            .expect("one");
        assert_eq!(one["title"], "Server IPs");
        assert!(one["modified"]
            .as_str()
            .is_some_and(|time| time.ends_with('Z')));
        assert!(one["sizeBytes"].as_u64().is_some());

        let reply = client
            .call(4, "read_note", &json!({ "note": "shared one.md" }))
            .await;
        let read = &reply["result"]["structuredContent"];
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert_eq!(read["text"], "# Server IPs\n\napi 10.0.0.4\n");
        assert_eq!(read["title"], "Server IPs");
        assert_eq!(read["source"], "window");

        // a note that is not shared is not there, whether it exists or not, and a name that is not a note's is no better
        for note in [
            "private.md",
            "nothing.md",
            "../private.md",
            "private.md/../shared one.md",
            ".trailmark/notes.json",
        ] {
            let reply = client.call(5, "read_note", &json!({ "note": note })).await;
            assert_eq!(reply["result"]["isError"], true, "{note}: {reply}");
            let message = reply["result"]["structuredContent"]["message"]
                .as_str()
                .expect("a message");
            assert!(message.contains("no shared note"), "{note}: {message}");
            assert!(
                !message.contains("private"),
                "{note}: nothing is said of what exists"
            );
        }
        let reply = client.call(6, "read_note", &json!({ "note": "  " })).await;
        assert_eq!(reply["result"]["isError"], true, "{reply}");

        // the window told its popover: the note by its title, and the listing
        let recent = world.bridge.status().recent;
        assert!(recent.iter().any(|r| r.tool == "list_notes"));
        let read = recent
            .iter()
            .find(|r| r.tool == "read_note" && r.subject.as_deref() == Some("Server IPs"));
        assert!(read.is_some(), "{recent:?}");
        client.close().await.expect("the server ended cleanly");
        world.bridge.shutdown();
    }

    #[tokio::test]
    async fn the_shared_notes_are_read_from_their_files_when_the_window_is_closed() {
        let world = World::new();
        world.connection(true);
        notes_of(&world);
        let mut client = Client::connect(world.core());
        client.handshake().await;
        let reply = client.call(3, "list_notes", &json!({})).await;
        let list = &reply["result"]["structuredContent"];
        assert_eq!(list["source"], "files", "{reply}");
        assert_eq!(list["total"], 2);
        let reply = client
            .call(4, "read_note", &json!({ "note": "shared two.md" }))
            .await;
        assert_eq!(
            reply["result"]["structuredContent"]["text"],
            "Weekly checklist\n"
        );
        assert_eq!(reply["result"]["structuredContent"]["source"], "files");
        let reply = client
            .call(5, "read_note", &json!({ "note": "private.md" }))
            .await;
        assert_eq!(reply["result"]["isError"], true);
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn stopping_to_share_takes_a_note_away_from_the_agent_at_once() {
        let world = World::new();
        world.connection(true);
        let notes = notes_of(&world);
        let mut client = Client::connect(world.core());
        client.handshake().await;
        let reply = client
            .call(3, "read_note", &json!({ "note": "shared two.md" }))
            .await;
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        std::fs::write(
            notes.join(".trailmark").join("notes.json"),
            r#"{"shared": ["shared one.md"]}"#,
        )
        .expect("stop sharing");
        let reply = client
            .call(4, "read_note", &json!({ "note": "shared two.md" }))
            .await;
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        let reply = client.call(5, "list_notes", &json!({})).await;
        assert_eq!(reply["result"]["structuredContent"]["total"], 1);
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn the_window_the_agent_does_not_serve_shares_nothing_when_the_connection_is_off() {
        let world = World::new();
        world.connection(false);
        notes_of(&world);
        let mut client = Client::connect(world.core());
        client.handshake().await;
        let reply = client.call(3, "list_notes", &json!({})).await;
        assert_eq!(reply["result"]["structuredContent"]["status"], "off");
        assert!(reply["result"]["structuredContent"].get("notes").is_none());
        client.close().await.expect("the server ended cleanly");
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
        let reply = client.call(4, "read_note", &json!({ "limit": 3 })).await;
        assert_eq!(reply["result"]["isError"], true, "got {reply}");
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert!(
            text.contains("note"),
            "the missing argument is named: {text}"
        );
        // the tags are a list, and the limit a number
        for arguments in [
            json!({ "query": "x", "tags": "adr" }),
            json!({ "query": "x", "limit": "three" }),
        ] {
            let reply = client.call(5, "search", &arguments).await;
            assert_eq!(reply["result"]["isError"], true, "got {reply}");
        }
        let reply = client.call(6, "list_tags", &json!({ "tag": 5 })).await;
        assert_eq!(reply["result"]["isError"], true, "got {reply}");
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
