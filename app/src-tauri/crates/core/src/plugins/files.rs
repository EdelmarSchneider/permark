//! Files of the workspace a window shows, for an App-scope plugin that works on what the person points at (the
//! converter reads a file and writes its result next to it). The shell offers it through the app's services; the
//! window's own scope applies (open-folder boundary, single file, Notes).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::error::{ErrorKind, IpcError};
use crate::tree::{Scan, ScanFile};
use crate::watch::{FsBatch, Subscription};

/// What a walk below a folder of a project found.
#[derive(Debug, Clone)]
pub struct FolderScan {
    /// The project folder on disk, as the windows name the project: what results of the project are kept under.
    pub project: String,
    pub scan: Scan,
}

/// Hears the verified changes of a project.
pub type ChangeListener = Arc<dyn Fn(&FsBatch) + Send + Sync>;

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

    /// Every file below `folder` (a project path; `""` = the project folder) in the project of `window` that the
    /// project's ignore rules leave in, with size and modified time, none of it read. `stop` sees each file and ends
    /// the walk by answering `true`; `cancel` ends it too. The scan is `cut_off` then.
    ///
    /// # Errors
    /// No such window | no project | outside | not a folder.
    fn scan_folder(
        &self,
        window: &str,
        folder: &str,
        cancel: &AtomicBool,
        stop: &mut dyn FnMut(&ScanFile) -> bool,
    ) -> Result<FolderScan, IpcError> {
        let _ = (window, folder, cancel, stop);
        Err(IpcError::new(ErrorKind::Unavailable).subject("project"))
    }

    /// Listens to the verified changes of the project of `window` until the subscription is dropped (`None`: the
    /// window shows no project). The listener runs on the watch thread: keep it short.
    fn watch(&self, window: &str, listener: ChangeListener) -> Option<Subscription> {
        let _ = (window, listener);
        None
    }
}

/// The service plugins look up (`AppServices.services.get::<WindowFilesApi>()`).
#[derive(Clone)]
pub struct WindowFilesApi(pub Arc<dyn WindowFiles>);
