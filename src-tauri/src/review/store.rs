//! The copies of the versions the person approved: text kept in a folder, one file for each version, named for
//! the xxh3 hash of its bytes (16 lowercase hex digits, the form `read.json` uses). The name tells the content,
//! so a version that two documents share is kept once, and a copy that was damaged is found out when it is read.
//!
//! A document larger than [`MAX_COPY_BYTES`] is not copied: it is only ever reported as changed.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::session::{remove_local_state, write_local_state};
use crate::util::{hash_bytes, read_with_retry};

/// The largest document that is copied.
pub const MAX_COPY_BYTES: usize = 2 * 1024 * 1024;

/// A leftover of a write that never finished (its temp file) is deleted after this long.
const STALE_PART: Duration = Duration::from_hours(1);

fn hash_to_hex(hash: u64) -> String {
    format!("{hash:016x}")
}

/// The hash a copy's file name spells, if it spells one.
fn hash_of_name(name: &str) -> Option<u64> {
    if name.len() == 16 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        u64::from_str_radix(name, 16).ok()
    } else {
        None
    }
}

#[derive(Debug)]
pub struct CopyStore {
    dir: PathBuf,
}

impl CopyStore {
    /// The copies are in `dir`, which is made when the first one is written.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_of(&self, hash: u64) -> PathBuf {
        self.dir.join(hash_to_hex(hash))
    }

    /// The hashes of the copies there are.
    pub fn hashes(&self) -> HashSet<u64> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return HashSet::new();
        };
        entries
            .flatten()
            .filter_map(|entry| hash_of_name(entry.file_name().to_str()?))
            .collect()
    }

    /// Keeps `bytes` and returns their hash, or `None` when they are too large to keep. Bytes that are kept
    /// already are not written again.
    pub fn put(&self, bytes: &[u8]) -> io::Result<Option<u64>> {
        if bytes.len() > MAX_COPY_BYTES {
            return Ok(None);
        }
        let hash = hash_bytes(bytes);
        let path = self.path_of(hash);
        // a copy of another length is a damaged one: it is written again
        let kept =
            std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() == bytes.len() as u64);
        if !kept {
            write_local_state(&path, bytes)?;
        }
        Ok(Some(hash))
    }

    /// The copy with this hash. A copy whose bytes do not hash to its name was damaged: it is deleted, and
    /// there is none.
    pub fn get(&self, hash: u64) -> Option<Vec<u8>> {
        let path = self.path_of(hash);
        let bytes = read_with_retry(&path).ok()?;
        if hash_bytes(&bytes) == hash {
            Some(bytes)
        } else {
            let _ = remove_local_state(&path);
            None
        }
    }

    /// Deletes the copies that are not in `used`, and the leftovers of writes that never finished. Returns how
    /// many copies were deleted.
    pub fn remove_unused(&self, used: &HashSet<u64>) -> usize {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return 0;
        };
        let mut removed = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let stale = match hash_of_name(name) {
                Some(hash) => !used.contains(&hash),
                None => {
                    Path::new(name).extension().is_some_and(|e| e == "part")
                        && is_older_than(&entry.path(), STALE_PART)
                }
            };
            if stale && remove_local_state(&entry.path()).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

fn is_older_than(path: &Path, age: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .is_some_and(|old| old >= age)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn store() -> (tempfile::TempDir, CopyStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CopyStore::new(dir.path().join("reviewed"));
        (dir, store)
    }

    #[test]
    fn a_copy_is_kept_under_the_hash_of_its_bytes_and_read_back() {
        let (_dir, store) = store();
        let hash = store.put(b"# Title\n").expect("put").expect("kept");
        assert_eq!(hash, hash_bytes(b"# Title\n"));
        assert!(store.dir().join(hash_to_hex(hash)).is_file());
        assert_eq!(store.get(hash), Some(b"# Title\n".to_vec()));
        assert_eq!(store.hashes(), HashSet::from([hash]));
    }

    #[test]
    fn what_is_not_there_is_not_found() {
        let (_dir, store) = store();
        assert_eq!(store.get(7), None);
        assert_eq!(store.hashes(), HashSet::new(), "no folder yet");
        assert_eq!(store.remove_unused(&HashSet::new()), 0);
    }

    #[test]
    fn bytes_kept_already_are_not_written_again() {
        let (_dir, store) = store();
        let hash = store.put(b"same").expect("put").expect("kept");
        let path = store.dir().join(hash_to_hex(hash));
        let before = fs::metadata(&path).expect("meta").modified().expect("time");
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(store.put(b"same").expect("put"), Some(hash));
        assert_eq!(
            fs::metadata(&path).expect("meta").modified().expect("time"),
            before
        );
    }

    #[test]
    fn a_document_over_the_limit_is_not_copied() {
        let (_dir, store) = store();
        let big = vec![b'x'; MAX_COPY_BYTES + 1];
        assert_eq!(store.put(&big).expect("put"), None);
        assert_eq!(store.hashes(), HashSet::new());
        let edge = vec![b'x'; MAX_COPY_BYTES];
        assert!(
            store.put(&edge).expect("put").is_some(),
            "exactly the limit"
        );
    }

    #[test]
    fn a_damaged_copy_is_deleted_when_read_and_written_again_by_the_next_put() {
        let (_dir, store) = store();
        let hash = store.put(b"original text").expect("put").expect("kept");
        let path = store.dir().join(hash_to_hex(hash));
        fs::write(&path, b"original text, damaged").expect("damage");
        assert_eq!(store.get(hash), None);
        assert!(!path.exists(), "it does not stay to be believed");
        // a damaged copy of another length is repaired by a put, even if nobody read it
        fs::write(&path, b"short").expect("damage");
        assert_eq!(store.put(b"original text").expect("put"), Some(hash));
        assert_eq!(store.get(hash), Some(b"original text".to_vec()));
    }

    #[test]
    fn copies_nobody_points_to_are_deleted() {
        let (_dir, store) = store();
        let keep = store.put(b"keep").expect("put").expect("kept");
        let drop_a = store.put(b"drop a").expect("put").expect("kept");
        let drop_b = store.put(b"drop b").expect("put").expect("kept");
        assert_eq!(store.remove_unused(&HashSet::from([keep])), 2);
        assert_eq!(store.hashes(), HashSet::from([keep]));
        assert_eq!(store.get(drop_a), None);
        assert_eq!(store.get(drop_b), None);
        assert_eq!(store.get(keep), Some(b"keep".to_vec()));
    }

    #[test]
    fn what_is_not_a_copy_is_left_alone_and_old_leftovers_of_writes_go() {
        let (_dir, store) = store();
        store.put(b"x").expect("put");
        let dir = store.dir();
        fs::write(dir.join("notes.txt"), "mine").expect("write");
        fs::write(dir.join("0123456789ABCDEF"), "upper case is no hash").expect("write");
        let old = dir.join(".0000000000000001.1.2.part");
        let fresh = dir.join(".0000000000000002.1.3.part");
        fs::write(&old, "half").expect("write");
        fs::write(&fresh, "half").expect("write");
        let long_ago = SystemTime::now() - Duration::from_hours(2);
        fs::File::options()
            .write(true)
            .open(&old)
            .expect("open")
            .set_modified(long_ago)
            .expect("time");
        assert_eq!(
            store.remove_unused(&HashSet::new()),
            2,
            "the copy and the old leftover"
        );
        assert!(dir.join("notes.txt").exists());
        assert!(dir.join("0123456789ABCDEF").exists());
        assert!(fresh.exists(), "that write may be under way");
        assert!(!old.exists());
    }

    #[test]
    fn in_a_project_the_folder_is_private_like_the_rest_of_the_local_state() {
        let project = tempfile::tempdir().expect("tempdir");
        let store = CopyStore::new(project.path().join(".trailmark/local/reviewed"));
        store.put(b"text").expect("put");
        let ignore = fs::read_to_string(project.path().join(".trailmark/local/.gitignore"))
            .expect("ignores itself");
        assert!(ignore.lines().any(|line| line == "*"));
    }
}
