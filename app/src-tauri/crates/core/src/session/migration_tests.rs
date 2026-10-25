//! Session v2: the 0.6.0 file (v1) read once and written as v2, round trips, the restore plan, damaged files.

use super::*;
use crate::state::AppDirs;
use crate::workspace::WorkspaceRef;

const V1: &str = include_str!("fixtures/session-v1.json");
const V1_AS_V2: &str = include_str!("fixtures/session-v1.expected-v2.json");
const V2: &str = include_str!("fixtures/session-v2.json");
const V3: &str = include_str!("fixtures/session-v3.json");

fn store() -> (tempfile::TempDir, StateStore) {
    let base = tempfile::tempdir().unwrap();
    let store = StateStore::app(&AppDirs::under(base.path()));
    std::fs::create_dir_all(store.dir()).unwrap();
    (base, store)
}

fn write(store: &StateStore, bytes: &[u8]) -> std::path::PathBuf {
    let path = store.dir().join("session.json");
    std::fs::write(&path, bytes).unwrap();
    path
}

fn backups(store: &StateStore) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(store.dir())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().contains("session.json.broken-"))
        .collect()
}

#[test]
fn a_0_6_0_session_is_read_once_and_saved_as_v2() {
    let (_b, store) = store();
    let path = write(&store, V1.as_bytes());
    let file = app_session_file(&store);
    let Loaded::Found(session) = file.load() else {
        panic!("v1 not upgraded");
    };
    // nothing written on read: the old file stays until the first save
    assert_eq!(std::fs::read_to_string(&path).unwrap(), V1);
    file.save(&session).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), V1_AS_V2);
    assert_eq!(backups(&store).len(), 0);
}

#[test]
fn from_v1_keeps_one_place_per_workspace_and_the_first_notes_window() {
    let v1: AppSessionV1 = serde_json::from_value(
        serde_json::from_str::<serde_json::Value>(V1).unwrap()["data"].clone(),
    )
    .unwrap();
    let v2 = from_v1(v1);
    // Alpha twice (other spelling) -> second window dropped; Notes twice -> first kept as the Notes tab
    assert_eq!(v2.windows.len(), 4);
    assert!(v2.notes.open);
    assert_eq!(v2.notes.geometry.map(|g| g.x), Some(1700));
    assert_eq!(v2.windows[2].tabs, []);
    assert_eq!(v2.windows[2].active, None);
    assert!(v2.windows.iter().all(|w| w.tabs.len() <= 1));
    // no Notes in v1 -> Notes closed
    let none = from_v1(AppSessionV1 {
        windows: vec![SavedWindowV1 {
            workspace: None,
            geometry: None,
        }],
    });
    assert_eq!(none.notes, SavedNotes::default());
}

/// Where paths ignore case (Windows, macOS), two spellings of one folder are one workspace; on Linux two.
#[test]
fn from_v1_folds_case_where_the_system_does() {
    let window = |path: &str| SavedWindowV1 {
        workspace: Some(WorkspaceRefDto::Project {
            path: path.to_owned(),
        }),
        geometry: None,
    };
    let v2 = from_v1(AppSessionV1 {
        windows: vec![window(r"C:\Work\Alpha"), window(r"c:\work\alpha")],
    });
    let expected = if crate::paths::fold("A") == "a" { 1 } else { 2 };
    assert_eq!(v2.windows.len(), expected);
}

#[test]
fn a_v2_session_round_trips_byte_for_byte() {
    let (_b, store) = store();
    let path = write(&store, V2.as_bytes());
    let file = app_session_file(&store);
    let Loaded::Found(session) = file.load() else {
        panic!("v2 not read");
    };
    file.save(&session).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), V2);
}

#[test]
fn a_session_of_a_newer_app_is_moved_aside_untouched() {
    let (_b, store) = store();
    write(&store, V3.as_bytes());
    let file = app_session_file(&store);
    assert!(matches!(file.load(), Loaded::Recovered { backup: Some(_) }));
    let kept = backups(&store);
    assert_eq!(kept.len(), 1);
    assert_eq!(std::fs::read(&kept[0]).unwrap(), V3.as_bytes());
    assert_eq!(file.load(), Loaded::Missing);
}

#[test]
fn damaged_session_files_never_panic() {
    let cases: Vec<Vec<u8>> = vec![
        b"".to_vec(),
        b"{".to_vec(),
        b"null".to_vec(),
        b"[]".to_vec(),
        br#"{"version":2}"#.to_vec(),
        br#"{"version":2,"data":{"windows":"x"}}"#.to_vec(),
        br#"{"version":1,"data":{"windows":[{"workspace":{"kind":"what"}}]}}"#.to_vec(),
        br#"{"version":0,"data":{}}"#.to_vec(),
        br#"{"version":-1,"data":{}}"#.to_vec(),
        vec![0xff, 0xfe, 0x00],
    ];
    for bytes in cases {
        let (_b, store) = store();
        write(&store, &bytes);
        assert!(
            matches!(app_session_file(&store).load(), Loaded::Recovered { .. }),
            "{:?}",
            String::from_utf8_lossy(&bytes)
        );
    }
    // a v2 file without the notes entry still loads (the entry defaults)
    let (_b, store) = store();
    write(
        &store,
        br#"{"version":2,"data":{"windows":[{"geometry":null,"tabs":[],"active":null}]}}"#,
    );
    assert!(matches!(app_session_file(&store).load(), Loaded::Found(s) if !s.notes.open));
}

// ---- restore plan ----------------------------------------------------------------------------------------------

fn project(dir: &std::path::Path) -> WorkspaceRefDto {
    WorkspaceRefDto::Project {
        path: dir.display().to_string(),
    }
}

fn window(tabs: Vec<WorkspaceRefDto>, active: Option<u32>) -> SavedWindow {
    SavedWindow {
        geometry: None,
        tabs,
        active,
    }
}

fn plan(session: &AppSession) -> RestorePlan {
    restore_plan(session, |dto| WorkspaceRef::parse(dto).ok())
}

#[test]
fn restore_drops_missing_duplicate_and_notes_tabs() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let gone = a.path().join("gone");
    let session = AppSession {
        windows: vec![
            window(
                vec![
                    project(a.path()),
                    WorkspaceRefDto::Notes,
                    project(&gone),
                    project(b.path()),
                ],
                Some(3),
            ),
            window(
                vec![project(b.path()), project(&a.path().join("."))],
                Some(0),
            ),
        ],
        notes: SavedNotes::default(),
    };
    let plan = plan(&session);
    assert_eq!(plan.windows[0].tabs.len(), 2);
    assert_eq!(plan.windows[0].active, Some(1), "active followed b");
    assert_eq!(plan.windows[0].missing, [gone.display().to_string()]);
    // both tabs of the second window are already open in the first: Welcome, geometry kept
    assert_eq!(plan.windows[1].tabs.len(), 0);
    assert_eq!(plan.windows[1].active, None);
}

#[test]
fn restore_fixes_the_active_tab() {
    let dirs: Vec<tempfile::TempDir> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
    let gone = dirs[0].path().join("gone");
    let tabs = || {
        vec![
            project(dirs[0].path()),
            project(&gone),
            project(dirs[2].path()),
        ]
    };
    let active = |a: Option<u32>| {
        plan(&AppSession {
            windows: vec![window(tabs(), a)],
            notes: SavedNotes::default(),
        })
        .windows[0]
            .active
    };
    assert_eq!(active(Some(0)), Some(0));
    assert_eq!(active(Some(1)), Some(1), "dropped -> its right neighbor");
    assert_eq!(active(Some(99)), Some(1), "out of range -> the last");
    assert_eq!(active(None), Some(0), "null with tabs -> the first");
    // dropped at the end -> the left neighbor
    let left = plan(&AppSession {
        windows: vec![window(
            vec![project(dirs[0].path()), project(&gone)],
            Some(1),
        )],
        notes: SavedNotes::default(),
    });
    assert_eq!(left.windows[0].active, Some(0));
    // nothing left
    let empty = plan(&AppSession {
        windows: vec![window(vec![project(&gone)], Some(0))],
        notes: SavedNotes::default(),
    });
    assert_eq!(empty.windows[0].active, None);
    assert_eq!(plan(&AppSession::default()).windows.len(), 0);
}

#[test]
fn restore_handles_many_tabs_and_odd_paths() {
    let base = tempfile::tempdir().unwrap();
    let odd = base.path().join("a b ação 项目");
    std::fs::create_dir(&odd).unwrap();
    let mut tabs: Vec<WorkspaceRefDto> = (0..1000)
        .map(|i| WorkspaceRefDto::SingleFile {
            path: base
                .path()
                .join(format!("missing-{i}.md"))
                .display()
                .to_string(),
        })
        .collect();
    tabs.push(project(&odd));
    tabs.push(WorkspaceRefDto::Project {
        path: r"\\server-that-is-not-there\share\x".to_owned(),
    });
    let plan = plan(&AppSession {
        windows: vec![window(tabs, Some(500))],
        notes: SavedNotes::default(),
    });
    assert_eq!(plan.windows[0].tabs.len(), 1);
    assert_eq!(plan.windows[0].active, Some(0));
    assert_eq!(plan.windows[0].missing.len(), 1001);
}

#[cfg(windows)]
#[test]
fn restore_takes_a_drive_root() {
    let plan = plan(&AppSession {
        windows: vec![window(
            vec![WorkspaceRefDto::Project {
                path: "C:\\".to_owned(),
            }],
            Some(0),
        )],
        notes: SavedNotes::default(),
    });
    assert_eq!(plan.windows[0].tabs.len(), 1);
}

/// A project session written by 0.6.1: the panels' hidden state (added later) reads as absent (= shown) and is added
/// beside the old keys, which stay as they were.
#[test]
fn a_0_6_1_project_session_takes_the_hidden_panels_beside_its_keys() {
    const OLD: &str = include_str!("fixtures/project-session-0.6.1.json");
    let (_base, store) = store();
    std::fs::write(store.dir().join("session.json"), OLD).unwrap();
    let session = ProjectSession::new(&store);
    assert_eq!(session.get("layout.sidebar-hidden").unwrap(), None);
    assert_eq!(session.get("layout.right-hidden").unwrap(), None);
    assert_eq!(
        session.get("layout.sidebar").unwrap(),
        Some(serde_json::json!(300))
    );
    session
        .set("layout.sidebar-hidden", Some(serde_json::json!(true)))
        .unwrap();
    let old: serde_json::Value = serde_json::from_str(OLD).unwrap();
    let now: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store.dir().join("session.json")).unwrap()).unwrap();
    assert_eq!(now["version"], old["version"], "same format version");
    let mut expected = old["data"]["values"].clone();
    expected["layout.sidebar-hidden"] = serde_json::json!(true);
    assert_eq!(now["data"]["values"], expected, "old keys untouched");
}

const RECENT_0_6: &str = include_str!("fixtures/recent-0.6.json");

/// The recent list 0.6 wrote: shares (WSL folders included) saved as `\\?\UNC\...`, beside a newer plain entry of
/// one of them. Listed plain, one entry per folder (the newest), names fixed; a read writes nothing.
#[test]
fn a_0_6_recent_list_lists_shares_plainly_once() {
    let (_base, store) = store();
    std::fs::write(store.dir().join("recent.json"), RECENT_0_6).unwrap();
    let recent = RecentStore::new(&store);
    let list = listed(recent.load(), |_| true);
    if cfg!(windows) {
        let shown: Vec<(&str, &str, u64)> = list
            .iter()
            .map(|r| (r.path.as_str(), r.name.as_str(), r.opened_ms))
            .collect();
        assert_eq!(
            shown,
            [
                (r"\\wsl.localhost\Ubuntu-24.04\tmp\pm07wsl", "pm07wsl", 300),
                (
                    r"\\wsl.localhost\Ubuntu-24.04\home\edelm\cursos",
                    "cursos",
                    200
                ),
                (r"\\srv\share", r"\\srv\share", 50),
                (r"C:\Users\me\docs", "docs", 10),
            ]
        );
        assert!(list
            .iter()
            .all(|r| !r.path.contains(r"\\?\") && !r.name.contains(r"\\?\")));
    } else {
        // no shares outside Windows: every entry as saved
        assert_eq!(list.len(), 5);
    }
    assert_eq!(recent.list().len(), list.len());
    let bytes = std::fs::read(store.dir().join("recent.json")).unwrap();
    assert_eq!(bytes, RECENT_0_6.as_bytes(), "a read writes nothing");
}

/// Removing a folder by the plain path the list shows removes its old `\\?\UNC\` entries too.
#[cfg(windows)]
#[test]
fn removing_a_listed_share_removes_its_0_6_spellings() {
    let (_base, store) = store();
    std::fs::write(store.dir().join("recent.json"), RECENT_0_6).unwrap();
    let recent = RecentStore::new(&store);
    recent
        .remove(r"\\wsl.localhost\Ubuntu-24.04\tmp\pm07wsl")
        .unwrap();
    recent
        .remove(r"\\wsl.localhost\Ubuntu-24.04\home\edelm\cursos")
        .unwrap();
    let left: Vec<String> = recent.load().entries.into_iter().map(|e| e.path).collect();
    assert_eq!(left, [r"\\?\UNC\srv\share", r"C:\Users\me\docs"]);
}

/// A share opened again (through the loopback admin share: a real one) replaces its 0.6 `\\?\UNC\` entry.
#[cfg(windows)]
#[test]
fn opening_a_share_again_replaces_its_0_6_entry() {
    let dir = tempfile::tempdir().unwrap();
    let Some(shared) = crate::paths::via_admin_share(dir.path()) else {
        eprintln!("skipped: no loopback admin share here");
        return;
    };
    let (_base, store) = store();
    let old = std::fs::canonicalize(&shared)
        .unwrap()
        .display()
        .to_string();
    assert!(old.starts_with(r"\\?\UNC\"));
    let file = serde_json::json!({
        "version": 1,
        "data": { "entries": [{ "path": old, "name": "x", "opened_ms": 1 }] }
    });
    std::fs::write(store.dir().join("recent.json"), file.to_string()).unwrap();
    let recent = RecentStore::new(&store);
    let root = Root::open(&shared).unwrap();
    recent.touch(&root, 2).unwrap();
    let list = recent.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].opened_ms, 2);
    assert!(
        list[0].path.starts_with(r"\\localhost\"),
        "{}",
        list[0].path
    );
    assert!(!list[0].missing);
}

/// A session 0.6 saved with a share that is gone: the open failure names it plainly.
#[cfg(windows)]
#[test]
fn a_missing_0_6_share_is_named_plainly() {
    let session = AppSession {
        windows: vec![SavedWindow {
            geometry: None,
            tabs: vec![WorkspaceRefDto::Project {
                path: r"\\?\UNC\wsl.localhost\Ubuntu-24.04\tmp\gone".to_owned(),
            }],
            active: Some(0),
        }],
        ..AppSession::default()
    };
    let plan = restore_plan(&session, |_| None);
    assert_eq!(
        plan.windows[0].missing,
        [r"\\wsl.localhost\Ubuntu-24.04\tmp\gone"]
    );
}
