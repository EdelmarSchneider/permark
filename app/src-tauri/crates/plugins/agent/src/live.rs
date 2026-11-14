//! The windows' half of the connection inside the app: what the person is looking at (published by each window of
//! the project) and showing them a place (an event the window handles without taking focus). With the project open in
//! several windows, the one the person used last speaks for them: its context is answered, only it shows a place.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use permark_core::plugins::PluginEvents;
use serde_json::json;

use crate::context::{Mode, WindowContext};
use crate::tools::Place;

/// Windows by label (`RpcCall.window`), the one the person used last at the end of `order`.
#[derive(Default)]
struct Windows {
    contexts: BTreeMap<String, WindowContext>,
    order: Vec<String>,
    /// A focus was told: from now on only a focus moves a window to the end (a window publishing in the background
    /// never takes over).
    focus_seen: bool,
}

impl Windows {
    fn current(&self) -> Option<&String> {
        self.order.last()
    }
}

pub struct Live {
    windows: Mutex<Windows>,
    events: PluginEvents,
}

impl Live {
    #[must_use]
    pub fn new(events: PluginEvents) -> Self {
        Self {
            windows: Mutex::new(Windows::default()),
            events,
        }
    }

    /// Tests: a window half whose events go nowhere.
    #[cfg(test)]
    pub(crate) fn quiet() -> std::sync::Arc<Self> {
        let cx = permark_core::plugins::PluginCx::new(
            crate::ID,
            permark_core::plugins::CxWorkspace::App,
            std::path::PathBuf::new(),
            None,
            permark_core::conditions::Context::welcome(
                permark_core::conditions::Distribution::Direct,
            ),
            std::sync::Arc::new(permark_core::events::NoEvents),
        );
        std::sync::Arc::new(Self::new(cx.events().clone()))
    }

    fn lock(&self) -> MutexGuard<'_, Windows> {
        self.windows.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A window's latest word (sanitized when it is answered, never trusted as it is). Before any focus is told, the
    /// window that published last speaks; after, a window new to the list comes after the ones already used.
    pub fn publish(&self, window: &str, context: WindowContext) {
        let mut windows = self.lock();
        if !windows.order.iter().any(|w| w == window) {
            if windows.focus_seen {
                windows.order.insert(0, window.to_owned());
            } else {
                windows.order.push(window.to_owned());
            }
        } else if !windows.focus_seen {
            windows.order.retain(|w| w != window);
            windows.order.push(window.to_owned());
        }
        windows.contexts.insert(window.to_owned(), context);
    }

    /// The person turned to `window`: it speaks for them from now on.
    pub fn focused(&self, window: &str) {
        let mut windows = self.lock();
        windows.focus_seen = true;
        windows.order.retain(|w| w != window);
        windows.order.push(window.to_owned());
    }

    /// `window` let go of the project (tab closed, window gone): its word goes, the one used before it speaks again.
    pub fn forget(&self, window: &str) {
        let mut windows = self.lock();
        windows.order.retain(|w| w != window);
        windows.contexts.remove(window);
    }

    /// What the window the person used last shows (empty until it has published).
    #[must_use]
    pub fn context(&self) -> WindowContext {
        let windows = self.lock();
        windows
            .current()
            .and_then(|w| windows.contexts.get(w))
            .cloned()
            .unwrap_or_default()
    }

    /// Asks the window the person used last to show `place` (`window: null` while no window is known: every window
    /// of the project). A person who is editing is not moved: the window shows a notice with a button instead
    /// (`deferred`).
    #[must_use]
    pub fn show(&self, place: &Place) -> &'static str {
        let (target, editing) = {
            let windows = self.lock();
            let target = windows.current().cloned();
            let editing = target
                .as_ref()
                .and_then(|w| windows.contexts.get(w))
                .is_some_and(|c| c.mode != Mode::View);
            (target, editing)
        };
        self.events.emit(
            "open",
            json!({ "path": place.path, "line": place.line, "heading": place.heading, "window": target }),
        );
        if editing {
            "deferred"
        } else {
            "shown"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(doc: &str) -> WindowContext {
        WindowContext {
            doc: Some(doc.to_owned()),
            ..WindowContext::default()
        }
    }

    fn doc(live: &Live) -> Option<String> {
        live.context().doc
    }

    #[test]
    fn the_window_used_last_speaks_and_the_one_before_it_when_it_goes() {
        let live = Live::quiet();
        live.publish("a", at("a.md"));
        live.publish("b", at("b.md"));
        assert_eq!(
            doc(&live).as_deref(),
            Some("b.md"),
            "no focus yet: last publisher"
        );
        live.focused("a");
        live.focused("b");
        assert_eq!(doc(&live).as_deref(), Some("b.md"));
        live.focused("a");
        assert_eq!(doc(&live).as_deref(), Some("a.md"));
        // a window in the background publishing does not take over
        live.publish("b", at("b2.md"));
        assert_eq!(doc(&live).as_deref(), Some("a.md"));
        live.focused("b");
        assert_eq!(doc(&live).as_deref(), Some("b2.md"));
        live.forget("b");
        assert_eq!(doc(&live).as_deref(), Some("a.md"));
        assert!(!live.lock().contexts.contains_key("b"), "entry removed");
        live.forget("a");
        assert_eq!(live.context(), WindowContext::default());
    }

    #[test]
    fn a_window_new_after_a_focus_comes_after_the_ones_used() {
        let live = Live::quiet();
        live.publish("a", at("a.md"));
        live.focused("a");
        live.publish("c", at("c.md"));
        assert_eq!(doc(&live).as_deref(), Some("a.md"));
    }
}
