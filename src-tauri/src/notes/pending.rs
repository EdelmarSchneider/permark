//! `.permark/pending/`: the text typed into a loose file that was not saved yet.
//!
//! PerMark never writes a loose file on its own, so the person's edits are kept here instead, next to the version they
//! started from. A restart finds them again and the file opens with its edits and the "unsaved" mark; closing the window
//! asks nothing. One file for each loose file, named after its id, written whole and atomically.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::util::write_atomic;

const DIR_NAME: &str = "pending";

/// The unsaved text of a loose file, and the version of the file it was typed over. Matches the UI's `PendingEdits`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PendingEdits {
    /// Where the file was, so that the edits are not lost with the list that named it.
    pub path: String,
    /// Hash of the file the text was typed over.
    pub base_hash: String,
    /// The whole text, as the editor had it.
    pub text: String,
    pub saved_ms: u64,
}

/// The folder of the unsaved edits of one notes folder.
pub(crate) struct PendingStore {
    dir: PathBuf,
}

impl PendingStore {
    pub(crate) fn new(notes_root: &Path) -> Self {
        Self {
            dir: crate::session::state_dir(notes_root).join(DIR_NAME),
        }
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Keeps `edits` for the loose file `id`, over what was kept before.
    pub(crate) fn save(&self, id: &str, edits: &PendingEdits) -> io::Result<()> {
        let bytes = serde_json::to_vec(edits).map_err(io::Error::other)?;
        write_atomic(&self.file(id), &bytes)
    }

    /// The edits kept for `id`. A file that cannot be read as edits is left where it is and reads as none.
    pub(crate) fn load(&self, id: &str) -> Option<PendingEdits> {
        let bytes = std::fs::read(self.file(id)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Forgets the edits kept for `id`.
    pub(crate) fn clear(&self, id: &str) {
        let _ = std::fs::remove_file(self.file(id));
    }

    /// The ids that have edits kept.
    pub(crate) fn ids(&self) -> BTreeSet<String> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return BTreeSet::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".json").map(str::to_owned)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edits(text: &str) -> PendingEdits {
        PendingEdits {
            path: "/home/ana/Downloads/proposal.md".to_owned(),
            base_hash: "00000000000000ab".to_owned(),
            text: text.to_owned(),
            saved_ms: 7,
        }
    }

    #[test]
    fn edits_are_kept_read_back_replaced_and_forgotten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = PendingStore::new(dir.path());
        assert_eq!(store.load("0a1b2c3d"), None);
        store.save("0a1b2c3d", &edits("one")).expect("save");
        assert_eq!(store.load("0a1b2c3d"), Some(edits("one")));
        store.save("0a1b2c3d", &edits("two")).expect("save again");
        assert_eq!(
            store.load("0a1b2c3d").map(|e| e.text),
            Some("two".to_owned())
        );
        store.save("11111111", &edits("other")).expect("save other");
        assert_eq!(
            store.ids().into_iter().collect::<Vec<_>>(),
            ["0a1b2c3d", "11111111"]
        );
        store.clear("0a1b2c3d");
        assert_eq!(store.load("0a1b2c3d"), None);
        assert_eq!(store.ids().len(), 1);
        // forgetting what is not there is fine
        store.clear("0a1b2c3d");
    }

    #[test]
    fn text_that_needs_escaping_comes_back_exactly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = PendingStore::new(dir.path());
        let text = "line one\r\n\"quoted\" \\ back\tslash\n\u{feff}Ünïcode 日本語\n";
        store.save("0a1b2c3d", &edits(text)).expect("save");
        assert_eq!(store.load("0a1b2c3d").expect("kept").text, text);
    }

    #[test]
    fn a_damaged_file_reads_as_no_edits_and_stays() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = PendingStore::new(dir.path());
        store.save("0a1b2c3d", &edits("x")).expect("save");
        std::fs::write(store.file("0a1b2c3d"), "{ not json").expect("damage");
        assert_eq!(store.load("0a1b2c3d"), None);
        assert!(store.file("0a1b2c3d").is_file());
    }
}
