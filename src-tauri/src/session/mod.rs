//! Per-project state (session, last visit), project settings, and the recent-folders list.
//!
//! State lives in AppData by default (`projects/<xxh3 of the canonical root>/`), so a project folder
//! stays untouched. When the project has a `.trailmark/` folder, state lives in `.trailmark/local/`
//! instead and settings are read from `.trailmark/config.toml`.

mod config;
mod recent;
mod store;

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use config::Config;
pub use recent::{RecentStore, RecentVault};
pub use store::{LastVisit, StateLocation, StateStore};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("cannot access {path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("invalid state data: {0}")]
    Json(String),
}

impl SessionError {
    pub(crate) fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PanelState {
    pub open: bool,
    pub width: f64,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            open: true,
            width: 260.0,
        }
    }
}

/// What the UI wants restored on the next visit. Matches the UI's `SessionState`; fields missing from
/// an older save take their defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionState {
    pub last_doc: Option<String>,
    pub scroll_top: f64,
    pub zoom: f64,
    pub wide: bool,
    pub ref_template: String,
    pub left_panel: PanelState,
    pub right_panel: PanelState,
    pub theme: Theme,
    pub follow: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            last_doc: None,
            scroll_top: 0.0,
            zoom: 1.0,
            wide: false,
            ref_template: config::DEFAULT_REF_TEMPLATE.to_owned(),
            left_panel: PanelState::default(),
            right_panel: PanelState {
                open: true,
                width: 280.0,
            },
            theme: Theme::System,
            follow: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_round_trips_the_ui_shape() {
        let json = serde_json::json!({
            "lastDoc": "docs/a.md", "scrollTop": 120.5, "zoom": 1.25, "wide": true,
            "refTemplate": "@{path}", "leftPanel": {"open": false, "width": 300.5},
            "rightPanel": {"open": true, "width": 320.5}, "theme": "dark", "follow": true
        });
        let s: SessionState = serde_json::from_value(json.clone()).expect("parse");
        assert_eq!(s.last_doc.as_deref(), Some("docs/a.md"));
        assert_eq!(s.theme, Theme::Dark);
        assert!(!s.left_panel.open);
        assert_eq!(serde_json::to_value(&s).expect("json"), json);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s: SessionState = serde_json::from_str("{\"zoom\": 2}").expect("parse");
        assert!((s.zoom - 2.0).abs() < f64::EPSILON);
        assert_eq!(s.ref_template, "@{path}");
        assert!(s.left_panel.open);
        assert_eq!(s.theme, Theme::System);
        let s: SessionState = serde_json::from_str("{}").expect("parse");
        assert_eq!(s, SessionState::default());
    }

    #[test]
    fn null_last_doc_is_allowed() {
        let s: SessionState = serde_json::from_str("{\"lastDoc\": null}").expect("parse");
        assert_eq!(s.last_doc, None);
    }
}
