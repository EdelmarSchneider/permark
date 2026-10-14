//! Markdown to sanitized HTML (comrak + ammonia).
//!
//! Mermaid and math stay as marked elements for the UI to enhance lazily:
//! ```` ```mermaid ```` becomes `<pre><code class="language-mermaid">`, math becomes
//! `<span data-math-style="inline|display">`.

use std::borrow::Cow;
use std::path::Path;
use std::sync::LazyLock;

use comrak::arena_tree::NodeEdge;
use comrak::nodes::{AstNode, NodeValue};
use comrak::{format_html, parse_document, Arena, Options};

use super::{blocks, frontmatter, plain, DocKind, RenderCtx, Rendered, Renderer};

/// A line that starts with more container markers (`>` quotes, list bullets) than this is not parsed:
/// comrak's parse time grows quadratically with quote nesting, so the check has to come first.
pub(crate) const MAX_CONTAINER_MARKERS: usize = 100;
/// The deepest tree (parsed Markdown, or the HTML made from it) that is sanitized and handed to the
/// webview. Real documents stay far below this; the sanitizer's cost grows quadratically with depth.
pub(crate) const MAX_TREE_DEPTH: usize = 256;
/// Shown next to the title when a document is displayed as plain text because of its nesting.
const DEEP_NESTING_NOTE: &str = "Shown as plain text: nesting too deep to render";

/// comrak options shared by rendering and search chunking.
pub(crate) fn options() -> Options<'static> {
    let mut o = Options::default();
    let ext = &mut o.extension;
    ext.table = true;
    ext.tasklist = true;
    ext.strikethrough = true;
    ext.autolink = true;
    ext.footnotes = true;
    ext.alerts = true;
    ext.description_lists = true;
    ext.math_dollars = true;
    ext.math_code = true;
    ext.front_matter_delimiter = Some("---".to_owned());
    // Heading ids, without a prefix so that `[x](#my-heading)` links work as authored.
    ext.header_id_prefix = Some(String::new());
    o.render.sourcepos = true;
    o.render.tasklist_classes = true;
    // Raw HTML is passed through here and made safe by the sanitizer afterwards.
    o.render.r#unsafe = true;
    o
}

pub(crate) struct MarkdownRenderer;

/// Whether the file name says Markdown (`.md`, `.markdown`, `.mdown`, `.mkd`, `.mdx`).
pub(crate) fn is_markdown_path(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_ascii_lowercase();
        matches!(e.as_str(), "md" | "markdown" | "mdown" | "mkd" | "mdx")
    })
}

impl Renderer for MarkdownRenderer {
    fn can_handle(&self, path: &Path) -> bool {
        is_markdown_path(path)
    }

    fn render(&self, src: &str, _ctx: &RenderCtx<'_>) -> Rendered {
        render_markdown(src)
    }
}

/// Renders Markdown to sanitized HTML, with the title and front matter found along the way.
pub(crate) fn render_markdown(src: &str) -> Rendered {
    if too_many_container_markers(src) {
        return plain_fallback(src);
    }
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, src, &opts);
    if is_deeper_than(root, MAX_TREE_DEPTH) {
        return plain_fallback(src);
    }

    let mut front: Option<Vec<(String, String)>> = None;
    let mut front_title: Option<String> = None;
    let mut h1_title: Option<String> = None;
    for node in root.descendants() {
        match &node.data.borrow().value {
            NodeValue::FrontMatter(raw) if front.is_none() => {
                let pairs = frontmatter::parse(frontmatter::strip_delimiters(raw));
                front_title = pairs
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("title"))
                    .map(|(_, v)| v.clone())
                    .filter(|v| !v.is_empty());
                front = (!pairs.is_empty()).then_some(pairs);
            }
            NodeValue::Heading(h) if h.level == 1 && h1_title.is_none() => {
                let text = plain_text(node);
                h1_title = (!text.is_empty()).then_some(text);
            }
            _ => {}
        }
    }

    let mut raw = String::new();
    // Writing into a String cannot fail; an error here would only mean an empty document.
    let _ = format_html(root, &opts, &mut raw);
    // Raw HTML in the document can nest without limit even when the Markdown tree is shallow.
    if blocks::max_depth(&raw) > MAX_TREE_DEPTH {
        return plain_fallback(src);
    }
    let html = sanitize(&strip_heading_anchors(&raw));
    Rendered {
        kind: DocKind::Markdown,
        html,
        title: h1_title.or(front_title),
        frontmatter: front,
        note: None,
    }
}

/// The document as escaped text, for sources too deeply nested to render safely.
fn plain_fallback(src: &str) -> Rendered {
    Rendered {
        kind: DocKind::Text,
        html: plain::pre("text", src),
        title: None,
        frontmatter: None,
        note: Some(DEEP_NESTING_NOTE.to_owned()),
    }
}

/// How many container markers (`>` and list bullets) start this line, ignoring spaces between them.
fn container_markers(line: &str) -> usize {
    let bytes = line.as_bytes();
    let (mut at, mut count) = (0, 0);
    loop {
        while matches!(bytes.get(at), Some(b' ' | b'\t')) {
            at += 1;
        }
        let blank_after = |i: usize| matches!(bytes.get(i), Some(b' ' | b'\t'));
        match bytes.get(at) {
            Some(b'>') => at += 1,
            Some(b'-' | b'+' | b'*') if blank_after(at + 1) => at += 1,
            Some(b'0'..=b'9') => {
                let digits = bytes[at..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                let end = at + digits;
                if matches!(bytes.get(end), Some(b'.' | b')')) && blank_after(end + 1) {
                    at = end + 1;
                } else {
                    return count;
                }
            }
            _ => return count,
        }
        count += 1;
    }
}

/// Whether some line of `src` opens more than [`MAX_CONTAINER_MARKERS`] nested quotes or list items.
/// Runs before parsing, in a single linear pass.
pub(crate) fn too_many_container_markers(src: &str) -> bool {
    src.lines()
        .any(|line| container_markers(line) > MAX_CONTAINER_MARKERS)
}

/// Whether the tree under `root` is deeper than `limit` levels (walked iteratively).
pub(crate) fn is_deeper_than<'a>(root: &'a AstNode<'a>, limit: usize) -> bool {
    let mut depth = 0_usize;
    for edge in root.traverse() {
        match edge {
            NodeEdge::Start(_) => {
                depth += 1;
                if depth > limit {
                    return true;
                }
            }
            NodeEdge::End(_) => depth = depth.saturating_sub(1),
        }
    }
    false
}

/// Plain text of a node's inline content (formatting dropped, line breaks as spaces).
pub(crate) fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut out = String::new();
    append_plain_text(node, &mut out);
    out.trim().to_owned()
}

/// Appends the plain text of `node` and its descendants; block boundaries become spaces.
///
/// Walks the tree with an explicit traversal instead of recursion: a document can be nested a hundred
/// thousand levels deep, which would overflow the stack of any thread.
pub(crate) fn append_plain_text<'a>(node: &'a AstNode<'a>, out: &mut String) {
    let block_break = |out: &mut String| {
        if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
    };
    for edge in node.traverse() {
        match edge {
            NodeEdge::Start(n) => match &n.data.borrow().value {
                NodeValue::Text(t) => out.push_str(t),
                NodeValue::Code(c) => out.push_str(&c.literal),
                NodeValue::Math(m) => out.push_str(&m.literal),
                NodeValue::CodeBlock(c) => {
                    block_break(out);
                    out.push_str(&c.literal);
                }
                NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
                NodeValue::Paragraph
                | NodeValue::Item(_)
                | NodeValue::TaskItem(_)
                | NodeValue::TableRow(_)
                | NodeValue::TableCell
                | NodeValue::DescriptionTerm
                | NodeValue::DescriptionDetails
                | NodeValue::Heading(_)
                | NodeValue::BlockQuote => block_break(out),
                // Raw HTML and front matter are not text, and have no children to skip.
                _ => {}
            },
            NodeEdge::End(n) => {
                if matches!(
                    n.data.borrow().value,
                    NodeValue::Paragraph | NodeValue::TableCell | NodeValue::Heading(_)
                ) {
                    block_break(out);
                }
            }
        }
    }
}

/// comrak appends an empty `<a class="anchor">` link to every heading; the heading already has the
/// `id`, so the extra link is dropped to keep headings clean.
fn strip_heading_anchors(html: &str) -> String {
    const OPEN: &str = "<a href=\"#";
    const CLOSE: &str = "</a>";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(OPEN) {
        let after = &rest[at..];
        let Some(end) = after.find(CLOSE) else { break };
        let candidate = &after[..end + CLOSE.len()];
        out.push_str(&rest[..at]);
        if !candidate.contains("class=\"anchor\"") {
            out.push_str(candidate);
        }
        rest = &after[candidate.len()..];
    }
    out.push_str(rest);
    out
}

fn class_token_allowed(token: &str) -> bool {
    if let Some(lang) = token.strip_prefix("language-") {
        return !lang.is_empty()
            && lang
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '+' | '-' | '_' | '#' | '.'));
    }
    token.starts_with("markdown-alert")
        || matches!(
            token,
            "footnotes"
                | "footnote-ref"
                | "footnote-backref"
                | "task-list-item"
                | "task-list-item-checkbox"
                | "contains-task-list"
        )
}

/// A URL attribute value the way the URL parser reads its scheme: tabs, line breaks, other control
/// characters and spaces are dropped (browsers ignore them, so `da<TAB>ta:` is `data:`), and the case
/// is folded.
fn normalized_url(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Raster image types that may be embedded as `data:` URLs. SVG is left out on purpose.
const DATA_IMAGE_TYPES: [&str; 4] = ["png", "jpeg", "gif", "webp"];

/// Whether a (normalized) URL is `data:image/<raster type>` followed by parameters or the payload.
fn is_raster_data_url(normalized: &str) -> bool {
    DATA_IMAGE_TYPES.iter().any(|kind| {
        normalized
            .strip_prefix("data:image/")
            .and_then(|rest| rest.strip_prefix(kind))
            .is_some_and(|rest| rest.starts_with([';', ',']))
    })
}

/// Attributes that hold a URL (checked for the `data:` scheme, which the sanitizer lets through).
fn is_url_attribute(attr: &str) -> bool {
    matches!(
        attr,
        "href" | "src" | "cite" | "action" | "formaction" | "poster" | "data" | "ping" | "longdesc"
    )
}

fn filter_attribute<'u>(element: &str, attr: &str, value: &'u str) -> Option<Cow<'u, str>> {
    match (element, attr) {
        (_, "class") => {
            let kept: Vec<&str> = value
                .split_whitespace()
                .filter(|t| class_token_allowed(t))
                .collect();
            (!kept.is_empty()).then_some(Cow::Owned(kept.join(" ")))
        }
        (_, "id") => (!value.is_empty() && !value.contains(char::is_whitespace))
            .then_some(Cow::Borrowed(value)),
        ("input", "type") => (value == "checkbox").then_some(Cow::Borrowed(value)),
        ("img", "src") => {
            let url = normalized_url(value);
            (!url.starts_with("data:") || is_raster_data_url(&url)).then_some(Cow::Borrowed(value))
        }
        (_, attr) if is_url_attribute(attr) => {
            (!normalized_url(value).starts_with("data:")).then_some(Cow::Borrowed(value))
        }
        _ => Some(Cow::Borrowed(value)),
    }
}

static SANITIZER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut b = ammonia::Builder::default();
    b.add_tags(["section", "input"])
        .add_generic_attributes([
            "id",
            "class",
            "data-sourcepos",
            "data-math-style",
            "aria-label",
        ])
        .add_tag_attributes("input", ["type", "disabled", "checked"])
        .add_tag_attributes("section", ["data-footnotes"])
        .add_tag_attributes(
            "a",
            [
                "data-footnote-ref",
                "data-footnote-backref",
                "data-footnote-backref-idx",
            ],
        )
        .add_tag_attributes("details", ["open"])
        .add_tag_attributes("img", ["width", "height", "alt", "title", "src"])
        .add_url_schemes(["data"])
        // Any `<input>` in a document is inert: only the disabled task-list checkbox is meant to exist.
        .set_tag_attribute_value("input", "disabled", "")
        .attribute_filter(filter_attribute);
    b
});

/// Allowlist-based HTML sanitizer for rendered Markdown.
pub(crate) fn sanitize(html: &str) -> String {
    SANITIZER.clean(html).to_string()
}

/// Names of the elements the sanitizer keeps; exposed for tests of the allowlist.
#[cfg(test)]
pub(crate) fn allowed_tags() -> std::collections::HashSet<&'static str> {
    SANITIZER.clone_tags()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::blocks::top_level_elements;

    /// `(html, title, front matter)` of a document.
    #[allow(clippy::type_complexity)]
    fn render_markdown(src: &str) -> (String, Option<String>, Option<Vec<(String, String)>>) {
        let r = super::render_markdown(src);
        (r.html, r.title, r.frontmatter)
    }

    fn html(src: &str) -> String {
        render_markdown(src).0
    }

    #[test]
    fn headings_get_unicode_safe_ids_without_anchor_links() {
        let h =
            html("# Descrição dos Personagens\n\n## Café & Chá\n\n# Descrição dos Personagens\n");
        assert!(h.contains("id=\"descrição-dos-personagens\""), "{h}");
        assert!(h.contains("id=\"descrição-dos-personagens-1\""), "{h}");
        assert!(h.contains("id=\"café--chá\""), "{h}");
        assert!(!h.contains("class=\"anchor\""), "{h}");
        assert!(!h.contains("<a "), "heading anchors must be stripped: {h}");
    }

    #[test]
    fn gfm_tables_tasklists_strikethrough_autolinks() {
        let h = html(
            "| a | b |\n|:-:|--:|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n~~gone~~ and https://example.com/x\n",
        );
        assert!(h.contains("<table"), "{h}");
        assert!(h.contains("<th"), "{h}");
        assert!(h.contains("type=\"checkbox\""), "{h}");
        assert!(h.contains("checked"), "{h}");
        assert!(h.contains(">gone</del>"), "{h}");
        assert!(h.contains("href=\"https://example.com/x\""), "{h}");
    }

    #[test]
    fn footnotes_alerts_and_description_lists() {
        let h = html("Text[^1]\n\n[^1]: The note.\n\n> [!WARNING]\n> Careful\n\nTerm\n: Details\n");
        assert!(h.contains("footnote"), "{h}");
        assert!(h.contains("The note."), "{h}");
        assert!(h.contains("markdown-alert-warning"), "{h}");
        assert!(h.contains("<dl"), "{h}");
        assert!(h.contains("<dt"), "{h}");
    }

    #[test]
    fn math_and_mermaid_are_left_as_marked_elements() {
        let h = html("Inline $a^2$ and\n\n$$\nx = y\n$$\n\n```mermaid\ngraph LR\n A-->B\n```\n\n```math\ne=mc^2\n```\n");
        assert!(h.contains("data-math-style=\"inline\""), "{h}");
        assert!(h.contains("data-math-style=\"display\""), "{h}");
        assert!(h.contains("class=\"language-mermaid\""), "{h}");
        assert!(h.contains("class=\"language-math\""), "{h}");
        assert!(h.contains("A--&gt;B"), "{h}");
    }

    #[test]
    fn sourcepos_is_kept_on_blocks() {
        let h = html("# T\n\npara\n");
        assert!(h.contains("data-sourcepos=\"1:1-1:3\""), "{h}");
        assert!(h.contains("data-sourcepos=\"3:1-3:4\""), "{h}");
    }

    #[test]
    fn safe_embedded_html_survives() {
        let h = html("<details open><summary>More</summary>\n\nbody <kbd>Ctrl</kbd> H<sub>2</sub>O x<sup>2</sup>\n\n</details>\n\n<img src=\"a.png\" width=\"100\" height=\"50\" alt=\"a\">\n");
        for needle in [
            "<details",
            "<summary>",
            "<kbd>Ctrl</kbd>",
            "<sub>2</sub>",
            "<sup>2</sup>",
            "width=\"100\"",
            "height=\"50\"",
        ] {
            assert!(h.contains(needle), "missing {needle} in {h}");
        }
    }

    #[test]
    fn dangerous_html_is_removed() {
        let h = html(
            "<script>alert(1)</script>\n\n[x](javascript:alert(1))\n\n<a href=\"data:text/html,x\" onclick=\"y()\">d</a>\n\n<img src=\"x.png\" onerror=\"alert(1)\">\n\n<iframe src=\"http://x\"></iframe>\n\n<style>p{}</style>\n\n<div class=\"tm-removed\" style=\"x\">c</div>\n",
        );
        for bad in [
            "<script",
            "javascript:",
            "data:text",
            "onclick",
            "onerror",
            "<iframe",
            "<style",
            "tm-removed",
            "style=",
        ] {
            assert!(!h.contains(bad), "{bad} survived in {h}");
        }
    }

    #[test]
    fn data_images_are_allowed_but_only_image_types() {
        let ok = sanitize("<img src=\"data:image/png;base64,AAAA\">");
        assert!(ok.contains("data:image/png"), "{ok}");
        let bad = sanitize("<img src=\"data:text/html;base64,AAAA\">");
        assert!(!bad.contains("data:text"), "{bad}");
    }

    #[test]
    fn data_urls_cannot_hide_behind_whitespace_control_characters_or_entities() {
        for scheme in [
            "da&#9;ta:",
            "data&#10;:",
            "&#1;data:",
            "&#13;&#10;data:",
            "  DaTa:",
            "d&#97;ta:",
            "&#x20;&#x20;data:",
        ] {
            let url = format!("{scheme}text/html;base64,PHNjcmlwdD4=");
            let h = sanitize(&format!(
                "<a href=\"{url}\">a</a><img src=\"{url}\"><blockquote cite=\"{url}\">q</blockquote>"
            ));
            for attr in ["href=", "src=", "cite="] {
                assert!(!h.contains(attr), "{scheme:?} survived as {attr} in {h}");
            }
        }
    }

    #[test]
    fn only_raster_image_data_urls_are_allowed_and_only_as_image_sources() {
        for ok in ["png", "jpeg", "gif", "webp"] {
            let h = sanitize(&format!("<img src=\"data:image/{ok};base64,AAAA\">"));
            assert!(h.contains(&format!("data:image/{ok};")), "{ok}: {h}");
        }
        let upper = sanitize("<img src=\"DATA:IMAGE/PNG;base64,AAAA\">");
        assert!(upper.contains("src="), "{upper}");
        for bad in [
            "data:image/svg+xml;base64,AAAA",
            "data:image/svg+xml,%3Csvg%3E",
            "data:image/png/../svg+xml;base64,AAAA",
            "data:text/html;base64,AAAA",
            "data:image/x-icon;base64,AAAA",
        ] {
            let h = sanitize(&format!("<img src=\"{bad}\">"));
            assert!(!h.contains("src="), "{bad} survived in {h}");
        }
        // An image type does not make a data URL acceptable as a link target.
        let link = sanitize("<a href=\"data:image/png;base64,AAAA\">x</a>");
        assert!(!link.contains("href="), "{link}");
    }

    #[test]
    fn javascript_urls_are_still_removed_in_every_spelling() {
        for url in [
            "javascript:alert(1)",
            "java&#9;script:alert(1)",
            "&#1;javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "vbscript:x",
        ] {
            let h = sanitize(&format!("<a href=\"{url}\">x</a>"));
            assert!(!h.contains("href="), "{url} survived in {h}");
        }
    }

    #[test]
    fn inputs_are_forced_to_be_inert_checkboxes() {
        let h = sanitize("<input type=\"text\" value=\"x\"><input type=\"checkbox\" checked>");
        assert!(!h.contains("type=\"text\""), "{h}");
        assert!(h.contains("type=\"checkbox\""), "{h}");
        assert!(h.matches("disabled").count() == 2, "{h}");
    }

    #[test]
    fn classes_are_restricted_to_known_safe_tokens() {
        let h = sanitize("<div class=\"markdown-alert markdown-alert-note evil\"><code class=\"language-rust x\">a</code></div>");
        assert!(
            h.contains("class=\"markdown-alert markdown-alert-note\""),
            "{h}"
        );
        assert!(h.contains("class=\"language-rust\""), "{h}");
        assert!(!h.contains("evil"), "{h}");
    }

    #[test]
    fn relative_links_and_images_pass_through() {
        let h = html("[doc](guide/intro.md#start) ![pic](img/a%20b.png)\n");
        assert!(h.contains("href=\"guide/intro.md#start\""), "{h}");
        assert!(h.contains("src=\"img/a%20b.png\""), "{h}");
    }

    #[test]
    fn front_matter_is_parsed_and_not_rendered() {
        let (h, title, fm) =
            render_markdown("---\ntitle: My Doc\ntags: [a, b]\n---\n\n# Heading\n\nBody\n");
        assert!(!h.contains("My Doc"), "{h}");
        assert!(!h.contains("tags"), "{h}");
        assert_eq!(title.as_deref(), Some("Heading"));
        assert_eq!(
            fm,
            Some(vec![
                ("title".to_owned(), "My Doc".to_owned()),
                ("tags".to_owned(), "a, b".to_owned())
            ])
        );
    }

    #[test]
    fn title_falls_back_to_front_matter_then_none() {
        let (_, t, _) = render_markdown("---\ntitle: From FM\n---\n\nNo heading\n");
        assert_eq!(t.as_deref(), Some("From FM"));
        let (_, t, fm) = render_markdown("Just text\n");
        assert_eq!((t, fm), (None, None));
    }

    #[test]
    fn title_is_plain_text_of_the_first_h1() {
        let (_, t, _) = render_markdown("## Sub\n\n# The *great* `code` title\n\n# Second\n");
        assert_eq!(t.as_deref(), Some("The great code title"));
    }

    #[test]
    fn empty_document_renders_to_nothing() {
        let (h, t, fm) = render_markdown("");
        assert_eq!((h.as_str(), t, fm), ("", None, None));
    }

    #[test]
    fn crlf_free_input_with_unicode_survives() {
        let h = html("Olá, **mundo** — 你好 🎉\n");
        assert!(h.contains(">mundo</strong> — 你好 🎉"), "{h}");
    }

    #[test]
    fn plain_text_drops_formatting() {
        let arena = Arena::new();
        let root = parse_document(
            &arena,
            "# A *b* `c`\n\nSome [link](x) text.\n\n| h1 | h2 |\n|--|--|\n| x | y |\n",
            &options(),
        );
        assert_eq!(plain_text(root), "A b c Some link text. h1 h2 x y");
    }

    #[test]
    fn allowlist_contains_the_documented_tags() {
        let tags = allowed_tags();
        for t in [
            "details", "summary", "kbd", "sub", "sup", "input", "section", "table",
        ] {
            assert!(tags.contains(t), "{t}");
        }
    }

    /// Nested emphasis (`_a _a ... a_ a_`) gives an AST as deep as the number of markers.
    fn nested_emphasis(levels: usize) -> String {
        format!("{}b{}\n", "_a ".repeat(levels), " a_".repeat(levels))
    }

    #[test]
    fn plain_text_of_a_100k_deep_tree_does_not_overflow_the_stack() {
        let arena = Arena::new();
        let root = parse_document(&arena, &nested_emphasis(100_000), &options());
        let text = plain_text(root);
        assert!(text.contains('b') && text.len() > 100_000, "{}", text.len());
    }

    #[test]
    fn extremely_nested_quotes_are_shown_as_plain_text_without_being_parsed() {
        let src = format!("{} x\n", ">".repeat(100_000));
        let started = std::time::Instant::now();
        let r = super::render_markdown(&src);
        assert_eq!(r.kind, DocKind::Text);
        assert!(r.html.starts_with("<pre>"), "{}", &r.html[..40]);
        assert!(r.note.is_some_and(|n| n.contains("nest")));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the nesting guard must run before the (quadratic) parse"
        );
    }

    #[test]
    fn deeply_nested_lists_and_mixed_containers_are_degraded_too() {
        for src in [
            format!("{}x\n", "- ".repeat(5_000)),
            format!("{}x\n", "> - ".repeat(2_500)),
            format!("{}x\n", "1. ".repeat(5_000)),
        ] {
            let r = super::render_markdown(&src);
            assert_eq!(r.kind, DocKind::Text);
            assert!(r.note.is_some());
        }
    }

    #[test]
    fn deep_inline_nesting_is_degraded_after_parsing() {
        let started = std::time::Instant::now();
        let r = super::render_markdown(&nested_emphasis(50_000));
        assert_eq!(r.kind, DocKind::Text);
        assert!(r.note.is_some());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "ammonia is quadratic on deep trees and must not see them"
        );
    }

    #[test]
    fn deeply_nested_raw_html_is_degraded_before_it_reaches_the_sanitizer() {
        let src = format!("{}x\n", "<div>".repeat(50_000));
        let started = std::time::Instant::now();
        let r = super::render_markdown(&src);
        assert_eq!(r.kind, DocKind::Text);
        assert!(r.note.is_some());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[test]
    fn realistic_nesting_is_rendered_normally() {
        let mut src = String::new();
        for depth in 0..12 {
            src += &"  ".repeat(depth);
            src += "- level ";
            src += &depth.to_string();
            src += "\n";
        }
        src.push_str("\n> a > b\n>\n> > c\n>\n> > > d\n\n<div><div><p>x</p></div></div>\n");
        let r = super::render_markdown(&src);
        assert_eq!(r.kind, DocKind::Markdown);
        assert!(r.note.is_none());
        assert!(r.html.contains("level 11"), "{}", r.html);
    }

    #[test]
    fn front_matter_with_endless_brackets_does_not_overflow_the_stack() {
        let src = format!(
            "---\ntitle: T\ntags: {}{}\n---\n\nbody\n",
            "[".repeat(100_000),
            "]".repeat(100_000)
        );
        let r = super::render_markdown(&src);
        assert_eq!(r.kind, DocKind::Markdown);
        assert!(r.html.contains("body"));
        assert!(r.frontmatter.is_some_and(|f| f.len() == 2));
    }

    #[test]
    fn top_level_blocks_match_the_produced_elements() {
        let h = html("# T\n\npara\n\n- a\n- b\n\n```rust\nfn main() {}\n```\n\n> quote\n\n---\n\n| a |\n|---|\n| 1 |\n");
        assert_eq!(top_level_elements(&h).len(), 7, "{h}");
    }
}
