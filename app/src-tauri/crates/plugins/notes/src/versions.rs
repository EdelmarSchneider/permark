//! Earlier versions of the notes, in the person's profile: `<folder id>/<note id>/<ms>.md`.
//!
//! A version is a copy of what a note held before a save replaced it. One is taken at the first save after the note is
//! opened in a window, then at most every ten minutes while it is edited; one is taken before a restore (so a restore
//! can be undone) and when text a crash left is found. The newest fifty of a note are kept. A version is named after
//! the moment it was taken, which is what the list shows. Nothing here reads a clock: the time is an argument.

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};

use permark_core::fsx;
use permark_core::paths::fold;
use serde::Serialize;
use ts_rs::TS;

/// At most one version this often while a note is edited.
pub const MIN_GAP_MS: u64 = 10 * 60 * 1000;
/// The newest this many of a note are kept.
pub const KEEP: usize = 50;
/// The versions of a note that is gone are removed when all are older than this.
pub const ORPHAN_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// One version of a note, as the window lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Version {
    /// Opaque: the moment it was taken, in milliseconds.
    pub id: String,
    pub at_ms: u64,
    pub size: u64,
}

/// When a version of each note was last taken, to decide when the next one is due.
#[derive(Debug, Default)]
pub struct Cadence {
    last_taken: HashMap<String, u64>,
}

impl Cadence {
    /// Whether a version of `note` is due at `now_ms`: none was taken since it was opened, or the last one is at least
    /// [`MIN_GAP_MS`] old.
    #[must_use]
    pub fn due(&self, note: &str, now_ms: u64) -> bool {
        self.last_taken
            .get(note)
            .is_none_or(|&last| now_ms.saturating_sub(last) >= MIN_GAP_MS)
    }

    /// A version of `note` was taken at `now_ms`.
    pub fn taken(&mut self, note: &str, now_ms: u64) {
        self.last_taken.insert(note.to_owned(), now_ms);
    }

    /// The note was opened: its next save takes a version.
    pub fn opened(&mut self, note: &str) {
        self.last_taken.remove(note);
    }

    /// The note was renamed.
    pub fn renamed(&mut self, from: &str, to: &str) {
        if let Some(at) = self.last_taken.remove(from) {
            self.last_taken.insert(to.to_owned(), at);
        }
    }
}

/// The versions of one notes folder.
#[derive(Debug, Clone)]
pub struct VersionStore {
    dir: PathBuf,
}

/// The id of a note in the store: its file name folded the way the system folds names.
#[must_use]
pub fn note_id(note: &str) -> String {
    fsx::hash(fold(note).as_bytes()).hex()
}

fn is_version_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())
}

impl VersionStore {
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn dir_of(&self, note: &str) -> PathBuf {
        self.dir.join(note_id(note))
    }

    /// Keeps `bytes`, the text of `note` as it was at `now_ms`. Nothing is kept when the newest version already holds
    /// exactly these bytes. Returns the version made.
    ///
    /// # Errors
    /// The version could not be written.
    pub fn snapshot(&self, note: &str, bytes: &[u8], now_ms: u64) -> io::Result<Option<Version>> {
        if let Some(newest) = self.versions(note).first() {
            if self
                .read(note, &newest.id)
                .is_ok_and(|kept| kept.as_slice() == bytes)
            {
                return Ok(None);
            }
        }
        let dir = self.dir_of(note);
        std::fs::create_dir_all(&dir)?;
        let mut at = now_ms;
        while dir.join(format!("{at}.md")).exists() {
            at += 1;
        }
        fsx::write_atomic(&dir.join(format!("{at}.md")), bytes)?;
        self.prune(note);
        Ok(Some(Version {
            id: at.to_string(),
            at_ms: at,
            size: bytes.len() as u64,
        }))
    }

    /// The versions of `note`, the newest first.
    #[must_use]
    pub fn versions(&self, note: &str) -> Vec<Version> {
        let Ok(entries) = std::fs::read_dir(self.dir_of(note)) else {
            return Vec::new();
        };
        let mut found: Vec<Version> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let id = name.strip_suffix(".md")?;
                if !is_version_id(id) {
                    return None;
                }
                Some(Version {
                    id: id.to_owned(),
                    at_ms: id.parse().ok()?,
                    size: entry.metadata().map_or(0, |m| m.len()),
                })
            })
            .collect();
        found.sort_by_key(|v| std::cmp::Reverse(v.at_ms));
        found
    }

    /// The bytes of a version. An id that is not one of a version is refused.
    ///
    /// # Errors
    /// Not a version id | not there.
    pub fn read(&self, note: &str, id: &str) -> io::Result<Vec<u8>> {
        if !is_version_id(id) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "version id"));
        }
        std::fs::read(self.dir_of(note).join(format!("{id}.md")))
    }

    /// Removes the versions of `note` past the newest [`KEEP`].
    fn prune(&self, note: &str) {
        for old in self.versions(note).into_iter().skip(KEEP) {
            let _ = std::fs::remove_file(self.dir_of(note).join(format!("{}.md", old.id)));
        }
    }

    /// The versions follow a note that was renamed. When the name it takes has versions of its own (an older note of
    /// that name was deleted), the two are joined: a version at the very moment of one already there is put a
    /// millisecond later, so that none is lost and none is left behind.
    pub fn rename(&self, from: &str, to: &str) {
        let (source, target) = (self.dir_of(from), self.dir_of(to));
        if source == target || !source.is_dir() {
            return;
        }
        if !target.exists() {
            let _ = std::fs::rename(&source, &target);
            return;
        }
        for entry in std::fs::read_dir(&source).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_suffix(".md").and_then(|s| s.parse::<u64>().ok()) else {
                continue;
            };
            let mut at = id;
            while target.join(format!("{at}.md")).exists() {
                at += 1;
            }
            let _ = std::fs::rename(entry.path(), target.join(format!("{at}.md")));
        }
        let _ = std::fs::remove_dir(&source);
        self.prune(to);
    }

    /// Removes the versions of notes that are gone (`existing` = the ids of the notes there are) when the newest of each
    /// is older than [`ORPHAN_MS`].
    pub fn prune_orphans(&self, existing: &BTreeSet<String>, now_ms: u64) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let id = entry.file_name().to_string_lossy().into_owned();
            if existing.contains(&id) || !entry.path().is_dir() {
                continue;
            }
            let newest = newest_in(&entry.path());
            if newest.is_none_or(|at| now_ms.saturating_sub(at) > ORPHAN_MS) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    /// Removes every version of `note` (the folder is not wanted any more).
    pub fn forget(&self, note: &str) {
        let _ = std::fs::remove_dir_all(self.dir_of(note));
    }
}

fn newest_in(dir: &Path) -> Option<u64> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_suffix(".md")?
                .parse::<u64>()
                .ok()
        })
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, VersionStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = VersionStore::new(dir.path().join("versions"));
        (dir, store)
    }

    #[test]
    fn the_first_version_is_due_and_then_every_ten_minutes() {
        let mut cadence = Cadence::default();
        assert!(cadence.due("a.md", 1_000));
        cadence.taken("a.md", 1_000);
        assert!(!cadence.due("a.md", 1_000 + MIN_GAP_MS - 1));
        assert!(cadence.due("a.md", 1_000 + MIN_GAP_MS));
        // opening the note again makes the next save take one
        cadence.opened("a.md");
        assert!(cadence.due("a.md", 1_001));
        // a clock that went back does not freeze it
        cadence.taken("a.md", 5_000_000);
        assert!(!cadence.due("a.md", 1_000));
        // rename keeps the schedule
        cadence.renamed("a.md", "b.md");
        assert!(!cadence.due("b.md", 5_000_001));
        assert!(cadence.due("a.md", 5_000_001));
    }

    #[test]
    fn a_snapshot_is_listed_read_back_and_not_repeated() {
        let (_dir, store) = store();
        let first = store.snapshot("a.md", b"one", 100).unwrap().unwrap();
        assert_eq!(first.at_ms, 100);
        assert_eq!(first.size, 3);
        // the same bytes again: nothing new
        assert!(store.snapshot("a.md", b"one", 200).unwrap().is_none());
        store.snapshot("a.md", b"two", 300).unwrap().unwrap();
        let listed = store.versions("a.md");
        assert_eq!(
            listed.iter().map(|v| v.at_ms).collect::<Vec<_>>(),
            [300, 100]
        );
        assert_eq!(store.read("a.md", &listed[0].id).unwrap(), b"two");
        assert_eq!(store.read("a.md", &listed[1].id).unwrap(), b"one");
        // another note has its own
        assert!(store.versions("b.md").is_empty());
    }

    #[test]
    fn two_versions_at_one_millisecond_both_stay() {
        let (_dir, store) = store();
        store.snapshot("a.md", b"one", 100).unwrap();
        let second = store.snapshot("a.md", b"two", 100).unwrap().unwrap();
        assert_eq!(second.at_ms, 101);
        assert_eq!(store.versions("a.md").len(), 2);
    }

    #[test]
    fn only_the_newest_fifty_are_kept() {
        let (_dir, store) = store();
        for n in 0..(KEEP as u64 + 8) {
            store
                .snapshot("a.md", format!("v{n}").as_bytes(), 1_000 + n)
                .unwrap();
        }
        let listed = store.versions("a.md");
        assert_eq!(listed.len(), KEEP);
        assert_eq!(listed[0].at_ms, 1_000 + KEEP as u64 + 7);
        assert_eq!(listed.last().map(|v| v.at_ms), Some(1_008));
    }

    #[test]
    fn version_ids_that_are_paths_are_refused() {
        let (_dir, store) = store();
        store.snapshot("a.md", b"x", 1).unwrap();
        for bad in [
            "",
            "../1",
            "1/../1",
            "1.md",
            "-1",
            "x",
            "99999999999999999999999",
        ] {
            assert!(store.read("a.md", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn names_are_folded_where_the_system_ignores_case() {
        let (_dir, store) = store();
        store.snapshot("Plan.md", b"x", 1).unwrap();
        let same = !store.versions("plan.md").is_empty();
        assert_eq!(same, cfg!(any(windows, target_os = "macos")));
    }

    #[test]
    fn a_rename_moves_the_versions_and_joins_those_of_the_name_it_takes() {
        let (_dir, store) = store();
        store.snapshot("a.md", b"a1", 10).unwrap();
        store.snapshot("a.md", b"a2", 20).unwrap();
        store.rename("a.md", "b.md");
        assert!(store.versions("a.md").is_empty());
        assert_eq!(store.versions("b.md").len(), 2);
        // a note of the name b.md was deleted earlier and left a version at the same moment
        store.snapshot("c.md", b"c1", 10).unwrap();
        store.rename("c.md", "b.md");
        let listed = store.versions("b.md");
        assert_eq!(listed.len(), 3);
        let times: BTreeSet<u64> = listed.iter().map(|v| v.at_ms).collect();
        assert_eq!(times.len(), 3, "no version lost to a clash");
    }

    #[test]
    fn orphans_go_when_old_and_stay_when_recent_or_wanted() {
        let (_dir, store) = store();
        store.snapshot("old.md", b"x", 1_000).unwrap();
        store
            .snapshot("recent.md", b"x", ORPHAN_MS + 1_000)
            .unwrap();
        store.snapshot("alive.md", b"x", 1_000).unwrap();
        let existing = BTreeSet::from([note_id("alive.md")]);
        store.prune_orphans(&existing, ORPHAN_MS + 5_000);
        assert!(store.versions("old.md").is_empty());
        assert_eq!(store.versions("recent.md").len(), 1);
        assert_eq!(store.versions("alive.md").len(), 1);
        store.forget("alive.md");
        assert!(store.versions("alive.md").is_empty());
    }

    #[test]
    fn a_missing_store_lists_nothing_and_prunes_quietly() {
        let (_dir, store) = store();
        assert!(store.versions("a.md").is_empty());
        store.prune_orphans(&BTreeSet::new(), 5);
        store.rename("a.md", "b.md");
        assert!(store.read("a.md", "1").is_err());
    }
}
