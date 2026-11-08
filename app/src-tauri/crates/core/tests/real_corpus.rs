//! Round-trip audit over real Markdown folders (opt-in, never in CI): writes one JSON line per file (path, editor
//! text, block map) for the Edit-mode audit in vitest, which cannot call Rust.
//!
//! Read-only toward the scanned folders. Env: `PERMARK_CORPUS_DIRS` (`;`-separated folders), `PERMARK_CORPUS_OUT`
//! (JSONL path, outside the scanned folders).
//! Run: `cargo test -p permark-core --test real_corpus -- --ignored --nocapture`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use permark_core::edit::lines;
use permark_core::format::{
    segments, Contributions, FormatId, FormatRegistry, MarkdownFeature, PromptBlocks, TextViewer,
    DEFAULT_PROMPT_TAGS,
};
use permark_core::fsx;
use serde::Serialize;

/// Files above this size are left out (the audit targets ordinary documents).
const MAX_BYTES: u64 = 2 * 1024 * 1024;
const SKIPPED_DIRS: [&str; 4] = ["node_modules", "target", ".git", "cargo-target"];
const EXTENSIONS: [&str; 3] = ["md", "markdown", "mdx"];

/// The registry of a default app window as far as the block map is concerned (prompt blocks on).
fn formats() -> FormatRegistry {
    FormatRegistry::build(
        &Contributions::default()
            .formats(vec![
                Arc::new(TextViewer::json()),
                Arc::new(TextViewer::plain()),
            ])
            .markdown(vec![MarkdownFeature::PromptBlocks(PromptBlocks {
                tags: DEFAULT_PROMPT_TAGS,
            })]),
    )
}

#[derive(Default, Debug)]
struct Counts {
    files: usize,
    too_large: usize,
    binary: usize,
    lossy: usize,
    unreadable: usize,
    not_markdown: usize,
}

#[derive(Serialize)]
struct Line<'a> {
    path: &'a str,
    text: &'a str,
    segments: &'a [permark_core::format::Segment],
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let name = entry.file_name();
            if !SKIPPED_DIRS.iter().any(|s| name == *s) {
                walk(&path, out);
            }
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
        {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "reads real folders named by PERMARK_CORPUS_DIRS"]
fn real_corpus_block_maps() {
    let dirs = std::env::var("PERMARK_CORPUS_DIRS").expect("PERMARK_CORPUS_DIRS");
    let out_path = PathBuf::from(std::env::var("PERMARK_CORPUS_OUT").expect("PERMARK_CORPUS_OUT"));
    let mut files = Vec::new();
    for dir in dirs.split(';').filter(|d| !d.trim().is_empty()) {
        walk(Path::new(dir.trim()), &mut files);
    }
    let registry = formats();
    let mut counts = Counts::default();
    let mut out = std::io::BufWriter::new(std::fs::File::create(&out_path).unwrap());
    let started = std::time::Instant::now();
    for path in &files {
        let Ok(meta) = std::fs::metadata(path) else {
            counts.unreadable += 1;
            continue;
        };
        if meta.len() > MAX_BYTES {
            counts.too_large += 1;
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            counts.unreadable += 1;
            continue;
        };
        // same refusals and normalization as the editor's source
        let Some(decoded) = fsx::decode(&bytes) else {
            counts.binary += 1;
            continue;
        };
        if decoded.lossy {
            counts.lossy += 1;
            continue;
        }
        let name = path.to_string_lossy().replace('\\', "/");
        let head = &bytes[..bytes.len().min(4096)];
        let markdown = registry
            .pick(&name, head)
            .is_some_and(|f| f.reads_text() && f.id() == FormatId::MARKDOWN);
        if !markdown {
            counts.not_markdown += 1;
            continue;
        }
        let text = lines::normalized(&decoded.text);
        let segments = segments(&text, registry.features());
        serde_json::to_writer(
            &mut out,
            &Line {
                path: &name,
                text: &text,
                segments: &segments,
            },
        )
        .unwrap();
        out.write_all(b"\n").unwrap();
        counts.files += 1;
    }
    out.flush().unwrap();
    println!(
        "real corpus: {} found, {counts:?}, {:?}",
        files.len(),
        started.elapsed()
    );
    assert!(counts.files > 0, "no Markdown file found");
}
