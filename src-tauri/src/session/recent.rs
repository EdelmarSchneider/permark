//! The global "recent folders" list shown on the welcome screen.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::SessionError;
use crate::util::write_atomic;
use crate::vault::key;

/// The list never grows beyond this.
pub const MAX_RECENT: usize = 15;
const FILE_NAME: &str = "recent.json";

/// Matches the UI's `RecentVault`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentVault {
    pub root: String,
    pub name: String,
    pub last_opened_ms: u64,
}

pub struct RecentStore {
    file: PathBuf,
}

impl RecentStore {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            file: app_data_dir.join(FILE_NAME),
        }
    }

    /// Stored list, newest first (empty when missing or unreadable).
    pub fn load(&self) -> Vec<RecentVault> {
        std::fs::read(&self.file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Like [`RecentStore::load`], without folders that no longer exist.
    pub fn load_existing(&self) -> Vec<RecentVault> {
        self.load()
            .into_iter()
            .filter(|r| Path::new(&r.root).exists())
            .collect()
    }

    /// Moves (or adds) a folder to the top and saves the list.
    pub fn touch(
        &self,
        root: &str,
        name: &str,
        now_ms: u64,
    ) -> Result<Vec<RecentVault>, SessionError> {
        let mut list = self.load();
        list.retain(|r| key(&r.root) != key(root));
        list.insert(
            0,
            RecentVault {
                root: root.to_owned(),
                name: name.to_owned(),
                last_opened_ms: now_ms,
            },
        );
        list.truncate(MAX_RECENT);
        let bytes =
            serde_json::to_vec_pretty(&list).map_err(|e| SessionError::Json(e.to_string()))?;
        write_atomic(&self.file, &bytes).map_err(|e| SessionError::io(&self.file, e))?;
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_and_deduplicated() {
        let d = tempfile::tempdir().expect("tempdir");
        let s = RecentStore::new(d.path());
        assert_eq!(s.load().len(), 0);
        s.touch("/a", "a", 1).expect("touch");
        s.touch("/b", "b", 2).expect("touch");
        let list = s.touch("/a", "a", 3).expect("touch");
        let roots: Vec<&str> = list.iter().map(|r| r.root.as_str()).collect();
        assert_eq!(roots, ["/a", "/b"]);
        assert_eq!(list[0].last_opened_ms, 3);
        assert_eq!(s.load(), list);
    }

    #[test]
    fn capped_at_fifteen() {
        let d = tempfile::tempdir().expect("tempdir");
        let s = RecentStore::new(d.path());
        for i in 0..20 {
            s.touch(&format!("/p{i}"), "p", i).expect("touch");
        }
        let list = s.load();
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0].root, "/p19");
        assert_eq!(list[MAX_RECENT - 1].root, "/p5");
    }

    #[test]
    fn load_existing_drops_folders_that_are_gone() {
        let d = tempfile::tempdir().expect("tempdir");
        let keep = d.path().join("keep");
        std::fs::create_dir(&keep).expect("mkdir");
        let s = RecentStore::new(d.path());
        s.touch(&keep.to_string_lossy(), "keep", 1).expect("touch");
        s.touch(&d.path().join("gone").to_string_lossy(), "gone", 2)
            .expect("touch");
        let list = s.load_existing();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "keep");
        // The stored list itself keeps both (the folder may come back, e.g. a removable drive).
        assert_eq!(s.load().len(), 2);
    }

    #[test]
    fn corrupt_file_reads_as_empty_and_is_replaced_on_write() {
        let d = tempfile::tempdir().expect("tempdir");
        std::fs::write(d.path().join(FILE_NAME), "{{{ not json").expect("write");
        let s = RecentStore::new(d.path());
        assert_eq!(s.load().len(), 0);
        assert_eq!(s.touch("/a", "a", 1).expect("touch").len(), 1);
    }

    #[test]
    fn serializes_to_the_ui_contract() {
        let r = RecentVault {
            root: "/a".to_owned(),
            name: "a".to_owned(),
            last_opened_ms: 7,
        };
        assert_eq!(
            serde_json::to_value(&r).expect("json"),
            serde_json::json!({"root": "/a", "name": "a", "lastOpenedMs": 7})
        );
    }
}
