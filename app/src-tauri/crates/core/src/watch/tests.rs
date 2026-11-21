use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::event::{CreateKind, DataChange, EventKind, ModifyKind, RemoveKind, RenameMode};
use notify::Event;

use super::reconcile::Pending;
use super::service::{hints_from, RenamePairs};
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
    make_with(files, native, FAST)
}

fn make_with(files: &[&str], native: bool, timing: Timing) -> Fx {
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
    watch_dir(dir, native, timing)
}

/// A watch over what `dir` holds now.
fn watch_dir(dir: tempfile::TempDir, native: bool, timing: Timing) -> Fx {
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
        timing,
        native,
    );
    settle(&seen);
    Fx {
        dir,
        watcher,
        seen,
        _sub: sub,
    }
}

/// Lets the OS watchers settle before the test touches the folder. Some systems (macOS) still report what happened
/// just before the watch began: that news is waited out, then forgotten.
fn settle(seen: &Seen) {
    std::thread::sleep(Duration::from_millis(150));
    let started = Instant::now();
    let mut count = seen.lock().unwrap().len();
    let mut since = Instant::now();
    while since.elapsed() < Duration::from_millis(600) && started.elapsed() < Duration::from_secs(5)
    {
        std::thread::sleep(Duration::from_millis(20));
        let now = seen.lock().unwrap().len();
        if now != count {
            count = now;
            since = Instant::now();
        }
    }
    seen.lock().unwrap().clear();
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
            started.elapsed() < Duration::from_secs(30),
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

/// Writes the new file `name` and waits for its report: by then the watch has looked at everything that happened
/// before it (the OS reports in order). Returns all that was reported so far.
fn fence(f: &Fx, name: &str) -> Vec<Change> {
    fs::write(f.dir.path().join(name), name).unwrap();
    wait_for(&f.seen, name, |c| c.iter().any(|c| c.path == name))
}

/// A file someone wrote is news (macOS may report a file the watch never read as created).
fn wrote(changes: &[Change], path: &str) -> bool {
    changes
        .iter()
        .any(|c| c.path == path && matches!(c.kind, ChangeKind::Modified | ChangeKind::Created))
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
fn the_two_halves_of_a_move_are_paired_only_when_in_a_row() {
    let name = |mode| EventKind::Modify(ModifyKind::Name(mode));
    let from = |h: &[Pending]| h[0].rename_from.clone();
    let after = |h: &[Pending]| h[0].after_removal.clone();
    // Windows rename: old name, then new name
    let mut pairs = RenamePairs::default();
    assert!(pairs.hints(&event(name(RenameMode::From), &["/p/old"]))[0].removed);
    let h = pairs.hints(&event(name(RenameMode::To), &["/p/new"]));
    assert_eq!(from(&h), Some("/p/old".into()));
    assert_eq!(after(&h), None);
    // used once
    let h = pairs.hints(&event(name(RenameMode::To), &["/p/again"]));
    assert_eq!(from(&h), None);
    // Windows move to another parent: removal, then creation -> a maybe, never a rename
    let mut pairs = RenamePairs::default();
    pairs.hints(&event(EventKind::Remove(RemoveKind::Any), &["/p/a/x"]));
    let h = pairs.hints(&event(EventKind::Create(CreateKind::Any), &["/p/b/x"]));
    assert_eq!((from(&h), after(&h)), (None, Some("/p/a/x".into())));
    // anything in between ends the pair
    let mut pairs = RenamePairs::default();
    pairs.hints(&event(name(RenameMode::From), &["/p/old"]));
    pairs.hints(&event(
        EventKind::Modify(ModifyKind::Data(DataChange::Any)),
        &["/p/z"],
    ));
    let h = pairs.hints(&event(name(RenameMode::To), &["/p/new"]));
    assert_eq!((from(&h), after(&h)), (None, None));
    // a rename's old half followed by a creation, a removal followed by a rename's new half: no pair
    let mut pairs = RenamePairs::default();
    pairs.hints(&event(name(RenameMode::From), &["/p/old"]));
    let h = pairs.hints(&event(EventKind::Create(CreateKind::Any), &["/p/new"]));
    assert_eq!((from(&h), after(&h)), (None, None));
    pairs.hints(&event(EventKind::Remove(RemoveKind::Any), &["/p/old"]));
    let h = pairs.hints(&event(name(RenameMode::To), &["/p/new"]));
    assert_eq!((from(&h), after(&h)), (None, None));
    // the same path again (an atomic save): no pair
    let mut pairs = RenamePairs::default();
    pairs.hints(&event(EventKind::Remove(RemoveKind::Any), &["/p/a"]));
    let h = pairs.hints(&event(EventKind::Create(CreateKind::Any), &["/p/a"]));
    assert_eq!(after(&h), None);
    // a one-event rename stays as it was and ends any pair
    let mut pairs = RenamePairs::default();
    pairs.hints(&event(name(RenameMode::From), &["/p/old"]));
    let h = pairs.hints(&event(name(RenameMode::Both), &["/p/x", "/p/y"]));
    assert_eq!(h[1].rename_from, Some("/p/x".into()));
    let h = pairs.hints(&event(name(RenameMode::To), &["/p/new"]));
    assert_eq!(from(&h), None);
}

#[test]
fn pending_hints_merge_per_path() {
    let mut a = Pending {
        abs: "/p/a".into(),
        created: true,
        removed: false,
        modified: false,
        rename_from: None,
        after_removal: None,
        deferred: false,
    };
    let b = Pending {
        removed: true,
        modified: true,
        rename_from: Some("/p/x".into()),
        after_removal: None,
        created: false,
        ..a.clone()
    };
    a.merge(&b);
    assert!(a.created && a.removed && a.modified);
    assert_eq!(a.rename_from.as_deref(), Some(Path::new("/p/x")));
    // the same path under another case of its name: the name it came to wins
    let mut old = Pending {
        abs: "/p/beta".into(),
        created: false,
        removed: true,
        ..b.clone()
    };
    old.merge(&Pending {
        abs: "/p/Beta".into(),
        created: true,
        removed: false,
        ..b
    });
    assert_eq!(old.abs, Path::new("/p/Beta"));
    assert!(old.created && old.removed);
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
        c.iter().any(|c| c.path == "a.md")
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
        c.iter().any(|c| c.path == "a.md")
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
    wait_for(&f.seen, "seen once", |c| c.iter().any(|c| c.path == "a.md"));
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
    // a flush comes after `quiet` without news or `max` after the first: with the fast timings a slow disk (a
    // runner's virus scan: ~3 ms a file) cut 1000 writes into batches under the bulk limit. A writer that never
    // pauses this long is one burst; `max` far beyond the writes.
    let burst = Timing {
        quiet: Duration::from_secs(1),
        max: Duration::from_secs(60),
        ..FAST
    };
    let f = make_with(&["start.md"], true, burst);
    // plenty over the bulk limit: a stall of the machine between two flushes still leaves one of them bulk
    for n in 0..1000 {
        fs::write(f.dir.path().join(format!("burst{n}.md")), "b").unwrap();
    }
    wait_for(&f.seen, "all created", |c| {
        c.iter().filter(|c| c.path.starts_with("burst")).count() >= 1000
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
    // a new file written after the touches: once it is reported, the touches before it were looked at (the OS reports
    // in order). Run 39550333941 (macOS) waited 500 ms instead: the first touches came after it, as "created".
    for p in ["a.md", "docs/b.md"] {
        let bytes = fs::read(f.dir.path().join(p)).unwrap();
        fs::write(f.dir.path().join(p), bytes).unwrap();
    }
    let all = fence(&f, "fence1.md");
    // the first sight of a file cannot be told from a change; a second identical write must be silent
    f.seen.lock().unwrap().clear();
    for p in ["a.md", "docs/b.md"] {
        let bytes = fs::read(f.dir.path().join(p)).unwrap();
        fs::write(f.dir.path().join(p), bytes).unwrap();
    }
    let again = fence(&f, "fence2.md");
    assert!(
        again.iter().all(|c| c.path.starts_with("fence")),
        "first: {all:?}, again: {again:?}"
    );
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

#[cfg(target_os = "linux")]
#[test]
fn the_watch_never_follows_a_link_out_of_the_project() {
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(outside.path().join("deep")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("keep.md"), "x").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("out")).unwrap();
    // followed, this one would have the watch walk the whole system
    std::os::unix::fs::symlink("/", dir.path().join("top")).unwrap();
    let f = watch_dir(dir, true, FAST);
    assert_eq!(f.watcher.status(), LiveStatus::Available);
    fs::write(outside.path().join("far.md"), "outside").unwrap();
    fs::write(outside.path().join("deep/farther.md"), "outside").unwrap();
    let all = fence(&f, "after.md");
    assert!(!all.iter().any(|c| c.path.starts_with("out/")), "{all:?}");
    assert_eq!(f.watcher.status(), LiveStatus::Available);
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
    let heard = quiet_for(&f.seen, 300);
    assert!(heard.is_empty(), "silent: {heard:?}");
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
    // a poll already under way still reports what the disk says when it is done
    quiet_for(&f.seen, 300);
    f.seen.lock().unwrap().clear();
    fs::write(&doc, "back").unwrap();
    let heard = quiet_for(&f.seen, 300);
    assert!(heard.is_empty(), "silent: {heard:?}");
}

#[test]
fn a_rescan_request_is_a_bulk_batch_that_asks_to_rebuild() {
    let f = make(&["a.md"], true);
    f.watcher.rescan();
    let started = Instant::now();
    loop {
        if f.seen.lock().unwrap().iter().any(|b| b.rescan && b.bulk) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_folder_gone_before_its_files_were_news_is_reported_as_a_folder() {
    // (the tree knows it, the system also flags it created, what is reported)
    for (knows, created, reported) in [
        (true, false, Some(true)),
        (true, true, Some(true)),
        (false, false, Some(false)),
        (false, true, None),
    ] {
        let f = make(&["lib/x.md"], false);
        if knows {
            f.watcher
                .knows_tree(Box::new(|path| (path == "lib").then_some(PathHold::Dir)));
        }
        fs::remove_dir_all(f.dir.path().join("lib")).unwrap();
        f.watcher.inject(Pending {
            abs: f.dir.path().join("lib"),
            created,
            removed: true,
            modified: false,
            rename_from: None,
            after_removal: None,
            deferred: false,
        });
        if let Some(is_dir) = reported {
            let all = wait_for(&f.seen, "removed", |c| has(c, "lib", &ChangeKind::Removed));
            assert_eq!(all.len(), 1, "{all:?}");
            assert_eq!(all[0].is_dir, is_dir, "{all:?}");
        } else {
            // created and gone inside one window, never known: nothing
            let heard = quiet_for(&f.seen, 300);
            assert!(heard.is_empty(), "{heard:?}");
        }
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
        after_removal: None,
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
fn a_file_made_through_the_projects_own_editor_is_listed_before_the_watch_reports_it() {
    let base = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("a.md"), "a").unwrap();
    let seen: Seen = Arc::default();
    let sink = Arc::new(Recorder::default());
    let env = project_env(base.path(), &seen, sink.clone());
    let core = ProjectCore::open(Root::open(project.path()).unwrap(), &env);
    tree_shown(&core, &seen);
    sink.take();
    // the editor plugins get (a document written back at its path): the tree is told with the write, not by the watch,
    // whose report waits for a quiet moment (150 ms). Whether the watch has reported yet is not asserted: a runner
    // stalled past that moment lets it report first, which proves nothing wrong; on every other run the listing below
    // can only come from the write.
    let back = core
        .services()
        .with_editor(|editor| editor.save_as("back.md", "back\n", true))
        .unwrap();
    assert!(core.explorer().listed(&back).is_some(), "listed at once");
    let told = sink.take().into_iter().any(|event| match event {
        CoreEvent::TreeChanged(c) => {
            c.window == "w" && c.delta.added.iter().any(|a| a.node.path == back)
        }
        _ => false,
    });
    assert!(told, "the window is told");
}

/// The project's tree as window `w` shows it: built, and the OS watch settled (what it still tells of the time before
/// it began is waited out and forgotten).
fn tree_shown(core: &ProjectCore, seen: &Seen) {
    core.explorer().open_view("w", Some(TreeFilter::default()));
    assert!(
        core.explorer().wait_built(Duration::from_secs(30)),
        "the tree is built"
    );
    settle(seen);
}

/// A new file written through the project's editor with nothing but the watch to tell the tree: the write is
/// announced as the app's own, as the app does for a window. Returns the file's project path.
fn write_beside(core: &ProjectCore, beside: &str, name: &str, text: &str) -> String {
    let formats = core.formats();
    let editor = crate::edit::Editor {
        scope: core.documents().scope(),
        formats: &formats,
        own: Some(core.watcher().own().as_ref()),
        observer: None,
        tree: None,
    };
    editor.write_beside(beside, name, text).unwrap()
}

/// Waits until the project's tree lists `path` (the watch told it).
fn listed_soon(core: &ProjectCore, path: &str) -> bool {
    let started = Instant::now();
    while core.explorer().listed(path).is_none() {
        if started.elapsed() > Duration::from_secs(8) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

#[test]
fn a_file_the_editor_creates_reaches_the_tree_through_the_real_watch_as_the_apps_own() {
    let base = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("Docs")).unwrap();
    fs::write(project.path().join("Docs/CONCEPT-0.md"), "# Concept\n").unwrap();
    fs::write(project.path().join("top.md"), "top\n").unwrap();
    let seen: Seen = Arc::default();
    let env = project_env(base.path(), &seen, Arc::new(Recorder::default()));
    let core = ProjectCore::open(Root::open(project.path()).unwrap(), &env);
    tree_shown(&core, &seen);
    // in a folder, a name with two dots; at the top of the project; a taken name (`(2)`)
    let copy = write_beside(
        &core,
        "Docs/CONCEPT-0.md",
        "CONCEPT-0.pt-BR.md",
        "# Conceito\n",
    );
    let top = write_beside(&core, "top.md", "top.en-US.md", "top\n");
    let again = write_beside(
        &core,
        "Docs/CONCEPT-0.md",
        "CONCEPT-0.pt-BR.md",
        "# Outra\n",
    );
    assert_eq!(copy, "Docs/CONCEPT-0.pt-BR.md");
    assert_eq!(again, "Docs/CONCEPT-0.pt-BR (2).md");
    for path in [&copy, &top, &again] {
        assert!(listed_soon(&core, path), "the tree lists {path}");
    }
    // the announced content is what the watch read: each creation is the app's own, told once
    let all = wait_for(&seen, "the plugin hears the files", |c| {
        [&copy, &top, &again]
            .iter()
            .all(|path| has(c, path, &ChangeKind::Created))
    });
    for path in [&copy, &top, &again] {
        let told: Vec<&Change> = all.iter().filter(|c| &c.path == path).collect();
        assert!(told.iter().all(|c| c.own), "{told:?}");
        assert_eq!(told.len(), 1, "{told:?}");
    }
}

#[test]
fn a_new_file_the_window_shows_before_the_watch_reports_it_is_still_news() {
    let base = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("Docs")).unwrap();
    fs::write(project.path().join("Docs/CONCEPT-0.md"), "# Concept\n").unwrap();
    let seen: Seen = Arc::default();
    let env = project_env(base.path(), &seen, Arc::new(Recorder::default()));
    let core = ProjectCore::open(Root::open(project.path()).unwrap(), &env);
    tree_shown(&core, &seen);
    // the app writes a translated copy and shows it at once: the window names its document while the events of the
    // write still wait for their quiet moment
    let copy = write_beside(
        &core,
        "Docs/CONCEPT-0.md",
        "CONCEPT-0.pt-BR.md",
        "# Conceito\n",
    );
    core.watcher()
        .poll_document("w", Some(&project.path().join("Docs/CONCEPT-0.pt-BR.md")));
    assert!(listed_soon(&core, &copy), "the tree lists {copy}");
    wait_for(&seen, "the plugin hears the file", |c| {
        has(c, &copy, &ChangeKind::Created)
    });
}

#[test]
fn a_change_of_the_document_the_window_shows_again_before_the_watch_reports_it_is_still_news() {
    let f = make(&["a.md"], true);
    let doc = f.dir.path().join("a.md");
    fs::write(&doc, "one").unwrap();
    wait_for(&f.seen, "first change", |c| {
        c.iter().any(|c| c.path == "a.md")
    });
    f.seen.lock().unwrap().clear();
    // someone saves the file and the window opens it before the quiet moment is over
    write_atomic(&doc, b"two").unwrap();
    f.watcher.poll_document("w", Some(&doc));
    wait_for(&f.seen, "modified", |c| {
        has(c, "a.md", &ChangeKind::Modified)
    });
}

// ---- live updates, a document shown that the watch never reported ----------------------------------------------------

/// Long enough for two file operations made one after the other to be looked at together.
const TOGETHER: Timing = Timing {
    quiet: Duration::from_millis(300),
    max: Duration::from_secs(3),
    ..FAST
};

/// A watch with live updates and the file `name` shown in window `w`, as when the app opens a document: the watch
/// has told nothing about it.
fn shown(files: &[&str], name: &str, timing: Timing) -> (Fx, PathBuf) {
    let f = make_with(files, true, timing);
    assert_eq!(f.watcher.status(), LiveStatus::Available);
    let doc = f.dir.path().join(name);
    f.watcher.poll_document("w", Some(&doc));
    (f, doc)
}

fn told<'a>(changes: &'a [Change], path: &str) -> Vec<&'a Change> {
    changes.iter().filter(|c| c.path == path).collect()
}

#[test]
fn showing_a_document_reports_nothing_by_itself() {
    let (f, _doc) = shown(&["a.md", "pic.png"], "a.md", FAST);
    f.watcher
        .poll_document("v", Some(&f.dir.path().join("pic.png")));
    // shown again (the window names its document more than once), and no longer shown
    f.watcher
        .poll_document("w", Some(&f.dir.path().join("a.md")));
    f.watcher.poll_document("v", None);
    let all = fence(&f, "fence.md");
    assert!(all.iter().all(|c| c.path == "fence.md"), "{all:?}");
}

#[test]
fn an_outside_change_of_a_shown_document_is_one_modified_saved_in_place_or_by_replacing_the_file() {
    for atomic in [false, true] {
        let (f, doc) = shown(&["a.md"], "a.md", FAST);
        if atomic {
            write_atomic(&doc, b"another program saved").unwrap();
        } else {
            fs::write(&doc, "another program saved").unwrap();
        }
        let all = wait_for(&f.seen, "the change", |c| {
            c.iter().any(|c| c.path == "a.md")
        });
        let told = told(&all, "a.md");
        assert_eq!(told.len(), 1, "atomic {atomic}: {all:?}");
        assert_eq!(
            told[0].kind,
            ChangeKind::Modified,
            "the window read the file before: it was there (atomic {atomic})"
        );
        assert!(!told[0].own);
    }
}

#[test]
fn a_shown_document_another_program_deletes_is_one_removed() {
    let (f, doc) = shown(&["a.md"], "a.md", FAST);
    fs::remove_file(&doc).unwrap();
    let all = wait_for(&f.seen, "the removal", |c| {
        c.iter().any(|c| c.path == "a.md")
    });
    let told = told(&all, "a.md");
    assert_eq!(told.len(), 1, "{all:?}");
    assert_eq!(told[0].kind, ChangeKind::Removed);
    assert!(!told[0].is_dir && !told[0].own);
}

#[test]
fn a_shown_document_moved_by_copy_and_delete_is_one_renamed() {
    // no rename is reported (another volume in between, a sync client): a creation with the content the window read
    // and the removal of the shown file are one move
    let (f, doc) = shown(&["docs/", "a.md"], "a.md", TOGETHER);
    let bytes = fs::read(&doc).unwrap();
    fs::write(f.dir.path().join("docs/b.md"), bytes).unwrap();
    fs::remove_file(&doc).unwrap();
    let all = wait_for(&f.seen, "both ends", |c| {
        c.iter().any(|c| c.path == "docs/b.md")
            && c.iter().any(|c| {
                c.path == "a.md"
                    || c.kind
                        == ChangeKind::Renamed {
                            from: "a.md".to_owned(),
                        }
            })
    });
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].path, "docs/b.md");
    assert_eq!(
        all[0].kind,
        ChangeKind::Renamed {
            from: "a.md".to_owned()
        }
    );
}

#[test]
fn a_touch_of_a_shown_picture_that_leaves_its_bytes_is_no_news() {
    // a sync client | a virus scan: the same bytes, a new time. Pictures are told apart by size + time: a report
    // would make the viewer load the unchanged picture again
    let (f, pic) = shown(&["pic.png"], "pic.png", FAST);
    let later = std::time::SystemTime::now() + Duration::from_secs(90);
    fs::File::options()
        .write(true)
        .open(&pic)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let all = fence(&f, "fence1.md");
    assert!(all.iter().all(|c| c.path == "fence1.md"), "{all:?}");
    // the same bytes written again, in place
    fs::write(&pic, fs::read(&pic).unwrap()).unwrap();
    let all = fence(&f, "fence2.md");
    assert!(all.iter().all(|c| c.path.starts_with("fence")), "{all:?}");
}

#[test]
fn the_apps_own_save_of_a_shown_document_is_marked_own() {
    let (f, doc) = shown(&["a.md"], "a.md", FAST);
    let bytes = b"saved by the person in the app";
    f.watcher.own().expect_write("a.md", hash(bytes));
    write_atomic(&doc, bytes).unwrap();
    let all = wait_for(&f.seen, "the save", |c| c.iter().any(|c| c.path == "a.md"));
    let told = told(&all, "a.md");
    assert_eq!(told.len(), 1, "{all:?}");
    assert!(told[0].own, "not an outside change: {all:?}");
    assert_eq!(told[0].kind, ChangeKind::Modified);
}

#[test]
fn when_live_updates_stop_the_first_poll_of_a_shown_unchanged_document_reports_nothing() {
    let (f, doc) = shown(&["a.md"], "a.md", FAST);
    f.watcher.stop_live();
    assert_eq!(
        f.watcher.status(),
        LiveStatus::Unavailable {
            reason: "watchFailed".to_owned()
        }
    );
    // several polls go by
    let heard = quiet_for(&f.seen, 400);
    assert!(heard.is_empty(), "silent: {heard:?}");
    // shown again while polling: still nothing
    f.watcher.poll_document("w", Some(&doc));
    let heard = quiet_for(&f.seen, 300);
    assert!(heard.is_empty(), "silent: {heard:?}");
    // a change is told once
    fs::write(&doc, "changed while polling").unwrap();
    wait_for(&f.seen, "the change", |c| {
        has(c, "a.md", &ChangeKind::Modified)
    });
    let after = quiet_for(&f.seen, 300);
    assert_eq!(told(&after, "a.md").len(), 1, "{after:?}");
}

#[test]
fn a_shown_document_another_program_moves_without_a_rename_report_is_followed_by_its_window() {
    use crate::document::{DocChange, DocumentChanged};
    let base = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join("docs")).unwrap();
    fs::write(project.path().join("a.md"), "# A\n\nmoved along\n").unwrap();
    let seen: Seen = Arc::default();
    let sink = Arc::new(Recorder::default());
    let env = project_env(base.path(), &seen, sink.clone());
    let core = ProjectCore::open(Root::open(project.path()).unwrap(), &env);
    tree_shown(&core, &seen);
    // the window opens the document, as the app does
    let formats = core.formats();
    core.documents().open("w", "a.md", &[], &formats).unwrap();
    core.watcher()
        .poll_document("w", Some(&project.path().join("a.md")));
    sink.take();
    fs::copy(
        project.path().join("a.md"),
        project.path().join("docs/b.md"),
    )
    .unwrap();
    fs::remove_file(project.path().join("a.md")).unwrap();
    let started = Instant::now();
    let heard: DocumentChanged = loop {
        let event = sink.take().into_iter().find_map(|e| match e {
            CoreEvent::DocumentChanged(c) if c.window == "w" => Some(c),
            _ => None,
        });
        if let Some(event) = event {
            break event;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the window hears of its document"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(heard.path, "a.md");
    assert_eq!(
        heard.change,
        DocChange::Deleted {
            moved_to: Some("docs/b.md".to_owned())
        },
        "the window goes where the file went"
    );
}

#[test]
fn a_rescan_lists_what_no_report_told() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "a").unwrap();
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    let sink = Arc::new(Recorder::default());
    let explorer = Arc::new(Explorer::new(
        &vault,
        &ProjectConfig::default(),
        sink.clone(),
        None,
    ));
    let hub = WatchHub::new();
    let _sub = hub.subscribe({
        let explorer = Arc::clone(&explorer);
        move |batch| explorer.apply_batch(batch)
    });
    // no OS watch: nothing reports what happens on disk (as when a report is lost)
    let watcher = Watcher::start_with(
        &vault,
        explorer.ignorers(),
        hub,
        Arc::new(OwnWrites::new()),
        None,
        FAST,
        false,
    );
    explorer.open_view("w", Some(TreeFilter::default()));
    fs::create_dir_all(dir.path().join("Docs")).unwrap();
    fs::write(dir.path().join("Docs/new.md"), "new").unwrap();
    fs::remove_file(dir.path().join("a.md")).unwrap();
    assert!(explorer.listed("Docs/new.md").is_none(), "nobody told");
    assert!(explorer.listed("a.md").is_some());
    watcher.rescan();
    let started = Instant::now();
    while explorer.listed("Docs/new.md").is_none() {
        assert!(started.elapsed() < Duration::from_secs(8), "walked again");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(explorer.listed("a.md").is_none());
    // the window is told what differs
    let deltas: Vec<_> = sink
        .take()
        .into_iter()
        .filter_map(|e| match e {
            CoreEvent::TreeChanged(c) if c.window == "w" => Some(c.delta),
            _ => None,
        })
        .collect();
    assert!(
        deltas
            .iter()
            .any(|d| d.added.iter().any(|a| a.node.path == "Docs/new.md")
                && d.removed.contains(&"a.md".to_owned())),
        "{deltas:?}"
    );
}

#[test]
fn a_project_refresh_asks_every_part_to_read_the_folder_again() {
    let base = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("a.md"), "a").unwrap();
    let seen: Seen = Arc::default();
    let env = project_env(base.path(), &seen, Arc::new(Recorder::default()));
    let core = ProjectCore::open(Root::open(project.path()).unwrap(), &env);
    tree_shown(&core, &seen);
    core.refresh();
    let started = Instant::now();
    while !seen.lock().unwrap().iter().any(|b| b.rescan && b.bulk) {
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "the plugins hear the rescan"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(core.explorer().listed("a.md").is_some());
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
        hidden_last: false,
        project: None,
    };
    assert_eq!(
        core.search_filter(&tags).unwrap_err().kind,
        crate::ErrorKind::Unsupported
    );
    let options = SearchOptions {
        include_silenced: true,
        tags: Vec::new(),
        hidden_last: false,
        project: None,
    };
    let open = core.search_filter(&options).unwrap();
    assert!(open.only.is_none() && open.skip.is_empty() && open.muted.is_none());
    // no silence plugin: nothing is silenced, whatever the option says
    let hidden = SearchOptions {
        include_silenced: false,
        tags: Vec::new(),
        hidden_last: false,
        project: None,
    };
    let none = core.search_filter(&hidden).unwrap();
    assert!(none.muted.is_none());
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
    let _ = registry.show_welcome_view("main");
    let crate::workspace::TabOpen::Opening { tab, view, outcome } = registry
        .open_tab(
            "main",
            &WorkspaceRef::Project(Root::open(project.path()).unwrap()),
            None,
        )
        .unwrap()
    else {
        panic!("not opened")
    };
    for (ticket, reference) in outcome.loads {
        registry.load(ticket, &reference).unwrap();
    }
    assert_eq!(
        registry.context_of(&view).unwrap().live_updates,
        LiveUpdates::Available
    );
    // the tab closing drops that view's tree view
    let core = registry.cores().remove(0);
    core.explorer().open_view(&view, None);
    let _ = registry.close_tab(tab);
    assert!(
        core.explorer().keep(&view, None).is_err(),
        "the view went with the workspace"
    );
}

// ---- nested projects below an ignored folder -----------------------------------------------------------------------

/// A master folder whose repo ignores `hubs/`; `hubs/a` is a repo of its own.
const NESTED: &[&str] = &[
    ".git/HEAD",
    ".gitignore",
    "hubs/own.md",
    "hubs/a/.git/HEAD",
    "hubs/a/.gitignore",
    "hubs/a/doc.md",
    "hubs/a/out/",
];

fn nested_rules(base: &Path) {
    fs::write(base.join(".gitignore"), "hubs/\n*.log\n").unwrap();
    fs::write(base.join("hubs/a/.gitignore"), "out/\n").unwrap();
}

#[test]
fn changes_inside_a_nested_repo_below_an_ignored_folder_are_news_under_its_own_rules() {
    let dir = tempfile::tempdir().unwrap();
    for f in NESTED {
        let path = dir.path().join(f);
        if f.ends_with('/') {
            fs::create_dir_all(&path).unwrap();
        } else {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
    }
    nested_rules(dir.path());
    let root = Root::open(dir.path()).unwrap();
    let base = root.path().to_path_buf();
    let vault = Vault::new(root);
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    );
    let hub = WatchHub::new();
    let seen: Seen = Arc::default();
    let _sub = hub.subscribe({
        let seen = Arc::clone(&seen);
        move |batch| seen.lock().unwrap().push(batch.clone())
    });
    let _watcher = Watcher::start_with(
        &vault,
        explorer.ignorers(),
        hub,
        Arc::new(OwnWrites::new()),
        None,
        FAST,
        true,
    );
    settle(&seen);
    fs::write(base.join("hubs/a/out/y.md"), "its own rules hide it").unwrap();
    fs::write(base.join("hubs/own.md"), "the container's own file").unwrap();
    fs::write(
        base.join("hubs/a/x.log"),
        "the master's rule does not reach here",
    )
    .unwrap();
    fs::write(base.join("hubs/a/doc.md"), "an agent wrote").unwrap();
    let all = wait_for(&seen, "doc and log", |c| {
        wrote(c, "hubs/a/doc.md") && wrote(c, "hubs/a/x.log")
    });
    for never in ["hubs/a/out/y.md", "hubs/own.md"] {
        assert!(!all.iter().any(|c| c.path == never), "{never}: {all:?}");
    }
    assert!(
        seen.lock().unwrap().iter().all(|b| !b.rescan),
        "plain saves rebuild nothing"
    );
}

#[test]
fn a_nested_change_is_news_whatever_the_spelling_of_its_path() {
    let dir = tempfile::tempdir().unwrap();
    for f in NESTED {
        let path = dir.path().join(f);
        fs::create_dir_all(if f.ends_with('/') {
            path.clone()
        } else {
            path.parent().unwrap().to_path_buf()
        })
        .unwrap();
        if !f.ends_with('/') {
            fs::write(path, "x").unwrap();
        }
    }
    nested_rules(dir.path());
    let root = Root::open(dir.path()).unwrap();
    let base = root.path().to_path_buf();
    let vault = Vault::new(root);
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    );
    let hub = WatchHub::new();
    let seen: Seen = Arc::default();
    let _sub = hub.subscribe({
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
        false,
    );
    let doc = base.join("hubs").join("a").join("doc.md");
    let mut spellings = vec![doc.clone()];
    if cfg!(windows) {
        spellings.push(PathBuf::from(format!(r"\\?\{}", doc.display())));
        spellings.push(PathBuf::from(doc.to_string_lossy().to_uppercase()));
        spellings.push(PathBuf::from(
            doc.to_string_lossy()
                .replace("hubs", "HUBS")
                .replace("doc.md", "Doc.MD"),
        ));
    }
    for (n, spelling) in spellings.into_iter().enumerate() {
        seen.lock().unwrap().clear();
        fs::write(&doc, format!("version {n}")).unwrap();
        watcher.inject(Pending {
            abs: spelling.clone(),
            created: false,
            removed: false,
            modified: true,
            rename_from: None,
            after_removal: None,
            deferred: false,
        });
        let all = wait_for(&seen, &format!("{}", spelling.display()), |c| !c.is_empty());
        assert_eq!(all.len(), 1, "{}: {all:?}", spelling.display());
        assert_eq!(
            crate::tree::key_of(&all[0].path),
            crate::tree::key_of("hubs/a/doc.md"),
            "{}",
            spelling.display()
        );
        assert_eq!(all[0].kind, ChangeKind::Modified);
    }
}

#[test]
fn nested_projects_coming_and_going_while_open_update_tree_index_and_watch() {
    use crate::search::SearchFilter;
    let base_dir = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = Root::open(project.path()).unwrap();
    let base = root.path().to_path_buf();
    let put = |rel: &str, text: &str| {
        let path = base.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    put(".git/HEAD", "ref: refs/heads/main\n");
    put(".gitignore", "hubs/\n");
    put("hubs/a/.git/HEAD", "ref: refs/heads/main\n");
    put("hubs/a/doc.md", "# A\n\nalphaword\n");
    put("hubs/c/.git/HEAD", "ref: refs/heads/main\n");
    put("hubs/c/c.md", "# C\n\ncharlieword\n");
    put("hubs/plain/n.md", "# N\n\ngammaword\n");
    let seen: Seen = Arc::default();
    let env = project_env(base_dir.path(), &seen, Arc::new(Recorder::default()));
    let core = ProjectCore::open(root, &env);
    assert!(core.search().wait_ready(Duration::from_secs(30)));
    let found = |q: &str| -> Vec<String> {
        core.search()
            .sections(q, 10, &SearchFilter::default())
            .hits
            .into_iter()
            .map(|h| h.path)
            .collect()
    };
    let listed = |path: &str| core.explorer().listed(path).is_some();
    let until = |what: &str, ok: &dyn Fn() -> bool| {
        let started = Instant::now();
        while !ok() {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "timed out: {what}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    assert_eq!(found("alphaword"), ["hubs/a/doc.md"]);
    assert!(found("gammaword").is_empty(), "a plain ignored folder");
    assert!(listed("hubs/a/doc.md") && !listed("hubs/plain/n.md"));
    settle(&seen);

    // an agent writes inside the nested repo: the plugins hear it, the index follows
    put("hubs/a/doc.md", "# A\n\nbetaword\n");
    wait_for(&seen, "the plugin hears the nested change", |c| {
        wrote(c, "hubs/a/doc.md")
    });
    until("the index follows", &|| {
        found("betaword") == ["hubs/a/doc.md"]
    });

    // `git init` inside the plain folder: it becomes a project, without reopening
    put("hubs/plain/.git/HEAD", "ref: refs/heads/main\n");
    until("the new repo is listed", &|| listed("hubs/plain/n.md"));
    until("and indexed", &|| found("gammaword") == ["hubs/plain/n.md"]);
    seen.lock().unwrap().clear();
    put("hubs/plain/n.md", "# N\n\nepsilonword\n");
    wait_for(&seen, "and watched", |c| wrote(c, "hubs/plain/n.md"));

    // a project folder renamed while open
    fs::rename(base.join("hubs/a"), base.join("hubs/b")).unwrap();
    until("the renamed project is listed", &|| {
        listed("hubs/b/doc.md") && !listed("hubs/a/doc.md")
    });
    seen.lock().unwrap().clear();
    put("hubs/b/doc.md", "# B\n\ndeltaword\n");
    until("and watched under its new name", &|| {
        found("deltaword") == ["hubs/b/doc.md"]
    });
    assert!(found("betaword").is_empty(), "nothing left at the old name");

    // a project moved out of the folder (the system tells only of the folder)
    let outside = tempfile::tempdir_in(base.parent().unwrap()).unwrap();
    fs::rename(base.join("hubs/c"), outside.path().join("c")).unwrap();
    until("the moved-out project is gone", &|| {
        !listed("hubs/c") && found("charlieword").is_empty()
    });

    // its `.git` deleted: the folder is plain and ignored again
    fs::remove_dir_all(base.join("hubs/plain/.git")).unwrap();
    until("the former repo is ignored", &|| {
        !listed("hubs/plain/n.md") && found("epsilonword").is_empty()
    });

    // the last project deleted: `hubs` is a plain ignored folder; a folder made there again is not news
    fs::remove_dir_all(base.join("hubs/b")).unwrap();
    until("the deleted project is gone", &|| {
        !listed("hubs/b") && found("deltaword").is_empty()
    });
    until("hubs is no container any more", &|| {
        matches!(
            core.explorer().listed("hubs"),
            Some(crate::tree::NodeKind::Dir {
                unlisted: Some(crate::tree::Unlisted::Ignored)
            })
        )
    });
    std::thread::sleep(Duration::from_millis(200));
    seen.lock().unwrap().clear();
    put("hubs/b/z.md", "# Z\n\nzetaword\n");
    let heard = quiet_for(&seen, 600);
    assert!(
        !heard.iter().any(|c| c.path.starts_with("hubs/b")),
        "{heard:?}"
    );
    assert!(!listed("hubs/b/z.md"));
}

#[test]
fn a_new_direct_child_of_an_opened_ignored_folder_is_listed_and_a_deeper_one_is_not() {
    use crate::tree::{NodeKind, Unlisted};
    let base_dir = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = Root::open(project.path()).unwrap();
    let base = root.path().to_path_buf();
    let put = |rel: &str, text: &str| {
        let path = base.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    fs::create_dir_all(base.join("data")).unwrap();
    fs::create_dir_all(base.join("sealed")).unwrap();
    put(".gitignore", "data/\nsealed/\n");
    put("readme.md", "# R\n");
    let seen: Seen = Arc::default();
    let env = project_env(base_dir.path(), &seen, Arc::new(Recorder::default()));
    let core = ProjectCore::open(root, &env);
    let listed = |path: &str| core.explorer().listed(path);
    let closed_ignored = |path: &str| {
        matches!(
            listed(path),
            Some(NodeKind::Dir {
                unlisted: Some(Unlisted::Ignored)
            })
        )
    };
    let until = |what: &str, ok: &dyn Fn() -> bool| {
        let started = Instant::now();
        while !ok() {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "timed out: {what}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    until("both ignored folders are closed", &|| {
        closed_ignored("data") && closed_ignored("sealed")
    });
    settle(&seen);
    core.explorer().open_folder("data").unwrap();
    assert!(matches!(
        listed("data"),
        Some(NodeKind::Dir { unlisted: None })
    ));
    assert!(
        closed_ignored("sealed"),
        "the other ignored folder stays closed"
    );
    seen.lock().unwrap().clear();

    // `data` is opened: a new dir is listed closed, a new file is listed, the dir's own child is not.
    // `sealed` was never opened: a new child is not news and is not listed.
    fs::create_dir_all(base.join("data/work/train")).unwrap();
    put("data/work/train/a.md", "a");
    put("data/work/train/b.md", "b");
    put("data/note.md", "n");
    put("sealed/hid.md", "h");
    until("the opened folder lists its new dir and file", &|| {
        closed_ignored("data/work") && matches!(listed("data/note.md"), Some(NodeKind::File { .. }))
    });
    let heard = quiet_for(&seen, 600);
    assert!(
        matches!(listed("data"), Some(NodeKind::Dir { unlisted: None })),
        "opening data survives the creates"
    );
    assert!(closed_ignored("data/work"), "{:?}", listed("data/work"));
    assert!(matches!(
        listed("data/note.md"),
        Some(NodeKind::File { .. })
    ));
    for absent in [
        "data/work/train",
        "data/work/train/a.md",
        "data/work/train/b.md",
        "sealed/hid.md",
    ] {
        assert!(listed(absent).is_none(), "{absent} listed");
        assert!(
            !heard
                .iter()
                .any(|c| c.path == absent || c.path.starts_with(&format!("{absent}/"))),
            "{absent} was news: {heard:?}"
        );
    }
    assert!(
        heard.iter().any(|c| c.path == "data/work" && c.is_dir),
        "the new dir came through the watch: {heard:?}"
    );
    assert!(
        heard.iter().any(|c| c.path == "data/note.md"),
        "the new file came through the watch: {heard:?}"
    );
}

// ---- verifier: rescan storms -----------------------------------------------------------------------------------------

#[test]
fn checkout_and_install_churn_inside_a_nested_repo_and_a_shipped_permark_folder_never_rescan() {
    let dir = tempfile::tempdir().unwrap();
    for f in NESTED {
        let path = dir.path().join(f);
        if f.ends_with('/') {
            fs::create_dir_all(&path).unwrap();
        } else {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
    }
    nested_rules(dir.path());
    let root = Root::open(dir.path()).unwrap();
    let base = root.path().to_path_buf();
    let vault = Vault::new(root);
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    );
    let hub = WatchHub::new();
    let seen: Seen = Arc::default();
    let _sub = hub.subscribe({
        let seen = Arc::clone(&seen);
        move |batch| seen.lock().unwrap().push(batch.clone())
    });
    let _watcher = Watcher::start_with(
        &vault,
        explorer.ignorers(),
        hub,
        Arc::new(OwnWrites::new()),
        None,
        FAST,
        true,
    );
    settle(&seen);
    let a = base.join("hubs/a");
    // a checkout: index.lock -> index, HEAD, refs, objects; files created, changed, removed; a folder made
    fs::write(a.join(".git/index.lock"), "i").unwrap();
    fs::rename(a.join(".git/index.lock"), a.join(".git/index")).unwrap();
    fs::write(a.join(".git/HEAD"), "ref: refs/heads/other\n").unwrap();
    fs::create_dir_all(a.join(".git/objects/ab")).unwrap();
    fs::write(a.join(".git/objects/ab/cd"), "o").unwrap();
    fs::create_dir_all(a.join("src/deep")).unwrap();
    for n in 0..60 {
        fs::write(a.join(format!("src/deep/f{n}.md")), format!("{n}")).unwrap();
    }
    fs::remove_file(a.join("doc.md")).unwrap();
    // an install: many folders below node_modules, none a project
    for n in 0..40 {
        fs::create_dir_all(a.join(format!("node_modules/p{n}/lib"))).unwrap();
        fs::write(a.join(format!("node_modules/p{n}/lib/i.js")), "x").unwrap();
    }
    // atomic saves (temp + rename) of a plain file
    fs::write(a.join("src/.tmpsave"), "y").unwrap();
    fs::rename(a.join("src/.tmpsave"), a.join("src/deep/f1.md")).unwrap();
    let _ = wait_for(&seen, "the churn", |c| {
        c.iter().any(|x| x.path.starts_with("hubs/a/src"))
    });
    let _ = quiet_for(&seen, 600);
    let rescans = seen.lock().unwrap().iter().filter(|b| b.rescan).count();
    assert_eq!(rescans, 0, "churn without markers rebuilds nothing");
    // an npm package that ships its `.permark` folder: no project, no rescan, nothing below it is news
    seen.lock().unwrap().clear();
    let staging = tempfile::tempdir_in(base.parent().unwrap()).unwrap();
    fs::create_dir_all(staging.path().join("pkg/.permark")).unwrap();
    fs::write(staging.path().join("pkg/.permark/config.toml"), "").unwrap();
    fs::write(staging.path().join("pkg/README.md"), "# pkg").unwrap();
    fs::rename(staging.path().join("pkg"), a.join("node_modules/pkg")).unwrap();
    // a `.permark` folder made inside the ignored `out/` of the nested repo: same
    fs::create_dir_all(a.join("out/.permark")).unwrap();
    fs::write(a.join("out/.permark/config.toml"), "").unwrap();
    fs::write(a.join("out/o.md"), "# o").unwrap();
    let heard = quiet_for(&seen, 1500);
    let rescans = seen.lock().unwrap().iter().filter(|b| b.rescan).count();
    assert_eq!(rescans, 0, "a `.permark` folder changes no decision");
    assert!(
        !heard
            .iter()
            .any(|c| c.path.contains("node_modules/pkg") || c.path.starts_with("hubs/a/out")),
        "{heard:?}"
    );
}

/// The watch caches a nested repo's `.git/info/exclude`: an edit of it is seen (the batch clears the cache) and counts
/// for the next decisions.
#[test]
fn an_edited_exclude_file_of_a_nested_repo_counts_at_once() {
    let dir = tempfile::tempdir().unwrap();
    for f in NESTED {
        let path = dir.path().join(f);
        if f.ends_with('/') {
            fs::create_dir_all(&path).unwrap();
        } else {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
    }
    nested_rules(dir.path());
    fs::create_dir_all(dir.path().join("hubs/a/.git/info")).unwrap();
    fs::write(dir.path().join("hubs/a/.git/info/exclude"), "").unwrap();
    let root = Root::open(dir.path()).unwrap();
    let base = root.path().to_path_buf();
    let vault = Vault::new(root);
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    );
    let hub = WatchHub::new();
    let seen: Seen = Arc::default();
    let _sub = hub.subscribe({
        let seen = Arc::clone(&seen);
        move |batch| seen.lock().unwrap().push(batch.clone())
    });
    let _watcher = Watcher::start_with(
        &vault,
        explorer.ignorers(),
        hub,
        Arc::new(OwnWrites::new()),
        None,
        FAST,
        true,
    );
    settle(&seen);
    fs::write(base.join("hubs/a/draft.md"), "1").unwrap();
    wait_for(&seen, "draft is news first", |c| {
        wrote(c, "hubs/a/draft.md")
    });
    let _ = quiet_for(&seen, 400);
    fs::write(base.join("hubs/a/.git/info/exclude"), "draft.md\n").unwrap();
    // its subscribers hear the change too: what the tree and the index list follows the new rules
    let heard = quiet_for(&seen, 600);
    assert!(
        heard.iter().any(|c| c.path == "hubs/a/.git/info/exclude"),
        "{heard:?}"
    );
    seen.lock().unwrap().clear();
    fs::write(base.join("hubs/a/draft.md"), "2").unwrap();
    fs::write(base.join("hubs/a/doc.md"), "an agent wrote").unwrap();
    wait_for(&seen, "doc", |c| wrote(c, "hubs/a/doc.md"));
    let all = quiet_for(&seen, 400);
    assert!(!all.iter().any(|c| c.path == "hubs/a/draft.md"), "{all:?}");
}

// ---- moves of a project folder ---------------------------------------------------------------------------------------

/// The batch's `moved` pairs for a project folder `inner/beta` (an included member) after `act` changed the disk and
/// the OS reported `events` (fed through the pairing as the native handler does).
fn moved_after(act: impl Fn(&Path), events: impl Fn(&Path) -> Vec<Event>) -> Vec<(String, String)> {
    let dir = tempfile::tempdir().unwrap();
    for f in ["inner/beta/.git/HEAD", "inner/beta/plan.md", "other/x.md"] {
        let path = dir.path().join(f);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }
    let root = Root::open(dir.path()).unwrap();
    let base = root.path().to_path_buf();
    let vault = Vault::new(root);
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    );
    explorer.set_members(&crate::tree::MemberSet {
        members: vec![(
            "inner/beta".to_owned(),
            crate::tree::MemberRule {
                hidden: crate::projects::HiddenRule::Own,
                git: true,
                ignore: Vec::new(),
            },
        )],
    });
    let hub = WatchHub::new();
    let seen: Seen = Arc::default();
    let _sub = hub.subscribe({
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
        false,
    );
    act(&base);
    let mut pairs = RenamePairs::default();
    for event in events(&base) {
        for hint in pairs.hints(&event) {
            watcher.inject(hint);
        }
    }
    let started = Instant::now();
    while seen.lock().unwrap().is_empty() && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(200));
    let moved: Vec<(String, String)> = seen
        .lock()
        .unwrap()
        .iter()
        .flat_map(|b| b.moved.clone())
        .collect();
    moved
}

fn at(base: &Path, kind: EventKind, rel: &[&str]) -> Event {
    let mut e = Event::new(kind);
    for r in rel {
        e = e.add_path(base.join(r));
    }
    e
}

fn mv(base: &Path, from: &str, to: &str) {
    fs::rename(base.join(from), base.join(to)).unwrap();
}

fn pair(from: &str, to: &str) -> Vec<(String, String)> {
    vec![(from.to_owned(), to.to_owned())]
}

#[test]
fn a_project_folder_move_is_paired_whatever_the_system_reported() {
    let name = |mode| EventKind::Modify(ModifyKind::Name(mode));
    let removed = EventKind::Remove(RemoveKind::Any);
    let created = EventKind::Create(CreateKind::Any);
    // Linux: one event
    let got = moved_after(
        |b| mv(b, "inner/beta", "inner/beta2"),
        |b| {
            vec![at(
                b,
                name(RenameMode::Both),
                &["inner/beta", "inner/beta2"],
            )]
        },
    );
    assert_eq!(got, pair("inner/beta", "inner/beta2"));
    // Windows rename: old name, new name
    let got = moved_after(
        |b| mv(b, "inner/beta", "inner/beta2"),
        |b| {
            vec![
                at(b, name(RenameMode::From), &["inner/beta"]),
                at(b, name(RenameMode::To), &["inner/beta2"]),
            ]
        },
    );
    assert_eq!(got, pair("inner/beta", "inner/beta2"));
    // Windows move to another parent, renamed on the way: removal, creation
    let got = moved_after(
        |b| mv(b, "inner/beta", "other/gamma"),
        |b| {
            vec![
                at(b, removed, &["inner/beta"]),
                at(b, created, &["other/gamma"]),
            ]
        },
    );
    assert_eq!(got, pair("inner/beta", "other/gamma"));
    // macOS: two renames at once, each step's events undirected (the chain's start lost)
    let got = moved_after(
        |b| {
            mv(b, "inner/beta", "inner/beta2");
            mv(b, "inner/beta2", "inner/beta3");
        },
        |b| {
            vec![
                at(b, name(RenameMode::Any), &["inner/beta"]),
                at(b, name(RenameMode::Any), &["inner/beta2"]),
                at(b, name(RenameMode::Any), &["inner/beta2"]),
                at(b, name(RenameMode::Any), &["inner/beta3"]),
            ]
        },
    );
    assert_eq!(got, pair("inner/beta", "inner/beta3"));
    // removal + creation apart, same name: paired by name
    let got = moved_after(
        |b| mv(b, "inner/beta", "other/beta"),
        |b| {
            vec![
                at(b, removed, &["inner/beta"]),
                at(b, EventKind::Modify(ModifyKind::Any), &["other/x.md"]),
                at(b, created, &["other/beta"]),
            ]
        },
    );
    assert_eq!(got, pair("inner/beta", "other/beta"));
}

#[test]
fn unrelated_folders_are_never_taken_for_a_project_move() {
    let removed = EventKind::Remove(RemoveKind::Any);
    let created = EventKind::Create(CreateKind::Any);
    // the project went, two folders of other names came, not right after it
    let got = moved_after(
        |b| {
            fs::remove_dir_all(b.join("inner/beta")).unwrap();
            fs::create_dir_all(b.join("other/gamma")).unwrap();
            fs::create_dir_all(b.join("other/delta")).unwrap();
        },
        |b| {
            vec![
                at(b, removed, &["inner/beta"]),
                at(b, EventKind::Modify(ModifyKind::Any), &["other/x.md"]),
                at(b, created, &["other/gamma"]),
                at(b, created, &["other/delta"]),
            ]
        },
    );
    assert_eq!(got, Vec::new());
    // a plain folder (no project) renamed: nothing for the projects
    let got = moved_after(
        |b| mv(b, "other", "other2"),
        |b| {
            vec![
                at(
                    b,
                    EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                    &["other"],
                ),
                at(
                    b,
                    EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                    &["other2"],
                ),
            ]
        },
    );
    assert_eq!(got, Vec::new());
    // the project went out of the folder; a file with its name came
    let outside = tempfile::tempdir().unwrap();
    let got = moved_after(
        |b| {
            fs::rename(b.join("inner/beta"), outside.path().join("beta")).unwrap();
            fs::write(b.join("other/beta"), "x").unwrap();
        },
        |b| {
            vec![
                at(b, removed, &["inner/beta"]),
                at(b, created, &["other/beta"]),
            ]
        },
    );
    assert_eq!(got, Vec::new());
}

// ---- the reconcile pass, called directly -----------------------------------------------------------------------------

/// A project folder for the reconcile pass without OS events: hints are made by the test.
struct Direct {
    _dir: tempfile::TempDir,
    vault: Vault,
    ignorers: Vec<(String, Arc<crate::tree::Ignorer>)>,
}

impl Direct {
    fn new() -> Self {
        Self::new_at(tempfile::tempdir().unwrap())
    }

    fn new_at(dir: tempfile::TempDir) -> Self {
        let vault = Vault::new(Root::open(dir.path()).unwrap());
        let ignorers = Explorer::new(
            &vault,
            &ProjectConfig::default(),
            Arc::new(Recorder::default()),
            None,
        )
        .ignorers();
        Self {
            _dir: dir,
            vault,
            ignorers,
        }
    }

    fn abs(&self, rel: &str) -> PathBuf {
        self.vault.main().path().join(rel)
    }

    fn hint(&self, rel: &str) -> Pending {
        Pending {
            abs: self.abs(rel),
            created: false,
            removed: false,
            modified: false,
            rename_from: None,
            after_removal: None,
            deferred: false,
        }
    }

    fn pass(
        &self,
        sigs: &SigStore,
        known: &dyn Fn(&str) -> Option<bool>,
        pending: Vec<Pending>,
    ) -> Vec<Change> {
        let env = Env {
            vault: &self.vault,
            sigs,
            ignorers: &self.ignorers,
            known,
            opened_ignored: &|_| false,
            wanted: &|_| false,
        };
        reconcile(&env, pending).changes
    }
}

fn renames(changes: &[Change]) -> usize {
    changes
        .iter()
        .filter(|c| matches!(c.kind, ChangeKind::Renamed { .. }))
        .count()
}

#[test]
fn moves_reported_together_are_paired_through_the_pass() {
    let fx = Direct::new();
    let n = 200;
    let left: Vec<String> = (0..n).map(|i| format!("a/f{i}.md")).collect();
    let right: Vec<String> = (0..n).map(|i| format!("b/f{i}.md")).collect();
    fs::create_dir_all(fx.abs("a")).unwrap();
    fs::create_dir_all(fx.abs("b")).unwrap();
    let sigs = SigStore::default();
    for (i, rel) in left.iter().enumerate() {
        fs::write(fx.abs(rel), format!("moved {i}")).unwrap();
        let meta = fs::symlink_metadata(fx.abs(rel)).unwrap();
        sigs.set(
            crate::tree::key_of(rel),
            rel,
            sig_of(&fx.abs(rel), &meta).unwrap(),
        );
    }
    // renames the OS reported (each creation names where it came from) and moves found by content
    let mut hints = Vec::new();
    for (i, (from, to)) in left.iter().zip(&right).enumerate() {
        fs::rename(fx.abs(from), fx.abs(to)).unwrap();
        hints.push(Pending {
            removed: true,
            ..fx.hint(from)
        });
        hints.push(Pending {
            created: true,
            rename_from: (i % 2 == 0).then(|| fx.abs(from)),
            ..fx.hint(to)
        });
    }
    let changes = fx.pass(&sigs, &|_| None, hints);
    assert_eq!((renames(&changes), changes.len()), (n, n));
    // moves the app announced, reported as removals + creations of files the watch never read
    let own = OwnWrites::new();
    let pairs: Vec<(String, String)> = right.iter().cloned().zip(left.iter().cloned()).collect();
    own.expect_moves(&pairs);
    let mut hints = Vec::new();
    for (from, to) in &pairs {
        fs::rename(fx.abs(from), fx.abs(to)).unwrap();
        hints.push(Pending {
            removed: true,
            ..fx.hint(from)
        });
        hints.push(Pending {
            created: true,
            ..fx.hint(to)
        });
    }
    let mut changes = fx.pass(&SigStore::default(), &|path| own.moved_from(path), hints);
    own.pair_moves(&mut changes);
    assert_eq!((renames(&changes), changes.len()), (n, n));
}

fn sig(len: u64, hash: u64) -> super::sigs::Sig {
    super::sigs::Sig {
        len,
        mtime_ms: 1,
        hash,
    }
}

fn removal(path: String) -> Change {
    Change {
        path,
        kind: ChangeKind::Removed,
        is_dir: false,
        own: false,
    }
}

fn creation(path: String) -> Change {
    Change {
        path,
        kind: ChangeKind::Created,
        is_dir: false,
        own: false,
    }
}

#[test]
fn thousands_of_moves_in_one_batch_are_paired_in_one_pass() {
    let n = 5000;
    // removals, then creations: half name where they came from, half hold the same content
    let mut changes: Vec<Change> = (0..n).map(|i| removal(format!("a/f{i}.md"))).collect();
    changes.extend((0..n).map(|i| creation(format!("b/f{i}.md"))));
    let removed: Vec<(usize, super::sigs::Sig)> =
        (0..n).map(|i| (i, sig(10, i as u64 + 1))).collect();
    let created: Vec<super::reconcile::Created> = (0..n)
        .map(|i| super::reconcile::Created {
            at: n + i,
            sig: sig(10, i as u64 + 1),
            from: (i % 2 == 0).then(|| format!("a/f{i}.md")),
            unread: None,
        })
        .collect();
    let started = Instant::now();
    super::reconcile::pair_renames(&mut changes, created, &removed, &mut |_, _| None);
    let took = started.elapsed();
    assert_eq!((renames(&changes), changes.len()), (n, n));
    // moves the app announced: each removal asks whether it was one, then the pairs are made
    let own = OwnWrites::new();
    let pairs: Vec<(String, String)> = (0..n)
        .map(|i| (format!("b/f{i}.md"), format!("c/f{i}.md")))
        .collect();
    own.expect_moves(&pairs);
    let mut changes: Vec<Change> = pairs
        .iter()
        .map(|(from, _)| removal(from.clone()))
        .collect();
    changes.extend(pairs.iter().map(|(_, to)| creation(to.clone())));
    let started = Instant::now();
    for (from, _) in &pairs {
        assert_eq!(own.moved_from(from), Some(false));
    }
    assert_eq!(own.moved_from("b"), Some(true));
    own.pair_moves(&mut changes);
    let took = took + started.elapsed();
    assert_eq!((renames(&changes), changes.len()), (n, n));
    assert!(took < Duration::from_secs(2), "{took:?}");
}

/// Reference for the property check: each named partner searched through every removal, paired removals taken out
/// one at a time.
fn pair_renames_one_by_one(
    changes: &mut Vec<Change>,
    created: Vec<super::reconcile::Created>,
    removed: &[(usize, super::sigs::Sig)],
) {
    let key_of = crate::tree::key_of;
    let mut by_sig: std::collections::HashMap<(u64, u64), usize> = std::collections::HashMap::new();
    for (at, sig) in removed {
        if sig.len > 0 && sig.hash != 0 {
            by_sig.entry((sig.len, sig.hash)).or_insert(*at);
        }
    }
    let mut dropped: Vec<usize> = Vec::new();
    for super::reconcile::Created { at, sig, from, .. } in created {
        let direct = from.and_then(|from| {
            removed
                .iter()
                .find(|(at, _)| key_of(&changes[*at].path) == key_of(&from))
                .map(|(at, _)| *at)
        });
        let partner = direct.or_else(|| {
            (sig.len > 0 && sig.hash != 0)
                .then(|| by_sig.get(&(sig.len, sig.hash)).copied())
                .flatten()
        });
        let Some(removed_at) = partner else {
            continue;
        };
        if dropped.contains(&removed_at) {
            continue;
        }
        let from = changes[removed_at].path.clone();
        if key_of(&from) == key_of(&changes[at].path) {
            continue;
        }
        changes[at].kind = ChangeKind::Renamed { from };
        dropped.push(removed_at);
    }
    dropped.sort_unstable();
    for at in dropped.into_iter().rev() {
        changes.remove(at);
    }
}

mod pairing {
    use proptest::prelude::*;

    use super::super::reconcile::{pair_renames, Created};
    use super::super::sigs::Sig;
    use super::*;

    const PATHS: [&str; 6] = ["a.md", "A.md", "b.md", "c/d.md", "C/d.md", "e.md"];

    /// (0 removed | 1 created | 2 modified, path, len, hash, named origin, created file not read yet)
    type Item = (u8, usize, u64, u64, Option<usize>, bool);

    /// The pass's input; `unread`: creations marked not read yet keep their content to themselves (hash 0) until read.
    fn inputs(items: &[Item], unread: bool) -> (Vec<Change>, Vec<Created>, Vec<(usize, Sig)>) {
        let (mut changes, mut created, mut removed) = (Vec::new(), Vec::new(), Vec::new());
        for (at, (what, path, len, hash, from, lazy)) in items.iter().enumerate() {
            let path = PATHS[*path].to_owned();
            match what {
                0 => {
                    changes.push(removal(path));
                    removed.push((at, sig(*len, *hash)));
                }
                1 => {
                    changes.push(creation(path));
                    let lazy = unread && *lazy;
                    created.push(Created {
                        at,
                        sig: sig(*len, if lazy { 0 } else { *hash }),
                        from: from.map(|f| PATHS[f].to_owned()),
                        unread: lazy.then(|| PathBuf::from(at.to_string())),
                    });
                }
                _ => changes.push(Change {
                    path,
                    kind: ChangeKind::Modified,
                    is_dir: false,
                    own: false,
                }),
            }
        }
        (changes, created, removed)
    }

    proptest! {
        #[test]
        fn renames_pair_as_each_partner_searched_one_by_one(
            items in proptest::collection::vec(
                (
                    0..3_u8,
                    0..PATHS.len(),
                    0..3_u64,
                    0..3_u64,
                    proptest::option::of(0..PATHS.len()),
                    proptest::bool::ANY,
                ),
                0..14,
            ),
        ) {
            // the files not read yet are read on demand (their content as it is): same pairs as all read at once
            let (mut indexed, created, removed) = inputs(&items, true);
            let mut read = |abs: &Path, _: &str| {
                let at: usize = abs.to_str()?.parse().ok()?;
                let (_, _, len, hash, _, _) = items[at];
                Some(sig(len, hash))
            };
            pair_renames(&mut indexed, created, &removed, &mut read);
            let (mut scanned, created, removed) = inputs(&items, false);
            pair_renames_one_by_one(&mut scanned, created, &removed);
            prop_assert_eq!(indexed, scanned);
        }
    }
}

// ---- git operations in progress ----------------------------------------------------------------------------------------

/// Reports a change of the project file `rel` as the OS would.
fn report_modified(f: &Fx, rel: &str) {
    f.watcher.inject(Pending {
        abs: dunce::canonicalize(f.dir.path().join(rel)).unwrap(),
        created: false,
        removed: false,
        modified: true,
        rename_from: None,
        after_removal: None,
        deferred: false,
    });
}

/// Writes `text` into the project file `rel`, reports it and returns the batch that told it.
fn told_batch(f: &Fx, rel: &str, text: &str) -> FsBatch {
    fs::write(f.dir.path().join(rel), text).unwrap();
    report_modified(f, rel);
    batch_telling(f, rel)
}

/// The batch that told `rel` (taken out of what was seen).
fn batch_telling(f: &Fx, rel: &str) -> FsBatch {
    let started = Instant::now();
    loop {
        let found = {
            let mut seen = f.seen.lock().unwrap();
            let at = seen
                .iter()
                .position(|b| b.changes.iter().any(|c| c.path == rel));
            at.map(|at| seen.remove(at))
        };
        if let Some(batch) = found {
            return batch;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no batch told {rel}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_git_operation_makes_a_batch_bulk_only_when_it_starts_or_moves_on() {
    // a merge waiting for its commit (its marker stays for hours): the person's saves are plain batches
    let f = make(&[".git/HEAD", ".git/MERGE_HEAD", "a.md", "b.md"], false);
    let git = f.dir.path().join(".git");
    assert!(!told_batch(&f, "a.md", "one").bulk, "a save during a merge");
    // another operation begins (its marker appears): the batch that sees it is bulk, the next save is not
    fs::write(git.join("CHERRY_PICK_HEAD"), "c").unwrap();
    assert!(told_batch(&f, "b.md", "two").bulk, "an operation began");
    assert!(!told_batch(&f, "a.md", "three").bulk, "nothing new");
    // the operation moves on (HEAD, the index, refs change with the files): bulk while a marker is there
    fs::write(git.join("HEAD"), "ref: refs/heads/other\n").unwrap();
    report_modified(&f, ".git/HEAD");
    assert!(
        batch_telling(&f, ".git/HEAD").bulk,
        "HEAD moved during the operation"
    );
    // the index lock stays a bulk reason as long as it is there
    fs::write(git.join("index.lock"), "l").unwrap();
    assert!(told_batch(&f, "b.md", "five").bulk, "index.lock");
    assert!(told_batch(&f, "a.md", "six").bulk, "index.lock still there");
    fs::remove_file(git.join("index.lock")).unwrap();
    // the operations end: plain batches
    fs::remove_file(git.join("MERGE_HEAD")).unwrap();
    fs::remove_file(git.join("CHERRY_PICK_HEAD")).unwrap();
    assert!(!told_batch(&f, "b.md", "seven").bulk, "no marker");
}

// ---- storms in ignored folders -------------------------------------------------------------------------------------------

#[test]
fn a_storm_in_an_ignored_folder_is_dropped_before_it_counts() {
    // an install then `rm -rf node_modules`: more reports than the watch keeps before it gives up and asks for a rescan
    let window = Timing {
        quiet: Duration::from_millis(300),
        max: Duration::from_secs(60),
        ..FAST
    };
    let f = make_with(&["a.md", "node_modules/"], false, window);
    let base = dunce::canonicalize(f.dir.path()).unwrap();
    let n = 60_000;
    for removed in [false, true] {
        for i in 0..n {
            // packages ship their own ignore files
            let name = if i % 100 == 0 {
                ".gitignore".to_owned()
            } else {
                format!("f{i}.js")
            };
            f.watcher.inject(Pending {
                abs: base.join(format!("node_modules/pkg{}/lib/{name}", i / 100)),
                created: !removed,
                removed,
                modified: false,
                rename_from: None,
                after_removal: None,
                deferred: false,
            });
        }
    }
    let fence = told_batch(&f, "a.md", "after the storm");
    assert!(!fence.rescan, "{} changes", fence.changes.len());
    // each hint taken in is looked at on disk: the storm's files never are (the fence is)
    assert!(f.watcher.admitted() <= 1, "{}", f.watcher.admitted());
    let seen = f.seen.lock().unwrap();
    assert!(seen.iter().all(|b| !b.rescan), "{} batches", seen.len());
}

#[test]
fn only_hints_the_pass_would_drop_unseen_are_dropped_early() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
    fs::create_dir_all(dir.path().join("build/out")).unwrap();
    fs::create_dir_all(dir.path().join("open")).unwrap();
    fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    let fx = Direct::new_at(dir);
    let sigs = SigStore::default();
    let opened = |path: &str| path == "build/out";
    let env = Env {
        vault: &fx.vault,
        sigs: &sigs,
        ignorers: &fx.ignorers,
        known: &|_| None,
        opened_ignored: &opened,
        wanted: &|_| false,
    };
    let mut folders = std::collections::HashMap::new();
    let mut early = |hint: Pending| env.ignored_early(&hint, &mut folders);
    let created = |rel: &str| Pending {
        created: true,
        ..fx.hint(rel)
    };
    let removed = |rel: &str| Pending {
        removed: true,
        ..fx.hint(rel)
    };
    // deep in an ignored folder: dropped, whatever happened
    assert!(early(created("node_modules/pkg/lib/a.js")));
    assert!(early(removed("node_modules/pkg/lib/a.js")));
    assert!(early(Pending {
        modified: true,
        ..fx.hint("build/x/y.md")
    }));
    assert!(early(removed("build/x")));
    // a creation below the topmost ignored folder may be a repository moved in, within the depth the search for
    // repositories looks; never one below `node_modules`
    assert!(!early(created("build/x")));
    assert!(!early(created("build/x/repo")));
    assert!(!early(created("build/x/y/repo")));
    assert!(early(created("build/x/y/z/repo")));
    assert!(early(created("node_modules/new")));
    // shown folders, `.git`, moves and children of an opened ignored folder go on to the pass
    assert!(!early(created("open/a.md")));
    assert!(!early(created("node_modules/pkg/sub/.git")));
    assert!(!early(Pending {
        modified: true,
        ..fx.hint("build/x/.git/HEAD")
    }));
    assert!(!early(Pending {
        created: true,
        rename_from: Some(fx.abs("open/a.md")),
        ..fx.hint("build/x/a.md")
    }));
    assert!(!early(created("build/out/new.md")));
}

#[test]
fn a_repository_moved_into_a_folder_inside_an_ignored_one_is_seen() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    fs::create_dir_all(dir.path().join("build/x")).unwrap();
    fs::write(dir.path().join("build/x/old.md"), "x").unwrap();
    let f = watch_dir(dir, false, FAST);
    let base = dunce::canonicalize(f.dir.path()).unwrap();
    // a change inside has the watch find `build` holding no repository
    report_modified(&f, "build/x/old.md");
    told_batch(&f, "a.md", "one");
    // a repository moved in from outside arrives as one creation
    let outside = tempfile::tempdir_in(base.parent().unwrap()).unwrap();
    fs::create_dir_all(outside.path().join("repo/.git")).unwrap();
    fs::write(
        outside.path().join("repo/.git/HEAD"),
        "ref: refs/heads/main\n",
    )
    .unwrap();
    fs::rename(outside.path().join("repo"), base.join("build/x/repo")).unwrap();
    f.watcher.inject(Pending {
        abs: base.join("build/x/repo"),
        created: true,
        removed: false,
        modified: false,
        rename_from: None,
        after_removal: None,
        deferred: false,
    });
    let fence = told_batch(&f, "a.md", "two");
    let seen = f.seen.lock().unwrap();
    assert!(
        fence.rescan || seen.iter().any(|b| b.rescan),
        "the new project makes everything look again"
    );
}

#[test]
fn new_big_files_are_read_only_when_their_content_may_pair_them() {
    // pictures copied in: hashing each would read every byte written on the watch thread
    let fx = Direct::new();
    let len = 1024 * 1024 + 4096;
    fs::create_dir_all(fx.abs("pics")).unwrap();
    let rels: Vec<String> = (0..20).map(|i| format!("pics/p{i}.png")).collect();
    for (i, rel) in rels.iter().enumerate() {
        let mut bytes = vec![b'x'; len];
        bytes[0] = u8::try_from(i).unwrap();
        fs::write(fx.abs(rel), bytes).unwrap();
    }
    let hints = rels
        .iter()
        .map(|rel| Pending {
            created: true,
            ..fx.hint(rel)
        })
        .collect();
    let before = super::sigs::hashed_bytes();
    let changes = fx.pass(&SigStore::default(), &|_| None, hints);
    assert_eq!(changes.len(), rels.len());
    assert_eq!(super::sigs::hashed_bytes() - before, 0, "nothing read");
    // one of them is moved by copy + delete: the removal's size matches, that file is read and the move paired
    let sigs = SigStore::default();
    let meta = fs::symlink_metadata(fx.abs(&rels[3])).unwrap();
    sigs.set(
        crate::tree::key_of(&rels[3]),
        &rels[3],
        sig_of(&fx.abs(&rels[3]), &meta).unwrap(),
    );
    fs::rename(fx.abs(&rels[3]), fx.abs("moved.png")).unwrap();
    let before = super::sigs::hashed_bytes();
    let changes = fx.pass(
        &sigs,
        &|_| None,
        vec![
            Pending {
                removed: true,
                ..fx.hint(&rels[3])
            },
            Pending {
                created: true,
                ..fx.hint("moved.png")
            },
        ],
    );
    assert_eq!(
        changes
            .iter()
            .map(|c| (c.path.as_str(), &c.kind))
            .collect::<Vec<_>>(),
        [(
            "moved.png",
            &ChangeKind::Renamed {
                from: rels[3].clone()
            }
        )]
    );
    assert!(super::sigs::hashed_bytes() - before <= len as u64);
}
