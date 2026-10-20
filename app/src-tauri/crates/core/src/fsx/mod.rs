//! IO helpers: atomic durable writes, text reads with encoding + line-ending facts, content hash, retry on
//! transient Windows locks.

mod decode;
#[cfg(windows)]
mod win;

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

pub use decode::{decode, Decoded, Encoding, LineEnding, BINARY_SNIFF};

use crate::error::{ErrorKind, IpcError};

/// xxh3-64 of file bytes; hex 16 on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentHash(pub u64);

impl ContentHash {
    #[must_use]
    pub fn hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

impl Serialize for ContentHash {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

#[must_use]
pub fn hash(bytes: &[u8]) -> ContentHash {
    ContentHash(xxhash_rust::xxh3::xxh3_64(bytes))
}

/// Sends files to the system recycle bin (the files stay where they are when it fails).
///
/// # Errors
/// The bin refused (the reason as text).
pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
    trash::delete_all(paths).map_err(|e| e.to_string())
}

/// Retry delays for transient Windows sharing | lock violations.
pub const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(20),
    Duration::from_millis(60),
    Duration::from_millis(150),
];

/// Windows sharing (32) | lock (33) violation, or access denied (antivirus | sync client holding the file).
fn is_transient(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33)) || error.kind() == io::ErrorKind::PermissionDenied
}

/// Runs `op`, retrying transient errors after each of `delays`.
///
/// # Errors
/// Last error of `op` (non-transient errors at once).
pub fn retry<T>(delays: &[Duration], mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut attempt = 0;
    loop {
        match op() {
            Ok(value) => return Ok(value),
            Err(error) if attempt < delays.len() && is_transient(&error) => {
                std::thread::sleep(delays[attempt]);
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FsxError {
    #[error("{path} is larger than {limit} bytes")]
    TooLarge { path: String, limit: u64 },
    #[error("{0} is a binary file")]
    Binary(String),
    #[error("cannot access {path}: {source}")]
    Io { path: String, source: io::Error },
}

impl FsxError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

impl From<FsxError> for IpcError {
    fn from(error: FsxError) -> Self {
        match error {
            FsxError::TooLarge { path, .. } => IpcError::new(ErrorKind::TooLarge).subject(path),
            FsxError::Binary(path) => IpcError::new(ErrorKind::Binary).subject(path),
            FsxError::Io { path, source } => IpcError::io(path, &source),
        }
    }
}

/// Reads a file of at most `limit` bytes.
///
/// # Errors
/// `TooLarge` (checked before reading), `Io`.
pub fn read_bytes(path: &Path, limit: u64) -> Result<Vec<u8>, FsxError> {
    let bytes = retry(&RETRY_DELAYS, || {
        let file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        if len > limit {
            return Ok(None);
        }
        let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
        // `take`: a file growing while read never exceeds the limit
        file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    })
    .map_err(|e| FsxError::io(path, e))?;
    let shown = || path.display().to_string();
    let bytes = bytes.ok_or_else(|| FsxError::TooLarge {
        path: shown(),
        limit,
    })?;
    if bytes.len() as u64 > limit {
        return Err(FsxError::TooLarge {
            path: shown(),
            limit,
        });
    }
    Ok(bytes)
}

/// Reads a text file of at most `limit` bytes.
///
/// # Errors
/// `TooLarge` (checked before reading), `Binary` (NUL in the head, no UTF-16 BOM), `Io`.
pub fn read_text(path: &Path, limit: u64) -> Result<Decoded, FsxError> {
    let bytes = read_bytes(path, limit)?;
    decode(&bytes).ok_or_else(|| FsxError::Binary(path.display().to_string()))
}

/// Scratch files of editors and tools (`*.tmp`, `*~`, `.#*`, `*.swp`, Vim's `4913`) and the hidden
/// `.<name>.<pid>.<n>.part` temps [`write_atomic`] leaves for an instant: never news, never listed.
#[must_use]
pub fn is_scratch_name(name: &str) -> bool {
    let has_ext = |wanted: &str| {
        Path::new(name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(wanted))
    };
    name.ends_with('~')
        || name.starts_with(".#")
        || name == "4913"
        || (name.starts_with('.') && has_ext("part"))
        || ["tmp", "swp", "swo", "swx"].into_iter().any(has_ext)
}

/// Temp files of this process: two writes of one file never share a temp.
static TEMP_COUNT: AtomicU64 = AtomicU64::new(0);

/// Hidden temp next to `path`: `.<name>.<pid>.<n>.part`.
fn temp_path(path: &Path) -> PathBuf {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
    let n = TEMP_COUNT.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{name}.{}.{n}.part", std::process::id()))
}

/// Hidden temp in `dir` for something being built under another name (`.import.<pid>.<n>.part`): short, so a name
/// at the length limit still has a temp.
pub(crate) fn scratch_in(dir: &Path) -> PathBuf {
    let n = TEMP_COUNT.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".import.{}.{n}.part", std::process::id()))
}

/// Writes `bytes` atomically + durably: temp in the same folder -> fsync -> rename over target (Windows:
/// `MoveFileExW` replace + write-through; Unix: folder fsync). Power loss leaves old or new file, never empty.
/// Target permissions carried over (Unix mode bits | Windows read-only). Parent folders created.
///
/// # Errors
/// IO failure; temp removed, target untouched.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_checked(path, bytes, || Ok(true)).map(|_| ())
}

/// [`write_atomic`] with `check` run after the temp is written and right before each rename attempt (a locked target
/// is retried, and whatever another writer did meanwhile is seen): `Ok(false)` leaves the target untouched (another
/// writer got in) and answers `Ok(false)`.
///
/// # Errors
/// IO failure | `check` failing; temp removed, target untouched.
pub fn write_atomic_checked(
    path: &Path,
    bytes: &[u8],
    mut check: impl FnMut() -> io::Result<bool>,
) -> io::Result<bool> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = temp_path(path);
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| {
            if let Some(meta) = std::fs::metadata(path)
                .ok()
                .filter(std::fs::Metadata::is_file)
            {
                // best effort: a file system without permissions still gets the bytes
                let _ = std::fs::set_permissions(&tmp, meta.permissions());
            }
            retry(&RETRY_DELAYS, || {
                if !check()? {
                    return Ok(false);
                }
                rename_durably(&tmp, path).map(|()| true)
            })
        });
    if !matches!(result, Ok(true)) {
        discard(&tmp);
    }
    result
}

fn rename_durably(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        win::move_replace_write_through(from, to)
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(from, to)?;
        if let Some(dir) = to.parent() {
            let dir = if dir.as_os_str().is_empty() {
                Path::new(".")
            } else {
                dir
            };
            // folder flush: some file systems cannot; the rename happened anyway
            if let Ok(folder) = std::fs::File::open(dir) {
                let _ = folder.sync_all();
            }
        }
        Ok(())
    }
}

/// Removes an unused temp (may carry the read-only flag of the file it was to replace).
fn discard(tmp: &Path) {
    if std::fs::remove_file(tmp).is_err() {
        if let Ok(meta) = std::fs::metadata(tmp) {
            let mut permissions = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)] // our own temp, deleted next line
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(tmp, permissions);
        }
        let _ = std::fs::remove_file(tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A target held open without sharing: the rename is retried, and each attempt asks the check again.
    #[cfg(windows)]
    #[test]
    fn a_locked_target_is_checked_again_before_each_rename_attempt() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        std::fs::write(&path, "theirs").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let mut calls = 0;
        let wrote = write_atomic_checked(&path, b"mine", || {
            calls += 1;
            // the other writer changed the file while it held it
            Ok(calls == 1)
        })
        .unwrap();
        drop(held);
        assert!(!wrote);
        assert!(calls >= 2);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "temp removed"
        );
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn write_atomic_creates_replaces_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new dir").join("ação 文.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(names_in(path.parent().unwrap()), ["ação 文.json"]);
    }

    #[test]
    fn write_atomic_failure_leaves_target_and_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("folder");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep.md"), "k").unwrap();
        assert!(write_atomic(&target, b"x").is_err());
        assert_eq!(names_in(dir.path()), ["folder"]);
        assert_eq!(std::fs::read(target.join("keep.md")).unwrap(), b"k");
    }

    #[test]
    fn concurrent_writes_of_one_file_never_tear() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let contents: Vec<Vec<u8>> = (0..8u8).map(|i| vec![b'a' + i; 4096]).collect();
        std::thread::scope(|s| {
            for bytes in &contents {
                let path = &path;
                s.spawn(move || {
                    for _ in 0..10 {
                        // a rename can lose to another writer holding the file: allowed to fail, never to tear
                        let _ = write_atomic(path, bytes);
                    }
                });
            }
        });
        let result = std::fs::read(&path).unwrap();
        assert!(contents.contains(&result), "torn write");
        assert_eq!(names_in(dir.path()), ["state.json"]);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sh");
        std::fs::write(&path, "a").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&path, b"b").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(windows)]
    #[test]
    fn write_atomic_handles_paths_over_260_characters() {
        let dir = tempfile::tempdir().unwrap();
        let mut deep = dir.path().to_path_buf();
        while deep.display().to_string().len() < 300 {
            deep.push("segment-with-a-long-name");
        }
        let path = deep.join("file.md");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
    }

    #[test]
    fn read_text_limits_and_binary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        std::fs::write(&path, "line\r\nnext\r\n").unwrap();
        let text = read_text(&path, 1024).unwrap();
        assert_eq!(text.line_ending, LineEnding::CrLf);
        assert!(matches!(
            read_text(&path, 4),
            Err(FsxError::TooLarge { .. })
        ));
        std::fs::write(&path, b"PK\x03\x04\0\0binary").unwrap();
        assert!(matches!(read_text(&path, 1024), Err(FsxError::Binary(_))));
        assert!(matches!(
            read_text(&dir.path().join("missing.md"), 1024),
            Err(FsxError::Io { .. })
        ));
        let empty = dir.path().join("empty.md");
        std::fs::write(&empty, "").unwrap();
        assert_eq!(read_text(&empty, 0).unwrap().text, "");
    }

    #[test]
    fn retry_retries_transient_errors_only() {
        let delays = [Duration::from_millis(1); 3];
        let mut calls = 0;
        let result: io::Result<()> = retry(&delays, || {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        });
        assert!(result.is_err());
        assert_eq!(calls, 4);
        let mut calls = 0;
        let result: io::Result<()> = retry(&delays, || {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::NotFound))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
        let mut calls = 0;
        let value = retry(&delays, || {
            calls += 1;
            if calls < 3 {
                Err(io::Error::from_raw_os_error(32))
            } else {
                Ok(7)
            }
        })
        .unwrap();
        assert_eq!(value, 7);
    }

    #[test]
    fn hash_is_stable_hex_16() {
        assert_eq!(hash(b"abc"), hash(b"abc"));
        assert_ne!(hash(b"abc"), hash(b"abd"));
        assert_eq!(hash(b"").hex().len(), 16);
        assert_eq!(serde_json::to_string(&hash(b"")).unwrap().len(), 18);
    }
}
