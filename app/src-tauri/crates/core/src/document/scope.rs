//! What a documents service reads from: a project's vault, or the one file of a single-file workspace.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use ts_rs::TS;

use crate::error::IpcError;
use crate::format::DocEnv;
use crate::links::{resolve, Target};
use crate::notes::NotesScope;
use crate::paths::{FileScope, Vault, VaultPath};

#[derive(Debug, Clone)]
pub enum DocScope {
    Project(Vault),
    /// A single-file workspace: its own file and any file below its folder (links to siblings open there); nothing
    /// outside the folder (`..`, absolute paths, links | junctions leading out are refused).
    File(FileScope),
    /// The Notes window: the notes folder and the loose files.
    Notes(Arc<NotesScope>),
}

/// Where a link leads (the UI acts on it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum LinkTarget {
    Doc {
        path: String,
        anchor: Option<String>,
    },
    Folder {
        path: String,
    },
    Asset {
        path: String,
    },
    Web {
        url: String,
    },
    Anchor {
        id: String,
    },
    Missing {
        path: String,
    },
    Outside,
}

impl From<Target> for LinkTarget {
    fn from(target: Target) -> Self {
        match target {
            Target::Doc(path, anchor) => Self::Doc {
                path: path.to_string(),
                anchor,
            },
            Target::Folder(path) => Self::Folder {
                path: path.to_string(),
            },
            Target::Asset(path) => Self::Asset {
                path: path.to_string(),
            },
            Target::Web(url) => Self::Web { url },
            Target::Anchor(id) => Self::Anchor { id },
            Target::Missing(path) => Self::Missing { path },
            Target::Outside => Self::Outside,
        }
    }
}

impl DocScope {
    /// Folded identity key + absolute path of a document path (checked against the boundary).
    ///
    /// # Errors
    /// Escapes the workspace (single file: its folder) | invalid | not found.
    pub fn resolve(&self, path: &str) -> Result<(String, PathBuf), IpcError> {
        let parsed = VaultPath::parse(path)?;
        let abs = match self {
            Self::Project(vault) => vault.resolve(&parsed)?,
            // any file under the folder (B3: links to siblings); `resolve_under` refuses every way out
            Self::File(scope) => scope.resolve_asset(&parsed)?,
            Self::Notes(scope) => return scope.resolve(path),
        };
        Ok((parsed.key(), abs))
    }

    /// Like [`DocScope::resolve`] for a file that may not exist yet (a new file of the workspace; single file: a file
    /// of its folder).
    ///
    /// # Errors
    /// Escapes the workspace | invalid.
    pub fn resolve_new(&self, path: &str) -> Result<(String, PathBuf), IpcError> {
        let parsed = VaultPath::parse(path)?;
        let abs = match self {
            Self::Project(vault) => vault.resolve(&parsed)?,
            Self::File(scope) => scope.resolve_asset(&parsed)?,
            Self::Notes(scope) => return scope.resolve_new(path),
        };
        Ok((parsed.key(), abs))
    }

    /// The vault links are resolved against, and the document's path in it (a single file: its folder; a loose
    /// note file: none, its links are inert).
    fn link_base(&self, doc: &str) -> Option<(Vault, VaultPath)> {
        match self {
            Self::Project(vault) => Some((vault.clone(), VaultPath::parse(doc).ok()?)),
            Self::File(scope) => Some((
                Vault::new(scope.folder().clone()),
                VaultPath::parse(doc).ok()?,
            )),
            Self::Notes(scope) => scope.link_base(doc),
        }
    }

    /// Where each `href` found in the document `doc` leads.
    #[must_use]
    pub fn resolve_links(&self, doc: &str, hrefs: &[String]) -> Vec<LinkTarget> {
        let Some((vault, from)) = self.link_base(doc) else {
            return hrefs.iter().map(|_| LinkTarget::Outside).collect();
        };
        hrefs
            .iter()
            .map(|href| resolve(&vault, &from, href).into())
            .collect()
    }
}

impl DocEnv for DocScope {
    fn file_exists(&self, project_path: &str) -> bool {
        if let Self::Notes(scope) = self {
            return scope.file_exists(project_path);
        }
        let Ok(parsed) = VaultPath::parse(project_path) else {
            return false;
        };
        let abs = match self {
            Self::Project(vault) => vault.resolve(&parsed),
            Self::File(scope) => scope.resolve_asset(&parsed),
            Self::Notes(_) => return false,
        };
        abs.is_ok_and(|abs| std::fs::metadata(abs).is_ok_and(|m| m.is_file()))
    }
}
