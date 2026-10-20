//! Block map of a Markdown text for editing: top-level nodes of comrak's tree as line ranges, with the kind and the
//! reason a visual editor must leave a block as written. Same parse as reading (front matter cut, prompt pre-pass),
//! so a block of the map is a block of the page. Lines are 1-based over the text split at `\n` (the edit text has
//! no other line break).

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena};
use serde::Serialize;
use ts_rs::TS;

use super::{frontmatter, is_deeper_than, options, prompt, too_many_container_markers};
use crate::format::MarkdownFeatures;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SegmentKind {
    Paragraph,
    Heading,
    List,
    Table,
    Code,
    Html,
    Math,
    Footnote,
    Rule,
    Quote,
    Alert,
    DescriptionList,
    FrontMatter,
    /// Lines comrak keeps in no node (link reference definitions).
    Raw,
    Other,
}

/// Why a block stays as written in the visual editor (backend part; the editor adds its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LockReason {
    FrontMatter,
    Html,
    Math,
    Footnote,
    Diagram,
    /// A data block drawn as a sheet (`toon`).
    Data,
    Definition,
    Kind,
    Parse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Segment {
    pub line_start: u32,
    /// Inclusive.
    pub line_end: u32,
    pub kind: SegmentKind,
    pub lock: Option<LockReason>,
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A paragraph holding nothing but display math (`$$ ... $$`).
fn display_math_only<'a>(node: &'a AstNode<'a>) -> bool {
    let mut math = false;
    for child in node.children() {
        match &child.data().value {
            NodeValue::Math(m) if m.display_math => math = true,
            NodeValue::SoftBreak | NodeValue::LineBreak => {}
            NodeValue::Text(t) if t.trim().is_empty() => {}
            _ => return false,
        }
    }
    math
}

fn classify<'a>(node: &'a AstNode<'a>) -> (SegmentKind, Option<LockReason>) {
    match &node.data().value {
        NodeValue::Paragraph if display_math_only(node) => {
            (SegmentKind::Math, Some(LockReason::Math))
        }
        NodeValue::Paragraph => (SegmentKind::Paragraph, None),
        NodeValue::Heading(_) => (SegmentKind::Heading, None),
        NodeValue::List(_) => (SegmentKind::List, None),
        NodeValue::Table(_) => (SegmentKind::Table, None),
        NodeValue::CodeBlock(code) => {
            let first = code.info.split_whitespace().next().unwrap_or("");
            if first.eq_ignore_ascii_case("math") {
                (SegmentKind::Math, Some(LockReason::Math))
            } else if ["mermaid", "plantuml", "puml"]
                .iter()
                .any(|d| first.eq_ignore_ascii_case(d))
            {
                (SegmentKind::Code, Some(LockReason::Diagram))
            } else if first.eq_ignore_ascii_case("toon") {
                (SegmentKind::Code, Some(LockReason::Data))
            } else {
                (SegmentKind::Code, None)
            }
        }
        NodeValue::HtmlBlock(_) => (SegmentKind::Html, Some(LockReason::Html)),
        NodeValue::FootnoteDefinition(_) => (SegmentKind::Footnote, Some(LockReason::Footnote)),
        NodeValue::ThematicBreak => (SegmentKind::Rule, None),
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => (SegmentKind::Quote, None),
        NodeValue::Alert(_) => (SegmentKind::Alert, Some(LockReason::Kind)),
        NodeValue::DescriptionList => (SegmentKind::DescriptionList, Some(LockReason::Kind)),
        NodeValue::FrontMatter(_) => (SegmentKind::FrontMatter, Some(LockReason::FrontMatter)),
        _ => (SegmentKind::Other, Some(LockReason::Kind)),
    }
}

/// The block map of `text` (lines split at `\n`).
#[must_use]
pub fn segments(text: &str, features: &MarkdownFeatures) -> Vec<Segment> {
    let lines: Vec<&str> = text.split('\n').collect();
    let blank = |line: usize| lines.get(line - 1).is_none_or(|l| l.trim().is_empty());
    let count = lines.len();
    if too_many_container_markers(text) {
        return whole(&lines);
    }
    let cut = frontmatter::find(text);
    let mut spans: Vec<Segment> = Vec::new();
    let prepared = match &cut {
        Some(cut) => {
            let end = cut.block.split_inclusive('\n').count();
            spans.push(Segment {
                line_start: 1,
                line_end: to_u32(end),
                kind: SegmentKind::FrontMatter,
                lock: Some(LockReason::FrontMatter),
            });
            frontmatter::replace(text, cut, None)
        }
        None => std::borrow::Cow::Borrowed(text),
    };
    let prepared = match features.prompt_blocks {
        Some(blocks) => prompt::mark(&prepared, blocks.tags).into_owned(),
        None => prepared.into_owned(),
    };
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, &prepared, &opts);
    if is_deeper_than(root, super::MAX_TREE_DEPTH) {
        return whole(&lines);
    }
    for node in root.children() {
        let pos = node.data().sourcepos;
        let first = pos.start.line;
        let mut last = pos.end.line.min(count);
        if first == 0 || first > last {
            continue;
        }
        // blank lines at the end (an unclosed fence at the end of the text) are separators
        while last > first && blank(last) {
            last -= 1;
        }
        let (kind, lock) = classify(node);
        spans.push(Segment {
            line_start: to_u32(first),
            line_end: to_u32(last),
            kind,
            lock,
        });
    }
    spans.sort_by_key(|s| s.line_start);
    // two nodes on one line stay together
    let mut merged: Vec<Segment> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(previous) if span.line_start <= previous.line_end => {
                previous.line_end = previous.line_end.max(span.line_end);
                previous.kind = SegmentKind::Other;
                previous.lock = Some(LockReason::Parse);
            }
            _ => merged.push(span),
        }
    }
    with_raw(merged, &lines)
}

/// Non-blank lines outside every node (definitions) become locked `raw` segments, one per run.
fn with_raw(nodes: Vec<Segment>, lines: &[&str]) -> Vec<Segment> {
    let mut out = Vec::with_capacity(nodes.len() + 4);
    let mut next = 1_usize;
    let push_gap = |out: &mut Vec<Segment>, from: usize, to: usize| {
        let mut run: Option<(usize, usize)> = None;
        for line in from..to {
            let filled = lines.get(line - 1).is_some_and(|l| !l.trim().is_empty());
            run = match (run, filled) {
                (None, true) => Some((line, line)),
                (Some((s, _)), true) => Some((s, line)),
                (Some((s, e)), false) => {
                    out.push(raw(s, e));
                    None
                }
                (None, false) => None,
            };
        }
        if let Some((s, e)) = run {
            out.push(raw(s, e));
        }
    };
    for node in nodes {
        let start = node.line_start as usize;
        if start > next {
            push_gap(&mut out, next, start);
        }
        next = node.line_end as usize + 1;
        out.push(node);
    }
    push_gap(&mut out, next, lines.len() + 1);
    out
}

fn raw(start: usize, end: usize) -> Segment {
    Segment {
        line_start: to_u32(start),
        line_end: to_u32(end),
        kind: SegmentKind::Raw,
        lock: Some(LockReason::Definition),
    }
}

/// The text between its first and last non-blank line as one locked block (too deep to parse safely).
fn whole(lines: &[&str]) -> Vec<Segment> {
    let filled = |l: &&str| !l.trim().is_empty();
    match (
        lines.iter().position(filled),
        lines.iter().rposition(filled),
    ) {
        (Some(first), Some(last)) => vec![Segment {
            line_start: to_u32(first + 1),
            line_end: to_u32(last + 1),
            kind: SegmentKind::Other,
            lock: Some(LockReason::Parse),
        }],
        _ => Vec::new(),
    }
}
