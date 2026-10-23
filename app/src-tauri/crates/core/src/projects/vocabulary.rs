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

/// Why an edit of an official name was not made (`projects.md` §7.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditRefusal {
    /// `text` is not TOML, or the kind's table is not a table.
    Unreadable(String),
    /// The name is not in the list.
    NotListed,
    /// Rename: the new name is already in the list (never merged).
    Taken,
}

/// The kind's table of the document, as a header table (`[tags]` | `[tags.x]` | dotted keys) or an inline table.
enum Parent<'a> {
    Table(&'a mut Table),
    Inline(&'a mut toml_edit::InlineTable),
}

fn parent_of(doc: &mut DocumentMut, kind: VocabKind) -> Result<Option<Parent<'_>>, EditRefusal> {
    match doc.get_mut(kind.table()) {
        None => Ok(None),
        Some(Item::Table(t)) => Ok(Some(Parent::Table(t))),
        Some(Item::Value(Value::InlineTable(t))) => Ok(Some(Parent::Inline(t))),
        Some(_) => Err(EditRefusal::Unreadable(format!(
            "`{}` is not a table",
            kind.table()
        ))),
    }
}

impl Parent<'_> {
    fn like(&mut self) -> &mut dyn TableLike {
        match self {
            Self::Table(t) => *t,
            Self::Inline(t) => *t,
        }
    }

    /// The keys as written whose normalized form is `name`, in file order (the first is the one [`parse`] reads).
    fn spellings(&mut self, name: &str) -> Vec<String> {
        self.like()
            .iter()
            .filter(|(k, _)| normalize_tag(k).as_deref() == Some(name))
            .map(|(k, _)| k.to_owned())
            .collect()
    }

    /// Gives the entry `old` the key `new` in place: the entries after it are taken out and put back in order (a
    /// header table keeps its place by its position anyway), the key keeps its decor.
    fn rename(&mut self, old: &str, new: &str) {
        let decor = |key: &toml_edit::Key| {
            toml_edit::Key::new(new)
                .with_leaf_decor(key.leaf_decor().clone())
                .with_dotted_decor(key.dotted_decor().clone())
        };
        match self {
            Self::Table(t) => {
                let keys: Vec<String> = t.iter().map(|(k, _)| k.to_owned()).collect();
                let Some(at) = keys.iter().position(|k| k == old) else {
                    return;
                };
                let tail: Vec<_> = keys[at..]
                    .iter()
                    .filter_map(|k| t.remove_entry(k))
                    .collect();
                for (i, (key, item)) in tail.into_iter().enumerate() {
                    let key = if i == 0 { decor(&key) } else { key };
                    t.insert_formatted(&key, item);
                }
            }
            Self::Inline(t) => {
                let keys: Vec<String> = t.iter().map(|(k, _)| k.to_owned()).collect();
                let Some(at) = keys.iter().position(|k| k == old) else {
                    return;
                };
                let tail: Vec<_> = keys[at..]
                    .iter()
                    .filter_map(|k| t.remove_entry(k))
                    .collect();
                for (i, (key, value)) in tail.into_iter().enumerate() {
                    let key = if i == 0 { decor(&key) } else { key };
                    t.insert_formatted(&key, value);
                }
            }
        }
    }
}

/// Every table with a header (position, key path), for carrying comments of a removed table.
fn header_tables(table: &Table, path: &mut Vec<String>, out: &mut Vec<(isize, Vec<String>)>) {
    for (key, item) in table {
        if let Item::Table(t) = item {
            path.push(key.to_owned());
            if !t.is_implicit() && !t.is_dotted() {
                if let Some(position) = t.position() {
                    out.push((position, path.clone()));
                }
            }
            header_tables(t, path, out);
            path.pop();
        }
    }
}

/// The comments written in front of a removed table go in front of the next table (else to the end of the file):
/// removing an entry never takes the person's other text with it.
fn carry_comments(doc: &mut DocumentMut, after: isize, prefix: &str) {
    let mut all = Vec::new();
    header_tables(doc.as_table(), &mut Vec::new(), &mut all);
    let next = all
        .into_iter()
        .filter(|(position, _)| *position > after)
        .min_by_key(|(position, _)| *position);
    let table = next.and_then(|(_, path)| {
        let mut table = doc.as_table_mut();
        for key in &path {
            table = table.get_mut(key)?.as_table_mut()?;
        }
        Some(table)
    });
    if let Some(table) = table {
        let had = table
            .decor()
            .prefix()
            .and_then(|p| p.as_str())
            .unwrap_or_default()
            .trim_start_matches(['\r', '\n'])
            .to_owned();
        table.decor_mut().set_prefix(format!("{prefix}{had}"));
    } else {
        let had = doc.trailing().as_str().unwrap_or_default().to_owned();
        doc.set_trailing(format!("{had}{prefix}"));
    }
}

fn doc_of(text: &str) -> Result<DocumentMut, EditRefusal> {
    text.parse::<DocumentMut>()
        .map_err(|e| EditRefusal::Unreadable(e.message().trim().to_owned()))
}

/// `text` with the official `name` renamed to `new` (both tag-normalized by the caller): only the key's text changes,
/// its place, keys and comments stay. Other spellings of `name` (ignored when read) are dropped. Same name -> `None`.
///
/// # Errors
/// Not TOML | not a table; `name` not listed; `new` already listed.
pub fn rename_text(
    text: &str,
    kind: VocabKind,
    name: &str,
    new: &str,
) -> Result<Option<String>, EditRefusal> {
    let mut doc = doc_of(text)?;
    let mut parent = parent_of(&mut doc, kind)?.ok_or(EditRefusal::NotListed)?;
    let spellings = parent.spellings(name);
    let Some((first, others)) = spellings.split_first() else {
        return Err(EditRefusal::NotListed);
    };
    if name == new {
        return Ok(None);
    }
    if !parent.spellings(new).is_empty() {
        return Err(EditRefusal::Taken);
    }
    parent.rename(first, new);
    let others = others.to_vec();
    remove_keys(&mut doc, kind, &others)?;
    Ok(Some(finish(&doc, text)))
}

/// Takes the entries `keys` (as written) out of the kind's table, keeping the comments in front of header tables.
fn remove_keys(doc: &mut DocumentMut, kind: VocabKind, keys: &[String]) -> Result<(), EditRefusal> {
    for key in keys {
        let carried = {
            let Some(mut parent) = parent_of(doc, kind)? else {
                return Ok(());
            };
            match parent.like().remove(key) {
                Some(Item::Table(t)) => t.position().and_then(|position| {
                    let prefix = t.decor().prefix().and_then(|p| p.as_str())?;
                    prefix.contains('#').then(|| (position, prefix.to_owned()))
                }),
                _ => None,
            }
        };
        if let Some((position, prefix)) = carried {
            carry_comments(doc, position, &prefix);
        }
    }
    Ok(())
}

/// `text` with the official `name` (tag-normalized) taken out of the kind's list, every spelling of it.
///
/// # Errors
/// Not TOML | not a table; `name` not listed.
pub fn remove_text(text: &str, kind: VocabKind, name: &str) -> Result<String, EditRefusal> {
    let mut doc = doc_of(text)?;
    let spellings = parent_of(&mut doc, kind)?
        .ok_or(EditRefusal::NotListed)?
        .spellings(name);
    if spellings.is_empty() {
        return Err(EditRefusal::NotListed);
    }
    remove_keys(&mut doc, kind, &spellings)?;
    Ok(finish(&doc, text))
}

/// `text` with the look of the official `name` (tag-normalized) set: `color` | `icon` written | removed, its other
/// keys kept. Same look already -> `None`. An entry written as a plain value (`spec = true`) becomes a table only when
/// it gets a look.
///
/// # Errors
/// Not TOML | not a table; `name` not listed.
pub fn set_style_text(
    text: &str,
    kind: VocabKind,
    name: &str,
    style: &VocabStyle,
) -> Result<Option<String>, EditRefusal> {
    let mut doc = doc_of(text)?;
    let mut parent = parent_of(&mut doc, kind)?.ok_or(EditRefusal::NotListed)?;
    let Some(first) = parent.spellings(name).into_iter().next() else {
        return Err(EditRefusal::NotListed);
    };
    let like = parent.like();
    let current = like
        .get(&first)
        .and_then(Item::as_table_like)
        .map(style_of)
        .unwrap_or_default();
    if &current == style {
        return Ok(None);
    }
    let like = parent.like();
    if like.get(&first).and_then(Item::as_table_like).is_none() {
        // stays on its line: `spec = { color = "blue" }`
        if let Some(slot) = like.get_mut(&first) {
            let decor = slot.as_value().map(|v| v.decor().clone());
            let mut table = Value::InlineTable(toml_edit::InlineTable::new());
            if let Some(decor) = decor {
                *table.decor_mut() = decor;
            }
            *slot = Item::Value(table);
        }
    }
    // `[tags.spec.more]` alone: `spec` gets its own header once it has keys
    if let Some(Item::Table(t)) = like.get_mut(&first) {
        t.set_implicit(false);
    }
    let closing = inline_closing(like.get(&first));
    let Some(entry) = like.get_mut(&first).and_then(Item::as_table_like_mut) else {
        return Err(EditRefusal::NotListed);
    };
    for (key, value) in [("color", &style.color), ("icon", &style.icon)] {
        match value {
            Some(value) => {
                let mut new = Value::from(value.as_str());
                if let Some(old) = entry.get(key).and_then(Item::as_value) {
                    *new.decor_mut() = old.decor().clone();
                }
                entry.insert(key, Item::Value(new));
            }
            None => {
                entry.remove(key);
            }
        }
    }
    if let Some(Item::Value(Value::InlineTable(t))) = like.get_mut(&first) {
        close_inline(t, closing.as_deref());
    }
    Ok(Some(finish(&doc, text)))
}

/// The space before an inline table's `}` (the last value's suffix), when it has values.
fn inline_closing(item: Option<&Item>) -> Option<String> {
    let Some(Item::Value(Value::InlineTable(t))) = item else {
        return None;
    };
    let (_, last) = t.iter().last()?;
    last.decor()
        .suffix()
        .and_then(|s| s.as_str())
        .map(str::to_owned)
}

/// After keys were added | removed in an inline table: only the last value carries the space before `}`
/// (`{ icon = "star", color = "red" }`, never `{ icon = "star" , color = "red" }`).
fn close_inline(table: &mut toml_edit::InlineTable, closing: Option<&str>) {
    let Some(closing) = closing else {
        return;
    };
    let n = table.len();
    for (i, (_, value)) in table.iter_mut().enumerate() {
        let decor = value.decor_mut();
        if i + 1 == n {
            decor.set_suffix(closing);
        } else if decor.suffix().and_then(|s| s.as_str()) == Some(closing) {
            decor.set_suffix("");
        }
    }
}

/// The document as text in the line endings of `original`, with its end kept: a file without a final newline gets
/// none from an edit.
fn finish(doc: &DocumentMut, original: &str) -> String {
    let mut out = crate::settings::render(doc, original);
    if !original.is_empty() && !original.ends_with('\n') {
        if let Some(cut) = out.strip_suffix("\r\n").or_else(|| out.strip_suffix('\n')) {
            out = cut.to_owned();
        }
    }
    out
}

/// Reads + writes one vocabulary edit tries before it gives up (`Conflict`): only a writer without the lock (a
/// person's editor) can make it try again.
pub(crate) const WRITE_ATTEMPTS: usize = 8;

/// One write of a `vocabulary.toml` (`state::shared_write` rules): under the file's lock, `edit` gets the text as it is
/// on disk now (BOM off; `""` = no file) and answers the new text (`None`: nothing to write); written only while the
/// file still holds what was read, else read again and `edit` asked again: an outside edit meanwhile is never written
/// over, the op lands on top of it. A newer | unreadable file as it is now is never written. Returns the bytes
/// written (`None`: nothing changed). The caller checked `.permark/` (real folder, no link).
///
/// # Errors
/// `Unavailable` (newer | unreadable | not UTF-8), what `edit` refuses, `Conflict` (lock held too long | kept
/// changing), IO.
pub(crate) fn write_file(
    path: &Path,
    own: &crate::watch::OwnWrites,
    mut edit: impl FnMut(&str) -> Result<Option<String>, crate::IpcError>,
) -> Result<Option<Vec<u8>>, crate::IpcError> {
    use crate::{ErrorKind, IpcError};
    const BOM: &[u8] = b"\xEF\xBB\xBF";
    const OWN_KEY: &str = ".permark/vocabulary.toml";
    let unavailable = || IpcError::new(ErrorKind::Unavailable).subject("vocabulary");
    let conflict = || IpcError::new(ErrorKind::Conflict).subject("vocabulary");
    let _lock = crate::state::shared_write::lock_for(path).map_err(|_| conflict())?;
    for _ in 0..WRITE_ATTEMPTS {
        let old = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(IpcError::io(VOCABULARY_FILE, &e)),
        };
        if old
            .as_deref()
            .is_some_and(|bytes| !matches!(parse(bytes), VocabRead::Read(_)))
        {
            return Err(unavailable());
        }
        let (bom, text) = match old.as_deref() {
            None => (false, ""),
            Some(bytes) => {
                let (bom, rest) = bytes
                    .strip_prefix(BOM)
                    .map_or((false, bytes), |rest| (true, rest));
                (bom, std::str::from_utf8(rest).map_err(|_| unavailable())?)
            }
        };
        let Some(new_text) = edit(text)? else {
            return Ok(None);
        };
        let mut bytes = Vec::with_capacity(new_text.len() + BOM.len());
        if bom {
            bytes.extend_from_slice(BOM);
        }
        bytes.extend_from_slice(new_text.as_bytes());
        if old.as_deref() == Some(bytes.as_slice()) {
            return Ok(None);
        }
        let hash = crate::fsx::hash(&bytes);
        own.expect_write(OWN_KEY, hash);
        let written =
            crate::fsx::write_atomic_checked(path, &bytes, || match std::fs::read(path) {
                Ok(now) => Ok(old.as_deref() == Some(now.as_slice())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(old.is_none()),
                Err(e) => Err(e),
            });
        match written {
            Ok(true) => return Ok(Some(bytes)),
            Ok(false) => own.retract_write(OWN_KEY, hash),
            Err(e) => {
                own.retract_write(OWN_KEY, hash);
                return Err(IpcError::io(VOCABULARY_FILE, &e));
            }
        }
    }
    Err(conflict())
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
