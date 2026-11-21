//! IO helpers: atomic durable writes, text reads with encoding + line-ending facts, content hash, retry on
//! transient Windows locks.

mod decode;
mod regular;
#[cfg(windows)]
mod win;

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

pub use decode::{decode, Decoded, Encoding, LineEnding, BINARY_SNIFF};
pub(crate) use regular::{is_link, symlink_metadata_below};
pub use regular::{read_regular, Zone};

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

/// Sends files to the system recycle bin. A refusal may come after the bin took some of them: [`recycle_each`] names
/// those.
///
/// # Errors
/// The bin refused (the reason as text).
pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
    recycle_each(paths).map_err(|refused| refused.reason)
}

/// A refusal of the recycle bin, with what it had taken before it refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unrecycled {
    pub reason: String,
    /// One per path given, same order: no longer there after the refusal.
    pub gone: Vec<bool>,
}

impl Unrecycled {
    /// `items` (one per path given, same order) split into those the bin took and those still in place.
    #[must_use]
    pub fn split<T>(&self, items: impl IntoIterator<Item = T>) -> (Vec<T>, Vec<T>) {
        let (mut taken, mut left) = (Vec::new(), Vec::new());
        for (at, item) in items.into_iter().enumerate() {
            if self.gone.get(at).copied().unwrap_or(false) {
                taken.push(item);
            } else {
                left.push(item);
            }
        }
        (taken, left)
    }
}

/// [`recycle`] with the outcome per item. The bins stop part way: the freedesktop one moves item after item and
/// stops at the first failure, a Windows file operation goes on past an item it cannot take and then fails. So after
/// a refusal each path is looked at again; only one that is surely absent counts as taken.
///
/// # Errors
/// The bin refused; [`Unrecycled::gone`] lists the items it took all the same.
pub fn recycle_each(paths: &[PathBuf]) -> Result<(), Unrecycled> {
    trash::delete_all(paths).map_err(|error| Unrecycled {
        reason: error.to_string(),
        gone: paths
            .iter()
            .map(|path| {
                std::fs::symlink_metadata(path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            })
            .collect(),
    })
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

/// One folder on disk, across a rename on the same volume: [`file_identity`] plus the creation time where the
/// platform keeps one. Unix file systems (ext4) may hand a folder deleted and created again at once the same inode,
/// so volume + file alone can match a new folder; its creation time differs. Compare with [`FolderIdentity::same`],
/// not `==`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderIdentity {
    pub volume: u64,
    pub file: u64,
    /// Creation time, nanoseconds since the UNIX epoch; `None`: not given by the platform, or recorded by a build
    /// that did not keep it. Stored as `created`: identities already saved in the profile keep loading, and builds
    /// that wrote them still read the creation time.
    #[serde(rename = "created", default, skip_serializing_if = "Option::is_none")]
    pub born: Option<u64>,
}

impl FolderIdentity {
    /// The folder at `path`, a link named itself (as [`file_identity`]); `None`: unknown (gone, refused, or a volume
    /// with no real ids).
    #[must_use]
    pub fn of(path: &Path) -> Option<Self> {
        let (volume, file) = file_identity(path)?;
        let born = std::fs::symlink_metadata(path)
            .and_then(|meta| meta.created())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|since| u64::try_from(since.as_nanos()).ok());
        Some(Self { volume, file, born })
    }

    /// Volume + file match, and the creation time too when both sides have one (one side without it: recorded by an
    /// earlier build, or not given now -> volume + file decide).
    #[must_use]
    pub fn same(self, other: Self) -> bool {
        self.volume == other.volume
            && self.file == other.file
            && match (self.born, other.born) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
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

/// Reads a plain file of at most `limit` bytes.
///
/// # Errors
/// `TooLarge` (checked before reading), `Io` (`InvalidData`: a folder, FIFO or device, never opened | waited on).
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
        // only a plain file is opened: a FIFO | device read can wait forever, a folder's open is answered with access
        // denied on Windows (retried); the open itself never waits for a FIFO's writer either
        if !std::fs::metadata(path)?.is_file() {
            return Err(regular::refused("not a plain file"));
        }
        let file = regular::open(path, Zone::Anywhere)?;
        let opened = if locate { opened_path(&file) } else { None };
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(regular::refused("not a plain file"));
        }
        let len = meta.len();
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

/// The process that made a temp of [`temp_path`] | [`scratch_in`] (`.<name>.<pid>.<n>.part`); `None`: another name.
fn temp_pid(name: &str) -> Option<u32> {
    let inner = name.strip_prefix('.')?.strip_suffix(".part")?;
    let (rest, n) = inner.rsplit_once('.')?;
    let (stem, pid) = rest.rsplit_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if stem.is_empty() || !digits(n) || !digits(pid) {
        return None;
    }
    pid.parse().ok()
}

/// Age past which a temp of another process was left by a crash: a temp lives for one write.
pub const STALE_TEMP_AGE: Duration = Duration::from_hours(1);

/// Temps of [`write_atomic`] in `dir` that a crash left behind: a plain file with the exact temp name, made by another
/// process, unchanged for longer than `older_than`. Never a folder ([`scratch_in`] builds | sets aside whole folders
/// under that name, a crash may leave one as the only copy of the person's data; a repository may track one), a link
/// or a device, and nothing directly in a `.permark` folder (its scratch names hold the person's data while it moves).
#[must_use]
pub fn stale_temps(dir: &Path, older_than: Duration) -> Vec<PathBuf> {
    if dir
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(crate::state::PERMARK_DIR))
    {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let own = std::process::id();
    let now = SystemTime::now();
    entries
        .flatten()
        .filter(|entry| {
            let foreign = entry
                .file_name()
                .to_str()
                .and_then(temp_pid)
                .is_some_and(|pid| pid != own);
            // the entry's own metadata: a link is not followed, and is no plain file
            foreign
                && entry.metadata().is_ok_and(|meta| {
                    meta.file_type().is_file()
                        && meta
                            .modified()
                            .ok()
                            .and_then(|at| now.duration_since(at).ok())
                            .is_some_and(|age| age > older_than)
                })
        })
        .map(|entry| entry.path())
        .collect()
}

/// Sends the [`stale_temps`] (older than [`STALE_TEMP_AGE`]) of each folder to the recycle bin: they hold the bytes of
/// a write a crash cut short, so they stay recoverable there. Only folders whose bin keeps them ([`bin_keeps`]).
/// Best effort; answers how many went.
#[must_use]
pub fn sweep_stale_temps(dirs: &[PathBuf]) -> usize {
    let found: Vec<PathBuf> = dirs
        .iter()
        .filter(|dir| bin_keeps(dir))
        .flat_map(|dir| stale_temps(dir, STALE_TEMP_AGE))
        .collect();
    if found.is_empty() {
        return 0;
    }
    match recycle_each(&found) {
        Ok(()) => found.len(),
        Err(refused) => refused.gone.iter().filter(|gone| **gone).count(),
    }
}

/// Whether the recycle bin keeps what is sent to it from `dir`. Windows: a folder on a fixed disk only (a share, a
/// removable | RAM disk has no bin, and the silent shell delete removes for good there). Other systems: a local path
/// ([`is_local_path`]); their bins refuse what they cannot keep.
fn bin_keeps(dir: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        dir.is_absolute()
            && match dir.components().next() {
                Some(Component::Prefix(prefix)) => match prefix.kind() {
                    Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => win::fixed_drive(letter),
                    _ => false,
                },
                _ => false,
            }
    }
    #[cfg(not(windows))]
    {
        is_local_path(dir)
    }
}

/// Creates the temp of a write: hidden on Windows (best effort), so one a crash leaves stays out of the file manager's
/// sight, as its leading dot does on unix.
fn new_temp(tmp: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.attributes(win::HIDDEN);
    }
    options.open(tmp)
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
    check: impl FnMut() -> io::Result<bool>,
) -> io::Result<bool> {
    write_atomic_checked_retrying(path, bytes, &RETRY_DELAYS, check)
}

/// [`write_atomic_checked`] with the waits of a locked target given (a write the person waits on may wait longer for a
/// program that holds the file).
///
/// # Errors
/// See [`write_atomic_checked`].
pub fn write_atomic_checked_retrying(
    path: &Path,
    bytes: &[u8],
    delays: &[Duration],
    mut check: impl FnMut() -> io::Result<bool>,
) -> io::Result<bool> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = temp_path(path);
    let result = new_temp(&tmp)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| {
            #[cfg(test)]
            slip::fire(path);
            let target = std::fs::metadata(path)
                .ok()
                .filter(std::fs::Metadata::is_file);
            if let Some(meta) = &target {
                // best effort: a file system without permissions still gets the bytes
                let _ = std::fs::set_permissions(&tmp, meta.permissions());
            }
            // a rename keeps the temp's hidden flag: never left on the person's file
            #[cfg(windows)]
            win::hidden_as(&tmp, target.as_ref())?;
            retry(delays, || {
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

/// Test seam: another writer's save landing between a write's temp and its check, the window the check exists for.
/// Windows can hold a rename back from outside (a handle without delete sharing); other systems cannot, so tests of
/// that window place the save here.
#[cfg(test)]
pub(crate) mod slip {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    type Save = Box<dyn FnOnce() + Send>;

    /// Saves waiting for a write of their file; tests run in parallel, each on files of its own.
    static SAVES: Mutex<Vec<(PathBuf, Save)>> = Mutex::new(Vec::new());

    /// The same file for both spellings of a path (a temp folder under a linked folder, the `\\?\` form).
    fn key(path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }

    /// Runs `save` once, at the next atomic write of `path` (it must exist), right before that write's check.
    #[cfg_attr(windows, allow(dead_code))] // Windows tests hold the file from outside instead
    pub(crate) fn before_check(path: &Path, save: impl FnOnce() + Send + 'static) {
        if let (Some(key), Ok(mut saves)) = (key(path), SAVES.lock()) {
            saves.push((key, Box::new(save)));
        }
    }

    pub(super) fn fire(path: &Path) {
        let save = SAVES.lock().ok().and_then(|mut saves| {
            if saves.is_empty() {
                return None; // the common case: no path lookup for every write of every test
            }
            let key = key(path)?;
            let at = saves.iter().position(|(file, _)| *file == key)?;
            Some(saves.swap_remove(at).1)
        });
        if let Some(save) = save {
            save();
        }
    }
}

/// [`write_atomic_checked`] guarded by `seen`: `bytes` replace `path` only while it still holds the bytes hashed `seen`.
/// Check, then rename: two steps, not one atomic compare-and-swap. The check runs right before each rename attempt,
/// which narrows the window but does not close it: on Unix any writer, and on Windows a writer that does not keep the
/// file open, can still land between the two. `Ok(false)`: the file changed or went meanwhile, untouched. Only a
/// plain file still holds what was read: a link is followed (a profile file a dotfile manager links elsewhere stays
/// writable; the rename then puts a plain file where the link was, as a plain atomic write does), a folder, FIFO or
/// device at the path or behind a link is a change, never read.
///
/// # Errors
/// IO failure; temp removed, target untouched.
pub fn write_atomic_if_unchanged(path: &Path, bytes: &[u8], seen: ContentHash) -> io::Result<bool> {
    write_atomic_checked(path, bytes, || still_holds(path, seen))
}

/// Whether the plain file at `path` (a link followed) holds the bytes hashed `seen`. Anything else there answers
/// `false` without being read: a FIFO would keep the check waiting, a device (`/dev/zero`) reading, forever. The file
/// is read no further than the size it had when looked at (one that grew since has changed).
fn still_holds(path: &Path, seen: ContentHash) -> io::Result<bool> {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if !meta.is_file() {
        return Ok(false);
    }
    match read_regular(path, meta.len(), Zone::Anywhere) {
        Ok(now) => Ok(hash(&now) == seen),
        Err(FsxError::Io { source, .. })
            if matches!(
                source.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::InvalidData
            ) =>
        {
            Ok(false)
        }
        Err(FsxError::Io { source, .. }) => Err(source),
        Err(FsxError::TooLarge { .. } | FsxError::Binary(_)) => Ok(false),
    }
}

/// [`write_atomic_checked`] that only creates: `bytes` land at `path` only while nothing (file, folder, link) is
/// there. Same residual check-then-rename window as [`write_atomic_if_unchanged`]: a writer landing between the
/// check and the rename is replaced. `Ok(false)`: something is at the path, untouched.
///
/// # Errors
/// IO failure; temp removed, target untouched.
pub fn write_atomic_if_absent(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    write_atomic_checked(path, bytes, || match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(true),
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

/// Makes `dir` (missing parents too) a real folder of this user that only this user can enter. Unix: mode 0700 (an
/// existing folder is tightened); a link, a file or another user's folder in the way is refused. Windows: created;
/// the per-user profile it sits in already keeps other users out.
///
/// # Errors
/// Creation failed, something other than this user's folder in the way, mode change refused.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let meta = std::fs::symlink_metadata(dir)?;
        if !meta.file_type().is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a folder"));
        }
        if meta.uid() != rustix::process::geteuid().as_raw() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "a folder of another user",
            ));
        }
        if meta.mode() & 0o777 != 0o700 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
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

/// Whether looking up `path` stays on this computer: an absolute path on a local drive. Windows: a drive letter path
/// (also its `\\?\C:\` spelling) on a fixed | removable | RAM disk; never a share, a mapped network drive, a device
/// path, a CD drive or an unknown drive. Unix: any absolute path but one that starts with two separators (a network
/// spelling: [`crate::process::is_network_path`]); a mount point of a network file system is not told apart. Use it
/// before the first lookup of a path that came from file content: even a metadata lookup on a share connects to the
/// server and offers it the person's sign-in.
#[must_use]
pub fn is_local_path(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        // a drive letter path never starts with two separators: no text check needed beside the prefix
        match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => win::local_drive(letter),
                _ => false,
            },
            _ => false,
        }
    }
    #[cfg(not(windows))]
    {
        !crate::process::is_network_path(path.as_os_str())
    }
}

/// Whether `path` is inside a folder a cloud sync service manages. Windows: a Cloud Files sync root (OneDrive,
/// Dropbox, iCloud, Box, ...), told by the system's registration of the root, never by attributes: a normal process
/// runs in the default placeholder compatibility mode, where a hydrated placeholder folder looks like a plain folder
/// (no reparse attribute, not a link). Other systems: false (their cloud folders are not told apart). Looks the path
/// up: a path that came from file content passes [`is_local_path`] first.
#[must_use]
pub fn in_cloud_folder(path: &Path) -> bool {
    #[cfg(windows)]
    {
        win::in_sync_root(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// The 8.3 short spelling of the existing `path` (Windows: `C:\Users\ANNLON~1` for `C:\Users\Ann Longname`), which some
/// tools print instead of the long one. `None`: not Windows, no such path, or no short name (the volume keeps none).
#[must_use]
pub fn short_name(path: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        win::short_path(path).ok().filter(|short| short != path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
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
pub(crate) mod tests {
    use super::*;

    /// A folder registered as a Cloud Files sync root, the way OneDrive | Dropbox register theirs, while this lives
    /// (no provider connected: nothing is ever fetched); the registration goes on drop. Unregister before the folder
    /// is removed: keep this after the temp folder in the test's bindings.
    #[cfg(windows)]
    pub(crate) struct SyncRoot(Vec<u16>);

    // Documented FFI, test only: the Cloud Files registration has no std wrapper. Every pointer passed comes from a
    // live local buffer | struct that outlives the call.
    #[cfg(windows)]
    #[allow(unsafe_code)]
    impl SyncRoot {
        /// `None` (the reason printed): the system refused, e.g. no Cloud Files service on this machine.
        pub(crate) fn register(dir: &Path) -> Option<Self> {
            use windows_sys::core::GUID;
            use windows_sys::Win32::Storage::CloudFilters::{
                CfRegisterSyncRoot, CF_HYDRATION_POLICY_FULL, CF_POPULATION_POLICY_ALWAYS_FULL,
                CF_REGISTER_FLAG_NONE, CF_SYNC_POLICIES, CF_SYNC_REGISTRATION,
            };
            let path = win::wide(dir).unwrap();
            let name: Vec<u16> = "PerMarkTest\0".encode_utf16().collect();
            let version: Vec<u16> = "1.0\0".encode_utf16().collect();
            let registration = CF_SYNC_REGISTRATION {
                StructSize: u32::try_from(std::mem::size_of::<CF_SYNC_REGISTRATION>()).unwrap(),
                ProviderName: name.as_ptr(),
                ProviderVersion: version.as_ptr(),
                ProviderId: GUID::from_u128(0x5f0c_2a47_9d3e_4b8a_a1c6_7e2d_9b40_f318),
                ..CF_SYNC_REGISTRATION::default()
            };
            let mut policies = CF_SYNC_POLICIES {
                StructSize: u32::try_from(std::mem::size_of::<CF_SYNC_POLICIES>()).unwrap(),
                ..CF_SYNC_POLICIES::default()
            };
            policies.Hydration.Primary = CF_HYDRATION_POLICY_FULL;
            policies.Population.Primary = CF_POPULATION_POLICY_ALWAYS_FULL;
            // SAFETY: `path`, `name`, `version` are live NUL-terminated buffers; both structs are live locals with
            // their sizes set; all outlive the call.
            let result = unsafe {
                CfRegisterSyncRoot(
                    path.as_ptr(),
                    &raw const registration,
                    &raw const policies,
                    CF_REGISTER_FLAG_NONE,
                )
            };
            if result != 0 {
                eprintln!("no Cloud Files sync root here (HRESULT {result:#010x}): skipped");
                return None;
            }
            Some(Self(path))
        }
    }

    #[cfg(windows)]
    #[allow(unsafe_code)]
    impl Drop for SyncRoot {
        fn drop(&mut self) {
            use windows_sys::Win32::Storage::CloudFilters::CfUnregisterSyncRoot;
            // SAFETY: `self.0` is the live NUL-terminated path the root was registered with.
            let _ = unsafe { CfUnregisterSyncRoot(self.0.as_ptr()) };
        }
    }

    #[test]
    fn a_reused_file_id_with_another_birth_is_another_folder() {
        let old = FolderIdentity {
            volume: 1,
            file: 2,
            born: Some(100),
        };
        assert!(!old.same(FolderIdentity {
            born: Some(200),
            ..old
        }));
        assert!(old.same(FolderIdentity { born: None, ..old }));
        assert!(FolderIdentity { born: None, ..old }.same(old));
        assert!(!old.same(FolderIdentity { volume: 3, ..old }));
        assert!(!old.same(FolderIdentity { file: 3, ..old }));
    }

    #[test]
    fn a_real_folder_keeps_its_identity_across_a_rename_and_a_new_one_differs() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a b ü");
        std::fs::create_dir(&a).unwrap();
        let Some(before) = FolderIdentity::of(&a) else {
            return; // a volume without ids: nothing to compare
        };
        #[cfg(windows)]
        assert!(before.born.is_some(), "NTFS keeps creation times");
        let b = dir.path().join("moved");
        std::fs::rename(&a, &b).unwrap();
        assert_eq!(FolderIdentity::of(&a), None, "gone: unknown");
        assert!(FolderIdentity::of(&b).unwrap().same(before));
        let c = dir.path().join("fresh");
        std::fs::create_dir(&c).unwrap();
        assert!(!FolderIdentity::of(&c).unwrap().same(before));
    }

    #[test]
    fn a_folder_identity_is_stored_under_the_names_earlier_builds_wrote() {
        let with: FolderIdentity =
            serde_json::from_str(r#"{"volume":1,"file":2,"created":3}"#).unwrap();
        assert_eq!(
            with,
            FolderIdentity {
                volume: 1,
                file: 2,
                born: Some(3)
            }
        );
        assert_eq!(
            serde_json::to_string(&with).unwrap(),
            r#"{"volume":1,"file":2,"created":3}"#
        );
        let without: FolderIdentity = serde_json::from_str(r#"{"volume":1,"file":2}"#).unwrap();
        assert_eq!(without.born, None);
        assert_eq!(
            serde_json::to_string(&without).unwrap(),
            r#"{"volume":1,"file":2}"#
        );
    }

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
    fn temps_a_crash_left_go_to_the_bin_fresh_ones_own_ones_and_other_names_stay() {
        let dir = tempfile::tempdir().unwrap();
        let own = std::process::id();
        assert_ne!(own, 999_999);
        let hours_ago = std::time::SystemTime::now() - Duration::from_hours(2);
        let make = |name: &str, old: bool| {
            let file = std::fs::File::create(dir.path().join(name)).unwrap();
            if old {
                file.set_modified(hours_ago).unwrap();
            }
        };
        make(".a.md.999999.0.part", true);
        make(".import.999998.3.part", true);
        make(".b.md.999999.1.part", false);
        make(&format!(".c.md.{own}.0.part"), true);
        make(".notes.part", true);
        make(".a.md.x9.0.part", true);
        make("a.md", true);
        let mut stale: Vec<String> = stale_temps(dir.path(), STALE_TEMP_AGE)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        stale.sort();
        assert_eq!(stale, [".a.md.999999.0.part", ".import.999998.3.part"]);
        let swept = sweep_stale_temps(&[dir.path().to_path_buf()]);
        // the bin of a fixed disk takes them; a unix test machine may have no usable bin for the temp folder
        #[cfg(windows)]
        assert_eq!(swept, 2);
        let mut kept = vec![
            ".a.md.x9.0.part".to_owned(),
            ".b.md.999999.1.part".to_owned(),
            format!(".c.md.{own}.0.part"),
            ".notes.part".to_owned(),
            "a.md".to_owned(),
        ];
        if swept == 0 {
            kept.extend([
                ".a.md.999999.0.part".to_owned(),
                ".import.999998.3.part".to_owned(),
            ]);
        }
        kept.sort();
        assert_eq!(names_in(dir.path()), kept);
    }

    /// Sets the modification time of the folder `dir` itself to `at`.
    fn age_folder(dir: &Path, at: SystemTime) {
        #[cfg(windows)]
        let folder = {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES,
            };
            // a folder opens only with backup semantics; a time is set with the right to write attributes
            std::fs::OpenOptions::new()
                .access_mode(FILE_WRITE_ATTRIBUTES)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(dir)
                .unwrap()
        };
        #[cfg(not(windows))]
        let folder = std::fs::File::open(dir).unwrap();
        folder.set_modified(at).unwrap();
    }

    /// A crash may leave a folder under a temp name as the only copy of the person's data, a repository may track one,
    /// and a link leads to files of their own: only plain temp files go, and nothing directly in `.permark`.
    #[test]
    fn a_folder_a_link_or_anything_in_permark_with_a_temp_name_is_never_swept() {
        let dir = tempfile::tempdir().unwrap();
        let hours_ago = SystemTime::now() - Duration::from_hours(2);
        let docs = dir.path().join("docs");
        let folder = docs.join(".a.md.999999.0.part");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("kept.md"), b"x").unwrap();
        age_folder(&folder, hours_ago);
        let permark = dir.path().join(".permark");
        let aside = permark.join(".import.999998.0.part");
        std::fs::create_dir_all(&aside).unwrap();
        std::fs::write(aside.join("state.json"), b"{}").unwrap();
        age_folder(&aside, hours_ago);
        let beside = permark.join(".config.toml.999998.1.part");
        let file = std::fs::File::create(&beside).unwrap();
        file.set_modified(hours_ago).unwrap();
        drop(file);
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("target.md"), b"t").unwrap();
        let link = docs.join(".l.md.999999.2.part");
        let linked = crate::tree::tests::link_out(&link, outside.path());
        let temp = docs.join(".b.md.999999.3.part");
        let file = std::fs::File::create(&temp).unwrap();
        file.set_modified(hours_ago).unwrap();
        drop(file);
        let dirs = [docs, permark];
        let stale: Vec<PathBuf> = dirs
            .iter()
            .flat_map(|dir| stale_temps(dir, STALE_TEMP_AGE))
            .collect();
        assert_eq!(stale, std::slice::from_ref(&temp));
        let swept = sweep_stale_temps(&dirs);
        #[cfg(windows)]
        assert_eq!(swept, 1);
        assert_eq!(temp.exists(), swept == 0);
        assert_eq!(std::fs::read(folder.join("kept.md")).unwrap(), b"x");
        assert_eq!(std::fs::read(aside.join("state.json")).unwrap(), b"{}");
        assert!(beside.exists());
        if linked {
            assert!(std::fs::symlink_metadata(&link).is_ok());
            assert_eq!(
                std::fs::read(outside.path().join("target.md")).unwrap(),
                b"t"
            );
        }
    }

    /// A temp a crash leaves stays out of the file manager's sight; the file it becomes shows as the one it replaced.
    #[cfg(windows)]
    #[test]
    fn a_temp_is_hidden_while_written_and_the_file_it_becomes_is_not() {
        use std::os::windows::fs::MetadataExt;
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        const HIDDEN: u32 = 0x2;
        let hidden = |path: &Path| std::fs::metadata(path).unwrap().file_attributes() & HIDDEN != 0;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        std::fs::write(&path, b"one").unwrap();
        let saw = Arc::new(AtomicBool::new(false));
        slip::before_check(&path, {
            let saw = Arc::clone(&saw);
            let dir = dir.path().to_path_buf();
            move || {
                for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                    if entry.file_name().to_string_lossy().ends_with(".part") {
                        let attrs = entry.metadata().unwrap().file_attributes();
                        saw.store(attrs & HIDDEN != 0, Ordering::SeqCst);
                    }
                }
            }
        });
        write_atomic(&path, b"two").unwrap();
        assert!(saw.load(Ordering::SeqCst), "the temp was hidden");
        assert!(!hidden(&path));
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        // a new file, and a file the person hid, keep how they show
        let new = dir.path().join("b.md");
        write_atomic(&new, b"x").unwrap();
        assert!(!hidden(&new));
        let secret = dir.path().join("c.md");
        std::fs::write(&secret, b"x").unwrap();
        win::set_hidden(&secret, true).unwrap();
        write_atomic(&secret, b"y").unwrap();
        assert!(hidden(&secret));
        assert_eq!(std::fs::read(&secret).unwrap(), b"y");
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

    /// A file symlink at `link` to `target` (Windows: only with the symlink privilege, else `false`: skipped).
    pub(crate) fn file_link(link: &Path, target: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(target, link).is_ok()
        }
    }

    /// Only a plain file can still hold what was read: a folder or a FIFO at the path is a change, never read (a FIFO
    /// would keep the check waiting forever); a link to a plain file is followed, so a linked file stays writable.
    /// Nothing is written on a change and no temp stays behind.
    #[test]
    fn a_write_while_unchanged_takes_anything_but_a_plain_file_for_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let seen = hash(b"seen");
        let folder = dir.path().join("folder.md");
        std::fs::create_dir(&folder).unwrap();
        assert!(!write_atomic_if_unchanged(&folder, b"mine", seen).unwrap());
        assert!(folder.is_dir());
        // a link to a file holding other bytes: a change, link and target stay as they are
        let target = dir.path().join("target.md");
        std::fs::write(&target, "other").unwrap();
        let link = dir.path().join("link.md");
        if file_link(&link, &target) {
            assert!(!write_atomic_if_unchanged(&link, b"mine", seen).unwrap());
            assert!(std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink());
            // a link to a file holding the bytes read: the write lands where the link was, the target untouched
            std::fs::write(&target, "seen").unwrap();
            assert!(write_atomic_if_unchanged(&link, b"mine", seen).unwrap());
            assert_eq!(std::fs::read(&link).unwrap(), b"mine");
            assert_eq!(std::fs::read(&target).unwrap(), b"seen");
        } else {
            eprintln!("skipped the link case: no file links here");
            std::fs::write(&link, "seen").unwrap();
        }
        #[cfg(unix)]
        {
            let fifo = dir.path().join("fifo.md");
            let made = std::process::Command::new("mkfifo").arg(&fifo).status();
            assert!(made.is_ok_and(|s| s.success()));
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(write_atomic_if_unchanged(&fifo, b"mine", seen).ok());
            });
            let answer = rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("the check waited on a FIFO");
            assert_eq!(answer, Some(false));
        }
        let mut names = names_in(dir.path());
        names.retain(|name| name != "fifo.md");
        assert_eq!(names, ["folder.md", "link.md", "target.md"]);
    }

    /// A reader opens only a plain file: a folder is refused at once (Windows answers its open with access denied,
    /// which is retried), a FIFO never makes it wait for a writer.
    #[test]
    fn a_read_of_what_is_not_a_plain_file_is_refused_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("folder.md");
        std::fs::create_dir(&folder).unwrap();
        let refusal = |result: Result<Vec<u8>, FsxError>| match result {
            Err(FsxError::Io { source, .. }) => Some(source.kind()),
            _ => None,
        };
        assert_eq!(
            refusal(read_bytes(&folder, 1024)),
            Some(io::ErrorKind::InvalidData)
        );
        #[cfg(unix)]
        {
            let fifo = dir.path().join("fifo.md");
            let made = std::process::Command::new("mkfifo").arg(&fifo).status();
            assert!(made.is_ok_and(|s| s.success()));
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(refusal(read_bytes(&fifo, 1024)));
            });
            let answer = rx
                .recv_timeout(Duration::from_secs(10))
                .expect("the read waited on a FIFO");
            assert_eq!(answer, Some(io::ErrorKind::InvalidData));
        }
    }

    #[test]
    fn a_write_if_absent_creates_once_and_never_replaces_what_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new dir").join("a.toml");
        assert!(write_atomic_if_absent(&path, b"mine").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"mine");
        // a file is there now: left byte for byte, no temp behind
        assert!(!write_atomic_if_absent(&path, b"other").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"mine");
        assert_eq!(names_in(path.parent().unwrap()), ["a.toml"]);
        // a folder of that name counts as something there too
        let folder = dir.path().join("taken.toml");
        std::fs::create_dir(&folder).unwrap();
        assert!(!write_atomic_if_absent(&folder, b"mine").unwrap());
        assert!(folder.is_dir());
        assert_eq!(names_in(dir.path()), ["new dir", "taken.toml"]);
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

    /// Only an absolute path on a disk of this computer is local: a share, a device path, a relative path is not,
    /// whatever its spelling.
    #[test]
    fn only_an_absolute_path_on_a_local_disk_is_local() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_local_path(dir.path()));
        for odd in [
            r"\\server\share\x",
            "//server/share/x",
            r"\\?\UNC\server\share\x",
            r"\\.\pipe\x",
            r"\\.\C:\x",
            "relative",
            "./x",
            "../x",
            "",
        ] {
            assert!(!is_local_path(Path::new(odd)), "{odd:?}");
        }
        #[cfg(windows)]
        {
            assert!(is_local_path(Path::new(r"C:\")));
            assert!(is_local_path(Path::new(r"\\?\C:\x")));
            for odd in [
                r"C:x",
                r"\x",
                "/x",
                r"\\?\Volume{00000000-0000-0000-0000-000000000000}\x",
            ] {
                assert!(!is_local_path(Path::new(odd)), "{odd:?}");
            }
            // a drive letter with no drive behind it tells nothing
            if let Some(letter) = (b'A'..=b'Z')
                .rev()
                .find(|l| !Path::new(&format!("{}:\\", char::from(*l))).exists())
            {
                let free = format!(r"{}:\x", char::from(letter));
                assert!(!is_local_path(Path::new(&free)), "{free}");
            }
        }
        #[cfg(unix)]
        {
            assert!(is_local_path(Path::new("/x")));
            assert!(is_local_path(Path::new("/")));
        }
    }

    /// A folder a cloud sync service registered (and anything below it) is a cloud folder; a plain folder beside it
    /// is not.
    #[test]
    fn a_folder_a_sync_service_registered_is_a_cloud_folder() {
        let outside = tempfile::tempdir().unwrap();
        assert!(!in_cloud_folder(outside.path()));
        #[cfg(windows)]
        {
            let cloud = tempfile::tempdir().unwrap();
            let inner = cloud.path().join("notes ação");
            std::fs::create_dir(&inner).unwrap();
            assert!(!in_cloud_folder(cloud.path()), "not registered yet");
            let Some(_root) = SyncRoot::register(cloud.path()) else {
                return;
            };
            assert!(in_cloud_folder(cloud.path()));
            assert!(in_cloud_folder(&inner));
            assert!(!in_cloud_folder(outside.path()));
        }
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
