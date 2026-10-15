//! Text that a crash left behind. A save writes the new text to a hidden temporary file (`.<note>.<pid>.<n>.part`) and
//! renames it over the note. If the program dies in between, that file may hold text newer than the note. When the
//! notes are opened, such a file whose program is gone is kept as a note of its own, "{title} (recovered)". The
//! temporary file itself is never deleted: it is renamed so that it is not found again.

use std::path::Path;

/// The note a temporary file was going to replace, and the process that was writing it, from the file's name
/// (`.<note>.<pid>.<n>.part`).
pub(crate) fn parse_temp_name(name: &str) -> Option<(&str, u32)> {
    let inner = name.strip_prefix('.')?.strip_suffix(".part")?;
    let (rest, counter) = inner.rsplit_once('.')?;
    let (note, pid) = rest.rsplit_once('.')?;
    counter.parse::<u64>().ok()?;
    let pid = pid.parse::<u32>().ok()?;
    (!note.is_empty()).then_some((note, pid))
}

/// The name a temporary file gets once it has been recovered, so it is not recovered again. The file stays.
pub(crate) fn recovered_name(temp_name: &str) -> String {
    format!("{}.recovered", temp_name.trim_end_matches(".part"))
}

/// Whether a process with this id is running now.
#[cfg(unix)]
pub(crate) fn process_is_running(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return false;
    };
    match rustix::process::test_kill_process(pid) {
        Ok(()) => true,
        // the process exists but is not ours to signal
        Err(error) => error == rustix::io::Errno::PERM,
    }
}

/// Whether a process with this id is running now.
#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) fn process_is_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: opening a process by id has no preconditions; a null handle is the failure (the process is gone, or it is
    // not ours to look at, which for a program of this user means it is gone).
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return false;
    }
    let mut code = 0_u32;
    // SAFETY: the handle is valid and `code` is a place for the exit code.
    let asked = unsafe { GetExitCodeProcess(process, &raw mut code) };
    // SAFETY: the handle was opened here and is closed once.
    unsafe { CloseHandle(process) };
    asked != 0 && i64::from(code) == i64::from(STILL_ACTIVE)
}

/// What a leftover temporary file holds, when it is worth keeping: the text, if it is not blank and is not what the
/// note holds already.
pub(crate) fn recoverable_text(temp: &Path, note: &Path) -> Option<String> {
    let bytes = std::fs::read(temp).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    if super::text::is_blank(&text) {
        return None;
    }
    if std::fs::read(note).is_ok_and(|current| current == text.as_bytes()) {
        return None;
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_temporary_file_names_its_note_and_the_process_that_was_writing() {
        assert_eq!(
            parse_temp_name(".2026-10-15 1432.md.4242.7.part"),
            Some(("2026-10-15 1432.md", 4242))
        );
        assert_eq!(parse_temp_name(".a.md.1.0.part"), Some(("a.md", 1)));
        for not_temp in [
            "a.md",
            ".a.md",
            ".a.md.part",
            ".a.md.x.7.part",
            ".a.md.1.x.part",
            "..1.2.part",
            "download.part",
            ".a.md.1.2.recovered",
        ] {
            assert_eq!(parse_temp_name(not_temp), None, "{not_temp}");
        }
    }

    #[test]
    fn a_recovered_file_is_renamed_so_it_is_not_found_again() {
        let renamed = recovered_name(".a.md.4242.7.part");
        assert_eq!(renamed, ".a.md.4242.7.recovered");
        assert_eq!(parse_temp_name(&renamed), None);
    }

    #[test]
    fn this_process_is_running_and_a_process_that_ended_is_not() {
        assert!(process_is_running(std::process::id()));
        let mut child = if cfg!(windows) {
            std::process::Command::new("cmd")
                .args(["/C", "exit 0"])
                .spawn()
        } else {
            std::process::Command::new("true").spawn()
        }
        .expect("start a process");
        let pid = child.id();
        child.wait().expect("it ends");
        assert!(!process_is_running(pid), "process {pid} has ended");
    }

    #[test]
    fn only_new_text_is_worth_recovering() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (temp, note) = (dir.path().join(".a.md.1.1.part"), dir.path().join("a.md"));
        std::fs::write(&note, "saved").expect("write");
        std::fs::write(&temp, "newer text").expect("write");
        assert_eq!(
            recoverable_text(&temp, &note).as_deref(),
            Some("newer text")
        );
        std::fs::write(&temp, "saved").expect("write");
        assert_eq!(
            recoverable_text(&temp, &note),
            None,
            "the note holds it already"
        );
        std::fs::write(&temp, "  \n").expect("write");
        assert_eq!(recoverable_text(&temp, &note), None, "blank");
        std::fs::write(&temp, [0xff, 0xfe, 0x00]).expect("write");
        assert_eq!(recoverable_text(&temp, &note), None, "not text");
        std::fs::remove_file(&note).expect("remove");
        std::fs::write(&temp, "no note at all").expect("write");
        assert_eq!(
            recoverable_text(&temp, &note).as_deref(),
            Some("no note at all")
        );
    }
}
