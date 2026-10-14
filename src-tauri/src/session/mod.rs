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

pub use config::{Config, TreeConfig, TreeShow};
pub use recent::{RecentStore, RecentVault};
pub use store::{FileStamp, LastVisit, StateLocation, StateStore};

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

/// The Explorer filter a person chose for this project. The UI interprets it; the app only keeps it, so
/// category names stay plain strings (a newer or older UI can save names this build does not know).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TreeFilterState {
    pub show: TreeShow,
    /// The categories shown by `Custom`.
    pub categories: Vec<String>,
    /// The advanced field of `Custom`: `+pattern` and `-pattern` entries.
    pub patterns: String,
    pub hide_empty_folders: bool,
}

impl Default for TreeFilterState {
    fn default() -> Self {
        Self {
            show: TreeShow::default(),
            categories: Vec::new(),
            patterns: String::new(),
            hide_empty_folders: true,
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
    /// `None` until the person picks a filter: the project's default (or the app's) applies.
    pub tree_filter: Option<TreeFilterState>,
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
            tree_filter: None,
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
            "rightPanel": {"open": true, "width": 320.5}, "theme": "dark", "follow": true,
            "treeFilter": {"show": "custom", "categories": ["documents", "data"],
                           "patterns": "+*.sql -archive/**", "hideEmptyFolders": false}
        });
        let s: SessionState = serde_json::from_value(json.clone()).expect("parse");
        assert_eq!(s.last_doc.as_deref(), Some("docs/a.md"));
        assert_eq!(s.theme, Theme::Dark);
        assert!(!s.left_panel.open);
        let filter = s.tree_filter.as_ref().expect("filter");
        assert_eq!(filter.show, TreeShow::Custom);
        assert!(!filter.hide_empty_folders);
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
    fn a_session_saved_before_the_filter_existed_has_none() {
        let s: SessionState = serde_json::from_str("{\"zoom\": 2}").expect("parse");
        assert_eq!(s.tree_filter, None);
        let s: SessionState = serde_json::from_str("{\"treeFilter\": null}").expect("parse");
        assert_eq!(s.tree_filter, None);
        // a partial filter takes the defaults for the rest
        let s: SessionState =
            serde_json::from_str("{\"treeFilter\": {\"show\": \"everything\"}}").expect("parse");
        let f = s.tree_filter.expect("filter");
        assert_eq!(f.show, TreeShow::Everything);
        assert!(f.hide_empty_folders);
        assert!(f.categories.is_empty());
    }

    #[test]
    fn null_last_doc_is_allowed() {
        let s: SessionState = serde_json::from_str("{\"lastDoc\": null}").expect("parse");
        assert_eq!(s.last_doc, None);
    }
}
