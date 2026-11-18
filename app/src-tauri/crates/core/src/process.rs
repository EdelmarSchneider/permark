//! Starting programs: one that outlives this one ([`spawn_detached`]: the app started by `permark mcp` to show a
//! place) | an attached child spoken to over its standard streams: [`spawn_piped`] (the model engine worker; ONLY
//! pinned, sha256-verified files from the profile) | [`spawn_person_command`] (a program the person named in the
//! app's own settings, found by [`resolve_program`]; never one named by a project file, document content or a click).

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
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
pub fn spawn_detached(program: &Path, args: &[&OsStr]) -> io::Result<()> {
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
pub fn spawn_piped(program: &Path, args: &[&OsStr], cwd: &Path) -> io::Result<Piped> {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    piped(command)
}

/// Starts a program the person chose in the app's own settings (profile, never a project's shared file) as an
/// attached child, like [`spawn_piped`]: no shell of ours, no console window, the three streams piped. `env` is set
/// on top of this process's environment (a listed name overrides the inherited value). Caller MUST NEVER pass a
/// program, argument or variable named by a file in a project, document content or a click. Windows: a `.bat` |
/// `.cmd` script runs through the system's command interpreter, with the arguments escaped by the standard library
/// (an argument it cannot pass safely is refused).
///
/// # Errors
/// The program cannot be started (`NotFound`: missing; `InvalidInput`: an argument a script cannot receive safely).
pub fn spawn_person_command(
    program: &Path,
    args: &[&OsStr],
    cwd: &Path,
    env: &[(&OsStr, &OsStr)],
) -> io::Result<Piped> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .envs(env.iter().copied());
    piped(command)
}

/// Spawns `command` (program, arguments and folder set) with the three streams piped and no console window.
fn piped(mut command: Command) -> io::Result<Piped> {
    command
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

/// Extensions Windows tries for a bare program name when `PATHEXT` is not set.
#[cfg(windows)]
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// The program file a name stands for. A name with a folder in it (`./x`, `C:\tools\x.cmd`, `/usr/bin/x`) is that
/// file when it exists; a bare name is searched in the folders of `PATH`, in order (never the current folder).
/// Windows: a bare name without an extension takes each `PATHEXT` extension in turn, as the command line does (`npx`
/// -> `npx.cmd`; the standard library would add only `.exe`). Unix: a regular file with an execute bit.
#[must_use]
pub fn resolve_program(name: &OsStr) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let pathext = if cfg!(windows) {
        std::env::var_os("PATHEXT")
    } else {
        None
    };
    resolve_program_in(name, &path, pathext.as_deref())
}

/// [`resolve_program`] over the folder list `path` (`PATH` syntax) and the extension list `pathext` (Windows only;
/// `None`: the system's usual list), never this process's own `PATH`: a caller that tells which programs a person
/// has checks the same rule against a list it holds.
#[must_use]
pub fn resolve_program_in(name: &OsStr, path: &OsStr, pathext: Option<&OsStr>) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let given = Path::new(name);
    let has_folder = given.has_root() || given.parent().is_some_and(|p| !p.as_os_str().is_empty());
    if has_folder {
        return runnable(given).then(|| given.to_path_buf());
    }
    let names = candidates(name, pathext);
    std::env::split_paths(path)
        // an empty entry would mean the current folder: never searched
        .filter(|dir| !dir.as_os_str().is_empty())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|candidate| runnable(candidate))
}

/// The file names a bare `name` may have in a folder of `PATH`.
#[cfg(windows)]
fn candidates(name: &OsStr, pathext: Option<&OsStr>) -> Vec<OsString> {
    if Path::new(name).extension().is_some() {
        return vec![name.to_owned()];
    }
    let list = pathext
        .and_then(OsStr::to_str)
        .filter(|l| !l.trim().is_empty())
        .unwrap_or(DEFAULT_PATHEXT);
    list.split(';')
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
        .map(|ext| {
            let mut full = name.to_owned();
            full.push(ext);
            full
        })
        .collect()
}

/// The file names a bare `name` may have in a folder of `PATH`.
#[cfg(not(windows))]
fn candidates(name: &OsStr, _pathext: Option<&OsStr>) -> Vec<OsString> {
    vec![name.to_owned()]
}

/// A file that can be started: Windows: a file; unix: a regular file with an execute bit.
fn runnable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
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

    /// The OS shell every test machine has, found on `PATH` by its bare name.
    #[test]
    fn a_bare_name_is_found_on_the_path() {
        let name = if cfg!(windows) { "cmd" } else { "sh" };
        let found = resolve_program(OsStr::new(name)).expect("on PATH");
        assert!(found.is_absolute(), "{found:?}");
        let stem = found.file_stem().unwrap().to_string_lossy().to_lowercase();
        assert_eq!(stem, name);
        assert!(resolve_program(OsStr::new("no-such-program-anywhere-x9")).is_none());
        assert!(resolve_program(OsStr::new("")).is_none());
    }

    /// A name with a folder in it is that file (when it exists), never a `PATH` search.
    #[test]
    fn a_name_with_a_folder_is_that_file() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = tmp
            .path()
            .join(if cfg!(windows) { "tool.cmd" } else { "tool" });
        std::fs::write(&tool, "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(resolve_program(tool.as_os_str()), Some(tool.clone()));
        let missing = tmp.path().join("nothing-here");
        assert_eq!(resolve_program(missing.as_os_str()), None);
    }

    /// Windows: a bare name takes each `PATHEXT` extension in turn; a name with an extension is looked up as written.
    #[cfg(windows)]
    #[test]
    fn windows_tries_each_pathext_extension() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(second.path().join("tool.cmd"), "x").unwrap();
        std::fs::write(second.path().join("tool"), "x").unwrap();
        let path = std::env::join_paths([first.path(), second.path()]).unwrap();
        let found = resolve_program_in(OsStr::new("tool"), &path, Some(OsStr::new(".EXE;.CMD")));
        assert_eq!(found, Some(second.path().join("tool.CMD")));
        let found = resolve_program_in(OsStr::new("tool.cmd"), &path, None);
        assert_eq!(found, Some(second.path().join("tool.cmd")));
        // no PATHEXT: the system's usual list
        let found = resolve_program_in(OsStr::new("tool"), &path, None);
        assert_eq!(found, Some(second.path().join("tool.CMD")));
    }

    /// Unix: a file without the execute bit is not a program.
    #[cfg(unix)]
    #[test]
    fn unix_needs_the_execute_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("tool");
        std::fs::write(&tool, "x").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            resolve_program_in(OsStr::new("tool"), dir.path().as_os_str(), None),
            None
        );
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            resolve_program_in(OsStr::new("tool"), dir.path().as_os_str(), None),
            Some(tool)
        );
    }

    /// The given variables reach the program on top of the inherited ones.
    #[test]
    fn a_persons_command_runs_with_piped_streams_and_its_variables() {
        let tmp = tempfile::tempdir().unwrap();
        let (name, args) = if cfg!(windows) {
            ("cmd", ["/C", "echo hello %PERMARK_TEST_GREETING%"])
        } else {
            ("sh", ["-c", "echo hello $PERMARK_TEST_GREETING"])
        };
        let program = resolve_program(OsStr::new(name)).unwrap();
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        let env = [(OsStr::new("PERMARK_TEST_GREETING"), OsStr::new("world"))];
        let Piped {
            stdin,
            stdout,
            stderr: _,
            mut child,
        } = spawn_person_command(&program, &args, tmp.path(), &env).unwrap();
        drop(stdin);
        assert_eq!(read_all(stdout).trim(), "hello world");
        assert!(child.stop(Duration::from_secs(10)).unwrap().success());
    }

    /// Windows: a `.cmd` script (how `npx` ships) runs without a shell of ours; its arguments arrive as given.
    #[cfg(windows)]
    #[test]
    fn a_cmd_script_runs_with_its_arguments() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("hello.cmd");
        std::fs::write(&script, "@echo hello %~1\r\n").unwrap();
        let Piped {
            stdin,
            stdout,
            stderr: _,
            mut child,
        } = spawn_person_command(&script, &[OsStr::new("big world")], tmp.path(), &[]).unwrap();
        drop(stdin);
        assert_eq!(read_all(stdout).trim(), "hello big world");
        child.stop(Duration::from_secs(10)).unwrap();
    }
}
