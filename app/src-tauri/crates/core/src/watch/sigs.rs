//! What the watch remembers of files it has seen: size, modified time, content hash. Events are hints; the disk is
//! compared with these to drop no-op changes (same bytes saved again, antivirus touches, sync clients) and to pair a
//! removal with a creation of the same content (a move).

use std::collections::HashMap;
use std::fs::{self, Metadata};
use std::io::Read;
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::UNIX_EPOCH;

use crate::fsx::{hash, retry, RETRY_DELAYS};
use crate::tree::key_of;

/// Files up to this size are hashed (read whole); bigger ones are compared by size + time only.
pub const HASH_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sig {
    pub len: u64,
    pub mtime_ms: u64,
    /// xxh3 of the bytes; 0 = unknown (too big, unreadable).
    pub hash: u64,
}

/// Size and modified time (ms since the epoch) of a file.
#[must_use]
pub fn stat_sig(meta: &Metadata) -> (u64, u64) {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    (meta.len(), mtime)
}

/// Signature of the file at `path` (hashing it when small enough). `None`: unreadable now.
#[must_use]
pub fn sig_of(path: &Path, meta: &Metadata) -> Option<Sig> {
    let (len, mtime_ms) = stat_sig(meta);
    if len > HASH_LIMIT {
        return Some(Sig {
            len,
            mtime_ms,
            hash: 0,
        });
    }
    let bytes = retry(&RETRY_DELAYS, || {
        let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
        fs::File::open(path)?
            .take(HASH_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    })
    .ok()?;
    Some(Sig {
        len: bytes.len() as u64,
        mtime_ms,
        hash: hash(&bytes).0,
    })
}

#[derive(Debug, Default)]
pub struct SigStore {
    /// Path key -> (path as shown, signature).
    map: Mutex<HashMap<String, (String, Sig)>>,
}

impl SigStore {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, (String, Sig)>> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<Sig> {
        self.map().get(key).map(|(_, sig)| *sig)
    }

    pub fn set(&self, key: String, shown: &str, sig: Sig) {
        self.map().insert(key, (shown.to_owned(), sig));
    }

    pub fn remove(&self, key: &str) -> Option<Sig> {
        self.map().remove(key).map(|(_, sig)| sig)
    }

    /// Signatures of everything below the folder `key`, removed from the store.
    pub fn take_under(&self, key: &str) -> Vec<(String, Sig)> {
        let prefix = format!("{key}/");
        let mut map = self.map();
        let keys: Vec<String> = map
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        keys.into_iter().filter_map(|k| map.remove(&k)).collect()
    }

    /// A folder moved: everything below it moves along.
    pub fn rename_under(&self, from: &str, to: &str) {
        let from_key = key_of(from);
        let prefix = format!("{from_key}/");
        let mut map = self.map();
        let moved: Vec<String> = map
            .keys()
            .filter(|k| **k == from_key || k.starts_with(&prefix))
            .cloned()
            .collect();
        for key in moved {
            if let Some((shown, sig)) = map.remove(&key) {
                let rest = shown.get(from.len()..).unwrap_or("");
                let new_shown = format!("{to}{rest}");
                map.insert(key_of(&new_shown), (new_shown, sig));
            }
        }
    }
}
