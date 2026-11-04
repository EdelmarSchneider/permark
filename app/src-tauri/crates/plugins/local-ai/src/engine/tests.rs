use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::catalog::ModelFile;

fn files(id: &str, prefix: Option<&str>) -> ModelFiles {
    ModelFiles {
        model_id: id.to_owned(),
        catalog_version: 3,
        files: vec![(FileRole::Weights, PathBuf::from("w.gguf"))],
        prefix_profile: prefix.map(str::to_owned),
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
    let engine: Box<dyn Engine> = Box::new(FakeEngine::new());
    let shared: Arc<dyn Engine> = Arc::from(engine);
    let other = Arc::clone(&shared);
    let session = std::thread::spawn(move || loaded(other.as_ref()))
        .join()
        .unwrap();
    assert_eq!(shared.context_len(session).unwrap(), 2048);
    assert!(shared.caps().formats.contains(&"gguf".to_owned()));
}

#[test]
fn load_reports_progress_and_unload_forgets_the_session() {
    let engine = FakeEngine::new();
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
    assert_eq!(engine.loaded(), 1);
    engine.unload(session);
    assert_eq!(engine.loaded(), 0);
    assert_eq!(engine.context_len(session), Err(EngineError::NotLoaded));
}

#[test]
fn load_refuses_files_without_weights_and_a_missing_gpu() {
    let engine = FakeEngine::with_caps(vec![Capability::Chat], false);
    let mut no_weights = files("m", None);
    no_weights.files = vec![(FileRole::Mmproj, PathBuf::from("p.gguf"))];
    let cancel = CancelToken::new();
    assert_eq!(
        engine.load(no_weights.clone(), opts(), &|_| {}, &cancel),
        Err(EngineError::Corrupt)
    );
    assert_eq!(
        engine.estimate_memory(&no_weights, &opts()),
        Err(EngineError::Corrupt)
    );
    let gpu = LoadOpts {
        device: Device::Gpu(0),
        ..opts()
    };
    assert!(matches!(
        engine.load(files("m", None), gpu, &|_| {}, &cancel),
        Err(EngineError::Unsupported { .. })
    ));
    let estimate = engine.estimate_memory(&files("m", None), &opts()).unwrap();
    assert!(estimate.ram > 0);
    assert_eq!(estimate.vram, 0, "no GPU -> nothing on it");
}

#[test]
fn tokens_count_words_in_the_fake() {
    let engine = FakeEngine::new();
    let session = loaded(&engine);
    let ids = engine.tokenize(session, "one two  three").unwrap();
    assert_eq!(ids.len(), 3);
    assert_eq!(engine.count_tokens(session, "one two  three").unwrap(), 3);
    assert_eq!(ids, engine.tokenize(session, "one two three").unwrap());
}

/// AC-10: deterministic vectors, dims honored, normalized on request; role changes the vector.
#[test]
fn embeddings_are_deterministic_and_honor_dims_and_role() {
    let engine = FakeEngine::new();
    let session = loaded(&engine);
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

/// AC-10: vectors carry what made them (model, version, dims, prefix set).
#[test]
fn the_fingerprint_carries_model_and_dims() {
    let engine = FakeEngine::new();
    let session = loaded(&engine);
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

#[test]
fn generate_streams_the_echo_then_done() {
    let engine = FakeEngine::new();
    let session = loaded(&engine);
    let events = run(&engine, session, &user("fix this text"));
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

#[test]
fn generate_honors_max_tokens_and_stop() {
    let engine = FakeEngine::new();
    let session = loaded(&engine);
    let mut short = user("a b c d");
    short.max_tokens = 2;
    let events = run(&engine, session, &short);
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
    let events = run(&engine, session, &stop);
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

#[test]
fn generate_calls_an_offered_tool_when_asked() {
    let engine = FakeEngine::new();
    let session = loaded(&engine);
    let mut request = user("please call:search now");
    request.tools = vec![ToolSpec {
        name: "search".into(),
        description: "Find notes".into(),
        json_schema: json!({"type": "object"}),
    }];
    request.constraint = Constraint::JsonSchema(json!({"type": "object"}));
    let events = run(&engine, session, &request);
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
    let events = run(&engine, session, &user("please call:search now"));
    assert!(matches!(events[0], GenEvent::Delta(_)));
}

/// AC-10: cancel honored mid-stream (generate ends Done/Cancelled; others answer Cancelled).
#[test]
fn cancel_stops_every_call() {
    let engine = FakeEngine::new();
    let session = loaded(&engine);
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
    assert!(cancel.flag().load(std::sync::atomic::Ordering::Relaxed));
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
}

/// AC-10: a capability the engine lacks -> Unsupported, never a fallback.
#[test]
fn missing_capabilities_are_unsupported() {
    let engine = FakeEngine::with_caps(vec![Capability::Chat], true);
    let session = loaded(&engine);
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

#[test]
fn transcribe_and_synthesize_stream_simple_output() {
    let engine = FakeEngine::new();
    let session = loaded(&engine);
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
    let files = ModelFiles::of(&assigned, catalog);
    assert_eq!(files.model_id, model.id);
    assert_eq!(files.catalog_version, 1, "installed version, not catalog's");
    assert_eq!(files.files, [(model.files[0].role, dir.join("old.gguf"))]);
    assert_eq!(
        files.prefix_profile.as_deref(),
        model.embedding.as_ref().map(|e| e.prefix_profile.as_str())
    );
}

#[test]
fn errors_read_as_plain_english() {
    assert_eq!(
        EngineError::EngineTooOld {
            needed: "b1200".into()
        }
        .to_string(),
        "This model needs a newer engine (b1200)."
    );
    assert_eq!(
        EngineError::Unsupported { what: "gpu".into() }.to_string(),
        "This is not supported here: gpu."
    );
}
