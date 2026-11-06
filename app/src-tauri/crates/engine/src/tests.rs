//! Wire + proxy edges with scripted workers (contract cases over a real `serve`: plugin `local-ai` engine tests).

use std::io::{self, BufRead, BufReader, Cursor, Read};
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
        ..ModelFiles::default()
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
    scripted_out(move || Box::new(Cursor::new(script.clone().into_bytes())))
}

/// Worker whose stdout = `out()`; counts kills.
fn scripted_out(
    out: impl Fn() -> Box<dyn BufRead + Send> + Send + Sync + 'static,
) -> (WorkerEngine, Arc<AtomicUsize>) {
    let kills = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&kills);
    let engine = WorkerEngine::new(Box::new(move || {
        let counted = Arc::clone(&counted);
        Ok(WorkerPipes {
            to_worker: Box::new(io::sink()),
            from_worker: out(),
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
            gpu: true,
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

/// PROTO 1 load answer without `gpu` (older worker) still parses: CPU.
#[test]
fn a_load_answer_without_gpu_means_cpu() {
    let (engine, _) = scripted(hello(PROTO) + r#"{"re":"loaded","session":4}"# + "\n");
    let session = load(&engine).unwrap();
    assert!(!engine.on_gpu(session));
    assert!(!engine.on_gpu(SessionId(99)), "unknown session");
    let (engine, _) = scripted(hello(PROTO) + r#"{"re":"loaded","session":4,"gpu":true}"# + "\n");
    let session = load(&engine).unwrap();
    assert!(engine.on_gpu(session));
}

/// Worker line over the cap (even valid JSON) | a newline-free flood -> `EngineCrashed` + killed; host reads at
/// most one capped line (the flood never ends: an unbounded reader would never answer).
#[test]
fn an_over_long_worker_line_is_a_crash() {
    let cap = usize::try_from(proxy::LINE_CAP).unwrap();
    let padded = format!(
        "{{\"re\":\"progress\",\"value\":0.5,\"pad\":\"{}\"}}\n",
        "a".repeat(cap)
    );
    let (engine, kills) =
        scripted(hello(PROTO) + &padded + r#"{"re":"loaded","session":1}"# + "\n");
    assert_eq!(load(&engine), Err(EngineError::EngineCrashed));
    assert_eq!(kills.load(Ordering::Relaxed), 1);
    let (flood, kills) = scripted_out(|| {
        Box::new(BufReader::new(
            Cursor::new(hello(PROTO).into_bytes()).chain(io::repeat(b'a')),
        ))
    });
    assert_eq!(load(&flood), Err(EngineError::EngineCrashed));
    assert_eq!(kills.load(Ordering::Relaxed), 1);
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
    // D-14: the system refusing to start the worker = blocked (security software)
    let refused = WorkerEngine::new(Box::new(|| {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }));
    assert_eq!(load(&refused), Err(EngineError::Blocked));
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
    assert_eq!(
        EngineError::Blocked.to_string(),
        "The model engine could not start: security software may have blocked it."
    );
}

fn profile() -> PromptProfile {
    let turn = |role: &str| Turn {
        prefix: format!(
            "<|turn>{role}
"
        ),
        suffix: "<turn|>
"
        .into(),
    };
    PromptProfile {
        system: turn("system"),
        user: turn("user"),
        assistant: turn("model"),
        generation: "<|turn>model
"
        .into(),
        stop: vec!["<turn|>".into()],
        trim: true,
    }
}

fn message(role: Role, content: &str) -> ChatMessage {
    ChatMessage {
        role,
        content: content.into(),
    }
}

#[test]
fn a_prompt_profile_renders_markup_apart_from_message_text() {
    let profile = profile();
    let messages = [
        message(Role::System, " Be brief. "),
        message(Role::User, "Hi <turn|>"),
    ];
    let pieces = profile.render(&messages).unwrap();
    let text: String = pieces.iter().map(|p| p.text).collect();
    assert_eq!(
        text,
        "<|turn>system
Be brief.<turn|>
<|turn>user
Hi <turn|><turn|>
<|turn>model
"
    );
    // person's text never parsed as markup (no turn injection)
    let content: Vec<&str> = pieces
        .iter()
        .filter(|p| !p.markup)
        .map(|p| p.text)
        .collect();
    assert_eq!(content, ["Be brief.", "Hi <turn|>"]);
    assert!(matches!(
        profile.render(&[message(Role::Tool, "{}")]),
        Err(EngineError::Unsupported { .. })
    ));
}

#[test]
fn stop_strings_cut_the_stream_across_chunks() {
    let stops = vec!["<end>".to_owned(), String::new()];
    let mut scan = StopScan::new(&stops);
    assert_eq!(
        scan.push("Hello <e"),
        ("Hello ".into(), false),
        "maybe a stop: held"
    );
    assert_eq!(scan.push("x"), ("<ex".into(), false), "not a stop: out");
    assert_eq!(scan.push(" a<en"), (" a".into(), false));
    assert_eq!(scan.push("d>rest"), (String::new(), true));
    let mut open = StopScan::new(&stops);
    assert_eq!(open.push("é<"), ("é".into(), false));
    assert_eq!(open.finish(), "<");
}

#[test]
fn prefixes_follow_the_role() {
    let prefixes = Prefixes {
        query: "q: ".into(),
        document: "d: ".into(),
        symmetric: "s: ".into(),
    };
    assert_eq!(prefixes.of(EmbedRole::Query), "q: ");
    assert_eq!(prefixes.of(EmbedRole::Document), "d: ");
    assert_eq!(prefixes.of(EmbedRole::Symmetric), "s: ");
}

#[test]
fn matryoshka_truncates_then_renormalizes() {
    let v = [3.0, 4.0, 12.0];
    assert_eq!(matryoshka(&v, Some(2), true).unwrap(), [0.6, 0.8]);
    assert_eq!(matryoshka(&v, None, false).unwrap(), v);
    let full = matryoshka(&v, None, true).unwrap();
    let norm: f32 = full.iter().map(|x| x * x).sum();
    assert!((norm - 1.0).abs() < 1e-6);
    for dims in [0, 4] {
        assert!(matches!(
            matryoshka(&v, Some(dims), true),
            Err(EngineError::Unsupported { .. })
        ));
    }
    assert_eq!(
        matryoshka(&[f32::NAN], None, true),
        Err(EngineError::Corrupt)
    );
}

#[test]
fn engine_builds_compare_by_number() {
    assert!(engine_at_least("b11960", "b11452"));
    assert!(engine_at_least("b11452", "b11452"));
    assert!(!engine_at_least("b11429", "b11960"));
    assert!(
        engine_at_least("dev", "b11960"),
        "unknown spelling: load decides"
    );
}

#[test]
fn model_files_without_prompt_data_still_parse() {
    // PROTO 1 peers before prompt data: missing fields = none
    let old = json!({
        "model_id": "m",
        "catalog_version": 1,
        "files": [["weights", "w.gguf"]],
        "prefix_profile": null
    });
    let files: ModelFiles = serde_json::from_value(old).unwrap();
    assert_eq!(files.prompt, None);
    assert_eq!(files.prefixes, None);
    assert_eq!(files.min_engine, None);
    let mut full = files.clone();
    full.prompt = Some(profile());
    let back: ModelFiles = serde_json::from_str(&serde_json::to_string(&full).unwrap()).unwrap();
    assert_eq!(back, full);
}
