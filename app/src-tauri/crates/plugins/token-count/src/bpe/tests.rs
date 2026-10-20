// Test code: counts and sizes here are small, and plain `assert!(x.is_empty())` reads best.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::items_after_statements,
    clippy::single_match_else
)]

use super::*;

/// A rank file for `tokens` (rank = position).
fn rank_file(tokens: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for (rank, token) in tokens.iter().enumerate() {
        out.extend_from_slice(STANDARD.encode(token).as_bytes());
        out.extend_from_slice(format!(" {rank}\n").as_bytes());
    }
    out
}

fn single_bytes() -> Vec<Vec<u8>> {
    (0..=255_u8).map(|b| vec![b]).collect()
}

/// Every string over `a b c` up to `max` bytes, after the 256 single bytes, in a fixed shuffled order.
fn synthetic_ranks(max: usize) -> Ranks {
    let mut tokens = single_bytes();
    let mut longer: Vec<Vec<u8>> = Vec::new();
    let mut level: Vec<Vec<u8>> = vec![Vec::new()];
    for _ in 0..max {
        let mut next = Vec::new();
        for base in &level {
            for letter in *b"abc" {
                let mut t = base.clone();
                t.push(letter);
                next.push(t);
            }
        }
        longer.extend(next.iter().filter(|t| t.len() > 1).cloned());
        level = next;
    }
    // a fixed shuffle (no randomness crate): multiply by a number coprime to the length
    let n = longer.len();
    let mut order: Vec<usize> = (0..n).map(|i| (i * 7919 + 13) % n).collect();
    order.dedup();
    let mut seen = vec![false; n];
    for i in order {
        if !seen[i] {
            seen[i] = true;
            tokens.push(longer[i].clone());
        }
    }
    for (i, t) in longer.iter().enumerate() {
        if !seen[i] {
            tokens.push(t.clone());
        }
    }
    Ranks::parse(&rank_file(&tokens)).unwrap()
}

#[test]
fn rank_files_are_checked() {
    assert!(matches!(Ranks::parse(b""), Err(RanksError::Size)));
    assert!(matches!(
        Ranks::parse(b"YQ== 0\nYg== 2\n"),
        Err(RanksError::Malformed { line: 2 })
    ));
    assert!(matches!(
        Ranks::parse(b"YQ== 0\n!!!! 1\n"),
        Err(RanksError::Malformed { line: 2 })
    ));
    assert!(matches!(
        Ranks::parse(b"YQ==0\n"),
        Err(RanksError::Malformed { line: 1 })
    ));
    let ranks = Ranks::parse(b"YQ== 0\r\nYg== 1\r\nYWI= 2\r\n").unwrap();
    assert_eq!(ranks.len(), 3);
    assert_eq!(ranks.rank(b"a"), Some(0));
    assert_eq!(ranks.rank(b"ab"), Some(2));
    assert_eq!(ranks.rank(b"ba"), None);
    assert_eq!(ranks.rank(b""), None);
}

#[test]
fn lookup_finds_every_token_among_many() {
    let mut tokens = single_bytes();
    for i in 0..5_000_u32 {
        tokens.push(format!("token{i}").into_bytes());
    }
    let ranks = Ranks::parse(&rank_file(&tokens)).unwrap();
    for (rank, token) in tokens.iter().enumerate() {
        assert_eq!(ranks.rank(token), Some(rank as u32));
    }
    assert_eq!(ranks.rank(b"token5000"), None);
}

#[test]
fn merging_takes_the_lowest_rank_pair_first_and_the_leftmost_on_a_tie() {
    let mut tokens = single_bytes();
    // "bc" merges before "ab"; "aa" is a token, "aaa" is not
    for t in ["bc", "ab", "aa", "abc"] {
        tokens.push(t.as_bytes().to_vec());
    }
    let ranks = Ranks::parse(&rank_file(&tokens)).unwrap();
    // abc is a token itself: one
    assert_eq!(scan_merge(&ranks, b"abc"), 1);
    // "abcb": bc first (rank 256) -> a, bc, b; then "abc" needs a + bc = "abc" (token) -> abc, b = 2
    assert_eq!(scan_merge(&ranks, b"abcb"), 2);
    // "aaa": leftmost pair first -> aa, a = 2
    assert_eq!(scan_merge(&ranks, b"aaa"), 2);
    // "aaaa": aa, aa = 2
    assert_eq!(scan_merge(&ranks, b"aaaa"), 2);
    // no token among the pairs: every byte alone
    assert_eq!(scan_merge(&ranks, b"xyz"), 3);
    for piece in [&b"abc"[..], b"abcb", b"aaa", b"aaaa", b"xyz"] {
        assert_eq!(heap_merge(&ranks, piece), scan_merge(&ranks, piece));
    }
}

#[test]
fn the_heap_and_the_scan_agree_on_long_random_pieces() {
    let ranks = synthetic_ranks(6);
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for round in 0..600 {
        let len = 2 + (next() % 400) as usize;
        let alphabet: &[u8] = if round % 3 == 0 { b"ab" } else { b"abc" };
        let piece: Vec<u8> = (0..len)
            .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
            .collect();
        assert_eq!(
            heap_merge(&ranks, &piece),
            scan_merge(&ranks, &piece),
            "{}",
            String::from_utf8_lossy(&piece)
        );
    }
}

#[test]
fn counting_uses_the_splitting_rules_and_the_cache() {
    let ranks = synthetic_ranks(3);
    let bpe = Bpe::new(ranks, Rules::Cl100k);
    // "abc abc": pieces "abc", " abc": the first is a token; " abc" has the bytes ' ' + "abc"
    assert_eq!(bpe.count(""), 0);
    assert_eq!(bpe.count("abc"), 1);
    assert_eq!(
        bpe.count("abc abc"),
        1 + u64::from(scan_merge(bpe.ranks(), b" abc"))
    );
    // a piece longer than the cache limit is counted too
    let long = "ab".repeat(40);
    assert!(bpe.count(&long) >= 1);
}

// ---- the reference counts (needs the downloaded rank files) -------------------------------------------------------

#[derive(serde::Deserialize)]
struct FileCounts {
    o200k: u64,
    cl100k: u64,
}

#[derive(serde::Deserialize)]
struct Expected {
    files: std::collections::BTreeMap<String, FileCounts>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    text: String,
    o200k: u64,
    cl100k: u64,
}

/// The folder with `o200k_base.tiktoken` and `cl100k_base.tiktoken`: `PERMARK_TOKEN_VOCAB_DIR`, else the cache of
/// the development profile (where the app puts them after the download).
fn vocab_dir() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("PERMARK_TOKEN_VOCAB_DIR") {
        return Some(dir.into());
    }
    let dirs = permark_core::state::AppDirs::system()?;
    let dir = dirs.cache.join("plugins").join("token-count");
    (dir.join("o200k_base.tiktoken").is_file() && dir.join("cl100k_base.tiktoken").is_file())
        .then_some(dir)
}

fn load(dir: &std::path::Path, file: &str, rules: Rules) -> Bpe {
    let bytes = std::fs::read(dir.join(file)).unwrap();
    let start = std::time::Instant::now();
    let ranks = Ranks::parse(&bytes).unwrap();
    eprintln!(
        "{file}: {} tokens parsed in {:?}",
        ranks.len(),
        start.elapsed()
    );
    Bpe::new(ranks, rules)
}

fn reference_vocab() -> Option<(Bpe, Bpe)> {
    let Some(dir) = vocab_dir() else {
        assert!(
            std::env::var_os("PERMARK_TOKEN_VOCAB_REQUIRED").is_none(),
            "the rank files are required here (PERMARK_TOKEN_VOCAB_DIR)"
        );
        eprintln!("skipped: the rank files are not on this computer (set PERMARK_TOKEN_VOCAB_DIR)");
        return None;
    };
    Some((
        load(&dir, "o200k_base.tiktoken", Rules::O200k),
        load(&dir, "cl100k_base.tiktoken", Rules::Cl100k),
    ))
}

fn corpus_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

#[test]
fn the_corpus_counts_match_the_reference() {
    let Some((o200k, cl100k)) = reference_vocab() else {
        return;
    };
    let expected: Expected =
        serde_json::from_str(&std::fs::read_to_string(corpus_dir().join("expected.json")).unwrap())
            .unwrap();
    assert!(expected.files.len() >= 30);
    for (name, counts) in &expected.files {
        let text = std::fs::read_to_string(corpus_dir().join(name)).unwrap();
        assert_eq!(o200k.count(&text), counts.o200k, "{name} (o200k)");
        assert_eq!(cl100k.count(&text), counts.cl100k, "{name} (cl100k)");
    }
}

#[test]
fn the_short_cases_match_the_reference() {
    let Some((o200k, cl100k)) = reference_vocab() else {
        return;
    };
    let cases: Vec<Case> =
        serde_json::from_str(&std::fs::read_to_string(corpus_dir().join("cases.json")).unwrap())
            .unwrap();
    assert!(cases.len() >= 50);
    for case in cases {
        assert_eq!(o200k.count(&case.text), case.o200k, "{} (o200k)", case.name);
        assert_eq!(
            cl100k.count(&case.text),
            case.cl100k,
            "{} (cl100k)",
            case.name
        );
    }
}

/// One megabyte of mixed text counts in time (release builds only: the debug build is not the product).
#[test]
fn one_megabyte_is_counted_quickly() {
    let Some((o200k, cl100k)) = reference_vocab() else {
        return;
    };
    // PERMARK_TOKEN_BENCH_FILE: a real, varied text to time (the corpus repeated has far fewer different pieces)
    let mut text = match std::env::var_os("PERMARK_TOKEN_BENCH_FILE") {
        Some(path) => std::fs::read_to_string(path).unwrap(),
        None => {
            let mut text = String::new();
            for name in [
                "en-novel.txt",
                "code.rs",
                "en-markdown.md",
                "de-novel.txt",
                "data.log",
            ] {
                text.push_str(&std::fs::read_to_string(corpus_dir().join(name)).unwrap());
            }
            text
        }
    };
    while text.len() < 1_000_000 {
        let copy = text.clone();
        text.push_str(&copy);
    }
    let mut cut = 1_000_000;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text.truncate(cut);
    for (name, bpe) in [("o200k", &o200k), ("cl100k", &cl100k)] {
        // the first call builds nothing more: the tables are loaded; time the count itself, best of 3
        let mut best = std::time::Duration::MAX;
        let mut tokens = 0;
        for _ in 0..3 {
            let start = std::time::Instant::now();
            tokens = bpe.count(&text);
            best = best.min(start.elapsed());
        }
        eprintln!("1 MB, {name}: {tokens} tokens in {best:?}");
        assert!(tokens > 100_000);
        if !cfg!(debug_assertions) {
            assert!(best.as_millis() < 150, "{name}: {best:?}");
        }
    }
}

/// Random texts against the reference (`tools/reference_fuzz.py` writes the file; set `PERMARK_TOKEN_FUZZ_FILE`).
#[test]
fn fuzz_against_the_reference() {
    let Some(path) = std::env::var_os("PERMARK_TOKEN_FUZZ_FILE") else {
        return;
    };
    let Some((o200k, cl100k)) = reference_vocab() else {
        return;
    };
    #[derive(serde::Deserialize)]
    struct Row {
        text: String,
        o200k: u64,
        cl100k: u64,
    }
    let file = std::fs::read_to_string(path).unwrap();
    let mut wrong = Vec::new();
    let mut rows = 0;
    for line in file.lines() {
        let row: Row = serde_json::from_str(line).unwrap();
        rows += 1;
        let (a, b) = (o200k.count(&row.text), cl100k.count(&row.text));
        if (a, b) != (row.o200k, row.cl100k) {
            wrong.push(format!(
                "{:?} o200k {a} != {} | cl100k {b} != {}",
                row.text, row.o200k, row.cl100k
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {rows} differ, first: {:#?}",
        wrong.len(),
        &wrong[..wrong.len().min(8)]
    );
}

/// Inputs built to be slow: long runs with no space to split on, one huge piece, whitespace, marks.
#[test]
fn adversarial_texts_stay_linear_enough() {
    let Some((o200k, cl100k)) = reference_vocab() else {
        return;
    };
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let letters: String = (0..1_000_000)
        .map(|_| char::from(b'a' + (next() % 26) as u8))
        .collect();
    let alnum: String = (0..1_000_000)
        .map(|_| {
            char::from(
                b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
                    [(next() % 62) as usize],
            )
        })
        .collect();
    let texts = [
        ("random letters, one piece", letters),
        ("random alphanumerics, one piece", alnum),
        ("one letter repeated", "a".repeat(1_000_000)),
        ("a pair repeated", "ab".repeat(500_000)),
        ("spaces", " ".repeat(1_000_000)),
        ("newlines", "\n".repeat(1_000_000)),
        ("space and newline", " \n".repeat(500_000)),
        ("punctuation", "!?".repeat(500_000)),
        ("marks", "e\u{301}".repeat(300_000)),
        (
            "capitals then a letter",
            format!("\u{4f60}{}", "A".repeat(500_000)),
        ),
        ("capital words", "AbCdEf ".repeat(140_000)),
    ];
    for (name, text) in &texts {
        for (encoding, bpe) in [("o200k", &o200k), ("cl100k", &cl100k)] {
            let start = std::time::Instant::now();
            let tokens = bpe.count(text);
            let took = start.elapsed();
            eprintln!("{name} / {encoding}: {tokens} tokens in {took:?}");
            assert!(tokens > 0);
            if !cfg!(debug_assertions) {
                assert!(took.as_secs_f64() < 3.0, "{name} / {encoding}: {took:?}");
            }
        }
    }
}
