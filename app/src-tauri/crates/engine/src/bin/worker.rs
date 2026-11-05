//! `permark-llm-worker`: PerMark's model engine worker (D-10). Speaks [`permark_engine::wire`] on stdin | stdout for
//! ONE [`LlamaEngine`] session; started only by the app (pinned + sha-verified files, D-15).
//!
//! - start: protocol moved to a duplicate of stdout; fd | handle 1 -> stderr (C++ | driver prints never reach the
//!   protocol). Logs = stderr (the host keeps a capped tail).
//! - end (stdin EOF | `Unload` | host gone): IMMEDIATE hard exit, NEVER `std::process::exit` (runs C++ static
//!   destructors + DLL detach: GPU drivers may hang there).
//! - no socket, no network, no file besides the model + its own folder (AC-2).

use std::fs::File;
use std::io::{self, BufReader, BufWriter};

use permark_engine::llama::LlamaEngine;
use permark_engine::serve;

fn main() {
    let protocol = match protocol_out() {
        Ok(file) => file,
        Err(e) => {
            eprintln!("permark-llm-worker: no protocol pipe: {e}");
            hard_exit(2)
        }
    };
    let engine = match LlamaEngine::new() {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("permark-llm-worker: engine start failed: {e}");
            hard_exit(3)
        }
    };
    let code = match serve(
        BufReader::new(io::stdin()),
        BufWriter::new(protocol),
        &engine,
    ) {
        Ok(()) => 0,
        Err(_) => 1,
    };
    hard_exit(code)
}

/// Protocol writer = the original stdout; stdout itself now goes to stderr.
#[cfg(unix)]
#[allow(unsafe_code)]
fn protocol_out() -> io::Result<File> {
    use std::os::fd::FromRawFd;
    // SAFETY: plain fd calls on the process's own descriptors 1 + 2; `dup` result checked before use and owned by
    // the returned `File` only.
    unsafe {
        let fd = libc::dup(1);
        if fd < 0 || libc::dup2(2, 1) < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(File::from_raw_fd(fd))
    }
}

/// Protocol writer = a duplicate of the original stdout handle; std handle + CRT fd 1 now go to stderr (libraries
/// loaded later read the std handle at start; a shared CRT sees fd 1).
#[cfg(windows)]
#[allow(unsafe_code)]
fn protocol_out() -> io::Result<File> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: Win32 calls on this process's own std handles; every result checked; the duplicate is owned by the
    // returned `File` only (the CRT closing fd 1 in `dup2` closes the original, not the duplicate).
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        let err = GetStdHandle(STD_ERROR_HANDLE);
        let mut dup: HANDLE = std::ptr::null_mut();
        let me = GetCurrentProcess();
        if out.is_null()
            || DuplicateHandle(me, out, me, &raw mut dup, 0, 0, DUPLICATE_SAME_ACCESS) == 0
        {
            return Err(io::Error::last_os_error());
        }
        if SetStdHandle(STD_OUTPUT_HANDLE, err) == 0 {
            return Err(io::Error::last_os_error());
        }
        // CRT level (no console: fd 1 may be unset -> failure harmless)
        let _ = libc::dup2(2, 1);
        Ok(File::from_raw_handle(dup))
    }
}

/// Ends the process now: no atexit, no static destructors, no DLL detach.
#[allow(unsafe_code)]
fn hard_exit(code: i32) -> ! {
    #[cfg(windows)]
    // SAFETY: terminates this process; nothing runs after.
    unsafe {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        TerminateProcess(GetCurrentProcess(), code.cast_unsigned());
    }
    // SAFETY: `_exit` ends the process without running any handler.
    unsafe { libc::_exit(code) }
}
