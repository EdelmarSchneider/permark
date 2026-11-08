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

/// Identity of a file | folder on its volume, `(volume, file)`: kept across a rename on the same volume (unix
/// `st_dev` + `st_ino`; Windows volume serial + file index). A link is named itself, never followed. `None`: unknown
/// (gone, refused, no such notion here, or a volume that reports no real ids).
#[must_use]
pub fn file_identity(path: &Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path).ok()?;
        known_identity(meta.dev(), meta.ino())
    }
    #[cfg(windows)]
    {
        let (volume, file) = win::file_identity(path).ok()?;
        known_identity(volume, file)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        None
    }
}

/// FAT and some network | NAS drivers answer 0 for every file: such an id tells two files apart from nothing.
fn known_identity(volume: u64, file: u64) -> Option<(u64, u64)> {
    (volume != 0 && file != 0).then_some((volume, file))
}

/// Whether two names are one file, as far as the volume can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameFile {
    /// One file: equal ids, size and last write.
    Yes,
    /// Two files: the ids differ.
    Other,
    /// Not provable: an id is unknown, or equal ids disagree on size | last write (ids that are not unique).
    Unknown,
}

/// [`IpcError`] detail of a refusal because two names may be one file but the volume cannot prove it
/// ([`SameFile::Unknown`]).
pub const IDENTITY_UNKNOWN: &str = "identityUnknown";

/// What has to agree for two names to be taken as one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFacts {
    id: (u64, u64),
    len: u64,
    modified: Option<std::time::SystemTime>,
}

fn facts(path: &Path) -> Option<FileFacts> {
    let id = file_identity(path)?;
    let meta = std::fs::symlink_metadata(path).ok()?;
    Some(FileFacts {
        id,
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// Size and last write corroborate the ids: a driver handing out non-unique ids would otherwise make another file
/// look like this one, and a rename "onto itself" would replace it.
fn compare(a: Option<FileFacts>, b: Option<FileFacts>) -> SameFile {
    match (a, b) {
        (Some(a), Some(b)) if a.id != b.id => SameFile::Other,
        (Some(a), Some(b)) if a == b => SameFile::Yes,
        _ => SameFile::Unknown,
    }
}

/// Whether two paths name one file | folder (a case | normalization spelling of one name, a hard link). A name that
/// only compares equal is never taken for the same file without the volume's proof.
#[must_use]
pub fn same_file(a: &Path, b: &Path) -> SameFile {
    compare(facts(a), facts(b))
}

/// Retry delays for transient Windows sharing | lock violations.
pub const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(20),
    Duration::from_millis(60),
    Duration::from_millis(150),
];

/// Windows sharing (32) | lock (33) violation: another program holds the file.
pub(crate) fn is_locked(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33))
}

/// [`is_locked`], or access denied (antivirus | sync client holding the file).
fn is_transient(error: &io::Error) -> bool {
    is_locked(error) || error.kind() == io::ErrorKind::PermissionDenied
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
    read_limited(path, limit, false).map(|(bytes, _)| bytes)
}

/// [`read_bytes`]; `locate`: also the path of the file as opened ([`opened_path`]).
fn read_limited(
    path: &Path,
    limit: u64,
    locate: bool,
) -> Result<(Vec<u8>, Option<PathBuf>), FsxError> {
    let read = retry(&RETRY_DELAYS, || {
        let file = std::fs::File::open(path)?;
        let opened = if locate { opened_path(&file) } else { None };
        let len = file.metadata()?.len();
        if len > limit {
            return Ok(None);
        }
        let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
        // `take`: a file growing while read never exceeds the limit
        file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        Ok(Some((bytes, opened)))
    })
    .map_err(|e| FsxError::io(path, e))?;
    let shown = || path.display().to_string();
    let (bytes, opened) = read.ok_or_else(|| FsxError::TooLarge {
        path: shown(),
        limit,
    })?;
    if bytes.len() as u64 > limit {
        return Err(FsxError::TooLarge {
            path: shown(),
            limit,
        });
    }
    Ok((bytes, opened))
}

/// Reads a text file of at most `limit` bytes.
///
/// # Errors
/// `TooLarge` (checked before reading), `Binary` (NUL in the head, no UTF-16 BOM), `Io`.
pub fn read_text(path: &Path, limit: u64) -> Result<Decoded, FsxError> {
    let bytes = read_bytes(path, limit)?;
    decode(&bytes).ok_or_else(|| FsxError::Binary(path.display().to_string()))
}

/// [`read_text`] plus where the file that was read really is: the path of the open handle, every link and junction
/// on the way followed (`None`: the system cannot tell). A path checked against a boundary before the open can be
/// swapped for a link before it; this one is the file the bytes came from.
///
/// # Errors
/// See [`read_text`].
pub fn read_text_opened(path: &Path, limit: u64) -> Result<(Decoded, Option<PathBuf>), FsxError> {
    let (bytes, opened) = read_limited(path, limit, true)?;
    let text = decode(&bytes).ok_or_else(|| FsxError::Binary(path.display().to_string()))?;
    Ok((text, opened))
}

/// The path of an open file as the system knows it (Windows: may carry `\\?\`, see [`crate::paths::simplified`]).
fn opened_path(file: &std::fs::File) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        win::final_path(file).ok()
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::fd::AsRawFd;
        std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use std::os::unix::ffi::OsStrExt;
        rustix::fs::getpath(file)
            .ok()
            .map(|p| PathBuf::from(std::ffi::OsStr::from_bytes(p.as_bytes())))
    }
    #[cfg(not(any(
        windows,
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    {
        let _ = file;
        None
    }
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

/// Bytes of the target's name a temp name keeps: a name near the 255 limit of file systems still has a temp.
const TEMP_NAME_KEEP: usize = 64;

/// Hidden temp next to `path`: `.<name>.<pid>.<n>.part`, `<name>` cut to [`TEMP_NAME_KEEP`] bytes.
fn temp_path(path: &Path) -> PathBuf {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut name = path
        .file_name()
        .map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
    if name.len() > TEMP_NAME_KEEP {
        let mut end = TEMP_NAME_KEEP;
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        name.truncate(end);
    }
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

/// [`write_atomic_checked`] guarded by `seen`: `bytes` replace `path` only while it still holds the bytes hashed `seen`.
/// Check, then rename: two steps, not one atomic compare-and-swap. The check runs right before each rename attempt,
/// which narrows the window but does not close it: on Unix any writer, and on Windows a writer that does not keep the
/// file open, can still land between the two. `Ok(false)`: the file changed or went meanwhile, untouched.
///
/// # Errors
/// IO failure; temp removed, target untouched.
pub fn write_atomic_if_unchanged(path: &Path, bytes: &[u8], seen: ContentHash) -> io::Result<bool> {
    write_atomic_checked(path, bytes, || match std::fs::read(path) {
        Ok(now) => Ok(hash(&now) == seen),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    })
}

/// Moves `from` over `to` durably (Windows: `MoveFileExW` replace + write-through; Unix: rename + folder fsync),
/// retrying transient locks ([`RETRY_DELAYS`]). Both on one volume (a finished download next to its part).
///
/// # Errors
/// IO failure (source missing, target locked past the retries, other volume); target untouched.
pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    retry(&RETRY_DELAYS, || rename_durably(from, to))
}

/// Bytes free for this user on the volume holding `path`; a path not made yet answers for its nearest existing
/// folder (a store root before its first download).
///
/// # Errors
/// No existing ancestor | the system refused.
pub fn free_space(path: &Path) -> io::Result<u64> {
    let existing = path
        .ancestors()
        .find(|p| !p.as_os_str().is_empty() && p.exists())
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    let folder = if existing.is_dir() {
        existing
    } else {
        existing.parent().unwrap_or_else(|| Path::new("."))
    };
    #[cfg(windows)]
    {
        win::free_space(folder)
    }
    #[cfg(unix)]
    {
        let stat = rustix::fs::statvfs(folder)?;
        Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = folder;
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
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
            sync_folder(dir);
        }
        Ok(())
    }
}

/// Flushes the entries of folder `dir` (names made, renamed, removed in it) to disk, best effort: some file systems
/// cannot, and what was done in the folder happened anyway. Unix only: std opens no folder handle on Windows, where
/// durable renames go through write-through instead.
pub(crate) fn sync_folder(dir: &Path) {
    #[cfg(not(windows))]
    {
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        if let Ok(folder) = std::fs::File::open(dir) {
            let _ = folder.sync_all();
        }
    }
    #[cfg(windows)]
    {
        let _ = dir;
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
    fn a_name_at_the_length_limit_gets_a_short_temp_that_is_still_scratch() {
        let dir = tempfile::tempdir().unwrap();
        let name = format!("{}.md", "n".repeat(247));
        assert_eq!(name.len(), 250);
        let path = dir.path().join(&name);
        for long in [path.clone(), dir.path().join("\u{e9}".repeat(125))] {
            let temp = temp_path(&long);
            let temp = temp.file_name().unwrap().to_string_lossy().into_owned();
            assert!(temp.len() <= 128 && is_scratch_name(&temp), "{temp}");
        }
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(names_in(dir.path()), [name]);
    }

    #[test]
    fn a_write_while_unchanged_refuses_a_file_that_changed_or_went() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        let seen = hash(b"seen");
        std::fs::write(&path, "theirs").unwrap();
        assert!(!write_atomic_if_unchanged(&path, b"mine", seen).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"theirs");
        std::fs::remove_file(&path).unwrap();
        assert!(!write_atomic_if_unchanged(&path, b"mine", seen).unwrap());
        assert!(!path.exists());
        std::fs::write(&path, "seen").unwrap();
        assert!(write_atomic_if_unchanged(&path, b"mine", seen).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"mine");
        assert_eq!(names_in(dir.path()), ["a.md"]);
    }

    #[test]
    fn two_names_are_the_same_file_only_when_the_volume_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        assert_eq!(same_file(&a, &a), SameFile::Yes);
        assert_eq!(same_file(&a, &b), SameFile::Other);
        assert_eq!(
            same_file(&a, &dir.path().join("missing.md")),
            SameFile::Unknown
        );
        if crate::paths::CASE_INSENSITIVE {
            assert_eq!(same_file(&a, &dir.path().join("A.md")), SameFile::Yes);
        }
    }

    #[test]
    fn a_zero_volume_serial_or_file_index_is_no_identity() {
        assert_eq!(known_identity(7, 0), None);
        assert_eq!(known_identity(0, 9), None);
        assert_eq!(known_identity(7, 9), Some((7, 9)));
    }

    #[test]
    fn equal_ids_are_one_file_only_when_size_and_last_write_agree() {
        let at = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(5);
        let one = FileFacts {
            id: (7, 9),
            len: 3,
            modified: Some(at),
        };
        assert_eq!(compare(Some(one), Some(one)), SameFile::Yes);
        let longer = FileFacts { len: 4, ..one };
        assert_eq!(compare(Some(one), Some(longer)), SameFile::Unknown);
        let later = FileFacts {
            modified: Some(at + Duration::from_secs(1)),
            ..one
        };
        assert_eq!(compare(Some(one), Some(later)), SameFile::Unknown);
        let other = FileFacts { id: (7, 10), ..one };
        assert_eq!(compare(Some(one), Some(other)), SameFile::Other);
        assert_eq!(compare(Some(one), None), SameFile::Unknown);
        assert_eq!(compare(None, Some(one)), SameFile::Unknown);
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

    #[test]
    fn free_space_answers_for_a_folder_a_file_and_a_folder_not_made_yet() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("João store");
        std::fs::create_dir(&folder).unwrap();
        let free = free_space(&folder).unwrap();
        assert!(free > 0);
        let file = folder.join("a.bin");
        std::fs::write(&file, b"x").unwrap();
        assert!(free_space(&file).unwrap() > 0);
        // a store root not created yet: its nearest existing folder answers
        assert!(free_space(&folder.join("not").join("yet")).unwrap() > 0);
    }

    /// The opened file's own path: a link (junction | symlink) on the way is followed to where the file really is.
    #[test]
    fn a_read_names_where_the_opened_file_really_is() {
        let dir = tempfile::tempdir().unwrap();
        let base = crate::paths::canonicalize(dir.path()).unwrap();
        let real = base.join("real ação");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("a.md"), "# A\n").unwrap();
        let (text, opened) = read_text_opened(&real.join("a.md"), 1024).unwrap();
        assert_eq!(text.text, "# A\n");
        let opened = opened.expect("the system names the opened file");
        assert_eq!(crate::paths::simplified(&opened), real.join("a.md"));
        let link = base.join("link");
        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .output()
            .is_ok_and(|o| o.status.success());
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&real, &link).is_ok();
        if made {
            let (_, opened) = read_text_opened(&link.join("a.md"), 1024).unwrap();
            assert_eq!(
                crate::paths::simplified(&opened.unwrap()),
                real.join("a.md")
            );
        }
        assert!(matches!(
            read_text_opened(&real.join("a.md"), 2),
            Err(FsxError::TooLarge { .. })
        ));
    }

    #[test]
    fn replace_file_moves_over_an_existing_target_and_leaves_no_source() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("model ação.gguf.part");
        let to = dir.path().join("model ação.gguf");
        std::fs::write(&from, b"new").unwrap();
        replace_file(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"new");
        std::fs::write(&from, b"newer").unwrap();
        replace_file(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"newer");
        assert_eq!(names_in(dir.path()), ["model ação.gguf"]);
        assert!(replace_file(&from, &to).is_err(), "source gone");
        assert_eq!(std::fs::read(&to).unwrap(), b"newer");
    }
}
