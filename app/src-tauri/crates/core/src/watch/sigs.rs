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

#[derive(Debug)]
struct Held {
    /// Path as shown.
    shown: String,
    sig: Sig,
    /// `false`: only read by a window (see [`SigStore`]).
    watched: bool,
}

/// What the watch knows about each path. A signature is held in one of two states:
/// - watched ([`SigStore::set`]): the watch looked at the file for a hint and told the change, or found the same
///   bytes. It is what the subscribers know of the file. (Without live updates the document a window shows is held
///   so from the start: only its polls tell of it.)
/// - read by a window ([`SigStore::note_read`]): a window shows the file, the watch never looked at it and nothing was
///   told about the path. It counts as the content before a hint all the same (a touch that leaves the bytes is no
///   news, an outside save is a `Modified`, a removal pairs with a creation of the same content), with one exception
///   ([`SigStore::previous`]): a hint that the file was created while it still is exactly as the window read it. The
///   creation is then what the window read (a file the app made and showed before its events had their quiet moment)
///   and it is still news.
///
/// A watched signature is never replaced by a read one: the next hint is compared with what was told last.
#[derive(Debug, Default)]
pub struct SigStore {
    /// Path key -> what is held for it.
    map: Mutex<HashMap<String, Held>>,
}

impl SigStore {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Held>> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The signature held for `key`, in either state.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Sig> {
        self.map().get(key).map(|held| held.sig)
    }

    /// The signature a hint about the file at `key` is compared with. `created`: the hint says the file was created;
    /// `stat`: its size and modified time now ([`stat_sig`]). `None`: nothing, whatever is there is news.
    #[must_use]
    pub fn previous(&self, key: &str, created: bool, stat: (u64, u64)) -> Option<Sig> {
        let map = self.map();
        let held = map.get(key)?;
        let as_read = (held.sig.len, held.sig.mtime_ms) == stat;
        (held.watched || !(created && as_read)).then_some(held.sig)
    }

    /// `sig` is what the subscribers know of the file from now on (watched).
    pub fn set(&self, key: String, shown: &str, sig: Sig) {
        self.map().insert(
            key,
            Held {
                shown: shown.to_owned(),
                sig,
                watched: true,
            },
        );
    }

    /// A window read the file as `sig`. Kept only for a path the store holds nothing for.
    pub fn note_read(&self, key: String, shown: &str, sig: Sig) {
        self.map().entry(key).or_insert_with(|| Held {
            shown: shown.to_owned(),
            sig,
            watched: false,
        });
    }

    pub fn remove(&self, key: &str) -> Option<Sig> {
        self.map().remove(key).map(|held| held.sig)
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
        keys.into_iter()
            .filter_map(|k| map.remove(&k))
            .map(|held| (held.shown, held.sig))
            .collect()
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
            if let Some(held) = map.remove(&key) {
                let rest = held.shown.get(from.len()..).unwrap_or("");
                let shown = format!("{to}{rest}");
                map.insert(key_of(&shown), Held { shown, ..held });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(len: u64, mtime_ms: u64, hash: u64) -> Sig {
        Sig {
            len,
            mtime_ms,
            hash,
        }
    }

    #[test]
    fn a_watched_signature_is_what_every_hint_is_compared_with() {
        let store = SigStore::default();
        assert_eq!(store.previous("a.md", true, (3, 10)), None, "unknown");
        store.set("a.md".to_owned(), "a.md", sig(3, 10, 7));
        for created in [false, true] {
            for stat in [(3, 10), (3, 11), (4, 10)] {
                assert_eq!(store.previous("a.md", created, stat), Some(sig(3, 10, 7)));
            }
        }
    }

    #[test]
    fn a_signature_a_window_read_counts_except_for_the_creation_it_read() {
        let store = SigStore::default();
        store.note_read("a.md".to_owned(), "a.md", sig(3, 10, 7));
        assert_eq!(store.get("a.md"), Some(sig(3, 10, 7)));
        // created, and still exactly as read: the creation was never told
        assert_eq!(store.previous("a.md", true, (3, 10)), None);
        // touched | saved after the reading (whatever the hint calls it), or no creation at all: what was read counts
        assert_eq!(store.previous("a.md", true, (3, 11)), Some(sig(3, 10, 7)));
        assert_eq!(store.previous("a.md", true, (9, 10)), Some(sig(3, 10, 7)));
        assert_eq!(store.previous("a.md", false, (3, 10)), Some(sig(3, 10, 7)));
        assert_eq!(store.previous("a.md", false, (3, 11)), Some(sig(3, 10, 7)));
    }

    #[test]
    fn a_reading_never_replaces_what_is_held_and_a_watched_signature_replaces_a_reading() {
        let store = SigStore::default();
        store.note_read("a.md".to_owned(), "a.md", sig(3, 10, 7));
        // read again after a change the watch has not looked at yet: the change is still to be told
        store.note_read("a.md".to_owned(), "a.md", sig(4, 20, 8));
        assert_eq!(store.get("a.md"), Some(sig(3, 10, 7)));
        store.set("a.md".to_owned(), "a.md", sig(4, 20, 8));
        assert_eq!(store.previous("a.md", true, (4, 20)), Some(sig(4, 20, 8)));
        store.note_read("a.md".to_owned(), "a.md", sig(5, 30, 9));
        assert_eq!(store.previous("a.md", true, (5, 30)), Some(sig(4, 20, 8)));
    }

    #[test]
    fn a_reading_goes_and_moves_with_its_path_like_any_signature() {
        let store = SigStore::default();
        store.note_read("docs/a.md".to_owned(), "docs/a.md", sig(3, 10, 7));
        store.rename_under("docs", "notes");
        assert_eq!(store.get("docs/a.md"), None);
        // still a reading at the new place
        assert_eq!(store.previous("notes/a.md", true, (3, 10)), None);
        assert_eq!(
            store.take_under("notes"),
            [("notes/a.md".to_owned(), sig(3, 10, 7))]
        );
        store.note_read("b.md".to_owned(), "b.md", sig(1, 1, 1));
        assert_eq!(store.remove("b.md"), Some(sig(1, 1, 1)));
        assert_eq!(store.get("b.md"), None);
    }
}
