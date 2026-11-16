//! Notes, core half: the workspace the Notes tab shows and the seams the `notes` plugin plugs into. The notes
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
use crate::fsx::FolderIdentity;
use crate::paths::Root;
use crate::state::ProjectId;

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

/// The project a note is shared with. Always derived by the app from the project folder it serves
/// ([`ProjectKey::of`]), never from a path a window or an agent names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectKey {
    pub id: ProjectId,
    /// The folder as it is on disk now; `None`: the volume gives no identity.
    pub folder: Option<FolderIdentity>,
    /// The folder's name, shown next to the note.
    pub label: String,
}

impl ProjectKey {
    /// The key of the project folder `root`.
    #[must_use]
    pub fn of(root: &Root) -> Self {
        Self {
            id: ProjectId::of(root),
            folder: FolderIdentity::of(root.path()),
            label: root.name(),
        }
    }
}

/// What the agent area asks of the notes: only the notes the person shared with one project, one by one.
///
/// Fail closed: a share reaches a project only while its id matches AND the folder recorded when it was shared is the
/// same folder now ([`FolderIdentity::same`]). A folder deleted and created again at the path, or one whose identity
/// cannot be read now, sees nothing; a share recorded without an identity (a volume with none) matches by id alone.
pub trait SharedNotes: Send + Sync {
    /// The notes shared with `project`, the person's order of sharing.
    fn list(&self, project: &ProjectKey) -> Vec<SharedNote>;
    /// The text of a note shared with `project`, as it is on disk.
    ///
    /// # Errors
    /// `NotFound` when the note is not shared with `project` (or not there).
    fn read(&self, project: &ProjectKey, id: &str) -> Result<String, IpcError>;
    /// Shares note `id` with `project` (the folder it is now).
    ///
    /// # Errors
    /// `NotFound`: not a note there; `Unsupported`: a loose file (never shared).
    fn share(&self, project: &ProjectKey, id: &str) -> Result<(), IpcError>;
    /// Stops sharing note `id` with `project` (whatever folder was recorded). Not shared: nothing happens.
    fn unshare(&self, project: &ProjectKey, id: &str);
    /// The person's notes that can be shared, newest first (the picker's list).
    fn candidates(&self) -> Vec<SharedNote>;
}

/// The shared-notes service, offered through the app's `Services` while the `notes` plugin runs.
#[derive(Clone)]
pub struct SharedNotesApi(pub Arc<dyn SharedNotes>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_key_is_the_folders_id_identity_and_name() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("My Project");
        std::fs::create_dir_all(&folder).unwrap();
        let root = Root::open(&folder).unwrap();
        let key = ProjectKey::of(&root);
        assert_eq!(key.id, ProjectId::of(&root));
        assert_eq!(key.label, "My Project");
        assert_eq!(key.folder, FolderIdentity::of(root.path()));
    }
}
