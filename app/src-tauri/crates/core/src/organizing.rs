//! What the organizing plugins (tags, silence, comments, unread) hand to core consumers: search and the agent
//! connection. Typed, data-only, offered through [`crate::plugins::Services`] while the plugin runs (RAII lease); a
//! consumer asks `services.get::<TagsApi>()` and works without when there is none.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use crate::IpcError;

/// A tag and how many files have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    pub tag: String,
    pub count: u32,
}

/// The tags in use: most used first, then by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagsSummary {
    pub tags: Vec<TagCount>,
    /// Markdown documents known to the tags service.
    pub documents: u32,
    /// Documents without any tag.
    pub untagged: u32,
}

/// What search and the agent ask of the tags.
pub trait TagLookup: Send + Sync {
    /// Project paths of the files that have EVERY one of `tags` (front matter and app tags together).
    fn files_with_all(&self, tags: &[String]) -> HashSet<String>;
    /// The tags of one file, front matter first.
    fn tags_of(&self, path: &str) -> Vec<String>;
    /// Every tag with its count, and the document counts.
    fn summary(&self) -> TagsSummary;
    /// The one-line description of a file (empty when none).
    fn description_of(&self, path: &str) -> String;
    /// Waits until the first look at the documents is done; `false` when the time ran out.
    fn wait_ready(&self, timeout: Duration) -> bool;
}

/// The tags service of a project.
#[derive(Clone)]
pub struct TagsApi(pub Arc<dyn TagLookup>);

/// Which paths the person silenced.
pub trait SilenceLookup: Send + Sync {
    fn is_silenced(&self, path: &str) -> bool;
}

/// The silence service of a project. Agents never learn the marks; a silenced path is simply not shared.
#[derive(Clone)]
pub struct SilenceApi(pub Arc<dyn SilenceLookup>);

/// Which comments a list holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentFilter {
    Open,
    Done,
    All,
}

/// A comment as the agent sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentItem {
    pub id: String,
    pub path: String,
    pub quote: String,
    pub text: String,
    pub heading_path: Vec<String>,
    /// From 1: where the passage is now (or the best guess).
    pub line: u32,
    /// `anchored | moved | passageChanged | documentMissing`.
    pub state: &'static str,
    pub created_ms: u64,
    pub done: bool,
    /// What the one who marked it done answered.
    pub reply: Option<String>,
}

/// What the agent asks of the comments.
pub trait CommentBook: Send + Sync {
    /// The comments with their places worked out against the documents as they are now.
    ///
    /// # Errors
    /// The comments file cannot be read right now.
    fn list(&self, which: CommentFilter) -> Result<Vec<CommentItem>, IpcError>;
    /// Marks a comment done by the agent, with an optional short reply.
    ///
    /// # Errors
    /// No such comment (`NotFound`), or the file cannot be used.
    fn resolve(&self, id: &str, reply: Option<&str>) -> Result<(), IpcError>;
}

/// The comments service of a project.
#[derive(Clone)]
pub struct CommentsApi(pub Arc<dyn CommentBook>);

/// The documents changed since the person read them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnreadNow {
    /// The first look at the disk is done (before it nothing is unread).
    pub ready: bool,
    /// Sorted by path.
    pub paths: Vec<String>,
}

/// What the agent asks of the unread markers.
pub trait UnreadLookup: Send + Sync {
    fn unread(&self) -> UnreadNow;
    /// Waits until the first look at the disk is done; `false` when the time ran out.
    fn wait_ready(&self, timeout: Duration) -> bool;
}

/// The unread service of a project.
#[derive(Clone)]
pub struct UnreadApi(pub Arc<dyn UnreadLookup>);
