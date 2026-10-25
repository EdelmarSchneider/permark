//! State files written by several cores at once (`projects.md` §9.1, §14 D51): the root tab and the project opened
//! alone in one app, two app instances, `permark mcp` resolving comments. Every write = read the file again, lay this
//! writer's own changes on top ([`merge3`]), write only while the file still holds what was read (compare-and-swap),
//! all under one OS lock per file ([`FileLock`]) so no writer ever slips in between.
//!
//! The lock lives in a folder private to the user (`permark-locks/<hash of the path>.lock`, empty files): nothing is
//! added to a project or to the profile. Windows + macOS: the user's own temp folder. Linux | BSD (`/tmp` is shared by
//! every user there): `$XDG_RUNTIME_DIR`, else the user cache folder; the folder must be a real folder (no link),
//! owned by this user, mode 0700 (tightened when wider), so nobody else can hold a lock or plant a link in it. No such
//! folder to be had -> the compare-and-swap alone still refuses to write over a version this writer has not read. A writer that holds the lock too long (a hung program;
//! the OS frees the lock of a program that ends) -> this write fails and is tried again later, never made unlocked.

use std::collections::BTreeMap;
use std::collections::{BTreeSet, HashMap};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use crate::paths::CASE_INSENSITIVE;

/// How long a writer waits for the others to finish (a write takes milliseconds; many writers queue up).
pub(crate) const WAIT: Duration = Duration::from_secs(30);
/// The wait of a writer that holds a plugin's lock meanwhile (comments, quiet: their lists wait for it). Longer -> a
/// hung holder: this write fails within seconds (the person is told | it is tried again later), lists keep answering.
/// 10 s, not 2: many writers queued on a slow disk (Windows CI) took > 2 s and refused honest writes.
pub const BRIEF_WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(5);
const OPEN_RETRY: [Duration; 3] = [
    Duration::from_millis(50),
    Duration::from_millis(150),
    Duration::from_millis(400),
];

/// Held while one writer reads, changes and writes a state file; released on drop (the OS lock first, then this
/// process's turn).
#[derive(Debug)]
pub struct FileLock {
    _file: File,
    _turn: Turn,
}

/// Inside this process the writers of one file take turns in arrival order. The OS lock alone is polled: a writer
/// that writes again and again (two tabs adding comments) took it back each time while the other one slept between
/// tries, until its wait ran out (CI 39816877707, `another PerMark writer holds this file` with no hung holder).
#[derive(Debug, Default)]
struct Turns {
    state: Mutex<TurnState>,
    cv: Condvar,
}

#[derive(Debug, Default)]
struct TurnState {
    next: u64,
    serving: u64,
    /// Tickets whose writer stopped waiting: skipped.
    gone: BTreeSet<u64>,
}

/// This writer's turn; the next one's on drop.
#[derive(Debug)]
struct Turn(Arc<Turns>);

impl Turns {
    /// The turns of the lock file `name` (one per state file ever written; tiny, kept like the lock files).
    fn of(name: &str) -> Arc<Self> {
        static ALL: OnceLock<Mutex<HashMap<String, Arc<Turns>>>> = OnceLock::new();
        let mut all = ALL
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Arc::clone(all.entry(name.to_owned()).or_default())
    }

    /// Waits for this writer's turn until `until` (`None`: still not its turn; its ticket is given up).
    fn take(self: &Arc<Self>, until: Instant) -> Option<Turn> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let ticket = state.next;
        state.next += 1;
        loop {
            if state.serving == ticket {
                return Some(Turn(Arc::clone(self)));
            }
            let now = Instant::now();
            if now >= until {
                state.gone.insert(ticket);
                return None;
            }
            state = self
                .cv
                .wait_timeout(state, until - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.serving += 1;
        loop {
            let serving = state.serving;
            if !state.gone.remove(&serving) {
                break;
            }
            state.serving += 1;
        }
        drop(state);
        self.0.cv.notify_all();
    }
}

fn held_too_long() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::WouldBlock,
        "another PerMark writer holds this file",
    )
}

/// The lock of the state file at `path` (same file = same lock in every thread and process of this user).
/// `Ok(None)`: no lock file to be had (the caller writes with compare-and-swap alone).
///
/// # Errors
/// `WouldBlock`: another writer held it longer than [`WAIT`] (the caller does not write now).
pub fn lock_for(path: &Path) -> std::io::Result<Option<FileLock>> {
    lock_within(path, WAIT)
}

/// [`lock_for`] waiting at most `wait`.
///
/// # Errors
/// `WouldBlock`: another writer held it longer than `wait`.
pub fn lock_within(path: &Path, wait: Duration) -> std::io::Result<Option<FileLock>> {
    let Some(dir) = lock_dir() else {
        return Ok(None);
    };
    let mut key = crate::paths::simplified(path)
        .to_string_lossy()
        .into_owned();
    if CASE_INSENSITIVE {
        key = key.to_lowercase();
    }
    let name = format!("{:016x}.lock", xxhash_rust::xxh3::xxh3_64(key.as_bytes()));
    // an antivirus | indexer may hold a fresh file for a moment: tried again before giving up on the lock
    let Ok(file) = crate::fsx::retry(&OPEN_RETRY, || {
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(&name))
    }) else {
        return Ok(None);
    };
    let until = Instant::now() + wait;
    // this process's writers first, in order; then the OS lock (other processes), in the time left
    let turn = Turns::of(&name).take(until).ok_or_else(held_too_long)?;
    loop {
        match file.try_lock() {
            Ok(()) => {
                return Ok(Some(FileLock {
                    _file: file,
                    _turn: turn,
                }))
            }
            Err(TryLockError::WouldBlock) if Instant::now() < until => std::thread::sleep(POLL),
            Err(TryLockError::WouldBlock) => return Err(held_too_long()),
            // a file system without locks: compare-and-swap alone
            Err(TryLockError::Error(_)) => return Ok(None),
        }
    }
}

/// The folder of the lock files, private to this user (`None`: none to be had; compare-and-swap alone).
fn lock_dir() -> Option<PathBuf> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
        let bases: Vec<PathBuf> = runtime
            .into_iter()
            .chain(dirs::cache_dir())
            .filter(|p| p.is_absolute())
            .collect();
        private_dir(&bases)
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        // per-user already (Windows `%TEMP%`, macOS `$TMPDIR`)
        let dir = std::env::temp_dir().join("permark-locks");
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }
}

/// `<base>/permark-locks` of the first base where it is (or is made) a real folder owned by this user, mode 0700.
#[cfg(all(unix, not(target_os = "macos")))]
fn private_dir(bases: &[PathBuf]) -> Option<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let me = rustix::process::geteuid().as_raw();
    bases.iter().find_map(|base| {
        let dir = base.join("permark-locks");
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir);
        let meta = std::fs::symlink_metadata(&dir).ok()?;
        if !meta.file_type().is_dir() || meta.uid() != me {
            return None;
        }
        if meta.mode() & 0o077 != 0 {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
        }
        Some(dir)
    })
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
        let held = lock_for(&path).unwrap().expect("lock");
        let other = path.clone();
        let waited = std::thread::spawn(move || {
            let start = Instant::now();
            let lock = lock_for(&other).unwrap();
            (lock.is_some(), start.elapsed())
        });
        std::thread::sleep(Duration::from_millis(200));
        drop(held);
        let (got, took) = waited.join().unwrap();
        assert!(got && took >= Duration::from_millis(150), "{took:?}");
        // another file: its own lock, no wait
        let _a = lock_for(&path).unwrap().unwrap();
        let _b = lock_for(&dir.path().join("y.json")).unwrap().unwrap();
    }
}

/// Verifier re-check round 4 (B1 fix, D51): the timeout path never writes unlocked; a dead holder frees the lock.
#[cfg(test)]
mod verify_tests {
    use super::*;

    #[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Count {
        n: u32,
    }

    impl super::super::StateData for Count {
        const VERSION: u32 = 1;
    }

    #[test]
    fn verify_lock_timeout_never_writes_unlocked() {
        let dir = tempfile::tempdir().unwrap();
        let raw = super::super::RawFile::private(dir.path().join("raw.json"), None);
        std::fs::write(raw.path(), b"before").unwrap();
        let store = super::super::StateStore::app(&super::super::AppDirs::under(dir.path()));
        let state = store.app_file::<Count>("count.json");
        std::fs::create_dir_all(state.path().parent().unwrap()).unwrap();
        std::fs::write(state.path(), br#"{"version":1,"data":{"n":7}}"#).unwrap();
        let state_before = std::fs::read(state.path()).unwrap();
        // another writer (own handle) holds both locks longer than the wait
        let held_raw = lock_for(raw.path()).unwrap().expect("lock");
        let held_state = lock_for(state.path()).unwrap().expect("lock");
        let start = Instant::now();
        let t_raw = {
            let raw = raw.clone();
            std::thread::spawn(move || raw.update(|_| Some(b"after".to_vec())).is_err())
        };
        let t_state = {
            let state = state.clone();
            std::thread::spawn(move || state.update(|_| Some(Count { n: 99 })).is_err())
        };
        assert!(t_raw.join().unwrap(), "raw update must fail on timeout");
        assert!(t_state.join().unwrap(), "state update must fail on timeout");
        assert!(start.elapsed() >= WAIT, "{:?}", start.elapsed());
        assert_eq!(std::fs::read(raw.path()).unwrap(), b"before");
        assert_eq!(std::fs::read(state.path()).unwrap(), state_before);
        drop((held_raw, held_state));
        // freed: writes go through again
        raw.update(|_| Some(b"after".to_vec())).unwrap();
        assert_eq!(std::fs::read(raw.path()).unwrap(), b"after");
    }

    /// Writers of one file in this process take turns: one writing again and again never starves another, also with
    /// the brief wait (CI 39816877707: two tabs adding comments, one gave up after 2 s with no hung holder).
    #[test]
    fn writers_of_one_file_take_turns_and_none_starves() {
        let dir = tempfile::tempdir().unwrap();
        let raw =
            super::super::RawFile::private(dir.path().join("turns.json"), None).brief_lock_wait();
        let threads: Vec<_> = (0..4)
            .map(|t| {
                let raw = raw.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        raw.update(|now| {
                            let mut bytes = now.map(<[u8]>::to_vec).unwrap_or_default();
                            bytes.push(b'a' + t);
                            Some(bytes)
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(std::fs::read(raw.path()).unwrap().len(), 400);
        // a writer that gave up its ticket never blocks the ones after it
        let held = lock_for(raw.path()).unwrap().expect("lock");
        let late = raw.clone();
        let gave_up = std::thread::spawn(move || {
            lock_within(late.path(), Duration::from_millis(50)).is_err()
        });
        assert!(gave_up.join().unwrap());
        drop(held);
        let start = Instant::now();
        assert!(lock_within(raw.path(), Duration::from_secs(5))
            .unwrap()
            .is_some());
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    /// N10 (verifier round 4): a writer holding a plugin's lock gives up after the brief wait, not the long one.
    #[test]
    fn a_brief_wait_writer_gives_up_long_before_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let raw =
            super::super::RawFile::private(dir.path().join("raw.json"), None).brief_lock_wait();
        std::fs::write(raw.path(), b"before").unwrap();
        let held = lock_for(raw.path()).unwrap().expect("lock");
        let start = Instant::now();
        assert!(raw.update(|_| Some(b"after".to_vec())).is_err());
        let took = start.elapsed();
        assert!(took >= BRIEF_WAIT && took < WAIT, "{took:?}");
        assert_eq!(std::fs::read(raw.path()).unwrap(), b"before");
        drop(held);
        raw.update(|_| Some(b"after".to_vec())).unwrap();
        assert_eq!(std::fs::read(raw.path()).unwrap(), b"after");
    }

    #[test]
    fn verify_lock_of_a_panicked_holder_is_free_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        let p = path.clone();
        let r = std::thread::spawn(move || {
            let _held = lock_for(&p).unwrap().expect("lock");
            panic!("holder dies");
        })
        .join();
        assert!(r.is_err());
        let start = Instant::now();
        let got = lock_for(&path).unwrap();
        assert!(got.is_some() && start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn verify_same_file_other_case_shares_the_lock_where_case_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Case.json");
        let other = dir.path().join("case.JSON");
        let held = lock_for(&path).unwrap().expect("lock");
        let start = Instant::now();
        let t = std::thread::spawn(move || {
            // either shares the lock (waits) or not; on case-insensitive systems it must wait
            let _ = lock_for(&other);
        });
        std::thread::sleep(Duration::from_millis(300));
        let finished = t.is_finished();
        if CASE_INSENSITIVE {
            assert!(!finished, "case variant must share the lock");
        }
        drop(held);
        t.join().unwrap();
        let _ = start;
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod private_dir_tests {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    use super::*;

    #[test]
    fn the_lock_folder_is_private_and_a_link_or_a_foreign_folder_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("fresh");
        let got = private_dir(std::slice::from_ref(&fresh)).unwrap();
        assert_eq!(got, fresh.join("permark-locks"));
        let meta = std::fs::metadata(&got).unwrap();
        assert_eq!(meta.mode() & 0o777, 0o700);
        assert_eq!(meta.uid(), rustix::process::geteuid().as_raw());
        // made by an older build (0755): tightened
        let wide = dir.path().join("wide");
        std::fs::create_dir_all(wide.join("permark-locks")).unwrap();
        std::fs::set_permissions(
            wide.join("permark-locks"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let got = private_dir(std::slice::from_ref(&wide)).unwrap();
        assert_eq!(std::fs::metadata(&got).unwrap().mode() & 0o777, 0o700);
        // a link in place of the folder: never used, the next base is
        let linked = dir.path().join("linked");
        std::fs::create_dir_all(&linked).unwrap();
        std::os::unix::fs::symlink(&fresh, linked.join("permark-locks")).unwrap();
        assert_eq!(
            private_dir(&[linked.clone(), wide.clone()]),
            Some(wide.join("permark-locks"))
        );
        assert_eq!(private_dir(&[linked]), None);
        // a file in the way
        let file = dir.path().join("file");
        std::fs::create_dir_all(&file).unwrap();
        std::fs::write(file.join("permark-locks"), b"x").unwrap();
        assert_eq!(private_dir(&[file]), None);
    }

    #[test]
    fn locks_still_work_in_the_private_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let held = lock_for(&path).unwrap();
        assert!(held.is_some(), "a lock to be had on this system");
    }
}
