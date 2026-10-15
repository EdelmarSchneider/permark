//! `notes.json`, in the notes folder's `.trailmark/` folder: what TrailMark remembers about the notes besides their text.
//! The order of the pinned notes, the note that was open, where the caret and the scroll were, the editor each note
//! was in, which notes were copied, and a cache of titles. Everything in it can be rebuilt from the files: a missing or
//! damaged file only means the notes come back in the order of their last change, unpinned, with the caret at the start.
//! Nothing is ever written inside a note.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::loose::{self, LooseFile};
use crate::util::write_atomic;

/// The folder next to the notes where TrailMark keeps its own data. The notes' watcher and index ignore it.
pub(crate) const STATE_DIR: &str = ".trailmark";
pub(crate) const FILE_NAME: &str = "notes.json";
/// Where a `notes.json` that cannot be read is kept before it is replaced.
const BACKUP_NAME: &str = "notes.json.bak";
const VERSION: u32 = 1;

/// The modes a note can be shown in.
const MODES: [&str; 3] = ["read", "visual", "source"];

/// Where a note was left: the editor it was in, the caret (a character offset) and the scroll position.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NoteView {
    pub mode: Option<String>,
    pub caret: Option<u64>,
    pub scroll: Option<f64>,
}

impl NoteView {
    /// The view with anything this build does not understand taken out.
    fn sanitized(mut self) -> Self {
        if !self.mode.as_deref().is_some_and(|m| MODES.contains(&m)) {
            self.mode = None;
        }
        if self.scroll.is_some_and(|s| !s.is_finite() || s < 0.0) {
            self.scroll = None;
        }
        self
    }
}

/// How a file looked when it was copied (or titled): if it looks the same now, nothing was edited since.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Stamp {
    pub size: u64,
    pub modified_ms: u64,
}

/// The title and snippet of a note as they were when its file had a given stamp, so the list is made without reading
/// every file (a note that lives in a synced folder may not be on this disk until it is read).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Cached {
    pub stamp: Stamp,
    pub title: String,
    pub snippet: String,
}

/// The content of `notes.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotesData {
    pub version: u32,
    /// The pinned notes, in the order the person chose.
    pub pins: Vec<String>,
    /// The note that was open when the window was closed.
    pub open: Option<String>,
    pub views: BTreeMap<String, NoteView>,
    /// The notes that were copied, with the file as it was then.
    pub copied: BTreeMap<String, Stamp>,
    pub cache: BTreeMap<String, Cached>,
    /// The files opened here that are not in the notes folder. Their pins, places and the open one are kept under the
    /// key of the file (`@file/<id>`), which a rename does not change.
    pub loose: Vec<LooseFile>,
    /// The notes the person shared with agents, by file name. Only these can be listed and read through an agent connection.
    pub shared: Vec<String>,
    /// The file was written by a newer version of TrailMark. It is read for what this version understands and never written
    /// over, so going back to an older version does not strip what the newer one keeps in it.
    #[serde(skip)]
    pub(crate) newer: bool,
}

/// How a `notes.json` that exists reads.
enum Health {
    Sound,
    /// Not a JSON object: it cannot be used, and saving would replace it.
    Damaged,
    /// Written by a newer version.
    Newer,
}

fn health(bytes: &[u8]) -> Health {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(root)) => {
            let version = root.get("version").and_then(Value::as_u64);
            if version.is_some_and(|v| v > u64::from(VERSION)) {
                Health::Newer
            } else {
                Health::Sound
            }
        }
        _ => Health::Damaged,
    }
}

/// Reads one field of the file, leniently: a value of the wrong shape reads as the default.
fn field<T: for<'de> Deserialize<'de> + Default>(
    root: &serde_json::Map<String, Value>,
    key: &str,
) -> T {
    root.get(key)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

/// Reads a map field, entry by entry: an entry of the wrong shape is dropped and the others are kept.
fn entries<T: for<'de> Deserialize<'de>>(
    root: &serde_json::Map<String, Value>,
    key: &str,
) -> BTreeMap<String, T> {
    let Some(Value::Object(map)) = root.get(key) else {
        return BTreeMap::new();
    };
    map.iter()
        // only objects: serde would read an empty array as a struct with all its defaults
        .filter(|(_, value)| value.is_object())
        .filter_map(|(name, value)| {
            Some((name.clone(), serde_json::from_value(value.clone()).ok()?))
        })
        .collect()
}

/// Reads the loose files, one by one: an entry that is not a file with an id and a path is dropped, and so is a second one
/// with an id (or a place) that an earlier one has.
fn loose_files(root: &serde_json::Map<String, Value>) -> Vec<LooseFile> {
    let Some(Value::Array(items)) = root.get("loose") else {
        return Vec::new();
    };
    let mut files: Vec<LooseFile> = Vec::new();
    for item in items.iter().filter(|item| item.is_object()) {
        let Ok(file) = serde_json::from_value::<LooseFile>(item.clone()) else {
            continue;
        };
        let usable = loose::is_id(&file.id) && !file.path.is_empty();
        let repeated = files
            .iter()
            .any(|known| known.id == file.id || loose::same_path(&known.path, &file.path));
        if usable && !repeated {
            files.push(file);
        }
    }
    files
}

/// Reads the names of the shared notes: only text counts, and a name is shared once.
fn shared_names(root: &serde_json::Map<String, Value>) -> Vec<String> {
    let Some(Value::Array(items)) = root.get("shared") else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for name in items.iter().filter_map(Value::as_str) {
        if !name.is_empty() && !names.iter().any(|known| known == name) {
            names.push(name.to_owned());
        }
    }
    names
}

impl NotesData {
    /// Reads the text of a `notes.json`. Whatever does not make sense is left out: the notes are listed from their files
    /// either way.
    pub(crate) fn parse(bytes: &[u8]) -> Self {
        let Ok(Value::Object(root)) = serde_json::from_slice::<Value>(bytes) else {
            return Self::default();
        };
        let mut pins: Vec<String> = field(&root, "pins");
        // a name twice would be listed twice
        let mut seen = std::collections::BTreeSet::new();
        pins.retain(|name| !name.is_empty() && seen.insert(name.clone()));
        Self {
            version: VERSION,
            pins,
            open: field::<Option<String>>(&root, "open").filter(|name| !name.is_empty()),
            views: entries::<NoteView>(&root, "views")
                .into_iter()
                .map(|(name, view)| (name, view.sanitized()))
                .collect(),
            copied: entries(&root, "copied"),
            cache: entries(&root, "cache"),
            loose: loose_files(&root),
            shared: shared_names(&root),
            newer: matches!(health(bytes), Health::Newer),
        }
    }

    /// Where the file is, for the notes folder `root`.
    pub(crate) fn path(root: &Path) -> PathBuf {
        root.join(STATE_DIR).join(FILE_NAME)
    }

    /// The data kept in the notes folder `root`, read and nothing else (the defaults when there is none or it cannot be read).
    /// What reads the notes for an agent uses this: it changes nothing in the folder.
    pub(crate) fn read(root: &Path) -> Self {
        std::fs::read(Self::path(root))
            .map_or_else(|_| Self::default(), |bytes| Self::parse(&bytes))
    }

    /// [`NotesData::read`] for the program that keeps the data. A file that is damaged is copied to `notes.json.bak` first: the
    /// next save replaces it, and what it held may still be wanted.
    pub(crate) fn load(root: &Path) -> Self {
        let path = Self::path(root);
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::default();
        };
        if !bytes.iter().all(u8::is_ascii_whitespace) && matches!(health(&bytes), Health::Damaged) {
            let _ = std::fs::write(path.with_file_name(BACKUP_NAME), &bytes);
        }
        Self::parse(&bytes)
    }

    /// Writes the data to the notes folder `root`. A file that a newer version wrote is not written over, but a note that stopped
    /// being shared must stop being shared in it too, since that file is what an agent reads: only the names this version does not
    /// share any more are taken out of its list, and everything else in it (what this version does not understand included) is
    /// left as it is.
    pub(crate) fn save(&self, root: &Path) -> io::Result<()> {
        if self.newer {
            return self.drop_unshared_names(root);
        }
        let mut data = self.clone();
        data.version = VERSION;
        let bytes = serde_json::to_vec_pretty(&data).map_err(io::Error::other)?;
        write_atomic(&Self::path(root), &bytes)
    }

    /// Takes the names that are not shared any more out of the `shared` list of a file from a newer version, and writes nothing
    /// when there is none (see [`NotesData::save`]).
    fn drop_unshared_names(&self, root: &Path) -> io::Result<()> {
        let path = Self::path(root);
        let bytes = std::fs::read(&path)?;
        let Ok(Value::Object(mut file)) = serde_json::from_slice::<Value>(&bytes) else {
            return Ok(());
        };
        let Some(Value::Array(items)) = file.get_mut("shared") else {
            return Ok(());
        };
        let before = items.len();
        // an item that is not a name is a thing of the newer version: it stays
        items.retain(|item| item.as_str().is_none_or(|name| self.is_shared(name)));
        if items.len() == before {
            return Ok(());
        }
        let bytes = serde_json::to_vec_pretty(&Value::Object(file)).map_err(io::Error::other)?;
        write_atomic(&path, &bytes)
    }

    /// Makes the data say `to` where it said `from` (a note was renamed).
    pub(crate) fn rename(&mut self, from: &str, to: &str) {
        for pin in &mut self.pins {
            if pin == from {
                to.clone_into(pin);
            }
        }
        if self.open.as_deref() == Some(from) {
            self.open = Some(to.to_owned());
        }
        if let Some(view) = self.views.remove(from) {
            self.views.insert(to.to_owned(), view);
        }
        if let Some(copied) = self.copied.remove(from) {
            self.copied.insert(to.to_owned(), copied);
        }
        // a note the person shared stays shared when they give it a name
        for name in &mut self.shared {
            if name == from {
                to.clone_into(name);
            }
        }
        self.cache.remove(from);
    }

    /// Forgets what is kept about notes that are not in `existing` (called when a notes folder is opened). What is kept
    /// under the key of a loose file stays as long as the file is listed.
    pub(crate) fn retain_existing(&mut self, existing: &std::collections::BTreeSet<String>) {
        let listed: std::collections::BTreeSet<String> =
            self.loose.iter().map(LooseFile::key).collect();
        let known = |name: &String| existing.contains(name) || listed.contains(name);
        self.pins.retain(|name| known(name));
        if self.open.as_ref().is_some_and(|name| !known(name)) {
            self.open = None;
        }
        self.views.retain(|name, _| known(name));
        self.copied.retain(|name, _| existing.contains(name));
        self.cache.retain(|name, _| existing.contains(name));
        // a name that is not a note any more is not shared: a note made later under that name must not be
        self.shared.retain(|name| existing.contains(name));
    }

    pub(crate) fn is_shared(&self, name: &str) -> bool {
        self.shared.iter().any(|shared| shared == name)
    }

    /// Forgets what is kept under the key of a loose file that is not listed any more.
    pub(crate) fn forget_loose(&mut self, key: &str) {
        self.pins.retain(|pin| pin != key);
        if self.open.as_deref() == Some(key) {
            self.open = None;
        }
        self.views.remove(key);
    }

    pub(crate) fn is_pinned(&self, name: &str) -> bool {
        self.pins.iter().any(|pin| pin == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NotesData {
        let mut data = NotesData {
            pins: vec!["b.md".to_owned(), "a.md".to_owned()],
            open: Some("a.md".to_owned()),
            shared: vec!["a.md".to_owned(), "b.md".to_owned()],
            ..NotesData::default()
        };
        data.views.insert(
            "a.md".to_owned(),
            NoteView {
                mode: Some("source".to_owned()),
                caret: Some(42),
                scroll: Some(120.5),
            },
        );
        data.copied.insert(
            "b.md".to_owned(),
            Stamp {
                size: 10,
                modified_ms: 99,
            },
        );
        data.cache.insert(
            "a.md".to_owned(),
            Cached {
                stamp: Stamp {
                    size: 5,
                    modified_ms: 7,
                },
                title: "A".to_owned(),
                snippet: "s".to_owned(),
            },
        );
        data.loose.push(LooseFile {
            id: "0a1b2c3d".to_owned(),
            path: "/home/ana/Downloads/proposal.md".to_owned(),
            opened_ms: 50,
            stamp: Stamp {
                size: 9,
                modified_ms: 8,
            },
        });
        data
    }

    #[test]
    fn the_data_round_trips_through_the_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut data = sample();
        data.save(dir.path()).expect("save");
        data.version = VERSION;
        assert_eq!(NotesData::load(dir.path()), data);
        assert!(dir.path().join(".trailmark").join("notes.json").is_file());
    }

    #[test]
    fn a_missing_file_is_the_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(NotesData::load(dir.path()), NotesData::default());
    }

    #[test]
    fn a_broken_file_is_the_defaults_and_is_replaced_on_the_next_save() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = NotesData::path(dir.path());
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        for broken in ["{{{ not json", "", "[1, 2]", "\"text\"", "null"] {
            std::fs::write(&path, broken).expect("write");
            assert_eq!(
                NotesData::load(dir.path()),
                NotesData::default(),
                "{broken}"
            );
        }
        sample().save(dir.path()).expect("save");
        assert_eq!(NotesData::load(dir.path()).pins, ["b.md", "a.md"]);
    }

    #[test]
    fn a_damaged_file_is_kept_as_a_copy_before_the_next_save_replaces_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = NotesData::path(dir.path());
        let backup = path.with_file_name("notes.json.bak");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "{\"pins\": [\"a.md\", ").expect("write");
        assert_eq!(NotesData::load(dir.path()), NotesData::default());
        assert_eq!(
            std::fs::read(&backup).expect("a copy"),
            b"{\"pins\": [\"a.md\", "
        );
        // the next save replaces the file and leaves the copy alone
        sample().save(dir.path()).expect("save");
        assert_eq!(NotesData::load(dir.path()).pins, ["b.md", "a.md"]);
        assert_eq!(
            std::fs::read(&backup).expect("still there"),
            b"{\"pins\": [\"a.md\", "
        );
        // a file with nothing in it has nothing worth a copy
        std::fs::remove_file(&backup).expect("rm");
        std::fs::write(&path, "  \n").expect("write");
        assert_eq!(NotesData::load(dir.path()), NotesData::default());
        assert!(!backup.exists());
    }

    #[test]
    fn a_file_from_a_newer_version_is_read_and_never_written_over() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = NotesData::path(dir.path());
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let newer = br#"{"version": 2, "pins": ["a.md"], "folders": {"work": ["a.md"]}}"#;
        std::fs::write(&path, newer).expect("write");
        let mut data = NotesData::load(dir.path());
        assert_eq!(data.pins, ["a.md"], "what this version understands is used");
        data.pins.push("b.md".to_owned());
        data.save(dir.path()).expect("saving is not an error");
        assert_eq!(
            std::fs::read(&path).expect("read"),
            newer,
            "the newer version's file is as it was"
        );
        assert!(!path.with_file_name("notes.json.bak").exists());
        // and the version this one writes is written over its own
        std::fs::write(&path, br#"{"version": 1, "pins": ["a.md"]}"#).expect("write");
        let mut own = NotesData::load(dir.path());
        own.pins.push("c.md".to_owned());
        own.save(dir.path()).expect("save");
        assert_eq!(NotesData::load(dir.path()).pins, ["a.md", "c.md"]);
    }

    #[test]
    fn what_does_not_make_sense_is_dropped_and_the_rest_is_kept() {
        let data = NotesData::parse(
            br#"{
              "pins": ["a.md", "", "a.md", "b.md"],
              "open": 7,
              "views": { "a.md": {"mode": "sepia", "caret": 3, "scroll": -4},
                         "b.md": "oops",
                         "c.md": {"mode": "read", "caret": "x"} },
              "copied": { "a.md": {"size": 1, "modifiedMs": 2}, "b.md": [] },
              "cache": 5,
              "future": {"anything": true}
            }"#,
        );
        assert_eq!(data.pins, ["a.md", "b.md"]);
        assert_eq!(data.open, None);
        assert_eq!(
            data.views.get("a.md"),
            Some(&NoteView {
                mode: None,
                caret: Some(3),
                scroll: None
            })
        );
        assert!(!data.views.contains_key("b.md"));
        assert!(
            !data.views.contains_key("c.md"),
            "a caret that is not a number drops the entry"
        );
        assert_eq!(data.copied.len(), 1);
        assert!(data.cache.is_empty());
    }

    #[test]
    fn a_renamed_note_keeps_what_was_kept_for_it() {
        let mut data = sample();
        data.rename("a.md", "z.md");
        assert_eq!(data.pins, ["b.md", "z.md"]);
        assert_eq!(data.shared, ["z.md", "b.md"], "a shared note stays shared");
        assert_eq!(data.open.as_deref(), Some("z.md"));
        assert!(data.views.contains_key("z.md") && !data.views.contains_key("a.md"));
        assert!(
            !data.cache.contains_key("a.md"),
            "the cache is rebuilt from the file"
        );
    }

    #[test]
    fn notes_that_are_gone_are_forgotten() {
        let mut data = sample();
        let existing = ["a.md".to_owned()].into_iter().collect();
        data.retain_existing(&existing);
        assert_eq!(data.pins, ["a.md"]);
        assert!(data.copied.is_empty());
        assert!(data.is_pinned("a.md") && !data.is_pinned("b.md"));
        // a note that is gone is not shared: another one made under its name must not be
        assert!(data.is_shared("a.md") && !data.is_shared("b.md"));
    }

    #[test]
    fn the_shared_names_are_read_as_text_and_once() {
        let data = NotesData::parse(br#"{"shared": ["a.md", "", 7, "a.md", "b.md", null]}"#);
        assert_eq!(data.shared, ["a.md", "b.md"]);
        assert_eq!(NotesData::parse(br#"{"shared": "a.md"}"#).shared.len(), 0);
        assert_eq!(NotesData::parse(b"{}").shared.len(), 0);
    }

    #[test]
    fn what_is_kept_for_a_loose_file_stays_while_it_is_listed_and_goes_when_it_is_not() {
        let mut data = sample();
        data.pins.push("@file/0a1b2c3d".to_owned());
        data.pins.push("@file/ffffffff".to_owned());
        data.open = Some("@file/0a1b2c3d".to_owned());
        data.views
            .insert("@file/0a1b2c3d".to_owned(), NoteView::default());
        data.retain_existing(&std::collections::BTreeSet::new());
        assert_eq!(
            data.pins,
            ["@file/0a1b2c3d"],
            "a file that is not listed has no pin"
        );
        assert_eq!(data.open.as_deref(), Some("@file/0a1b2c3d"));
        assert!(data.views.contains_key("@file/0a1b2c3d"));
        data.forget_loose("@file/0a1b2c3d");
        assert!(data.pins.is_empty() && data.open.is_none() && data.views.is_empty());
    }

    #[test]
    fn loose_files_are_read_one_by_one_and_what_is_wrong_with_one_drops_only_it() {
        let data = NotesData::parse(
            br#"{
              "loose": [
                {"id": "0a1b2c3d", "path": "/a/one.md", "openedMs": 5, "stamp": {"size": 1, "modifiedMs": 2}},
                {"id": "0a1b2c3d", "path": "/a/again.md"},
                {"id": "1b2c3d4e", "path": "/a/one.md"},
                {"id": "NOT-AN-ID", "path": "/a/two.md"},
                {"id": "2c3d4e5f", "path": ""},
                {"id": "3d4e5f60", "path": "/a/three.md", "openedMs": "yesterday"},
                "text",
                {"id": "4e5f6071", "path": "/a/four.md"}
              ]
            }"#,
        );
        let ids: Vec<&str> = data.loose.iter().map(|file| file.id.as_str()).collect();
        assert_eq!(ids, ["0a1b2c3d", "4e5f6071"]);
        assert_eq!(data.loose[0].opened_ms, 5);
        assert_eq!(NotesData::parse(br#"{"loose": 5}"#).loose.len(), 0);
    }
}
