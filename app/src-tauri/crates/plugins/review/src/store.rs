//! The copies of the versions the person read: text kept in the plugin's state folder, one file for each, named for
//! the hash of the bytes that were read (16 hex digits). The name tells the content, so a version two documents share
//! is kept once, and a copy that was damaged is found out when it is read (its own check).

use std::collections::HashSet;
use std::path::PathBuf;

use permark_core::fsx;
use permark_core::plugins::PluginId;
use permark_core::state::{Loaded, StateData, StateFile, StateStore};
use serde::{Deserialize, Serialize};

/// A copy larger than this is not kept.
pub const MAX_COPY_BYTES: usize = 2 * 1024 * 1024;

/// The text of one version (line breaks as `\n`) and a check of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CopyFile {
    pub text: String,
    pub check: String,
}

impl StateData for CopyFile {
    const VERSION: u32 = 1;
}

fn check_of(text: &str) -> String {
    fsx::hash(text.as_bytes()).hex()
}

#[derive(Clone)]
pub struct CopyStore {
    store: StateStore,
    id: PluginId,
}

impl CopyStore {
    #[must_use]
    pub fn new(store: StateStore, id: PluginId) -> Self {
        Self { store, id }
    }

    fn file(&self, hash: u64) -> StateFile<CopyFile> {
        self.store.plugin_file(
            &self.id,
            &format!("copies/{}.json", fsx::ContentHash(hash).hex()),
        )
    }

    fn folder(&self) -> PathBuf {
        self.store.plugin_dir(&self.id).join("copies")
    }

    #[must_use]
    pub fn has(&self, hash: u64) -> bool {
        self.file(hash).path().is_file()
    }

    /// The text of a version; `None`: no copy, or a damaged one.
    #[must_use]
    pub fn get(&self, hash: u64) -> Option<String> {
        match self.file(hash).load() {
            Loaded::Found(copy) if copy.check == check_of(&copy.text) => Some(copy.text),
            Loaded::Found(_) | Loaded::Missing | Loaded::Recovered { .. } => None,
        }
    }

    /// Keeps the text under `hash` unless it is there already or too large. Returns whether the copy exists.
    #[must_use]
    pub fn put(&self, hash: u64, text: &str) -> bool {
        if text.len() > MAX_COPY_BYTES {
            return false;
        }
        if self.has(hash) {
            return true;
        }
        self.file(hash)
            .save(&CopyFile {
                text: text.to_owned(),
                check: check_of(text),
            })
            .is_ok()
    }

    /// How many copies there are.
    #[must_use]
    pub fn count(&self) -> usize {
        std::fs::read_dir(self.folder()).map_or(0, |entries| entries.filter_map(Result::ok).count())
    }

    /// Deletes the copies no version in `keep` needs. Returns how many went.
    pub fn remove_unused(&self, keep: &HashSet<u64>) -> usize {
        let Ok(entries) = std::fs::read_dir(self.folder()) else {
            return 0;
        };
        let mut removed = 0;
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".json") else {
                continue;
            };
            let Some(hash) = permark_plugin_unread::hash_from_hex(stem) else {
                continue;
            };
            if !keep.contains(&hash) && std::fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use permark_core::paths::Root;
    use permark_core::state::AppDirs;

    use super::*;

    fn store(base: &std::path::Path, project: &std::path::Path) -> CopyStore {
        CopyStore::new(
            StateStore::for_project(&AppDirs::under(base), &Root::open(project).unwrap()),
            PluginId::of("review"),
        )
    }

    #[test]
    fn copies_are_kept_once_read_back_and_checked() {
        let base = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let copies = store(base.path(), project.path());
        assert!(!copies.has(7) && copies.get(7).is_none());
        assert!(copies.put(7, "one\ntwo ü\n"));
        assert!(copies.put(7, "one\ntwo ü\n"), "again: no harm");
        assert_eq!(copies.get(7).as_deref(), Some("one\ntwo ü\n"));
        assert_eq!(copies.count(), 1);
        // a damaged copy is not believed
        let path = copies.file(7).path().to_path_buf();
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("two", "TWO");
        std::fs::write(&path, text).unwrap();
        assert_eq!(copies.get(7), None);
        // too large is not kept
        assert!(!copies.put(8, &"x".repeat(MAX_COPY_BYTES + 1)));
        assert!(!copies.has(8));
    }

    #[test]
    fn unused_copies_go_and_stray_files_stay() {
        let base = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let copies = store(base.path(), project.path());
        for hash in [1_u64, 2, 3] {
            assert!(copies.put(hash, "t"));
        }
        std::fs::write(copies.folder().join("notes.txt"), "mine").unwrap();
        std::fs::write(copies.folder().join("zz.json"), "{}").unwrap();
        assert_eq!(copies.remove_unused(&HashSet::from([2_u64])), 2);
        assert!(copies.has(2) && !copies.has(1) && !copies.has(3));
        assert!(copies.folder().join("notes.txt").exists());
        assert!(copies.folder().join("zz.json").exists());
        assert_eq!(
            CopyStore::remove_unused(
                &store(base.path(), tempfile::tempdir().unwrap().path()),
                &HashSet::new()
            ),
            0
        );
    }
}
