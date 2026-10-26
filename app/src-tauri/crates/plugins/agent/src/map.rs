//! `map`: the files below a folder of the project, by path (no ranking), each with its approximate size and the facts
//! the person gave it (tags, one-line description), unread | changed since the last visit when the person shares
//! changes; folders past the depth are counted (files, size). Hidden paths are not there.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, PoisonError};
use std::time::UNIX_EPOCH;

use permark_core::fsx;
use permark_core::organizing::{TagsApi, UnreadApi};
use permark_core::paths::VaultPath;
use serde_json::{json, Map, Value};

use crate::guard::Guard;
use crate::structure;
use crate::tools::{Tools, MAX_READ_BYTES, READY_WAIT};
use crate::ToolError;

/// Deepest and default depth.
pub const DEPTH: (usize, usize) = (6, 2);
/// Most files listed, and the default.
pub const LIMIT: (usize, usize) = (500, 200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    Tokens(u64),
    Binary,
    Large,
}

/// Sizes measured before: path -> (bytes, modified ms, size). A file that changed is measured again.
#[derive(Default)]
pub struct SizeCache(Mutex<HashMap<String, (u64, u64, Size)>>);

fn size_of(cache: &SizeCache, path: &str, abs: &std::path::Path) -> Option<Size> {
    let meta = std::fs::metadata(abs).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let bytes = meta.len();
    let mut cache = cache.0.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((b, m, size)) = cache.get(path) {
        if *b == bytes && *m == modified {
            return Some(*size);
        }
    }
    let size = if bytes > MAX_READ_BYTES {
        Size::Large
    } else {
        match fsx::read_text(abs, MAX_READ_BYTES) {
            Ok(decoded) => Size::Tokens(structure::file_size(&decoded.text)),
            Err(fsx::FsxError::TooLarge { .. }) => Size::Large,
            Err(_) => Size::Binary,
        }
    };
    cache.insert(path.to_owned(), (bytes, modified, size));
    Some(size)
}

/// The folder an agent named (`""`: the project root).
fn folder_of(tools: &Tools, guard: &Guard, raw: Option<&str>) -> Result<String, ToolError> {
    let raw = raw.map(str::trim).unwrap_or_default();
    let trimmed = raw.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || trimmed == "." {
        return Ok(String::new());
    }
    let missing = || ToolError::NotFound(raw.to_owned());
    let path = VaultPath::parse(trimmed).map_err(|_| missing())?;
    if path.is_root() {
        return Ok(String::new());
    }
    if guard.hidden(path.as_str(), true) {
        return Err(missing());
    }
    let abs = tools.vault().resolve(&path).map_err(|_| missing())?;
    if !abs.is_dir() {
        return Err(missing());
    }
    Ok(path.as_str().to_owned())
}

/// The `map` answer.
///
/// # Errors
/// `NotFound` for a folder that is not there (or hidden).
#[allow(clippy::too_many_lines)] // one walk: folder, files, cut folders, facts per file
pub fn map(
    tools: &Tools,
    cache: &SizeCache,
    guard: &Guard,
    path: Option<&str>,
    depth: Option<usize>,
    limit: Option<usize>,
    shares_changes: bool,
) -> Result<Value, ToolError> {
    let folder = folder_of(tools, guard, path)?;
    let depth = depth.unwrap_or(DEPTH.1).clamp(1, DEPTH.0);
    let max = limit.unwrap_or(LIMIT.1).clamp(1, LIMIT.0);
    let index = &tools.services().search;
    let ready = index.wait_ready(READY_WAIT);
    let prefix = if folder.is_empty() {
        String::new()
    } else {
        format!("{folder}/")
    };
    let mut listed: Vec<String> = Vec::new();
    // folder past the depth -> (files, bytes)
    let mut cut: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for file in index.files_under(&folder) {
        if guard.hidden(&file, false) {
            continue;
        }
        let rel = file.strip_prefix(&prefix).unwrap_or(&file);
        let segments: Vec<&str> = rel.split('/').collect();
        if segments.len() <= depth {
            listed.push(file);
        } else {
            let dir = format!("{prefix}{}/", segments[..depth].join("/"));
            let bytes = VaultPath::parse(&file)
                .ok()
                .and_then(|p| tools.vault().resolve(&p).ok())
                .and_then(|abs| std::fs::metadata(abs).ok())
                .map_or(0, |m| m.len());
            let entry = cut.entry(dir).or_default();
            entry.0 += 1;
            entry.1 += bytes;
        }
    }
    let total = listed.len();
    listed.truncate(max);
    let tags = tools.services().services.get::<TagsApi>();
    let marks = shares_changes
        .then(|| tools.services().services.get::<UnreadApi>())
        .flatten();
    let (unread_set, changed_set): (HashSet<String>, HashSet<String>) = marks.map_or_else(
        || (HashSet::new(), HashSet::new()),
        |api| {
            let unread = api.0.unread().paths.into_iter().collect();
            let changed = api
                .0
                .changed_since_visit()
                .unwrap_or_default()
                .into_iter()
                .collect();
            (unread, changed)
        },
    );
    let files: Vec<Value> = listed
        .iter()
        .map(|path| {
            let mut row = Map::new();
            row.insert("path".to_owned(), json!(path));
            let abs = VaultPath::parse(path)
                .ok()
                .and_then(|p| tools.vault().resolve(&p).ok());
            match abs.and_then(|abs| size_of(cache, path, &abs)) {
                Some(Size::Tokens(n)) => {
                    row.insert("size_tok".to_owned(), json!(n));
                }
                Some(Size::Binary) => {
                    row.insert("binary".to_owned(), json!(true));
                }
                Some(Size::Large) => {
                    row.insert("large".to_owned(), json!(true));
                }
                None => {}
            }
            if unread_set.contains(path) {
                row.insert("unread".to_owned(), json!(true));
            }
            if changed_set.contains(path) {
                row.insert("changed".to_owned(), json!(true));
            }
            if let Some(api) = &tags {
                let list = api.0.tags_of(path);
                if !list.is_empty() {
                    row.insert("tags".to_owned(), json!(list));
                }
                let description = api.0.description_of(path);
                if !description.trim().is_empty() {
                    row.insert("description".to_owned(), json!(description.trim()));
                }
            }
            Value::Object(row)
        })
        .collect();
    let dirs_cut: Vec<Value> = cut
        .into_iter()
        // past the depth only bytes are known (chars / 4): reading every file would cost the time saved
        .map(|(dir, (files, bytes))| json!({ "path": dir, "files": files, "size_tok": bytes.div_ceil(4) }))
        .collect();
    let mut answer = json!({
        "path": if folder.is_empty() { ".".to_owned() } else { folder },
        "depth": depth,
        "files": files,
        "total": total,
        "more": total.saturating_sub(max),
    });
    if !dirs_cut.is_empty() {
        answer["dirs_cut"] = json!(dirs_cut);
    }
    if !ready {
        answer["partial"] = json!(true);
    }
    Ok(answer)
}
