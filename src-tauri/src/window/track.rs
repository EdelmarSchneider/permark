//! Following the window while it is moved and resized: what is worth remembering, and when to look.
//!
//! No window system here either: [`Tracker`] is told what the window looks like and says what to
//! remember, and [`Debounce`] waits until the window has been still for a moment before anyone looks.
//! Looking late matters: the window system reports a move before it reports that the window was
//! maximized, so a look taken for each event would take the maximized frame for a normal window.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::session::WindowState;
use crate::util::Coalescer;

/// How the window looks right now, as the window system reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapshot {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub maximized: bool,
    pub minimized: bool,
    pub fullscreen: bool,
}

/// What to remember about the window. While it is maximized the position and size stay those of the
/// window when it was last a normal one (so that "restore" has somewhere to go back to); a minimized
/// window (which the system parks far off screen) and a full-screen one are never remembered, and
/// neither is a window with nothing in it.
#[derive(Debug, Default)]
pub struct Tracker {
    current: Option<WindowState>,
}

impl Tracker {
    /// `start` is what is already known, such as the state the window was restored from.
    pub fn new(start: Option<WindowState>) -> Self {
        Self { current: start }
    }

    pub fn current(&self) -> Option<WindowState> {
        self.current
    }

    /// Takes in how the window looks now. Returns the new state to remember when it changed.
    pub fn observe(&mut self, snapshot: &Snapshot) -> Option<WindowState> {
        if snapshot.minimized || snapshot.fullscreen || snapshot.width == 0 || snapshot.height == 0
        {
            return None;
        }
        let next = match self.current {
            Some(known) if snapshot.maximized => WindowState {
                maximized: true,
                ..known
            },
            _ => WindowState {
                x: snapshot.x,
                y: snapshot.y,
                width: snapshot.width,
                height: snapshot.height,
                scale: snapshot.scale,
                maximized: snapshot.maximized,
            },
        };
        if self.current == Some(next) {
            return None;
        }
        self.current = Some(next);
        Some(next)
    }
}

struct Timing {
    coalescer: Coalescer,
    /// A thread is waiting for the quiet to come.
    armed: bool,
}

struct Shared {
    timing: Mutex<Timing>,
    action: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Timing> {
        self.timing.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Forgets the changes that are waiting. Returns whether there were any.
    fn take(&self) -> bool {
        let mut timing = self.lock();
        let waiting = timing.coalescer.deadline().is_some();
        timing.coalescer.reset();
        waiting
    }
}

/// Runs an action once things have been quiet for `quiet` after a change, but at least every `max` while
/// they keep changing, on a thread of its own: a window dragged across the screen reports hundreds of
/// moves, and only the place where it ends up is worth looking at.
#[derive(Clone)]
pub struct Debounce {
    shared: Arc<Shared>,
}

impl Debounce {
    pub fn new(quiet: Duration, max: Duration, action: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(Shared {
                timing: Mutex::new(Timing {
                    coalescer: Coalescer::new(quiet, max),
                    armed: false,
                }),
                action: Box::new(action),
            }),
        }
    }

    /// Something changed: the action runs soon. Changes that follow postpone it (up to `max`).
    pub fn poke(&self) {
        let start = {
            let mut timing = self.shared.lock();
            timing.coalescer.touch(Instant::now());
            !std::mem::replace(&mut timing.armed, true)
        };
        if start {
            let shared = Arc::clone(&self.shared);
            let spawned = thread::Builder::new()
                .name("permark-window-state".to_owned())
                .spawn(move || Self::wait_and_run(&shared));
            if spawned.is_err() {
                // No thread to wait on: act now rather than never.
                self.shared.lock().armed = false;
                self.flush();
            }
        }
    }

    /// Forgets the changes that are waiting, because the caller deals with them itself.
    pub fn cancel(&self) {
        self.shared.take();
    }

    /// Runs the action now if changes are waiting.
    pub fn flush(&self) {
        if self.shared.take() {
            (self.shared.action)();
        }
    }

    /// The thread: sleeps until the changes have been quiet long enough, runs the action, and ends.
    /// When the changes were taken meanwhile (by `cancel` or `flush`) it just ends.
    fn wait_and_run(shared: &Shared) {
        loop {
            let pause = {
                let mut timing = shared.lock();
                match timing.coalescer.deadline() {
                    None => {
                        timing.armed = false;
                        return;
                    }
                    Some(deadline) if Instant::now() >= deadline => {
                        timing.coalescer.reset();
                        timing.armed = false;
                        drop(timing);
                        (shared.action)();
                        return;
                    }
                    Some(deadline) => deadline.saturating_duration_since(Instant::now()),
                }
            };
            thread::sleep(pause);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn snapshot(x: i32, y: i32, width: u32, height: u32) -> Snapshot {
        Snapshot {
            x,
            y,
            width,
            height,
            scale: 1.5,
            maximized: false,
            minimized: false,
            fullscreen: false,
        }
    }

    fn state(x: i32) -> WindowState {
        WindowState {
            x,
            y: 0,
            width: 1280,
            height: 800,
            scale: 1.0,
            maximized: false,
        }
    }

    #[test]
    fn a_normal_window_is_remembered_and_only_when_it_changed() {
        let mut tracker = Tracker::default();
        assert_eq!(tracker.current(), None);
        let first = tracker.observe(&snapshot(10, 20, 1280, 800)).expect("new");
        assert_eq!(
            first,
            WindowState {
                x: 10,
                y: 20,
                width: 1280,
                height: 800,
                scale: 1.5,
                maximized: false
            }
        );
        assert_eq!(tracker.current(), Some(first));
        assert_eq!(tracker.observe(&snapshot(10, 20, 1280, 800)), None);
        let moved = tracker
            .observe(&snapshot(30, 20, 1280, 800))
            .expect("moved");
        assert_eq!(moved.x, 30);
        let resized = tracker
            .observe(&snapshot(30, 20, 1000, 700))
            .expect("resized");
        assert_eq!((resized.width, resized.height), (1000, 700));
    }

    #[test]
    fn a_maximized_window_keeps_the_bounds_it_had_as_a_normal_one() {
        let mut tracker = Tracker::new(Some(state(10)));
        let maximized = Snapshot {
            maximized: true,
            ..snapshot(-8, -8, 1936, 1056)
        };
        let remembered = tracker.observe(&maximized).expect("changed");
        assert!(remembered.maximized);
        assert_eq!((remembered.x, remembered.width), (10, 1280));
        // events that keep arriving while it stays maximized change nothing
        assert_eq!(tracker.observe(&maximized), None);
        // back to normal: the bounds it has now
        let restored = tracker
            .observe(&snapshot(12, 14, 1280, 800))
            .expect("restored");
        assert!(!restored.maximized);
        assert_eq!((restored.x, restored.y), (12, 14));
    }

    #[test]
    fn a_window_that_was_maximized_from_the_start_remembers_what_it_can() {
        let mut tracker = Tracker::default();
        let maximized = Snapshot {
            maximized: true,
            ..snapshot(-8, -8, 1936, 1056)
        };
        let remembered = tracker.observe(&maximized).expect("changed");
        assert!(remembered.maximized);
        assert_eq!((remembered.x, remembered.width), (-8, 1936));
    }

    #[test]
    fn a_minimized_or_full_screen_window_is_never_remembered() {
        let mut tracker = Tracker::new(Some(state(10)));
        // Windows parks a minimized window far away
        let minimized = Snapshot {
            minimized: true,
            ..snapshot(-32000, -32000, 160, 28)
        };
        assert_eq!(tracker.observe(&minimized), None);
        let fullscreen = Snapshot {
            fullscreen: true,
            ..snapshot(0, 0, 1920, 1080)
        };
        assert_eq!(tracker.observe(&fullscreen), None);
        assert_eq!(tracker.current(), Some(state(10)));
        // and from nothing, nothing is made up
        let mut empty = Tracker::default();
        assert_eq!(empty.observe(&minimized), None);
        assert_eq!(empty.current(), None);
    }

    #[test]
    fn a_window_with_no_size_is_ignored() {
        let mut tracker = Tracker::new(Some(state(10)));
        assert_eq!(tracker.observe(&snapshot(0, 0, 0, 0)), None);
        assert_eq!(tracker.observe(&snapshot(0, 0, 800, 0)), None);
        assert_eq!(tracker.current(), Some(state(10)));
    }

    /// Counts how often the debounced action runs.
    fn counting(quiet_ms: u64, max_ms: u64) -> (Debounce, Arc<AtomicUsize>) {
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let debounce = Debounce::new(
            Duration::from_millis(quiet_ms),
            Duration::from_millis(max_ms),
            move || {
                counted.fetch_add(1, Ordering::SeqCst);
            },
        );
        (debounce, runs)
    }

    fn runs(counter: &AtomicUsize) -> usize {
        counter.load(Ordering::SeqCst)
    }

    fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cond() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_burst_of_changes_runs_the_action_once_after_it_ends() {
        let (debounce, count) = counting(80, 10_000);
        for _ in 0..20 {
            debounce.poke();
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(runs(&count), 0, "not while it keeps changing");
        wait_until("the action", || runs(&count) > 0);
        thread::sleep(Duration::from_millis(250));
        assert_eq!(runs(&count), 1);
    }

    #[test]
    fn a_window_that_never_stops_moving_is_still_looked_at_now_and_then() {
        let (debounce, count) = counting(3_000, 100);
        let start = Instant::now();
        while runs(&count) == 0 {
            assert!(start.elapsed() < Duration::from_secs(10), "never ran");
            debounce.poke();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            start.elapsed() < Duration::from_millis(2_500),
            "waited for quiet"
        );
    }

    #[test]
    fn separate_bursts_run_the_action_separately() {
        let (debounce, count) = counting(30, 10_000);
        debounce.poke();
        wait_until("the first run", || runs(&count) == 1);
        debounce.poke();
        wait_until("the second run", || runs(&count) == 2);
    }

    #[test]
    fn cancelling_drops_the_waiting_changes_and_the_debounce_keeps_working() {
        let (debounce, count) = counting(50, 10_000);
        debounce.poke();
        debounce.cancel();
        thread::sleep(Duration::from_millis(250));
        assert_eq!(runs(&count), 0);
        debounce.poke();
        wait_until("a later run", || runs(&count) == 1);
    }

    #[test]
    fn flushing_runs_the_action_now_but_only_when_changes_wait() {
        let (debounce, count) = counting(60_000, 60_000);
        debounce.flush();
        assert_eq!(runs(&count), 0, "nothing waiting");
        debounce.poke();
        debounce.poke();
        debounce.flush();
        assert_eq!(runs(&count), 1);
        debounce.flush();
        assert_eq!(runs(&count), 1, "once");
    }

    #[test]
    fn a_poke_right_after_a_cancel_is_not_lost() {
        // the waiting thread is still asleep from the first poke when the second one comes
        let (debounce, count) = counting(60, 10_000);
        debounce.poke();
        debounce.cancel();
        debounce.poke();
        wait_until("the run", || runs(&count) == 1);
        thread::sleep(Duration::from_millis(200));
        assert_eq!(runs(&count), 1);
    }
}
