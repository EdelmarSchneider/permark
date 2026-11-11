//! Eval set, step 1 of 2: every case of `app/src/plugins/correction/eval/cases.json` through the production piece
//! path of [`CorrectionJob`] (same prompt, sampling, answer cap, too-long + length + thinking discards) -> outputs
//! JSON, on the eval harness of the piece runner's test kit ([`eval`]). Step 2 (TS report: restore + guards + scores)
//! = `app/src/plugins/correction/eval/eval.test.ts`.
//!
//! - Always run: cases file shape; 2 cases over [`FakeEngine`] into a temp outputs file (harness check, no model).
//! - `#[ignore]`, by hand: the REAL worker + a local chat GGUF. Env: `PERMARK_WORKER` = worker build (debug override),
//!   `PERMARK_EVAL_GGUF` = chat model, `PERMARK_EVAL_PROFILE` = catalog prompt profile (default `gemma-4`),
//!   `PERMARK_EVAL_OUT` = outputs JSON path, `PERMARK_SMOKE_DEVICE=auto` -> GPU (never without a go for it; default
//!   CPU).
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

use std::collections::{BTreeMap, BTreeSet};

use permark_plugin_local_ai::engine::FakeEngine;
use permark_plugin_local_ai::pieces::testkit::eval::{self, Model, Outputs};
use permark_plugin_local_ai::pieces::PieceResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Fake;
use crate::prompt::CorrectionJob;

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

fn cases() -> Vec<Case> {
    serde_json::from_str(&std::fs::read_to_string(CASES).unwrap()).unwrap()
}

/// Every case in order on `model`: one record each.
fn evaluate(model: Model, cases: &[Case]) -> Outputs<Record> {
    eval::evaluate::<CorrectionJob, _, _>(
        model,
        cases,
        |case| (&(), case.text.as_str()),
        |case, answered| Record {
            id: case.id.clone(),
            result: answered.result,
            ms: answered.ms,
            tokens: answered.tokens,
        },
    )
    .unwrap()
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
    let outputs = evaluate(Model::fake(&fake), &cases);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("outputs.json");
    eval::write(&path, &outputs);
    let back: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let records = back["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    for (record, case) in records.iter().zip(&cases) {
        assert_eq!(record["id"], case.id.as_str());
        assert_eq!(record["result"]["kind"], "output");
        assert_eq!(record["result"]["text"], case.text.as_str(), "echo");
        assert!(record["tokens"].as_u64().unwrap() > 0);
        let fields: BTreeSet<_> = record.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            fields,
            BTreeSet::from(["id", "result", "ms", "tokens"].map(String::from))
        );
    }
    let facts: BTreeSet<_> = back.as_object().unwrap().keys().cloned().collect();
    let read = [
        "profile",
        "model",
        "onGpu",
        "loadMs",
        "totalMs",
        "tokPerSec",
        "records",
    ];
    assert_eq!(
        facts,
        BTreeSet::from(read.map(String::from)),
        "as the report reads them"
    );
    assert_eq!(back["profile"], "fake");
    assert_eq!(back["onGpu"], Value::Null);
    assert_eq!(
        fake.engine.loaded(),
        0,
        "the model is unloaded after its cases"
    );
}

/// Step 1 over the REAL worker + model (env + commands: module doc).
#[test]
#[ignore = "needs a built worker + a local chat model (GPU only with the owner's go); run by hand"]
fn eval_the_real_model() {
    let tmp = tempfile::tempdir().unwrap();
    let (model, out) = Model::real::<CorrectionJob>(tmp.path());
    let outputs = evaluate(model, &cases());
    eval::write(&out, &outputs);
    println!("{} -> {}", outputs.summary(), out.display());
}
