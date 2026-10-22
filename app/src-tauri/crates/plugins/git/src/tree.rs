//! The installed git's whole process tree on Windows: Git for Windows' `git.exe` (in `cmd\` and `bin\`) is a launcher
//! that starts the real git as its child, and git starts shells | helpers of its own. Killing the launcher alone left
//! them running with the folder as working directory -> the person's rename | delete failed (OS error 32) until they
//! ended. The child starts suspended, joins a job object, then runs; stopping it ends the job (every process in it)
//! and waits until none is left. Elsewhere the child is killed as is (the real git, no launcher).
//!
//! The only unsafe code of this crate besides libgit2's: plain Win32 calls on handles this module owns.
#![allow(unsafe_code)]

use std::os::windows::io::AsRawHandle;
use std::process::Child;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    QueryInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
};
use windows_sys::Win32::System::Threading::{
    OpenThread, ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
};

/// Creation flag the child MUST be spawned with; [`Tree::adopt`] lets it run.
pub const START_SUSPENDED: u32 = CREATE_SUSPENDED;

/// Most time a stopped tree may take to be gone.
const GONE_WAIT: Duration = Duration::from_secs(2);

/// A job holding the child and everything it starts. No job (the system refused one) -> the child alone.
pub struct Tree(Option<HANDLE>);

impl Tree {
    /// Puts the suspended `child` in a new job, then lets it run (also when there is no job).
    ///
    /// # Errors
    /// The child could not be resumed: it is killed (it would never run).
    pub fn adopt(child: &mut Child) -> std::io::Result<Self> {
        // SAFETY: no attributes, no name: a new unnamed job; null on failure.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        let job = (!job.is_null()).then_some(job).filter(|&job| {
            // SAFETY: `job` is the handle made above; the child's process handle stays open while `child` lives.
            let joined = unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) } != 0;
            if !joined {
                // SAFETY: `job` is ours, closed once (not kept).
                unsafe { CloseHandle(job) };
            }
            joined
        });
        let tree = Self(job);
        if resume(child.id()) {
            Ok(tree)
        } else {
            let error = std::io::Error::last_os_error();
            tree.kill(child);
            Err(error)
        }
    }

    /// Ends every process of the tree and waits until none is left (bounded), then reaps the child.
    pub fn kill(&self, child: &mut Child) {
        if let Some(job) = self.0 {
            // SAFETY: `job` is open until drop.
            if unsafe { TerminateJobObject(job, 1) } != 0 {
                let until = Instant::now() + GONE_WAIT;
                while active(job) > 0 && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Resumes the threads of the suspended process `pid` (a new process: its main thread only). Whether one ran.
fn resume(pid: u32) -> bool {
    // SAFETY: a snapshot of the system's threads; INVALID_HANDLE_VALUE (-1) on failure, closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot.is_null() || snapshot as isize == -1 {
        return false;
    }
    let mut resumed = false;
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>()).unwrap_or(0),
        ..THREADENTRY32::default()
    };
    // SAFETY: `entry` is a writable THREADENTRY32 with its size set; `snapshot` is open.
    let mut more = unsafe { Thread32First(snapshot, &raw mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: opens the thread by id for resuming only; null on failure, closed below.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !thread.is_null() {
                // SAFETY: `thread` is open; ResumeThread returns the previous suspend count, u32::MAX on failure.
                resumed |= unsafe { ResumeThread(thread) } != u32::MAX;
                // SAFETY: `thread` is ours, closed once.
                unsafe { CloseHandle(thread) };
            }
        }
        // SAFETY: as Thread32First.
        more = unsafe { Thread32Next(snapshot, &raw mut entry) } != 0;
    }
    // SAFETY: `snapshot` is ours, closed once.
    unsafe { CloseHandle(snapshot) };
    resumed
}

/// Processes still in the job (0 when it cannot be read).
fn active(job: HANDLE) -> u32 {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    let size =
        u32::try_from(std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>()).unwrap_or(0);
    // SAFETY: `info` is a writable struct of exactly `size` bytes for this information class; `job` is open.
    let ok = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            (&raw mut info).cast(),
            size,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        0
    } else {
        info.ActiveProcesses
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if let Some(job) = self.0.take() {
            // SAFETY: `job` is ours, closed once. No kill-on-close: a finished call leaves nothing to end.
            unsafe { CloseHandle(job) };
        }
    }
}
