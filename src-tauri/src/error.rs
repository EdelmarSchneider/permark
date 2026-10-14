//! The error type returned by every command. It serializes as `{ kind, message }` so the UI can
//! branch on `kind` and show `message`.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::git::GitError;
use crate::meta::MetaError;
use crate::render::RenderError;
use crate::session::SessionError;
use crate::vault::VaultError;
use crate::watch::WatchError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    Watch(#[from] WatchError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Meta(#[from] MetaError),
    #[error("no folder is open")]
    NoVault,
    /// A newer request to open a folder arrived first, so this one was dropped.
    #[error("another folder was opened in the meantime")]
    Superseded,
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    /// A short stable identifier for the UI.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Vault(e) => e.kind(),
            Self::Render(RenderError::Io { source, .. }) => {
                if source.kind() == std::io::ErrorKind::NotFound {
                    "notFound"
                } else {
                    "io"
                }
            }
            Self::Watch(_) => "watch",
            Self::Git(_) => "git",
            Self::Session(_) => "session",
            Self::Meta(e) => e.kind(),
            Self::NoVault => "noVault",
            Self::Superseded => "superseded",
            Self::Internal(_) => "internal",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn json(e: &AppError) -> serde_json::Value {
        serde_json::to_value(e).expect("json")
    }

    #[test]
    fn serializes_as_kind_and_message() {
        let e = AppError::from(VaultError::Escapes("../x".to_owned()));
        assert_eq!(
            json(&e),
            serde_json::json!({"kind": "pathEscape", "message": "path is outside the folder: ../x"})
        );
        assert_eq!(
            json(&AppError::NoVault),
            serde_json::json!({"kind": "noVault", "message": "no folder is open"})
        );
    }

    #[test]
    fn kinds_distinguish_not_found_from_other_io_errors() {
        let nf = AppError::Render(RenderError::Io {
            path: "a.md".to_owned(),
            source: io::Error::from(io::ErrorKind::NotFound),
        });
        assert_eq!(nf.kind(), "notFound");
        let denied = AppError::Render(RenderError::Io {
            path: "a.md".to_owned(),
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        });
        assert_eq!(denied.kind(), "io");
        assert_eq!(AppError::from(GitError::WorkerGone).kind(), "git");
        assert_eq!(AppError::Internal("x".to_owned()).kind(), "internal");
        assert_eq!(AppError::Superseded.kind(), "superseded");
        assert_eq!(
            AppError::from(VaultError::NotFound("p".to_owned())).kind(),
            "notFound"
        );
    }
}
