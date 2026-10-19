//! A comment as it is kept (`comments.json`, the layout of 0.5) and as the window gets it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// The most characters of a selection a comment quotes (the start of a longer selection is what is found again).
pub const MAX_QUOTE_CHARS: usize = 1000;
/// The most characters in the text of a comment.
pub const MAX_COMMENT_CHARS: usize = 2000;
/// The most characters in a reply.
pub const MAX_REPLY_CHARS: usize = 1000;
/// The most comments a project keeps.
pub const MAX_COMMENTS: usize = 5000;
/// The most replies of one comment.
pub const MAX_REPLIES: usize = 200;
/// The version of the file's layout. A file written by a newer version is set aside rather than rewritten.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum By {
    User,
    Agent,
}

/// A comment that was marked done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub at_ms: u64,
    pub by: By,
    /// A short answer from the agent that resolved it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// An answer under a comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub at_ms: u64,
    pub by: By,
    pub text: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Where a comment was made: the passage and what surrounded it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    /// The selected text, as the reading view showed it (cut at [`MAX_QUOTE_CHARS`]).
    pub quote: String,
    /// The few letters and digits just before the quote, and just after it, folded: they tell apart two places with
    /// the same words.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub before: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after: String,
    /// The headings above the passage (its own section last); empty above the first heading.
    #[serde(default)]
    pub heading_path: Vec<String>,
    /// The lines the passage was on (from 1).
    pub line: u32,
    pub end_line: u32,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A comment as it is kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    /// The document, in the project's form (`/` separators, `@wt/<name>/` inside a linked worktree).
    pub path: String,
    pub text: String,
    pub anchor: Anchor,
    pub created_ms: u64,
    #[serde(default)]
    pub done: Option<Done>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<Reply>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// The file: its comments, and whatever else a newer version of the layout put beside them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Book {
    pub version: u32,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for Book {
    fn default() -> Self {
        Self {
            version: VERSION,
            comments: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

/// Whether a comment's passage could be found again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PassageState {
    /// The quote is in the document where it was.
    Anchored,
    /// The quote is in the document, somewhere else now.
    Moved,
    /// The document has no such text any more: `line` is the best guess (its section, else where it was).
    PassageChanged,
    /// The document is gone (deleted, moved by something other than PerMark, or no text).
    DocumentMissing,
}

/// An answer under a comment, for the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReplyView {
    #[ts(type = "number")]
    pub at_ms: u64,
    pub by: By,
    pub text: String,
}

/// A comment with its place worked out against the document as it is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommentView {
    pub id: String,
    pub path: String,
    pub text: String,
    pub quote: String,
    pub heading_path: Vec<String>,
    /// From 1: where the passage is now (or the best guess, see [`PassageState`]).
    pub line: u32,
    pub end_line: u32,
    #[ts(type = "number")]
    pub created_ms: u64,
    pub state: PassageState,
    pub done: bool,
    #[ts(type = "number | null")]
    pub done_ms: Option<u64>,
    pub done_by: Option<By>,
    /// What the one who marked it done answered.
    pub reply: Option<String>,
    pub replies: Vec<ReplyView>,
}

/// Which comments a list holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Scope {
    #[default]
    Open,
    Done,
    All,
}

impl Scope {
    #[must_use]
    pub fn admits(self, done: bool) -> bool {
        match self {
            Self::Open => !done,
            Self::Done => done,
            Self::All => true,
        }
    }
}
