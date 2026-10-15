//! File operations in the Explorer: rename, move, delete and new folders, with the links of the documents
//! kept working.
//!
//! Everything here is plain Rust over a [`Vault`](crate::vault::Vault), so it is tested without a window:
//!
//! * [`scan`] finds the destination of every link, image and reference definition in a Markdown text, as
//!   exact byte ranges;
//! * [`plan`] decides which of them change when files move, and what they change to;
//! * [`ops`] checks and performs the file system operations (nothing is ever overwritten, nothing leaves the
//!   project, deleting goes to the recycle bin);
//! * [`service`] ties them together: preview, apply (guarded writes, own-write suppression) and undo.

mod ops;
mod plan;
pub(crate) mod scan;
mod service;
mod world;

use std::io;

use serde::{Deserialize, Serialize};

pub use ops::{OsRecycler, Recycler};
pub use plan::Move;
pub use service::{FileOpsService, Followers};

use crate::edit::NameProblem;
use crate::vault::VaultError;

#[derive(Debug, thiserror::Error)]
pub enum FileOpError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("{0} already exists")]
    Exists(String),
    #[error("{0}")]
    InvalidName(NameProblem),
    #[error("{0} can't be moved into itself")]
    IntoItself(String),
    #[error("{0} is in another root: files can't be moved between the project and a worktree")]
    OtherRoot(String),
    #[error("{0} can't be changed here")]
    Protected(String),
    #[error("{path} holds the linked worktree {worktree}, so it can't be moved or deleted here")]
    HoldsWorktree { path: String, worktree: String },
    #[error("{0} overlaps another item of the same move")]
    Overlap(String),
    #[error("file operations need a folder to be open, not a single file")]
    SimpleMode,
    #[error("there is nothing to do")]
    Nothing,
    #[error("that preview is no longer available; try again")]
    UnknownPlan,
    #[error("that operation can no longer be undone")]
    UnknownOperation,
    #[error("couldn't move {path}: {source}")]
    Io { path: String, source: io::Error },
    #[error(
        "{reason}. These could not be put back and stay where they were moved to: {}",
        .left.iter().map(|m| m.to.as_str()).collect::<Vec<_>>().join(", ")
    )]
    PartlyMoved {
        reason: Box<FileOpError>,
        left: Vec<Move>,
    },
    #[error("couldn't move to the recycle bin: {0}")]
    Trash(String),
}

impl FileOpError {
    /// A short stable identifier for the UI.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Vault(e) => e.kind(),
            Self::Exists(_) => "exists",
            Self::InvalidName(_) => "invalidName",
            Self::IntoItself(_) => "intoItself",
            Self::OtherRoot(_) => "otherRoot",
            Self::Protected(_) => "protected",
            Self::HoldsWorktree { .. } => "holdsWorktree",
            Self::Overlap(_) => "overlap",
            Self::SimpleMode => "simpleMode",
            Self::Nothing => "nothing",
            Self::UnknownPlan => "unknownPlan",
            Self::UnknownOperation => "unknownOperation",
            Self::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied => "denied",
            Self::Io { .. } => "io",
            Self::PartlyMoved { .. } => "partlyMoved",
            Self::Trash(_) => "trash",
        }
    }
}

/// Why a document's links were not updated (or an undo could not restore it). Matches the UI's `SkipReason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// Too large to look at.
    TooLarge,
    /// Not UTF-8 text.
    NotUtf8,
    Unreadable,
    /// The file changed after the preview (or since the operation), so it was left alone.
    Changed,
    /// The new version could not be written.
    WriteFailed,
    /// The file is gone.
    Missing,
    /// Something else is where the file has to go back to (an undo).
    Occupied,
    /// The document is open with unsaved edits in the Visual editor, which cannot take link changes the way
    /// the Source editor does: it is left alone, to be saved first.
    UnsavedVisual,
}

/// A file that was left alone, and why. Matches the UI's `SkippedFile`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFile {
    pub path: String,
    pub reason: SkipReason,
}

/// One link to be changed in a document. Matches the UI's `PreviewChange`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChange {
    pub line: usize,
    pub old: String,
    pub new: String,
}

/// The links of one document that change. Matches the UI's `PreviewFile`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFile {
    pub path: String,
    /// Where the document will be, when it moves too.
    pub moved_to: Option<String>,
    pub changes: Vec<PreviewChange>,
}

/// What a move would do. Matches the UI's `MovePreview`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MovePreview {
    /// Identifies the preview for [`FileOpsService::apply_move`].
    pub plan_id: u64,
    /// The moves as they will be made (paths with the real case).
    pub moves: Vec<Move>,
    /// How many links change in all, and in how many files.
    pub link_count: usize,
    pub file_count: usize,
    /// The first documents and links (the list is cut off after a few thousand links).
    pub files: Vec<PreviewFile>,
    pub truncated: bool,
    /// Documents that could not be looked at.
    pub skipped: Vec<SkippedFile>,
}

/// The text of a document being edited, with changes that are not saved. Input of `apply_move`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferText {
    pub path: String,
    pub text: String,
}

/// A change to the text of an editor: UTF-16 offsets (what the editor counts in).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub from: usize,
    pub to: usize,
    pub insert: String,
}

/// The changes for the editor's unsaved text of one document. Matches the UI's `BufferEdits`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferEdits {
    pub path: String,
    /// Where the document is after the move.
    pub new_path: String,
    pub edits: Vec<TextEdit>,
}

/// The result of a move. Matches the UI's `MoveOutcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveOutcome {
    /// For `undo_file_op`.
    pub op_id: u64,
    pub moved: Vec<Move>,
    pub files_updated: usize,
    pub links_updated: usize,
    pub buffers: Vec<BufferEdits>,
    pub skipped: Vec<SkippedFile>,
}

/// The result of a delete. Matches the UI's `DeleteOutcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOutcome {
    pub deleted: Vec<String>,
}

/// The result of a new folder. Matches the UI's `CreatedFolder`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedFolder {
    pub op_id: u64,
    pub path: String,
}

/// The result of an undo. Matches the UI's `UndoOutcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoOutcome {
    pub moved_back: Vec<Move>,
    pub files_restored: usize,
    pub links_restored: usize,
    pub folders_removed: usize,
    pub could_not: Vec<SkippedFile>,
}
