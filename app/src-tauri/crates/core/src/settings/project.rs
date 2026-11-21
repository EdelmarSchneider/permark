//! `.permark/config.toml`: project settings shared with the team. Read only when `.permark/` is a real folder.

use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use toml_edit::{Array, DocumentMut, Item, Table, Value};
use ts_rs::TS;

use super::{
    edit_file, plugin_tables, read_team_text, set_keeping_decor, set_plugin_key, tokenizers_of,
    tokenizers_value, valid_tokenizers, SettingsError,
};
use crate::fsx;
use crate::paths::Root;
use crate::plugins::PluginId;
use crate::state::{Sharing, PERMARK_DIR};

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
    /// `agent` is no table, or its `deny` | `deny_paths` no list of texts: the lenient read drops such a value, so the
    /// agent rules read takes the file as unreadable rather than as setting nothing.
    pub agent_bad_value: bool,
    /// `[plugins.token-count] tokenizers`: the tokenizers this project counts with, first = primary (replaces the
    /// app's list when it has an entry).
    pub tokenizers: Vec<String>,
    /// `[plugins.<id>] enabled = false` (a project may only disable).
    pub plugins_off: BTreeSet<PluginId>,
    /// `<id>.<key>` of every other plugin key in the file, sorted: a plugin reads its settings from the person's own
    /// app settings only, so a shared file never changes them for everyone.
    pub ignored_plugin_keys: Vec<String>,
}

/// The one plugin key besides `enabled` a project file is read for.
const TOKENIZERS: (&str, &str) = ("token-count", "tokenizers");

/// Whether `item` is a list of texts only (absent counts: nothing set).
fn only_strings(item: Option<&Item>) -> bool {
    item.is_none_or(|item| {
        item.as_array()
            .is_some_and(|array| array.iter().all(|v| v.as_str().is_some()))
    })
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

/// Why `.permark/config.toml` is there but gives no settings ([`ProjectConfig::load_checked`]).
#[derive(Debug)]
pub enum ConfigUnreadable {
    /// Not read: larger than settings are, not UTF-8, held by another program, a link | folder, denied; `.permark/`
    /// that cannot be looked at.
    Read(SettingsError),
    /// `.permark/` is a link: never followed, so the team file it may lead to stays unknown.
    LinkedFolder,
    /// Not TOML (a merge conflict left in it).
    NotToml,
}

/// `.permark/config.toml` read checked ([`ProjectConfig::load_checked`]).
#[derive(Debug)]
pub enum ConfigRead {
    /// No `.permark/` (or a plain file of that name), or no file in it.
    Absent,
    /// Its text and what it says.
    Read(String, ProjectConfig),
    Unreadable(ConfigUnreadable),
}

impl ProjectConfig {
    /// Reads `<root>/.permark/config.toml`; defaults when `.permark/` is no real folder | file missing | invalid.
    #[must_use]
    pub fn load(root: &Root) -> Self {
        match Self::load_checked(root) {
            ConfigRead::Read(_, config) => config,
            ConfigRead::Absent | ConfigRead::Unreadable(_) => Self::default(),
        }
    }

    /// [`ProjectConfig::load`] for what a broken file must never turn into "nothing set" (the agent rules): a file
    /// that is there but cannot be read is told apart from a missing one; so is a `.permark` that is a link or cannot
    /// be looked at (it may hold one).
    #[must_use]
    pub fn load_checked(root: &Root) -> ConfigRead {
        let permark = root.path().join(PERMARK_DIR);
        match std::fs::symlink_metadata(&permark) {
            Ok(meta) if fsx::is_link(&meta) => {
                return ConfigRead::Unreadable(ConfigUnreadable::LinkedFolder)
            }
            Ok(meta) if meta.is_dir() => {}
            // a plain file holds no team file
            Ok(_) => return ConfigRead::Absent,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return ConfigRead::Absent,
            Err(source) => {
                return ConfigRead::Unreadable(ConfigUnreadable::Read(SettingsError::Io {
                    path: permark.display().to_string(),
                    source,
                }))
            }
        }
        let text = match read_team_text(&permark, "config.toml") {
            Ok(Some(text)) => text,
            Ok(None) => return ConfigRead::Absent,
            Err(error) => return ConfigRead::Unreadable(ConfigUnreadable::Read(error)),
        };
        match Self::from_text(&text) {
            Some(config) => ConfigRead::Read(text, config),
            None => ConfigRead::Unreadable(ConfigUnreadable::NotToml),
        }
    }

    /// What a config text says; `None`: not TOML.
    #[must_use]
    pub fn from_text(text: &str) -> Option<Self> {
        text.parse::<DocumentMut>()
            .ok()
            .map(|doc| Self::from_doc(&doc))
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
        let agent_item = doc.get("agent").filter(|item| !item.is_none());
        let agent = agent_item.and_then(Item::as_table_like);
        let agent_bad_value = (agent_item.is_some() && agent.is_none())
            || !only_strings(agent.and_then(|t| t.get("deny")))
            || !only_strings(agent.and_then(|t| t.get("deny_paths")));
        let plugin_tables = plugin_tables(doc);
        let plugins_off = plugin_tables
            .iter()
            .filter(|(_, table)| table.0.get("enabled").and_then(Item::as_bool) == Some(false))
            .map(|(id, _)| id.clone())
            .collect();
        let mut ignored_plugin_keys: Vec<String> = plugin_tables
            .iter()
            .flat_map(|(id, table)| {
                table
                    .0
                    .iter()
                    // `enabled` is read only as true | false: any other value turns nothing off
                    .filter(|(key, value)| {
                        if *key == "enabled" {
                            value.as_bool().is_none()
                        } else {
                            (id.as_str(), *key) != TOKENIZERS
                        }
                    })
                    .map(move |(key, _)| format!("{id}.{key}"))
            })
            .collect();
        ignored_plugin_keys.sort();
        Self {
            tree,
            ignore: strings(doc.get("ignore")),
            agent_deny: strings(agent.and_then(|t| t.get("deny")))
                .into_iter()
                .collect(),
            agent_deny_paths: strings(agent.and_then(|t| t.get("deny_paths"))),
            agent_bad_value,
            tokenizers: tokenizers_of(
                PluginId::parse(TOKENIZERS.0)
                    .and_then(|id| plugin_tables.get(&id))
                    .map(|table| &table.0),
            ),
            plugins_off,
            ignored_plugin_keys,
        }
    }
}

// ---- writing -----------------------------------------------------------------------------------------------------

/// Most entries of a list setting; longest entry (characters); longest group name.
const MAX_ENTRIES: usize = 200;
const MAX_ENTRY: usize = 300;
const MAX_GROUP: usize = 32;

/// What the settings page shows of the project: whether `.permark/` exists, what git may see of it, and the shared
/// settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectSettings {
    pub initialized: bool,
    /// As PerMark's own `.permark/.gitignore` says ([`crate::state::sharing`]).
    pub sharing: Sharing,
    pub tree: TreeConfig,
    pub ignore: Vec<String>,
    pub agent_deny: Vec<String>,
    pub agent_deny_paths: Vec<String>,
    pub tokenizers: Vec<String>,
    /// Plugin keys of the file no plugin reads (`<id>.<key>`, sorted).
    pub ignored_plugin_keys: Vec<String>,
}

impl ProjectSettings {
    #[must_use]
    pub fn of(initialized: bool, sharing: Sharing, config: &ProjectConfig) -> Self {
        Self {
            initialized,
            sharing,
            tree: config.tree.clone(),
            ignore: config.ignore.clone(),
            agent_deny: config.agent_deny.iter().cloned().collect(),
            agent_deny_paths: config.agent_deny_paths.clone(),
            tokenizers: config.tokenizers.clone(),
            ignored_plugin_keys: config.ignored_plugin_keys.clone(),
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
    /// The token count's tokenizers for this project (empty = the app's choice; key removed).
    Tokenizers(Vec<String>),
    /// A plugin off for everyone in this project (`enabled = false`); `off: false` removes the key (a project can only
    /// turn a plugin off, never on).
    PluginOff {
        plugin: PluginId,
        off: bool,
    },
}

impl ProjectSettingChange {
    /// Key path as written in the file.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::TreeShow(_) => "tree.show".to_owned(),
            Self::TreeInclude(_) => "tree.include".to_owned(),
            Self::TreeExclude(_) => "tree.exclude".to_owned(),
            Self::Ignore(_) => "ignore".to_owned(),
            Self::AgentDeny(_) => "agent.deny".to_owned(),
            Self::AgentDenyPaths(_) => "agent.deny_paths".to_owned(),
            Self::Tokenizers(_) => format!("plugins.{}.{}", TOKENIZERS.0, TOKENIZERS.1),
            Self::PluginOff { plugin, .. } => format!("plugins.{plugin}.enabled"),
        }
    }

    fn list(&self) -> Option<&[String]> {
        match self {
            Self::TreeShow(_) | Self::Tokenizers(_) | Self::PluginOff { .. } => None,
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
            key: self.key(),
            reason: reason.to_owned(),
        };
        if let Self::Tokenizers(ids) = self {
            return if valid_tokenizers(ids) {
                Ok(())
            } else {
                Err(invalid("tokenizers"))
            };
        }
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

/// Bytes of the file's header: its first whole lines, while blank or a comment, up to the last comment (blank lines
/// after it belong to what follows: they go with it).
fn header_len(text: &str) -> usize {
    let mut len = 0;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !line.ends_with('\n') || !(trimmed.is_empty() || trimmed.starts_with('#')) {
            break;
        }
        at += line.len();
        if !trimmed.is_empty() {
            len = at;
        }
    }
    len
}

/// Applies a change to a document: only the bytes of that key change. The header (comments above the first key |
/// table) stays first: TOML keeps it as the trailing text of a file of comments only (the one "Initialize PerMark"
/// writes), which would put a new table above it, or as the prefix of the first table, which would go with it.
pub fn apply_project_change(doc: &mut DocumentMut, change: &ProjectSettingChange) {
    let text = doc.to_string();
    let (head, body) = text.split_at(header_len(&text));
    if !head.is_empty() {
        if let Ok(mut rest) = body.parse::<DocumentMut>() {
            change_keys(&mut rest, change);
            rest.as_table_mut().decor_mut().set_prefix(head);
            *doc = rest;
            return;
        }
    }
    change_keys(doc, change);
}

fn change_keys(doc: &mut DocumentMut, change: &ProjectSettingChange) {
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
        ProjectSettingChange::Tokenizers(ids) => {
            set_plugin_key(
                root,
                &PluginId::of(TOKENIZERS.0),
                TOKENIZERS.1,
                tokenizers_value(ids),
            );
        }
        ProjectSettingChange::PluginOff { plugin, off } => {
            set_plugin_key(root, plugin, "enabled", off.then(|| Value::from(false)));
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
                key: change.key(),
                reason: "link".to_owned(),
            });
        }
        let doc = edit_file(&self.path, |doc| {
            apply_project_change(doc, change);
            Ok(())
        })?;
        Ok(ProjectConfig::from_doc(&doc))
    }
}
