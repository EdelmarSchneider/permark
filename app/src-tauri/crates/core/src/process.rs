//! Starting programs: one that outlives this one ([`spawn_detached`]: the app started by `permark mcp` to show a
//! place) | an attached child spoken to over its standard streams ([`spawn_piped`]: the model engine worker; ONLY
//! pinned, sha256-verified files from the profile).

use std::io;
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

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

/// Exit check interval while [`PipedChild::stop`] waits.
const STOP_POLL: Duration = Duration::from_millis(20);

/// An attached child's standard streams + the child itself.
#[derive(Debug)]
pub struct Piped {
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
    pub stderr: ChildStderr,
    pub child: PipedChild,
}

/// A running attached child. Drop = killed + reaped (never left running | as a zombie).
#[derive(Debug)]
pub struct PipedChild {
    child: Child,
    /// Exit status collected.
    reaped: bool,
}

impl PipedChild {
    /// OS process id.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Waits up to `grace` for the child to end by itself (caller closed its stdin | asked it to exit), then kills
    /// it; reaped either way.
    ///
    /// # Errors
    /// The exit status cannot be read.
    pub fn stop(&mut self, grace: Duration) -> io::Result<ExitStatus> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.reaped = true;
                return Ok(status);
            }
            if started.elapsed() >= grace {
                break;
            }
            std::thread::sleep(STOP_POLL);
        }
        // ended between the last check and now -> kill fails harmlessly, wait answers its status
        let _ = self.child.kill();
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }
}

impl Drop for PipedChild {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Starts `program` with `args` in `cwd` as an attached child (no shell, no console window) with stdin, stdout and
/// stderr piped. Caller MUST pass ONLY a pinned, sha256-verified program from the profile (never one named by
/// document content | a click).
///
/// # Errors
/// The program cannot be started (`NotFound`: missing; `PermissionDenied`: blocked, e.g. by security software).
pub fn spawn_piped(program: &Path, args: &[&std::ffi::OsStr], cwd: &Path) -> io::Result<Piped> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn()?;
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let child = PipedChild {
        child,
        reaped: false,
    };
    match (stdin, stdout, stderr) {
        (Some(stdin), Some(stdout), Some(stderr)) => Ok(Piped {
            stdin,
            stdout,
            stderr,
            child,
        }),
        // piped above: always present; dropping `child` kills it
        _ => Err(io::Error::other("child started without its pipes")),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::io::{Read, Write};
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::*;

    /// OS shell running `script`: `cmd /C` (Windows) | `sh -c` (unix).
    fn shell(script: &str, cwd: &Path) -> io::Result<Piped> {
        #[cfg(windows)]
        let (program, flag) = ("cmd", "/C");
        #[cfg(unix)]
        let (program, flag) = ("sh", "-c");
        spawn_piped(
            Path::new(program),
            &[OsStr::new(flag), OsStr::new(script)],
            cwd,
        )
    }

    fn read_all(mut from: impl Read) -> String {
        let mut text = String::new();
        from.read_to_string(&mut text).unwrap();
        text
    }

    /// `sort` (Windows) | `cat` (unix): stdin back on stdout once stdin ends.
    const ECHO: &str = if cfg!(windows) { "sort" } else { "cat" };
    /// Shell builtin waiting for a stdin line (no grandchild that would outlive a killed shell).
    const WAIT: &str = if cfg!(windows) { "set /p x=" } else { "read x" };

    #[test]
    fn a_piped_child_reads_stdin_and_answers_on_stdout() {
        let tmp = tempfile::tempdir().unwrap();
        let Piped {
            mut stdin,
            stdout,
            stderr: _,
            mut child,
        } = shell(ECHO, tmp.path()).unwrap();
        stdin.write_all(b"hello worker\n").unwrap();
        drop(stdin);
        assert_eq!(read_all(stdout).trim(), "hello worker");
        let status = child.stop(Duration::from_secs(10)).unwrap();
        assert!(status.success(), "{status:?}");
    }

    #[test]
    fn stderr_is_its_own_pipe_and_cwd_is_the_given_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let script = if cfg!(windows) {
            "cd & echo oops 1>&2"
        } else {
            "pwd; echo oops 1>&2"
        };
        let Piped {
            stdin,
            stdout,
            stderr,
            mut child,
        } = shell(script, tmp.path()).unwrap();
        drop(stdin);
        let cwd = read_all(stdout);
        assert_eq!(read_all(stderr).trim(), "oops");
        let expected = tmp.path().canonicalize().unwrap();
        let got = Path::new(cwd.trim()).canonicalize().unwrap();
        assert_eq!(got, expected);
        child.stop(Duration::from_secs(10)).unwrap();
    }

    /// A child that does not end within the grace: killed + reaped (stdin kept open: it waits for input forever).
    #[test]
    fn stop_kills_a_child_that_does_not_end_within_the_grace() {
        let tmp = tempfile::tempdir().unwrap();
        let Piped {
            stdin,
            stdout,
            stderr: _,
            mut child,
        } = shell(WAIT, tmp.path()).unwrap();
        let started = Instant::now();
        let status = child.stop(Duration::from_millis(200)).unwrap();
        assert!(!status.success(), "{status:?}");
        assert!(started.elapsed() >= Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(10));
        // gone: its stdout ends
        assert_eq!(read_all(stdout), "");
        drop(stdin);
    }

    #[test]
    fn a_dropped_child_is_killed() {
        let tmp = tempfile::tempdir().unwrap();
        let Piped {
            stdin,
            stdout,
            stderr: _,
            child,
        } = shell(WAIT, tmp.path()).unwrap();
        drop(child);
        assert_eq!(read_all(stdout), "");
        drop(stdin);
    }

    #[test]
    fn a_missing_program_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such-worker");
        let Err(error) = spawn_piped(&missing, &[], tmp.path()) else {
            panic!("started");
        };
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
