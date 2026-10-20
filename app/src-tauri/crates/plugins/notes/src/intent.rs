//! What the person asked the Notes tab to show next (a note, a new note), kept until the Notes tab takes it. The asker
//! may live in another view than the Notes tab (the Welcome of another window, a project tab): the wish waits here, in
//! one slot (newest wins), and every view hears that one came; the Notes tab takes it once.

use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A wish for the Notes tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum NotesIntent {
    /// A new empty note, ready for typing.
    New,
    /// Show this note (its name in the folder | a loose file's path).
    Open { path: String },
}

/// The slot + how a new wish is announced (the app plugin event `notes:intent`).
pub struct Intents {
    slot: Mutex<Option<NotesIntent>>,
    announce: Box<dyn Fn() + Send + Sync>,
}

impl Intents {
    #[must_use]
    pub fn new(announce: Box<dyn Fn() + Send + Sync>) -> Self {
        Self {
            slot: Mutex::new(None),
            announce,
        }
    }

    /// Keeps `intent` (an older one not taken yet is replaced) and tells every view.
    pub fn want(&self, intent: NotesIntent) {
        *self.slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(intent);
        (self.announce)();
    }

    /// The wish, once: the slot is empty afterwards.
    #[must_use]
    pub fn take(&self) -> Option<NotesIntent> {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;

    fn counted() -> (Intents, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        let intents = Intents::new(Box::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
        }));
        (intents, count)
    }

    #[test]
    fn a_wish_is_announced_and_taken_once() {
        let (intents, announced) = counted();
        assert_eq!(intents.take(), None, "nothing asked yet");
        intents.want(NotesIntent::Open {
            path: "2026-10-20 0930.md".to_owned(),
        });
        assert_eq!(announced.load(Ordering::SeqCst), 1);
        assert_eq!(
            intents.take(),
            Some(NotesIntent::Open {
                path: "2026-10-20 0930.md".to_owned()
            })
        );
        assert_eq!(intents.take(), None, "taken once");
    }

    #[test]
    fn the_newest_wish_wins() {
        let (intents, announced) = counted();
        intents.want(NotesIntent::New);
        intents.want(NotesIntent::Open {
            path: "a.md".to_owned(),
        });
        assert_eq!(announced.load(Ordering::SeqCst), 2);
        assert_eq!(
            intents.take(),
            Some(NotesIntent::Open {
                path: "a.md".to_owned()
            })
        );
    }

    #[test]
    fn the_wire_shape_is_a_tagged_object() {
        let new: NotesIntent =
            serde_json::from_value(serde_json::json!({ "kind": "new" })).unwrap();
        assert_eq!(new, NotesIntent::New);
        let open: NotesIntent =
            serde_json::from_value(serde_json::json!({ "kind": "open", "path": "x.md" })).unwrap();
        assert_eq!(
            serde_json::to_value(&open).unwrap(),
            serde_json::json!({ "kind": "open", "path": "x.md" })
        );
        for bad in [
            serde_json::json!({ "kind": "focus" }),
            serde_json::json!({ "kind": "open" }),
            serde_json::json!("new"),
            serde_json::json!(null),
        ] {
            assert!(serde_json::from_value::<NotesIntent>(bad).is_err());
        }
    }

    #[test]
    fn many_askers_and_takers_at_once_take_each_wish_at_most_once() {
        let (intents, _) = counted();
        let intents = Arc::new(intents);
        let taken = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let intents = Arc::clone(&intents);
                let taken = Arc::clone(&taken);
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        if i % 2 == 0 {
                            intents.want(NotesIntent::New);
                        } else if intents.take().is_some() {
                            taken.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let left = usize::from(intents.take().is_some());
        assert!(
            taken.load(Ordering::SeqCst) + left <= 800,
            "never more takes than wishes"
        );
        assert_eq!(intents.take(), None);
    }
}
