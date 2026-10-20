//! Settings: app (`<profile>/settings.toml`) + project (`.permark/config.toml`). Read leniently (bad value of a key ->
//! that key's default); written surgically via `toml_edit` (only the changed key's bytes; comments, order, unknown
//! keys survive).

mod installer;
mod project;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, Value};
use ts_rs::TS;

pub use installer::installer_language;
pub use project::{
    ProjectConfig, ProjectConfigFile, ProjectSettingChange, ProjectSettings, TreeConfig, TreeShow,
};

use crate::error::IpcError;
use crate::fsx::write_atomic;
use crate::plugins::PluginId;
use crate::state::AppDirs;

/// Settings files larger than this are not settings (refused, defaults used).
pub(crate) const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;

macro_rules! choice {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? } default $default:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[ts(export)]
        pub enum $name {
            $(
                #[serde(rename = $text)]
                $variant,
            )+
        }

        impl Default for $name {
            fn default() -> Self {
                Self::$default
            }
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            #[must_use]
            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

choice!(
    /// Interface language: the system's, or one of the six.
    LanguagePref { System = "system", En = "en", PtBr = "pt-BR", Es = "es", De = "de", ZhCn = "zh-CN", It = "it" } default System
);
choice!(ThemePref { System = "system", Light = "light", Dark = "dark" } default System);
choice!(
    /// Tone of the light theme.
    Tone { White = "white", Paper = "paper", Sage = "sage", Gray = "gray" } default White
);
choice!(
    /// What a start with nothing to open shows.
    StartIn { Welcome = "welcome", Notes = "notes" } default Welcome
);
choice!(
    /// Whether moving or renaming a file rewrites the links that point at it.
    LinkUpdate { Ask = "ask", Always = "always", Never = "never" } default Ask
);
choice!(
    /// The editor Ctrl+E opens for Markdown.
    EditModePref { Edit = "edit", Source = "source" } default Edit
);

choice!(
    /// Where a folder or file opened from the app goes: a new tab in this window, the active tab, a new window.
    OpenIn { Tab = "tab", ThisTab = "this", NewWindow = "window" } default Tab
);

impl OpenIn {
    /// Read side: 0.6.0 values map on (`ask` -> `tab`, `new` -> `window`); anything else -> the default. The file
    /// keeps its old value until the person changes the setting.
    #[must_use]
    pub fn read(text: &str) -> Self {
        match text {
            "ask" => Self::Tab,
            "new" => Self::NewWindow,
            other => Self::parse(other).unwrap_or_default(),
        }
    }
}

/// Command ids 0.6.0 used, and their names now (`[keymap]` of 0.6.0 keeps working).
pub const RENAMED_COMMANDS: &[(&str, &str)] = &[
    ("app.closeWorkspace", "tab.close"),
    ("workspace.next", "tab.next"),
    ("workspace.previous", "tab.previous"),
];
choice!(
    /// Which git the git plugin uses: the one installed (`System`), the built-in one, or the first that works.
    GitBackendPref { Automatic = "automatic", System = "system", Builtin = "builtin" } default Automatic
);

/// Reference template when none is set.
pub const DEFAULT_REFERENCE_TEMPLATE: &str = "{path}#{section}";
/// Longest reference template kept.
pub const MAX_REFERENCE_TEMPLATE: usize = 200;

/// Longest command id | combo kept in the keyboard map; most entries read.
pub const MAX_COMBO: usize = 40;
pub const MAX_COMMAND_ID: usize = 64;
pub const MAX_KEYMAP: usize = 200;

/// A command id: `[A-Za-z0-9._:-]`, 1..=64.
#[must_use]
pub fn valid_command_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_COMMAND_ID
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

/// A combo as the keyboard map stores it: printable ASCII without spaces, <= 40 (the grammar is the frontend's;
/// empty = the command is unbound).
#[must_use]
pub fn valid_combo(combo: &str) -> bool {
    combo.len() <= MAX_COMBO && combo.chars().all(|c| c.is_ascii_graphic())
}

/// A plugin's raw settings table (equality by its TOML text).
#[derive(Debug, Clone, Default)]
pub struct RawTable(pub Table);

impl PartialEq for RawTable {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_string() == other.0.to_string()
    }
}

/// App settings as the app uses them.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppSettings {
    pub language: LanguagePref,
    pub theme: ThemePref,
    pub tone: Tone,
    pub start_in: StartIn,
    pub link_update: LinkUpdate,
    pub edit_mode: EditModePref,
    /// What opening something over a shown workspace does.
    pub open_in: OpenIn,
    /// Which git the git plugin uses (`[plugins.git] backend`).
    pub git_backend: GitBackendPref,
    /// Where the notes are (`[plugins.notes] folder`; empty = the default folder).
    pub notes_folder: String,
    /// How "copy reference" is written (`{path} {section} {line} {name}`).
    pub reference_template: String,
    /// The person's editor: a command template (empty = not set). Only the person sets it (app settings).
    pub editor: String,
    /// The person's keyboard map: command id -> combo (empty = unbound). Absent = the command's defaults.
    pub keymap: BTreeMap<String, String>,
    /// Tokenizers the token count shows, first = primary (`[plugins.token-count] tokenizers`; at most 3).
    pub tokenizers: Vec<String>,
    /// Explicit plugin switches (absent = manifest default).
    pub plugins: BTreeMap<PluginId, bool>,
    /// Raw `[plugins.<id>]` tables: each plugin owns its schema.
    #[serde(skip)]
    #[ts(skip)]
    pub plugin_tables: BTreeMap<PluginId, RawTable>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: LanguagePref::default(),
            theme: ThemePref::default(),
            tone: Tone::default(),
            start_in: StartIn::default(),
            link_update: LinkUpdate::default(),
            edit_mode: EditModePref::default(),
            open_in: OpenIn::default(),
            git_backend: GitBackendPref::default(),
            notes_folder: String::new(),
            reference_template: DEFAULT_REFERENCE_TEMPLATE.to_owned(),
            editor: String::new(),
            keymap: BTreeMap::new(),
            tokenizers: Vec::new(),
            plugins: BTreeMap::new(),
            plugin_tables: BTreeMap::new(),
        }
    }
}

/// One change of one key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "key", content = "value", rename_all = "camelCase")]
#[ts(export)]
pub enum SettingChange {
    Language(LanguagePref),
    Theme(ThemePref),
    Tone(Tone),
    StartIn(StartIn),
    LinkUpdate(LinkUpdate),
    EditMode(EditModePref),
    OpenIn(OpenIn),
    GitBackend(GitBackendPref),
    /// An absolute folder; empty = the default folder.
    NotesFolder(String),
    /// Empty = the default template.
    ReferenceTemplate(String),
    /// Empty = not set.
    Editor(String),
    /// One command's combo; `None` = back to its defaults (key removed); `Some("")` = unbound.
    Keymap {
        command: String,
        combo: Option<String>,
    },
    /// Tokenizer ids of the token count, first = primary; empty = none chosen (key removed).
    Tokenizers(Vec<String>),
    /// `None` = back to the manifest default (key removed).
    PluginEnabled {
        plugin: PluginId,
        enabled: Option<bool>,
    },
}

impl SettingChange {
    /// Key path as written in the file (for `SettingsChanged.keys`).
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::Language(_) => "language".to_owned(),
            Self::Theme(_) => "theme".to_owned(),
            Self::Tone(_) => "tone".to_owned(),
            Self::StartIn(_) => "start_in".to_owned(),
            Self::LinkUpdate(_) => "link_update".to_owned(),
            Self::EditMode(_) => "edit_mode".to_owned(),
            Self::OpenIn(_) => "open_in".to_owned(),
            Self::GitBackend(_) => "plugins.git.backend".to_owned(),
            Self::NotesFolder(_) => "plugins.notes.folder".to_owned(),
            Self::ReferenceTemplate(_) => "reference_template".to_owned(),
            Self::Editor(_) => "editor".to_owned(),
            Self::Keymap { command, .. } => format!("keymap.{command}"),
            Self::Tokenizers(_) => "plugins.token-count.tokenizers".to_owned(),
            Self::PluginEnabled { plugin, .. } => format!("plugins.{plugin}.enabled"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "scope", content = "project", rename_all = "camelCase")]
#[ts(export)]
pub enum SettingsScope {
    App,
    Project(crate::state::ProjectId),
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("cannot write {path}: {source}")]
    Io { path: String, source: io::Error },
    /// The value is not acceptable (`reason`: a word the UI can word).
    #[error("{key} is not valid: {reason}")]
    Invalid { key: String, reason: String },
    /// A project setting was asked for in a folder PerMark is not initialized in.
    #[error("PerMark is not initialized in this folder")]
    NotInitialized,
}

impl From<SettingsError> for IpcError {
    fn from(error: SettingsError) -> Self {
        match error {
            SettingsError::Io { path, source } => IpcError::io(path, &source),
            SettingsError::Invalid { key, reason } => {
                IpcError::new(crate::error::ErrorKind::InvalidData)
                    .subject(key)
                    .detail(reason)
            }
            SettingsError::NotInitialized => {
                IpcError::new(crate::error::ErrorKind::Unavailable).subject("permark")
            }
        }
    }
}

impl SettingChange {
    /// Refuses values that must never reach the file.
    ///
    /// # Errors
    /// `Invalid`: reference template too long; editor command that is not a valid template.
    pub fn validate(&self) -> Result<(), SettingsError> {
        let invalid = |reason: &str| SettingsError::Invalid {
            key: self.key(),
            reason: reason.to_owned(),
        };
        match self {
            Self::ReferenceTemplate(text) if text.chars().count() > MAX_REFERENCE_TEMPLATE => {
                Err(invalid("tooLong"))
            }
            Self::NotesFolder(text) if !valid_notes_folder(text) => Err(invalid("folder")),
            Self::Editor(text) => crate::document::editor::EditorTemplate::parse(text)
                .map(|_| ())
                .map_err(|problem| invalid(problem.code())),
            Self::Tokenizers(ids) if !valid_tokenizers(ids) => Err(invalid("tokenizers")),
            Self::Keymap { command, combo } => {
                if !valid_command_id(command) {
                    Err(invalid("command"))
                } else if combo.as_deref().is_some_and(|c| !valid_combo(c)) {
                    Err(invalid("combo"))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

/// Most tokenizers shown at once; longest id.
pub const MAX_TOKENIZERS: usize = 3;
const MAX_TOKENIZER_ID: usize = 16;

/// A tokenizer id as a setting keeps it: lowercase letters and digits.
fn valid_tokenizer_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_TOKENIZER_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// At most [`MAX_TOKENIZERS`] ids, each valid, none twice.
#[must_use]
pub fn valid_tokenizers(ids: &[String]) -> bool {
    ids.len() <= MAX_TOKENIZERS
        && ids.iter().all(|id| valid_tokenizer_id(id))
        && ids
            .iter()
            .enumerate()
            .all(|(at, id)| !ids[..at].contains(id))
}

/// The ids of a `tokenizers` array, leniently: valid ones, first occurrence, the first [`MAX_TOKENIZERS`].
pub(crate) fn tokenizers_of(table: Option<&Table>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(array) = table
        .and_then(|t| t.get("tokenizers"))
        .and_then(Item::as_array)
    else {
        return out;
    };
    for id in array.iter().filter_map(Value::as_str) {
        if valid_tokenizer_id(id) && !out.iter().any(|seen| seen == id) {
            out.push(id.to_owned());
        }
        if out.len() == MAX_TOKENIZERS {
            break;
        }
    }
    out
}

/// A TOML array of the ids; `None` for an empty list (the key goes).
pub(crate) fn tokenizers_value(ids: &[String]) -> Option<Value> {
    (!ids.is_empty()).then(|| Value::Array(ids.iter().map(String::as_str).collect()))
}

/// The longest notes folder path accepted.
const MAX_NOTES_FOLDER: usize = 1024;

/// A notes folder as the setting keeps it: empty (the default) or an absolute path of reasonable length.
fn valid_notes_folder(text: &str) -> bool {
    text.is_empty()
        || (text.chars().count() <= MAX_NOTES_FOLDER
            && !text.contains('\0')
            && std::path::Path::new(text).is_absolute())
}

/// Reads a settings file as a document; missing | too large | unreadable | not TOML -> `None`.
pub(crate) fn read_doc(path: &Path) -> Option<DocumentMut> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_SETTINGS_BYTES {
        return None;
    }
    std::fs::read_to_string(path)
        .ok()?
        .parse::<DocumentMut>()
        .ok()
}

fn str_of<'a>(doc: &'a DocumentMut, key: &str) -> Option<&'a str> {
    doc.get(key).and_then(Item::as_str)
}

/// `[plugins.<id>]` tables of a document (invalid ids skipped).
pub(crate) fn plugin_tables(doc: &DocumentMut) -> BTreeMap<PluginId, RawTable> {
    let Some(plugins) = doc.get("plugins").and_then(Item::as_table_like) else {
        return BTreeMap::new();
    };
    plugins
        .iter()
        .filter_map(|(id, item)| {
            let id = PluginId::parse(id)?;
            let table = match item {
                Item::Table(table) => table.clone(),
                Item::Value(Value::InlineTable(inline)) => inline.clone().into_table(),
                _ => return None,
            };
            Some((id, RawTable(table)))
        })
        .collect()
}

/// `[keymap]` of a document: entries with a valid command id and combo (at most [`MAX_KEYMAP`]).
fn keymap_of(doc: &DocumentMut) -> BTreeMap<String, String> {
    let Some(table) = doc.get("keymap").and_then(Item::as_table_like) else {
        return BTreeMap::new();
    };
    let mut map: BTreeMap<String, String> = table
        .iter()
        .filter_map(|(command, item)| {
            let combo = item.as_str()?;
            (valid_command_id(command) && valid_combo(combo))
                .then(|| (command.to_owned(), combo.to_owned()))
        })
        .take(MAX_KEYMAP)
        .collect();
    // a 0.6.0 id reads as its new name (the new one, when also there, wins)
    for (old, new) in RENAMED_COMMANDS {
        if let Some(combo) = map.remove(*old) {
            map.entry((*new).to_owned()).or_insert(combo);
        }
    }
    map
}

impl AppSettings {
    /// Lenient read of a document: each key on its own.
    #[must_use]
    pub fn from_doc(doc: &DocumentMut) -> Self {
        let keymap = keymap_of(doc);
        let plugin_tables = plugin_tables(doc);
        let plugins = plugin_tables
            .iter()
            .filter_map(|(id, table)| Some((id.clone(), table.0.get("enabled")?.as_bool()?)))
            .collect();
        Self {
            language: str_of(doc, "language")
                .and_then(LanguagePref::parse)
                .unwrap_or_default(),
            theme: str_of(doc, "theme")
                .and_then(ThemePref::parse)
                .unwrap_or_default(),
            tone: str_of(doc, "tone")
                .and_then(Tone::parse)
                .unwrap_or_default(),
            start_in: str_of(doc, "start_in")
                .and_then(StartIn::parse)
                .unwrap_or_default(),
            link_update: str_of(doc, "link_update")
                .and_then(LinkUpdate::parse)
                .unwrap_or_default(),
            edit_mode: str_of(doc, "edit_mode")
                .and_then(EditModePref::parse)
                .unwrap_or_default(),
            open_in: str_of(doc, "open_in").map(OpenIn::read).unwrap_or_default(),
            git_backend: PluginId::parse("git")
                .and_then(|git| plugin_tables.get(&git))
                .and_then(|table| table.0.get("backend")?.as_str())
                .and_then(GitBackendPref::parse)
                .unwrap_or_default(),
            notes_folder: PluginId::parse("notes")
                .and_then(|notes| plugin_tables.get(&notes))
                .and_then(|table| table.0.get("folder")?.as_str())
                .filter(|folder| !folder.is_empty() && valid_notes_folder(folder))
                .map(str::to_owned)
                .unwrap_or_default(),
            tokenizers: tokenizers_of(
                PluginId::parse("token-count")
                    .and_then(|id| plugin_tables.get(&id))
                    .map(|table| &table.0),
            ),
            reference_template: str_of(doc, "reference_template")
                .filter(|t| !t.trim().is_empty() && t.chars().count() <= MAX_REFERENCE_TEMPLATE)
                .map_or_else(|| DEFAULT_REFERENCE_TEMPLATE.to_owned(), str::to_owned),
            // a hand-edited value that is not a valid template counts as not set
            editor: str_of(doc, "editor")
                .filter(|t| crate::document::editor::EditorTemplate::parse(t).is_ok())
                .map(str::to_owned)
                .unwrap_or_default(),
            keymap,
            plugins,
            plugin_tables,
        }
    }
}

/// Sets `table[key] = value`, keeping the old value's decor (inline comment, spacing) and the key's position.
fn set_keeping_decor(table: &mut Table, key: &str, value: Value) {
    match table.get_mut(key) {
        Some(Item::Value(old)) => {
            let decor = old.decor().clone();
            *old = value;
            *old.decor_mut() = decor;
        }
        Some(other) => *other = Item::Value(value),
        None => {
            table.insert(key, Item::Value(value));
        }
    }
}

/// Applies a change to a document: only the bytes of that key change.
pub fn apply_change(doc: &mut DocumentMut, change: &SettingChange) {
    let root = doc.as_table_mut();
    match change {
        SettingChange::Language(v) => set_keeping_decor(root, "language", v.as_str().into()),
        SettingChange::Theme(v) => set_keeping_decor(root, "theme", v.as_str().into()),
        SettingChange::Tone(v) => set_keeping_decor(root, "tone", v.as_str().into()),
        SettingChange::StartIn(v) => set_keeping_decor(root, "start_in", v.as_str().into()),
        SettingChange::LinkUpdate(v) => set_keeping_decor(root, "link_update", v.as_str().into()),
        SettingChange::EditMode(v) => set_keeping_decor(root, "edit_mode", v.as_str().into()),
        SettingChange::OpenIn(v) => set_keeping_decor(root, "open_in", v.as_str().into()),
        SettingChange::GitBackend(v) => {
            set_plugin_key(
                root,
                &PluginId::of("git"),
                "backend",
                (*v != GitBackendPref::Automatic).then(|| v.as_str().into()),
            );
        }
        SettingChange::NotesFolder(v) => {
            set_plugin_key(
                root,
                &PluginId::of("notes"),
                "folder",
                (!v.trim().is_empty()).then(|| v.trim().into()),
            );
        }
        SettingChange::ReferenceTemplate(v) => set_text(root, "reference_template", v),
        SettingChange::Editor(v) => set_text(root, "editor", v),
        SettingChange::Keymap { command, combo } => set_keymap(root, command, combo.as_deref()),
        SettingChange::Tokenizers(ids) => {
            set_plugin_key(
                root,
                &PluginId::of("token-count"),
                "tokenizers",
                tokenizers_value(ids),
            );
        }
        SettingChange::PluginEnabled { plugin, enabled } => {
            set_plugin_key(root, plugin, "enabled", enabled.map(Value::from));
        }
    }
}

fn set_keymap(root: &mut Table, command: &str, combo: Option<&str>) {
    if !root.get("keymap").is_some_and(Item::is_table) {
        if combo.is_none() {
            return;
        }
        root.insert("keymap", Item::Table(Table::new()));
    }
    let Some(Item::Table(map)) = root.get_mut("keymap") else {
        return;
    };
    // the entry under its 0.6.0 name goes when the person changes it
    for (old, new) in RENAMED_COMMANDS {
        if *new == command {
            map.remove(old);
        }
    }
    if let Some(combo) = combo {
        set_keeping_decor(map, command, combo.into());
    } else {
        map.remove(command);
        if map.is_empty() {
            root.remove("keymap");
        }
    }
}

/// A text key: empty text removes the key (back to the default).
fn set_text(table: &mut Table, key: &str, value: &str) {
    if value.trim().is_empty() {
        table.remove(key);
    } else {
        set_keeping_decor(table, key, value.into());
    }
}

/// Sets | removes `[plugins.<id>] <key>`; an emptied table goes with its last key.
fn set_plugin_key(root: &mut Table, plugin: &PluginId, key: &str, value: Option<Value>) {
    let id = plugin.as_str();
    if !root.get("plugins").is_some_and(Item::is_table) {
        if value.is_none() {
            return;
        }
        let mut plugins = Table::new();
        plugins.set_implicit(true);
        root.insert("plugins", Item::Table(plugins));
    }
    let Some(Item::Table(plugins)) = root.get_mut("plugins") else {
        return;
    };
    // an inline table `[plugins] git = { enabled = true }` becomes a normal table (rare; still surgical elsewhere)
    if let Some(Item::Value(Value::InlineTable(inline))) = plugins.get(id) {
        let table = inline.clone().into_table();
        plugins.insert(id, Item::Table(table));
    }
    match value {
        Some(value) => {
            if !plugins.get(id).is_some_and(Item::is_table) {
                plugins.insert(id, Item::Table(Table::new()));
            }
            if let Some(Item::Table(table)) = plugins.get_mut(id) {
                set_keeping_decor(table, key, value);
            }
        }
        None => {
            if let Some(Item::Table(table)) = plugins.get_mut(id) {
                table.remove(key);
                if table.is_empty() {
                    plugins.remove(id);
                }
            }
        }
    }
}

/// Document text in the line endings of `original` (`toml_edit` writes LF): an all-CRLF file stays CRLF.
#[must_use]
pub fn render(doc: &DocumentMut, original: &str) -> String {
    let text = doc.to_string();
    let crlf = original.contains("\r\n") && !original.replace("\r\n", "").contains('\n');
    if crlf {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text
    }
}

/// The app settings file.
#[derive(Debug, Clone)]
pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    #[must_use]
    pub fn app(dirs: &AppDirs) -> Self {
        Self {
            path: dirs.profile.join("settings.toml"),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Current settings (defaults when missing | unreadable).
    #[must_use]
    pub fn load(&self) -> AppSettings {
        read_doc(&self.path).map_or_else(AppSettings::default, |doc| AppSettings::from_doc(&doc))
    }

    /// Applies one change to the file on disk (read fresh: hand edits kept) and returns the new settings.
    /// A file that is not TOML is moved to `settings.toml.broken-<ms>` first (never silently overwritten).
    ///
    /// # Errors
    /// `Invalid` value; IO failure writing the file.
    pub fn apply(&self, change: &SettingChange) -> Result<AppSettings, SettingsError> {
        change.validate()?;
        let doc = edit_file(&self.path, |doc| apply_change(doc, change))?;
        Ok(AppSettings::from_doc(&doc))
    }

    /// First start: the language the installer recorded becomes the `language` setting, once. Nothing happens when
    /// the file already has a `language` key (the person's own pick, `system` included, always wins: a reinstall in
    /// another language never replaces it) or when nothing was recorded. Returns the settings when it wrote.
    ///
    /// # Errors
    /// IO failure writing the file.
    pub fn adopt_language(
        &self,
        recorded: Option<LanguagePref>,
    ) -> Result<Option<AppSettings>, SettingsError> {
        let Some(language) = recorded else {
            return Ok(None);
        };
        if read_doc(&self.path).is_some_and(|doc| doc.contains_key("language")) {
            return Ok(None);
        }
        self.apply(&SettingChange::Language(language)).map(Some)
    }

    /// First start after 0.5: the choices 0.5 kept in its `settings.json` (`older`, in its own profile folder) become
    /// settings, once: only while this version has no settings file. Taken: the notes folder, the editor command and
    /// the language; a value this version does not accept is skipped. 0.5's file is only read. Returns the settings
    /// when it wrote.
    ///
    /// # Errors
    /// IO failure writing the file.
    pub fn adopt_older(&self, older: &Path) -> Result<Option<AppSettings>, SettingsError> {
        if std::fs::symlink_metadata(&self.path).is_ok() {
            return Ok(None);
        }
        let Ok(bytes) = crate::fsx::read_bytes(older, 1024 * 1024) else {
            return Ok(None);
        };
        let Ok(serde_json::Value::Object(old)) = serde_json::from_slice(&bytes) else {
            return Ok(None);
        };
        let text = |key: &str| {
            old.get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let changes: Vec<SettingChange> = [
            text("notesFolder").map(SettingChange::NotesFolder),
            text("editorCommand").map(SettingChange::Editor),
            text("language")
                .and_then(|v| LanguagePref::parse(&v))
                .map(SettingChange::Language),
        ]
        .into_iter()
        .flatten()
        .filter(|change| change.validate().is_ok())
        .collect();
        if changes.is_empty() {
            return Ok(None);
        }
        let doc = edit_file(&self.path, |doc| {
            for change in &changes {
                apply_change(doc, change);
            }
        })?;
        Ok(Some(AppSettings::from_doc(&doc)))
    }
}

/// Reads a settings file fresh (hand edits kept; a file that is not TOML is moved to `<name>.broken-<ms>` first),
/// lets `change` edit the document, writes it back atomically (line endings of an all-CRLF file kept).
pub(crate) fn edit_file(
    path: &Path,
    change: impl FnOnce(&mut DocumentMut),
) -> Result<DocumentMut, SettingsError> {
    let io = |source: io::Error| SettingsError::Io {
        path: path.display().to_string(),
        source,
    };
    let original = std::fs::read_to_string(path).unwrap_or_default();
    let exists = std::fs::symlink_metadata(path).is_ok();
    let mut doc = if let Some(doc) = read_doc(path) {
        doc
    } else {
        if exists {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            let name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            std::fs::rename(path, path.with_file_name(format!("{name}.broken-{ms}")))
                .map_err(io)?;
        }
        DocumentMut::new()
    };
    change(&mut doc);
    write_atomic(path, render(&doc, &original).as_bytes()).map_err(io)?;
    Ok(doc)
}

#[cfg(test)]
mod tests;
