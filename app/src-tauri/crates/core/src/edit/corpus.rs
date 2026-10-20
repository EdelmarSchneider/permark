//! Round-trip corpus: every document of `app/testdata/edit-corpus` (the old app's corpus inputs: hostile cases, mock
//! documents, a frozen snapshot of project documents) saved untouched and with one block edited at a time.
//!
//! Also keeps `segments.json` (text + block map per document) for the frontend's Edit-mode corpus test: vitest
//! cannot call Rust. Stale fixture -> this test fails; `UPDATE_CORPUS=1 cargo test corpus` rewrites it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::tests::Fx;
use super::*;
use crate::format::Segment;

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../testdata/edit-corpus")
}

fn collect(dir: &Path, base: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, base, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(format!("file {rel}"), std::fs::read(&path).unwrap());
        }
    }
}

/// Name -> file bytes.
pub(super) fn corpus() -> BTreeMap<String, Vec<u8>> {
    let dir = corpus_dir();
    let inline: BTreeMap<String, String> =
        serde_json::from_slice(&std::fs::read(dir.join("inline.json")).unwrap()).unwrap();
    let mut out: BTreeMap<String, Vec<u8>> = inline
        .into_iter()
        .map(|(name, text)| (name, text.into_bytes()))
        .collect();
    collect(&dir.join("docs"), &dir.join("docs"), &mut out);
    // encodings the inline set cannot hold
    out.insert(
        "utf16 le with crlf".to_owned(),
        lines::encode(
            "# Título\r\n\r\nTexto 文\r\n\r\n- a\r\n- b\r\n",
            Encoding::Utf16Le,
        ),
    );
    out.insert(
        "utf16 be without final newline".to_owned(),
        lines::encode("para one\n\npara two", Encoding::Utf16Be),
    );
    out
}

#[derive(Serialize)]
struct Fixture {
    text: String,
    segments: Vec<Segment>,
}

fn file_name(i: usize) -> String {
    format!("doc {i} ç.md")
}

#[test]
fn the_corpus_holds_the_old_cases() {
    let corpus = corpus();
    assert!(corpus.len() >= 74, "{} documents", corpus.len());
    assert!(corpus.keys().any(|k| k.contains("mixed line endings")));
    assert!(corpus.keys().any(|k| k.starts_with("file design/")));
}

#[test]
fn every_document_saved_untouched_is_byte_identical() {
    let fx = Fx::new();
    for (i, (name, bytes)) in corpus().iter().enumerate() {
        let file = file_name(i);
        fx.write(&file, bytes);
        let source = match fx.editor().source(&file) {
            Ok(source) => source,
            Err(e) => panic!("{name}: {e:?}"),
        };
        let out = fx.editor().save(&file, &source.hash, &[]).unwrap();
        assert!(matches!(out, SaveOutcome::Saved { .. }), "{name}");
        assert_eq!(&fx.read(&file), bytes, "{name}");
    }
}

#[test]
fn every_block_map_covers_each_filled_line_once_in_order() {
    for (name, bytes) in corpus() {
        let text = lines::normalized(&fsx::decode(&bytes).unwrap().text);
        let all: Vec<&str> = text.split('\n').collect();
        let segments = crate::format::segments(&text, super::tests::formats().features());
        let mut covered = vec![false; all.len()];
        let mut last = 0;
        for s in &segments {
            assert!(s.line_start > last, "{name}: order at {s:?}");
            assert!(s.line_end >= s.line_start, "{name}");
            assert!((s.line_end as usize) <= all.len(), "{name}");
            for line in s.line_start..=s.line_end {
                covered[line as usize - 1] = true;
            }
            last = s.line_end;
        }
        for (i, line) in all.iter().enumerate() {
            assert!(
                covered[i] || line.trim().is_empty(),
                "{name}: line {} not in a block: {line:?}",
                i + 1
            );
        }
    }
}

#[test]
fn every_one_block_edit_changes_only_that_blocks_bytes() {
    let fx = Fx::new();
    let mut cases = 0;
    for (i, (name, bytes)) in corpus().iter().enumerate() {
        let file = file_name(i);
        let decoded = fsx::decode(bytes).unwrap();
        let encoding = decoded.encoding;
        let original_lines = lines::split(&decoded.text);
        let source_text = lines::normalized(&decoded.text);
        let segments = crate::format::segments(&source_text, super::tests::formats().features());
        for segment in segments.iter().take(40) {
            fx.write(&file, bytes);
            let hash = fsx::hash(bytes).hex();
            let (s, e) = (segment.line_start as usize, segment.line_end as usize);
            let mut insert: Vec<String> = original_lines[s - 1..e]
                .iter()
                .map(|l| l.text.to_owned())
                .collect();
            if let Some(last) = insert.last_mut() {
                last.push_str(" ✎");
            }
            let edits = [LineEdit {
                at: u32::try_from(s - 1).unwrap(),
                remove: u32::try_from(e - s + 1).unwrap(),
                insert,
            }];
            let out = fx.editor().save(&file, &hash, &edits).unwrap();
            assert!(matches!(out, SaveOutcome::Saved { .. }), "{name}");
            let written = fsx::decode(&fx.read(&file)).unwrap();
            assert_eq!(written.encoding, encoding, "{name}");
            // text before the block's first line and after its last line's ending: unchanged
            let joined = |lines: &[lines::Line<'_>]| {
                lines.iter().fold(String::new(), |mut acc, l| {
                    acc.push_str(l.text);
                    acc.push_str(l.ending);
                    acc
                })
            };
            let prefix = joined(&original_lines[..s - 1]);
            let suffix = joined(&original_lines[e..]);
            assert!(
                written.text.starts_with(&prefix),
                "{name}: prefix of block {s}"
            );
            assert!(
                written.text.ends_with(&suffix),
                "{name}: suffix of block {s}"
            );
            assert!(written.text.contains(" ✎"), "{name}");
            cases += 1;
        }
    }
    assert!(cases > 500, "{cases} block edits");
}

#[test]
fn the_frontend_fixture_is_current() {
    let mut fixture: BTreeMap<String, Fixture> = BTreeMap::new();
    for (name, bytes) in corpus() {
        let text = lines::normalized(&fsx::decode(&bytes).unwrap().text);
        let segments = crate::format::segments(&text, super::tests::formats().features());
        fixture.insert(name, Fixture { text, segments });
    }
    let mut json = serde_json::to_string_pretty(&fixture).unwrap();
    json.push('\n');
    let path = corpus_dir().join("segments.json");
    if std::env::var_os("UPDATE_CORPUS").is_some() {
        std::fs::write(&path, &json).unwrap();
    }
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == json,
        "segments.json is stale: run UPDATE_CORPUS=1 cargo test corpus"
    );
}
