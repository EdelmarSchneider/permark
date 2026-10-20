//! File watch: OS events -> verified changes ([`FsBatch`]) -> subscribers (the Explorer first, plugins after).

mod noise;
mod own;
mod reconcile;
mod service;
mod sigs;
#[cfg(test)]
mod tests;
mod types;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

pub use noise::{is_git_signal, is_news};
pub use own::OwnWrites;
pub use service::{FolderLookup, Timing, Watcher};
pub use types::{Change, ChangeKind, FsBatch, BULK_CHANGES};

/// Whether changes of the project's files reach the app as they happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveStatus {
    Available,
    /// `reason` is an i18n suffix (`watchFailed`).
    Unavailable {
        reason: String,
    },
}

/// Called when [`LiveStatus`] changes after start (the context is recomputed).
pub type StatusHook = Arc<dyn Fn() + Send + Sync>;

type Callback = Arc<dyn Fn(&FsBatch) + Send + Sync>;

/// Subscribers of a project's batches, called in subscription order on the watch thread (keep callbacks short).
#[derive(Default)]
pub struct WatchHub {
    subs: Mutex<Vec<(u64, Callback)>>,
    next: AtomicU64,
}

impl std::fmt::Debug for WatchHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchHub").finish_non_exhaustive()
    }
}

impl WatchHub {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Listens until the returned [`Subscription`] is dropped.
    #[must_use]
    pub fn subscribe(
        self: &Arc<Self>,
        callback: impl Fn(&FsBatch) + Send + Sync + 'static,
    ) -> Subscription {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.subs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((id, Arc::new(callback)));
        Subscription {
            hub: Arc::downgrade(self),
            id,
        }
    }

    /// Hands a batch to every subscriber, in order.
    pub fn publish(&self, batch: &FsBatch) {
        let subs: Vec<Callback> = self
            .subs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(_, f)| Arc::clone(f))
            .collect();
        for callback in subs {
            callback(batch);
        }
    }
}

/// Teardown symmetry: the callback is removed when this goes.
#[derive(Debug)]
pub struct Subscription {
    hub: Weak<WatchHub>,
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            hub.subs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|(id, _)| *id != self.id);
        }
    }
}
