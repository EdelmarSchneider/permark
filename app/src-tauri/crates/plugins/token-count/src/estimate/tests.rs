// Test code: counts and sizes here are small, and plain `assert!(x.is_empty())` reads best.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::items_after_statements,
    clippy::single_match_else
)]

use std::collections::BTreeMap;

use super::*;

#[derive(serde::Deserialize)]
struct Fixtures {
    claude: BTreeMap<String, u64>,
    cases: Vec<Case>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    text: String,
    estimates: BTreeMap<String, u64>,
}

fn fixtures() -> Fixtures {
    serde_json::from_str(include_str!("../../fixtures/estimate.json")).unwrap()
}

#[test]
fn every_family_gives_the_fixture_numbers() {
    let fixtures = fixtures();
    assert!(fixtures.cases.len() >= 50);
    for case in &fixtures.cases {
        let want = |family: &str| case.estimates[family];
        assert_eq!(
            permark_core::tokens::estimate(&case.text),
            want("generic"),
            "{} (generic)",
            case.name
        );
        for (tokenizer, family) in [
            (Tokenizer::Exact(Encoding::O200k), "o200k"),
            (Tokenizer::Exact(Encoding::Cl100k), "cl100k"),
            (Tokenizer::Gemini, "gemini"),
            (Tokenizer::Claude, "claude"),
            (Tokenizer::Chars4, "chars4"),
        ] {
            assert_eq!(
                estimate(tokenizer, &case.text),
                want(family),
                "{} ({family})",
                case.name
            );
        }
    }
}

#[test]
fn the_claude_weights_are_the_o200k_weights_times_the_factor() {
    let w = &fixtures().claude;
    assert_eq!(w["space"], CLAUDE.space);
    assert_eq!(w["upper"], CLAUDE.upper);
    assert_eq!(w["digit"], CLAUDE.digit);
    assert_eq!(w["latinExt"], CLAUDE.lat_ext);
    assert_eq!(w["cjk"], CLAUDE.cjk);
    assert_eq!(w["emoji"], CLAUDE.emoji);
    assert_eq!(w["word"], CLAUDE.word);
    assert_eq!(w["excess"], CLAUDE.excess);
    // every claude weight is the o200k one x 1.07, rounded
    let pairs = [
        (O200K_INTERIM.space, CLAUDE.space),
        (O200K_INTERIM.newline, CLAUDE.newline),
        (O200K_INTERIM.upper, CLAUDE.upper),
        (O200K_INTERIM.digit, CLAUDE.digit),
        (O200K_INTERIM.lat_ext, CLAUDE.lat_ext),
        (O200K_INTERIM.cjk, CLAUDE.cjk),
        (O200K_INTERIM.greek_cyr, CLAUDE.greek_cyr),
        (O200K_INTERIM.letter_other, CLAUDE.letter_other),
        (O200K_INTERIM.emoji, CLAUDE.emoji),
        (O200K_INTERIM.symbol, CLAUDE.symbol),
        (O200K_INTERIM.punct, CLAUDE.punct),
        (O200K_INTERIM.word, CLAUDE.word),
        (O200K_INTERIM.excess, CLAUDE.excess),
    ];
    for (base, scaled) in pairs {
        assert_eq!((base * 1070 + 500) / 1000, scaled);
    }
}

#[test]
fn claude_reads_english_prose_at_three_and_a_half_characters_per_token() {
    let text = std::fs::read_to_string(corpus("en-novel.txt")).unwrap();
    let per_token = text.chars().count() as f64 / estimate(Tokenizer::Claude, &text) as f64;
    assert!((3.45..3.55).contains(&per_token), "{per_token}");
}

#[test]
fn chars4_is_the_literal_rule() {
    assert_eq!(estimate(Tokenizer::Chars4, ""), 0);
    assert_eq!(estimate(Tokenizer::Chars4, "abcd"), 1);
    assert_eq!(estimate(Tokenizer::Chars4, "abcde"), 2);
    // scalar values, not bytes and not UTF-16 units
    assert_eq!(
        estimate(Tokenizer::Chars4, "\u{1f600}\u{1f600}\u{1f600}\u{1f600}"),
        1
    );
    assert_eq!(estimate(Tokenizer::Chars4, "\u{4f60}\u{597d}"), 1);
}

fn corpus(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join(name)
}

#[derive(serde::Deserialize)]
struct Counts {
    o200k: u64,
    cl100k: u64,
    gemini: u64,
}

#[derive(serde::Deserialize)]
struct Expected {
    files: BTreeMap<String, Counts>,
}

/// Mean absolute relative error of `tokenizer`'s estimate over the corpus files against `pick`.
fn mean_error(tokenizer: Tokenizer, pick: impl Fn(&Counts) -> u64) -> f64 {
    let expected: Expected =
        serde_json::from_str(&std::fs::read_to_string(corpus("expected.json")).unwrap()).unwrap();
    let mut sum = 0.0;
    for (name, counts) in &expected.files {
        let text = std::fs::read_to_string(corpus(name)).unwrap();
        let want = pick(counts) as f64;
        let got = estimate(tokenizer, &text) as f64;
        sum += (got - want).abs() / want.max(1.0);
    }
    sum / expected.files.len() as f64
}

#[test]
fn the_estimates_are_near_what_they_were_fitted_to() {
    // measured on the corpus (tools/calibrate.py): 7 %, 9 %, 7 %, and chars/4 vs o200k 28 %
    let o200k = mean_error(Tokenizer::Exact(Encoding::O200k), |c| c.o200k);
    let cl100k = mean_error(Tokenizer::Exact(Encoding::Cl100k), |c| c.cl100k);
    let gemini = mean_error(Tokenizer::Gemini, |c| c.gemini);
    let rule = mean_error(Tokenizer::Chars4, |c| c.o200k);
    assert!(o200k < 0.09, "o200k {o200k}");
    assert!(cl100k < 0.11, "cl100k {cl100k}");
    assert!(gemini < 0.09, "gemini {gemini}");
    // the calibrated estimates beat the rule of thumb on this corpus
    assert!(rule > 2.0 * o200k, "rule {rule}");
}

#[test]
fn json_of_an_answer_costs_about_what_real_tokenizers_say_over_its_tab_table() {
    // the same 28 answers as tab table and as compact JSON; o200k (and two local models' tokenizers) count the JSON
    // ~43 % bigger
    let answers: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../calibration/agent-answers.json")).unwrap();
    let sum = |suffix: &str| -> u64 {
        answers
            .iter()
            .filter(|(name, _)| name.ends_with(suffix))
            .map(|(_, text)| permark_core::tokens::estimate(text))
            .sum()
    };
    let (table, json) = (sum(".table"), sum(".json"));
    let ratio = json as f64 / table as f64;
    assert!((1.35..=1.55).contains(&ratio), "json / table = {ratio}");
}

#[test]
fn a_tab_and_joined_punctuation_are_features_of_their_own() {
    let f = permark_core::tokens::Features::of("a\tb \":\" x");
    assert_eq!((f.tab, f.space, f.punct, f.punct_join), (1, 2, 1, 2));
}
