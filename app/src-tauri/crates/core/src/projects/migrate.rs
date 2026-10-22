//! One-time copy of 0.6 state into an included project (`projects.md` §9.3, §14 D3): what the opened folder kept for
//! paths inside the project (unread marks + the last-visit picture, comments, silence marks, tags + descriptions)
//! goes to the files that project uses when it is opened alone.
//!
//! - Copy, never move: the opened folder's files stay byte-identical (their entries rest unused while the project is
//!   included; back in use after it is left out).
//! - The project's own entries win; a second run writes nothing (idempotent: an interrupted copy converges).
//! - Any failure -> `Err`: the caller does not mark the project, the copy is tried again at the next start.
//! - Journal + project session: not copied (the journal ages out; merged views still show it).
//!
//! The layouts are the ones 0.6.3 writes (`following.md` unread, `organizing.md` comments | quiet | tags); read here
//! as plain JSON | TOML so unknown keys of newer layouts survive.

use std::path::Path;

use serde_json::{Map, Value};

use crate::organizing::meta::{edit_text, Entries, Op};
use crate::paths::key_of;
use crate::plugins::PluginId;
use crate::state::{RawFile, StateStore};

const UNREAD: PluginId = PluginId::of("unread");
const READ_FILE: &str = "read.json";
const VISIT_FILE: &str = "visit.json";
const COMMENTS_FILE: &str = "comments.json";
const QUIET_FILE: &str = "quiet.json";
const META_FILE: &str = "meta.toml";

/// What a copy did (tests, the report).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Copied {
    /// Files written in the project's state.
    pub written: Vec<&'static str>,
}

/// Whether `key` is the member folder `base` or inside it.
fn within(key: &str, base: &str) -> bool {
    key == base
        || key
            .strip_prefix(base)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The selection rule: root-relative `path` at | below the member -> the path relative to it.
struct Prefix {
    key: String,
    depth: usize,
}

impl Prefix {
    fn new(member: &str) -> Self {
        Self {
            key: key_of(member),
            depth: member.split('/').count(),
        }
    }

    fn rebase(&self, path: &str) -> Option<String> {
        if path.is_empty() || path.starts_with(crate::paths::WORKTREE_PREFIX) {
            return None;
        }
        within(&key_of(path), &self.key).then(|| {
            path.split('/')
                .skip(self.depth)
                .collect::<Vec<_>>()
                .join("/")
        })
    }
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn json_of(bytes: &[u8]) -> Option<Value> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    serde_json::from_slice(bytes).ok()
}

/// A source the opened folder wrote: unreadable now -> `Err` (retry later); not the layout -> nothing to copy.
fn source_json(path: &Path) -> Result<Option<Value>, String> {
    Ok(read_bytes(path)?.as_deref().and_then(json_of))
}

/// A target in the project's state: missing -> `None`; present but not the layout -> `Err` (the project's own data is
/// never written over).
fn target_json(path: &Path) -> Result<Option<Value>, String> {
    match read_bytes(path)? {
        None => Ok(None),
        Some(bytes) => json_of(&bytes)
            .map(Some)
            .ok_or_else(|| format!("{}: not readable as JSON", path.display())),
    }
}

/// The map `data.<field>` of a state envelope (`{ version: 1, data: {...} }`), as written by 0.6.
fn envelope_map<'a>(value: &'a Value, field: &str) -> Option<&'a Map<String, Value>> {
    (value.get("version")?.as_u64()? == 1)
        .then(|| value.get("data")?.get(field)?.as_object())
        .flatten()
}

/// The same value with every object's keys in sorted order, whatever map `serde_json` was built with: the bytes a
/// copy writes are the same in every build.
fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut pairs: Vec<(String, Value)> = map.into_iter().collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(pairs.into_iter().map(|(k, v)| (k, canonical(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonical).collect()),
        other => other,
    }
}

/// Pretty JSON with sorted keys (the plugins read any key order).
fn json_bytes(value: Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(&canonical(value)).map_err(|e| e.to_string())
}

fn write_json_envelope(file: &RawFile, data: &Value) -> Result<(), String> {
    let bytes = json_bytes(serde_json::json!({ "version": 1, "data": data }))?;
    file.write(&bytes).map_err(|e| e.to_string())
}

/// Copies the opened folder's 0.6 state for `member` (root-relative folder) from `root` into `target` (the store the
/// member uses opened alone).
///
/// # Errors
/// A source that cannot be read now, a target that is not readable as its layout, any write failure.
pub fn copy_state(root: &StateStore, target: &StateStore, member: &str) -> Result<Copied, String> {
    let prefix = Prefix::new(member);
    let mut copied = Copied::default();
    unread(root, target, &prefix, &mut copied)?;
    comments(root, target, &prefix, &mut copied)?;
    quiet(root, target, &prefix, &mut copied)?;
    meta(root, target, &prefix, &mut copied)?;
    Ok(copied)
}

/// `read.json` + `visit.json` of the `unread` plugin.
fn unread(
    root: &StateStore,
    target: &StateStore,
    prefix: &Prefix,
    copied: &mut Copied,
) -> Result<(), String> {
    let source_dir = root.plugin_dir(&UNREAD);
    let target_dir = target.plugin_dir(&UNREAD);
    let selected = |value: &Option<Value>| -> Vec<(String, Value)> {
        value
            .as_ref()
            .and_then(|v| envelope_map(v, "docs"))
            .map(|docs| {
                docs.iter()
                    .filter_map(|(path, hash)| Some((prefix.rebase(path)?, hash.clone())))
                    .filter(|(rel, _)| !rel.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let read_src = source_json(&source_dir.join(READ_FILE))?;
    let visit_src = source_json(&source_dir.join(VISIT_FILE))?;
    let read_sel = selected(&read_src);
    let visit_sel = selected(&visit_src);
    // the member was seen from the opened folder (its documents are in the last-visit picture): its read state is
    // carried even when empty, so documents unread there stay unread (no fresh baseline)
    if !read_sel.is_empty() || (read_src.is_some() && !visit_sel.is_empty()) {
        let path = target_dir.join(READ_FILE);
        let mut value = target_json(&path)?
            .unwrap_or_else(|| serde_json::json!({ "version": 1, "data": { "docs": {} } }));
        let docs = value
            .get_mut("data")
            .and_then(|d| d.get_mut("docs"))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("{}: not the read layout", path.display()))?;
        let existed = std::fs::symlink_metadata(&path).is_ok();
        let before = docs.len();
        for (rel, hash) in read_sel {
            let key = key_of(&rel);
            if !docs.keys().any(|k| key_of(k) == key) {
                docs.insert(rel, hash);
            }
        }
        if docs.len() != before || !existed {
            let data = value.get("data").cloned().unwrap_or(Value::Null);
            write_json_envelope(&target.plugin_raw_file(&UNREAD, READ_FILE), &data)?;
            copied.written.push(READ_FILE);
        }
    }
    if !visit_sel.is_empty() {
        let path = target_dir.join(VISIT_FILE);
        let at_ms = visit_src
            .as_ref()
            .and_then(|v| v.get("data")?.get("at_ms")?.as_u64())
            .unwrap_or(0);
        let mut value = target_json(&path)?.unwrap_or_else(
            || serde_json::json!({ "version": 1, "data": { "at_ms": at_ms, "docs": {} } }),
        );
        let docs = value
            .get_mut("data")
            .and_then(|d| d.get_mut("docs"))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("{}: not the visit layout", path.display()))?;
        let existed = std::fs::symlink_metadata(&path).is_ok();
        let before = docs.len();
        for (rel, hash) in visit_sel {
            let key = key_of(&rel);
            if !docs.keys().any(|k| key_of(k) == key) {
                docs.insert(rel, hash);
            }
        }
        if docs.len() != before || !existed {
            let data = value.get("data").cloned().unwrap_or(Value::Null);
            write_json_envelope(&target.plugin_raw_file(&UNREAD, VISIT_FILE), &data)?;
            copied.written.push(VISIT_FILE);
        }
    }
    Ok(())
}

/// `comments.json` of the `comments` plugin (pretty JSON, no trailing newline, as 0.6 writes it).
fn comments(
    root: &StateStore,
    target: &StateStore,
    prefix: &Prefix,
    copied: &mut Copied,
) -> Result<(), String> {
    let Some(source) = source_json(&root.dir().join(COMMENTS_FILE))? else {
        return Ok(());
    };
    let picked: Vec<Value> = source
        .get("comments")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|c| {
                    let rel = prefix.rebase(c.get("path")?.as_str()?)?;
                    let mut c = c.clone();
                    c.as_object_mut()?
                        .insert("path".to_owned(), Value::String(rel));
                    Some(c)
                })
                .collect()
        })
        .unwrap_or_default();
    if picked.is_empty() {
        return Ok(());
    }
    let file = target.raw_file(COMMENTS_FILE);
    let mut book = target_json(file.path())?
        .unwrap_or_else(|| serde_json::json!({ "version": 1, "comments": [] }));
    let newer = book
        .get("version")
        .and_then(Value::as_u64)
        .is_none_or(|v| v > 1);
    let list = book
        .get_mut("comments")
        .and_then(Value::as_array_mut)
        .filter(|_| !newer)
        .ok_or_else(|| format!("{}: not the comments layout", file.path().display()))?;
    let before = list.len();
    for comment in picked {
        let id = comment
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !list
            .iter()
            .any(|c| c.get("id").and_then(Value::as_str) == Some(id))
        {
            list.push(comment);
        }
    }
    if list.len() != before {
        let bytes = json_bytes(book)?;
        file.write(&bytes).map_err(|e| e.to_string())?;
        copied.written.push(COMMENTS_FILE);
    }
    Ok(())
}

/// `quiet.json` of the `quiet` plugin: marks at | below the member (marks above stay with the opened folder and keep
/// covering the member through the chain).
fn quiet(
    root: &StateStore,
    target: &StateStore,
    prefix: &Prefix,
    copied: &mut Copied,
) -> Result<(), String> {
    let Some(source) = source_json(&root.dir().join(QUIET_FILE))? else {
        return Ok(());
    };
    let picked: Vec<(String, Value)> = source
        .as_object()
        .map(|marks| {
            marks
                .iter()
                .filter(|(_, v)| matches!(v.as_str(), Some("quiet" | "speaking")))
                .filter_map(|(path, v)| Some((prefix.rebase(path)?, v.clone())))
                .collect()
        })
        .unwrap_or_default();
    if picked.is_empty() {
        return Ok(());
    }
    let file = target.raw_file(QUIET_FILE);
    let mut marks = match target_json(file.path())? {
        None => Map::new(),
        Some(Value::Object(map)) => map,
        Some(_) => return Err(format!("{}: not the quiet layout", file.path().display())),
    };
    let before = marks.len();
    for (rel, mark) in picked {
        let key = key_of(&rel);
        if !marks.keys().any(|k| key_of(k) == key) {
            marks.insert(rel, mark);
        }
    }
    if marks.len() != before {
        let bytes = json_bytes(Value::Object(marks))?;
        file.write(&bytes).map_err(|e| e.to_string())?;
        copied.written.push(QUIET_FILE);
    }
    Ok(())
}

/// The text of a `meta.toml` (BOM apart).
fn toml_text(bytes: &[u8]) -> Option<(bool, &str)> {
    let (bom, rest) = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    std::str::from_utf8(rest).ok().map(|t| (bom, t))
}

/// `meta.toml` (tags + descriptions, shared file): through the tags plugin's own surgical ops with the legacy-merge
/// rules: tags united, a description only where none, styles of the copied tags only where none.
fn meta(
    root: &StateStore,
    target: &StateStore,
    prefix: &Prefix,
    copied: &mut Copied,
) -> Result<(), String> {
    let source_file = root.shared_file(META_FILE);
    let Some(bytes) = source_file.read().map_err(|e| e.to_string())? else {
        return Ok(());
    };
    let Some(source) = toml_text(&bytes).and_then(|(_, t)| Entries::parse(t).ok()) else {
        // unreadable in the opened folder: nothing this version can copy (the tags plugin says so there)
        return Ok(());
    };
    let picked: Vec<(String, &crate::organizing::meta::Entry)> = source
        .iter()
        .filter_map(|(path, entry)| Some((prefix.rebase(path)?, entry)))
        .filter(|(rel, _)| !rel.is_empty())
        .collect();
    if picked.is_empty() {
        return Ok(());
    }
    let file = target.shared_file(META_FILE);
    let old = file.read().map_err(|e| e.to_string())?;
    let (bom, text) = match old.as_deref() {
        None => (false, ""),
        Some(bytes) => {
            toml_text(bytes).ok_or_else(|| format!("{}: not UTF-8", file.path().display()))?
        }
    };
    let current =
        Entries::parse(text).map_err(|p| format!("{}: {}", file.path().display(), p.message))?;
    let mut ops = Vec::new();
    let mut tags_seen: Vec<String> = Vec::new();
    for (rel, entry) in &picked {
        let have = current.get(rel).cloned().unwrap_or_default();
        let mut tags = have.tags.clone();
        for tag in &entry.tags {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
            if !tags_seen.contains(tag) {
                tags_seen.push(tag.clone());
            }
        }
        if tags != have.tags {
            ops.push(Op::set_tags(rel, &tags));
        }
        if have.description.is_empty() && !entry.description.is_empty() {
            ops.push(Op::set_description(rel, &entry.description));
        }
    }
    for tag in &tags_seen {
        if let (Some(style), None) = (source.style(tag), current.style(tag)) {
            ops.extend(Op::set_style(tag, style.color, style.icon));
        }
    }
    if ops.is_empty() {
        return Ok(());
    }
    let new_text = edit_text(text, &ops).map_err(|p| p.message)?;
    let mut out = Vec::with_capacity(new_text.len() + 3);
    if bom {
        out.extend_from_slice(b"\xEF\xBB\xBF");
    }
    out.extend_from_slice(new_text.as_bytes());
    match file.write_over(old.as_deref(), &out) {
        Ok(true) => {
            copied.written.push(META_FILE);
            Ok(())
        }
        Ok(false) => Err(format!("{}: changed while copying", file.path().display())),
        Err(e) => Err(e.to_string()),
    }
}
