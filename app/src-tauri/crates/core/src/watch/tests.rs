use std::borrow::Cow;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::event::{CreateKind, DataChange, EventKind, ModifyKind, RemoveKind, RenameMode};
use notify::Event;

use super::reconcile::Pending;
use super::service::hints_from;
use super::*;
use crate::conditions::{Distribution, Requirement};
use crate::events::{CoreEvent, Recorder};
use crate::fsx::{hash, write_atomic};
use crate::paths::{Root, Vault};
use crate::plugins::{
    Manifest, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginRegistry, PluginScope,
    Running,
};
use crate::settings::{AppSettings, ProjectConfig};
use crate::state::AppDirs;
use crate::tree::{Explorer, TreeFilter};
use crate::workspace::{CoreEnv, ProjectCore};

type Seen = Arc<Mutex<Vec<FsBatch>>>;

const FAST: Timing = Timing {
    quiet: Duration::from_millis(30),
    max: Duration::from_millis(250),
    poll: Duration::from_millis(60),
};

struct Fx {
    dir: tempfile::TempDir,
    watcher: Watcher,
    seen: Seen,
    _sub: Subscription,
}

fn make(files: &[&str], native: bool) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    for f in files {
        let path = dir.path().join(f);
        if f.ends_with('/') {
            fs::create_dir_all(&path).unwrap();
        } else {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
    }
    let root = Root::open(dir.path()).unwrap();
    let vault = Vault::new(root);
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    );
    let hub = WatchHub::new();
    let seen: Seen = Arc::default();
    let sub = hub.subscribe({
        let seen = Arc::clone(&seen);
        move |batch| seen.lock().unwrap().push(batch.clone())
    });
    let watcher = Watcher::start_with(
        &vault,
        explorer.ignorers(),
        hub,
        Arc::new(OwnWrites::new()),
        None,
        FAST,
        native,
    );
    // let the OS watchers settle before the test touches the folder
    std::thread::sleep(Duration::from_millis(150));
    Fx {
        dir,
        watcher,
        seen,
        _sub: sub,
    }
}

fn wait_for(seen: &Seen, what: &str, ok: impl Fn(&[Change]) -> bool) -> Vec<Change> {
    let started = Instant::now();
    loop {
        let all: Vec<Change> = seen
            .lock()
            .unwrap()
            .iter()
            .flat_map(|b| b.changes.clone())
            .collect();
        if ok(&all) {
            return all;
        }
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "timed out waiting for {what}: {all:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn quiet_for(seen: &Seen, ms: u64) -> Vec<Change> {
    std::thread::sleep(Duration::from_millis(ms));
    seen.lock()
        .unwrap()
        .iter()
        .flat_map(|b| b.changes.clone())
        .collect()
}

fn has(changes: &[Change], path: &str, kind: &ChangeKind) -> bool {
    changes.iter().any(|c| c.path == path && &c.kind == kind)
}

// ---- hints -----------------------------------------------------------------------------------------------------------

fn event(kind: EventKind, paths: &[&str]) -> Event {
    let mut e = Event::new(kind);
    for p in paths {
        e = e.add_path(Path::new(p).to_path_buf());
    }
    e
}

#[test]
fn os_events_become_hints() {
    let h = hints_from(&event(EventKind::Create(CreateKind::File), &["/p/a"]));
    assert_eq!(h.len(), 1);
    assert!(h[0].created && !h[0].removed && !h[0].modified);
    let h = hints_from(&event(
        EventKind::Remove(RemoveKind::File),
        &["/p/a", "/p/b"],
    ));
    assert!(h.len() == 2 && h.iter().all(|x| x.removed));
    let h = hints_from(&event(
        EventKind::Modify(ModifyKind::Data(DataChange::Any)),
        &["/p/a"],
    ));
    assert!(h[0].modified);
    let h = hints_from(&event(
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
        &["/p/old", "/p/new"],
    ));
    assert!(h[0].removed && h[0].abs.ends_with("old"));
    assert!(
        h[1].created
            && h[1]
                .rename_from
                .as_deref()
                .is_some_and(|f| f.ends_with("old"))
    );
    let h = hints_from(&event(
        EventKind::Modify(ModifyKind::Name(RenameMode::From)),
        &["/p/old"],
    ));
    assert!(h[0].removed);
    let h = hints_from(&event(
        EventKind::Modify(ModifyKind::Name(RenameMode::To)),
        &["/p/new"],
    ));
    assert!(h[0].created);
    let h = hints_from(&event(
        EventKind::Access(notify::event::AccessKind::Any),
        &["/p/a"],
    ));
    assert!(h.is_empty(), "no hint");
    let h = hints_from(&event(EventKind::Other, &["/p/a"]));
    assert!(h.is_empty(), "no hint");
    // a rename with both paths missing from one report: nothing breaks
    let h = hints_from(&event(
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
        &["/p/only"],
    ));
    assert_eq!(h.len(), 1);
}

#[test]
fn pending_hints_merge_per_path() {
    let mut a = Pending {
        abs: "/p/a".into(),
        created: true,
        removed: false,
        modified: false,
        rename_from: None,
        deferred: false,
    };
    let b = Pending {
        removed: true,
        modified: true,
        rename_from: Some("/p/x".into()),
        created: false,
        ..a.clone()
    };
    a.merge(&b);
    assert!(a.created && a.removed && a.modified);
    assert_eq!(a.rename_from.as_deref(), Some(Path::new("/p/x")));
}

// ---- hub -------------------------------------------------------------------------------------------------------------

#[test]
fn the_hub_calls_subscribers_in_order_until_their_subscription_drops() {
    let hub = WatchHub::new();
    let log: Arc<Mutex<Vec<&'static str>>> = Arc::default();
    let first = hub.subscribe({
        let log = Arc::clone(&log);
        move |_| log.lock().unwrap().push("first")
    });
    let second = hub.subscribe({
        let log = Arc::clone(&log);
        move |_| log.lock().unwrap().push("second")
    });
    hub.publish(&FsBatch::default());
    drop(first);
    hub.publish(&FsBatch::default());
    drop(second);
    hub.publish(&FsBatch::default());
    assert_eq!(*log.lock().unwrap(), ["first", "second", "second"]);
}

// ---- real file system -------------------------------------------------------------------------------------------------

#[test]
fn a_new_file_is_one_created_change() {
    let f = make(&["keep.md"], true);
    fs::write(f.dir.path().join("new.md"), "hello").unwrap();
    let all = wait_for(&f.seen, "created", |c| {
        has(c, "new.md", &ChangeKind::Created)
    });
    assert_eq!(all.iter().filter(|c| c.path == "new.md").count(), 1);
    assert!(all.iter().all(|c| !c.own));
}

#[test]
fn a_save_is_one_modified_and_the_same_bytes_again_are_nothing() {
    let f = make(&["a.md"], true);
    // a first change makes the watch remember the file
    fs::write(f.dir.path().join("a.md"), "one").unwrap();
    wait_for(&f.seen, "first change", |c| {
        has(c, "a.md", &ChangeKind::Modified)
    });
    f.seen.lock().unwrap().clear();
    // an atomic save of new content: one modified
    write_atomic(&f.dir.path().join("a.md"), b"two").unwrap();
    let all = wait_for(&f.seen, "modified", |c| {
        has(c, "a.md", &ChangeKind::Modified)
    });
    assert_eq!(
        all.iter().filter(|c| c.path == "a.md").count(),
        1,
        "{all:?}"
    );
    assert!(
        all.iter().all(|c| !c.path.contains(".part")),
        "no temp file news"
    );
    f.seen.lock().unwrap().clear();
    // the same bytes saved again (new time): not news
    std::thread::sleep(Duration::from_millis(20));
    write_atomic(&f.dir.path().join("a.md"), b"two").unwrap();
    let after = quiet_for(&f.seen, 500);
    assert!(after.iter().all(|c| c.path != "a.md"), "{after:?}");
}

#[test]
fn an_own_write_is_marked_and_other_content_at_the_same_path_is_not() {
    let f = make(&["a.md"], true);
    fs::write(f.dir.path().join("a.md"), "v1").unwrap();
    wait_for(&f.seen, "first change", |c| {
        has(c, "a.md", &ChangeKind::Modified)
    });
    f.seen.lock().unwrap().clear();
    let bytes = b"written by the app";
    f.watcher.own().expect_write("a.md", hash(bytes));
    write_atomic(&f.dir.path().join("a.md"), bytes).unwrap();
    let all = wait_for(&f.seen, "own write", |c| {
        has(c, "a.md", &ChangeKind::Modified)
    });
    assert!(
        all.iter().filter(|c| c.path == "a.md").all(|c| c.own),
        "{all:?}"
    );
    f.seen.lock().unwrap().clear();
    // an agent saves different content right after: news
    f.watcher.own().expect_write("a.md", hash(b"expected"));
    write_atomic(&f.dir.path().join("a.md"), b"something else").unwrap();
    let all = wait_for(&f.seen, "foreign write", |c| {
        has(c, "a.md", &ChangeKind::Modified)
    });
    assert!(
        all.iter().filter(|c| c.path == "a.md").all(|c| !c.own),
        "{all:?}"
    );
}

#[test]
fn a_moved_file_is_one_renamed_change() {
    let f = make(&["docs/", "a.md"], true);
    fs::write(
        f.dir.path().join("a.md"),
        "content that is long enough to tell",
    )
    .unwrap();
    wait_for(&f.seen, "seen once", |c| {
        has(c, "a.md", &ChangeKind::Modified)
    });
    f.seen.lock().unwrap().clear();
    fs::rename(f.dir.path().join("a.md"), f.dir.path().join("docs/b.md")).unwrap();
    let all = wait_for(&f.seen, "renamed", |c| {
        has(
            c,
            "docs/b.md",
            &ChangeKind::Renamed {
                from: "a.md".to_owned(),
            },
        )
    });
    assert!(
        !has(&all, "a.md", &ChangeKind::Removed),
        "the removal is part of the rename: {all:?}"
    );
}

#[test]
fn a_moved_folder_is_reported_as_a_folder_move_or_as_its_files() {
    let f = make(&["old/inner.md", "old/sub/deep.md"], true);
    fs::rename(f.dir.path().join("old"), f.dir.path().join("new")).unwrap();
    let all = wait_for(&f.seen, "folder move", |c| {
        c.iter()
            .any(|c| c.path == "new" && matches!(c.kind, ChangeKind::Renamed { .. }))
            || (c.iter().any(|c| c.path == "new/inner.md")
                && c.iter().any(|c| c.path == "new/sub/deep.md"))
    });
    // never a half picture: nothing is reported as appearing at the old place
    assert!(
        !all.iter()
            .any(|c| c.path.starts_with("old") && matches!(c.kind, ChangeKind::Created)),
        "{all:?}"
    );
}

#[test]
fn noise_is_dropped_and_git_signals_pass() {
    let f = make(&[".git/", "build/", ".gitignore"], true);
    fs::write(f.dir.path().join(".gitignore"), "*.log\nbuild/\n").unwrap();
    wait_for(&f.seen, "gitignore", |c| {
        c.iter().any(|c| c.path == ".gitignore")
    });
    f.seen.lock().unwrap().clear();
    for name in [
        "x.tmp",
        "y.md~",
        ".#z.md",
        ".w.swp",
        ".a.md.1.2.part",
        "4913",
        "app.log",
    ] {
        fs::write(f.dir.path().join(name), "n").unwrap();
    }
    fs::write(f.dir.path().join("build/out.md"), "n").unwrap();
    fs::create_dir_all(f.dir.path().join(".git/objects")).unwrap();
    fs::write(f.dir.path().join(".git/objects/blob"), "n").unwrap();
    fs::write(f.dir.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(f.dir.path().join("real.md"), "r").unwrap();
    let all = wait_for(&f.seen, "real and head", |c| {
        c.iter().any(|c| c.path == "real.md") && c.iter().any(|c| c.path == ".git/HEAD")
    });
    let paths: Vec<&str> = all.iter().map(|c| c.path.as_str()).collect();
    for noisy in [
        "x.tmp",
        "y.md~",
        ".#z.md",
        ".w.swp",
        ".a.md.1.2.part",
        "4913",
        "app.log",
        "build/out.md",
        ".git/objects/blob",
    ] {
        assert!(!paths.contains(&noisy), "{noisy} in {paths:?}");
    }
}

#[test]
fn a_burst_of_files_is_one_bulk_batch() {
    let f = make(&["start.md"], true);
    for n in 0..300 {
        fs::write(f.dir.path().join(format!("burst{n}.md")), "b").unwrap();
    }
    wait_for(&f.seen, "all created", |c| {
        c.iter().filter(|c| c.path.starts_with("burst")).count() >= 300
    });
    let batches = f.seen.lock().unwrap();
    assert!(batches.iter().any(|b| b.bulk), "{}", batches.len());
}

#[test]
fn a_created_folder_is_expanded_into_its_files() {
    let f = make(&["start.md"], true);
    let staging = tempfile::tempdir().unwrap();
    fs::create_dir_all(staging.path().join("pkg/sub")).unwrap();
    fs::write(staging.path().join("pkg/a.md"), "a").unwrap();
    fs::write(staging.path().join("pkg/sub/b.md"), "b").unwrap();
    // moved in from outside (same volume or copied): its files may report nothing themselves
    let target = f.dir.path().join("pkg");
    if fs::rename(staging.path().join("pkg"), &target).is_err() {
        fs::create_dir_all(target.join("sub")).unwrap();
        fs::write(target.join("a.md"), "a").unwrap();
        fs::write(target.join("sub/b.md"), "b").unwrap();
    }
    wait_for(&f.seen, "expanded", |c| {
        has(c, "pkg/a.md", &ChangeKind::Created) && has(c, "pkg/sub/b.md", &ChangeKind::Created)
    });
}

#[test]
fn nothing_is_reported_for_a_touched_but_unchanged_tree() {
    let f = make(&["a.md", "docs/b.md"], true);
    for p in ["a.md", "docs/b.md"] {
        let bytes = fs::read(f.dir.path().join(p)).unwrap();
        fs::write(f.dir.path().join(p), bytes).unwrap();
    }
    let all = quiet_for(&f.seen, 500);
    // the first sight of a file cannot be told from a change; a second identical write must be silent
    f.seen.lock().unwrap().clear();
    for p in ["a.md", "docs/b.md"] {
        let bytes = fs::read(f.dir.path().join(p)).unwrap();
        fs::write(f.dir.path().join(p), bytes).unwrap();
    }
    let again = quiet_for(&f.seen, 500);
    assert!(again.is_empty(), "first: {all:?}, again: {again:?}");
}

// ---- teardown ------------------------------------------------------------------------------------------------------------

#[test]
fn dropping_the_watcher_while_events_flow_is_prompt_and_silent_afterwards() {
    let f = make(&["a.md"], true);
    let dir = f.dir.path().to_path_buf();
    let writer = std::thread::spawn(move || {
        for n in 0..200 {
            let _ = fs::write(dir.join(format!("s{n}.md")), "x");
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    std::thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    let Fx {
        watcher,
        seen,
        dir,
        _sub,
    } = f;
    drop(watcher);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    writer.join().unwrap();
    let count = seen.lock().unwrap().len();
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(seen.lock().unwrap().len(), count, "no batch after the drop");
    drop(dir);
}

#[test]
fn a_watcher_over_a_vanished_folder_starts_unavailable_and_drops_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::open(dir.path()).unwrap();
    let vault = Vault::new(root);
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    );
    drop(dir);
    let watcher = Watcher::start_with(
        &vault,
        explorer.ignorers(),
        WatchHub::new(),
        Arc::new(OwnWrites::new()),
        None,
        FAST,
        true,
    );
    assert_eq!(
        watcher.status(),
        LiveStatus::Unavailable {
            reason: "watchFailed".to_owned()
        }
    );
    drop(watcher);
}

// ---- unavailable: polling the open document -------------------------------------------------------------------------------

#[test]
fn without_live_updates_the_open_document_is_polled() {
    let f = make(&["doc.md", "other.md"], false);
    assert_eq!(
        f.watcher.status(),
        LiveStatus::Unavailable {
            reason: "watchFailed".to_owned()
        }
    );
    let doc = f.dir.path().join("doc.md");
    f.watcher.poll_document("w", Some(&doc));
    // untouched: silent
    assert!(quiet_for(&f.seen, 300).is_empty(), "silent");
    // changed: one modified; other files are not looked at
    fs::write(&doc, "changed content").unwrap();
    fs::write(f.dir.path().join("other.md"), "changed too").unwrap();
    let all = wait_for(&f.seen, "polled change", |c| {
        has(c, "doc.md", &ChangeKind::Modified)
    });
    assert!(!all.iter().any(|c| c.path == "other.md"));
    // removed
    fs::remove_file(&doc).unwrap();
    wait_for(&f.seen, "polled removal", |c| {
        has(c, "doc.md", &ChangeKind::Removed)
    });
    // no longer polled
    f.watcher.poll_document("w", None);
    f.seen.lock().unwrap().clear();
    fs::write(&doc, "back").unwrap();
    assert!(quiet_for(&f.seen, 300).is_empty(), "silent");
}

#[test]
fn a_rescan_request_is_a_bulk_batch_that_asks_to_rebuild() {
    let f = make(&["a.md"], true);
    f.watcher.inject_rescan();
    let started = Instant::now();
    loop {
        if f.seen.lock().unwrap().iter().any(|b| b.rescan && b.bulk) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn hints_for_paths_outside_the_project_are_ignored() {
    let f = make(&["a.md"], true);
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("x.md"), "x").unwrap();
    f.watcher.inject(Pending {
        abs: outside.path().join("x.md"),
        created: true,
        removed: false,
        modified: false,
        rename_from: None,
        deferred: false,
    });
    assert!(quiet_for(&f.seen, 300).is_empty(), "silent");
}

// ---- inside a project ----------------------------------------------------------------------------------------------------

struct Listener {
    manifest: Manifest,
    seen: Seen,
}

struct ListenerRunning {
    _subscription: Subscription,
}

impl Running for ListenerRunning {}

impl Plugin for Listener {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let hub = cx.watch().ok_or_else(|| PluginError::Start {
            id: cx.id.clone(),
            reason: "no watch".to_owned(),
        })?;
        let seen = Arc::clone(&self.seen);
        Ok(Box::new(ListenerRunning {
            _subscription: hub.subscribe(move |batch| seen.lock().unwrap().push(batch.clone())),
        }))
    }
}

fn project_env(base: &Path, seen: &Seen, sink: Arc<Recorder>) -> CoreEnv {
    let plugin = Listener {
        manifest: Manifest {
            id: PluginId::of("feed"),
            kind: PluginKind::Feature,
            scope: PluginScope::Project,
            requires: Requirement::NONE,
            depends_on: Cow::Owned(Vec::new()),
            default_enabled: true,
        },
        seen: Arc::clone(seen),
    };
    CoreEnv::new(
        AppDirs::under(base),
        Arc::new(PluginRegistry::new(vec![Arc::new(plugin) as Arc<dyn Plugin>]).unwrap()),
        sink,
        Distribution::Direct,
        AppSettings::default(),
    )
}

#[test]
fn a_project_core_updates_the_tree_before_plugins_hear_and_tears_everything_down() {
    let base = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("a.md"), "a").unwrap();
    let seen: Seen = Arc::default();
    let sink = Arc::new(Recorder::default());
    let env = project_env(base.path(), &seen, sink.clone());
    let core = ProjectCore::open(Root::open(project.path()).unwrap(), &env);
    assert_eq!(core.live_status(), LiveStatus::Available);
    let snapshot = core.explorer().open_view("w", Some(TreeFilter::default()));
    assert_eq!(snapshot.nodes.len(), 1);
    std::thread::sleep(Duration::from_millis(200));
    fs::write(project.path().join("b.md"), "b").unwrap();
    wait_for(&seen, "the plugin hears the file", |c| {
        has(c, "b.md", &ChangeKind::Created)
    });
    // the Explorer heard it first: by the time the plugin saw it, the window had its delta
    let deltas: Vec<_> = sink
        .take()
        .into_iter()
        .filter_map(|e| match e {
            CoreEvent::TreeChanged(c) => Some(c),
            _ => None,
        })
        .collect();
    assert!(
        deltas
            .iter()
            .any(|c| c.window == "w" && c.delta.added.iter().any(|a| a.node.path == "b.md")),
        "{deltas:?}"
    );
    // closing the project drops the watcher and the plugin's subscription
    let started = Instant::now();
    drop(core);
    assert!(started.elapsed() < Duration::from_secs(3));
    let count = seen.lock().unwrap().len();
    fs::write(project.path().join("c.md"), "c").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        seen.lock().unwrap().len(),
        count,
        "nothing after the project closed"
    );
}

#[test]
fn a_project_core_indexes_its_files_follows_the_real_watch_and_lets_go_on_close() {
    use crate::search::{SearchFilter, SearchOptions};
    let base = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("a.md"),
        "# A

firstwords
",
    )
    .unwrap();
    let seen: Seen = Arc::default();
    let sink = Arc::new(Recorder::default());
    let env = project_env(base.path(), &seen, sink.clone());
    let core = ProjectCore::open(Root::open(project.path()).unwrap(), &env);
    assert!(core.search().wait_ready(Duration::from_secs(30)));
    let found = |q: &str| -> Vec<String> {
        core.search()
            .sections(q, 10, &SearchFilter::default())
            .hits
            .into_iter()
            .map(|h| h.path)
            .collect()
    };
    assert_eq!(found("firstwords"), ["a.md"]);
    std::thread::sleep(Duration::from_millis(200));
    fs::write(
        project.path().join("b.md"),
        "# B

secondwords
",
    )
    .unwrap();
    let start = Instant::now();
    while found("secondwords").is_empty() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the watch reaches the index"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // tags need their plugin; nothing else is asked of the filter yet
    let tags = SearchOptions {
        include_silenced: false,
        tags: vec!["x".to_owned()],
    };
    assert_eq!(
        core.search_filter(&tags).unwrap_err().kind,
        crate::ErrorKind::Unsupported
    );
    let options = SearchOptions {
        include_silenced: true,
        tags: Vec::new(),
    };
    assert_eq!(
        core.search_filter(&options).unwrap(),
        SearchFilter::default()
    );
    let started = Instant::now();
    drop(core);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "joined in {:?}",
        started.elapsed()
    );
}

#[test]
fn live_updates_in_the_context_follow_the_watcher() {
    use crate::conditions::LiveUpdates;
    use crate::workspace::{WorkspaceRef, WorkspaceRegistry};
    let base = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let seen: Seen = Arc::default();
    let sink = Arc::new(Recorder::default());
    let env = Arc::new(project_env(base.path(), &seen, sink));
    let registry = WorkspaceRegistry::new_shared(&env);
    registry.add_window("main");
    registry
        .open(
            "main",
            &WorkspaceRef::Project(Root::open(project.path()).unwrap()),
            None,
        )
        .unwrap();
    assert_eq!(
        registry.context_of("main").unwrap().live_updates,
        LiveUpdates::Available
    );
    // the window leaving its project drops that window's tree view
    let core = registry.cores().remove(0);
    core.explorer().open_view("main", None);
    registry.show_welcome("main");
    assert!(
        core.explorer().keep("main", None).is_err(),
        "the view went with the workspace"
    );
}
