//! Model service (feature-correction U1) over in-process `serve(FakeEngine)` workers: open by task -> no model
//! (`noModel`) | engine files missing (`notAvailable`, nothing fetched) | session (context capped, drop unloads);
//! `is_assigned` from the state alone; service offered while the plugin is on, gone after off.

use std::sync::Arc;
use std::time::{Duration, Instant};

use permark_core::{ErrorKind, IpcError};

use super::installs::{catalog, serving, Lab, E2B};
use super::testrun::Bench;
use crate::engine::{files, CancelToken, FakeEngine};
use crate::host::{HostError, ModelHost, ModelSession};
use crate::tasks::Task;

fn open(bench: &Bench, context_len: u32) -> Result<ModelSession, HostError> {
    bench
        .host
        .open(Task::Correction, context_len, &|_| {}, &CancelToken::new())
}

fn engine_dir(bench: &Bench) -> std::path::PathBuf {
    bench.lab.store().plugin_dir(&crate::ID).join(files::DIR)
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

/// Correction never downloads: engine files missing -> `notAvailable`, no download enqueued, no worker.
#[test]
fn open_never_fetches_a_missing_engine() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    let error = open(&bench, 4096).unwrap_err();
    assert!(matches!(error, HostError::NoEngine), "{error:?}");
    assert_eq!(
        ipc(error),
        (
            ErrorKind::Unavailable,
            Some("engine".into()),
            Some("notAvailable".into())
        )
    );
    assert_eq!(bench.workers.launched(), 0);
    assert!(!engine_dir(&bench).exists(), "no download enqueued");
}

/// Success: the assigned model loads (context capped at the asked size), the session runs; drop unloads.
#[test]
fn open_loads_the_assigned_model_and_drop_unloads() {
    let bench = Bench::fake(FakeEngine::new);
    bench.install(E2B);
    bench.installs.assign(Task::Correction, Some(E2B)).unwrap();
    bench.host.files.enqueue().unwrap();
    let start = Instant::now();
    while !bench.host.files.is_installed() {
        assert!(start.elapsed() < Duration::from_secs(10), "engine files");
        std::thread::sleep(Duration::from_millis(5));
    }
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
