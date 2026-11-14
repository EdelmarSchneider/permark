//! The notes the person shares with this project's agents, as the project's agent popover asks for them. The project
//! is always the folder this plugin serves (derived here, never sent by a window); the shares themselves are kept by
//! the `notes` plugin, which may be off: then every answer is `Unavailable`.

use std::sync::Arc;

use permark_core::notes::{SharedNote, SharedNotesApi};
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::tools::Tools;

/// The notes shared with this project, and the person's other notes that can be added (newest first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectNotes {
    pub shared: Vec<SharedNote>,
    pub candidates: Vec<SharedNote>,
}

#[derive(Deserialize)]
pub(crate) struct NoteArgs {
    /// The note's file name.
    pub name: String,
}

fn api(tools: &Tools) -> Result<Arc<SharedNotesApi>, IpcError> {
    tools
        .shared_notes()
        .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("notes"))
}

/// What the popover lists.
///
/// # Errors
/// `Unavailable` while the notes are off.
pub(crate) fn project_notes(tools: &Tools) -> Result<ProjectNotes, IpcError> {
    let api = api(tools)?;
    let shared = api.0.list(&tools.notes_key());
    let candidates = api
        .0
        .candidates()
        .into_iter()
        .filter(|note| !shared.iter().any(|s| s.id == note.id))
        .collect();
    Ok(ProjectNotes { shared, candidates })
}

/// Shares a note with this project; answers the new list.
///
/// # Errors
/// `Unavailable` while the notes are off; not a note there.
pub(crate) fn share(tools: &Tools, args: &NoteArgs) -> Result<ProjectNotes, IpcError> {
    api(tools)?.0.share(&tools.notes_key(), &args.name)?;
    project_notes(tools)
}

/// Stops sharing a note with this project; answers the new list.
///
/// # Errors
/// `Unavailable` while the notes are off.
pub(crate) fn unshare(tools: &Tools, args: &NoteArgs) -> Result<ProjectNotes, IpcError> {
    api(tools)?.0.unshare(&tools.notes_key(), &args.name);
    project_notes(tools)
}
