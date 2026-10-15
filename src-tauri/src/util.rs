//! Small helpers shared by several modules: clocks, hashing, read retry, and the quiet/max coalescer
//! used by both the filesystem aggregator and the git worker.

use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch for a `SystemTime` (0 for times before the epoch).
pub(crate) fn system_time_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Current wall-clock time in milliseconds since the Unix epoch.
pub(crate) fn now_ms() -> u64 {
    system_time_ms(SystemTime::now())
}

/// Modification time of a file in milliseconds since the epoch, or 0 when unavailable.
pub(crate) fn modified_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified().map_or(0, system_time_ms)
}

/// xxh3 64-bit hash of some bytes.
pub(crate) fn hash_bytes(bytes: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(bytes)
}

/// xxh3 64-bit hash as 16 lowercase hex digits.
pub(crate) fn hash_hex(bytes: &[u8]) -> String {
    format!("{:016x}", hash_bytes(bytes))
}

/// The MIME type of an image file, from its extension.
pub(crate) fn image_mime(rel: &str) -> Option<&'static str> {
    let ext = Path::new(rel)
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

/// Windows sharing (32) and lock (33) violations, which are transient while another process writes.
pub(crate) fn is_sharing_violation(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(32 | 33))
}

/// Runs `op`, retrying transient errors (sharing violations, access denied) after each delay in `delays`.
pub(crate) fn retry_io<T>(
    delays: &[Duration],
    mut op: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    let mut attempt = 0;
    loop {
        match op() {
            Ok(v) => return Ok(v),
            Err(e)
                if attempt < delays.len()
                    && (is_sharing_violation(&e)
                        || e.kind() == io::ErrorKind::PermissionDenied) =>
            {
                thread::sleep(delays[attempt]);
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

const READ_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(200),
];

/// Reads a whole file, retrying Windows sharing violations at 50/100/200 ms.
pub(crate) fn read_with_retry(path: &Path) -> io::Result<Vec<u8>> {
    retry_io(&READ_RETRY_DELAYS, || std::fs::read(path))
}

/// Canonicalizes the deepest existing ancestor of `path` and returns it with the components below it
/// that do not exist (yet), in path order. Canonicalizing follows symlinks and junctions, expands
/// Windows 8.3 short names (`RUNNER~1`) and fixes the case, and drops the `\\?\` prefix when it can
/// (see `dunce`). `None` when nothing on the path exists or it ends in `..`.
pub(crate) fn split_existing(path: &Path) -> Option<(PathBuf, Vec<&OsStr>)> {
    let mut existing = path;
    let mut tail: Vec<&OsStr> = Vec::new();
    let canon = loop {
        if let Ok(c) = dunce::canonicalize(existing) {
            break c;
        }
        tail.push(existing.file_name()?);
        existing = existing.parent()?;
    };
    tail.reverse();
    Some((canon, tail))
}

/// `path` in canonical form even when its last components do not exist: the deepest existing
/// ancestor is canonicalized (see [`split_existing`]) and the remaining components are appended.
/// Use it to compare an arbitrary absolute path with a canonical root.
pub(crate) fn canonicalize_lenient(path: &Path) -> Option<PathBuf> {
    let (mut canon, tail) = split_existing(path)?;
    canon.extend(tail);
    Some(canon)
}

const RENAME_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(200),
];

/// Counts the temp files of this process, so two writes of the same file never share one.
static TEMP_COUNT: AtomicU64 = AtomicU64::new(0);

/// A fresh hidden temp name next to `path` (`.<name>.<pid>.<n>.part`).
fn temp_path(path: &Path) -> PathBuf {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map_or_else(|| "state".to_owned(), |n| n.to_string_lossy().into_owned());
    let n = TEMP_COUNT.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{name}.{}.{n}.part", std::process::id()))
}

/// What [`write_atomic_guarded`] did with the target.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Guarded<R> {
    /// The new bytes are in place.
    Written,
    /// The guard said no, and the target was left alone.
    Refused(R),
}

/// Writes a file atomically: the bytes go to a temp file in the same directory, which is flushed to
/// disk before it is renamed into place, so a power loss leaves the old file or the new one but never
/// an empty one. A rename that fails because another program holds the target (antivirus, a sync
/// client) is retried briefly. The permissions of the file being replaced (the mode bits on Unix, the
/// read-only attribute on Windows) are given to the new one; ACLs, ownership and hard links are not
/// carried over.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_guarded(path, bytes, || Ok(None::<()>)).map(|_| ())
}

/// [`write_atomic`] with a last look at the target. `guard` runs after the temp file is written and
/// flushed (which can take many milliseconds), immediately before each attempt to rename it into place:
/// `Ok(None)` lets the rename go ahead, `Ok(Some(why))` throws the temp file away and leaves the target
/// as it is. The save of a document uses it to compare the file on disk with the version the editor
/// started from, so that a writer who got in during the flush is noticed.
///
/// What cannot be closed portably is the gap between the guard's read and the rename itself (no
/// operating system offers "replace this file only if it still holds these bytes"). It is a few
/// microseconds of syscalls instead of the whole write, and it is the known limit of this check.
pub(crate) fn write_atomic_guarded<R>(
    path: &Path,
    bytes: &[u8],
    mut guard: impl FnMut() -> io::Result<Option<R>>,
) -> io::Result<Guarded<R>> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = temp_path(path);
    let mut refused: Option<R> = None;
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| {
            // Best effort: a file system that cannot carry the permissions over still gets the new bytes.
            if let Some(meta) = std::fs::metadata(path)
                .ok()
                .filter(std::fs::Metadata::is_file)
            {
                let _ = std::fs::set_permissions(&tmp, meta.permissions());
            }
            retry_io(&RENAME_RETRY_DELAYS, || {
                if let Some(why) = guard()? {
                    refused = Some(why);
                    return Ok(());
                }
                std::fs::rename(&tmp, path)
            })
        });
    match (result, refused) {
        (Ok(()), None) => Ok(Guarded::Written),
        (Ok(()), Some(why)) => {
            discard(&tmp);
            Ok(Guarded::Refused(why))
        }
        (Err(e), _) => {
            discard(&tmp);
            Err(e)
        }
    }
}

/// Removes a temp file that is not going to be used. It may carry the read-only attribute of the file it
/// was going to replace, which on Windows can stop it from being deleted.
fn discard(tmp: &Path) {
    if std::fs::remove_file(tmp).is_err() {
        if let Ok(meta) = std::fs::metadata(tmp) {
            let mut permissions = meta.permissions();
            // the temp file is ours and about to be deleted: making it writable exposes nothing
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(tmp, permissions);
        }
        let _ = std::fs::remove_file(tmp);
    }
}

/// Escapes text for HTML element content and double-quoted attribute values.
pub(crate) fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Stack size of the threads that parse or render documents. The parsers refuse absurd nesting
/// first; this is the second line of defence, so that a missed case cannot overflow the default
/// (1 to 2 MiB) stack and take the whole process down. The memory is only reserved, not used.
pub(crate) const PARSER_STACK_BYTES: usize = 64 * 1024 * 1024;

/// A thread builder with the parser stack size and the given name.
pub(crate) fn parser_thread(name: &str) -> thread::Builder {
    thread::Builder::new()
        .name(name.to_owned())
        .stack_size(PARSER_STACK_BYTES)
}

/// Runs `work` on a scoped thread with a [`PARSER_STACK_BYTES`] stack and returns its result. If no
/// thread can be started, `work` runs on the calling thread instead.
pub(crate) fn on_parser_stack<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    let mut task = Some(work);
    let mut result: Option<T> = None;
    thread::scope(|scope| {
        let (task, result) = (&mut task, &mut result);
        let spawned = parser_thread("trailmark-render").spawn_scoped(scope, move || {
            *result = task.take().map(|work| work());
        });
        if let Ok(handle) = spawned {
            if let Err(panic) = handle.join() {
                std::panic::resume_unwind(panic);
            }
        }
    });
    match (result, task) {
        (Some(value), _) => value,
        (None, Some(work)) => work(),
        // Only reachable if the thread took the work and produced nothing, which it cannot do.
        (None, None) => unreachable!("the render thread returns a value or panics"),
    }
}

/// Fires once activity has been quiet for `quiet`, but no later than `max` after the first activity.
#[derive(Debug, Clone)]
pub(crate) struct Coalescer {
    quiet: Duration,
    max: Duration,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Coalescer {
    pub(crate) fn new(quiet: Duration, max: Duration) -> Self {
        Self {
            quiet,
            max,
            first: None,
            last: None,
        }
    }

    /// Records activity at `now`.
    pub(crate) fn touch(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    #[cfg(test)]
    pub(crate) fn is_pending(&self) -> bool {
        self.first.is_some()
    }

    /// The instant at which the pending activity should be flushed.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        let (first, last) = (self.first?, self.last?);
        Some((last + self.quiet).min(first + self.max))
    }

    pub(crate) fn due(&self, now: Instant) -> bool {
        self.deadline().is_some_and(|d| now >= d)
    }

    pub(crate) fn reset(&mut self) {
        self.first = None;
        self.last = None;
    }
}

/// The 8.3 short spelling of an existing Windows path (`C:\Users\RUNNER~1\...`), or `None` when the
/// volume has 8.3 names disabled (the path then comes back unchanged). Test helper: a CI runner's temp
/// dir is spelled this way, while the vault root is canonical.
#[cfg(all(test, windows))]
pub(crate) fn short_path(path: &Path) -> Option<std::path::PathBuf> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("cmd")
        .arg("/C")
        .raw_arg(format!("for %I in (\"{}\") do @echo %~sI", path.display()))
        .output()
        .ok()?;
    let short = std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    (!short.as_os_str().is_empty()
        && short.to_string_lossy().to_lowercase() != path.to_string_lossy().to_lowercase())
    .then_some(short)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalescer_fires_after_quiet_period() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(Duration::from_millis(150), Duration::from_secs(1));
        assert!(!c.is_pending());
        assert!(!c.due(t0));
        c.touch(t0);
        assert!(!c.due(t0 + Duration::from_millis(149)));
        assert!(c.due(t0 + Duration::from_millis(150)));
    }

    #[test]
    fn coalescer_extends_while_busy_but_caps_at_max() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(Duration::from_millis(150), Duration::from_secs(1));
        for ms in (0..=900).step_by(100) {
            c.touch(t0 + Duration::from_millis(ms));
        }
        // Still busy: the quiet deadline keeps moving but the max deadline wins at 1 s.
        assert!(!c.due(t0 + Duration::from_millis(999)));
        assert_eq!(c.deadline(), Some(t0 + Duration::from_secs(1)));
        assert!(c.due(t0 + Duration::from_secs(1)));
        c.reset();
        assert!(!c.is_pending());
    }

    #[test]
    fn retry_io_retries_transient_errors_only() {
        let mut calls = 0;
        let r = retry_io(&[Duration::ZERO, Duration::ZERO], || {
            calls += 1;
            if calls < 3 {
                Err(io::Error::from_raw_os_error(32))
            } else {
                Ok(calls)
            }
        });
        assert_eq!(r.ok(), Some(3));

        let mut calls = 0;
        let r: io::Result<()> = retry_io(&[Duration::ZERO], || {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::NotFound))
        });
        assert!(r.is_err());
        assert_eq!(calls, 1, "NotFound is not transient");
    }

    #[test]
    fn retry_io_gives_up_after_the_last_delay() {
        let mut calls = 0;
        let r: io::Result<()> = retry_io(&[Duration::ZERO], || {
            calls += 1;
            Err(io::Error::from_raw_os_error(33))
        });
        assert!(r.is_err());
        assert_eq!(calls, 2);
    }

    #[test]
    fn escape_html_escapes_markup() {
        assert_eq!(
            escape_html("<a href=\"x\">&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn write_atomic_replaces_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("sub").join("a.json");
        write_atomic(&p, b"one").expect("first write");
        write_atomic(&p, b"two").expect("second write");
        assert_eq!(std::fs::read(&p).expect("read"), b"two");
    }

    #[test]
    fn write_atomic_leaves_no_temp_file_behind_even_when_the_target_cannot_be_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ok = dir.path().join("a.json");
        write_atomic(&ok, b"data").expect("write");
        // a directory in the way: the rename fails, and the temp file is cleaned up
        let blocked = dir.path().join("b.json");
        std::fs::create_dir(&blocked).expect("mkdir");
        assert!(write_atomic(&blocked, b"data").is_err());
        let mut names: Vec<String> = std::fs::read_dir(dir.path())
            .expect("ls")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["a.json", "b.json"]);
    }

    /// Whether `name` looks like `.a.md.<pid>.<n>.part`.
    fn is_temp_of_a(name: &str) -> bool {
        name.starts_with(".a.md.") && Path::new(name).extension().is_some_and(|e| e == "part")
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("ls")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_guard_looks_at_the_target_after_the_temp_file_is_written_and_can_refuse() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("a.md");
        std::fs::write(&p, "old").expect("write");
        let mut temp_was_complete = false;
        let outcome = write_atomic_guarded(&p, b"new", || {
            temp_was_complete = names_in(dir.path()).iter().any(|name| {
                is_temp_of_a(name)
                    && std::fs::read(dir.path().join(name)).is_ok_and(|b| b == b"new")
            });
            Ok(Some("someone else wrote it"))
        })
        .expect("no io error");
        assert_eq!(outcome, Guarded::Refused("someone else wrote it"));
        assert!(
            temp_was_complete,
            "the guard must run once the new bytes are on disk"
        );
        assert_eq!(std::fs::read(&p).expect("read"), b"old", "target untouched");
        assert_eq!(names_in(dir.path()), ["a.md"], "temp file removed");
    }

    #[test]
    fn a_guard_that_lets_go_replaces_the_target_and_an_error_leaves_nothing_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("a.md");
        std::fs::write(&p, "old").expect("write");
        let outcome = write_atomic_guarded(&p, b"new", || Ok(None::<()>)).expect("write");
        assert_eq!(outcome, Guarded::Written);
        assert_eq!(std::fs::read(&p).expect("read"), b"new");

        let failed = write_atomic_guarded(&p, b"newer", || -> io::Result<Option<()>> {
            Err(io::Error::other("cannot look"))
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&p).expect("read"), b"new");
        assert_eq!(names_in(dir.path()), ["a.md"]);
    }

    #[test]
    fn every_write_gets_a_temp_file_of_its_own() {
        let p = Path::new("dir").join("a.md");
        let (one, two) = (temp_path(&p), temp_path(&p));
        assert_ne!(one, two);
        assert_eq!(one.parent(), Some(Path::new("dir")));
        let name = one
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        assert!(is_temp_of_a(&name), "{name}");
    }

    #[cfg(unix)]
    #[test]
    fn a_rewritten_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("private.md");
        std::fs::write(&p, "secret").expect("write");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        write_atomic(&p, b"still secret").expect("write");
        let mode = std::fs::metadata(&p).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(std::fs::read(&p).expect("read"), b"still secret");
    }

    #[test]
    fn on_parser_stack_returns_the_value_and_has_room_for_deep_recursion() {
        fn depth(n: u32) -> u32 {
            if n == 0 {
                0
            } else {
                1 + depth(std::hint::black_box(n - 1))
            }
        }
        // About 40 MiB of frames in a debug build: far more than a default thread stack allows.
        assert_eq!(on_parser_stack(|| depth(300_000)), 300_000);
        let local = String::from("borrowed");
        assert_eq!(on_parser_stack(|| local.len()), 8);
    }

    #[test]
    fn split_existing_separates_the_missing_tail() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("a")).expect("mkdir");
        let canon = dunce::canonicalize(dir.path()).expect("canonicalize");

        let missing = dir.path().join("a").join("b").join("c.md");
        let (head, tail) = split_existing(&missing).expect("the temp dir exists");
        assert_eq!(head, canon.join("a"));
        assert_eq!(tail, [OsStr::new("b"), OsStr::new("c.md")]);

        let (head, tail) = split_existing(dir.path()).expect("exists");
        assert_eq!((head, tail.len()), (canon.clone(), 0));

        assert_eq!(
            canonicalize_lenient(&dir.path().join("a").join("b").join("c.md")),
            Some(canon.join("a").join("b").join("c.md"))
        );
        // Nothing to anchor a relative path to.
        assert_eq!(canonicalize_lenient(Path::new("no-such-dir/x.md")), None);
    }

    #[cfg(windows)]
    #[test]
    fn canonicalize_lenient_expands_8dot3_short_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let long = dir.path().join("a-long-folder-name");
        std::fs::create_dir_all(&long).expect("mkdir");
        let Some(short) = short_path(&long) else {
            eprintln!("skipped: 8.3 short names are disabled on this volume");
            return;
        };
        assert!(short.to_string_lossy().contains('~'));
        assert_eq!(
            canonicalize_lenient(&short.join("not-yet").join("n.md")),
            Some(
                dunce::canonicalize(&long)
                    .expect("canonicalize")
                    .join("not-yet")
                    .join("n.md")
            )
        );
    }

    #[test]
    fn hash_hex_is_stable_and_16_chars() {
        assert_eq!(hash_hex(b"abc").len(), 16);
        assert_eq!(hash_hex(b"abc"), hash_hex(b"abc"));
        assert_ne!(hash_hex(b"abc"), hash_hex(b"abd"));
    }
}
