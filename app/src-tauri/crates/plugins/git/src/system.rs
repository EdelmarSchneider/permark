//! The installed git as a backend: the only program PerMark starts on its own. Absolute path found by walking `PATH`,
//! no shell, fixed arguments, a scrubbed environment, a timeout and a cap on the output.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use permark_core::plugins::{CommitId, FileVersions};

use crate::ops::{
    below_prefix, finish, pathspec, prefix_of, repo_path, shallow, too_broad, tracked_entry,
    Baseline, Discovery, GitError, GitOps, RawFile, RawStatus, RepoLocation, MAX_RENAME_PATHS,
    MAX_VERSIONS,
};
use crate::porcelain::{
    parse_name_status, parse_status, parse_tree_file, parse_untracked, parse_versions_log, Entry,
    LogEntry, LOG_FORMAT,
};
use crate::types::{BackendKind, HeadInfo, Letter};

/// How long one git call may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Most bytes one call may print.
const MAX_OUTPUT: usize = 64 * 1024 * 1024;
/// Most bytes of a committed file.
const MAX_BLOB: usize = 8 * 1024 * 1024;
const MAX_STDERR: usize = 64 * 1024;
/// Most bytes of index entries listed for one folder (whether it is tracked needs one).
const MAX_LISTED: usize = 64 * 1024;
/// The largest number of commits `git log --skip` takes.
const MAX_SKIP: usize = 0x7fff_ffff;

/// Variables every call sets: no optional locks (a status never blocks the person's git), plain messages, no prompt,
/// and no fetch of an object a partial copy of a repository left out (a fetch goes where the repository's config
/// says, which may be a program). Two locks, because PerMark only reads what is on this computer:
/// `GIT_NO_LAZY_FETCH` switches the fetch off (git 2.45+ reads it), and `GIT_ALLOW_PROTOCOL` is the list of
/// transports git may use, which overrides every `protocol.*.allow` of the config: `:` names none.
/// `GIT_NO_REPLACE_OBJECTS`: a replace ref (`refs/replace/<id>`) makes git read another object in place of one, and
/// the built-in backend does not know them; every object is read as it is, so both backends tell the same history.
const SET: [(&str, &str); 6] = [
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("LC_ALL", "C"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_NO_LAZY_FETCH", "1"),
    ("GIT_ALLOW_PROTOCOL", ":"),
    ("GIT_NO_REPLACE_OBJECTS", "1"),
];

/// Options before every command: no pager, no filesystem monitor (both are programs the config names). The hooks
/// option ([`hooks_option`]) follows.
const BASE: [&str; 3] = ["--no-pager", "-c", "core.fsmonitor=false"];

/// The first git that reads `core.hooksPath`: an older one would run the hooks of the repository's own folder.
const MIN_VERSION: (u32, u32) = (2, 9);

/// The `log` that lists the versions of a file, up to its revision and path. A repository's config can make `log` run
/// a program or answer another way, so every such setting is fixed here: no signature check (it starts
/// `gpg.program`), no following of renames, the first commit's changes shown, names as committed (no mailmap), no
/// color, UTF-8. `--full-history`: a merged branch is walked even when the merge took the other side's file.
/// `--no-use-mailmap` is the spelling every git that knows `status --porcelain=v2` takes (`--no-mailmap`: 2.27+).
const VERSIONS_LOG: [&str; 20] = [
    "-c",
    "log.showSignature=false",
    "-c",
    "log.follow=false",
    "-c",
    "log.showRoot=true",
    "log",
    "--full-history",
    "--no-merges",
    "--raw",
    "--no-abbrev",
    "--no-renames",
    "-z",
    "--no-show-signature",
    "--no-use-mailmap",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--encoding=UTF-8",
    LOG_FORMAT,
];

/// The `diff-tree` that names the paths one commit changed, a rename as one change (the limit of the search for
/// renames and the commit's id follow). Names only, and no external diff | text conversion: no program of the config
/// runs.
const RENAMES_OF: [&str; 8] = [
    "diff-tree",
    "-r",
    "-M",
    "--name-status",
    "-z",
    "--no-commit-id",
    "--no-ext-diff",
    "--no-textconv",
];

/// Variables that would point git somewhere else than the folder it is asked about.
const SCRUBBED: [&str; 8] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_EXTERNAL_DIFF",
    "GIT_NAMESPACE",
];

/// What a finished git call left.
#[derive(Debug)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

/// The git on `PATH`, as an absolute path (relative `PATH` entries are skipped).
#[must_use]
pub fn find_git() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Where every call tells git to look for hook files: a path no file can be found below, so git starts none of the
/// hooks a folder's `.git/` brings. Windows: below this program's own file; `NUL` | `/dev/null` are no choice there,
/// git reads them as folder names (a folder `NUL\` can be made through `\\?\`). Elsewhere: the null device. `None`:
/// no such path here.
fn hooks_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        hooks_path_below(&std::env::current_exe().ok()?)
    }
    #[cfg(not(windows))]
    {
        let null = PathBuf::from("/dev/null");
        nothing_below(&null).then_some(null)
    }
}

/// `<exe>\no-hooks` when `exe` is a file named by a plain drive path: no `\\?\` | `\\.\` left after
/// [`permark_core::paths::simplified`], no share.
#[cfg(windows)]
fn hooks_path_below(exe: &Path) -> Option<PathBuf> {
    use std::path::{Component, Prefix};
    let plain = permark_core::paths::simplified(exe);
    let drive = matches!(
        plain.components().next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))
    );
    let value = plain.join("no-hooks");
    (drive && nothing_below(&value)).then_some(value)
}

/// Whether no file can ever be found below `value`: an absolute path whose parent is a regular file (Windows) | that
/// exists and is no folder (the null device elsewhere: a device, not a regular file).
fn nothing_below(value: &Path) -> bool {
    if !value.is_absolute() {
        return false;
    }
    if cfg!(windows) {
        value
            .parent()
            .and_then(|parent| std::fs::metadata(parent).ok())
            .is_some_and(|meta| meta.is_file())
    } else {
        std::fs::metadata(value).is_ok_and(|meta| !meta.is_dir())
    }
}

/// `core.hooksPath=<`[`hooks_path`]`>`, built and checked once for the process; `None`: git is never started.
fn hooks_option() -> Option<&'static OsStr> {
    static OPTION: OnceLock<Option<OsString>> = OnceLock::new();
    OPTION
        .get_or_init(|| {
            let mut option = OsString::from("core.hooksPath=");
            option.push(hooks_path()?);
            Some(option)
        })
        .as_deref()
}

/// What a reader thread has read so far. The thread keeps filling it while the pipe is open; whoever waits for the
/// program takes what is there once the program is gone (a helper that inherited the pipe cannot hold us up).
#[derive(Default)]
struct Capture {
    data: Mutex<Vec<u8>>,
    done: AtomicBool,
}

impl Capture {
    fn take(&self) -> Vec<u8> {
        std::mem::take(&mut *self.data.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Waits a moment for the pipe to close after the program exited.
    fn settle(&self) {
        let started = Instant::now();
        while !self.done.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(1) {
            thread::sleep(Duration::from_millis(2));
        }
    }
}

fn spawn_reader(
    source: Option<impl Read + Send + 'static>,
    max: usize,
    overflow: Arc<AtomicBool>,
) -> Arc<Capture> {
    let capture = Arc::new(Capture::default());
    let sink = Arc::clone(&capture);
    thread::spawn(move || {
        if let Some(mut source) = source {
            let mut chunk = [0_u8; 16 * 1024];
            let mut total = 0_usize;
            loop {
                match source.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        total += n;
                        if total > max {
                            overflow.store(true, Ordering::SeqCst);
                            break;
                        }
                        sink.data
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .extend_from_slice(&chunk[..n]);
                    }
                }
            }
        }
        sink.done.store(true, Ordering::SeqCst);
    });
    capture
}

/// Runs `exe args` in `dir`.
///
/// # Errors
/// The program cannot start; it ran longer than `timeout` (killed); it printed more than `max` bytes (killed).
pub fn run(
    exe: &Path,
    dir: &Path,
    args: &[&OsStr],
    timeout: Duration,
    max: usize,
) -> Result<Output, GitError> {
    run_until(exe, dir, args, timeout, max, None)
}

/// [`run`] that also stops (the program killed) when `cancel` is set.
///
/// # Errors
/// As [`run`]; `Failed("stopped")` when cancelled.
pub fn run_until(
    exe: &Path,
    dir: &Path,
    args: &[&OsStr],
    timeout: Duration,
    max: usize,
    cancel: Option<&AtomicBool>,
) -> Result<Output, GitError> {
    if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
        return Err(GitError::Failed("stopped".to_owned()));
    }
    let hooks = hooks_option()
        .ok_or_else(|| GitError::Spawn("no path keeps the repository's hooks off".to_owned()))?;
    let mut all: Vec<&OsStr> = BASE.into_iter().map(OsStr::new).collect();
    all.extend([OsStr::new("-c"), hooks]);
    all.extend_from_slice(args);
    let mut child = Proc::start(
        exe,
        &permark_core::paths::simplified(dir),
        &all,
        &SET,
        &SCRUBBED,
    )
    .map_err(|e| GitError::Spawn(e.to_string()))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let out = spawn_reader(child.stdout.take(), max, Arc::clone(&overflow));
    let err = spawn_reader(
        child.stderr.take(),
        MAX_STDERR,
        Arc::new(AtomicBool::new(false)),
    );
    let started = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Wait::Ended(code)) => break code,
            Ok(Wait::Running) => {}
            Err(e) => return Err(GitError::Spawn(e.to_string())),
        }
        if overflow.load(Ordering::SeqCst) {
            child.kill();
            return Err(GitError::TooLarge(max));
        }
        if started.elapsed() > timeout {
            child.kill();
            return Err(GitError::Timeout);
        }
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            child.kill();
            return Err(GitError::Failed("stopped".to_owned()));
        }
        thread::sleep(Duration::from_millis(3));
    };
    out.settle();
    err.settle();
    if overflow.load(Ordering::SeqCst) {
        return Err(GitError::TooLarge(max));
    }
    Ok(Output {
        code,
        stdout: out.take(),
        stderr: String::from_utf8_lossy(&err.take()).trim().to_owned(),
    })
}

/// A started git: still running, or ended with this exit code (`None`: ended by a signal).
pub(crate) enum Wait {
    Running,
    Ended(Option<i32>),
}

/// Elsewhere git on `PATH` is the real program (no launcher): `std::process`, killed as is (see `tree.rs` for
/// Windows).
#[cfg(not(windows))]
struct Proc {
    child: std::process::Child,
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
}

#[cfg(not(windows))]
impl Proc {
    fn start(
        exe: &Path,
        dir: &Path,
        args: &[&OsStr],
        set: &[(&str, &str)],
        remove: &[&str],
    ) -> std::io::Result<Self> {
        let mut command = std::process::Command::new(exe);
        command
            .current_dir(dir)
            .args(args)
            .envs(set.iter().copied())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for name in remove {
            command.env_remove(name);
        }
        let mut child = command.spawn()?;
        Ok(Self {
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            child,
        })
    }

    fn try_wait(&mut self) -> std::io::Result<Wait> {
        Ok(self
            .child
            .try_wait()?
            .map_or(Wait::Running, |status| Wait::Ended(status.code())))
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(windows)]
use crate::tree::Proc;

pub struct SystemGit {
    exe: PathBuf,
    /// Set -> the running call is killed and later calls refuse (a member repository stopped).
    cancel: Option<Arc<AtomicBool>>,
}

impl std::fmt::Debug for SystemGit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemGit")
            .field("exe", &self.exe)
            .field("cancellable", &self.cancel.is_some())
            .finish()
    }
}

impl SystemGit {
    /// The git on `PATH` if it answers `--version`.
    #[must_use]
    pub fn find() -> Option<Self> {
        Self::at(find_git()?)
    }

    /// The git at `exe` if it answers `--version` with [`MIN_VERSION`] or later, and the repository's hooks can be
    /// kept off here ([`hooks_option`]).
    #[must_use]
    pub fn at(exe: PathBuf) -> Option<Self> {
        hooks_option()?;
        let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let out = run(
            &exe,
            &here,
            &[OsStr::new("--version")],
            Duration::from_secs(5),
            4096,
        )
        .ok()?;
        (out.code == Some(0) && version_of(&out.stdout).is_some_and(|v| v >= MIN_VERSION))
            .then_some(Self { exe, cancel: None })
    }

    fn call(&self, dir: &Path, args: &[&str]) -> Result<Output, GitError> {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        run_until(
            &self.exe,
            dir,
            &args,
            TIMEOUT,
            MAX_OUTPUT,
            self.cancel.as_deref(),
        )
    }

    /// Text of a call that must succeed; `None` when git answered with a non-zero code (no such revision, no
    /// upstream, no commit yet).
    fn text_or_none(&self, dir: &Path, args: &[&str]) -> Result<Option<String>, GitError> {
        let out = self.call(dir, args)?;
        Ok((out.code == Some(0)).then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()))
    }

    /// `-c` options that switch off the commands a config adds: the filter drivers the repository's own config sets
    /// (see [`repo_filter_drivers`]) and every hook a config defines (see [`config_hooks`]). A config that cannot be
    /// read | a hook that cannot be named -> error (the call that needs it is not made).
    fn repo_commands_off(&self, dir: &Path) -> Result<Vec<String>, GitError> {
        let out = self.call(
            dir,
            &[
                "config",
                "--show-scope",
                "-z",
                "--get-regexp",
                r"^(filter\..*\.(clean|smudge|process)|hook\..*\.(command|event))$",
            ],
        )?;
        match out.code {
            Some(0) => {
                let hooks = config_hooks(&out.stdout).ok_or_else(|| {
                    GitError::Failed("a hook of the config cannot be switched off".to_owned())
                })?;
                let mut off = filters_off(&repo_filter_drivers(&out.stdout));
                off.extend(
                    hooks
                        .iter()
                        .flat_map(|name| ["-c".to_owned(), format!("hook.{name}.enabled=false")]),
                );
                Ok(off)
            }
            // no filter driver | hook set anywhere
            Some(1) => Ok(Vec::new()),
            _ => Err(GitError::Failed(first_line(&out.stderr))),
        }
    }

    /// The file `path` (relative to the project folder) in the tree of `revision`, as stored (`cat-file`: no filter,
    /// no line-ending conversion). `revision` is `HEAD` | the digits of a [`CommitId`] + `^{commit}`, never a
    /// caller's text: it cannot start an option, and git splits `<revision>:<path>` at the first colon outside
    /// braces, so the path stays a path whatever it holds (it is looked up in the tree: no glob, no pathspec magic).
    /// `None`: git answered with a non-zero code (no such revision | file, a folder, an id that leads to no commit,
    /// an object that is not on this computer).
    fn blob(
        &self,
        location: &RepoLocation,
        revision: &str,
        path: &str,
    ) -> Result<Option<Vec<u8>>, GitError> {
        if path.contains('\0') {
            return Ok(None);
        }
        let spec = format!("{revision}:{}", repo_path(&location.prefix, path));
        let args = [
            OsStr::new("cat-file"),
            OsStr::new("blob"),
            OsStr::new(&spec),
        ];
        let out = run_until(
            &self.exe,
            &location.workdir,
            &args,
            TIMEOUT,
            MAX_BLOB,
            self.cancel.as_deref(),
        )?;
        Ok((out.code == Some(0)).then_some(out.stdout))
    }

    /// The path (relative to the working tree) the file at `path` had before `commit` renamed it; `None`: that
    /// commit renamed nothing to it | git failed | the commit changed more than [`MAX_RENAME_PATHS`] paths (the
    /// rule of the built-in backend, so both say the same of a commit).
    fn renamed_in(&self, dir: &Path, commit: CommitId, path: &str) -> Option<String> {
        // the limit as an option: `diff.renameLimit` of the repository's config does not count
        let limit = format!("-l{MAX_RENAME_PATHS}");
        let mut args = RENAMES_OF.to_vec();
        args.extend([limit.as_str(), commit.as_str()]);
        let out = self.call_ok(dir, &args).ok()?;
        let changes = parse_name_status(&out.stdout).ok()?;
        // a rename is two paths, the one that went and the one that came
        let renames = changes
            .iter()
            .filter(|entry| entry.letter == Letter::Renamed)
            .count();
        if changes.len() + renames > MAX_RENAME_PATHS {
            return None;
        }
        changes
            .into_iter()
            .find(|entry| entry.letter == Letter::Renamed && entry.path == path)?
            .from
    }

    fn call_ok(&self, dir: &Path, args: &[&str]) -> Result<Output, GitError> {
        let out = self.call(dir, args)?;
        if out.code == Some(0) {
            Ok(out)
        } else {
            Err(GitError::Failed(first_line(&out.stderr)))
        }
    }

    /// HEAD from two small calls (the commit baseline lists changes with other commands).
    fn head_info(&self, location: &RepoLocation) -> Result<HeadInfo, GitError> {
        let dir = &location.workdir;
        let verify = self.call(dir, &["rev-parse", "-q", "--verify", "HEAD"])?;
        let oid = (verify.code == Some(0))
            .then(|| String::from_utf8_lossy(&verify.stdout).trim().to_owned())
            .filter(|oid| CommitId::parse(oid).is_some());
        let branch = self.call(dir, &["symbolic-ref", "-q", "--short", "HEAD"])?;
        let name = (branch.code == Some(0))
            .then(|| String::from_utf8_lossy(&branch.stdout).trim().to_owned())
            .filter(|name| !name.is_empty());
        Ok(HeadInfo {
            detached: name.is_none() && oid.is_some(),
            unborn: oid.is_none(),
            branch: name,
            oid,
        })
    }
}

/// `(scope, key)` of each entry of `git config --show-scope -z --get-regexp` output (a value follows the key after a
/// line break).
fn config_keys(stdout: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut fields = stdout.split(|b| *b == 0);
    std::iter::from_fn(move || {
        let (scope, entry) = (fields.next()?, fields.next()?);
        let key = entry.split(|b| *b == b'\n').next().unwrap_or_default();
        Some((scope, key))
    })
}

/// `<name>` of the key `<section>.<name>.<variable>` (`section` given with its dot); the name may hold dots.
fn subsection<'a>(key: &'a [u8], section: &str) -> Option<&'a [u8]> {
    let rest = key.strip_prefix(section.as_bytes())?;
    let dot = rest.iter().rposition(|b| *b == b'.')?;
    Some(&rest[..dot])
}

/// Names of the filter drivers (`filter.<name>.clean | smudge | process`) set by the repository's own config (`local`
/// | `worktree` scope) in `git config --show-scope -z --get-regexp` output. A folder that came with its `.git/`
/// decides these commands, and `status` | `diff` would run them: they are switched off for every such call.
pub(crate) fn repo_filter_drivers(stdout: &[u8]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (scope, key) in config_keys(stdout) {
        if scope != b"local" && scope != b"worktree" {
            continue;
        }
        let Some(name) = subsection(key, "filter.") else {
            continue;
        };
        let name = String::from_utf8_lossy(name);
        if !name.is_empty() && !names.iter().any(|n| *n == name) {
            names.push(name.into_owned());
        }
    }
    names
}

/// Names of the hooks a config defines (`hook.<name>.command | event`, every scope) in `git config --show-scope -z
/// --get-regexp` output. git 2.54+ runs them whatever `core.hooksPath` says, so each is switched off by its name.
/// `None`: a name that cannot be passed back exactly in `-c hook.<name>.enabled=false` (not UTF-8; a `=`, which ends
/// the key; a control character).
pub(crate) fn config_hooks(stdout: &[u8]) -> Option<Vec<String>> {
    let mut names: Vec<String> = Vec::new();
    for (_, key) in config_keys(stdout) {
        let Some(name) = subsection(key, "hook.") else {
            continue;
        };
        let name = std::str::from_utf8(name).ok()?;
        if name.contains(|c: char| c == '=' || c.is_control()) {
            return None;
        }
        if !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
        }
    }
    Some(names)
}

/// `(major, minor)` of a `git --version` answer (`git version 2.54.0.windows.1`); `None`: no such line.
pub(crate) fn version_of(stdout: &[u8]) -> Option<(u32, u32)> {
    let text = std::str::from_utf8(stdout).ok()?;
    let rest = text.trim().strip_prefix("git version ")?;
    let mut numbers = rest.split(|c: char| !c.is_ascii_digit());
    Some((numbers.next()?.parse().ok()?, numbers.next()?.parse().ok()?))
}

/// `-c filter.<name>.clean=` (and smudge, process, required=false) for each driver of [`repo_filter_drivers`].
fn filters_off(names: &[String]) -> Vec<String> {
    names
        .iter()
        .flat_map(|name| {
            ["clean=", "smudge=", "process=", "required=false"]
                .into_iter()
                .flat_map(move |rest| ["-c".to_owned(), format!("filter.{name}.{rest}")])
        })
        .collect()
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_owned()
}

fn raw(prefix: &str, entries: Vec<Entry>) -> Vec<RawFile> {
    entries
        .into_iter()
        .filter_map(|entry| {
            let path = below_prefix(prefix, &entry.path)?.to_owned();
            let from = entry
                .from
                .and_then(|from| below_prefix(prefix, &from).map(str::to_owned));
            Some(RawFile {
                path,
                letter: entry.letter,
                staged: entry.staged,
                from,
            })
        })
        .collect()
}

impl GitOps for SystemGit {
    fn kind(&self) -> BackendKind {
        BackendKind::System
    }

    fn discover(&self, root: &Path, home: Option<&Path>) -> Discovery {
        let out = match self.call(
            root,
            &[
                "rev-parse",
                "--show-toplevel",
                "--absolute-git-dir",
                "--show-prefix",
            ],
        ) {
            Ok(out) => out,
            Err(error) => return Discovery::Error(error.to_string()),
        };
        if out.code != Some(0) {
            let message = out.stderr.to_lowercase();
            return if message.contains("not a git repository") || message.contains("work tree") {
                Discovery::NoRepo
            } else if message.contains("dubious ownership") {
                Discovery::Error("dubious ownership".to_owned())
            } else {
                Discovery::Error(first_line(&out.stderr))
            };
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = text.lines();
        let (Some(top), Some(git_dir)) = (lines.next(), lines.next()) else {
            return Discovery::Error("unreadable answer of git".to_owned());
        };
        let prefix = lines.next().unwrap_or("").trim_end_matches('/').to_owned();
        let workdir = PathBuf::from(top);
        if too_broad(&workdir, root, home) {
            return Discovery::NoRepo;
        }
        // keep the prefix git computed, unless it disagrees with the folders (links)
        let prefix = if prefix_of(&workdir, root).is_some_and(|ours| ours == prefix) {
            prefix
        } else {
            match prefix_of(&workdir, root) {
                Some(ours) => ours,
                None => return Discovery::NoRepo,
            }
        };
        if !prefix.is_empty() {
            let ignored = self.call(&workdir, &["check-ignore", "-q", "--", &prefix]);
            if ignored.is_ok_and(|o| o.code == Some(0)) {
                return Discovery::NoRepo;
            }
        }
        Discovery::Repo(RepoLocation {
            workdir,
            git_dir: PathBuf::from(git_dir),
            prefix,
        })
    }

    fn status(&self, location: &RepoLocation, baseline: &Baseline) -> Result<RawStatus, GitError> {
        let spec = pathspec(&location.prefix);
        let dir = &location.workdir;
        // the repository's own filter commands and the hooks of any config never run; submodules are compared by
        // commit only (their work trees would run their own config's commands)
        let off = self.repo_commands_off(dir)?;
        let off: Vec<&str> = off.iter().map(String::as_str).collect();
        match baseline {
            Baseline::Head => {
                let mut args = off.clone();
                args.extend([
                    "status",
                    "--porcelain=v2",
                    "-z",
                    "--branch",
                    "--untracked-files=all",
                    "--ignore-submodules=dirty",
                    "--",
                ]);
                if let Some(spec) = &spec {
                    args.push(spec);
                }
                let out = self.call_ok(dir, &args)?;
                let parsed = parse_status(&out.stdout)?;
                let (files, truncated) = finish(raw(&location.prefix, parsed.entries));
                Ok(RawStatus {
                    head: parsed.head,
                    files,
                    truncated,
                })
            }
            Baseline::Commit(oid) => {
                // a full id only: never a short one, a ref name, an option or a revision expression
                let Some(commit) = CommitId::parse(oid) else {
                    return Err(GitError::Failed("not a commit id".to_owned()));
                };
                let head = self.head_info(location)?;
                let mut diff = off.clone();
                diff.extend([
                    "diff",
                    "--name-status",
                    "-z",
                    "--no-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--ignore-submodules=dirty",
                    commit.as_str(),
                    "--",
                ]);
                let mut others = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
                if let Some(spec) = &spec {
                    diff.push(spec);
                    others.push(spec);
                }
                let mut entries = parse_name_status(&self.call_ok(dir, &diff)?.stdout)?;
                entries.extend(parse_untracked(&self.call_ok(dir, &others)?.stdout));
                let (files, truncated) = finish(raw(&location.prefix, entries));
                Ok(RawStatus {
                    head,
                    files,
                    truncated,
                })
            }
        }
    }

    fn blob_at_head(
        &self,
        location: &RepoLocation,
        path: &str,
    ) -> Result<Option<Vec<u8>>, GitError> {
        self.blob(location, "HEAD", path)
    }

    fn blob_at(
        &self,
        location: &RepoLocation,
        commit: CommitId,
        path: &str,
    ) -> Result<Option<Vec<u8>>, GitError> {
        // `^{commit}`: git reads `<tree id>:<path>` too, and a tree is any folder of the repository (also one
        // outside the project folder); only an id that leads to a commit is read from
        self.blob(location, &format!("{}^{{commit}}", commit.as_str()), path)
    }

    fn tracked_under(&self, location: &RepoLocation, path: &str) -> Result<bool, GitError> {
        let at = repo_path(&location.prefix, path);
        // a literal (no pattern) that also matches other cases; which ones count is decided below. A pathspec ignores
        // `core.ignorecase`, so without `icase` a caseless repository's differently cased entry would not be listed.
        let spec = format!(":(literal,icase){at}");
        let args = ["ls-files", "-z", "--cached", "--", spec.as_str()].map(OsStr::new);
        let out = match run_until(
            &self.exe,
            &location.workdir,
            &args,
            TIMEOUT,
            MAX_LISTED,
            self.cancel.as_deref(),
        ) {
            // more entries than the cap: some are there; counted as tracked rather than read whole
            Err(GitError::TooLarge(_)) => return Ok(true),
            other => other?,
        };
        if out.code != Some(0) {
            return Err(GitError::Failed(first_line(&out.stderr)));
        }
        let entries: Vec<&[u8]> = out
            .stdout
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
            .collect();
        if entries.iter().any(|entry| tracked_entry(entry, &at, false)) {
            return Ok(true);
        }
        if entries.is_empty() {
            return Ok(false);
        }
        // only other cases listed: they count when the repository is caseless
        let caseless = self.call(
            &location.workdir,
            &["config", "--bool", "--get", "core.ignorecase"],
        )?;
        let caseless = match caseless.code {
            Some(0) => caseless.stdout.trim_ascii() == b"true",
            // not set: git compares with case
            Some(1) => false,
            _ => return Err(GitError::Failed(first_line(&caseless.stderr))),
        };
        Ok(caseless && entries.iter().any(|entry| tracked_entry(entry, &at, true)))
    }

    fn file_versions(
        &self,
        location: &RepoLocation,
        path: &str,
        after: usize,
        limit: usize,
    ) -> Result<FileVersions, GitError> {
        let mut found = FileVersions {
            cut: shallow(&location.git_dir),
            ..FileVersions::default()
        };
        if path.is_empty() || path.contains('\0') {
            return Ok(found);
        }
        let dir = &location.workdir;
        let full = repo_path(&location.prefix, path);
        // after `--`, as a literal: the path is no option, no pattern, no pathspec magic
        let spec = format!(":(literal){full}");
        let limit = limit.clamp(1, MAX_VERSIONS);
        // git reads the number as an `int` and refuses a larger one: no history holds that many commits, so the
        // largest it takes is past the end all the same
        let skip = format!("--skip={}", after.min(MAX_SKIP));
        // one change more than the page holds: whether the history goes on is known without another call
        let count = (limit + 1).to_string();
        let mut args = VERSIONS_LOG.to_vec();
        args.extend([
            skip.as_str(),
            "-n",
            count.as_str(),
            "HEAD",
            "--",
            spec.as_str(),
        ]);
        let out = self.call(dir, &args)?;
        if out.code != Some(0) {
            // a branch without a commit has nothing to list; anything else is a failure
            let head = self.head_info(location)?;
            return if head.unborn && head.branch.is_some() {
                Ok(found)
            } else {
                Err(GitError::Failed(first_line(&out.stderr)))
            };
        }
        let mut entries = parse_versions_log(&out.stdout, &full);
        if entries.len() > limit {
            entries.truncate(limit);
            found.next = Some(after.saturating_add(limit));
        }
        let oldest = entries.iter().rev().find(|entry| entry.version.is_some());
        if let (
            None,
            Some(LogEntry {
                version: Some(row),
                added: true,
            }),
        ) = (found.next, oldest)
        {
            // the end of the list: a file that came with its oldest version may have come under another name
            found.renamed_from = self
                .renamed_in(dir, row.commit, &full)
                .and_then(|old| below_prefix(&location.prefix, &old).map(str::to_owned));
        }
        found.rows = entries.into_iter().filter_map(|e| e.version).collect();
        let tree = self.call(dir, &["ls-tree", "-z", "HEAD", "--", spec.as_str()])?;
        if tree.code == Some(0) {
            found.head_blob = parse_tree_file(&tree.stdout, &full);
        }
        Ok(found)
    }

    fn commit_count(&self, location: &RepoLocation, old: &str, new: &str) -> Option<u32> {
        let (Some(old), Some(new)) = (CommitId::parse(old), CommitId::parse(new)) else {
            return None;
        };
        let range = format!("{old}..{new}");
        let out = self
            .call_ok(&location.workdir, &["rev-list", "--count", &range])
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    fn commit_exists(&self, location: &RepoLocation, oid: &str) -> bool {
        let Some(oid) = CommitId::parse(oid) else {
            return false;
        };
        let spec = format!("{oid}^{{commit}}");
        self.call(&location.workdir, &["cat-file", "-e", &spec])
            .is_ok_and(|out| out.code == Some(0))
    }

    fn ahead_behind(&self, location: &RepoLocation) -> Result<Option<(u32, u32)>, GitError> {
        let dir = &location.workdir;
        // `@{upstream}` of the branch HEAD names; none (or detached) -> no answer
        let Some(upstream) = self
            .text_or_none(
                dir,
                &["rev-parse", "-q", "--verify", "@{upstream}^{commit}"],
            )?
            .filter(|oid| CommitId::parse(oid).is_some())
        else {
            return Ok(None);
        };
        let range = format!("HEAD...{upstream}");
        let Some(text) =
            self.text_or_none(dir, &["rev-list", "--left-right", "--count", &range, "--"])?
        else {
            return Ok(None);
        };
        let mut parts = text.split_whitespace().map(str::parse::<u32>);
        match (parts.next(), parts.next()) {
            (Some(Ok(ahead)), Some(Ok(behind))) => Ok(Some((ahead, behind))),
            _ => Err(GitError::Parse(text)),
        }
    }

    fn last_commit(&self, location: &RepoLocation) -> Result<Option<(i64, String)>, GitError> {
        // the raw commit object: no pretty-printing, so no signature check (`log.showSignature` would run gpg)
        let out = self.call(&location.workdir, &["cat-file", "commit", "HEAD"])?;
        if out.code != Some(0) {
            return Ok(None);
        }
        Ok(parse_commit_object(&String::from_utf8_lossy(&out.stdout)))
    }

    fn root_commit(&self, location: &RepoLocation) -> Result<Option<String>, GitError> {
        let Some(text) = self.text_or_none(
            &location.workdir,
            &["rev-list", "--max-parents=0", "HEAD", "--"],
        )?
        else {
            return Ok(None);
        };
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|l| CommitId::parse(l).is_some())
            .min()
            .map(str::to_owned))
    }

    fn cancellable(&self, flag: &Arc<AtomicBool>) -> Option<Arc<dyn GitOps>> {
        Some(Arc::new(Self {
            exe: self.exe.clone(),
            cancel: Some(Arc::clone(flag)),
        }))
    }
}

/// Committer time (ms) + subject of a raw commit object (`git cat-file commit`); `None`: unreadable.
pub(crate) fn parse_commit_object(text: &str) -> Option<(i64, String)> {
    let (headers, message) = text.split_once("\n\n").unwrap_or((text, ""));
    let committer = headers.lines().find_map(|l| l.strip_prefix("committer "))?;
    // `Name <mail> 1700000000 +0100`: the time is the second field from the end
    let mut fields = committer.rsplitn(3, ' ');
    let _zone = fields.next()?;
    let seconds: i64 = fields.next()?.parse().ok()?;
    Some((
        seconds.saturating_mul(1000),
        crate::ops::subject_of(message),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_gives_major_and_minor() {
        assert_eq!(version_of(b"git version 2.54.0.windows.1\n"), Some((2, 54)));
        assert_eq!(
            version_of(b"git version 2.39.3 (Apple Git-146)\n"),
            Some((2, 39))
        );
        assert!(version_of(b"git version 2.8.4\n").is_some_and(|v| v < MIN_VERSION));
        assert!(version_of(b"git version 2.9.0").is_some_and(|v| v >= MIN_VERSION));
        assert!(version_of(b"git version 10.1").is_some_and(|v| v >= MIN_VERSION));
        for odd in [
            b"".as_slice(),
            b"hg version 2.54",
            b"git version x",
            b"git version 2",
        ] {
            assert_eq!(version_of(odd), None, "{odd:?}");
        }
    }

    #[test]
    fn every_call_points_git_at_a_hooks_path_nothing_sits_below() {
        let option = hooks_option().unwrap().to_string_lossy().into_owned();
        let value = option.strip_prefix("core.hooksPath=").unwrap();
        assert!(Path::new(value).is_absolute(), "{value}");
        assert!(nothing_below(Path::new(value)), "{value}");
        if cfg!(windows) {
            assert!(value.ends_with(r"\no-hooks"), "{value}");
            assert!(!value.starts_with(r"\\"), "{value}");
        } else {
            assert_eq!(value, "/dev/null");
        }
    }

    #[cfg(windows)]
    #[test]
    fn the_hooks_path_lies_below_a_file_named_by_a_plain_drive_path() {
        let exe = std::env::current_exe().unwrap();
        let plain = permark_core::paths::simplified(&exe).into_owned();
        let expected = plain.join("no-hooks");
        assert_eq!(hooks_path_below(&exe), Some(expected.clone()));
        let verbatim = PathBuf::from(format!(r"\\?\{}", plain.display()));
        assert_eq!(hooks_path_below(&verbatim), Some(expected));
        let dir = tempfile::tempdir().unwrap();
        let folder = dunce::canonicalize(dir.path()).unwrap();
        for refused in [
            PathBuf::from("permark.exe"),
            PathBuf::from("C:permark.exe"),
            PathBuf::from(r"\\server\share\permark.exe"),
            PathBuf::from(format!(r"\\.\{}", plain.display())),
            PathBuf::from(r"\\?\GLOBALROOT\Device\HarddiskVolume1\permark.exe"),
            folder.join("missing.exe"),
            // a folder: files can sit below it
            folder,
        ] {
            assert_eq!(hooks_path_below(&refused), None, "{}", refused.display());
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_null_device_is_a_place_nothing_sits_below() {
        assert!(nothing_below(Path::new("/dev/null")));
        assert!(!nothing_below(Path::new("dev/null")));
        assert!(!nothing_below(Path::new("/")));
        assert!(!nothing_below(Path::new("/no/such/place")));
    }
}
