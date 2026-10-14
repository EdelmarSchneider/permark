//! Small helpers shared by several modules: clocks, hashing, read retry, and the quiet/max coalescer
//! used by both the filesystem aggregator and the git worker.

use std::io;
use std::path::Path;
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

/// Writes a file atomically (temp file in the same directory, then rename).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map_or_else(|| "state".to_owned(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.{}.part", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
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
    fn hash_hex_is_stable_and_16_chars() {
        assert_eq!(hash_hex(b"abc").len(), 16);
        assert_eq!(hash_hex(b"abc"), hash_hex(b"abc"));
        assert_ne!(hash_hex(b"abc"), hash_hex(b"abd"));
    }
}
