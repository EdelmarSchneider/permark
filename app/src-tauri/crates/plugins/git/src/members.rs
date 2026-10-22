//! Repositories of included projects (`projects.md` §8). One slot per included project with a `.git` marker: cheap
//! facts read from files at once (no process); git work queued per repository (`RepoFsm`) and run by a pool of
//! [`POOL_THREADS`] threads (started at the first need, joined on drop): never more git work at once, whatever the
//! number of repositories. Triggers: its files changed (even with no view interest, D24), a view's interest, a signal
//! of its `.git`, window focus. Debounced per repository (300 ms quiet, 2 s at most), FIFO, interest jumps ahead; a
//! stopped repository's running work is cancelled (the installed git's program killed). The same bound covers the
//! root-commit lookups re-attach asks for (queued behind member work, cancelled on drop) and the member reads other
//! plugins make on their own thread ([`Members::with_permit`]): never more than [`POOL_THREADS`] member git processes.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use permark_core::conditions::RepoState;
use permark_core::state::StateFile;

use crate::cheap::{cheap_facts, member_location, Cheap};
use crate::ops::{repo_state, GitOps, RepoLocation};
use crate::service::{measured_status, next_seq, BackendCell, Timings, Visit, Visiting};
use crate::types::{
    BackendKind, BaselineInfo, BaselineKind, CommitLine, GitFile, GitSnapshot, HeadInfo,
    InterestLevel, ProjectSnapshot, RepoError, RepoSummary, ReposChanged,
};

/// Threads running git work for all member repositories.
pub const POOL_THREADS: usize = 2;
/// Repositories whose file lists are kept (the others keep their counts).
pub const MAX_LISTS: usize = 8;
/// Most ids one view's interest holds.
const MAX_INTEREST: usize = 500;
/// How often a waiting `index.lock` is looked at again.
const LOCK_POLL: Duration = Duration::from_millis(40);
/// How long dropping waits for running work before leaving it to end on its own (the built-in backend cannot be
/// cancelled).
const STOP_WAIT: Duration = Duration::from_millis(200);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- the machine of one repository (pure) ----------------------------------------------------------------------

/// Where one repository's git work is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoFsm {
    /// Cheap facts only (never asked).
    Cold,
    /// Waiting for a pool thread.
    Queued {
        level: InterestLevel,
    },
    /// A pool thread asks git; `again`: asked once more meanwhile (runs after this one).
    Running {
        level: InterestLevel,
        again: Option<InterestLevel>,
    },
    /// The answer is current up to `level`.
    Fresh {
        level: InterestLevel,
    },
    /// An answer is there but may be old (a rescan; nobody looks): asked again on interest.
    Stale {
        level: InterestLevel,
    },
    Failed {
        reason: String,
    },
}

/// What happens to a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoEvent {
    /// A view wants to know this much (newly).
    Interest(InterestLevel),
    /// Its files | `.git` changed, focus came back, the baseline changed: look again at this level.
    Changed(InterestLevel),
    /// Something changed widely (rescan) and nobody looks: the answer may be old.
    Outdated,
    /// The pool thread's answer.
    Done(Result<(), String>),
}

/// When the queued work may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// Nothing to (re)schedule.
    Keep,
    /// At once, ahead of the queue (a view waits).
    Now,
    /// After the quiet time (more changes may follow).
    Debounced,
}

/// `(state, event) -> (state, schedule)`.
#[must_use]
pub fn step(state: &RepoFsm, event: &RepoEvent) -> (RepoFsm, Schedule) {
    use RepoEvent as E;
    use RepoFsm as S;
    match (state, event) {
        (S::Fresh { level: have }, E::Interest(want)) if want <= have => {
            (state.clone(), Schedule::Keep)
        }
        (S::Cold | S::Fresh { .. } | S::Failed { .. }, E::Interest(want)) => {
            (S::Queued { level: *want }, Schedule::Now)
        }
        (S::Stale { level } | S::Queued { level }, E::Interest(want)) => (
            S::Queued {
                level: (*level).max(*want),
            },
            Schedule::Now,
        ),
        (S::Running { level, again }, E::Interest(want)) => {
            if want <= level {
                (state.clone(), Schedule::Keep)
            } else {
                (
                    S::Running {
                        level: *level,
                        again: Some(again.map_or(*want, |a| a.max(*want))),
                    },
                    Schedule::Keep,
                )
            }
        }
        (S::Cold | S::Fresh { .. } | S::Stale { .. } | S::Failed { .. }, E::Changed(want)) => {
            (S::Queued { level: *want }, Schedule::Debounced)
        }
        (S::Queued { level }, E::Changed(want)) => (
            S::Queued {
                level: (*level).max(*want),
            },
            Schedule::Debounced,
        ),
        (S::Running { level, again }, E::Changed(want)) => (
            S::Running {
                level: *level,
                again: Some(again.map_or(*want, |a| a.max(*want))),
            },
            Schedule::Keep,
        ),
        (S::Fresh { level }, E::Outdated) => (S::Stale { level: *level }, Schedule::Keep),
        (S::Running { level, again }, E::Done(result)) => match (again, result) {
            (Some(next), _) => (S::Queued { level: *next }, Schedule::Debounced),
            (None, Ok(())) => (S::Fresh { level: *level }, Schedule::Keep),
            (None, Err(reason)) => (
                S::Failed {
                    reason: reason.clone(),
                },
                Schedule::Keep,
            ),
        },
        // nothing to look again at | an answer nobody waits for (stopped, or never asked): ignored
        (_, E::Outdated | E::Done(_)) => (state.clone(), Schedule::Keep),
    }
}

// ---- slots + pool ----------------------------------------------------------------------------------------------

/// One included project with its own repository, as the projects map gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRef {
    pub uid: String,
    /// Project path (root-relative, `/`).
    pub path: String,
    /// Path key (owner matching).
    pub key: String,
    /// The folder on disk.
    pub folder: PathBuf,
    /// The record already keeps its first commit.
    pub first_commit: bool,
}

/// The visit file of a project folder (its own state location; `None`: folder gone | no store).
pub type VisitFile = Arc<dyn Fn(&str) -> Option<StateFile<Visit>> + Send + Sync>;
/// Hands learned first commits to the projects record (one call per batch).
pub type FirstCommits = Arc<dyn Fn(&[(String, String)]) + Send + Sync>;
/// Hands root-commit lookups back (`(path key, commit)`; `None`: none | no git).
pub type RootCommits = Arc<dyn Fn(&[(String, Option<String>)]) + Send + Sync>;

/// What a member folder says when first seen | moved: where its repository is + its cheap facts.
type Read = (Result<RepoLocation, String>, Option<Cheap>);

/// What the pool needs from around it.
#[derive(Clone)]
pub struct Env {
    pub backends: Arc<BackendCell>,
    pub timings: Timings,
    /// `git:repos`.
    pub on_repos: Arc<dyn Fn(&ReposChanged) + Send + Sync>,
    /// `git:projectSnapshot`.
    pub on_snapshot: Arc<dyn Fn(&ProjectSnapshot) + Send + Sync>,
    /// The baseline in use (the root tab's choice).
    pub baseline: Arc<dyn Fn() -> BaselineKind + Send + Sync>,
    /// Path keys of member repositories: the root status leaves their files out.
    pub exclude: Arc<dyn Fn(Vec<String>) + Send + Sync>,
    pub visit_file: VisitFile,
    pub first_commits: FirstCommits,
    /// Answers of [`Members::look_up_roots`].
    pub root_commits: RootCommits,
    /// Summaries are announced this long after the first change (more may follow).
    pub announce_after: Duration,
    /// Learned first commits are written this long after the first one (one write per batch).
    pub first_commit_after: Duration,
}

/// The last status of a repository.
#[derive(Debug, Clone)]
struct StatusFacts {
    head: HeadInfo,
    changed: u32,
    truncated: bool,
    compared: Option<String>,
    /// The baseline it was measured against: a new choice is announced even when the files stay the same.
    kind: BaselineKind,
    backend: BackendKind,
    seq: u64,
}

struct Slot {
    path: String,
    key: String,
    folder: PathBuf,
    location: Result<RepoLocation, String>,
    cheap: Option<Cheap>,
    fsm: RepoFsm,
    /// FIFO position (set when queued).
    order: u64,
    priority: bool,
    due: Option<Instant>,
    first_at: Option<Instant>,
    lock_since: Option<Instant>,
    /// Changes when the slot is made again (an answer for an older one is dropped).
    generation: u64,
    cancel: Arc<AtomicBool>,
    status: Option<StatusFacts>,
    /// Changed files, root-relative paths (kept for at most [`MAX_LISTS`] repositories).
    files: Option<Arc<Vec<GitFile>>>,
    ahead_behind: Option<[u32; 2]>,
    last_commit: Option<CommitLine>,
    /// "Since the last visit" (taken by a running job; `None` before the first look loads it).
    visiting: Option<Visiting>,
    first_commit: bool,
    touched: u64,
    sent: Option<RepoSummary>,
}

impl Slot {
    fn new(
        member: &MemberRef,
        generation: u64,
        location: Result<RepoLocation, String>,
        cheap: Option<Cheap>,
    ) -> Self {
        Self {
            path: member.path.clone(),
            key: member.key.clone(),
            folder: member.folder.clone(),
            location,
            cheap,
            fsm: RepoFsm::Cold,
            order: 0,
            priority: false,
            due: None,
            first_at: None,
            lock_since: None,
            generation,
            cancel: Arc::new(AtomicBool::new(false)),
            status: None,
            files: None,
            ahead_behind: None,
            last_commit: None,
            visiting: None,
            first_commit: member.first_commit,
            touched: 0,
            sent: None,
        }
    }
}

#[derive(Default)]
struct State {
    slots: BTreeMap<String, Slot>,
    /// `(window, level)` -> ids.
    interest: HashMap<(String, InterestLevel), BTreeSet<String>>,
    counter: u64,
    generation: u64,
    stopping: bool,
    running: usize,
    max_running: usize,
    jobs_run: usize,
    dirty: BTreeSet<String>,
    removed: Vec<String>,
    announce_at: Option<Instant>,
    announcements: usize,
    learned: Vec<(String, String)>,
    learned_at: Option<Instant>,
    /// Pool threads started.
    threads: usize,
    /// Root-commit lookups waiting (`(path key, folder)`; run when no member work may).
    lookups: VecDeque<(String, PathBuf)>,
    lookups_run: usize,
}

impl State {
    fn interest_of(&self, uid: &str) -> Option<InterestLevel> {
        self.interest
            .iter()
            .filter(|(_, ids)| ids.contains(uid))
            .map(|((_, level), _)| *level)
            .max()
    }

    fn tick(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }

    fn mark(&mut self, uid: &str, now: Instant, env: &Env) {
        self.dirty.insert(uid.to_owned());
        self.announce_at.get_or_insert(now + env.announce_after);
    }

    /// Applies an event to a slot and schedules what it asks.
    fn apply(&mut self, uid: &str, event: &RepoEvent, now: Instant, env: &Env) {
        let order = self.tick();
        let Some(slot) = self.slots.get_mut(uid) else {
            return;
        };
        let was_pending = matches!(slot.fsm, RepoFsm::Queued { .. } | RepoFsm::Running { .. });
        let (next, schedule) = step(&slot.fsm, event);
        let queued_before = matches!(slot.fsm, RepoFsm::Queued { .. });
        slot.fsm = next;
        if matches!(slot.fsm, RepoFsm::Queued { .. }) {
            if !queued_before {
                slot.order = order;
            }
            match schedule {
                Schedule::Now => {
                    slot.priority = true;
                    slot.due = Some(now);
                }
                Schedule::Debounced => {
                    let first = *slot.first_at.get_or_insert(now);
                    let due = (now + env.timings.quiet).min(first + env.timings.max_wait);
                    slot.due = Some(if slot.priority { now } else { due });
                }
                Schedule::Keep => {}
            }
        }
        let pending = matches!(slot.fsm, RepoFsm::Queued { .. } | RepoFsm::Running { .. });
        if pending != was_pending || matches!(event, RepoEvent::Done(_)) {
            self.mark(uid, now, env);
        }
    }

    /// The level a change asks for: Glance, or what views want.
    fn change_level(&self, uid: &str) -> InterestLevel {
        self.interest_of(uid).unwrap_or(InterestLevel::Glance)
    }

    /// Keeps at most [`MAX_LISTS`] file lists: the least recently looked at without interest go first.
    fn evict_lists(&mut self) {
        loop {
            let with: Vec<(&String, &Slot)> = self
                .slots
                .iter()
                .filter(|(_, s)| s.files.is_some())
                .collect();
            if with.len() <= MAX_LISTS {
                return;
            }
            let victim = with
                .iter()
                .min_by_key(|(uid, s)| (self.interest_of(uid).is_some(), s.touched))
                .map(|(uid, _)| (*uid).clone());
            match victim.and_then(|uid| self.slots.get_mut(&uid)) {
                Some(slot) => slot.files = None,
                None => return,
            }
        }
    }

    fn summary(uid: &str, slot: &Slot, unavailable: bool) -> RepoSummary {
        let base = RepoSummary {
            id: uid.to_owned(),
            path: slot.path.clone(),
            branch: None,
            detached: false,
            oid: None,
            unborn: false,
            state: RepoState::Clean,
            changed: None,
            ahead_behind: None,
            last_commit: None,
            error: None,
            pending: false,
        };
        if unavailable {
            return RepoSummary {
                error: Some(RepoError::Unavailable),
                ..base
            };
        }
        let head = slot
            .cheap
            .as_ref()
            .map(|c| c.head.clone())
            .or_else(|| slot.status.as_ref().map(|s| s.head.clone()))
            .unwrap_or_default();
        let error = match (&slot.location, &slot.fsm) {
            (Err(reason), _) | (_, RepoFsm::Failed { reason }) => Some(RepoError::Failed {
                reason: reason.clone(),
            }),
            _ => None,
        };
        RepoSummary {
            branch: head.branch,
            detached: head.detached,
            oid: head.oid,
            unborn: head.unborn,
            state: slot.cheap.as_ref().map_or(RepoState::Clean, |c| c.state),
            changed: slot.status.as_ref().map(|s| s.changed),
            ahead_behind: slot.ahead_behind,
            last_commit: slot.last_commit.clone(),
            error,
            pending: matches!(slot.fsm, RepoFsm::Queued { .. } | RepoFsm::Running { .. }),
            ..base
        }
    }
}

struct Pool {
    state: Mutex<State>,
    cv: Condvar,
    env: Env,
    /// Cancels running lookups (set on drop).
    lookup_cancel: Arc<AtomicBool>,
}

/// Work handed to a pool thread.
struct Job {
    backend: Arc<dyn GitOps>,
    uid: String,
    generation: u64,
    level: InterestLevel,
    path: String,
    location: RepoLocation,
    cancel: Arc<AtomicBool>,
    visiting: Option<Visiting>,
    kind: BaselineKind,
    want_root: bool,
}

/// What a pool thread brings back.
struct Answer {
    uid: String,
    generation: u64,
    path: String,
    visiting: Visiting,
    facts: Result<Facts, String>,
    root: Option<String>,
}

/// Ahead | behind + last commit (Detail).
type Detail = (Option<(u32, u32)>, Option<(i64, String)>);

struct Facts {
    status: crate::ops::RawStatus,
    compared: Option<String>,
    kind: BaselineKind,
    state: RepoState,
    backend: BackendKind,
    detail: Option<Detail>,
}

/// What a thread does after it let go of the state.
#[derive(Default)]
struct Outside {
    repos: Option<ReposChanged>,
    snapshots: Vec<ProjectSnapshot>,
    visits: Vec<(String, Visiting)>,
    learned: Vec<(String, String)>,
}

/// The member repositories of one project (see the module docs). Drop = stop: running work cancelled, threads
/// joined, each repository's visit remembered in its own state location, learned first commits handed over.
pub struct Members {
    pool: Arc<Pool>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl std::fmt::Debug for Members {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Members").finish_non_exhaustive()
    }
}

fn write_visit(env: &Env, path: &str, visiting: &Visiting) {
    if !matches!(visiting.session, crate::service::Session::Unset) {
        if let (Some(file), Some(head)) = ((env.visit_file)(path), &visiting.last_head) {
            let _ = file.save(&Visit {
                head: Some(head.clone()),
            });
        }
    }
}

fn rebase(path: &str, file: &crate::ops::RawFile) -> GitFile {
    GitFile {
        path: format!("{path}/{}", file.path),
        letter: file.letter,
        staged: file.staged,
        from: file.from.as_ref().map(|from| format!("{path}/{from}")),
    }
}

impl Members {
    #[must_use]
    pub fn new(env: Env) -> Self {
        Self {
            pool: Arc::new(Pool {
                state: Mutex::new(State::default()),
                cv: Condvar::new(),
                env,
                lookup_cancel: Arc::new(AtomicBool::new(false)),
            }),
            threads: Mutex::new(Vec::new()),
        }
    }

    /// Starts the pool threads the first time there is something to do.
    fn ensure_threads(&self) {
        let mut threads = lock(&self.threads);
        if !threads.is_empty() || lock(&self.pool.state).stopping {
            return;
        }
        for n in 0..POOL_THREADS {
            let pool = Arc::clone(&self.pool);
            if let Ok(handle) = std::thread::Builder::new()
                .name(format!("permark-git-pool-{n}"))
                .stack_size(8 * 1024 * 1024)
                .spawn(move || work(&pool))
            {
                threads.push(handle);
            }
        }
        lock(&self.pool.state).threads = threads.len();
    }

    fn wake(&self) {
        self.ensure_threads();
        self.pool.cv.notify_all();
    }

    /// The included projects with their own repository now (from the projects map): new ones get a slot (cheap
    /// facts read at once, no git), gone ones stop (work cancelled, visit remembered), moved ones keep their session.
    pub fn sync(&self, members: &[MemberRef]) {
        let known: HashMap<String, String> = lock(&self.pool.state)
            .slots
            .iter()
            .map(|(uid, s)| (uid.clone(), s.path.clone()))
            .collect();
        // files read without holding the state
        let prepared: Vec<(&MemberRef, Option<Read>)> = members
            .iter()
            .map(|m| {
                if known.get(&m.uid) == Some(&m.path) {
                    (m, None)
                } else {
                    let location = member_location(&m.folder);
                    let cheap = location.as_ref().ok().and_then(|l| cheap_facts(&l.git_dir));
                    (m, Some((location, cheap)))
                }
            })
            .collect();
        let now = Instant::now();
        let env = self.pool.env.clone();
        let mut visits = Vec::new();
        let (keys, changed) = {
            let mut st = lock(&self.pool.state);
            let wanted: BTreeSet<&str> = members.iter().map(|m| m.uid.as_str()).collect();
            let gone: Vec<String> = st
                .slots
                .keys()
                .filter(|uid| !wanted.contains(uid.as_str()))
                .cloned()
                .collect();
            for uid in &gone {
                if let Some(mut slot) = st.slots.remove(uid) {
                    slot.cancel.store(true, Ordering::SeqCst);
                    if let Some(visiting) = slot.visiting.take() {
                        visits.push((slot.path.clone(), visiting));
                    }
                }
                for ids in st.interest.values_mut() {
                    ids.remove(uid);
                }
                st.dirty.remove(uid);
                st.removed.push(uid.clone());
                st.announce_at.get_or_insert(now + env.announce_after);
            }
            let mut changed = !gone.is_empty();
            for (member, read) in prepared {
                let Some((location, cheap)) = read else {
                    if let Some(slot) = st.slots.get_mut(&member.uid) {
                        slot.first_commit |= member.first_commit;
                    }
                    continue;
                };
                changed = true;
                if let Some(slot) = st.slots.get_mut(&member.uid) {
                    // moved: same repository, same session; the list had the old paths
                    slot.path.clone_from(&member.path);
                    slot.key.clone_from(&member.key);
                    slot.folder.clone_from(&member.folder);
                    slot.location = location;
                    slot.cheap = cheap;
                    slot.files = None;
                    slot.first_commit |= member.first_commit;
                    slot.sent = None;
                } else {
                    st.generation += 1;
                    let generation = st.generation;
                    st.slots.insert(
                        member.uid.clone(),
                        Slot::new(member, generation, location, cheap),
                    );
                }
                st.mark(&member.uid, now, &env);
            }
            let keys: Vec<String> = st.slots.values().map(|s| s.key.clone()).collect();
            (keys, changed)
        };
        for (path, visiting) in visits {
            write_visit(&env, &path, &visiting);
        }
        if changed {
            (env.exclude)(keys);
            self.wake();
        }
    }

    /// Files of these repositories changed (`signal`: their `.git` HEAD | index | refs: cheap facts read again now).
    pub fn changed(&self, uids: &[(String, bool)]) {
        if uids.is_empty() {
            return;
        }
        let signals: Vec<(String, PathBuf)> = {
            let st = lock(&self.pool.state);
            uids.iter()
                .filter(|(_, signal)| *signal)
                .filter_map(|(uid, _)| {
                    let git_dir = st.slots.get(uid)?.location.as_ref().ok()?.git_dir.clone();
                    Some((uid.clone(), git_dir))
                })
                .collect()
        };
        let cheap: Vec<(String, Option<Cheap>)> = signals
            .into_iter()
            .map(|(uid, git_dir)| (uid, cheap_facts(&git_dir)))
            .collect();
        let now = Instant::now();
        {
            let mut st = lock(&self.pool.state);
            for (uid, facts) in cheap {
                if let Some(slot) = st.slots.get_mut(&uid) {
                    slot.cheap = facts;
                }
            }
            let env = self.pool.env.clone();
            let mut seen = BTreeSet::new();
            for (uid, _) in uids {
                if seen.insert(uid) && st.slots.contains_key(uid) {
                    let level = st.change_level(uid);
                    st.apply(uid, &RepoEvent::Changed(level), now, &env);
                }
            }
        }
        self.wake();
    }

    /// A rescan: repositories a view looks at are asked again; the others may be old (asked on interest).
    pub fn outdated(&self) {
        self.refresh_all();
    }

    /// The baseline changed: as a rescan (lists measured against the old one).
    pub fn baseline_changed(&self) {
        self.refresh_all();
    }

    fn refresh_all(&self) {
        let now = Instant::now();
        {
            let mut st = lock(&self.pool.state);
            let env = self.pool.env.clone();
            let uids: Vec<String> = st.slots.keys().cloned().collect();
            for uid in uids {
                match st.interest_of(&uid) {
                    Some(level) => st.apply(&uid, &RepoEvent::Changed(level), now, &env),
                    None => st.apply(&uid, &RepoEvent::Outdated, now, &env),
                }
            }
        }
        self.wake();
    }

    /// Window focus: `None` = every repository a view looks at; `Some(uid)` = that one.
    pub fn focus(&self, project: Option<&str>) {
        let now = Instant::now();
        {
            let mut st = lock(&self.pool.state);
            let env = self.pool.env.clone();
            let uids: Vec<String> = match project {
                Some(uid) => st
                    .slots
                    .contains_key(uid)
                    .then(|| uid.to_owned())
                    .into_iter()
                    .collect(),
                None => st
                    .slots
                    .keys()
                    .filter(|uid| st.interest_of(uid).is_some())
                    .cloned()
                    .collect(),
            };
            for uid in uids {
                let level = st.change_level(&uid);
                st.apply(&uid, &RepoEvent::Changed(level), now, &env);
            }
        }
        self.wake();
    }

    /// A view's interest at `level` (replaces what that view wanted at that level; ids not followed are ignored).
    pub fn interest(&self, window: &str, level: InterestLevel, ids: &[String]) {
        let now = Instant::now();
        {
            let mut st = lock(&self.pool.state);
            let env = self.pool.env.clone();
            let wanted: BTreeSet<String> = ids
                .iter()
                .filter(|id| st.slots.contains_key(*id))
                .take(MAX_INTEREST)
                .cloned()
                .collect();
            let before = st
                .interest
                .get(&(window.to_owned(), level))
                .cloned()
                .unwrap_or_default();
            if wanted.is_empty() {
                st.interest.remove(&(window.to_owned(), level));
            } else {
                st.interest
                    .insert((window.to_owned(), level), wanted.clone());
            }
            for uid in wanted.difference(&before) {
                let touched = st.tick();
                if let Some(slot) = st.slots.get_mut(uid) {
                    slot.touched = touched;
                    // a list is wanted: an answer without one is not enough
                    if slot.files.is_none() && matches!(slot.fsm, RepoFsm::Fresh { .. }) {
                        slot.fsm = RepoFsm::Stale {
                            level: InterestLevel::Glance,
                        };
                    }
                }
                let want = st.interest_of(uid).unwrap_or(level);
                st.apply(uid, &RepoEvent::Interest(want), now, &env);
            }
        }
        self.wake();
    }

    /// A caller (window view) let go of the project: its interest goes, whatever level (`projects.md` §8 RAII; the
    /// frontend's `[]` may never come when a view dies).
    pub fn forget_caller(&self, caller: &str) {
        let prefix = format!("{caller}/");
        lock(&self.pool.state)
            .interest
            .retain(|(who, _), _| who != caller && !who.starts_with(&prefix));
    }

    /// Looks up the root commits of these found repositories (`(path key, folder)`; re-attach, `projects.md` §4.4):
    /// queued behind member work in the same pool, answered through [`Env::root_commits`]. Returns at once.
    pub fn look_up_roots(&self, folders: Vec<(String, PathBuf)>) {
        {
            let mut st = lock(&self.pool.state);
            if st.stopping {
                return;
            }
            for (key, folder) in folders {
                if !st.lookups.iter().any(|(k, _)| *k == key) {
                    st.lookups.push_back((key, folder));
                }
            }
        }
        self.wake();
    }

    /// Runs `read` (a member git read on the caller's thread, e.g. review's committed text) as one of the pool's
    /// [`POOL_THREADS`] places: waits while they are all busy. `None`: closing.
    pub fn with_permit<R>(&self, read: impl FnOnce() -> R) -> Option<R> {
        let mut st = lock(&self.pool.state);
        while st.running >= POOL_THREADS && !st.stopping {
            st = self
                .pool
                .cv
                .wait(st)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if st.stopping {
            return None;
        }
        st.running += 1;
        st.max_running = st.max_running.max(st.running);
        drop(st);
        let _permit = Permit(&self.pool);
        Some(read())
    }

    /// Every followed repository, sorted by path.
    #[must_use]
    pub fn summaries(&self) -> Vec<RepoSummary> {
        let unavailable = self.pool.env.backends.unavailable();
        let st = lock(&self.pool.state);
        let mut out: Vec<RepoSummary> = st
            .slots
            .iter()
            .map(|(uid, slot)| State::summary(uid, slot, unavailable))
            .collect();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    /// The changed files of a project's repository (root-relative paths); `None`: no list kept now (declare
    /// interest first) | not followed.
    #[must_use]
    pub fn snapshot(&self, uid: &str) -> Option<GitSnapshot> {
        let kind = (self.pool.env.baseline)();
        let mut st = lock(&self.pool.state);
        let touched = st.tick();
        let slot = st.slots.get_mut(uid)?;
        slot.touched = touched;
        snapshot_of(slot, kind)
    }

    /// Where the repository of a followed project is.
    #[must_use]
    pub fn location(&self, uid: &str) -> Option<RepoLocation> {
        lock(&self.pool.state)
            .slots
            .get(uid)?
            .location
            .as_ref()
            .ok()
            .cloned()
    }

    /// Repositories followed now (teardown tests).
    #[must_use]
    pub fn held(&self) -> usize {
        lock(&self.pool.state).slots.len()
    }

    /// Most git work at once so far, work done, announcements made (tests).
    #[must_use]
    pub fn counters(&self) -> (usize, usize, usize) {
        let st = lock(&self.pool.state);
        (st.max_running, st.jobs_run, st.announcements)
    }

    /// Root-commit lookups done (tests).
    #[must_use]
    pub fn lookups_run(&self) -> usize {
        lock(&self.pool.state).lookups_run
    }

    /// Pool threads started (0 until there is something to do).
    #[must_use]
    pub fn threads(&self) -> usize {
        lock(&self.pool.state).threads
    }
}

impl Drop for Members {
    fn drop(&mut self) {
        {
            let mut st = lock(&self.pool.state);
            st.stopping = true;
            st.lookups.clear();
            for slot in st.slots.values() {
                slot.cancel.store(true, Ordering::SeqCst);
            }
        }
        self.pool.lookup_cancel.store(true, Ordering::SeqCst);
        self.pool.cv.notify_all();
        // the installed git is killed at once; a built-in (libgit2) read cannot be: closing leaves it to end on its
        // own (its answer is dropped, its visit written when it ends)
        let until = Instant::now() + STOP_WAIT;
        for handle in lock(&self.threads).drain(..) {
            while !handle.is_finished() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(2));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
        let env = self.pool.env.clone();
        let (visits, learned) = {
            let mut st = lock(&self.pool.state);
            let visits: Vec<(String, Visiting)> = std::mem::take(&mut st.slots)
                .into_values()
                .filter_map(|mut s| s.visiting.take().map(|v| (s.path, v)))
                .collect();
            (visits, std::mem::take(&mut st.learned))
        };
        for (path, visiting) in &visits {
            write_visit(&env, path, visiting);
        }
        if !learned.is_empty() {
            (env.first_commits)(&learned);
        }
    }
}

/// One of the pool's places taken by a caller's own read; given back on drop (also on a panic).
struct Permit<'p>(&'p Pool);

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        lock(&self.0.state).running -= 1;
        self.0.cv.notify_all();
    }
}

fn snapshot_of(slot: &Slot, kind: BaselineKind) -> Option<GitSnapshot> {
    let status = slot.status.as_ref()?;
    let files = slot.files.as_ref()?;
    Some(GitSnapshot {
        seq: status.seq,
        backend: status.backend,
        head: slot
            .cheap
            .as_ref()
            .map_or_else(|| status.head.clone(), |c| c.head.clone()),
        state: slot.cheap.as_ref().map_or(RepoState::Clean, |c| c.state),
        baseline: BaselineInfo {
            kind,
            oid: status.compared.clone(),
        },
        files: files.as_ref().clone(),
        truncated: status.truncated,
        worktrees: Vec::new(),
    })
}

/// The next job, if one may run now (`backend`: this thread's; `None` -> every queued repository fails).
fn pick(st: &mut State, now: Instant, env: &Env, backend: Option<&Arc<dyn GitOps>>) -> Option<Job> {
    let mut candidates: Vec<(bool, u64, String)> = st
        .slots
        .iter()
        .filter(|(_, s)| matches!(s.fsm, RepoFsm::Queued { .. }) && s.due.is_some_and(|d| d <= now))
        .map(|(uid, s)| (!s.priority, s.order, uid.clone()))
        .collect();
    candidates.sort();
    for (_, _, uid) in candidates {
        let Some(slot) = st.slots.get_mut(&uid) else {
            continue;
        };
        let reason = match (&slot.location, backend) {
            (Err(reason), _) => Some(reason.clone()),
            (Ok(_), None) => Some("git is not available".to_owned()),
            (Ok(_), Some(_)) => None,
        };
        if let Some(reason) = reason {
            slot.fsm = RepoFsm::Running {
                level: InterestLevel::Glance,
                again: None,
            };
            st.apply(&uid, &RepoEvent::Done(Err(reason)), now, env);
            continue;
        }
        let (Ok(location), Some(backend)) = (slot.location.clone(), backend) else {
            continue;
        };
        // git is writing the index: its answer would race the write; wait a little
        if location.git_dir.join("index.lock").exists() {
            let since = *slot.lock_since.get_or_insert(now);
            if now.duration_since(since) < env.timings.lock_wait {
                slot.due = Some(now + LOCK_POLL);
                continue;
            }
        }
        let RepoFsm::Queued { level } = slot.fsm else {
            continue;
        };
        slot.fsm = RepoFsm::Running { level, again: None };
        slot.priority = false;
        slot.due = None;
        slot.first_at = None;
        slot.lock_since = None;
        let job = Job {
            backend: Arc::clone(backend),
            uid: uid.clone(),
            generation: slot.generation,
            level,
            path: slot.path.clone(),
            location,
            cancel: Arc::clone(&slot.cancel),
            visiting: slot.visiting.take(),
            kind: (env.baseline)(),
            want_root: !slot.first_commit,
        };
        st.mark(&uid, now, env);
        return Some(job);
    }
    None
}

fn run_job(env: &Env, job: Job) -> Answer {
    let backend = job
        .backend
        .cancellable(&job.cancel)
        .unwrap_or_else(|| Arc::clone(&job.backend));
    let mut visiting = job.visiting.unwrap_or_else(|| {
        Visiting::new((env.visit_file)(&job.path).and_then(|file| file.load().into_value().head))
    });
    let facts = measured_status(backend.as_ref(), &job.location, job.kind, &mut visiting)
        .map_err(|e| e.to_string())
        .map(|(status, compared)| {
            let detail = (job.level == InterestLevel::Detail).then(|| {
                (
                    backend.ahead_behind(&job.location).ok().flatten(),
                    backend.last_commit(&job.location).ok().flatten(),
                )
            });
            Facts {
                status,
                compared,
                kind: job.kind,
                state: repo_state(&job.location.git_dir),
                backend: backend.kind(),
                detail,
            }
        });
    let root = (job.want_root && facts.is_ok() && !job.cancel.load(Ordering::SeqCst))
        .then(|| backend.root_commit(&job.location).ok().flatten())
        .flatten();
    Answer {
        uid: job.uid,
        generation: job.generation,
        path: job.path,
        visiting,
        facts,
        root,
    }
}

/// Puts an answer into its slot (an answer for a slot that went is dropped; its visit remembered).
fn settle(st: &mut State, answer: Answer, now: Instant, env: &Env, out: &mut Outside) {
    let keep_list = st.interest_of(&answer.uid).is_some();
    let Some(slot) = st
        .slots
        .get_mut(&answer.uid)
        .filter(|s| s.generation == answer.generation)
    else {
        out.visits.push((answer.path, answer.visiting));
        return;
    };
    slot.visiting = Some(answer.visiting);
    let result = match answer.facts {
        Ok(facts) => {
            let changed = u32::try_from(facts.status.files.len()).unwrap_or(u32::MAX);
            let files: Vec<GitFile> = facts
                .status
                .files
                .iter()
                .map(|f| rebase(&slot.path, f))
                .collect();
            let cheap = Cheap {
                head: facts.status.head.clone(),
                state: facts.state,
            };
            let before = slot.files.clone();
            let same = slot.status.as_ref().is_some_and(|s| {
                s.head == facts.status.head
                    && s.truncated == facts.status.truncated
                    && s.compared == facts.compared
                    && s.kind == facts.kind
            }) && before.as_deref() == Some(&files);
            let seq = if same {
                slot.status.as_ref().map_or_else(next_seq, |s| s.seq)
            } else {
                next_seq()
            };
            slot.status = Some(StatusFacts {
                head: facts.status.head,
                changed,
                truncated: facts.status.truncated,
                compared: facts.compared,
                kind: facts.kind,
                backend: facts.backend,
                seq,
            });
            slot.cheap = Some(cheap);
            if let Some((ahead_behind, last)) = facts.detail {
                slot.ahead_behind = ahead_behind.map(|(a, b)| [a, b]);
                slot.last_commit = last.map(|(at_ms, subject)| CommitLine { at_ms, subject });
            }
            if keep_list || slot.files.is_some() {
                slot.files = Some(Arc::new(files));
                if !same {
                    out.snapshots.push(ProjectSnapshot {
                        project: answer.uid.clone(),
                        snapshot: snapshot_of(slot, (env.baseline)()),
                    });
                }
            }
            if let Some(root) = answer.root {
                if !slot.first_commit {
                    slot.first_commit = true;
                    st.learned.push((answer.uid.clone(), root));
                    st.learned_at.get_or_insert(now + env.first_commit_after);
                }
            }
            Ok(())
        }
        Err(reason) => Err(reason),
    };
    st.apply(&answer.uid, &RepoEvent::Done(result), now, env);
    st.evict_lists();
}

fn take_announcement(st: &mut State, env: &Env) -> Option<ReposChanged> {
    st.announce_at = None;
    let unavailable = env.backends.unavailable();
    let dirty = std::mem::take(&mut st.dirty);
    let mut upsert = Vec::new();
    for uid in dirty {
        let Some(slot) = st.slots.get(&uid) else {
            continue;
        };
        let summary = State::summary(&uid, slot, unavailable);
        if slot.sent.as_ref() != Some(&summary) {
            upsert.push(summary.clone());
            if let Some(slot) = st.slots.get_mut(&uid) {
                slot.sent = Some(summary);
            }
        }
    }
    let removed = std::mem::take(&mut st.removed);
    if upsert.is_empty() && removed.is_empty() {
        return None;
    }
    st.announcements += 1;
    Some(ReposChanged { upsert, removed })
}

/// A pool thread: announcements when due, the next job when one may run, else sleep until something is due.
fn work(pool: &Pool) {
    let env = &pool.env;
    // chosen once for the project (the root worker usually chose already)
    let backend = env.backends.get();
    let mut st = lock(&pool.state);
    loop {
        if st.stopping {
            break;
        }
        let now = Instant::now();
        let mut out = Outside::default();
        if st.announce_at.is_some_and(|at| at <= now) {
            out.repos = take_announcement(&mut st, env);
        }
        if st.learned_at.is_some_and(|at| at <= now) {
            st.learned_at = None;
            out.learned = std::mem::take(&mut st.learned);
        }
        if out.repos.is_some() || !out.learned.is_empty() {
            drop(st);
            deliver(env, &out);
            st = lock(&pool.state);
            continue;
        }
        let room = st.running < POOL_THREADS;
        if let Some(job) = room
            .then(|| pick(&mut st, now, env, backend.as_ref()))
            .flatten()
        {
            st.running += 1;
            st.max_running = st.max_running.max(st.running);
            drop(st);
            let answer = run_job(env, job);
            st = lock(&pool.state);
            st.running -= 1;
            st.jobs_run += 1;
            let mut out = Outside::default();
            settle(&mut st, answer, Instant::now(), env, &mut out);
            drop(st);
            deliver(env, &out);
            pool.cv.notify_all();
            st = lock(&pool.state);
            continue;
        }
        // no member work may run now: a root-commit lookup (same bound)
        if room && !st.lookups.is_empty() {
            st = run_lookup(pool, st, backend.as_ref());
            continue;
        }
        // full (callers' reads hold the places): only announcements are due until a place is given back
        let dues = st
            .slots
            .values()
            .filter(|s| room && matches!(s.fsm, RepoFsm::Queued { .. }))
            .filter_map(|s| s.due);
        let next = dues.chain(st.announce_at).chain(st.learned_at).min();
        st = match next {
            Some(at) => {
                pool.cv
                    .wait_timeout(
                        st,
                        at.saturating_duration_since(now)
                            .max(Duration::from_millis(1)),
                    )
                    .unwrap_or_else(PoisonError::into_inner)
                    .0
            }
            None => pool.cv.wait(st).unwrap_or_else(PoisonError::into_inner),
        };
    }
}

/// Runs the next root-commit lookup with the state let go meanwhile (no git: every waiting one answers none at once).
fn run_lookup<'p>(
    pool: &'p Pool,
    mut st: MutexGuard<'p, State>,
    backend: Option<&Arc<dyn GitOps>>,
) -> MutexGuard<'p, State> {
    let env = &pool.env;
    let Some(backend) = backend else {
        let answers: Vec<(String, Option<String>)> =
            st.lookups.drain(..).map(|(k, _)| (k, None)).collect();
        drop(st);
        (env.root_commits)(&answers);
        return lock(&pool.state);
    };
    let Some((key, folder)) = st.lookups.pop_front() else {
        return st;
    };
    st.running += 1;
    st.max_running = st.max_running.max(st.running);
    drop(st);
    let commit = root_commit_of(backend, &folder, &pool.lookup_cancel);
    let mut st = lock(&pool.state);
    st.running -= 1;
    st.lookups_run += 1;
    let stopping = st.stopping;
    drop(st);
    if !stopping {
        (env.root_commits)(&[(key, commit)]);
    }
    pool.cv.notify_all();
    lock(&pool.state)
}

/// The root commit of the repository at `folder` (cancellable with the installed git).
fn root_commit_of(
    backend: &Arc<dyn GitOps>,
    folder: &Path,
    cancel: &Arc<AtomicBool>,
) -> Option<String> {
    let location = member_location(folder).ok()?;
    let backend = backend
        .cancellable(cancel)
        .unwrap_or_else(|| Arc::clone(backend));
    backend.root_commit(&location).ok().flatten()
}

fn deliver(env: &Env, out: &Outside) {
    for (path, visiting) in &out.visits {
        write_visit(env, path, visiting);
    }
    for snapshot in &out.snapshots {
        (env.on_snapshot)(snapshot);
    }
    if let Some(repos) = &out.repos {
        (env.on_repos)(repos);
    }
    if !out.learned.is_empty() {
        (env.first_commits)(&out.learned);
    }
}

#[cfg(test)]
#[path = "members_tests.rs"]
mod tests;
