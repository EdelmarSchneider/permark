//! Derived-index freshness: which files of a folder changed since an index was built.
//!
//! - Index keeps own [`Snapshot`] w/ its data (atomic, consumer's job); disk = truth at [`check`] time.
//! - Pure: no state, threads, watch. Reads only candidate files (new | len/mtime differ) of the given scan.
//! - Racy rule (git-index model): entry w/ mtime >= scan start - 2 s | unknown (0) stored unverified -> `changed` next check.
//! - Limit: files > `HASH_LIMIT` compared by len + mtime only.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fsx;
use crate::paths::key_of;
use crate::tree::{Scan, ScanFile};
use crate::watch::HASH_LIMIT;

/// Window before scan start in which an mtime is not trusted (FAT 2 s, SMB, same-tick edits).
const RACY_MS: i64 = 2000;

/// Files an index was built from, keyed by [`key_of`] of the project path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Opaque consumer identity (model, revision, ...); differs -> full rebuild.
    pub fingerprint: String,
    pub files: BTreeMap<String, Entry>,
}

/// What a snapshot remembers of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Shown project path (latest spelling).
    pub path: String,
    pub len: u64,
    /// Modified time, ms since the epoch.
    pub mtime_ms: i64,
    /// xxh3 of the bytes; `None` = too big | unreadable.
    pub hash: Option<u64>,
    /// Unverified (mtime too close to the check | unknown; small file unreadable) -> reported changed next check.
    pub racy: bool,
}

/// Result of [`check`]: project paths (shown spelling), sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Stale {
        changed: Vec<String>,
        added: Vec<String>,
        removed: Vec<String>,
    },
    /// Scan cut off: no removals known; never fresh.
    Partial {
        changed: Vec<String>,
        added: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// `prev` `None` | other fingerprint: consumer MUST drop all its data first (every file listed as added).
    pub rebuild: bool,
    pub freshness: Freshness,
    /// Snapshot to store w/ the index once it processed `freshness`; store it even when `Fresh` (touch, case rename
    /// update entries).
    pub next: Snapshot,
    /// Files read + hashed by this check.
    pub hashed: usize,
}

/// Compares `prev` w/ the folder as `scan` finds it now. `prev` `None` | other `fingerprint` -> every file added.
/// `scan` MUST be a folder scan (`Explorer::scan_folder` | `WindowFiles::scan_folder`: size, mtime, `abs` per file);
/// a root scan (size, mtime 0) detects nothing.
///
/// # Errors
/// Error of `scan` (nothing compared).
pub fn check<E>(
    prev: Option<&Snapshot>,
    fingerprint: &str,
    scan: impl FnOnce() -> Result<Scan, E>,
) -> Result<Checked, E> {
    // before the scan: a file written during it is racy
    let racy_from = now_ms().saturating_sub(RACY_MS);
    let scan = scan()?;
    let prev = prev.filter(|p| p.fingerprint == fingerprint);
    let mut next = Snapshot {
        fingerprint: fingerprint.to_owned(),
        files: BTreeMap::new(),
    };
    let (mut changed, mut added, mut hashed) = (Vec::new(), Vec::new(), 0);
    for file in &scan.files {
        let key = key_of(&file.path);
        if next.files.contains_key(&key) {
            // same identity seen twice (Linux: NFC + NFD names): first tracked, other always reported changed
            changed.push(file.path.clone());
            continue;
        }
        let old = prev.and_then(|p| p.files.get(&key));
        let same_stat = old.is_some_and(|o| o.len == file.size && o.mtime_ms == file.modified_ms);
        let entry = match old {
            Some(o) if same_stat && !o.racy => Entry {
                path: file.path.clone(),
                ..o.clone()
            },
            _ => {
                let hash = hash_of(file);
                hashed += usize::from(hash.is_some());
                match old {
                    None => added.push(file.path.clone()),
                    Some(o) if o.racy || hash.is_none() || hash != o.hash => {
                        changed.push(file.path.clone());
                    }
                    Some(_) => {}
                }
                Entry {
                    path: file.path.clone(),
                    len: file.size,
                    mtime_ms: file.modified_ms,
                    hash,
                    // small file unreadable now: retried (reported changed) next check
                    racy: hash.is_none() && file.size <= HASH_LIMIT,
                }
            }
        };
        next.files.insert(key, entry);
    }
    let mut removed = Vec::new();
    if let Some(prev) = prev {
        for (key, entry) in &prev.files {
            if next.files.contains_key(key) {
                continue;
            }
            if scan.cut_off {
                next.files.insert(key.clone(), entry.clone());
            } else {
                removed.push(entry.path.clone());
            }
        }
    }
    for entry in next.files.values_mut() {
        // mtime 0: unknown (stat failed) -> never trusted
        entry.racy = entry.racy || entry.mtime_ms <= 0 || entry.mtime_ms >= racy_from;
    }
    changed.sort();
    added.sort();
    removed.sort();
    let freshness = if scan.cut_off {
        Freshness::Partial { changed, added }
    } else if changed.is_empty() && added.is_empty() && removed.is_empty() {
        Freshness::Fresh
    } else {
        Freshness::Stale {
            changed,
            added,
            removed,
        }
    };
    Ok(Checked {
        rebuild: prev.is_none(),
        freshness,
        next,
        hashed,
    })
}

/// Content hash of a scanned file; `None`: too big, no disk path, unreadable.
fn hash_of(file: &ScanFile) -> Option<u64> {
    if file.size > HASH_LIMIT {
        return None;
    }
    let abs = file.abs.as_deref()?;
    fsx::read_bytes(abs, HASH_LIMIT)
        .ok()
        .map(|b| fsx::hash(&b).0)
}

/// Clock before the epoch (unreadable) -> `i64::MAX`: everything racy (fails safe).
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(i64::MAX, |d| {
            i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests;
