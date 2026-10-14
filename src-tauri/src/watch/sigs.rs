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

/// Reads the file and computes its full signature.
pub(crate) fn hash_file(path: &Path, meta: &std::fs::Metadata) -> io::Result<FileSig> {
    let (len, mtime_ms) = stat_sig(meta);
    let hash = if len > MAX_HASHED_BYTES {
        0
    } else {
        hash_bytes(&read_with_retry(path)?)
    };
    Ok(FileSig {
        len,
        mtime_ms,
        hash,
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

    /// Forgets everything (events were lost, so remembered signatures may be stale).
    pub fn clear(&self) {
        if let Ok(mut m) = self.map.lock() {
            m.clear();
        }
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
