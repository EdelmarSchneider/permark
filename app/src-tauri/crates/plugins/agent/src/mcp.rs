//! `permark mcp`: the Model Context Protocol over standard input and output (newline-delimited JSON-RPC). It only
//! translates; [`Agent`] decides where a call goes. Generic over the transport so tests drive it in memory.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::{NotificationContext, RequestContext};
use rmcp::transport::IntoTransport;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Value};

use crate::cli::Agent;

const INSTRUCTIONS: &str = "PerMark shows the person a project's documents. Ask what they look at, what changed, \
where something is described, the outline then the part of a file you need, their comments (mark one done) and the \
notes they shared; show them a place. Read-only toward files. Available while the person has the agent connection \
on for this project.";

fn schema(value: Value) -> Arc<JsonObject> {
    Arc::new(match value {
        Value::Object(map) => map,
        _ => JsonObject::default(),
    })
}

fn no_args() -> Arc<JsonObject> {
    schema(json!({ "type": "object", "properties": {}, "additionalProperties": false }))
}

/// The tools with compact schemas (names and arguments as in 0.5, plus `outline` and `read`).
#[must_use]
pub fn tool_list() -> Vec<Tool> {
    let reading = ToolAnnotations::new().read_only(true);
    let int = |d: &str| json!({ "type": "integer", "minimum": 1, "description": d });
    let text = |d: &str| json!({ "type": "string", "description": d });
    vec![
        Tool::new(
            "get_context",
            "What the person looks at in PerMark now: document, mode, section, selection, unsaved, open comments.",
            no_args(),
        )
        .annotate(reading.clone()),
        Tool::new(
            "list_unread",
            "Documents changed since the person last read them, newest first.",
            schema(json!({ "type": "object", "properties": { "limit": int("Max items (default 50).") }, "additionalProperties": false })),
        )
        .annotate(reading.clone()),
        Tool::new(
            "search",
            "Sections matching words (path, heading path, line, snippet); tags narrow; tags alone list documents.",
            schema(json!({ "type": "object", "properties": {
                "query": text("Words or a short phrase."),
                "limit": int("Max sections (default 10, max 25)."),
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Only documents with all these tags." }
            }, "additionalProperties": false })),
        )
        .annotate(reading.clone()),
        Tool::new(
            "list_tags",
            "Tags with document counts; with `tag`: the documents that have it (path, description, tags).",
            schema(json!({ "type": "object", "properties": {
                "tag": text("A tag: list its documents."),
                "limit": int("Max items.")
            }, "additionalProperties": false })),
        )
        .annotate(reading.clone()),
        Tool::new(
            "outline",
            "Structure of a file before reading it: Markdown headings | JSON top-level keys, with lines.",
            schema(json!({ "type": "object", "properties": { "path": text("Project-relative path.") }, "required": ["path"], "additionalProperties": false })),
        )
        .annotate(reading.clone()),
        Tool::new(
            "read",
            "Text of one part of a file: `at` = heading path `A > B`, `#anchor`, JSON key, or lines `L10-L40`; none = all.",
            schema(json!({ "type": "object", "properties": {
                "path": text("Project-relative path."),
                "at": text("Part to read.")
            }, "required": ["path"], "additionalProperties": false })),
        )
        .annotate(reading.clone()),
        Tool::new(
            "list_comments",
            "The person's comments on passages (id, path, quote, text, line, state); open by default.",
            schema(json!({ "type": "object", "properties": {
                "which": { "type": "string", "enum": ["open", "done", "all"] }
            }, "additionalProperties": false })),
        )
        .annotate(reading.clone()),
        Tool::new(
            "resolve_comment",
            "Mark one comment done, with a short reply the person reads. The only change an agent can make.",
            schema(json!({ "type": "object", "properties": {
                "id": text("Comment id from list_comments."),
                "reply": text("What you did, short.")
            }, "required": ["id"], "additionalProperties": false })),
        )
        .annotate(ToolAnnotations::new().read_only(false).destructive(false)),
        Tool::new(
            "list_notes",
            "Notes the person shared with agents (note, title, updated).",
            no_args(),
        )
        .annotate(reading.clone()),
        Tool::new(
            "read_note",
            "Text of one shared note, by the `note` name list_notes gives.",
            schema(json!({ "type": "object", "properties": { "note": text("Note name.") }, "required": ["note"], "additionalProperties": false })),
        )
        .annotate(reading),
        Tool::new(
            "open",
            "Show the person a document in PerMark (line or heading) without taking focus; starts PerMark if closed.",
            schema(json!({ "type": "object", "properties": {
                "path": text("Project-relative path."),
                "line": int("Line from 1."),
                "heading": text("Heading or path `A > B`.")
            }, "required": ["path"], "additionalProperties": false })),
        )
        .annotate(ToolAnnotations::new().read_only(false).destructive(false)),
    ]
}

/// The MCP face of an [`Agent`].
#[derive(Clone)]
pub struct McpServer {
    agent: Arc<Agent>,
}

impl McpServer {
    #[must_use]
    pub fn new(agent: Arc<Agent>) -> Self {
        Self { agent }
    }
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "permark",
                permark_core::app_info().version,
            ))
            .with_instructions(INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(tool_list())))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        let args = request.arguments.map_or(Value::Null, Value::Object);
        let agent = Arc::clone(&self.agent);
        let result = tokio::task::spawn_blocking(move || agent.call(&name, &args))
            .await
            .unwrap_or_else(|e| Err(crate::ToolError::Failed(e.to_string())));
        Ok(CallToolResponse::Complete(match result {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => CallToolResult::structured_error(json!({
                "error": error.code(),
                "message": error.to_string(),
            })),
        }))
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        let client = context
            .peer
            .peer_info()
            .map_or_else(|| "agent".to_owned(), |info| info.client_info.name.clone());
        let agent = Arc::clone(&self.agent);
        let _ = tokio::task::spawn_blocking(move || agent.hello(&client)).await;
    }
}

/// Why `permark mcp` stopped with an error.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("could not start: {0}")]
    Runtime(#[source] std::io::Error),
    #[error("the client did not complete the MCP handshake: {0}")]
    Handshake(String),
    #[error("stopped unexpectedly: {0}")]
    Stopped(String),
}

/// Serves MCP over `transport` until the client closes it.
///
/// # Errors
/// Handshake failure, or the service stopped.
pub async fn serve<T, E, A>(agent: Arc<Agent>, transport: T) -> Result<(), McpError>
where
    T: IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let running = McpServer::new(agent)
        .serve(transport)
        .await
        .map_err(|e| McpError::Handshake(e.to_string()))?;
    running
        .waiting()
        .await
        .map_err(|e| McpError::Stopped(e.to_string()))?;
    Ok(())
}

/// Serves MCP over standard input and output until the input closes.
///
/// # Errors
/// See [`serve`].
pub fn run_stdio(agent: Arc<Agent>) -> Result<(), McpError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(McpError::Runtime)?;
    let outcome = runtime.block_on(serve(agent, rmcp::transport::stdio()));
    // a read of standard input may still be pending on a helper thread: do not wait for it
    runtime.shutdown_background();
    outcome
}
