use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::events::{CoreEvent, EventSink};
use permark_core::paths::Root;
use permark_core::plugins::{PluginId, PluginRegistry, RpcCall};
use permark_core::search::SearchOptions;
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use permark_core::ErrorKind;
use serde_json::{json, Value};

use super::*;

fn marks(pairs: &[(&str, Interest)]) -> Marks {
    let mut m = Marks::default();
    for (path, interest) in pairs {
        m.set(&[(*path).to_owned()], *interest);
    }
    m
}

// ---- resolution ---------------------------------------------------------------------------------------------------

#[test]
fn nothing_is_quiet_until_a_mark_says_so() {
    let m = Marks::default();
    assert!(!m.is_quiet("src/main.rs"));
    assert!(!m.is_quiet(""));
}

#[test]
fn a_folder_mark_covers_what_is_inside_and_a_closer_mark_wins() {
    let m = marks(&[("src", Interest::Quiet), ("src/docs", Interest::Speaking)]);
    assert!(m.is_quiet("src"));
    assert!(m.is_quiet("src/main.rs"));
    assert!(
        m.is_quiet("src/deep/er/file.md"),
        "files that appear later are covered"
    );
    assert!(!m.is_quiet("src/docs"));
    assert!(!m.is_quiet("src/docs/guide.md"));
    assert!(
        !m.is_quiet("srcx/file.md"),
        "a sibling with the same start is not inside"
    );
    assert!(!m.is_quiet("other.md"));
}

#[test]
fn a_mark_on_the_project_covers_everything_but_a_worktree_is_a_root() {
    let m = marks(&[("", Interest::Quiet)]);
    assert!(m.is_quiet("a/b.md"));
    assert!(
        !m.is_quiet("@wt/feature/a.md"),
        "a mark above does not cross into a worktree"
    );
    let wt = marks(&[("@wt/feature", Interest::Quiet)]);
    assert!(wt.is_quiet("@wt/feature/a/b.md"));
    assert!(!wt.is_quiet("a/b.md"));
    assert!(!wt.is_quiet("@wt/other/a.md"));
}

#[test]
fn marks_follow_the_case_of_the_file_system() {
    let m = marks(&[("Docs", Interest::Quiet)]);
    assert_eq!(
        m.is_quiet("docs/a.md"),
        permark_core::paths::CASE_INSENSITIVE
    );
    assert!(m.is_quiet("Docs/a.md"));
}

#[test]
fn a_moved_folder_takes_its_marks_along_and_a_sibling_does_not() {
    let mut m = marks(&[
        ("old", Interest::Quiet),
        ("old/keep", Interest::Speaking),
        ("older", Interest::Quiet),
    ]);
    assert!(m.renamed("old", "new/place"));
    assert_eq!(
        m.iter().collect::<Vec<_>>(),
        [
            ("new/place", Interest::Quiet),
            ("new/place/keep", Interest::Speaking),
            ("older", Interest::Quiet)
        ]
    );
    assert!(!m.renamed("nothing", "x"));
    assert!(!m.renamed("", "x"));
    assert!(!m.renamed("older", "older"));
}

#[test]
fn documents_that_were_quiet_and_now_speak_are_the_ones_to_catch_up() {
    let before = marks(&[("a", Interest::Quiet)]);
    let after = marks(&[("a", Interest::Quiet), ("a/b", Interest::Speaking)]);
    let all = ["a/x.md", "a/b/y.md", "z.md"];
    assert_eq!(waking(all, &before, &after), ["a/b/y.md"]);
    assert!(waking(all, &after, &before).is_empty());
}

// ---- the file of 0.5 -----------------------------------------------------------------------------------------------

#[test]
fn the_0_5_file_reads_and_writes_the_same_layout() {
    let text = "{\r\n  \"docs/é 档\": \"quiet\",\r\n  \"docs/é 档/keep.md\": \"speaking\"\r\n}";
    let m = Marks::parse(text.as_bytes()).unwrap();
    assert!(m.is_quiet("docs/é 档/other.md"));
    assert!(!m.is_quiet("docs/é 档/keep.md"));
    let back = Marks::parse(&m.to_json()).unwrap();
    assert_eq!(back, m);
    let json: Value = serde_json::from_slice(&m.to_json()).unwrap();
    assert_eq!(json["docs/é 档"], "quiet");
}

#[test]
fn what_is_not_a_mark_is_skipped_and_what_is_not_a_map_is_no_marks() {
    let m = Marks::parse(br#"{ "a": "quiet", "b": "loud", "c": 3, "d": null, "e": "speaking" }"#)
        .unwrap();
    assert_eq!(
        m.iter().collect::<Vec<_>>(),
        [("a", Interest::Quiet), ("e", Interest::Speaking)]
    );
    assert!(Marks::parse(b"[1, 2]").is_none());
    assert!(Marks::parse(b"{ not json").is_none());
    assert!(Marks::parse(b"").is_none());
    assert!(Marks::parse("\u{feff}{}".as_bytes()).unwrap().is_empty());
}

// ---- in a project --------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Sink(Mutex<Vec<(String, Value)>>);

impl EventSink for Sink {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, payload } = event {
            self.0.lock().unwrap().push((name, payload));
        }
    }
}

struct Harness {
    base: tempfile::TempDir,
    project: tempfile::TempDir,
    sink: Arc<Sink>,
    env: CoreEnv,
    core: Option<Arc<ProjectCore>>,
}

impl Harness {
    fn new() -> Self {
        let sink = Arc::new(Sink::default());
        let base = tempfile::tempdir().unwrap();
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
            sink.clone(),
            Distribution::Direct,
            AppSettings::default(),
        );
        Self {
            base,
            project: tempfile::tempdir().unwrap(),
            sink,
            env,
            core: None,
        }
    }

    fn root(&self) -> PathBuf {
        dunce::canonicalize(self.project.path()).unwrap()
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn open(&mut self) {
        self.core = Some(ProjectCore::open(
            Root::open(&self.root()).unwrap(),
            &self.env,
        ));
    }

    fn core(&self) -> &Arc<ProjectCore> {
        self.core.as_ref().unwrap()
    }

    fn call(&self, method: &str, args: Value) -> Result<Value, permark_core::IpcError> {
        self.core().rpc().call(
            "quiet",
            method,
            &RpcCall {
                window: "w".to_owned(),
                args,
            },
        )
    }

    fn file(&self) -> PathBuf {
        self.core().store().dir().join(FILE)
    }

    fn marks(&self) -> BTreeMap<String, String> {
        self.call("rules", Value::Null).unwrap()["marks"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
            .collect()
    }
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(15));
    }
}

#[test]
fn silencing_is_saved_in_the_local_state_in_the_layout_of_0_5_and_announced() {
    let mut h = Harness::new();
    h.write("docs/a.md", "# A\n");
    h.open();
    assert!(
        !h.file().exists(),
        "nothing is written until a mark is made"
    );
    let answer = h
        .call("set", json!({ "paths": ["docs"], "interest": "quiet" }))
        .unwrap();
    assert_eq!(answer, json!({ "marks": { "docs": "quiet" } }));
    let saved: Value = serde_json::from_slice(&std::fs::read(h.file()).unwrap()).unwrap();
    assert_eq!(saved, json!({ "docs": "quiet" }));
    assert!(h
        .sink
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|(n, p)| n == "quiet:changed" && p["data"]["marks"]["docs"] == "quiet"));
    // nothing was written into the project itself
    assert!(std::fs::read_dir(h.root()).unwrap().all(|e| {
        let name = e.unwrap().file_name();
        name == "docs"
    }));
}

#[test]
fn the_marks_of_the_person_are_read_when_the_project_opens() {
    let mut h = Harness::new();
    h.write("a.md", "# A\n");
    // a file as 0.5 wrote it, in the profile of this project
    let store = permark_core::state::StateStore::for_project(
        &AppDirs::under(h.base.path()),
        &Root::open(&h.root()).unwrap(),
    );
    store
        .raw_file(FILE)
        .write(b"{\n  \"archive\": \"quiet\",\n  \"archive/now\": \"speaking\"\n}")
        .unwrap();
    h.open();
    assert_eq!(
        h.marks(),
        BTreeMap::from([
            ("archive".to_owned(), "quiet".to_owned()),
            ("archive/now".to_owned(), "speaking".to_owned())
        ])
    );
}

#[test]
fn listening_again_drops_a_mark_that_changes_nothing_and_keeps_an_override() {
    let mut h = Harness::new();
    h.write("src/docs/a.md", "# A\n");
    h.open();
    h.call("set", json!({ "paths": ["src"], "interest": "quiet" }))
        .unwrap();
    // under a silenced folder a path can speak: the mark is needed
    h.call(
        "set",
        json!({ "paths": ["src/docs"], "interest": "speaking" }),
    )
    .unwrap();
    assert_eq!(h.marks().len(), 2);
    // the folder listens again: the override has nothing to override and goes with it
    h.call("set", json!({ "paths": ["src"], "interest": "speaking" }))
        .unwrap();
    assert!(h.marks().is_empty(), "{:?}", h.marks());
    let saved: Value = serde_json::from_slice(&std::fs::read(h.file()).unwrap()).unwrap();
    assert_eq!(saved, json!({}));
}

#[test]
fn a_move_by_the_app_carries_the_mark() {
    let mut h = Harness::new();
    h.write("old/a.md", "alpha content\n");
    h.open();
    h.call("set", json!({ "paths": ["old"], "interest": "quiet" }))
        .unwrap();
    let ops = h.core().fileops();
    let plan = ops
        .plan_move(
            &[permark_core::fileops::MoveSpec {
                from: "old".to_owned(),
                to: "new".to_owned(),
            }],
            false,
        )
        .unwrap();
    ops.apply_move(plan.plan_id, false).unwrap();
    wait_until("the mark moved", || h.marks().contains_key("new"));
    assert!(!h.marks().contains_key("old"));
}

#[test]
fn search_leaves_silenced_documents_out_unless_asked() {
    let mut h = Harness::new();
    h.write("loud/a.md", "# A\n\nuniqueword here\n");
    h.write("quiet/b.md", "# B\n\nuniqueword there\n");
    h.open();
    assert!(h.core().search().wait_ready(Duration::from_secs(20)));
    let hits = |include: bool| -> Vec<String> {
        let filter = h
            .core()
            .search_filter(&SearchOptions {
                include_silenced: include,
                tags: Vec::new(),
                hidden_last: false,
                project: None,
            })
            .unwrap();
        let mut paths: Vec<String> = h
            .core()
            .search()
            .sections("uniqueword", 10, &filter)
            .hits
            .into_iter()
            .map(|hit| hit.path)
            .collect();
        paths.sort();
        paths
    };
    assert_eq!(hits(false), ["loud/a.md", "quiet/b.md"]);
    h.call("set", json!({ "paths": ["quiet"], "interest": "quiet" }))
        .unwrap();
    assert_eq!(hits(false), ["loud/a.md"]);
    assert_eq!(
        hits(true),
        ["loud/a.md", "quiet/b.md"],
        "asked for: they answer"
    );
    // files that appear later in the silenced folder are covered
    h.write("quiet/later.md", "# L\n\nuniqueword later\n");
    wait_until("indexed", || hits(true).len() == 3);
    assert_eq!(hits(false), ["loud/a.md"]);
}

#[test]
fn hostile_input_is_refused_and_a_damaged_file_is_put_aside_not_overwritten() {
    let mut h = Harness::new();
    h.write("a.md", "# A\n");
    let store = permark_core::state::StateStore::for_project(
        &AppDirs::under(h.base.path()),
        &Root::open(&h.root()).unwrap(),
    );
    store.raw_file(FILE).write(b"{ this is not json").unwrap();
    h.open();
    assert!(h.marks().is_empty());
    for (paths, interest, kind) in [
        (json!(["../outside"]), "quiet", ErrorKind::PathEscape),
        (json!(["a.md"]), "loud", ErrorKind::InvalidData),
        (json!(["/abs"]), "quiet", ErrorKind::InvalidPath),
    ] {
        let error = h
            .call("set", json!({ "paths": paths, "interest": interest }))
            .unwrap_err();
        assert_eq!(error.kind, kind);
    }
    assert_eq!(
        std::fs::read(h.file()).unwrap(),
        b"{ this is not json",
        "refused calls write nothing"
    );
    h.call("set", json!({ "paths": ["a.md"], "interest": "quiet" }))
        .unwrap();
    let aside: Vec<_> = std::fs::read_dir(h.file().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("quiet.unreadable-"))
        .collect();
    assert_eq!(
        aside.len(),
        1,
        "the unreadable file is kept next to the new one"
    );
}

#[test]
fn off_leaves_nothing_and_the_marks_wait_for_the_next_time() {
    let mut h = Harness::new();
    h.write("a.md", "# A\n");
    h.open();
    h.call("set", json!({ "paths": ["a.md"], "interest": "quiet" }))
        .unwrap();
    let core = h.core().clone();
    h.env.set_settings(AppSettings {
        plugins: BTreeMap::from([(PluginId::of("quiet"), false)]),
        ..AppSettings::default()
    });
    core.apply_plugins(&h.env);
    assert_eq!(core.rpc().served(), Vec::<String>::new());
    assert!(h.file().exists(), "the person's choices stay");
    let filter = core.search_filter(&SearchOptions::default()).unwrap();
    assert!(filter.muted.is_none(), "no plugin, nothing is silenced");
    h.env.set_settings(AppSettings::default());
    core.apply_plugins(&h.env);
    assert_eq!(h.marks().len(), 1);
    drop(core);
}
