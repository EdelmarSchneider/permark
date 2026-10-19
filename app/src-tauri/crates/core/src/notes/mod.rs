//! Notes, core half: the workspace the Notes window shows and the seams the `notes` plugin plugs into. The notes
//! themselves (listing, versions, recovery, loose files) are the plugin's; core knows where the documents are
//! ([`NotesScope`]) and reads | saves them like any workspace's.

mod scope;

use std::sync::Arc;

use serde::Serialize;
use ts_rs::TS;

pub use scope::{
    is_loose_id, loose_id_of, loose_key, loose_path, parse_loose, NotesScope, ObserverLease,
    LOOSE_PREFIX,
};

use crate::document::{DocScope, Documents};
use crate::error::IpcError;
use crate::events::EventSink;

/// The Notes workspace: one per process. Its documents are polled for changes (the folder is the person's, other
/// programs and computers write to it).
pub struct NotesCore {
    scope: Arc<NotesScope>,
    documents: Arc<Documents>,
}

impl std::fmt::Debug for NotesCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotesCore").finish_non_exhaustive()
    }
}

impl NotesCore {
    #[must_use]
    pub fn new(scope: Arc<NotesScope>, sink: Arc<dyn EventSink>) -> Arc<Self> {
        let documents = Documents::polled(DocScope::Notes(Arc::clone(&scope)), sink);
        Arc::new(Self { scope, documents })
    }

    #[must_use]
    pub fn scope(&self) -> &Arc<NotesScope> {
        &self.scope
    }

    #[must_use]
    pub fn documents(&self) -> &Arc<Documents> {
        &self.documents
    }
}

/// A note the person shared with agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SharedNote {
    /// The note's file name: what the agent asks for.
    pub id: String,
    pub title: String,
    #[ts(type = "number")]
    pub updated_ms: u64,
}

/// What the agent area asks of the notes: only the notes the person shared, one by one.
pub trait SharedNotes: Send + Sync {
    fn list(&self) -> Vec<SharedNote>;
    /// The text of a shared note, as it is on disk.
    ///
    /// # Errors
    /// `NotFound` when the note is not shared (or not there).
    fn read(&self, id: &str) -> Result<String, IpcError>;
}

/// The shared-notes service, offered through the app's `Services` while the `notes` plugin runs.
#[derive(Clone)]
pub struct SharedNotesApi(pub Arc<dyn SharedNotes>);
