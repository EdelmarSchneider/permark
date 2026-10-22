//! Official vocabularies (`projects.md` §7.1-§7.2, concept §2c): `<folder>/.permark/vocabulary.toml` declares tags and
//! categories with their look. The opened folder's lists are official for every included project below it; an
//! included project's own lists count as local to it; a tag found in files and in no list is unlisted. Nothing is
//! rejected or rewritten in files; marks stay dormant until the opened folder lists at least one name.
//!
//! ```toml
//! # Official tags and categories of this folder. PerMark changes only the lines it needs.
//! version = 1
//!
//! [tags.spec]
//! color = "blue"
//! icon = "book-open"
//!
//! [categories.software]
//! color = "teal"
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};
use ts_rs::TS;

use super::uid::ProjectUid;
use crate::organizing::normalize::{known_color, known_icon, normalize_tag};

/// The file, in a folder's `.permark/`.
pub const VOCABULARY_FILE: &str = "vocabulary.toml";
/// Larger files are not read (`Unreadable`).
pub const MAX_BYTES: u64 = 1024 * 1024;
/// Names per table (the rest is not read).
pub const MAX_NAMES: usize = 2000;

const HEADER: &str = "\
# Official tags and categories of this folder. PerMark changes only the lines it needs.
version = 1
";

/// Tag | category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "VocabularyKind")]
pub enum VocabKind {
    Tag,
    Category,
}

impl VocabKind {
    fn table(self) -> &'static str {
        match self {
            Self::Tag => "tags",
            Self::Category => "categories",
        }
    }
}

/// Where a tag | category stands for a path (`projects.md` §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TagStatus {
    /// In the opened folder's list.
    Official,
    /// In the list of an included project on the path's chain (not in the opened folder's).
    Local,
    /// Found in files, in no list.
    Unlisted,
}

/// The look a list gives a name (values outside the tag style sets = none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VocabStyle {
    pub color: Option<String>,
    pub icon: Option<String>,
}

/// One folder's lists (names normalized by the tag rule).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vocab {
    pub tags: BTreeMap<String, VocabStyle>,
    pub categories: BTreeMap<String, VocabStyle>,
}

impl Vocab {
    #[must_use]
    pub fn list(&self, kind: VocabKind) -> &BTreeMap<String, VocabStyle> {
        match kind {
            VocabKind::Tag => &self.tags,
            VocabKind::Category => &self.categories,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.categories.is_empty()
    }

    #[must_use]
    pub fn has(&self, kind: VocabKind, name: &str) -> bool {
        normalize_tag(name).is_some_and(|n| self.list(kind).contains_key(&n))
    }
}

/// What reading a vocabulary file gave.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum VocabRead {
    #[default]
    Absent,
    Read(Vocab),
    /// Written by a newer version: known keys read, writes refused.
    Newer(Vocab),
    /// Lists empty, never written over.
    Unreadable {
        line: u32,
        message: String,
    },
}

impl VocabRead {
    /// The lists (empty when absent | unreadable).
    #[must_use]
    pub fn vocab(&self) -> Option<&Vocab> {
        match self {
            Self::Read(v) | Self::Newer(v) => Some(v),
            Self::Absent | Self::Unreadable { .. } => None,
        }
    }
}

fn line_of(text: &str, at: usize) -> u32 {
    let end = at.min(text.len());
    u32::try_from(1 + text[..end].matches('\n').count()).unwrap_or(u32::MAX)
}

fn style_of(table: &dyn TableLike) -> VocabStyle {
    let text = |key: &str| table.get(key).and_then(Item::as_str);
    VocabStyle {
        color: text("color").and_then(known_color).map(str::to_owned),
        icon: text("icon").and_then(known_icon).map(str::to_owned),
    }
}

fn names_of(doc: &DocumentMut, kind: VocabKind) -> Result<BTreeMap<String, VocabStyle>, String> {
    let mut out = BTreeMap::new();
    let Some(item) = doc.get(kind.table()) else {
        return Ok(out);
    };
    let table = item
        .as_table_like()
        .ok_or_else(|| format!("`{}` is not a table", kind.table()))?;
    for (name, item) in table.iter() {
        if out.len() >= MAX_NAMES {
            break;
        }
        let Some(name) = normalize_tag(name) else {
            continue;
        };
        let style = item.as_table_like().map(style_of).unwrap_or_default();
        out.entry(name).or_insert(style);
    }
    Ok(out)
}

/// Reads the bytes of a vocabulary file (pure).
#[must_use]
pub fn parse(bytes: &[u8]) -> VocabRead {
    let unreadable = |line: u32, message: &str| VocabRead::Unreadable {
        line,
        message: message.to_owned(),
    };
    if bytes.len() as u64 > MAX_BYTES {
        return unreadable(1, "the file is larger than 1 MiB");
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return unreadable(1, "the file is not valid UTF-8");
    };
    let doc = match text.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(e) => {
            return unreadable(
                e.span().map_or(1, |s| line_of(text, s.start)),
                e.message().trim(),
            )
        }
    };
    let version = match doc.get("version") {
        None => 1,
        Some(item) => match item.as_integer() {
            Some(v) => v,
            None => return unreadable(1, "`version` is not a number"),
        },
    };
    if version < 1 {
        return unreadable(1, "`version` is not valid");
    }
    let lists = names_of(&doc, VocabKind::Tag).and_then(|tags| {
        Ok(Vocab {
            tags,
            categories: names_of(&doc, VocabKind::Category)?,
        })
    });
    match lists {
        Ok(vocab) if version > 1 => VocabRead::Newer(vocab),
        Ok(vocab) => VocabRead::Read(vocab),
        Err(message) => unreadable(1, &message),
    }
}

/// Reads `<folder>/.permark/vocabulary.toml` (a link in place of `.permark/` or of the file: absent).
#[must_use]
pub fn read(folder: &Path) -> VocabRead {
    let permark = folder.join(crate::state::PERMARK_DIR);
    let path = permark.join(VOCABULARY_FILE);
    if crate::tree::is_link_meta_of(&permark) || crate::tree::is_link_meta_of(&path) {
        return VocabRead::Absent;
    }
    match crate::fsx::read_bytes(&path, MAX_BYTES) {
        Ok(bytes) => parse(&bytes),
        Err(crate::fsx::FsxError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            VocabRead::Absent
        }
        Err(e) => VocabRead::Unreadable {
            line: 1,
            message: e.to_string(),
        },
    }
}

/// `text` (empty = no file yet) with `[<kind>.<name>]` added at the end (with the style's keys); only those lines
/// are new, the rest stays as written (line endings included). Already listed -> `None`.
///
/// # Errors
/// `text` is not TOML, or the kind's table is not a table.
pub fn promote_text(
    text: &str,
    kind: VocabKind,
    name: &str,
    style: &VocabStyle,
) -> Result<Option<String>, String> {
    let original = if text.trim().is_empty() { HEADER } else { text };
    let mut doc = original
        .parse::<DocumentMut>()
        .map_err(|e| e.message().trim().to_owned())?;
    if !doc.contains_key(kind.table()) {
        let mut table = Table::new();
        table.set_implicit(true);
        doc.insert(kind.table(), Item::Table(table));
    }
    let parent = doc
        .get_mut(kind.table())
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| format!("`{}` is not a table", kind.table()))?;
    if parent
        .iter()
        .any(|(k, _)| normalize_tag(k).as_deref() == Some(name))
    {
        return Ok(None);
    }
    let mut table = Table::new();
    if let Some(color) = &style.color {
        table.insert("color", Item::Value(Value::from(color.as_str())));
    }
    if let Some(icon) = &style.icon {
        table.insert("icon", Item::Value(Value::from(icon.as_str())));
    }
    parent.insert(name, Item::Table(table));
    // after every table already there
    let last = doc
        .as_table()
        .iter()
        .filter_map(|(_, item)| item.as_table_like())
        .flat_map(|t| {
            t.iter()
                .filter_map(|(_, i)| i.as_table().and_then(Table::position))
                .collect::<Vec<_>>()
        })
        .max()
        .unwrap_or(0);
    if let Some(table) = doc
        .get_mut(kind.table())
        .and_then(Item::as_table_like_mut)
        .and_then(|p| p.get_mut(name))
        .and_then(Item::as_table_mut)
    {
        table.set_position(Some(last + 1));
    }
    Ok(Some(crate::settings::render(&doc, original)))
}

/// The vocabularies an opened folder knows: its own lists (official) and each included project's (local).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vocabularies {
    pub official: VocabRead,
    pub local: BTreeMap<ProjectUid, Vocab>,
}

impl Vocabularies {
    /// Marks are shown only while the opened folder lists at least one tag | category (dormant otherwise).
    #[must_use]
    pub fn active(&self) -> bool {
        self.official.vocab().is_some_and(|v| !v.is_empty())
    }

    /// Status of `name` for a path whose owner chain is `chain` (outer -> inner): official | local (the innermost
    /// project on the chain that lists it) | unlisted. `None` while dormant.
    #[must_use]
    pub fn status(
        &self,
        kind: VocabKind,
        name: &str,
        chain: &[ProjectUid],
    ) -> Option<(TagStatus, Option<ProjectUid>)> {
        if !self.active() {
            return None;
        }
        let name = normalize_tag(name)?;
        if self
            .official
            .vocab()
            .is_some_and(|v| v.list(kind).contains_key(&name))
        {
            return Some((TagStatus::Official, None));
        }
        let local = chain.iter().rev().find(|uid| {
            self.local
                .get(*uid)
                .is_some_and(|v| v.list(kind).contains_key(&name))
        });
        Some(match local {
            Some(uid) => (TagStatus::Local, Some(uid.clone())),
            None => (TagStatus::Unlisted, None),
        })
    }

    /// Status of `name` anywhere in the folder (overview rows): official | local to the first project (by uid) that
    /// lists it | unlisted. `None` while dormant.
    #[must_use]
    pub fn status_anywhere(
        &self,
        kind: VocabKind,
        name: &str,
    ) -> Option<(TagStatus, Option<ProjectUid>)> {
        let all: Vec<ProjectUid> = self.local.keys().cloned().collect();
        self.status(kind, name, &all)
    }

    /// The look of a name: the official list's, else `None` (the caller falls back to its own styles).
    #[must_use]
    pub fn official_style(&self, kind: VocabKind, name: &str) -> Option<&VocabStyle> {
        let name = normalize_tag(name)?;
        self.official.vocab()?.list(kind).get(&name)
    }

    /// The local look of a name (the first project listing it), copied when it is promoted.
    #[must_use]
    pub fn local_style(&self, kind: VocabKind, name: &str) -> Option<&VocabStyle> {
        let name = normalize_tag(name)?;
        self.local.values().find_map(|v| v.list(kind).get(&name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(s: &str) -> ProjectUid {
        ProjectUid::parse(s).unwrap()
    }

    #[test]
    fn parse_table() {
        let read = parse(
            b"\xEF\xBB\xBF# c\r\nversion = 1\r\n\r\n[tags.Spec]\r\ncolor = \"BLUE\"\r\nicon = \"nope\"\r\n\r\n[tags.\"needs review\"]\r\n\r\n[categories.software]\r\ncolor = \"teal\"\r\nicon = \"code\"\r\nmore = 1\r\n",
        );
        let VocabRead::Read(v) = read else {
            panic!("{read:?}")
        };
        assert_eq!(
            v.tags["spec"],
            VocabStyle {
                color: Some("blue".to_owned()),
                icon: None
            }
        );
        assert!(v.tags.contains_key("needs-review"));
        assert_eq!(v.categories["software"].icon.as_deref(), Some("code"));
        assert_eq!(parse(b""), VocabRead::Read(Vocab::default()));
        assert!(
            matches!(parse(b"version = 2\n[tags.a]\n"), VocabRead::Newer(v) if v.tags.contains_key("a"))
        );
        assert!(matches!(
            parse(b"version = 0\n"),
            VocabRead::Unreadable { .. }
        ));
        assert!(matches!(
            parse(b"version = \"1\"\n"),
            VocabRead::Unreadable { .. }
        ));
        assert!(matches!(parse(b"tags = 3\n"), VocabRead::Unreadable { .. }));
        assert!(matches!(
            parse(b"[tags.a]\n<<<<<<< HEAD\n"),
            VocabRead::Unreadable { line: 2, .. }
        ));
        assert!(matches!(parse(&[0xff, 0xfe]), VocabRead::Unreadable { .. }));
        let big = vec![b'#'; usize::try_from(MAX_BYTES + 1).unwrap()];
        assert!(matches!(parse(&big), VocabRead::Unreadable { .. }));
        // huge counts: capped, never more than MAX_NAMES
        let mut many = String::from("version = 1\n");
        for i in 0..(MAX_NAMES + 50) {
            std::fmt::Write::write_fmt(&mut many, format_args!("[tags.t{i}]\n")).unwrap();
        }
        let VocabRead::Read(v) = parse(many.as_bytes()) else {
            panic!()
        };
        assert_eq!(v.tags.len(), MAX_NAMES);
    }

    #[test]
    fn promote_adds_only_its_table_and_keeps_the_rest() {
        let style = VocabStyle {
            color: Some("teal".to_owned()),
            icon: None,
        };
        let new = promote_text("", VocabKind::Tag, "spec", &style)
            .unwrap()
            .unwrap();
        assert_eq!(
            new,
            "# Official tags and categories of this folder. PerMark changes only the lines it needs.\nversion = 1\n\n[tags.spec]\ncolor = \"teal\"\n"
        );
        assert!(matches!(parse(new.as_bytes()), VocabRead::Read(v) if v.tags.contains_key("spec")));
        let crlf = "# mine\r\nversion = 1\r\n\r\n[categories.software] # keep\r\ncolor = \"teal\"\r\nother = 1\r\n";
        let out = promote_text(crlf, VocabKind::Tag, "needs-review", &VocabStyle::default())
            .unwrap()
            .unwrap();
        assert!(out.starts_with(crlf), "{out:?}");
        assert_eq!(&out[crlf.len()..], "\r\n[tags.needs-review]\r\n");
        assert_eq!(
            promote_text(&out, VocabKind::Tag, "needs-review", &VocabStyle::default()).unwrap(),
            None,
            "already listed"
        );
        let cat = promote_text(&out, VocabKind::Category, "docs", &VocabStyle::default())
            .unwrap()
            .unwrap();
        assert!(
            cat.ends_with("[tags.needs-review]\r\n\r\n[categories.docs]\r\n"),
            "{cat:?}"
        );
        assert!(promote_text("tags = 1\n", VocabKind::Tag, "a", &style).is_err());
    }

    #[test]
    fn status_table() {
        let mut official = Vocab::default();
        official
            .tags
            .insert("spec".to_owned(), VocabStyle::default());
        let mut a = Vocab::default();
        a.tags.insert("draft".to_owned(), VocabStyle::default());
        a.tags.insert("spec".to_owned(), VocabStyle::default());
        let mut sub = Vocab::default();
        sub.tags.insert("draft".to_owned(), VocabStyle::default());
        sub.categories
            .insert("lab".to_owned(), VocabStyle::default());
        let vocab = Vocabularies {
            official: VocabRead::Read(official),
            local: [(uid("aaaaaaaaaa"), a), (uid("bbbbbbbbbb"), sub)].into(),
        };
        let chain = [uid("aaaaaaaaaa"), uid("bbbbbbbbbb")];
        assert_eq!(
            vocab.status(VocabKind::Tag, "Spec", &chain),
            Some((TagStatus::Official, None)),
            "same name local = official"
        );
        assert_eq!(
            vocab.status(VocabKind::Tag, "draft", &chain),
            Some((TagStatus::Local, Some(uid("bbbbbbbbbb")))),
            "innermost lister"
        );
        assert_eq!(
            vocab.status(VocabKind::Tag, "draft", &chain[..1]),
            Some((TagStatus::Local, Some(uid("aaaaaaaaaa"))))
        );
        assert_eq!(
            vocab.status(VocabKind::Tag, "draft", &[]),
            Some((TagStatus::Unlisted, None)),
            "root-owned path: a member's list does not reach it"
        );
        assert_eq!(
            vocab.status(VocabKind::Category, "lab", &chain),
            Some((TagStatus::Local, Some(uid("bbbbbbbbbb"))))
        );
        assert_eq!(vocab.status(VocabKind::Tag, "  ", &chain), None);
        let dormant = Vocabularies {
            official: VocabRead::Read(Vocab::default()),
            ..vocab.clone()
        };
        assert_eq!(dormant.status(VocabKind::Tag, "draft", &chain), None);
        assert!(!Vocabularies::default().active());
    }
}
