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
    /// Moves the app announced: key of the new path -> the old path. Tells one move from a removal and a creation that
    /// happen to be reported together (the watch has not seen the old file's content, so it cannot pair them itself).
    pairs: Mutex<HashMap<String, (String, Instant)>>,
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
        let mut known = self.pairs.lock().unwrap_or_else(PoisonError::into_inner);
        known.retain(|_, (_, at)| fresh(*at, now));
        for (from, to) in pairs {
            map.insert(key_of(from), now);
            map.insert(key_of(to), now);
            known.insert(key_of(to), (from.clone(), now));
        }
    }

    /// A removal and a creation reported together that are a move the app announced become one `Renamed` change
    /// (files and folders). The announcement stays for [`OwnWrites::mark`], which marks the change as the app's own.
    pub fn pair_moves(&self, changes: &mut Vec<Change>) {
        let now = Instant::now();
        let mut known = self.pairs.lock().unwrap_or_else(PoisonError::into_inner);
        known.retain(|_, (_, at)| fresh(*at, now));
        if known.is_empty() {
            return;
        }
        let mut dropped: Vec<usize> = Vec::new();
        for at in 0..changes.len() {
            if !matches!(changes[at].kind, ChangeKind::Created) {
                continue;
            }
            let key = key_of(&changes[at].path);
            let Some((from, _)) = known.get(&key) else {
                continue;
            };
            let from_key = key_of(from);
            let partner = changes.iter().enumerate().find(|(i, c)| {
                // a removal does not always know it was a folder (never seen): a folder created may pair with it
                matches!(c.kind, ChangeKind::Removed)
                    && (c.is_dir == changes[at].is_dir || changes[at].is_dir)
                    && !dropped.contains(i)
                    && key_of(&c.path) == from_key
            });
            let Some((removed_at, _)) = partner else {
                continue;
            };
            let from = from.clone();
            changes[at].kind = ChangeKind::Renamed { from };
            dropped.push(removed_at);
            known.remove(&key);
        }
        dropped.sort_unstable();
        for at in dropped.into_iter().rev() {
            changes.remove(at);
        }
    }

    /// Whether the app announced moving `path` away: `Some(true)` when it announced files below it too (a folder). A
    /// removal the watch knows nothing of (the system names no side of a move, and the tree already shows the new
    /// place) is then a real one, of that kind.
    #[must_use]
    pub fn moved_from(&self, path: &str) -> Option<bool> {
        let now = Instant::now();
        let key = key_of(path);
        let prefix = format!("{key}/");
        let pairs = self.pairs.lock().unwrap_or_else(PoisonError::into_inner);
        let mut found = None;
        for (from, _) in pairs.values().filter(|(_, at)| fresh(*at, now)) {
            let from = key_of(from);
            if from.starts_with(&prefix) {
                return Some(true);
            }
            if from == key {
                found = Some(false);
            }
        }
        found
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
        let mut known = self.pairs.lock().unwrap_or_else(PoisonError::into_inner);
        for path in paths {
            let key = key_of(path);
            map.remove(&key);
            known.remove(&key);
            known.retain(|_, (from, _)| key_of(from) != key);
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

    #[test]
    fn an_announced_move_reported_as_removal_and_creation_is_one_rename() {
        let own = OwnWrites::new();
        own.expect_moves(&[("x.md".to_owned(), "docs/y.md".to_owned())]);
        let mut changes = vec![
            change("other.md", ChangeKind::Modified),
            change("x.md", ChangeKind::Removed),
            change("docs/y.md", ChangeKind::Created),
            change("z.md", ChangeKind::Removed),
        ];
        own.pair_moves(&mut changes);
        assert_eq!(changes.len(), 3);
        assert_eq!(
            changes[1].kind,
            ChangeKind::Renamed {
                from: "x.md".to_owned()
            }
        );
        assert_eq!(changes[1].path, "docs/y.md");
        own.mark(&mut changes, |_| None);
        assert!(changes[1].own, "still the app's own doing");
        assert!(!changes[2].own, "an unrelated removal stays what it was");
    }

    #[test]
    fn pairing_needs_the_announcement_both_ends_and_the_same_kind() {
        let own = OwnWrites::new();
        // nothing announced: the watch's own pairing (content) is the only one
        let mut plain = vec![
            change("x.md", ChangeKind::Removed),
            change("y.md", ChangeKind::Created),
        ];
        own.pair_moves(&mut plain);
        assert_eq!(plain.len(), 2);
        // announced, but the removal is not in this report: the creation stays a creation
        own.expect_moves(&[("x.md".to_owned(), "y.md".to_owned())]);
        let mut half = vec![change("y.md", ChangeKind::Created)];
        own.pair_moves(&mut half);
        assert_eq!(half[0].kind, ChangeKind::Created);
        // a folder removed next to a file created at the new place is no move of that folder
        let mut mixed = vec![change("y.md", ChangeKind::Created)];
        mixed.push(Change {
            path: "x.md".to_owned(),
            kind: ChangeKind::Removed,
            is_dir: true,
            own: false,
        });
        own.pair_moves(&mut mixed);
        assert_eq!(mixed.len(), 2);
        // retracted: gone
        own.retract_moves(&["x.md".to_owned()]);
        let mut gone = vec![
            change("x.md", ChangeKind::Removed),
            change("y.md", ChangeKind::Created),
        ];
        own.pair_moves(&mut gone);
        assert_eq!(gone.len(), 2);
    }

    #[test]
    fn a_moved_folder_pairs_with_a_removal_that_did_not_know_it_was_one() {
        let own = OwnWrites::new();
        own.expect_moves(&[("old".to_owned(), "new".to_owned())]);
        let mut changes = vec![
            change("old", ChangeKind::Removed),
            Change {
                path: "new".to_owned(),
                kind: ChangeKind::Created,
                is_dir: true,
                own: false,
            },
            change("new/a.md", ChangeKind::Created),
        ];
        own.pair_moves(&mut changes);
        assert_eq!(changes.len(), 2);
        assert!(changes[0].is_dir);
        assert_eq!(
            changes[0].kind,
            ChangeKind::Renamed {
                from: "old".to_owned()
            }
        );
        assert_eq!(changes[1].path, "new/a.md");
    }

    #[test]
    fn a_folder_move_is_known_by_the_files_announced_below_it() {
        let own = OwnWrites::new();
        own.expect_moves(&[
            ("lib".to_owned(), "vendor".to_owned()),
            ("lib/x.md".to_owned(), "vendor/x.md".to_owned()),
            ("one.md".to_owned(), "two.md".to_owned()),
        ]);
        assert_eq!(own.moved_from("lib"), Some(true));
        assert_eq!(own.moved_from("li"), None);
        assert_eq!(own.moved_from("one.md"), Some(false));
        assert_eq!(own.moved_from("vendor"), None);
    }

    #[test]
    fn two_moves_in_one_report_pair_each_with_its_own_partner() {
        let own = OwnWrites::new();
        own.expect_moves(&[
            ("a.md".to_owned(), "b.md".to_owned()),
            ("b2.md".to_owned(), "a2.md".to_owned()),
        ]);
        let mut changes = vec![
            change("a.md", ChangeKind::Removed),
            change("b2.md", ChangeKind::Removed),
            change("b.md", ChangeKind::Created),
            change("a2.md", ChangeKind::Created),
        ];
        own.pair_moves(&mut changes);
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().all(|c| matches!(
            &c.kind,
            ChangeKind::Renamed { from } if (c.path == "b.md" && from == "a.md") || (c.path == "a2.md" && from == "b2.md")
        )));
    }
}
