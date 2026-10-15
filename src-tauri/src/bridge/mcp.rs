//! `trailmark mcp`: TrailMark as a Model Context Protocol server over standard input and output.
//!
//! An agent starts the program with the `mcp` argument and talks to it in newline-delimited JSON-RPC; no window opens.
//! The server is generic over the transport, so the tests drive it over an in-memory pipe and the program gives it the
//! real standard streams ([`run_stdio`]).

use std::io::Write;
use std::path::PathBuf;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::CallToolResult;
use rmcp::service::ServerInitializeError;
use rmcp::transport::{stdio, IntoTransport};
use rmcp::{tool, tool_handler, tool_router, ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::json;

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

/// What an agent can ask TrailMark. The project is the folder the agent works in.
#[derive(Clone)]
struct AgentServer {
    project: PathBuf,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl AgentServer {
    fn new(project: PathBuf) -> Self {
        Self {
            project,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Check the connection to TrailMark. Returns the TrailMark version and the project folder."
    )]
    #[allow(clippy::unused_async)] // The tool macro requires an async method.
    async fn ping(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::structured(json!({
            "ok": true,
            "version": env!("CARGO_PKG_VERSION"),
            "project": self.project.to_string_lossy(),
        })))
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "trailmark",
    instructions = "TrailMark reads and follows the Markdown documents of a project."
)]
#[allow(clippy::unused_async_trait_impl)] // The tool macro writes async handlers that do not wait.
impl ServerHandler for AgentServer {}

/// Serves MCP for `project` over `transport` until the client closes it.
pub(crate) async fn serve<T, E, A>(project: PathBuf, transport: T) -> Result<(), McpError>
where
    T: IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let running = AgentServer::new(project)
        .serve(transport)
        .await
        .map_err(|error| McpError::Handshake(Box::new(error)))?;
    running.waiting().await?;
    Ok(())
}

/// Serves MCP over the program's standard input and output until the input closes.
pub(crate) fn run_stdio(project: PathBuf) -> Result<(), McpError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(McpError::Runtime)?;
    let outcome = runtime.block_on(serve(project, stdio()));
    // A read of the standard input can still be pending on a helper thread; do not wait for it.
    runtime.shutdown_background();
    outcome
}

/// The `trailmark mcp` command: serves the project in the working folder and returns the exit code.
/// Standard output carries only protocol messages, so a failure is reported on standard error.
pub(crate) fn run_cli() -> i32 {
    let outcome = std::env::current_dir()
        .map_err(McpError::WorkingFolder)
        .and_then(|folder| run_stdio(dunce::simplified(&folder).to_path_buf()));
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
    use std::time::Duration;

    use serde_json::{json, Value};
    use tokio::io::{
        duplex, split, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf,
    };
    use tokio::task::JoinHandle;
    use tokio::time::timeout;

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    /// A raw MCP client: newline-delimited JSON-RPC over an in-memory pipe, as an agent's MCP client speaks it.
    struct Client {
        reader: BufReader<ReadHalf<DuplexStream>>,
        writer: WriteHalf<DuplexStream>,
        server: JoinHandle<Result<(), McpError>>,
    }

    impl Client {
        fn connect(project: &str) -> Self {
            let (server_side, client_side) = duplex(64 * 1024);
            let server = tokio::spawn(serve(PathBuf::from(project), server_side));
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
        let mut client = Client::connect("project");
        let reply = client.handshake().await;
        let result = &reply["result"];
        assert_eq!(result["serverInfo"]["name"], "trailmark");
        assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
        assert!(result["capabilities"]["tools"].is_object());
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn lists_the_ping_tool() {
        let mut client = Client::connect("project");
        client.handshake().await;
        let reply = client.request(2, "tools/list", &json!({})).await;
        let tools = reply["result"]["tools"]
            .as_array()
            .expect("a list of tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "ping");
        assert!(tools[0]["description"]
            .as_str()
            .expect("a description")
            .contains("TrailMark"));
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn ping_returns_the_version_and_the_project() {
        let mut client = Client::connect("some/project");
        client.handshake().await;
        let reply = client
            .request(3, "tools/call", &json!({ "name": "ping", "arguments": {} }))
            .await;
        let result = &reply["result"];
        assert_eq!(
            result["structuredContent"],
            json!({ "ok": true, "version": env!("CARGO_PKG_VERSION"), "project": "some/project" })
        );
        assert_eq!(result["isError"], false);
        // The same answer as text, for clients that read only text.
        let text = result["content"][0]["text"].as_str().expect("text content");
        assert_eq!(
            serde_json::from_str::<Value>(text).expect("the text is JSON"),
            result["structuredContent"]
        );
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn an_unknown_tool_is_an_error_reply() {
        let mut client = Client::connect("project");
        client.handshake().await;
        let reply = client
            .request(4, "tools/call", &json!({ "name": "nope", "arguments": {} }))
            .await;
        assert!(reply["error"].is_object(), "got {reply}");
        client.close().await.expect("the server ended cleanly");
    }

    #[tokio::test]
    async fn the_server_ends_when_the_client_closes_the_connection() {
        let mut client = Client::connect("project");
        client.handshake().await;
        assert!(client.close().await.is_ok());
    }

    #[tokio::test]
    async fn closing_before_the_handshake_is_reported() {
        let client = Client::connect("project");
        let outcome = client.close().await;
        assert!(
            matches!(outcome, Err(McpError::Handshake(_))),
            "got {outcome:?}"
        );
    }
}
