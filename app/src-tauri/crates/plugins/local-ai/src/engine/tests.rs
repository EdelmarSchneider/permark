//! Contract cases on [`FakeEngine`] AND on [`WorkerEngine`] serving it over OS pipes (one in-process `serve` thread
//! per "worker"; no process, no model).

use std::cell::RefCell;
use std::io::{self, BufRead, BufReader, PipeWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::catalog::ModelFile;

/// Worker stdout that breaks once the worker is "killed" (serve then returns: host sees EOF).
struct Dying {
    inner: PipeWriter,
    dead: Arc<AtomicBool>,
}

impl Write for Dying {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.dead.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Workers = `serve(make())` threads: launch counts, kill counts, kill switch of the latest (plugin tests building
/// their own host share it).
#[derive(Clone, Default)]
pub(crate) struct Workers {
    launched: Arc<AtomicUsize>,
    kills: Arc<AtomicUsize>,
    /// Kill switch of the latest worker.
    last: Arc<Mutex<Option<Arc<AtomicBool>>>>,
}

impl Workers {
    /// Launch of one `serve(make())` worker over OS pipes.
    pub(crate) fn launch(&self, make: fn() -> FakeEngine) -> Launch {
        let rig = self.clone();
        Box::new(move || {
            let (worker_in, to_worker) = io::pipe()?;
            let (from_worker, worker_out) = io::pipe()?;
            let dead = Arc::new(AtomicBool::new(false));
            let out = Dying {
                inner: worker_out,
                dead: Arc::clone(&dead),
            };
            std::thread::spawn(move || serve(BufReader::new(worker_in), out, &make(), || {}));
            rig.launched.fetch_add(1, Ordering::Relaxed);
            *rig.last.lock().unwrap() = Some(Arc::clone(&dead));
            let killed = Arc::clone(&rig.kills);
            Ok(WorkerPipes {
                to_worker: Box::new(to_worker),
                from_worker: Box::new(BufReader::new(from_worker)),
                kill: Box::new(move || {
                    dead.store(true, Ordering::Relaxed);
                    killed.fetch_add(1, Ordering::Relaxed);
                }),
            })
        })
    }

    /// The latest worker dies (its next write fails).
    pub(crate) fn crash(&self) {
        self.last
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .store(true, Ordering::Relaxed);
    }

    pub(crate) fn kills(&self) -> usize {
        self.kills.load(Ordering::Relaxed)
    }

    pub(crate) fn launched(&self) -> usize {
        self.launched.load(Ordering::Relaxed)
    }
}

/// [`WorkerEngine`] over [`Workers`].
struct Rig {
    engine: WorkerEngine,
    workers: Workers,
}

impl Rig {
    fn new(make: fn() -> FakeEngine) -> Self {
        let workers = Workers::default();
        Self {
            engine: WorkerEngine::new(workers.launch(make)),
            workers,
        }
    }

    fn crash(&self) {
        self.workers.crash();
    }

    fn kills(&self) -> usize {
        self.workers.kills()
    }

    fn launched(&self) -> usize {
        self.workers.launched()
    }
}

/// Same fake, in process and behind a worker pipe.
fn both(make: fn() -> FakeEngine) -> [Box<dyn Engine>; 2] {
    [Box::new(make()), Box::new(Rig::new(make).engine)]
}

/// Fake waiting for cancel after each streamed item: cancel lands deterministically through the pipe.
fn paced() -> FakeEngine {
    FakeEngine::new().paced(Duration::from_secs(10))
}

fn files(id: &str, prefix: Option<&str>) -> ModelFiles {
    ModelFiles {
        model_id: id.to_owned(),
        catalog_version: 3,
        files: vec![(FileRole::Weights, PathBuf::from("w.gguf"))],
        prefix_profile: prefix.map(str::to_owned),
        ..ModelFiles::default()
    }
}

fn opts() -> LoadOpts {
    LoadOpts {
        context_len: 2048,
        device: Device::Auto,
        threads: None,
    }
}

fn loaded(engine: &dyn Engine) -> SessionId {
    engine
        .load(
            files("embed-1", Some("gemma")),
            opts(),
            &|_| {},
            &CancelToken::new(),
        )
        .unwrap()
}

fn user(text: &str) -> ChatRequest {
    ChatRequest {
        messages: vec![
            ChatMessage {
                role: Role::System,
                content: "be brief".into(),
            },
            ChatMessage {
                role: Role::User,
                content: text.into(),
            },
        ],
        max_tokens: 100,
        ..ChatRequest::default()
    }
}

fn run(engine: &dyn Engine, session: SessionId, request: &ChatRequest) -> Vec<GenEvent> {
    let mut events = Vec::new();
    engine
        .generate(
            session,
            request,
            &mut |e| events.push(e),
            &CancelToken::new(),
        )
        .unwrap();
    events
}

/// AC-10: features hold the engine as a trait object, shared across threads.
#[test]
fn the_engine_is_object_safe_and_shareable() {
    for engine in both(FakeEngine::new) {
        let shared: Arc<dyn Engine> = Arc::from(engine);
        let other = Arc::clone(&shared);
        let session = std::thread::spawn(move || loaded(other.as_ref()))
            .join()
            .unwrap();
        assert_eq!(shared.context_len(session).unwrap(), 2048);
        assert!(shared.caps().formats.contains(&"gguf".to_owned()));
    }
}

#[test]
fn load_reports_progress_and_unload_forgets_the_session() {
    for engine in both(FakeEngine::new) {
        let seen = RefCell::new(Vec::new());
        let session = engine
            .load(
                files("m", None),
                opts(),
                &|p| seen.borrow_mut().push(p),
                &CancelToken::new(),
            )
            .unwrap();
        let seen = seen.into_inner();
        assert_eq!(seen.first(), Some(&0.0));
        assert_eq!(seen.last(), Some(&1.0));
        assert!(seen.windows(2).all(|w| w[0] <= w[1]));
        engine.unload(session);
        assert_eq!(engine.context_len(session), Err(EngineError::NotLoaded));
    }
    let engine = FakeEngine::new();
    let session = loaded(&engine);
    assert_eq!(engine.loaded(), 1);
    engine.unload(session);
    assert_eq!(engine.loaded(), 0);
}

#[test]
fn load_refuses_files_without_weights() {
    for engine in both(|| FakeEngine::with_caps(vec![Capability::Chat], false)) {
        let mut no_weights = files("m", None);
        no_weights.files = vec![(FileRole::Mmproj, PathBuf::from("p.gguf"))];
        assert_eq!(
            engine.load(no_weights.clone(), opts(), &|_| {}, &CancelToken::new()),
            Err(EngineError::Corrupt)
        );
        assert_eq!(
            engine.estimate_memory(&no_weights, &opts()),
            Err(EngineError::Corrupt)
        );
    }
    let engine = FakeEngine::with_caps(vec![Capability::Chat], false);
    let estimate = engine.estimate_memory(&files("m", None), &opts()).unwrap();
    assert!(estimate.ram > 0);
    assert_eq!(estimate.vram, 0, "no GPU -> nothing on it");
}

#[test]
fn tokens_count_words_in_the_fake() {
    for engine in both(FakeEngine::new) {
        let session = loaded(engine.as_ref());
        let ids = engine.tokenize(session, "one two  three").unwrap();
        assert_eq!(ids.len(), 3);
        assert_eq!(engine.count_tokens(session, "one two  three").unwrap(), 3);
        assert_eq!(ids, engine.tokenize(session, "one two three").unwrap());
    }
}

/// AC-10: deterministic vectors, dims honored, normalized on request; role changes the vector.
#[test]
fn embeddings_are_deterministic_and_honor_dims_and_role() {
    for engine in both(FakeEngine::new) {
        let session = loaded(engine.as_ref());
        let cancel = CancelToken::new();
        let embed = |texts: &[&str], role, dims, normalize| {
            engine
                .embed(session, texts, role, dims, normalize, &cancel)
                .unwrap()
        };
        let a = embed(&["hello", "world"], EmbedRole::Document, Some(8), true);
        assert_eq!(a.vectors.len(), 2);
        assert!(a.vectors.iter().all(|v| v.len() == 8));
        for v in &a.vectors {
            let len = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((len - 1.0).abs() < 1e-5, "length {len}");
        }
        assert_ne!(a.vectors[0], a.vectors[1]);
        assert_eq!(
            a,
            embed(&["hello", "world"], EmbedRole::Document, Some(8), true)
        );
        let query = embed(&["hello"], EmbedRole::Query, Some(8), true);
        assert_ne!(query.vectors[0], a.vectors[0]);
        let native = embed(&["hello"], EmbedRole::Symmetric, None, false);
        assert_eq!(native.vectors[0].len(), FakeEngine::NATIVE_DIMS as usize);
        assert!(matches!(
            engine.embed(
                session,
                &["x"],
                EmbedRole::Query,
                Some(FakeEngine::NATIVE_DIMS + 1),
                true,
                &cancel
            ),
            Err(EngineError::Unsupported { .. })
        ));
    }
}

/// AC-10: vectors carry what made them (model, version, dims, prefix set).
#[test]
fn the_fingerprint_carries_model_and_dims() {
    for engine in both(FakeEngine::new) {
        let session = loaded(engine.as_ref());
        let out = engine
            .embed(
                session,
                &["x"],
                EmbedRole::Query,
                Some(4),
                true,
                &CancelToken::new(),
            )
            .unwrap();
        assert_eq!(
            out.fingerprint,
            EmbeddingFingerprint {
                model_id: "embed-1".into(),
                catalog_version: 3,
                dims: 4,
                prefix_profile: Some("gemma".into()),
            }
        );
    }
}

#[test]
fn generate_streams_the_echo_then_done() {
    for engine in both(FakeEngine::new) {
        let session = loaded(engine.as_ref());
        let events = run(engine.as_ref(), session, &user("fix this text"));
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                GenEvent::Delta(d) => Some(d.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "fix this text");
        assert_eq!(
            events.last(),
            Some(&GenEvent::Done {
                usage: Usage {
                    prompt: 5,
                    completion: 3
                },
                finish: Finish::Stop
            })
        );
    }
}

#[test]
fn generate_honors_max_tokens_and_stop() {
    for engine in both(FakeEngine::new) {
        let engine = engine.as_ref();
        let session = loaded(engine);
        let mut short = user("a b c d");
        short.max_tokens = 2;
        let events = run(engine, session, &short);
        assert_eq!(events.len(), 3);
        assert!(matches!(
            events[2],
            GenEvent::Done {
                finish: Finish::Length,
                ..
            }
        ));
        let mut stop = user("a b END c");
        stop.stop = vec!["END".into()];
        let events = run(engine, session, &stop);
        assert_eq!(
            events[..2],
            [GenEvent::Delta("a".into()), GenEvent::Delta(" b".into())]
        );
        assert!(matches!(
            events[2],
            GenEvent::Done {
                finish: Finish::Stop,
                ..
            }
        ));
    }
}

#[test]
fn generate_calls_an_offered_tool_when_asked() {
    for engine in both(FakeEngine::new) {
        let engine = engine.as_ref();
        let session = loaded(engine);
        let mut request = user("please call:search now");
        request.tools = vec![ToolSpec {
            name: "search".into(),
            description: "Find notes".into(),
            json_schema: json!({"type": "object"}),
        }];
        request.constraint = Constraint::JsonSchema(json!({"type": "object"}));
        let events = run(engine, session, &request);
        assert_eq!(
            events[0],
            GenEvent::ToolCall {
                name: "search".into(),
                args: json!({})
            }
        );
        assert!(matches!(
            events[1],
            GenEvent::Done {
                finish: Finish::Tool,
                ..
            }
        ));
        // no tools offered -> plain echo
        let events = run(engine, session, &user("please call:search now"));
        assert!(matches!(events[0], GenEvent::Delta(_)));
    }
}

/// AC-10 + AC-3: cancel honored mid-stream (generate ends Done/Cancelled; others answer Cancelled).
#[test]
fn cancel_stops_every_call() {
    for engine in both(paced) {
        let engine = engine.as_ref();
        let session = loaded(engine);
        let cancel = CancelToken::new();
        let mut events = Vec::new();
        engine
            .generate(
                session,
                &user("one two three four"),
                &mut |e| {
                    events.push(e);
                    cancel.cancel();
                },
                &cancel,
            )
            .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[1],
            GenEvent::Done {
                usage: Usage {
                    prompt: 6,
                    completion: 1
                },
                finish: Finish::Cancelled
            }
        );
        assert!(cancel.flag().load(Ordering::Relaxed));
        let pcm = vec![0.0; 16_000 * 3];
        let mut segments = Vec::new();
        let stopped = CancelToken::new();
        assert_eq!(
            engine.transcribe(
                session,
                &pcm,
                None,
                &mut |s| {
                    segments.push(s);
                    stopped.cancel();
                },
                &stopped
            ),
            Err(EngineError::Cancelled)
        );
        assert_eq!(segments.len(), 1, "what reached the sink stays");
        assert_eq!(
            engine.embed(session, &["x"], EmbedRole::Query, None, true, &stopped),
            Err(EngineError::Cancelled)
        );
        assert_eq!(
            engine.synthesize(session, "a b", None, None, &mut |_| {}, &stopped),
            Err(EngineError::Cancelled)
        );
        assert_eq!(
            engine.load(files("m", None), opts(), &|_| {}, &stopped),
            Err(EngineError::Cancelled)
        );
        // the session survives a cancelled call
        assert_eq!(engine.count_tokens(session, "a b"), Ok(2));
    }
}

/// AC-3: cancel during load = worker killed, nothing loaded.
#[test]
fn cancel_during_load_kills_the_worker() {
    let rig = Rig::new(FakeEngine::new);
    let cancel = CancelToken::new();
    let seen = RefCell::new(Vec::new());
    assert_eq!(
        rig.engine.load(
            files("m", None),
            opts(),
            &|p| {
                seen.borrow_mut().push(p);
                cancel.cancel();
            },
            &cancel
        ),
        Err(EngineError::Cancelled)
    );
    assert_eq!(seen.into_inner(), [0.0]);
    assert_eq!((rig.launched(), rig.kills()), (1, 1));
}

/// D-10: stdin EOF (host gone) reaches `on_host_gone` at once, while a call still runs (worker: hard exit).
#[test]
fn the_worker_loop_reports_the_host_gone_mid_call() {
    let (worker_in, mut to_worker) = io::pipe().unwrap();
    let (from_worker, worker_out) = io::pipe().unwrap();
    let (gone, gone_rx) = std::sync::mpsc::channel();
    let serving = std::thread::spawn(move || {
        serve(BufReader::new(worker_in), worker_out, &paced(), move || {
            let _ = gone.send(());
        })
    });
    let mut lines = BufReader::new(from_worker).lines();
    let mut send = |request: &crate::engine::wire::Request| {
        let mut line = serde_json::to_vec(request).unwrap();
        line.push(b'\n');
        to_worker.write_all(&line).unwrap();
    };
    send(&crate::engine::wire::Request::Load {
        model: files("m", None),
        opts: opts(),
    });
    let session = loop {
        let line = lines.next().unwrap().unwrap();
        if let Ok(crate::engine::wire::Response::Loaded { session, .. }) =
            serde_json::from_str(&line)
        {
            break session;
        }
    };
    send(&crate::engine::wire::Request::Generate {
        session,
        request: user("one two three"),
    });
    // first delta out = the paced call now waits (10 s) for a cancel that never comes
    assert!(lines.next().unwrap().unwrap().contains("Delta"));
    drop(send);
    drop(to_worker);
    gone_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("host gone reported while the call runs");
    assert!(!serving.is_finished(), "the call itself still runs");
}

/// D-7: the session tells whether it runs on a GPU (Auto + GPU in caps), also across the worker pipe.
#[test]
fn a_session_tells_whether_it_runs_on_the_gpu() {
    for engine in both(FakeEngine::new) {
        let auto = loaded(engine.as_ref());
        assert!(engine.on_gpu(auto));
        let cpu = LoadOpts {
            device: Device::Cpu,
            ..opts()
        };
        let session = engine
            .load(files("m", None), cpu, &|_| {}, &CancelToken::new())
            .unwrap();
        assert!(!engine.on_gpu(session));
        engine.unload(auto);
        assert!(!engine.on_gpu(auto), "unloaded");
    }
    for engine in both(|| FakeEngine::with_caps(vec![Capability::Chat], false)) {
        let session = loaded(engine.as_ref());
        assert!(!engine.on_gpu(session), "no GPU in caps");
    }
}

/// AC-10: a capability the engine lacks -> Unsupported, never a fallback.
#[test]
fn missing_capabilities_are_unsupported() {
    for engine in both(|| FakeEngine::with_caps(vec![Capability::Chat], true)) {
        let engine = engine.as_ref();
        let session = loaded(engine);
        let cancel = CancelToken::new();
        let unsupported =
            |r: Result<(), EngineError>| matches!(r, Err(EngineError::Unsupported { .. }));
        assert!(unsupported(
            engine
                .embed(session, &["x"], EmbedRole::Query, None, true, &cancel)
                .map(|_| ())
        ));
        assert!(unsupported(engine.transcribe(
            session,
            &[0.0; 10],
            None,
            &mut |_| {},
            &cancel
        )));
        assert!(unsupported(engine.synthesize(
            session,
            "a",
            None,
            None,
            &mut |_| {},
            &cancel
        )));
        let mut with_tools = user("call:x");
        with_tools.tools = vec![ToolSpec {
            name: "x".into(),
            description: String::new(),
            json_schema: json!({}),
        }];
        assert!(unsupported(engine.generate(
            session,
            &with_tools,
            &mut |_| {},
            &cancel
        )));
        assert!(engine
            .generate(session, &user("hi"), &mut |_| {}, &cancel)
            .is_ok());
    }
}

#[test]
fn transcribe_and_synthesize_stream_simple_output() {
    for engine in both(FakeEngine::new) {
        let session = loaded(engine.as_ref());
        let cancel = CancelToken::new();
        let mut segments = Vec::new();
        engine
            .transcribe(
                session,
                &vec![0.0; 16_000 + 8_000],
                Some("pt-BR"),
                &mut |s| segments.push(s),
                &cancel,
            )
            .unwrap();
        assert_eq!(
            segments,
            [
                Segment {
                    t0_ms: 0,
                    t1_ms: 1000,
                    text: "pt-BR second 0".into()
                },
                Segment {
                    t0_ms: 1000,
                    t1_ms: 1500,
                    text: "pt-BR second 1".into()
                },
            ]
        );
        let mut chunks = Vec::new();
        engine
            .synthesize(
                session,
                "read this aloud",
                None,
                None,
                &mut |c| chunks.push(c),
                &cancel,
            )
            .unwrap();
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.rate > 0 && !c.samples.is_empty()));
    }
}

/// Worker crash: sessions lost, calls say so; a new load works.
#[test]
fn a_crash_loses_sessions_until_the_next_load() {
    let engine = FakeEngine::new();
    let session = loaded(&engine);
    engine.crash();
    assert_eq!(
        engine.count_tokens(session, "a"),
        Err(EngineError::EngineCrashed)
    );
    let again = loaded(&engine);
    assert_ne!(again, session);
    assert_eq!(engine.count_tokens(again, "a b"), Ok(2));
    assert_eq!(
        engine.count_tokens(session, "a"),
        Err(EngineError::NotLoaded)
    );
}

/// AC-3: worker dies mid-call -> `EngineCrashed`, session forgotten, next load = fresh worker; others unharmed.
#[test]
fn a_dead_worker_is_a_crash_and_the_next_load_starts_fresh() {
    let rig = Rig::new(FakeEngine::new);
    let engine = &rig.engine;
    let other = loaded(engine);
    let session = loaded(engine);
    rig.crash();
    assert_eq!(
        engine.count_tokens(session, "a"),
        Err(EngineError::EngineCrashed)
    );
    assert_eq!(rig.kills(), 1, "crashed worker cleaned up");
    assert_eq!(
        engine.count_tokens(session, "a"),
        Err(EngineError::NotLoaded)
    );
    let again = loaded(engine);
    assert_ne!(again, session);
    assert_eq!(rig.launched(), 3);
    assert_eq!(engine.count_tokens(again, "a b"), Ok(2));
    assert_eq!(engine.count_tokens(other, "a b c"), Ok(3));
}

/// One worker per session; unload + drop = every worker gone (memory back to the OS).
#[test]
fn unload_and_drop_end_the_workers() {
    let rig = Rig::new(FakeEngine::new);
    let a = loaded(&rig.engine);
    let b = loaded(&rig.engine);
    assert_ne!(a, b);
    assert_eq!((rig.launched(), rig.kills()), (2, 0));
    rig.engine.unload(a);
    assert_eq!(rig.kills(), 1);
    rig.engine.unload(a);
    assert_eq!(rig.kills(), 1, "unknown session -> nothing");
    let Rig { engine, workers } = rig;
    drop(engine);
    assert_eq!(workers.kills(), 2);
}

/// Worker proxy estimate = host-side lower bound: file bytes, no worker started.
#[test]
fn the_worker_estimate_is_the_file_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let weights = dir.path().join("w.gguf");
    std::fs::write(&weights, [0u8; 1000]).unwrap();
    let rig = Rig::new(FakeEngine::new);
    let mut model = files("m", None);
    model.files = vec![(FileRole::Weights, weights)];
    assert_eq!(
        rig.engine.estimate_memory(&model, &opts()),
        Ok(MemoryEstimate { ram: 1000, vram: 0 })
    );
    model.files[0].1 = dir.path().join("missing.gguf");
    assert!(matches!(
        rig.engine.estimate_memory(&model, &opts()),
        Err(EngineError::Io(_))
    ));
    assert_eq!(rig.launched(), 0);
}

/// Installed version -> absolute file paths by role + catalog prefix set.
#[test]
fn model_files_come_from_the_installed_version() {
    let catalog = crate::catalog::bundled().unwrap();
    let model = catalog
        .models
        .iter()
        .find(|m| m.embedding.is_some())
        .unwrap();
    let dir = PathBuf::from("store").join(&model.lineage).join("1");
    let assigned = AssignedModel {
        id: model.id.clone(),
        lineage: model.lineage.clone(),
        catalog_version: 1,
        dir: dir.clone(),
        files: vec![ModelFile {
            path: "sub/old.gguf".into(),
            ..model.files[0].clone()
        }],
    };
    let files = model_files(&assigned, catalog);
    assert_eq!(files.model_id, model.id);
    assert_eq!(files.catalog_version, 1, "installed version, not catalog's");
    assert_eq!(files.files, [(model.files[0].role, dir.join("old.gguf"))]);
    assert_eq!(
        files.prefix_profile.as_deref(),
        model.embedding.as_ref().map(|e| e.prefix_profile.as_str())
    );
    let name = files.prefix_profile.as_ref().unwrap();
    assert_eq!(files.prefixes.as_ref(), catalog.prefix_profiles.get(name));
    assert_eq!(files.prompt, None);
    assert_eq!(files.min_engine, model.min_engine.version);
    // chat model: its markup crosses the wire with the files
    let chat = catalog.model("lfm3-1.7b-q4_k_m").unwrap();
    let assigned = AssignedModel {
        id: chat.id.clone(),
        lineage: chat.lineage.clone(),
        catalog_version: 1,
        dir,
        files: chat.files.clone(),
    };
    let files = model_files(&assigned, catalog);
    assert_eq!(files.prompt.as_ref(), catalog.prompt_profiles.get("lfm3"));
    assert_eq!(files.min_engine.as_deref(), Some("b11960"));
    assert_eq!(files.prefixes, None);
}
