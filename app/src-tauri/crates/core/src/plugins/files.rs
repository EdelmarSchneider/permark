//! Files of the workspace a window shows, for an App-scope plugin that works on what the person points at (the
//! converter reads a file and writes its result next to it). The shell offers it through the app's services; the
//! window's own scope applies (open-folder boundary, single file, Notes).

use std::sync::Arc;

use crate::error::IpcError;

pub trait WindowFiles: Send + Sync {
    /// The text of `path` in the workspace of `window`, refused when it has more than `cap` bytes.
    ///
    /// # Errors
    /// No such window | outside | missing | too large | not text.
    fn read(&self, window: &str, path: &str, cap: u64) -> Result<String, IpcError>;

    /// A new file `name` next to `beside` in the workspace of `window`, never over a file (a taken name gets ` (2)`).
    /// Returns its path.
    ///
    /// # Errors
    /// No such window | bad name | outside | the write failed.
    fn write_beside(
        &self,
        window: &str,
        beside: &str,
        name: &str,
        text: &str,
    ) -> Result<String, IpcError>;

    /// Notes something the person did in the activity journal of the window's project (nothing for a single file
    /// | the Notes).
    fn note(&self, window: &str, fact: crate::activity::Fact);
}

/// The service plugins look up (`AppServices.services.get::<WindowFilesApi>()`).
#[derive(Clone)]
pub struct WindowFilesApi(pub Arc<dyn WindowFiles>);
