//! Workspaces: what a window shows. `Project` (a folder, shared `ProjectCore`), `SingleFile`, `Notes`.

pub mod fsm;
mod project;
mod registry;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use project::{CoreEnv, ProjectCore};
pub use registry::{OpenError, Prepared, Ticket, WorkspaceRegistry};

use crate::conditions::WorkspaceKind;
use crate::error::{ErrorKind, IpcError};
use crate::paths::{FileScope, Root, VaultError, VaultPath};
use crate::state::StateLocation;

/// Process-unique workspace id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(type = "number")]
pub struct WorkspaceId(pub u64);

/// What to show, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRef {
    Project(Root),
    /// Canonical existing file.
    SingleFile(PathBuf),
    Notes,
}

/// Wire | session form of a workspace reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum WorkspaceRefDto {
    Project { path: String },
    SingleFile { path: String },
    Notes,
}

/// Identity of a workspace (two refs, one key -> one workspace).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum WorkspaceKey {
    Project(String),
    File(String),
    Notes,
}

impl WorkspaceRef {
    /// Folder -> project, file -> single file.
    ///
    /// # Errors
    /// `NotFound`, `Io`.
    pub fn classify(path: &Path) -> Result<Self, VaultError> {
        let meta = std::fs::metadata(path).map_err(|e| VaultError::io(path, e))?;
        if meta.is_dir() {
            Ok(Self::Project(Root::open(path)?))
        } else {
            Ok(Self::SingleFile(
                FileScope::open(path)?.file().to_path_buf(),
            ))
        }
    }

    /// Validates a wire reference (paths re-checked on disk).
    ///
    /// # Errors
    /// `NotFound`, `NotAFolder` | `NotAFile` (kind mismatch), `Io`.
    pub fn parse(dto: &WorkspaceRefDto) -> Result<Self, VaultError> {
        match dto {
            WorkspaceRefDto::Project { path } => Ok(Self::Project(Root::open(Path::new(path))?)),
            WorkspaceRefDto::SingleFile { path } => Ok(Self::SingleFile(
                FileScope::open(Path::new(path))?.file().to_path_buf(),
            )),
            WorkspaceRefDto::Notes => Ok(Self::Notes),
        }
    }

    #[must_use]
    pub fn to_dto(&self) -> WorkspaceRefDto {
        match self {
            Self::Project(root) => WorkspaceRefDto::Project {
                path: root.path().display().to_string(),
            },
            Self::SingleFile(path) => WorkspaceRefDto::SingleFile {
                path: path.display().to_string(),
            },
            Self::Notes => WorkspaceRefDto::Notes,
        }
    }

    #[must_use]
    pub fn kind(&self) -> WorkspaceKind {
        match self {
            Self::Project(_) => WorkspaceKind::Project,
            Self::SingleFile(_) => WorkspaceKind::SingleFile,
            Self::Notes => WorkspaceKind::Notes,
        }
    }

    pub(crate) fn key(&self) -> WorkspaceKey {
        match self {
            Self::Project(root) => WorkspaceKey::Project(root.key().as_str().to_owned()),
            Self::SingleFile(path) => {
                WorkspaceKey::File(crate::paths::fold(&crate::paths::slash(path)))
            }
            Self::Notes => WorkspaceKey::Notes,
        }
    }
}

/// Single-file workspace: the file's scope only (no project state, no `.permark/`).
#[derive(Debug, Clone)]
pub struct FileSession {
    pub scope: FileScope,
    /// Reads the file and tells windows when it changes.
    pub documents: std::sync::Arc<crate::document::Documents>,
}

/// A live workspace.
#[derive(Debug, Clone)]
pub enum Workspace {
    Project(std::sync::Arc<ProjectCore>),
    SingleFile(FileSession),
    Notes(std::sync::Arc<crate::notes::NotesCore>),
}

/// One workspace as windows list it (switcher bar).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkspaceSummary {
    pub id: WorkspaceId,
    pub kind: WorkspaceKind,
    pub title: String,
    pub path: String,
    /// Number of windows showing it.
    pub windows: u32,
}

/// What a window shows, for its frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum WorkspaceView {
    #[serde(rename_all = "camelCase")]
    Project {
        id: WorkspaceId,
        name: String,
        root: String,
        state_location: StateLocation,
    },
    #[serde(rename_all = "camelCase")]
    SingleFile {
        id: WorkspaceId,
        name: String,
        path: String,
        folder: String,
    },
    /// The Notes window; `folder`: where the notes are (none until the plugin knows).
    #[serde(rename_all = "camelCase")]
    Notes {
        id: WorkspaceId,
        name: String,
        folder: Option<String>,
    },
}

/// State of a window, for its frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum WindowView {
    Welcome,
    /// `showing`: what stays on screen meanwhile.
    Opening {
        target: WorkspaceRefDto,
        showing: Option<WorkspaceView>,
    },
    /// `document`: file to show once a document view exists (project-relative).
    Showing {
        workspace: WorkspaceView,
        document: Option<String>,
    },
    Closing,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("the Notes need the notes plugin")]
    NotesUnavailable,
}

impl From<WorkspaceError> for IpcError {
    fn from(error: WorkspaceError) -> Self {
        match error {
            WorkspaceError::Vault(e) => e.into(),
            WorkspaceError::NotesUnavailable => {
                IpcError::new(ErrorKind::Unsupported).subject("notes")
            }
        }
    }
}

/// Document hint of an open: a vault path inside a project.
pub type DocumentHint = Option<VaultPath>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_and_parse_check_kinds_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a b.md");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(
            WorkspaceRef::classify(dir.path()).unwrap().kind(),
            WorkspaceKind::Project
        );
        assert_eq!(
            WorkspaceRef::classify(&file).unwrap().kind(),
            WorkspaceKind::SingleFile
        );
        assert!(WorkspaceRef::classify(&dir.path().join("missing")).is_err());
        let wrong = WorkspaceRefDto::Project {
            path: file.display().to_string(),
        };
        assert!(matches!(
            WorkspaceRef::parse(&wrong),
            Err(VaultError::NotAFolder(_))
        ));
        let wrong = WorkspaceRefDto::SingleFile {
            path: dir.path().display().to_string(),
        };
        assert!(matches!(
            WorkspaceRef::parse(&wrong),
            Err(VaultError::NotAFile(_))
        ));
    }

    #[test]
    fn dto_round_trip_and_wire_shape() {
        let dir = tempfile::tempdir().unwrap();
        let reference = WorkspaceRef::classify(dir.path()).unwrap();
        let dto = reference.to_dto();
        assert_eq!(WorkspaceRef::parse(&dto).unwrap(), reference);
        let json = serde_json::to_value(&WorkspaceRefDto::Notes).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "notes" }));
    }
}
