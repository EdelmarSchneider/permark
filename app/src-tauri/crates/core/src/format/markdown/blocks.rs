//! Sanitized HTML -> blocks (its top-level elements): content ids, line ranges, `data-b` marks.
//!
//! The input is the sanitizer's output: balanced tags, quoted attributes, no comments. Text between top-level
//! elements is wrapped in `p` when it is more than whitespace, so every block is one element.

use std::collections::HashMap;

use crate::fsx::hash;

const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

const SOURCEPOS: &str = " data-sourcepos=\"";

/// One top-level element.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct RawBlock {
    /// Without `data-sourcepos`.
    pub html: String,
    pub tag: String,
    pub line_start: u32,
    pub line_end: u32,
}

/// Index of the `>` that closes the tag starting at `from` (`<` at `from`), skipping quoted attribute values.
fn tag_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (offset, &byte) in bytes.get(from..)?.iter().enumerate() {
        match quote {
            Some(q) if byte == q => quote = None,
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'>' => return Some(from + offset),
            _ => {}
        }
    }
    None
}

fn tag_name(tag: &str) -> &str {
    let inner = tag.trim_start_matches('<').trim_start_matches('/');
    let end = inner
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(inner.len());
    &inner[..end]
}

/// Removes every ` data-sourcepos="L:C-L:C"`; returns the text and the smallest start and largest end line seen.
fn strip_sourcepos(html: &str) -> (String, Option<(u32, u32)>) {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut lines: Option<(u32, u32)> = None;
    while let Some(at) = rest.find(SOURCEPOS) {
        out.push_str(&rest[..at]);
        let value_start = at + SOURCEPOS.len();
        let Some(len) = rest[value_start..].find('"') else {
            // unterminated: keep the rest as it is
            out.push_str(&rest[at..]);
            return (out, lines);
        };
        if let Some((start, end)) = parse_sourcepos(&rest[value_start..value_start + len]) {
            lines = Some(match lines {
                Some((s, e)) => (s.min(start), e.max(end)),
                None => (start, end),
            });
        }
        rest = &rest[value_start + len + 1..];
    }
    out.push_str(rest);
    (out, lines)
}

/// `"3:1-5:20"` -> `(3, 5)`.
fn parse_sourcepos(value: &str) -> Option<(u32, u32)> {
    let (start, end) = value.split_once('-')?;
    let line = |part: &str| part.split(':').next()?.parse::<u32>().ok();
    Some((line(start)?, line(end)?))
}

/// Splits sanitized HTML into its top-level elements.
pub(super) fn split(html: &str) -> Vec<RawBlock> {
    let bytes = html.as_bytes();
    let mut blocks = Vec::new();
    let mut at = 0;
    let mut last_end = 1_u32;
    while at < bytes.len() {
        let next = html[at..].find('<').map_or(bytes.len(), |i| at + i);
        let text = &html[at..next];
        if !text.trim().is_empty() {
            blocks.push(RawBlock {
                html: format!("<p>{}</p>", text.trim()),
                tag: "p".to_owned(),
                line_start: last_end,
                line_end: last_end,
            });
        }
        if next >= bytes.len() {
            break;
        }
        // one element: from its opening tag to the matching close
        let start = next;
        let mut depth = 0_usize;
        let mut cursor = start;
        let mut end = bytes.len();
        let first_tag_name;
        {
            let Some(open_end) = tag_end(bytes, start) else {
                // broken tail: the rest is one block
                break;
            };
            first_tag_name = tag_name(&html[start..=open_end]).to_ascii_lowercase();
        }
        while cursor < bytes.len() {
            let Some(lt) = html[cursor..].find('<').map(|i| cursor + i) else {
                break;
            };
            let Some(gt) = tag_end(bytes, lt) else {
                break;
            };
            let tag = &html[lt..=gt];
            let name = tag_name(tag).to_ascii_lowercase();
            if tag.starts_with("</") {
                depth = depth.saturating_sub(1);
            } else if !(VOID.contains(&name.as_str()) || tag.ends_with("/>")) {
                depth += 1;
            }
            cursor = gt + 1;
            if depth == 0 {
                end = cursor;
                break;
            }
        }
        let (clean, lines) = strip_sourcepos(&html[start..end]);
        let (line_start, line_end) = lines.unwrap_or((last_end, last_end));
        last_end = line_end.max(last_end);
        blocks.push(RawBlock {
            html: clean,
            tag: first_tag_name,
            line_start,
            line_end,
        });
        at = end;
        if end >= bytes.len() {
            break;
        }
    }
    blocks
}

/// Content ids: equal HTML -> equal hash; repeats get `-<n>`.
pub(super) fn ids(blocks: &[RawBlock]) -> Vec<String> {
    let mut seen: HashMap<u64, u32> = HashMap::new();
    blocks
        .iter()
        .map(|block| {
            let digest = hash(block.html.as_bytes());
            let count = seen.entry(digest.0).or_insert(0);
            let id = if *count == 0 {
                digest.hex()
            } else {
                format!("{}-{count}", digest.hex())
            };
            *count += 1;
            id
        })
        .collect()
}

/// `html` with `data-b="<id>"` right after the name of its first tag.
pub(super) fn mark(html: &str, id: &str) -> String {
    let Some(name_end) = html
        .char_indices()
        .skip(1)
        .find(|(_, c)| c.is_whitespace() || *c == '>' || *c == '/')
        .map(|(i, _)| i)
    else {
        return html.to_owned();
    };
    let mut out = String::with_capacity(html.len() + id.len() + 12);
    out.push_str(&html[..name_end]);
    out.push_str(" data-b=\"");
    out.push_str(id);
    out.push('"');
    out.push_str(&html[name_end..]);
    out
}

/// What one pass over rendered (not yet sanitized) HTML says about it.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Scan {
    /// The deepest element nesting; void elements and comments do not count.
    pub deepest: usize,
    /// The HTML without the `data-sourcepos` of nested elements: the position of a top-level element covers its
    /// whole extent, which is all the block lines need. A top-level element without one keeps its nested ones.
    pub html: String,
    /// Byte offsets in `html` right after each top-level element (where the nesting returns to zero).
    pub cuts: Vec<usize>,
}

/// The range of ` data-sourcepos="..."` inside a tag, if it has one.
fn sourcepos_range(tag: &str) -> Option<std::ops::Range<usize>> {
    let start = tag.find(SOURCEPOS)?;
    let value = start + SOURCEPOS.len();
    let end = value + tag[value..].find('"')? + 1;
    Some(start..end)
}

/// One pass over `html`, which may be unbalanced and need not be sanitized.
pub(super) fn scan(html: &str) -> Scan {
    let bytes = html.as_bytes();
    let mut out = Scan {
        html: String::with_capacity(html.len()),
        ..Scan::default()
    };
    let (mut depth, mut at, mut copied) = (0_usize, 0_usize, 0_usize);
    let mut keep_nested = true;
    while let Some(lt) = html[at..].find('<').map(|i| at + i) {
        if html[lt..].starts_with("<!--") {
            at = html[lt..].find("-->").map_or(bytes.len(), |i| lt + i + 3);
            continue;
        }
        let Some(gt) = tag_end(bytes, lt) else {
            break;
        };
        let tag = &html[lt..=gt];
        let name = tag_name(tag).to_ascii_lowercase();
        let named = name.starts_with(|c: char| c.is_ascii_alphabetic());
        let closing = tag.starts_with("</");
        if depth == 0 && named {
            keep_nested = sourcepos_range(tag).is_none();
        } else if depth > 0 && !keep_nested {
            if let Some(range) = sourcepos_range(tag) {
                out.html.push_str(&html[copied..lt + range.start]);
                copied = lt + range.end;
            }
        }
        if closing {
            depth = depth.saturating_sub(1);
        } else if named && !(VOID.contains(&name.as_str()) || tag.ends_with("/>")) {
            depth += 1;
            out.deepest = out.deepest.max(depth);
        }
        at = gt + 1;
        if depth == 0 && (named || closing) {
            out.html.push_str(&html[copied..at]);
            copied = at;
            out.cuts.push(out.html.len());
        }
    }
    out.html.push_str(&html[copied..]);
    out
}

/// The deepest element nesting of `html`.
#[cfg(test)]
pub(super) fn max_depth(html: &str) -> usize {
    scan(html).deepest
}

/// `html` cut at the given offsets into pieces of at least `target` bytes (the last piece may be shorter).
pub(super) fn pieces<'a>(html: &'a str, cuts: &[usize], target: usize) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0;
    for &cut in cuts {
        if cut - from >= target {
            out.push(&html[from..cut]);
            from = cut;
        }
    }
    if from < html.len() {
        out.push(&html[from..]);
    }
    out
}

#[cfg(test)]
#[allow(clippy::assert_is_empty)] // `assert!(x.is_empty())` reads best for "nothing found"
mod tests {
    use super::*;

    #[test]
    fn one_block_per_top_level_element() {
        let html = "<h1 data-sourcepos=\"1:1-1:5\" id=\"a\">A</h1>\n<p data-sourcepos=\"3:1-4:9\">x <em data-sourcepos=\"3:3-3:6\">y</em></p>\n<ul data-sourcepos=\"6:1-8:3\"><li data-sourcepos=\"6:1-7:3\"><p>a</p></li><li>b</li></ul>\n<hr data-sourcepos=\"9:1-9:3\" />\n";
        let blocks = split(html);
        let tags: Vec<_> = blocks.iter().map(|b| b.tag.as_str()).collect();
        assert_eq!(tags, ["h1", "p", "ul", "hr"]);
        assert_eq!(
            blocks
                .iter()
                .map(|b| (b.line_start, b.line_end))
                .collect::<Vec<_>>(),
            [(1, 1), (3, 4), (6, 8), (9, 9)]
        );
        assert!(blocks.iter().all(|b| !b.html.contains("data-sourcepos")));
        assert_eq!(blocks[0].html, "<h1 id=\"a\">A</h1>");
        assert_eq!(blocks[2].html, "<ul><li><p>a</p></li><li>b</li></ul>");
    }

    #[test]
    fn stray_text_is_wrapped_and_whitespace_ignored() {
        let blocks = split("  \n hello <b>bold</b>\n<p>x</p>\n tail ");
        let tags: Vec<_> = blocks
            .iter()
            .map(|b| (b.tag.as_str(), b.html.as_str()))
            .collect();
        assert_eq!(
            tags,
            [
                ("p", "<p>hello</p>"),
                ("b", "<b>bold</b>"),
                ("p", "<p>x</p>"),
                ("p", "<p>tail</p>")
            ]
        );
        assert!(split("").is_empty());
        assert!(split("   \n ").is_empty());
    }

    #[test]
    fn quoted_angle_brackets_do_not_end_a_tag() {
        let blocks = split("<p title=\"a > b <i>\">x</p><p>y</p>");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].html, "<p title=\"a > b <i>\">x</p>");
    }

    #[test]
    fn lines_of_a_wrapper_come_from_its_children() {
        let html = "<details class=\"prompt-block\"><summary>context</summary><p data-sourcepos=\"2:1-2:4\">a</p><p data-sourcepos=\"4:1-4:4\">b</p></details>";
        let blocks = split(html);
        assert_eq!(blocks.len(), 1);
        assert_eq!((blocks[0].line_start, blocks[0].line_end), (2, 4));
    }

    #[test]
    fn unbalanced_input_never_panics() {
        for html in [
            "<p>open",
            "<p><b>x</p>",
            "</p>",
            "<",
            "<p",
            "<p a=\"",
            "<<>>",
            "x<",
            "<br><br>",
        ] {
            let _ = split(html);
        }
    }

    #[test]
    fn ids_are_stable_and_unique_for_repeats() {
        let a = split("<p>same</p><p>other</p><p>same</p><p>same</p>");
        let ids = ids(&a);
        assert_eq!(ids.len(), 4);
        assert_ne!(ids[0], ids[1]);
        assert_eq!(ids[2], format!("{}-1", ids[0]));
        assert_eq!(ids[3], format!("{}-2", ids[0]));
        // the same content gives the same id in another document
        assert_eq!(super::ids(&split("<p>other</p>"))[0], ids[1]);
    }

    #[test]
    fn marks_go_after_the_tag_name() {
        assert_eq!(mark("<p>x</p>", "ab"), "<p data-b=\"ab\">x</p>");
        assert_eq!(mark("<hr />", "ab"), "<hr data-b=\"ab\" />");
        assert_eq!(
            mark("<h1 id=\"a\">x</h1>", "ab"),
            "<h1 data-b=\"ab\" id=\"a\">x</h1>"
        );
    }

    #[test]
    fn scan_finds_top_level_cuts() {
        let raw = "<h1>A</h1>\n<p>x <b>y</b></p>\n<hr>\n<ul><li>a</li></ul>\n";
        let found = scan(raw);
        let html = found.html.as_str();
        assert_eq!(html, raw);
        let pieces: Vec<&str> = found.cuts.windows(2).map(|w| &html[w[0]..w[1]]).collect();
        assert_eq!(found.cuts.len(), 4);
        assert_eq!(&html[..found.cuts[0]], "<h1>A</h1>");
        assert_eq!(pieces[0].trim(), "<p>x <b>y</b></p>");
        assert_eq!(pieces[1].trim(), "<hr>");
        assert_eq!(pieces[2].trim(), "<ul><li>a</li></ul>");
        assert_eq!(found.deepest, 2);
        // an unclosed element never returns to the top: one piece to the end
        assert!(scan("<div><p>x</p>").cuts.is_empty());
        // pieces of at least the target size, nothing lost
        let all = super::pieces(html, &found.cuts, 12);
        assert_eq!(all.concat(), html);
        assert!(all.len() >= 2);
        assert_eq!(super::pieces("", &[], 10), Vec::<&str>::new());
    }

    #[test]
    fn nested_source_positions_go_unless_the_top_element_has_none() {
        let raw = "\n<ul data-sourcepos=\"1:1-3:4\"><li data-sourcepos=\"1:1-2:4\">a</li></ul>\n<p data-sourcepos=\"5:1-5:2\">b <em data-sourcepos=\"5:3-5:4\">c</em></p>";
        let found = scan(raw);
        assert_eq!(
            found.html,
            "\n<ul data-sourcepos=\"1:1-3:4\"><li>a</li></ul>\n<p data-sourcepos=\"5:1-5:2\">b <em>c</em></p>"
        );
        // the cuts are offsets in the reduced text
        assert_eq!(
            &found.html[..found.cuts[0]],
            "\n<ul data-sourcepos=\"1:1-3:4\"><li>a</li></ul>"
        );
        // a wrapper without one keeps what is inside, for the block lines
        let wrapper = "<details><p data-sourcepos=\"2:1-2:4\">a</p></details>";
        assert_eq!(scan(wrapper).html, wrapper);
        let blocks = split(&scan(raw).html);
        assert_eq!(
            blocks
                .iter()
                .map(|b| (b.line_start, b.line_end))
                .collect::<Vec<_>>(),
            [(1, 3), (5, 5)]
        );
    }

    #[test]
    fn depth_counts_open_elements() {
        assert_eq!(max_depth("<p>x</p>"), 1);
        assert_eq!(max_depth("<div><div><p>x</p></div></div>"), 3);
        assert_eq!(max_depth("<br><img src=x><hr/>"), 0);
        assert_eq!(max_depth("<!-- <div><div> --><p>x"), 1);
        assert_eq!(max_depth(&"<div>".repeat(1000)), 1000);
        assert_eq!(max_depth("a < b and <3"), 0);
    }
}
