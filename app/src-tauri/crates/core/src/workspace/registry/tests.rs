use super::*;
use crate::conditions::Distribution;
use crate::events::Recorder;
use crate::paths::Root;
use crate::plugins::fake::{Fake, Log};
use crate::plugins::{Plugin, PluginRegistry, PluginScope};
use crate::settings::AppSettings;
use crate::state::AppDirs;

struct Fx {
    _base: tempfile::TempDir,
    registry: Arc<WorkspaceRegistry>,
    sink: Arc<Recorder>,
    log: Log,
}

fn fx_with(plugins: Vec<Arc<dyn Plugin>>, log: Log) -> Fx {
    let base = tempfile::tempdir().unwrap();
    let sink = Arc::new(Recorder::default());
    let env = CoreEnv::new(
        AppDirs::under(base.path()),
        Arc::new(PluginRegistry::new(plugins).unwrap()),
        sink.clone(),
        Distribution::Direct,
        AppSettings::default(),
    );
    let registry = Arc::new(WorkspaceRegistry::new(Arc::new(env)));
    for window in ["main", "w-1"] {
        registry.add_window(window);
        let _ = registry.show_welcome_view(window);
    }
    Fx {
        _base: base,
        registry,
        sink,
        log,
    }
}

fn fx() -> Fx {
    let log = Log::default();
    let plugins: Vec<Arc<dyn Plugin>> = vec![
        Arc::new(Fake::new("git", PluginScope::Project, &[]).with_log(&log)),
        Arc::new(Fake::new("unread", PluginScope::Project, &["git"]).with_log(&log)),
    ];
    fx_with(plugins, log)
}

fn project(dir: &Path) -> WorkspaceRef {
    WorkspaceRef::Project(Root::open(dir).unwrap())
}

fn file(path: &Path) -> WorkspaceRef {
    std::fs::write(path, "x").unwrap();
    WorkspaceRef::classify(path).unwrap()
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.lock().unwrap())
}

fn starts(log: &Log) -> usize {
    take(log).iter().filter(|e| *e == "start:git").count()
}

/// The whole open of a new tab; `Err(place)` when the workspace was open already.
fn tab(f: &Fx, window: &str, reference: &WorkspaceRef) -> Result<(TabId, String), Place> {
    match f.registry.open_tab(window, reference, None).unwrap() {
        TabOpen::Opening { tab, view, outcome } => {
            for (ticket, reference) in outcome.loads {
                f.registry.load(ticket, &reference).unwrap();
            }
            Ok((tab, view))
        }
        TabOpen::Existing(place) => Err(place),
    }
}

/// Runs the loads an outcome started.
fn run(f: &Fx, outcome: Outcome) -> Vec<ViewChange> {
    for (ticket, reference) in outcome.loads {
        let _ = f.registry.load(ticket, &reference);
    }
    outcome.changes
}

fn core_of(f: &Fx, view: &str) -> Arc<ProjectCore> {
    match f.registry.workspace_of(view).unwrap().1 {
        Workspace::Project(core) => core,
        Workspace::SingleFile(_) | Workspace::Notes(_) => panic!("not a project"),
    }
}

fn window_of(f: &Fx, window: &str) -> WindowSummary {
    f.registry
        .list_windows()
        .into_iter()
        .find(|w| w.window == window)
        .unwrap()
}

// ---- one place per workspace ------------------------------------------------------------------------------------

#[test]
fn a_workspace_lives_in_one_tab_and_its_core_goes_with_it() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (tab_a, view) = tab(&f, "main", &project(dir.path())).unwrap();
    assert_eq!(take(&f.log), ["start:git", "start:unread"]);
    // the same folder, spelled another way, from another window -> where it is
    let again = tab(&f, "w-1", &project(&dir.path().join("."))).unwrap_err();
    assert_eq!(
        again,
        Place {
            window: "main".into(),
            tab: tab_a
        }
    );
    assert_eq!(f.registry.find(&project(dir.path())), Some(again));
    let weak = Arc::downgrade(&core_of(&f, &view));
    let changes = f.registry.close_tab(tab_a).changes;
    assert_eq!(
        changes,
        [ViewChange::Show {
            window: "main".into(),
            view: view.clone()
        }],
        "the last tab's view stays, on the Welcome"
    );
    assert_eq!(take(&f.log), ["stop:unread", "stop:git"]);
    assert!(weak.upgrade().is_none(), "core dropped with its tab");
    assert_eq!(f.registry.window_view(&view), Some(WindowView::Welcome));
    assert_eq!(lock(&f.registry.cores).len(), 0, "dead slots pruned");
}

#[test]
fn the_first_tab_adopts_the_welcome_view_and_the_next_ones_get_their_own() {
    let f = fx();
    let welcome = f.registry.shown_view("main").unwrap();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let opened = f
        .registry
        .open_tab("main", &project(a.path()), None)
        .unwrap();
    let TabOpen::Opening { view, outcome, .. } = opened else {
        panic!()
    };
    assert_eq!(view, welcome);
    assert_eq!(
        run(&f, outcome),
        [ViewChange::Show {
            window: "main".into(),
            view: welcome.clone()
        }]
    );
    let TabOpen::Opening {
        view: second,
        outcome,
        ..
    } = f
        .registry
        .open_tab("main", &project(b.path()), None)
        .unwrap()
    else {
        panic!()
    };
    assert_ne!(second, welcome);
    assert_eq!(
        run(&f, outcome),
        [
            ViewChange::Create {
                window: "main".into(),
                view: second.clone()
            },
            ViewChange::Show {
                window: "main".into(),
                view: second.clone()
            },
            ViewChange::Hide { view: welcome }
        ]
    );
    assert_eq!(f.registry.shown_view("main"), Some(second));
}

/// Adversarial: two windows + an outside open start one root at the same moment -> one ticket, one tab, one core.
#[test]
fn concurrent_opens_of_one_root_make_one_tab_and_one_core() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    for n in 0..8 {
        f.registry.add_window(&format!("w-{}", n + 2));
    }
    let results: Vec<Result<(TabId, String), Place>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let f = &f;
                let reference = project(dir.path());
                s.spawn(move || tab(f, &format!("w-{}", n + 2), &reference))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let place = results.iter().find_map(|r| r.as_ref().ok()).unwrap().0;
    assert!(results
        .iter()
        .filter_map(|r| r.as_ref().err())
        .all(|p| p.tab == place));
    assert_eq!(starts(&f.log), 1);
    assert_eq!(f.registry.cores().len(), 1);
    let tabs: usize = f.registry.list_windows().iter().map(|w| w.tabs.len()).sum();
    assert_eq!(tabs, 1);
}

/// Adversarial: a file inside a project that is still loading counts as inside it.
#[test]
fn a_file_inside_a_project_being_opened_finds_that_tab() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    let TabOpen::Opening {
        tab: id, outcome, ..
    } = f
        .registry
        .open_tab("main", &project(dir.path()), None)
        .unwrap()
    else {
        panic!()
    };
    // not finished yet
    let (place, rel) = f
        .registry
        .find_containing(&dir.path().join("docs").join("a.md"))
        .unwrap();
    assert_eq!((place.tab, rel.as_str()), (id, "docs/a.md"));
    run(&f, outcome);
}

#[test]
fn find_containing_prefers_the_deepest_project_live_or_pending() {
    let f = fx();
    let outer = tempfile::tempdir().unwrap();
    let inner = outer.path().join("sub project");
    std::fs::create_dir_all(inner.join("docs")).unwrap();
    tab(&f, "main", &project(outer.path())).unwrap();
    let pending = f
        .registry
        .restore_tab("w-1", &project(&inner), None)
        .unwrap();
    let (place, rel) = f
        .registry
        .find_containing(&inner.join("docs").join("a.md"))
        .unwrap();
    assert_eq!((place.tab, rel.as_str()), (pending, "docs/a.md"));
    assert!(f
        .registry
        .find_containing(tempfile::tempdir().unwrap().path())
        .is_none());
}

// ---- opening in a view ------------------------------------------------------------------------------------------

#[test]
fn a_superseded_open_is_dropped_and_its_core_torn_down() {
    let f = fx();
    let (zero, one, two) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let (_, view) = tab(&f, "main", &project(zero.path())).unwrap();
    take(&f.log);
    let Begin::Ticket(first) = f
        .registry
        .begin_open(&view, &project(one.path()), None)
        .unwrap()
    else {
        panic!()
    };
    let Begin::Ticket(second) = f
        .registry
        .begin_open(&view, &project(two.path()), None)
        .unwrap()
    else {
        panic!()
    };
    let late = f.registry.prepare(&project(one.path()));
    let won = f.registry.prepare(&project(two.path()));
    assert!(f.registry.finish_open(second, won).is_ok());
    take(&f.log);
    assert!(matches!(
        f.registry.finish_open(first, late),
        Err(OpenError::Superseded)
    ));
    assert_eq!(
        take(&f.log),
        ["stop:unread", "stop:git"],
        "stale core dropped"
    );
    assert_eq!(f.registry.cores().len(), 1);
    assert_eq!(core_of(&f, &view).root(), &Root::open(two.path()).unwrap());
}

#[test]
fn a_failed_open_keeps_what_was_shown() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (_, view) = tab(&f, "main", &project(dir.path())).unwrap();
    let id = f.registry.workspace_of(&view).unwrap().0;
    let result = f.registry.attach(&view, &WorkspaceRef::Notes, None);
    assert!(matches!(
        result,
        Err(OpenError::Workspace(WorkspaceError::NotesUnavailable))
    ));
    assert_eq!(f.registry.workspace_of(&view).unwrap().0, id);
    let ipc: IpcError = OpenError::Workspace(WorkspaceError::NotesUnavailable).into();
    assert_eq!(ipc.kind, ErrorKind::Unsupported);
}

#[test]
fn this_tab_placement_releases_the_old_workspace() {
    let f = fx();
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (id, view) = tab(&f, "main", &project(one.path())).unwrap();
    f.registry
        .attach(&view, &project(two.path()), None)
        .unwrap();
    assert_eq!(f.registry.cores().len(), 1);
    let named = file(&two.path().join("ñ 文.md"));
    f.registry.attach(&view, &named, None).unwrap();
    assert_eq!(f.registry.cores().len(), 0);
    let tabs = window_of(&f, "main").tabs;
    assert_eq!(
        (
            tabs.len(),
            tabs[0].tab,
            tabs[0].kind,
            tabs[0].title.as_str()
        ),
        (1, id, WorkspaceKind::SingleFile, "ñ 文.md")
    );
    // opening what this tab shows already: it is where it is
    assert!(matches!(
        f.registry.attach(&view, &named, None),
        Ok(Attached::Existing(_))
    ));
    // a Welcome view is no tab: opening in it is refused (the shell adds a tab instead)
    let welcome = f.registry.shown_view("w-1").unwrap();
    let other = tempfile::tempdir().unwrap();
    assert!(matches!(
        f.registry
            .begin_open(&welcome, &project(other.path()), None),
        Err(OpenError::UnknownView(_))
    ));
}

/// Adversarial: the tab closes while its workspace loads -> the result is stale, the core dropped, no tab returns.
#[test]
fn close_during_load_drops_the_result() {
    let f = fx();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    tab(&f, "main", &project(a.path())).unwrap();
    take(&f.log);
    let TabOpen::Opening {
        tab: id, outcome, ..
    } = f
        .registry
        .open_tab("main", &project(b.path()), None)
        .unwrap()
    else {
        panic!()
    };
    let (ticket, reference) = outcome.loads.into_iter().next().unwrap();
    let prepared = f.registry.prepare(&reference);
    let _ = f.registry.close_tab(id);
    assert!(matches!(
        f.registry.finish_open(ticket, prepared),
        Err(OpenError::Superseded)
    ));
    assert_eq!(
        take(&f.log),
        ["start:git", "start:unread", "stop:unread", "stop:git"]
    );
    assert_eq!(f.registry.cores().len(), 1);
    assert_eq!(window_of(&f, "main").tabs.len(), 1);
    assert!(f.registry.find(&project(b.path())).is_none());
}

/// Adversarial: the window asked to close while a tab loads -> no prompt for it (nothing unsaved yet).
#[test]
fn request_close_during_load_closes_without_a_prompt() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let TabOpen::Opening { outcome, .. } = f
        .registry
        .open_tab("main", &project(dir.path()), None)
        .unwrap()
    else {
        panic!()
    };
    let changes = f.registry.request_close("main").changes;
    assert_eq!(
        changes,
        [ViewChange::CloseWindow {
            window: "main".into()
        }]
    );
    // until it is gone, the closing window is saved as it was (the last window closed stays in the session)
    let session = f.registry.session_windows();
    assert_eq!(session[0].window, "main");
    assert_eq!(session[0].tabs.len(), 1);
    run(&f, outcome);
    f.registry.forget_window("main");
    assert_eq!(f.registry.cores().len(), 0);
}

#[test]
fn events_only_when_something_differs() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    f.sink.take();
    let (_, view) = tab(&f, "main", &project(dir.path())).unwrap();
    let names: Vec<String> = f.sink.take().iter().map(|e| e.name().to_owned()).collect();
    assert!(names.contains(&"windows-changed".to_owned()));
    assert!(names.contains(&"context-changed".to_owned()));
    assert!(names.contains(&"window-changed".to_owned()));
    f.registry.refresh();
    // the search index reports its own progress whenever it likes; everything else stays quiet
    let rest: Vec<String> = f
        .sink
        .take()
        .iter()
        .map(|e| e.name().to_owned())
        .filter(|n| n != "index-progress")
        .collect();
    assert_eq!(rest, Vec::<String>::new());
    let context = f.registry.context_of(&view).unwrap();
    assert_eq!(context.workspace, WorkspaceKind::Project);
    assert_eq!(context.permark, PermarkCondition::NotInitialized);
    assert!(context
        .plugins
        .contains(&crate::plugins::PluginId::of("unread")));
    let welcome = f.registry.shown_view("w-1").unwrap();
    assert_eq!(
        f.registry.context_of(&welcome).unwrap().workspace,
        WorkspaceKind::Welcome
    );
}

#[test]
fn document_hint_shows_in_the_view_and_the_session_lists_tabs() {
    let f = fx();
    let (dir, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let doc = VaultPath::parse("untitled.md").unwrap();
    let TabOpen::Opening { view, outcome, .. } = f
        .registry
        .open_tab("main", &project(dir.path()), Some(doc))
        .unwrap()
    else {
        panic!()
    };
    run(&f, outcome);
    let Some(WindowView::Showing { document, .. }) = f.registry.window_view(&view) else {
        panic!("not showing");
    };
    assert_eq!(document.as_deref(), Some("untitled.md"));
    f.registry
        .restore_tab("main", &project(other.path()), None)
        .unwrap();
    let session = f.registry.session_windows();
    assert_eq!(session.len(), 2);
    assert_eq!(session[0].window, "main");
    assert_eq!(session[0].tabs.len(), 2, "pending tabs saved as their ref");
    assert_eq!(session[0].active, Some(0));
    assert_eq!(session[1].tabs, []);
    assert_eq!(session[1].active, None);
}

#[test]
fn rapid_open_storm_in_one_tab_ends_on_the_last() {
    let f = fx();
    let first = tempfile::tempdir().unwrap();
    let (_, view) = tab(&f, "main", &project(first.path())).unwrap();
    let dirs: Vec<tempfile::TempDir> = (0..5).map(|_| tempfile::tempdir().unwrap()).collect();
    let tickets: Vec<(Ticket, WorkspaceRef)> = (0..50)
        .filter_map(|n| {
            let reference = project(dirs[n % 5].path());
            match f.registry.begin_open(&view, &reference, None).unwrap() {
                Begin::Ticket(ticket) => Some((ticket, reference)),
                // the same folder is being opened here already: one place
                Begin::Existing(_) => None,
            }
        })
        .collect();
    let mut ok = 0;
    for (ticket, reference) in tickets.into_iter().rev() {
        if f.registry.load(ticket, &reference).is_ok() {
            ok += 1;
        }
    }
    assert_eq!(ok, 1);
    assert_eq!(f.registry.cores().len(), 1);
}

// ---- the Notes (stage A: a tab) ---------------------------------------------------------------------------------

#[test]
fn the_notes_open_only_while_the_notes_plugin_is_on_and_in_one_place() {
    let off = fx_with(Vec::new(), Log::default());
    let TabOpen::Opening {
        tab: id, outcome, ..
    } = off
        .registry
        .open_tab("main", &WorkspaceRef::Notes, None)
        .unwrap()
    else {
        panic!()
    };
    let (ticket, reference) = outcome.loads.into_iter().next().unwrap();
    let error = off.registry.load(ticket, &reference).unwrap_err();
    assert!(matches!(
        error,
        OpenError::Workspace(WorkspaceError::NotesUnavailable)
    ));
    assert_eq!(IpcError::from(error).kind, ErrorKind::Unsupported);
    let _ = off.registry.close_tab(id);

    let log = Log::default();
    let on = fx_with(
        vec![Arc::new(
            Fake::new("notes", PluginScope::App, &[]).with_log(&log),
        )],
        log,
    );
    on.registry.env().apply_app_plugins();
    let (_, view) = tab(&on, "main", &WorkspaceRef::Notes).unwrap();
    assert!(matches!(
        on.registry.workspace_of(&view).unwrap().1,
        Workspace::Notes(_)
    ));
    assert!(tab(&on, "w-1", &WorkspaceRef::Notes).is_err(), "one place");
    assert!(on.registry.notes_open());
    assert_eq!(window_of(&on, "main").tabs[0].title, "Notes");
    assert_eq!(
        on.registry.context_of(&view).unwrap().workspace,
        WorkspaceKind::Notes
    );
    // the session leaves the Notes tab out (it keeps `notes.open`)
    assert_eq!(on.registry.session_windows()[0].tabs, []);
}

// ---- lazy restore + tab switching -------------------------------------------------------------------------------

/// Pending tabs build no core; only the activated one loads.
#[test]
fn restore_loads_only_the_active_tab() {
    let f = fx();
    let dirs: Vec<tempfile::TempDir> = (0..4).map(|_| tempfile::tempdir().unwrap()).collect();
    f.registry.add_window("w-2");
    let tabs: Vec<TabId> = dirs
        .iter()
        .map(|d| {
            f.registry
                .restore_tab("w-2", &project(d.path()), None)
                .unwrap()
        })
        .collect();
    // a duplicate in another window is dropped
    assert!(f
        .registry
        .restore_tab("main", &project(dirs[0].path()), None)
        .is_none());
    assert_eq!(starts(&f.log), 0, "no core for pending tabs");
    let changes = run(&f, f.registry.activate(tabs[2]));
    assert_eq!(starts(&f.log), 1);
    assert!(matches!(&changes[0], ViewChange::Create { window, .. } if window == "w-2"));
    let summary = window_of(&f, "w-2");
    assert_eq!(summary.active, Some(tabs[2]));
    assert_eq!(
        summary.tabs.iter().map(|t| t.loaded).collect::<Vec<_>>(),
        [false, false, true, false]
    );
    assert!(summary.tabs.iter().all(|t| !t.unsaved));
    assert_eq!(
        summary.tabs[0].title,
        Root::open(dirs[0].path()).unwrap().name()
    );
}

/// Adversarial: 500 activations across 20 tabs (half pending) -> the last one shown, one view per visited tab, no
/// view of a closed tab, no leaked core.
#[test]
fn rapid_tab_switching_keeps_one_view_per_visited_tab() {
    let f = fx();
    let dirs: Vec<tempfile::TempDir> = (0..20).map(|_| tempfile::tempdir().unwrap()).collect();
    let mut tabs = Vec::new();
    for (n, dir) in dirs.iter().enumerate() {
        if n % 2 == 0 {
            tabs.push(tab(&f, "main", &project(dir.path())).unwrap().0);
        } else {
            tabs.push(
                f.registry
                    .restore_tab("main", &project(dir.path()), None)
                    .unwrap(),
            );
        }
    }
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut visited: std::collections::BTreeSet<TabId> = tabs.iter().step_by(2).copied().collect();
    let mut last = tabs[0];
    for _ in 0..500 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        last = tabs[usize::try_from(seed % 20).unwrap()];
        visited.insert(last);
        run(&f, f.registry.activate(last));
    }
    let summary = window_of(&f, "main");
    assert_eq!(summary.active, Some(last));
    let shown = f.registry.shown_view("main").unwrap();
    assert_eq!(f.registry.view_of_tab(last), Some(shown));
    let live: Vec<TabId> = summary
        .tabs
        .iter()
        .filter(|t| t.loaded)
        .map(|t| t.tab)
        .collect();
    assert_eq!(live.len(), visited.len());
    assert!(live.iter().all(|t| visited.contains(t)));
    // views: one per live tab (the Welcome view was adopted by the first tab)
    let main_views = f
        .registry
        .views()
        .into_iter()
        .filter(|v| f.registry.window_of(v).as_deref() == Some("main"))
        .count();
    assert_eq!(main_views, visited.len());
    assert_eq!(f.registry.cores().len(), visited.len());
    for t in tabs {
        let _ = f.registry.close_tab(t);
    }
    assert_eq!(f.registry.cores().len(), 0, "no leaked core");
    let main_views = f
        .registry
        .views()
        .into_iter()
        .filter(|v| f.registry.window_of(v).as_deref() == Some("main"))
        .count();
    assert_eq!(main_views, 1, "the Welcome view only");
    assert_eq!(window_of(&f, "main").tabs.len(), 0);
}

#[test]
fn focusing_a_pending_tab_with_a_document_opens_it_there() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let pending = f
        .registry
        .restore_tab("main", &project(dir.path()), None)
        .unwrap();
    let place = Place {
        window: "main".into(),
        tab: pending,
    };
    run(
        &f,
        f.registry
            .activate_with(&place, Some(VaultPath::parse("a.md").unwrap())),
    );
    let view = f.registry.view_of_tab(pending).unwrap();
    assert!(matches!(
        f.registry.window_view(&view),
        Some(WindowView::Showing { document: Some(d), .. }) if d == "a.md"
    ));
}

/// The last `window-changed` of a view.
fn last_said(events: &[CoreEvent], view: &str) -> Option<WindowChanged> {
    events.iter().rev().find_map(|e| match e {
        CoreEvent::WindowChanged(changed) if changed.window == view => Some(changed.clone()),
        _ => None,
    })
}

/// Regression: the Welcome view adopted by the first tab must learn its tab (else "this tab" opens skip the unsaved
/// prompt there), and lose it again when the last tab closes.
#[test]
fn a_view_hears_which_tab_it_belongs_to() {
    let f = fx();
    let welcome = f.registry.shown_view("main").unwrap();
    let dir = tempfile::tempdir().unwrap();
    f.sink.take();
    let (tab_a, view) = tab(&f, "main", &project(dir.path())).unwrap();
    assert_eq!(view, welcome, "adopted");
    assert_eq!(last_said(&f.sink.take(), &view).unwrap().tab, Some(tab_a));
    let _ = f.registry.close_tab(tab_a);
    assert_eq!(last_said(&f.sink.take(), &view).unwrap().tab, None);
}

/// Regression: while the close prompt is up, an outside open of a tab of that window must not hide the prompt (the
/// window could not close any more); the window comes forward as it is, no document switched under the prompt.
#[test]
fn focusing_a_tab_while_the_window_asks_to_close_keeps_the_prompt_on_screen() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (other, other_view) = tab(&f, "main", &project(dir.path())).unwrap();
    let (_, shown_view) = tab(&f, "main", &file(&dir.path().join("b.md"))).unwrap();
    let (_, workspace) = f.registry.workspace_of(&shown_view).unwrap();
    workspace
        .documents()
        .set_unsaved(&shown_view, Some("b.md".into()));
    let _ = f.registry.request_close("main");
    assert!(f.registry.is_confirming_close("main"));
    let place = Place {
        window: "main".into(),
        tab: other,
    };
    let outcome = f
        .registry
        .activate_with(&place, Some(VaultPath::parse("a.md").unwrap()));
    assert!(outcome.refused);
    assert_eq!(outcome.changes, Vec::<ViewChange>::new());
    assert_eq!(f.registry.shown_view("main"), Some(shown_view));
    assert!(matches!(
        f.registry.window_view(&other_view),
        Some(WindowView::Showing { document: None, .. })
    ));
    assert!(f.registry.activate(other).refused);
}

// ---- close prompts ----------------------------------------------------------------------------------------------

/// Adversarial: unsaved edits in a hidden tab -> the prompt lists it; Stay shows it; Close closes the window.
#[test]
fn unsaved_in_a_hidden_tab_is_listed_when_the_window_closes() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (hidden, hidden_view) = tab(&f, "main", &file(&dir.path().join("a.md"))).unwrap();
    let (shown, shown_view) = tab(&f, "main", &file(&dir.path().join("b.md"))).unwrap();
    let (_, workspace) = f.registry.workspace_of(&hidden_view).unwrap();
    workspace
        .documents()
        .set_unsaved(&hidden_view, Some("a.md".into()));
    f.registry.refresh();
    assert!(window_of(&f, "main").tabs[0].unsaved, "dot on its chip");
    let changes = f.registry.request_close("main").changes;
    let [ViewChange::AskClose { view, tabs, .. }] = changes.as_slice() else {
        panic!("{changes:?}")
    };
    assert_eq!(*view, shown_view, "asked in the view on screen");
    assert_eq!(tabs.len(), 1);
    assert_eq!((tabs[0].tab, tabs[0].document.as_str()), (hidden, "a.md"));
    assert!(f.registry.is_confirming_close("main"));
    // meanwhile: outside opens go elsewhere, adds are refused here
    assert_eq!(f.registry.last_focused_work().as_deref(), Some("w-1"));
    let other = tempfile::tempdir().unwrap();
    assert!(matches!(
        f.registry.open_tab("main", &project(other.path()), None),
        Err(OpenError::Closing)
    ));
    // the prompt still lists the same thing on a second request (one prompt)
    assert_eq!(f.registry.request_close("main").changes.len(), 0);
    // a save failed there: the window stays, that tab comes forward
    let changes = f
        .registry
        .resolve_close("main", Resolution::Stay { show: Some(hidden) })
        .changes;
    assert_eq!(
        changes,
        [
            ViewChange::Show {
                window: "main".into(),
                view: hidden_view.clone()
            },
            ViewChange::Hide { view: shown_view }
        ]
    );
    assert!(!f.registry.is_confirming_close("main"));
    let _ = shown;
    let _ = f.registry.request_close("main");
    let changes = f.registry.resolve_close("main", Resolution::Close).changes;
    assert_eq!(
        changes,
        [ViewChange::CloseWindow {
            window: "main".into()
        }]
    );
    assert!(f.registry.is_closing("main"));
    f.registry.forget_window("main");
    assert!(f.registry.list_windows().iter().all(|w| w.window != "main"));
    assert!(
        workspace.documents().unsaved(&hidden_view).is_none(),
        "guard released"
    );
}

// ---- move -------------------------------------------------------------------------------------------------------

#[test]
fn a_moved_tab_keeps_its_workspace_and_unsaved_flag() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (_, keep_view) = tab(&f, "main", &file(&dir.path().join("keep.md"))).unwrap();
    let moving_ref = file(&dir.path().join("move.md"));
    let (moving, view) = tab(&f, "main", &moving_ref).unwrap();
    let (id, workspace) = f.registry.workspace_of(&view).unwrap();
    workspace
        .documents()
        .set_unsaved(&view, Some("move.md".into()));
    f.registry.add_window("w-9");
    let ticket = f.registry.begin_move(moving, &MoveTo::NewWindow).unwrap();
    let outcome = f
        .registry
        .move_tab(&ticket, "w-9", Some(view.as_str()))
        .unwrap();
    drop(ticket);
    assert_eq!(
        outcome.changes,
        [
            ViewChange::Show {
                window: "main".into(),
                view: keep_view.clone()
            },
            ViewChange::Transfer {
                from: "main".into(),
                tab: moving,
                view: Some(view.clone()),
                to: MoveTo::Window("w-9".into())
            },
            ViewChange::Show {
                window: "w-9".into(),
                view: view.clone()
            }
        ]
    );
    assert_eq!(f.registry.shown_view("main"), Some(keep_view));
    assert_eq!(f.registry.workspace_of(&view).unwrap().0, id);
    assert_eq!(f.registry.window_of(&view).as_deref(), Some("w-9"));
    assert!(window_of(&f, "w-9").tabs[0].unsaved);
    assert_eq!(f.registry.window_of_tab(moving).as_deref(), Some("w-9"));
    // one place all along: the workspace is found where it went (no second tab for it)
    assert_eq!(
        f.registry.find(&moving_ref),
        Some(Place {
            window: "w-9".into(),
            tab: moving
        })
    );
    assert!(matches!(
        f.registry.open_tab("w-1", &moving_ref, None),
        Ok(TabOpen::Existing(_))
    ));
}

/// Adversarial: a move that cannot happen changes nothing (target gone | closing, the tab's view not the one the
/// shell moved, the source asking to close).
#[test]
fn a_refused_move_leaves_both_windows_as_they_were() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (_, keep_view) = tab(&f, "main", &file(&dir.path().join("keep.md"))).unwrap();
    let (moving, view) = tab(&f, "main", &file(&dir.path().join("move.md"))).unwrap();
    let before = f.registry.list_windows();
    let refused = |to: &str, view: &str| {
        // a source asking to close refuses at once; the rest when the registry step runs
        f.registry
            .begin_move(moving, &MoveTo::NewWindow)
            .map_or(true, |ticket| {
                matches!(
                    f.registry.move_tab(&ticket, to, Some(view)),
                    Err(MoveRefused)
                )
            })
    };
    assert!(refused("nowhere", &view));
    assert!(refused("w-1", &keep_view), "not the view the shell moved");
    assert!(refused("main", &view));
    f.registry.add_window("w-9");
    let _ = f.registry.request_close("w-9");
    assert!(refused("w-9", &view), "target closing");
    f.registry.add_window("w-8");
    let (_, workspace) = f.registry.workspace_of(&keep_view).unwrap();
    workspace
        .documents()
        .set_unsaved(&keep_view, Some("keep.md".into()));
    let _ = f.registry.request_close("main");
    assert!(refused("w-8", &view), "source asking to close");
    let _ = f
        .registry
        .resolve_close("main", Resolution::Stay { show: None });
    workspace.documents().set_unsaved(&keep_view, None);
    let after: Vec<WindowSummary> = f
        .registry
        .list_windows()
        .into_iter()
        .filter(|w| w.window != "w-8")
        .collect();
    assert_eq!(after, before);
    assert_eq!(f.registry.window_of(&view).as_deref(), Some("main"));
}

/// Adversarial: a tab moved back and forth while another thread looks for its workspace and opens it again. One
/// place throughout: always found (in one window or the other), never a second tab for it.
#[test]
fn a_tab_moving_back_and_forth_is_always_found_in_one_place() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    f.registry.add_window("w-9");
    tab(&f, "main", &file(&dir.path().join("keep-a.md"))).unwrap();
    tab(&f, "w-9", &file(&dir.path().join("keep-b.md"))).unwrap();
    let moving_ref = file(&dir.path().join("move.md"));
    let (moving, view) = tab(&f, "main", &moving_ref).unwrap();
    let tabs_before: usize = f.registry.list_windows().iter().map(|w| w.tabs.len()).sum();
    std::thread::scope(|s| {
        s.spawn(|| {
            for round in 0..200 {
                let to = if round % 2 == 0 { "w-9" } else { "main" };
                let to_window = MoveTo::Window(to.to_owned());
                let ticket = f.registry.begin_move(moving, &to_window).unwrap();
                f.registry.move_tab(&ticket, to, Some(&view)).unwrap();
            }
        });
        s.spawn(|| {
            for _ in 0..2000 {
                let place = f.registry.find(&moving_ref).expect("found while moving");
                assert_eq!(place.tab, moving);
                assert!(matches!(
                    f.registry.open_tab("w-9", &moving_ref, None),
                    Ok(TabOpen::Existing(_))
                ));
            }
        });
    });
    let tabs_after: usize = f.registry.list_windows().iter().map(|w| w.tabs.len()).sum();
    assert_eq!(tabs_after, tabs_before);
    assert_eq!(f.registry.window_of(&view).as_deref(), Some("main"));
}

/// Adversarial: two moves of one tab at once (a double click on "Move to new window"): the second is refused while
/// the first is under way, so the tab and its view always end in the same window; once the first is done, the tab
/// can move again.
#[test]
fn a_second_move_of_a_tab_under_way_is_refused() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    tab(&f, "main", &file(&dir.path().join("keep.md"))).unwrap();
    let (moving, view) = tab(&f, "main", &file(&dir.path().join("move.md"))).unwrap();
    f.registry.add_window("w-8");
    f.registry.add_window("w-9");
    let first = f.registry.begin_move(moving, &MoveTo::NewWindow).unwrap();
    assert!(matches!(
        f.registry.begin_move(moving, &MoveTo::NewWindow),
        Err(MoveRefused)
    ));
    assert!(matches!(
        f.registry.begin_move(moving, &MoveTo::Window("w-9".into())),
        Err(MoveRefused)
    ));
    f.registry.move_tab(&first, "w-8", Some(&view)).unwrap();
    drop(first);
    assert_eq!(f.registry.window_of_tab(moving).as_deref(), Some("w-8"));
    assert_eq!(f.registry.window_of(&view).as_deref(), Some("w-8"));
    // done: the tab moves again (to a window: its only tab may go, the window it leaves closes)
    let again = f
        .registry
        .begin_move(moving, &MoveTo::Window("w-9".into()))
        .unwrap();
    f.registry.move_tab(&again, "w-9", Some(&view)).unwrap();
    assert_eq!(f.registry.window_of(&view).as_deref(), Some("w-9"));
}

/// Adversarial: many threads move one tab to new windows at once: at most one holds a ticket at a time, every move
/// that ran left the tab with its view, and the tab is found exactly once.
#[test]
fn concurrent_moves_of_one_tab_never_split_tab_and_view() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    tab(&f, "main", &file(&dir.path().join("keep.md"))).unwrap();
    let (moving, view) = tab(&f, "main", &file(&dir.path().join("move.md"))).unwrap();
    // each window keeps a tab of its own: the moving one is never the only one (it may always go on)
    for n in 0..8 {
        let window = format!("w-{n}");
        f.registry.add_window(&window);
        tab(&f, &window, &file(&dir.path().join(format!("keep-{n}.md")))).unwrap();
    }
    let held = std::sync::atomic::AtomicUsize::new(0);
    let moves = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for n in 0..8 {
            let (f, view, held, moves) = (&f, &view, &held, &moves);
            s.spawn(move || {
                for round in 0..200 {
                    let Ok(ticket) = f.registry.begin_move(moving, &MoveTo::NewWindow) else {
                        continue;
                    };
                    assert_eq!(held.fetch_add(1, std::sync::atomic::Ordering::SeqCst), 0);
                    let to = format!("w-{}", (n + round) % 8);
                    if f.registry.move_tab(&ticket, &to, Some(view)).is_ok() {
                        moves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    assert_eq!(f.registry.window_of_tab(moving), f.registry.window_of(view));
                    held.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                }
            });
        }
    });
    let count: usize = f
        .registry
        .list_windows()
        .iter()
        .map(|w| w.tabs.iter().filter(|t| t.tab == moving).count())
        .sum();
    assert_eq!(count, 1);
    assert_eq!(
        f.registry.window_of_tab(moving),
        f.registry.window_of(&view)
    );
    assert!(moves.into_inner() > 8, "the tab moved many times");
}

// ---- move to an existing window ("Move to window") ---------------------------------------------------------------

/// The tab goes into a window that has tabs of its own: it is the active one there, its workspace keeps its id and
/// its one place; the window it left shows its other tab.
#[test]
fn a_tab_moved_into_an_existing_window_is_active_there() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (_, keep_view) = tab(&f, "main", &file(&dir.path().join("keep.md"))).unwrap();
    let moving_ref = file(&dir.path().join("move.md"));
    let (moving, view) = tab(&f, "main", &moving_ref).unwrap();
    let (theirs, their_view) = tab(&f, "w-1", &file(&dir.path().join("theirs.md"))).unwrap();
    let id = f.registry.workspace_of(&view).unwrap().0;
    let to = MoveTo::Window("w-1".into());
    let ticket = f.registry.begin_move(moving, &to).unwrap();
    let changes = run(
        &f,
        f.registry.move_tab(&ticket, "w-1", Some(&view)).unwrap(),
    );
    drop(ticket);
    assert!(changes.contains(&ViewChange::Show {
        window: "w-1".into(),
        view: view.clone()
    }));
    assert!(
        !changes
            .iter()
            .any(|c| matches!(c, ViewChange::CloseWindow { .. })),
        "main keeps a tab: it stays"
    );
    let target = window_of(&f, "w-1");
    assert_eq!(
        target.tabs.iter().map(|t| t.tab).collect::<Vec<_>>(),
        [theirs, moving]
    );
    assert_eq!(target.active, Some(moving));
    assert_eq!(f.registry.shown_view("w-1"), Some(view.clone()));
    assert_eq!(f.registry.shown_view("main"), Some(keep_view));
    assert_eq!(f.registry.window_of(&their_view).as_deref(), Some("w-1"));
    assert_eq!(f.registry.workspace_of(&view).unwrap().0, id);
    assert_eq!(
        f.registry.find(&moving_ref),
        Some(Place {
            window: "w-1".into(),
            tab: moving
        })
    );
}

/// "Move to window" takes a window's only tab too: the window it leaves closes (after the target shows: the shell
/// applies `CloseWindow` last). A pending tab moves as its ref and loads where it lands.
#[test]
fn the_only_tab_may_move_into_another_window_and_its_window_closes() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    f.registry.add_window("w-2");
    let _ = f.registry.show_welcome_view("w-2");
    let (only, view) = tab(&f, "w-2", &file(&dir.path().join("only.md"))).unwrap();
    let ticket = f
        .registry
        .begin_move(only, &MoveTo::Window("main".into()))
        .unwrap();
    let changes = run(
        &f,
        f.registry.move_tab(&ticket, "main", Some(&view)).unwrap(),
    );
    drop(ticket);
    assert!(changes.contains(&ViewChange::CloseWindow {
        window: "w-2".into()
    }));
    assert_eq!(window_of(&f, "main").active, Some(only));
    // a pending tab (restored, never shown) moves as its ref
    let pending_ref = file(&dir.path().join("pending.md"));
    let pending = f.registry.restore_tab("w-1", &pending_ref, None).unwrap();
    let _ = f
        .registry
        .restore_tab("w-1", &file(&dir.path().join("stays.md")), None);
    let ticket = f
        .registry
        .begin_move(pending, &MoveTo::Window("main".into()))
        .unwrap();
    let outcome = f.registry.move_tab(&ticket, "main", None).unwrap();
    // it loads where it lands (activated there); the tab left in w-1 becomes active there and loads too
    assert_eq!(outcome.loads.len(), 2);
    assert!(outcome.loads.iter().any(|(ticket, reference)| {
        *reference == pending_ref && f.registry.window_of(ticket.view()).as_deref() == Some("main")
    }));
    run(&f, outcome);
    drop(ticket);
    assert_eq!(window_of(&f, "main").active, Some(pending));
    assert_eq!(
        f.registry.find(&pending_ref).map(|p| p.window).as_deref(),
        Some("main")
    );
}

/// Adversarial: the target is asked to close while a tab is moving into it (the person closes it at that moment).
/// The close waits (nothing changes: the view sits in that native window already and would go with it), then is
/// asked again once the move is done: the moved tab's unsaved edits are in the prompt, never lost.
#[test]
fn a_window_asked_to_close_during_a_move_into_it_waits_for_the_move() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    tab(&f, "main", &file(&dir.path().join("keep.md"))).unwrap();
    let (moving, view) = tab(&f, "main", &file(&dir.path().join("move.md"))).unwrap();
    tab(&f, "w-1", &file(&dir.path().join("theirs.md"))).unwrap();
    let (_, workspace) = f.registry.workspace_of(&view).unwrap();
    workspace
        .documents()
        .set_unsaved(&view, Some("move.md".into()));
    let ticket = f
        .registry
        .begin_move(moving, &MoveTo::Window("w-1".into()))
        .unwrap();
    let held = f.registry.request_close("w-1");
    assert!(held.changes.is_empty() && held.loads.is_empty());
    assert!(!f.registry.is_closing("w-1") && !f.registry.is_confirming_close("w-1"));
    assert!(
        !f.registry.take_deferred_close("w-1"),
        "the move is still under way"
    );
    // a second ask meanwhile is held the same way
    assert_eq!(f.registry.request_close("w-1").changes, []);
    f.registry.move_tab(&ticket, "w-1", Some(&view)).unwrap();
    assert!(!f.registry.take_deferred_close("w-1"), "ticket still held");
    drop(ticket);
    assert!(f.registry.take_deferred_close("w-1"));
    assert!(!f.registry.take_deferred_close("w-1"), "once");
    let changes = f.registry.request_close("w-1").changes;
    let [ViewChange::AskClose { tabs, .. }] = changes.as_slice() else {
        panic!("one prompt: {changes:?}");
    };
    assert_eq!(tabs.iter().map(|t| t.tab).collect::<Vec<_>>(), [moving]);
    // a window not asked meanwhile has nothing waiting; a window asking to close lets no tab move out
    assert!(!f.registry.take_deferred_close("main"));
    assert!(f
        .registry
        .begin_move(moving, &MoveTo::Window("main".into()))
        .is_err());
}

/// Adversarial: a target that cannot take the tab refuses at `begin_move` (nothing made, nothing moved): unknown,
/// the tab's own window, closing, asking to close. A target gone between `begin_move` and `move_tab` refuses there;
/// the tab stays with its view. A wait for a window that is then forgotten is dropped with it.
#[test]
fn a_target_that_cannot_take_the_tab_refuses_and_the_tab_stays() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    tab(&f, "main", &file(&dir.path().join("keep.md"))).unwrap();
    let (moving, view) = tab(&f, "main", &file(&dir.path().join("move.md"))).unwrap();
    let refused = |to: &str| {
        matches!(
            f.registry
                .begin_move(moving, &MoveTo::Window(to.to_owned())),
            Err(MoveRefused)
        )
    };
    assert!(refused("nowhere"));
    assert!(refused("main"), "its own window");
    f.registry.add_window("w-7");
    let _ = f.registry.request_close("w-7");
    assert!(f.registry.is_closing("w-7"));
    assert!(refused("w-7"), "closing");
    let (_, asking_view) = tab(&f, "w-1", &file(&dir.path().join("asking.md"))).unwrap();
    let (_, asking) = f.registry.workspace_of(&asking_view).unwrap();
    asking
        .documents()
        .set_unsaved(&asking_view, Some("asking.md".into()));
    let _ = f.registry.request_close("w-1");
    assert!(f.registry.is_confirming_close("w-1"));
    assert!(refused("w-1"), "asking to close");
    // gone between the two steps (its native window destroyed)
    f.registry.add_window("w-8");
    let ticket = f
        .registry
        .begin_move(moving, &MoveTo::Window("w-8".into()))
        .unwrap();
    let _ = f.registry.request_close("w-8");
    f.registry.forget_window("w-8");
    assert!(matches!(
        f.registry.move_tab(&ticket, "w-8", Some(&view)),
        Err(MoveRefused)
    ));
    drop(ticket);
    assert!(!f.registry.take_deferred_close("w-8"), "forgotten with it");
    assert_eq!(f.registry.window_of_tab(moving).as_deref(), Some("main"));
    assert_eq!(f.registry.window_of(&view).as_deref(), Some("main"));
}

/// Adversarial: 8 threads move 8 tabs between 4 windows (each keeps a tab of its own) for 200 rounds. Every move
/// that ran left its tab with its view; at the end every tab is in exactly one window, every workspace found once.
#[test]
fn concurrent_moves_between_existing_windows_keep_one_place() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let windows = ["main", "w-1", "w-2", "w-3"];
    for window in &windows[2..] {
        f.registry.add_window(window);
        let _ = f.registry.show_welcome_view(window);
    }
    let mut moving = Vec::new();
    for (n, window) in windows.iter().enumerate() {
        tab(&f, window, &file(&dir.path().join(format!("keep-{n}.md")))).unwrap();
        for k in 0..2 {
            let reference = file(&dir.path().join(format!("move-{n}-{k}.md")));
            let (id, view) = tab(&f, window, &reference).unwrap();
            moving.push((id, view, reference));
        }
    }
    let moves = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for n in 0..8 {
            let (f, moving, moves, windows) = (&f, &moving, &moves, &windows);
            s.spawn(move || {
                for round in 0..200 {
                    let (tab_id, view, _) = &moving[(n + round) % moving.len()];
                    let to = windows[(n + 3 * round) % windows.len()];
                    let Ok(ticket) = f
                        .registry
                        .begin_move(*tab_id, &MoveTo::Window(to.to_owned()))
                    else {
                        continue;
                    };
                    if f.registry.move_tab(&ticket, to, Some(view)).is_ok() {
                        moves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        assert_eq!(f.registry.window_of_tab(*tab_id).as_deref(), Some(to));
                    }
                    assert_eq!(
                        f.registry.window_of_tab(*tab_id),
                        f.registry.window_of(view)
                    );
                }
            });
        }
    });
    let listed = f.registry.list_windows();
    for (tab_id, view, reference) in &moving {
        let count: usize = listed
            .iter()
            .map(|w| w.tabs.iter().filter(|t| t.tab == *tab_id).count())
            .sum();
        assert_eq!(count, 1);
        assert_eq!(
            f.registry.window_of_tab(*tab_id),
            f.registry.window_of(view)
        );
        assert_eq!(f.registry.find(reference).map(|p| p.tab), Some(*tab_id));
    }
    assert_eq!(listed.iter().map(|w| w.tabs.len()).sum::<usize>(), 12);
    assert!(moves.into_inner() > 100, "the tabs moved many times");
}

/// "Move to new window" never takes a window's only tab (spec: disabled with one tab), nor its last one staying
/// while another is being moved out; a window that is closing | asking lets no tab move.
#[test]
fn the_only_tab_never_moves_to_a_new_window() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (only, _) = tab(&f, "main", &file(&dir.path().join("only.md"))).unwrap();
    assert!(matches!(
        f.registry.begin_move(only, &MoveTo::NewWindow),
        Err(MoveRefused)
    ));
    let (second, _) = tab(&f, "main", &file(&dir.path().join("second.md"))).unwrap();
    let first = f.registry.begin_move(only, &MoveTo::NewWindow).unwrap();
    assert!(
        matches!(
            f.registry.begin_move(second, &MoveTo::NewWindow),
            Err(MoveRefused)
        ),
        "the other tab is leaving: this one stays"
    );
    drop(first);
    assert!(f.registry.begin_move(second, &MoveTo::NewWindow).is_ok());
    assert!(matches!(
        f.registry.begin_move(TabId(9999), &MoveTo::NewWindow),
        Err(MoveRefused)
    ));
    let _ = f.registry.request_close("main");
    assert!(matches!(
        f.registry.begin_move(second, &MoveTo::NewWindow),
        Err(MoveRefused)
    ));
}

/// The last window on screen: windows being made (not shown yet) and closing ones do not count; a shown one does,
/// minimized or not (the registry knows, the OS visibility is not asked).
#[test]
fn the_last_window_on_screen_counts_shown_windows_only() {
    let f = fx();
    f.registry.window_shown("main");
    assert!(f.registry.is_last_on_screen("main"));
    f.registry.add_window("w-1");
    assert!(f.registry.is_last_on_screen("main"), "w-1 still being made");
    f.registry.window_shown("w-1");
    assert!(!f.registry.is_last_on_screen("main"));
    assert!(!f.registry.is_last_on_screen("w-1"));
    let _ = f.registry.request_close("w-1");
    assert!(f.registry.is_closing("w-1"));
    assert!(f.registry.is_last_on_screen("main"), "w-1 closing");
    f.registry.forget_window("w-1");
    assert!(f.registry.is_last_on_screen("main"));
    f.registry.window_shown("gone");
    assert!(
        f.registry.is_last_on_screen("main"),
        "unknown windows are never marked"
    );
}

// ---- live reload of the project state (PerMark initialized while the project is open) ---------------------------------

fn open_project(f: &Fx, dir: &Path) -> (Arc<ProjectCore>, String) {
    let (_, view) = tab(f, "main", &project(dir)).unwrap();
    take(&f.log);
    f.sink.take();
    (core_of(f, &view), view)
}

#[test]
fn initializing_an_open_project_moves_its_state_and_restarts_its_plugins() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (core, view) = open_project(&f, dir.path());
    assert_eq!(core.state_location(), StateLocation::Profile);
    let env = f.registry.env().clone();
    crate::state::init_project(&env.dirs, core.root()).unwrap();
    // nothing changes until the core is told: the plugins still hold the old folder
    assert_eq!(core.state_location(), StateLocation::Profile);
    core.reload_state(&env);
    f.registry.refresh();
    assert_eq!(core.state_location(), StateLocation::Project);
    assert_eq!(
        take(&f.log),
        ["stop:unread", "stop:git", "start:git", "start:unread"]
    );
    let names: Vec<String> = f.sink.take().iter().map(|e| e.name().to_owned()).collect();
    assert!(names.contains(&"window-changed".to_owned()), "{names:?}");
    assert!(names.contains(&"context-changed".to_owned()), "{names:?}");
    assert_eq!(
        f.registry.context_of(&view).unwrap().permark,
        PermarkCondition::Initialized
    );
    match f.registry.window_view(&view).unwrap() {
        WindowView::Showing {
            workspace: WorkspaceView::Project { state_location, .. },
            ..
        } => assert_eq!(state_location, StateLocation::Project),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_second_reload_with_nothing_new_still_leaves_the_project_working() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = open_project(&f, dir.path());
    let env = f.registry.env().clone();
    crate::state::init_project(&env.dirs, core.root()).unwrap();
    for _ in 0..3 {
        core.reload_state(&env);
    }
    let log = take(&f.log);
    assert_eq!(log.iter().filter(|e| *e == "start:git").count(), 3);
    assert_eq!(log.iter().filter(|e| *e == "stop:git").count(), 3);
    assert_eq!(core.running_plugins().len(), 2);
}

#[test]
fn a_project_setting_is_written_and_applies_at_once() {
    use crate::settings::{ProjectSettingChange, TreeShow};
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "# a\n").unwrap();
    let (core, _) = open_project(&f, dir.path());
    // not initialized: refused, nothing written
    let refused = core.change_config(&ProjectSettingChange::TreeShow(Some(TreeShow::Everything)));
    assert!(matches!(
        refused,
        Err(crate::settings::SettingsError::NotInitialized)
    ));
    assert!(!dir.path().join(".permark").exists());
    let env = f.registry.env().clone();
    crate::state::init_project(&env.dirs, core.root()).unwrap();
    core.reload_state(&env);
    let config = core
        .change_config(&ProjectSettingChange::TreeShow(Some(TreeShow::Everything)))
        .unwrap();
    assert_eq!(config.tree.show, Some(TreeShow::Everything));
    assert_eq!(core.config().tree.show, Some(TreeShow::Everything));
    assert_eq!(
        core.explorer().default_filter().show,
        crate::tree::FilterShow::Everything
    );
    let text = std::fs::read_to_string(dir.path().join(".permark/config.toml")).unwrap();
    assert!(text.contains("show = \"everything\""), "{text}");
    // the commented defaults of the file are still there
    assert!(text.contains("# PerMark project settings."), "{text}");
}

#[test]
fn a_window_on_the_default_filter_follows_the_new_default_and_one_with_its_own_stays() {
    use crate::settings::{ProjectSettingChange, TreeShow};
    use crate::tree::{FilterShow, TreeFilter};
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "# a\n").unwrap();
    std::fs::write(dir.path().join("b.json"), "{}").unwrap();
    let (core, _) = open_project(&f, dir.path());
    let env = f.registry.env().clone();
    crate::state::init_project(&env.dirs, core.root()).unwrap();
    core.reload_state(&env);
    core.explorer().open_view("main", None);
    core.explorer().open_view(
        "w-1",
        Some(TreeFilter {
            show: FilterShow::Documents,
            ..TreeFilter::default()
        }),
    );
    f.sink.take();
    core.change_config(&ProjectSettingChange::TreeShow(Some(TreeShow::Everything)))
        .unwrap();
    let moved: Vec<_> = f
        .sink
        .take()
        .into_iter()
        .filter_map(|e| match e {
            crate::events::CoreEvent::TreeChanged(t) => Some(t),
            _ => None,
        })
        .collect();
    assert!(
        moved.iter().any(|t| t.window == "main"
            && t.delta.filter.as_ref().map(|f| f.show) == Some(FilterShow::Everything)),
        "the window on the default is told its filter moved: {moved:?}"
    );
    assert!(
        moved.iter().all(|t| t.window != "w-1"),
        "a window with its own filter hears nothing: {moved:?}"
    );
}

#[test]
fn the_ignore_patterns_apply_at_once_and_back() {
    use crate::settings::ProjectSettingChange;
    use crate::tree::FilterShow;
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "# a\n").unwrap();
    std::fs::write(dir.path().join("draft.md"), "# d\n").unwrap();
    let (core, _) = open_project(&f, dir.path());
    let env = f.registry.env().clone();
    crate::state::init_project(&env.dirs, core.root()).unwrap();
    core.reload_state(&env);
    let shown = |core: &ProjectCore| -> Vec<String> {
        let snapshot = core.explorer().open_view(
            "main",
            Some(crate::tree::TreeFilter {
                show: FilterShow::Everything,
                ..crate::tree::TreeFilter::default()
            }),
        );
        snapshot.nodes.iter().map(|n| n.path.clone()).collect()
    };
    assert!(shown(&core).contains(&"draft.md".to_owned()));
    core.change_config(&ProjectSettingChange::Ignore(vec!["draft.md".to_owned()]))
        .unwrap();
    let after = shown(&core);
    assert!(!after.contains(&"draft.md".to_owned()), "{after:?}");
    assert!(after.contains(&"a.md".to_owned()));
    core.change_config(&ProjectSettingChange::Ignore(Vec::new()))
        .unwrap();
    assert!(shown(&core).contains(&"draft.md".to_owned()));
}

#[test]
fn a_per_project_choice_stops_and_starts_the_plugin_and_is_kept_in_the_profile() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = open_project(&f, dir.path());
    let env = f.registry.env().clone();
    let git = crate::plugins::PluginId::of("git");
    core.set_plugin_choice(&env, &git, Some(false)).unwrap();
    // git off in this project: its dependent (unread) goes with it
    assert_eq!(
        take(&f.log),
        ["stop:unread", "stop:git"],
        "dependents stop first"
    );
    assert_eq!(core.plugin_choices().get(&git), Some(&false));
    assert!(!core.plugins_on().contains(&git));
    // the project's own state folder is the profile one: nothing written into the folder
    assert!(!dir.path().join(".permark").exists());
    core.set_plugin_choice(&env, &git, None).unwrap();
    assert_eq!(take(&f.log), ["start:git", "start:unread"]);
    assert_eq!(core.plugin_choices().len(), 0);
}
// ---- move to window: adversarial (verifier) ---------------------------------------------------------------------

fn saved_count(f: &Fx, reference: &WorkspaceRef) -> usize {
    let dto = reference.to_dto();
    f.registry
        .session_windows()
        .iter()
        .map(|w| w.tabs.iter().filter(|t| **t == dto).count())
        .sum()
}

/// Adversarial: the only tab of an older window moves into a newer one; the emptied source is Closing until its
/// native window is destroyed. A session saved in that gap (saver tick | the target's deferred close, which is then
/// the last on screen) must list the moved tab once, in the target; else restore (creation order, first wins)
/// brings back the source window holding it.
#[test]
fn only_tab_move_is_saved_once_in_the_target() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    f.registry.window_shown("main");
    f.registry.window_shown("w-1");
    let moving_ref = file(&dir.path().join("only.md"));
    let (only, view) = tab(&f, "main", &moving_ref).unwrap();
    tab(&f, "w-1", &file(&dir.path().join("theirs.md"))).unwrap();
    let ticket = f
        .registry
        .begin_move(only, &MoveTo::Window("w-1".into()))
        .unwrap();
    // the person closes the target meanwhile: deferred
    assert_eq!(f.registry.request_close("w-1").changes, []);
    let changes = run(
        &f,
        f.registry.move_tab(&ticket, "w-1", Some(&view)).unwrap(),
    );
    drop(ticket);
    assert!(changes.contains(&ViewChange::CloseWindow {
        window: "main".into()
    }));
    // main is Closing, not yet forgotten (Destroyed comes later on the main thread)
    assert!(f.registry.take_deferred_close("w-1"));
    let close = f.registry.request_close("w-1").changes;
    assert!(close.contains(&ViewChange::CloseWindow {
        window: "w-1".into()
    }));
    assert!(
        f.registry.is_last_on_screen("w-1"),
        "shell saves the session now"
    );
    let session = f.registry.session_windows();
    assert_eq!(saved_count(&f, &moving_ref), 1, "moved tab saved once");
    assert!(
        !session.iter().any(|w| w.window == "main"),
        "the window the move emptied is not brought back"
    );
}

/// Adversarial: rapid back-and-forth "Move to window" of an only tab between two windows that each close when
/// emptied: the second move finds its source Closing -> refused, never a tab lost.
#[test]
fn only_tab_cannot_move_out_of_a_window_that_closed() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let moving_ref = file(&dir.path().join("only.md"));
    let (only, view) = tab(&f, "main", &moving_ref).unwrap();
    let t = f
        .registry
        .begin_move(only, &MoveTo::Window("w-1".into()))
        .unwrap();
    run(&f, f.registry.move_tab(&t, "w-1", Some(&view)).unwrap());
    drop(t);
    // back into main (Closing now): refused at begin_move
    assert!(f
        .registry
        .begin_move(only, &MoveTo::Window("main".into()))
        .is_err());
    assert_eq!(f.registry.window_of_tab(only).as_deref(), Some("w-1"));
    assert_eq!(
        f.registry.find(&moving_ref).map(|p| p.window).as_deref(),
        Some("w-1")
    );
}

/// Adversarial: two moves into one target, close asked between them: the close waits for both, fires once.
#[test]
fn deferred_close_waits_for_every_move_into_the_target() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    tab(&f, "main", &file(&dir.path().join("keep.md"))).unwrap();
    let (a, va) = tab(&f, "main", &file(&dir.path().join("a.md"))).unwrap();
    let (b, vb) = tab(&f, "main", &file(&dir.path().join("b.md"))).unwrap();
    let ta = f
        .registry
        .begin_move(a, &MoveTo::Window("w-1".into()))
        .unwrap();
    let tb = f
        .registry
        .begin_move(b, &MoveTo::Window("w-1".into()))
        .unwrap();
    assert_eq!(f.registry.request_close("w-1").changes, []);
    f.registry.move_tab(&ta, "w-1", Some(&va)).unwrap();
    drop(ta);
    assert!(!f.registry.take_deferred_close("w-1"), "b still moving");
    f.registry.move_tab(&tb, "w-1", Some(&vb)).unwrap();
    drop(tb);
    assert!(f.registry.take_deferred_close("w-1"));
    assert!(!f.registry.take_deferred_close("w-1"));
    assert!(f
        .registry
        .request_close("w-1")
        .changes
        .contains(&ViewChange::CloseWindow {
            window: "w-1".into()
        }));
}

/// Adversarial: a window whose close waits for a move into it takes no new move (else a stream of moves would hold
/// its close for ever); it is listed as closing (the "Move to window" menu leaves it out) until the close is asked
/// again; a window asking to close (prompt up) is listed so too.
#[test]
fn a_window_waiting_to_close_takes_no_new_move_and_is_listed_as_closing() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    tab(&f, "main", &file(&dir.path().join("keep.md"))).unwrap();
    let (a, va) = tab(&f, "main", &file(&dir.path().join("a.md"))).unwrap();
    let (b, _) = tab(&f, "main", &file(&dir.path().join("b.md"))).unwrap();
    tab(&f, "w-1", &file(&dir.path().join("theirs.md"))).unwrap();
    assert!(!window_of(&f, "w-1").closing);
    let ta = f
        .registry
        .begin_move(a, &MoveTo::Window("w-1".into()))
        .unwrap();
    assert_eq!(f.registry.request_close("w-1").changes, []);
    assert!(window_of(&f, "w-1").closing, "published at once");
    assert!(matches!(
        f.registry.begin_move(b, &MoveTo::Window("w-1".into())),
        Err(MoveRefused)
    ));
    f.registry.move_tab(&ta, "w-1", Some(&va)).unwrap();
    drop(ta);
    assert!(f.registry.take_deferred_close("w-1"));
    // asked again: the moved tab has unsaved edits -> prompt up, still closing
    let (_, workspace) = f.registry.workspace_of(&va).unwrap();
    workspace.documents().set_unsaved(&va, Some("a.md".into()));
    let _ = f.registry.request_close("w-1");
    assert!(f.registry.is_confirming_close("w-1"));
    assert!(window_of(&f, "w-1").closing);
    let _ = f
        .registry
        .resolve_close("w-1", Resolution::Stay { show: None });
    assert!(!window_of(&f, "w-1").closing);
    assert!(f
        .registry
        .begin_move(b, &MoveTo::Window("w-1".into()))
        .is_ok());
}
