//! Runtime over in-process `serve(FakeEngine)` workers (no process, no model): dormant, guard, release per lineage,
//! drop ends every worker, capped log.

use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::*;
use crate::engine::files::{self, WorkerFiles};
use crate::engine::tests::Workers;
use crate::engine::{Device, FakeEngine, FileRole};
use crate::tests::entries;

const GIB: u64 = 1 << 30;

fn memory(available: u64) -> MachineProfile {
    MachineProfile {
        ram_total: Some(64 * GIB),
        ram_available: Some(available),
        cpu_logical: Some(8),
        gpus: Vec::new(),
        os: "test".into(),
    }
}

fn plenty() -> MachineProfile {
    memory(32 * GIB)
}

fn scarce() -> MachineProfile {
    memory(GIB)
}

fn needs() -> Needs {
    Needs {
        ram_min: 2 * GIB,
        ram_rec: 3 * GIB,
        vram_rec: 3 * GIB,
    }
}

fn model(id: &str) -> ModelFiles {
    ModelFiles {
        model_id: id.to_owned(),
        catalog_version: 1,
        files: vec![(FileRole::Weights, PathBuf::from("w.gguf"))],
        prefix_profile: None,
    }
}

fn opts() -> LoadOpts {
    LoadOpts {
        context_len: 512,
        device: Device::Cpu,
        threads: Some(1),
    }
}

fn rig(machine: fn() -> MachineProfile, log: PathBuf) -> (Runtime, Workers) {
    let workers = Workers::default();
    let runtime = Runtime::with_launch(
        workers.launch(FakeEngine::new),
        machine,
        Arc::new(WorkerLog::new(log)),
    );
    (runtime, workers)
}

fn load(runtime: &Runtime, lineage: &str) -> Result<SessionId, EngineError> {
    runtime.load(
        lineage,
        &needs(),
        model(lineage),
        opts(),
        &|_| {},
        &CancelToken::new(),
    )
}

/// AC-8: a runtime (pinned worker files included) creates, spawns, starts nothing until a load.
#[test]
fn a_new_runtime_is_dormant() {
    let tmp = tempfile::tempdir().unwrap();
    let (runtime, workers) = rig(plenty, tmp.path().join("worker.log"));
    let real = Runtime::new(
        Arc::new(WorkerFiles::new(
            None,
            tmp.path().join(files::DIR),
            Arc::new(|_, _| {}),
        )),
        Arc::new(WorkerLog::new(tmp.path().join("worker.log"))),
    );
    runtime.release_all();
    drop((runtime, real));
    assert_eq!(workers.launched(), 0);
    assert_eq!(entries(tmp.path()), Vec::<String>::new());
}

/// Real launch, worker not installed: load fails (IO), nothing spawned | created.
#[test]
fn a_missing_worker_fails_the_load() {
    let tmp = tempfile::tempdir().unwrap();
    let files = WorkerFiles::with_pin(None, None, tmp.path().join(files::DIR), Arc::new(|_, _| {}));
    let runtime = Runtime::new(
        Arc::new(files),
        Arc::new(WorkerLog::new(tmp.path().join("worker.log"))),
    );
    assert!(matches!(load(&runtime, "a"), Err(EngineError::Io(_))));
    assert_eq!(runtime.loaded(), Vec::<String>::new());
    assert_eq!(entries(tmp.path()), Vec::<String>::new());
}

/// AC-5: too little memory -> refused before any worker starts.
#[test]
fn the_guard_refuses_before_a_worker_starts() {
    let tmp = tempfile::tempdir().unwrap();
    let (runtime, workers) = rig(scarce, tmp.path().join("worker.log"));
    assert_eq!(load(&runtime, "a"), Err(EngineError::OutOfMemory));
    assert_eq!(workers.launched(), 0);
}

/// D-11: release(lineage) unloads that lineage's sessions only (their workers end); others keep working.
#[test]
fn release_unloads_every_session_of_a_lineage() {
    let tmp = tempfile::tempdir().unwrap();
    let (runtime, workers) = rig(plenty, tmp.path().join("worker.log"));
    let a1 = load(&runtime, "a").unwrap();
    let a2 = load(&runtime, "a").unwrap();
    let b = load(&runtime, "b").unwrap();
    assert_eq!(runtime.loaded(), ["a", "b"]);
    runtime.release("a");
    assert_eq!(workers.kills(), 2);
    assert_eq!(runtime.loaded(), ["b"]);
    for gone in [a1, a2] {
        assert_eq!(
            runtime.engine().count_tokens(gone, "x"),
            Err(EngineError::NotLoaded)
        );
    }
    assert_eq!(runtime.engine().count_tokens(b, "x y"), Ok(2));
    runtime.release("none");
    assert_eq!(workers.kills(), 2);
    runtime.unload(b);
    assert_eq!(runtime.loaded(), Vec::<String>::new());
    assert_eq!(workers.kills(), 3);
}

/// Plugin off: dropping the runtime ends every worker.
#[test]
fn drop_ends_every_worker() {
    let tmp = tempfile::tempdir().unwrap();
    let (runtime, workers) = rig(plenty, tmp.path().join("worker.log"));
    load(&runtime, "a").unwrap();
    load(&runtime, "b").unwrap();
    drop(runtime);
    assert_eq!((workers.launched(), workers.kills()), (2, 2));
}

/// Worker stderr: capped in memory, tail = newest lines, file written (atomic) when a worker's stderr ends.
#[test]
fn the_log_keeps_capped_lines_and_saves_them_when_a_worker_ends() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("logs").join("worker.log");
    let log = Arc::new(WorkerLog::new(path.clone()));
    let line = "x".repeat(1000);
    for _ in 0..400 {
        log.add(&line);
    }
    log.add("last words");
    let tail = log.tail();
    assert!(tail.ends_with("\nlast words"), "newest last");
    assert!(tail.len() <= TAIL_CAP, "{}", tail.len());
    assert!(!path.exists(), "nothing written while workers run");
    log.save();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.len() <= LOG_CAP && on_disk.ends_with("last words\n"));

    // drained from a stream: CRLF trimmed, saved at its end
    let drained = Arc::new(WorkerLog::new(tmp.path().join("drained.log")));
    drained
        .drain(Cursor::new(b"loading\r\ncrashed: bad tensor\n".to_vec()))
        .unwrap();
    let started = Instant::now();
    let saved = tmp.path().join("drained.log");
    while std::fs::read_to_string(&saved).ok().as_deref() != Some("loading\ncrashed: bad tensor\n")
    {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(drained.tail(), "loading\ncrashed: bad tensor");
}
