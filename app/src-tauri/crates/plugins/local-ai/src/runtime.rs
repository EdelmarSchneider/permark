//! Model runtime of one plugin instance: ONE [`WorkerEngine`] whose workers = the pinned worker program
//! ([`WorkerFiles::exe`]) started by core [`process::spawn_piped`] (cwd = its unit folder), stderr drained into a
//! capped log; sessions tracked per model lineage.
//!
//! - Dormant (AC-8): building one spawns, threads, creates nothing; first `load` starts a worker (+ its stderr
//!   thread), the log file is written when a worker's stderr ends.
//! - Loads serialized (one at a time) with the memory guard ([`guard::check`]) on a profile read now.
//! - [`Runtime::release`] (D-11, Windows mmap lock): every session of a lineage unloaded (= its workers exit) before
//!   its files are removed | moved. [`Runtime::release_all`] on store change + plugin off.
//! - Worker stop: stdin closed (+ `unload` exit message), [`STOP_GRACE`] for its own exit, then kill + wait.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use permark_core::{fsx, process};

use crate::catalog::Needs;
use crate::engine::files::WorkerFiles;
use crate::engine::{
    CancelToken, Engine, EngineError, Launch, LoadOpts, ModelFiles, SessionId, WorkerEngine,
    WorkerPipes,
};
use crate::guard;
use crate::machine::MachineProfile;

/// Wait for a worker's own exit before it is killed.
pub const STOP_GRACE: Duration = Duration::from_secs(2);
/// Worker output kept for the log file, bytes.
const LOG_CAP: usize = 256 << 10;
/// Worker output kept in memory (crash messages), bytes.
const TAIL_CAP: usize = 8 << 10;
/// Longest stderr line taken at once (a longer one is split).
const LINE_CAP: u64 = 4 << 10;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Workers' stderr: last [`LOG_CAP`] bytes in memory, saved (atomic) to the log file each time a worker's stderr
/// ends (exit | crash); [`WorkerLog::tail`] = last [`TAIL_CAP`] bytes (crash messages).
pub struct WorkerLog {
    path: PathBuf,
    inner: Mutex<Lines>,
}

#[derive(Default)]
struct Lines {
    lines: VecDeque<String>,
    bytes: usize,
}

impl WorkerLog {
    /// Log at `path` (nothing created until a worker's stderr ends).
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            inner: Mutex::new(Lines::default()),
        }
    }

    /// Takes one worker line (oldest dropped past [`LOG_CAP`]).
    fn add(&self, line: &str) {
        let mut inner = lock(&self.inner);
        inner.bytes += line.len() + 1;
        inner.lines.push_back(line.to_owned());
        while inner.bytes > LOG_CAP {
            let Some(old) = inner.lines.pop_front() else {
                break;
            };
            inner.bytes -= old.len() + 1;
        }
    }

    /// Last worker lines within [`TAIL_CAP`] bytes (newest last).
    #[must_use]
    pub fn tail(&self) -> String {
        let inner = lock(&self.inner);
        let mut bytes = 0;
        let mut kept: Vec<&str> = inner
            .lines
            .iter()
            .rev()
            .take_while(|l| {
                bytes += l.len() + 1;
                bytes <= TAIL_CAP
            })
            .map(String::as_str)
            .collect();
        kept.reverse();
        kept.join("\n")
    }

    /// Writes the kept lines to the log file (best effort: a diagnostic only).
    fn save(&self) {
        let text = {
            let inner = lock(&self.inner);
            let mut text = String::with_capacity(inner.bytes);
            for line in &inner.lines {
                text.push_str(line);
                text.push('\n');
            }
            text
        };
        let made = self.path.parent().map_or(Ok(()), std::fs::create_dir_all);
        if made.is_ok() {
            let _ = fsx::write_atomic(&self.path, text.as_bytes());
        }
    }

    /// Drains `stderr` on a thread until it ends (worker gone), then saves.
    fn drain(self: &Arc<Self>, stderr: impl Read + Send + 'static) -> io::Result<()> {
        let log = Arc::clone(self);
        std::thread::Builder::new()
            .name("engine-log".into())
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    match reader.by_ref().take(LINE_CAP).read_until(b'\n', &mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let text = String::from_utf8_lossy(&line);
                            log.add(text.trim_end_matches(['\r', '\n']));
                        }
                    }
                }
                log.save();
            })
            .map(|_| ())
    }
}

/// Spawn failure of a resolved (installed, verified) worker: missing | refused now = taken by security software
/// (D-14) -> `PermissionDenied` (the proxy answers [`EngineError::Blocked`]).
fn blocked(error: io::Error) -> io::Error {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
            io::Error::new(io::ErrorKind::PermissionDenied, error)
        }
        _ => error,
    }
}

/// Launch of the pinned worker: spawned piped in its folder, stderr drained into `log`.
fn worker_launch(files: Arc<WorkerFiles>, log: Arc<WorkerLog>) -> Launch {
    Box::new(move || {
        let exe = files.exe()?;
        let cwd = exe
            .parent()
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "worker without a folder"))?;
        let process::Piped {
            stdin,
            stdout,
            stderr,
            mut child,
        } = process::spawn_piped(&exe, &[], &cwd).map_err(blocked)?;
        // no drain thread: dropping `child` kills the worker
        log.drain(stderr)?;
        Ok(WorkerPipes {
            to_worker: Box::new(stdin),
            from_worker: Box::new(BufReader::new(stdout)),
            kill: Box::new(move || {
                let _ = child.stop(STOP_GRACE);
            }),
        })
    })
}

/// Engine host of one plugin instance (rules: module doc).
pub struct Runtime {
    engine: WorkerEngine,
    /// Session -> lineage of its model.
    sessions: Mutex<HashMap<SessionId, String>>,
    /// One load at a time (guard reads memory with no other load in flight).
    loading: Mutex<()>,
    /// Memory now (guard input).
    machine: fn() -> MachineProfile,
    log: Arc<WorkerLog>,
}

impl Runtime {
    /// Runtime over the pinned worker of `files`; stderr into `log`.
    #[must_use]
    pub fn new(files: Arc<WorkerFiles>, log: Arc<WorkerLog>) -> Self {
        let launch = worker_launch(files, Arc::clone(&log));
        Self::with_launch(launch, crate::machine::detect, log)
    }

    /// Runtime over `launch` with memory read by `machine` (tests).
    #[must_use]
    pub fn with_launch(
        launch: Launch,
        machine: fn() -> MachineProfile,
        log: Arc<WorkerLog>,
    ) -> Self {
        Self {
            engine: WorkerEngine::new(launch),
            sessions: Mutex::new(HashMap::new()),
            loading: Mutex::new(()),
            machine,
            log,
        }
    }

    /// The engine, for calls on a loaded session (load | unload through [`Runtime`]).
    #[must_use]
    pub fn engine(&self) -> &dyn Engine {
        &self.engine
    }

    /// Loads `model` of `lineage` after the memory guard on `needs`.
    ///
    /// # Errors
    /// [`EngineError::OutOfMemory`] (guard, no worker started) | the engine's load errors.
    pub fn load(
        &self,
        lineage: &str,
        needs: &Needs,
        model: ModelFiles,
        opts: LoadOpts,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<SessionId, EngineError> {
        let _one = lock(&self.loading);
        guard::check(needs, &(self.machine)())?;
        let session = self.engine.load(model, opts, progress, cancel)?;
        lock(&self.sessions).insert(session, lineage.to_owned());
        Ok(session)
    }

    /// Unloads `session` (its worker exits).
    pub fn unload(&self, session: SessionId) {
        lock(&self.sessions).remove(&session);
        self.engine.unload(session);
    }

    /// Unloads every session of `lineage` (its files then free to remove | move).
    pub fn release(&self, lineage: &str) {
        self.unload_where(|l| l == lineage);
    }

    /// Unloads every session.
    pub fn release_all(&self) {
        self.unload_where(|_| true);
    }

    fn unload_where(&self, pick: impl Fn(&str) -> bool) {
        let picked: Vec<SessionId> = {
            let mut sessions = lock(&self.sessions);
            let picked = sessions
                .iter()
                .filter(|(_, l)| pick(l))
                .map(|(s, _)| *s)
                .collect::<Vec<_>>();
            for s in &picked {
                sessions.remove(s);
            }
            picked
        };
        for session in picked {
            self.engine.unload(session);
        }
    }

    /// Lineages with a loaded session (sorted, once each).
    #[must_use]
    pub fn loaded(&self) -> Vec<String> {
        let mut lineages: Vec<String> = lock(&self.sessions).values().cloned().collect();
        lineages.sort();
        lineages.dedup();
        lineages
    }

    /// Last worker output (crash messages).
    #[must_use]
    pub fn log_tail(&self) -> String {
        self.log.tail()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.release_all();
    }
}

#[cfg(test)]
mod tests;
