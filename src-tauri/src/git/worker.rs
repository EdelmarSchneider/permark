//! The git worker: one thread owns the `Repository` and answers every git question.
//!
//! Triggers (filesystem batches, the git-dir watcher, focus) arrive over a channel and are coalesced
//! (400 ms of quiet, at most 2 s) into a single status computation, which waits out `index.lock`.
//! The result is announced only when it differs from the previous snapshot, and carries a
//! `generation` so the UI can discard stale ones.

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use git2::{Oid, Repository};

use super::dirwatch::GitDirWatcher;
use super::discover::{discover, Discovery, GitContext};
use super::snapshot::{compute, pick_session_baseline, Computed, SessionBaseline};
use super::{BaselineInfo, BaselineKind, GitError, GitSnapshot, GitState};
use crate::util::Coalescer;

/// How long a pending refresh waits for `index.lock` to disappear before computing anyway.
const LOCK_WAIT_MAX: Duration = Duration::from_secs(10);
/// How long a caller waits for the worker to answer.
const REPLY_TIMEOUT: Duration = Duration::from_mins(1);
/// How long closing the project waits for the list of uncommitted files before saving without it.
const DIRTY_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy)]
pub struct GitTimings {
    pub quiet: Duration,
    pub max_wait: Duration,
}

impl Default for GitTimings {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(400),
            max_wait: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GitSettings {
    /// Canonical vault root (used to look for a repository again if none was found at first).
    pub vault_root: PathBuf,
    /// The user's home folder: a repository whose working directory is it is not used.
    pub home: Option<PathBuf>,
    pub baseline: BaselineKind,
    /// HEAD stored when the project was last closed.
    pub last_visit_oid: Option<String>,
    pub timings: GitTimings,
}

pub type SnapshotSink = Arc<dyn Fn(&GitSnapshot) + Send + Sync>;

enum Msg {
    Trigger,
    SetBaseline(BaselineKind, Sender<GitSnapshot>),
    Snapshot(Sender<GitSnapshot>),
    SinceVisit(Sender<Option<Vec<(String, GitState)>>>),
    Dirty(Sender<Option<Vec<String>>>),
    Shutdown,
}

/// Handle to the worker thread. Dropping it stops the thread.
pub struct GitHandle {
    tx: Sender<Msg>,
    thread: Mutex<Option<JoinHandle<()>>>,
    kind_fallback: BaselineKind,
}

impl GitHandle {
    /// Starts the worker for an already performed [`discover`] call. Never fails: problems end up in
    /// the snapshot's `gitError`.
    pub fn start(settings: GitSettings, discovery: Discovery, sink: SnapshotSink) -> Self {
        let (tx, rx) = mpsc::channel::<Msg>();
        let kind_fallback = settings.baseline;
        let worker_tx = tx.clone();
        let thread = thread::Builder::new()
            .name("trailmark-git".to_owned())
            .spawn(move || {
                let mut w = Worker::new(settings, worker_tx, sink);
                w.attach(discovery);
                w.update(false);
                w.run(&rx);
            })
            .ok();
        Self {
            tx,
            thread: Mutex::new(thread),
            kind_fallback,
        }
    }

    /// Asks for a refresh (coalesced with other triggers).
    pub fn trigger(&self) {
        let _ = self.tx.send(Msg::Trigger);
    }

    /// A cloneable function that does what [`GitHandle::trigger`] does (for other threads).
    pub fn trigger_fn(&self) -> Arc<dyn Fn() + Send + Sync> {
        let tx = self.tx.clone();
        Arc::new(move || {
            let _ = tx.send(Msg::Trigger);
        })
    }

    /// The latest snapshot.
    pub fn snapshot(&self) -> GitSnapshot {
        let (tx, rx) = mpsc::channel();
        self.ask(Msg::Snapshot(tx), &rx)
            .unwrap_or_else(|_| self.gone())
    }

    /// Switches the baseline and returns the recomputed snapshot.
    pub fn set_baseline(&self, kind: BaselineKind) -> GitSnapshot {
        let (tx, rx) = mpsc::channel();
        self.ask(Msg::SetBaseline(kind, tx), &rx)
            .unwrap_or_else(|_| self.gone())
    }

    /// Files changed since the commit stored at the last visit; `None` when there is no usable commit.
    pub fn since_last_visit(&self) -> Result<Option<Vec<(String, GitState)>>, GitError> {
        let (tx, rx) = mpsc::channel();
        self.ask(Msg::SinceVisit(tx), &rx)
    }

    /// Files that exist and differ from HEAD (modified, added or untracked), whatever baseline is
    /// selected; `None` when there is no repository.
    pub fn dirty_files(&self) -> Result<Option<Vec<String>>, GitError> {
        let (tx, rx) = mpsc::channel();
        self.ask_within(Msg::Dirty(tx), &rx, DIRTY_TIMEOUT)
    }

    fn ask<T>(&self, msg: Msg, rx: &mpsc::Receiver<T>) -> Result<T, GitError> {
        self.ask_within(msg, rx, REPLY_TIMEOUT)
    }

    fn ask_within<T>(
        &self,
        msg: Msg,
        rx: &mpsc::Receiver<T>,
        timeout: Duration,
    ) -> Result<T, GitError> {
        self.tx.send(msg).map_err(|_| GitError::WorkerGone)?;
        rx.recv_timeout(timeout).map_err(|_| GitError::WorkerGone)
    }

    fn gone(&self) -> GitSnapshot {
        GitSnapshot::without_repo(self.kind_fallback, Some(GitError::WorkerGone.to_string()))
    }

    /// Stops the worker thread and waits for it. Later calls degrade to "worker not available".
    pub fn shutdown(&self) {
        let _ = self.tx.send(Msg::Shutdown);
        let thread = self.thread.lock().ok().and_then(|mut t| t.take());
        if let Some(t) = thread {
            let _ = t.join();
        }
    }
}

impl Drop for GitHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Worker {
    settings: GitSettings,
    kind: BaselineKind,
    /// The commit stored at the last close (used for "since your last visit").
    stored_visit: Option<Oid>,
    /// What "last visit" means in this session.
    session: SessionBaseline,
    repo: Option<(Repository, GitContext)>,
    error: Option<String>,
    #[allow(dead_code)] // held only to keep the OS watcher alive
    dir_watcher: Option<GitDirWatcher>,
    tx: Sender<Msg>,
    sink: SnapshotSink,
    generation: u64,
    last: Option<GitSnapshot>,
}

impl Worker {
    fn new(settings: GitSettings, tx: Sender<Msg>, sink: SnapshotSink) -> Self {
        let stored_visit = settings
            .last_visit_oid
            .as_deref()
            .and_then(|s| Oid::from_str(s).ok());
        Self {
            kind: settings.baseline,
            stored_visit,
            session: SessionBaseline::Empty,
            settings,
            repo: None,
            error: None,
            dir_watcher: None,
            tx,
            sink,
            generation: 0,
            last: None,
        }
    }

    fn attach(&mut self, discovery: Discovery) {
        self.repo = None;
        self.error = None;
        self.dir_watcher = None;
        match discovery {
            Discovery::NoRepo => {}
            Discovery::Error(message) => self.error = Some(message),
            Discovery::Repo(ctx) => match ctx.open() {
                Ok(repo) => {
                    let tx = self.tx.clone();
                    self.dir_watcher = GitDirWatcher::start(
                        &ctx,
                        Arc::new(move || {
                            let _ = tx.send(Msg::Trigger);
                        }),
                    );
                    self.session = pick_session_baseline(&repo, self.stored_visit);
                    self.repo = Some((repo, ctx));
                }
                Err(e) => self.error = Some(e.message().to_owned()),
            },
        }
    }

    /// A repository may have appeared since the last look (`git init`, or a repaired `.git`).
    fn rediscover(&mut self) {
        if self.repo.is_none() {
            let found = discover(&self.settings.vault_root, self.settings.home.as_deref());
            self.attach(found);
        }
    }

    fn fresh(&mut self) -> GitSnapshot {
        let Some((repo, ctx)) = &self.repo else {
            return GitSnapshot::without_repo(self.kind, self.error.clone());
        };
        match compute(repo, ctx, self.kind, self.session) {
            Ok(c) => snapshot_from(c, self.kind),
            Err(first) => {
                // Cached state may be stale (repository replaced underneath us): reopen once.
                let retry = ctx.open().and_then(|r| {
                    let out = compute(&r, ctx, self.kind, self.session)?;
                    Ok((r, out))
                });
                if let Ok((reopened, c)) = retry {
                    let ctx = ctx.clone();
                    self.repo = Some((reopened, ctx));
                    snapshot_from(c, self.kind)
                } else {
                    let mut s =
                        GitSnapshot::without_repo(self.kind, Some(first.message().to_owned()));
                    s.is_repo = true;
                    s
                }
            }
        }
    }

    /// Recomputes; announces and stores the result only when it differs from the previous one.
    fn update(&mut self, emit: bool) -> GitSnapshot {
        let mut snap = self.fresh();
        if let Some(prev) = &self.last {
            if prev.same_content(&snap) {
                return prev.clone();
            }
        }
        self.generation += 1;
        snap.generation = self.generation;
        self.last = Some(snap.clone());
        if emit {
            (self.sink)(&snap);
        }
        snap
    }

    fn current(&mut self) -> GitSnapshot {
        match &self.last {
            Some(s) => s.clone(),
            None => self.update(false),
        }
    }

    fn since_visit(&self) -> Option<Vec<(String, GitState)>> {
        let (repo, ctx) = self.repo.as_ref()?;
        let oid = self.stored_visit?;
        repo.find_commit(oid).ok()?;
        let c = compute(
            repo,
            ctx,
            BaselineKind::LastVisit,
            SessionBaseline::Commit(oid),
        )
        .ok()?;
        Some(c.files.into_iter().map(|(p, f)| (p, f.state)).collect())
    }

    fn dirty(&self) -> Option<Vec<String>> {
        let (repo, ctx) = self.repo.as_ref()?;
        let c = compute(repo, ctx, BaselineKind::Head, SessionBaseline::Empty).ok()?;
        Some(
            c.files
                .into_iter()
                .filter(|(_, f)| f.state != GitState::Deleted)
                .map(|(path, _)| path)
                .collect(),
        )
    }

    fn index_locked(&self) -> bool {
        self.repo
            .as_ref()
            .is_some_and(|(_, ctx)| ctx.git_dir.join("index.lock").exists())
    }

    fn run(&mut self, rx: &mpsc::Receiver<Msg>) {
        let mut pending =
            Coalescer::new(self.settings.timings.quiet, self.settings.timings.max_wait);
        let mut waiting_for_lock: Option<Instant> = None;
        loop {
            let msg = match pending.deadline() {
                Some(deadline) => {
                    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                }
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match msg {
                Ok(Msg::Trigger) => pending.touch(Instant::now()),
                Ok(Msg::SetBaseline(kind, reply)) => {
                    self.kind = kind;
                    pending.reset();
                    let _ = reply.send(self.update(true));
                }
                Ok(Msg::Snapshot(reply)) => {
                    let _ = reply.send(self.current());
                }
                Ok(Msg::SinceVisit(reply)) => {
                    let _ = reply.send(self.since_visit());
                }
                Ok(Msg::Dirty(reply)) => {
                    let _ = reply.send(self.dirty());
                }
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if pending.due(Instant::now()) {
                let now = Instant::now();
                let since = *waiting_for_lock.get_or_insert(now);
                if self.index_locked() && now.duration_since(since) < LOCK_WAIT_MAX {
                    // A git command is rewriting the index: look again shortly instead of
                    // reporting half-done work.
                    pending.reset();
                    pending.touch(now);
                    continue;
                }
                waiting_for_lock = None;
                pending.reset();
                self.rediscover();
                self.update(true);
            }
        }
    }
}

fn snapshot_from(c: Computed, kind: BaselineKind) -> GitSnapshot {
    GitSnapshot {
        generation: 0,
        is_repo: true,
        git_error: None,
        head: c.head,
        repo_state: c.repo_state,
        baseline: BaselineInfo {
            kind,
            oid: c.baseline_oid,
        },
        files: c.files,
        truncated: c.truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::{commit_all, init_repo, write};
    use std::sync::Mutex;

    const FAST: GitTimings = GitTimings {
        quiet: Duration::from_millis(40),
        max_wait: Duration::from_millis(200),
    };

    struct Fx {
        dir: tempfile::TempDir,
        repo: Repository,
        handle: GitHandle,
        events: Arc<Mutex<Vec<GitSnapshot>>>,
    }

    impl Fx {
        fn count(&self) -> usize {
            self.events.lock().expect("lock").len()
        }

        fn wait_events(&self, at_least: usize) -> bool {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if self.count() >= at_least {
                    return true;
                }
                thread::sleep(Duration::from_millis(20));
            }
            false
        }

        fn last_event(&self) -> GitSnapshot {
            self.events
                .lock()
                .expect("lock")
                .last()
                .cloned()
                .expect("an event")
        }
    }

    fn start_with(
        kind: BaselineKind,
        last_visit: Option<String>,
        prepare: impl FnOnce(&Repository, &std::path::Path) -> Option<String>,
    ) -> Fx {
        start_timed(kind, last_visit, FAST, prepare)
    }

    fn start_timed(
        kind: BaselineKind,
        last_visit: Option<String>,
        timings: GitTimings,
        prepare: impl FnOnce(&Repository, &std::path::Path) -> Option<String>,
    ) -> Fx {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path());
        let last_visit = prepare(&repo, dir.path()).or(last_visit);
        let vault = dunce::canonicalize(dir.path()).expect("canonicalize");
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = Arc::clone(&events);
        let settings = GitSettings {
            vault_root: vault.clone(),
            home: None,
            baseline: kind,
            last_visit_oid: last_visit,
            timings,
        };
        let handle = GitHandle::start(
            settings,
            discover(&vault, None),
            Arc::new(move |s: &GitSnapshot| sink_events.lock().expect("lock").push(s.clone())),
        );
        // Wait for the worker's initial computation so tests change files strictly after it.
        let _ = handle.snapshot();
        Fx {
            dir,
            repo,
            handle,
            events,
        }
    }

    fn committed(kind: BaselineKind) -> Fx {
        start_with(kind, None, |repo, dir| {
            write(dir, "a.md", "one\n");
            commit_all(repo, "init");
            None
        })
    }

    #[test]
    fn initial_snapshot_is_generation_one_and_not_announced() {
        let fx = committed(BaselineKind::Head);
        let s = fx.handle.snapshot();
        assert!(s.is_repo && s.git_error.is_none());
        assert_eq!(s.generation, 1);
        assert_eq!(s.head.branch.as_deref(), Some("main"));
        assert_eq!(s.baseline.kind, BaselineKind::Head);
        assert!(s.files.is_empty());
        assert_eq!(fx.count(), 0);
    }

    #[test]
    fn a_trigger_after_a_change_announces_a_new_generation_once() {
        let fx = committed(BaselineKind::Head);
        write(fx.dir.path(), "new.md", "x\n");
        fx.handle.trigger();
        assert!(fx.wait_events(1));
        let e = fx.last_event();
        assert_eq!(e.generation, 2);
        assert_eq!(
            e.files.get("new.md").map(|f| f.state),
            Some(GitState::Untracked)
        );

        // Nothing changed: more triggers must not produce more events (echo-loop protection).
        for _ in 0..5 {
            fx.handle.trigger();
        }
        thread::sleep(Duration::from_millis(400));
        assert_eq!(fx.count(), 1);
        assert_eq!(fx.handle.snapshot().generation, 2);
    }

    #[test]
    fn bursts_of_triggers_are_coalesced_into_one_computation() {
        // Generous windows, so that a slow disk or a busy machine cannot stretch the burst past the
        // quiet period (which would legitimately produce a second snapshot): the burst takes roughly
        // 100 ms, the quiet period is 400 ms and the cap 5 s.
        let slow = GitTimings {
            quiet: Duration::from_millis(400),
            max_wait: Duration::from_secs(5),
        };
        let fx = start_timed(BaselineKind::Head, None, slow, |repo, dir| {
            write(dir, "a.md", "one\n");
            commit_all(repo, "init");
            None
        });
        for i in 0..20 {
            write(fx.dir.path(), &format!("n{i}.md"), "x\n");
            fx.handle.trigger();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(fx.wait_events(1));
        // Give a stray second computation every chance to show up before counting.
        thread::sleep(slow.quiet * 2);
        assert_eq!(fx.count(), 1, "one announcement for the whole burst");
        assert_eq!(fx.last_event().files.len(), 20);
    }

    #[test]
    fn index_lock_defers_the_refresh_until_it_is_gone() {
        let fx = committed(BaselineKind::Head);
        let lock = fx.dir.path().join(".git").join("index.lock");
        std::fs::write(&lock, "").expect("lock");
        write(fx.dir.path(), "new.md", "x\n");
        fx.handle.trigger();
        thread::sleep(Duration::from_millis(500));
        assert_eq!(fx.count(), 0, "must wait while index.lock exists");
        std::fs::remove_file(&lock).expect("unlock");
        assert!(fx.wait_events(1));
    }

    #[test]
    fn the_git_dir_watcher_notices_commits_without_any_trigger() {
        let fx = committed(BaselineKind::Head);
        write(fx.dir.path(), "a.md", "two\n");
        fx.handle.trigger();
        assert!(fx.wait_events(1));
        assert_eq!(
            fx.last_event().files.get("a.md").map(|f| f.state),
            Some(GitState::Modified)
        );
        // A commit by another tool: only .git changes.
        commit_all(&fx.repo, "agent commit");
        assert!(fx.wait_events(2), "commit must be noticed");
        // A commit is several writes (the index, then the branch): on a busy machine the first of them can be announced on
        // its own, so the announcement of the finished commit is waited for, not counted.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !fx.last_event().files.is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        let e = fx.last_event();
        assert!(e.files.is_empty());
        assert!(e.generation >= 3);
    }

    #[test]
    fn switching_the_baseline_returns_and_announces_a_new_snapshot() {
        let fx = start_with(BaselineKind::LastVisit, None, |repo, dir| {
            write(
                dir, "a.md", "one
",
            );
            let visited = commit_all(repo, "visit");
            write(
                dir, "b.md", "agent
",
            );
            commit_all(repo, "agent");
            Some(visited.to_string())
        });
        let s = fx.handle.snapshot();
        assert_eq!(s.baseline.kind, BaselineKind::LastVisit);
        assert_eq!(s.files.get("b.md").map(|f| f.state), Some(GitState::Added));

        let head = fx.handle.set_baseline(BaselineKind::Head);
        assert_eq!(head.baseline.kind, BaselineKind::Head);
        assert!(head.files.is_empty());
        assert!(head.generation > s.generation);
        assert!(fx.wait_events(1), "a baseline switch is announced");

        let back = fx.handle.set_baseline(BaselineKind::LastVisit);
        assert_eq!(back.files, s.files);
        assert!(back.generation > head.generation);
    }

    #[test]
    fn last_visit_baseline_and_since_last_visit_use_the_stored_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path());
        write(dir.path(), "a.md", "one\n");
        let visited = commit_all(&repo, "visit");
        write(dir.path(), "b.md", "agent\n");
        commit_all(&repo, "agent");
        let vault = dunce::canonicalize(dir.path()).expect("canonicalize");
        let make = |oid: Option<String>| {
            GitHandle::start(
                GitSettings {
                    vault_root: vault.clone(),
                    home: None,
                    baseline: BaselineKind::LastVisit,
                    last_visit_oid: oid,
                    timings: FAST,
                },
                discover(&vault, None),
                Arc::new(|_: &GitSnapshot| {}),
            )
        };
        let h = make(Some(visited.to_string()));
        let s = h.snapshot();
        assert_eq!(s.baseline.oid, Some(visited.to_string()));
        assert_eq!(s.files.get("b.md").map(|f| f.state), Some(GitState::Added));
        assert_eq!(
            h.since_last_visit().expect("ask"),
            Some(vec![("b.md".to_owned(), GitState::Added)])
        );
        // The "since" list always uses the stored commit, whatever baseline is selected.
        h.set_baseline(BaselineKind::Head);
        assert_eq!(h.since_last_visit().expect("ask").map(|v| v.len()), Some(1));

        // First visit: no stored commit, the baseline is HEAD and there is nothing "since".
        let fresh = make(None);
        let s = fresh.snapshot();
        assert!(s.files.is_empty());
        assert_eq!(fresh.since_last_visit().expect("ask"), None);
    }

    #[test]
    fn dirty_files_are_measured_against_head_whatever_the_baseline() {
        let fx = start_with(BaselineKind::LastVisit, None, |repo, dir| {
            write(dir, "a.md", "one\n");
            write(dir, "gone.md", "x\n");
            write(dir, "kept.md", "same\n");
            commit_all(repo, "init");
            None
        });
        assert_eq!(fx.handle.dirty_files().expect("ask"), Some(vec![]));
        write(fx.dir.path(), "a.md", "two\n");
        write(fx.dir.path(), "new.md", "x\n");
        std::fs::remove_file(fx.dir.path().join("gone.md")).expect("rm");
        assert_eq!(
            fx.handle.dirty_files().expect("ask"),
            Some(vec!["a.md".to_owned(), "new.md".to_owned()])
        );
    }

    #[test]
    fn folders_without_git_report_not_a_repo_until_one_appears() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = dunce::canonicalize(dir.path()).expect("canonicalize");
        let events = Arc::new(Mutex::new(Vec::<GitSnapshot>::new()));
        let sink_events = Arc::clone(&events);
        let h = GitHandle::start(
            GitSettings {
                vault_root: vault.clone(),
                home: None,
                baseline: BaselineKind::LastVisit,
                last_visit_oid: None,
                timings: FAST,
            },
            discover(&vault, None),
            Arc::new(move |s: &GitSnapshot| sink_events.lock().expect("lock").push(s.clone())),
        );
        let s = h.snapshot();
        assert!(!s.is_repo);
        assert_eq!(s.git_error, None);
        assert_eq!(h.since_last_visit().expect("ask"), None);
        assert_eq!(h.dirty_files().expect("ask"), None);

        init_repo(dir.path());
        write(dir.path(), "a.md", "x\n");
        h.trigger();
        let deadline = Instant::now() + Duration::from_secs(10);
        while events.lock().expect("lock").is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        let e = events.lock().expect("lock").last().cloned().expect("event");
        assert!(e.is_repo);
        assert!(e.head.unborn);
        assert_eq!(
            e.files.get("a.md").map(|f| f.state),
            Some(GitState::Untracked)
        );
    }

    #[test]
    fn an_unusable_repository_surfaces_its_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = dunce::canonicalize(dir.path()).expect("canonicalize");
        let h = GitHandle::start(
            GitSettings {
                vault_root: vault,
                home: None,
                baseline: BaselineKind::Head,
                last_visit_oid: None,
                timings: FAST,
            },
            Discovery::Error("The repository is owned by someone else".to_owned()),
            Arc::new(|_: &GitSnapshot| {}),
        );
        let s = h.snapshot();
        assert!(!s.is_repo);
        assert_eq!(
            s.git_error.as_deref(),
            Some("The repository is owned by someone else")
        );
    }

    #[test]
    fn shutdown_joins_the_thread_and_later_calls_degrade_gracefully() {
        let fx = committed(BaselineKind::Head);
        let tx = fx.handle.tx.clone();
        fx.handle.shutdown();
        assert!(tx.send(Msg::Trigger).is_err());
    }
}
