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

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use serde::Serialize;

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
}

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
    /// Documents the person is reading: when their read version moves, the one before is held.
    opened: BTreeSet<String>,
    /// The documents as the last look at the project showed them.
    seen: BTreeMap<String, Seen>,
    /// The disk has been looked at: a document that is not there is gone.
    looked: bool,
}

impl Book {
    /// `saved` is what `reviewed.json` held: the approved version of each document, by path.
    pub fn new(saved: BTreeMap<String, u64>) -> Self {
        let reviewed = saved
            .into_iter()
            .filter(|(path, _)| is_tracked(path))
            .map(|(path, hash)| (key(&path), Version { path, hash }))
            .collect();
        Self {
            reviewed,
            ..Self::default()
        }
    }

    /// What `reviewed.json` holds.
    pub fn saved(&self) -> BTreeMap<String, u64> {
        self.reviewed
            .values()
            .map(|v| (v.path.clone(), v.hash))
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

    /// The person is reading the document: if the version that counts as read moves with the next look at
    /// the project, the one before it is held.
    pub fn opening(&mut self, path: &str) {
        self.opened.insert(key(path));
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
                        self.reviewed.entry(k.clone()).or_insert_with(|| Version {
                            path: doc.path.clone(),
                            hash: read,
                        });
                    }
                    Some(seen) if seen.read != Some(read) => self.advance(&k, &doc.path, read),
                    Some(_) => {}
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
        self.opened.clear();
        self.seen = now;
        self.saved() != before
    }

    /// What counts as read moved to `read`: so does the approved version, and the one it was is held when the
    /// person is the one who read.
    fn advance(&mut self, k: &str, path: &str, read: u64) {
        let old = self.reviewed.get(k).map(|v| v.hash);
        self.reviewed.insert(
            k.to_owned(),
            Version {
                path: path.to_owned(),
                hash: read,
            },
        );
        if let Some(old) = old.filter(|old| *old != read) {
            if self.opened.contains(k) && !self.held.contains_key(k) {
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
        if opened {
            self.opened.insert(to_key);
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
        let old = self.reviewed.insert(
            key(path),
            Version {
                path: path.to_owned(),
                hash,
            },
        );
        old.map(|v| v.hash) != Some(hash)
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
