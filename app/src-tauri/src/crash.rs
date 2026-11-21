//! Crash reports and failed starts of this process: the panic hook (set first thing in `main`), the message a start
//! that failed before any window leaves, and the next start's offer of the last report: copy it, or open a GitHub issue
//! prefilled with it in the browser. The person sends it; the app never sends anything.

use std::sync::OnceLock;

use permark_core::crash::{CrashReport, Reporter};
use permark_core::open::Cli;
use serde::Serialize;
use ts_rs::TS;

use crate::commands::web_url_allowed;
use crate::StartError;

/// How this process runs, for the report. The hook only reads it (no waiting); unset = the command line not read yet.
static MODE: OnceLock<&'static str> = OnceLock::new();

/// The last report the person has not seen, as the first window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CrashOffer {
    /// The whole report, the home folder as `~` (what Copy report puts on the clipboard).
    pub report: String,
    /// A new GitHub issue prefilled with the report (its head when the whole does not fit a link); opened in the browser,
    /// where the person reviews and submits it.
    pub issue_url: String,
}

/// Sets the panic hook: each panic leaves a report in the profile, then the hook set before runs (the message on
/// standard error). No profile folder on this system: the hook set before, alone.
pub fn install() {
    let Some(reporter) = Reporter::system() else {
        return;
    };
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // the report first: whatever the earlier hook does, it is on disk
        reporter.record(info, MODE.get().copied().unwrap_or("starting"));
        before(info);
    }));
}

/// The command line was read: reports name the mode this process runs in.
pub fn running_as(cli: &Cli) {
    let mode = match cli {
        Cli::Open(_) => "app",
        Cli::Mcp { .. } => "mcp",
        Cli::Context => "context",
        Cli::Help | Cli::Version | Cli::Usage(_) => "command line",
    };
    let _ = MODE.set(mode);
}

/// Debug builds only: `PERMARK_DEBUG_PANIC` set -> a panic on the main thread before the app starts, to try the report
/// and the next start's offer. A release build has no such switch.
///
/// # Panics
/// When `PERMARK_DEBUG_PANIC` is set: the point of it.
#[cfg(debug_assertions)]
pub fn panic_if_asked() {
    assert!(
        std::env::var_os("PERMARK_DEBUG_PANIC").is_none(),
        "forced by PERMARK_DEBUG_PANIC (debug builds only)"
    );
}

/// Removes all but the newest crash reports of the profile: for the agent connection modes, which never offer one.
pub fn trim_reports() {
    if let Some(dirs) = permark_core::state::AppDirs::system() {
        permark_core::crash::trim(&dirs);
    }
}

/// Tells the person PerMark could not start (no window exists yet): standard error, plus a system message box on
/// Windows, where a release build has no console to show it.
pub fn start_failed(error: &StartError) {
    let text = start_failed_text(error);
    eprintln!("{text}");
    #[cfg(windows)]
    native::message_box("PerMark", &text);
}

/// The failed start's text, naming the step that failed (`error` says it).
fn start_failed_text(error: &StartError) -> String {
    format!("PerMark could not start: {error}")
}

/// The offer of `report`.
pub(crate) fn offer_of(report: &CrashReport) -> CrashOffer {
    CrashOffer {
        issue_url: issue_url(
            &report.text,
            report.version().unwrap_or("unknown"),
            report.system().unwrap_or("unknown"),
        ),
        report: report.text.clone(),
    }
}

/// The parts of a report's `text` a link may carry: the header lines (version, system, mode, thread, location) and
/// the backtrace. Never the message: it may quote document text or paths, and a link leaves the computer before the
/// person reviews it (query string, sign-in return address, browser history). A text not shaped like a report gives
/// nothing.
fn link_parts(text: &str) -> (&str, &str) {
    let Some((header, rest)) = text.split_once("\n\nMessage:\n") else {
        return ("", "");
    };
    // the last one: a message may hold the same words
    let backtrace = rest
        .rsplit_once("\n\nBacktrace:\n")
        .map_or("", |(_, backtrace)| backtrace);
    (header, backtrace.trim_end())
}

/// A new issue of the app's repository, titled with `version` and `system`, its body the report `text` without its
/// message ([`link_parts`]), the backtrace cut to the longest head that keeps the link one the browser may be given
/// ([`web_url_allowed`]).
fn issue_url(text: &str, version: &str, system: &str) -> String {
    let repo = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    let Ok(new_issue) = tauri::Url::parse(&format!("{repo}/issues/new")) else {
        return repo.to_owned();
    };
    let title = format!("Crash report: PerMark {version} on {system}");
    let (header, backtrace) = link_parts(text);
    let link = |head: &str, trimmed: bool| {
        let mut url = new_issue.clone();
        url.query_pairs_mut()
            .append_pair("title", &title)
            .append_pair("body", &issue_body(header, head, trimmed));
        String::from(url)
    };
    let whole = link(backtrace, false);
    if web_url_allowed(&whole) {
        return whole;
    }
    // the longest head that fits, found by halving (a longer head never makes the link shorter)
    let ends: Vec<usize> = backtrace.char_indices().map(|(at, _)| at).collect();
    let head = |k: usize| {
        backtrace
            .get(..ends.get(k).copied().unwrap_or(0))
            .unwrap_or_default()
    };
    let (mut fits, mut too_long) = (0, ends.len());
    while too_long - fits > 1 {
        let mid = fits + (too_long - fits) / 2;
        if web_url_allowed(&link(head(mid), true)) {
            fits = mid;
        } else {
            too_long = mid;
        }
    }
    link(head(fits), true)
}

/// The issue's text: the report's `header` and backtrace (or its `head`) as a block, then where to find the message
/// left out.
fn issue_body(header: &str, head: &str, trimmed: bool) -> String {
    let more = if trimmed {
        "\n… (trimmed to fit a link: Copy report in PerMark gives the whole text)"
    } else {
        ""
    };
    format!(
        "PerMark closed unexpectedly. The report it left, without its message (the home folder shows as ~):\n\n\
         ````text\n{header}\n\nBacktrace:\n{head}{more}\n````\n\n\
         The report's message is left out of this link because it can quote document text or file paths. Please use \
         Copy report in PerMark, check that the message holds nothing private, and paste it here.\n"
    )
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND,
    };

    /// A system message box with an OK button; returns once the person closed it.
    pub(super) fn message_box(title: &str, text: &str) {
        // SAFETY: both strings are owned for the whole call; no owner window (none exists yet); the call only blocks
        // this thread until the box is closed.
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(text),
                &HSTRING::from(title),
                MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::*;

    const REPO: &str = env!("CARGO_PKG_REPOSITORY");

    const HEAD: &str = "PerMark crash report\nVersion: 9.8.7\nSystem: windows x86_64\nMode: app\n\
                        Location: ~/src/a.rs:3:9";

    fn report(lines: usize) -> String {
        report_with("boom é ü", lines)
    }

    fn report_with(message: &str, lines: usize) -> String {
        let mut text = format!("{HEAD}\n\nMessage:\n{message}\n\nBacktrace:\n");
        for n in 0..lines {
            writeln!(
                text,
                "  {n}: permark_core::something::deep at ~/src/x.rs:{n}"
            )
            .unwrap();
        }
        text
    }

    fn query(url: &str) -> Vec<(String, String)> {
        tauri::Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    fn body_of(url: &str) -> String {
        query(url).into_iter().find(|(k, _)| k == "body").unwrap().1
    }

    #[test]
    fn a_short_report_goes_into_a_new_issue_without_its_message() {
        let text = report(3);
        let url = issue_url(&text, "9.8.7", "windows x86_64");
        assert!(url.starts_with(&format!("{REPO}/issues/new?")), "{url}");
        assert!(web_url_allowed(&url), "{url}");
        let pairs = query(&url);
        let title = &pairs.iter().find(|(k, _)| k == "title").unwrap().1;
        assert!(
            title.contains("9.8.7") && title.contains("windows x86_64"),
            "{title}"
        );
        let body = body_of(&url);
        assert!(body.starts_with("PerMark closed unexpectedly"), "{body}");
        assert!(body.contains(HEAD), "{body}");
        assert!(body.contains("2: permark_core::something::deep"), "{body}");
        assert!(body.contains("Copy report"), "{body}");
        assert!(!body.contains("boom"), "{body}");
        assert!(!body.contains("trimmed"), "{body}");
    }

    /// A panic message may quote document text or paths; the link leaves the computer before the person reviews it
    /// (query string, sign-in return address, browser history).
    #[test]
    fn the_issue_link_never_carries_the_reports_message() {
        let message =
            "secret words of my-notes in /home/ann/hidden-plans/q3.md\n\nBacktrace:\nalso secret";
        for text in [report_with(message, 3), report_with(message, 2000)] {
            let url = issue_url(&text, "9.8.7", "windows x86_64");
            let body = body_of(&url);
            for part in ["secret", "my-notes", "hidden-plans", "q3.md"] {
                assert!(!body.contains(part), "{part} in {body}");
                assert!(!url.contains(part), "{part} in {url}");
            }
            assert!(body.contains("Location: ~/src/a.rs:3:9"), "{body}");
        }
        // a text not shaped like a report gives nothing of itself
        let url = issue_url("secret text", "9.8.7", "windows x86_64");
        assert!(!body_of(&url).contains("secret"), "{url}");
    }

    #[test]
    fn a_long_report_keeps_its_backtrace_head_and_the_link_stays_one_the_browser_may_get() {
        let text = report(2000);
        let url = issue_url(&text, "9.8.7", "windows x86_64");
        assert!(web_url_allowed(&url), "{} bytes", url.len());
        assert!(
            url.len() > 3000,
            "the link uses the room it has: {}",
            url.len()
        );
        let body = body_of(&url);
        assert!(body.contains(HEAD), "{body}");
        assert!(
            body.contains("0: permark_core") && body.contains("trimmed"),
            "{body}"
        );
        assert!(!body.contains("1999:"), "{body}");
    }

    #[test]
    fn a_failed_start_names_the_step_that_failed() {
        let profile = start_failed_text(&StartError::NoProfile);
        assert!(profile.starts_with("PerMark could not start"), "{profile}");
        assert!(
            profile.contains("finding the folder for its settings"),
            "{profile}"
        );
        let window = start_failed_text(&StartError::Tauri(tauri::Error::FailedToReceiveMessage));
        assert!(window.contains("setting up the app window"), "{window}");
    }
}
