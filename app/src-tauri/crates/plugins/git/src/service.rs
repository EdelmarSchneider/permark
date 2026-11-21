//! The git worker: one thread owns the questions to git. Triggers (watch batches, git signals, window focus,
//! baseline) are coalesced (300 ms of quiet, at most 2 s), wait out `index.lock`, and make one snapshot; it is
//! announced only when its content differs. Discovery runs on the worker too: opening a project never waits for git.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, UNIX_EPOCH};

use permark_core::conditions::GitCondition;
use permark_core::paths::WORKTREE_PREFIX;
use permark_core::plugins::{
    CommitId, FileVersions, NoBlob, NoTracking, NoVersions, ProjectFacts, RepoReader,
};
use permark_core::settings::GitBackendPref;
use permark_core::state::{StateData, StateFile};
use serde::{Deserialize, Serialize};

use crate::builtin::BuiltinGit;
use crate::config_scan::{Scan, Seen};
use crate::ops::{
    blob_or_miss, finish, repo_path, repo_state, tracked_of, tracked_question, versions_of,
    Baseline, Discovery, GitError, GitOps, RawFile, RawStatus, RepoLocation,
};
use crate::system::SystemGit;
use crate::types::{BackendKind, BaselineInfo, BaselineKind, GitFile, GitSnapshot, WorktreeInfo};
use crate::worktrees::{branch_of, git_dir_scan, opens_locally, NOT_LOCAL};

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
    /// After a status that did not answer in time, file changes wait this long before git is asked again; each
    /// timeout in a row doubles it, up to [`MAX_BACKOFF`].
    pub backoff: Duration,
}

/// The longest file changes wait for git after timeouts in a row.
pub const MAX_BACKOFF: Duration = Duration::from_secs(60);

impl Default for Timings {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(300),
            max_wait: Duration::from_secs(2),
            lock_wait: Duration::from_millis(1500),
            // as long as git was given to answer
            backoff: crate::system::TIMEOUT,
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
    /// Path keys of included projects with their own repository: the root status never lists their files
    /// (`projects.md` §8). `None`: not known yet (the worker asks before its first look).
    excluded: RwLock<Option<Vec<String>>>,
    /// The baseline in use (members measure against the same kind).
    baseline: RwLock<Option<BaselineKind>>,
    /// What the reader answers a tracked question with while no repository is held, from the worker's last look
    /// (`None`: no look yet): "no repository here" is told from "not looked at yet" and from "one is there that cannot
    /// answer".
    looked: RwLock<Option<NoTracking>>,
    /// Index answers of the reader.
    tracked: TrackedMemo,
}

/// Most git folders a [`LocalMemo`] keeps an answer for (a project's root, its worktrees, its members).
const MAX_LOCAL: usize = 64;

/// Answers of [`git_dir_scan`] kept while every path the look went by looks the same ([`Seen::unchanged`]): a few
/// stats per call instead of reading the config files again.
#[derive(Default)]
pub(crate) struct LocalMemo {
    kept: Mutex<HashMap<PathBuf, (Seen, bool)>>,
}

impl LocalMemo {
    /// Whether git may open the git folder `git_dir` now ([`git_dir_scan`]).
    pub(crate) fn stays_local(&self, git_dir: &Path) -> bool {
        // taken out while it is checked: the lock is not held during the stats (another caller looks for itself)
        let held = self
            .kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(git_dir);
        let (seen, answer) = match held {
            Some((seen, answer)) if seen.unchanged() => (seen, answer),
            _ => {
                let mut scan = Scan::new();
                let answer = git_dir_scan(git_dir, &mut scan);
                (scan.seen, answer)
            }
        };
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.len() >= MAX_LOCAL {
            kept.clear();
        }
        kept.insert(git_dir.to_path_buf(), (seen, answer));
        answer
    }
}

/// A backend that asks nothing of a repository git may not open without leaving this computer ([`git_dir_scan`]),
/// looked at again before every call: the files of its git folder may change while the project is open, and both
/// backends read them again on every open. Discovery is not looked at here: [`discover_here`] looks before.
pub(crate) struct LocalOnly {
    inner: Arc<dyn GitOps>,
    memo: Arc<LocalMemo>,
}

impl LocalOnly {
    fn wrap(inner: Arc<dyn GitOps>, memo: Arc<LocalMemo>) -> Arc<dyn GitOps> {
        Arc::new(Self { inner, memo })
    }

    fn check(&self, location: &RepoLocation) -> Result<(), GitError> {
        if self.memo.stays_local(&location.git_dir) {
            Ok(())
        } else {
            Err(GitError::Failed(NOT_LOCAL.to_owned()))
        }
    }
}

impl GitOps for LocalOnly {
    fn kind(&self) -> BackendKind {
        self.inner.kind()
    }

    fn discover(&self, root: &Path, home: Option<&Path>) -> Discovery {
        self.inner.discover(root, home)
    }

    fn status(&self, location: &RepoLocation, baseline: &Baseline) -> Result<RawStatus, GitError> {
        self.check(location)?;
        self.inner.status(location, baseline)
    }

    fn blob_at_head(
        &self,
        location: &RepoLocation,
        path: &str,
    ) -> Result<Option<Vec<u8>>, GitError> {
        self.check(location)?;
        self.inner.blob_at_head(location, path)
    }

    fn blob_at(
        &self,
        location: &RepoLocation,
        commit: CommitId,
        path: &str,
    ) -> Result<Option<Vec<u8>>, GitError> {
        self.check(location)?;
        self.inner.blob_at(location, commit, path)
    }

    fn file_versions(
        &self,
        location: &RepoLocation,
        path: &str,
        after: usize,
        limit: usize,
    ) -> Result<FileVersions, GitError> {
        self.check(location)?;
        self.inner.file_versions(location, path, after, limit)
    }

    fn tracked_under(&self, location: &RepoLocation, path: &str) -> Result<bool, GitError> {
        self.check(location)?;
        self.inner.tracked_under(location, path)
    }

    fn commit_exists(&self, location: &RepoLocation, oid: &str) -> bool {
        self.check(location).is_ok() && self.inner.commit_exists(location, oid)
    }

    fn commit_count(&self, location: &RepoLocation, old: &str, new: &str) -> Option<u32> {
        self.check(location).ok()?;
        self.inner.commit_count(location, old, new)
    }

    fn ahead_behind(&self, location: &RepoLocation) -> Result<Option<(u32, u32)>, GitError> {
        self.check(location)?;
        self.inner.ahead_behind(location)
    }

    fn last_commit(&self, location: &RepoLocation) -> Result<Option<(i64, String)>, GitError> {
        self.check(location)?;
        self.inner.last_commit(location)
    }

    fn root_commit(&self, location: &RepoLocation) -> Result<Option<String>, GitError> {
        self.check(location)?;
        self.inner.root_commit(location)
    }

    fn cancellable(&self, flag: &Arc<AtomicBool>) -> Option<Arc<dyn GitOps>> {
        let inner = self.inner.cancellable(flag)?;
        Some(Self::wrap(inner, Arc::clone(&self.memo)))
    }
}

/// The backend of a project, chosen once (the installed git answers `--version`, or the built-in one) and shared by
/// the root worker and the member pool: one `git --version` per plugin start. Every call of it looks at the
/// repository's git folder first ([`LocalOnly`]).
pub struct BackendCell {
    choice: BackendChoice,
    chosen: OnceLock<Option<Arc<dyn GitOps>>>,
}

impl BackendCell {
    #[must_use]
    pub fn new(choice: BackendChoice) -> Self {
        Self {
            choice,
            chosen: OnceLock::new(),
        }
    }

    /// A cell whose choice is made already (tests: `None` = no git).
    #[cfg(test)]
    pub(crate) fn decided(backend: Option<Arc<dyn GitOps>>) -> Self {
        Self {
            choice: BackendChoice::Pref(GitBackendPref::Builtin),
            chosen: OnceLock::from(backend),
        }
    }

    /// The backend (chosen at the first call; `None`: no git to ask).
    #[must_use]
    pub fn get(&self) -> Option<Arc<dyn GitOps>> {
        self.chosen
            .get_or_init(|| {
                let system = || SystemGit::find().map(|s| Arc::new(s) as Arc<dyn GitOps>);
                let builtin = || Arc::new(BuiltinGit) as Arc<dyn GitOps>;
                let chosen = match &self.choice {
                    BackendChoice::Fixed(backend) => Some(Arc::clone(backend)),
                    BackendChoice::Pref(GitBackendPref::Builtin) => Some(builtin()),
                    BackendChoice::Pref(GitBackendPref::System) => system(),
                    BackendChoice::Pref(GitBackendPref::Automatic) => {
                        Some(system().unwrap_or_else(builtin))
                    }
                };
                let memo = Arc::new(LocalMemo::default());
                chosen.map(|backend| LocalOnly::wrap(backend, memo))
            })
            .clone()
    }

    /// Whether the choice was made and found no git (without choosing).
    #[must_use]
    pub fn unavailable(&self) -> bool {
        matches!(self.chosen.get(), Some(None))
    }

    /// Whether the chosen backend's running calls can be killed (the installed git), without choosing.
    #[must_use]
    pub fn killable(&self) -> bool {
        self.chosen
            .get()
            .and_then(Option::as_ref)
            .is_some_and(|b| b.cancellable(&Arc::new(AtomicBool::new(false))).is_some())
    }
}

/// Whether the file `path` (project path) is at | below one of `excluded` (path keys).
pub(crate) fn excluded_path(excluded: &[String], path: &str) -> bool {
    if excluded.is_empty() || path.starts_with(WORKTREE_PREFIX) {
        return false;
    }
    let key = permark_core::paths::key_of(path.trim_end_matches('/'));
    excluded.iter().any(|e| {
        key == *e
            || key
                .strip_prefix(e.as_str())
                .is_some_and(|r| r.starts_with('/'))
    })
}

/// [`GitOps::discover`] of `folder`, unless opening its repository would make git look up a path that is not on this
/// computer ([`opens_locally`]: a `.git` file | `commondir` naming a share, which both backends follow): refused then.
pub(crate) fn discover_here(backend: &dyn GitOps, folder: &Path, home: Option<&Path>) -> Discovery {
    if !opens_locally(folder) {
        return Discovery::Refused;
    }
    backend.discover(folder, home)
}

/// A backend with the working trees it answers for.
struct Repo {
    backend: Arc<dyn GitOps>,
    main: RepoLocation,
    worktrees: Vec<(String, RepoLocation)>,
}

/// What "since your last visit" is measured from, fixed at the first look of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Session {
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
    /// Path keys of included projects with their own repository: left out of the very first status (`projects.md` §8;
    /// later changes: [`GitService::set_excluded`]).
    pub excluded: ExcludedAtStart,
}

/// Asked on the worker right before its very first look (the projects service may still be reading its members: the
/// open call never waits for them). A list set meanwhile ([`GitService::set_excluded`]) is newer and wins.
pub type ExcludedAtStart = Box<dyn FnOnce() -> Vec<String> + Send>;

fn sorted(mut keys: Vec<String>) -> Vec<String> {
    keys.sort();
    keys.dedup();
    keys
}

/// "Since the last visit" of one repository: the commit stored at the last close, the session's start, HEAD now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Visiting {
    pub stored: Option<String>,
    pub session: Session,
    pub last_head: Option<String>,
}

impl Visiting {
    /// `stored` comes from a state file of the folder: kept only as a full commit id (lowercase, as git prints it).
    pub(crate) fn new(stored: Option<String>) -> Self {
        Self {
            stored: stored
                .and_then(|oid| CommitId::parse(&oid))
                .map(|commit| commit.as_str().to_owned()),
            session: Session::Unset,
            last_head: None,
        }
    }
}

/// Status of `location` against `kind`, fixing the session's start at the first look (the stored visit when the
/// repository still has it, else HEAD now). Returns the status + the commit compared with (`None`: HEAD).
pub(crate) fn measured_status(
    backend: &dyn GitOps,
    location: &RepoLocation,
    kind: BaselineKind,
    visiting: &mut Visiting,
) -> Result<(crate::ops::RawStatus, Option<String>), GitError> {
    let measured_against = match (&visiting.session, kind) {
        (Session::Commit(oid), BaselineKind::LastVisit)
            if visiting.last_head.as_deref() != Some(oid.as_str()) =>
        {
            Some(oid.clone())
        }
        _ => None,
    };
    let mut compared = measured_against.clone();
    let mut status = backend.status(
        location,
        &measured_against.map_or(Baseline::Head, Baseline::Commit),
    )?;
    if visiting.session == Session::Unset {
        // first look: the stored visit if the repository still has it, else where HEAD is now
        let start = visiting
            .stored
            .clone()
            .filter(|oid| backend.commit_exists(location, oid))
            .or_else(|| status.head.oid.clone());
        visiting.session = start.clone().map_or(Session::Empty, Session::Commit);
        if let Some(oid) = start {
            if kind == BaselineKind::LastVisit && status.head.oid.as_deref() != Some(oid.as_str()) {
                status = backend.status(location, &Baseline::Commit(oid.clone()))?;
                compared = Some(oid);
            }
        }
    }
    visiting.last_head.clone_from(&status.head.oid);
    Ok((status, compared))
}

/// The next number of an announced snapshot (one counter for every snapshot of the process, root and members).
pub(crate) fn next_seq() -> u64 {
    SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1
}

pub struct GitService {
    shared: Arc<Shared>,
    backends: Arc<BackendCell>,
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
    pub fn start(mut args: ServiceArgs) -> Self {
        let shared = Arc::new(Shared::default());
        let excluded_at_start = std::mem::replace(&mut args.excluded, Box::new(Vec::new));
        let backends = Arc::new(BackendCell::new(args.choice.clone()));
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
        *shared
            .baseline
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(prefs.baseline);
        let worker = Worker {
            args,
            shared: Arc::clone(&shared),
            backends: Arc::clone(&backends),
            stop: Arc::clone(&stop),
            baseline: prefs.baseline,
            visiting: Visiting::new(stored_visit),
            looked: false,
            backend: None,
            main: None,
            worktree_locations: Vec::new(),
            timeouts: 0,
            backoff_until: None,
        };
        let thread = thread::Builder::new()
            .name("permark-git".to_owned())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let keys = sorted(excluded_at_start());
                worker
                    .shared
                    .excluded
                    .write()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get_or_insert(keys);
                worker.run(&rx);
            })
            .ok();
        Self {
            shared,
            backends,
            tx,
            stop,
            thread,
        }
    }

    /// The backend cell the member pool shares.
    #[must_use]
    pub fn backends(&self) -> Arc<BackendCell> {
        Arc::clone(&self.backends)
    }

    /// The baseline in use now.
    #[must_use]
    pub fn baseline(&self) -> BaselineKind {
        self.shared
            .baseline
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .unwrap_or(BaselineKind::LastVisit)
    }

    /// Included projects with their own repository (path keys): their files leave the root status; a change of the
    /// set looks at the root again.
    pub fn set_excluded(&self, keys: Vec<String>) {
        let keys = sorted(keys);
        {
            let mut excluded = self
                .shared
                .excluded
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            if excluded.as_ref() == Some(&keys) {
                return;
            }
            *excluded = Some(keys);
        }
        self.trigger(Why::Focus);
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
        // members read it at their next look: known at once
        *self
            .shared
            .baseline
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(kind);
        let _ = self.tx.send(Msg::Baseline(kind));
    }

    /// The repository reader other plugins use (a file at HEAD | at a commit, the versions of a file).
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

/// Most answers a [`TrackedMemo`] keeps (a few folders per project are asked about).
const MAX_TRACKED: usize = 64;

/// How the files an index answer comes from look: name, size and modification time (ns) of the git folder's `index`,
/// every `sharedindex.*` (a split index) and `HEAD`, sorted. Nanoseconds: an index written again at the same size
/// within the same millisecond is told apart.
type IndexStamp = Vec<(OsString, u64, u128)>;

/// The [`IndexStamp`] of `git_dir`, links taken as they are (never followed); `None`: the folder cannot be listed |
/// a time cannot be read (nothing is kept then).
fn index_stamp(git_dir: &Path) -> Option<IndexStamp> {
    let mut stamp = Vec::new();
    for entry in std::fs::read_dir(git_dir).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let wanted = name == "index"
            || name == "HEAD"
            || name
                .to_str()
                .is_some_and(|name| name.starts_with("sharedindex."));
        if !wanted {
            continue;
        }
        let meta = entry.metadata().ok()?;
        let time = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        stamp.push((name, meta.len(), time.as_nanos()));
    }
    stamp.sort();
    Some(stamp)
}

/// Answers of [`tracked_of`] kept while the index looks the same ([`IndexStamp`], taken before git is asked): a folder
/// asked about again and again reads the index once per change of it, also when no refresh ran. Only answers are kept:
/// a failure, a slow git are asked again. Another repository below is looked for before a kept answer is used (a few
/// stats): one made later changes nothing in the index, and a kept "not tracked" must not hide it.
#[derive(Default)]
pub(crate) struct TrackedMemo {
    kept: Mutex<HashMap<(PathBuf, String), (IndexStamp, bool)>>,
}

impl TrackedMemo {
    /// [`tracked_of`], kept per (git folder, path below the working tree).
    pub(crate) fn tracked(
        &self,
        backend: &dyn GitOps,
        location: &RepoLocation,
        path: &str,
    ) -> Result<bool, NoTracking> {
        tracked_question(location, path)?;
        let key = (location.git_dir.clone(), repo_path(&location.prefix, path));
        let stamp = index_stamp(&location.git_dir);
        if let Some(stamp) = &stamp {
            let kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((_, answer)) = kept.get(&key).filter(|(held, _)| held == stamp) {
                return Ok(*answer);
            }
        }
        let answer = tracked_of(backend, location, path)?;
        if let Some(stamp) = stamp {
            let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
            if kept.len() >= MAX_TRACKED && !kept.contains_key(&key) {
                kept.clear();
            }
            kept.insert(key, (stamp, answer));
        }
        Ok(answer)
    }
}

struct Reader {
    shared: Arc<Shared>,
}

impl Reader {
    /// Where the file `path` (project path) is read: the backend, the working tree holding it (`@wt/<name>/...`: that
    /// linked worktree, else the main one) and the path below that tree. Copied out: the lock is not held while git
    /// runs. `None`: no repository | no such worktree.
    fn place<'p>(&self, path: &'p str) -> Option<(Arc<dyn GitOps>, RepoLocation, &'p str)> {
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
        Some((Arc::clone(&repo.backend), location.clone(), rest))
    }

    /// Why [`Self::place`] found no working tree for a tracked question: with a repository, the worktree named is
    /// none of the project's; without one, what the last look found. Before the first look nothing is known yet:
    /// answered as slow, so a caller that withholds on doubt does not take it for "no repository" and withholds
    /// during startup too (fail closed: a folder with no repository is served once the worker has looked).
    fn no_place(&self) -> NoTracking {
        if self
            .shared
            .repo
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
        {
            return NoTracking::NoRepository;
        }
        self.shared
            .looked
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .unwrap_or(NoTracking::Slow)
    }
}

impl RepoReader for Reader {
    fn blob_at_head(&self, path: &str) -> Option<Vec<u8>> {
        let (backend, location, rest) = self.place(path)?;
        backend.blob_at_head(&location, rest).ok().flatten()
    }

    fn blob_at(&self, commit: CommitId, path: &str) -> Result<Vec<u8>, NoBlob> {
        let (backend, location, rest) = self.place(path).ok_or(NoBlob::Unavailable)?;
        blob_or_miss(backend.blob_at(&location, commit, rest))
    }

    fn file_versions(
        &self,
        path: &str,
        after: usize,
        limit: usize,
    ) -> Result<FileVersions, NoVersions> {
        let (backend, location, rest) = self.place(path).ok_or(NoVersions::Unavailable)?;
        versions_of(backend.as_ref(), &location, rest, after, limit)
    }

    fn tracked_under(&self, path: &str) -> Result<bool, NoTracking> {
        let (backend, location, rest) = self.place(path).ok_or_else(|| self.no_place())?;
        self.shared
            .tracked
            .tracked(backend.as_ref(), &location, rest)
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
        // a caller's text (a stored HEAD) reaches git only as full commit ids
        let (old, new) = (CommitId::parse(old)?, CommitId::parse(new)?);
        let guard = self
            .shared
            .repo
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        let repo = guard.as_ref()?;
        repo.backend
            .commit_count(&repo.main, old.as_str(), new.as_str())
    }
}

/// What the windows are told when git could not answer.
pub(crate) fn condition_of(error: &GitError) -> GitCondition {
    GitCondition::Error {
        code: error.code(),
        reason: error.reason(),
    }
}

/// The number of the last snapshot announced by any service of the process. One counter for all, so a service that
/// starts again (the backend setting changed) never announces a number below what the window already has.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Worker {
    args: ServiceArgs,
    shared: Arc<Shared>,
    backends: Arc<BackendCell>,
    stop: Arc<AtomicBool>,
    baseline: BaselineKind,
    visiting: Visiting,
    /// A discovery has run (a folder found without a repository is not looked at again for plain file changes).
    looked: bool,
    backend: Option<Arc<dyn GitOps>>,
    main: Option<RepoLocation>,
    worktree_locations: Vec<(String, RepoLocation)>,
    /// Status calls in a row that did not answer in time.
    timeouts: u32,
    /// File changes wait until then before git is asked again (after a timeout).
    backoff_until: Option<Instant>,
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
            if self.stop.load(Ordering::SeqCst)
                || !self.wait_out_backoff(rx, &mut pending)
                || !self.wait_out_lock(rx)
            {
                break;
            }
            self.refresh(pending.why());
        }
        self.finish();
    }

    /// Git did not answer in time lately: file changes wait out the backoff (a signal, focus or a new baseline asks at
    /// once). `false`: asked to stop.
    fn wait_out_backoff(&mut self, rx: &Receiver<Msg>, pending: &mut Pending) -> bool {
        while let Some(until) = self.backoff_until {
            if pending.forced || pending.why() != Why::Files {
                return true;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            match rx.recv_timeout(left) {
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
                Ok(Msg::Baseline(kind)) => {
                    self.set_baseline(kind);
                    pending.forced = true;
                }
                Ok(Msg::Refresh(why)) => pending.merge(why),
                Err(RecvTimeoutError::Timeout) => return true,
            }
        }
        true
    }

    /// What a status that did not answer in time leaves: the next file changes wait longer.
    fn timed_out(&mut self) {
        self.timeouts = self.timeouts.saturating_add(1);
        let doubled = self
            .args
            .timings
            .backoff
            .saturating_mul(1_u32 << self.timeouts.saturating_sub(1).min(16));
        self.backoff_until = Some(Instant::now() + doubled.min(MAX_BACKOFF));
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
        *self
            .shared
            .baseline
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(kind);
        if let Some(file) = &self.args.prefs {
            let _ = file.save(&Prefs { baseline: kind });
        }
    }

    /// The project closes: where HEAD is now is where the next visit starts.
    fn finish(&self) {
        if let (Some(file), Some(head)) = (&self.args.visit, &self.visiting.last_head) {
            let _ = file.save(&Visit {
                head: Some(head.clone()),
            });
        }
    }

    /// What a look found, told to the windows (the project's facts) and to the reader. `refused`: the repository there
    /// may not be opened ([`Discovery::Refused`]): the windows see none, as for no repository, while the reader cannot
    /// tell what its index holds.
    fn tell(&self, condition: GitCondition, refused: bool) {
        let unplaced = match &condition {
            GitCondition::Unavailable => NoTracking::Unavailable,
            GitCondition::NoRepo if !refused => NoTracking::NoRepository,
            // a repository is there, refused | its status failed: it cannot answer now
            GitCondition::NoRepo | GitCondition::Error { .. } | GitCondition::Repo { .. } => {
                NoTracking::Failed
            }
        };
        *self
            .shared
            .looked
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(unplaced);
        self.args.facts.set_git(condition);
    }

    fn discover(&mut self, backend: &Arc<dyn GitOps>) -> Discovery {
        let home = self.args.home.as_deref();
        let found = discover_here(backend.as_ref(), &self.args.root, home);
        self.looked = true;
        self.worktree_locations.clear();
        self.main = None;
        if let Discovery::Repo(location) = &found {
            self.main = Some(location.clone());
            for (name, folder) in &self.args.worktrees {
                if let Discovery::Repo(wt) = discover_here(backend.as_ref(), folder, home) {
                    self.worktree_locations.push((name.clone(), wt));
                }
            }
        }
        found
    }

    fn refresh(&mut self, why: Why) {
        if self.backend.is_none() {
            self.backend = self.backends.get();
        }
        let Some(backend) = self.backend.clone() else {
            self.tell(GitCondition::Unavailable, false);
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
                found @ (Discovery::NoRepo | Discovery::Refused) => {
                    self.tell(GitCondition::NoRepo, found == Discovery::Refused);
                    self.publish(None);
                    return;
                }
                Discovery::Error(error) => {
                    self.tell(condition_of(&error), false);
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
                self.timeouts = 0;
                self.backoff_until = None;
                *self
                    .shared
                    .repo
                    .write()
                    .unwrap_or_else(PoisonError::into_inner) = Some(Repo {
                    backend,
                    main,
                    worktrees: self.worktree_locations.clone(),
                });
                self.tell(
                    GitCondition::Repo {
                        state: snapshot.state,
                    },
                    false,
                );
                self.publish(Some(snapshot));
            }
            // a status too slow for once (a huge working tree, a busy disk): the last picture and the repository stay
            // (what was committed can still be read), the windows are told it may be old
            Err(GitError::Timeout) if self.holds(&main) => {
                self.timed_out();
                self.tell(condition_of(&GitError::Timeout), false);
            }
            Err(error) => {
                if error == GitError::Timeout {
                    self.timed_out();
                } else {
                    self.timeouts = 0;
                    self.backoff_until = None;
                }
                self.tell(condition_of(&error), false);
                self.publish(None);
            }
        }
    }

    /// The repository at `main` is held from an earlier look.
    fn holds(&self, main: &RepoLocation) -> bool {
        self.shared
            .repo
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|repo| repo.main == *main)
    }

    fn snapshot_of(
        &mut self,
        backend: &Arc<dyn GitOps>,
        main: &RepoLocation,
    ) -> Result<GitSnapshot, GitError> {
        let (status, compared) =
            measured_status(backend.as_ref(), main, self.baseline, &mut self.visiting)?;
        let excluded = self
            .shared
            .excluded
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or_default();
        // included projects with their own repository: that repository owns their files
        let mut files: Vec<RawFile> = status
            .files
            .into_iter()
            .filter(|file| !excluded_path(&excluded, &file.path))
            .collect();
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
            next.seq = next_seq();
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
            self.visiting.last_head = None;
            (self.args.announce)(None);
        }
    }
}
