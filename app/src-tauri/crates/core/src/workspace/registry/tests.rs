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
        Workspace::SingleFile(_) => panic!("not a project"),
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
