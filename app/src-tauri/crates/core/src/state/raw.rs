//! Raw files: the person's data kept in the layout of an earlier version (`meta.toml`, `comments.json`, `quiet.json`),
//! not wrapped in the state envelope. Same safety as the other state files: nothing is written behind a link.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{private, Guard, StateError};
use crate::fsx::{retry, write_atomic, write_atomic_checked};

/// Pauses before a transient read error (an antivirus scan, a sync client) is believed.
const READ_RETRY: [Duration; 2] = [Duration::from_millis(50), Duration::from_millis(150)];

#[derive(Debug, Clone)]
enum Guarded {
    /// Not inside a project's `.permark/`.
    No,
    /// In a state folder (`local/`): the folder and the `.permark/` above it are real folders.
    State(Guard),
    /// Directly in `.permark/` (shared with the team): `.permark/` is a real folder; no self-ignore file.
    Shared { permark: PathBuf },
}

/// One file read and written as bytes.
#[derive(Debug, Clone)]
pub struct RawFile {
    path: PathBuf,
    guard: Guarded,
}

impl RawFile {
    pub(super) fn private(path: PathBuf, guard: Option<Guard>) -> Self {
        Self {
            path,
            guard: guard.map_or(Guarded::No, Guarded::State),
        }
    }

    pub(super) fn shared(path: PathBuf, permark: PathBuf) -> Self {
        Self {
            path,
            guard: Guarded::Shared { permark },
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The bytes, `None` when there is no file.
    ///
    /// # Errors
    /// Any other IO failure (the file may be fine a moment later).
    pub fn read(&self) -> io::Result<Option<Vec<u8>>> {
        match retry(&READ_RETRY, || std::fs::read(&self.path)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Writes atomically; refused when a link stands in place of `.permark/` or its state folder.
    ///
    /// # Errors
    /// The refusal, or the IO failure.
    pub fn write(&self, bytes: &[u8]) -> Result<(), StateError> {
        self.guarded()?;
        write_atomic(&self.path, bytes).map_err(|e| StateError::io(&self.path, e))
    }

    /// [`RawFile::write`] only while the file still holds `expected` (`None`: no file), checked right before the
    /// rename: another program that wrote it since it was read is never written over (`Ok(false)`, file untouched).
    ///
    /// # Errors
    /// The refusal, or the IO failure.
    pub fn write_over(&self, expected: Option<&[u8]>, bytes: &[u8]) -> Result<bool, StateError> {
        self.guarded()?;
        write_atomic_checked(&self.path, bytes, || match std::fs::read(&self.path) {
            Ok(now) => Ok(expected == Some(now.as_slice())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(expected.is_none()),
            Err(error) => Err(error),
        })
        .map_err(|e| StateError::io(&self.path, e))
    }

    /// Refuses a write when a link stands in place of `.permark/` or its state folder.
    fn guarded(&self) -> Result<(), StateError> {
        let fail = |e| StateError::io(&self.path, e);
        match &self.guard {
            Guarded::No => {}
            Guarded::State(guard) => {
                if let Some(dir) = self.path.parent() {
                    private::prepare(&guard.permark, &guard.state, dir).map_err(fail)?;
                }
            }
            Guarded::Shared { permark } => {
                if !private::is_real_folder(permark) {
                    return Err(fail(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "the .permark folder is a link or not a folder",
                    )));
                }
            }
        }
        Ok(())
    }

    /// Deletes the file (a missing one is done).
    ///
    /// # Errors
    /// IO failure.
    pub fn remove(&self) -> io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Length and modification time (ms) of the file, `None` when it is missing: a cheap "did it change".
    #[must_use]
    pub fn stamp(&self) -> Option<(u64, u128)> {
        let meta = std::fs::metadata(&self.path).ok()?;
        let ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis());
        Some((meta.len(), ms))
    }
}
