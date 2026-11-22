//! Crash reports: what a panic leaves behind, as plain text files in the profile's [`FOLDER`]. The panic hook writes one
//! file per panic ([`Reporter::record`]); the next start offers the newest one the person has not seen ([`offer`]) and
//! keeps only the [`KEEP`] newest. Nothing is ever sent from here: the person copies a report or files it themself.
//! The home folder is written as `~` (a report may leave the computer, by the person's choice).

use std::backtrace::Backtrace;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat};
use serde::{Deserialize, Serialize};

use crate::error::IpcError;
use crate::fsx::{self, Zone};
use crate::state::{AppDirs, StateData, StateFile, StateStore};

/// The reports' folder in the profile.
pub const FOLDER: &str = "crash";
/// Reports kept; older ones are removed at the next start (never by the hook).
pub const KEEP: usize = 10;
const STATE_FILE: &str = "crash-reports.json";
/// Most bytes of a panic message a report holds (a message may quote document text at length).
const MESSAGE_CAP: usize = 16 * 1024;
/// Frames of a backtrace a report keeps from its start and from its end: a deep recursion repeats the same frames by
/// the thousand, the panic's own frames come first and the thread's start last.
const FRAMES_HEAD: usize = 200;
const FRAMES_TAIL: usize = 50;
/// Most bytes of a backtrace a report holds, whatever its frames (frames of very long names).
const BACKTRACE_CAP: usize = 256 * 1024;
/// Most bytes of a report read back; the hook's own files stay far below (message and backtrace are capped).
const READ_CAP: u64 = 1024 * 1024;
/// Names tried for one report: threads of one process may panic in the same millisecond.
const NAME_TRIES: u32 = 4;

/// What a panic hook knows about one panic.
#[derive(Debug, Clone)]
pub struct Panic<'a> {
    /// How the process runs: `app`, `mcp`, ...
    pub mode: &'a str,
    pub thread: Option<&'a str>,
    /// `file:line:column` of the panic.
    pub location: Option<String>,
    pub message: &'a str,
    pub backtrace: String,
}

/// Writes reports into the crash folder of one profile. Made once before the hook is set: the hook itself takes no
/// lock, reads no setting and never panics.
#[derive(Debug, Clone)]
pub struct Reporter {
    folder: PathBuf,
    /// The home folder as it appears in paths: its name, and on Windows its 8.3 short name when the volume keeps one
    /// (some tools print that). Empty: unknown, or a root (nothing worth hiding, everything to break).
    homes: Vec<String>,
}

impl Reporter {
    /// Looks up the short name of `home` now: the hook itself asks the system nothing.
    #[must_use]
    pub fn new(dirs: &AppDirs, home: Option<&Path>) -> Self {
        let homes = home
            .filter(|home| home.parent().is_some())
            .into_iter()
            .flat_map(|home| std::iter::once(home.to_path_buf()).chain(fsx::short_name(home)))
            .map(|home| {
                home.to_string_lossy()
                    .trim_end_matches(['/', '\\'])
                    .to_owned()
            })
            .filter(|home| !home.is_empty())
            .collect();
        Self {
            folder: folder(dirs),
            homes,
        }
    }

    /// The system's profile and home folder; `None`: no profile folder on this system.
    #[must_use]
    pub fn system() -> Option<Self> {
        let dirs = AppDirs::system()?;
        Some(Self::new(&dirs, dirs::home_dir().as_deref()))
    }

    /// Writes the report of the panic `info` tells; a failure leaves no report (the panic goes on regardless).
    pub fn record(&self, info: &std::panic::PanicHookInfo<'_>, mode: &str) {
        let thread = std::thread::current();
        let panic = Panic {
            mode,
            thread: thread.name(),
            location: info.location().map(ToString::to_string),
            message: info.payload_as_str().unwrap_or("(no message)"),
            backtrace: Backtrace::force_capture().to_string(),
        };
        let _ = self.write(&panic);
    }

    /// Writes `panic`'s report as `<ms>-<pid>.txt`, create only: two processes never write one file.
    ///
    /// # Errors
    /// IO failure.
    pub fn write(&self, panic: &Panic<'_>) -> io::Result<PathBuf> {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        self.write_at(panic, at, std::process::id())
    }

    fn write_at(&self, panic: &Panic<'_>, at: u64, pid: u32) -> io::Result<PathBuf> {
        let text = self
            .homes
            .iter()
            .fold(report_text(panic, at), |text, home| {
                tilde(&text, home, cfg!(windows))
            });
        // a report may quote document text: other users of the computer never read it
        fsx::private_dir(&self.folder)?;
        for n in 0..NAME_TRIES {
            let name = if n == 0 {
                format!("{at}-{pid}.txt")
            } else {
                format!("{at}-{pid}-{n}.txt")
            };
            let path = self.folder.join(name);
            if fsx::write_atomic_if_absent(&path, text.as_bytes())? {
                return Ok(path);
            }
        }
        Err(io::Error::from(io::ErrorKind::AlreadyExists))
    }
}

/// The crash folder of the profile in `dirs`.
#[must_use]
pub fn folder(dirs: &AppDirs) -> PathBuf {
    dirs.profile.join(FOLDER)
}

/// A report found in the crash folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashReport {
    /// The file's name (`<ms>-<pid>.txt`).
    pub file: String,
    pub path: PathBuf,
    /// The text as written (home folder as `~`).
    pub text: String,
    /// When it was written (milliseconds since 1970).
    at: u64,
}

impl CrashReport {
    /// The version of PerMark that stopped.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        header(&self.text, "Version")
    }

    /// The system it ran on (`windows x86_64`).
    #[must_use]
    pub fn system(&self) -> Option<&str> {
        header(&self.text, "System")
    }
}

/// The newest report a dismissed offer covered.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrashState {
    /// Time of the newest report the person dismissed (milliseconds since 1970).
    seen: u64,
}

impl StateData for CrashState {
    const VERSION: u32 = 1;
}

fn state_file(store: &StateStore) -> StateFile<CrashState> {
    store.app_file(STATE_FILE)
}

/// One report file: time in its name, name, path.
type Found = (u64, String, PathBuf);

/// The newest report the person has not dismissed yet, after removing all but the [`KEEP`] newest (the app store's
/// profile).
#[must_use]
pub fn offer(store: &StateStore) -> Option<CrashReport> {
    let (at, file, path) = rotate(&folder(store.app_dirs()), KEEP).pop()?;
    if at <= state_file(store).load().into_value().seen {
        return None;
    }
    let bytes = fsx::read_regular(&path, READ_CAP, Zone::Anywhere).ok()?;
    Some(CrashReport {
        file,
        path,
        text: String::from_utf8_lossy(&bytes).into_owned(),
        at,
    })
}

/// Removes all but the [`KEEP`] newest reports of the profile in `dirs` (best effort): for processes that never offer
/// one (agent connections), so their crashes do not pile up.
pub fn trim(dirs: &AppDirs) {
    rotate(&folder(dirs), KEEP);
}

/// The person dismissed `report`'s offer: neither it nor an older one is offered again.
///
/// # Errors
/// The record cannot be written.
pub fn dismiss(store: &StateStore, report: &CrashReport) -> Result<(), IpcError> {
    state_file(store).update(|now| {
        let seen = now.map_or(0, |state| state.seen);
        (report.at > seen).then_some(CrashState { seen: report.at })
    })?;
    Ok(())
}

/// The reports in `folder`, oldest first. Files of other names are none of them.
fn reports(folder: &Path) -> Vec<Found> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<Found> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            Some((report_time(&name)?, name, entry.path()))
        })
        .collect();
    found.sort();
    found
}

/// The time in a report's name (`<ms>-<pid>[-<n>].txt`).
fn report_time(name: &str) -> Option<u64> {
    let (at, rest) = name.strip_suffix(".txt")?.split_once('-')?;
    let numbers = rest
        .split('-')
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if numbers {
        at.parse().ok()
    } else {
        None
    }
}

/// Removes all but the `keep` newest reports (best effort); returns the ones left, oldest first.
fn rotate(folder: &Path, keep: usize) -> Vec<Found> {
    let mut found = reports(folder);
    let old = found.len().saturating_sub(keep);
    for (_, _, path) in found.drain(..old) {
        // the hook's own files, never the person's: removed, not recycled
        let _ = fs::remove_file(path);
    }
    found
}

/// A header line's value (`Version: 0.17.0` -> `0.17.0`), from the lines before the message.
fn header<'t>(text: &'t str, name: &str) -> Option<&'t str> {
    text.lines()
        .take_while(|line| !line.is_empty())
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(": "))
}

/// `text` with every spelling of the folder `home` written as `~`: case-blind and with either separator on Windows
/// (paths there come in both), exact elsewhere; a run of separators counts as one (a path printed with `{:?}` doubles
/// each backslash). A longer name sharing the start (`/home/ann2` for `/home/ann`) or a folder ending the same
/// (`/data/home/ann`) is another folder and stays.
fn tilde(text: &str, home: &str, windows: bool) -> String {
    let separator = |b: u8| b == b'/' || (windows && b == b'\\');
    // a byte that goes on with a file name (non-ASCII bytes too)
    let in_name =
        |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.') || !b.is_ascii();
    let same = |a: u8, b: u8| a == b || (windows && a.eq_ignore_ascii_case(&b));
    let (bytes, home) = (text.as_bytes(), home.as_bytes());
    // where `home` spelled from `at` ends; `None`: not there
    let end_of = |at: usize| {
        let (mut i, mut h) = (at, 0);
        while let Some(&want) = home.get(h) {
            let got = *bytes.get(i)?;
            if separator(want) {
                if !separator(got) {
                    return None;
                }
                while bytes.get(i).is_some_and(|&b| separator(b)) {
                    i += 1;
                }
                while home.get(h).is_some_and(|&b| separator(b)) {
                    h += 1;
                }
            } else if same(got, want) {
                (i, h) = (i + 1, h + 1);
            } else {
                return None;
            }
        }
        Some(i)
    };
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let end = (!home.is_empty() && (i == 0 || bytes.get(i - 1).is_some_and(|&b| !in_name(b))))
            .then(|| end_of(i))
            .flatten()
            .filter(|&end| bytes.get(end).is_none_or(|&b| !in_name(b)));
        if let Some(end) = end {
            out.push(b'~');
            i = end;
        } else {
            out.extend(bytes.get(i));
            i += 1;
        }
    }
    // a match starts and ends on whole characters (home is text; only ASCII bytes compare loosely): still UTF-8
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// The head of `text` up to `cap` bytes, cut on a character (`…` marks a cut).
fn cut(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_owned();
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", text.get(..end).unwrap_or_default())
}

/// `backtrace` with at most [`FRAMES_HEAD`] + [`FRAMES_TAIL`] frames (a line `  12: symbol` starts one; the frames
/// between are counted), then at most [`BACKTRACE_CAP`] bytes: a deep recursion still leaves a report the next start
/// can read.
fn short_backtrace(backtrace: &str) -> String {
    let frame = |line: &str| {
        let line = line.trim_start();
        let digits = line.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && line.as_bytes().get(digits) == Some(&b':')
    };
    let mut starts = Vec::new();
    let mut at = 0;
    for line in backtrace.split_inclusive('\n') {
        if frame(line) {
            starts.push(at);
        }
        at += line.len();
    }
    let left_out = starts.len().saturating_sub(FRAMES_HEAD + FRAMES_TAIL);
    let kept = match (
        starts.get(FRAMES_HEAD),
        starts.get(starts.len().saturating_sub(FRAMES_TAIL)),
    ) {
        (Some(&head), Some(&tail)) if left_out > 0 => format!(
            "{}      … {left_out} frames left out …\n{}",
            backtrace.get(..head).unwrap_or_default(),
            backtrace.get(tail..).unwrap_or_default(),
        ),
        _ => backtrace.to_owned(),
    };
    cut(&kept, BACKTRACE_CAP)
}

/// The report's text; `at` = milliseconds since 1970.
fn report_text(panic: &Panic<'_>, at: u64) -> String {
    let time = i64::try_from(at)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .map_or_else(
            || "unknown".to_owned(),
            |time| time.to_rfc3339_opts(SecondsFormat::Secs, true),
        );
    format!(
        "PerMark crash report\nVersion: {version}\nSystem: {os} {arch}\nMode: {mode}\nTime: {time}\n\
         Thread: {thread}\nLocation: {location}\n\nMessage:\n{message}\n\nBacktrace:\n{backtrace}\n",
        version = crate::app_info().version,
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        mode = panic.mode,
        thread = panic.thread.unwrap_or("unnamed"),
        location = panic.location.as_deref().unwrap_or("unknown"),
        message = cut(panic.message, MESSAGE_CAP),
        backtrace = short_backtrace(&panic.backtrace),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panic_of<'a>(message: &'a str, location: &str) -> Panic<'a> {
        Panic {
            mode: "app",
            thread: Some("main"),
            location: Some(location.to_owned()),
            message,
            backtrace: "   0: permark::main\n   1: std::rt::lang_start".to_owned(),
        }
    }

    fn fx() -> (tempfile::TempDir, AppDirs) {
        let base = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(base.path());
        (base, dirs)
    }

    #[test]
    fn home_becomes_a_tilde_case_blind_with_either_separator_on_windows() {
        let home = r"C:\Users\Ann";
        let text =
            r"at C:\Users\Ann\.cargo\registry\x.rs and c:/users/ann/doc.md, \\?\C:\USERS\ANN\a";
        assert_eq!(
            tilde(text, home, true),
            r"at ~\.cargo\registry\x.rs and ~/doc.md, \\?\~\a"
        );
        // a longer name sharing the start is another folder
        assert_eq!(
            tilde(r"C:\Users\Anna\x C:\Users\Ann", home, true),
            r"C:\Users\Anna\x ~"
        );
    }

    #[test]
    fn a_run_of_separators_counts_as_one() {
        // a path printed with `{:?}` doubles each backslash
        assert_eq!(
            tilde(r"at C:\\Users\\Ann\\x.rs", r"C:\Users\Ann", true),
            r"at ~\\x.rs"
        );
        assert_eq!(
            tilde(r"C:\\Users\\Anna\\x", r"C:\Users\Ann", true),
            r"C:\\Users\\Anna\\x"
        );
        assert_eq!(tilde("/home//ann/x", "/home/ann", false), "~/x");
        assert_eq!(tilde(r"\home\ann", "/home/ann", false), r"\home\ann");
    }

    #[test]
    fn a_deep_backtrace_keeps_its_head_and_tail_and_the_report_is_still_offered() {
        use std::fmt::Write;
        let (_base, dirs) = fx();
        let mut backtrace = String::new();
        let mut frames = 0;
        while backtrace.len() < 2 * 1024 * 1024 {
            writeln!(
                backtrace,
                "{frames:>4}: permark_core::deep::recursion\n             at ./src/deep.rs:{frames}:5"
            )
            .unwrap();
            frames += 1;
        }
        let panic = Panic {
            backtrace,
            ..panic_of("deep", "a.rs:1:1")
        };
        Reporter::new(&dirs, None)
            .write_at(&panic, 1000, 1)
            .unwrap();
        let report = offer(&StateStore::app(&dirs)).expect("a deep recursion is offered too");
        assert!(report.text.len() < 512 * 1024, "{}", report.text.len());
        assert!(report.text.contains("   0: permark_core::deep::recursion"));
        assert!(report
            .text
            .contains(&format!("{}: permark_core::deep::recursion", frames - 1)));
        assert!(report.text.contains("frames left out"));
    }

    #[test]
    fn every_spelling_of_home_the_reporter_knows_becomes_a_tilde() {
        let (_base, dirs) = fx();
        let mut reporter = Reporter::new(&dirs, Some(Path::new("/x/Ann Longname")));
        reporter.homes.push("/x/ANNLON~1".to_owned());
        let panic = panic_of("at /x/ANNLON~1/doc.md", "/x/Ann Longname/src/a.rs:1:1");
        let text = fs::read_to_string(reporter.write_at(&panic, 5, 1).unwrap()).unwrap();
        assert!(
            text.contains("at ~/doc.md") && text.contains("Location: ~/src/a.rs"),
            "{text}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_short_name_of_home_is_hidden_on_windows() {
        let (base, dirs) = fx();
        let home = base.path().join("A Long Home Folder Name");
        fs::create_dir(&home).unwrap();
        // a volume that keeps no short names has nothing more to hide
        let Some(short) = fsx::short_name(&home) else {
            return;
        };
        let short = short.display().to_string();
        let panic = panic_of("boom", &format!(r"{short}\src\a.rs:1:1"));
        let reporter = Reporter::new(&dirs, Some(&home));
        let text = fs::read_to_string(reporter.write_at(&panic, 5, 1).unwrap()).unwrap();
        assert!(text.contains(r"Location: ~\src\a.rs"), "{short}\n{text}");
    }

    #[test]
    fn trim_keeps_the_newest_reports_and_marks_none_seen() {
        let (_base, dirs) = fx();
        let reporter = Reporter::new(&dirs, None);
        for at in 1..=(KEEP as u64 + 2) {
            reporter
                .write_at(&panic_of("x", "a.rs:1:1"), at * 1000, 3)
                .unwrap();
        }
        trim(&dirs);
        assert_eq!(fs::read_dir(folder(&dirs)).unwrap().count(), KEEP);
        let newest = format!("{}-3.txt", (KEEP as u64 + 2) * 1000);
        assert_eq!(offer(&StateStore::app(&dirs)).unwrap().file, newest);
    }

    #[cfg(unix)]
    #[test]
    fn only_this_user_may_enter_the_crash_folder() {
        use std::os::unix::fs::PermissionsExt;
        let (_base, dirs) = fx();
        Reporter::new(&dirs, None)
            .write_at(&panic_of("x", "a.rs:1:1"), 1, 1)
            .unwrap();
        let mode = fs::metadata(folder(&dirs)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn home_becomes_a_tilde_exactly_elsewhere() {
        let home = "/home/ann";
        assert_eq!(tilde("/home/ann/x.md", home, false), "~/x.md");
        assert_eq!(tilde("/HOME/ANN/x.md", home, false), "/HOME/ANN/x.md");
        assert_eq!(
            tilde("/home/ann2/x /data/home/ann/y", home, false),
            "/home/ann2/x /data/home/ann/y"
        );
        assert_eq!(tilde("é /home/ann:é", home, false), "é ~:é");
        assert_eq!(tilde("é/home/ann", home, false), "é/home/ann");
    }

    #[test]
    fn a_root_or_missing_home_hides_nothing() {
        let (_base, dirs) = fx();
        let panic = panic_of("at /x", "src/a.rs:1:2");
        for home in [Some(Path::new("/")), None] {
            let path = Reporter::new(&dirs, home).write_at(&panic, 5, 1).unwrap();
            assert!(fs::read_to_string(path).unwrap().contains("at /x"));
        }
    }

    #[test]
    fn report_names_version_system_mode_thread_location_message_backtrace() {
        let text = report_text(
            &panic_of("index out of range", "src/a.rs:3:9"),
            1_763_719_200_000,
        );
        for part in [
            format!("Version: {}", env!("CARGO_PKG_VERSION")),
            format!(
                "System: {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            "Mode: app".to_owned(),
            "Thread: main".to_owned(),
            "Location: src/a.rs:3:9".to_owned(),
            "Time: 2025-11-21T10:00:00Z".to_owned(),
            "index out of range".to_owned(),
            "1: std::rt::lang_start".to_owned(),
        ] {
            assert!(text.contains(&part), "{part} missing in:\n{text}");
        }
        let unnamed = Panic {
            thread: None,
            location: None,
            ..panic_of("x", "")
        };
        let text = report_text(&unnamed, 0);
        assert!(
            text.contains("Thread: unnamed") && text.contains("Location: unknown"),
            "{text}"
        );
    }

    #[test]
    fn a_long_message_is_cut_on_a_character() {
        let long = "é".repeat(MESSAGE_CAP);
        let text = report_text(&panic_of(&long, "a.rs:1:1"), 0);
        assert!(text.len() < MESSAGE_CAP + 1024, "{}", text.len());
        assert!(text.contains("é…"));
    }

    #[test]
    fn writes_a_new_file_per_panic_in_the_profile_and_never_over_one() {
        let (base, dirs) = fx();
        let home = base.path().join("home");
        let reporter = Reporter::new(&dirs, Some(&home));
        let panic = panic_of("boom", &format!("{}/src/a.rs:1:1", home.display()));
        let first = reporter.write_at(&panic, 1000, 42).unwrap();
        assert_eq!(first, folder(&dirs).join("1000-42.txt"));
        assert_eq!(folder(&dirs), dirs.profile.join("crash"));
        let text = fs::read_to_string(&first).unwrap();
        assert!(
            text.contains("Location: ~/src/a.rs:1:1") || text.contains(r"Location: ~\src/a.rs:1:1"),
            "{text}"
        );
        assert!(!text.contains(&home.display().to_string()), "{text}");
        // a second thread of the same process in the same millisecond
        let second = reporter
            .write_at(&panic_of("again", "b.rs:1:1"), 1000, 42)
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read_to_string(&first).unwrap(), text);
        assert!(fs::read_to_string(second).unwrap().contains("again"));
    }

    #[test]
    fn offers_the_newest_unseen_report_until_dismissed() {
        let (_base, dirs) = fx();
        let store = StateStore::app(&dirs);
        assert_eq!(offer(&store), None);
        let reporter = Reporter::new(&dirs, None);
        reporter
            .write_at(&panic_of("old", "a.rs:1:1"), 1000, 1)
            .unwrap();
        reporter
            .write_at(&panic_of("new", "a.rs:1:1"), 2000, 1)
            .unwrap();
        let report = offer(&store).unwrap();
        assert_eq!(report.file, "2000-1.txt");
        assert!(report.text.contains("new"));
        assert_eq!(report.version(), Some(env!("CARGO_PKG_VERSION")));
        let system = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
        assert_eq!(report.system(), Some(system.as_str()));
        dismiss(&store, &report).unwrap();
        assert_eq!(offer(&store), None);
        // the next crash is offered again
        reporter
            .write_at(&panic_of("later", "a.rs:1:1"), 3000, 1)
            .unwrap();
        assert_eq!(offer(&store).unwrap().file, "3000-1.txt");
    }

    #[test]
    fn the_next_start_keeps_the_newest_reports_only() {
        let (_base, dirs) = fx();
        let reporter = Reporter::new(&dirs, None);
        for at in 1..=(KEEP as u64 + 3) {
            reporter
                .write_at(&panic_of("x", "a.rs:1:1"), at * 1000, 7)
                .unwrap();
        }
        let foreign = folder(&dirs).join("notes.txt");
        fs::write(&foreign, "mine").unwrap();
        let report = offer(&StateStore::app(&dirs)).unwrap();
        assert_eq!(report.file, format!("{}-7.txt", (KEEP as u64 + 3) * 1000));
        let mut left: Vec<_> = fs::read_dir(folder(&dirs))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left.len(), KEEP + 1, "{left:?}");
        assert!(foreign.exists());
        for gone in 1..=3_u64 {
            assert!(
                !left.contains(&format!("{}-7.txt", gone * 1000)),
                "{left:?}"
            );
        }
    }
}
