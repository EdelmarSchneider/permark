//! Remembered file signatures, so a "modified" event is only reported when the content really changed.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Mutex;

use crate::util::{hash_bytes, modified_ms, read_with_retry};

/// Files larger than this are compared by size and mtime only (hashing them would stall the watcher).
const MAX_HASHED_BYTES: u64 = 32 * 1024 * 1024;

/// What we last saw of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSig {
    pub len: u64,
    pub mtime_ms: u64,
    /// xxh3 of the content (0 for files too large to hash).
    pub hash: u64,
}

/// Size and mtime from a stat, without reading the file.
pub(crate) fn stat_sig(meta: &std::fs::Metadata) -> (u64, u64) {
    (meta.len(), modified_ms(meta))
}

/// How many times a file that keeps changing under the read is read again.
const STABLE_TRIES: usize = 3;

/// Reads the file and computes its full signature.
pub(crate) fn hash_file(path: &Path, meta: &std::fs::Metadata) -> io::Result<FileSig> {
    sign(
        stat_sig(meta),
        || read_with_retry(path),
        || std::fs::metadata(path).map(|meta| stat_sig(&meta)),
    )
}

/// The signature of a file whose size and modification time were `first` when it was looked at, read with `read` and
/// looked at again with `stat`.
///
/// A signature has to describe one version of the file. The app saves by writing a temp file and renaming it over the
/// target, so a file can be replaced between the stat and the read; the old stat with the new text would then make the
/// next look at the file (which sees the new stat) report a change that nobody can tell from someone else's. The stat
/// is taken again after the read, and the read is repeated if it changed. A file that never holds still is remembered
/// with a modification time that no file has, so that the next look compares it again.
fn sign(
    first: (u64, u64),
    mut read: impl FnMut() -> io::Result<Vec<u8>>,
    mut stat: impl FnMut() -> io::Result<(u64, u64)>,
) -> io::Result<FileSig> {
    let mut before = first;
    if before.0 > MAX_HASHED_BYTES {
        return Ok(FileSig {
            len: before.0,
            mtime_ms: before.1,
            hash: 0,
        });
    }
    let mut latest = (0, 0);
    for _ in 0..STABLE_TRIES {
        let bytes = read()?;
        let after = stat()?;
        let hash = hash_bytes(&bytes);
        if after == before {
            return Ok(FileSig {
                len: before.0,
                mtime_ms: before.1,
                hash,
            });
        }
        latest = (bytes.len() as u64, hash);
        before = after;
    }
    Ok(FileSig {
        len: latest.0,
        mtime_ms: 0,
        hash: latest.1,
    })
}

/// Thread-safe map from case-folded vault path to signature (plus the display path).
#[derive(Debug, Default)]
pub struct SigStore {
    map: Mutex<HashMap<String, (String, FileSig)>>,
}

impl SigStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<FileSig> {
        self.map.lock().ok()?.get(key).map(|(_, s)| *s)
    }

    pub fn set(&self, key: String, display: &str, sig: FileSig) {
        if let Ok(mut m) = self.map.lock() {
            m.insert(key, (display.to_owned(), sig));
        }
    }

    /// Like [`SigStore::set`], but keeps a signature that is already there: the watcher saw that file
    /// change, so what it remembers is newer than anything the initial index build read earlier.
    pub fn seed(&self, key: String, display: &str, sig: FileSig) {
        if let Ok(mut m) = self.map.lock() {
            m.entry(key).or_insert_with(|| (display.to_owned(), sig));
        }
    }

    pub fn remove(&self, key: &str) -> Option<FileSig> {
        self.map.lock().ok()?.remove(key).map(|(_, s)| s)
    }

    /// Removes and returns every file below the folder `dir_key` (display path and signature).
    pub fn take_under(&self, dir_key: &str) -> Vec<(String, FileSig)> {
        let prefix = format!("{dir_key}/");
        let Ok(mut m) = self.map.lock() else {
            return Vec::new();
        };
        let keys: Vec<String> = m
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        let mut out: Vec<(String, FileSig)> = keys.iter().filter_map(|k| m.remove(k)).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.lock().map_or(0, |m| m.len())
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(hash: u64) -> FileSig {
        FileSig {
            len: 1,
            mtime_ms: 2,
            hash,
        }
    }

    #[test]
    fn set_get_remove() {
        let s = SigStore::new();
        assert!(s.is_empty());
        s.set("a.md".to_owned(), "A.md", sig(1));
        assert_eq!(s.get("a.md"), Some(sig(1)));
        assert_eq!(s.remove("a.md"), Some(sig(1)));
        assert_eq!(s.get("a.md"), None);
    }

    #[test]
    fn seed_never_overwrites_what_the_watcher_already_saw() {
        let s = SigStore::new();
        s.seed("a.md".to_owned(), "a.md", sig(1));
        assert_eq!(s.get("a.md"), Some(sig(1)));
        s.set("a.md".to_owned(), "a.md", sig(2));
        s.seed("a.md".to_owned(), "a.md", sig(3));
        assert_eq!(s.get("a.md"), Some(sig(2)));
    }

    #[test]
    fn take_under_returns_only_the_folder_contents() {
        let s = SigStore::new();
        s.set("docs/a.md".to_owned(), "Docs/a.md", sig(1));
        s.set("docs/sub/b.md".to_owned(), "Docs/sub/b.md", sig(2));
        s.set("docs2/c.md".to_owned(), "Docs2/c.md", sig(3));
        let taken = s.take_under("docs");
        let names: Vec<&str> = taken.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, ["Docs/a.md", "Docs/sub/b.md"]);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn a_file_replaced_between_the_stat_and_the_read_gets_a_signature_of_one_version() {
        // version 1 is on disk when the stat is taken; the app saves version 2 before the read
        let v1 = (5, 100);
        let v2 = (6, 200);
        let mut reads = 0;
        let mut stats = 0;
        let sig = sign(
            v1,
            || {
                reads += 1;
                Ok(b"second".to_vec())
            },
            || {
                stats += 1;
                Ok(v2)
            },
        )
        .expect("sign");
        // not (5, 100, hash of "second"): that would pass for the old version's stat with the new text
        assert_eq!(
            sig,
            FileSig {
                len: 6,
                mtime_ms: 200,
                hash: hash_bytes(b"second")
            }
        );
        assert_eq!((reads, stats), (2, 2), "read again once the stat had moved");
    }

    #[test]
    fn a_file_that_never_holds_still_is_remembered_so_that_the_next_look_compares_it_again() {
        let mut n = 0;
        let sig = sign(
            (1, 1),
            || Ok(b"x".to_vec()),
            || {
                n += 1;
                Ok((1, 1 + n))
            },
        )
        .expect("sign");
        assert_eq!(sig.mtime_ms, 0, "no file has this modification time");
        assert_eq!((sig.len, sig.hash), (1, hash_bytes(b"x")));
    }

    #[test]
    fn a_file_that_holds_still_is_read_once() {
        let mut reads = 0;
        let sig = sign(
            (3, 7),
            || {
                reads += 1;
                Ok(b"abc".to_vec())
            },
            || Ok((3, 7)),
        )
        .expect("sign");
        assert_eq!(reads, 1);
        assert_eq!(
            sig,
            FileSig {
                len: 3,
                mtime_ms: 7,
                hash: hash_bytes(b"abc")
            }
        );
    }

    #[test]
    fn a_file_too_large_to_hash_is_not_read_and_a_file_that_vanishes_is_an_error() {
        let big = sign(
            (MAX_HASHED_BYTES + 1, 9),
            || panic!("must not be read"),
            || panic!("must not be looked at"),
        )
        .expect("sign");
        assert_eq!(
            big,
            FileSig {
                len: MAX_HASHED_BYTES + 1,
                mtime_ms: 9,
                hash: 0
            }
        );
        let gone = sign(
            (1, 1),
            || Ok(b"x".to_vec()),
            || Err(io::Error::from(io::ErrorKind::NotFound)),
        );
        assert_eq!(gone.expect_err("vanished").kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn hash_file_hashes_content() {
        let d = tempfile::tempdir().expect("tempdir");
        let p = d.path().join("a.txt");
        std::fs::write(&p, "hello").expect("write");
        let meta = std::fs::metadata(&p).expect("meta");
        let a = hash_file(&p, &meta).expect("hash");
        std::fs::write(&p, "hellp").expect("write");
        let meta = std::fs::metadata(&p).expect("meta");
        let b = hash_file(&p, &meta).expect("hash");
        assert_eq!(a.len, b.len);
        assert_ne!(a.hash, b.hash);
    }
}
