//! Live moves of an included project (`projects.md` §4.4 step 1, real fs renames under the real watch): the folder
//! renamed | moved while open keeps its id, the record path follows surgically, one `moved` row, never offered as new.

use super::*;

const UID: &str = "bbbbbbbbbb";

/// Root with `inner/beta` (own `.git`) included in the record; the core open, ready, its news collected.
struct Live {
    base: tempfile::TempDir,
    dir: tempfile::TempDir,
    core: Arc<ProjectCore>,
    news: Arc<Mutex<Vec<ProjectEvent>>>,
    _sub: ProjectsSubscription,
}

impl Live {
    fn open(extra: impl FnOnce(&Path)) -> Self {
        let base = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, "inner/beta");
        fs::write(root.join("inner/beta/plan.md"), "# plan").unwrap();
        permark(root, "");
        fs::write(
            root.join(".permark/projects.toml"),
            format!("# kept\r\n[[project]]\r\nid = \"{UID}\"\r\npath = \"inner/beta\"\r\n"),
        )
        .unwrap();
        extra(root);
        let (env, _sink) = env(base.path());
        let core = ProjectCore::open(Root::open(root).unwrap(), &env);
        let view = ready(&core);
        assert_eq!(state(&view, "inner/beta"), Some(MemberState::Included));
        let news: Arc<Mutex<Vec<ProjectEvent>>> = Arc::default();
        let sub = core.projects().subscribe({
            let news = Arc::clone(&news);
            move |e| news.lock().unwrap().push(e.clone())
        });
        let live = Self {
            base,
            dir,
            core,
            news,
            _sub: sub,
        };
        // the OS watch is running before the first move
        live.settled();
        live
    }

    /// Everything that happened so far went through the watch and the projects worker (board item 9: was a fixed
    /// sleep). A marker file goes through the watch twice: a path the watch looks at again (`deferred`, at most
    /// once) comes at the latest with the second marker's batch. The hub calls this test's subscriber after the
    /// project's own, so every batch of the projects worker is queued by then; its answer to `settle` comes after them.
    fn settled(&self) {
        static MARKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let _sub = self.core.hub().subscribe({
            let seen = Arc::clone(&seen);
            move |batch| {
                seen.lock()
                    .unwrap()
                    .extend(batch.changes.iter().map(|c| c.path.clone()));
            }
        });
        for _ in 0..2 {
            let n = MARKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mark = format!("settle-{n}.txt");
            fs::write(self.root().join(&mark), "x").unwrap();
            wait(&format!("the watch saw {mark}"), || {
                seen.lock().unwrap().contains(&mark)
            });
        }
        assert!(
            self.core.projects().settle(Duration::from_secs(10)),
            "the projects worker answered"
        );
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn record(&self) -> String {
        fs::read_to_string(self.root().join(".permark/projects.toml")).unwrap()
    }

    /// Waits until the project is Included at `path` (same id), then checks what the person sees: the record line
    /// changed alone, one `moved` row, no "gone" | "appeared" row, no strip, no question.
    fn kept_at(&self, path: &str) {
        wait(&format!("included at {path}"), || {
            row(&self.core.projects_view(), path).is_some_and(|r| {
                r.state == MemberState::Included
                    && r.uid.as_ref().map(ProjectUid::as_str) == Some(UID)
            })
        });
        // late batches of the same move have had their chance to do harm
        self.settled();
        let view = self.core.projects_view();
        assert_eq!(view.rows.len(), 1, "{view:#?}");
        assert_eq!(state(&view, path), Some(MemberState::Included));
        assert_eq!(view.offered.count, 0, "no new-project strip: {view:#?}");
        assert!(view.ask.is_none(), "no move question: {view:#?}");
        assert_eq!(
            self.record(),
            format!("# kept\r\n[[project]]\r\nid = \"{UID}\"\r\npath = \"{path}\"\r\n"),
            "only the path changed"
        );
        let news = self.news.lock().unwrap().clone();
        assert!(
            news.iter()
                .any(|e| matches!(e, ProjectEvent::Moved { path: p, .. } if p == path)),
            "{news:?}"
        );
        assert!(
            !news.iter().any(|e| matches!(
                e,
                ProjectEvent::Gone { .. }
                    | ProjectEvent::Appeared { .. }
                    | ProjectEvent::Back { .. }
            )),
            "{news:?}"
        );
        assert_eq!(self.core.projects().held_members(), 1);
    }
}

#[test]
fn a_project_renamed_while_open_keeps_its_identity() {
    let live = Live::open(|_| {});
    fs::rename(
        live.root().join("inner/beta"),
        live.root().join("inner/beta2"),
    )
    .unwrap();
    live.kept_at("inner/beta2");
}

#[test]
fn a_project_moved_to_another_parent_while_open_keeps_its_identity() {
    let live = Live::open(|root| mkdirs(root, "other"));
    fs::rename(
        live.root().join("inner/beta"),
        live.root().join("other/beta"),
    )
    .unwrap();
    live.kept_at("other/beta");
}

#[test]
fn a_project_moved_and_renamed_to_another_parent_keeps_its_identity() {
    let live = Live::open(|root| mkdirs(root, "other"));
    fs::rename(
        live.root().join("inner/beta"),
        live.root().join("other/gamma"),
    )
    .unwrap();
    live.kept_at("other/gamma");
}

#[test]
fn a_renamed_container_carries_the_project_along() {
    let live = Live::open(|_| {});
    fs::rename(live.root().join("inner"), live.root().join("outer")).unwrap();
    live.kept_at("outer/beta");
}

#[test]
fn a_rename_then_an_edit_inside_at_once_keeps_its_identity() {
    let live = Live::open(|_| {});
    fs::rename(
        live.root().join("inner/beta"),
        live.root().join("inner/beta2"),
    )
    .unwrap();
    fs::write(live.root().join("inner/beta2/plan.md"), "# plan, edited").unwrap();
    fs::write(live.root().join("inner/beta2/new.md"), "# new").unwrap();
    live.kept_at("inner/beta2");
}

#[test]
fn a_project_renamed_twice_quickly_ends_where_it_landed() {
    let live = Live::open(|_| {});
    fs::rename(
        live.root().join("inner/beta"),
        live.root().join("inner/beta2"),
    )
    .unwrap();
    fs::rename(
        live.root().join("inner/beta2"),
        live.root().join("inner/beta3"),
    )
    .unwrap();
    live.kept_at("inner/beta3");
}

#[test]
fn a_project_renamed_twice_slowly_ends_where_it_landed() {
    let live = Live::open(|_| {});
    fs::rename(
        live.root().join("inner/beta"),
        live.root().join("inner/beta2"),
    )
    .unwrap();
    wait("first move", || {
        state(&live.core.projects_view(), "inner/beta2") == Some(MemberState::Included)
    });
    fs::rename(
        live.root().join("inner/beta2"),
        live.root().join("inner/beta3"),
    )
    .unwrap();
    live.kept_at("inner/beta3");
}

/// Moves the project out of the opened folder (the watch sees only a removal) and waits until it is held.
fn away_and_held(live: &Live) -> std::path::PathBuf {
    let away = live.base.path().join("away");
    fs::rename(live.root().join("inner/beta"), &away).unwrap();
    wait("held", || live.core.projects().vanished_count() == 1);
    let view = live.core.projects_view();
    assert_eq!(
        state(&view, "inner/beta"),
        Some(MemberState::Included),
        "{view:#?}"
    );
    assert_eq!(view.offered.count, 0);
    away
}

fn news_of(live: &Live) -> Vec<ProjectEvent> {
    live.news.lock().unwrap().clone()
}

/// `(gone, appeared)` rows told so far.
fn told(live: &Live) -> (usize, usize) {
    let news = news_of(live);
    let count = |f: fn(&ProjectEvent) -> bool| news.iter().filter(|e| f(e)).count();
    (
        count(|e| matches!(e, ProjectEvent::Gone { .. })),
        count(|e| matches!(e, ProjectEvent::Appeared { .. })),
    )
}

/// A move whose halves come in two batches (the watch cannot pair them; macOS splits a rename so): the folder that
/// comes during the window is the same folder on disk -> one `moved` row, no gone, no strip. The clock is stopped: the
/// window cannot end by itself here.
#[test]
fn a_move_split_across_batches_keeps_the_project() {
    let live = Live::open(|_| {});
    live.core.projects().freeze_clock();
    let away = away_and_held(&live);
    assert!(news_of(&live).is_empty(), "nothing told while held");
    fs::rename(&away, live.root().join("inner/beta3")).unwrap();
    live.kept_at("inner/beta3");
    assert_eq!(live.core.projects().vanished_count(), 0);
}

/// Gone for good: told once the window ends (the clock moved by hand), not before; a folder that came meanwhile (same
/// name, another folder on disk) waited, then is offered, never silently paired.
#[test]
fn a_lookalike_during_the_window_waits_and_is_never_paired() {
    let live = Live::open(|root| mkdirs(root, "x"));
    live.core.projects().freeze_clock();
    let _away = away_and_held(&live);
    git(live.root(), "x/beta");
    wait("waiting", || live.core.projects().waiting_count() == 1);
    let view = live.core.projects_view();
    assert_eq!(view.offered.count, 0, "no strip while waiting: {view:#?}");
    assert_eq!(state(&view, "inner/beta"), Some(MemberState::Included));
    assert!(news_of(&live).is_empty(), "{:?}", news_of(&live));
    live.core.projects().advance(grace::GRACE);
    wait("gone", || {
        state(&live.core.projects_view(), "inner/beta") == Some(MemberState::Missing)
    });
    wait("offered", || live.core.projects_view().offered.count == 1);
    assert!(
        live.record().contains("path = \"inner/beta\""),
        "never re-attached silently"
    );
    // rows go out after the map (the view may show the end state first)
    wait("rows", || told(&live) == (1, 1));
    // late batches have had their chance to add a row
    live.settled();
    let news = news_of(&live);
    assert_eq!(told(&live), (1, 1), "{news:?}");
    assert!(
        !news.iter().any(|e| matches!(e, ProjectEvent::Moved { .. })),
        "{news:?}"
    );
    assert_eq!(live.core.projects().waiting_count(), 0);
}

/// Back at the same place within the window (restored, checked out again): nothing told at all.
#[test]
fn a_project_back_in_place_within_the_window_tells_nothing() {
    let live = Live::open(|_| {});
    live.core.projects().freeze_clock();
    let away = away_and_held(&live);
    fs::rename(&away, live.root().join("inner/beta")).unwrap();
    wait("back", || live.core.projects().vanished_count() == 0);
    live.core.projects().advance(grace::GRACE * 2);
    // late batches of the same moves have had their chance to do harm
    live.settled();
    let view = live.core.projects_view();
    assert_eq!(state(&view, "inner/beta"), Some(MemberState::Included));
    assert_eq!(view.offered.count, 0);
    assert!(news_of(&live).is_empty(), "{:?}", news_of(&live));
    assert!(live.record().contains("path = \"inner/beta\""));
}

/// Closing while a window runs: the wake-up goes with the worker, nothing stays held, the late folder changes nothing.
#[test]
fn closing_during_a_window_drops_it() {
    let live = Live::open(|_| {});
    let away = away_and_held(&live);
    live.core.projects().stop();
    assert_eq!(live.core.projects().vanished_count(), 0);
    assert_eq!(live.core.projects().held_members(), 0);
    fs::rename(&away, live.root().join("inner/beta3")).unwrap();
    assert_eq!(live.core.projects().vanished_count(), 0);
    assert!(live.record().contains("path = \"inner/beta\""));
}

/// The real clock: with no folder coming back, the window ends by itself (bounded) and the project is told gone.
#[test]
fn the_window_ends_by_itself() {
    let live = Live::open(|_| {});
    let start = Instant::now();
    let _away = away_and_held(&live);
    wait("gone", || {
        state(&live.core.projects_view(), "inner/beta") == Some(MemberState::Missing)
    });
    assert!(
        start.elapsed() >= grace::GRACE,
        "not before the window ends"
    );
    assert_eq!(live.core.projects().vanished_count(), 0);
    wait("gone row", || told(&live).0 == 1);
    live.settled();
    assert_eq!(told(&live), (1, 0), "{:?}", news_of(&live));
}

#[test]
fn a_project_moved_into_an_ignored_folder_keeps_its_identity() {
    let live = Live::open(|root| {
        fs::write(root.join(".gitignore"), "stash/\n").unwrap();
        mkdirs(root, "stash");
    });
    fs::rename(
        live.root().join("inner/beta"),
        live.root().join("stash/beta"),
    )
    .unwrap();
    live.kept_at("stash/beta");
    assert!(
        listed(&live.core).iter().any(|p| p == "stash/beta/plan.md"),
        "an included project below an ignored folder is shown"
    );
}

#[test]
fn a_project_moved_into_a_dot_folder_keeps_its_identity() {
    let live = Live::open(|root| mkdirs(root, ".attic"));
    fs::rename(
        live.root().join("inner/beta"),
        live.root().join(".attic/beta"),
    )
    .unwrap();
    live.kept_at(".attic/beta");
}

/// Windows | macOS (case-insensitive): a case-only rename is the same folder; the record follows the new case.
#[test]
fn a_case_only_rename_keeps_the_project_and_follows_the_case() {
    if !crate::paths::CASE_INSENSITIVE {
        return;
    }
    let live = Live::open(|_| {});
    fs::rename(
        live.root().join("inner/beta"),
        live.root().join("inner/Beta"),
    )
    .unwrap();
    live.kept_at("inner/Beta");
}

/// Two unrelated changes in one batch (a listed project removed, another repo with the same name appearing elsewhere,
/// and a second one of that name): not one obvious pair -> no silent re-attach.
#[test]
fn a_removal_and_two_lookalikes_are_never_paired() {
    let live = Live::open(|_| {});
    let away = live.base.path().join("away");
    fs::rename(live.root().join("inner/beta"), &away).unwrap();
    git(live.root(), "x/beta");
    git(live.root(), "y/beta");
    wait("gone", || {
        state(&live.core.projects_view(), "inner/beta") == Some(MemberState::Missing)
    });
    wait("offered", || live.core.projects_view().offered.count == 2);
    assert!(live.record().contains("path = \"inner/beta\""));
}

/// Locate… opens at the old parent folder while it exists, else at the opened folder; never elsewhere.
#[test]
fn locate_starts_at_the_old_parent_else_at_the_opened_folder() {
    let live = Live::open(|_| {});
    let root = dunce::canonicalize(live.root()).unwrap();
    assert_eq!(
        row(&live.core.projects_view(), "inner/beta")
            .unwrap()
            .locate_from,
        None,
        "included: no picker"
    );
    fs::rename(
        live.root().join("inner/beta"),
        live.base.path().join("away"),
    )
    .unwrap();
    wait("missing", || {
        state(&live.core.projects_view(), "inner/beta") == Some(MemberState::Missing)
    });
    let from = |core: &ProjectCore| {
        row(&core.projects_view(), "inner/beta")
            .and_then(|r| r.locate_from.clone())
            .unwrap()
    };
    assert_eq!(from(&live.core), root.join("inner").display().to_string());
    fs::remove_dir_all(live.root().join("inner")).unwrap();
    assert_eq!(from(&live.core), root.display().to_string());
}
