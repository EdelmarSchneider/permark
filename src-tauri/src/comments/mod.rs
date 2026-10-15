//! Comments: a person reading a document can leave a comment on a passage ("this contradicts section 2",
//! "simplify") and later ask an agent to go through them. Comments belong to the person, not to the document: they
//! are kept in `comments.json` next to the other local state (`.trailmark/local/` in an initialized project, the app's
//! data folder otherwise), never in the project's files.
//!
//! ```json
//! { "version": 1, "comments": [ { "id": "c3fa9c1d2", "path": "docs/plan.md", "text": "Too vague.",
//!     "anchor": { "quote": "ship it on Friday", "before": "...", "after": "...",
//!                 "headingPath": ["Plan", "Rollout"], "line": 12, "endLine": 12 },
//!     "createdMs": 1760520600000, "done": null } ] }
//! ```
//!
//! - [`anchor`] finds a comment's passage again after the document changed, with no files and no window: by its
//!   quote, then by its heading, then by its line. A comment whose passage is gone is kept and says so.
//! - [`store`] reads and writes the file (atomically).
//! - [`CommentBook`] ties them to a project: it adds, lists, completes and deletes comments, follows renames, and tells
//!   whoever listens when something changed.
//!
//! The window uses [`CommentBook`] directly, and `trailmark mcp` uses the same one over the same file when the window
//! is closed, so a comment reads and resolves the same way either way.

pub(crate) mod anchor;
mod book;
mod store;
#[cfg(test)]
mod tests;

pub(crate) use book::{CommentBook, Listener, NewComment};
pub(crate) use store::Problem as FileProblem;

use serde::{Deserialize, Serialize};

/// The most characters of a selection a comment quotes (the start of a longer selection is what is found again).
pub const MAX_QUOTE_CHARS: usize = 1000;
/// The most characters in the text of a comment.
pub const MAX_COMMENT_CHARS: usize = 2000;
/// The most characters in an agent's reply.
pub const MAX_REPLY_CHARS: usize = 1000;
/// The most comments a project keeps.
pub const MAX_COMMENTS: usize = 5000;

/// Who marked a comment done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DoneBy {
    Person,
    Agent,
}

/// A comment that was marked done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub at_ms: u64,
    pub by: DoneBy,
    /// A short answer from the agent that resolved it ("Rewritten in 3f2a1c").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
}

/// Where a comment was made: the passage and what surrounded it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    /// The selected text, as the reading view showed it (cut at [`MAX_QUOTE_CHARS`]).
    pub quote: String,
    /// The few letters and digits just before the quote, and just after it, as [`anchor::fold`] writes them. They tell
    /// apart two places with the same words.
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
}

/// Whether a comment's passage could be found again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PassageState {
    /// The quote is in the document: `line` is where it is now.
    Anchored,
    /// The document has no such text any more: `line` is the best guess (its section, else where it was).
    PassageChanged,
    /// The document is gone (deleted, or moved by something other than TrailMark).
    DocumentGone,
}

/// A comment with its place worked out against the document as it is now. This is the shape the window and the
/// agent tools share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentView {
    pub id: String,
    pub path: String,
    pub text: String,
    pub quote: String,
    pub heading_path: Vec<String>,
    /// From 1: where the passage is now (or the best guess, see [`PassageState`]).
    pub line: u32,
    pub end_line: u32,
    pub created_ms: u64,
    pub state: PassageState,
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_by: Option<DoneBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
}

/// Which comments a list holds. An agent gets the open ones unless it asks for more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommentFilter {
    #[default]
    Open,
    Done,
    All,
}

impl CommentFilter {
    #[must_use]
    pub fn admits(self, comment: &CommentView) -> bool {
        match self {
            Self::Open => !comment.done,
            Self::Done => comment.done,
            Self::All => true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommentError {
    /// The path is not a Markdown document of the project.
    #[error("{0} is not a Markdown document of this project")]
    NotADocument(String),
    /// Nothing was selected, or the comment says nothing.
    #[error("a comment needs a selected passage and some text")]
    Empty,
    #[error("a project keeps at most {MAX_COMMENTS} comments")]
    TooMany,
    #[error("there is no comment with the id {0}")]
    NotFound(String),
    /// A single file has no project to keep comments in.
    #[error("comments belong to a project folder, not to a single file")]
    SingleFile,
    #[error("cannot keep the comments in {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

impl CommentError {
    /// A short stable identifier for the UI (and for an agent that branches on it).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotADocument(_) => "notADocument",
            Self::Empty => "emptyComment",
            Self::TooMany => "tooManyComments",
            Self::NotFound(_) => "commentNotFound",
            Self::SingleFile => "commentSingleFile",
            Self::Io { .. } => "io",
        }
    }
}

/// `text` cut to `max` characters (never in the middle of one).
pub(crate) fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}
