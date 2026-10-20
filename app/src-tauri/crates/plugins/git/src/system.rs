//! The installed git as a backend: the only program PerMark starts on its own. Absolute path found by walking `PATH`,
//! no shell, fixed arguments, a scrubbed environment, a timeout and a cap on the output.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::ops::{
    below_prefix, finish, pathspec, prefix_of, too_broad, Baseline, Discovery, GitError, GitOps,
    RawFile, RawStatus, RepoLocation,
};
use crate::porcelain::{parse_name_status, parse_status, parse_untracked, Entry};
use crate::types::{BackendKind, HeadInfo};

/// How long one git call may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Most bytes one call may print.
const MAX_OUTPUT: usize = 64 * 1024 * 1024;
/// Most bytes of a committed file.
const MAX_BLOB: usize = 8 * 1024 * 1024;
const MAX_STDERR: usize = 64 * 1024;

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
    let mut command = Command::new(exe);
    command
        .current_dir(dunce::simplified(dir))
        .args(["--no-pager", "-c", "core.fsmonitor=false"])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in SCRUBBED {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // no console window flashing up for each call
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| GitError::Spawn(e.to_string()))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let out = spawn_reader(child.stdout.take(), max, Arc::clone(&overflow));
    let err = spawn_reader(
        child.stderr.take(),
        MAX_STDERR,
        Arc::new(AtomicBool::new(false)),
    );
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return Err(GitError::Spawn(e.to_string())),
        }
        if overflow.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(GitError::TooLarge(max));
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(GitError::Timeout);
        }
        thread::sleep(Duration::from_millis(3));
    };
    out.settle();
    err.settle();
    if overflow.load(Ordering::SeqCst) {
        return Err(GitError::TooLarge(max));
    }
    Ok(Output {
        code: status.code(),
        stdout: out.take(),
        stderr: String::from_utf8_lossy(&err.take()).trim().to_owned(),
    })
}

/// A hex object id as git prints it (4..=64 digits): the only text of ours that reaches git as a revision.
fn is_hex_oid(text: &str) -> bool {
    (4..=64).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

pub struct SystemGit {
    exe: PathBuf,
}

impl std::fmt::Debug for SystemGit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemGit").field("exe", &self.exe).finish()
    }
}

impl SystemGit {
    /// The git on `PATH` if it answers `--version`.
    #[must_use]
    pub fn find() -> Option<Self> {
        Self::at(find_git()?)
    }

    /// The git at `exe` if it answers `--version`.
    #[must_use]
    pub fn at(exe: PathBuf) -> Option<Self> {
        let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let out = run(
            &exe,
            &here,
            &[OsStr::new("--version")],
            Duration::from_secs(5),
            4096,
        )
        .ok()?;
        (out.code == Some(0) && out.stdout.starts_with(b"git version")).then_some(Self { exe })
    }

    fn call(&self, dir: &Path, args: &[&str]) -> Result<Output, GitError> {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        run(&self.exe, dir, &args, TIMEOUT, MAX_OUTPUT)
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
            .filter(|oid| is_hex_oid(oid));
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
        match baseline {
            Baseline::Head => {
                let mut args = vec![
                    "status",
                    "--porcelain=v2",
                    "-z",
                    "--branch",
                    "--untracked-files=all",
                    "--",
                ];
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
                if !is_hex_oid(oid) {
                    return Err(GitError::Failed("not a commit id".to_owned()));
                }
                let head = self.head_info(location)?;
                let mut diff = vec![
                    "diff",
                    "--name-status",
                    "-z",
                    "--no-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    oid.as_str(),
                    "--",
                ];
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
        if path.contains('\0') {
            return Ok(None);
        }
        let full = if location.prefix.is_empty() {
            path.to_owned()
        } else {
            format!("{}/{path}", location.prefix)
        };
        let spec = format!("HEAD:{full}");
        let args = [
            OsStr::new("cat-file"),
            OsStr::new("blob"),
            OsStr::new(&spec),
        ];
        let out = run(&self.exe, &location.workdir, &args, TIMEOUT, MAX_BLOB)?;
        Ok((out.code == Some(0)).then_some(out.stdout))
    }

    fn commit_count(&self, location: &RepoLocation, old: &str, new: &str) -> Option<u32> {
        if !is_hex_oid(old) || !is_hex_oid(new) {
            return None;
        }
        let range = format!("{old}..{new}");
        let out = self
            .call_ok(&location.workdir, &["rev-list", "--count", &range])
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    fn commit_exists(&self, location: &RepoLocation, oid: &str) -> bool {
        if !is_hex_oid(oid) {
            return false;
        }
        let spec = format!("{oid}^{{commit}}");
        self.call(&location.workdir, &["cat-file", "-e", &spec])
            .is_ok_and(|out| out.code == Some(0))
    }
}
