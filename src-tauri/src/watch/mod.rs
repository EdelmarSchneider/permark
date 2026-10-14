//! Following the filesystem: events become clean, deduplicated batches for the UI.
//!
//! Pipeline: `notify` (one watcher per root) -> [`aggregate`] (150 ms quiet / 1 s max, noise dropped
//! early by [`filter`]) -> [`reconcile`] (stat + content hash against remembered [`sigs`], so no-op
//! changes vanish and moves become renames) -> [`service::assemble`] (bulk detection, 200-path cap)
//! -> a sink. Everything except [`service`] is pure logic tested without real filesystem events.

pub mod aggregate;
pub mod filter;
pub mod own;
pub mod reconcile;
pub mod service;
pub mod sigs;
pub mod types;

pub use filter::{PathFilter, Verdict};
pub use own::OwnWrites;
pub use service::{GitProbe, WatchConfig, WatchService};
pub use sigs::{FileSig, SigStore};
pub use types::{ChangeKind, FsBatch, FsChange};

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("cannot watch the folder: {0}")]
    Notify(String),
    #[error("cannot start the watcher thread: {0}")]
    Spawn(String),
}
