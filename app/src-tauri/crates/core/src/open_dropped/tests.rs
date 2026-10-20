use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::conditions::Distribution;
use crate::events::Recorder;
use crate::plugins::PluginRegistry;
use crate::settings::AppSettings;
use crate::state::AppDirs;
use crate::workspace::{CoreEnv, TabOpen, ViewChange, WindowSummary};

struct Fx {
    _base: tempfile::TempDir,
    dir: tempfile::TempDir,
    registry: WorkspaceRegistry,
}

/// Two work windows on their Welcome, and a folder for files + projects.
fn fx() -> Fx {
    let base = tempfile::tempdir().unwrap();
    let env = CoreEnv::new(
        AppDirs::under(base.path()),
        Arc::new(PluginRegistry::new(Vec::new()).unwrap()),
        Arc::new(Recorder::default()),
        Distribution::Direct,
        AppSettings::default(),
    );
    let registry = WorkspaceRegistry::new(Arc::new(env));
    for window in ["main", "w-1"] {
        registry.add_window(window);
        let _ = registry.show_welcome_view(window);
    }
    Fx {
        _base: base,
        dir: tempfile::tempdir().unwrap(),
        registry,
    }
}

impl Fx {
    /// A file (made) or a folder (made) under the fixture folder, absolute.
    fn file(&self, rel: &str) -> PathBuf {
        let path = self.dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "x").unwrap();
        path
    }

    fn folder(&self, rel: &str) -> PathBuf {
        let path = self.dir.path().join(rel);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn window(&self, window: &str) -> WindowSummary {
        self.registry
            .list_windows()
            .into_iter()
            .find(|w| w.window == window)
            .unwrap()
    }

    /// Runs the loads of what a drop did (as the shell would).
    fn run(&self, dropped: Dropped) -> (Vec<ViewChange>, Option<String>, DropOpened) {
        let mut changes = Vec::new();
        for outcome in dropped.outcomes {
            for (ticket, reference) in outcome.loads {
                let _ = self.registry.load(ticket, &reference);
            }
            changes.extend(outcome.changes);
        }
        (changes, dropped.forward, dropped.answer)
    }

    fn drop_on(
        &self,
        window: &str,
        paths: &[PathBuf],
    ) -> (Vec<ViewChange>, Option<String>, DropOpened) {
        self.run(open_dropped(&self.registry, window, paths, DropScope::Tabs))
    }

    /// A live tab of `reference` in `window`.
    fn open(&self, window: &str, path: &Path) -> TabId {
        let reference = WorkspaceRef::classify(path).unwrap();
        match self.registry.open_tab(window, &reference, None).unwrap() {
            TabOpen::Opening { tab, outcome, .. } => {
                for (ticket, reference) in outcome.loads {
                    self.registry.load(ticket, &reference).unwrap();
                }
                tab
            }
            TabOpen::Existing(_) => panic!("open already"),
        }
    }
}

fn loaded(summary: &WindowSummary) -> Vec<bool> {
    summary.tabs.iter().map(|t| t.loaded).collect()
}

// ---- spec ---------------------------------------------------------------------------------------------------------

#[test]
fn several_paths_on_the_welcome_open_as_tabs_and_only_the_last_gets_a_view() {
    let f = fx();
    let paths = [f.file("a.md"), f.folder("docs"), f.file("b b/ç 文.md")];
    let welcome = f.registry.shown_view("main").unwrap();
    let (changes, forward, answer) = f.drop_on("main", &paths);
    assert_eq!(answer, DropOpened::default());
    assert_eq!(forward.as_deref(), Some("main"));
    let main = f.window("main");
    assert_eq!(main.tabs.len(), 3);
    assert_eq!(
        loaded(&main),
        [false, false, true],
        "Pending before the last"
    );
    assert_eq!(main.active, Some(main.tabs[2].tab));
    // no new view: the Welcome view became the shown tab's
    assert!(!changes
        .iter()
        .any(|c| matches!(c, ViewChange::Create { .. })));
    assert_eq!(f.registry.shown_view("main"), Some(welcome));
    assert!(
        f.window("w-1").tabs.is_empty(),
        "the other window is untouched"
    );
}

#[test]
fn a_drop_on_a_window_with_tabs_adds_them_after_its_tabs() {
    let f = fx();
    let first = f.open("main", &f.file("first.md"));
    let (changes, _, _) = f.drop_on("main", &[f.file("x.md"), f.file("y.md")]);
    let main = f.window("main");
    assert_eq!(main.tabs.len(), 3);
    assert_eq!(main.tabs[0].tab, first);
    assert_eq!(loaded(&main), [true, false, true]);
    assert_eq!(main.active, Some(main.tabs[2].tab));
    assert!(changes
        .iter()
        .any(|c| matches!(c, ViewChange::Create { .. })));
    assert!(
        changes.iter().any(|c| matches!(c, ViewChange::Hide { .. })),
        "the old tab hides"
    );
}

#[test]
fn a_path_open_elsewhere_goes_to_its_place_and_is_not_opened_twice() {
    let f = fx();
    let doc = f.file("a.md");
    let there = f.open("w-1", &doc);
    let (_, forward, answer) = f.drop_on("main", std::slice::from_ref(&doc));
    assert_eq!(answer.failed.len(), 0);
    assert_eq!(forward.as_deref(), Some("w-1"), "its window comes forward");
    assert_eq!(f.window("main").tabs.len(), 0);
    assert_eq!(f.window("w-1").active, Some(there));
}

#[test]
fn a_file_inside_an_open_project_goes_to_the_project_tab() {
    let f = fx();
    let project = f.folder("proj");
    let inside = f.file("proj/notes/a.md");
    let tab = f.open("w-1", &project);
    let (_, forward, _) = f.drop_on("main", &[inside]);
    assert_eq!(forward.as_deref(), Some("w-1"));
    assert_eq!(f.window("w-1").active, Some(tab));
    assert_eq!(f.window("main").tabs.len(), 0);
}

#[test]
fn new_tabs_on_a_welcome_show_one_of_them_when_the_last_path_is_open_elsewhere() {
    let f = fx();
    let open = f.file("open.md");
    f.open("w-1", &open);
    let (_, forward, _) = f.drop_on("main", &[f.file("new.md"), open]);
    assert_eq!(forward.as_deref(), Some("w-1"));
    let main = f.window("main");
    assert_eq!(main.tabs.len(), 1);
    assert_eq!(
        loaded(&main),
        [true],
        "a window with tabs shows one (it adopted the Welcome view)"
    );
}

#[test]
fn the_same_path_twice_in_one_drop_opens_once() {
    let f = fx();
    let doc = f.file("a.md");
    // the same file spelled another way (works on every system: `a.md/.` is an error on Unix)
    let again = f.dir.path().join(".").join("a.md");
    let (_, _, answer) = f.drop_on("main", &[doc.clone(), again, doc]);
    assert_eq!(answer.failed.len(), 0);
    assert_eq!(f.window("main").tabs.len(), 1);
}

#[test]
fn failures_are_listed_and_the_rest_still_opens() {
    let f = fx();
    let gone = f.dir.path().join("gone.md");
    let (_, forward, answer) = f.drop_on(
        "main",
        &[
            gone.clone(),
            PathBuf::from("relative/a.md"),
            f.file("ok.md"),
        ],
    );
    assert_eq!(answer.failed.len(), 2);
    assert_eq!(answer.failed[0].path, gone.display().to_string());
    assert_eq!(answer.failed[0].error.kind, ErrorKind::NotFound);
    assert_eq!(answer.failed[1].error.kind, ErrorKind::InvalidPath);
    assert_eq!(forward.as_deref(), Some("main"));
    assert_eq!(f.window("main").tabs.len(), 1);
}

#[test]
fn nothing_openable_changes_nothing() {
    let f = fx();
    let (changes, forward, answer) = f.drop_on("main", &[f.dir.path().join("nope")]);
    assert_eq!(changes.len(), 0);
    assert_eq!(forward, None);
    assert_eq!(answer.failed.len(), 1);
    assert_eq!(f.window("main").tabs.len(), 0);
}

#[test]
fn existing_only_brings_open_places_forward_and_hands_the_rest_back() {
    let f = fx();
    let open = f.file("open.md");
    let new = f.file("new.md");
    f.open("w-1", &open);
    let dropped = open_dropped(
        &f.registry,
        "main",
        &[new.clone(), open],
        DropScope::ExistingOnly,
    );
    let (_, forward, answer) = f.run(dropped);
    assert_eq!(answer.left, [new.display().to_string()]);
    assert_eq!(forward.as_deref(), Some("w-1"));
    assert!(f.window("main").tabs.is_empty(), "nothing opened here");
}

// ---- adversarial --------------------------------------------------------------------------------------------------

#[test]
fn two_hundred_paths_cost_one_view() {
    let f = fx();
    let paths: Vec<PathBuf> = (0..200)
        .map(|n| f.file(&format!("many/{n} é.md")))
        .collect();
    let (changes, _, answer) = f.drop_on("main", &paths);
    assert_eq!(answer.failed.len(), 0);
    let main = f.window("main");
    assert_eq!(main.tabs.len(), 200);
    assert_eq!(main.tabs.iter().filter(|t| t.loaded).count(), 1);
    assert!(!changes
        .iter()
        .any(|c| matches!(c, ViewChange::Create { .. })));
}

#[test]
fn a_closing_window_takes_no_tabs() {
    let f = fx();
    let _ = f.registry.request_close("main");
    let (_, forward, answer) = f.drop_on("main", &[f.file("a.md")]);
    assert_eq!(forward, None);
    assert_eq!(answer.failed.len(), 1);
    assert_eq!(answer.failed[0].error.kind, ErrorKind::Unavailable);
}

#[test]
fn the_scope_reads_as_the_frontend_writes_it() {
    let tabs: DropScope = serde_json::from_str("\"tabs\"").unwrap();
    let existing: DropScope = serde_json::from_str("\"existingOnly\"").unwrap();
    assert_eq!((tabs, existing), (DropScope::Tabs, DropScope::ExistingOnly));
}
