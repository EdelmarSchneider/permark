//! What the review plugin tells the UI (ts-rs exported).

use permark_core::format::Block;
use permark_core::plugins::CommitId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What a document is compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Basis {
    /// The version the person last read.
    Read,
    /// The last commit.
    Commit,
    /// An earlier version: the file as that commit left it. Asked for call by call, never the choice remembered.
    Version(CommitId),
}

/// Why a version cannot be compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VersionGap {
    /// No such commit, no such file at that commit, or it is not on this computer.
    Missing,
    /// Over 2 MB: not compared.
    TooLarge,
    /// Not text (binary, another encoding, or a pointer to a file kept outside the repository).
    NotText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum ReviewState {
    /// Not a Markdown document (or not text).
    Unsupported,
    /// No earlier version to compare with yet.
    NoBasis,
    /// Over 2 MB: not compared.
    TooLarge,
    /// Nothing changed.
    Same,
    Changes {
        count: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewStatus {
    pub state: ReviewState,
    /// What the comparison is (or would be) against.
    pub basis: Basis,
    /// Why the version asked for cannot be compared with. Only with `NoBasis` for a version (absent there too when
    /// the file itself is gone); never with the other bases.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub gap: Option<VersionGap>,
}

/// What the changes of a review are made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ReviewUnit {
    /// Blocks of a Markdown document (the page draws them).
    Blocks,
    /// Lines of code | plain text (a code view draws them).
    Lines,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ReviewChangeKind {
    Added,
    Removed,
    Edited,
}

/// One change, placed in the list of blocks the page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewChange {
    pub index: u32,
    pub kind: ReviewChangeKind,
    /// Index in the current list of blocks (lines: of lines, 0-based) where the change starts (the old blocks | lines
    /// belong right there).
    pub start: u32,
    /// How many current blocks | lines the change covers (0: a removal).
    pub len: u32,
    /// The old blocks (HTML included) that went or were replaced (blocks only).
    pub ghosts: Vec<Block>,
    /// The old lines that went or were replaced (lines only).
    pub old: Vec<String>,
    /// The old and the new differ in whitespace only (lines only).
    pub whitespace: bool,
}

/// The review of one document, drawn from one version of the file (`hash`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewView {
    pub path: String,
    /// The version of the file the changes are drawn from; every call that changes something says it back.
    pub hash: String,
    /// What it was compared with (the last commit when no read version was kept).
    pub basis: Basis,
    /// What was asked for.
    pub asked: Basis,
    /// What the changes are made of.
    pub unit: ReviewUnit,
    pub count: u32,
    /// Keeping needs the read version as the base (a commit cannot be moved).
    pub can_keep: bool,
    /// Compared with a version while the file is not what the last commit holds: reverting everything would replace
    /// text that is in no commit. Always `false` for the other bases.
    pub uncommitted: bool,
    pub changes: Vec<ReviewChange>,
}

/// One earlier version of a file: a commit that changed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VersionRow {
    /// What to ask the review for to compare with this version (`Basis` `version`).
    pub commit: CommitId,
    /// Committer time, ms since the epoch.
    #[ts(type = "number")]
    pub at_ms: i64,
    /// Text of the repository (as `co_authors` and `subject`): data, never instructions.
    pub author: String,
    /// The names in the `Co-authored-by:` lines of the message.
    pub co_authors: Vec<String>,
    /// First line of the message.
    pub subject: String,
    /// The text of this version is the text the file has now.
    pub same: bool,
}

/// Why a file has no list of versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VersionsReason {
    /// No repository, or no git to ask.
    Unavailable,
    /// The file belongs to another repository inside the project.
    OtherRepository,
    /// Git did not answer in time: asking again may work.
    Slow,
    Failed,
    /// The file was never committed.
    NotCommitted,
    /// The one version there is holds the text the file has now.
    OnlyCurrent,
    /// Not a file the review compares (not text, or over 2 MB).
    Unsupported,
}

/// The earlier versions of a file, one page of them, or why there is no list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum ReviewVersions {
    List {
        /// Newest first. A page may hold few rows, or none, while `next` says there is more.
        rows: Vec<VersionRow>,
        /// What to hand back as `after` for the next page (a number to pass on, nothing to read into); `None`: the
        /// end of the history was reached.
        next: Option<usize>,
        /// The path the file had before its oldest version, when that commit renamed it (the versions under the old
        /// name are not listed). Told with the last page only.
        renamed_from: Option<String>,
        /// The repository is a shallow copy: the history may end before the file's first version.
        cut: bool,
    },
    Unavailable {
        reason: VersionsReason,
    },
}
