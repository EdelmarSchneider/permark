//! The app's own writes. When the editor saves a document, the watcher sees the file change like any
//! other; that change is not news for the person who just made it (no glow, no toast, no feed row, no
//! Follow jump), but the search index and the git status still have to learn about it.
//!
//! The writer announces the content it is about to write ([`OwnWrites::expect`]). When the watcher
//! reports a change whose content is exactly that, the change is recognised as ours and is dropped
//! from what the UI hears ([`OwnWrites::strip`]). A different content at the same path (an agent saved
//! right after us) is not recognised, so it stays news.
//!
//! An announcement is for the next report of that path only. The first change the watcher reports for
//! a path ends the announcements of the path, whether or not it was ours: a change whose content could not
//! be read cannot be told from an agent's, and a change with other content means ours is already past.
//! So a leftover announcement can never hide a later change that happens to have the same content.
//!
//! Moving and deleting files in the Explorer are the app's own doing too. The paths involved are announced
//! before the operation ([`OwnWrites::expect_moves`], [`OwnWrites::expect_removals`]) and the watcher's
//! removed, created and renamed reports for them are dropped, one report per announced path. The UI shows
//! the operation as one row of its own instead of a row for every file.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::sigs::SigStore;
use super::types::{ChangeKind, FsChange};
use crate::vault::key;

/// An announced write that the watcher never reported (it is outside what the watcher looks at, or the
/// write failed) is forgotten after this long. The watcher reports a write within about a second
/// (150 ms of quiet, one second at most), so this leaves room for a slow disk and no more.
const EXPIRES_AFTER: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct OwnWrites {
    /// Case-folded vault path to the content hashes announced for it, with the time of the announcement.
    pending: Mutex<HashMap<String, Vec<(u64, Instant)>>>,
    /// Case-folded vault paths the app moved away from, moved to or deleted, with the time of the announcement.
    moved: Mutex<HashMap<String, Instant>>,
}

impl OwnWrites {
    pub fn new() -> Self {
        Self::default()
    }

    /// Announces that the app is about to move files: each pair is `(from, to)`, vault paths of files.
    /// The watcher's reports of the old and the new path are not news.
    pub fn expect_moves(&self, pairs: &[(String, String)]) {
        let now = Instant::now();
        if let Ok(mut map) = self.moved.lock() {
            map.retain(|_, at| is_fresh(*at, now));
            for (from, to) in pairs {
                map.insert(key(from), now);
                map.insert(key(to), now);
            }
        }
    }

    /// Announces that the app is about to delete files (vault paths).
    pub fn expect_removals(&self, paths: &[String]) {
        let now = Instant::now();
        if let Ok(mut map) = self.moved.lock() {
            map.retain(|_, at| is_fresh(*at, now));
            for path in paths {
                map.insert(key(path), now);
            }
        }
    }

    /// Takes back the announcements of deletions that were not made.
    pub fn retract_removals(&self, paths: &[String]) {
        if let Ok(mut map) = self.moved.lock() {
            for path in paths {
                map.remove(&key(path));
            }
        }
    }

    /// Takes back the announcements of moves that were not made.
    pub fn retract_moves(&self, pairs: &[(String, String)]) {
        if let Ok(mut map) = self.moved.lock() {
            for (from, to) in pairs {
                map.remove(&key(from));
                map.remove(&key(to));
            }
        }
    }

    /// Whether `key` was announced as moved or deleted, and still is fresh; the announcement is used up.
    fn take_moved(&self, key: &str, now: Instant) -> bool {
        self.moved
            .lock()
            .ok()
            .and_then(|mut map| map.remove(key))
            .is_some_and(|at| is_fresh(at, now))
    }

    /// Announces that `hash` (xxh3 of the bytes) is about to be written to `rel`. Call it before the
    /// write: the watcher may report the change before the writer has returned.
    pub fn expect(&self, rel: &str, hash: u64) {
        self.expect_at(rel, hash, Instant::now());
    }

    fn expect_at(&self, rel: &str, hash: u64, now: Instant) {
        if let Ok(mut map) = self.pending.lock() {
            map.retain(|_, list| {
                list.retain(|(_, at)| is_fresh(*at, now));
                !list.is_empty()
            });
            map.entry(key(rel)).or_default().push((hash, now));
        }
    }

    /// Takes back an announcement whose write failed.
    pub fn retract(&self, rel: &str, hash: u64) {
        let Ok(mut map) = self.pending.lock() else {
            return;
        };
        let k = key(rel);
        if let Some(list) = map.get_mut(&k) {
            if let Some(at) = list.iter().position(|(h, _)| *h == hash) {
                list.remove(at);
            }
            if list.is_empty() {
                map.remove(&k);
            }
        }
    }

    /// Whether a change of `key` to content `hash` was announced. If so the announcement is used up,
    /// together with the older ones for the path (their writes were replaced before the watcher looked);
    /// the ones announced after it wait for their own report. If not, whatever is left for the path is
    /// over: this was the report the announcements were for, and it was not ours.
    fn take(&self, key: &str, hash: Option<u64>, now: Instant) -> bool {
        let Ok(mut map) = self.pending.lock() else {
            return false;
        };
        let Some(mut list) = map.remove(key) else {
            return false;
        };
        let matched = hash.and_then(|hash| {
            list.iter()
                .rposition(|(h, at)| *h == hash && is_fresh(*at, now))
        });
        let Some(at) = matched else {
            return false;
        };
        list.drain(..=at);
        list.retain(|(_, announced)| is_fresh(*announced, now));
        if !list.is_empty() {
            map.insert(key.to_owned(), list);
        }
        true
    }

    /// Drops the created and modified changes that are this app's own writes. `sigs` holds the content
    /// hash the watcher just computed for every file it reported; a file it could not read (hash 0, or
    /// no signature at all) is never recognised as ours.
    pub fn strip(&self, changes: &mut Vec<FsChange>, sigs: &SigStore) {
        self.strip_at(changes, sigs, Instant::now());
    }

    fn strip_at(&self, changes: &mut Vec<FsChange>, sigs: &SigStore, now: Instant) {
        changes.retain(|change| {
            let k = key(&change.path);
            // moved or deleted by the app: both ends of a rename are used up, whichever is announced
            let moved = match change.kind {
                ChangeKind::Removed | ChangeKind::Created => self.take_moved(&k, now),
                ChangeKind::Renamed => {
                    let to = self.take_moved(&k, now);
                    let from = change
                        .from
                        .as_deref()
                        .is_some_and(|from| self.take_moved(&key(from), now));
                    to || from
                }
                ChangeKind::Modified => false,
            };
            // written by the app: a moved document whose links were rewritten has both announcements
            let written = matches!(change.kind, ChangeKind::Created | ChangeKind::Modified) && {
                let hash = sigs.get(&k).map(|sig| sig.hash).filter(|hash| *hash != 0);
                self.take(&k, hash, now)
            };
            !(moved || written)
        });
    }
}

fn is_fresh(announced: Instant, now: Instant) -> bool {
    now.saturating_duration_since(announced) < EXPIRES_AFTER
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
    fn a_created_file_is_recognised_and_paths_ignore_case_where_the_system_does() {
        let own = OwnWrites::new();
        own.expect("Docs/New.md", 7);
        let sigs = sigs_with("Docs/New.md", 7);
        let mut changes = vec![change(ChangeKind::Created, "Docs/New.md")];
        own.strip(&mut changes, &sigs);
        assert_eq!(changes, Vec::<FsChange>::new());
        if crate::vault::CASE_INSENSITIVE {
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

    fn is_stripped(own: &OwnWrites, path: &str, hash: u64) -> bool {
        let mut changes = vec![change(ChangeKind::Modified, path)];
        own.strip(&mut changes, &sigs_with(path, hash));
        changes.is_empty()
    }

    #[test]
    fn a_change_that_could_not_be_read_is_news_and_ends_the_announcement() {
        let own = OwnWrites::new();
        own.expect("a.md", 42);
        // The watcher found the file locked: its signature has no content hash (0).
        assert!(!is_stripped(&own, "a.md", 0));
        // The announcement is over: a later change that happens to have that content is news too.
        assert!(!is_stripped(&own, "a.md", 42));

        // No signature at all is the same.
        own.expect("a.md", 42);
        let mut changes = vec![change(ChangeKind::Modified, "a.md")];
        own.strip(&mut changes, &SigStore::new());
        assert_eq!(changes.len(), 1);
        assert!(!is_stripped(&own, "a.md", 42));
    }

    #[test]
    fn the_first_change_that_is_not_ours_ends_the_announcement() {
        let own = OwnWrites::new();
        own.expect("a.md", 42);
        // An agent wrote something else first.
        assert!(!is_stripped(&own, "a.md", 7));
        // If the agent later writes our old content back, that is not our write.
        assert!(!is_stripped(&own, "a.md", 42));
        // Other paths are not affected.
        own.expect("a.md", 1);
        own.expect("b.md", 2);
        assert!(!is_stripped(&own, "a.md", 9));
        assert!(is_stripped(&own, "b.md", 2));
    }

    #[test]
    fn a_match_uses_up_the_older_announcements_and_keeps_the_newer_ones() {
        let own = OwnWrites::new();
        own.expect("a.md", 1);
        own.expect("a.md", 2);
        own.expect("a.md", 3);
        // The watcher only saw the second write (the first was replaced inside its quiet period).
        assert!(is_stripped(&own, "a.md", 2));
        assert!(!is_stripped(&own, "a.md", 1), "the older one was used up");
        own.expect("a.md", 1);
        own.expect("a.md", 2);
        own.expect("a.md", 3);
        assert!(is_stripped(&own, "a.md", 2));
        assert!(
            is_stripped(&own, "a.md", 3),
            "the newer one is still waiting"
        );
    }

    fn renamed(path: &str, from: &str) -> FsChange {
        FsChange {
            kind: ChangeKind::Renamed,
            path: path.to_owned(),
            from: Some(from.to_owned()),
            at_ms: 1,
        }
    }

    #[test]
    fn the_reports_of_a_move_the_app_made_are_dropped_once_each() {
        let own = OwnWrites::new();
        own.expect_moves(&[("docs/a.md".to_owned(), "docs/b.md".to_owned())]);
        let sigs = SigStore::new();
        let mut changes = vec![
            change(ChangeKind::Removed, "docs/a.md"),
            change(ChangeKind::Created, "docs/b.md"),
            change(ChangeKind::Removed, "docs/other.md"),
        ];
        own.strip(&mut changes, &sigs);
        assert_eq!(changes, [change(ChangeKind::Removed, "docs/other.md")]);

        // used up: the same report again is news
        let mut again = vec![change(ChangeKind::Removed, "docs/a.md")];
        own.strip(&mut again, &sigs);
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn a_rename_report_of_an_announced_move_is_dropped_and_both_ends_are_used_up() {
        let own = OwnWrites::new();
        own.expect_moves(&[("a.md".to_owned(), "b.md".to_owned())]);
        let mut changes = vec![renamed("b.md", "a.md")];
        own.strip(&mut changes, &SigStore::new());
        assert!(changes.is_empty(), "{changes:?}");
        let mut again = vec![change(ChangeKind::Removed, "a.md")];
        own.strip(&mut again, &SigStore::new());
        assert_eq!(again.len(), 1, "the old path was used up with the rename");
    }

    #[test]
    fn deletions_the_app_made_are_dropped_and_modifications_are_not_affected() {
        let own = OwnWrites::new();
        own.expect_removals(&["Docs/Gone.md".to_owned()]);
        let mut changes = vec![
            change(ChangeKind::Removed, "Docs/Gone.md"),
            change(ChangeKind::Modified, "Docs/Gone.md"),
        ];
        own.strip(&mut changes, &SigStore::new());
        // the removal is ours (case aside on Windows); a modification was never announced
        assert_eq!(changes, [change(ChangeKind::Modified, "Docs/Gone.md")]);
    }

    #[test]
    fn a_moved_document_with_rewritten_links_uses_up_both_announcements() {
        let own = OwnWrites::new();
        own.expect_moves(&[("a.md".to_owned(), "docs/a.md".to_owned())]);
        own.expect("docs/a.md", 77);
        let sigs = sigs_with("docs/a.md", 77);
        let mut changes = vec![change(ChangeKind::Created, "docs/a.md")];
        own.strip(&mut changes, &sigs);
        assert!(changes.is_empty(), "{changes:?}");
        // nothing is left to hide a later change with the same content
        assert!(!is_stripped(&own, "docs/a.md", 77));
    }

    #[test]
    fn retracted_removals_are_news_again() {
        let own = OwnWrites::new();
        let paths = ["docs/a.md".to_owned(), "docs/b.md".to_owned()];
        own.expect_removals(&paths);
        own.retract_removals(&paths[..1]);
        let mut changes = vec![
            change(ChangeKind::Removed, "docs/a.md"),
            change(ChangeKind::Removed, "docs/b.md"),
        ];
        own.strip(&mut changes, &SigStore::new());
        // a.md was taken back; b.md is still ours
        assert_eq!(changes, [change(ChangeKind::Removed, "docs/a.md")]);
    }

    #[test]
    fn retracted_moves_are_news_again() {
        let own = OwnWrites::new();
        let pairs = [("a.md".to_owned(), "b.md".to_owned())];
        own.expect_moves(&pairs);
        own.retract_moves(&pairs);
        let mut changes = vec![change(ChangeKind::Removed, "a.md")];
        own.strip(&mut changes, &SigStore::new());
        assert_eq!(changes.len(), 1);
    }

    #[test]
    fn an_announcement_expires_quickly() {
        assert!(EXPIRES_AFTER <= Duration::from_secs(10));
        let now = Instant::now();
        let Some(long_ago) = now.checked_sub(EXPIRES_AFTER + Duration::from_secs(1)) else {
            return; // the machine started less than that ago
        };
        let own = OwnWrites::new();
        own.expect_at("a.md", 5, long_ago);
        let mut changes = vec![change(ChangeKind::Modified, "a.md")];
        own.strip_at(&mut changes, &sigs_with("a.md", 5), now);
        assert_eq!(changes.len(), 1, "too old to be ours");
    }
}
