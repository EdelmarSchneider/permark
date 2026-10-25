//! What the broken-links plugin tells the UI (ts-rs exported).

use serde::Serialize;
use ts_rs::TS;

/// One link that leads nowhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrokenLink {
    /// 1-based line of the destination.
    pub line: u32,
    /// The destination as written, shortened when very long.
    pub written: String,
}

/// The broken links of one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrokenDoc {
    pub path: String,
    pub links: Vec<BrokenLink>,
}

/// How many links are broken in one root of the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrokenRoot {
    /// `""` = the opened folder; `@wt/<name>` = a linked worktree.
    pub prefix: String,
    pub total: u32,
}

/// Every broken link of the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrokenReport {
    /// The first scan is done (before it the list may be short).
    pub ready: bool,
    /// The opened folder's documents first, then each worktree's.
    pub docs: Vec<BrokenDoc>,
    /// How many there are in every root, including the ones past the list.
    pub total: u32,
    /// How many there are in the opened folder alone (no linked worktree).
    pub current: u32,
    /// The count of each root: the opened folder first, then the worktrees.
    pub roots: Vec<BrokenRoot>,
    pub truncated: bool,
    /// Documents that were too large, not text, or unreadable, and were left out.
    pub skipped: u32,
}

/// What the window is told when the picture changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrokenSummary {
    pub ready: bool,
    /// Every root.
    pub total: u32,
    /// The opened folder alone.
    pub current: u32,
}
