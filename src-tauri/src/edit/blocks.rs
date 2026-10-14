//! The top-level blocks of a Markdown source as byte ranges of the text itself.
//!
//! The editor works on the file's text and rewrites only the blocks a person changed, so it needs to
//! know where every block starts and ends. A block is a run of whole lines (from the first line of the
//! comrak node to its last, without the line terminator), so the ranges do not depend on how comrak
//! counts columns, tabs or multi-byte characters. Everything between two blocks (blank lines, link
//! reference definitions that comrak consumes, a leading BOM, the final newline) is a *separator* that
//! the editor keeps as it is: the blocks and the gaps between them always add up to the whole input.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena};
use serde::Serialize;

use crate::render::markdown::{
    is_deeper_than, options, too_many_container_markers, MAX_TREE_DEPTH,
};
use crate::render::{normalize_newlines, MAX_RENDER_BYTES};
use crate::util::on_parser_stack;

pub(crate) const BOM: char = '\u{FEFF}';

/// What a block is, which decides what the Visual editor may do with it. Mirrors the UI's `BlockKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockKind {
    Paragraph,
    Heading,
    List,
    Table,
    Code,
    Html,
    Frontmatter,
    Math,
    FootnoteDef,
    ThematicBreak,
    Blockquote,
    Other,
}

/// One top-level block: `start_byte..end_byte` of the source it was parsed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub start_byte: usize,
    pub end_byte: usize,
    pub kind: BlockKind,
}

/// The line ending a file uses; `Mixed` when it uses several (or a lone `\r`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Eol {
    Lf,
    Crlf,
    Mixed,
}

/// The blocks of a source and its line ending. Matches the UI's `ParsedBlocks`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedBlocks {
    pub eol: Eol,
    pub blocks: Vec<BlockInfo>,
}

/// One line of the source: `start..end` without its terminator.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Line {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// Splits on `\n`, `\r\n` and a lone `\r` (the three terminators comrak knows), starting after any BOM.
pub(crate) fn split_lines(text: &str, from: usize) -> (Vec<Line>, Eol) {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let (mut lf, mut crlf, mut cr) = (0_usize, 0_usize, 0_usize);
    let (mut start, mut i) = (from, from);
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                lf += 1;
                lines.push(Line { start, end: i });
                i += 1;
                start = i;
            }
            b'\r' => {
                let end = i;
                if bytes.get(i + 1) == Some(&b'\n') {
                    crlf += 1;
                    i += 2;
                } else {
                    cr += 1;
                    i += 1;
                }
                lines.push(Line { start, end });
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        lines.push(Line {
            start,
            end: bytes.len(),
        });
    }
    let eol = match (lf, crlf, cr) {
        (_, 0, 0) => Eol::Lf,
        (0, _, 0) => Eol::Crlf,
        _ => Eol::Mixed,
    };
    (lines, eol)
}

fn is_blank(text: &str, line: Line) -> bool {
    text[line.start..line.end].trim().is_empty()
}

/// A paragraph that holds nothing but display math (`$$ ... $$`).
fn is_display_math_only<'a>(node: &'a AstNode<'a>) -> bool {
    let mut math = false;
    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::Math(m) if m.display_math => math = true,
            NodeValue::SoftBreak | NodeValue::LineBreak => {}
            NodeValue::Text(t) if t.trim().is_empty() => {}
            _ => return false,
        }
    }
    math
}

fn kind_of<'a>(node: &'a AstNode<'a>) -> BlockKind {
    match &node.data.borrow().value {
        NodeValue::Paragraph if is_display_math_only(node) => BlockKind::Math,
        NodeValue::Paragraph => BlockKind::Paragraph,
        NodeValue::Heading(_) => BlockKind::Heading,
        NodeValue::List(_) => BlockKind::List,
        NodeValue::Table(_) => BlockKind::Table,
        NodeValue::CodeBlock(c) if c.info.split_whitespace().next() == Some("math") => {
            BlockKind::Math
        }
        NodeValue::CodeBlock(_) => BlockKind::Code,
        NodeValue::HtmlBlock(_) => BlockKind::Html,
        NodeValue::FrontMatter(_) => BlockKind::Frontmatter,
        NodeValue::FootnoteDefinition(_) => BlockKind::FootnoteDef,
        NodeValue::ThematicBreak => BlockKind::ThematicBreak,
        NodeValue::BlockQuote => BlockKind::Blockquote,
        _ => BlockKind::Other,
    }
}

/// The whole text between its first and last non-blank line as one block of kind `Other`: what a
/// source gets when it is too deeply nested to parse safely. Empty when the text is blank.
fn one_block(text: &str, lines: &[Line]) -> Vec<BlockInfo> {
    let first = lines.iter().find(|l| !is_blank(text, **l));
    let last = lines.iter().rev().find(|l| !is_blank(text, **l));
    match (first, last) {
        (Some(first), Some(last)) => vec![BlockInfo {
            start_byte: first.start,
            end_byte: last.end,
            kind: BlockKind::Other,
        }],
        _ => Vec::new(),
    }
}

/// Parses `text` (with any line ending and an optional BOM) into its top-level blocks. A text over the size
/// the app edits (`MAX_RENDER_BYTES`, what a document can be) has no blocks: the editor then leaves it all as it is.
pub fn parse_blocks(text: &str) -> ParsedBlocks {
    if text.len() as u64 > MAX_RENDER_BYTES {
        return ParsedBlocks {
            eol: Eol::Lf,
            blocks: Vec::new(),
        };
    }
    on_parser_stack(|| parse(text))
}

fn parse(text: &str) -> ParsedBlocks {
    let body_start = if text.starts_with(BOM) {
        BOM.len_utf8()
    } else {
        0
    };
    let (lines, eol) = split_lines(text, body_start);
    // comrak sees the text the reader sees: one `\n` for every terminator, so line numbers agree with ours.
    let normalized = normalize_newlines(&text[body_start..]);
    if too_many_container_markers(&normalized) {
        return ParsedBlocks {
            eol,
            blocks: one_block(text, &lines),
        };
    }
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, &normalized, &opts);
    if is_deeper_than(root, MAX_TREE_DEPTH) {
        return ParsedBlocks {
            eol,
            blocks: one_block(text, &lines),
        };
    }

    // comrak moves footnote definitions to the end of the document; the editor wants them where they are.
    let mut spans: Vec<(usize, usize, BlockKind)> = Vec::new();
    for node in root.children() {
        let span = node.data.borrow().sourcepos;
        let (first, mut last) = (span.start.line, span.end.line.min(lines.len()));
        if first == 0 || first > last {
            continue;
        }
        // Blank lines at the end (an unclosed fence at the end of the file) belong to the separator.
        while last > first && is_blank(text, lines[last - 1]) {
            last -= 1;
        }
        spans.push((first, last, kind_of(node)));
    }
    spans.sort_by_key(|(first, _, _)| *first);

    let mut blocks: Vec<BlockInfo> = Vec::new();
    let mut last_line = 0_usize;
    for (first, last, kind) in spans {
        if first <= last_line {
            // Two nodes on the same lines: keep them together rather than split a line in two.
            if let Some(previous) = blocks.last_mut() {
                previous.end_byte = previous.end_byte.max(lines[last - 1].end);
                previous.kind = BlockKind::Other;
            }
            last_line = last_line.max(last);
            continue;
        }
        blocks.push(BlockInfo {
            start_byte: lines[first - 1].start,
            end_byte: lines[last - 1].end,
            kind,
        });
        last_line = last;
    }
    ParsedBlocks { eol, blocks }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blocks as `(kind, text)`.
    fn parts(text: &str) -> Vec<(BlockKind, &str)> {
        parse_blocks(text)
            .blocks
            .iter()
            .map(|b| (b.kind, &text[b.start_byte..b.end_byte]))
            .collect()
    }

    /// The text between the blocks (and before the first, after the last), in order.
    fn separators<'a>(text: &'a str, blocks: &[BlockInfo]) -> Vec<&'a str> {
        let mut out = Vec::new();
        let mut at = 0;
        for b in blocks {
            out.push(&text[at..b.start_byte]);
            at = b.end_byte;
        }
        out.push(&text[at..]);
        out
    }

    /// The invariant the editor stands on: blocks and separators give back the input, byte for byte.
    fn assert_partition(text: &str) {
        let parsed = parse_blocks(text);
        let mut at = 0;
        let mut rebuilt = String::new();
        for b in &parsed.blocks {
            assert!(b.start_byte >= at, "blocks overlap or are out of order");
            assert!(b.start_byte < b.end_byte, "empty block");
            assert!(b.end_byte <= text.len());
            assert!(text.is_char_boundary(b.start_byte) && text.is_char_boundary(b.end_byte));
            rebuilt.push_str(&text[at..b.start_byte]);
            rebuilt.push_str(&text[b.start_byte..b.end_byte]);
            at = b.end_byte;
        }
        rebuilt.push_str(&text[at..]);
        assert_eq!(rebuilt, text);
    }

    #[test]
    fn plain_blocks_have_exact_ranges() {
        let text = "# Title\n\nOne paragraph\nover two lines.\n\n- a\n- b\n";
        assert_eq!(
            parts(text),
            [
                (BlockKind::Heading, "# Title"),
                (BlockKind::Paragraph, "One paragraph\nover two lines."),
                (BlockKind::List, "- a\n- b"),
            ]
        );
        let blocks = parse_blocks(text).blocks;
        assert_eq!(separators(text, &blocks), ["", "\n\n", "\n\n", "\n"]);
        assert_partition(text);
    }

    #[test]
    fn crlf_is_kept_inside_blocks_and_out_of_the_ends() {
        let text = "# T\r\n\r\nline one\r\nline two\r\n\r\n- x\r\n";
        let parsed = parse_blocks(text);
        assert_eq!(parsed.eol, Eol::Crlf);
        assert_eq!(
            parts(text),
            [
                (BlockKind::Heading, "# T"),
                (BlockKind::Paragraph, "line one\r\nline two"),
                (BlockKind::List, "- x"),
            ]
        );
        assert_partition(text);
    }

    #[test]
    fn line_endings_are_reported() {
        assert_eq!(parse_blocks("a\nb\n").eol, Eol::Lf);
        assert_eq!(parse_blocks("no break").eol, Eol::Lf);
        assert_eq!(parse_blocks("a\r\nb\r\n").eol, Eol::Crlf);
        assert_eq!(parse_blocks("a\r\nb\n").eol, Eol::Mixed);
        assert_eq!(parse_blocks("a\rb\r").eol, Eol::Mixed);
    }

    #[test]
    fn a_lone_cr_is_a_line_ending_like_comrak_reads_it() {
        let text = "# T\r\rbody\r";
        assert_eq!(
            parts(text),
            [(BlockKind::Heading, "# T"), (BlockKind::Paragraph, "body")]
        );
        assert_partition(text);
    }

    #[test]
    fn a_bom_stays_in_the_first_separator() {
        let text = "\u{FEFF}# T\n\nbody\n";
        let blocks = parse_blocks(text).blocks;
        assert_eq!(blocks[0].start_byte, 3);
        assert_eq!(separators(text, &blocks)[0], "\u{FEFF}");
        assert_partition(text);
        let crlf = "\u{FEFF}---\r\ntitle: x\r\n---\r\n\r\ntext\r\n";
        assert_eq!(
            parts(crlf)[0],
            (BlockKind::Frontmatter, "---\r\ntitle: x\r\n---")
        );
        assert_partition(crlf);
    }

    #[test]
    fn the_final_newline_and_a_missing_one_are_both_kept() {
        for text in ["# T\n\nx\n", "# T\n\nx", "# T\n\nx\n\n\n"] {
            assert_partition(text);
            assert_eq!(parts(text)[1], (BlockKind::Paragraph, "x"));
        }
    }

    #[test]
    fn blank_and_empty_sources_have_no_blocks() {
        for text in ["", "\n", "  \n\t\n", "\u{FEFF}", "\r\n\r\n"] {
            assert_eq!(
                parse_blocks(text).blocks,
                Vec::<BlockInfo>::new(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn multi_byte_text_is_sliced_on_character_boundaries() {
        let text = "# Olá, 世界 😀\n\nÀ vida é «bela» — ok 😀\n\n> ñandú\n";
        assert_eq!(
            parts(text),
            [
                (BlockKind::Heading, "# Olá, 世界 😀"),
                (BlockKind::Paragraph, "À vida é «bela» — ok 😀"),
                (BlockKind::Blockquote, "> ñandú"),
            ]
        );
        assert_partition(text);
    }

    #[test]
    fn every_kind_is_recognised() {
        let text = "---\ntitle: x\n---\n\n# H\n\npara[^1]\n\n- a\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nlet a = 1;\n```\n\n<div>\nhtml\n</div>\n\n$$\nx^2\n$$\n\n[^1]: note\n\n---\n\n> quote\n\n> [!NOTE]\n> alert\n";
        let kinds: Vec<BlockKind> = parse_blocks(text).blocks.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            [
                BlockKind::Frontmatter,
                BlockKind::Heading,
                BlockKind::Paragraph,
                BlockKind::List,
                BlockKind::Table,
                BlockKind::Code,
                BlockKind::Html,
                BlockKind::Math,
                BlockKind::FootnoteDef,
                BlockKind::ThematicBreak,
                BlockKind::Blockquote,
                BlockKind::Other,
            ]
        );
        assert_partition(text);
    }

    #[test]
    fn footnote_definitions_keep_their_place_though_comrak_moves_them_to_the_end() {
        let text = "text[^1] and more[^2]\n\n[^1]: first note\n    continued\n\nmiddle\n\n[^2]: second\n\nlast\n";
        assert_eq!(
            parts(text),
            [
                (BlockKind::Paragraph, "text[^1] and more[^2]"),
                (BlockKind::FootnoteDef, "[^1]: first note\n    continued"),
                (BlockKind::Paragraph, "middle"),
                (BlockKind::FootnoteDef, "[^2]: second"),
                (BlockKind::Paragraph, "last"),
            ]
        );
        assert_partition(text);
    }

    #[test]
    fn math_fences_and_inline_math_are_told_apart() {
        let text = "```math\nx\n```\n\nSome $x$ inline.\n\n$$a+b$$\n";
        let kinds: Vec<BlockKind> = parse_blocks(text).blocks.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            [BlockKind::Math, BlockKind::Paragraph, BlockKind::Math]
        );
    }

    #[test]
    fn code_fences_hide_markdown_and_blank_lines() {
        let text = "intro\n\n```md\n# not a heading\n\n\n- not a list\n```\n\nafter\n";
        assert_eq!(
            parts(text),
            [
                (BlockKind::Paragraph, "intro"),
                (
                    BlockKind::Code,
                    "```md\n# not a heading\n\n\n- not a list\n```"
                ),
                (BlockKind::Paragraph, "after"),
            ]
        );
    }

    #[test]
    fn an_unclosed_fence_does_not_swallow_the_trailing_blank_lines() {
        let text = "```\ncode\n\n\n";
        assert_eq!(parts(text), [(BlockKind::Code, "```\ncode")]);
        assert_partition(text);
    }

    #[test]
    fn nested_lists_and_lazy_continuation_stay_in_one_block() {
        let text = "1. one\n   - nested\n     more\n   - nested two\n\n2. two\nlazy continuation\n\ntail\n";
        assert_eq!(
            parts(text),
            [
                (
                    BlockKind::List,
                    "1. one\n   - nested\n     more\n   - nested two\n\n2. two\nlazy continuation"
                ),
                (BlockKind::Paragraph, "tail"),
            ]
        );
    }

    #[test]
    fn setext_headings_and_tables_with_pipes_in_code() {
        let text = "Title\n=====\n\n| a | b |\n|:--|--:|\n| `x | y` | 2 |\n\nSub\n---\n";
        assert_eq!(
            parts(text),
            [
                (BlockKind::Heading, "Title\n====="),
                (BlockKind::Table, "| a | b |\n|:--|--:|\n| `x | y` | 2 |"),
                (BlockKind::Heading, "Sub\n---"),
            ]
        );
    }

    #[test]
    fn link_reference_definitions_stay_in_the_separators() {
        let text = "See [the docs][d].\n\n[d]: https://example.com/docs\n\nEnd.\n";
        let parsed = parse_blocks(text);
        assert_eq!(
            parts(text),
            [
                (BlockKind::Paragraph, "See [the docs][d]."),
                (BlockKind::Paragraph, "End."),
            ]
        );
        assert_eq!(
            separators(text, &parsed.blocks)[1],
            "\n\n[d]: https://example.com/docs\n\n"
        );
        assert_partition(text);
    }

    #[test]
    fn indentation_tabs_and_trailing_spaces_belong_to_their_block() {
        let text = "  indented para  \n\n\ttabbed code\n\ntrailing spaces  \nnext line\n";
        let p = parts(text);
        assert_eq!(p[0], (BlockKind::Paragraph, "  indented para  "));
        assert_eq!(p[1], (BlockKind::Code, "\ttabbed code"));
        assert_eq!(p[2], (BlockKind::Paragraph, "trailing spaces  \nnext line"));
        assert_partition(text);
    }

    #[test]
    fn too_deeply_nested_sources_become_one_block() {
        let text = format!("intro\n\n{} deep\n\nend\n", ">".repeat(500));
        let parsed = parse_blocks(&text);
        assert_eq!(parsed.blocks.len(), 1);
        assert_eq!(parsed.blocks[0].kind, BlockKind::Other);
        assert!(text[parsed.blocks[0].start_byte..parsed.blocks[0].end_byte].starts_with("intro"));
        assert_partition(&text);
    }

    #[test]
    fn a_text_over_the_size_limit_is_not_parsed() {
        let big = "word\n\n".repeat(usize::try_from(MAX_RENDER_BYTES).expect("fits") / 6 + 1);
        assert!(big.len() as u64 > MAX_RENDER_BYTES);
        let parsed = parse_blocks(&big);
        assert_eq!(parsed.blocks, Vec::<BlockInfo>::new());
        assert_eq!(parsed.eol, Eol::Lf);
        // just under the limit it still is
        let fits = "word\n\n".repeat(10);
        assert_eq!(parse_blocks(&fits).blocks.len(), 10);
    }

    #[test]
    fn a_large_flat_document_is_cut_into_a_block_per_paragraph() {
        let text = "para\n\n".repeat(2000);
        let parsed = parse_blocks(&text);
        assert_eq!(parsed.blocks.len(), 2000);
        assert_partition(&text);
    }

    /// Whether a separator holds only blank lines and link reference definitions (what comrak consumes).
    fn only_blank_or_definitions(separator: &str) -> bool {
        separator.lines().all(|line| {
            let line = line.trim();
            line.is_empty()
                || (line.starts_with('[') && line.contains("]:") && !line.starts_with("[^"))
        })
    }

    #[test]
    fn the_repository_documents_split_into_blocks_that_add_up_to_the_file() {
        let corpus = super::super::corpus::frozen_docs();
        assert!(corpus.len() >= 5, "found only {} documents", corpus.len());
        for (name, text) in &corpus {
            assert_partition(text);
            // Nothing a reader would see may hide in a separator.
            let blocks = parse_blocks(text).blocks;
            for (i, sep) in separators(text, &blocks).iter().enumerate() {
                let sep = if i == 0 {
                    sep.trim_start_matches(BOM)
                } else {
                    sep
                };
                assert!(
                    only_blank_or_definitions(sep),
                    "{name}: content outside any block: {sep:?}"
                );
            }
            // And the same with Windows line endings and a BOM, which must not change the blocks.
            let windows = format!("\u{FEFF}{}", text.replace('\n', "\r\n"));
            assert_partition(&windows);
            let plain: Vec<BlockKind> = blocks.iter().map(|b| b.kind).collect();
            let shifted: Vec<BlockKind> = parse_blocks(&windows)
                .blocks
                .iter()
                .map(|b| b.kind)
                .collect();
            assert_eq!(plain, shifted, "{name}");
        }
    }
}
