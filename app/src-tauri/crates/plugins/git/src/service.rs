//! The git worker: one thread owns the questions to git. Triggers (watch batches, git signals, window focus,
//! baseline) are coalesced (300 ms of quiet, at most 2 s), wait out `index.lock`, and make one snapshot; it is
//! announced only when its content differs. Discovery runs on the worker too: opening a project never waits for git.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, PoisonError, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use permark_core::conditions::GitCondition;
use permark_core::paths::WORKTREE_PREFIX;
use permark_core::plugins::{ProjectFacts, RepoReader};
use permark_core::settings::GitBackendPref;
use permark_core::state::{StateData, StateFile};
use serde::{Deserialize, Serialize};

use crate::builtin::BuiltinGit;
use crate::ops::{
    finish, repo_state, Baseline, Discovery, GitError, GitOps, RawFile, RepoLocation,
};
use crate::system::SystemGit;
use crate::types::{BaselineInfo, BaselineKind, GitFile, GitSnapshot, WorktreeInfo};
use crate::worktrees::branch_of;

/// The person's choice of baseline (`prefs.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefs {
    pub baseline: BaselineKind,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            baseline: BaselineKind::LastVisit,
        }
    }
}

impl StateData for Prefs {
    const VERSION: u32 = 1;
}

/// Where HEAD was when the project was last closed (`visit.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visit {
    pub head: Option<String>,
}

impl StateData for Visit {
    const VERSION: u32 = 1;
}

/// How the worker waits.
#[derive(Debug, Clone, Copy)]
pub struct Timings {
    /// A trigger waits this long for more.
    pub quiet: Duration,
    /// ... but never longer than this in all.
    pub max_wait: Duration,
    /// A pending refresh waits this long for `index.lock` to go.
    pub lock_wait: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(300),
            max_wait: Duration::from_secs(2),
            lock_wait: Duration::from_millis(1500),
        }
    }
}

/// Which backend: by the person's setting, or fixed (tests).
#[derive(Clone)]
pub enum BackendChoice {
    Pref(GitBackendPref),
    Fixed(Arc<dyn GitOps>),
}

/// Why a refresh is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Files changed.
    Files,
    /// `.git/HEAD | index | refs` changed.
    Signal,
    /// The window got focus.
    Focus,
}

enum Msg {
    Refresh(Why),
    Baseline(BaselineKind),
    Stop,
}

#[derive(Default)]
struct Pending {
    signal: bool,
    focus: bool,
    forced: bool,
}

impl Pending {
    fn merge(&mut self, why: Why) {
        match why {
            Why::Files => {}
            Why::Signal => self.signal = true,
            Why::Focus => self.focus = true,
        }
    }

    fn why(&self) -> Why {
        if self.signal {
            Why::Signal
        } else if self.focus {
            Why::Focus
        } else {
            Why::Files
        }
    }
}

/// What the worker shares with the callers.
#[derive(Default)]
struct Shared {
    snapshot: RwLock<Option<Arc<GitSnapshot>>>,
    repo: RwLock<Option<Repo>>,
}

/// A backend with the working trees it answers for.
struct Repo {
    backend: Arc<dyn GitOps>,
    main: RepoLocation,
    worktrees: Vec<(String, RepoLocation)>,
}

/// What "since your last visit" is measured from, fixed at the first look of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Session {
    Unset,
    /// The commit stored at the last close (if the repository still has it), else HEAD at the first look.
    Commit(String),
    /// The session started in a repository without commits: letters are measured against HEAD.
    Empty,
}

/// Told every announced snapshot (`None`: there is no repository now).
pub type Announce = Arc<dyn Fn(Option<&GitSnapshot>) + Send + Sync>;

pub struct ServiceArgs {
    pub root: PathBuf,
    /// `(name, folder)` of the linked worktrees that are roots of the project.
    pub worktrees: Vec<(String, PathBuf)>,
    pub choice: BackendChoice,
    pub home: Option<PathBuf>,
    pub timings: Timings,
    pub facts: Arc<ProjectFacts>,
    /// Called with every announced snapshot (`None`: no repository now).
    pub announce: Announce,
    pub prefs: Option<StateFile<Prefs>>,
    pub visit: Option<StateFile<Visit>>,
}

pub struct GitService {
    shared: Arc<Shared>,
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for GitService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitService").finish_non_exhaustive()
    }
}

impl GitService {
    #[must_use]
    pub fn start(args: ServiceArgs) -> Self {
        let shared = Arc::new(Shared::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let prefs = args
            .prefs
            .as_ref()
            .map(|file| file.load().into_value())
            .unwrap_or_default();
        let stored_visit = args
            .visit
            .as_ref()
            .and_then(|file| file.load().into_value().head);
        let worker = Worker {
            args,
            shared: Arc::clone(&shared),
            stop: Arc::clone(&stop),
            baseline: prefs.baseline,
            stored_visit,
            session: Session::Unset,
            last_head: None,
            looked: false,
            backend: None,
            main: None,
            worktree_locations: Vec::new(),
        };
        let thread = thread::Builder::new()
            .name("permark-git".to_owned())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || worker.run(&rx))
            .ok();
        Self {
            shared,
            tx,
            stop,
            thread,
        }
    }

    /// The latest announced snapshot.
    #[must_use]
    pub fn snapshot(&self) -> Option<Arc<GitSnapshot>> {
        self.shared
            .snapshot
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Asks for a refresh (coalesced).
    pub fn trigger(&self, why: Why) {
        let _ = self.tx.send(Msg::Refresh(why));
    }

    /// A cheap, thread-safe way to ask for refreshes (a watch subscription holds one).
    pub fn trigger_fn(&self) -> impl Fn(Why) + Send + Sync + 'static {
        let tx = self.tx.clone();
        move |why| {
            let _ = tx.send(Msg::Refresh(why));
        }
    }

    /// Changes what the letters are measured against (remembered).
    pub fn set_baseline(&self, kind: BaselineKind) {
        let _ = self.tx.send(Msg::Baseline(kind));
    }

    /// The repository reader other plugins use (HEAD blobs).
    #[must_use]
    pub fn reader(&self) -> Arc<dyn RepoReader> {
        Arc::new(Reader {
            shared: Arc::clone(&self.shared),
        })
    }
}

impl Drop for GitService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Reader {
    shared: Arc<Shared>,
}

impl RepoReader for Reader {
    fn blob_at_head(&self, path: &str) -> Option<Vec<u8>> {
        let guard = self
            .shared
            .repo
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        let repo = guard.as_ref()?;
        let (location, rest) = match path.strip_prefix(WORKTREE_PREFIX) {
            Some(tail) => {
                let (name, rest) = tail.split_once('/')?;
                let (_, location) = repo.worktrees.iter().find(|(n, _)| n == name)?;
                (location, rest)
            }
            None => (&repo.main, path),
        };
        repo.backend.blob_at_head(location, rest).ok().flatten()
    }

    fn head(&self) -> Option<String> {
        self.shared
            .snapshot
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|s| s.head.oid.clone())
    }

    fn commits_between(&self, old: &str, new: &str) -> Option<u32> {
        let guard = self
            .shared
            .repo
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        let repo = guard.as_ref()?;
        repo.backend.commit_count(&repo.main, old, new)
    }
}

/// The number of the last snapshot announced by any service of the process. One counter for all, so a service that
/// starts again (the backend setting changed) never announces a number below what the window already has.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Worker {
    args: ServiceArgs,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    baseline: BaselineKind,
    stored_visit: Option<String>,
    session: Session,
    last_head: Option<String>,
    /// A discovery has run (a folder found without a repository is not looked at again for plain file changes).
    looked: bool,
    backend: Option<Arc<dyn GitOps>>,
    main: Option<RepoLocation>,
    worktree_locations: Vec<(String, RepoLocation)>,
}

impl Worker {
    fn run(mut self, rx: &Receiver<Msg>) {
        self.refresh(Why::Signal);
        while let Ok(msg) = rx.recv() {
            let mut pending = Pending::default();
            match msg {
                Msg::Stop => break,
                Msg::Baseline(kind) => {
                    self.set_baseline(kind);
                    pending.forced = true;
                }
                Msg::Refresh(why) => pending.merge(why),
            }
            let first = Instant::now();
            while !pending.forced {
                let left = self
                    .args
                    .timings
                    .quiet
                    .min(self.args.timings.max_wait.saturating_sub(first.elapsed()));
                if left.is_zero() {
                    break;
                }
                match rx.recv_timeout(left) {
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => {
                        self.finish();
                        return;
                    }
                    Ok(Msg::Baseline(kind)) => {
                        self.set_baseline(kind);
                        pending.forced = true;
                    }
                    Ok(Msg::Refresh(why)) => pending.merge(why),
                    Err(RecvTimeoutError::Timeout) => break,
                }
            }
            if self.stop.load(Ordering::SeqCst) || !self.wait_out_lock(rx) {
                break;
            }
            self.refresh(pending.why());
        }
        self.finish();
    }

    /// Git is writing the index: its answer would race the write. Waits a little; `false`: asked to stop.
    fn wait_out_lock(&self, rx: &Receiver<Msg>) -> bool {
        let Some(main) = &self.main else { return true };
        let lock = main.git_dir.join("index.lock");
        let started = Instant::now();
        while lock.exists() && started.elapsed() < self.args.timings.lock_wait {
            if self.stop.load(Ordering::SeqCst) {
                return false;
            }
            // whatever arrives meanwhile only repeats what is already pending
            match rx.recv_timeout(Duration::from_millis(40)) {
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            }
        }
        true
    }

    fn set_baseline(&mut self, kind: BaselineKind) {
        if self.baseline == kind {
            return;
        }
        self.baseline = kind;
        if let Some(file) = &self.args.prefs {
            let _ = file.save(&Prefs { baseline: kind });
        }
    }

    /// The project closes: where HEAD is now is where the next visit starts.
    fn finish(&self) {
        if let (Some(file), Some(head)) = (&self.args.visit, &self.last_head) {
            let _ = file.save(&Visit {
                head: Some(head.clone()),
            });
        }
    }

    fn choose_backend(&self) -> Option<Arc<dyn GitOps>> {
        let system = || SystemGit::find().map(|s| Arc::new(s) as Arc<dyn GitOps>);
        let builtin = || Arc::new(BuiltinGit) as Arc<dyn GitOps>;
        match &self.args.choice {
            BackendChoice::Fixed(backend) => Some(Arc::clone(backend)),
            BackendChoice::Pref(GitBackendPref::Builtin) => Some(builtin()),
            BackendChoice::Pref(GitBackendPref::System) => system(),
            BackendChoice::Pref(GitBackendPref::Automatic) => {
                Some(system().unwrap_or_else(builtin))
            }
        }
    }

    fn discover(&mut self, backend: &Arc<dyn GitOps>) -> Discovery {
        let home = self.args.home.as_deref();
        let found = backend.discover(&self.args.root, home);
        self.looked = true;
        self.worktree_locations.clear();
        self.main = None;
        if let Discovery::Repo(location) = &found {
            self.main = Some(location.clone());
            for (name, folder) in &self.args.worktrees {
                if let Discovery::Repo(wt) = backend.discover(folder, home) {
                    self.worktree_locations.push((name.clone(), wt));
                }
            }
        }
        found
    }

    fn refresh(&mut self, why: Why) {
        if self.backend.is_none() {
            self.backend = self.choose_backend();
        }
        let Some(backend) = self.backend.clone() else {
            self.args.facts.set_git(GitCondition::Unavailable);
            self.publish(None);
            return;
        };
        let gone = self
            .main
            .as_ref()
            .is_some_and(|main| !main.git_dir.exists());
        if self.main.is_none() || gone {
            // a folder without a repository is looked at again for signals and focus, not for every file change
            if self.looked && !gone && why == Why::Files {
                return;
            }
            match self.discover(&backend) {
                Discovery::Repo(_) => {}
                Discovery::NoRepo => {
                    self.args.facts.set_git(GitCondition::NoRepo);
                    self.publish(None);
                    return;
                }
                Discovery::Error(reason) => {
                    self.args.facts.set_git(GitCondition::Error { reason });
                    self.publish(None);
                    return;
                }
            }
        }
        let Some(main) = self.main.clone() else {
            return;
        };
        match self.snapshot_of(&backend, &main) {
            Ok(snapshot) => {
                *self
                    .shared
                    .repo
                    .write()
                    .unwrap_or_else(PoisonError::into_inner) = Some(Repo {
                    backend,
                    main,
                    worktrees: self.worktree_locations.clone(),
                });
                self.args.facts.set_git(GitCondition::Repo {
                    state: snapshot.state,
                });
                self.publish(Some(snapshot));
            }
            Err(error) => {
                self.args.facts.set_git(GitCondition::Error {
                    reason: error.to_string(),
                });
                self.publish(None);
            }
        }
    }

    fn snapshot_of(
        &mut self,
        backend: &Arc<dyn GitOps>,
        main: &RepoLocation,
    ) -> Result<GitSnapshot, GitError> {
        let measured_against = match (&self.session, self.baseline) {
            (Session::Commit(oid), BaselineKind::LastVisit)
                if self.last_head.as_deref() != Some(oid.as_str()) =>
            {
                Some(oid.clone())
            }
            _ => None,
        };
        let mut compared = measured_against.clone();
        let mut status = backend.status(
            main,
            &measured_against.map_or(Baseline::Head, Baseline::Commit),
        )?;
        if self.session == Session::Unset {
            // first look: the stored visit if the repository still has it, else where HEAD is now
            let start = self
                .stored_visit
                .clone()
                .filter(|oid| backend.commit_exists(main, oid))
                .or_else(|| status.head.oid.clone());
            self.session = start.clone().map_or(Session::Empty, Session::Commit);
            if let Some(oid) = start {
                if self.baseline == BaselineKind::LastVisit
                    && status.head.oid.as_deref() != Some(oid.as_str())
                {
                    status = backend.status(main, &Baseline::Commit(oid.clone()))?;
                    compared = Some(oid);
                }
            }
        }
        self.last_head.clone_from(&status.head.oid);

        let mut files: Vec<RawFile> = status.files;
        let mut truncated = status.truncated;
        let mut infos = Vec::new();
        for (name, location) in &self.worktree_locations {
            let branch = branch_of(&location.git_dir);
            if let Ok(wt) = backend.status(location, &Baseline::Head) {
                truncated |= wt.truncated;
                let prefix = format!("{WORKTREE_PREFIX}{name}/");
                files.extend(wt.files.into_iter().map(|file| RawFile {
                    path: format!("{prefix}{}", file.path),
                    from: file.from.map(|from| format!("{prefix}{from}")),
                    ..file
                }));
                infos.push(WorktreeInfo {
                    name: name.clone(),
                    branch: wt.head.branch.or(branch),
                });
            } else {
                infos.push(WorktreeInfo {
                    name: name.clone(),
                    branch,
                });
            }
        }
        let (files, cut) = finish(files);
        Ok(GitSnapshot {
            seq: 0,
            backend: backend.kind(),
            head: status.head,
            state: repo_state(&main.git_dir),
            baseline: BaselineInfo {
                kind: self.baseline,
                oid: compared,
            },
            files: files
                .into_iter()
                .map(|file| GitFile {
                    path: file.path,
                    letter: file.letter,
                    staged: file.staged,
                    from: file.from,
                })
                .collect(),
            truncated: truncated || cut,
            worktrees: infos,
        })
    }

    /// Stores and announces `snapshot` when its content differs from the last one (`None`: no repository).
    fn publish(&mut self, snapshot: Option<GitSnapshot>) {
        let mut current = self
            .shared
            .snapshot
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(mut next) = snapshot {
            if current.as_deref().is_some_and(|c| c.same_content(&next)) {
                return;
            }
            next.seq = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
            let arc = Arc::new(next);
            *current = Some(Arc::clone(&arc));
            drop(current);
            (self.args.announce)(Some(&arc));
        } else {
            if current.is_none() {
                return;
            }
            *current = None;
            drop(current);
            *self
                .shared
                .repo
                .write()
                .unwrap_or_else(PoisonError::into_inner) = None;
            self.last_head = None;
            (self.args.announce)(None);
        }
    }
}
