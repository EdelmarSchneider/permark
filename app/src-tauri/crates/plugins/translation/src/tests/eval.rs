//! Eval set, step 1 of 2: every case of `app/src/plugins/translation/eval/cases.json` through the production piece
//! path of [`TranslationJob`] (same prompt, sampling, answer cap, too-long + length + thinking discards) for the
//! languages of the case -> outputs JSON, on the eval harness of the piece runner's test kit ([`eval`]). Step 2 (TS
//! report: restore + guards + what was expected of each case) = `app/src/plugins/translation/eval/eval.test.ts`.
//!
//! - Always run: cases file shape; every case over [`FakeEngine`] into a temp outputs file (harness check, no model).
//! - `#[ignore]`, by hand: [`eval_the_real_model`], the REAL worker + a local chat GGUF.

use std::collections::{BTreeMap, BTreeSet};

use permark_plugin_local_ai::engine::FakeEngine;
use permark_plugin_local_ai::pieces::testkit::eval::{self, Model, Outputs};
use permark_plugin_local_ai::pieces::testkit::Fake;
use permark_plugin_local_ai::pieces::PieceResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::prompt::{self, Language, Pair, Tag, TranslationJob};

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../src/plugins/translation/eval/cases.json"
);
const KINDS: [&str; 14] = [
    "plain",
    "markers",
    "heading",
    "listItem",
    "cell",
    "footnote",
    "numbers",
    "identifiers",
    "request",
    "same",
    "mixed",
    "long",
    "alert",
    "emphasis",
];
/// Language of the document -> target of the run.
const PAIRS: [(&str, &str); 10] = [
    ("en", "pt-BR"),
    ("en", "ja"),
    ("en", "de"),
    ("en", "zh-Hans"),
    ("en", "ar"),
    ("pt-BR", "en-US"),
    ("es", "pt-BR"),
    ("ja", "en-US"),
    ("ko", "en-US"),
    ("de", "fr"),
];

/// The cases file (shape shared with the TS report).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Set {
    /// English name of every language a run names, by tag: what the window sends with the tag.
    languages: BTreeMap<String, String>,
    cases: Vec<Case>,
}

/// One eval case.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    id: String,
    kind: String,
    /// Language of the document the piece comes from (with `target`: the pair the report tallies by); the piece
    /// itself may be in another one (kinds `same`, `mixed`).
    from: String,
    /// Source the run names; `None`: read off the piece.
    source: Option<String>,
    target: String,
    /// Markdown block the piece is cut from (the report cuts it again with the app's segmenter).
    markdown: String,
    /// Piece text as sent to the model (non-prose as `⟦n⟧`).
    text: String,
    /// `translated` | `same` (the piece is in the target already: nothing to suggest).
    expect: String,
    /// Must stand unchanged in the answer.
    #[serde(default)]
    must_keep: Vec<String>,
    /// Must not stand in the answer (an answer to a question, words of another language).
    #[serde(default)]
    must_not_contain: Vec<String>,
    /// For the person reading the report.
    #[serde(default)]
    note: Option<String>,
}

/// A case with the languages of its run as the prompt names them.
struct Asked {
    case: Case,
    pair: Pair,
}

/// Model answer for one case.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    id: String,
    kind: String,
    from: String,
    source: Option<String>,
    target: String,
    result: PieceResult,
    ms: u64,
    prompt_tokens: u32,
    /// Answer tokens.
    tokens: u32,
    /// Answer cap asked.
    max_tokens: u32,
}

fn set() -> Set {
    serde_json::from_str(&std::fs::read_to_string(CASES).unwrap()).unwrap()
}

/// Every case with its run's languages, named as the cases file names them.
fn asked() -> Vec<Asked> {
    let Set { languages, cases } = set();
    let language = |tag: &String| {
        let name = languages
            .get(tag)
            .unwrap_or_else(|| panic!("no name for {tag}"));
        Language::parse(tag, name).unwrap_or_else(|e| panic!("{tag} {name}: {e:?}"))
    };
    cases
        .into_iter()
        .map(|case| {
            let pair = Pair::new(language(&case.target), case.source.as_ref().map(language))
                .unwrap_or_else(|e| panic!("{}: {e:?}", case.id));
            Asked { case, pair }
        })
        .collect()
}

/// Every case in order on `model`, with the app's prompt.
fn evaluate(model: Model, cases: &[Asked]) -> Outputs<Record> {
    eval::evaluate::<TranslationJob, _, _>(
        model,
        cases,
        |asked| (&asked.pair, asked.case.text.as_str()),
        |Asked { case, .. }, answered| Record {
            id: case.id.clone(),
            kind: case.kind.clone(),
            from: case.from.clone(),
            source: case.source.clone(),
            target: case.target.clone(),
            result: answered.result,
            ms: answered.ms,
            prompt_tokens: answered.prompt_tokens,
            tokens: answered.tokens,
            max_tokens: answered.max_tokens,
        },
    )
    .unwrap()
}

fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}

/// Cases file: about 50 cases, unique ids, every kind and every pair, tags of the crate's own shape and names the
/// prompt takes, markers exact `⟦1⟧…⟦n⟧`, expectations that can hold.
#[test]
fn eval_cases_are_well_formed() {
    let Set { languages, cases } = set();
    assert!((44..=56).contains(&cases.len()), "{} cases", cases.len());
    let ids: BTreeSet<_> = cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), cases.len(), "duplicate ids");
    let kinds: BTreeSet<_> = cases.iter().map(|c| c.kind.as_str()).collect();
    assert_eq!(kinds, KINDS.into_iter().collect());
    let pairs: BTreeSet<_> = cases
        .iter()
        .map(|c| (c.from.as_str(), c.target.as_str()))
        .collect();
    assert_eq!(pairs, PAIRS.into_iter().collect());
    // every name is one a run sends, and one the prompt takes
    let sent: BTreeSet<_> = cases
        .iter()
        .flat_map(|c| c.source.iter().chain([&c.target]))
        .collect();
    assert_eq!(sent, languages.keys().collect());
    for (tag, name) in &languages {
        assert!(Language::parse(tag, name).is_ok(), "{tag} {name}");
    }
    let named = cases.iter().filter(|c| c.source.is_some()).count();
    assert!(
        named > 0 && named * 2 < cases.len(),
        "{named} name a source"
    );
    for case in &cases {
        let id = &case.id;
        assert!(Tag::parse(&case.from).is_ok(), "{id}");
        assert!(
            ["translated", "same"].contains(&case.expect.as_str()),
            "{id}"
        );
        assert_eq!(case.kind == "same", case.expect == "same", "{id}");
        assert!(!case.text.trim().is_empty(), "{id}");
        // markers: ⟦1⟧…⟦n⟧, each once, in order
        let count = case.text.matches('⟦').count();
        let at: Vec<_> = (1..=count)
            .filter_map(|n| {
                let marker = format!("⟦{n}⟧");
                (case.text.matches(&marker).count() == 1).then(|| case.text.find(&marker))
            })
            .collect();
        assert!(at.len() == count && at.is_sorted(), "{id}");
        assert_eq!(case.text.matches('⟧').count(), count, "{id}");
        if count == 0 {
            assert!(case.markdown.contains(&case.text), "{id}: not in its block");
        }
        // what must be kept is there to keep; what must not come back (words of the source language, an answer)
        // is told in lowercase, as the report compares it, and is nothing that must be kept
        for kept in &case.must_keep {
            assert!(
                !kept.is_empty() && case.markdown.contains(kept),
                "{id} {kept}"
            );
        }
        for banned in &case.must_not_contain {
            assert!(
                !banned.is_empty() && *banned == banned.to_lowercase(),
                "{id} {banned}"
            );
            let kept = |k: &String| k.to_lowercase().contains(banned);
            assert!(!case.must_keep.iter().any(kept), "{id} {banned}");
        }
        assert!(
            case.note.as_ref().is_none_or(|n| !n.trim().is_empty()),
            "{id}"
        );
    }
}

/// Harness without a model: every case over [`FakeEngine`] (echo of the request) with the app's prompt -> an outputs
/// file, one record per case with its pair, its token counts and the runner's result.
#[test]
fn eval_runs_every_case_over_the_fake_engine_into_an_outputs_file() {
    let cases = asked();
    let fake = Fake::new::<TranslationJob>(FakeEngine::new());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("outputs.json");
    eval::write(&path, &evaluate(Model::fake(&fake), &cases));
    let back: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let facts = [
        "profile",
        "model",
        "onGpu",
        "loadMs",
        "totalMs",
        "tokPerSec",
        "records",
    ];
    assert_eq!(
        keys(&back),
        BTreeSet::from(facts),
        "as the report reads them"
    );
    assert_eq!(back["model"], "fake");
    assert_eq!(back["onGpu"], Value::Null);
    let records = back["records"].as_array().unwrap();
    assert_eq!(records.len(), cases.len());
    let requests = fake.requests.lock().unwrap();
    assert_eq!(requests.len(), cases.len());
    let rules = prompt::SYSTEM.split_whitespace().count();
    for ((record, Asked { case, pair }), request) in records.iter().zip(&cases).zip(&*requests) {
        let fields = [
            "id",
            "kind",
            "from",
            "source",
            "target",
            "result",
            "ms",
            "promptTokens",
            "tokens",
            "maxTokens",
        ];
        assert_eq!(keys(record), BTreeSet::from(fields), "{}", case.id);
        assert_eq!(record["id"], case.id.as_str());
        assert_eq!(record["kind"], case.kind.as_str());
        assert_eq!(record["from"], case.from.as_str());
        assert_eq!(record["source"], serde_json::json!(case.source));
        assert_eq!(record["target"], case.target.as_str());
        // the rules, then the request for the case's languages with its piece: no turn between them
        let asked = prompt::user_turn(pair, &case.text);
        let turns: Vec<&str> = request
            .messages
            .iter()
            .map(|m| m.content.as_str())
            .collect();
        assert_eq!(turns, [prompt::SYSTEM, asked.as_str()], "{}", case.id);
        let echo: Vec<_> = asked.split_whitespace().collect();
        assert_eq!(record["result"]["kind"], "output", "{}", case.id);
        assert_eq!(record["result"]["text"], echo.join(" "), "{}", case.id);
        assert_eq!(record["tokens"], echo.len());
        assert_eq!(record["promptTokens"], rules + echo.len(), "{}", case.id);
        assert_eq!(record["maxTokens"], request.max_tokens);
        let piece = u32::try_from(case.text.split_whitespace().count()).unwrap();
        assert_eq!(request.max_tokens, prompt::max_tokens(piece), "{}", case.id);
    }
    drop(requests);
    assert_eq!(
        fake.engine.loaded(),
        0,
        "the model is unloaded after its cases"
    );
}

/// The request's languages are named as the app names them: the tag with its English name, the source only when the
/// case names one.
#[test]
fn eval_requests_name_the_languages_of_each_case() {
    let cases = asked();
    let of = |id: &str| cases.iter().find(|a| a.case.id == id).unwrap();
    let named = of("en-ptbr-plain");
    assert_eq!(
        prompt::user_turn(&named.pair, "Text."),
        "Translate from American English (en-US) to Brazilian Portuguese (pt-BR):\n\nText."
    );
    let detected = of("ja-en-plain");
    assert_eq!(
        prompt::user_turn(&detected.pair, "Text."),
        "Translate from the language it is written in to American English (en-US):\n\nText."
    );
}

/// Step 1 over the REAL worker + model: every case with the app's prompt -> outputs JSON at `PERMARK_EVAL_OUT`.
///
/// Env: `PERMARK_WORKER` = worker build (debug override), `PERMARK_EVAL_GGUF` = chat model, `PERMARK_EVAL_PROFILE` =
/// catalog prompt profile (default `gemma-4`), `PERMARK_EVAL_OUT` = outputs JSON path (one per model),
/// `PERMARK_SMOKE_DEVICE=auto` -> GPU (never without a go for it; default CPU).
///
/// Run (cwd `app/`), PowerShell:
/// ```text
/// $env:PERMARK_WORKER="<worker exe>"; $env:PERMARK_EVAL_GGUF="<model.gguf>"
/// $env:PERMARK_EVAL_OUT="F:/tmp/translation-e4b.json"
/// cargo test --manifest-path src-tauri/Cargo.toml -p permark-plugin-translation eval_the_real_model -- --ignored --nocapture
/// ```
/// bash:
/// ```text
/// PERMARK_WORKER=<worker exe> PERMARK_EVAL_GGUF=<model.gguf> PERMARK_EVAL_OUT=/f/tmp/translation-e4b.json ///   cargo test --manifest-path src-tauri/Cargo.toml -p permark-plugin-translation eval_the_real_model -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs a built worker + a local chat model (GPU never without a go for it); run by hand"]
fn eval_the_real_model() {
    let tmp = tempfile::tempdir().unwrap();
    let (model, out) = Model::real::<TranslationJob>(tmp.path());
    let outputs = evaluate(model, &asked());
    eval::write(&out, &outputs);
    println!("{} -> {}", outputs.summary(), out.display());
}
