//! The documents of the editor's round-trip test, with the block ranges the real parser finds in them (test only).
//!
//! `src/lib/editor/corpus.test.ts` loads every document of the corpus into the block model, saves it without an
//! edit and with edits, and needs the real ranges for that: vitest cannot call Rust, and the line-based splitter
//! of the browser mock is only an approximation. So this test parses the corpus with [`parse_blocks`] and keeps the
//! result in `src/lib/editor/corpus/ranges.json`; the vitest suite reads it. Both sides check that the file is
//! current, so a change to the parser that moves a range fails here until the file is regenerated:
//!
//! 1. `UPDATE_CORPUS=1 npx vitest run src/lib/editor/corpus.test.ts` writes `corpus/inline.json`, the texts of the
//!    hostile set and of the mock documents (they live in TypeScript);
//! 2. `UPDATE_CORPUS=1 cargo test --manifest-path src-tauri/Cargo.toml corpus` writes `corpus/ranges.json`.
//!
//! The project's own documents are a frozen snapshot in `src/lib/editor/corpus/docs/` (the README, the changelog,
//! the contributing guide and the design notes as they were when the snapshot was taken). Neither this test nor the
//! vitest suite reads the live files, so editing them never makes the fixtures stale, and the corpus is the same on
//! every checkout. To refresh the snapshot on purpose, copy the current files over it, then run the two commands
//! above:
//!
//! ```text
//! cp README.md CHANGELOG.md CONTRIBUTING.md src/lib/editor/corpus/docs/
//! cp docs/design/*.md src/lib/editor/corpus/docs/design/
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::blocks::{parse_blocks, BlockKind};

const INLINE: &str = "src/lib/editor/corpus/inline.json";
const RANGES: &str = "src/lib/editor/corpus/ranges.json";
const FROZEN: &str = "src/lib/editor/corpus/docs";
const BOM: char = '\u{FEFF}';

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A fingerprint of a text (FNV-1a over its UTF-8 bytes) that the TypeScript side computes the same way. It
/// tells the fixture's reader that a document has changed since the ranges were made.
fn fingerprint(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// The frozen snapshot of the project's own documents (the README, the changelog, the contributing guide and the
/// design notes), with names relative to the snapshot folder (`README.md`, `design/v0.2.md`). The live files are
/// never read, so editing them cannot make the fixtures stale.
pub(super) fn frozen_docs() -> Vec<(String, String)> {
    let root = repository_root().join(FROZEN);
    let mut files: Vec<String> = Vec::new();
    let mut stack = vec![String::new()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(root.join(&dir))
            .into_iter()
            .flatten()
            .flatten()
        {
            let file = entry.file_name().to_string_lossy().into_owned();
            let name = if dir.is_empty() {
                file
            } else {
                format!("{dir}/{file}")
            };
            if entry.path().is_dir() {
                stack.push(name);
            } else if Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            {
                files.push(name);
            }
        }
    }
    files.sort();
    files
        .into_iter()
        .filter_map(|name| {
            let text = std::fs::read_to_string(root.join(&name)).ok()?;
            Some((name, text))
        })
        .collect()
}

/// The hostile and mock documents, as the TypeScript suite wrote them.
fn inline_docs() -> BTreeMap<String, String> {
    let path = repository_root().join(INLINE);
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nrun `UPDATE_CORPUS=1 npx vitest run src/lib/editor/corpus.test.ts` first",
            path.display()
        )
    });
    serde_json::from_str(&json).expect("inline.json is a map from names to texts")
}

/// Every document of the corpus once as written, and once the way a Windows editor saves it (CRLF and a BOM).
fn corpus() -> BTreeMap<String, String> {
    let mut docs: BTreeMap<String, String> = frozen_docs().into_iter().collect();
    docs.extend(inline_docs());
    let windows: Vec<(String, String)> = docs
        .iter()
        .map(|(name, text)| {
            let unified = text.replace("\r\n", "\n").replace('\r', "\n");
            (
                format!("{name} (CRLF, BOM)"),
                format!("{BOM}{}", unified.replace('\n', "\r\n")),
            )
        })
        .collect();
    docs.extend(windows);
    docs
}

fn kind_name(kind: BlockKind) -> Value {
    serde_json::to_value(kind).expect("a kind is a string")
}

/// The fixture: one line per document with its size, fingerprint, line ending and block ranges.
fn fixture() -> String {
    let lines: Vec<String> = corpus()
        .iter()
        .map(|(name, text)| {
            let parsed = parse_blocks(text);
            let blocks: Vec<Value> = parsed
                .blocks
                .iter()
                .map(|b| json!([b.start_byte, b.end_byte, kind_name(b.kind)]))
                .collect();
            let entry = json!({
                "name": name,
                "bytes": text.len(),
                "fnv": fingerprint(text),
                "eol": serde_json::to_value(parsed.eol).expect("an eol is a string"),
                "blocks": blocks,
            });
            format!("  {entry}")
        })
        .collect();
    format!("[\n{}\n]\n", lines.join(",\n"))
}

#[test]
fn the_corpus_has_the_documents_it_is_meant_to_have() {
    let docs = frozen_docs();
    let names: Vec<&str> = docs.iter().map(|(n, _)| n.as_str()).collect();
    for expected in ["README.md", "CHANGELOG.md", "CONTRIBUTING.md"] {
        assert!(
            names.contains(&expected),
            "{expected} is missing: {names:?}"
        );
    }
    assert!(names.iter().any(|n| n.starts_with("design/")), "{names:?}");
    let all = corpus();
    assert!(all.len() > 100, "only {} documents", all.len());
}

#[test]
fn the_ranges_fixture_matches_what_the_parser_finds() {
    let path = repository_root().join(RANGES);
    let actual = fixture();
    if std::env::var("UPDATE_CORPUS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, &actual).expect("write the fixture");
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    // the committed file may have been checked out with other line endings
    assert!(
        committed.replace("\r\n", "\n") == actual,
        "{RANGES} is out of date: the parser or a corpus document changed.\n\
         Regenerate it: UPDATE_CORPUS=1 npx vitest run src/lib/editor/corpus.test.ts && \
         UPDATE_CORPUS=1 cargo test --manifest-path src-tauri/Cargo.toml corpus"
    );
}

#[test]
fn every_document_of_the_corpus_splits_into_ordered_blocks_on_character_boundaries() {
    for (name, text) in corpus() {
        let mut at = 0;
        for b in parse_blocks(&text).blocks {
            assert!(
                b.start_byte >= at && b.start_byte < b.end_byte && b.end_byte <= text.len(),
                "{name}: block {}..{} after {at}",
                b.start_byte,
                b.end_byte
            );
            assert!(
                text.is_char_boundary(b.start_byte) && text.is_char_boundary(b.end_byte),
                "{name}: a block cuts a character in two"
            );
            at = b.end_byte;
        }
    }
}
