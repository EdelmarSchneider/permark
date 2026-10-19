//! The app's own writes. A save, a move or a delete the app makes shows up in the watch like any other change; it is
//! not news for the person who just made it (no feed row, no glow), but the tree and the index still learn from it.
//!
//! The writer announces what it is about to do; the first matching report is marked `own` and uses the
//! announcement up. A different content at the same path (an agent saved right after us) is not recognized, so it
//! stays news. An announcement is for the next report of that path only.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::types::{Change, ChangeKind};
use crate::fsx::ContentHash;
use crate::tree::key_of;

/// An announcement the watch never matched (outside what it looks at, or the write failed) is forgotten after this.
const EXPIRES_AFTER: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct OwnWrites {
    /// Path key -> hashes announced, with the time of the announcement.
    writes: Mutex<HashMap<String, Vec<(u64, Instant)>>>,
    /// Path keys moved from, moved to or deleted by the app.
    moves: Mutex<HashMap<String, Instant>>,
}

fn fresh(at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(at) < EXPIRES_AFTER
}

impl OwnWrites {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `hash` is about to be written to `path`. Call before the write: the watch may report it before the writer
    /// returns.
    pub fn expect_write(&self, path: &str, hash: ContentHash) {
        self.expect_write_at(path, hash, Instant::now());
    }

    fn expect_write_at(&self, path: &str, hash: ContentHash, now: Instant) {
        let mut map = self.writes.lock().unwrap_or_else(PoisonError::into_inner);
        map.retain(|_, list| {
            list.retain(|(_, at)| fresh(*at, now));
            !list.is_empty()
        });
        map.entry(key_of(path)).or_default().push((hash.0, now));
    }

    /// Takes back an announcement whose write failed.
    pub fn retract_write(&self, path: &str, hash: ContentHash) {
        let mut map = self.writes.lock().unwrap_or_else(PoisonError::into_inner);
        let key = key_of(path);
        if let Some(list) = map.get_mut(&key) {
            if let Some(at) = list.iter().position(|(h, _)| *h == hash.0) {
                list.remove(at);
            }
            if list.is_empty() {
                map.remove(&key);
            }
        }
    }

    /// The app is about to move files: pairs of `(from, to)` project paths (files and folders).
    pub fn expect_moves(&self, pairs: &[(String, String)]) {
        let now = Instant::now();
        let mut map = self.moves.lock().unwrap_or_else(PoisonError::into_inner);
        map.retain(|_, at| fresh(*at, now));
        for (from, to) in pairs {
            map.insert(key_of(from), now);
            map.insert(key_of(to), now);
        }
    }

    /// The app is about to delete these project paths.
    pub fn expect_removals(&self, paths: &[String]) {
        let now = Instant::now();
        let mut map = self.moves.lock().unwrap_or_else(PoisonError::into_inner);
        map.retain(|_, at| fresh(*at, now));
        for path in paths {
            map.insert(key_of(path), now);
        }
    }

    /// The app is about to create these project paths (a new file | folder).
    pub fn expect_creations(&self, paths: &[String]) {
        self.expect_removals(paths);
    }

    /// Takes back the announcements of moves | deletes that did not happen.
    pub fn retract_moves(&self, paths: &[String]) {
        let mut map = self.moves.lock().unwrap_or_else(PoisonError::into_inner);
        for path in paths {
            map.remove(&key_of(path));
        }
    }

    fn take_move(&self, key: &str, now: Instant) -> bool {
        self.moves
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key)
            .is_some_and(|at| fresh(at, now))
    }

    /// A change of `key` to content `hash` was announced: the announcement is used up, with the older ones for the
    /// path. Not announced: whatever is left for the path is over (this was the report the announcements were for).
    fn take_write(&self, key: &str, hash: Option<u64>, now: Instant) -> bool {
        let mut map = self.writes.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(mut list) = map.remove(key) else {
            return false;
        };
        let matched = hash.and_then(|hash| {
            list.iter()
                .rposition(|(h, at)| *h == hash && fresh(*at, now))
        });
        let Some(at) = matched else {
            return false;
        };
        list.drain(..=at);
        list.retain(|(_, announced)| fresh(*announced, now));
        if !list.is_empty() {
            map.insert(key.to_owned(), list);
        }
        true
    }

    /// Marks the changes that are this app's own doing. `hash_of(path)` is the content hash the watch just computed
    /// for a file (None: unknown, never recognized as ours).
    pub fn mark(&self, changes: &mut [Change], hash_of: impl Fn(&str) -> Option<u64>) {
        self.mark_at(changes, hash_of, Instant::now());
    }

    fn mark_at(&self, changes: &mut [Change], hash_of: impl Fn(&str) -> Option<u64>, now: Instant) {
        for change in changes {
            let key = key_of(&change.path);
            let moved = match &change.kind {
                ChangeKind::Removed | ChangeKind::Created => self.take_move(&key, now),
                ChangeKind::Renamed { from } => {
                    let to = self.take_move(&key, now);
                    let from = self.take_move(&key_of(from), now);
                    to || from
                }
                ChangeKind::Modified => false,
            };
            let written = matches!(change.kind, ChangeKind::Created | ChangeKind::Modified)
                && !change.is_dir
                && self.take_write(&key, hash_of(&change.path).filter(|h| *h != 0), now);
            change.own = moved || written;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, kind: ChangeKind) -> Change {
        Change {
            path: path.to_owned(),
            kind,
            is_dir: false,
            own: false,
        }
    }

    #[test]
    fn a_write_with_the_announced_content_is_ours_once() {
        let own = OwnWrites::new();
        own.expect_write("a.md", ContentHash(7));
        let mut changes = [change("a.md", ChangeKind::Modified)];
        own.mark(&mut changes, |_| Some(7));
        assert!(changes[0].own);
        let mut again = [change("a.md", ChangeKind::Modified)];
        own.mark(&mut again, |_| Some(7));
        assert!(!again[0].own, "used up");
    }

    #[test]
    fn other_content_at_the_same_path_stays_news_and_ends_the_announcement() {
        let own = OwnWrites::new();
        own.expect_write("a.md", ContentHash(7));
        let mut changes = [change("a.md", ChangeKind::Modified)];
        own.mark(&mut changes, |_| Some(8));
        assert!(!changes[0].own);
        let mut later = [change("a.md", ChangeKind::Modified)];
        own.mark(&mut later, |_| Some(7));
        assert!(
            !later[0].own,
            "a leftover announcement never hides a later change"
        );
    }

    #[test]
    fn unknown_content_is_never_ours() {
        let own = OwnWrites::new();
        own.expect_write("a.md", ContentHash(7));
        let mut changes = [change("a.md", ChangeKind::Modified)];
        own.mark(&mut changes, |_| None);
        assert!(!changes[0].own);
        let mut zero = [change("a.md", ChangeKind::Modified)];
        own.expect_write("a.md", ContentHash(0));
        own.mark(&mut zero, |_| Some(0));
        assert!(!zero[0].own, "hash 0 means unknown");
    }

    #[test]
    fn moves_and_removals_are_used_up_per_path_and_case_folded() {
        let own = OwnWrites::new();
        own.expect_moves(&[("A/old.md".to_owned(), "b/new.md".to_owned())]);
        let mut changes = [
            change("a/old.md", ChangeKind::Removed),
            change("b/new.md", ChangeKind::Created),
            change("b/new.md", ChangeKind::Created),
        ];
        own.mark(&mut changes, |_| None);
        let fold = crate::paths::CASE_INSENSITIVE;
        assert_eq!(changes[0].own, fold);
        assert!(changes[1].own);
        assert!(!changes[2].own);
    }

    #[test]
    fn a_rename_report_uses_up_both_ends() {
        let own = OwnWrites::new();
        own.expect_moves(&[("x.md".to_owned(), "y.md".to_owned())]);
        let mut changes = [change(
            "y.md",
            ChangeKind::Renamed {
                from: "x.md".to_owned(),
            },
        )];
        own.mark(&mut changes, |_| None);
        assert!(changes[0].own);
        let mut leftover = [change("x.md", ChangeKind::Removed)];
        own.mark(&mut leftover, |_| None);
        assert!(!leftover[0].own);
    }

    #[test]
    fn announcements_expire() {
        let own = OwnWrites::new();
        let long_ago = Instant::now()
            .checked_sub(Duration::from_secs(6))
            .unwrap_or_else(Instant::now);
        own.expect_write_at("a.md", ContentHash(7), long_ago);
        let mut changes = [change("a.md", ChangeKind::Modified)];
        own.mark(&mut changes, |_| Some(7));
        assert!(!changes[0].own);
    }

    #[test]
    fn retracted_announcements_do_not_match() {
        let own = OwnWrites::new();
        own.expect_write("a.md", ContentHash(7));
        own.retract_write("a.md", ContentHash(7));
        let mut changes = [change("a.md", ChangeKind::Modified)];
        own.mark(&mut changes, |_| Some(7));
        assert!(!changes[0].own);
    }
}
