//! Model runtime of one plugin instance: ONE [`WorkerEngine`] whose workers = the pinned worker program
//! ([`WorkerFiles::exe`]) started by core [`process::spawn_piped`] (cwd = its unit folder), stderr drained into a
//! capped log; sessions tracked per model lineage.
//!
//! - Dormant (AC-8): building one spawns, threads, creates nothing; first `load` starts a worker (+ its stderr
//!   thread), the log file is written when a worker's stderr ends.
//! - Loads serialized (one at a time) with the memory guard ([`guard::check`]) on a profile read now, limited to the
//!   load's device (processor only: graphics memory never counted).
//! - [`Runtime::release`] (D-11, Windows mmap lock): every session of a lineage unloaded (= its workers exit) before
//!   its files are removed | moved; a load of it in flight is cancelled (its token) + waited for (no session missed).
//!   The token a released session was loaded with is cancelled too (its owner's work, e.g. a model test, stops).
//!   [`Runtime::release_all`] on store change, another device chosen + plugin off.
//! - Load options read once the load is registered ([`Runtime::load`]): a state they follow (the device chosen)
//!   changes + releases -> that load is cancelled | read the new state, never ends on the old one.
//! - Worker stop: stdin closed (+ `unload` exit message): the worker hard-exits on stdin EOF (even mid-load);
//!   [`STOP_GRACE`] only for a wedged one, then kill + wait.

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
/// Longest stderr line taken at once (a longer one is split).
const LINE_CAP: u64 = 4 << 10;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Workers' stderr: last [`LOG_CAP`] bytes in memory, saved (atomic) to the log file each time a worker's stderr
/// ends (exit | crash).
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
        } = process::spawn_piped(&exe, &[], &cwd)?;
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
    /// Session -> lineage of its model + the token it was loaded with.
    sessions: Mutex<HashMap<SessionId, (String, CancelToken)>>,
    /// Loads asked for and not ended (lineage, token): [`Runtime::release`] cancels them.
    pending: Mutex<Vec<(String, CancelToken)>>,
    /// One load at a time (guard reads memory with no other load in flight); held by a release while it waits.
    loading: Mutex<()>,
    /// Memory now (guard input).
    machine: fn() -> MachineProfile,
}

impl Runtime {
    /// Runtime over the pinned worker of `files`; stderr into `log`.
    #[must_use]
    pub fn new(files: Arc<WorkerFiles>, log: Arc<WorkerLog>) -> Self {
        Self::with_launch(worker_launch(files, log), crate::machine::detect)
    }

    /// Runtime over `launch` with memory read by `machine` (tests).
    #[must_use]
    pub fn with_launch(launch: Launch, machine: fn() -> MachineProfile) -> Self {
        Self {
            engine: WorkerEngine::new(launch),
            sessions: Mutex::new(HashMap::new()),
            pending: Mutex::new(Vec::new()),
            loading: Mutex::new(()),
            machine,
        }
    }

    /// The engine, for calls on a loaded session (load | unload through [`Runtime`]).
    #[must_use]
    pub fn engine(&self) -> &dyn Engine {
        &self.engine
    }

    /// Loads `model` of `lineage` after the memory guard on `needs` (room = what the options' device may use);
    /// answers the session + the options it was loaded with.
    ///
    /// `opts` is read once the load is registered, no runtime lock held. Options following a state whose change
    /// releases sessions (the device the person chose) MUST be read there, not before the call: a change before the
    /// read is then the one read, a later one finds this load to cancel | its session to unload. Read before the
    /// call, a change in between cancels nothing and the load ends on the options just left.
    ///
    /// # Errors
    /// [`EngineError::OutOfMemory`] (guard, no worker started) | [`EngineError::Cancelled`] (`cancel` | released
    /// meanwhile) | the engine's load errors.
    pub fn load(
        &self,
        lineage: &str,
        needs: &Needs,
        model: ModelFiles,
        opts: impl FnOnce() -> LoadOpts,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<(SessionId, LoadOpts), EngineError> {
        lock(&self.pending).push((lineage.to_owned(), cancel.clone()));
        // registered above; outside the load section: no lock held over the caller's read
        let opts = opts();
        let loaded = {
            let _one = lock(&self.loading);
            let loaded = guard::check(needs, &(self.machine)().limited_to(opts.device))
                .and_then(|()| self.engine.load(model, opts, progress, cancel));
            if let Ok(session) = loaded {
                lock(&self.sessions).insert(session, (lineage.to_owned(), cancel.clone()));
            }
            loaded.map(|session| (session, opts))
        };
        lock(&self.pending).retain(|(_, token)| !std::ptr::eq(token.flag(), cancel.flag()));
        loaded
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

    /// Picked loads in flight cancelled + waited for, then picked sessions unloaded (their tokens cancelled).
    fn unload_where(&self, pick: impl Fn(&str) -> bool) {
        for (lineage, token) in lock(&self.pending).iter() {
            if pick(lineage) {
                token.cancel();
            }
        }
        // in-flight load ended (its session, if any, listed below); no new load until done
        let _wait = lock(&self.loading);
        let picked: Vec<(SessionId, CancelToken)> = {
            let mut sessions = lock(&self.sessions);
            let picked: Vec<SessionId> = sessions
                .iter()
                .filter(|(_, (l, _))| pick(l))
                .map(|(s, _)| *s)
                .collect();
            picked
                .into_iter()
                .filter_map(|s| sessions.remove(&s).map(|(_, token)| (s, token)))
                .collect()
        };
        for (session, token) in picked {
            token.cancel();
            self.engine.unload(session);
        }
    }

    /// Lineages with a loaded session (sorted, once each).
    #[cfg(test)]
    pub(crate) fn loaded(&self) -> Vec<String> {
        let mut lineages: Vec<String> = lock(&self.sessions)
            .values()
            .map(|(l, _)| l.clone())
            .collect();
        lineages.sort();
        lineages.dedup();
        lineages
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.release_all();
    }
}

#[cfg(test)]
mod tests;
