//! The app's own writes. When the editor saves a document, the watcher sees the file change like any
//! other; that change is not news for the person who just made it (no glow, no toast, no feed row, no
//! Follow jump), but the search index and the git status still have to learn about it.
//!
//! The writer announces the content it is about to write ([`OwnWrites::expect`]). When the watcher
//! reports a change whose content is exactly that, the change is recognised as ours and is dropped
//! from what the UI hears ([`OwnWrites::strip`]). A different content at the same path (an agent saved
//! right after us) is not recognised, so it stays news.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::sigs::SigStore;
use super::types::{ChangeKind, FsChange};
use crate::vault::key;

/// An announced write that the watcher never reported (the content was already on disk, or the write
/// failed) is forgotten after this long.
const EXPIRES_AFTER: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct OwnWrites {
    /// Case-folded vault path to the content hashes announced for it, with the time of the announcement.
    pending: Mutex<HashMap<String, Vec<(u64, Instant)>>>,
}

impl OwnWrites {
    pub fn new() -> Self {
        Self::default()
    }

    /// Announces that `hash` (xxh3 of the bytes) is about to be written to `rel`. Call it before the
    /// write: the watcher may report the change before the writer has returned.
    pub fn expect(&self, rel: &str, hash: u64) {
        if let Ok(mut map) = self.pending.lock() {
            let now = Instant::now();
            map.retain(|_, list| {
                list.retain(|(_, at)| now.duration_since(*at) < EXPIRES_AFTER);
                !list.is_empty()
            });
            map.entry(key(rel)).or_default().push((hash, now));
        }
    }

    /// Takes back an announcement whose write failed.
    pub fn retract(&self, rel: &str, hash: u64) {
        self.take(&key(rel), hash);
    }

    /// Whether a change of `key` to content `hash` was announced; if so the announcement is used up.
    fn take(&self, key: &str, hash: u64) -> bool {
        let Ok(mut map) = self.pending.lock() else {
            return false;
        };
        let Some(list) = map.get_mut(key) else {
            return false;
        };
        let Some(at) = list.iter().position(|(h, _)| *h == hash) else {
            return false;
        };
        list.remove(at);
        if list.is_empty() {
            map.remove(key);
        }
        true
    }

    /// Drops the created and modified changes that are this app's own writes. `sigs` holds the content
    /// hash the watcher just computed for every file it reported.
    pub fn strip(&self, changes: &mut Vec<FsChange>, sigs: &SigStore) {
        changes.retain(|change| {
            if !matches!(change.kind, ChangeKind::Created | ChangeKind::Modified) {
                return true;
            }
            let k = key(&change.path);
            let own = sigs
                .get(&k)
                .is_some_and(|sig| sig.hash != 0 && self.take(&k, sig.hash));
            !own
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::sigs::FileSig;

    fn change(kind: ChangeKind, path: &str) -> FsChange {
        FsChange {
            kind,
            path: path.to_owned(),
            from: None,
            at_ms: 1,
        }
    }

    fn sigs_with(path: &str, hash: u64) -> SigStore {
        let sigs = SigStore::new();
        sigs.set(
            key(path),
            path,
            FileSig {
                len: 5,
                mtime_ms: 9,
                hash,
            },
        );
        sigs
    }

    #[test]
    fn a_change_with_the_announced_content_is_dropped_once() {
        let own = OwnWrites::new();
        own.expect("docs/A.md", 42);
        let sigs = sigs_with("docs/A.md", 42);
        let mut changes = vec![change(ChangeKind::Modified, "docs/A.md")];
        own.strip(&mut changes, &sigs);
        assert_eq!(changes, Vec::<FsChange>::new());

        // The announcement was used up: the same content again is news.
        let mut again = vec![change(ChangeKind::Modified, "docs/A.md")];
        own.strip(&mut again, &sigs);
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn other_content_other_paths_and_other_kinds_stay_news() {
        let own = OwnWrites::new();
        own.expect("a.md", 1);
        let mut changes = vec![
            change(ChangeKind::Modified, "a.md"),
            change(ChangeKind::Modified, "b.md"),
            change(ChangeKind::Removed, "a.md"),
            change(ChangeKind::Renamed, "a.md"),
        ];
        // a.md now holds something else (an agent saved over us); b.md was never announced.
        let sigs = sigs_with("a.md", 2);
        sigs.set(
            key("b.md"),
            "b.md",
            FileSig {
                len: 1,
                mtime_ms: 1,
                hash: 1,
            },
        );
        own.strip(&mut changes, &sigs);
        assert_eq!(changes.len(), 4);
    }

    #[test]
    fn a_created_file_is_recognised_and_paths_ignore_case_on_windows() {
        let own = OwnWrites::new();
        own.expect("Docs/New.md", 7);
        let sigs = sigs_with("Docs/New.md", 7);
        let mut changes = vec![change(ChangeKind::Created, "Docs/New.md")];
        own.strip(&mut changes, &sigs);
        assert_eq!(changes, Vec::<FsChange>::new());
        if cfg!(windows) {
            own.expect("docs/new.md", 7);
            let mut again = vec![change(ChangeKind::Modified, "DOCS/NEW.md")];
            let sigs = sigs_with("DOCS/NEW.md", 7);
            own.strip(&mut again, &sigs);
            assert_eq!(again, Vec::<FsChange>::new());
        }
    }

    #[test]
    fn two_quick_saves_are_both_recognised_and_a_failed_write_can_be_retracted() {
        let own = OwnWrites::new();
        own.expect("a.md", 1);
        own.expect("a.md", 2);
        for hash in [1, 2] {
            let sigs = sigs_with("a.md", hash);
            let mut changes = vec![change(ChangeKind::Modified, "a.md")];
            own.strip(&mut changes, &sigs);
            assert_eq!(changes, Vec::<FsChange>::new(), "hash {hash}");
        }
        own.expect("a.md", 3);
        own.retract("a.md", 3);
        let sigs = sigs_with("a.md", 3);
        let mut changes = vec![change(ChangeKind::Modified, "a.md")];
        own.strip(&mut changes, &sigs);
        assert_eq!(changes.len(), 1);
    }

    #[test]
    fn files_too_large_to_hash_are_never_recognised() {
        let own = OwnWrites::new();
        own.expect("big.md", 0);
        let sigs = sigs_with("big.md", 0);
        let mut changes = vec![change(ChangeKind::Modified, "big.md")];
        own.strip(&mut changes, &sigs);
        assert_eq!(changes.len(), 1);
    }
}
