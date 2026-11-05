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
        ..ModelFiles::default()
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

/// U5 smoke, run by hand on CPU only: the REAL worker (`PERMARK_WORKER` = release `permark-llm-worker`, debug
/// override) started by `spawn_piped`, over local GGUF files (read only): `PERMARK_SMOKE_GGUF` = embedding model;
/// optional `PERMARK_SMOKE_CHAT_GGUF` = chat model rendered with catalog profile `PERMARK_SMOKE_PROFILE` (default
/// `gemma-4`). `cargo test -p permark-plugin-local-ai smoke -- --ignored --nocapture`
#[test]
#[ignore = "needs a built worker + local model files; run by hand"]
fn smoke_the_real_worker_on_cpu() {
    use crate::engine::EmbedRole;
    let env = |key| std::env::var_os(key).map(PathBuf::from);
    let (Some(_), Some(embed)) = (env("PERMARK_WORKER"), env("PERMARK_SMOKE_GGUF")) else {
        panic!("set PERMARK_WORKER and PERMARK_SMOKE_GGUF");
    };
    let tmp = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(
        Arc::new(WorkerFiles::new(
            None,
            tmp.path().join(files::DIR),
            Arc::new(|_, _| {}),
        )),
        Arc::new(WorkerLog::new(tmp.path().join("worker.log"))),
    );
    let small = Needs {
        ram_min: 1,
        ram_rec: 1,
        vram_rec: 1,
    };
    let cpu = LoadOpts {
        context_len: 2048,
        device: Device::Cpu,
        threads: None,
    };
    let files = |path: &PathBuf| ModelFiles {
        model_id: "smoke".into(),
        catalog_version: 1,
        files: vec![(FileRole::Weights, path.clone())],
        ..ModelFiles::default()
    };
    let caps = runtime.engine().caps();
    println!("caps: {caps:?}");
    assert!(!caps.gpu, "CPU-only build expected here");
    // engine floor from the catalog: refused before any load
    let mut too_new = files(&embed);
    too_new.min_engine = Some("b99999".into());
    let refused = runtime.load("smoke", &small, too_new, cpu, &|_| {}, &CancelToken::new());
    assert!(
        matches!(refused, Err(EngineError::EngineTooOld { .. })),
        "{refused:?}"
    );

    let started = Instant::now();
    let steps = std::sync::Mutex::new(0u32);
    let progress = |_: f32| *steps.lock().unwrap() += 1;
    let session = runtime
        .load(
            "smoke",
            &small,
            files(&embed),
            cpu,
            &progress,
            &CancelToken::new(),
        )
        .unwrap_or_else(|e| panic!("load: {e:?}\n{}", runtime.log_tail()));
    let load = started.elapsed();
    let started = Instant::now();
    let out = runtime
        .engine()
        .embed(
            session,
            &["The northern lights glow green."],
            EmbedRole::Document,
            None,
            true,
            &CancelToken::new(),
        )
        .unwrap();
    let vector = &out.vectors[0];
    let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    println!(
        "embed: dims {} norm {norm:.5} load {load:?} ({} progress steps) embed {:?}",
        vector.len(),
        steps.lock().unwrap(),
        started.elapsed()
    );
    assert_eq!(out.fingerprint.dims as usize, vector.len());
    assert!((norm - 1.0).abs() < 1e-3);
    let short = runtime
        .engine()
        .embed(
            session,
            &["x"],
            EmbedRole::Query,
            Some(128),
            true,
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(short.vectors[0].len(), 128);
    runtime.unload(session);
    assert_eq!(runtime.loaded(), Vec::<String>::new());

    if let Some(chat) = env("PERMARK_SMOKE_CHAT_GGUF") {
        smoke_chat(&runtime, &chat);
    }
}

/// Chat half of [`smoke_the_real_worker_on_cpu`]: plain answer + JSON-schema answer, greedy.
fn smoke_chat(runtime: &Runtime, chat: &std::path::Path) {
    use crate::engine::{ChatMessage, ChatRequest, Constraint, GenEvent, Role, Sampling};
    let small = Needs {
        ram_min: 1,
        ram_rec: 1,
        vram_rec: 1,
    };
    let cpu = LoadOpts {
        context_len: 2048,
        device: Device::Cpu,
        threads: None,
    };
    let name = std::env::var("PERMARK_SMOKE_PROFILE").unwrap_or_else(|_| "gemma-4".into());
    let mut model = ModelFiles {
        model_id: "smoke-chat".into(),
        catalog_version: 1,
        files: vec![(FileRole::Weights, chat.to_path_buf())],
        ..ModelFiles::default()
    };
    model.prompt = crate::catalog::bundled()
        .unwrap()
        .prompt_profiles
        .get(&name)
        .cloned();
    let started = Instant::now();
    let session = runtime
        .load(
            "smoke-chat",
            &small,
            model,
            cpu,
            &|_| {},
            &CancelToken::new(),
        )
        .unwrap_or_else(|e| panic!("load: {e:?}\n{}", runtime.log_tail()));
    let load = started.elapsed();
    let ask = |content: &str, constraint| {
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: Role::User,
                content: content.into(),
            }],
            constraint,
            sampling: Sampling {
                temperature: 0.0,
                ..Sampling::default()
            },
            max_tokens: 40,
            ..ChatRequest::default()
        };
        let (mut answer, mut end) = (String::new(), None);
        let started = Instant::now();
        runtime
            .engine()
            .generate(
                session,
                &request,
                &mut |event| match event {
                    GenEvent::Delta(text) => answer.push_str(&text),
                    GenEvent::Done { usage, finish } => end = Some((usage, finish)),
                    GenEvent::ToolCall { .. } => {}
                },
                &CancelToken::new(),
            )
            .unwrap();
        let (usage, finish) = end.unwrap();
        let secs = started.elapsed().as_secs_f64();
        println!(
            "chat: {finish:?} {usage:?} {:.1} tok/s (load {load:?}): {answer:?}",
            f64::from(usage.completion) / secs
        );
        assert!(usage.completion > 0 && !answer.is_empty());
        answer
    };
    ask("Say hello in one short sentence.", Constraint::None);
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "ok": { "type": "boolean" } },
        "required": ["ok"]
    });
    let json = ask(
        "Is water wet? Answer as JSON.",
        Constraint::JsonSchema(schema),
    );
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed["ok"].is_boolean(), "{json}");
    runtime.unload(session);
}
