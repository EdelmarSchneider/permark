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

use crate::comments::{CommentFilter, CommentView};

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
/// The most comments listed.
pub const MAX_COMMENTS_LISTED: usize = 200;
/// The most shared notes listed.
pub const MAX_NOTES_LISTED: usize = 200;
/// What a search with neither words nor tags says.
pub const EMPTY_QUERY: &str =
    "the query is empty: give words to look for, or tags to list the documents that have them";
/// How many tags `list_tags` lists when the client does not say: the answer of one step should stay small (a few
/// thousand characters) even in a project of a thousand documents.
pub const DEFAULT_TAGS_LISTED: usize = 50;
/// The most tags listed, whatever the client asks for.
pub const MAX_TAGS_LISTED: usize = 200;
/// How many documents are listed for a tag (or tags) when the client does not say.
pub const DEFAULT_TAGGED_LISTED: usize = 20;
/// The most documents listed for a tag (or tags), whatever the client asks for.
pub const MAX_TAGGED_LISTED: usize = 100;
/// A snippet in an answer is cut to about this many characters (the match is near its start).
pub const MAX_SNIPPET_CHARS: usize = 160;

/// What a client can ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Request {
    /// What the person is looking at.
    GetContext,
    /// The documents that changed since they were last read, newest first.
    ListUnread,
    /// Where something is described. With `tags`, only in the documents that have all of them; with no `query`, the
    /// documents that have them instead of sections.
    Search {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
    },
    /// The tags in use, with how many documents have each; or, with `tag`, the documents that have it.
    ListTags {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tag: Option<String>,
        /// How many tags (or, with `tag`, documents) to list at most.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
    },
    /// The comments the person left on passages (the open ones unless asked for more).
    ListComments {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        which: Option<CommentFilter>,
    },
    /// Mark a comment done, with an optional short reply.
    ResolveComment {
        /// The comment's id (not `id`: that is the request's own).
        comment: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply: Option<String>,
    },
    /// Show a place to the person.
    Open {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<String>,
    },
    /// The notes the person shared with agents.
    ListNotes,
    /// The text of one shared note.
    ReadNote {
        /// The note's name, as `listNotes` gives it.
        note: String,
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
            Self::ListTags { .. } => "list_tags",
            Self::ListComments { .. } => "list_comments",
            Self::ResolveComment { .. } => "resolve_comment",
            Self::Open { .. } => "open",
            Self::ListNotes => "list_notes",
            Self::ReadNote { .. } => "read_note",
        }
    }

    /// What the request was about, in a few words (the query, the path); `None` when it has no subject.
    #[must_use]
    pub fn subject(&self) -> Option<String> {
        let subject = match self {
            Self::GetContext | Self::ListUnread | Self::ListComments { .. } | Self::ListNotes => {
                return None
            }
            // tags alone are told the way the search box takes them: `tag:adr tag:api`
            Self::Search { query, tags, .. } if query.trim().is_empty() && !tags.is_empty() => {
                let told: Vec<String> = tags
                    .iter()
                    .map(|tag| format!("tag:{}", tag.trim()))
                    .collect();
                return Some(told.join(" ").chars().take(80).collect());
            }
            Self::Search { query, .. } => query.trim(),
            Self::ListTags { tag, .. } => match tag.as_deref().map(str::trim) {
                Some(tag) if !tag.is_empty() => tag,
                _ => return None,
            },
            Self::ResolveComment { comment, .. } => comment.trim(),
            Self::Open { path, .. } => path.trim(),
            Self::ReadNote { note } => note.trim(),
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
    /// The tags of the document the section is in, written in it or added in TrailMark, alphabetical. Left out when it has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// The one-line description of that document. Left out when it has none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
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

/// What `listComments` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentList {
    /// By document and line, at most [`MAX_COMMENTS_LISTED`].
    pub comments: Vec<CommentView>,
    /// How many comments there are in all (of the kind asked for).
    pub total: usize,
}

/// What `search` answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hits {
    pub hits: Vec<Hit>,
}

/// What `listNotes` answers: the notes the person shared with agents, the newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteList {
    /// At most [`MAX_NOTES_LISTED`].
    pub notes: Vec<crate::notes::share::SharedNote>,
    /// How many notes are shared in all.
    pub total: usize,
}

impl NoteList {
    /// The newest [`MAX_NOTES_LISTED`] of `notes` (already newest first), and how many there are.
    #[must_use]
    pub fn new(mut notes: Vec<crate::notes::share::SharedNote>) -> Self {
        let total = notes.len();
        notes.truncate(MAX_NOTES_LISTED);
        Self { notes, total }
    }
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
                limit: None,
                tags: Vec::new()
            }
        );
    }

    #[test]
    fn the_tag_requests_are_flat_objects_and_say_nothing_they_do_not_carry() {
        let call: Call =
            serde_json::from_str(r#"{"id":1,"op":"search","query":"retry","tags":["adr","api"]}"#)
                .expect("parse");
        assert_eq!(
            call.request,
            Request::Search {
                query: "retry".to_owned(),
                limit: None,
                tags: vec!["adr".to_owned(), "api".to_owned()]
            }
        );
        // a search without tags is written as it always was
        let plain = Call {
            id: 2,
            request: Request::Search {
                query: "retry".to_owned(),
                limit: Some(3),
                tags: Vec::new(),
            },
        };
        assert_eq!(
            serde_json::to_string(&plain).expect("json"),
            r#"{"id":2,"op":"search","query":"retry","limit":3}"#
        );
        let call: Call = serde_json::from_str(r#"{"id":3,"op":"listTags"}"#).expect("parse");
        assert_eq!(
            call.request,
            Request::ListTags {
                tag: None,
                limit: None
            }
        );
        let call: Call = serde_json::from_str(r#"{"id":4,"op":"listTags","tag":"adr","limit":5}"#)
            .expect("parse");
        assert_eq!(
            call.request,
            Request::ListTags {
                tag: Some("adr".to_owned()),
                limit: Some(5)
            }
        );
        let line = serde_json::to_string(&call).expect("json");
        assert_eq!(serde_json::from_str::<Call>(&line).expect("parse"), call);
    }

    #[test]
    fn a_hit_says_the_tags_and_description_only_when_the_document_has_them() {
        let bare = Hit {
            path: "a.md".to_owned(),
            heading_path: Vec::new(),
            line: 1,
            snippet: "x".to_owned(),
            tags: Vec::new(),
            description: String::new(),
        };
        let json = serde_json::to_value(&bare).expect("json");
        assert!(json.get("tags").is_none() && json.get("description").is_none());
        let described = Hit {
            tags: vec!["adr".to_owned()],
            description: "Why.".to_owned(),
            ..bare.clone()
        };
        let json = serde_json::to_value(&described).expect("json");
        assert_eq!(
            (&json["tags"], &json["description"]),
            (&serde_json::json!(["adr"]), &serde_json::json!("Why."))
        );
        // an answer from before the fields existed still reads
        let old: Hit =
            serde_json::from_str(r#"{"path":"a.md","headingPath":[],"line":1,"snippet":"x"}"#)
                .expect("parse");
        assert_eq!(old, bare);
    }

    #[test]
    fn comment_requests_are_flat_objects_too() {
        let call: Call = serde_json::from_str(r#"{"id":1,"op":"listComments"}"#).expect("parse");
        assert_eq!(call.request, Request::ListComments { which: None });
        let call: Call =
            serde_json::from_str(r#"{"id":2,"op":"listComments","which":"all"}"#).expect("parse");
        assert_eq!(
            call.request,
            Request::ListComments {
                which: Some(CommentFilter::All)
            }
        );
        let call: Call =
            serde_json::from_str(r#"{"id":3,"op":"resolveComment","comment":"c1","reply":"Done"}"#)
                .expect("parse");
        assert_eq!(call.id, 3);
        assert_eq!(
            call.request,
            Request::ResolveComment {
                comment: "c1".to_owned(),
                reply: Some("Done".to_owned())
            }
        );
        // written back the same way
        let line = serde_json::to_string(&call).expect("json");
        assert_eq!(serde_json::from_str::<Call>(&line).expect("parse"), call);
        assert!(serde_json::from_str::<Call>(r#"{"id":4,"op":"resolveComment"}"#).is_err());
        assert!(
            serde_json::from_str::<Call>(r#"{"id":5,"op":"listComments","which":"some"}"#).is_err()
        );
    }

    #[test]
    fn the_comment_tools_are_named_for_the_status_popover() {
        let list = Request::ListComments { which: None };
        assert_eq!((list.tool(), list.subject()), ("list_comments", None));
        let resolve = Request::ResolveComment {
            comment: " c1 ".to_owned(),
            reply: None,
        };
        assert_eq!(resolve.tool(), "resolve_comment");
        assert_eq!(resolve.subject().as_deref(), Some("c1"));
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
            tags: vec!["adr".to_owned()],
        };
        assert_eq!(search.tool(), "search");
        assert_eq!(search.subject().as_deref(), Some("rollout plan"));
        // tags alone are told the way the search box takes them
        let tagged = Request::Search {
            query: "  ".to_owned(),
            limit: None,
            tags: vec![" adr ".to_owned(), "api".to_owned()],
        };
        assert_eq!(tagged.subject().as_deref(), Some("tag:adr tag:api"));
        let tags = Request::ListTags {
            tag: None,
            limit: None,
        };
        assert_eq!((tags.tool(), tags.subject()), ("list_tags", None));
        let one = Request::ListTags {
            tag: Some(" adr ".to_owned()),
            limit: None,
        };
        assert_eq!(one.subject().as_deref(), Some("adr"));
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
