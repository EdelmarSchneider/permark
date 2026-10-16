//! What the review knows about the documents of a project, without files or threads: the version of each
//! that the person approved (its *reviewed* version), the earlier one kept for a review that is open, and
//! which documents differ from theirs.
//!
//! Versions are hashes here; the text behind them is kept by [`super::store`]. Paths are the vault paths the UI
//! uses, compared the way the file system compares them (see [`crate::vault::key`]).
//!
//! **The reviewed version follows what was read.** The unread markers say, for every document, the version that was
//! last read; whenever that changes, the reviewed version becomes it. This is level 1 of the design; level 2
//! (a project where reading does not approve) is the same book with the follow in [`Book::observe`] switched off.
//!
//! **A review that is open keeps the version before it.** Opening a changed document reads it, which moves the
//! reviewed version to what the document holds now; the version it was read from is *held*, so that the
//! changes stay there to be kept or reverted for as long as the review is open ([`Book::release`], or until the
//! document is back to the held version). One version is held for each document, in memory only (a restart of the
//! app ends every review), and a later reading does not replace it: whatever was not reviewed yet stays in the
//! diff. The held version, when there is one, is what a document is compared with ([`Book::baseline`]).
//!
//! The person's own saves are not changes to review: they move the reviewed version along, and hold nothing.
//! But a save while a version is held does not release it either, so the person's own edit then shows in the
//! changes, next to the ones still unreviewed.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::Serialize;

use super::saved::{Saved, SavedDoc};
use crate::render::markdown::is_markdown_path;
use crate::unread::DocState;
use crate::vault::key;

fn is_tracked(path: &str) -> bool {
    is_markdown_path(Path::new(path))
}

/// A version of a document: the hash of its bytes, with the path as the UI spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    path: String,
    hash: u64,
    /// When the file was first seen to be gone, while it is.
    deleted_at: Option<u64>,
}

impl Version {
    fn new(path: &str, hash: u64) -> Self {
        Self {
            path: path.to_owned(),
            hash,
            deleted_at: None,
        }
    }
}

/// Where a document stands with its approved version: the version it is compared with is the held one, else the
/// approved one. Keeping and reverting move it, and undoing puts it back ([`Book::restore_standing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Standing {
    pub reviewed: Option<u64>,
    pub held: Option<u64>,
}

impl Standing {
    /// The hashes whose copies this standing needs.
    pub fn hashes(&self) -> impl Iterator<Item = u64> {
        [self.reviewed, self.held].into_iter().flatten()
    }
}

/// A document that is gone is forgotten (and its copy let go) after this long.
const DELETED_KEPT_FOR_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Of the documents that are gone, this many are remembered, the most recently deleted ones.
const MAX_DELETED: usize = 200;

/// A document as the last look at the project showed it.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
    current: Option<u64>,
    read: Option<u64>,
}

/// How a document differs from what the person approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// It holds something else than the approved version.
    Changed,
    /// It was never approved: nothing was read of it.
    New,
    /// It had an approved version and the file is gone.
    Deleted,
}

/// A document that differs from its approved version. Matches the UI's `ReviewEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub path: String,
    pub status: Status,
}

/// A version that should have a copy, and what the document holds now (when it is known), which may be it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    pub path: String,
    pub hash: u64,
    pub current: Option<u64>,
}

#[derive(Debug, Default)]
pub struct Book {
    /// The approved version of each document, by case-folded path. This is what `reviewed.json` holds.
    reviewed: BTreeMap<String, Version>,
    /// The earlier version kept for a review that is open.
    held: BTreeMap<String, u64>,
    /// The version the person read of a document they opened, until the unread markers say it counts as read: when the
    /// read version moves to it, the one before is held.
    opened: BTreeMap<String, u64>,
    /// The documents as the last look at the project showed them.
    seen: BTreeMap<String, Seen>,
    /// The disk has been looked at: a document that is not there is gone.
    looked: bool,
}

impl Book {
    /// `saved` is what `reviewed.json` held: the approved version of each document, by path.
    pub fn new(saved: Saved) -> Self {
        let reviewed = saved
            .into_iter()
            .filter(|(path, _)| is_tracked(path))
            .map(|(path, doc)| {
                (
                    key(&path),
                    Version {
                        path,
                        hash: doc.hash,
                        deleted_at: doc.deleted_at,
                    },
                )
            })
            .collect();
        Self {
            reviewed,
            ..Self::default()
        }
    }

    /// What `reviewed.json` holds.
    pub fn saved(&self) -> Saved {
        self.reviewed
            .values()
            .map(|v| {
                (
                    v.path.clone(),
                    SavedDoc {
                        hash: v.hash,
                        deleted_at: v.deleted_at,
                    },
                )
            })
            .collect()
    }

    /// The approved version of a document.
    pub fn reviewed(&self, path: &str) -> Option<u64> {
        self.reviewed.get(&key(path)).map(|v| v.hash)
    }

    /// The version kept for the review of a document that is open.
    pub fn held(&self, path: &str) -> Option<u64> {
        self.held.get(&key(path)).copied()
    }

    /// What a document is compared with: the held version, else the approved one. `None` for a document
    /// that was never approved.
    pub fn baseline(&self, path: &str) -> Option<u64> {
        self.held(path).or_else(|| self.reviewed(path))
    }

    /// Where a document stands.
    pub fn standing(&self, path: &str) -> Standing {
        Standing {
            reviewed: self.reviewed(path),
            held: self.held(path),
        }
    }

    /// Puts a document back from where it stands now to `to`, if it still stands at `from`: how undoing a keep is
    /// done. `None` when it stands somewhere else. Otherwise whether `reviewed.json` changed.
    pub fn restore_standing(&mut self, path: &str, from: Standing, to: Standing) -> Option<bool> {
        if self.standing(path) != from {
            return None;
        }
        let k = key(path);
        let before = self.reviewed.get(&k).cloned();
        match to.reviewed {
            Some(hash) => {
                let version = self
                    .reviewed
                    .entry(k.clone())
                    .or_insert_with(|| Version::new(path, hash));
                version.hash = hash;
            }
            None => {
                self.reviewed.remove(&k);
            }
        }
        match to.held {
            Some(hash) => {
                self.held.insert(k.clone(), hash);
            }
            None => {
                self.held.remove(&k);
            }
        }
        Some(self.reviewed.get(&k).map(|v| v.hash) != before.map(|v| v.hash))
    }

    /// The versions that are held, with the path of their document.
    pub fn holds(&self) -> Vec<(String, u64)> {
        self.held
            .iter()
            .map(|(k, hash)| {
                let path = self
                    .reviewed
                    .get(k)
                    .map_or_else(|| k.clone(), |v| v.path.clone());
                (path, *hash)
            })
            .collect()
    }

    /// The person is reading the document, which holds `hash`: when the version that counts as read moves to it, the
    /// one before it is held. (It is the version that decides, not the order of the looks at the project, which a
    /// watcher can get between the reading and the look that says it.)
    pub fn opening(&mut self, path: &str, hash: u64) {
        let k = key(path);
        // reading what is read already moves nothing, so there is nothing to wait for
        if self
            .seen
            .get(&k)
            .is_some_and(|seen| seen.read == Some(hash))
        {
            return;
        }
        self.opened.insert(k, hash);
    }

    /// Takes in a look at the project: the documents that moved, and every document as it is now.
    /// Returns whether `reviewed.json` changed.
    pub fn observe(&mut self, moves: &[(String, String)], looked: bool, docs: &[DocState]) -> bool {
        let before = self.saved();
        for (from, to) in moves {
            self.moved(from, to);
        }
        self.looked |= looked;
        let mut now = BTreeMap::new();
        for doc in docs {
            let k = key(&doc.path);
            if let Some(read) = doc.read {
                match self.seen.get(&k) {
                    // the first time this document is seen: an approved version that was saved stands
                    None => {
                        self.reviewed
                            .entry(k.clone())
                            .or_insert_with(|| Version::new(&doc.path, read));
                    }
                    Some(seen) if seen.read != Some(read) => self.advance(&k, &doc.path, read),
                    Some(_) => {}
                }
                // the reading was taken in: it is no longer waiting
                if self.opened.get(&k) == Some(&read) {
                    self.opened.remove(&k);
                }
            }
            if let Some(version) = self.reviewed.get_mut(&k) {
                version.path.clone_from(&doc.path);
            }
            now.insert(
                k,
                Seen {
                    path: doc.path.clone(),
                    current: doc.current,
                    read: doc.read,
                },
            );
        }
        // a reading of a document the project does not have never comes to anything
        self.opened.retain(|k, _| now.contains_key(k));
        self.seen = now;
        self.saved() != before
    }

    /// Keeps the books on documents that are gone, once the disk has been looked at: a document seen to be gone is
    /// dated `now_ms`, one that is back loses its date, and a document gone for [`DELETED_KEPT_FOR_MS`] is forgotten,
    /// as are the oldest ones beyond [`MAX_DELETED`]. Returns whether `reviewed.json` changed.
    pub fn age_deleted(&mut self, now_ms: u64) -> bool {
        if !self.looked {
            return false;
        }
        let before = self.saved();
        for (k, version) in &mut self.reviewed {
            if self.seen.contains_key(k) {
                version.deleted_at = None;
            } else {
                version.deleted_at.get_or_insert(now_ms);
            }
        }
        let cutoff = now_ms.saturating_sub(DELETED_KEPT_FOR_MS);
        let mut gone: Vec<(u64, String)> = self
            .reviewed
            .iter()
            .filter_map(|(k, v)| Some((v.deleted_at?, k.clone())))
            .collect();
        gone.sort();
        // oldest first: the expired ones are in front, and the ones beyond the cap are the oldest of the rest
        let expired = gone.partition_point(|(at, _)| *at <= cutoff);
        let forgotten = gone.len().saturating_sub(MAX_DELETED).max(expired);
        for (_, k) in &gone[..forgotten] {
            self.reviewed.remove(k);
            self.held.remove(k);
            self.opened.remove(k);
        }
        self.saved() != before
    }

    /// What counts as read moved to `read`: so does the approved version, and the one it was is held when the
    /// person is the one who read.
    fn advance(&mut self, k: &str, path: &str, read: u64) {
        let old = self.reviewed.get(k).map(|v| v.hash);
        self.reviewed.insert(k.to_owned(), Version::new(path, read));
        if let Some(old) = old.filter(|old| *old != read) {
            if self.opened.get(k) == Some(&read) && !self.held.contains_key(k) {
                self.held.insert(k.to_owned(), old);
            }
        }
        // back at the version that was held: nothing is left to review
        if self.held.get(k) == Some(&read) {
            self.held.remove(k);
        }
    }

    fn moved(&mut self, from: &str, to: &str) {
        let (from_key, to_key) = (key(from), key(to));
        let keep = is_tracked(to);
        let reviewed = self.reviewed.remove(&from_key);
        let held = self.held.remove(&from_key);
        let opened = self.opened.remove(&from_key);
        let seen = self.seen.remove(&from_key);
        if !keep {
            return;
        }
        // what was read moved with the document: reading it at the new place is a new reading, not a first sight
        if let Some(seen) = seen {
            self.seen.insert(to_key.clone(), seen);
        }
        if let Some(mut version) = reviewed {
            to.clone_into(&mut version.path);
            self.reviewed.insert(to_key.clone(), version);
        }
        if let Some(hash) = held {
            self.held.insert(to_key.clone(), hash);
        }
        if let Some(hash) = opened {
            self.opened.insert(to_key, hash);
        }
    }

    /// The baseline of a document is now `hash` (a change was kept). A held version moves; without one, the
    /// approved version does. Returns whether `reviewed.json` changed.
    pub fn set_baseline(&mut self, path: &str, hash: u64) -> bool {
        let k = key(path);
        if self.held.contains_key(&k) {
            if self.reviewed.get(&k).map(|v| v.hash) == Some(hash) {
                self.held.remove(&k);
            } else {
                self.held.insert(k, hash);
            }
            return false;
        }
        self.approve(path, hash)
    }

    /// The document is approved as it is, in the version with `hash`; nothing is held any more. Returns whether
    /// `reviewed.json` changed.
    pub fn accept(&mut self, path: &str, hash: u64) -> bool {
        self.held.remove(&key(path));
        self.approve(path, hash)
    }

    fn approve(&mut self, path: &str, hash: u64) -> bool {
        let old = self.reviewed.insert(key(path), Version::new(path, hash));
        old.map(|v| (v.hash, v.deleted_at)) != Some((hash, None))
    }

    /// The document has no approved version any more (its deletion was accepted). Returns whether
    /// `reviewed.json` changed.
    pub fn forget(&mut self, path: &str) -> bool {
        let k = key(path);
        self.held.remove(&k);
        self.reviewed.remove(&k).is_some()
    }

    /// The review of the document is closed: the held version is let go.
    pub fn release(&mut self, path: &str) {
        self.held.remove(&key(path));
    }

    /// Every document that differs from its approved version, sorted by path. Nothing is listed before the disk
    /// has been looked at.
    pub fn entries(&self) -> Vec<Entry> {
        let mut out: Vec<Entry> = Vec::new();
        for (k, seen) in &self.seen {
            let Some(current) = seen.current else {
                continue;
            };
            let status = match self
                .held
                .get(k)
                .copied()
                .or_else(|| self.reviewed.get(k).map(|v| v.hash))
            {
                None => Status::New,
                Some(baseline) if baseline != current => Status::Changed,
                Some(_) => continue,
            };
            out.push(Entry {
                path: seen.path.clone(),
                status,
            });
        }
        if self.looked {
            out.extend(
                self.reviewed
                    .iter()
                    .filter(|(k, _)| !self.seen.contains_key(*k))
                    .map(|(_, v)| Entry {
                        path: v.path.clone(),
                        status: Status::Deleted,
                    }),
            );
        }
        out.sort_by_key(|e| key(&e.path));
        out
    }

    /// The versions that should have a copy: the approved ones and the held ones.
    pub fn wanted(&self) -> Vec<Want> {
        let current = |k: &str| self.seen.get(k).and_then(|s| s.current);
        let reviewed = self.reviewed.iter().map(|(k, v)| Want {
            path: v.path.clone(),
            hash: v.hash,
            current: current(k),
        });
        let held = self.held.iter().map(|(k, hash)| Want {
            path: self
                .reviewed
                .get(k)
                .map_or_else(|| k.clone(), |v| v.path.clone()),
            hash: *hash,
            current: current(k),
        });
        reviewed.chain(held).collect()
    }

    /// The hashes of the versions that are in use: their copies are kept.
    pub fn roots(&self) -> HashSet<u64> {
        self.reviewed
            .values()
            .map(|v| v.hash)
            .chain(self.held.values().copied())
            .collect()
    }
}

#[cfg(test)]
mod tests;
