//! Typed download failure: state for plugin UIs (ts-rs), mapped onto existing `ErrorKind`s for IPC (no new kind).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{ErrorKind, IpcError};

/// Why a download stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum DownloadError {
    /// No answer: no connection, DNS, TLS, stalled | dropped transfer.
    #[error("offline")]
    Offline,
    /// Server answered `status` instead of the file (incl. a redirect chain past the cap).
    #[error("server answered {status}")]
    Http { status: u16 },
    /// A hop left `https` | the source's hosts, or resolved to a non-public address.
    #[error("host not allowed")]
    HostNotAllowed,
    /// Bytes differ from the pin (size | sha256); part deleted.
    #[error("integrity check failed")]
    Integrity,
    /// Not enough room: `needed` bytes (remaining + margin), `free` available.
    #[error("not enough space: {needed} needed, {free} free")]
    NoSpace {
        #[ts(type = "number")]
        needed: u64,
        #[ts(type = "number")]
        free: u64,
    },
    /// Store read | write failed (`detail`: OS message).
    #[error("disk: {detail}")]
    Disk { detail: String },
    /// Person stopped it; part kept for resume.
    #[error("cancelled")]
    Cancelled,
}

impl DownloadError {
    /// Store IO failure.
    #[must_use]
    pub fn disk(error: &std::io::Error) -> Self {
        Self::Disk {
            detail: error.to_string(),
        }
    }
}

impl From<DownloadError> for IpcError {
    fn from(error: DownloadError) -> Self {
        match error {
            DownloadError::Offline => IpcError::new(ErrorKind::Unavailable).detail("offline"),
            DownloadError::Http { status } => {
                IpcError::new(ErrorKind::Unavailable).detail(format!("status {status}"))
            }
            DownloadError::HostNotAllowed => IpcError::new(ErrorKind::Denied).detail("host"),
            DownloadError::Integrity => IpcError::new(ErrorKind::InvalidData).detail("integrity"),
            DownloadError::NoSpace { needed, free } => IpcError::new(ErrorKind::Io)
                .detail(format!("no space: {needed} needed, {free} free")),
            DownloadError::Disk { detail } => IpcError::new(ErrorKind::Io).detail(detail),
            DownloadError::Cancelled => IpcError::new(ErrorKind::Superseded).detail("cancelled"),
        }
    }
}
