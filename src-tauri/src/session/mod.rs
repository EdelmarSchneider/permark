//! Per-project state (session, last visit, window geometry), project settings, and the recent-folders list.
//!
//! State lives in AppData by default (`projects/<xxh3 of the canonical root>/`), so a project folder
//! stays untouched. When the project has a `.permark/` folder, state lives in `.permark/local/`
//! instead and settings are read from `.permark/config.toml`.

mod app_settings;
mod config;
mod legacy;
mod private;
mod recent;
mod store;
mod window;

use std::io;
use std::path::Path;

use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serialize};

pub use app_settings::{
    effective_language, installer_language, AppSettingsStore, Language, StartIn, Tone,
};
pub use config::{Config, TreeConfig, TreeShow};
pub(crate) use legacy::state_dir;
pub(crate) use private::{
    open_lock_file, prepare_local_folder, remove_local_state, write_local_cache, write_local_state,
    write_prepared_cache,
};
pub use recent::{RecentStore, RecentVault};
pub(crate) use store::project_key;
pub use store::{FileStamp, LastVisit, StateLocation, StateStore};
pub use window::{saved_for_launch, WindowState};

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
// The filter's switches are independent on/off choices that the UI stores as they are.
#[allow(clippy::struct_excessive_bools)]
pub struct TreeFilterState {
    /// A preset this build does not know (written by a newer UI) reads as the default.
    #[serde(deserialize_with = "or_default")]
    pub show: TreeShow,
    /// The categories shown by `Custom`.
    pub categories: Vec<String>,
    /// The advanced field of `Custom`: `+pattern` and `-pattern` entries.
    pub patterns: String,
    pub hide_empty_folders: bool,
    /// Images, data and XML files show only in folders that hold documents (docs-and-assets and custom).
    pub assets_near_docs: bool,
    /// Also list the files that version control ignores. Off unless the person turned it on; a value this build
    /// cannot read reads as off.
    #[serde(deserialize_with = "or_default")]
    pub show_ignored: bool,
    /// Also list hidden files and folders (names that start with a dot). Off unless the person turned it on.
    #[serde(deserialize_with = "or_default")]
    pub show_hidden: bool,
}

impl Default for TreeFilterState {
    fn default() -> Self {
        Self {
            show: TreeShow::default(),
            categories: Vec::new(),
            patterns: String::new(),
            hide_empty_folders: true,
            assets_near_docs: true,
            show_ignored: false,
            show_hidden: false,
        }
    }
}

/// Reads a `T`, or its default when the saved value is not one this build understands (a setting added
/// or reshaped by a newer UI): a session must never be lost to a setting it cannot read.
fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Lenient<T> {
        Known(T),
        Unknown(IgnoredAny),
    }
    Ok(match Lenient::<T>::deserialize(deserializer)? {
        Lenient::Known(value) => value,
        Lenient::Unknown(_) => T::default(),
    })
}

/// Like `or_default`, but a value this build cannot read is `true` (settings that are on by default).
fn or_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Lenient {
        Known(bool),
        Unknown(IgnoredAny),
    }
    Ok(match Lenient::deserialize(deserializer)? {
        Lenient::Known(value) => value,
        Lenient::Unknown(_) => true,
    })
}

/// How wide the reading column is. The UI draws it; the app only keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReadingWidth {
    #[default]
    Comfortable,
    Wide,
    Full,
}

/// Which view the right panel shows. The UI draws it; the app only keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RightTab {
    #[default]
    Changes,
    Outline,
    Comments,
}

/// What happens to the links of documents when files are moved or renamed in the Explorer. The UI
/// acts on it; the app only keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkUpdate {
    /// Show what would change and ask.
    #[default]
    Ask,
    Always,
    Never,
}

/// How a document opens for editing. The UI draws it; the app only keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditMode {
    #[default]
    Visual,
    Source,
}

/// What the UI wants restored on the next visit. Matches the UI's `SessionState`; fields missing from
/// an older save take their defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
// The settings are independent on/off choices that the UI stores as they are.
#[allow(clippy::struct_excessive_bools)]
pub struct SessionState {
    pub last_doc: Option<String>,
    pub scroll_top: f64,
    pub zoom: f64,
    /// `None` until a width is chosen (a save from before v0.2 only has `wide`); a width this build
    /// does not know (written by a newer UI) reads as `None` too, and the UI picks its default.
    #[serde(
        deserialize_with = "or_default",
        skip_serializing_if = "Option::is_none"
    )]
    pub width: Option<ReadingWidth>,
    /// The old on/off width choice. Kept only so the UI can migrate it to `width`; it is never written back.
    #[serde(
        deserialize_with = "or_default",
        skip_serializing_if = "Option::is_none"
    )]
    pub wide: Option<bool>,
    pub ref_template: String,
    pub left_panel: PanelState,
    pub right_panel: PanelState,
    pub theme: Theme,
    pub follow: bool,
    /// `None` until the person picks a filter: the project's default (or the app's) applies. A filter
    /// this build cannot read at all reads as `None`.
    #[serde(deserialize_with = "or_default")]
    pub tree_filter: Option<TreeFilterState>,
    /// The Explorer selects (and scrolls to) the document that was just opened. On unless turned off.
    #[serde(deserialize_with = "or_true")]
    pub auto_reveal: bool,
    /// The right panel's view. A view this build does not know (written by a newer UI) reads as Changes.
    #[serde(deserialize_with = "or_default")]
    pub right_tab: RightTab,
    /// Whether moving files rewrites the links to them. A value this build does not know reads as Ask.
    #[serde(deserialize_with = "or_default")]
    pub link_update: LinkUpdate,
    /// How a document opens for editing. A mode this build does not know reads as Visual.
    #[serde(deserialize_with = "or_default")]
    pub edit_mode: EditMode,
    /// The person chose "Never for this folder" on the hint to keep settings with the project. Kept here, in
    /// the app's own storage, so nothing about it is written into the project.
    #[serde(deserialize_with = "or_default")]
    pub init_hint_never: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            last_doc: None,
            scroll_top: 0.0,
            zoom: 1.0,
            width: None,
            wide: None,
            ref_template: config::DEFAULT_REF_TEMPLATE.to_owned(),
            left_panel: PanelState::default(),
            right_panel: PanelState {
                open: true,
                width: 280.0,
            },
            theme: Theme::System,
            follow: false,
            tree_filter: None,
            auto_reveal: true,
            right_tab: RightTab::Changes,
            link_update: LinkUpdate::Ask,
            edit_mode: EditMode::Visual,
            init_hint_never: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_round_trips_the_ui_shape() {
        let json = serde_json::json!({
            "lastDoc": "docs/a.md", "scrollTop": 120.5, "zoom": 1.25, "width": "full",
            "refTemplate": "@{path}", "leftPanel": {"open": false, "width": 300.5},
            "rightPanel": {"open": true, "width": 320.5}, "theme": "dark", "follow": true,
            "autoReveal": false, "rightTab": "outline", "linkUpdate": "always",
            "editMode": "source", "initHintNever": true,
            "treeFilter": {"show": "custom", "categories": ["documents", "data"],
                           "patterns": "+*.sql -archive/**", "hideEmptyFolders": false,
                           "assetsNearDocs": false, "showIgnored": true, "showHidden": true}
        });
        let s: SessionState = serde_json::from_value(json.clone()).expect("parse");
        assert_eq!(s.last_doc.as_deref(), Some("docs/a.md"));
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.width, Some(ReadingWidth::Full));
        assert!(!s.auto_reveal);
        assert_eq!(s.right_tab, RightTab::Outline);
        assert_eq!(s.link_update, LinkUpdate::Always);
        assert_eq!(s.edit_mode, EditMode::Source);
        assert!(!s.left_panel.open);
        let filter = s.tree_filter.as_ref().expect("filter");
        assert_eq!(filter.show, TreeShow::Custom);
        assert!(!filter.hide_empty_folders);
        assert!(!filter.assets_near_docs);
        assert!(filter.show_ignored && filter.show_hidden);
        assert!(s.init_hint_never);
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
        assert!(f.assets_near_docs, "on unless the UI turned it off");
        assert_eq!(f.categories, Vec::<String>::new());
    }

    #[test]
    fn hidden_and_ignored_files_are_off_unless_the_person_turned_them_on() {
        // a filter saved before the options existed
        let s: SessionState = serde_json::from_str(
            "{\"treeFilter\": {\"show\": \"everything\", \"patterns\": \"+x\"}}",
        )
        .expect("parse");
        let f = s.tree_filter.expect("filter");
        assert!(!f.show_ignored && !f.show_hidden);
        assert_eq!(f.patterns, "+x");
        assert!(!TreeFilterState::default().show_ignored);
        assert!(!TreeFilterState::default().show_hidden);
        // a value of another type reads as off and keeps the rest of the filter
        for value in ["null", "\"yes\"", "1", "[]", "{}"] {
            let json = format!(
                "{{\"treeFilter\": {{\"show\": \"custom\", \"patterns\": \"+x\", \
                 \"showIgnored\": {value}, \"showHidden\": {value}}}}}"
            );
            let s: SessionState = serde_json::from_str(&json).expect("parse");
            let f = s
                .tree_filter
                .unwrap_or_else(|| panic!("{value} lost the filter"));
            assert!(!f.show_ignored && !f.show_hidden, "{value}");
            assert_eq!(f.patterns, "+x");
        }
        // each one on its own survives a save and a load
        for (ignored, hidden) in [(true, false), (false, true), (true, true)] {
            let filter = TreeFilterState {
                show_ignored: ignored,
                show_hidden: hidden,
                ..TreeFilterState::default()
            };
            let saved = serde_json::to_string(&filter).expect("json");
            let back: TreeFilterState = serde_json::from_str(&saved).expect("parse");
            assert_eq!(back, filter);
        }
    }

    #[test]
    fn the_init_hint_is_shown_unless_the_person_chose_never() {
        assert!(!SessionState::default().init_hint_never);
        let s: SessionState = serde_json::from_str("{\"zoom\": 2}").expect("parse");
        assert!(
            !s.init_hint_never,
            "a session saved before the hint existed"
        );
        let s: SessionState = serde_json::from_str("{\"initHintNever\": true}").expect("parse");
        assert!(s.init_hint_never);
        let s: SessionState =
            serde_json::from_str("{\"follow\": true, \"initHintNever\": \"yes\"}").expect("parse");
        assert!(
            !s.init_hint_never && s.follow,
            "unreadable reads as not chosen"
        );
    }

    #[test]
    fn a_preset_from_a_newer_ui_reads_as_the_default_and_the_session_survives() {
        let s: SessionState = serde_json::from_str(
            "{\"zoom\": 2, \"lastDoc\": \"a.md\", \"treeFilter\": {\"show\": \"by-tag\", \
             \"categories\": [\"data\"], \"patterns\": \"+x\", \"hideEmptyFolders\": false}}",
        )
        .expect("an unknown preset must not fail the whole session");
        assert!((s.zoom - 2.0).abs() < f64::EPSILON);
        assert_eq!(s.last_doc.as_deref(), Some("a.md"));
        let f = s.tree_filter.expect("filter");
        assert_eq!(f.show, TreeShow::default());
        // the rest of the filter is kept
        assert_eq!(f.categories, ["data"]);
        assert_eq!(f.patterns, "+x");
        assert!(!f.hide_empty_folders);
        // a preset of the wrong type reads the same way
        for show in ["null", "7", "[\"custom\"]", "{}"] {
            let json = format!("{{\"treeFilter\": {{\"show\": {show}}}}}");
            let s: SessionState = serde_json::from_str(&json).expect("parse");
            assert_eq!(s.tree_filter.expect("filter").show, TreeShow::default());
        }
    }

    #[test]
    fn a_filter_this_build_cannot_read_is_dropped_without_losing_the_session() {
        for filter in [
            "\"documents\"",
            "42",
            "[1, 2]",
            "{\"hideEmptyFolders\": \"yes\"}",
            "{\"categories\": 5}",
        ] {
            let json = format!("{{\"zoom\": 1.5, \"follow\": true, \"treeFilter\": {filter}}}");
            let s: SessionState = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{filter} lost the session: {e}"));
            assert!((s.zoom - 1.5).abs() < f64::EPSILON);
            assert!(s.follow);
            assert_eq!(s.tree_filter, None, "{filter}");
        }
    }

    #[test]
    fn a_session_saved_before_reading_widths_keeps_its_wide_flag_for_the_ui_to_migrate() {
        let s: SessionState = serde_json::from_str("{\"zoom\": 2, \"wide\": true}").expect("parse");
        assert_eq!(s.wide, Some(true));
        assert_eq!(s.width, None, "no width was chosen yet");
        let s: SessionState = serde_json::from_str("{\"wide\": false}").expect("parse");
        assert_eq!(s.wide, Some(false));
        // nothing chosen at all: both stay unset, and a save writes neither
        let s: SessionState = serde_json::from_str("{}").expect("parse");
        assert_eq!((s.width, s.wide), (None, None));
        let json = serde_json::to_value(&s).expect("json");
        assert!(
            json.get("width").is_none() && json.get("wide").is_none(),
            "{json}"
        );
    }

    #[test]
    fn every_reading_width_round_trips() {
        for (name, width) in [
            ("comfortable", ReadingWidth::Comfortable),
            ("wide", ReadingWidth::Wide),
            ("full", ReadingWidth::Full),
        ] {
            let json = format!("{{\"width\": \"{name}\"}}");
            let s: SessionState = serde_json::from_str(&json).expect("parse");
            assert_eq!(s.width, Some(width));
            assert_eq!(serde_json::to_value(&s).expect("json")["width"], name);
        }
    }

    #[test]
    fn a_width_from_a_newer_ui_reads_as_unset_and_the_session_survives() {
        for width in ["\"ultrawide\"", "null", "7", "true", "[\"full\"]", "{}"] {
            let json = format!(
                "{{\"zoom\": 1.5, \"lastDoc\": \"a.md\", \"width\": {width}, \"wide\": true}}"
            );
            let s: SessionState = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{width} lost the session: {e}"));
            assert!((s.zoom - 1.5).abs() < f64::EPSILON);
            assert_eq!(s.last_doc.as_deref(), Some("a.md"));
            assert_eq!(s.width, None, "{width}");
            assert_eq!(s.wide, Some(true), "the old flag is still there to migrate");
        }
        // an old flag of the wrong type is dropped too
        let s: SessionState =
            serde_json::from_str("{\"zoom\": 2, \"wide\": \"yes\"}").expect("parse");
        assert_eq!(s.wide, None);
    }

    #[test]
    fn auto_reveal_is_on_unless_the_person_turned_it_off() {
        let s: SessionState = serde_json::from_str("{\"zoom\": 2}").expect("parse");
        assert!(s.auto_reveal, "missing means the default");
        assert!(SessionState::default().auto_reveal);
        let s: SessionState = serde_json::from_str("{\"autoReveal\": false}").expect("parse");
        assert!(!s.auto_reveal);
        let s: SessionState = serde_json::from_str("{\"autoReveal\": true}").expect("parse");
        assert!(s.auto_reveal);
        // a value of another type reads as the default and keeps the session
        for value in ["null", "\"no\"", "0", "[]", "{}"] {
            let json = format!("{{\"follow\": true, \"autoReveal\": {value}}}");
            let s: SessionState = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{value} lost the session: {e}"));
            assert!(s.auto_reveal, "{value}");
            assert!(s.follow);
        }
    }

    #[test]
    fn a_session_saved_before_the_outline_has_the_changes_view() {
        let s: SessionState = serde_json::from_str("{\"zoom\": 2}").expect("parse");
        assert_eq!(s.right_tab, RightTab::Changes, "missing means the default");
        assert_eq!(SessionState::default().right_tab, RightTab::Changes);
        let s: SessionState = serde_json::from_str("{}").expect("parse");
        assert_eq!(s, SessionState::default());
    }

    #[test]
    fn every_right_tab_round_trips() {
        for (name, tab) in [
            ("changes", RightTab::Changes),
            ("outline", RightTab::Outline),
            ("comments", RightTab::Comments),
        ] {
            let json = format!("{{\"rightTab\": \"{name}\"}}");
            let s: SessionState = serde_json::from_str(&json).expect("parse");
            assert_eq!(s.right_tab, tab);
            assert_eq!(serde_json::to_value(&s).expect("json")["rightTab"], name);
        }
    }

    #[test]
    fn a_view_from_a_newer_ui_reads_as_changes_and_the_session_survives() {
        for tab in [
            "\"commits\"",
            "\"Outline\"",
            "\"\"",
            "null",
            "7",
            "true",
            "[\"outline\"]",
            "{}",
        ] {
            let json = format!("{{\"zoom\": 1.5, \"lastDoc\": \"a.md\", \"rightTab\": {tab}}}");
            let s: SessionState = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{tab} lost the session: {e}"));
            assert!((s.zoom - 1.5).abs() < f64::EPSILON);
            assert_eq!(s.last_doc.as_deref(), Some("a.md"));
            assert_eq!(s.right_tab, RightTab::Changes, "{tab}");
        }
    }

    #[test]
    fn links_are_asked_about_unless_the_person_chose_otherwise() {
        let s: SessionState = serde_json::from_str("{\"zoom\": 2}").expect("parse");
        assert_eq!(s.link_update, LinkUpdate::Ask, "missing means the default");
        for (name, expected) in [
            ("ask", LinkUpdate::Ask),
            ("always", LinkUpdate::Always),
            ("never", LinkUpdate::Never),
        ] {
            let json = format!("{{\"linkUpdate\": \"{name}\"}}");
            let s: SessionState = serde_json::from_str(&json).expect("parse");
            assert_eq!(s.link_update, expected);
            assert_eq!(serde_json::to_value(&s).expect("json")["linkUpdate"], name);
        }
        // a value from a newer UI reads as the default and keeps the session
        for value in ["\"sometimes\"", "null", "7", "true", "[]"] {
            let json = format!("{{\"follow\": true, \"linkUpdate\": {value}}}");
            let s: SessionState = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{value} lost the session: {e}"));
            assert!(s.follow);
            assert_eq!(s.link_update, LinkUpdate::Ask, "{value}");
        }
    }

    #[test]
    fn documents_open_in_the_visual_editor_unless_the_person_chose_source() {
        let s: SessionState = serde_json::from_str("{\"zoom\": 2}").expect("parse");
        assert_eq!(s.edit_mode, EditMode::Visual, "missing means the default");
        for (name, mode) in [("visual", EditMode::Visual), ("source", EditMode::Source)] {
            let json = format!("{{\"editMode\": \"{name}\"}}");
            let s: SessionState = serde_json::from_str(&json).expect("parse");
            assert_eq!(s.edit_mode, mode);
            assert_eq!(serde_json::to_value(&s).expect("json")["editMode"], name);
        }
        // a mode from a newer UI reads as Visual and the session survives
        for mode in ["\"split\"", "\"Source\"", "null", "7", "{}"] {
            let json = format!("{{\"zoom\": 1.5, \"editMode\": {mode}}}");
            let s: SessionState = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{mode} lost the session: {e}"));
            assert!((s.zoom - 1.5).abs() < f64::EPSILON);
            assert_eq!(s.edit_mode, EditMode::Visual, "{mode}");
        }
    }

    /// The agent connection's switch is not part of the session: a session file lives in the project's folder, which
    /// a repository can fill, and the switch is kept in the app's own data (see `StateStore::agent_connection`).
    #[test]
    fn a_session_file_has_no_say_in_the_agent_connection() {
        let s: SessionState =
            serde_json::from_str("{\"follow\": true, \"agentConnection\": true}").expect("parse");
        assert!(s.follow, "the rest of the session is kept");
        assert!(serde_json::to_value(&s)
            .expect("json")
            .get("agentConnection")
            .is_none());
    }

    #[test]
    fn null_last_doc_is_allowed() {
        let s: SessionState = serde_json::from_str("{\"lastDoc\": null}").expect("parse");
        assert_eq!(s.last_doc, None);
    }
}
