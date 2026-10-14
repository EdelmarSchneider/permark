//! Markdown to sanitized HTML (comrak + ammonia).
//!
//! Mermaid and math stay as marked elements for the UI to enhance lazily:
//! ```` ```mermaid ```` becomes `<pre><code class="language-mermaid">`, math becomes
//! `<span data-math-style="inline|display">`.

use std::borrow::Cow;
use std::path::Path;
use std::sync::LazyLock;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{format_html, parse_document, Arena, Options};

use super::{frontmatter, DocKind, RenderCtx, Rendered, Renderer};

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

impl Renderer for MarkdownRenderer {
    fn can_handle(&self, path: &Path) -> bool {
        path.extension().is_some_and(|e| {
            let e = e.to_string_lossy().to_ascii_lowercase();
            matches!(e.as_str(), "md" | "markdown" | "mdown" | "mkd" | "mdx")
        })
    }

    fn render(&self, src: &str, _ctx: &RenderCtx<'_>) -> Rendered {
        render_markdown(src)
    }
}

/// Renders Markdown to sanitized HTML, with the title and front matter found along the way.
pub(crate) fn render_markdown(src: &str) -> Rendered {
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, src, &opts);

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
    let html = sanitize(&strip_heading_anchors(&raw));
    Rendered {
        kind: DocKind::Markdown,
        html,
        title: h1_title.or(front_title),
        frontmatter: front,
    }
}

/// Plain text of a node's inline content (formatting dropped, line breaks as spaces).
pub(crate) fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut out = String::new();
    append_plain_text(node, &mut out);
    out.trim().to_owned()
}

/// Appends the plain text of `node` and its descendants; block boundaries become spaces.
pub(crate) fn append_plain_text<'a>(node: &'a AstNode<'a>, out: &mut String) {
    let block_break = |out: &mut String| {
        if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
    };
    match &node.data.borrow().value {
        NodeValue::Text(t) => out.push_str(t),
        NodeValue::Code(c) => out.push_str(&c.literal),
        NodeValue::Math(m) => out.push_str(&m.literal),
        NodeValue::CodeBlock(c) => {
            block_break(out);
            out.push_str(&c.literal);
            return;
        }
        NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
        NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) | NodeValue::FrontMatter(_) => return,
        NodeValue::Paragraph
        | NodeValue::Item(_)
        | NodeValue::TaskItem(_)
        | NodeValue::TableRow(_)
        | NodeValue::TableCell
        | NodeValue::DescriptionTerm
        | NodeValue::DescriptionDetails
        | NodeValue::Heading(_)
        | NodeValue::BlockQuote => block_break(out),
        _ => {}
    }
    for child in node.children() {
        append_plain_text(child, out);
    }
    if matches!(
        node.data.borrow().value,
        NodeValue::Paragraph | NodeValue::TableCell | NodeValue::Heading(_)
    ) {
        block_break(out);
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
        ("a", "href") => (!value.trim_start().to_ascii_lowercase().starts_with("data:"))
            .then_some(Cow::Borrowed(value)),
        ("img", "src") => {
            let v = value.trim_start().to_ascii_lowercase();
            let ok = !v.starts_with("data:")
                || ["png", "jpeg", "jpg", "gif", "webp", "avif"]
                    .iter()
                    .any(|t| v.starts_with(&format!("data:image/{t};")));
            ok.then_some(Cow::Borrowed(value))
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

    #[test]
    fn top_level_blocks_match_the_produced_elements() {
        let h = html("# T\n\npara\n\n- a\n- b\n\n```rust\nfn main() {}\n```\n\n> quote\n\n---\n\n| a |\n|---|\n| 1 |\n");
        assert_eq!(top_level_elements(&h).len(), 7, "{h}");
    }
}
