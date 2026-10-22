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
    f.watcher.inject_rescan();
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
                .knows_tree(Box::new(|path| (path == "lib").then_some(true)));
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
    let _ = quiet_for(&seen, 600);
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
