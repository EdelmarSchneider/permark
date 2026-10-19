//! The project tree: one model per `ProjectCore` (every listed entry, unfiltered), per-window views (filter + what was
//! sent) and deltas. See `specs/explorer.md`.

mod category;
mod explorer;
mod filter;
mod ignore;
mod model;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use category::{category_of, extension_of, is_html, may_open_with_default_app, Category};
pub use explorer::Explorer;
pub use filter::{FilterShow, TreeFilter};
pub use ignore::Ignorer;
pub(crate) use model::{below_root, key_of, path_always_out};
pub use model::{Scan, ScanFile};

use crate::error::{ErrorKind, IpcError};

/// Folders with more direct entries than this are listed unopened (the project folder itself excepted).
pub const LARGE_FOLDER: usize = 5000;

/// Why a folder shows no children yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum Unlisted {
    /// More than [`LARGE_FOLDER`] direct entries.
    #[serde(rename_all = "camelCase")]
    Large { entries: u32 },
    /// Ignored folder, not entered until opened.
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum NodeKind {
    #[serde(rename_all = "camelCase")]
    Dir { unlisted: Option<Unlisted> },
    #[serde(rename_all = "camelCase")]
    File { category: Category },
}

/// One entry of the tree as a window sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TreeNode {
    /// Project path (`@wt/<name>/...` in a worktree).
    pub path: String,
    pub name: String,
    pub kind: NodeKind,
    /// Name starts with a dot.
    pub hidden: bool,
    /// Matched by ignore rules or below an ignored folder.
    pub ignored: bool,
    /// Shown only because it is the kept (open) document.
    pub forced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RootKind {
    Main,
    Worktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RootNode {
    pub name: String,
    pub kind: RootKind,
    /// `""` for the project folder, `@wt/<name>` for a worktree.
    pub path: String,
}

/// The whole tree as one window's filter shows it: flat, parents before children, in display order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TreeSnapshot {
    pub roots: Vec<RootNode>,
    pub nodes: Vec<TreeNode>,
    pub filter: TreeFilter,
    /// Sequence number of this list: the next delta carries `seq + 1`; a gap means a missed delta (ask again).
    #[ts(type = "number")]
    pub seq: u64,
    /// Patterns of the filter that are not valid (shown in the filter popover).
    pub invalid_patterns: Vec<String>,
    pub total_files: u32,
    pub shown_files: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Added {
    pub node: TreeNode,
    /// The node before it in the list (None: first).
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Renamed {
    pub from: String,
    pub to: String,
}

/// What changed in one window's list since it was last told.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TreeDelta {
    /// Sequence number: the previous list's `seq + 1`.
    #[ts(type = "number")]
    pub seq: u64,
    /// Paths to drop (a renamed folder: its whole old subtree).
    pub removed: Vec<String>,
    /// In list order, each after its predecessor.
    pub added: Vec<Added>,
    /// Hints to move UI state (selection, expansion) before the list changes.
    pub renamed: Vec<Renamed>,
    pub total_files: u32,
    pub shown_files: u32,
    /// Many changes at once (git operation, large move): the UI may batch its reaction.
    pub bulk: bool,
}

impl TreeDelta {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.added.is_empty() && self.renamed.is_empty()
    }
}

/// Event: a window's list changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TreeChanged {
    pub window: String,
    pub delta: TreeDelta,
}

#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error(transparent)]
    Vault(#[from] crate::paths::VaultError),
    #[error("{0} is not a folder")]
    NotAFolder(String),
    #[error("no tree is open for this window")]
    NoView,
}

impl From<TreeError> for IpcError {
    fn from(error: TreeError) -> Self {
        match error {
            TreeError::Vault(e) => e.into(),
            TreeError::NotAFolder(path) => IpcError::new(ErrorKind::Unsupported).subject(path),
            TreeError::NoView => IpcError::new(ErrorKind::Unavailable),
        }
    }
}
