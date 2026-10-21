//! The feed: what the watch reported, as the list the person reads. Pure state: batches in, items out.
//!
//! One item per file touched (same file, same kind, within [`COALESCE_MS`] = one item with a save count), one group per
//! bulk batch (a git operation, or a flood of files), a note when events were lost. The person's own moves and
//! deletes are listed as operations; their saves are not news. Directories appear only when moved or removed.

use std::collections::VecDeque;

use permark_core::projects::ProjectEvent;
use permark_core::watch::{is_git_signal, ChangeKind, FsBatch};
use serde::Serialize;
use ts_rs::TS;

/// A save inside this window of the last one is the same item (its count goes up).
pub const COALESCE_MS: u64 = 2000;
/// Items kept (oldest dropped).
pub const MAX_ITEMS: usize = 500;
/// How many of the newest items a new change is compared with.
const LOOKBACK: usize = 24;
/// Paths shown for a group.
const SAMPLE: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum FeedEntryKind {
    Created,
    Modified,
    Removed,
    Renamed { from: String },
}

/// Why a batch is one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FeedGroupReason {
    /// A git operation moved the files (checkout, merge, pull, ...).
    Git,
    /// A great many files changed at once.
    Large,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "item",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum FeedItem {
    /// One file (or folder, when moved | removed).
    Entry {
        #[ts(type = "number")]
        id: u64,
        path: String,
        #[serde(flatten)]
        kind: FeedEntryKind,
        #[ts(type = "number")]
        at_ms: u64,
        /// Saves in a row (1 = one).
        saves: u32,
        /// The person's own doing (a move, a delete).
        own: bool,
        is_dir: bool,
    },
    /// Many files at once.
    Group {
        #[ts(type = "number")]
        id: u64,
        reason: FeedGroupReason,
        count: u32,
        #[ts(type = "number")]
        at_ms: u64,
        sample: Vec<String>,
        /// All of it was the person's own doing.
        own: bool,
    },
    /// Events were lost; the app looked at the disk again.
    Resync {
        #[ts(type = "number")]
        id: u64,
        #[ts(type = "number")]
        at_ms: u64,
    },
    /// A project inside the folder appeared, moved, went, came back, or the project list changed
    /// (`projects.md` §12.5).
    Project {
        #[ts(type = "number")]
        id: u64,
        #[ts(type = "number")]
        at_ms: u64,
        event: ProjectEvent,
    },
}

impl FeedItem {
    #[must_use]
    pub fn id(&self) -> u64 {
        match self {
            Self::Entry { id, .. }
            | Self::Group { id, .. }
            | Self::Resync { id, .. }
            | Self::Project { id, .. } => *id,
        }
    }
}

#[derive(Debug, Default)]
pub struct Feed {
    /// Newest first.
    items: VecDeque<FeedItem>,
    next_id: u64,
}

impl Feed {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Newest first.
    #[must_use]
    pub fn items(&self) -> Vec<FeedItem> {
        self.items.iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    fn take_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn push(&mut self, item: FeedItem) -> FeedItem {
        self.items.push_front(item.clone());
        self.items.truncate(MAX_ITEMS);
        item
    }

    /// A project news item at `now_ms`.
    pub fn project(&mut self, event: &ProjectEvent, now_ms: u64) -> FeedItem {
        let id = self.take_id();
        self.push(FeedItem::Project {
            id,
            at_ms: now_ms,
            event: event.clone(),
        })
    }

    /// Takes one batch at time `now_ms`; returns the items it created or changed (for the UI to upsert).
    pub fn apply(&mut self, batch: &FsBatch, now_ms: u64) -> Vec<FeedItem> {
        let mut changed = Vec::new();
        if batch.rescan {
            let id = self.take_id();
            changed.push(self.push(FeedItem::Resync { id, at_ms: now_ms }));
        }
        if batch.bulk {
            if let Some(group) = self.group(batch, now_ms) {
                changed.push(group);
            }
            return changed;
        }
        for change in &batch.changes {
            if is_git_signal(&change.path) {
                continue;
            }
            let kind = match &change.kind {
                ChangeKind::Created => FeedEntryKind::Created,
                ChangeKind::Modified => FeedEntryKind::Modified,
                ChangeKind::Removed => FeedEntryKind::Removed,
                ChangeKind::Renamed { from } => FeedEntryKind::Renamed { from: from.clone() },
            };
            // the person's saves are not news; their moves and deletes are operations
            let operation = matches!(kind, FeedEntryKind::Renamed { .. } | FeedEntryKind::Removed);
            if change.own && !operation {
                continue;
            }
            // folders: only when they move or go
            if change.is_dir && !operation {
                continue;
            }
            if let Some(item) = self.entry(&change.path, kind, change.own, change.is_dir, now_ms) {
                changed.push(item);
            }
        }
        changed
    }

    fn group(&mut self, batch: &FsBatch, now_ms: u64) -> Option<FeedItem> {
        let files: Vec<&str> = batch
            .changes
            .iter()
            .filter(|c| !is_git_signal(&c.path))
            .map(|c| c.path.as_str())
            .collect();
        let git = batch.changes.iter().any(|c| is_git_signal(&c.path));
        if files.is_empty() && !git {
            return None;
        }
        let own = !batch.changes.is_empty()
            && batch
                .changes
                .iter()
                .filter(|c| !is_git_signal(&c.path))
                .all(|c| c.own);
        let id = self.take_id();
        let item = FeedItem::Group {
            id,
            reason: if git {
                FeedGroupReason::Git
            } else {
                FeedGroupReason::Large
            },
            count: u32::try_from(files.len()).unwrap_or(u32::MAX),
            at_ms: now_ms,
            sample: files.iter().take(SAMPLE).map(|p| (*p).to_owned()).collect(),
            own,
        };
        Some(self.push(item))
    }

    /// A new item, or the recent one this save belongs to.
    fn entry(
        &mut self,
        path: &str,
        kind: FeedEntryKind,
        own: bool,
        is_dir: bool,
        now_ms: u64,
    ) -> Option<FeedItem> {
        let same_kind = |existing: &FeedEntryKind| match (existing, &kind) {
            (FeedEntryKind::Modified | FeedEntryKind::Created, FeedEntryKind::Modified) => true,
            (a, b) => a == b,
        };
        let found = self
            .items
            .iter_mut()
            .take(LOOKBACK)
            .find_map(|item| match item {
                FeedItem::Entry {
                    path: p,
                    kind: k,
                    at_ms,
                    own: o,
                    ..
                } if p == path
                    && *o == own
                    && now_ms.saturating_sub(*at_ms) <= COALESCE_MS
                    && same_kind(k) =>
                {
                    Some(item)
                }
                _ => None,
            });
        if let Some(FeedItem::Entry { saves, at_ms, .. }) = found {
            *saves = saves.saturating_add(1);
            *at_ms = now_ms;
            let updated = self
                .items
                .iter()
                .find(|i| matches!(i, FeedItem::Entry { path: p, at_ms: t, .. } if p == path && *t == now_ms))
                .cloned();
            return updated;
        }
        let id = self.take_id();
        Some(self.push(FeedItem::Entry {
            id,
            path: path.to_owned(),
            kind,
            at_ms: now_ms,
            saves: 1,
            own,
            is_dir,
        }))
    }
}

#[cfg(test)]
mod tests {
    use permark_core::watch::Change;

    use super::*;

    fn change(path: &str, kind: ChangeKind) -> Change {
        Change {
            path: path.to_owned(),
            kind,
            is_dir: false,
            own: false,
        }
    }

    fn batch(changes: Vec<Change>) -> FsBatch {
        FsBatch {
            changes,
            bulk: false,
            rescan: false,
        }
    }

    fn entries(feed: &Feed) -> Vec<(String, u32)> {
        feed.items()
            .into_iter()
            .filter_map(|i| match i {
                FeedItem::Entry { path, saves, .. } => Some((path, saves)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn saves_in_a_row_are_one_item_with_a_count_and_later_ones_are_new() {
        let mut feed = Feed::new();
        let first = feed.apply(&batch(vec![change("a.md", ChangeKind::Modified)]), 1_000);
        assert_eq!(first.len(), 1);
        let again = feed.apply(&batch(vec![change("a.md", ChangeKind::Modified)]), 2_500);
        assert_eq!(again.len(), 1);
        assert_eq!(first[0].id(), again[0].id(), "same item, updated");
        assert_eq!(entries(&feed), [("a.md".to_owned(), 2)]);
        // a pause longer than the window starts a new item, newest first
        feed.apply(&batch(vec![change("a.md", ChangeKind::Modified)]), 10_000);
        assert_eq!(
            entries(&feed),
            [("a.md".to_owned(), 1), ("a.md".to_owned(), 2)]
        );
    }

    #[test]
    fn created_then_modified_stays_created_and_removed_is_its_own_item() {
        let mut feed = Feed::new();
        feed.apply(&batch(vec![change("n.md", ChangeKind::Created)]), 0);
        feed.apply(&batch(vec![change("n.md", ChangeKind::Modified)]), 500);
        let items = feed.items();
        assert!(matches!(
            &items[0],
            FeedItem::Entry {
                kind: FeedEntryKind::Created,
                saves: 2,
                ..
            }
        ));
        feed.apply(&batch(vec![change("n.md", ChangeKind::Removed)]), 900);
        assert_eq!(feed.items().len(), 2);
        assert!(matches!(
            &feed.items()[0],
            FeedItem::Entry {
                kind: FeedEntryKind::Removed,
                ..
            }
        ));
    }

    #[test]
    fn the_persons_saves_are_not_news_but_their_moves_and_deletes_are_listed() {
        let mut feed = Feed::new();
        let own = |path: &str, kind| Change {
            own: true,
            ..change(path, kind)
        };
        let out = feed.apply(
            &batch(vec![
                own("saved.md", ChangeKind::Modified),
                own("made.md", ChangeKind::Created),
                own(
                    "moved.md",
                    ChangeKind::Renamed {
                        from: "old.md".to_owned(),
                    },
                ),
                own("deleted.md", ChangeKind::Removed),
            ]),
            0,
        );
        assert_eq!(out.len(), 2);
        let kinds: Vec<_> = feed
            .items()
            .into_iter()
            .map(|i| match i {
                FeedItem::Entry { path, own, .. } => (path, own),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                ("deleted.md".to_owned(), true),
                ("moved.md".to_owned(), true)
            ]
        );
        // someone else's save of the same file is news and is not folded into the person's item
        feed.apply(&batch(vec![change("deleted.md", ChangeKind::Removed)]), 100);
        assert_eq!(feed.items().len(), 3);
    }

    #[test]
    fn folders_show_only_when_moved_or_removed_and_git_signals_never() {
        let mut feed = Feed::new();
        let dir = |path: &str, kind| Change {
            is_dir: true,
            ..change(path, kind)
        };
        let out = feed.apply(
            &batch(vec![
                dir("docs", ChangeKind::Created),
                dir("docs", ChangeKind::Modified),
                dir("docs", ChangeKind::Removed),
                change(".git/index", ChangeKind::Modified),
                change(".git/HEAD", ChangeKind::Modified),
            ]),
            0,
        );
        assert_eq!(out.len(), 1);
        assert!(matches!(
            &out[0],
            FeedItem::Entry {
                is_dir: true,
                kind: FeedEntryKind::Removed,
                ..
            }
        ));
    }

    #[test]
    fn a_bulk_batch_is_one_group_named_for_its_cause() {
        let mut feed = Feed::new();
        let many: Vec<Change> = (0..300)
            .map(|n| change(&format!("src/f{n}.md"), ChangeKind::Modified))
            .collect();
        let large = FsBatch {
            changes: many,
            bulk: true,
            rescan: false,
        };
        let out = feed.apply(&large, 5);
        assert_eq!(out.len(), 1);
        let FeedItem::Group {
            reason,
            count,
            sample,
            own,
            ..
        } = &out[0]
        else {
            panic!("not a group")
        };
        assert_eq!(
            (*reason, *count, sample.len(), *own),
            (FeedGroupReason::Large, 300, 5, false)
        );

        let mut checkout = vec![
            change(".git/HEAD", ChangeKind::Modified),
            change(".git/index", ChangeKind::Modified),
        ];
        checkout.extend((0..3).map(|n| change(&format!("a{n}.md"), ChangeKind::Modified)));
        let git = FsBatch {
            changes: checkout,
            bulk: true,
            rescan: false,
        };
        let out = feed.apply(&git, 6);
        assert!(matches!(
            &out[0],
            FeedItem::Group {
                reason: FeedGroupReason::Git,
                count: 3,
                ..
            }
        ));
        // a bulk batch with nothing but git signals says nothing about files, but it was a git operation
        let signals = FsBatch {
            changes: vec![change(".git/index", ChangeKind::Modified)],
            bulk: true,
            rescan: false,
        };
        assert!(matches!(
            &feed.apply(&signals, 7)[0],
            FeedItem::Group {
                reason: FeedGroupReason::Git,
                count: 0,
                ..
            }
        ));
        // nothing at all: no group
        let empty = FsBatch {
            changes: Vec::new(),
            bulk: true,
            rescan: false,
        };
        assert!(feed.apply(&empty, 8).is_empty());
    }

    #[test]
    fn a_group_of_the_persons_own_moves_says_so() {
        let mut feed = Feed::new();
        let moved: Vec<Change> = (0..250)
            .map(|n| Change {
                own: true,
                ..change(
                    &format!("m/f{n}.md"),
                    ChangeKind::Renamed {
                        from: format!("o/f{n}.md"),
                    },
                )
            })
            .collect();
        let out = feed.apply(
            &FsBatch {
                changes: moved,
                bulk: true,
                rescan: false,
            },
            0,
        );
        assert!(matches!(
            &out[0],
            FeedItem::Group {
                own: true,
                count: 250,
                ..
            }
        ));
    }

    #[test]
    fn a_rescan_leaves_a_note_and_the_ring_is_capped() {
        let mut feed = Feed::new();
        let out = feed.apply(
            &FsBatch {
                changes: Vec::new(),
                bulk: false,
                rescan: true,
            },
            1,
        );
        assert!(matches!(&out[0], FeedItem::Resync { .. }));
        for n in 0..(MAX_ITEMS + 50) {
            feed.apply(
                &batch(vec![change(&format!("f{n}.md"), ChangeKind::Created)]),
                n as u64 * 10_000,
            );
        }
        assert_eq!(feed.items().len(), MAX_ITEMS);
        // ids keep going up, newest first
        let ids: Vec<u64> = feed.items().iter().map(FeedItem::id).collect();
        assert!(ids.windows(2).all(|w| w[0] > w[1]));
        feed.clear();
        assert_eq!(feed.items(), Vec::new());
    }

    #[test]
    fn hostile_names_and_clock_going_backwards_do_not_panic() {
        let mut feed = Feed::new();
        feed.apply(
            &batch(vec![change("日本 ü/a b.md", ChangeKind::Modified)]),
            10_000,
        );
        feed.apply(
            &batch(vec![change("日本 ü/a b.md", ChangeKind::Modified)]),
            5,
        );
        feed.apply(&batch(vec![change("", ChangeKind::Created)]), u64::MAX);
        assert!(!feed.items().is_empty());
    }
}
