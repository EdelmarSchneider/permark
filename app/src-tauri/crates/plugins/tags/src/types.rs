//! What the tags plugin tells the UI (ts-rs exported).

use std::collections::BTreeMap;

use serde::Serialize;
use ts_rs::TS;

/// A file that has a tag or a description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IndexEntry {
    pub path: String,
    /// Front matter tags first, then the ones only the app has.
    pub tags: Vec<String>,
    pub description: String,
}

/// The look of a tag: names the UI knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TagStyleView {
    pub color: Option<String>,
    pub icon: Option<String>,
}

/// Where the tags live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TagsLocation {
    /// `.permark/meta.toml`, shared with the team.
    Project,
    /// The person's profile.
    Profile,
}

/// Why `meta.toml` cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MetaProblem {
    pub message: String,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TagsStatus {
    pub location: TagsLocation,
    /// Unreadable: the app's tags are read-only until it is fixed.
    pub problem: Option<MetaProblem>,
    /// The last write failed (it is tried again).
    pub write_error: Option<String>,
    /// Changes made and not written yet.
    pub parked: u32,
}

/// Everything the UI shows next to files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TagsIndex {
    /// Changes with every update (the UI drops older answers).
    #[ts(type = "number")]
    pub rev: u64,
    pub entries: Vec<IndexEntry>,
    pub styles: BTreeMap<String, TagStyleView>,
    pub status: TagsStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TagCountView {
    pub name: String,
    pub count: u32,
    /// No file has it from the app: it exists only in front matter.
    pub only_in_files: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TagsOverview {
    #[ts(type = "number")]
    pub rev: u64,
    pub tags: Vec<TagCountView>,
    /// Markdown documents without any tag.
    pub untagged: u32,
    /// Entries of files that are gone.
    pub orphans: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaggedFile {
    pub path: String,
    pub description: String,
    #[ts(type = "number")]
    pub modified_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TagRefView {
    pub name: String,
    /// Written in the file's front matter: the app cannot change it.
    pub file: bool,
    /// Added in the app.
    pub app: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileMeta {
    pub path: String,
    pub tags: Vec<TagRefView>,
    pub description: String,
    /// The app's tags of this file can be changed now.
    pub editable: bool,
}
