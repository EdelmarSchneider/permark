//! Generated inputs of the measurements: Markdown texts and folder trees. Deterministic (a fixed seed per input), so
//! every run and every machine measures the same bytes.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub(crate) const MB: usize = 1024 * 1024;

/// Words the texts are made of; `harbor` is in every prose paragraph (a term found in every section).
const VOCABULARY: &[&str] = &[
    "the", "plan", "review", "notes", "meeting", "draft", "budget", "team", "release", "harbor",
    "river", "garden", "window", "letter", "summary", "question", "answer", "project", "folder",
    "document", "change", "result", "update", "design", "reader", "writer", "number", "table",
    "picture", "version", "history", "morning", "evening", "travel", "market", "kitchen",
    "station", "library", "student", "teacher", "doctor", "measure", "signal", "pattern", "simple",
    "careful", "quiet", "bright", "early", "later", "often", "never", "always", "because",
    "between", "around", "through", "before", "after", "almost",
];

/// Xorshift generator: cheap and the same sequence everywhere.
pub(crate) struct Gen(u64);

impl Gen {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn step(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A number in `0..n`.
    pub(crate) fn below(&mut self, n: usize) -> usize {
        let n = u64::try_from(n.max(1)).unwrap_or(1);
        usize::try_from(self.step() % n).unwrap_or(0)
    }

    pub(crate) fn word(&mut self) -> &'static str {
        VOCABULARY[self.below(VOCABULARY.len())]
    }

    /// `count` words separated by spaces.
    fn words(&mut self, count: usize, out: &mut String) {
        for at in 0..count {
            if at > 0 {
                out.push(' ');
            }
            out.push_str(self.word());
        }
    }
}

/// A paragraph of `words` words with some inline syntax (emphasis, code, links), ending in a blank line.
fn paragraph(g: &mut Gen, words: usize, n: usize, out: &mut String) {
    for at in 0..words {
        if at > 0 {
            out.push(if at % 14 == 0 { '\n' } else { ' ' });
        }
        match g.below(40) {
            0 => {
                out.push_str("**");
                out.push_str(g.word());
                out.push_str("**");
            }
            1 => {
                out.push('`');
                out.push_str(g.word());
                out.push('`');
            }
            2 => {
                let target = g.below(1000);
                let _ = write!(out, "[{}](notes/n{target}.md)", g.word());
            }
            3 => {
                let _ = write!(out, "harbor{n}");
            }
            _ => out.push_str(g.word()),
        }
    }
    out.push_str("\n\n");
}

/// Markdown as notes are written: a heading every few blocks, paragraphs, lists, tables, code; about `bytes` long.
pub(crate) fn typical_markdown(bytes: usize, seed: u64) -> String {
    let mut g = Gen::new(seed);
    let mut out = String::with_capacity(bytes + 4096);
    out.push_str("# Generated notes\n\n");
    let mut n = 0;
    while out.len() < bytes {
        n += 1;
        let _ = write!(out, "## {} {} {n}\n\n", g.word(), g.word());
        for _ in 0..2 + g.below(3) {
            let words = 40 + g.below(40);
            paragraph(&mut g, words, n, &mut out);
        }
        match g.below(4) {
            0 => {
                for _ in 0..3 + g.below(4) {
                    out.push_str("- ");
                    let words = 4 + g.below(8);
                    g.words(words, &mut out);
                    out.push('\n');
                }
                out.push('\n');
            }
            1 => {
                out.push_str("| name | value | note |\n|---|---|---|\n");
                for row in 0..5 {
                    let _ = writeln!(out, "| {} | {row} | {} {} |", g.word(), g.word(), g.word());
                }
                out.push('\n');
            }
            2 => {
                out.push_str("```rust\n");
                for line in 0..4 + g.below(6) {
                    let _ = writeln!(out, "let {}_{line} = {};", g.word(), g.below(1000));
                }
                out.push_str("```\n\n");
            }
            _ => {
                out.push_str("> ");
                let words = 12 + g.below(12);
                g.words(words, &mut out);
                out.push_str("\n\n");
            }
        }
    }
    out
}

/// Markdown made almost only of headings (one short line under each): about `bytes` long (5 MB: about 150k headings).
pub(crate) fn heading_dense_markdown(bytes: usize) -> String {
    let mut out = String::with_capacity(bytes + 64);
    let mut n = 0;
    while out.len() < bytes {
        n += 1;
        let _ = write!(out, "### Heading number {n}\n\nNote {n}.\n\n");
    }
    out
}

/// [`typical_markdown`] with a footnote reference in every tenth block and the definitions at the end.
pub(crate) fn footnoted_markdown(bytes: usize) -> String {
    let text = typical_markdown(bytes, 4);
    let mut out = String::with_capacity(text.len() + text.len() / 50);
    let mut notes = 0;
    for (at, block) in text.split_inclusive("\n\n").enumerate() {
        let prose = !block.starts_with(['#', '`', '|', '-', '>']);
        if at % 10 == 5 && prose {
            notes += 1;
            out.push_str(block.trim_end());
            let _ = write!(out, "[^n{notes}]\n\n");
        } else {
            out.push_str(block);
        }
    }
    for n in 1..=notes {
        let _ = writeln!(out, "[^n{n}]: Footnote number {n}.");
    }
    out
}

/// Prose without a heading (one search section for the whole file): about `bytes` long, `harbor` in every paragraph.
pub(crate) fn prose(bytes: usize, seed: u64) -> String {
    let mut g = Gen::new(seed);
    let mut out = String::with_capacity(bytes + 1024);
    while out.len() < bytes {
        out.push_str("harbor ");
        let words = 60 + g.below(40);
        paragraph(&mut g, words, 0, &mut out);
    }
    out
}

/// Writes `bytes` at `base/rel`, making the folders on the way.
pub(crate) fn write(base: &Path, rel: &str, bytes: &[u8]) {
    let path = base.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap(); // arch-allow: ARCH-002 generated measurement input in a temporary folder
}

/// What [`large_folder`] wrote.
pub(crate) struct LargeFolder {
    pub documents: usize,
    pub media: usize,
    pub ignored: usize,
}

/// Files per folder of [`large_folder`] (no folder is large enough for the tree's large-folder guard).
const PER_FOLDER: usize = 100;

/// A large project: `files` listed files (70% small Markdown documents linking to a neighbor, 30% pictures) in folders
/// of 100, plus `ignored` small files under `node_modules` (always ignored).
pub(crate) fn large_folder(base: &Path, files: usize, ignored: usize) -> LargeFolder {
    let mut g = Gen::new(3);
    let mut documents = 0;
    let mut media = 0;
    let png = [&b"\x89PNG\r\n\x1a\n"[..], &[0u8; 504]].concat();
    for at in 0..files {
        let folder = at / PER_FOLDER;
        let dir = format!("area{}/topic{}", folder / 10, folder % 10);
        if at % 10 < 7 {
            documents += 1;
            let mut text = format!("# Note {at}\n\n");
            let words = 30 + g.below(40);
            paragraph(&mut g, words, at, &mut text);
            let _ = writeln!(text, "See [the next note](note{}.md).", at + 1);
            write(base, &format!("{dir}/note{at}.md"), text.as_bytes());
        } else {
            media += 1;
            write(base, &format!("{dir}/image{at}.png"), &png);
        }
    }
    for at in 0..ignored {
        let rel = format!("node_modules/pkg{}/lib/file{at}.js", at / PER_FOLDER);
        write(base, &rel, format!("module.exports = {at};\n").as_bytes());
    }
    LargeFolder {
        documents,
        media,
        ignored,
    }
}
