//! Eval set (D-8) step 1 of 2: every case of `app/src/plugins/correction/eval/cases.json` through the production piece
//! path ([`answer`] of [`CorrectionJob`]: same prompt, sampling, answer cap, too-long + length + thinking discards) ->
//! outputs JSON. Step 2 (TS report: restore + guards + scores) = `app/src/plugins/correction/eval/eval.test.ts`.
//!
//! - Always run: cases file shape; 2 cases over [`FakeEngine`] into a temp outputs file (harness check, no model).
//! - `#[ignore]`, by hand: the REAL worker + a local chat GGUF. Env: `PERMARK_WORKER` = worker build (debug override),
//!   `PERMARK_EVAL_GGUF` = chat model, `PERMARK_EVAL_PROFILE` = catalog prompt profile (default `gemma-4`),
//!   `PERMARK_EVAL_OUT` = outputs JSON path, `PERMARK_SMOKE_DEVICE=auto` -> GPU (ONLY with the owner's go; default CPU).
//!
//! Run (cwd `app/`), PowerShell:
//! ```text
//! $env:PERMARK_WORKER="<worker exe>"; $env:PERMARK_EVAL_GGUF="<model.gguf>"; $env:PERMARK_EVAL_OUT="F:/tmp/eval-e2b.json"
//! cargo test --manifest-path src-tauri/Cargo.toml -p permark-plugin-correction eval -- --ignored --nocapture
//! ```
//! bash:
//! ```text
//! PERMARK_WORKER=<worker exe> PERMARK_EVAL_GGUF=<model.gguf> PERMARK_EVAL_OUT=/f/tmp/eval-e2b.json \
//!   cargo test --manifest-path src-tauri/Cargo.toml -p permark-plugin-correction eval -- --ignored --nocapture
//! ```

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use permark_core::fsx;
use permark_plugin_local_ai::catalog::{self, Needs};
use permark_plugin_local_ai::engine::files::{self, WorkerFiles};
use permark_plugin_local_ai::engine::{
    CancelToken, ChatRequest, Device, EngineError, FakeEngine, FileRole, GenEvent, LoadOpts,
    ModelFiles, SessionId,
};
use permark_plugin_local_ai::pieces::{answer, preamble_tokens, Models, PieceResult, Session};
use permark_plugin_local_ai::runtime::{Runtime, WorkerLog};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Fake;
use crate::prompt::{CorrectionJob, CONTEXT};

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../src/plugins/correction/eval/cases.json"
);
const KINDS: [&str; 6] = ["errors", "clean", "marker", "heading", "injection", "mixed"];

/// One eval case (shape shared with the TS report).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    lang: String,
    kind: String,
    /// Piece text as sent to the model (non-prose as `⟦n⟧`).
    text: String,
    /// Original bytes per marker.
    #[serde(default)]
    markers: BTreeMap<String, String>,
    /// Expected answer (markers kept).
    expected: String,
}

/// Model answer for one case.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    id: String,
    result: PieceResult,
    ms: u64,
    /// Answer tokens.
    tokens: u32,
}

/// Outputs file (read by the TS report).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Outputs {
    profile: String,
    model: String,
    on_gpu: Option<bool>,
    load_ms: u64,
    total_ms: u64,
    tok_per_sec: f64,
    records: Vec<Record>,
}

fn cases() -> Vec<Case> {
    serde_json::from_str(&std::fs::read_to_string(CASES).unwrap()).unwrap()
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Session that keeps the answer tokens of its last `generate` (from `Done` usage).
struct Metered {
    inner: Box<dyn Session>,
    tokens: Cell<u32>,
}

impl Session for Metered {
    fn serves(&self) -> bool {
        self.inner.serves()
    }

    fn count_tokens(&self, text: &str) -> Result<usize, EngineError> {
        self.inner.count_tokens(text)
    }

    fn generate(
        &self,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        self.tokens.set(0);
        self.inner.generate(
            request,
            &mut |event| {
                if let GenEvent::Done { usage, .. } = &event {
                    self.tokens.set(usage.completion);
                }
                sink(event);
            },
            cancel,
        )
    }
}

/// Every case in order through [`answer`]; answers the records + total generate time + answer tokens.
fn evaluate(
    session: Box<dyn Session>,
    cases: &[Case],
) -> Result<(Vec<Record>, Duration, u32), EngineError> {
    let session = Metered {
        inner: session,
        tokens: Cell::new(0),
    };
    let preamble = preamble_tokens::<CorrectionJob>(&session, &())?;
    let cancel = CancelToken::new();
    let (mut records, mut total, mut tokens) = (Vec::new(), Duration::ZERO, 0u32);
    for case in cases {
        let started = Instant::now();
        let result = answer::<CorrectionJob>(&session, &(), preamble, &case.text, &cancel)?
            .ok_or(EngineError::Cancelled)?;
        let elapsed = started.elapsed();
        total += elapsed;
        tokens = tokens.saturating_add(session.tokens.get());
        records.push(Record {
            id: case.id.clone(),
            result,
            ms: millis(elapsed),
            tokens: session.tokens.get(),
        });
    }
    Ok((records, total, tokens))
}

fn write(path: &Path, outputs: &Outputs) {
    fsx::write_atomic(path, &serde_json::to_vec_pretty(outputs).unwrap()).unwrap();
}

/// Cases file: unique ids, every kind for the main languages, markers exact `⟦1⟧…⟦n⟧` in text + expected, clean =
/// unchanged.
#[test]
fn eval_cases_are_well_formed() {
    let cases = cases();
    let ids: BTreeSet<_> = cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), cases.len(), "duplicate ids");
    for lang in ["en", "pt-BR", "es", "de", "ja"] {
        let kinds: BTreeSet<_> = cases
            .iter()
            .filter(|c| c.lang == lang)
            .map(|c| c.kind.as_str())
            .collect();
        assert_eq!(kinds, KINDS.into_iter().collect(), "{lang}");
    }
    for case in &cases {
        assert!(KINDS.contains(&case.kind.as_str()), "{}", case.id);
        let markers: Vec<_> = (1..=case.markers.len()).map(|n| format!("⟦{n}⟧")).collect();
        assert_eq!(
            case.markers.keys().cloned().collect::<BTreeSet<_>>(),
            markers.iter().cloned().collect(),
            "{}",
            case.id
        );
        for text in [&case.text, &case.expected] {
            let order: Vec<_> = markers
                .iter()
                .filter_map(|m| text.find(m.as_str()))
                .collect();
            assert!(
                order.len() == markers.len() && order.is_sorted(),
                "{}",
                case.id
            );
        }
        assert_eq!(
            case.kind == "marker",
            !case.markers.is_empty(),
            "{}",
            case.id
        );
        if case.kind == "clean" {
            assert_eq!(case.text, case.expected, "{}", case.id);
        } else {
            assert_ne!(case.text, case.expected, "{}", case.id);
        }
    }
}

/// Harness without a model: 2 cases over [`FakeEngine`] (echo) -> outputs file with one record each.
#[test]
fn eval_runs_cases_over_the_fake_engine_into_an_outputs_file() {
    let cases: Vec<_> = cases().into_iter().take(2).collect();
    let fake = Fake::new::<CorrectionJob>(FakeEngine::new());
    let session = fake.open(&|_| {}, &CancelToken::new()).unwrap();
    let (records, total, tokens) = evaluate(session, &cases).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("outputs.json");
    write(
        &path,
        &Outputs {
            profile: "fake".into(),
            model: "fake".into(),
            on_gpu: None,
            load_ms: 0,
            total_ms: millis(total),
            tok_per_sec: 0.0,
            records,
        },
    );
    let back: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let records = back["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    for (record, case) in records.iter().zip(&cases) {
        assert_eq!(record["id"], case.id.as_str());
        assert_eq!(record["result"]["kind"], "output");
        assert_eq!(record["result"]["text"], case.text.as_str(), "echo");
        assert!(record["tokens"].as_u64().unwrap() > 0);
    }
    assert!(tokens > 0);
    assert_eq!(back["onGpu"], Value::Null);
}

/// Real worker session (Drop unloads).
struct Worker {
    runtime: Arc<Runtime>,
    id: SessionId,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.runtime.unload(self.id);
    }
}

impl Session for Worker {
    fn serves(&self) -> bool {
        true
    }

    fn count_tokens(&self, text: &str) -> Result<usize, EngineError> {
        self.runtime.engine().count_tokens(self.id, text)
    }

    fn generate(
        &self,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        self.runtime
            .engine()
            .generate(self.id, request, sink, cancel)
    }
}

/// Step 1 over the REAL worker + model (env + commands: module doc).
#[test]
#[ignore = "needs a built worker + a local chat model (GPU only with the owner's go); run by hand"]
fn eval_the_real_model() {
    let env = |key| std::env::var_os(key).map(PathBuf::from);
    let (Some(_), Some(gguf), Some(out)) = (
        env("PERMARK_WORKER"),
        env("PERMARK_EVAL_GGUF"),
        env("PERMARK_EVAL_OUT"),
    ) else {
        panic!("set PERMARK_WORKER, PERMARK_EVAL_GGUF and PERMARK_EVAL_OUT");
    };
    let profile = std::env::var("PERMARK_EVAL_PROFILE").unwrap_or_else(|_| "gemma-4".into());
    let auto = std::env::var("PERMARK_SMOKE_DEVICE").is_ok_and(|d| d == "auto");
    let tmp = tempfile::tempdir().unwrap();
    let runtime = Arc::new(Runtime::new(
        Arc::new(WorkerFiles::new(
            None,
            tmp.path().join(files::DIR),
            Arc::new(|_, _| {}),
        )),
        Arc::new(WorkerLog::new(tmp.path().join("worker.log"))),
    ));
    let model = ModelFiles {
        model_id: "eval".into(),
        catalog_version: 1,
        files: vec![(FileRole::Weights, gguf.clone())],
        prompt: Some(
            catalog::bundled()
                .unwrap()
                .prompt_profiles
                .get(&profile)
                .cloned()
                .unwrap_or_else(|| panic!("no prompt profile {profile}")),
        ),
        ..ModelFiles::default()
    };
    let opts = LoadOpts {
        context_len: CONTEXT,
        device: if auto { Device::Auto } else { Device::Cpu },
        threads: None,
    };
    let small = Needs {
        ram_min: 1,
        ram_rec: 1,
        vram_rec: 1,
    };
    let started = Instant::now();
    let id = runtime
        .load("eval", &small, model, opts, &|_| {}, &CancelToken::new())
        .unwrap_or_else(|e| panic!("load: {e:?}"));
    let load = started.elapsed();
    let on_gpu = runtime.engine().on_gpu(id);
    let session = Box::new(Worker {
        runtime: Arc::clone(&runtime),
        id,
    });
    let cases = cases();
    let (records, total, tokens) = evaluate(session, &cases).unwrap();
    let secs = total.as_secs_f64();
    let tok_per_sec = if secs > 0.0 {
        f64::from(tokens) / secs
    } else {
        0.0
    };
    let model = gguf
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    write(
        &out,
        &Outputs {
            profile,
            model,
            on_gpu: Some(on_gpu),
            load_ms: millis(load),
            total_ms: millis(total),
            tok_per_sec,
            records,
        },
    );
    println!(
        "eval: {} cases, gpu {on_gpu}, load {load:?}, generate {total:?}, {tokens} tokens, {tok_per_sec:.1} tok/s -> {}",
        cases.len(),
        out.display()
    );
}
