//! Starting programs: one that outlives this one ([`spawn_detached`]: the app started by `permark mcp` to show a
//! place) | an attached child spoken to over its standard streams, which takes every process it starts with it when
//! stopped ([`PipedChild`]): [`spawn_piped`] (the model engine worker; ONLY pinned, sha256-verified files from the
//! profile) | [`spawn_person_command`] (a program the person named in the app's own settings, found by
//! [`resolve_program`]; never one named by a project file, document content or a click).

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
/// Unix: wait after SIGTERM to the group before SIGKILL.
#[cfg(unix)]
const TERM_WAIT: Duration = Duration::from_millis(500);

/// An attached child's standard streams + the child itself.
#[derive(Debug)]
pub struct Piped {
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
    pub stderr: ChildStderr,
    pub child: PipedChild,
}

/// A running attached child with every process it starts: Windows: a job object that ends them all once its last
/// handle closes; unix: the child leads a process group of its own. Killing the child alone left its own children
/// running (`npx.cmd` -> `cmd.exe` -> `node`; a shell command an agent started), with the person's rights. Drop = the
/// whole tree killed + the child reaped (never left running | as a zombie).
#[derive(Debug)]
pub struct PipedChild {
    child: Child,
    /// Exit status collected.
    reaped: bool,
    /// The job holding the child and what it starts (`None`: the system refused one: the child alone is killed).
    #[cfg(windows)]
    job: Option<std::os::windows::io::OwnedHandle>,
}

impl PipedChild {
    /// Waits up to `grace` for the child to end by itself (caller closed its stdin | asked it to exit); then unix:
    /// SIGTERM to its group and a short wait. Then every process of the tree still running is killed (also what a
    /// child that ended by itself left behind); the child is reaped either way.
    ///
    /// # Errors
    /// The exit status cannot be read.
    pub fn stop(&mut self, grace: Duration) -> io::Result<ExitStatus> {
        if !self.ends_within(grace)? {
            self.ask_tree_to_end()?;
        }
        self.kill_tree();
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }

    /// Unix: SIGTERM to the group (what a terminal's close sends), then a short wait for the child.
    #[cfg(unix)]
    fn ask_tree_to_end(&mut self) -> io::Result<()> {
        self.signal_group(rustix::process::Signal::TERM);
        self.ends_within(TERM_WAIT).map(|_| ())
    }

    /// Windows: no polite signal reaches a tree without a console window: the kill follows.
    #[cfg(windows)]
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)]
    fn ask_tree_to_end(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// Whether the child ended within `wait` (checked every [`STOP_POLL`]).
    fn ends_within(&mut self, wait: Duration) -> io::Result<bool> {
        let started = Instant::now();
        loop {
            if self.ended()? {
                return Ok(true);
            }
            if started.elapsed() >= wait {
                return Ok(false);
            }
            std::thread::sleep(STOP_POLL);
        }
    }

    /// The child ended (Windows: its handle stays open until drop, so its id is never reused meanwhile).
    #[cfg(windows)]
    fn ended(&mut self) -> io::Result<bool> {
        Ok(self.child.try_wait()?.is_some())
    }

    /// The child ended, left unreaped: its id, which is also its group's, stays taken until [`Child::wait`], so a
    /// signal to the group never reaches a stranger's group.
    #[cfg(unix)]
    fn ended(&mut self) -> io::Result<bool> {
        use rustix::process::{waitid, Pid, WaitId, WaitIdOptions};
        let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        Ok(waitid(WaitId::Pid(Pid::from_child(&self.child)), options)?.is_some())
    }

    /// Unix: `signal` to every process of the child's group (none left: harmless).
    #[cfg(unix)]
    fn signal_group(&self, signal: rustix::process::Signal) {
        let group = rustix::process::Pid::from_child(&self.child);
        let _ = rustix::process::kill_process_group(group, signal);
    }

    /// Kills every process of the tree still running, then the child itself (ended ones: harmless). Not reaped.
    #[allow(unsafe_code)]
    fn kill_tree(&mut self) {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            use std::os::windows::io::AsRawHandle;
            // SAFETY: `job` is an open job handle owned by `self`.
            unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(job.as_raw_handle(), 1)
            };
        }
        #[cfg(unix)]
        self.signal_group(rustix::process::Signal::KILL);
        let _ = self.child.kill();
    }
}

impl Drop for PipedChild {
    fn drop(&mut self) {
        if !self.reaped {
            self.kill_tree();
            let _ = self.child.wait();
        }
    }
}

/// A job that ends every process in it once its last handle closes, holding `child` (`None`: refused). Assigned right
/// after the start: whatever the child starts from then on joins the job (a child's own start takes far longer than
/// this call). Not inheritable: no process of the tree keeps the job open.
#[cfg(windows)]
#[allow(unsafe_code)]
fn tree_job(child: &Child) -> Option<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    // SAFETY: no attributes, no name: a new unnamed, non-inheritable job; null on failure.
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw.is_null() {
        return None;
    }
    // SAFETY: a fresh handle, owned from here on (closed once, on drop).
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let size = u32::try_from(std::mem::size_of_val(&info)).ok()?;
    // SAFETY: `info` is a readable struct of `size` bytes for this information class; `job` is open.
    let limited = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&raw const info).cast(),
            size,
        )
    } != 0;
    // SAFETY: `job` is open; the child's process handle is open while `child` lives.
    let assigned = limited
        && unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } != 0;
    assigned.then_some(job)
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
/// attached child, like [`spawn_piped`]: no shell of ours, no console window, the three streams piped. `PATH` =
/// [`person_path`] (unix: the program's own folder first, then this process's `PATH`, then [`tool_folders`]: a
/// `#!/usr/bin/env node` script finds the `node` beside it, also in an app started from the desktop); `env` is set on
/// top (a listed name overrides). `program` MUST be absolute ([`resolve_program`]); `cwd` MUST hold nothing a
/// project chose (a runner reads its settings from its folder). Caller MUST NEVER pass a program, argument or
/// variable named by a file in a project, document content or a click. Windows: a `.bat` | `.cmd` script runs
/// through the system's command interpreter, with the arguments escaped by the standard library (an argument it
/// cannot pass safely is refused).
///
/// # Errors
/// The program cannot be started (`NotFound`: missing; `InvalidInput`: a relative program | an argument a script
/// cannot receive safely).
pub fn spawn_person_command(
    program: &Path,
    args: &[&OsStr],
    cwd: &Path,
    env: &[(&OsStr, &OsStr)],
) -> io::Result<Piped> {
    if !program.is_absolute() {
        // the standard library resolves a relative program from the new folder on unix, from this one on Windows
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "relative program path",
        ));
    }
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    let first = if cfg!(windows) {
        None
    } else {
        program.parent()
    };
    let inherited = std::env::var_os("PATH");
    if let Some(path) = person_path(first, inherited.as_deref(), tool_folders()) {
        command.env("PATH", path);
    }
    command.envs(env.iter().copied());
    piped(command)
}

/// The `PATH` of a person's command: `first`, then the folders of `inherited`, then `tools`; relative entries (they
/// would mean folders under wherever the program starts) and repeats left out. `None`: nothing left to set.
fn person_path(
    first: Option<&Path>,
    inherited: Option<&OsStr>,
    tools: Vec<PathBuf>,
) -> Option<OsString> {
    let mut folders: Vec<PathBuf> = Vec::new();
    let all = first
        .map(Path::to_path_buf)
        .into_iter()
        .chain(inherited.into_iter().flat_map(std::env::split_paths))
        .chain(tools);
    for folder in all {
        if folder.is_absolute() && !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    if folders.is_empty() {
        return None;
    }
    std::env::join_paths(folders).ok()
}

/// Spawns `command` (program, arguments and folder set) with the three streams piped, no console window, and every
/// process it starts tied to it ([`PipedChild`]).
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
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let child = PipedChild {
        #[cfg(windows)]
        job: tree_job(&child),
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

/// The program file a name stands for. An absolute path (`C:\tools\x.cmd`, `/usr/bin/x`) is that file when it
/// exists; a relative one (`./x`, `bin/x`) is never resolved ([`is_relative_path`]); a bare name is searched in the
/// absolute folders of `PATH`, in order (never the current folder, never a relative entry), then in [`tool_folders`].
/// Windows: a bare name without an extension takes each `PATHEXT` extension in turn, as the command line does (`npx`
/// -> `npx.cmd`; the standard library would add only `.exe`). Unix: a regular file with an execute bit.
#[must_use]
pub fn resolve_program(name: &OsStr) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let pathext = if cfg!(windows) {
        std::env::var_os("PATHEXT")
    } else {
        None
    };
    let folders = std::env::split_paths(&path).chain(tool_folders());
    find_program(name, folders, pathext.as_deref())
}

/// [`resolve_program`] over the folder list `path` (`PATH` syntax) alone and the extension list `pathext` (Windows
/// only; `None`: the system's usual list), never this process's own `PATH`: a caller that tells which programs a
/// person has checks the same rule against a list it holds.
#[must_use]
pub fn resolve_program_in(name: &OsStr, path: &OsStr, pathext: Option<&OsStr>) -> Option<PathBuf> {
    find_program(name, std::env::split_paths(path), pathext)
}

/// A name with a folder in it that is not absolute (`./x`, `bin/x`; Windows also `\x`, `C:x`): it names a different
/// file in each folder a program may start in, so it is never resolved.
#[must_use]
pub fn is_relative_path(name: &OsStr) -> bool {
    let given = Path::new(name);
    has_folder(given) && !given.is_absolute()
}

fn has_folder(given: &Path) -> bool {
    given.has_root() || given.parent().is_some_and(|p| !p.as_os_str().is_empty())
}

fn find_program(
    name: &OsStr,
    folders: impl Iterator<Item = PathBuf>,
    pathext: Option<&OsStr>,
) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let given = Path::new(name);
    if has_folder(given) {
        return (given.is_absolute() && runnable(given)).then(|| given.to_path_buf());
    }
    let names = candidates(name, pathext);
    folders
        // an empty | relative entry would mean a folder under the current one: never searched
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|candidate| runnable(candidate))
}

/// Unix: the usual folders of people's own tools that exist on this computer ([`user_tool_folders`] of the home
/// folder). Windows: none (installers there add their folders to the person's `PATH`, which every app gets).
#[must_use]
pub fn tool_folders() -> Vec<PathBuf> {
    if cfg!(windows) {
        return Vec::new();
    }
    user_tool_folders(dirs::home_dir().as_deref(), cfg!(target_os = "macos"))
}

/// Folders where people's own tools usually live that an app started from the desktop misses in its `PATH` (macOS
/// gives an app opened from Finder `/usr/bin:/bin:/usr/sbin:/sbin`; nvm, Volta, Bun, rustup and npm's own prefix add
/// theirs only in a shell's startup file): macOS: Homebrew's `/opt/homebrew/bin` and `/usr/local/bin`; under `home`:
/// `.local/bin`, `.volta/bin`, `.bun/bin`, `.cargo/bin`, `.npm-global/bin`, the newest nvm Node's `bin`. Only those
/// that exist, in that order.
fn user_tool_folders(home: Option<&Path>, macos: bool) -> Vec<PathBuf> {
    let system: &[&str] = if macos {
        &["/opt/homebrew/bin", "/usr/local/bin"]
    } else {
        &[]
    };
    let mut folders: Vec<PathBuf> = system.iter().map(PathBuf::from).collect();
    if let Some(home) = home {
        folders.extend(
            [
                ".local/bin",
                ".volta/bin",
                ".bun/bin",
                ".cargo/bin",
                ".npm-global/bin",
            ]
            .iter()
            .map(|sub| home.join(sub)),
        );
        folders.extend(newest_nvm_node(&home.join(".nvm/versions/node")));
    }
    folders.retain(|folder| folder.is_dir());
    folders
}

/// `<versions>/v<x.y.z>/bin` of the highest version (folders not named `v` + numbers are left out).
fn newest_nvm_node(versions: &Path) -> Option<PathBuf> {
    let number = |name: &str| -> Option<Vec<u64>> {
        name.strip_prefix('v')?
            .split('.')
            .map(|part| part.parse().ok())
            .collect()
    };
    std::fs::read_dir(versions)
        .ok()?
        .flatten()
        .filter_map(|entry| Some((number(entry.file_name().to_str()?)?, entry.path())))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, folder)| folder.join("bin"))
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

    /// Shell line that starts a grandchild holding the shell's stdout for a minute: `then` runs after it, in the
    /// shell (`None`: the shell ends at once, leaving the grandchild behind).
    fn with_grandchild(then: Option<&str>) -> String {
        let start = if cfg!(windows) {
            "start /b ping -n 60 127.0.0.1"
        } else {
            "sleep 60 &"
        };
        match (then, cfg!(windows)) {
            (Some(then), true) => format!("{start} & {then}"),
            (Some(then), false) => format!("{start} {then}"),
            (None, _) => start.to_owned(),
        }
    }

    /// Reads `stdout` to its end on a thread of its own: `true` when it ended within `wait` (every process holding it
    /// is gone).
    fn ends_within(stdout: ChildStdout, wait: Duration) -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = read_all(stdout);
            let _ = tx.send(());
        });
        rx.recv_timeout(wait).is_ok()
    }

    /// A child that does not end within the grace: it and the grandchild it started are both gone after `stop`.
    #[test]
    fn stop_ends_the_whole_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let Piped {
            stdin,
            stdout,
            stderr: _,
            mut child,
        } = shell(&with_grandchild(Some(WAIT)), tmp.path()).unwrap();
        // the grandchild runs before the stop
        std::thread::sleep(Duration::from_millis(500));
        child.stop(Duration::from_millis(200)).unwrap();
        assert!(
            ends_within(stdout, Duration::from_secs(15)),
            "a grandchild survived"
        );
        drop(stdin);
    }

    /// A child that ended by itself within the grace: what it left running ends with the stop.
    #[test]
    fn stop_ends_what_a_finished_child_left_running() {
        let tmp = tempfile::tempdir().unwrap();
        let Piped {
            stdin,
            stdout,
            stderr: _,
            mut child,
        } = shell(&with_grandchild(None), tmp.path()).unwrap();
        child.stop(Duration::from_secs(5)).unwrap();
        assert!(
            ends_within(stdout, Duration::from_secs(15)),
            "a grandchild survived"
        );
        drop(stdin);
    }

    #[test]
    fn a_dropped_child_takes_its_tree_with_it() {
        let tmp = tempfile::tempdir().unwrap();
        let Piped {
            stdin,
            stdout,
            stderr: _,
            child,
        } = shell(&with_grandchild(Some(WAIT)), tmp.path()).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        drop(child);
        assert!(
            ends_within(stdout, Duration::from_secs(15)),
            "a grandchild survived"
        );
        drop(stdin);
    }

    /// A relative path (with a folder in it) names a different file in each folder a program starts in: never
    /// resolved, also when it exists from this process's folder; a relative `PATH` entry is never searched either.
    /// (Tests run in the crate's folder: `src/lib.rs` is there.)
    #[test]
    fn relative_paths_are_never_resolved() {
        let relative = Path::new("src").join("lib.rs");
        assert!(relative.is_file());
        assert!(is_relative_path(relative.as_os_str()));
        assert_eq!(resolve_program(relative.as_os_str()), None);
        assert_eq!(
            resolve_program_in(relative.as_os_str(), OsStr::new(""), None),
            None
        );
        let dot = Path::new(".").join("Cargo.toml");
        assert_eq!(
            resolve_program_in(dot.as_os_str(), OsStr::new(""), None),
            None
        );
        // a relative PATH entry
        assert_eq!(
            resolve_program_in(OsStr::new("lib.rs"), OsStr::new("src"), None),
            None
        );
        let absolute = std::env::current_dir().unwrap().join("src");
        if cfg!(windows) {
            assert_eq!(
                resolve_program_in(OsStr::new("lib.rs"), absolute.as_os_str(), None),
                Some(absolute.join("lib.rs"))
            );
        }
        assert!(!is_relative_path(OsStr::new("npx")));
        assert!(!is_relative_path(absolute.join("lib.rs").as_os_str()));
        // never started either: unix resolves it after changing into the new folder
        let Err(refused) = spawn_person_command(&relative, &[], &absolute, &[]) else {
            panic!("started");
        };
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
    }

    /// The usual folders of people's own tools: only those that exist; nvm's newest Node.
    #[test]
    fn user_tool_folders_are_the_existing_usual_ones() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        for sub in [
            ".local/bin",
            ".cargo/bin",
            ".nvm/versions/node/v9.11.2/bin",
            ".nvm/versions/node/v20.10.0/bin",
            ".nvm/versions/node/v18.19.1/bin",
            ".nvm/versions/node/not-a-version/bin",
        ] {
            std::fs::create_dir_all(home.join(sub)).unwrap();
        }
        let found = user_tool_folders(Some(home), false);
        assert_eq!(
            found,
            [
                home.join(".local/bin"),
                home.join(".cargo/bin"),
                home.join(".nvm/versions/node/v20.10.0/bin"),
            ]
        );
        assert_eq!(user_tool_folders(None, false), Vec::<PathBuf>::new());
        // macOS: the system-wide folders only when present (never on a test machine without them)
        for dir in user_tool_folders(None, true) {
            assert!(dir.is_dir(), "{dir:?}");
        }
    }

    /// A person's command searches: the program's own folder (when given), this process's `PATH`, the tool
    /// folders; relative and repeated entries left out.
    #[test]
    fn a_persons_command_path_puts_the_programs_folder_first() {
        let tmp = tempfile::tempdir().unwrap();
        let [own, a, b, tool] = ["own", "a", "b", "tool"].map(|n| tmp.path().join(n));
        let inherited =
            std::env::join_paths([a.as_path(), Path::new("rel"), b.as_path(), own.as_path()])
                .unwrap();
        let path =
            person_path(Some(&own), Some(&inherited), vec![tool.clone(), a.clone()]).unwrap();
        assert_eq!(
            std::env::split_paths(&path).collect::<Vec<_>>(),
            [own.clone(), a.clone(), b.clone(), tool.clone()]
        );
        let path = person_path(None, Some(&inherited), Vec::new()).unwrap();
        assert_eq!(
            std::env::split_paths(&path).collect::<Vec<_>>(),
            [a, b, own]
        );
        assert_eq!(person_path(None, None, Vec::new()), None);
        assert_eq!(person_path(None, Some(OsStr::new("rel")), Vec::new()), None);
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
