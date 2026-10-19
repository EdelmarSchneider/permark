//! The window's half of the connection inside the app: what the person is looking at (published by the window) and
//! showing them a place (an event the window handles without taking focus).

use std::sync::{Mutex, PoisonError};

use permark_core::plugins::PluginEvents;
use serde_json::json;

use crate::context::{Mode, WindowContext};
use crate::tools::Place;

pub struct Live {
    context: Mutex<WindowContext>,
    events: PluginEvents,
}

impl Live {
    #[must_use]
    pub fn new(events: PluginEvents) -> Self {
        Self {
            context: Mutex::new(WindowContext::default()),
            events,
        }
    }

    /// The window's latest word (sanitized when it is answered, never trusted as it is).
    pub fn publish(&self, context: WindowContext) {
        *self.context.lock().unwrap_or_else(PoisonError::into_inner) = context;
    }

    #[must_use]
    pub fn context(&self) -> WindowContext {
        self.context
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Asks the windows of the project to show `place`. A person who is editing is not moved: the window shows a
    /// notice with a button instead (`deferred`).
    #[must_use]
    pub fn show(&self, place: &Place) -> &'static str {
        let editing = self.context().mode != Mode::View;
        self.events.emit(
            "open",
            json!({ "path": place.path, "line": place.line, "heading": place.heading }),
        );
        if editing {
            "deferred"
        } else {
            "shown"
        }
    }
}
