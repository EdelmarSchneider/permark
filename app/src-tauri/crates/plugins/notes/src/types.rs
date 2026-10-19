//! What the notes service says to the window: plain data, one shape per answer.

use serde::Serialize;
use ts_rs::TS;

use crate::folder::FolderInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum EntryKind {
    /// A file in the notes folder.
    Note,
    /// A loose file: one opened here that is somewhere else.
    File,
}

/// How a loose file stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LooseState {
    Clean,
    /// Text was typed into it and is not in the file.
    Unsaved,
    ChangedOnDisk,
    /// Not where it was.
    Gone,
}

/// One row of the list.
#[allow(clippy::struct_excessive_bools)] // marks of a row, sent as they are
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteEntry {
    /// The document path: a note's file name, a loose file's `@file/<id>/<name>`.
    pub path: String,
    pub kind: EntryKind,
    /// The file's name.
    pub name: String,
    /// The first line, marks removed. Empty for a note with no text (the window says "Untitled note").
    pub title: String,
    pub snippet: String,
    #[ts(type = "number")]
    pub modified_ms: u64,
    #[ts(type = "number")]
    pub size: u64,
    pub pinned: bool,
    /// Shared with agents.
    pub shared: bool,
    /// Copied out and not edited since.
    pub copied: bool,
    /// Text typed and never saved was found after a crash.
    pub recovered: bool,
    /// A loose file: the folder it is in, as the list shows it.
    pub folder: Option<String>,
    /// A loose file: how it stands.
    pub state: Option<LooseState>,
    /// A loose file: text that was typed is kept and not in the file.
    pub pending: bool,
}

/// The notes folder and what is in it, in the order the list shows: pinned (the person's order), the other notes
/// newest first, then the loose files (opened last first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteList {
    pub folder: FolderInfo,
    pub entries: Vec<NoteEntry>,
    /// The document that was open when the window was last closed.
    pub open: Option<String>,
}

/// What the welcome screen shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NotesSummary {
    pub pinned: Vec<NoteEntry>,
    pub latest: Vec<NoteEntry>,
    pub count: u32,
}

/// Notes sent to the recycle bin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Deleted {
    /// Asks for them back; none when they are too big to keep for it.
    #[ts(type = "number | null")]
    pub undo_id: Option<u64>,
    pub names: Vec<String>,
}

/// A note moved into a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Moved {
    /// Where the copy is (a full path: the person chose the folder).
    pub destination: String,
    #[ts(type = "number | null")]
    pub undo_id: Option<u64>,
    /// The note changed while it was copied: both are kept, nothing was removed.
    pub kept: bool,
}

/// What undoing a move did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MoveUndone {
    /// The note that is back.
    pub note: String,
    /// The copy in the project was taken away; false when it was changed since and was left there.
    pub removed: bool,
}
