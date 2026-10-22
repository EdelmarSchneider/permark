//! Copy of 0.6 state into an included project (`projects.md` §9.3, §14 D3, D49, D50): what the opened folder kept for
//! paths inside the project (unread marks + the last-visit picture, comments, silence marks) goes to the private files
//! that project uses when it is opened alone.
//!
//! - Copy, never move: the opened folder's files stay byte-identical (their entries rest unused while the project is
//!   included; back in use after it is left out).
//! - By entry, once ([`Ledger`]): an entry copied before is never copied again (removed in the project since -> stays
//!   removed); an entry the opened folder got since (written while the project was left out) is copied at the next
//!   include. The project's own entries win, except a value that still is the one copied earlier (the person changed
//!   it in the opened folder since).
//! - Private files only: tags + descriptions (`meta.toml`, shared, often committed) are never copied; the tags plugin
//!   reads the opened folder's entries for the project's files together with the project's own (D49).
//! - Every write = lock + compare-and-swap over the file as it is now (`state::shared_write`): a tab with the project
//!   open alone may be writing the same file.
//! - Any failure -> `Err`: the caller does not mark the project, the copy is tried again at the next start (idempotent).
//! - Journal + project session: not copied (the journal ages out; merged views still show it).
//!
//! The layouts are the ones 0.6.3 writes (`following.md` unread, `organizing.md` comments | quiet); read here as plain
//! JSON so unknown keys of newer layouts survive.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::paths::key_of;
use crate::plugins::PluginId;
use crate::state::{RawFile, StateData, StateStore};

const UNREAD: PluginId = PluginId::of("unread");
const READ_FILE: &str = "read.json";
const VISIT_FILE: &str = "visit.json";
const COMMENTS_FILE: &str = "comments.json";
const QUIET_FILE: &str = "quiet.json";
/// The ledger of copied entries (profile of the opened folder).
pub const LEDGER_FILE: &str = "projects-copied.json";

/// What a copy did (tests, the report).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Copied {
    /// Files written in the project's state.
    pub written: Vec<&'static str>,
}

/// The entries of the opened folder already copied into one project (by kind, path and value | comment id; hashed):
/// copied once, never again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub entries: BTreeSet<String>,
}

/// Every project's ledger, by the project's folder key (a project left out and included again may get a new uid).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerFile {
    pub members: BTreeMap<String, Ledger>,
}

impl StateData for LedgerFile {
    const VERSION: u32 = 1;
}

/// The ledger id of one entry: kind, path key (case folded where the file system is), value.
fn entry_id(kind: &str, rel: &str, value: &str) -> String {
    let text = format!("{kind}\u{1f}{}\u{1f}{value}", key_of(rel));
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(text.as_bytes()))
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

fn json_of(bytes: &[u8]) -> Option<Value> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    serde_json::from_slice(bytes).ok()
}

/// A source the opened folder wrote: unreadable now -> `Err` (retry later); not the layout -> nothing to copy.
fn source_json(file: &RawFile) -> Result<Option<Value>, String> {
    Ok(file
        .read()
        .map_err(|e| format!("{}: {e}", file.path().display()))?
        .as_deref()
        .and_then(json_of))
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
fn json_bytes(value: Value) -> Option<Vec<u8>> {
    serde_json::to_vec_pretty(&canonical(value)).ok()
}

/// Copies the opened folder's 0.6 state for `member` (root-relative folder) from `root` into `target` (the store the
/// member uses opened alone). `ledger`: what was copied into this member before (grows with what this copy took in;
/// unchanged when it fails).
///
/// # Errors
/// A source that cannot be read now, a target that is not readable as its layout, any write failure.
pub fn copy_state(
    root: &StateStore,
    target: &StateStore,
    member: &str,
    ledger: &mut Ledger,
) -> Result<Copied, String> {
    let prefix = Prefix::new(member);
    let mut copied = Copied::default();
    let mut seen = ledger.clone();
    unread(root, target, &prefix, &mut seen, &mut copied)?;
    comments(root, target, &prefix, &mut seen, &mut copied)?;
    quiet(root, target, &prefix, &mut seen, &mut copied)?;
    *ledger = seen;
    Ok(copied)
}

/// Lays `picked` (`(rel, value)`) into `target` by path key: an entry copied before is skipped; a new one goes in
/// where the project has none, or where the project's value still is one copied before. Returns whether `target`
/// changed; `added` gets the ledger ids of every entry looked at.
fn lay_map(
    target: &mut Map<String, Value>,
    picked: &[(String, Value)],
    kind: &str,
    seen: &Ledger,
    added: &mut BTreeSet<String>,
) -> bool {
    let id = |rel: &str, value: &Value| entry_id(kind, rel, &value.to_string());
    let mut changed = false;
    for (rel, value) in picked {
        let own = id(rel, value);
        if seen.entries.contains(&own) {
            continue;
        }
        added.insert(own);
        let key = key_of(rel);
        let have = target
            .iter()
            .find(|(k, _)| key_of(k) == key)
            .map(|(k, v)| (k.clone(), v.clone()));
        match have {
            None => {
                target.insert(rel.clone(), value.clone());
                changed = true;
            }
            Some((k, v)) if v != *value && seen.entries.contains(&id(&k, &v)) => {
                target.remove(&k);
                target.insert(rel.clone(), value.clone());
                changed = true;
            }
            Some(_) => {}
        }
    }
    changed
}

/// One file of the project's state, read + changed + written under its lock (`change` gets the JSON now, `None`:
/// no file; returns the JSON to write, `None`: nothing to write; `Err`: the file is not its layout).
fn update_json(
    file: &RawFile,
    mut change: impl FnMut(Option<Value>) -> Result<Option<Value>, String>,
) -> Result<bool, String> {
    let mut wrote = false;
    let mut refused = None;
    file.update(|now| {
        wrote = false;
        refused = None;
        let current = match now {
            None => None,
            Some(bytes) => {
                let Some(value) = json_of(bytes) else {
                    refused = Some(format!("{}: not readable as JSON", file.path().display()));
                    return None;
                };
                Some(value)
            }
        };
        match change(current) {
            Ok(Some(value)) => {
                let bytes = json_bytes(value)?;
                wrote = true;
                Some(bytes)
            }
            Ok(None) => None,
            Err(why) => {
                refused = Some(why);
                None
            }
        }
    })
    .map_err(|e| e.to_string())?;
    match refused {
        Some(why) => Err(why),
        None => Ok(wrote),
    }
}

/// `read.json` + `visit.json` of the `unread` plugin.
fn unread(
    root: &StateStore,
    target: &StateStore,
    prefix: &Prefix,
    seen: &mut Ledger,
    copied: &mut Copied,
) -> Result<(), String> {
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
    let read_src = source_json(&root.plugin_raw_file(&UNREAD, READ_FILE))?;
    let visit_src = source_json(&root.plugin_raw_file(&UNREAD, VISIT_FILE))?;
    let read_sel = selected(&read_src);
    let visit_sel = selected(&visit_src);
    // the member was seen from the opened folder (its documents are in the last-visit picture): its read state is
    // carried even when empty, so documents unread there stay unread (no fresh baseline)
    if !read_sel.is_empty() || (read_src.is_some() && !visit_sel.is_empty()) {
        let file = target.plugin_raw_file(&UNREAD, READ_FILE);
        let mut added = BTreeSet::new();
        let wrote = update_json(&file, |now| {
            added.clear();
            let existed = now.is_some();
            let mut value =
                now.unwrap_or_else(|| serde_json::json!({ "version": 1, "data": { "docs": {} } }));
            let docs = docs_of(&mut value, file.path())?;
            let changed = lay_map(docs, &read_sel, "r", seen, &mut added);
            Ok((changed || !existed).then_some(value))
        })?;
        seen.entries.extend(added);
        if wrote {
            copied.written.push(READ_FILE);
        }
    }
    if !visit_sel.is_empty() {
        let file = target.plugin_raw_file(&UNREAD, VISIT_FILE);
        let at_ms = visit_src
            .as_ref()
            .and_then(|v| v.get("data")?.get("at_ms")?.as_u64())
            .unwrap_or(0);
        let mut added = BTreeSet::new();
        let wrote = update_json(&file, |now| {
            added.clear();
            let existed = now.is_some();
            let mut value = now.unwrap_or_else(
                || serde_json::json!({ "version": 1, "data": { "at_ms": at_ms, "docs": {} } }),
            );
            let docs = docs_of(&mut value, file.path())?;
            let changed = lay_map(docs, &visit_sel, "v", seen, &mut added);
            Ok((changed || !existed).then_some(value))
        })?;
        seen.entries.extend(added);
        if wrote {
            copied.written.push(VISIT_FILE);
        }
    }
    Ok(())
}

/// `data.docs` of an unread file of the project (version 1; anything else is the project's own, never written over).
fn docs_of<'a>(
    value: &'a mut Value,
    path: &std::path::Path,
) -> Result<&'a mut Map<String, Value>, String> {
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(format!("{}: not the unread layout", path.display()));
    }
    value
        .get_mut("data")
        .and_then(|d| d.get_mut("docs"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{}: not the unread layout", path.display()))
}

/// `comments.json` of the `comments` plugin (by id: a comment copied before is never copied again; the project's
/// comment with the same id wins).
fn comments(
    root: &StateStore,
    target: &StateStore,
    prefix: &Prefix,
    seen: &mut Ledger,
    copied: &mut Copied,
) -> Result<(), String> {
    let Some(source) = source_json(&root.raw_file(COMMENTS_FILE))? else {
        return Ok(());
    };
    let picked: Vec<(String, Value)> = source
        .get("comments")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|c| {
                    let rel = prefix.rebase(c.get("path")?.as_str()?)?;
                    let id = c.get("id")?.as_str()?.to_owned();
                    let mut c = c.clone();
                    c.as_object_mut()?
                        .insert("path".to_owned(), Value::String(rel));
                    Some((id, c))
                })
                .filter(|(id, _)| !seen.entries.contains(&entry_id("c", "", id)))
                .collect()
        })
        .unwrap_or_default();
    if picked.is_empty() {
        return Ok(());
    }
    let file = target.raw_file(COMMENTS_FILE);
    let wrote = update_json(&file, |now| {
        let mut book = now.unwrap_or_else(|| serde_json::json!({ "version": 1, "comments": [] }));
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
        for (id, comment) in &picked {
            if !list
                .iter()
                .any(|c| c.get("id").and_then(Value::as_str) == Some(id.as_str()))
            {
                list.push(comment.clone());
            }
        }
        Ok((list.len() != before).then_some(book))
    })?;
    seen.entries
        .extend(picked.iter().map(|(id, _)| entry_id("c", "", id)));
    if wrote {
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
    seen: &mut Ledger,
    copied: &mut Copied,
) -> Result<(), String> {
    let Some(source) = source_json(&root.raw_file(QUIET_FILE))? else {
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
    let mut added = BTreeSet::new();
    let wrote = update_json(&file, |now| {
        added.clear();
        let mut marks = match now {
            None => Map::new(),
            Some(Value::Object(map)) => map,
            Some(_) => return Err(format!("{}: not the quiet layout", file.path().display())),
        };
        let changed = lay_map(&mut marks, &picked, "q", seen, &mut added);
        Ok(changed.then_some(Value::Object(marks)))
    })?;
    seen.entries.extend(added);
    if wrote {
        copied.written.push(QUIET_FILE);
    }
    Ok(())
}
