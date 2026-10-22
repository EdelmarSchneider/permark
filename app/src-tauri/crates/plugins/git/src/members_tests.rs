//! Member repositories: the machine of one repository (table), the pool with a fake backend (bound, order, coalescing,
//! stop, failures, lists, first commits, announcements). Real repositories: `members_real_tests.rs`.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::AtomicUsize;

use permark_core::state::{AppDirs, StateStore};

use super::*;
use crate::ops::{Baseline, Discovery, GitError, RawFile, RawStatus};
use crate::service::BackendChoice;
use crate::types::Letter;

use InterestLevel::{Detail, Glance};

// ---- the machine ---------------------------------------------------------------------------------------------------

#[test]
#[allow(clippy::too_many_lines)] // one row per (state, event)
fn the_repository_machine_table() {
    use RepoEvent as E;
    use RepoFsm as S;
    let failed = S::Failed {
        reason: "x".to_owned(),
    };
    let cases: Vec<(RepoFsm, RepoEvent, RepoFsm, Schedule)> = vec![
        // interest: asked at once, ahead of the queue
        (
            S::Cold,
            E::Interest(Glance),
            S::Queued { level: Glance },
            Schedule::Now,
        ),
        (
            S::Fresh { level: Glance },
            E::Interest(Glance),
            S::Fresh { level: Glance },
            Schedule::Keep,
        ),
        (
            S::Fresh { level: Detail },
            E::Interest(Glance),
            S::Fresh { level: Detail },
            Schedule::Keep,
        ),
        (
            S::Fresh { level: Glance },
            E::Interest(Detail),
            S::Queued { level: Detail },
            Schedule::Now,
        ),
        (
            S::Stale { level: Detail },
            E::Interest(Glance),
            S::Queued { level: Detail },
            Schedule::Now,
        ),
        (
            failed.clone(),
            E::Interest(Glance),
            S::Queued { level: Glance },
            Schedule::Now,
        ),
        (
            S::Queued { level: Glance },
            E::Interest(Detail),
            S::Queued { level: Detail },
            Schedule::Now,
        ),
        (
            S::Running {
                level: Glance,
                again: None,
            },
            E::Interest(Glance),
            S::Running {
                level: Glance,
                again: None,
            },
            Schedule::Keep,
        ),
        (
            S::Running {
                level: Glance,
                again: None,
            },
            E::Interest(Detail),
            S::Running {
                level: Glance,
                again: Some(Detail),
            },
            Schedule::Keep,
        ),
        // changes: debounced (coalesced)
        (
            S::Cold,
            E::Changed(Glance),
            S::Queued { level: Glance },
            Schedule::Debounced,
        ),
        (
            S::Fresh { level: Detail },
            E::Changed(Glance),
            S::Queued { level: Glance },
            Schedule::Debounced,
        ),
        (
            failed.clone(),
            E::Changed(Glance),
            S::Queued { level: Glance },
            Schedule::Debounced,
        ),
        (
            S::Queued { level: Detail },
            E::Changed(Glance),
            S::Queued { level: Detail },
            Schedule::Debounced,
        ),
        (
            S::Running {
                level: Detail,
                again: None,
            },
            E::Changed(Glance),
            S::Running {
                level: Detail,
                again: Some(Glance),
            },
            Schedule::Keep,
        ),
        // rescans
        (
            S::Fresh { level: Glance },
            E::Outdated,
            S::Stale { level: Glance },
            Schedule::Keep,
        ),
        (S::Cold, E::Outdated, S::Cold, Schedule::Keep),
        (
            S::Queued { level: Glance },
            E::Outdated,
            S::Queued { level: Glance },
            Schedule::Keep,
        ),
        // answers
        (
            S::Running {
                level: Detail,
                again: None,
            },
            E::Done(Ok(())),
            S::Fresh { level: Detail },
            Schedule::Keep,
        ),
        (
            S::Running {
                level: Glance,
                again: None,
            },
            E::Done(Err("x".to_owned())),
            failed.clone(),
            Schedule::Keep,
        ),
        (
            S::Running {
                level: Glance,
                again: Some(Detail),
            },
            E::Done(Err("x".to_owned())),
            S::Queued { level: Detail },
            Schedule::Debounced,
        ),
        (
            S::Running {
                level: Glance,
                again: Some(Glance),
            },
            E::Done(Ok(())),
            S::Queued { level: Glance },
            Schedule::Debounced,
        ),
        (S::Cold, E::Done(Ok(())), S::Cold, Schedule::Keep),
        (
            S::Fresh { level: Glance },
            E::Done(Err("late".to_owned())),
            S::Fresh { level: Glance },
            Schedule::Keep,
        ),
    ];
    for (state, event, want, schedule) in cases {
        assert_eq!(
            step(&state, &event),
            (want.clone(), schedule),
            "{state:?} + {event:?}"
        );
    }
}

#[test]
fn random_walks_keep_the_machine_whole() {
    // whatever happens, a queued | running repository is pending and an answer never invents a level
    let events = [
        RepoEvent::Interest(Glance),
        RepoEvent::Interest(Detail),
        RepoEvent::Changed(Glance),
        RepoEvent::Changed(Detail),
        RepoEvent::Outdated,
        RepoEvent::Done(Ok(())),
        RepoEvent::Done(Err("e".to_owned())),
    ];
    let mut state = RepoFsm::Cold;
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..10_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let event = &events[usize::try_from(seed % 7).unwrap()];
        // the pool starts queued work: a queued repository may be running before an answer
        if let RepoFsm::Queued { level } = state {
            if seed.is_multiple_of(3) {
                state = RepoFsm::Running { level, again: None };
            }
        }
        let (next, schedule) = step(&state, event);
        if schedule != Schedule::Keep {
            assert!(
                matches!(next, RepoFsm::Queued { .. }),
                "{state:?} + {event:?}"
            );
        }
        if let (RepoFsm::Running { level, again }, RepoEvent::Done(_)) = (&state, event) {
            match again {
                Some(a) => assert_eq!(next, RepoFsm::Queued { level: *a }),
                None => assert!(
                    matches!(next, RepoFsm::Fresh { level: l } if l == *level)
                        || matches!(next, RepoFsm::Failed { .. })
                ),
            }
        }
        state = next;
    }
}

// ---- a fake backend ------------------------------------------------------------------------------------------------

/// Answers from memory; counts calls at once (status, root commit, blob: each one "git process"); each takes
/// `delay`; folders named `bad*` fail. `stubborn`: cannot be cancelled (as the built-in backend).
struct Fake {
    delay: Duration,
    now: AtomicUsize,
    most: AtomicUsize,
    order: Mutex<Vec<String>>,
    roots: AtomicUsize,
    files: usize,
    stubborn: bool,
}

impl Fake {
    fn new(delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            delay,
            now: AtomicUsize::new(0),
            most: AtomicUsize::new(0),
            order: Mutex::new(Vec::new()),
            roots: AtomicUsize::new(0),
            files: 2,
            stubborn: false,
        })
    }

    fn stubborn(delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            stubborn: true,
            ..Arc::into_inner(Self::new(delay)).unwrap()
        })
    }

    fn name(location: &RepoLocation) -> String {
        location
            .workdir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }
}

/// The fake with a stop flag: a cancelled call ends early.
struct FakeCall {
    fake: Arc<Fake>,
    cancel: Option<Arc<AtomicBool>>,
}

impl FakeCall {
    /// One "git process": counted while it runs, `delay` long unless cancelled.
    fn busy(&self) {
        let fake = &self.fake;
        let now = fake.now.fetch_add(1, Ordering::SeqCst) + 1;
        fake.most.fetch_max(now, Ordering::SeqCst);
        let started = Instant::now();
        while started.elapsed() < fake.delay {
            if self
                .cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::SeqCst))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        fake.now.fetch_sub(1, Ordering::SeqCst);
    }
}

impl GitOps for FakeCall {
    fn kind(&self) -> BackendKind {
        BackendKind::Builtin
    }

    fn discover(&self, _: &Path, _: Option<&Path>) -> Discovery {
        Discovery::NoRepo
    }

    fn status(&self, location: &RepoLocation, _: &Baseline) -> Result<RawStatus, GitError> {
        let fake = &self.fake;
        lock(&fake.order).push(Fake::name(location));
        self.busy();
        if Fake::name(location).starts_with("bad") {
            return Err(GitError::Failed("detected dubious ownership".to_owned()));
        }
        Ok(RawStatus {
            head: HeadInfo {
                branch: Some("main".to_owned()),
                oid: Some("a".repeat(40)),
                detached: false,
                unborn: false,
            },
            files: (0..fake.files)
                .map(|n| RawFile {
                    path: format!("f{n}.md"),
                    letter: Letter::Modified,
                    staged: false,
                    from: None,
                })
                .collect(),
            truncated: false,
        })
    }

    fn blob_at_head(&self, _: &RepoLocation, _: &str) -> Result<Option<Vec<u8>>, GitError> {
        self.busy();
        Ok(Some(b"committed".to_vec()))
    }

    fn commit_exists(&self, _: &RepoLocation, _: &str) -> bool {
        true
    }

    fn commit_count(&self, _: &RepoLocation, _: &str, _: &str) -> Option<u32> {
        None
    }

    fn ahead_behind(&self, _: &RepoLocation) -> Result<Option<(u32, u32)>, GitError> {
        Ok(Some((2, 1)))
    }

    fn last_commit(&self, _: &RepoLocation) -> Result<Option<(i64, String)>, GitError> {
        Ok(Some((1_700_000_000_000, "subject".to_owned())))
    }

    fn root_commit(&self, _: &RepoLocation) -> Result<Option<String>, GitError> {
        self.fake.roots.fetch_add(1, Ordering::SeqCst);
        self.busy();
        Ok(Some("b".repeat(40)))
    }

    fn cancellable(&self, flag: &Arc<AtomicBool>) -> Option<Arc<dyn GitOps>> {
        if self.fake.stubborn {
            return None;
        }
        Some(Arc::new(Self {
            fake: Arc::clone(&self.fake),
            cancel: Some(Arc::clone(flag)),
        }))
    }
}

#[derive(Default)]
struct Heard {
    repos: Mutex<Vec<ReposChanged>>,
    snapshots: Mutex<Vec<ProjectSnapshot>>,
    first: Mutex<Vec<Vec<(String, String)>>>,
    excluded: Mutex<Vec<Vec<String>>>,
    roots: Mutex<Vec<(String, Option<String>)>>,
}

fn quick() -> Timings {
    Timings {
        quiet: Duration::from_millis(30),
        max_wait: Duration::from_millis(200),
        lock_wait: Duration::from_millis(150),
    }
}

fn env(backend: Option<Arc<dyn GitOps>>, heard: &Arc<Heard>, visits: Option<PathBuf>) -> Env {
    let choice = match backend {
        Some(b) => BackendChoice::Fixed(b),
        None => BackendChoice::Pref(permark_core::settings::GitBackendPref::System),
    };
    let (h1, h2, h3, h4, h5) = (
        Arc::clone(heard),
        Arc::clone(heard),
        Arc::clone(heard),
        Arc::clone(heard),
        Arc::clone(heard),
    );
    Env {
        backends: Arc::new(BackendCell::new(choice)),
        timings: quick(),
        on_repos: Arc::new(move |r| lock(&h1.repos).push(r.clone())),
        on_snapshot: Arc::new(move |s| lock(&h2.snapshots).push(s.clone())),
        baseline: Arc::new(|| BaselineKind::Head),
        exclude: Arc::new(move |keys| lock(&h3.excluded).push(keys)),
        visit_file: Arc::new(move |path: &str| {
            let base = visits.as_ref()?;
            let dirs = AppDirs {
                profile: base.join(path.replace('/', "_")),
                cache: base.join("cache"),
            };
            Some(StateStore::app(&dirs).file("visit.json"))
        }),
        first_commits: Arc::new(move |learned| lock(&h4.first).push(learned.to_vec())),
        root_commits: Arc::new(move |answers| lock(&h5.roots).extend(answers.iter().cloned())),
        announce_after: Duration::from_millis(20),
        first_commit_after: Duration::from_millis(100),
    }
}

/// `n` member folders `p<i>` (each with a `.git` folder holding HEAD) below a temp root.
fn members_in(root: &Path, names: &[String]) -> Vec<MemberRef> {
    names
        .iter()
        .map(|name| {
            let folder = root.join(name);
            std::fs::create_dir_all(folder.join(".git")).unwrap();
            std::fs::write(folder.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
            MemberRef {
                uid: format!("u{name}"),
                path: format!("hubs/{name}"),
                key: format!("hubs/{name}"),
                folder,
                first_commit: false,
            }
        })
        .collect()
}

fn names(n: usize, prefix: &str) -> Vec<String> {
    (0..n).map(|i| format!("{prefix}{i:02}")).collect()
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn fake_backend(fake: &Arc<Fake>) -> Arc<dyn GitOps> {
    Arc::new(FakeCall {
        fake: Arc::clone(fake),
        cancel: None,
    })
}

fn settled(members: &Members) -> bool {
    members
        .summaries()
        .iter()
        .all(|s| !s.pending && s.changed.is_some())
}

// ---- the pool ------------------------------------------------------------------------------------------------------

#[test]
fn members_start_cold_with_cheap_facts_and_no_git_work() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::ZERO);
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    members.sync(&[]);
    assert_eq!(members.threads(), 0, "a plain folder starts nothing");
    let list = members_in(dir.path(), &names(3, "p"));
    members.sync(&list);
    let all = members.summaries();
    assert_eq!(all.len(), 3);
    assert!(all.iter().all(|s| s.branch.as_deref() == Some("main")
        && s.unborn
        && s.changed.is_none()
        && !s.pending));
    assert_eq!(
        lock(&heard.excluded).last().unwrap().len(),
        3,
        "the root leaves their files out"
    );
    wait_until("the first announcement", || !lock(&heard.repos).is_empty());
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(members.counters().1, 0, "no git work at open");
    assert_eq!(fake.most.load(Ordering::SeqCst), 0);
}

#[test]
fn thirty_four_repositories_changing_at_once_never_run_more_than_two_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::from_millis(15));
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let list = members_in(dir.path(), &names(34, "p"));
    members.sync(&list);
    // a checkout in each: many batches for each repository
    for _ in 0..5 {
        let all: Vec<(String, bool)> = list.iter().map(|m| (m.uid.clone(), true)).collect();
        members.changed(&all);
    }
    wait_until("every repository answered", || settled(&members));
    let (most, jobs, announcements) = members.counters();
    assert!(most <= POOL_THREADS, "{most} at once");
    assert_eq!(fake.most.load(Ordering::SeqCst), most.min(2));
    assert!((34..=34 * 2).contains(&jobs), "coalesced: {jobs} jobs");
    assert!(announcements < 34, "events bounded: {announcements}");
    assert!(members.summaries().iter().all(|s| s.changed == Some(2)));
}

#[test]
fn a_burst_of_changes_to_one_repository_is_one_look() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::ZERO);
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let list = members_in(dir.path(), &names(1, "p"));
    members.sync(&list);
    for _ in 0..50 {
        members.changed(&[(list[0].uid.clone(), false)]);
    }
    wait_until("the answer", || settled(&members));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(members.counters().1, 1);
}

#[test]
fn interest_jumps_ahead_of_the_queue() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::from_millis(20));
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let list = members_in(dir.path(), &names(12, "p"));
    members.sync(&list);
    let all: Vec<(String, bool)> = list.iter().map(|m| (m.uid.clone(), false)).collect();
    members.changed(&all);
    // the debounced queue waits; the card asks for the last one
    members.interest("main", Detail, &[list[11].uid.clone()]);
    wait_until("every repository answered", || settled(&members));
    let order = lock(&fake.order).clone();
    let at = order.iter().position(|n| n == "p11").unwrap();
    assert!(at < 2, "interest ran first: {order:?}");
    let card = members
        .summaries()
        .into_iter()
        .find(|s| s.id == "up11")
        .unwrap();
    assert_eq!(card.ahead_behind, Some([2, 1]));
    assert_eq!(
        card.last_commit.as_ref().map(|c| c.subject.as_str()),
        Some("subject")
    );
    let other = members
        .summaries()
        .into_iter()
        .find(|s| s.id == "up00")
        .unwrap();
    assert_eq!(other.ahead_behind, None, "Glance only");
}

#[test]
fn stopping_a_repository_cancels_its_work_and_drops_its_answer() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::from_secs(5));
    let heard = Arc::new(Heard::default());
    let visits = dir.path().join("visits");
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, Some(visits.clone())));
    let list = members_in(dir.path(), &names(2, "p"));
    members.sync(&list);
    members.interest("main", Glance, &[list[0].uid.clone()]);
    wait_until("the look started", || fake.now.load(Ordering::SeqCst) == 1);
    let started = Instant::now();
    members.sync(&list[1..]);
    wait_until("the look stopped", || fake.now.load(Ordering::SeqCst) == 0);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "cancelled, not waited out"
    );
    assert_eq!(members.held(), 1);
    std::thread::sleep(Duration::from_millis(80));
    assert!(members.summaries().iter().all(|s| s.id != list[0].uid));
    assert!(lock(&heard.repos)
        .iter()
        .any(|r| r.removed.contains(&list[0].uid)));
    // drop twice | drop while a look runs: joined, nothing left
    members.interest("main", Glance, &[list[1].uid.clone()]);
    wait_until("the second look", || fake.now.load(Ordering::SeqCst) == 1);
    let started = Instant::now();
    drop(members);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(fake.now.load(Ordering::SeqCst), 0);
}

#[test]
fn one_failing_repository_fails_alone() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::ZERO);
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let mut all = names(3, "p");
    all.push("bad".to_owned());
    let list = members_in(dir.path(), &all);
    members.sync(&list);
    let ids: Vec<String> = list.iter().map(|m| m.uid.clone()).collect();
    members.interest("main", Glance, &ids);
    wait_until("answers", || members.summaries().iter().all(|s| !s.pending));
    for s in members.summaries() {
        if s.id == "ubad" {
            assert!(
                matches!(&s.error, Some(RepoError::Failed { reason }) if reason.contains("dubious")),
                "{s:?}"
            );
            assert_eq!(s.branch.as_deref(), Some("main"), "cheap facts stay");
        } else {
            assert_eq!(s.error, None);
            assert_eq!(s.changed, Some(2));
        }
    }
}

#[test]
fn a_member_whose_gitfile_points_nowhere_is_a_row_error_without_git_work() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::ZERO);
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let folder = dir.path().join("wt");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(".git"),
        "gitdir: ../nowhere/.git/worktrees/wt\n",
    )
    .unwrap();
    members.sync(&[MemberRef {
        uid: "uwt".to_owned(),
        path: "wt".to_owned(),
        key: "wt".to_owned(),
        folder,
        first_commit: true,
    }]);
    members.interest("main", Glance, &["uwt".to_owned()]);
    wait_until("the answer", || !members.summaries()[0].pending);
    let s = &members.summaries()[0];
    assert!(matches!(&s.error, Some(RepoError::Failed { .. })), "{s:?}");
    assert_eq!(s.branch, None);
    assert_eq!(members.counters().1, 0, "no git work");
}

#[test]
fn git_missing_shows_no_branch_anywhere() {
    let dir = tempfile::tempdir().unwrap();
    let heard = Arc::new(Heard::default());
    let mut env = env(None, &heard, None);
    // the installed git chosen and missing
    env.backends = Arc::new(BackendCell::decided(None));
    let members = Members::new(env);
    let list = members_in(dir.path(), &names(2, "p"));
    members.sync(&list);
    members.interest("main", Glance, &[list[0].uid.clone()]);
    wait_until("the answer", || {
        members.summaries().iter().all(|s| !s.pending)
    });
    for s in members.summaries() {
        assert_eq!(s.error, Some(RepoError::Unavailable));
        assert_eq!(s.branch, None);
        assert_eq!(s.changed, None);
    }
}

#[test]
fn file_lists_are_kept_for_eight_repositories_at_most() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::ZERO);
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let list = members_in(dir.path(), &names(12, "p"));
    members.sync(&list);
    for (n, m) in list.iter().enumerate() {
        members.interest(&format!("w{n}"), Glance, std::slice::from_ref(&m.uid));
    }
    wait_until("answers", || settled(&members));
    let lists = list
        .iter()
        .filter(|m| members.snapshot(&m.uid).is_some())
        .count();
    assert_eq!(lists, MAX_LISTS);
    let snapshot = members.snapshot(&list[11].uid).unwrap();
    assert_eq!(
        snapshot.files[0].path, "hubs/p11/f0.md",
        "root-relative paths"
    );
    assert!(
        members.summaries().iter().all(|s| s.changed == Some(2)),
        "counts kept"
    );
    assert!(!lock(&heard.snapshots).is_empty());
    // without interest a change keeps no list
    let dir2 = tempfile::tempdir().unwrap();
    let quiet = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let one = members_in(dir2.path(), &names(1, "q"));
    quiet.sync(&one);
    quiet.changed(&[(one[0].uid.clone(), false)]);
    wait_until("answer", || settled(&quiet));
    assert!(quiet.snapshot(&one[0].uid).is_none());
}

#[test]
fn first_commits_are_learned_once_and_handed_over_in_one_batch() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::ZERO);
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let mut list = members_in(dir.path(), &names(5, "p"));
    list[4].first_commit = true;
    members.sync(&list);
    let all: Vec<(String, bool)> = list.iter().map(|m| (m.uid.clone(), false)).collect();
    members.changed(&all);
    wait_until("one batch", || !lock(&heard.first).is_empty());
    // a second round learns nothing new
    members.changed(&all);
    wait_until("answers", || settled(&members));
    std::thread::sleep(Duration::from_millis(200));
    let batches = lock(&heard.first).clone();
    assert_eq!(batches.len(), 1, "{batches:?}");
    let learned: HashSet<&str> = batches[0].iter().map(|(u, _)| u.as_str()).collect();
    assert_eq!(learned.len(), 4, "not the one the record keeps");
    assert!(!learned.contains("up04"));
    assert_eq!(
        fake.roots.load(Ordering::SeqCst),
        4,
        "asked once per repository"
    );
}

#[test]
fn a_moved_member_keeps_its_session_and_a_visit_is_remembered_where_it_lives() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::ZERO);
    let heard = Arc::new(Heard::default());
    let visits = dir.path().join("visits");
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, Some(visits.clone())));
    let mut list = members_in(dir.path(), &names(1, "p"));
    members.sync(&list);
    members.interest("main", Glance, &[list[0].uid.clone()]);
    wait_until("answer", || settled(&members));
    list[0].path = "moved/p00".to_owned();
    list[0].key = "moved/p00".to_owned();
    members.sync(&list);
    assert_eq!(members.summaries()[0].path, "moved/p00");
    assert!(
        members.snapshot(&list[0].uid).is_none(),
        "old paths dropped"
    );
    drop(members);
    let file = visits.join("moved_p00").join("visit.json");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains(&"a".repeat(40)), "{text}");
}

/// Ordering, not elapsed time: the window is far longer than any runner stall, so only the lock going away can
/// release the refresh. No git call while `index.lock` exists; one soon after it is removed.
#[test]
fn the_index_lock_is_waited_out_then_git_runs_once_it_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::ZERO);
    let heard = Arc::new(Heard::default());
    let mut env = env(Some(fake_backend(&fake)), &heard, None);
    env.timings.lock_wait = Duration::from_secs(60);
    let members = Members::new(env);
    let list = members_in(dir.path(), &names(1, "p"));
    let lock_file = list[0].folder.join(".git/index.lock");
    std::fs::write(&lock_file, "").unwrap();
    members.sync(&list);
    let started = Instant::now();
    members.interest("main", Glance, &[list[0].uid.clone()]);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(members.counters().1, 0, "waits while git writes the index");
    std::fs::remove_file(&lock_file).unwrap();
    wait_until("answer after the lock is gone", || settled(&members));
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "released by the lock going, not by the wait running out"
    );
    assert_eq!(members.counters().1, 1, "one look once the lock is gone");
}

/// A lock that never goes (a stale file) holds the refresh back only for `lock_wait`, then the look is made anyway.
#[test]
fn a_stale_index_lock_delays_the_look_by_the_wait_and_no_more() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::ZERO);
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let list = members_in(dir.path(), &names(1, "p"));
    std::fs::write(list[0].folder.join(".git/index.lock"), "").unwrap();
    members.sync(&list);
    let started = Instant::now();
    members.interest("main", Glance, &[list[0].uid.clone()]);
    wait_until("answer despite the stale lock", || settled(&members));
    assert!(
        started.elapsed() >= quick().lock_wait,
        "waited the window out first"
    );
}

// ---- fix round (verifier round 3: S2, S3, N3) ------------------------------------------------------------------------

/// Folders to look up (`(key, folder)`), each a repository on disk.
fn lookups_in(root: &Path, n: usize) -> Vec<(String, PathBuf)> {
    members_in(root, &names(n, "x"))
        .into_iter()
        .map(|m| (m.key, m.folder))
        .collect()
}

/// S2: root-commit lookups, member work and other plugins' member reads share one bound: never more than
/// [`POOL_THREADS`] member git processes at once, whatever arrives together.
#[test]
fn lookups_member_work_and_callers_reads_never_run_more_than_two_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::from_millis(10));
    let heard = Arc::new(Heard::default());
    let members = Arc::new(Members::new(env(Some(fake_backend(&fake)), &heard, None)));
    let list = members_in(dir.path(), &names(20, "p"));
    members.sync(&list);
    let lookups = lookups_in(&dir.path().join("found"), 12);
    members.look_up_roots(lookups.clone());
    let all: Vec<(String, bool)> = list.iter().map(|m| (m.uid.clone(), true)).collect();
    members.changed(&all);
    let backend = fake_backend(&fake);
    let readers: Vec<_> = (0..4)
        .map(|n| {
            let members = Arc::clone(&members);
            let backend = Arc::clone(&backend);
            let location = members.location(&list[n].uid).unwrap();
            std::thread::spawn(move || {
                for _ in 0..5 {
                    let blob = members
                        .with_permit(|| backend.blob_at_head(&location, "a.md").unwrap())
                        .flatten();
                    assert_eq!(blob.as_deref(), Some(&b"committed"[..]));
                }
            })
        })
        .collect();
    for reader in readers {
        reader.join().unwrap();
    }
    wait_until("every lookup answered", || lock(&heard.roots).len() == 12);
    wait_until("every repository answered", || settled(&members));
    let (most, _, _) = members.counters();
    assert!(most <= POOL_THREADS, "{most} at once");
    assert!(
        fake.most.load(Ordering::SeqCst) <= POOL_THREADS,
        "{} git processes at once",
        fake.most.load(Ordering::SeqCst)
    );
    assert_eq!(members.lookups_run(), 12, "asked once per folder");
    let keys: HashSet<String> = lock(&heard.roots).iter().map(|(k, _)| k.clone()).collect();
    assert_eq!(keys.len(), 12);
    assert!(lock(&heard.roots)
        .iter()
        .all(|(_, c)| c.as_deref() == Some(&"b".repeat(40)[..])));
}

/// S2: no git -> lookups answer "none" without a process; a folder asked twice while queued is looked up once.
#[test]
fn lookups_without_git_answer_none_and_repeats_coalesce() {
    let dir = tempfile::tempdir().unwrap();
    let heard = Arc::new(Heard::default());
    let mut no_git = env(None, &heard, None);
    no_git.backends = Arc::new(BackendCell::decided(None));
    let members = Members::new(no_git);
    let lookups = lookups_in(dir.path(), 3);
    members.look_up_roots(lookups.clone());
    members.look_up_roots(lookups);
    wait_until("answers", || lock(&heard.roots).len() >= 3);
    std::thread::sleep(Duration::from_millis(50));
    let answers = lock(&heard.roots).clone();
    assert!(answers.iter().all(|(_, c)| c.is_none()), "{answers:?}");
    assert!(answers.len() <= 6);
    assert_eq!(members.lookups_run(), 0, "no git process");
}

/// S2: closing cancels a running lookup at once (installed git killed) and answers nothing more.
#[test]
fn dropping_cancels_running_lookups_and_answers_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::from_secs(10));
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    members.look_up_roots(lookups_in(dir.path(), 4));
    wait_until("lookups running", || fake.now.load(Ordering::SeqCst) == 2);
    let started = Instant::now();
    drop(members);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert!(lock(&heard.roots).is_empty(), "no answer after close");
}

/// S3: a view that let go of the project takes its interest with it (every level), other views keep theirs.
#[test]
fn a_caller_that_goes_takes_its_interest_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(Duration::ZERO);
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let list = members_in(dir.path(), &names(3, "p"));
    members.sync(&list);
    let ids: Vec<String> = list.iter().map(|m| m.uid.clone()).collect();
    members.interest("v-1/7", Glance, &ids);
    members.interest("v-1/7", Detail, &ids[..1]);
    members.interest("v-1", Glance, &ids[1..]);
    members.interest("v-12/3", Glance, &ids[2..]);
    wait_until("answers", || settled(&members));
    members.forget_caller("v-1");
    let st = lock(&members.pool.state);
    let left: Vec<&(String, InterestLevel)> = st.interest.keys().collect();
    assert_eq!(
        left,
        [&("v-12/3".to_owned(), Glance)],
        "only the other view"
    );
    assert_eq!(st.interest_of(&ids[0]), None);
    assert_eq!(st.interest_of(&ids[2]), Some(Glance));
}

/// N3: closing never waits for a running built-in (not cancellable) read; it ends on its own later.
#[test]
fn closing_does_not_wait_for_a_read_that_cannot_be_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::stubborn(Duration::from_millis(1500));
    let heard = Arc::new(Heard::default());
    let members = Members::new(env(Some(fake_backend(&fake)), &heard, None));
    let list = members_in(dir.path(), &names(1, "p"));
    members.sync(&list);
    members.interest("v-1", Glance, &[list[0].uid.clone()]);
    wait_until("the read runs", || fake.now.load(Ordering::SeqCst) == 1);
    let started = Instant::now();
    drop(members);
    assert!(
        started.elapsed() < Duration::from_millis(1000),
        "{:?}",
        started.elapsed()
    );
    // it ends on its own
    wait_until("the read ended", || fake.now.load(Ordering::SeqCst) == 0);
}
