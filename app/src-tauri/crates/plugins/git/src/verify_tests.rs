//! Verifier adversarial tests (job-object tree, 2026-10-22). Windows only. `nested_*` + `perf_*` are `#[ignore]`:
//! run one per process (`--exact --ignored`), they put the whole test process in a job.
#![allow(unsafe_code)]
#![allow(clippy::unwrap_used)] // test helpers (not `#[test]` fns) inside a `cfg(all(test, windows))` module

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use git2::Repository;

use crate::ops::GitError;
use crate::system::{find_git, run, run_until};

struct Hold {
    _dir: tempfile::TempDir,
    base: PathBuf,
    folder: PathBuf,
    marker: PathBuf,
}

fn hold(name: &str) -> Hold {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    let folder = base.join(name);
    std::fs::create_dir_all(&folder).unwrap();
    Repository::init(&folder).unwrap();
    let marker = base.join("started.txt");
    Hold {
        _dir: dir,
        base,
        folder,
        marker,
    }
}

fn alias_sleep(marker: &Path) -> String {
    let m = marker.to_string_lossy().replace('\\', "/");
    format!("alias.hold=!echo started > \"{m}\"; sleep 30")
}

fn wait_marker(marker: &Path) {
    let started = Instant::now();
    while !marker.exists() {
        assert!(started.elapsed() < Duration::from_secs(30), "never started");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Rename at once; on failure retry for up to 1 s and report how long it took (None = at once).
fn rename_now(from: &Path, to: &Path) -> Result<Option<Duration>, std::io::Error> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(None),
        Err(first) => {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(1));
                if std::fs::rename(from, to).is_ok() {
                    eprintln!(
                        "rename failed at once ({first}), ok after {:?}",
                        start.elapsed()
                    );
                    return Ok(Some(start.elapsed()));
                }
            }
            Err(first)
        }
    }
}

fn cancel_once(exe: &Path, name: &str) -> Result<Option<Duration>, std::io::Error> {
    let h = hold(name);
    let alias = alias_sleep(&h.marker);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker = {
        let (exe, folder, cancel) = (exe.to_path_buf(), h.folder.clone(), Arc::clone(&cancel));
        std::thread::spawn(move || {
            run_until(
                &exe,
                &folder,
                &[OsStr::new("-c"), OsStr::new(&alias), OsStr::new("hold")],
                Duration::from_secs(60),
                1024,
                Some(&cancel),
            )
        })
    };
    wait_marker(&h.marker);
    cancel.store(true, Ordering::SeqCst);
    assert_eq!(
        worker.join().unwrap().unwrap_err(),
        GitError::Failed("stopped".to_owned())
    );
    rename_now(&h.folder, &h.base.join("renamed é x"))
}

#[test]
fn v_timeout_kills_the_whole_tree() {
    let Some(exe) = find_git() else { return };
    let h = hold("time out ü");
    let alias = alias_sleep(&h.marker);
    let started = Instant::now();
    let result = run_until(
        &exe,
        &h.folder,
        &[OsStr::new("-c"), OsStr::new(&alias), OsStr::new("hold")],
        Duration::from_secs(3),
        1024,
        None,
    );
    assert_eq!(result.unwrap_err(), GitError::Timeout);
    assert!(
        h.marker.exists(),
        "grandchild never ran: test proves nothing"
    );
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(rename_now(&h.folder, &h.base.join("moved")).unwrap(), None);
}

#[test]
fn v_overflow_kills_the_whole_tree() {
    let Some(exe) = find_git() else { return };
    let h = hold("flood ü");
    // `yes` (grandchild) floods the pipe; cap 4 KiB
    let alias = "alias.flood=!yes permark-flood-line";
    let result = run_until(
        &exe,
        &h.folder,
        &[OsStr::new("-c"), OsStr::new(alias), OsStr::new("flood")],
        Duration::from_secs(20),
        4096,
        None,
    );
    assert_eq!(result.unwrap_err(), GitError::TooLarge(4096));
    assert_eq!(rename_now(&h.folder, &h.base.join("moved")).unwrap(), None);
}

/// Race hypothesis: job "empty" before the cwd handle is closed -> rename at once fails sometimes.
#[test]
#[ignore = "verifier loop; run alone"]
fn v_loop_cancel_rename_50() {
    let Some(exe) = find_git() else { return };
    let mut late = Vec::new();
    for i in 0..50 {
        match cancel_once(&exe, &format!("loop {i} ü")) {
            Ok(None) => {}
            Ok(Some(d)) => late.push((i, d)),
            Err(e) => panic!("run {i}: rename never worked: {e}"),
        }
    }
    eprintln!("late renames: {} / 50: {late:?}", late.len());
    // A rename can need a few ms after the job is empty (Windows closes handles after the count drops); the app
    // retries renames, so up to 200 ms is fine. Longer = a real hold.
    assert!(
        late.iter()
            .all(|(_, d)| *d < std::time::Duration::from_millis(200)),
        "{late:?}"
    );
}

#[test]
fn v_concurrent_cancels_unicode_space() {
    let Some(exe) = find_git() else { return };
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let exe = exe.clone();
            std::thread::spawn(move || cancel_once(&exe, &format!("pär {i} ö")))
        })
        .collect();
    for h in handles {
        let late = h.join().unwrap().unwrap();
        assert!(
            late.is_none_or(|d| d < std::time::Duration::from_millis(200)),
            "{late:?}"
        );
    }
}

#[test]
fn v_normal_calls_still_answer_and_leave_folder_free() {
    let Some(exe) = find_git() else { return };
    let h = hold("plain ü");
    for _ in 0..20 {
        let out = run(
            &exe,
            &h.folder,
            &[OsStr::new("status"), OsStr::new("--porcelain=v2")],
            Duration::from_secs(10),
            1 << 20,
        )
        .unwrap();
        assert_eq!(out.code, Some(0), "{}", out.stderr);
    }
    assert_eq!(rename_now(&h.folder, &h.base.join("moved")).unwrap(), None);
}

// ---- the whole test process inside a job (PerMark under MSIX | CI | a terminal) ----------------------------------

mod job {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicUIRestrictions,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_UILIMIT_HANDLES,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    pub fn enter(kill_on_close: bool, ui_limit: bool) -> HANDLE {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            assert!(!job.is_null());
            if kill_on_close {
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&raw const info).cast(),
                    u32::try_from(std::mem::size_of_val(&info)).unwrap(),
                );
                assert_ne!(ok, 0);
            }
            if ui_limit {
                let info = JOBOBJECT_BASIC_UI_RESTRICTIONS {
                    UIRestrictionsClass: JOB_OBJECT_UILIMIT_HANDLES,
                };
                let ok = SetInformationJobObject(
                    job,
                    JobObjectBasicUIRestrictions,
                    (&raw const info).cast(),
                    u32::try_from(std::mem::size_of_val(&info)).unwrap(),
                );
                assert_ne!(ok, 0);
            }
            let ok = AssignProcessToJobObject(job, GetCurrentProcess());
            assert_ne!(
                ok,
                0,
                "test process could not join job: {}",
                std::io::Error::last_os_error()
            );
            job
        }
    }
}

#[test]
#[ignore = "puts the test process in a job; run alone"]
fn nested_kill_on_close_parent_job() {
    let Some(exe) = find_git() else { return };
    let _job = job::enter(true, false);
    assert_eq!(cancel_once(&exe, "nested ü").unwrap(), None);
}

#[test]
#[ignore = "puts the test process in a job; run alone"]
fn nested_ui_limited_parent_job_falls_back() {
    let Some(exe) = find_git() else { return };
    let _job = job::enter(false, true);
    // the call must still answer (job refused -> child alone)
    let h = hold("ui ü");
    let out = run(
        &exe,
        &h.folder,
        &[OsStr::new("--version")],
        Duration::from_secs(10),
        4096,
    )
    .unwrap();
    assert_eq!(out.code, Some(0));
    // whether the tree is stopped too (job nested) or not (fallback): reported, not asserted
    eprintln!(
        "ui-limited parent: cancel+rename -> {:?}",
        cancel_once(&exe, "ui2 ü")
    );
}

#[test]
#[ignore = "perf; run alone in release"]
fn perf_version_calls() {
    let Some(exe) = find_git() else { return };
    let here = std::env::temp_dir();
    for _ in 0..5 {
        run(
            &exe,
            &here,
            &[OsStr::new("--version")],
            Duration::from_secs(10),
            4096,
        )
        .unwrap();
    }
    let n = 100_u32;
    let mut all = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let t = Instant::now();
        run(
            &exe,
            &here,
            &[OsStr::new("--version")],
            Duration::from_secs(10),
            4096,
        )
        .unwrap();
        all.push(t.elapsed());
    }
    all.sort();
    let mean = all.iter().sum::<Duration>() / n;
    eprintln!(
        "PERF git --version x{n}: mean {mean:?} median {:?} p90 {:?} min {:?}",
        all[50], all[90], all[0]
    );
}

/// Control for the loop: same folder setup, no git process at all (AV | indexer baseline).
#[test]
#[ignore = "verifier control; run alone"]
fn v_control_rename_no_git_50() {
    let mut late = Vec::new();
    for i in 0..50 {
        let h = hold(&format!("ctl {i} ü"));
        std::fs::write(&h.marker, b"started").unwrap();
        std::fs::write(h.folder.join("a.md"), b"x").unwrap();
        match rename_now(&h.folder, &h.base.join("renamed é x")) {
            Ok(None) => {}
            Ok(Some(d)) => late.push((i, d)),
            Err(e) => panic!("run {i}: {e}"),
        }
    }
    eprintln!("control late renames: {} / 50: {late:?}", late.len());
}

/// Perf split: plain `std::process` spawn + wait vs the job-list start (`tree::Proc`) + wait, same program.
#[test]
#[ignore = "perf; run alone in release"]
fn perf_split() {
    use std::os::windows::process::CommandExt;
    let Some(exe) = find_git() else { return };
    let here = std::env::temp_dir();
    let n = 50_u32;
    let t = Instant::now();
    for _ in 0..n {
        let mut c = std::process::Command::new(&exe);
        c.arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        c.creation_flags(0x0800_0000);
        c.spawn().unwrap().wait().unwrap();
    }
    eprintln!("PERF plain spawn+wait: {:?}", t.elapsed() / n);
    let t = Instant::now();
    for _ in 0..n {
        let mut p =
            crate::tree::Proc::start(&exe, &here, &[OsStr::new("--version")], &[], &[]).unwrap();
        while matches!(p.try_wait().unwrap(), crate::system::Wait::Running) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    eprintln!("PERF job-list start+wait: {:?}", t.elapsed() / n);
}
