//! Corrections within a time budget: the engine can take many seconds on a crafted word, and no word menu may wait
//! that long. The engine cannot be stopped midway, so a run past its budget ends on its own thread with its answer
//! dropped; a cap on such threads keeps repeated asks from piling them up.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// Runs work on a thread of its own, waits at most `wait` for it, and lets at most `max` such threads run at once.
pub(crate) struct Budget {
    wait: Duration,
    max: usize,
    running: Arc<AtomicUsize>,
}

/// A place counted among the running threads, given back when dropped: the work ended, or it never started (no
/// place left, no thread).
struct Place(Arc<AtomicUsize>);

impl Drop for Place {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Budget {
    pub(crate) fn new(wait: Duration, max: usize) -> Self {
        Self {
            wait,
            max,
            running: Arc::default(),
        }
    }

    /// What `work` returns within the wait. `None`: it took longer (it ends in the background, its answer dropped),
    /// or `max` runs still go on (it is not started), or no thread could be started.
    pub(crate) fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let place = Place(Arc::clone(&self.running));
        if self.running.fetch_add(1, Ordering::AcqRel) >= self.max {
            return None;
        }
        let (answer, answered) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("spelling-suggest".to_owned())
            .spawn(move || {
                let _place = place;
                // the caller may have stopped waiting
                let _ = answer.send(work());
            })
            .ok()?;
        answered.recv_timeout(self.wait).ok()
    }
}
