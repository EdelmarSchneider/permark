//! Settings: app (`<profile>/settings.toml`) + project (`.permark/config.toml`). Read leniently (bad value of a key ->
//! that key's default); written surgically via `toml_edit` (only the changed key's bytes; comments, order, unknown
//! keys survive).

mod characters;
mod installer;
mod project;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, Value};
use ts_rs::TS;

pub use characters::{CharacterEntry, CharacterGroup, CharactersError};
pub use installer::installer_language;
pub use project::{
    ProjectConfig, ProjectConfigFile, ProjectSettingChange, ProjectSettings, TreeConfig, TreeShow,
};

use crate::error::IpcError;
use crate::fileops::{check_name, NameProblem};
use crate::fsx::{read_bytes, write_atomic, FsxError};
use crate::plugins::PluginId;
use crate::state::AppDirs;
use crate::tree::extension_of;

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
    /// Interface language: the system's, or one of the thirteen.
    LanguagePref {
        System = "system", En = "en", PtBr = "pt-BR", Es = "es", De = "de", ZhCn = "zh-CN", It = "it",
        Ja = "ja", Ko = "ko", Fr = "fr", PtPt = "pt-PT", Ca = "ca", Gl = "gl", Ro = "ro",
    } default System
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

/// The right column's arrangement (app-wide; `[right_views]`, `core.md` §settings): groups of view ids, each a
/// share of the column height. Shape rules (duplicates, unknown ids, normalizing) are the frontend's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RightViews {
    pub v: u32,
    pub groups: Vec<ViewGroup>,
}

/// One group of a region: one view, or several shown as tabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ViewGroup {
    pub views: Vec<String>,
    pub share: f64,
}

/// The arrangement of every region (app-wide; `[panels]`): per region, groups of view ids, each a share of the
/// region's extent. Shape rules (duplicates, unknown ids, normalizing) are the frontend's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Panels {
    pub v: u32,
    pub left: Vec<ViewGroup>,
    pub right: Vec<ViewGroup>,
    pub bottom: Vec<ViewGroup>,
}

/// The arrangements the person saved under a name (app-wide; `[layouts]`). Rules across the list (the same name
/// twice, case | normalization folding) are the frontend's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Layouts {
    pub v: u32,
    pub items: Vec<LayoutItem>,
}

/// One named arrangement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LayoutItem {
    pub name: String,
    pub arrangement: Panels,
}

/// Most groups | views per group a written arrangement may hold.
pub const MAX_VIEW_GROUPS: usize = 3;
pub const MAX_GROUP_VIEWS: usize = 16;
/// Most named layouts written; longest layout name, in characters.
pub const MAX_LAYOUTS: usize = 20;
pub const MAX_LAYOUT_NAME: usize = 40;
/// The format (`v`) of `[panels]` | `[layouts]` this build writes: the only one a written value may carry, and
/// what a removal is measured by.
pub const PANELS_VERSION: u32 = 2;
pub const LAYOUTS_VERSION: u32 = 1;

/// Write rule of one stack of groups (the right column | a region): <= 3 groups, <= 16 views each, view ids like
/// command ids, shares finite 0..=1.
fn valid_view_groups(groups: &[ViewGroup]) -> bool {
    groups.len() <= MAX_VIEW_GROUPS
        && groups.iter().all(|group| {
            group.views.len() <= MAX_GROUP_VIEWS
                && group.views.iter().all(|id| valid_command_id(id))
                && group.share.is_finite()
                && (0.0..=1.0).contains(&group.share)
        })
}

/// Write rule: <= 3 groups, <= 16 views each, view ids like command ids, shares finite 0..=1.
#[must_use]
pub fn valid_right_views(views: &RightViews) -> bool {
    valid_view_groups(&views.groups)
}

/// Write rule: the format this build writes ([`PANELS_VERSION`]: any other would be stored without being
/// understood here, and a higher one would shut every later write out, see `set_versioned_table`); every region
/// within the bounds of the right column ([`valid_right_views`]).
#[must_use]
pub fn valid_panels(panels: &Panels) -> bool {
    panels.v == PANELS_VERSION
        && [&panels.left, &panels.right, &panels.bottom]
            .into_iter()
            .all(|region| valid_view_groups(region))
}

/// Write rule: the format this build writes ([`LAYOUTS_VERSION`], for the reasons of [`valid_panels`]); <= 20
/// layouts; each name 1..=40 characters, already trimmed, without control characters; each arrangement a valid
/// [`Panels`] (its format included).
#[must_use]
pub fn valid_layouts(layouts: &Layouts) -> bool {
    layouts.v == LAYOUTS_VERSION
        && layouts.items.len() <= MAX_LAYOUTS
        && layouts.items.iter().all(|item| {
            let name = item.name.as_str();
            !name.is_empty()
                && name.trim() == name
                && name.chars().count() <= MAX_LAYOUT_NAME
                && !name.chars().any(char::is_control)
                && valid_panels(&item.arrangement)
        })
}

/// The tables of an array, in both spellings: an array of inline tables | `[[an.array]]` of tables. `None`: no
/// array, or an element that is no table.
fn rows_of(item: &Item) -> Option<Vec<&dyn toml_edit::TableLike>> {
    match item {
        Item::Value(Value::Array(array)) => array
            .iter()
            .map(|value| {
                value
                    .as_inline_table()
                    .map(|t| t as &dyn toml_edit::TableLike)
            })
            .collect(),
        Item::ArrayOfTables(array) => Some(
            array
                .iter()
                .map(|t| t as &dyn toml_edit::TableLike)
                .collect(),
        ),
        _ => None,
    }
}

/// The format number `v` of a settings table; `None`: absent | no whole number from 0 on.
fn version_of(table: &dyn toml_edit::TableLike) -> Option<u32> {
    u32::try_from(table.get("v")?.as_integer()?).ok()
}

/// One stack of groups, leniently: anything unreadable in it (wrong types, a share that is no number) -> `None`.
fn view_groups_of(item: &Item) -> Option<Vec<ViewGroup>> {
    rows_of(item)?
        .into_iter()
        .map(|group| {
            let views = group
                .get("views")?
                .as_array()?
                .iter()
                .map(|id| id.as_str().map(str::to_owned))
                .collect::<Option<Vec<String>>>()?;
            let share = group.get("share")?;
            #[allow(clippy::cast_precision_loss)]
            // a hand-written whole number of a fraction (0 | 1)
            let share = share
                .as_float()
                .or_else(|| share.as_integer().map(|n| n as f64))?;
            share.is_finite().then_some(ViewGroup { views, share })
        })
        .collect()
}

/// One stack of groups as an array of inline tables: `[{ views = [..], share = n }, ..]`.
fn view_groups_value(groups: &[ViewGroup]) -> Value {
    Value::Array(
        groups
            .iter()
            .map(|group| {
                let mut inline = toml_edit::InlineTable::new();
                inline.insert(
                    "views",
                    Value::Array(group.views.iter().map(String::as_str).collect()),
                );
                inline.insert("share", Value::from(group.share));
                Value::InlineTable(inline)
            })
            .collect(),
    )
}

/// `[right_views]` of a document, leniently: anything unreadable (wrong types, no `v`, a share that is no number)
/// -> `None` (the frontend's default). Array of inline tables or `[[right_views.groups]]` both read.
fn right_views_of(doc: &DocumentMut) -> Option<RightViews> {
    let table = doc.get("right_views")?.as_table_like()?;
    Some(RightViews {
        v: version_of(table)?,
        groups: view_groups_of(table.get("groups")?)?,
    })
}

/// The `[right_views]` table of an arrangement: `v`, then `groups` as an array of inline tables.
fn right_views_table(views: &RightViews) -> Table {
    let mut table = Table::new();
    table.insert("v", Item::Value(Value::from(i64::from(views.v))));
    table.insert("groups", Item::Value(view_groups_value(&views.groups)));
    table
}

/// A `[panels]` table (the key itself | the `arrangement` of a layout), leniently: no `v`, a region absent |
/// unreadable -> `None`. Each region reads in both spellings (`left = [{ .. }]` | `[[panels.left]]`).
fn panels_of(table: &dyn toml_edit::TableLike) -> Option<Panels> {
    Some(Panels {
        v: version_of(table)?,
        left: view_groups_of(table.get("left")?)?,
        right: view_groups_of(table.get("right")?)?,
        bottom: view_groups_of(table.get("bottom")?)?,
    })
}

/// A `[panels]` table: `v`, then each region as an array of inline tables.
fn panels_table(panels: &Panels) -> Table {
    let mut table = Table::new();
    table.insert("v", Item::Value(Value::from(i64::from(panels.v))));
    for (region, groups) in [
        ("left", &panels.left),
        ("right", &panels.right),
        ("bottom", &panels.bottom),
    ] {
        table.insert(region, Item::Value(view_groups_value(groups)));
    }
    table
}

/// `[layouts]` of a document, leniently: no `v`, no list, one unreadable layout (a name that is no string, an
/// unreadable arrangement) -> `None` (no layout). `[[layouts.items]]` or an array of inline tables both read.
fn layouts_of(doc: &DocumentMut) -> Option<Layouts> {
    let table = doc.get("layouts")?.as_table_like()?;
    let items = rows_of(table.get("items")?)?
        .into_iter()
        .map(|item| {
            Some(LayoutItem {
                name: item.get("name")?.as_str()?.to_owned(),
                arrangement: panels_of(item.get("arrangement")?.as_table_like()?)?,
            })
        })
        .collect::<Option<Vec<LayoutItem>>>()?;
    Some(Layouts {
        v: version_of(table)?,
        items,
    })
}

/// The `[layouts]` table: `v`, then one `[[layouts.items]]` per layout (`name`, its `arrangement` as a table).
fn layouts_table(layouts: &Layouts) -> Table {
    let mut table = Table::new();
    table.insert("v", Item::Value(Value::from(i64::from(layouts.v))));
    let items: toml_edit::ArrayOfTables = layouts
        .items
        .iter()
        .map(|item| {
            let mut row = Table::new();
            row.insert("name", Item::Value(Value::from(item.name.as_str())));
            row.insert("arrangement", Item::Table(panels_table(&item.arrangement)));
            row
        })
        .collect();
    // an array of tables without a table writes no line: the empty list would read back as absent
    let items = if items.is_empty() {
        Item::Value(Value::Array(toml_edit::Array::new()))
    } else {
        Item::ArrayOfTables(items)
    };
    table.insert("items", items);
    table
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
    /// The right column's arrangement (`[right_views]`); `None` = absent | unreadable = the frontend's default.
    pub right_views: Option<RightViews>,
    /// The arrangement of every region (`[panels]`); `None` = absent | unreadable = the frontend's default.
    pub panels: Option<Panels>,
    /// The named arrangements (`[layouts]`); `None` = absent | unreadable = no layout.
    pub layouts: Option<Layouts>,
    /// Long lines wrap in the text viewer and the source editors (`wrap_lines`; absent = on).
    pub wrap_lines: bool,
    /// Folder under the document's folder for pasted pictures (`image_folder`). Absent = `img`.
    /// Empty = beside the document.
    pub image_folder: String,
    /// File under the profile folder with the picker groups (`characters_file`). Absent = `characters.toml`.
    /// Empty = the built-in groups only.
    pub characters_file: String,
    /// Character groups the emoji + symbol pickers show: the built-ins, replaced whole by the file.
    pub characters: Vec<CharacterGroup>,
    /// Path of the groups file the groups came from ("" = the built-ins only).
    pub characters_path: String,
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
            right_views: None,
            panels: None,
            layouts: None,
            wrap_lines: true,
            image_folder: "img".to_owned(),
            characters_file: characters::DEFAULT_FILE_NAME.to_owned(),
            characters: characters::defaults(),
            characters_path: String::new(),
            plugin_tables: BTreeMap::new(),
        }
    }
}

/// One change of one key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
    /// The right column's arrangement, written whole; `None` = back to the default (table removed).
    RightViews(Option<RightViews>),
    /// The arrangement of every region, written whole; `None` = back to the default (table removed).
    Panels(Option<Panels>),
    /// The named arrangements, written whole; `None` = no layout kept (table removed).
    Layouts(Option<Layouts>),
    WrapLines(bool),
    /// One path segment under the document's folder. Empty = beside the document.
    ImageFolder(String),
    /// File under the profile folder with the picker groups. Empty = the built-in groups only.
    CharactersFile(String),
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
            Self::RightViews(_) => "right_views".to_owned(),
            Self::Panels(_) => "panels".to_owned(),
            Self::Layouts(_) => "layouts".to_owned(),
            Self::WrapLines(_) => "wrap_lines".to_owned(),
            Self::ImageFolder(_) => "image_folder".to_owned(),
            Self::CharactersFile(_) => "characters_file".to_owned(),
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
    /// The settings file is there but cannot be read as text: it stays as it is and nothing is changed.
    #[error("cannot read {path}: {}", why.code())]
    Unreadable { path: String, why: Unreadable },
}

/// Why a settings file cannot be read as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    NotText,
    TooLarge,
    /// Another program holds it, past the retries.
    Locked,
}

impl Unreadable {
    /// The word the UI words it by (`detail` of [`crate::error::ErrorKind::SettingsUnreadable`]).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::NotText => "notText",
            Self::TooLarge => "tooLarge",
            Self::Locked => "locked",
        }
    }
}

impl From<SettingsError> for IpcError {
    fn from(error: SettingsError) -> Self {
        match error {
            SettingsError::Io { path, source } => IpcError::io(path, &source),
            SettingsError::Unreadable { path, why } => {
                IpcError::new(crate::error::ErrorKind::SettingsUnreadable)
                    .subject(path)
                    .detail(why.code())
            }
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
    /// The refusal of this change (`reason`: a word the UI can word).
    fn invalid(&self, reason: &str) -> SettingsError {
        SettingsError::Invalid {
            key: self.key(),
            reason: reason.to_owned(),
        }
    }

    /// Refuses values that must never reach the file.
    ///
    /// # Errors
    /// `Invalid`: reference template too long; editor command that is not a valid template.
    pub fn validate(&self) -> Result<(), SettingsError> {
        let invalid = |reason: &str| self.invalid(reason);
        match self {
            Self::ReferenceTemplate(text) if text.chars().count() > MAX_REFERENCE_TEMPLATE => {
                Err(invalid("tooLong"))
            }
            Self::NotesFolder(text) if !valid_notes_folder(text) => Err(invalid("folder")),
            Self::Editor(text) => crate::document::editor::EditorTemplate::parse(text)
                .map(|_| ())
                .map_err(|problem| invalid(problem.code())),
            Self::Tokenizers(ids) if !valid_tokenizers(ids) => Err(invalid("tokenizers")),
            Self::RightViews(Some(views)) if !valid_right_views(views) => {
                Err(invalid("rightViews"))
            }
            Self::Panels(Some(panels)) if !valid_panels(panels) => Err(invalid("panels")),
            Self::Layouts(Some(layouts)) if !valid_layouts(layouts) => Err(invalid("layouts")),
            Self::ImageFolder(text) => match image_folder_problem(text) {
                Some(reason) => Err(invalid(reason)),
                None => Ok(()),
            },
            Self::CharactersFile(text) => match characters_file_problem(text) {
                Some(reason) => Err(invalid(reason)),
                None => Ok(()),
            },
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
    read_settings_text(path).ok()??.parse::<DocumentMut>().ok()
}

/// The text of a settings file; `None`: there is none.
///
/// # Errors
/// `Unreadable`: larger than [`MAX_SETTINGS_BYTES`] | not UTF-8 | locked past the retries; `Io`: denied, any other
/// failure.
fn read_settings_text(path: &Path) -> Result<Option<String>, SettingsError> {
    let unreadable = |why| SettingsError::Unreadable {
        path: path.display().to_string(),
        why,
    };
    let bytes = match read_bytes(path, MAX_SETTINGS_BYTES) {
        Ok(bytes) => bytes,
        Err(FsxError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(FsxError::Io { source, .. }) if crate::fsx::is_locked(&source) => {
            return Err(unreadable(Unreadable::Locked));
        }
        Err(FsxError::Io { path, source }) => return Err(SettingsError::Io { path, source }),
        Err(FsxError::TooLarge { .. }) => return Err(unreadable(Unreadable::TooLarge)),
        Err(FsxError::Binary(_)) => return Err(unreadable(Unreadable::NotText)),
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| unreadable(Unreadable::NotText))
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
            right_views: right_views_of(doc),
            panels: doc
                .get("panels")
                .and_then(Item::as_table_like)
                .and_then(panels_of),
            layouts: layouts_of(doc),
            // a value that is not a boolean counts as not set
            wrap_lines: doc
                .get("wrap_lines")
                .and_then(Item::as_bool)
                .unwrap_or(true),
            image_folder: image_folder_of(str_of(doc, "image_folder")),
            characters_file: characters_file_of(str_of(doc, "characters_file")),
            // the groups ride on the profile file: SettingsFile attaches them (`attach_characters`)
            characters: Vec::new(),
            characters_path: String::new(),
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
///
/// # Errors
/// `Invalid`: the document holds `[panels]` | `[layouts]` in a newer format than the one being written (nothing
/// in the document changed).
pub fn apply_change(doc: &mut DocumentMut, change: &SettingChange) -> Result<(), SettingsError> {
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
        SettingChange::RightViews(views) => {
            set_table(root, "right_views", views.as_ref().map(right_views_table));
        }
        SettingChange::Panels(panels) => {
            let written = panels.as_ref().map_or(PANELS_VERSION, |p| p.v);
            let fresh = panels.as_ref().map(panels_table);
            if !set_versioned_table(root, "panels", written, fresh) {
                return Err(change.invalid("panels"));
            }
        }
        SettingChange::Layouts(layouts) => {
            let written = layouts.as_ref().map_or(LAYOUTS_VERSION, |l| l.v);
            let fresh = layouts.as_ref().map(layouts_table);
            if !set_versioned_table(root, "layouts", written, fresh) {
                return Err(change.invalid("layouts"));
            }
        }
        SettingChange::WrapLines(on) => set_keeping_decor(root, "wrap_lines", (*on).into()),
        // empty stays in the file: removing the key would read back as the default `img`
        SettingChange::ImageFolder(v) => set_keeping_decor(root, "image_folder", v.trim().into()),
        // empty stays in the file: removing the key would read back as the default groups file
        SettingChange::CharactersFile(v) => {
            set_keeping_decor(root, "characters_file", v.trim().into());
        }
    }
    Ok(())
}

/// Absent or not a single path segment: `img`. A stored empty string stays empty (beside the document).
fn image_folder_of(stored: Option<&str>) -> String {
    match stored {
        None => "img".to_owned(),
        Some(text) => {
            let name = text.trim();
            if name.is_empty() || check_name(name).is_ok() {
                name.to_owned()
            } else {
                "img".to_owned()
            }
        }
    }
}

/// Why `text` cannot be the image folder, after trim. `None`: empty or a single segment.
fn image_folder_problem(text: &str) -> Option<&'static str> {
    let name = text.trim();
    if name.is_empty() {
        return None;
    }
    check_name(name).err().map(NameProblem::as_str)
}

/// Absent: `characters.toml`. A stored empty string stays empty (the built-in groups only). A hand-edited value
/// the validator refuses reads as the default name.
fn characters_file_of(stored: Option<&str>) -> String {
    match stored {
        None => characters::DEFAULT_FILE_NAME.to_owned(),
        Some(text) => {
            let name = text.trim();
            if characters_file_problem(name).is_none() {
                name.to_owned()
            } else {
                characters::DEFAULT_FILE_NAME.to_owned()
            }
        }
    }
}

/// Why `text` cannot be the groups file, after trim. `None`: empty (the built-in groups only) | a name a file
/// system accepts whose extension is `.toml` (any case).
fn characters_file_problem(text: &str) -> Option<&'static str> {
    let name = text.trim();
    if name.is_empty() {
        return None;
    }
    if let Some(problem) = check_name(name).err() {
        return Some(problem.as_str());
    }
    (extension_of(name).as_deref() != Some("toml")).then_some("tomlOnly")
}

/// Writes | removes (`None`) a table of the root whole (`[right_views]`, `[panels]`, `[layouts]`); an existing
/// table keeps its place (and its header comment).
fn set_table(root: &mut Table, key: &str, fresh: Option<Table>) {
    let Some(fresh) = fresh else {
        root.remove(key);
        return;
    };
    if let Some(Item::Table(table)) = root.get_mut(key) {
        table.clear();
        for (name, item) in fresh {
            table.insert(&name, item);
        }
    } else {
        root.insert(key, Item::Table(fresh));
    }
}

/// [`set_table`] for a table that carries its format number `v`. One stored in a newer format than `written`
/// is another PerMark's (this build may not even read it): never written over | removed, `false` = left as it
/// is. A stored table without a readable `v` gives way like any other.
fn set_versioned_table(root: &mut Table, key: &str, written: u32, fresh: Option<Table>) -> bool {
    let newer = root
        .get(key)
        .and_then(Item::as_table_like)
        .and_then(version_of)
        .is_some_and(|stored| stored > written);
    if !newer {
        set_table(root, key, fresh);
    }
    !newer
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
    profile: PathBuf,
}

impl SettingsFile {
    #[must_use]
    pub fn app(dirs: &AppDirs) -> Self {
        Self {
            path: dirs.profile.join("settings.toml"),
            profile: dirs.profile.clone(),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Attaches the emoji | symbol groups (and the resolved file path) to settings read from disk.
    /// A missing | unreadable | invalid characters file falls back to the built-in defaults.
    fn attach_characters(&self, settings: AppSettings) -> AppSettings {
        let characters = match characters::load(&self.profile, &settings.characters_file) {
            Ok(groups) => groups,
            Err(_) => characters::defaults(),
        };
        let characters_path = if settings.characters_file.is_empty() {
            String::new()
        } else {
            self.profile
                .join(&settings.characters_file)
                .display()
                .to_string()
        };
        AppSettings {
            characters,
            characters_path,
            ..settings
        }
    }

    /// Current settings (defaults when missing | unreadable).
    #[must_use]
    pub fn load(&self) -> AppSettings {
        self.attach_characters(
            read_doc(&self.path)
                .map_or_else(AppSettings::default, |doc| AppSettings::from_doc(&doc)),
        )
    }

    /// Why the file is there but cannot be read (`load` gave the defaults then); `None`: read, or there is none.
    #[must_use]
    pub fn read_failure(&self) -> Option<SettingsError> {
        read_settings_text(&self.path).err()
    }

    /// Applies one change to the file on disk (read fresh: hand edits kept) and returns the new settings.
    /// A file that is not TOML is moved to `settings.toml.broken-<ms>` first (never silently overwritten).
    ///
    /// # Errors
    /// `Invalid` value; IO failure writing the file.
    pub fn apply(&self, change: &SettingChange) -> Result<AppSettings, SettingsError> {
        change.validate()?;
        let doc = edit_file(&self.path, |doc| apply_change(doc, change))?;
        Ok(self.attach_characters(AppSettings::from_doc(&doc)))
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
            changes
                .iter()
                .try_for_each(|change| apply_change(doc, change))
        })?;
        Ok(Some(self.attach_characters(AppSettings::from_doc(&doc))))
    }
}

/// Reads a settings file fresh (hand edits kept; a file that is not TOML is moved to `<name>.broken-<ms>` first),
/// lets `change` edit the document, writes it back atomically (line endings of an all-CRLF file kept). A change
/// that refuses what it read is an error and nothing is written. A file that cannot be read as text (too large,
/// not UTF-8, locked) is an error and stays as it is: it may be fine and only unreadable now, so it is never set
/// aside or reset.
pub(crate) fn edit_file(
    path: &Path,
    change: impl FnOnce(&mut DocumentMut) -> Result<(), SettingsError>,
) -> Result<DocumentMut, SettingsError> {
    let io = |source: io::Error| SettingsError::Io {
        path: path.display().to_string(),
        source,
    };
    let original = read_settings_text(path)?;
    let mut doc = match original.as_deref().map(str::parse::<DocumentMut>) {
        None => DocumentMut::new(),
        Some(Ok(doc)) => doc,
        Some(Err(_)) => {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            let name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            std::fs::rename(path, path.with_file_name(format!("{name}.broken-{ms}")))
                .map_err(io)?;
            DocumentMut::new()
        }
    };
    change(&mut doc)?;
    let original = original.unwrap_or_default();
    write_atomic(path, render(&doc, &original).as_bytes()).map_err(io)?;
    Ok(doc)
}

#[cfg(test)]
mod tests;
