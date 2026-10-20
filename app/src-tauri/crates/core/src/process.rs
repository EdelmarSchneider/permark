//! Starting a program that outlives this one (the app started by `permark mcp` to show a place).

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// Windows: this process's standard handles are made non-inheritable, so a detached child never holds a client's
/// pipes open (the client would wait for an end of file that never comes). The handles keep working here.
#[cfg(windows)]
// Documented FFI: `SetHandleInformation` on the process's own standard handles (borrowed, never closed).
#[allow(unsafe_code)]
fn keep_std_handles_private() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    let handles = [
        io::stdin().as_raw_handle(),
        io::stdout().as_raw_handle(),
        io::stderr().as_raw_handle(),
    ];
    for handle in handles {
        if !handle.is_null() {
            // SAFETY: a standard handle of this process (or an invalid one, which the call rejects); only its
            // inherit flag changes.
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
        }
    }
}

/// Starts `program` with `args`, detached (own process group, no console, standard streams to null, no shell),
/// and lets it go its own way.
///
/// # Errors
/// The program cannot be started.
pub fn spawn_detached(program: &Path, args: &[&std::ffi::OsStr]) -> io::Result<()> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        keep_std_handles_private();
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    // collects the exit status so the finished process is not left as a zombie
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
