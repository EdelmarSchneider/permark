//! What the unread plugin tells the UI (ts-rs exported).

use serde::Serialize;
use ts_rs::TS;

/// The unread documents of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UnreadList {
    /// The first look at the disk is done (before it nothing is unread).
    pub ready: bool,
    /// Newest change first (file time); the same time | none: by path.
    pub unread: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VisitKind {
    Changed,
    New,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VisitItem {
    pub path: String,
    pub kind: VisitKind,
}

/// What changed since the project was last closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SinceVisit {
    /// The first look at the disk is done (ask again after `unread:changed` until it is).
    pub ready: bool,
    /// This project was never closed before: nothing to compare with.
    pub first: bool,
    /// When the last visit ended.
    #[ts(type = "number")]
    pub at_ms: u64,
    pub items: Vec<VisitItem>,
    /// Items beyond the list.
    pub more: u32,
}
