//! Per-block hashes for the UI's content-based diff.
//!
//! **Invariant:** `block_hashes(html)` returns exactly one hash per *top-level element* of `html`, in
//! document order. The UI zips this list with `container.children`, so anything that changes the number
//! of top-level elements in the sanitized HTML must go through this function. Whitespace and stray text
//! between top-level elements are ignored (they are not elements).
//!
//! A block's hash covers its outer HTML **without `data-sourcepos`**: inserting a line above a block
//! shifts every source position but must not mark every later block as changed.

use crate::util::hash_hex;

const VOID_ELEMENTS: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Splits well-formed HTML into the source slices of its top-level elements.
///
/// The input is expected to be the output of the sanitizer (balanced tags, quoted attributes, no
/// comments). If the markup is unbalanced, the unclosed remainder is returned as one last element.
pub(crate) fn top_level_elements(html: &str) -> Vec<&str> {
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(end) = tag_end(bytes, i) else {
            break;
        };
        let tag = &html[i..=end];
        if tag.starts_with("<!--") || tag.starts_with("<!") || tag.starts_with("<?") {
            i = end + 1;
            continue;
        }
        if tag.starts_with("</") {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                out.push(&html[start..=end]);
            }
        } else {
            let self_closing = tag.ends_with("/>") || VOID_ELEMENTS.contains(&tag_name(tag));
            if depth == 0 {
                start = i;
            }
            if self_closing {
                if depth == 0 {
                    out.push(&html[start..=end]);
                }
            } else {
                depth += 1;
            }
        }
        i = end + 1;
    }
    if depth > 0 {
        out.push(&html[start..]);
    }
    out
}

/// Elements whose end tag may be left out in HTML, so a missing close does not mean deeper nesting.
const IMPLIED_END: [&str; 16] = [
    "p", "li", "dt", "dd", "tr", "td", "th", "thead", "tbody", "tfoot", "option", "optgroup",
    "colgroup", "caption", "rt", "rp",
];

/// The deepest element nesting of `html`, which may be unbalanced and need not be sanitized.
///
/// Void elements and elements with optional end tags do not count. Used to refuse documents whose raw
/// HTML would make the sanitizer (quadratic in depth) or the webview struggle.
pub(crate) fn max_depth(html: &str) -> usize {
    let bytes = html.as_bytes();
    let (mut depth, mut deepest, mut i) = (0_usize, 0_usize, 0_usize);
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(end) = tag_end(bytes, i) else { break };
        let tag = &html[i..=end];
        i = end + 1;
        if tag.starts_with("<!") || tag.starts_with("<?") {
            continue;
        }
        let closing = tag.starts_with("</");
        let inner = tag.trim_start_matches('<').trim_start_matches('/');
        let name_len = inner
            .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .unwrap_or(inner.len());
        let name = &inner[..name_len];
        let listed = |list: &[&str]| list.iter().any(|n| n.eq_ignore_ascii_case(name));
        if listed(&VOID_ELEMENTS) || listed(&IMPLIED_END) {
            continue;
        }
        if closing {
            depth = depth.saturating_sub(1);
        } else if !tag.ends_with("/>") {
            depth += 1;
            deepest = deepest.max(depth);
        }
    }
    deepest
}

/// Index of the `>` that closes the tag starting at `from`, skipping quoted attribute values.
fn tag_end(bytes: &[u8], from: usize) -> Option<usize> {
    if bytes[from..].starts_with(b"<!--") {
        let rel = find(&bytes[from + 4..], b"-->")?;
        return Some(from + 4 + rel + 2);
    }
    let mut quote: Option<u8> = None;
    for (off, &b) in bytes[from + 1..].iter().enumerate() {
        match (quote, b) {
            (Some(q), c) if c == q => quote = None,
            (None, b'"' | b'\'') => quote = Some(b),
            (None, b'>') => return Some(from + 1 + off),
            _ => {}
        }
    }
    None
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Lowercase tag name of an opening tag such as `<img src="x" />`.
fn tag_name(tag: &str) -> &str {
    let inner = tag.trim_start_matches('<');
    let end = inner
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(inner.len());
    &inner[..end]
}

/// Removes every ` data-sourcepos="..."` attribute.
fn strip_sourcepos(html: &str) -> String {
    const ATTR: &str = " data-sourcepos=\"";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(ATTR) {
        out.push_str(&rest[..at]);
        let after = &rest[at + ATTR.len()..];
        rest = after.find('"').map_or("", |q| &after[q + 1..]);
    }
    out.push_str(rest);
    out
}

/// One hash per top-level element (see the module docs for the invariant).
pub(crate) fn block_hashes(html: &str) -> Vec<String> {
    top_level_elements(html)
        .into_iter()
        .map(|el| hash_hex(strip_sourcepos(el).as_bytes()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_top_level_elements_and_ignores_whitespace() {
        let html = "<h1 id=\"a\">A</h1>\n<p>one <em>two</em></p>\n<ul>\n<li>x</li>\n</ul>\n<hr>\n";
        assert_eq!(
            top_level_elements(html),
            vec![
                "<h1 id=\"a\">A</h1>",
                "<p>one <em>two</em></p>",
                "<ul>\n<li>x</li>\n</ul>",
                "<hr>",
            ]
        );
    }

    #[test]
    fn nested_same_name_elements_stay_in_one_block() {
        let html = "<div><div><p>x</p></div></div><p>y</p>";
        assert_eq!(top_level_elements(html).len(), 2);
    }

    #[test]
    fn void_and_self_closing_elements_do_not_nest() {
        let html = "<p>a<br>b<img src=\"x.png\" alt=\"\"></p><p>c<br/>d</p>";
        assert_eq!(top_level_elements(html).len(), 2);
        let top = "<img src=\"x.png\"><input type=\"checkbox\">";
        assert_eq!(top_level_elements(top).len(), 2);
    }

    #[test]
    fn angle_brackets_inside_attribute_values_do_not_end_the_tag() {
        let html = "<a href=\"x\" title=\"a > b\">t</a><p>z</p>";
        assert_eq!(top_level_elements(html).len(), 2);
    }

    #[test]
    fn empty_and_text_only_input_have_no_blocks() {
        assert_eq!(top_level_elements("").len(), 0);
        assert_eq!(top_level_elements("\n  just text \n").len(), 0);
    }

    #[test]
    fn unbalanced_markup_returns_the_remainder_as_one_block() {
        assert_eq!(
            top_level_elements("<p>a</p><div><p>b"),
            ["<p>a</p>", "<div><p>b"]
        );
    }

    #[test]
    fn hashes_ignore_sourcepos_but_not_content() {
        let a = block_hashes("<p data-sourcepos=\"1:1-1:3\">hi</p>");
        let moved = block_hashes("<p data-sourcepos=\"9:1-9:3\">hi</p>");
        let edited = block_hashes("<p data-sourcepos=\"1:1-1:3\">ho</p>");
        assert_eq!(a, moved);
        assert_ne!(a, edited);
        assert_eq!(a[0].len(), 16);
    }

    #[test]
    fn strip_sourcepos_removes_all_occurrences() {
        let s = "<ul data-sourcepos=\"1:1-2:3\"><li data-sourcepos=\"1:1-1:3\">a</li></ul>";
        assert_eq!(strip_sourcepos(s), "<ul><li>a</li></ul>");
    }
}
