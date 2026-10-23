//! Identity card of a project (`projects.md` §6): `<project>/.permark/project.toml` v1 when the project has a real
//! `.permark/` (shared, travels), else `<profile>/projects/<ProjectId>/card.json`. Written only by the person's edit;
//! surgical text splices (same helpers as the record); never creates `.permark/`. Card text = data.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{Document, Item};
use ts_rs::TS;

use super::record::{eol_of, key_at, quoted, splice, strip_bom, Lines};
use super::uid::ProjectUid;
use crate::state::{AppDirs, StateData, PERMARK_DIR};

pub const CARD_FILE: &str = "project.toml";
pub const PROFILE_CARD: &str = "card.json";
/// Largest card read; a bigger one -> defaults + warning.
const MAX_CARD_BYTES: u64 = 4 * 1024;
const HEADER: &str = "# This project's card. PerMark changes only the lines it needs.";
const MAX_NAME: usize = 80;
const MAX_SHORT: usize = 24;
const MAX_DESCRIPTION: usize = 160;
/// Same rule as a tag (`organizing.md`): at most 40 characters.
const MAX_CATEGORY: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CardField {
    Name,
    Short,
    Description,
    Category,
}

impl CardField {
    pub const ALL: [Self; 4] = [Self::Name, Self::Short, Self::Description, Self::Category];

    fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Short => "short",
            Self::Description => "description",
            Self::Category => "category",
        }
    }
}

/// Where a card is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CardPlace {
    /// `.permark/project.toml` of the project.
    Project,
    Profile,
}

/// A card that could not be read whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CardWarning {
    TooLarge,
    Unreadable,
    /// Written by a newer PerMark: shown, not written.
    Newer,
}

/// A card as the UI shows it: defaults filled in (folder name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CardDto {
    pub name: String,
    pub short: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub place: CardPlace,
    /// Fields whose written value is not valid (the default is shown).
    pub invalid: Vec<CardField>,
    pub warning: Option<CardWarning>,
}

/// One field edit; an empty value removes the field (back to its default).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export)]
pub struct CardChange {
    pub field: CardField,
    pub value: String,
}

/// A card as read (values valid; `None` = not written | invalid).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Card {
    pub id: Option<ProjectUid>,
    pub name: Option<String>,
    pub short: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub invalid: Vec<CardField>,
    pub warning: Option<CardWarning>,
}

/// A field value as it may be stored, or why not (pure). `None`: refused.
#[must_use]
pub fn clean(field: CardField, raw: &str) -> Option<String> {
    // control characters are refused; a description folds its line breaks and tabs into spaces
    if raw
        .chars()
        .any(|c| c.is_control() && (field != CardField::Description || !c.is_whitespace()))
    {
        return None;
    }
    let value = match field {
        CardField::Name | CardField::Short => raw.trim().to_owned(),
        CardField::Description => raw.split_whitespace().collect::<Vec<_>>().join(" "),
        CardField::Category => raw
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
            .to_lowercase(),
    };
    let max = match field {
        CardField::Name => MAX_NAME,
        CardField::Short => MAX_SHORT,
        CardField::Description => MAX_DESCRIPTION,
        CardField::Category => MAX_CATEGORY,
    };
    let len = value.chars().count();
    (len >= 1 && len <= max).then_some(value)
}

/// Reads card text (pure, lenient: each field on its own).
#[must_use]
pub fn parse(bytes: &[u8]) -> Card {
    let mut card = Card::default();
    if bytes.len() as u64 > MAX_CARD_BYTES {
        card.warning = Some(CardWarning::TooLarge);
        return card;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        card.warning = Some(CardWarning::Unreadable);
        return card;
    };
    let (_, text) = strip_bom(text);
    let Ok(doc) = Document::parse(text) else {
        card.warning = Some(CardWarning::Unreadable);
        return card;
    };
    match doc.get("version").map(Item::as_integer) {
        None | Some(Some(1)) => {}
        Some(Some(v)) if v > 1 => card.warning = Some(CardWarning::Newer),
        Some(_) => {
            card.warning = Some(CardWarning::Unreadable);
            return card;
        }
    }
    card.id = doc
        .get("id")
        .and_then(Item::as_str)
        .and_then(ProjectUid::parse);
    for field in CardField::ALL {
        let Some(item) = doc.get(field.key()) else {
            continue;
        };
        let value = item.as_str().and_then(|raw| clean(field, raw));
        if value.is_none() {
            card.invalid.push(field);
        }
        match field {
            CardField::Name => card.name = value,
            CardField::Short => card.short = value,
            CardField::Description => card.description = value,
            CardField::Category => card.category = value,
        }
    }
    card
}

/// The card's values with defaults (folder name).
#[must_use]
pub fn dto(card: &Card, folder_name: &str, place: CardPlace) -> CardDto {
    let fallback: String = folder_name.chars().take(MAX_NAME).collect();
    CardDto {
        name: card.name.clone().unwrap_or_else(|| fallback.clone()),
        short: card.short.clone().unwrap_or(fallback),
        description: card.description.clone(),
        category: card.category.clone(),
        place,
        invalid: card.invalid.clone(),
        warning: card.warning,
    }
}

/// Sets (`Some`) | removes (`None`) a top-level key of card text; `id` + `version` written when missing (pure).
#[must_use]
pub fn apply(text: &str, id: &ProjectUid, field: CardField, value: Option<&str>) -> String {
    let (bom, body) = strip_bom(text);
    let eol = eol_of(body);
    let mut out = if body.trim().is_empty() {
        format!("{HEADER}{eol}version = 1{eol}")
    } else {
        body.to_owned()
    };
    if !Document::parse(out.as_str()).is_ok_and(|d| d.contains_key("id")) {
        out = set_key(&out, "id", Some(&quoted(id.as_str())), eol);
    }
    out = set_key(&out, field.key(), value.map(quoted).as_deref(), eol);
    format!("{bom}{out}")
}

/// One top-level key set to the TOML text `value` (replaced in place, else a new line after the last top-level key)
/// or removed (its line).
fn set_key(text: &str, key: &str, value: Option<&str>, eol: &str) -> String {
    let Ok(doc) = Document::parse(text) else {
        return text.to_owned();
    };
    let lines = Lines::of(text);
    let at = key_at(doc.as_table(), key, &lines, text.len());
    match (at, value) {
        (Some(at), Some(value)) => splice(text, at.value, value),
        (Some(at), None) => splice(text, at.lines, ""),
        (None, None) => text.to_owned(),
        (None, Some(value)) => {
            let last = doc
                .as_table()
                .iter()
                .filter_map(|(k, item)| {
                    item.as_value()?;
                    key_at(doc.as_table(), k, &lines, text.len()).map(|a| a.lines.end)
                })
                .max();
            let at = last.unwrap_or(text.len());
            let mut line = String::new();
            if at > 0 && !text[..at].ends_with('\n') {
                line.push_str(eol);
            }
            line.push_str(key);
            line.push_str(" = ");
            line.push_str(value);
            line.push_str(eol);
            splice(text, at..at, &line)
        }
    }
}

/// Profile copy of a card (no `id`: the profile folder is the identity).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileCard {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

impl StateData for ProfileCard {
    const VERSION: u32 = 1;
}

/// Where the card of the folder `abs` is kept.
#[derive(Debug, Clone)]
pub enum CardFile {
    Project(PathBuf),
    Profile(crate::state::StateFile<ProfileCard>),
}

/// The card file of a project folder: its `.permark/project.toml` when `.permark/` is a real folder, else the
/// profile (by the folder's `ProjectId`).
#[must_use]
pub fn file_of(dirs: &AppDirs, abs: &Path) -> Option<CardFile> {
    let permark = abs.join(PERMARK_DIR);
    if std::fs::symlink_metadata(&permark)
        .is_ok_and(|m| m.is_dir() && !crate::tree::is_link_meta(&m))
    {
        return Some(CardFile::Project(permark.join(CARD_FILE)));
    }
    let root = crate::paths::Root::open(abs).ok()?;
    let store = crate::state::StateStore::for_project(dirs, &root);
    Some(CardFile::Profile(store.profile_file(PROFILE_CARD)))
}

impl CardFile {
    #[must_use]
    pub fn place(&self) -> CardPlace {
        match self {
            Self::Project(_) => CardPlace::Project,
            Self::Profile(_) => CardPlace::Profile,
        }
    }

    /// The card as kept (missing = empty card).
    #[must_use]
    pub fn read(&self) -> Card {
        match self {
            Self::Project(path) => {
                if super::record::is_link(path) {
                    return Card {
                        warning: Some(CardWarning::Unreadable),
                        ..Card::default()
                    };
                }
                match crate::fsx::read_bytes(path, MAX_CARD_BYTES) {
                    Ok(bytes) => parse(&bytes),
                    Err(crate::fsx::FsxError::TooLarge { .. }) => Card {
                        warning: Some(CardWarning::TooLarge),
                        ..Card::default()
                    },
                    Err(_) => Card::default(),
                }
            }
            Self::Profile(file) => {
                let kept = file.load().into_value();
                let take =
                    |field, raw: &Option<String>| raw.as_deref().and_then(|r| clean(field, r));
                Card {
                    id: None,
                    name: take(CardField::Name, &kept.name),
                    short: take(CardField::Short, &kept.short),
                    description: take(CardField::Description, &kept.description),
                    category: take(CardField::Category, &kept.category),
                    invalid: Vec::new(),
                    warning: None,
                }
            }
        }
    }

    /// Writes one field (the person's edit). `id`: the project's uid (written into a project card that has none).
    /// Returns the bytes written to a project card (announced as the app's own write).
    ///
    /// # Errors
    /// `InvalidData` (value refused), `Unavailable` (card newer | unreadable | a link), `Conflict`, IO.
    pub fn write(
        &self,
        id: &ProjectUid,
        change: &CardChange,
        announce: &dyn Fn(&Path, &[u8]),
    ) -> Result<(), crate::IpcError> {
        use crate::{ErrorKind, IpcError};
        let value =
            if change.value.trim().is_empty() {
                None
            } else {
                Some(clean(change.field, &change.value).ok_or_else(|| {
                    IpcError::new(ErrorKind::InvalidData).subject(change.field.key())
                })?)
            };
        match self {
            Self::Project(path) => {
                if super::record::is_link(path) {
                    return Err(IpcError::new(ErrorKind::Unavailable).subject("card"));
                }
                let io = |e: std::io::Error| IpcError::io(path.display().to_string(), &e);
                for _ in 0..2 {
                    let before = match std::fs::read(path) {
                        Ok(bytes) => Some(bytes),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                        Err(e) => return Err(io(e)),
                    };
                    if let Some(bytes) = &before {
                        let card = parse(bytes);
                        if card.warning.is_some() {
                            return Err(IpcError::new(ErrorKind::Unavailable).subject("card"));
                        }
                    }
                    let text = before
                        .as_deref()
                        .map(|b| String::from_utf8_lossy(b).into_owned())
                        .unwrap_or_default();
                    let after = apply(&text, id, change.field, value.as_deref()).into_bytes();
                    let seen = before.as_deref().map(crate::fsx::hash);
                    announce(path, &after);
                    let done = crate::fsx::write_atomic_checked(path, &after, || {
                        Ok(match std::fs::read(path) {
                            Ok(now) => Some(crate::fsx::hash(&now)) == seen,
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => seen.is_none(),
                            Err(e) => return Err(e),
                        })
                    })
                    .map_err(io)?;
                    if done {
                        return Ok(());
                    }
                }
                Err(IpcError::new(ErrorKind::Conflict).subject("card"))
            }
            Self::Profile(file) => {
                let mut kept = file.load().into_value();
                let slot = match change.field {
                    CardField::Name => &mut kept.name,
                    CardField::Short => &mut kept.short,
                    CardField::Description => &mut kept.description,
                    CardField::Category => &mut kept.category,
                };
                *slot = value;
                file.save(&kept)?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid() -> ProjectUid {
        ProjectUid::parse("k7m2q7xd2a").unwrap()
    }

    #[test]
    fn parse_reads_valid_fields_and_flags_invalid_ones() {
        let card = parse(
            "version = 1\nid = \"k7m2q7xd2a\"\nname = \"PerMark software\"\nshort = \"this short name is far too long to fit\"\ndescription = \"  one\\n two  \"\ncategory = \"Soft Ware\"\nmore = 1\n".as_bytes(),
        );
        assert_eq!(card.id, Some(uid()));
        assert_eq!(card.name.as_deref(), Some("PerMark software"));
        assert_eq!(card.short, None);
        assert_eq!(card.invalid, vec![CardField::Short]);
        assert_eq!(card.description.as_deref(), Some("one two"));
        assert_eq!(card.category.as_deref(), Some("soft-ware"));
        let shown = dto(&card, "hub-b-software", CardPlace::Project);
        assert_eq!(shown.short, "hub-b-software");
    }

    #[test]
    fn bad_cards_fall_back_to_defaults_with_a_warning() {
        assert_eq!(parse(b"not toml [").warning, Some(CardWarning::Unreadable));
        assert_eq!(parse(&[b'#'; 5000]).warning, Some(CardWarning::TooLarge));
        assert_eq!(
            parse(b"version = 2\nname = \"x\"\n").warning,
            Some(CardWarning::Newer)
        );
        assert_eq!(
            parse(b"version = 2\nname = \"x\"\n").name.as_deref(),
            Some("x")
        );
        assert_eq!(
            parse(b"name = \"a\\u0007b\"\n").invalid,
            vec![CardField::Name]
        );
        assert_eq!(parse(b"name = 3\n").invalid, vec![CardField::Name]);
    }

    #[test]
    fn apply_creates_a_card_with_id_then_edits_surgically() {
        let made = apply("", &uid(), CardField::Name, Some("Hub B"));
        assert_eq!(
            made,
            format!("{HEADER}\nversion = 1\nid = \"k7m2q7xd2a\"\nname = \"Hub B\"\n")
        );
        let hand = "# mine\r\nname = \"Old\"  # keep\r\n\r\n[extra]\r\nx = 1\r\n";
        let out = apply(hand, &uid(), CardField::Name, Some("New"));
        assert_eq!(
            out,
            "# mine\r\nname = \"New\"  # keep\r\nid = \"k7m2q7xd2a\"\r\n\r\n[extra]\r\nx = 1\r\n"
        );
        let out = apply(&out, &uid(), CardField::Short, Some("hb"));
        assert!(out.contains("id = \"k7m2q7xd2a\"\r\nshort = \"hb\"\r\n\r\n[extra]"));
        let out = apply(&out, &uid(), CardField::Name, None);
        assert!(!out.contains("name ="));
        assert!(out.contains("# mine\r\n"));
        assert_eq!(parse(out.as_bytes()).short.as_deref(), Some("hb"));
    }

    #[test]
    fn project_card_never_creates_permark_and_profile_card_round_trips() {
        let base = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(&base.path().join("dirs"));
        let project = base.path().join("proj ü");
        std::fs::create_dir(&project).unwrap();
        let file = file_of(&dirs, &project).unwrap();
        assert_eq!(file.place(), CardPlace::Profile);
        file.write(
            &uid(),
            &CardChange {
                field: CardField::Name,
                value: "Notes app".into(),
            },
            &|_, _| {},
        )
        .unwrap();
        assert!(
            !project.join(PERMARK_DIR).exists(),
            "never creates .permark/"
        );
        assert_eq!(file.read().name.as_deref(), Some("Notes app"));
        std::fs::create_dir(project.join(PERMARK_DIR)).unwrap();
        let file = file_of(&dirs, &project).unwrap();
        assert_eq!(file.place(), CardPlace::Project);
        let announced = std::cell::Cell::new(false);
        file.write(
            &uid(),
            &CardChange {
                field: CardField::Category,
                value: "Soft ware".into(),
            },
            &|_, _| announced.set(true),
        )
        .unwrap();
        assert!(announced.get(), "own write announced before writing");
        let card = file.read();
        assert_eq!(card.id, Some(uid()));
        assert_eq!(card.category.as_deref(), Some("soft-ware"));
        let refused = file.write(
            &uid(),
            &CardChange {
                field: CardField::Short,
                value: "x".repeat(25),
            },
            &|_, _| {},
        );
        assert!(refused.is_err());
    }
}
