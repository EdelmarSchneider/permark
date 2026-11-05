//! Wire + proxy edges with scripted workers (contract cases over a real `serve`: plugin `local-ai` engine tests).

use std::io::{self, Cursor};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::json;

use super::wire::{Hello, Request, Response, PROTO};
use super::*;

fn files() -> ModelFiles {
    ModelFiles {
        model_id: "m".into(),
        catalog_version: 1,
        files: vec![(FileRole::Weights, PathBuf::from("w.gguf"))],
        prefix_profile: Some("gemma".into()),
    }
}

fn opts() -> LoadOpts {
    LoadOpts {
        context_len: 512,
        device: Device::Cpu,
        threads: Some(2),
    }
}

fn hello(proto: u32) -> String {
    let caps = EngineCaps {
        formats: vec!["gguf".into()],
        capabilities: vec![Capability::Chat],
        gpu: false,
        version: "b1".into(),
    };
    serde_json::to_string(&Hello { proto, caps }).unwrap() + "\n"
}

/// Worker whose stdout = `script`, then EOF; counts kills.
fn scripted(script: String) -> (WorkerEngine, Arc<AtomicUsize>) {
    let kills = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&kills);
    let engine = WorkerEngine::new(Box::new(move || {
        let counted = Arc::clone(&counted);
        Ok(WorkerPipes {
            to_worker: Box::new(io::sink()),
            from_worker: Box::new(Cursor::new(script.clone().into_bytes())),
            kill: Box::new(move || {
                counted.fetch_add(1, Ordering::Relaxed);
            }),
        })
    }));
    (engine, kills)
}

fn load(engine: &WorkerEngine) -> Result<SessionId, EngineError> {
    engine.load(files(), opts(), &|_| {}, &CancelToken::new())
}

#[test]
fn requests_and_responses_round_trip_as_json_lines() {
    let requests = [
        Request::Load {
            model: files(),
            opts: opts(),
        },
        Request::Embed {
            session: SessionId(1),
            texts: vec!["a".into()],
            role: EmbedRole::Query,
            dims: Some(8),
            normalize: true,
        },
        Request::Generate {
            session: SessionId(1),
            request: ChatRequest {
                messages: vec![ChatMessage {
                    role: Role::User,
                    content: "hi\nthere".into(),
                }],
                constraint: Constraint::JsonSchema(json!({"type": "object"})),
                stop: vec!["END".into()],
                max_tokens: 9,
                ..ChatRequest::default()
            },
        },
        Request::Transcribe {
            session: SessionId(1),
            pcm_16k_mono: vec![0.5, -0.25],
            lang: None,
        },
        Request::Unload {
            session: SessionId(1),
        },
        Request::Cancel,
    ];
    for request in requests {
        let line = serde_json::to_string(&request).unwrap();
        assert!(!line.contains('\n'), "one line: {line}");
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), request);
    }
    assert_eq!(
        serde_json::to_string(&Request::Cancel).unwrap(),
        r#"{"op":"cancel"}"#
    );
    let responses = [
        Response::Progress { value: 0.5 },
        Response::Loaded {
            session: SessionId(7),
        },
        Response::Gen {
            event: GenEvent::ToolCall {
                name: "search".into(),
                args: json!({"q": 1}),
            },
        },
        Response::Gen {
            event: GenEvent::Done {
                usage: Usage {
                    prompt: 1,
                    completion: 2,
                },
                finish: Finish::Cancelled,
            },
        },
        Response::Pcm {
            chunk: PcmChunk {
                rate: 24_000,
                samples: vec![0.0, 0.125],
            },
        },
        Response::Done,
        Response::Error {
            error: EngineError::Unsupported { what: "x".into() },
        },
        Response::Error {
            error: EngineError::Io("disk".into()),
        },
    ];
    for response in responses {
        let line = serde_json::to_string(&response).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&line).unwrap(), response);
    }
}

/// AC-2: other protocol -> `EngineTooOld`, worker killed; caps stay unknown.
#[test]
fn a_hello_with_another_protocol_is_an_engine_too_old() {
    let (engine, kills) = scripted(hello(PROTO + 1));
    assert_eq!(
        load(&engine),
        Err(EngineError::EngineTooOld {
            needed: format!("protocol {PROTO}")
        })
    );
    assert_eq!(kills.load(Ordering::Relaxed), 1);
    // a future Hello may look different: only `proto` is read first
    let (engine, _) = scripted(format!("{{\"proto\":{},\"x\":[]}}\n", PROTO + 1));
    assert!(matches!(
        load(&engine),
        Err(EngineError::EngineTooOld { .. })
    ));
    assert_eq!(engine.caps(), EngineCaps::default());
}

/// AC-3: stdout ends mid-call -> `EngineCrashed`, worker killed.
#[test]
fn a_worker_ending_mid_load_is_a_crash() {
    let (engine, kills) = scripted(hello(PROTO) + r#"{"re":"progress","value":0.0}"# + "\n");
    let seen = std::cell::Cell::new(0);
    assert_eq!(
        engine.load(
            files(),
            opts(),
            &|_| seen.set(seen.get() + 1),
            &CancelToken::new()
        ),
        Err(EngineError::EngineCrashed)
    );
    assert_eq!(seen.get(), 1);
    assert_eq!(kills.load(Ordering::Relaxed), 1);
    let (engine, _) = scripted(hello(PROTO) + "not json\n");
    assert_eq!(load(&engine), Err(EngineError::EngineCrashed));
}

#[test]
fn caps_come_from_a_hello_and_a_failed_launch_is_io() {
    let (engine, kills) = scripted(hello(PROTO));
    let caps = engine.caps();
    assert_eq!(caps.version, "b1");
    assert_eq!(caps.capabilities, [Capability::Chat]);
    assert_eq!(kills.load(Ordering::Relaxed), 1, "hello-only worker gone");
    assert_eq!(engine.caps(), caps, "kept, no second worker");
    assert_eq!(kills.load(Ordering::Relaxed), 1);
    let failing = WorkerEngine::new(Box::new(|| Err(io::Error::other("no worker"))));
    assert_eq!(load(&failing), Err(EngineError::Io("no worker".into())));
    assert_eq!(failing.caps(), EngineCaps::default());
}

#[test]
fn calls_on_unknown_sessions_are_not_loaded() {
    let (engine, kills) = scripted(hello(PROTO));
    assert_eq!(
        engine.count_tokens(SessionId(3), "a"),
        Err(EngineError::NotLoaded)
    );
    engine.unload(SessionId(3));
    assert_eq!(kills.load(Ordering::Relaxed), 0, "nothing launched");
    let mut no_weights = files();
    no_weights.files[0].0 = FileRole::Mmproj;
    assert_eq!(
        engine.estimate_memory(&no_weights, &opts()),
        Err(EngineError::Corrupt)
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
    assert_eq!(EngineError::Cancelled.to_string(), "Stopped.");
}
