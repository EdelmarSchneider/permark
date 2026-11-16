//! LaTeX display math `\[..\]` only where it stands on lines of its own.
//!
//! comrak's `math_latex` reads `\[..\]` as display math anywhere, so the escaped brackets HTML converters write in
//! prose (`see \[1\]`) would become formulas. A formula is kept when, in a paragraph, its opening `\[` is first on
//! its line with nothing after it there (or the formula is all the paragraph holds) and its closing `\]` ends its
//! line. Any other one is read again with its opening written as one character of the same byte length that is no
//! Markdown, so its brackets and inner Markdown read as with escaped brackets, and every position stays in place
//! (links are located by them). The stand-in becomes `[` again in the tree.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document as comrak_parse, Arena, Options};

use crate::links::Lines;

const OPEN: &str = "\\[";
/// Stand-ins for an opening `\[`: two bytes each, punctuation like `[` (emphasis next to it reads the same), no
/// Markdown. The first one the text does not hold is used.
const STAND_INS: [char; 6] = [
    '\u{a7}', '\u{b6}', '\u{55a}', '\u{55b}', '\u{55c}', '\u{55d}',
];
/// Reads of a text at most: a formula read again can uncover another one inside it.
const MAX_READS: usize = 4;

/// [`comrak::parse_document`] with `\[..\]` a formula only on lines of its own (see the module).
pub(crate) fn parse_document<'a>(
    arena: &'a Arena<'a>,
    text: &str,
    opts: &Options<'_>,
) -> &'a AstNode<'a> {
    let mut root = comrak_parse(arena, text, opts);
    if !opts.extension.math_latex || !text.contains(OPEN) {
        return root;
    }
    let lines = Lines::new(text);
    let mut inline = openers_in_text(root, text, &lines);
    if inline.is_empty() {
        return root;
    }
    let Some(stand_in) = STAND_INS.iter().copied().find(|c| !text.contains(*c)) else {
        return root;
    };
    let mut read = text.to_owned();
    let mut buffer = [0; 4];
    let stand_in = stand_in.encode_utf8(&mut buffer);
    for _ in 1..MAX_READS {
        if inline.is_empty() {
            break;
        }
        for at in inline {
            read.replace_range(at..at + OPEN.len(), stand_in);
        }
        root = comrak_parse(arena, &read, opts);
        inline = openers_in_text(root, &read, &lines);
    }
    restore(root, stand_in);
    root
}

/// Byte offsets of the openings of `\[..\]` formulas that do not stand on lines of their own.
fn openers_in_text<'a>(root: &'a AstNode<'a>, text: &str, lines: &Lines) -> Vec<usize> {
    root.descendants()
        .filter_map(|node| {
            let data = node.data();
            let NodeValue::Math(math) = &data.value else {
                return None;
            };
            if !math.dollar_math || !math.display_math {
                return None;
            }
            // `$$..$$` is display math as well: only an opening written `\[` is one of these
            let at = lines.range_of(data.sourcepos, text)?.start;
            let bracket = text.get(at..)?.starts_with(OPEN);
            (bracket && !stands_alone(node, &math.literal)).then_some(at)
        })
        .collect()
}

/// What sits next to a formula on its side within the paragraph (white space skipped).
#[derive(PartialEq)]
enum Side {
    /// The paragraph's start | end.
    Edge,
    Break,
    Other,
}

fn side<'a>(siblings: impl Iterator<Item = &'a AstNode<'a>>) -> Side {
    for node in siblings {
        match &node.data().value {
            NodeValue::Text(text) if text.trim_matches([' ', '\t']).is_empty() => {}
            NodeValue::SoftBreak | NodeValue::LineBreak => return Side::Break,
            _ => return Side::Other,
        }
    }
    Side::Edge
}

/// Whether the formula `node` (TeX `literal`) stands on lines of its own in a paragraph.
fn stands_alone<'a>(node: &'a AstNode<'a>, literal: &str) -> bool {
    let in_paragraph = node
        .parent()
        .is_some_and(|p| matches!(p.data().value, NodeValue::Paragraph));
    if !in_paragraph {
        return false;
    }
    let before = side(node.preceding_siblings().skip(1));
    let after = side(node.following_siblings().skip(1));
    if before == Side::Other || after == Side::Other {
        return false;
    }
    let whole = before == Side::Edge && after == Side::Edge;
    let rest = literal.trim_start_matches([' ', '\t']);
    whole || rest.starts_with(['\n', '\r'])
}

/// The stand-in back to what it reads as: `[` in text and link destinations, the written `\[` in code, HTML and math.
fn restore<'a>(root: &'a AstNode<'a>, stand_in: &str) {
    for node in root.descendants() {
        let mut data = node.data_mut();
        let (field, back): (&mut String, &str) = match &mut data.value {
            NodeValue::Text(text) => {
                if text.contains(stand_in) {
                    *text = text.replace(stand_in, "[").into();
                }
                continue;
            }
            NodeValue::Link(link) | NodeValue::Image(link) => (&mut link.url, "["),
            NodeValue::Code(code) => (&mut code.literal, OPEN),
            NodeValue::Math(math) => (&mut math.literal, OPEN),
            NodeValue::HtmlInline(html) => (html, OPEN),
            _ => continue,
        };
        if field.contains(stand_in) {
            *field = field.replace(stand_in, back);
        }
    }
}
