//! The person's yes before a program they set up in the app starts for the first time. The shell asks in a system
//! dialog it words itself: a page that could word it could also hide which program starts.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use crate::process::ProgramCall;

/// Asks the person whether a program may start.
pub trait ProgramConsent: Send + Sync {
    /// Shows `call` (the program's full path, its arguments and variables) and waits for the answer: `true` = allowed.
    /// Blocks; MUST NOT be called on the UI thread (the dialog needs it).
    fn allow(&self, call: &ProgramCall) -> bool;
}

/// The service callers look up (`services.get::<ProgramConsentApi>()`).
#[derive(Clone)]
pub struct ProgramConsentApi(pub Arc<dyn ProgramConsent>);

/// Shortest time a person takes to read the dialog and allow: Allow is its default button (the system dialog cannot
/// change that), so a faster yes is an Enter meant for the text being typed when the dialog came up.
pub const MIN_ANSWER: Duration = Duration::from_millis(700);

/// The system dialog as the app asks it for its whole run: one question at a time (an ask while one is showing is a
/// no, nothing shown), a no kept until the app ends (the same call is never asked about again: repeated asks cannot
/// wear the person down), a yes faster than [`MIN_ANSWER`] taken as a no (and kept as one: asks repeated until a key
/// lands later would get through).
pub struct GuardedConsent {
    dialog: Arc<dyn ProgramConsent>,
    min_answer: Duration,
    /// Held while a dialog is showing.
    showing: Mutex<()>,
    refused: Mutex<HashSet<ProgramCall>>,
}

impl GuardedConsent {
    #[must_use]
    pub fn new(dialog: Arc<dyn ProgramConsent>) -> Self {
        Self::with_min_answer(dialog, MIN_ANSWER)
    }

    fn with_min_answer(dialog: Arc<dyn ProgramConsent>, min_answer: Duration) -> Self {
        Self {
            dialog,
            min_answer,
            showing: Mutex::new(()),
            refused: Mutex::new(HashSet::new()),
        }
    }

    fn refused(&self) -> std::sync::MutexGuard<'_, HashSet<ProgramCall>> {
        self.refused.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ProgramConsent for GuardedConsent {
    fn allow(&self, call: &ProgramCall) -> bool {
        if self.refused().contains(call) {
            return false;
        }
        // released when this ask ends, also by a panic of the dialog
        let _showing = match self.showing.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        let shown = Instant::now();
        let yes = self.dialog.allow(call) && shown.elapsed() >= self.min_answer;
        if !yes {
            self.refused().insert(call.clone());
        }
        yes
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;

    use super::*;

    /// Answers with `yes`, after `hold` returns (when set); counts the dialogs shown.
    #[derive(Default)]
    struct Dialog {
        yes: AtomicBool,
        shown: AtomicUsize,
        hold: Mutex<Option<mpsc::Receiver<()>>>,
    }

    impl ProgramConsent for Dialog {
        fn allow(&self, _call: &ProgramCall) -> bool {
            self.shown.fetch_add(1, Ordering::SeqCst);
            let hold = self.hold.lock().unwrap().take();
            if let Some(hold) = hold {
                let _ = hold.recv();
            }
            self.yes.load(Ordering::SeqCst)
        }
    }

    fn call(program: &str) -> ProgramCall {
        ProgramCall {
            program: PathBuf::from(program),
            args: vec!["{path}".to_owned()],
            env: Vec::new(),
        }
    }

    fn guarded(dialog: &Arc<Dialog>, min_answer: Duration) -> GuardedConsent {
        GuardedConsent::with_min_answer(Arc::clone(dialog) as _, min_answer)
    }

    #[test]
    fn an_ask_while_a_dialog_is_showing_is_a_no_without_a_second_dialog() {
        let dialog = Arc::new(Dialog::default());
        dialog.yes.store(true, Ordering::SeqCst);
        let (release, hold) = mpsc::channel();
        *dialog.hold.lock().unwrap() = Some(hold);
        let consent = Arc::new(guarded(&dialog, Duration::ZERO));
        let first = std::thread::spawn({
            let consent = Arc::clone(&consent);
            move || consent.allow(&call("/opt/a"))
        });
        while dialog.shown.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        assert!(!consent.allow(&call("/opt/b")));
        release.send(()).unwrap();
        assert!(first.join().unwrap());
        assert_eq!(dialog.shown.load(Ordering::SeqCst), 1);
        // the one turned away was never refused by the person: asked once the dialog is gone
        assert!(consent.allow(&call("/opt/b")));
        assert_eq!(dialog.shown.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_no_is_kept_for_the_run_and_the_same_call_is_never_asked_again() {
        let dialog = Arc::new(Dialog::default());
        let consent = guarded(&dialog, Duration::ZERO);
        assert!(!consent.allow(&call("/opt/a")));
        dialog.yes.store(true, Ordering::SeqCst);
        assert!(!consent.allow(&call("/opt/a")));
        assert_eq!(dialog.shown.load(Ordering::SeqCst), 1);
        // another call is its own question
        assert!(consent.allow(&call("/opt/b")));
        assert_eq!(dialog.shown.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_yes_faster_than_a_person_reads_is_a_no_and_kept_as_one() {
        let dialog = Arc::new(Dialog::default());
        dialog.yes.store(true, Ordering::SeqCst);
        let consent = guarded(&dialog, Duration::from_secs(3600));
        assert!(!consent.allow(&call("/opt/a")));
        assert!(!consent.allow(&call("/opt/a")));
        assert_eq!(dialog.shown.load(Ordering::SeqCst), 1);
        assert_eq!(GuardedConsent::new(dialog).min_answer, MIN_ANSWER);
    }
}
