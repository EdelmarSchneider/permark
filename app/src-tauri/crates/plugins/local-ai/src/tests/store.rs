//! Store location (AC-7): choice, project boundary, move | keep, gone store, bytes; Remove data (AC-13); orphans.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use permark_core::plugins::{
    FolderReveal, FolderRevealApi, ProjectPlaces, ProjectPlacesApi, ServiceLease, Services,
};
use permark_core::{ErrorKind, IpcError};
use serde_json::{json, Value};

use super::installs::{
    body, catalog, files_in, model_args, refused, serving, url, version_dir, Held, Lab, Run, E2B,
    EMBED, LFM,
};
use crate::catalog::Catalog;
use crate::{plugin_with, ID};

const WHISPER: &str = "whisper-large-v3-turbo-q5_0";
const WHISPER_LINEAGE: &str = "whisper-large-v3-turbo";
const WHISPER_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";
const LFM_LINEAGE: &str = "lfm3-1.7b";

struct Places(Vec<PathBuf>);

impl ProjectPlaces for Places {
    fn folders(&self) -> Vec<PathBuf> {
        self.0.clone()
    }
}

#[derive(Default)]
struct Revealed(Mutex<Vec<PathBuf>>);

impl FolderReveal for Revealed {
    fn reveal(&self, folder: &Path) -> Result<(), IpcError> {
        self.0.lock().unwrap().push(folder.to_path_buf());
        Ok(())
    }
}

/// A run whose app knows `projects` (open | recent) and records reveals.
fn start(lab: &Lab, cat: &Catalog, projects: Vec<PathBuf>, revealed: &Arc<Revealed>) -> Run {
    let revealed = Arc::clone(revealed);
    lab.start_with(
        cat.clone(),
        Arc::new(serving(cat)),
        move |services: &Arc<Services>| -> Vec<ServiceLease> {
            vec![
                services.provide(Arc::new(ProjectPlacesApi(Arc::new(Places(projects))))),
                services.provide(Arc::new(FolderRevealApi(revealed))),
            ]
        },
    )
}

fn set_store(run: &Run, path: Option<&Path>, mode: Option<&str>) -> Result<Value, IpcError> {
    let path = path.map(|p| p.display().to_string());
    run.call("setStore", json!({ "path": path, "mode": mode }))
}

/// Waits until no move runs; the final state.
fn settled(run: &Run) -> Value {
    let start = Instant::now();
    loop {
        let state = run.state();
        if state["storeMove"]["kind"] != "moving" {
            return state;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "stuck: {state}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn install(run: &Run, id: &str) {
    run.call("get", model_args(id)).unwrap();
    run.wait(id, "installed");
}

fn kind_of(state: &Value, id: &str) -> Value {
    state["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["model"] == id)
        .unwrap()["state"]["kind"]
        .clone()
}

fn assigned(state: &Value, task: &str) -> Value {
    state["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["task"] == task)
        .unwrap()["model"]
        .clone()
}

/// Folder outside the profile with a space + non-ASCII letters.
fn elsewhere(lab: &Lab, name: &str) -> PathBuf {
    lab.tmp.path().join("Jo\u{e3}o m\u{f3}delos").join(name)
}

/// AC-7: a store inside | holding an open or recent project, inside the default store, or relative: refused.
#[test]
fn a_store_must_stay_clear_of_projects_and_stores() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let project = lab.tmp.path().join("work").join("Notas");
    std::fs::create_dir_all(&project).unwrap();
    let run = start(&lab, &cat, vec![project.clone()], &Arc::default());
    for inside in [
        project.join("models"),
        project.clone(),
        lab.tmp.path().join("work"),
    ] {
        refused(
            set_store(&run, Some(&inside), None),
            ErrorKind::Denied,
            "insideProject",
        );
    }
    refused(
        set_store(&run, Some(&lab.models_dir().join("sub")), None),
        ErrorKind::Denied,
        "insideStore",
    );
    refused(
        set_store(&run, Some(lab.tmp.path()), None),
        ErrorKind::Denied,
        "insideStore",
    );
    refused(
        run.call("setStore", json!({ "path": "relative/models" })),
        ErrorKind::InvalidPath,
        "notAbsolute",
    );
    assert!(!project.join("models").exists());
    let chosen = elsewhere(&lab, "store");
    let state = set_store(&run, Some(&chosen), None).unwrap();
    assert_eq!(state["store"]["path"], chosen.display().to_string());
    assert_eq!(state["store"]["isDefault"], false);
    assert_eq!(state["store"]["available"], true);
    assert!(chosen.is_dir());
    // back to the default (nothing installed: no mode needed)
    let state = set_store(&run, None, None).unwrap();
    assert_eq!(state["store"]["isDefault"], true);
}

/// AC-7: no store change while a download runs.
#[test]
fn the_store_cannot_change_while_a_download_runs() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let held = Arc::new(Held {
        inner: serving(&cat),
        url: url(&cat, WHISPER, 0),
        open: Mutex::new(false),
        wake: Condvar::new(),
        reached: AtomicBool::new(false),
    });
    let run = lab.start(cat, Arc::clone(&held) as _);
    run.call("get", model_args(WHISPER)).unwrap();
    let start = Instant::now();
    while !held.reached.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(2));
    }
    refused(
        set_store(&run, Some(&elsewhere(&lab, "s")), None),
        ErrorKind::Conflict,
        "downloading",
    );
    held.release();
    run.wait(WHISPER, "installed");
}

/// AC-7: installed models -> mode asked; keep = new empty folder, old files stay; back again = listed again.
#[test]
fn keep_starts_an_empty_store_and_leaves_the_old_files() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let run = start(&lab, &cat, Vec::new(), &Arc::default());
    install(&run, WHISPER);
    run.call("assign", json!({"task": "dictation", "model": WHISPER}))
        .unwrap();
    let chosen = elsewhere(&lab, "keep");
    refused(
        set_store(&run, Some(&chosen), None),
        ErrorKind::Conflict,
        "modeRequired",
    );
    assert!(!chosen.exists());
    let state = set_store(&run, Some(&chosen), Some("keep")).unwrap();
    assert_eq!(kind_of(&state, WHISPER), "notInstalled");
    assert_eq!(assigned(&state, "dictation"), Value::Null);
    assert_eq!(state["store"]["usedBytes"], 0);
    let old = version_dir(&lab, WHISPER_LINEAGE, 1);
    assert!(old.join(WHISPER_FILE).is_file());
    let state = set_store(&run, None, None).unwrap();
    assert_eq!(kind_of(&state, WHISPER), "installed");
    assert_eq!(assigned(&state, "dictation"), WHISPER);
}

/// AC-7: move copies + verifies into the new store (non-ASCII path), then removes the old copy; assignments
/// follow; progress told.
#[test]
fn move_copies_checks_and_removes_the_old_copy() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let run = start(&lab, &cat, Vec::new(), &Arc::default());
    install(&run, WHISPER);
    run.call("acceptLicense", model_args(LFM)).unwrap();
    install(&run, LFM);
    run.call("assign", json!({"task": "dictation", "model": WHISPER}))
        .unwrap();
    run.call("assign", json!({"task": "assistant", "model": LFM}))
        .unwrap();
    let used = run.state()["store"]["usedBytes"].clone();
    let chosen = elsewhere(&lab, "moved");
    set_store(&run, Some(&chosen), Some("move")).unwrap();
    let state = settled(&run);
    assert_eq!(state["storeMove"], Value::Null);
    assert_eq!(state["store"]["path"], chosen.display().to_string());
    assert_eq!(kind_of(&state, WHISPER), "installed");
    assert_eq!(kind_of(&state, LFM), "installed");
    assert_eq!(assigned(&state, "dictation"), WHISPER);
    assert_eq!(assigned(&state, "assistant"), LFM);
    assert_eq!(state["store"]["usedBytes"], used);
    let dir = chosen.join(WHISPER_LINEAGE).join("1");
    assert_eq!(
        files_in(&dir),
        [WHISPER_FILE.to_owned(), format!("{WHISPER}.installed.json")]
    );
    assert_eq!(
        std::fs::read(dir.join(WHISPER_FILE)).unwrap(),
        body(WHISPER, WHISPER_FILE, 1)
    );
    assert!(!lab.models_dir().join(WHISPER_LINEAGE).exists());
    assert!(!lab.models_dir().join(LFM_LINEAGE).exists());
    let names = run.event_names();
    assert!(names.iter().any(|n| n == "local-ai:move"));
}

/// AC-7: a copy that fails its check -> move failed, copies removed, old store in use and intact.
#[test]
fn a_corrupted_copy_fails_and_keeps_the_old_store() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let run = start(&lab, &cat, Vec::new(), &Arc::default());
    install(&run, WHISPER);
    let old = version_dir(&lab, WHISPER_LINEAGE, 1).join(WHISPER_FILE);
    let mut bytes = std::fs::read(&old).unwrap();
    bytes[3] ^= 0xff;
    std::fs::write(&old, &bytes).unwrap();
    let chosen = elsewhere(&lab, "bad");
    set_store(&run, Some(&chosen), Some("move")).unwrap();
    let state = settled(&run);
    assert_eq!(state["storeMove"]["kind"], "failed");
    assert_eq!(state["storeMove"]["error"]["kind"], "integrity");
    assert_eq!(state["store"]["isDefault"], true);
    assert_eq!(kind_of(&state, WHISPER), "installed");
    assert_eq!(std::fs::read(&old).unwrap(), bytes);
    assert!(!chosen.join(WHISPER_LINEAGE).exists());
    // the next change clears the failure
    let state = set_store(&run, Some(&chosen), Some("keep")).unwrap();
    assert_eq!(state["storeMove"], Value::Null);
}

/// AC-7: cancel mid-copy -> copies removed, old store in use; installs refused while moving.
#[test]
fn a_move_can_be_cancelled() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let run = start(&lab, &cat, Vec::new(), &Arc::default());
    install(&run, WHISPER);
    run.events.1.close();
    let chosen = elsewhere(&lab, "cancel");
    let state = set_store(&run, Some(&chosen), Some("move")).unwrap();
    assert_eq!(state["storeMove"]["kind"], "moving");
    let start = Instant::now();
    while !run.events.1.reached.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(2));
    }
    refused(
        run.call("get", model_args(LFM)),
        ErrorKind::Conflict,
        "moving",
    );
    refused(set_store(&run, None, None), ErrorKind::Conflict, "moving");
    run.call("cancelMove", json!({})).unwrap();
    run.events.1.open();
    let state = settled(&run);
    assert_eq!(state["storeMove"], Value::Null);
    assert_eq!(state["store"]["isDefault"], true);
    assert_eq!(kind_of(&state, WHISPER), "installed");
    assert!(!chosen.join(WHISPER_LINEAGE).exists());
    assert!(version_dir(&lab, WHISPER_LINEAGE, 1)
        .join(WHISPER_FILE)
        .is_file());
}

/// AC-7: a chosen store that is gone -> unavailable (not "not installed"), nothing made there; back -> listed.
#[test]
fn a_gone_store_is_unavailable_not_empty() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let run = start(&lab, &cat, Vec::new(), &Arc::default());
    let chosen = elsewhere(&lab, "drive");
    set_store(&run, Some(&chosen), None).unwrap();
    install(&run, WHISPER);
    let parked = elsewhere(&lab, "parked");
    std::fs::rename(&chosen, &parked).unwrap();
    let state = run.state();
    assert_eq!(state["store"]["available"], false);
    assert_eq!(state["store"]["freeBytes"], Value::Null);
    assert!(state["models"]
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["state"]["kind"] == "storeUnavailable"));
    let error = run.call("get", model_args(EMBED)).unwrap_err();
    assert_eq!(
        (error.kind, error.subject.as_deref()),
        (ErrorKind::Unavailable, Some("store"))
    );
    assert!(!chosen.exists());
    std::fs::rename(&parked, &chosen).unwrap();
    let state = run.state();
    assert_eq!(state["store"]["available"], true);
    assert_eq!(kind_of(&state, WHISPER), "installed");
}

/// Storage card: free, used, partial bytes.
#[test]
fn the_store_tells_free_used_and_partial_bytes() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let run = start(&lab, &cat, Vec::new(), &Arc::default());
    let fresh = run.state();
    assert!(fresh["store"]["freeBytes"].as_u64().unwrap() > 0);
    assert_eq!(fresh["store"]["usedBytes"], 0);
    install(&run, WHISPER);
    let whisper: u64 = cat
        .model(WHISPER)
        .unwrap()
        .files
        .iter()
        .map(|f| f.bytes)
        .sum();
    let state = run.state();
    assert_eq!(state["store"]["usedBytes"], whisper);
    assert_eq!(state["store"]["partialBytes"], 0);
    drop(run);

    // a paused download: its kept bytes
    let lab = Lab::new();
    let held = Arc::new(Held {
        inner: serving(&cat),
        url: url(&cat, E2B, 1),
        open: Mutex::new(false),
        wake: Condvar::new(),
        reached: AtomicBool::new(false),
    });
    let run = lab.start(cat.clone(), Arc::clone(&held) as _);
    run.call("get", model_args(E2B)).unwrap();
    let start = Instant::now();
    while !held.reached.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(2));
    }
    run.call("cancel", model_args(E2B)).unwrap();
    held.release();
    let paused = run.wait(E2B, "paused");
    let done = paused["state"]["done"].as_u64().unwrap();
    assert!(done > 0);
    assert_eq!(run.state()["store"]["partialBytes"], done);
}

/// Store start: version folders of catalog lineages neither in the catalog nor recorded are removed; anything else
/// stays.
#[test]
fn orphan_version_folders_are_removed_at_store_start() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let store = lab.models_dir();
    let lineage = store.join(WHISPER_LINEAGE);
    for dir in [
        lineage.join("7"),
        lineage.join("1"),
        lineage.join("notes"),
        lineage.join("007"),
        store.join("someone-else").join("3"),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f"), b"x").unwrap();
    }
    let run = start(&lab, &cat, Vec::new(), &Arc::default());
    run.state();
    assert!(!lineage.join("7").exists());
    assert!(lineage.join("1").join("f").is_file());
    assert!(lineage.join("notes").join("f").is_file());
    assert!(lineage.join("007").join("f").is_file());
    assert!(store.join("someone-else").join("3").join("f").is_file());
}

/// AC-13: Remove data tells the bytes, then removes the models of a chosen store ONLY (its other files stay);
/// default store: left to core (its namespace).
#[test]
fn remove_data_removes_the_models_of_a_chosen_store_only() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let mut run = start(&lab, &cat, Vec::new(), &Arc::default());
    let chosen = elsewhere(&lab, "mine");
    std::fs::create_dir_all(chosen.join("photos")).unwrap();
    std::fs::write(chosen.join("photos").join("a.jpg"), b"jpg").unwrap();
    set_store(&run, Some(&chosen), None).unwrap();
    install(&run, WHISPER);
    std::fs::write(chosen.join(WHISPER_LINEAGE).join("read me.txt"), b"x").unwrap();
    run.stop();
    let plugin = plugin_with(cat.clone());
    let app = lab.store();
    let whisper: u64 = cat
        .model(WHISPER)
        .unwrap()
        .files
        .iter()
        .map(|f| f.bytes)
        .sum();
    let told = plugin.data_bytes(&app).unwrap();
    assert!(told >= whisper && told < whisper + 4096, "{told}");
    plugin.remove_data(&app).unwrap();
    assert!(!chosen.join(WHISPER_LINEAGE).join("1").exists());
    assert!(chosen.join(WHISPER_LINEAGE).join("read me.txt").is_file());
    assert!(chosen.join("photos").join("a.jpg").is_file());
    // core then deletes the namespace (state, default store)
    app.remove_plugin_data(&ID).unwrap();
    assert!(!app.plugin_dir(&ID).exists());

    // default store: nothing outside the namespace
    let lab = Lab::new();
    let mut run = start(&lab, &cat, Vec::new(), &Arc::default());
    install(&run, WHISPER);
    run.stop();
    let app = lab.store();
    assert!(plugin.data_bytes(&app).unwrap() >= whisper);
    plugin.remove_data(&app).unwrap();
    assert!(version_dir(&lab, WHISPER_LINEAGE, 1).is_dir());
}

/// Show in folder: the plugin's own store only, once it exists.
#[test]
fn reveal_shows_the_store_folder() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let revealed = Arc::new(Revealed::default());
    let run = start(&lab, &cat, Vec::new(), &revealed);
    let error = run.call("revealStore", json!({})).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unavailable);
    install(&run, WHISPER);
    run.call("revealStore", json!({})).unwrap();
    assert_eq!(*revealed.0.lock().unwrap(), [lab.models_dir()]);
}
