//! The messages between `trailmark mcp` (or `trailmark context`) and the window of a project, over the local
//! socket: one JSON object per line.
//!
//! ```text
//! -> {"id":1,"op":"getContext"}
//! <- {"id":1,"ok":true,"result":{"document":"docs/plan.md", ...}}
//! -> {"id":2,"op":"open","path":"docs/plan.md","line":40}
//! <- {"id":2,"ok":false,"error":{"kind":"notFound","message":"docs/missing.md is not a document of this project"}}
//! ```
//!
//! A reply carries the `id` of its request, so a client can keep one connection open and send requests one after
//! the other. The MCP layer only translates; nothing here knows about MCP.

use std::io;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// A line longer than this is not a request: the connection is closed.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
/// The most a client reads for one reply.
pub const MAX_REPLY_BYTES: usize = 4 * 1024 * 1024;

/// The most hits a search returns, whatever the client asks for. One constant to change.
pub const MAX_SEARCH_HITS: usize = 25;
/// What a search returns when the client does not say.
pub const DEFAULT_SEARCH_HITS: usize = 10;
/// The most unread documents listed.
pub const MAX_UNREAD_LISTED: usize = 200;

/// What a client can ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Request {
    /// What the person is looking at.
    GetContext,
    /// The documents that changed since they were last read, newest first.
    ListUnread,
    /// Where something is described.
    Search {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
    },
    /// Show a place to the person.
    Open {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<String>,
    },
}

impl Request {
    /// The tool an agent used to ask this, as the status popover lists it.
    #[must_use]
    pub fn tool(&self) -> &'static str {
        match self {
            Self::GetContext => "get_context",
            Self::ListUnread => "list_unread",
            Self::Search { .. } => "search",
            Self::Open { .. } => "open",
        }
    }

    /// What the request was about, in a few words (the query, the path); `None` when it has no subject.
    #[must_use]
    pub fn subject(&self) -> Option<String> {
        let subject = match self {
            Self::GetContext | Self::ListUnread => return None,
            Self::Search { query, .. } => query.trim(),
            Self::Open { path, .. } => path.trim(),
        };
        Some(subject.chars().take(80).collect())
    }
}

/// One line from a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub id: u64,
    #[serde(flatten)]
    pub request: Request,
}

/// Why a request failed. `kind` is stable (a client may branch on it); `message` is for people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The line was not a request this program understands.
    BadRequest,
    /// The path is not a document of the project.
    NotFound,
    /// The program could not do it now.
    Unavailable,
}

impl ProtocolError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// One line to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

impl Reply {
    pub fn success(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: u64, error: ProtocolError) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(error),
        }
    }

    /// The result, or the error the other side reported.
    pub fn into_result(self) -> Result<serde_json::Value, ProtocolError> {
        match (self.ok, self.result, self.error) {
            (true, Some(result), _) => Ok(result),
            (_, _, Some(error)) => Err(error),
            _ => Err(ProtocolError::new(
                ErrorKind::BadRequest,
                "the reply carried neither a result nor an error",
            )),
        }
    }
}

/// One section that matched a search, as an agent reads it: plain text, no markup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub path: String,
    pub heading_path: Vec<String>,
    pub line: u32,
    pub snippet: String,
}

/// A document that changed since it was last read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadDoc {
    pub path: String,
    /// When the file was last written (milliseconds since the Unix epoch).
    pub modified_ms: u64,
}

/// Where `open` took the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub path: String,
    /// The line shown: the one asked for, or the one the heading is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// A heading was asked for and the document has no such heading (the document opened at its top).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub heading_not_found: bool,
}

/// What `listUnread` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadList {
    /// Newest change first, at most [`MAX_UNREAD_LISTED`].
    pub unread: Vec<UnreadDoc>,
    /// How many documents are unread in all.
    pub total: usize,
}

impl UnreadList {
    /// The newest [`MAX_UNREAD_LISTED`] of `docs` (already newest first), and how many there are.
    #[must_use]
    pub fn new(mut docs: Vec<UnreadDoc>) -> Self {
        let total = docs.len();
        docs.truncate(MAX_UNREAD_LISTED);
        Self {
            unread: docs,
            total,
        }
    }
}

/// What `search` answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hits {
    pub hits: Vec<Hit>,
}

/// Reads one line (without its newline) of at most `max` bytes. `Ok(None)` when the other side closed the
/// connection. A longer line, or one that is not text, is an error: the caller closes the connection.
pub async fn read_line<R>(reader: &mut R, max: usize) -> io::Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes = Vec::new();
    // One byte more than the limit, to tell a line that fits exactly from one that does not.
    let mut limited = (&mut *reader).take(max as u64 + 1);
    let read = limited.read_until(b'\n', &mut bytes).await?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    } else if bytes.len() > max {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "line is not UTF-8"))
}

/// Writes `message` as one line and flushes it.
pub async fn write_line<W, T>(writer: &mut W, message: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let mut line = serde_json::to_vec(message).map_err(io::Error::other)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn requests_are_one_flat_object_with_an_op() {
        let call: Call = serde_json::from_str(r#"{"id":7,"op":"getContext"}"#).expect("parse");
        assert_eq!(
            call,
            Call {
                id: 7,
                request: Request::GetContext
            }
        );
        let call: Call = serde_json::from_str(
            r#"{"id":8,"op":"open","path":"docs/a.md","line":40,"heading":"Rollout"}"#,
        )
        .expect("parse");
        assert_eq!(
            call.request,
            Request::Open {
                path: "docs/a.md".to_owned(),
                line: Some(40),
                heading: Some("Rollout".to_owned())
            }
        );
        let call: Call =
            serde_json::from_str(r#"{"id":9,"op":"search","query":"rollout"}"#).expect("parse");
        assert_eq!(
            call.request,
            Request::Search {
                query: "rollout".to_owned(),
                limit: None
            }
        );
    }

    #[test]
    fn a_request_is_written_the_way_it_is_read() {
        let call = Call {
            id: 3,
            request: Request::Open {
                path: "a.md".to_owned(),
                line: None,
                heading: None,
            },
        };
        let line = serde_json::to_string(&call).expect("json");
        assert_eq!(line, r#"{"id":3,"op":"open","path":"a.md"}"#);
        assert_eq!(serde_json::from_str::<Call>(&line).expect("parse"), call);
    }

    #[test]
    fn lines_that_are_not_requests_do_not_parse() {
        for line in [
            "",
            "not json",
            "[]",
            r#"{"id":1}"#,
            r#"{"op":"getContext"}"#,
            r#"{"id":1,"op":"rm"}"#,
            r#"{"id":1,"op":"open"}"#,
            r#"{"id":"x","op":"getContext"}"#,
        ] {
            assert!(serde_json::from_str::<Call>(line).is_err(), "{line:?}");
        }
    }

    #[test]
    fn replies_carry_a_result_or_an_error() {
        let ok = Reply::success(1, json!({ "a": 1 }));
        assert_eq!(
            serde_json::to_string(&ok).expect("json"),
            r#"{"id":1,"ok":true,"result":{"a":1}}"#
        );
        assert_eq!(ok.into_result().expect("a result"), json!({ "a": 1 }));
        let failed = Reply::failure(2, ProtocolError::new(ErrorKind::NotFound, "no such page"));
        assert_eq!(
            serde_json::to_string(&failed).expect("json"),
            r#"{"id":2,"ok":false,"error":{"kind":"notFound","message":"no such page"}}"#
        );
        let error = failed.into_result().expect_err("an error");
        assert_eq!(error.kind, ErrorKind::NotFound);
        // A reply that says neither is an error, not a panic.
        let empty: Reply = serde_json::from_str(r#"{"id":3,"ok":true}"#).expect("parse");
        assert!(empty.into_result().is_err());
    }

    #[test]
    fn the_unread_list_is_cut_and_still_counts_everything() {
        let docs: Vec<UnreadDoc> = (0..MAX_UNREAD_LISTED + 5)
            .map(|n| UnreadDoc {
                path: format!("d{n}.md"),
                modified_ms: 1_000 - n as u64,
            })
            .collect();
        let list = UnreadList::new(docs);
        assert_eq!(list.unread.len(), MAX_UNREAD_LISTED);
        assert_eq!(list.total, MAX_UNREAD_LISTED + 5);
        assert_eq!(list.unread[0].path, "d0.md");
    }

    #[test]
    fn the_status_popover_names_the_tool_and_what_it_was_about() {
        let search = Request::Search {
            query: "  rollout plan ".to_owned(),
            limit: Some(3),
        };
        assert_eq!(search.tool(), "search");
        assert_eq!(search.subject().as_deref(), Some("rollout plan"));
        assert_eq!(Request::GetContext.tool(), "get_context");
        assert_eq!(Request::GetContext.subject(), None);
        assert_eq!(Request::ListUnread.tool(), "list_unread");
        let open = Request::Open {
            path: "x".repeat(200),
            line: None,
            heading: None,
        };
        assert_eq!(open.tool(), "open");
        assert_eq!(open.subject().map(|s| s.len()), Some(80));
    }
}
