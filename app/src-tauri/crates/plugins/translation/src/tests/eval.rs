//! Eval set, step 1 of 2: every case of `app/src/plugins/translation/eval/cases.json` through the production piece
//! path of [`TranslationJob`] (same prompt, sampling, answer cap, too-long + length + thinking discards) for the
//! languages of the case -> outputs JSON, on the eval harness of the piece runner's test kit ([`eval`]). Step 2 (TS
//! report: restore + guards + what was expected of each case) = `app/src/plugins/translation/eval/eval.test.ts`.
//!
//! - Always run: cases file shape; every case over [`FakeEngine`] into a temp outputs file, once per prompt variant
//!   (harness check, no model).
//! - `#[ignore]`, by hand: [`eval_the_real_model`], the REAL worker + a local chat GGUF.
//! - Prompt variants ([`Variant`]): the app's prompt (`fewShot`) | the same without its example turns (`zeroShot`,
//!   [`ZeroShot`]: built here, never the app's). The outputs file says which one made it.

use std::collections::{BTreeMap, BTreeSet};

use permark_plugin_local_ai::engine::{ChatMessage, FakeEngine, Sampling};
use permark_plugin_local_ai::pieces::testkit::eval::{self, Model, Outputs};
use permark_plugin_local_ai::pieces::testkit::Fake;
use permark_plugin_local_ai::pieces::{PieceJob, PieceResult};
use permark_plugin_local_ai::Task;
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
/// Picks the prompt variant of [`eval_the_real_model`].
const VARIANT: &str = "PERMARK_EVAL_VARIANT";

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

/// Outputs file (read by the TS report): the prompt variant + the run.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Run {
    variant: Variant,
    #[serde(flatten)]
    outputs: Outputs<Record>,
}

/// Prompt of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum Variant {
    /// The app's: rules + example turns + request.
    FewShot,
    /// Rules + request ([`ZeroShot`]).
    ZeroShot,
}

impl Variant {
    /// From [`VARIANT`] (not set: the app's prompt).
    fn from_env() -> Self {
        match std::env::var(VARIANT).ok().as_deref() {
            None | Some("fewShot") => Self::FewShot,
            Some("zeroShot") => Self::ZeroShot,
            Some(other) => panic!("{VARIANT}={other}: fewShot | zeroShot"),
        }
    }
}

/// The translation job without its example turns: what a model does on the rules + the request alone. Compared with
/// the app's prompt on the same cases; test-only.
struct ZeroShot;

impl PieceJob for ZeroShot {
    type Params = Pair;

    const TASK: Task = TranslationJob::TASK;
    const CONTEXT: u32 = TranslationJob::CONTEXT;
    const SAMPLING: Sampling = TranslationJob::SAMPLING;

    fn messages(pair: &Pair, text: &str) -> Vec<ChatMessage> {
        let mut turns = TranslationJob::messages(pair, text);
        // the rules open, the request closes: the examples are every turn between them
        turns.drain(1..turns.len() - 1);
        turns
    }

    fn preamble(pair: &Pair) -> String {
        format!("{}\n{}", prompt::SYSTEM, prompt::user_turn(pair, ""))
    }

    fn max_tokens(piece_tokens: u32) -> u32 {
        TranslationJob::max_tokens(piece_tokens)
    }
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

/// Every case in order on `model` with the prompt of job `J`.
fn answered<J: PieceJob<Params = Pair>>(model: Model, cases: &[Asked]) -> Outputs<Record> {
    eval::evaluate::<J, _, _>(
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

/// The run of `variant` over `cases` on `model`.
fn run(variant: Variant, model: Model, cases: &[Asked]) -> Run {
    let outputs = match variant {
        Variant::FewShot => answered::<TranslationJob>(model, cases),
        Variant::ZeroShot => answered::<ZeroShot>(model, cases),
    };
    Run { variant, outputs }
}

fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}

/// Cases file: about 40 cases, unique ids, every kind and every pair, tags of the crate's own shape and names the
/// prompt takes, markers exact `⟦1⟧…⟦n⟧`, expectations that can hold.
#[test]
fn eval_cases_are_well_formed() {
    let Set { languages, cases } = set();
    assert!((36..=48).contains(&cases.len()), "{} cases", cases.len());
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

/// Harness without a model: every case over [`FakeEngine`] (echo of the request) with each prompt variant -> an
/// outputs file naming the variant, one record per case with its pair, its token counts and the runner's result.
#[test]
fn eval_runs_every_case_over_the_fake_engine_into_an_outputs_file() {
    let cases = asked();
    let tmp = tempfile::tempdir().unwrap();
    let mut prompts = Vec::new();
    for (variant, name, turns) in [
        (Variant::FewShot, "fewShot", 2 + 2 * prompt::FEW_SHOT.len()),
        (Variant::ZeroShot, "zeroShot", 2),
    ] {
        let fake = Fake::new::<TranslationJob>(FakeEngine::new());
        let path = tmp.path().join(format!("{name}.json"));
        eval::write(&path, &run(variant, Model::fake(&fake), &cases));
        let back: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let facts = [
            "variant",
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
        assert_eq!(back["variant"], name);
        assert_eq!(back["model"], "fake");
        assert_eq!(back["onGpu"], Value::Null);
        let records = back["records"].as_array().unwrap();
        assert_eq!(records.len(), cases.len());
        let requests = fake.requests.lock().unwrap();
        assert_eq!(requests.len(), cases.len());
        for ((record, Asked { case, pair }), request) in records.iter().zip(&cases).zip(&*requests)
        {
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
            // the rules, the examples of the variant, then the request for the case's languages with its piece
            let asked = prompt::user_turn(pair, &case.text);
            assert_eq!(request.messages.len(), turns, "{}", case.id);
            assert_eq!(request.messages[0].content, prompt::SYSTEM);
            assert_eq!(request.messages[turns - 1].content, asked);
            let echo: Vec<_> = asked.split_whitespace().collect();
            assert_eq!(record["result"]["kind"], "output", "{}", case.id);
            assert_eq!(record["result"]["text"], echo.join(" "), "{}", case.id);
            assert_eq!(record["tokens"], echo.len());
            assert_eq!(record["maxTokens"], request.max_tokens);
            let piece = u32::try_from(case.text.split_whitespace().count()).unwrap();
            assert_eq!(request.max_tokens, prompt::max_tokens(piece), "{}", case.id);
        }
        prompts.push(
            records
                .iter()
                .map(|r| r["promptTokens"].as_u64().unwrap())
                .collect::<Vec<_>>(),
        );
        drop(requests);
        assert_eq!(
            fake.engine.loaded(),
            0,
            "the model is unloaded after its cases"
        );
    }
    // the examples are the whole difference between the two prompts
    let examples: usize = prompt::FEW_SHOT
        .iter()
        .map(|(user, answer)| user.split_whitespace().count() + answer.split_whitespace().count())
        .sum();
    for (few, zero) in prompts[0].iter().zip(&prompts[1]) {
        assert_eq!(*few, zero + examples as u64);
    }
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

/// Step 1 over the REAL worker + model: every case with the prompt variant `PERMARK_EVAL_VARIANT` names (`fewShot`,
/// the app's prompt: default | `zeroShot`: no example turns) -> outputs JSON at `PERMARK_EVAL_OUT`.
///
/// Env: `PERMARK_WORKER` = worker build (debug override), `PERMARK_EVAL_GGUF` = chat model, `PERMARK_EVAL_PROFILE` =
/// catalog prompt profile (default `gemma-4`), `PERMARK_EVAL_OUT` = outputs JSON path (one per model + variant),
/// `PERMARK_EVAL_VARIANT`, `PERMARK_SMOKE_DEVICE=auto` -> GPU (never without a go for it; default CPU).
///
/// Run (cwd `app/`), PowerShell:
/// ```text
/// $env:PERMARK_WORKER="<worker exe>"; $env:PERMARK_EVAL_GGUF="<model.gguf>"
/// $env:PERMARK_EVAL_VARIANT="fewShot"; $env:PERMARK_EVAL_OUT="F:/tmp/translation-e2b-fewShot.json"
/// cargo test --manifest-path src-tauri/Cargo.toml -p permark-plugin-translation eval_the_real_model -- --ignored --nocapture
/// $env:PERMARK_EVAL_VARIANT="zeroShot"; $env:PERMARK_EVAL_OUT="F:/tmp/translation-e2b-zeroShot.json"
/// cargo test --manifest-path src-tauri/Cargo.toml -p permark-plugin-translation eval_the_real_model -- --ignored --nocapture
/// ```
/// bash:
/// ```text
/// PERMARK_WORKER=<worker exe> PERMARK_EVAL_GGUF=<model.gguf> PERMARK_EVAL_VARIANT=zeroShot \
///   PERMARK_EVAL_OUT=/f/tmp/translation-e2b-zeroShot.json \
///   cargo test --manifest-path src-tauri/Cargo.toml -p permark-plugin-translation eval_the_real_model -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs a built worker + a local chat model (GPU never without a go for it); run by hand"]
fn eval_the_real_model() {
    let variant = Variant::from_env();
    let cases = asked();
    let tmp = tempfile::tempdir().unwrap();
    let (model, out) = Model::real::<TranslationJob>(tmp.path());
    let run = run(variant, model, &cases);
    eval::write(&out, &run);
    println!(
        "{} [{variant:?}] -> {}",
        run.outputs.summary(),
        out.display()
    );
}
