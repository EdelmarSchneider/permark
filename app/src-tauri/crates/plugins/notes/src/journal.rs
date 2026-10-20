//! The journal of unsaved text: what the editor holds that is not in the file yet, kept in the person's profile every
//! couple of seconds, so a crash, a power cut or a window that was killed loses nothing.
//!
//! One file per document (a note's file name, a loose file's `@file/<id>`), written whole and atomically. The window
//! removes the entry when the text is on disk. An entry found whose text differs from the file is text the person typed
//! and never saw saved: the note is marked recovered (a loose file: unsaved) until the person chooses.

use std::io;
use std::path::PathBuf;

use permark_core::fsx;
use permark_core::paths::fold;
use serde::{Deserialize, Serialize};

/// The largest text journaled (the editor refuses to open bigger files).
pub const MAX_BYTES: usize = 5 * 1024 * 1024;

/// The unsaved text of one document and the version of the file it was typed over.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Entry {
    /// What the notes call the document: a note's file name, a loose file's `@file/<id>`.
    pub key: String,
    /// Hash of the file the text was typed over.
    pub base_hash: String,
    /// The whole text, as the editor had it.
    pub text: String,
    pub saved_ms: u64,
}

#[derive(Debug, Clone)]
pub struct Journal {
    dir: PathBuf,
}

impl Journal {
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn file(&self, key: &str) -> PathBuf {
        self.dir
            .join(format!("{}.json", fsx::hash(fold(key).as_bytes()).hex()))
    }

    /// Keeps `entry` over what was kept before.
    ///
    /// # Errors
    /// The entry is too big | could not be written.
    pub fn save(&self, entry: &Entry) -> io::Result<()> {
        if entry.text.len() > MAX_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "too large"));
        }
        let bytes = serde_json::to_vec(entry).map_err(io::Error::other)?;
        std::fs::create_dir_all(&self.dir)?;
        fsx::write_atomic(&self.file(&entry.key), &bytes)
    }

    /// The entry kept for `key`. A file that cannot be read as an entry reads as none (and is left where it is).
    #[must_use]
    pub fn load(&self, key: &str) -> Option<Entry> {
        let bytes = std::fs::read(self.file(key)).ok()?;
        let entry: Entry = serde_json::from_slice(&bytes).ok()?;
        (fold(&entry.key) == fold(key)).then_some(entry)
    }

    /// Forgets the entry kept for `key`.
    pub fn clear(&self, key: &str) {
        let _ = std::fs::remove_file(self.file(key));
    }

    /// Every entry kept (unreadable files are skipped).
    #[must_use]
    pub fn all(&self) -> Vec<Entry> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|e| e == "json"))
            .filter_map(|entry| {
                let bytes = std::fs::read(entry.path()).ok()?;
                serde_json::from_slice::<Entry>(&bytes)
                    .ok()
                    .filter(|e| !e.key.is_empty())
            })
            .collect()
    }

    /// The entry of `from` is kept under `to` (a note was renamed).
    pub fn rename(&self, from: &str, to: &str) {
        if let Some(mut entry) = self.load(from) {
            self.clear(from);
            to.clone_into(&mut entry.key);
            let _ = self.save(&entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> (tempfile::TempDir, Journal) {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::new(dir.path().join("journal"));
        (dir, journal)
    }

    fn entry(key: &str, text: &str) -> Entry {
        Entry {
            key: key.to_owned(),
            base_hash: "0123456789abcdef".to_owned(),
            text: text.to_owned(),
            saved_ms: 7,
        }
    }

    #[test]
    fn an_entry_is_kept_read_back_replaced_and_cleared() {
        let (_dir, journal) = journal();
        assert!(journal.load("a.md").is_none());
        journal.save(&entry("a.md", "first")).unwrap();
        assert_eq!(journal.load("a.md").unwrap().text, "first");
        journal.save(&entry("a.md", "second")).unwrap();
        assert_eq!(journal.load("a.md").unwrap().text, "second");
        assert_eq!(journal.all().len(), 1);
        journal.clear("a.md");
        assert!(journal.load("a.md").is_none());
        // clearing what is not there is fine
        journal.clear("a.md");
    }

    #[test]
    fn entries_are_per_document_and_keep_any_text() {
        let (_dir, journal) = journal();
        journal.save(&entry("a.md", "A\r\nline\u{feff}")).unwrap();
        journal.save(&entry("@file/0a1b2c3d", "日本語 😀")).unwrap();
        assert_eq!(journal.all().len(), 2);
        assert_eq!(journal.load("a.md").unwrap().text, "A\r\nline\u{feff}");
        assert_eq!(journal.load("@file/0a1b2c3d").unwrap().text, "日本語 😀");
    }

    #[test]
    fn a_damaged_file_reads_as_nothing_and_is_not_removed() {
        let (dir, journal) = journal();
        journal.save(&entry("a.md", "x")).unwrap();
        let path = std::fs::read_dir(dir.path().join("journal"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(journal.load("a.md").is_none());
        assert!(journal.all().is_empty());
        assert!(path.exists());
        // a file named for another document is not this one's entry
        std::fs::write(&path, serde_json::to_vec(&entry("other.md", "y")).unwrap()).unwrap();
        assert!(journal.load("a.md").is_none());
    }

    #[test]
    fn a_rename_moves_the_entry() {
        let (_dir, journal) = journal();
        journal.save(&entry("a.md", "x")).unwrap();
        journal.rename("a.md", "b.md");
        assert!(journal.load("a.md").is_none());
        assert_eq!(journal.load("b.md").unwrap().key, "b.md");
        journal.rename("none.md", "c.md");
        assert!(journal.load("c.md").is_none());
    }

    #[test]
    fn a_text_past_the_limit_is_refused_not_cut() {
        let (_dir, journal) = journal();
        let big = "x".repeat(MAX_BYTES + 1);
        assert!(journal.save(&entry("a.md", &big)).is_err());
        assert!(journal.load("a.md").is_none());
    }
}
