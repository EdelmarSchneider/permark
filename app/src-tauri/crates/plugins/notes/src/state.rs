//! What the notes remember about a folder besides its text: the order of the pinned notes, the shared ones, the copied
//! ones, the loose files, the note that was open. Kept in the person's profile (never in the notes folder) and
//! rebuildable from the files: a missing or damaged file only means notes in the order of their last change, unpinned.
//!
//! The first time a folder is opened, the side data 0.5 kept in `<folder>/.permark/notes.json` is read once and carried
//! over. That file is never written and never required.

use std::collections::BTreeMap;
use std::path::Path;

use permark_core::notes::is_loose_id;
use permark_core::state::StateData;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How a file looked: if it looks the same now, nothing was edited since.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Stamp {
    pub size: u64,
    pub modified_ms: u64,
}

impl Stamp {
    /// The stamp of the file at `path`; `None` when it is not a file there.
    #[must_use]
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path)
            .ok()
            .filter(std::fs::Metadata::is_file)?;
        Some(Self::of_meta(&meta))
    }

    #[must_use]
    pub fn of_meta(meta: &std::fs::Metadata) -> Self {
        Self {
            size: meta.len(),
            modified_ms: modified_ms(meta),
        }
    }
}

/// Modification time in milliseconds since the epoch (0 when the system does not say).
#[must_use]
pub fn modified_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A loose file as the state keeps it (the layout of 0.5).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LooseEntry {
    pub id: String,
    /// The file's place, as it was when last seen.
    pub path: String,
    /// When it was last opened: the list shows the files by this.
    pub opened_ms: u64,
    /// How the file looked when it was last opened or saved here.
    pub stamp: Stamp,
}

/// The state of one notes folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderState {
    /// Keys of the pinned notes in the person's order: a note's file name, a loose file's `@file/<id>`.
    pub pins: Vec<String>,
    /// File names of the notes shared with agents.
    pub shared: Vec<String>,
    /// Notes copied out and how they looked then.
    pub copied: BTreeMap<String, Stamp>,
    pub loose: Vec<LooseEntry>,
    /// The key of the note that was open when the window was last closed.
    pub open: Option<String>,
    /// The side data of 0.5 was looked at (once).
    pub imported: bool,
}

impl StateData for FolderState {
    const VERSION: u32 = 1;
}

impl FolderState {
    #[must_use]
    pub fn is_pinned(&self, key: &str) -> bool {
        self.pins.iter().any(|p| p == key)
    }

    #[must_use]
    pub fn is_shared(&self, name: &str) -> bool {
        self.shared.iter().any(|s| s == name)
    }

    /// A note or loose file was renamed (`from` -> `to`): what is kept about it follows.
    pub fn rename(&mut self, from: &str, to: &str) {
        for list in [&mut self.pins, &mut self.shared] {
            for item in list.iter_mut().filter(|item| item.as_str() == from) {
                to.clone_into(item);
            }
            dedupe(list);
        }
        if let Some(stamp) = self.copied.remove(from) {
            self.copied.insert(to.to_owned(), stamp);
        }
        if self.open.as_deref() == Some(from) {
            self.open = Some(to.to_owned());
        }
    }

    /// Forgets what is kept about notes that are not there any more (`exists` says which are).
    pub fn retain_existing(&mut self, exists: impl Fn(&str) -> bool) {
        self.pins.retain(|key| exists(key));
        self.shared.retain(|name| exists(name));
        self.copied.retain(|name, _| exists(name));
        if self.open.as_deref().is_some_and(|key| !exists(key)) {
            self.open = None;
        }
    }
}

fn dedupe(list: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    list.retain(|item| seen.insert(item.clone()));
}

/// The largest `notes.json` of 0.5 read.
const MAX_LEGACY_BYTES: u64 = 4 * 1024 * 1024;

/// The side data 0.5 left in `<folder>/.permark/notes.json`, read leniently: an entry of the wrong shape is dropped, the
/// others are kept; a file that is not JSON, not an object, or of a newer version gives nothing.
#[must_use]
pub fn read_legacy(folder: &Path) -> Option<FolderState> {
    let path = folder.join(".permark").join("notes.json");
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_LEGACY_BYTES {
        return None;
    }
    let value: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    let object = value.as_object()?;
    if object.get("version").and_then(Value::as_u64).unwrap_or(1) > 1 {
        return None;
    }
    let names = |key: &str| -> Vec<String> {
        let mut list: Vec<String> = object
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|s| !s.is_empty() && s.len() <= 1024)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        dedupe(&mut list);
        list
    };
    let copied = object
        .get("copied")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(name, stamp)| Some((name.clone(), stamp_of(stamp)?)))
                .collect()
        })
        .unwrap_or_default();
    let mut loose: Vec<LooseEntry> = Vec::new();
    for item in object
        .get("loose")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(entry) = loose_of(item) else {
            continue;
        };
        if loose
            .iter()
            .any(|known| known.id == entry.id || known.path == entry.path)
        {
            continue;
        }
        loose.push(entry);
    }
    Some(FolderState {
        pins: names("pins"),
        shared: names("shared"),
        copied,
        loose,
        open: object
            .get("open")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        imported: true,
    })
}

fn stamp_of(value: &Value) -> Option<Stamp> {
    let object = value.as_object()?;
    Some(Stamp {
        size: object.get("size").and_then(Value::as_u64).unwrap_or(0),
        modified_ms: object
            .get("modifiedMs")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

fn loose_of(value: &Value) -> Option<LooseEntry> {
    let object = value.as_object()?;
    let id = object.get("id")?.as_str()?;
    let path = object.get("path")?.as_str()?;
    if !is_loose_id(id) || path.is_empty() || path.contains('\0') {
        return None;
    }
    Some(LooseEntry {
        id: id.to_owned(),
        path: path.to_owned(),
        opened_ms: object.get("openedMs").and_then(Value::as_u64).unwrap_or(0),
        stamp: object.get("stamp").and_then(stamp_of).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy(json: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".permark")).unwrap();
        std::fs::write(dir.path().join(".permark").join("notes.json"), json).unwrap();
        dir
    }

    #[test]
    fn the_real_0_5_layout_is_read() {
        let dir = legacy(
            r#"{
              "version": 1,
              "pins": ["2026-10-16 0140.md", "@file/d7d5f937"],
              "open": "@file/22dbfa5e",
              "views": { "2026-10-16 0140.md": { "mode": "visual", "caret": 18, "scroll": 0 } },
              "copied": { "2026-10-17 1536.md": { "size": 15542, "modifiedMs": 1792275678192 } },
              "cache": { "a.md": { "stamp": { "size": 1, "modifiedMs": 2 }, "title": "T", "snippet": "S" } },
              "loose": [ { "id": "d7d5f937", "path": "F:\\work\\trailer.md", "openedMs": 1792157981235,
                           "stamp": { "size": 60566, "modifiedMs": 1792157553089 } } ],
              "shared": ["2026-10-18 1855.md"]
            }"#,
        );
        let state = read_legacy(dir.path()).unwrap();
        assert_eq!(state.pins, ["2026-10-16 0140.md", "@file/d7d5f937"]);
        assert_eq!(state.shared, ["2026-10-18 1855.md"]);
        assert_eq!(state.open.as_deref(), Some("@file/22dbfa5e"));
        assert_eq!(
            state.copied["2026-10-17 1536.md"],
            Stamp {
                size: 15542,
                modified_ms: 1_792_275_678_192
            }
        );
        assert_eq!(state.loose.len(), 1);
        assert_eq!(state.loose[0].id, "d7d5f937");
        assert_eq!(state.loose[0].path, "F:\\work\\trailer.md");
        assert_eq!(state.loose[0].stamp.size, 60566);
        assert!(state.imported);
    }

    #[test]
    fn wrong_shapes_drop_their_entry_and_keep_the_rest() {
        let dir = legacy(
            r#"{ "pins": ["a.md", 3, null, "a.md", ""], "shared": "nope",
                 "copied": { "a.md": 5, "b.md": { "size": "x" } },
                 "loose": [ 7, { "id": "zz", "path": "x" }, { "id": "0a1b2c3d", "path": "" },
                            { "id": "0a1b2c3d", "path": "/x/a.md" }, { "id": "0a1b2c3d", "path": "/x/b.md" },
                            { "id": "11111111", "path": "/x/a.md" } ] }"#,
        );
        let state = read_legacy(dir.path()).unwrap();
        assert_eq!(state.pins, ["a.md"]);
        assert!(state.shared.is_empty());
        assert_eq!(state.copied.len(), 1, "{:?}", state.copied);
        assert_eq!(state.loose.len(), 1, "{:?}", state.loose);
        assert_eq!(state.loose[0].path, "/x/a.md");
    }

    #[test]
    fn hostile_files_give_nothing_and_no_error() {
        for json in [
            "",
            "not json",
            "[]",
            "null",
            "{\"version\": 2, \"pins\": [\"a.md\"]}",
            "{\"version\": 1",
        ] {
            let dir = legacy(json);
            assert!(read_legacy(dir.path()).is_none(), "{json}");
        }
        let empty = tempfile::tempdir().unwrap();
        assert!(read_legacy(empty.path()).is_none());
        // a folder in the place of the file
        std::fs::create_dir_all(empty.path().join(".permark").join("notes.json")).unwrap();
        assert!(read_legacy(empty.path()).is_none());
    }

    #[test]
    fn the_old_file_is_never_written() {
        let dir = legacy(r#"{ "pins": ["a.md"] }"#);
        let file = dir.path().join(".permark").join("notes.json");
        let before = std::fs::read(&file).unwrap();
        let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
        let _ = read_legacy(dir.path());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn a_rename_is_followed_by_every_list() {
        let mut state = FolderState {
            pins: vec!["a.md".into(), "b.md".into()],
            shared: vec!["a.md".into()],
            open: Some("a.md".into()),
            ..FolderState::default()
        };
        state.copied.insert("a.md".into(), Stamp::default());
        state.rename("a.md", "c.md");
        assert_eq!(state.pins, ["c.md", "b.md"]);
        assert_eq!(state.shared, ["c.md"]);
        assert!(state.copied.contains_key("c.md") && !state.copied.contains_key("a.md"));
        assert_eq!(state.open.as_deref(), Some("c.md"));
        // renamed onto a name already pinned: one pin
        state.rename("b.md", "c.md");
        assert_eq!(state.pins, ["c.md"]);
    }

    #[test]
    fn what_is_kept_about_gone_notes_is_forgotten() {
        let mut state = FolderState {
            pins: vec!["a.md".into(), "gone.md".into()],
            shared: vec!["gone.md".into()],
            open: Some("gone.md".into()),
            ..FolderState::default()
        };
        state.copied.insert("gone.md".into(), Stamp::default());
        state.retain_existing(|key| key == "a.md");
        assert_eq!(state.pins, ["a.md"]);
        assert!(state.shared.is_empty() && state.copied.is_empty());
        assert!(state.open.is_none());
    }
}
