//! Model service (feature-correction U1) over in-process `serve(FakeEngine)` workers + scripted downloads: open by
//! task -> no model (`noModel`) | engine files missing (fetched with progress, cancel stops it; no pin:
//! `notAvailable`) | session (context capped, drop unloads);
//! `is_assigned` from the state alone; service offered while the plugin is on, gone after off.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use permark_core::downloads::{ScriptedTransport, Transport};
use permark_core::{ErrorKind, IpcError};

use super::installs::{catalog, serving, Held, Lab, E2B};
use super::testrun::{engine_files, plenty, Bench};
use crate::engine::files::tests::{pin, url};
use crate::engine::{files, CancelToken, EngineError, FakeEngine};
use crate::host::{HostError, HostProgress, ModelHost, ModelSession};
use crate::tasks::Task;

const E4B: &str = "gemma-4-e4b-it-q4_0";

fn open(bench: &Bench, context_len: u32) -> Result<ModelSession, HostError> {
    bench
        .host
        .open(Task::Correction, context_len, &|_| {}, &CancelToken::new())
}

fn engine_dir(bench: &Bench) -> std::path::PathBuf {
    bench.lab.store().plugin_dir(&crate::ID).join(files::DIR)
}

/// Engine files of `bench` installed: the opens that follow fetch nothing.
fn engine_ready(bench: &Bench) {
    bench.host.files.enqueue().unwrap();
    let start = Instant::now();
    while !bench.host.files.is_installed() {
        assert!(start.elapsed() < Duration::from_secs(10), "engine files");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn ipc(error: HostError) -> (ErrorKind, Option<String>, Option<String>) {
    let ipc = IpcError::from(error);
    (ipc.kind, ipc.subject, ipc.detail)
}

/// Nothing assigned to the task: typed `noModel`, nothing started.
#[test]
fn open_without_an_assigned_model_is_no_model() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    let error = open(&bench, 4096).unwrap_err();
    assert!(
        matches!(error, HostError::NoModel(Task::Correction)),
        "{error:?}"
    );
    assert_eq!(
        ipc(error),
        (
            ErrorKind::Unavailable,
            Some("model".into()),
            Some("noModel".into())
        )
    );
    assert_eq!(bench.workers.launched(), 0);
}

/// AC-1 dormancy: assignment read from the state alone (no worker, no engine folder).
#[test]
fn is_assigned_reads_the_state_alone() {
    let bench = Bench::fake(FakeEngine::new);
    assert!(!bench.host.is_assigned(Task::Correction));
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    assert!(bench.host.is_assigned(Task::Correction));
    assert!(bench.installs.is_assigned(Task::Correction));
    assert!(!bench.host.is_assigned(Task::Translation));
    bench.installs.assign(Task::Correction, None).unwrap();
    assert!(!bench.host.is_assigned(Task::Correction));
    assert_eq!(bench.workers.launched(), 0);
    assert!(!engine_dir(&bench).exists());
}

/// AC-8: engine files missing -> fetched by the open itself (progress: engine bytes, then loading from 0), then the
/// model loads; a second open fetches nothing.
#[test]
fn open_fetches_a_missing_engine_then_loads() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    assert!(!bench.host.files.is_installed());
    let seen = Mutex::new(Vec::new());
    let session = bench
        .host
        .open(
            Task::Correction,
            512,
            &|p| seen.lock().unwrap().push(p),
            &CancelToken::new(),
        )
        .unwrap();
    assert!(bench.host.files.is_installed());
    let seen = seen.into_inner().unwrap();
    let engine: Vec<_> = seen
        .iter()
        .filter_map(|p| match p {
            HostProgress::Engine { received, total } => Some((*received, *total)),
            HostProgress::Loading(_) => None,
        })
        .collect();
    let last = engine.last().copied().expect("engine progress");
    assert!(last.1 > 0 && last.0 == last.1, "{engine:?}");
    let first_load = seen
        .iter()
        .position(|p| matches!(p, HostProgress::Loading(_)))
        .unwrap();
    assert_eq!(seen[first_load], HostProgress::Loading(0.0));
    assert!(
        seen[first_load..]
            .iter()
            .all(|p| matches!(p, HostProgress::Loading(_))),
        "engine before loading: {seen:?}"
    );
    assert_eq!(bench.workers.launched(), 1);
    drop(session);
    let again = Mutex::new(Vec::new());
    let session = bench
        .host
        .open(
            Task::Correction,
            512,
            &|p| again.lock().unwrap().push(p),
            &CancelToken::new(),
        )
        .unwrap();
    assert!(!again
        .into_inner()
        .unwrap()
        .iter()
        .any(|p| matches!(p, HostProgress::Engine { .. })));
    drop(session);
    bench.unloaded();
}

/// AC-8: cancel while the engine downloads -> cancelled (engine code `cancelled`), nothing loaded | started.
#[test]
fn cancel_during_the_engine_download_cancels_the_open() {
    let held = Arc::new(Held {
        inner: engine_files(),
        url: url(&pin().files[0]),
        open: Mutex::new(false),
        wake: Condvar::new(),
        reached: AtomicBool::new(false),
    });
    let bench = Bench::new(
        FakeEngine::new,
        plenty,
        Some(pin()),
        Arc::clone(&held) as Arc<dyn Transport>,
    );
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    let cancel = CancelToken::new();
    let opening = {
        let (host, cancel) = (Arc::clone(&bench.host), cancel.clone());
        std::thread::spawn(move || host.open(Task::Correction, 512, &|_| {}, &cancel).map(drop))
    };
    let start = Instant::now();
    while !held.reached.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    cancel.cancel();
    let error = opening.join().unwrap().unwrap_err();
    held.release();
    assert_eq!(
        ipc(error),
        (
            ErrorKind::Superseded,
            Some("engine".into()),
            Some("cancelled".into())
        )
    );
    assert_eq!(bench.workers.launched(), 0);
    assert_eq!(bench.runtime.loaded(), Vec::<String>::new());
}

/// No worker built for this computer: typed `notAvailable`, nothing fetched | started.
#[test]
fn open_without_an_engine_for_this_computer_is_not_available() {
    let bench = Bench::new(
        FakeEngine::new,
        plenty,
        None,
        Arc::new(ScriptedTransport::new()),
    );
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    let error = open(&bench, 4096).unwrap_err();
    assert_eq!(
        ipc(error),
        (
            ErrorKind::Unavailable,
            Some("engine".into()),
            Some("notAvailable".into())
        )
    );
    assert_eq!(bench.workers.launched(), 0);
    assert!(!engine_dir(&bench).exists(), "nothing fetched");
}

/// Success: the assigned model loads (context capped at the asked size), the session runs; drop unloads.
#[test]
fn open_loads_the_assigned_model_and_drop_unloads() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    engine_ready(&bench);
    let session = open(&bench, 512).unwrap();
    assert!(session.on_gpu());
    assert!(session.memory_bytes().is_some_and(|m| m > 0));
    assert_eq!(session.engine().context_len(session.id()).unwrap(), 512);
    assert!(
        session
            .engine()
            .count_tokens(session.id(), "Hello")
            .unwrap()
            > 0
    );
    assert_eq!(bench.runtime.loaded().len(), 1);
    assert_eq!(bench.workers.launched(), 1);
    drop(session);
    bench.unloaded();
}

/// AC-1 teardown: `ModelHost` offered as an app service while on; plugin off withdraws it.
#[test]
fn the_plugin_offers_the_model_host_while_on() {
    let lab = Lab::new();
    let cat = catalog(1, |_| {});
    let mut offered = None;
    let mut run = lab.start_with(cat.clone(), Arc::new(serving(&cat)), |services| {
        offered = Some(Arc::clone(services));
        Vec::new()
    });
    let services = offered.unwrap();
    let host = services.get::<ModelHost>().expect("offered while on");
    assert!(!host.is_assigned(Task::Correction));
    drop(host);
    run.stop();
    assert!(services.get::<ModelHost>().is_none(), "gone after off");
}

/// Warm sessions of a feature (plugin `correction`): `serves` = the session still carries the model assigned to the
/// task (state alone); reassigning | unassigning ends it.
#[test]
fn serves_follows_the_assignment() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    bench.install(E4B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    engine_ready(&bench);
    let session = open(&bench, 512).unwrap();
    assert!(bench.host.serves(&session, Task::Correction));
    assert!(!bench.host.serves(&session, Task::Translation));
    bench.installs.assign(Task::Correction, Some(E4B)).unwrap();
    assert!(!bench.host.serves(&session, Task::Correction));
    bench.installs.assign(Task::Correction, None).unwrap();
    assert!(!bench.host.serves(&session, Task::Correction));
}

/// Models load where the person chose: the graphics card when there is one, or the processor only (the fake engine
/// places a load on its graphics card unless the load options say processor).
#[test]
fn open_loads_on_the_processor_when_the_person_chose_so() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    engine_ready(&bench);
    assert!(open(&bench, 512).unwrap().on_gpu());
    assert!(bench.installs.set_cpu_only(true).unwrap().cpu_only);
    let cpu = open(&bench, 512).unwrap();
    assert!(!cpu.on_gpu());
    assert!(cpu.memory_bytes().is_some_and(|m| m > 0));
    drop(cpu);
    bench.unloaded();
}

/// A session loaded under the other choice never keeps serving: unloaded under its holder when the choice changes
/// (as on a store change) and no longer `serves`; the next open loads where the person now chose. Choosing what is
/// chosen leaves it alone.
#[test]
fn a_changed_choice_unloads_what_was_loaded_under_the_other() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    engine_ready(&bench);
    let session = open(&bench, 512).unwrap();
    bench.installs.set_cpu_only(false).unwrap();
    assert!(bench.host.serves(&session, Task::Correction), "same choice");
    assert_eq!(bench.runtime.loaded().len(), 1);

    bench.installs.set_cpu_only(true).unwrap();
    assert!(!bench.host.serves(&session, Task::Correction));
    assert_eq!(
        session.engine().count_tokens(session.id(), "Hello"),
        Err(EngineError::NotLoaded)
    );
    bench.unloaded();
    let next = open(&bench, 512).unwrap();
    assert!(!next.on_gpu());
    assert!(bench.host.serves(&next, Task::Correction));

    bench.installs.set_cpu_only(false).unwrap();
    assert!(!bench.host.serves(&next, Task::Correction));
    bench.unloaded();
    assert!(open(&bench, 512).unwrap().on_gpu());
}
