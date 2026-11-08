//! The emoji + symbol picker groups: `groups` of the profile file named by `characters_file`. Read leniently
//! (an unreadable group | entry is skipped, the rest survives); missing file | empty name = the built-ins below.
//! A written file replaces the built-ins wholesale: adding, removing, regrouping is an edit to that file.

use std::io;
use std::path::Path;

use serde::Serialize;
use toml_edit::{DocumentMut, Item, Value};
use ts_rs::TS;

use crate::error::{ErrorKind, IpcError};
use crate::fsx;

/// File name of the picker groups under the profile folder when the setting is absent.
pub const DEFAULT_FILE_NAME: &str = "characters.toml";

/// A groups file larger than this is not config (refused, never read).
pub const MAX_CHARACTERS_BYTES: u64 = 256 * 1024;
/// Most groups one file may define.
pub const MAX_CHARACTERS_GROUPS: usize = 64;
/// Longest group name (chars).
pub const MAX_GROUP_NAME_CHARS: usize = 48;
/// Most entries one group may define.
pub const MAX_GROUP_ENTRIES: usize = 400;
/// Most search names one entry may carry.
pub const MAX_ENTRY_NAMES: usize = 12;
/// Longest search name (chars).
pub const MAX_ENTRY_NAME_CHARS: usize = 48;
/// Longest entry (chars: combining marks and ZWJ sequences count as one entry).
pub const MAX_ENTRY_CHARS: usize = 8;

/// One insertable character with the names the picker searches by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CharacterEntry {
    pub char: String,
    pub names: Vec<String>,
}

/// One named group of entries (the symbols picker shows groups; the emoji picker shows one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CharacterGroup {
    pub name: String,
    pub entries: Vec<CharacterEntry>,
}

/// Why the groups file cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum CharactersError {
    /// The file is there and is no readable config (binary | too large | not TOML).
    #[error("{path} is not a groups file: {detail}")]
    Unreadable { path: String, detail: &'static str },
    #[error("cannot read {path}: {source}")]
    Io { path: String, source: io::Error },
}

impl From<CharactersError> for IpcError {
    fn from(error: CharactersError) -> Self {
        match error {
            CharactersError::Unreadable { path, detail } => IpcError::new(ErrorKind::InvalidData)
                .subject(path)
                .detail(detail),
            CharactersError::Io { path, source } => IpcError::io(path, &source),
        }
    }
}

/// `groups` of a document, leniently: each group on its own (an unreadable group | entry is skipped).
/// `[[groups]]` tables and `groups = [{ name = …, entries = [{ char = …, names = […] }] }]` inline
/// tables both read (`right_views_of` reads the same two spellings).
pub(crate) fn groups_of(doc: &DocumentMut) -> Vec<CharacterGroup> {
    let rows: Vec<&dyn toml_edit::TableLike> = match doc.get("groups") {
        Some(Item::Value(Value::Array(array))) => array
            .iter()
            .filter_map(Value::as_inline_table)
            .map(|row| row as &dyn toml_edit::TableLike)
            .collect(),
        Some(Item::ArrayOfTables(array)) => array
            .iter()
            .map(|row| row as &dyn toml_edit::TableLike)
            .collect(),
        _ => return Vec::new(),
    };
    rows.iter()
        .filter_map(|row| {
            let name = row.get("name")?.as_str()?.trim();
            if name.is_empty() || name.chars().count() > MAX_GROUP_NAME_CHARS {
                return None;
            }
            let entries = entries_of(row.get("entries")?)?;
            Some(CharacterGroup {
                name: name.to_owned(),
                entries,
            })
        })
        .take(MAX_CHARACTERS_GROUPS)
        .collect()
}

/// One group's `entries`, leniently; `None` when nothing readable is left.
fn entries_of(item: &Item) -> Option<Vec<CharacterEntry>> {
    let rows: Vec<&dyn toml_edit::TableLike> = match item {
        Item::Value(Value::Array(array)) => array
            .iter()
            .filter_map(Value::as_inline_table)
            .map(|row| row as &dyn toml_edit::TableLike)
            .collect(),
        Item::ArrayOfTables(array) => array
            .iter()
            .map(|row| row as &dyn toml_edit::TableLike)
            .collect(),
        _ => return None,
    };
    let entries = rows
        .iter()
        .filter_map(|row| {
            let ch = row.get("char")?.as_str()?.trim();
            if ch.is_empty() || ch.chars().count() > MAX_ENTRY_CHARS {
                return None;
            }
            let names = row
                .get("names")
                .and_then(Item::as_array)
                .map(|names| {
                    names
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|name| {
                            !name.is_empty() && name.chars().count() <= MAX_ENTRY_NAME_CHARS
                        })
                        .map(str::to_owned)
                        .take(MAX_ENTRY_NAMES)
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default();
            Some(CharacterEntry {
                char: ch.to_owned(),
                names,
            })
        })
        .take(MAX_GROUP_ENTRIES)
        .collect::<Vec<CharacterEntry>>();
    (!entries.is_empty()).then_some(entries)
}

/// The built-in groups, in the file's own shape (a written file replaces them wholesale).
#[must_use]
pub fn defaults() -> Vec<CharacterGroup> {
    DEFAULT_CHARACTERS_TOML
        .parse::<DocumentMut>()
        .map_or_else(|_| Vec::new(), |doc| groups_of(&doc))
}

/// The picker groups: file `name` under the profile folder through [`fsx::read_text`]. Empty name and a
/// missing file are not errors: the built-ins. A valid file whose groups all read empty also gives the
/// built-ins (a picker never shows nothing).
///
/// # Errors
/// `Unreadable`: binary | too large | not TOML. `Io`: the file cannot be read.
pub(crate) fn load(profile: &Path, name: &str) -> Result<Vec<CharacterGroup>, CharactersError> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(defaults());
    }
    let path = profile.join(name);
    if std::fs::symlink_metadata(&path).is_err() {
        return Ok(defaults());
    }
    let shown = path.display().to_string();
    let text = fsx::read_text(&path, MAX_CHARACTERS_BYTES).map_err(|error| match error {
        fsx::FsxError::Io { source, .. } => CharactersError::Io {
            path: shown.clone(),
            source,
        },
        fsx::FsxError::TooLarge { .. } => CharactersError::Unreadable {
            path: shown.clone(),
            detail: "tooLarge",
        },
        fsx::FsxError::Binary(_) => CharactersError::Unreadable {
            path: shown.clone(),
            detail: "binary",
        },
    })?;
    let doc = text
        .text
        .parse::<DocumentMut>()
        .map_err(|_| CharactersError::Unreadable {
            path: shown,
            detail: "notToml",
        })?;
    let groups = groups_of(&doc);
    Ok(if groups.is_empty() {
        defaults()
    } else {
        groups
    })
}

/// The built-in groups: emoji, Greek, operators, relations, arrows, common units, sub/superscripts.
const DEFAULT_CHARACTERS_TOML: &str = r#"
[[groups]]
name = "emoji"
entries = [
  { char = "😀", names = ["grinning face"] },
  { char = "😂", names = ["face with tears of joy", "laughing"] },
  { char = "😉", names = ["winking face"] },
  { char = "😊", names = ["smiling face with smiling eyes"] },
  { char = "😍", names = ["smiling face with heart eyes", "in love"] },
  { char = "🤔", names = ["thinking face"] },
  { char = "👍", names = ["thumbs up", "yes"] },
  { char = "👎", names = ["thumbs down", "no"] },
  { char = "👏", names = ["clapping hands"] },
  { char = "🙏", names = ["folded hands", "pray"] },
  { char = "🎉", names = ["party popper", "celebration"] },
  { char = "❤", names = ["heart", "red heart"] },
  { char = "⭐", names = ["star"] },
  { char = "🔥", names = ["fire", "flame"] },
  { char = "💡", names = ["light bulb", "idea"] },
  { char = "✅", names = ["check mark", "done", "checked"] },
  { char = "❌", names = ["cross mark", "failed"] },
  { char = "⚠", names = ["warning"] },
  { char = "📌", names = ["pin", "pinned"] },
  { char = "🚧", names = ["construction", "work in progress"] },
]

[[groups]]
name = "greek"
entries = [
  { char = "α", names = ["alpha"] },
  { char = "β", names = ["beta"] },
  { char = "γ", names = ["gamma"] },
  { char = "δ", names = ["delta"] },
  { char = "ε", names = ["epsilon"] },
  { char = "ζ", names = ["zeta"] },
  { char = "η", names = ["eta"] },
  { char = "θ", names = ["theta"] },
  { char = "ι", names = ["iota"] },
  { char = "κ", names = ["kappa"] },
  { char = "λ", names = ["lambda"] },
  { char = "μ", names = ["mu"] },
  { char = "ν", names = ["nu"] },
  { char = "ξ", names = ["xi"] },
  { char = "ο", names = ["omicron"] },
  { char = "π", names = ["pi"] },
  { char = "ρ", names = ["rho"] },
  { char = "σ", names = ["sigma"] },
  { char = "τ", names = ["tau"] },
  { char = "υ", names = ["upsilon"] },
  { char = "φ", names = ["phi"] },
  { char = "χ", names = ["chi"] },
  { char = "ψ", names = ["psi"] },
  { char = "ω", names = ["omega"] },
  { char = "Γ", names = ["capital gamma", "Gamma"] },
  { char = "Δ", names = ["capital delta", "Delta", "delta change"] },
  { char = "Θ", names = ["capital theta", "Theta"] },
  { char = "Λ", names = ["capital lambda", "Lambda"] },
  { char = "Π", names = ["capital pi", "Pi", "product"] },
  { char = "Σ", names = ["capital sigma", "Sigma", "sum"] },
  { char = "Φ", names = ["capital phi", "Phi"] },
  { char = "Ψ", names = ["capital psi", "Psi"] },
  { char = "Ω", names = ["capital omega", "Omega", "ohm"] },
]

[[groups]]
name = "operators"
entries = [
  { char = "±", names = ["plus minus", "plus/minus"] },
  { char = "×", names = ["multiplication sign", "times"] },
  { char = "÷", names = ["division sign", "divide"] },
  { char = "⋅", names = ["dot operator"] },
  { char = "√", names = ["square root", "radical"] },
  { char = "∛", names = ["cube root"] },
  { char = "∑", names = ["n-ary summation", "sum"] },
  { char = "∏", names = ["n-ary product", "product"] },
  { char = "∫", names = ["integral"] },
  { char = "∮", names = ["contour integral"] },
  { char = "∂", names = ["partial differential", "partial derivative"] },
  { char = "∇", names = ["nabla", "del", "gradient"] },
  { char = "∞", names = ["infinity"] },
  { char = "⊕", names = ["circled plus", "direct sum", "xor"] },
  { char = "⊗", names = ["circled times", "tensor product"] },
  { char = "∴", names = ["therefore"] },
  { char = "∵", names = ["because"] },
]

[[groups]]
name = "relations"
entries = [
  { char = "≤", names = ["less than or equal to"] },
  { char = "≥", names = ["greater than or equal to"] },
  { char = "≠", names = ["not equal to"] },
  { char = "≈", names = ["almost equal to", "approximately"] },
  { char = "≡", names = ["identical to", "equivalent", "congruent"] },
  { char = "∝", names = ["proportional to"] },
  { char = "≪", names = ["much less than"] },
  { char = "≫", names = ["much greater than"] },
  { char = "⊂", names = ["subset of"] },
  { char = "⊆", names = ["subset of or equal to"] },
  { char = "⊃", names = ["superset of"] },
  { char = "⊇", names = ["superset of or equal to"] },
  { char = "∈", names = ["element of", "in"] },
  { char = "∉", names = ["not an element of", "not in"] },
  { char = "∋", names = ["contains", "has element"] },
  { char = "∅", names = ["empty set", "null set"] },
  { char = "⊥", names = ["perpendicular", "up tack", "bottom"] },
  { char = "∥", names = ["parallel", "parallel to"] },
]

[[groups]]
name = "arrows"
entries = [
  { char = "→", names = ["right arrow", "rightwards arrow", "yields", "implies"] },
  { char = "←", names = ["left arrow", "leftwards arrow"] },
  { char = "↑", names = ["up arrow", "upwards arrow"] },
  { char = "↓", names = ["down arrow", "downwards arrow"] },
  { char = "↔", names = ["left right arrow", "bidirectional"] },
  { char = "↕", names = ["up down arrow"] },
  { char = "↖", names = ["north west arrow", "up left arrow"] },
  { char = "↗", names = ["north east arrow", "up right arrow"] },
  { char = "↘", names = ["south east arrow", "down right arrow"] },
  { char = "↙", names = ["south west arrow", "down left arrow"] },
  { char = "↦", names = ["rightwards arrow from bar", "maps to"] },
  { char = "⇔", names = ["left right double arrow", "if and only if", "iff"] },
  { char = "⇐", names = ["leftwards double arrow"] },
  { char = "⇒", names = ["rightwards double arrow", "implies"] },
  { char = "⇑", names = ["upwards double arrow"] },
  { char = "⇓", names = ["downwards double arrow"] },
]

[[groups]]
name = "units"
entries = [
  { char = "°", names = ["degree sign", "degrees"] },
  { char = "′", names = ["prime", "minutes", "feet"] },
  { char = "″", names = ["double prime", "seconds", "inches"] },
  { char = "µ", names = ["micro sign", "micro", "mu"] },
  { char = "Ω", names = ["ohm sign", "omega", "ohms"] },
  { char = "℃", names = ["degree celsius", "celsius", "degrees celsius"] },
  { char = "℉", names = ["degree fahrenheit", "fahrenheit", "degrees fahrenheit"] },
  { char = "½", names = ["vulgar fraction one half", "half"] },
  { char = "⅓", names = ["vulgar fraction one third", "one third"] },
  { char = "¼", names = ["vulgar fraction one quarter", "quarter"] },
  { char = "¾", names = ["vulgar fraction three quarters", "three quarters"] },
  { char = "‰", names = ["per mille", "per thousand"] },
  { char = "€", names = ["euro sign", "euro", "euros"] },
  { char = "£", names = ["pound sign", "pound", "pounds"] },
  { char = "¥", names = ["yen sign", "yen", "yuan"] },
  { char = "№", names = ["numero sign", "number sign", "no"] },
  { char = "⌀", names = ["diameter sign", "diameter"] },
  { char = "Å", names = ["angstrom", "angstrom sign"] },
]

[[groups]]
name = "scripts"
entries = [
  { char = "¹", names = ["superscript one", "superscript 1", "first power"] },
  { char = "²", names = ["superscript two", "superscript 2", "squared"] },
  { char = "³", names = ["superscript three", "superscript 3", "cubed"] },
  { char = "⁴", names = ["superscript four", "superscript 4", "fourth power"] },
  { char = "⁵", names = ["superscript five", "superscript 5", "fifth power"] },
  { char = "⁶", names = ["superscript six", "superscript 6"] },
  { char = "⁷", names = ["superscript seven", "superscript 7"] },
  { char = "⁸", names = ["superscript eight", "superscript 8"] },
  { char = "⁹", names = ["superscript nine", "superscript 9"] },
  { char = "⁰", names = ["superscript zero", "superscript 0"] },
  { char = "⁺", names = ["superscript plus"] },
  { char = "⁻", names = ["superscript minus", "negative exponent"] },
  { char = "₀", names = ["subscript zero", "subscript 0"] },
  { char = "₁", names = ["subscript one", "subscript 1"] },
  { char = "₂", names = ["subscript two", "subscript 2"] },
  { char = "₃", names = ["subscript three", "subscript 3"] },
  { char = "₄", names = ["subscript four", "subscript 4"] },
  { char = "₅", names = ["subscript five", "subscript 5"] },
  { char = "₆", names = ["subscript six", "subscript 6"] },
  { char = "₇", names = ["subscript seven", "subscript 7"] },
  { char = "₈", names = ["subscript eight", "subscript 8"] },
  { char = "₉", names = ["subscript nine", "subscript 9"] },
  { char = "₊", names = ["subscript plus"] },
  { char = "₋", names = ["subscript minus"] },
]
"#;
