//! What the window and the plugin exchange about the work copy.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One file whose copy differs from the real file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileChange {
    /// Project path (`/` separators).
    pub path: String,
    pub kind: FileChangeKind,
    /// Line runs that differ (`modified`); the whole text as one run (`added` | `deleted` text files); none for
    /// `binary` | `conflict` (whole-file choices only).
    pub hunks: Vec<HunkView>,
}

/// How a file differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FileChangeKind {
    /// Text changed in the copy only: each hunk can be accepted | rejected alone.
    Modified,
    /// New in the copy.
    Added,
    /// Gone from the copy.
    Deleted,
    /// Changed in the copy, not text the app can splice (binary, undecodable, too large).
    Binary,
    /// Changed in the copy and in the real folder since the copy was taken.
    Conflict,
}

/// One run of lines that differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HunkView {
    /// Index in this list (what accept | reject name).
    pub id: u32,
    /// 1-based line of the real file where the run starts (no old lines: the line the new ones go before).
    pub old_start: u32,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
}

/// Arguments of `acp.accept` | `acp.reject`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecideArgs {
    /// Project path.
    pub path: String,
    /// Hunk ids of the last list; `null`: the whole file.
    #[serde(default)]
    pub hunks: Option<Vec<u32>>,
}
