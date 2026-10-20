//! IPC error shape. Module errors stay typed (`thiserror`); each maps itself into [`IpcError`] in its own
//! `impl From` (no central match). UI words an error by `kind` (i18n `error.<kind>`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Flat error category: what the UI words. camelCase on the wire = i18n key suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ErrorKind {
    NotFound,
    PathEscape,
    InvalidPath,
    Denied,
    Io,
    Exists,
    Binary,
    TooLarge,
    Encoding,
    Unsupported,
    InvalidData,
    Conflict,
    Unavailable,
    Superseded,
    Internal,
    /// A name that cannot be used (`detail`: the reason, `empty | chars | edge | dot | reserved | tooLong | notMarkdown`).
    InvalidName,
    /// A folder moved into itself | its own descendant.
    IntoItself,
    /// Between the project and a worktree (or two roots in one move).
    OtherRoot,
    /// Git's folder, PerMark's own folder, a root, a folder holding a worktree.
    Protected,
    /// Two items of one operation overlap or land on one name.
    Overlap,
    /// The recycle bin refused.
    Trash,
    /// A move failed half way and something could not be put back (`detail` lists it).
    PartlyMoved,
    /// The person has not set an editor.
    EditorNotSet,
    /// The editor could not be started (`detail`: `notFound | dash | <template problem> | <system text>`).
    EditorFailed,
}

impl ErrorKind {
    /// Every kind (i18n parity test: one `error.<kind>` per entry).
    pub const ALL: [Self; 24] = [
        Self::NotFound,
        Self::PathEscape,
        Self::InvalidPath,
        Self::Denied,
        Self::Io,
        Self::Exists,
        Self::Binary,
        Self::TooLarge,
        Self::Encoding,
        Self::Unsupported,
        Self::InvalidData,
        Self::Conflict,
        Self::Unavailable,
        Self::Superseded,
        Self::Internal,
        Self::InvalidName,
        Self::IntoItself,
        Self::OtherRoot,
        Self::Protected,
        Self::Overlap,
        Self::Trash,
        Self::PartlyMoved,
        Self::EditorNotSet,
        Self::EditorFailed,
    ];
}

/// Error as the frontend receives it: kind + what it is about + OS detail (for a "details" disclosure).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IpcError {
    pub kind: ErrorKind,
    /// Path | name the message is about (shown).
    pub subject: Option<String>,
    /// OS | parser message (details disclosure).
    pub detail: Option<String>,
}

impl IpcError {
    #[must_use]
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            subject: None,
            detail: None,
        }
    }

    #[must_use]
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// IO failure about `subject`: kind from the OS error (`NotFound`, `Denied`, `Exists`, else `Io`).
    #[must_use]
    pub fn io(subject: impl Into<String>, source: &std::io::Error) -> Self {
        let kind = match source.kind() {
            std::io::ErrorKind::NotFound => ErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorKind::Denied,
            std::io::ErrorKind::AlreadyExists => ErrorKind::Exists,
            _ => ErrorKind::Io,
        };
        Self::new(kind).subject(subject).detail(source.to_string())
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.kind)?;
        if let Some(subject) = &self.subject {
            write!(f, ": {subject}")?;
        }
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

impl std::error::Error for IpcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_serialize_as_camel_case_i18n_suffixes() {
        let json = serde_json::to_string(&ErrorKind::PathEscape).unwrap();
        assert_eq!(json, "\"pathEscape\"");
        assert_eq!(ErrorKind::ALL.len(), 24);
        assert_eq!(
            serde_json::to_string(&ErrorKind::EditorNotSet).unwrap(),
            "\"editorNotSet\""
        );
    }

    #[test]
    fn io_errors_map_by_os_kind() {
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(IpcError::io("a.md", &missing).kind, ErrorKind::NotFound);
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(IpcError::io("a.md", &denied).kind, ErrorKind::Denied);
        let other = std::io::Error::other("disk on fire");
        let e = IpcError::io("a.md", &other);
        assert_eq!(e.kind, ErrorKind::Io);
        assert_eq!(e.subject.as_deref(), Some("a.md"));
        assert_eq!(e.detail.as_deref(), Some("disk on fire"));
    }
}
