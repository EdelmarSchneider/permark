//! The installed git started on Windows with its whole process tree in a job object. Git for Windows' `git.exe`
//! (in `cmd\` and `bin\`) is a launcher that starts the real git as its child, and git starts shells | helpers of its
//! own: killing the launcher alone left them running with the folder as working directory -> the person's rename |
//! delete failed (OS error 32) until they ended. `CreateProcessW` puts the child in a new job as it is created
//! (`PROC_THREAD_ATTRIBUTE_JOB_LIST`, Windows 10+): no suspended start, nothing to resume, no look at other
//! processes. Stopping it ends the job (every process in it) and waits until none is left. The system refuses the job
//! -> the child alone, killed as before. Only `exe` is ever started (absolute path, no shell, no search).
//!
//! The only unsafe code of this crate besides libgit2's: plain Win32 calls on handles and buffers this module owns.
#![allow(unsafe_code)]

use std::ffi::{c_void, OsStr, OsString};
use std::io::{self, PipeReader};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use crate::system::Wait;
use windows_sys::Win32::Foundation::{
    SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JobObjectBasicAccountingInformation, QueryInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTF_USESTDHANDLES,
    STARTUPINFOEXW,
};

/// Most time a stopped tree may take to be gone.
const GONE_WAIT: Duration = Duration::from_secs(2);
const GONE_WAIT_MS: u32 = 2000;

/// A started git: its output pipes, its process, the job holding it and everything it starts (`None`: refused).
pub struct Proc {
    pub stdout: Option<PipeReader>,
    pub stderr: Option<PipeReader>,
    process: OwnedHandle,
    job: Option<OwnedHandle>,
}

impl Proc {
    /// Starts `exe args` in `dir` with the environment of this process minus `remove`, plus `set` (names compared
    /// ignoring case, as Windows does); stdin = `NUL`, stdout + stderr = pipes; no console window.
    ///
    /// # Errors
    /// A NUL inside a value, a pipe | `NUL` that cannot be opened, the program cannot start.
    pub fn start(
        exe: &Path,
        dir: &Path,
        args: &[&OsStr],
        set: &[(&str, &str)],
        remove: &[&str],
    ) -> io::Result<Self> {
        let application = wide(exe.as_os_str())?;
        let current = wide(dir.as_os_str())?;
        let command = command_line(exe.as_os_str(), args)?;
        let block = env_block(set, remove)?;
        let (out_read, out_write) = io::pipe()?;
        let (err_read, err_write) = io::pipe()?;
        let nul = std::fs::OpenOptions::new().read(true).open("NUL")?;
        // only these three reach the child (handle list); they are inheritable for the call
        let inherited: [HANDLE; 3] = [
            nul.as_raw_handle(),
            out_write.as_raw_handle(),
            err_write.as_raw_handle(),
        ];
        for handle in inherited {
            // SAFETY: `handle` belongs to an open local above (`nul`, a pipe's write end).
            if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) }
                == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let parts = Parts {
            application: &application,
            command: &command,
            current: &current,
            block: &block,
            inherited: &inherited,
        };
        let job = new_job();
        let (process, job) = match job {
            Some(job) => match create(&parts, Some(job.as_raw_handle())) {
                Ok(process) => (process, Some(job)),
                // a job the system will not nest here: the child alone (killed as before)
                Err(_) => (create(&parts, None)?, None),
            },
            None => (create(&parts, None)?, None),
        };
        // our copies of the child's ends go: the pipes end when the child (and what it started) closes them
        drop((nul, out_write, err_write));
        Ok(Self {
            stdout: Some(out_read),
            stderr: Some(err_read),
            process,
            job,
        })
    }

    /// Whether it still runs, else its exit code.
    ///
    /// # Errors
    /// The process cannot be asked.
    pub fn try_wait(&mut self) -> io::Result<Wait> {
        let process = self.process.as_raw_handle();
        // SAFETY: `process` is open while `self` lives; 0 ms = a look, no wait.
        match unsafe { WaitForSingleObject(process, 0) } {
            WAIT_OBJECT_0 => {
                let mut code = 0_u32;
                // SAFETY: `process` is open; `code` is a writable u32.
                if unsafe { GetExitCodeProcess(process, &raw mut code) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Wait::Ended(Some(i32::from_ne_bytes(code.to_ne_bytes()))))
            }
            WAIT_TIMEOUT => Ok(Wait::Running),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// Ends every process of the tree and waits until none is left (bounded), then the child itself.
    pub fn kill(&mut self) {
        if let Some(job) = &self.job {
            let job = job.as_raw_handle();
            // SAFETY: `job` is open while `self` lives.
            if unsafe { TerminateJobObject(job, 1) } != 0 {
                let until = Instant::now() + GONE_WAIT;
                while active(job) > 0 && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
        let process = self.process.as_raw_handle();
        // SAFETY: `process` is open while `self` lives (ending an ended process fails harmlessly).
        unsafe { TerminateProcess(process, 1) };
        // SAFETY: as above; bounded wait.
        unsafe { WaitForSingleObject(process, GONE_WAIT_MS) };
    }
}

/// What one `CreateProcessW` call needs (wide, NUL-terminated strings; the handles the child inherits).
struct Parts<'a> {
    application: &'a [u16],
    command: &'a [u16],
    current: &'a [u16],
    block: &'a [u16],
    inherited: &'a [HANDLE; 3],
}

/// A new unnamed job (`None`: refused).
fn new_job() -> Option<OwnedHandle> {
    // SAFETY: no attributes, no name: a new unnamed job; null on failure.
    let job = unsafe { CreateJobObjectW(null(), null()) };
    // SAFETY: a fresh handle, owned from here on (closed once, on drop).
    (!job.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(job) })
}

/// The process of one start, inside `job` when given. No kill-on-close: a finished call keeps the 0.6.4 behavior (a
/// helper that git left running on its own, if any, is not ended).
fn create(parts: &Parts<'_>, job: Option<HANDLE>) -> io::Result<OwnedHandle> {
    let jobs: [HANDLE; 1] = [job.unwrap_or(null_mut())];
    let mut list = AttributeList::new(if job.is_some() { 2 } else { 1 })?;
    list.set(
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        parts.inherited.as_ptr().cast(),
        std::mem::size_of_val(parts.inherited),
    )?;
    if job.is_some() {
        list.set(
            PROC_THREAD_ATTRIBUTE_JOB_LIST,
            jobs.as_ptr().cast(),
            std::mem::size_of_val(&jobs),
        )?;
    }
    let mut info = STARTUPINFOEXW::default();
    info.StartupInfo.cb = u32::try_from(std::mem::size_of::<STARTUPINFOEXW>()).unwrap_or(0);
    info.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    info.StartupInfo.hStdInput = parts.inherited[0];
    info.StartupInfo.hStdOutput = parts.inherited[1];
    info.StartupInfo.hStdError = parts.inherited[2];
    info.lpAttributeList = list.as_ptr();
    // CreateProcessW may write into the command line: its own copy
    let mut command = parts.command.to_vec();
    let mut created = PROCESS_INFORMATION::default();
    let flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT;
    // SAFETY: every string is NUL-terminated wide text owned by the caller; the environment block ends with two NULs;
    // `info` (with its attribute list, the handle + job arrays it points to) lives until the call returns; the
    // inherited handles are open and inheritable; `created` is writable.
    let ok = unsafe {
        CreateProcessW(
            parts.application.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            1,
            flags,
            parts.block.as_ptr().cast(),
            parts.current.as_ptr(),
            (&raw const info).cast(),
            &raw mut created,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both handles are new and ours (closed once, on drop); the thread's is not needed.
    let (process, _thread) = unsafe {
        (
            OwnedHandle::from_raw_handle(created.hProcess),
            OwnedHandle::from_raw_handle(created.hThread),
        )
    };
    Ok(process)
}

/// A `PROC_THREAD_ATTRIBUTE_LIST` in a pointer-aligned buffer; deleted on drop.
struct AttributeList {
    buffer: Vec<usize>,
}

impl AttributeList {
    fn new(count: u32) -> io::Result<Self> {
        let mut size = 0_usize;
        // SAFETY: a null list only asks for the size (the call fails, `size` is set).
        unsafe { InitializeProcThreadAttributeList(null_mut(), count, 0, &raw mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0_usize; size.div_ceil(std::mem::size_of::<usize>())];
        // SAFETY: `buffer` holds at least `size` bytes, aligned for pointers.
        let ok = unsafe {
            InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), count, 0, &raw mut size)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { buffer })
    }

    fn as_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buffer.as_mut_ptr().cast()
    }

    /// `value` (`size` bytes) MUST live until the process is created: the list keeps the pointer.
    fn set(&mut self, attribute: u32, value: *const c_void, size: usize) -> io::Result<()> {
        let attribute = usize::try_from(attribute).map_err(|_| io::ErrorKind::InvalidInput)?;
        // SAFETY: the list is initialized; `value` points to `size` readable bytes (caller keeps them alive).
        let ok = unsafe {
            UpdateProcThreadAttribute(self.as_ptr(), 0, attribute, value, size, null_mut(), null())
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: built only after a successful initialization; deleted once.
        unsafe { DeleteProcThreadAttributeList(self.as_ptr()) };
    }
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
            null_mut(),
        )
    };
    if ok == 0 {
        0
    } else {
        info.ActiveProcesses
    }
}

fn nul_inside() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "a NUL inside a value")
}

/// `text` as NUL-terminated wide text.
fn wide(text: &OsStr) -> io::Result<Vec<u16>> {
    let mut out: Vec<u16> = text.encode_wide().collect();
    if out.contains(&0) {
        return Err(nul_inside());
    }
    out.push(0);
    Ok(out)
}

/// The command line as the C runtime splits it again (the rules `std::process` uses): the program always quoted;
/// an argument with a space | tab, or empty, quoted; backslashes doubled before a quote; quotes escaped.
pub fn command_line(program: &OsStr, args: &[&OsStr]) -> io::Result<Vec<u16>> {
    let mut out: Vec<u16> = Vec::new();
    append(&mut out, program, true)?;
    for arg in args {
        out.push(u16::from(b' '));
        append(&mut out, arg, false)?;
    }
    out.push(0);
    Ok(out)
}

fn append(out: &mut Vec<u16>, arg: &OsStr, always: bool) -> io::Result<()> {
    let (quote_mark, backslash) = (u16::from(b'"'), u16::from(b'\\'));
    let units: Vec<u16> = arg.encode_wide().collect();
    if units.contains(&0) {
        return Err(nul_inside());
    }
    let quote = always
        || units.is_empty()
        || units
            .iter()
            .any(|&u| u == u16::from(b' ') || u == u16::from(b'\t'));
    if quote {
        out.push(quote_mark);
    }
    let mut backslashes = 0_usize;
    for unit in units {
        if unit == backslash {
            backslashes += 1;
        } else {
            if unit == quote_mark {
                out.extend(std::iter::repeat_n(backslash, backslashes + 1));
            }
            backslashes = 0;
        }
        out.push(unit);
    }
    if quote {
        out.extend(std::iter::repeat_n(backslash, backslashes));
        out.push(quote_mark);
    }
    Ok(())
}

/// This process's environment minus `remove`, with `set` (names ignoring case), sorted by name ignoring case, as a
/// wide block (`NAME=value\0...\0\0`).
pub fn env_block(set: &[(&str, &str)], remove: &[&str]) -> io::Result<Vec<u16>> {
    let upper = |name: &OsStr| name.to_string_lossy().to_uppercase();
    let mut vars: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| {
            let name = upper(name);
            !remove.iter().any(|r| r.eq_ignore_ascii_case(&name))
                && !set.iter().any(|(s, _)| s.eq_ignore_ascii_case(&name))
        })
        .collect();
    vars.extend(
        set.iter()
            .map(|(name, value)| ((*name).into(), (*value).into())),
    );
    vars.sort_by_cached_key(|(name, _)| upper(name));
    let mut block: Vec<u16> = Vec::new();
    for (name, value) in &vars {
        let (name, value): (Vec<u16>, Vec<u16>) =
            (name.encode_wide().collect(), value.encode_wide().collect());
        if name.contains(&0) || value.contains(&0) {
            return Err(nul_inside());
        }
        block.extend(name);
        block.push(u16::from(b'='));
        block.extend(value);
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(units: &[u16]) -> String {
        String::from_utf16_lossy(units.strip_suffix(&[0]).unwrap_or(units))
    }

    #[test]
    fn the_command_line_splits_back_into_the_same_arguments() {
        let os = |s: &str| OsString::from(s);
        let args = [
            os("--no-pager"),
            os(""),
            os("a b"),
            os("tab\there"),
            os(r#"say "hi""#),
            os(r"C:\dir with space\"),
            os(r"back\\slash"),
            os(r#"alias.hold=!echo started > "C:/x ü/m.txt"; sleep 30"#),
            os("ü 日本"),
        ];
        let refs: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
        let line = command_line(OsStr::new(r"C:\Program Files\Git\cmd\git.exe"), &refs).unwrap();
        let expected = [
            r#""C:\Program Files\Git\cmd\git.exe""#,
            "--no-pager",
            r#""""#,
            r#""a b""#,
            "\"tab\there\"",
            r#""say \"hi\"""#,
            r#""C:\dir with space\\""#,
            r"back\\slash",
            r#""alias.hold=!echo started > \"C:/x ü/m.txt\"; sleep 30""#,
            r#""ü 日本""#,
        ]
        .join(" ");
        assert_eq!(text(&line), expected);
        assert!(command_line(OsStr::new("git"), &[OsStr::new("a\0b")]).is_err());
    }

    #[test]
    fn the_environment_is_scrubbed_set_and_sorted_ignoring_case() {
        let block = env_block(
            &[("LC_ALL", "C"), ("GIT_TERMINAL_PROMPT", "0")],
            &["GIT_DIR", "Path_That_Is_Not_There"],
        )
        .unwrap();
        assert_eq!(&block[block.len() - 2..], &[0, 0]);
        let entries: Vec<String> = text(&block[..block.len() - 1])
            .split('\0')
            .map(str::to_owned)
            .collect();
        let names: Vec<String> = entries
            .iter()
            .map(|e| e.split('=').next().unwrap_or("").to_uppercase())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        assert!(entries.contains(&"LC_ALL=C".to_owned()), "{entries:?}");
        assert!(entries.contains(&"GIT_TERMINAL_PROMPT=0".to_owned()));
        assert!(!names.iter().any(|n| n == "GIT_DIR"));
        assert_eq!(names.iter().filter(|n| *n == "LC_ALL").count(), 1);
    }
}
