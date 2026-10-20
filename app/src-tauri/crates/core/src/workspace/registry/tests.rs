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

fn fx() -> Fx {
    let base = tempfile::tempdir().unwrap();
    let log = Log::default();
    let plugins: Vec<Arc<dyn Plugin>> = vec![
        Arc::new(Fake::new("git", PluginScope::Project, &[]).with_log(&log)),
        Arc::new(Fake::new("unread", PluginScope::Project, &["git"]).with_log(&log)),
    ];
    let sink = Arc::new(Recorder::default());
    let env = CoreEnv::new(
        AppDirs::under(base.path()),
        Arc::new(PluginRegistry::new(plugins).unwrap()),
        sink.clone(),
        Distribution::Direct,
        AppSettings::default(),
    );
    let registry = Arc::new(WorkspaceRegistry::new(Arc::new(env)));
    registry.add_window("main");
    registry.add_window("w-1");
    Fx {
        _base: base,
        registry,
        sink,
        log,
    }
}

fn project(dir: &Path) -> WorkspaceRef {
    WorkspaceRef::Project(Root::open(dir).unwrap())
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.lock().unwrap())
}

fn core_of(f: &Fx, label: &str) -> Arc<ProjectCore> {
    match f.registry.workspace_of(label).unwrap().1 {
        Workspace::Project(core) => core,
        Workspace::SingleFile(_) | Workspace::Notes(_) => panic!("not a project"),
    }
}

#[test]
fn two_windows_on_one_root_share_one_core_until_the_last_leaves() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let a = f.registry.open("main", &project(dir.path()), None).unwrap();
    let b = f
        .registry
        .open("w-1", &project(&dir.path().join(".")), None)
        .unwrap();
    assert_eq!(a, b);
    assert!(Arc::ptr_eq(&core_of(&f, "main"), &core_of(&f, "w-1")));
    assert_eq!(take(&f.log), ["start:git", "start:unread"]);
    assert_eq!(f.registry.list()[0].windows, 2);
    assert_eq!(f.registry.windows_of(a), ["main", "w-1"]);
    f.registry.show_welcome("main");
    assert_eq!(take(&f.log).len(), 0, "core alive while w-1 shows it");
    f.registry.forget_window("w-1");
    assert_eq!(take(&f.log), ["stop:unread", "stop:git"]);
    assert_eq!(f.registry.list().len(), 0);
    assert_eq!(lock(&f.registry.cores).len(), 0, "dead slots pruned");
}

#[test]
fn concurrent_opens_of_one_root_build_one_core() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    for n in 0..8 {
        f.registry.add_window(&format!("w-{}", n + 2));
    }
    let ids: Vec<WorkspaceId> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let registry = Arc::clone(&f.registry);
                let reference = project(dir.path());
                s.spawn(move || {
                    registry
                        .open(&format!("w-{}", n + 2), &reference, None)
                        .unwrap()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(ids.windows(2).all(|w| w[0] == w[1]));
    let starts = take(&f.log)
        .iter()
        .filter(|e| e.as_str() == "start:git")
        .count();
    assert_eq!(starts, 1);
    assert_eq!(f.registry.list().len(), 1);
}

#[test]
fn a_superseded_open_is_dropped_and_its_core_torn_down() {
    let f = fx();
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let first = f
        .registry
        .begin_open("main", &project(one.path()), None)
        .unwrap();
    let second = f
        .registry
        .begin_open("main", &project(two.path()), None)
        .unwrap();
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
    assert_eq!(f.registry.list().len(), 1);
    assert_eq!(core_of(&f, "main").root(), &Root::open(two.path()).unwrap());
}

#[test]
fn a_failed_open_keeps_what_was_shown() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let id = f.registry.open("main", &project(dir.path()), None).unwrap();
    let result = f.registry.open("main", &WorkspaceRef::Notes, None);
    assert!(matches!(
        result,
        Err(OpenError::Workspace(WorkspaceError::NotesUnavailable))
    ));
    assert_eq!(f.registry.workspace_of("main").unwrap().0, id);
    let ipc: IpcError = OpenError::Workspace(WorkspaceError::NotesUnavailable).into();
    assert_eq!(ipc.kind, ErrorKind::Unsupported);
}

#[test]
fn switching_a_window_releases_the_old_workspace() {
    let f = fx();
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    f.registry.open("main", &project(one.path()), None).unwrap();
    f.registry.open("main", &project(two.path()), None).unwrap();
    assert_eq!(f.registry.list().len(), 1);
    let file = two.path().join("ñ 文.md");
    std::fs::write(&file, "x").unwrap();
    f.registry
        .open("main", &WorkspaceRef::classify(&file).unwrap(), None)
        .unwrap();
    let list = f.registry.list();
    assert_eq!(
        (list.len(), list[0].kind, list[0].title.as_str()),
        (1, WorkspaceKind::SingleFile, "ñ 文.md")
    );
}

#[test]
fn closing_window_mid_open_makes_the_result_stale() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let ticket = f
        .registry
        .begin_open("w-1", &project(dir.path()), None)
        .unwrap();
    let prepared = f.registry.prepare(&project(dir.path()));
    f.registry.forget_window("w-1");
    assert!(matches!(
        f.registry.finish_open(ticket, prepared),
        Err(OpenError::UnknownWindow(_))
    ));
    assert_eq!(f.registry.list().len(), 0);
    assert!(matches!(
        f.registry.begin_open("w-1", &project(dir.path()), None),
        Err(OpenError::UnknownWindow(_))
    ));
}

#[test]
fn events_only_when_something_differs() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    f.sink.take();
    f.registry.open("main", &project(dir.path()), None).unwrap();
    let names: Vec<String> = f.sink.take().iter().map(|e| e.name().to_owned()).collect();
    assert!(names.contains(&"workspaces-changed".to_owned()));
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
    let context = f.registry.context_of("main").unwrap();
    assert_eq!(context.workspace, WorkspaceKind::Project);
    assert_eq!(context.permark, PermarkCondition::NotInitialized);
    assert!(context
        .plugins
        .contains(&crate::plugins::PluginId::of("unread")));
    assert_eq!(
        f.registry.context_of("w-1").unwrap().workspace,
        WorkspaceKind::Welcome
    );
}

#[test]
fn find_containing_prefers_the_most_specific_project() {
    let f = fx();
    let outer = tempfile::tempdir().unwrap();
    let inner = outer.path().join("sub project");
    std::fs::create_dir_all(inner.join("docs")).unwrap();
    f.registry
        .open("main", &project(outer.path()), None)
        .unwrap();
    let inner_id = f.registry.open("w-1", &project(&inner), None).unwrap();
    let (id, rel) = f
        .registry
        .find_containing(&inner.join("docs").join("a.md"))
        .unwrap();
    assert_eq!((id, rel.as_str()), (inner_id, "docs/a.md"));
    assert!(f
        .registry
        .find_containing(tempfile::tempdir().unwrap().path())
        .is_none());
}

#[test]
fn document_hint_shows_in_the_window_view() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let doc = VaultPath::parse("untitled.md").unwrap();
    f.registry
        .open("main", &project(dir.path()), Some(doc))
        .unwrap();
    let Some(WindowView::Showing { document, .. }) = f.registry.window_view("main") else {
        panic!("not showing");
    };
    assert_eq!(document.as_deref(), Some("untitled.md"));
    let session = f.registry.session_windows();
    assert_eq!(session.len(), 2);
    assert!(
        matches!(&session[0], (label, Some(WorkspaceRefDto::Project { .. })) if label == "main")
    );
}

#[test]
fn rapid_open_storm_from_one_window_ends_on_the_last() {
    let f = fx();
    let dirs: Vec<tempfile::TempDir> = (0..5).map(|_| tempfile::tempdir().unwrap()).collect();
    let tickets: Vec<(Ticket, WorkspaceRef)> = (0..50)
        .map(|n| {
            let reference = project(dirs[n % 5].path());
            (
                f.registry.begin_open("main", &reference, None).unwrap(),
                reference,
            )
        })
        .collect();
    let mut ok = 0;
    for (ticket, reference) in tickets.into_iter().rev() {
        let prepared = f.registry.prepare(&reference);
        if f.registry.finish_open(ticket, prepared).is_ok() {
            ok += 1;
        }
    }
    assert_eq!(ok, 1);
    assert_eq!(f.registry.list().len(), 1);
    assert_eq!(
        core_of(&f, "main").root(),
        &Root::open(dirs[49 % 5].path()).unwrap()
    );
    drop(f.registry);
}

fn fx_notes(plugin_on: bool) -> Fx {
    let base = tempfile::tempdir().unwrap();
    let log = Log::default();
    let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();
    if plugin_on {
        plugins.push(Arc::new(
            Fake::new("notes", PluginScope::App, &[]).with_log(&log),
        ));
    }
    let sink = Arc::new(Recorder::default());
    let env = CoreEnv::new(
        AppDirs::under(base.path()),
        Arc::new(PluginRegistry::new(plugins).unwrap()),
        sink.clone(),
        Distribution::Direct,
        AppSettings::default(),
    );
    let registry = Arc::new(WorkspaceRegistry::new(Arc::new(env)));
    registry.add_window("main");
    registry.add_window("w-1");
    Fx {
        _base: base,
        registry,
        sink,
        log,
    }
}

#[test]
fn the_notes_open_only_while_the_notes_plugin_is_on() {
    let off = fx_notes(false);
    let error = off
        .registry
        .open("main", &WorkspaceRef::Notes, None)
        .unwrap_err();
    assert!(matches!(
        error,
        OpenError::Workspace(WorkspaceError::NotesUnavailable)
    ));
    assert_eq!(IpcError::from(error).kind, ErrorKind::Unsupported);
    assert!(off.registry.workspace_of("main").is_none());

    let on = fx_notes(true);
    on.env_apply();
    let id = on
        .registry
        .open("main", &WorkspaceRef::Notes, None)
        .unwrap();
    assert!(matches!(
        on.registry.workspace_of("main").unwrap().1,
        Workspace::Notes(_)
    ));
    // one Notes workspace for the process: a second window shows the same one
    let again = on.registry.open("w-1", &WorkspaceRef::Notes, None).unwrap();
    assert_eq!(id, again);
    let list = on.registry.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "Notes");
    assert_eq!(list[0].windows, 2);
    assert!(matches!(
        on.registry.window_view("main"),
        Some(WindowView::Showing {
            workspace: WorkspaceView::Notes { .. },
            ..
        })
    ));
    assert_eq!(
        on.registry.context_of("main").unwrap().workspace,
        WorkspaceKind::Notes
    );
    on.registry.show_welcome("main");
    on.registry.show_welcome("w-1");
    assert_eq!(on.registry.list().len(), 0);
}

impl Fx {
    /// Starts the app plugins the way the shell does at launch.
    fn env_apply(&self) {
        self.registry.env().apply_app_plugins();
    }
}

// ---- live reload of the project state (PerMark initialized while the project is open) ---------------------------------

fn open_project(f: &Fx, dir: &Path) -> Arc<ProjectCore> {
    f.registry.open("main", &project(dir), None).unwrap();
    take(&f.log);
    f.sink.take();
    core_of(f, "main")
}

#[test]
fn initializing_an_open_project_moves_its_state_and_restarts_its_plugins() {
    let f = fx();
    let dir = tempfile::tempdir().unwrap();
    let core = open_project(&f, dir.path());
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
        f.registry.context_of("main").unwrap().permark,
        PermarkCondition::Initialized
    );
    match f.registry.window_view("main").unwrap() {
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
    let core = open_project(&f, dir.path());
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
    let core = open_project(&f, dir.path());
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
    let core = open_project(&f, dir.path());
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
    let core = open_project(&f, dir.path());
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
    let core = open_project(&f, dir.path());
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
    assert!(core.plugin_choices().is_empty());
}
