//! `.permark/config.toml`: project settings shared with the team. Read only when `.permark/` is a real folder.

use std::collections::{BTreeMap, BTreeSet};

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use toml_edit::{Array, DocumentMut, Item, Table, Value};
use ts_rs::TS;

use super::{edit_file, plugin_tables, read_doc, set_keeping_decor, RawTable, SettingsError};
use crate::paths::Root;
use crate::plugins::PluginId;
use crate::state::PERMARK_DIR;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TreeShow {
    Documents,
    DocsAndAssets,
    Everything,
    Custom,
}

impl TreeShow {
    /// The word the file keeps.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Documents => "documents",
            Self::DocsAndAssets => "docs-and-assets",
            Self::Everything => "everything",
            Self::Custom => "custom",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "documents" => Some(Self::Documents),
            "docs-and-assets" => Some(Self::DocsAndAssets),
            "everything" => Some(Self::Everything),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// `[tree]`: the project's default Explorer filter (each person's own choice wins).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, TS)]
#[ts(export)]
pub struct TreeConfig {
    pub show: Option<TreeShow>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectConfig {
    pub tree: TreeConfig,
    /// Extra gitignore-style patterns hidden from feed + search.
    pub ignore: Vec<String>,
    /// `[agent] deny`: permission groups this project never allows (narrowing only).
    pub agent_deny: BTreeSet<String>,
    /// `[agent] deny_paths`: gitignore-style patterns of paths agents never get (narrowing only).
    pub agent_deny_paths: Vec<String>,
    /// `[plugins.<id>] enabled = false` (a project may only disable).
    pub plugins_off: BTreeSet<PluginId>,
    pub plugin_tables: BTreeMap<PluginId, RawTable>,
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

impl ProjectConfig {
    /// Reads `<root>/.permark/config.toml`; defaults when `.permark/` is no real folder | file missing | invalid.
    #[must_use]
    pub fn load(root: &Root) -> Self {
        let permark = root.path().join(PERMARK_DIR);
        let real = std::fs::symlink_metadata(&permark).is_ok_and(|m| m.file_type().is_dir());
        if !real {
            return Self::default();
        }
        read_doc(&permark.join("config.toml"))
            .map_or_else(Self::default, |doc| Self::from_doc(&doc))
    }

    /// Lenient: each key on its own; wrong types read as defaults.
    #[must_use]
    pub fn from_doc(doc: &DocumentMut) -> Self {
        let tree = doc.get("tree").and_then(Item::as_table_like);
        let tree = TreeConfig {
            show: tree
                .and_then(|t| t.get("show"))
                .and_then(Item::as_str)
                .and_then(TreeShow::parse),
            include: strings(tree.and_then(|t| t.get("include"))),
            exclude: strings(tree.and_then(|t| t.get("exclude"))),
        };
        let agent = doc.get("agent").and_then(Item::as_table_like);
        let plugin_tables = plugin_tables(doc);
        let plugins_off = plugin_tables
            .iter()
            .filter(|(_, table)| table.0.get("enabled").and_then(Item::as_bool) == Some(false))
            .map(|(id, _)| id.clone())
            .collect();
        Self {
            tree,
            ignore: strings(doc.get("ignore")),
            agent_deny: strings(agent.and_then(|t| t.get("deny")))
                .into_iter()
                .collect(),
            agent_deny_paths: strings(agent.and_then(|t| t.get("deny_paths"))),
            plugins_off,
            plugin_tables,
        }
    }
}

// ---- writing -----------------------------------------------------------------------------------------------------

/// Most entries of a list setting; longest entry (characters); longest group name.
const MAX_ENTRIES: usize = 200;
const MAX_ENTRY: usize = 300;
const MAX_GROUP: usize = 32;

/// What the settings page shows of the project: whether `.permark/` exists, and the shared settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectSettings {
    pub initialized: bool,
    pub tree: TreeConfig,
    pub ignore: Vec<String>,
    pub agent_deny: Vec<String>,
    pub agent_deny_paths: Vec<String>,
}

impl ProjectSettings {
    #[must_use]
    pub fn of(initialized: bool, config: &ProjectConfig) -> Self {
        Self {
            initialized,
            tree: config.tree.clone(),
            ignore: config.ignore.clone(),
            agent_deny: config.agent_deny.iter().cloned().collect(),
            agent_deny_paths: config.agent_deny_paths.clone(),
        }
    }
}

/// One change of one key of `.permark/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "key", content = "value", rename_all = "camelCase")]
#[ts(export)]
pub enum ProjectSettingChange {
    /// `None` = back to the default (key removed).
    TreeShow(Option<TreeShow>),
    TreeInclude(Vec<String>),
    TreeExclude(Vec<String>),
    Ignore(Vec<String>),
    /// Permission groups this project never allows.
    AgentDeny(Vec<String>),
    AgentDenyPaths(Vec<String>),
}

impl ProjectSettingChange {
    /// Key path as written in the file.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::TreeShow(_) => "tree.show",
            Self::TreeInclude(_) => "tree.include",
            Self::TreeExclude(_) => "tree.exclude",
            Self::Ignore(_) => "ignore",
            Self::AgentDeny(_) => "agent.deny",
            Self::AgentDenyPaths(_) => "agent.deny_paths",
        }
    }

    fn list(&self) -> Option<&[String]> {
        match self {
            Self::TreeShow(_) => None,
            Self::TreeInclude(v)
            | Self::TreeExclude(v)
            | Self::Ignore(v)
            | Self::AgentDeny(v)
            | Self::AgentDenyPaths(v) => Some(v),
        }
    }

    /// Refuses lists and entries that must never reach the file.
    ///
    /// # Errors
    /// `Invalid`: `tooMany` (> 200 entries), `empty` | `tooLong` (entry outside 1..=300 characters) | `control`
    /// (a control character) | `group` (an agent group name that is not `name` or `name.name`).
    pub fn validate(&self) -> Result<(), SettingsError> {
        let invalid = |reason: &str| SettingsError::Invalid {
            key: self.key().to_owned(),
            reason: reason.to_owned(),
        };
        let Some(entries) = self.list() else {
            return Ok(());
        };
        if entries.len() > MAX_ENTRIES {
            return Err(invalid("tooMany"));
        }
        for entry in entries {
            let entry = entry.trim();
            if entry.is_empty() {
                return Err(invalid("empty"));
            }
            if entry.chars().count() > MAX_ENTRY {
                return Err(invalid("tooLong"));
            }
            if entry.chars().any(char::is_control) {
                return Err(invalid("control"));
            }
            if matches!(self, Self::AgentDeny(_)) && !valid_group(entry) {
                return Err(invalid("group"));
            }
        }
        Ok(())
    }
}

/// `name` or `name.name`, lowercase letters, <= 32 characters (a permission group).
fn valid_group(name: &str) -> bool {
    name.len() <= MAX_GROUP
        && name.split('.').count() <= 2
        && name
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase()))
}

/// The list as a TOML array (entries trimmed, duplicates dropped); `None` for an empty list (the key goes).
fn strings_value(entries: &[String]) -> Option<Value> {
    let mut seen = std::collections::BTreeSet::new();
    let mut array = Array::new();
    for entry in entries {
        let entry = entry.trim();
        if seen.insert(entry.to_owned()) {
            array.push(entry);
        }
    }
    (!array.is_empty()).then(|| Value::Array(array))
}

/// Sets | removes `key` (in `[table]` when given); a table left empty goes with its last key.
fn set_key(root: &mut Table, table: Option<&str>, key: &str, value: Option<Value>) {
    let Some(name) = table else {
        match value {
            Some(value) => set_keeping_decor(root, key, value),
            None => {
                root.remove(key);
            }
        }
        return;
    };
    match value {
        Some(value) => {
            if !root.get(name).is_some_and(Item::is_table) {
                root.insert(name, Item::Table(Table::new()));
            }
            if let Some(Item::Table(table)) = root.get_mut(name) {
                set_keeping_decor(table, key, value);
            }
        }
        None => {
            if let Some(Item::Table(table)) = root.get_mut(name) {
                table.remove(key);
                if table.is_empty() {
                    root.remove(name);
                }
            }
        }
    }
}

/// Applies a change to a document: only the bytes of that key change.
pub fn apply_project_change(doc: &mut DocumentMut, change: &ProjectSettingChange) {
    let root = doc.as_table_mut();
    match change {
        ProjectSettingChange::TreeShow(v) => {
            set_key(
                root,
                Some("tree"),
                "show",
                v.map(|v| Value::from(v.as_str())),
            );
        }
        ProjectSettingChange::TreeInclude(v) => {
            set_key(root, Some("tree"), "include", strings_value(v));
        }
        ProjectSettingChange::TreeExclude(v) => {
            set_key(root, Some("tree"), "exclude", strings_value(v));
        }
        ProjectSettingChange::Ignore(v) => set_key(root, None, "ignore", strings_value(v)),
        ProjectSettingChange::AgentDeny(v) => {
            set_key(root, Some("agent"), "deny", strings_value(v));
        }
        ProjectSettingChange::AgentDenyPaths(v) => {
            set_key(root, Some("agent"), "deny_paths", strings_value(v));
        }
    }
}

/// `<root>/.permark/config.toml` as a file to write.
#[derive(Debug, Clone)]
pub struct ProjectConfigFile {
    permark: PathBuf,
    path: PathBuf,
}

impl ProjectConfigFile {
    #[must_use]
    pub fn of(root: &Root) -> Self {
        let permark = root.path().join(PERMARK_DIR);
        Self {
            path: permark.join("config.toml"),
            permark,
        }
    }

    /// `.permark/` is a real folder (a link or a file does not count).
    #[must_use]
    pub fn initialized(&self) -> bool {
        std::fs::symlink_metadata(&self.permark).is_ok_and(|m| m.file_type().is_dir())
    }

    /// Applies one change to the file on disk (read fresh: hand edits kept) and returns the config now.
    ///
    /// # Errors
    /// `NotInitialized` (no real `.permark/`); `Invalid` value; the file is a link; IO failure.
    pub fn apply(&self, change: &ProjectSettingChange) -> Result<ProjectConfig, SettingsError> {
        change.validate()?;
        if !self.initialized() {
            return Err(SettingsError::NotInitialized);
        }
        if std::fs::symlink_metadata(&self.path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(SettingsError::Invalid {
                key: change.key().to_owned(),
                reason: "link".to_owned(),
            });
        }
        let doc = edit_file(&self.path, |doc| apply_project_change(doc, change))?;
        Ok(ProjectConfig::from_doc(&doc))
    }
}
