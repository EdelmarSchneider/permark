//! Folder count: the tokens of every text file below a folder, as one number with its parts.
//!
//! A job per window, on a thread of its own, that can be cancelled. Before it reads anything it looks at the listing of
//! the folder (file count, bytes) and asks the person first when the folder is big, or refuses when it is too big. The
//! count of every file is remembered (size, modified time, content hash, tokenizer) so a repeat count reads only what
//! changed; the project's file watch marks results stale when something under their folder changes. Nothing is written
//! in the project, and a job that was cancelled leaves no result.

mod cache;
mod guard;
mod job;
mod summary;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;

use permark_core::paths::VaultPath;
use permark_core::plugins::{typed, AppServices, NoArgs, PluginCx, PluginEvents, RpcRegistration};
use permark_core::watch::{ChangeKind, FsBatch, Subscription};
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::tokenizer::Tokenizer;
use crate::vocab::VocabStore;
use cache::FileCache;
pub use guard::{FolderRefusal, FolderSkip, Thresholds};
pub use summary::{FolderSkipped, FolderSummary, FolderTopFile, FolderTypeRow};

/// Projects whose results are kept (the least recently used goes first).
const PROJECTS_KEPT: usize = 8;

/// Why a job ended without a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FolderFailure {
    /// The folder is not there (any more), or is outside the project.
    NotFound,
    /// The window shows no project.
    Unavailable,
    /// The job could not run (no thread, a panic).
    Io,
}

/// A window's job: what it is doing, as it is announced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum FolderJob {
    Idle,
    #[serde(rename_all = "camelCase")]
    Scanning {
        folder: String,
    },
    /// Big folder: the person is asked first.
    #[serde(rename_all = "camelCase")]
    Confirm {
        folder: String,
        files: u32,
        #[ts(type = "number")]
        bytes: u64,
        /// Expected time, whole seconds.
        seconds: u32,
    },
    /// Too big: not counted.
    #[serde(rename_all = "camelCase")]
    Refused {
        folder: String,
        /// Found before the walk stopped (more than this, when `reason` is `files`).
        files: u32,
        #[ts(type = "number")]
        bytes: u64,
        reason: FolderRefusal,
    },
    #[serde(rename_all = "camelCase")]
    Counting {
        folder: String,
        done: u32,
        total: u32,
        tokenizer: String,
        approx: bool,
    },
    #[serde(rename_all = "camelCase")]
    Done {
        folder: String,
    },
    #[serde(rename_all = "camelCase")]
    Cancelled {
        folder: String,
    },
    #[serde(rename_all = "camelCase")]
    Failed {
        folder: String,
        reason: FolderFailure,
    },
}

impl FolderJob {
    /// The job is working (or waiting for the person's answer): a new one cannot start.
    fn busy(&self) -> bool {
        match self {
            Self::Scanning { .. } | Self::Confirm { .. } | Self::Counting { .. } => true,
            Self::Idle
            | Self::Refused { .. }
            | Self::Done { .. }
            | Self::Cancelled { .. }
            | Self::Failed { .. } => false,
        }
    }
}

/// The badge of a counted folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderBadge {
    pub folder: String,
    #[ts(type = "number")]
    pub tokens: u64,
    pub approx: bool,
    /// Something under the folder changed since it was counted.
    pub stale: bool,
    pub tokenizer: String,
}

/// The counted folders of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderBadges {
    pub root: String,
    pub folders: Vec<FolderBadge>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartArgs {
    pub folder: String,
    pub tokenizer: String,
    /// A quiet job for the totals shown automatically: never asks, ends silently when the folder is not small.
    #[serde(default)]
    pub auto: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootArgs {
    pub root: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryArgs {
    pub root: String,
    pub folder: String,
}

/// The scan of a folder that waits for the person's answer.
struct Held {
    request: job::Request,
    plan: job::Plan,
    project: String,
}

struct Slot {
    id: u64,
    job: FolderJob,
    cancel: Arc<AtomicBool>,
    held: Option<Held>,
    thread: Option<JoinHandle<()>>,
}

struct Stored {
    summary: Arc<FolderSummary>,
    stale: bool,
}

/// A job counting a folder of the project now: told when something under it changes.
struct Run {
    job: u64,
    folder: String,
    touched: bool,
}

struct Project {
    cache: Arc<Mutex<FileCache>>,
    results: BTreeMap<String, Stored>,
    sub: Option<Subscription>,
    runs: Vec<Run>,
}

#[derive(Default)]
struct Projects {
    by_root: HashMap<String, Project>,
    /// Least recently used first.
    order: Vec<String>,
}

impl Projects {
    fn touch(&mut self, root: &str) {
        self.order.retain(|r| r != root);
        self.order.push(root.to_owned());
    }

    /// The project's state, made (and the least recent one dropped past the limit) when there is none.
    fn project(&mut self, root: &str, make: impl FnOnce() -> Project) -> &mut Project {
        if !self.by_root.contains_key(root) {
            while self.by_root.len() >= PROJECTS_KEPT {
                let idle = self
                    .order
                    .iter()
                    .position(|r| self.by_root.get(r).is_none_or(|p| p.runs.is_empty()));
                let Some(at) = idle else { break };
                let gone = self.order.remove(at);
                self.by_root.remove(&gone);
            }
            self.by_root.insert(root.to_owned(), make());
        }
        self.touch(root);
        self.by_root
            .get_mut(root)
            .unwrap_or_else(|| unreachable!("inserted above"))
    }
}

/// The calls of the folder count, served while the plugin runs.
pub fn register(cx: &PluginCx, folders: &Arc<Folders>) -> Vec<RpcRegistration> {
    let mut rpc = Vec::new();
    let f = Arc::clone(folders);
    rpc.extend(cx.rpc(
        "folderStart",
        typed(move |args: StartArgs, call| f.start(&call.window, &args)),
    ));
    let f = Arc::clone(folders);
    rpc.extend(cx.rpc(
        "folderConfirm",
        typed(move |_: NoArgs, call| Ok(f.confirm(&call.window))),
    ));
    let f = Arc::clone(folders);
    rpc.extend(cx.rpc(
        "folderCancel",
        typed(move |_: NoArgs, call| Ok(f.cancel(&call.window))),
    ));
    let f = Arc::clone(folders);
    rpc.extend(cx.rpc(
        "folderDismiss",
        typed(move |_: NoArgs, call| Ok(f.dismiss(&call.window))),
    ));
    let f = Arc::clone(folders);
    rpc.extend(cx.rpc(
        "folderJob",
        typed(move |_: NoArgs, call| Ok(f.job(&call.window))),
    ));
    let f = Arc::clone(folders);
    rpc.extend(cx.rpc(
        "folders",
        typed(move |args: RootArgs, _| Ok(f.badges(&args.root))),
    ));
    let f = Arc::clone(folders);
    rpc.extend(cx.rpc(
        "folderSummary",
        typed(move |args: SummaryArgs, _| {
            Ok(f.summary(&args.root, &args.folder).map(|s| (*s).clone()))
        }),
    ));
    rpc
}

/// Everything the folder count shares: the job slots, the results and caches of the projects.
pub struct Folders {
    me: Weak<Folders>,
    events: PluginEvents,
    app: AppServices,
    vocab: Arc<VocabStore>,
    thresholds: Thresholds,
    /// Where the per-file caches are written.
    dir: PathBuf,
    /// Tests: microseconds each file takes beyond its work, so a job can be caught half way.
    #[cfg(test)]
    pace: AtomicU64,
    jobs: Mutex<HashMap<String, Slot>>,
    projects: Mutex<Projects>,
    next_job: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `folder` as the plugin keeps it: a normalized project path.
fn normal(folder: &str) -> Result<String, IpcError> {
    Ok(VaultPath::parse(folder)?.as_str().to_owned())
}

/// Whether `path` is `folder` or below it (`""` holds everything).
fn covers(folder: &str, path: &str) -> bool {
    folder.is_empty()
        || path == folder
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn failure_of(error: &IpcError) -> FolderFailure {
    match error.kind {
        ErrorKind::NotFound | ErrorKind::PathEscape | ErrorKind::InvalidPath => {
            FolderFailure::NotFound
        }
        ErrorKind::Unsupported if error.subject.as_deref() == Some("project") => {
            FolderFailure::Unavailable
        }
        ErrorKind::Unsupported => FolderFailure::NotFound,
        ErrorKind::Unavailable => FolderFailure::Unavailable,
        _ => FolderFailure::Io,
    }
}

impl Folders {
    #[must_use]
    pub fn new(
        events: PluginEvents,
        app: AppServices,
        vocab: Arc<VocabStore>,
        thresholds: Thresholds,
        dir: PathBuf,
    ) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            me: me.clone(),
            events,
            app,
            vocab,
            thresholds,
            dir,
            #[cfg(test)]
            pace: AtomicU64::new(0),
            jobs: Mutex::new(HashMap::new()),
            projects: Mutex::new(Projects::default()),
            next_job: AtomicU64::new(1),
        })
    }

    // ---- the window's job ----------------------------------------------------------------------------------------

    /// The job of `window` now.
    #[must_use]
    pub fn job(&self, window: &str) -> FolderJob {
        lock(&self.jobs)
            .get(window)
            .map_or(FolderJob::Idle, |slot| slot.job.clone())
    }

    /// Starts counting `args.folder` for `window`. While a job works the answer is its state, unchanged.
    ///
    /// # Errors
    /// A tokenizer or folder that is not valid; no thread to run on.
    pub fn start(&self, window: &str, args: &StartArgs) -> Result<FolderJob, IpcError> {
        let tokenizer = Tokenizer::parse(&args.tokenizer)
            .ok_or_else(|| IpcError::new(ErrorKind::InvalidData).detail("tokenizer"))?;
        let folder = normal(&args.folder)?;
        let Some(me) = self.me.upgrade() else {
            return Err(IpcError::new(ErrorKind::Unavailable));
        };
        let request = job::Request {
            window: window.to_owned(),
            folder: folder.clone(),
            tokenizer,
            auto: args.auto,
        };
        let finished;
        let job = FolderJob::Scanning { folder };
        {
            // announced under the lock: the job's thread cannot report before this does
            let mut jobs = lock(&self.jobs);
            if let Some(slot) = jobs.get(window) {
                if slot.job.busy() {
                    return Ok(slot.job.clone());
                }
            }
            let id = self.next_job.fetch_add(1, Ordering::Relaxed);
            let cancel = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&cancel);
            let thread = std::thread::Builder::new()
                .name("token-count-folder".to_owned())
                .spawn(move || job::scan_phase(&me, id, &request, &flag))
                .map_err(|e| IpcError::io(String::new(), &e))?;
            finished = jobs
                .insert(
                    window.to_owned(),
                    Slot {
                        id,
                        job: job.clone(),
                        cancel,
                        held: None,
                        thread: Some(thread),
                    },
                )
                .and_then(|old| old.thread);
            self.announce(window, &job);
        }
        join(finished);
        Ok(job)
    }

    /// The person agreed to a big folder.
    pub fn confirm(&self, window: &str) -> FolderJob {
        let Some(me) = self.me.upgrade() else {
            return self.job(window);
        };
        let (job, finished) = {
            let mut jobs = lock(&self.jobs);
            let Some(slot) = jobs.get_mut(window) else {
                return FolderJob::Idle;
            };
            if !matches!(slot.job, FolderJob::Confirm { .. }) {
                return slot.job.clone();
            }
            let Some(held) = slot.held.take() else {
                return slot.job.clone();
            };
            let id = slot.id;
            let flag = Arc::clone(&slot.cancel);
            let folder = held.request.folder.clone();
            let started = job::counting_state(&held.request, &held.plan, &self.vocab);
            let spawned = std::thread::Builder::new()
                .name("token-count-folder".to_owned())
                .spawn(move || job::count_phase(&me, id, held, &flag));
            let (job, finished) = if let Ok(thread) = spawned {
                slot.job = started.clone();
                (started, slot.thread.replace(thread))
            } else {
                slot.job = FolderJob::Failed {
                    folder,
                    reason: FolderFailure::Io,
                };
                (slot.job.clone(), None)
            };
            self.announce(window, &job);
            (job, finished)
        };
        join(finished);
        job
    }

    /// Stops the job: a waiting one is dropped at once, a working one ends at its next file.
    pub fn cancel(&self, window: &str) -> FolderJob {
        let job = {
            let mut jobs = lock(&self.jobs);
            let Some(slot) = jobs.get_mut(window) else {
                return FolderJob::Idle;
            };
            slot.cancel.store(true, Ordering::Relaxed);
            if !matches!(slot.job, FolderJob::Confirm { .. }) {
                return slot.job.clone();
            }
            slot.held = None;
            slot.job = FolderJob::Idle;
            self.announce(window, &slot.job);
            slot.job.clone()
        };
        job
    }

    /// The person has seen how the job ended.
    pub fn dismiss(&self, window: &str) -> FolderJob {
        let job = {
            let mut jobs = lock(&self.jobs);
            let Some(slot) = jobs.get_mut(window) else {
                return FolderJob::Idle;
            };
            if slot.job.busy() {
                return slot.job.clone();
            }
            slot.job = FolderJob::Idle;
            self.announce(window, &slot.job);
            slot.job.clone()
        };
        job
    }

    /// A job reports: sets the state when the slot is still the job's own (`false`: it is not, stop working).
    fn set(&self, window: &str, id: u64, job: FolderJob) -> bool {
        let mut jobs = lock(&self.jobs);
        match jobs.get_mut(window) {
            Some(slot) if slot.id == id => {
                self.announce(window, &job);
                slot.job = job;
                true
            }
            _ => false,
        }
    }

    fn hold(&self, window: &str, id: u64, held: Held, job: FolderJob) {
        let mut jobs = lock(&self.jobs);
        if let Some(slot) = jobs.get_mut(window) {
            if slot.id == id {
                self.announce(window, &job);
                slot.held = Some(held);
                slot.job = job;
            }
        }
    }

    /// Tells the windows. Called with the job slots locked, so the announcements keep the order of the states.
    fn announce(&self, window: &str, job: &FolderJob) {
        if let Ok(job) = serde_json::to_value(job) {
            self.events.emit(
                "folder",
                serde_json::json!({ "window": window, "job": job }),
            );
        }
    }

    // ---- results ---------------------------------------------------------------------------------------------------

    /// The counted folders of a project, as badges.
    #[must_use]
    pub fn badges(&self, root: &str) -> FolderBadges {
        let projects = lock(&self.projects);
        FolderBadges {
            root: root.to_owned(),
            folders: projects
                .by_root
                .get(root)
                .map(|p| {
                    p.results
                        .iter()
                        .map(|(folder, stored)| FolderBadge {
                            folder: folder.clone(),
                            tokens: stored.summary.tokens,
                            approx: stored.summary.approx,
                            stale: stored.stale,
                            tokenizer: stored.summary.tokenizer.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    #[must_use]
    pub fn summary(&self, root: &str, folder: &str) -> Option<Arc<FolderSummary>> {
        let folder = normal(folder).ok()?;
        lock(&self.projects)
            .by_root
            .get(root)?
            .results
            .get(&folder)
            .map(|stored| Arc::clone(&stored.summary))
    }

    fn announce_badges(&self, root: &str) {
        if let Ok(badges) = serde_json::to_value(self.badges(root)) {
            self.events.emit("folders", badges);
        }
    }

    /// The project's verified changes: files that changed are read again, results above them are stale.
    fn changed(&self, root: &str, batch: &FsBatch) {
        let told = {
            let mut projects = lock(&self.projects);
            let Some(project) = projects.by_root.get_mut(root) else {
                return;
            };
            let everything = batch.rescan || batch.bulk;
            {
                let mut cache = lock(&project.cache);
                for change in &batch.changes {
                    let mut paths = vec![change.path.as_str()];
                    if let ChangeKind::Renamed { from } = &change.kind {
                        paths.push(from.as_str());
                    }
                    for path in paths {
                        cache.drop_path(path);
                        if change.is_dir || matches!(change.kind, ChangeKind::Renamed { .. }) {
                            cache.drop_below(path);
                        }
                    }
                }
            }
            let touches = |folder: &str| {
                everything || batch.changes.iter().any(|c| {
                    covers(folder, &c.path)
                        || matches!(&c.kind, ChangeKind::Renamed { from } if covers(folder, from))
                })
            };
            let mut told = false;
            for (folder, stored) in &mut project.results {
                if !stored.stale && touches(folder) {
                    stored.stale = true;
                    told = true;
                }
            }
            for run in &mut project.runs {
                run.touched |= touches(&run.folder);
            }
            told
        };
        if told {
            self.announce_badges(root);
        }
    }

    // ---- teardown --------------------------------------------------------------------------------------------------

    /// Plugin off | app exit: every job ends and its thread is joined, the watches go, the results are forgotten.
    pub fn shutdown(&self) {
        let threads: Vec<JoinHandle<()>> = {
            let mut jobs = lock(&self.jobs);
            for slot in jobs.values() {
                slot.cancel.store(true, Ordering::Relaxed);
            }
            jobs.drain().filter_map(|(_, slot)| slot.thread).collect()
        };
        for thread in threads {
            let _ = thread.join();
        }
        let mut projects = lock(&self.projects);
        projects.by_root.clear();
        projects.order.clear();
    }
}

fn join(thread: Option<JoinHandle<()>>) {
    if let Some(thread) = thread {
        let _ = thread.join();
    }
}

#[cfg(test)]
mod tests;
