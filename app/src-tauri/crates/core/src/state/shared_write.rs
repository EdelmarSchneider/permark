//! State files written by several cores at once (`projects.md` §9.1, §14 D51): the root tab and the project opened
//! alone in one app, two app instances, `permark mcp` resolving comments. Every write = read the file again, lay this
//! writer's own changes on top ([`merge3`]), write only while the file still holds what was read (compare-and-swap),
//! all under one OS lock per file ([`FileLock`]) so no writer ever slips in between.
//!
//! The lock lives in the temp folder (`permark-locks/<hash of the path>.lock`, empty files): nothing is added to a
//! project or to the profile. No lock (temp folder not writable, a writer that holds it too long) -> the
//! compare-and-swap alone still refuses to write over a version this writer has not read; the caller retries.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::paths::CASE_INSENSITIVE;

/// How long a writer waits for another one to finish (a write takes milliseconds).
const WAIT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(5);

/// Held while one writer reads, changes and writes a state file; released on drop.
#[derive(Debug)]
pub struct FileLock {
    _file: File,
}

/// The lock of the state file at `path` (same file = same lock in every thread and process of this user).
/// `None`: no lock to be had now (the caller still writes with compare-and-swap).
#[must_use]
pub fn lock_for(path: &Path) -> Option<FileLock> {
    let dir = std::env::temp_dir().join("permark-locks");
    std::fs::create_dir_all(&dir).ok()?;
    let mut key = dunce::simplified(path).to_string_lossy().into_owned();
    if CASE_INSENSITIVE {
        key = key.to_lowercase();
    }
    let name = format!("{:016x}.lock", xxhash_rust::xxh3::xxh3_64(key.as_bytes()));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(name))
        .ok()?;
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Some(FileLock { _file: file }),
            Err(TryLockError::WouldBlock) if start.elapsed() < WAIT => std::thread::sleep(POLL),
            Err(_) => return None,
        }
    }
}

/// Three-way merge of keyed entries: what this writer changed since `base` (added, changed, removed) goes on top of
/// `fresh` (the file as it is now); every other key keeps the file's value. Another writer's entries survive; a key
/// another writer removed stays removed (no tombstones needed: `base` says what this writer saw).
#[must_use]
pub fn merge3<K: Ord + Clone, V: PartialEq + Clone>(
    base: &BTreeMap<K, V>,
    mine: &BTreeMap<K, V>,
    fresh: BTreeMap<K, V>,
) -> BTreeMap<K, V> {
    let mut out = fresh;
    for (key, value) in mine {
        if base.get(key) != Some(value) {
            out.insert(key.clone(), value.clone());
        }
    }
    for key in base.keys() {
        if !mine.contains_key(key) {
            out.remove(key);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, u32)]) -> BTreeMap<String, u32> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    #[test]
    fn merge3_keeps_both_writers_and_both_removals() {
        let base = map(&[("a", 1), ("b", 1), ("c", 1)]);
        // this writer: changed a, removed b, added d
        let mine = map(&[("a", 2), ("c", 1), ("d", 1)]);
        // the other writer: added e, removed c
        let fresh = map(&[("a", 1), ("b", 1), ("e", 1)]);
        let out = merge3(&base, &mine, fresh);
        assert_eq!(out, map(&[("a", 2), ("d", 1), ("e", 1)]));
    }

    #[test]
    fn merge3_with_no_own_change_is_the_file() {
        let base = map(&[("a", 1)]);
        let fresh = map(&[("b", 7)]);
        assert_eq!(merge3(&base, &base.clone(), fresh.clone()), fresh);
    }

    #[test]
    fn raw_updates_from_many_threads_keep_every_change() {
        let dir = tempfile::tempdir().unwrap();
        let file = super::super::RawFile::private(dir.path().join("list.json"), None);
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let file = file.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        file.update(|now| {
                            let mut list: Vec<String> = now
                                .and_then(|b| serde_json::from_slice(b).ok())
                                .unwrap_or_default();
                            list.push(format!("{t}-{i}"));
                            serde_json::to_vec(&list).ok()
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let list: Vec<String> =
            serde_json::from_slice(&std::fs::read(file.path()).unwrap()).unwrap();
        assert_eq!(list.len(), 200);
    }

    #[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Count {
        n: u32,
    }

    impl super::super::StateData for Count {
        const VERSION: u32 = 1;
    }

    #[test]
    fn state_updates_add_up_and_a_file_of_another_layout_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let store = super::super::StateStore::app(&super::super::AppDirs::under(dir.path()));
        let file = store.app_file::<Count>("count.json");
        std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        // a newer version's file: kept beside, never written over
        std::fs::write(file.path(), br#"{"version":9,"data":{"n":100}}"#).unwrap();
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let file = file.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        file.update(|now| {
                            Some(Count {
                                n: now.map_or(0, |c| c.n) + 1,
                            })
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(file.load(), super::super::Loaded::Found(Count { n: 80 }));
        let aside = std::fs::read_dir(file.path().parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".broken-"))
            .count();
        assert_eq!(aside, 1);
    }

    #[test]
    fn one_lock_per_file_across_threads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        let held = lock_for(&path).expect("lock");
        let other = path.clone();
        let waited = std::thread::spawn(move || {
            let start = Instant::now();
            let lock = lock_for(&other);
            (lock.is_some(), start.elapsed())
        });
        std::thread::sleep(Duration::from_millis(200));
        drop(held);
        let (got, took) = waited.join().unwrap();
        assert!(got && took >= Duration::from_millis(150), "{took:?}");
        // another file: its own lock, no wait
        let _a = lock_for(&path).unwrap();
        let _b = lock_for(&dir.path().join("y.json")).unwrap();
    }
}
