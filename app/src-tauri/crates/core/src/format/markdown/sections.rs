//! Sections of a Markdown text for search (the Markdown `Structure`): a heading and the body up to the next heading
//! of any level. Same front matter cut and parse as reading, so a section's `anchor` is the id the reader gives the
//! heading and its `line` is the reader's line.

use std::collections::HashMap;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena};

use super::{
    frontmatter, headings_of, is_deeper_than, options, too_many_container_markers, MAX_TREE_DEPTH,
};

/// One section of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Ancestor headings (by level), own heading last; empty before the first heading.
    pub heading_path: Vec<String>,
    /// Id the reader gives the heading; `None` for the preamble.
    pub anchor: Option<String>,
    /// 1-based: the heading's line, or the first block's line of a preamble.
    pub line: u32,
    /// Plain text of the body (and of the front matter values, in the preamble).
    pub text: String,
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `<tag attr="x">text</tag>` -> `text` (tags removed, nothing else decoded).
fn strip_tags(html: &str, out: &mut String) {
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        let opens_tag = c == '<'
            && chars
                .peek()
                .is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!'));
        if opens_tag {
            for inner in chars.by_ref() {
                if inner == '>' {
                    break;
                }
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
}

/// Appends the plain text of `node` and everything below it (iterative: no recursion over hostile nesting).
fn append_plain<'a>(node: &'a AstNode<'a>, out: &mut String) {
    for n in node.descendants() {
        match &n.data().value {
            NodeValue::Text(t) => out.push_str(t),
            NodeValue::Code(c) => out.push_str(&c.literal),
            NodeValue::CodeBlock(b) => {
                out.push(' ');
                out.push_str(&b.literal);
            }
            NodeValue::Math(m) => out.push_str(&m.literal),
            NodeValue::HtmlBlock(h) => strip_tags(&h.literal, out),
            NodeValue::SoftBreak
            | NodeValue::LineBreak
            | NodeValue::Paragraph
            | NodeValue::Item(_)
            | NodeValue::TaskItem(_)
            | NodeValue::TableRow(_)
            | NodeValue::TableCell
            | NodeValue::DescriptionTerm
            | NodeValue::DescriptionDetails
            | NodeValue::FootnoteDefinition(_)
            | NodeValue::BlockQuote => out.push(' '),
            _ => {}
        }
    }
    out.push(' ');
}

fn whole(text: &str) -> Vec<Section> {
    let body = text.trim();
    if body.is_empty() {
        return Vec::new();
    }
    vec![Section {
        heading_path: Vec::new(),
        anchor: None,
        line: 1,
        text: body.to_owned(),
    }]
}

fn finish(mut section: Section, has_heading: bool, out: &mut Vec<Section>) {
    let trimmed = section.text.trim().to_owned();
    if has_heading || !trimmed.is_empty() {
        section.text = trimmed;
        out.push(section);
    }
}

/// The sections of `text` (any line endings), in document order.
#[must_use]
pub fn sections(text: &str) -> Vec<Section> {
    // absurdly nested quotes or lists are not parsed (the parse time is quadratic): the raw text stays searchable
    if too_many_container_markers(text) {
        return whole(text);
    }
    let cut = frontmatter::find(text);
    let prepared = match &cut {
        Some(cut) => frontmatter::replace(text, cut, None),
        None => std::borrow::Cow::Borrowed(text),
    };
    let arena = Arena::new();
    let root = parse_document(&arena, &prepared, &options());
    if is_deeper_than(root, MAX_TREE_DEPTH) {
        return whole(text);
    }
    let headings = headings_of(root);
    let mut by_line: HashMap<u32, usize> = HashMap::new();
    for (at, heading) in headings.iter().enumerate() {
        by_line.entry(heading.line).or_insert(at);
    }

    let mut out: Vec<Section> = Vec::new();
    // (level, label) of the open headings
    let mut stack: Vec<(u8, String)> = Vec::new();
    let mut current = Section {
        heading_path: Vec::new(),
        anchor: None,
        line: 1,
        text: cut.as_ref().map(|c| c.raw.clone()).unwrap_or_default(),
    };
    let mut has_heading = false;
    for node in root.children() {
        let (level, line) = {
            let data = node.data();
            let level = match &data.value {
                NodeValue::Heading(h) => Some(h.level),
                _ => None,
            };
            (level, to_u32(data.sourcepos.start.line))
        };
        if let Some(level) = level {
            finish(current, has_heading, &mut out);
            while stack.last().is_some_and(|(top, _)| *top >= level) {
                stack.pop();
            }
            let found = by_line.get(&line).and_then(|&at| headings.get(at));
            stack.push((level, found.map(|h| h.label.clone()).unwrap_or_default()));
            current = Section {
                heading_path: stack.iter().map(|(_, label)| label.clone()).collect(),
                anchor: found.map(|h| h.id.clone()),
                line,
                text: String::new(),
            };
            has_heading = true;
        } else {
            if !has_heading && current.text.trim().is_empty() {
                current.line = line;
            }
            append_plain(node, &mut current.text);
        }
    }
    finish(current, has_heading, &mut out);
    out
}

/// A leaf block of a document (heading, paragraph, table cell, code block, HTML block) with its lines and the
/// headings above it: the places a comment's passage can be found in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextUnit {
    /// 1-based, the reader's lines.
    pub start_line: u32,
    pub end_line: u32,
    /// Ancestor headings (by level), own heading last for a heading; empty before the first heading.
    pub heading_path: Vec<String>,
    /// Plain text of the block.
    pub text: String,
}

/// The text units of `text` (any line endings), in document order, and the number of its last line. Same pipeline and
/// guards as reading: absurd nesting is read line by line.
#[must_use]
pub fn units(text: &str) -> (Vec<TextUnit>, u32) {
    let last_line = to_u32(text.lines().count().max(1));
    let by_lines = || {
        text.lines()
            .enumerate()
            .map(|(at, line)| TextUnit {
                start_line: to_u32(at + 1),
                end_line: to_u32(at + 1),
                heading_path: Vec::new(),
                text: line.to_owned(),
            })
            .collect::<Vec<_>>()
    };
    if too_many_container_markers(text) {
        return (by_lines(), last_line);
    }
    let cut = frontmatter::find(text);
    let prepared = match &cut {
        Some(cut) => frontmatter::replace(text, cut, None),
        None => std::borrow::Cow::Borrowed(text),
    };
    let arena = Arena::new();
    let root = parse_document(&arena, &prepared, &options());
    if is_deeper_than(root, MAX_TREE_DEPTH) {
        return (by_lines(), last_line);
    }
    let mut out: Vec<TextUnit> = Vec::new();
    // (level, label) of the open headings
    let mut stack: Vec<(u8, String)> = Vec::new();
    for node in root.descendants() {
        let (level, start, end) = {
            let data = node.data();
            let level = match &data.value {
                NodeValue::Heading(h) => Some(Some(h.level)),
                NodeValue::Paragraph
                | NodeValue::TableCell
                | NodeValue::CodeBlock(_)
                | NodeValue::HtmlBlock(_) => Some(None),
                _ => None,
            };
            let Some(level) = level else { continue };
            (
                level,
                to_u32(data.sourcepos.start.line),
                to_u32(data.sourcepos.end.line),
            )
        };
        let mut plain = String::new();
        append_plain(node, &mut plain);
        let plain = plain.trim().to_owned();
        if let Some(level) = level {
            while stack.last().is_some_and(|(top, _)| *top >= level) {
                stack.pop();
            }
            stack.push((level, plain.clone()));
        }
        if plain.is_empty() {
            continue;
        }
        out.push(TextUnit {
            start_line: start,
            end_line: end.max(start),
            heading_path: stack.iter().map(|(_, label)| label.clone()).collect(),
            text: plain,
        });
    }
    (out, last_line)
}

#[cfg(test)]
mod unit_tests {
    use super::units;

    #[test]
    fn units_carry_lines_and_the_headings_above() {
        let text = "intro\n\n# Plan\n\nfirst **bold** claim\n\n## Rollout\n\n- item one\n- item two\n\n| a | b |\n|---|---|\n| c | d |\n";
        let (units, last) = units(text);
        assert_eq!(last, 14);
        let at = |needle: &str| units.iter().find(|u| u.text.contains(needle)).unwrap();
        assert_eq!(at("intro").heading_path, Vec::<String>::new());
        assert_eq!(at("first").heading_path, ["Plan"]);
        assert_eq!((at("first").start_line, at("first").end_line), (5, 5));
        assert_eq!(at("item two").heading_path, ["Plan", "Rollout"]);
        assert_eq!(at("item two").start_line, 10);
        assert_eq!(at("Rollout").heading_path, ["Plan", "Rollout"]);
        let cell = units.iter().find(|u| u.text == "d").unwrap();
        assert_eq!(cell.start_line, 14, "a table cell has its own line");
    }

    #[test]
    fn front_matter_keeps_the_lines_and_is_not_a_unit() {
        let (units, _) = units("---\ntitle: x\n---\n\nbody here\n");
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].start_line, 5);
        assert_eq!(units[0].text, "body here");
    }

    #[test]
    fn hostile_nesting_is_read_by_lines() {
        let text = ">".repeat(5000);
        let (units, last) = units(&text);
        assert_eq!(last, 1);
        assert_eq!(units.len(), 1);
    }
}
