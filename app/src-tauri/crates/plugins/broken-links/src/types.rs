//! What the broken-links plugin tells the UI (ts-rs exported).

use serde::{Deserialize, Serialize};
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

/// One destination to point at one chosen file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct RepairArgs {
    /// Project path of the document.
    pub path: String,
    /// 1-based line of the destination.
    pub line: u32,
    /// The destination as written, inner text.
    pub href: String,
    /// The chosen file, a project path.
    pub target: String,
}

/// Why `repair` wrote nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RepairRefusal {
    /// Open with unsaved edits.
    Unsaved,
    /// The document could not be read.
    Unreadable,
    /// Not utf-8, so it was left alone.
    NotUtf8,
    /// The file is gone.
    Missing,
    /// The destination is not that one href on that line.
    Mismatch,
    /// The chosen path is not a same-root candidate.
    NotCandidate,
    /// The destination cannot be rewritten.
    NoReplacement,
    /// The bytes changed after they were read.
    Changed,
    /// The new bytes could not be written.
    WriteFailed,
}

/// Whether the one destination was rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "outcome", rename_all = "camelCase")]
#[ts(export)]
pub enum RepairResult {
    Wrote {
        /// The destination as written now.
        href: String,
    },
    Refused {
        reason: RepairRefusal,
    },
}
