//! What a documents service reads from: a project's vault, or the one file of a single-file workspace.

use std::path::PathBuf;

use serde::Serialize;
use ts_rs::TS;

use crate::error::{ErrorKind, IpcError};
use crate::format::DocEnv;
use crate::links::{resolve, Target};
use crate::paths::{FileScope, Vault, VaultPath};

#[derive(Debug, Clone)]
pub enum DocScope {
    Project(Vault),
    /// Only its own file is a document; the files next to it are assets.
    File(FileScope),
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
    /// Escapes the workspace | invalid | not found; in a single-file workspace, any file but its own.
    pub fn resolve(&self, path: &str) -> Result<(String, PathBuf), IpcError> {
        let parsed = VaultPath::parse(path)?;
        let abs = match self {
            Self::Project(vault) => vault.resolve(&parsed)?,
            Self::File(scope) => {
                let abs = scope.resolve_asset(&parsed)?;
                if abs != scope.file() {
                    return Err(IpcError::new(ErrorKind::NotFound).subject(path));
                }
                abs
            }
        };
        Ok((parsed.key(), abs))
    }

    /// The vault links are resolved against (a single file: its folder).
    fn link_vault(&self) -> Vault {
        match self {
            Self::Project(vault) => vault.clone(),
            Self::File(scope) => Vault::new(scope.folder().clone()),
        }
    }

    /// Where each `href` found in the document `doc` leads.
    #[must_use]
    pub fn resolve_links(&self, doc: &str, hrefs: &[String]) -> Vec<LinkTarget> {
        let Ok(from) = VaultPath::parse(doc) else {
            return hrefs.iter().map(|_| LinkTarget::Outside).collect();
        };
        let vault = self.link_vault();
        hrefs
            .iter()
            .map(|href| resolve(&vault, &from, href).into())
            .collect()
    }
}

impl DocEnv for DocScope {
    fn file_exists(&self, project_path: &str) -> bool {
        let Ok(parsed) = VaultPath::parse(project_path) else {
            return false;
        };
        let abs = match self {
            Self::Project(vault) => vault.resolve(&parsed),
            Self::File(scope) => scope.resolve_asset(&parsed),
        };
        abs.is_ok_and(|abs| std::fs::metadata(abs).is_ok_and(|m| m.is_file()))
    }
}
