use super::*;
use crate::watch::ChangeKind;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

const QUICK: Timing = Timing {
    quiet: Duration::from_millis(25),
    max_wait: Duration::from_millis(250),
};

struct Fx {
    project: tempfile::TempDir,
    app: tempfile::TempDir,
    notified: Arc<AtomicUsize>,
}

impl Fx {
    fn new() -> Self {
        Self {
            project: tempfile::tempdir().expect("tempdir"),
            app: tempfile::tempdir().expect("tempdir"),
            notified: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.project.path().join(rel);
        fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        fs::write(p, text).expect("write");
    }

    fn app_meta(&self) -> PathBuf {
        self.app.path().join("meta.toml")
    }

    fn project_meta(&self) -> PathBuf {
        self.project.path().join(".trailmark/meta.toml")
    }

    fn service_at(&self, path: PathBuf, watched: bool, timing: Timing) -> MetaService {
        let vault = Arc::new(Vault::open(self.project.path()).expect("vault"));
        let n = Arc::clone(&self.notified);
        MetaService::open(
            vault,
            path,
            watched,
            timing,
            Arc::new(move || {
                n.fetch_add(1, Ordering::SeqCst);
            }),
        )
    }

    /// A service keeping its file in AppData, writing a moment after each change.
    fn service(&self) -> MetaService {
        self.service_at(self.app_meta(), false, QUICK)
    }

    /// A service keeping its file in the project, as after "Initialize".
    fn project_service(&self) -> MetaService {
        self.service_at(self.project_meta(), true, QUICK)
    }

    /// A service that writes only when stopped.
    fn lazy_service(&self, path: PathBuf, watched: bool) -> MetaService {
        let lazy = Timing {
            quiet: Duration::from_mins(1),
            max_wait: Duration::from_mins(1),
        };
        self.service_at(path, watched, lazy)
    }
}

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|x| (*x).to_owned()).collect()
}

/// `(name, from the file, from the app)` of each tag.
fn names(meta: &FileMeta) -> Vec<(&str, bool, bool)> {
    meta.tags
        .iter()
        .map(|t| (t.name.as_str(), t.file, t.app))
        .collect()
}

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for: {what}");
}

fn change(path: &str) -> FsChange {
    FsChange {
        kind: ChangeKind::Modified,
        path: path.to_owned(),
        from: None,
        at_ms: 1,
    }
}

#[test]
fn app_tags_and_descriptions_round_trip_through_the_file_in_both_places() {
    for in_project in [false, true] {
        let fx = Fx::new();
        fx.write("docs/a.md", "# A\n");
        let (svc, path) = if in_project {
            (fx.project_service(), fx.project_meta())
        } else {
            (fx.service(), fx.app_meta())
        };
        assert!(!path.exists(), "nothing is written before the first change");

        let meta = svc
            .set_tags("docs/a.md", &s(&["Spec", "needs review"]))
            .expect("tags");
        assert_eq!(
            names(&meta),
            [("spec", false, true), ("needs-review", false, true)]
        );
        let meta = svc
            .set_description("docs/a.md", "About\nthe thing")
            .expect("description");
        assert_eq!(meta.description, "About the thing");
        assert!(meta.writable);
        svc.stop();

        let text = fs::read_to_string(&path).expect("meta.toml");
        assert!(text.contains("[files.\"docs/a.md\"]"), "{text}");
        assert!(
            text.contains("tags = [\"spec\", \"needs-review\"]"),
            "{text}"
        );
        assert!(text.contains("description = \"About the thing\""), "{text}");

        // A new service reads the same thing back.
        let again = fx.service_at(path, in_project, QUICK);
        let meta = again.file_meta("docs/a.md").expect("meta");
        assert_eq!(meta.description, "About the thing");
        assert_eq!(meta.tags.len(), 2);
        again.stop();
    }
}

#[test]
fn writes_wait_for_the_changes_to_settle_and_then_happen_by_themselves() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    let svc = fx.lazy_service(fx.app_meta(), false);
    svc.set_tags("a.md", &s(&["one"])).expect("tags");
    assert!(!fx.app_meta().exists(), "not written at once");
    svc.stop();
    assert!(fx.app_meta().exists(), "written when stopping");

    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    let svc = fx.service();
    svc.set_tags("a.md", &s(&["one"])).expect("tags");
    svc.set_tags("a.md", &s(&["one", "two"])).expect("tags");
    wait_until("the last change to be written", || {
        fs::read_to_string(fx.app_meta()).is_ok_and(|t| t.contains("\"two\""))
    });
    assert!(fx.notified.load(Ordering::SeqCst) >= 2);
    svc.stop();
}

#[test]
fn constant_changes_cannot_hold_the_write_back_for_ever() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    let svc = fx.service_at(
        fx.app_meta(),
        false,
        Timing {
            quiet: Duration::from_millis(400),
            max_wait: Duration::from_millis(150),
        },
    );
    let began = Instant::now();
    while began.elapsed() < Duration::from_millis(100) {
        svc.set_description("a.md", &format!("{:?}", began.elapsed()))
            .expect("set");
        thread::sleep(Duration::from_millis(5));
    }
    wait_until("a write despite constant changes", || {
        fx.app_meta().exists()
    });
    svc.stop();
}

#[test]
fn front_matter_tags_are_read_only_and_a_tag_in_both_places_shows_once_as_a_file_tag() {
    let fx = Fx::new();
    fx.write("a.md", "---\ntags: [feed, watcher]\n---\n# A\n");
    fx.write("notes.txt", "---\ntags: [ignored]\n---\n");
    let svc = fx.service();
    let meta = svc.file_meta("a.md").expect("meta");
    assert_eq!(
        names(&meta),
        [("feed", true, false), ("watcher", true, false)]
    );
    let meta = svc.set_tags("a.md", &s(&["feed", "spec"])).expect("tags");
    assert_eq!(
        names(&meta),
        [
            ("feed", true, true),
            ("watcher", true, false),
            ("spec", false, true),
        ]
    );
    // Only the Markdown family has front matter.
    assert!(svc.file_meta("notes.txt").expect("meta").tags.is_empty());
    svc.stop();
}

#[test]
fn paths_outside_the_folder_are_refused() {
    let fx = Fx::new();
    let svc = fx.service();
    for bad in ["../outside.md", "/abs.md", "C:/abs.md", "a\0b.md"] {
        assert!(svc.set_tags(bad, &s(&["x"])).is_err(), "{bad}");
        assert!(svc.set_description(bad, "x").is_err(), "{bad}");
        assert!(svc.file_meta(bad).is_err(), "{bad}");
    }
    assert!(matches!(
        svc.set_tags("@wt/none/a.md", &s(&["x"])),
        Err(MetaError::Vault(_))
    ));
    svc.stop();
    assert!(!fx.app_meta().exists());
}

#[test]
fn an_unreadable_file_makes_app_tags_read_only_but_file_tags_still_work() {
    let fx = Fx::new();
    fx.write("a.md", "---\ntags: [feed]\n---\n");
    let broken = "[files.\"a.md\"]\ntags = [\"x\"]\n<<<<<<< HEAD\ntags = [\"y\"]\n=======\n";
    fx.write(".trailmark/meta.toml", broken);
    let svc = fx.project_service();

    let status = svc.status();
    assert_eq!(status.problem.as_ref().and_then(|p| p.line), Some(3));
    let meta = svc.file_meta("a.md").expect("meta");
    assert!(!meta.writable);
    assert_eq!(names(&meta), [("feed", true, false)]);

    let err = svc.set_tags("a.md", &s(&["new"])).expect_err("read-only");
    assert!(matches!(err, MetaError::Unreadable(_)));
    assert_eq!(err.kind(), "metaUnreadable");
    assert!(err.to_string().contains("line 3"), "{err}");
    assert!(svc.set_description("a.md", "d").is_err());
    assert!(svc.clean_up().is_ok());
    let overview = svc.overview();
    assert_eq!(overview.tags.len(), 1, "file tags are still listed");
    assert!(overview.status.problem.is_some());
    svc.stop();
    assert_eq!(
        fs::read_to_string(fx.project_meta()).expect("read"),
        broken,
        "left alone"
    );

    // Fixing the file brings the app's tags back.
    fx.write(".trailmark/meta.toml", "[files.\"a.md\"]\ntags = [\"x\"]\n");
    assert_eq!(svc.on_disk_event(), DiskEvent::Reloaded);
    assert!(svc.status().problem.is_none());
    let meta = svc.file_meta("a.md").expect("meta");
    assert!(meta.writable);
    assert_eq!(meta.tags.len(), 2);
}

#[test]
fn our_own_writes_are_echoes_and_other_edits_are_read_in() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    fx.write("b.md", "# B\n");
    let svc = fx.project_service();
    svc.set_tags("a.md", &s(&["mine"])).expect("tags");
    wait_until("the write", || fx.project_meta().exists());
    wait_until("the write to be recorded", || {
        svc.on_disk_event() == DiskEvent::Echo
    });

    // Someone else (an agent, a teammate's pull) edits the file.
    fx.write(
        ".trailmark/meta.toml",
        "[files.\"a.md\"]\ntags = [\"theirs\"]\n[files.\"b.md\"]\ndescription = \"new\"\n",
    );
    let before = fx.notified.load(Ordering::SeqCst);
    assert_eq!(svc.on_disk_event(), DiskEvent::Reloaded);
    assert!(
        fx.notified.load(Ordering::SeqCst) > before,
        "the UI is told"
    );
    assert_eq!(svc.file_meta("a.md").expect("a").tags[0].name, "theirs");
    assert_eq!(svc.file_meta("b.md").expect("b").description, "new");

    // Deleting it empties the app's tags.
    fs::remove_file(fx.project_meta()).expect("remove");
    assert_eq!(svc.on_disk_event(), DiskEvent::Reloaded);
    assert!(svc.file_meta("a.md").expect("a").tags.is_empty());
    svc.stop();
}

#[test]
fn a_file_in_appdata_is_not_watched() {
    let fx = Fx::new();
    let svc = fx.service();
    assert_eq!(svc.on_disk_event(), DiskEvent::Ignored);
    assert!(
        !svc.on_fs_changes(&[change(PROJECT_META_PATH)]),
        "nothing to hide"
    );
    svc.stop();
}

#[test]
fn the_watcher_hides_our_own_writes_but_not_other_edits() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    let svc = fx.lazy_service(fx.project_meta(), true);
    svc.set_tags("a.md", &s(&["x"])).expect("tags");
    svc.stop(); // writes now
    let own = [change(".trailmark/meta.toml")];
    assert!(svc.on_fs_changes(&own), "an echo of our write");
    assert!(
        !svc.on_fs_changes(&[change("a.md")]),
        "other files are news"
    );

    fx.write(
        ".trailmark/meta.toml",
        "[files.\"a.md\"]\ntags = [\"edited\"]\n",
    );
    assert!(!svc.on_fs_changes(&own), "an edit by someone else is news");
    assert_eq!(svc.file_meta("a.md").expect("a").tags[0].name, "edited");

    let mut changes = vec![change("a.md"), change(".trailmark/meta.toml")];
    MetaService::without_meta_file(&mut changes);
    assert_eq!(changes, [change("a.md")]);
}

#[test]
fn an_edit_that_lands_before_our_delayed_write_is_kept_and_ours_is_added_on_top() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    fx.write("b.md", "# B\n");
    fx.write(
        ".trailmark/meta.toml",
        "[files.\"b.md\"]\ntags = [\"before\"]\n",
    );
    let svc = fx.lazy_service(fx.project_meta(), true);
    svc.set_tags("a.md", &s(&["mine"])).expect("tags");
    // The agent's edit arrives within our delay, and the watcher has not reported it yet.
    fx.write(
        ".trailmark/meta.toml",
        "[files.\"b.md\"]\ntags = [\"agent\"]\n[files.\"c.md\"]\ntags = [\"also\"]\n",
    );
    svc.stop();
    let text = fs::read_to_string(fx.project_meta()).expect("read");
    for kept in ["\"agent\"", "\"also\"", "\"mine\""] {
        assert!(text.contains(kept), "{kept} in {text}");
    }
    assert!(!text.contains("before"), "{text}");
}

#[test]
fn a_file_that_becomes_unreadable_before_our_write_is_left_untouched() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    let svc = fx.lazy_service(fx.project_meta(), true);
    svc.set_tags("a.md", &s(&["mine"])).expect("tags");
    fx.write(".trailmark/meta.toml", "<<<<<<< HEAD\n");
    svc.stop();
    assert_eq!(
        fs::read_to_string(fx.project_meta()).expect("read"),
        "<<<<<<< HEAD\n"
    );
    assert!(svc.status().problem.is_some());
}

#[test]
fn renamed_files_take_their_tags_along() {
    let fx = Fx::new();
    fx.write("old.md", "# A\n");
    let svc = fx.service();
    svc.set_tags("old.md", &s(&["keep"])).expect("tags");
    svc.set_description("old.md", "described")
        .expect("description");
    fs::rename(
        fx.project.path().join("old.md"),
        fx.project.path().join("new.md"),
    )
    .expect("rename");
    svc.on_fs_changes(&[FsChange {
        kind: ChangeKind::Renamed,
        path: "new.md".to_owned(),
        from: Some("old.md".to_owned()),
        at_ms: 1,
    }]);
    let meta = svc.file_meta("new.md").expect("new");
    assert_eq!(
        (meta.tags.len(), meta.description.as_str()),
        (1, "described")
    );
    svc.stop();
    let text = fs::read_to_string(fx.app_meta()).expect("read");
    assert!(
        text.contains("new.md") && !text.contains("old.md"),
        "{text}"
    );
}

#[test]
fn entries_of_missing_files_are_kept_until_clean_up() {
    let fx = Fx::new();
    fx.write("here.md", "# A\n");
    fx.write("gone.md", "# B\n");
    let svc = fx.service();
    svc.set_tags("here.md", &s(&["a"])).expect("tags");
    svc.set_tags("gone.md", &s(&["b"])).expect("tags");
    fs::remove_file(fx.project.path().join("gone.md")).expect("remove");

    let overview = svc.overview();
    assert_eq!(overview.missing, 1);
    let listed: Vec<&str> = overview.tags.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(listed, ["a"], "tags of missing files are not counted");
    svc.stop();
    assert!(fs::read_to_string(fx.app_meta())
        .expect("read")
        .contains("gone.md"));

    // The file comes back (a checkout): its tags are still there.
    let svc = fx.service();
    fx.write("gone.md", "# B\n");
    assert_eq!(svc.overview().missing, 0);
    assert_eq!(svc.file_meta("gone.md").expect("meta").tags.len(), 1);
    fs::remove_file(fx.project.path().join("gone.md")).expect("remove");

    assert_eq!(svc.clean_up().expect("clean up"), 1);
    assert_eq!(svc.overview().missing, 0);
    assert_eq!(svc.clean_up().expect("again"), 0);
    svc.stop();
    let text = fs::read_to_string(fx.app_meta()).expect("read");
    assert!(
        text.contains("here.md") && !text.contains("gone.md"),
        "{text}"
    );
}

#[test]
fn the_overview_counts_tags_from_both_sources_and_finds_files() {
    let fx = Fx::new();
    fx.write("docs/live-feed.md", "---\ntags: [feed]\n---\n# Feed\n");
    fx.write(
        "docs/watcher.md",
        "---\ntags:\n  - feed\n  - watcher\n---\n",
    );
    fx.write("plan.md", "# Plan\n");
    fx.write("data.json", "{}");
    let svc = fx.service();
    svc.set_tags("docs/live-feed.md", &s(&["spec"]))
        .expect("tags");
    svc.set_tags("plan.md", &s(&["spec", "feed"]))
        .expect("tags");
    svc.set_description("data.json", "Sample data")
        .expect("description");

    let overview = svc.overview();
    let tags: Vec<(&str, usize, bool)> = overview
        .tags
        .iter()
        .map(|t| (t.name.as_str(), t.count, t.only_in_files))
        .collect();
    assert_eq!(
        tags,
        [("feed", 3, false), ("spec", 2, false), ("watcher", 1, true)]
    );
    assert_eq!(
        overview.untagged, 1,
        "data.json has a description but no tag"
    );
    assert_eq!(overview.missing, 0);

    let paths = |tags: &[&str], untagged: bool| -> Vec<String> {
        svc.files_with_tags(&s(tags), untagged)
            .into_iter()
            .map(|f| f.path)
            .collect()
    };
    assert_eq!(
        paths(&["feed"], false),
        ["docs/live-feed.md", "docs/watcher.md", "plan.md"]
    );
    assert_eq!(
        paths(&["feed", "spec"], false),
        ["docs/live-feed.md", "plan.md"]
    );
    assert_eq!(paths(&[], true), ["data.json"]);
    let index = svc.index();
    assert!(index
        .iter()
        .any(|e| e.path == "data.json" && e.description == "Sample data"));
    assert_eq!(index.len(), 4);
    svc.stop();
}

#[test]
fn answers_follow_the_files_once_the_watcher_says_they_changed() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    let svc = fx.service();
    assert!(svc.overview().tags.is_empty());
    fx.write("a.md", "---\ntags: [fresh]\n---\n# A\n");
    svc.on_fs_changes(&[change("a.md")]);
    assert_eq!(svc.overview().tags[0].name, "fresh");
    assert_eq!(svc.file_meta("a.md").expect("a").tags[0].name, "fresh");
    svc.stop();
}

#[test]
fn initializing_the_project_moves_appdata_metadata_into_meta_toml() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    let svc = fx.service();
    svc.set_tags("a.md", &s(&["moved"])).expect("tags");
    svc.set_description("a.md", "kept").expect("description");

    svc.migrate_to(fx.project_meta(), true).expect("migrate");
    let text = fs::read_to_string(fx.project_meta()).expect("project file");
    assert!(
        text.contains("\"moved\"") && text.contains("kept"),
        "{text}"
    );
    assert!(!fx.app_meta().exists(), "moved, not copied");
    assert_eq!(svc.on_disk_event(), DiskEvent::Echo);

    // From now on changes go to the project file.
    svc.set_tags("a.md", &s(&["moved", "later"])).expect("tags");
    svc.stop();
    assert!(fs::read_to_string(fx.project_meta())
        .expect("read")
        .contains("later"));
    assert!(!fx.app_meta().exists());
}

#[test]
fn initializing_merges_with_a_meta_toml_a_teammate_committed() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    fx.write(
        ".trailmark/meta.toml",
        "[files.\"a.md\"]\ntags = [\"team\"]\ndescription = \"theirs\"\n",
    );
    let svc = fx.service();
    svc.set_tags("a.md", &s(&["mine"])).expect("tags");
    svc.set_description("a.md", "mine").expect("description");
    svc.set_tags("b.md", &s(&["only-mine"])).expect("tags");

    svc.migrate_to(fx.project_meta(), true).expect("migrate");
    let meta = svc.file_meta("a.md").expect("a");
    assert_eq!(meta.description, "theirs", "the project's description wins");
    assert_eq!(names(&meta), [("team", false, true), ("mine", false, true)]);
    assert_eq!(svc.file_meta("b.md").expect("b").tags.len(), 1);
    svc.stop();
}

#[test]
fn initializing_leaves_a_team_file_byte_for_byte_and_never_creates_an_empty_one() {
    let fx = Fx::new();
    let team = "# hand formatted\n[files.\"a.md\"]\ntags=[\"team\"]\n";
    fx.write(".trailmark/meta.toml", team);
    let svc = fx.service();
    svc.migrate_to(fx.project_meta(), true).expect("migrate");
    svc.stop();
    assert_eq!(fs::read_to_string(fx.project_meta()).expect("read"), team);

    let fx = Fx::new();
    fs::create_dir_all(fx.project.path().join(".trailmark")).expect("mkdir");
    let svc = fx.service();
    svc.migrate_to(fx.project_meta(), true).expect("migrate");
    svc.stop();
    assert!(!fx.project_meta().exists());
}

#[test]
fn initializing_does_not_overwrite_a_meta_toml_it_cannot_read() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    fx.write(".trailmark/meta.toml", "<<<<<<< HEAD\n");
    let svc = fx.service();
    svc.set_tags("a.md", &s(&["mine"])).expect("tags");
    let err = svc
        .migrate_to(fx.project_meta(), true)
        .expect_err("conflict");
    assert!(matches!(err, MetaError::Unreadable(_)));
    assert_eq!(
        fs::read_to_string(fx.project_meta()).expect("read"),
        "<<<<<<< HEAD\n"
    );
    // Still in AppData, still working.
    svc.stop();
    assert!(fs::read_to_string(fx.app_meta())
        .expect("appdata")
        .contains("mine"));
}
