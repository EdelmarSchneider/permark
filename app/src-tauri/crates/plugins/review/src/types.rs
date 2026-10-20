//! What the review plugin tells the UI (ts-rs exported).

use permark_core::format::Block;
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
    pub changes: Vec<ReviewChange>,
}
