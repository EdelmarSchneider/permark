//! `meta.toml`: the tags and descriptions added in the app (the person's own data, layout of 0.5 kept).
//!
//! ```toml
//! [files."docs/specs/live-feed.md"]
//! tags = ["spec", "needs-review"]
//! description = "How the live feed groups watcher events."
//!
//! [tag_styles.spec]
//! color = "blue"
//! icon = "book-open"
//! ```
//!
//! Reading gives [`Entries`] (normalized, what the UI shows). Writing never serializes `Entries`: [`edit_text`] applies
//! [`Op`]s to the text with `toml_edit`, so only the touched keys change and everything else (comments, order, spacing,
//! unknown keys and tables, line endings) stays as the person or another version left it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::paths::key_of;
use crate::settings::render;
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value};

use super::normalize::{
    known_color, known_icon, normalize_description, normalize_tag, normalize_tags,
};

/// Written at the top of a `meta.toml` this app creates.
pub const HEADER: &str = "\
# PerMark metadata: tags and descriptions added in the app.
# Safe to commit and share. PerMark rewrites this file when you edit tags.

";

/// What the app knows about one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub tags: Vec<String>,
    pub description: String,
}

impl Entry {
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.tags.is_empty() && self.description.is_empty()
    }
}

/// The look of a tag: values this version knows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    pub color: Option<&'static str>,
    pub icon: Option<&'static str>,
}

impl Style {
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.color.is_none() && self.icon.is_none()
    }
}

/// Why a `meta.toml` cannot be used (a merge conflict, a hand edit gone wrong).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub message: String,
    /// 1-based line of the first error, when known.
    pub line: Option<usize>,
}

impl Problem {
    fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: None,
        }
    }
}

/// One change to the store. Values are already normalized (see the constructors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    SetTags {
        path: String,
        tags: Vec<String>,
    },
    SetDescription {
        path: String,
        description: String,
    },
    /// Both fields replaced; `None` = none.
    SetStyle {
        tag: String,
        color: Option<&'static str>,
        icon: Option<&'static str>,
    },
    Remove {
        path: String,
    },
    /// A file moved.
    Rename {
        from: String,
        to: String,
    },
    /// A folder moved: every entry below it follows.
    RenameTree {
        from: String,
        to: String,
    },
}

impl Op {
    #[must_use]
    pub fn set_tags<S: AsRef<str>>(path: &str, tags: &[S]) -> Self {
        Self::SetTags {
            path: path.to_owned(),
            tags: normalize_tags(tags),
        }
    }

    #[must_use]
    pub fn set_description(path: &str, description: &str) -> Self {
        Self::SetDescription {
            path: path.to_owned(),
            description: normalize_description(description),
        }
    }

    /// `None` when the tag name is empty. A color or icon outside the sets means none.
    #[must_use]
    pub fn set_style(tag: &str, color: Option<&str>, icon: Option<&str>) -> Option<Self> {
        Some(Self::SetStyle {
            tag: normalize_tag(tag)?,
            color: color.and_then(known_color),
            icon: icon.and_then(known_icon),
        })
    }
}

/// Every entry, looked up by path (a spelling that differs only in case finds the entry on systems that ignore case).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entries {
    /// Path as written to entry.
    map: BTreeMap<String, Entry>,
    /// Identity key of a path to the spelling in `map`.
    folded: HashMap<String, String>,
    /// The look of tags, by normalized name.
    styles: BTreeMap<String, Style>,
}

fn line_of(text: &str, at: usize) -> usize {
    let end = at.min(text.len());
    1 + text[..end.min(text.len())].matches('\n').count()
}

fn parse_doc(text: &str) -> Result<DocumentMut, Problem> {
    text.parse::<DocumentMut>().map_err(|e| Problem {
        message: e.message().trim().to_owned(),
        line: e.span().map(|s| line_of(text, s.start)),
    })
}

fn strings_of(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|v| v.as_str())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn entry_of(table: &dyn TableLike) -> Entry {
    Entry {
        tags: normalize_tags(&strings_of(table.get("tags"))),
        description: table
            .get("description")
            .and_then(Item::as_str)
            .map(normalize_description)
            .unwrap_or_default(),
    }
}

impl Entries {
    /// Reads the text of a `meta.toml`. Tags and descriptions are normalized, blank entries dropped, what is not
    /// understood ignored.
    ///
    /// # Errors
    /// The text is not TOML.
    pub fn parse(text: &str) -> Result<Self, Problem> {
        let doc = parse_doc(text)?;
        let mut entries = Self::default();
        if let Some(files) = doc.get("files").and_then(Item::as_table_like) {
            for (path, item) in files.iter() {
                let Some(table) = item.as_table_like() else {
                    continue;
                };
                entries.put(path, entry_of(table));
            }
        }
        if let Some(styles) = doc.get("tag_styles").and_then(Item::as_table_like) {
            for (name, item) in styles.iter() {
                let (Some(tag), Some(table)) = (normalize_tag(name), item.as_table_like()) else {
                    continue;
                };
                let style = Style {
                    color: table
                        .get("color")
                        .and_then(Item::as_str)
                        .and_then(known_color),
                    icon: table
                        .get("icon")
                        .and_then(Item::as_str)
                        .and_then(known_icon),
                };
                if !style.is_blank() {
                    entries.styles.entry(tag).or_insert(style);
                }
            }
        }
        Ok(entries)
    }

    /// The spelling of `path` in the store: exactly like it, else one that differs only in case.
    fn locate(&self, path: &str) -> Option<String> {
        if self.map.contains_key(path) {
            return Some(path.to_owned());
        }
        self.folded.get(&key_of(path)).cloned()
    }

    fn put(&mut self, path: &str, entry: Entry) {
        if path.is_empty() {
            return;
        }
        if entry.is_blank() {
            if let Some(at) = self.locate(path) {
                self.take(&at);
            }
            return;
        }
        let at = self.locate(path).unwrap_or_else(|| path.to_owned());
        self.folded.entry(key_of(&at)).or_insert_with(|| at.clone());
        self.map.insert(at, entry);
    }

    fn take(&mut self, at: &str) -> Option<Entry> {
        let entry = self.map.remove(at)?;
        let key = key_of(at);
        if self.folded.get(&key).is_some_and(|spelled| spelled == at) {
            self.folded.remove(&key);
        }
        Some(entry)
    }

    #[must_use]
    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.locate(path).and_then(|at| self.map.get(&at))
    }

    /// Every path with its entry, in path order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Entry)> {
        self.map.iter().map(|(path, entry)| (path.as_str(), entry))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty() && self.styles.is_empty()
    }

    #[must_use]
    pub fn style(&self, tag: &str) -> Option<&Style> {
        normalize_tag(tag).and_then(|tag| self.styles.get(&tag))
    }

    /// Every styled tag, by name.
    pub fn styles(&self) -> impl Iterator<Item = (&str, &Style)> {
        self.styles.iter().map(|(tag, style)| (tag.as_str(), style))
    }

    /// Adds `entry` under `path` (from another store, its path rebased; a blank entry adds nothing; an entry already
    /// there for that path is replaced).
    pub fn insert(&mut self, path: &str, entry: Entry) {
        self.put(path, entry);
    }

    /// Gives `tag` a look when it has none (merging stores: the first one's look wins).
    pub fn insert_style(&mut self, tag: &str, style: Style) {
        if !style.is_blank() {
            self.styles.entry(tag.to_owned()).or_insert(style);
        }
    }

    /// Applies a change in memory, with the meaning [`edit_text`] gives it on the text.
    pub fn apply(&mut self, op: &Op) {
        match op {
            Op::SetTags { path, tags } => {
                let mut entry = self.get(path).cloned().unwrap_or_default();
                entry.tags = normalize_tags(tags);
                self.put(path, entry);
            }
            Op::SetDescription { path, description } => {
                let mut entry = self.get(path).cloned().unwrap_or_default();
                entry.description = normalize_description(description);
                self.put(path, entry);
            }
            Op::SetStyle { tag, color, icon } => {
                let style = Style {
                    color: *color,
                    icon: *icon,
                };
                if style.is_blank() {
                    self.styles.remove(tag);
                } else {
                    self.styles.insert(tag.clone(), style);
                }
            }
            Op::Remove { path } => {
                if let Some(at) = self.locate(path) {
                    self.take(&at);
                }
            }
            Op::Rename { from, to } => self.rename(from, to),
            Op::RenameTree { from, to } => {
                let prefix = format!("{}/", key_of(from));
                let moved: Vec<(String, String)> = self
                    .map
                    .keys()
                    .filter(|k| key_of(k).starts_with(&prefix))
                    .filter_map(|k| Some((k.clone(), tree_target(k, from, to)?)))
                    .collect();
                for (old, new) in moved {
                    self.rename(&old, &new);
                }
            }
        }
    }

    fn rename(&mut self, from: &str, to: &str) {
        let Some(at) = self.locate(from) else { return };
        let Some(moved) = self.take(&at) else { return };
        let mut entry = self.get(to).cloned().unwrap_or_default();
        for tag in moved.tags {
            if !entry.tags.contains(&tag) {
                entry.tags.push(tag);
            }
        }
        if entry.description.is_empty() {
            entry.description = moved.description;
        }
        entry.tags = normalize_tags(&entry.tags);
        self.put(to, entry);
    }
}

/// `k` (below `from`) with `from` replaced by `to`.
fn tree_target(k: &str, from: &str, to: &str) -> Option<String> {
    let rest = k.get(from.len()..)?;
    Some(format!("{to}{rest}"))
}

// ---- writing: ops applied to the text -------------------------------------------------------------------------------

fn section<'a>(doc: &'a mut DocumentMut, name: &str) -> Result<&'a mut dyn TableLike, Problem> {
    if !doc.contains_key(name) {
        let mut table = Table::new();
        table.set_implicit(true);
        doc.insert(name, Item::Table(table));
    }
    doc.get_mut(name)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| Problem::plain(format!("`{name}` is not a table")))
}

/// The key under which `path` is in `files`: exactly like it, else one that differs only in case.
fn doc_key(files: &dyn TableLike, path: &str) -> Option<String> {
    if files.contains_key(path) {
        return Some(path.to_owned());
    }
    let wanted = key_of(path);
    files
        .iter()
        .map(|(k, _)| k)
        .find(|k| key_of(k) == wanted)
        .map(str::to_owned)
}

fn table_mut<'a>(
    parent: &'a mut dyn TableLike,
    key: &str,
) -> Result<Option<&'a mut dyn TableLike>, Problem> {
    match parent.get_mut(key) {
        None => Ok(None),
        Some(item) => item
            .as_table_like_mut()
            .map(Some)
            .ok_or_else(|| Problem::plain(format!("`{key}` is not a table"))),
    }
}

/// Sets `key` keeping the decor (a trailing comment) of the value it replaces.
fn put(table: &mut dyn TableLike, key: &str, mut value: Value) {
    if let Some(old) = table.get(key).and_then(Item::as_value) {
        *value.decor_mut() = old.decor().clone();
    }
    table.insert(key, Item::Value(value));
}

fn array_of(tags: &[String]) -> Value {
    Value::Array(tags.iter().map(String::as_str).collect::<Array>())
}

/// Comments written in front of tables that were removed: kept, and put in front of the first table that is left.
type Lost = String;

fn note_prefix(item: &Item, lost: &mut Lost) {
    if let Some(prefix) = item
        .as_table()
        .and_then(|t| t.decor().prefix())
        .and_then(|p| p.as_str())
    {
        if prefix.contains('#') {
            lost.push_str(prefix);
        }
    }
}

/// Takes `key` out of `parent`; the comments that stood in front of its header are noted in `lost`.
fn forget(parent: &mut dyn TableLike, key: &str, lost: &mut Lost) -> Option<Item> {
    let item = parent.remove(key)?;
    note_prefix(&item, lost);
    Some(item)
}

/// Drops the entry `key` of `files` when nothing is left in it.
fn drop_if_blank(files: &mut dyn TableLike, key: &str, lost: &mut Lost) {
    if files
        .get(key)
        .and_then(Item::as_table_like)
        .is_some_and(TableLike::is_empty)
    {
        forget(files, key, lost);
    }
}

/// Every table with a header of the document: its position and the keys leading to it.
fn headers(table: &Table, path: &mut Vec<String>, out: &mut Vec<(isize, Vec<String>)>) {
    for (key, item) in table {
        let Some(child) = item.as_table() else {
            continue;
        };
        path.push(key.to_owned());
        if !child.is_implicit() {
            out.push((child.position().unwrap_or(isize::MAX), path.clone()));
        }
        headers(child, path, out);
        path.pop();
    }
}

/// Puts the comments of removed tables in front of the first table that is left (after the document's own text when
/// there is none), so a removal never takes the person's comments with it.
fn restore(doc: &mut DocumentMut, lost: &str) {
    if lost.is_empty() {
        return;
    }
    let mut all = Vec::new();
    headers(doc.as_table(), &mut Vec::new(), &mut all);
    let first = all.into_iter().min_by_key(|(position, _)| *position);
    let table = first.and_then(|(_, path)| {
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
        table.decor_mut().set_prefix(format!("{lost}{had}"));
    } else {
        let had = doc.trailing().as_str().unwrap_or_default().to_owned();
        doc.set_trailing(format!("{lost}{had}"));
    }
}

fn apply_doc(doc: &mut DocumentMut, op: &Op, lost: &mut Lost) -> Result<(), Problem> {
    match op {
        Op::SetTags { path, tags } => set_field(doc, path, lost, |entry| {
            if tags.is_empty() {
                entry.remove("tags");
            } else {
                put(entry, "tags", array_of(tags));
            }
        }),
        Op::SetDescription { path, description } => set_field(doc, path, lost, |entry| {
            if description.is_empty() {
                entry.remove("description");
            } else {
                put(entry, "description", Value::from(description.as_str()));
            }
        }),
        Op::SetStyle { tag, color, icon } => set_style(doc, tag, *color, *icon, lost),
        Op::Remove { path } => {
            let files = section(doc, "files")?;
            if let Some(key) = doc_key(files, path) {
                forget(files, &key, lost);
            }
            Ok(())
        }
        Op::Rename { from, to } => rename_doc(doc, from, to, lost),
        Op::RenameTree { from, to } => {
            let files = section(doc, "files")?;
            let prefix = format!("{}/", key_of(from));
            let moved: Vec<(String, String)> = files
                .iter()
                .map(|(k, _)| k)
                .filter(|k| key_of(k).starts_with(&prefix))
                .filter_map(|k| Some((k.to_owned(), tree_target(k, from, to)?)))
                .collect();
            for (old, new) in moved {
                rename_doc(doc, &old, &new, lost)?;
            }
            Ok(())
        }
    }
}

/// Runs `change` on the entry of `path` (made when `change` leaves something in it), dropping an entry left blank.
fn set_field(
    doc: &mut DocumentMut,
    path: &str,
    lost: &mut Lost,
    change: impl FnOnce(&mut dyn TableLike),
) -> Result<(), Problem> {
    let files = section(doc, "files")?;
    let key = if let Some(key) = doc_key(files, path) {
        key
    } else {
        files.insert(path, Item::Table(Table::new()));
        path.to_owned()
    };
    let Some(entry) = table_mut(files, &key)? else {
        return Ok(());
    };
    change(entry);
    drop_if_blank(files, &key, lost);
    Ok(())
}

fn set_style(
    doc: &mut DocumentMut,
    tag: &str,
    color: Option<&'static str>,
    icon: Option<&'static str>,
    lost: &mut Lost,
) -> Result<(), Problem> {
    {
        let styles = section(doc, "tag_styles")?;
        if styles.get(tag).is_none() && color.is_none() && icon.is_none() {
            return Ok(());
        }
        if styles.get(tag).is_none() {
            styles.insert(tag, Item::Table(Table::new()));
        }
        if let Some(entry) = table_mut(styles, tag)? {
            for (key, value) in [("color", color), ("icon", icon)] {
                match value {
                    Some(value) => put(entry, key, Value::from(value)),
                    None => {
                        entry.remove(key);
                    }
                }
            }
        }
        drop_if_blank(styles, tag, lost);
    }
    // an empty section leaves no trace
    if doc
        .get("tag_styles")
        .and_then(Item::as_table_like)
        .is_some_and(TableLike::is_empty)
    {
        doc.remove("tag_styles");
    }
    Ok(())
}

fn rename_doc(doc: &mut DocumentMut, from: &str, to: &str, lost: &mut Lost) -> Result<(), Problem> {
    let files = section(doc, "files")?;
    let Some(from_key) = doc_key(files, from) else {
        return Ok(());
    };
    if key_of(from) == key_of(to) {
        // only the spelling changes
        if from_key != to {
            if let Some(item) = files.remove(&from_key) {
                files.insert(to, item);
            }
        }
        return Ok(());
    }
    let Some(item) = files.remove(&from_key) else {
        return Ok(());
    };
    let Some(to_key) = doc_key(files, to) else {
        files.insert(to, item);
        return Ok(());
    };
    // merged into the target: the moved table's own comments are noted, its keys are carried over below
    note_prefix(&item, lost);
    // the target has an entry: the two are merged (tags united, the target's description wins)
    let moved = item.as_table_like().map(entry_of).unwrap_or_default();
    let Some(target) = table_mut(files, &to_key)? else {
        return Ok(());
    };
    let mut entry = entry_of(target);
    let before = entry.clone();
    for tag in moved.tags {
        if !entry.tags.contains(&tag) {
            entry.tags.push(tag);
        }
    }
    if entry.description.is_empty() {
        entry.description = moved.description;
    }
    if entry.tags != before.tags {
        put(target, "tags", array_of(&entry.tags));
    }
    if entry.description != before.description && !entry.description.is_empty() {
        put(
            target,
            "description",
            Value::from(entry.description.as_str()),
        );
    }
    Ok(())
}

/// `text` (the whole `meta.toml`; empty = no file yet) with `ops` applied. Only the touched keys change; the result
/// keeps the line endings of `text`. New entries go after the last one, in path order among themselves.
///
/// # Errors
/// `text` is not TOML, or `files` | `tag_styles` is not a table.
pub fn edit_text(text: &str, ops: &[Op]) -> Result<String, Problem> {
    let original = if text.trim().is_empty() { HEADER } else { text };
    let mut doc = parse_doc(original)?;
    // a file of only comments: `toml_edit` keeps them as the document's trailing text, which would land after the
    // first entry; they are put in front of it instead
    let lead = if doc.as_table().is_empty() {
        let lead = doc.trailing().as_str().unwrap_or_default().to_owned();
        doc.set_trailing("");
        lead
    } else {
        String::new()
    };
    // entries that do not exist yet are made in path order, so a first write is sorted
    let mut fresh: BTreeSet<&str> = BTreeSet::new();
    {
        let files = section(&mut doc, "files")?;
        for op in ops {
            let path = match op {
                Op::SetTags { path, tags } if !tags.is_empty() => path,
                Op::SetDescription { path, description } if !description.is_empty() => path,
                _ => continue,
            };
            if doc_key(files, path).is_none() {
                fresh.insert(path);
            }
        }
        for path in &fresh {
            files.insert(path, Item::Table(Table::new()));
        }
    }
    let mut lost = Lost::new();
    for op in ops {
        apply_doc(&mut doc, op, &mut lost)?;
    }
    // a made entry that ended up blank (its ops cancelled out) leaves no trace
    {
        let files = section(&mut doc, "files")?;
        for path in fresh {
            drop_if_blank(files, path, &mut lost);
        }
    }
    if doc
        .get("files")
        .and_then(Item::as_table_like)
        .is_some_and(TableLike::is_empty)
    {
        doc.remove("files");
    }
    restore(&mut doc, &lost);
    Ok(format!("{lead}{}", render(&doc, original)))
}
