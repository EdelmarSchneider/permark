//! Markdown pipeline: spec tests (what is rendered) and adversarial tests (what must never happen).

#![allow(clippy::assert_is_empty)] // `assert!(x.is_empty())` reads best for "nothing happened"

use std::collections::HashSet;
use std::fmt::Write;

use super::*;
use crate::format::{DocEnv, Field, FieldValue, FrontMatterSyntax, MarkdownFeatures, PromptBlocks};

struct Files(&'static [&'static str]);
impl DocEnv for Files {
    fn file_exists(&self, path: &str) -> bool {
        self.0.contains(&path)
    }
}

fn card(syntax: FrontMatterSyntax, raw: &str) -> Vec<Field> {
    // a stand-in parser: one text field per `key: value` line
    let _ = syntax;
    raw.lines()
        .filter_map(|line| line.split_once(": "))
        .map(|(k, v)| Field {
            key: k.to_owned(),
            value: FieldValue::Text { text: v.to_owned() },
        })
        .collect()
}

fn all_features() -> MarkdownFeatures {
    MarkdownFeatures {
        prompt_blocks: Some(PromptBlocks {
            tags: crate::format::DEFAULT_PROMPT_TAGS,
        }),
        front_matter_card: Some(card),
    }
}

fn render_with(
    text: &str,
    path: &str,
    features: &MarkdownFeatures,
    known: &HashSet<String>,
) -> Rendered {
    let env = Files(&["docs/plan.md", "src/main.rs"]);
    render(
        text,
        &RenderCx {
            path,
            env: &env,
            features,
            known,
        },
    )
}

fn parts(
    text: &str,
    features: &MarkdownFeatures,
) -> (
    Vec<Block>,
    Vec<OutlineItem>,
    Option<FrontMatter>,
    Vec<&'static str>,
) {
    let rendered = render_with(text, "docs/a.md", features, &HashSet::new());
    match rendered.body {
        Body::Markdown {
            blocks,
            outline,
            front_matter,
        } => (blocks, outline, front_matter, rendered.notes),
        other => panic!("not markdown: {other:?}"),
    }
}

fn html_of(text: &str, features: &MarkdownFeatures) -> String {
    parts(text, features)
        .0
        .iter()
        .map(|b| b.html.clone().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

fn plain_html(text: &str) -> String {
    html_of(text, &MarkdownFeatures::default())
}

/// `html` without the block marks (`data-b`) the pipeline puts on top-level elements.
fn unmarked(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(" data-b=\"") {
        out.push_str(&rest[..at]);
        let value = &rest[at + 9..];
        rest = value.find('"').map_or("", |end| &value[end + 1..]);
    }
    out.push_str(rest);
    out
}

// ---- spec ---------------------------------------------------------------------------------------------------------

#[test]
fn gfm_features_render() {
    let md = "# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n~~strike~~ https://example.com\n\nText[^1]\n\n[^1]: Note.\n\n> [!WARNING]\n> careful\n\nterm\n: definition\n\n$a^2$ and\n\n$$\nx\n$$\n\n```rust\nfn main() {}\n```\n";
    let html = plain_html(md);
    for needle in [
        "<h1",
        "<table",
        "<th dir=\"auto\">a</th>",
        "type=\"checkbox\"",
        "checked",
        "task-list-item",
        "<del>strike</del>",
        "href=\"https://example.com\"",
        "footnote-ref",
        "footnotes",
        "markdown-alert-warning",
        "<dl",
        "data-math-style=\"inline\"",
        "data-math-style=\"display\"",
        "class=\"language-rust\"",
    ] {
        assert!(html.contains(needle), "{needle} missing in {html}");
    }
}

#[test]
fn a_character_reference_right_after_inline_math_keeps_the_formula() {
    // no digit may follow the closing `$` (`$y$5` is text): the Edit-mode editor writes a digit typed there as a
    // numeric character reference, which must read as the formula followed by the digit
    let html = plain_html("Cost $x$&#53; now, $z$&#x35; here and $y$5 there\n");
    let formulas = html.matches("data-math-style=\"inline\"").count();
    assert_eq!(formulas, 2, "{html}");
    assert!(html.contains(">x</span>5 now"), "{html}");
    assert!(html.contains(">z</span>5 here"), "{html}");
    assert!(html.contains("$y$5 there"), "{html}");
}

#[test]
fn bracket_math_renders_as_formulas_outside_code() {
    // `\(..\)` and `\[..\]` (LaTeX delimiters, common in generated text) are formulas like `$..$` and `$$..$$`; code
    // spans and blocks keep them as written, and an escaped backslash before `(` starts nothing
    let md = "Inline \\(a+b\\) here.\n\n\\[\nx^2\n\\]\n\nCode `\\(c\\)` and \\\\(d\\) stay text.\n\n```\n\\(e\\)\n```\n";
    let html = plain_html(md);
    assert_eq!(
        html.matches("data-math-style=\"inline\"").count(),
        1,
        "{html}"
    );
    assert_eq!(
        html.matches("data-math-style=\"display\"").count(),
        1,
        "{html}"
    );
    assert!(html.contains(">a+b</span>"), "{html}");
    assert!(html.contains(">\\(c\\)</code>"), "{html}");
    assert!(html.contains("\\(d) stay"), "{html}");
    assert!(html.contains("\\(e\\)\n</code></pre>"), "{html}");
}

#[test]
fn bracket_display_math_stands_only_on_lines_of_its_own() {
    // escaped brackets in prose (what HTML converters write) stay text with their inner Markdown; `\[..\]` is a
    // formula on lines of its own or as a whole paragraph; `\(..\)` is one anywhere
    let md = concat!(
        "see \\[1\\] and \\[2\\] here, \\[*em* [r](r.md)\\] and \\[3 \\[4\\]\n\n",
        "\\[\nE = mc^2\n\\]\n\n",
        "\\[ whole \\]\n\n",
        "text\n\\[\nown\n\\]  \nmore \\(z\\)\n\n",
        "> \\[\n> quoted\n> \\]\n\n",
        "\\[ a \\] trailing\n\n",
        "lead \\[\nb\n\\]\n\n",
        "`\\[c\\]` and [\\[5\\]](#n)\n\n",
        "| \\[6\\] | x |\n|---|---|\n\n",
        "# \\[7\\]\n"
    );
    let html = plain_html(md);
    assert_eq!(
        html.matches("data-math-style=\"display\"").count(),
        4,
        "{html}"
    );
    assert_eq!(
        html.matches("data-math-style=\"inline\"").count(),
        1,
        "{html}"
    );
    for needle in [
        "see [1] and [2] here, [<em>em</em> <a",
        ">r</a>] and [3 [4]</p>",
        ">\nE = mc^2\n</span>",
        "> whole </span>",
        ">\nown\n</span>",
        ">\nquoted\n</span>",
        "[ a ] trailing",
        "lead [\nb\n]",
        ">\\[c\\]</code>",
        ">[5]</a>",
        ">[6]</th>",
        ">[7]</h1>",
    ] {
        assert!(html.contains(needle), "{needle} missing in {html}");
    }
}

#[test]
fn table_column_alignment_reaches_the_cells() {
    let html = plain_html("| l | c | r | n |\n|:--|:-:|--:|---|\n| 1 | 2 | 3 | 4 |\n");
    for needle in [
        "<th align=\"left\"",
        "<th align=\"center\"",
        "<th align=\"right\"",
        "<td align=\"center\"",
        "<td align=\"right\"",
    ] {
        assert!(html.contains(needle), "{needle} missing in {html}");
    }
}

#[test]
fn readme_centering_and_named_anchors_survive() {
    let md = "<p align=\"center\">\n  <img src=\"logo.png\" width=\"80\">\n</p>\n\n<h1 align=\"center\">Name</h1>\n\n<a name=\"setup\"></a>\n\nSee [setup](#setup).\n";
    let html = plain_html(md);
    assert_eq!(
        html.matches(" align=\"center\" dir=\"auto\">").count(),
        2,
        "{html}"
    );
    for needle in ["<a name=\"setup\">", "href=\"#setup\""] {
        assert!(html.contains(needle), "{needle} missing in {html}");
    }
}

#[test]
fn highlight_emoji_shortcodes_and_inline_footnotes_render() {
    let html = unmarked(&plain_html(
        "A ==bright== word :smile: and :+1: here^[An *inline* note.].\n",
    ));
    for needle in [
        "<mark>bright</mark>",
        "<span class=\"emoji\" title=\":smile:\">\u{1f604}</span>",
        "<span class=\"emoji\" title=\":+1:\">\u{1f44d}</span>",
        "href=\"#fn-__inline_1\"",
        "data-footnote-ref",
        "<em>inline</em> note.",
    ] {
        assert!(html.contains(needle), "{needle} missing in {html}");
    }
}

#[test]
fn escaped_brackets_in_an_inline_note_stay_text() {
    let html = plain_html("A word^[see \\[1\\] and \\[*2*\\]] here.\n\nmore \\[3\\]^[\\[4\\]]\n");
    assert!(!html.contains("data-math-style"), "{html}");
    for needle in ["see [1] and [<em>2</em>]", "more [3]", ">[4]"] {
        assert!(html.contains(needle), "{needle} missing in {html}");
    }
}

#[test]
fn highlight_and_shortcode_lookalikes_stay_as_written() {
    for (md, kept) in [
        ("Code `a==b==c` here\n", "<code>a==b==c</code>"),
        ("Math $a==b==c$ here\n", ">a==b==c</span>"),
        ("```\n==x== :smile: ^[y]\n```\n", "==x== :smile: ^[y]"),
        ("C++ and C++ stay\n", "C++ and C++ stay"),
        ("At 12:30:45 sharp\n", "At 12:30:45 sharp"),
        ("An :unknown_code: stays\n", "An :unknown_code: stays"),
        ("If a == b and c == d\n", "If a == b and c == d"),
        ("Three ===x=== stay\n", "Three ===x=== stay"),
        // single tildes keep GitHub's strikethrough; no subscript, superscript, underline or spoiler syntax
        ("One ~gone~ word\n", "<del>gone</del>"),
        (
            "No ^sup^ or ++ins++ or ||spoiler||\n",
            "No ^sup^ or ++ins++ or ||spoiler||",
        ),
        ("Some __strong__ text\n", "<strong>strong</strong>"),
    ] {
        let html = unmarked(&plain_html(md));
        assert!(html.contains(kept), "{md:?}: {kept} missing in {html}");
        assert!(!html.contains("<mark"), "{md:?} -> {html}");
        assert!(!html.contains("class=\"emoji\""), "{md:?} -> {html}");
    }
}

#[test]
fn obsidian_callouts_render_as_alerts_with_their_kind_title_and_fold() {
    for (md, open_tag, title) in [
        (
            "> [!info]\n> Body.\n",
            "<div class=\"markdown-alert markdown-alert-note markdown-alert-kind-info\">",
            "<p class=\"markdown-alert-title\" dir=\"auto\">Info</p>",
        ),
        (
            "> [!faq] Why *now*?\n> Body.\n",
            "<div class=\"markdown-alert markdown-alert-warning markdown-alert-kind-question\">",
            "<p class=\"markdown-alert-title\" dir=\"auto\">Why now?</p>",
        ),
        (
            "> [!Success]\n> Body.\n",
            "<div class=\"markdown-alert markdown-alert-tip markdown-alert-kind-success\">",
            "<p class=\"markdown-alert-title\" dir=\"auto\">Success</p>",
        ),
        (
            "> [!my-kind]\n> Body.\n",
            "<div class=\"markdown-alert markdown-alert-note\">",
            "<p class=\"markdown-alert-title\" dir=\"auto\">My-kind</p>",
        ),
        (
            "> [!danger]- Folded\n> Body.\n",
            "<details class=\"markdown-alert markdown-alert-caution markdown-alert-kind-danger\">",
            "<summary class=\"markdown-alert-title\">Folded</summary>",
        ),
        (
            "> [!quote]+\n> Body.\n",
            "<details class=\"markdown-alert markdown-alert-quote\" open=\"\">",
            "<summary class=\"markdown-alert-title\">Quote</summary>",
        ),
        // GitHub's own kinds fold too, and keep the classes they always had
        (
            "> [!NOTE]- Later\n> Body.\n",
            "<details class=\"markdown-alert markdown-alert-note\">",
            "<summary class=\"markdown-alert-title\">Later</summary>",
        ),
        (
            "> [!WARNING]\n> Body.\n",
            "<div class=\"markdown-alert markdown-alert-warning\">",
            "<p class=\"markdown-alert-title\" dir=\"auto\">Warning</p>",
        ),
    ] {
        let html = unmarked(&plain_html(md));
        assert!(
            html.contains(open_tag),
            "{md:?}: {open_tag} missing in {html}"
        );
        assert!(html.contains(title), "{md:?}: {title} missing in {html}");
        assert!(
            html.contains("<p dir=\"auto\">Body.</p>"),
            "{md:?} -> {html}"
        );
        assert!(!html.contains("[!"), "{md:?}: marker left in {html}");
    }
    // nested, and a marker alone on its line followed by more text in the same paragraph
    let html = unmarked(&plain_html(
        "> [!tip]\n> First line\n> > [!bug] Inner\n> > x\n",
    ));
    assert!(html.contains("markdown-alert-kind-bug"), "{html}");
    assert!(html.contains("<p dir=\"auto\">First line</p>"), "{html}");
    // not a callout: a marker after other text, a plain quote
    for md in ["> Text [!info] later\n", "> Just a quote\n", "> [!]\n> x\n"] {
        let html = unmarked(&plain_html(md));
        assert!(html.contains("<blockquote>"), "{md:?} -> {html}");
        assert!(!html.contains("markdown-alert"), "{md:?} -> {html}");
    }
}

#[test]
fn colon_containers_render_as_alerts() {
    for (md, open_tag, title) in [
        (
            ":::tip\nBody.\n:::\n",
            "<div class=\"markdown-alert markdown-alert-tip\">",
            "<p class=\"markdown-alert-title\" dir=\"auto\">Tip</p>",
        ),
        (
            "::: warning Mind the gap\nBody.\n:::\n",
            "<div class=\"markdown-alert markdown-alert-warning\">",
            "<p class=\"markdown-alert-title\" dir=\"auto\">Mind the gap</p>",
        ),
        (
            ":::info[Read this]\nBody.\n:::\n",
            "<div class=\"markdown-alert markdown-alert-note markdown-alert-kind-info\">",
            "<p class=\"markdown-alert-title\" dir=\"auto\">Read this</p>",
        ),
        (
            ":::{danger}\nBody.\n:::\n",
            "<div class=\"markdown-alert markdown-alert-caution markdown-alert-kind-danger\">",
            "<p class=\"markdown-alert-title\" dir=\"auto\">Danger</p>",
        ),
        (
            "::: details Click me\nBody.\n:::\n",
            "<details class=\"markdown-alert markdown-alert-quote markdown-alert-kind-details\">",
            "<summary class=\"markdown-alert-title\">Click me</summary>",
        ),
    ] {
        let html = unmarked(&plain_html(md));
        assert!(
            html.contains(open_tag),
            "{md:?}: {open_tag} missing in {html}"
        );
        assert!(html.contains(title), "{md:?}: {title} missing in {html}");
        assert!(
            html.contains("<p dir=\"auto\">Body.</p>"),
            "{md:?} -> {html}"
        );
    }
    // a container of no known kind keeps its content, in no box
    let html = unmarked(&plain_html("::: columns\nBody.\n:::\n"));
    assert!(
        html.contains("<div>\n<p dir=\"auto\">Body.</p>\n</div>"),
        "{html}"
    );
    assert!(!html.contains("markdown-alert"), "{html}");
}

#[test]
fn callouts_and_containers_are_blocks_the_visual_editor_keeps() {
    let md = "> [!info] Title\n> Body.\n\n> Plain quote.\n\n:::tip\nBody.\n:::\n\n> [!NOTE]\n> x\n";
    let found: Vec<(u32, u32, SegmentKind, Option<LockReason>)> =
        segments(md, &MarkdownFeatures::default())
            .into_iter()
            .map(|s| (s.line_start, s.line_end, s.kind, s.lock))
            .collect();
    assert_eq!(
        found,
        vec![
            (1, 2, SegmentKind::Alert, Some(LockReason::Kind)),
            (4, 4, SegmentKind::Quote, None),
            (6, 8, SegmentKind::Alert, Some(LockReason::Kind)),
            (10, 11, SegmentKind::Alert, Some(LockReason::Kind)),
        ]
    );
}

#[test]
fn a_paragraph_with_inline_syntax_stays_one_editable_block() {
    // an inline note becomes a footnote of the page, but its paragraph is still the only block of its line
    let md = "Mark ==this==, :smile: and a note^[Inline *note*.].\n\nNext.\n";
    let found: Vec<(u32, u32, SegmentKind, Option<LockReason>)> =
        segments(md, &MarkdownFeatures::default())
            .into_iter()
            .map(|s| (s.line_start, s.line_end, s.kind, s.lock))
            .collect();
    assert_eq!(
        found,
        vec![
            (1, 1, SegmentKind::Paragraph, None),
            (3, 3, SegmentKind::Paragraph, None),
        ]
    );
}

#[test]
fn code_info_extras_become_checked_data_attributes() {
    for (md, code_tag) in [
        (
            "```rust title=\"main.rs\" {1,3-5} showLineNumbers\nfn main() {}\n```\n",
            "<code class=\"language-rust\" data-title=\"main.rs\" data-lines=\"1,3-5\" data-numbers=\"1\">",
        ),
        (
            "```py hl_lines=\"2 4-5\" linenums=\"10\" title='a b.py'\nx\n```\n",
            "<code class=\"language-py\" data-title=\"a b.py\" data-lines=\"2,4-5\" data-numbers=\"10\">",
        ),
        (
            "```js{2}\nx\n```\n",
            "<code class=\"language-js\" data-lines=\"2\">",
        ),
        (
            "```text {.line-numbers}\nx\n```\n",
            "<code class=\"language-text\" data-numbers=\"1\">",
        ),
        (
            "```ts showLineNumbers{7} filename=app.ts\nx\n```\n",
            "<code class=\"language-ts\" data-title=\"app.ts\" data-numbers=\"7\">",
        ),
        ("```rust\nx\n```\n", "<code class=\"language-rust\">"),
        ("```\nx\n```\n", "<pre><code>x"),
    ] {
        let html = unmarked(&plain_html(md));
        assert!(html.contains(code_tag), "{md:?}: {code_tag} missing in {html}");
    }
    // nothing else of the info string reaches the page: unknown keys, classes, ids, handlers; bad ranges go
    let html = unmarked(&plain_html(
        "```js title=\"<b>x</b>\" onclick=alert(1) {.evil #id key=v} {0,5-2,99999999999,3}\nx\n```\n",
    ));
    assert!(
        html.contains(
            "<code class=\"language-js\" data-title=\"&lt;b&gt;x&lt;/b&gt;\" data-lines=\"3\">"
        ),
        "{html}"
    );
    for bad in ["onclick", "evil", "id=", "key="] {
        assert!(!html.contains(bad), "{bad} in {html}");
    }
}

#[test]
fn blocks_have_lines_kinds_and_marks() {
    let (blocks, _, _, _) = parts(
        "# One\n\nPara\ntwo lines\n\n- a\n- b\n\n```\ncode\n```\n",
        &MarkdownFeatures::default(),
    );
    let shape: Vec<_> = blocks
        .iter()
        .map(|b| (b.kind.as_str(), b.line_start, b.line_end))
        .collect();
    assert_eq!(shape[0], ("h1", 1, 1));
    assert_eq!(shape[1], ("p", 3, 4));
    assert_eq!((shape[2].0, shape[2].1), ("ul", 6));
    assert_eq!(shape[3], ("pre", 9, 11));
    for block in &blocks {
        let html = block.html.as_ref().unwrap();
        assert!(html.contains(&format!("data-b=\"{}\"", block.id)), "{html}");
        assert!(!html.contains("data-sourcepos"));
    }
}

#[test]
fn outline_has_anchor_ids_and_blocks() {
    let md = "# A\n\ntext\n\n## B `code` *em*\n\n## B `code` *em*\n\n> ### Quoted\n\n#### \n";
    let (blocks, outline, _, _) = parts(md, &MarkdownFeatures::default());
    let shape: Vec<_> = outline
        .iter()
        .map(|o| (o.level, o.label.as_str(), o.id.as_str(), o.line))
        .collect();
    assert_eq!(
        shape,
        [
            (1, "A", "a", 1),
            (2, "B code em", "b-code-em", 5),
            (2, "B code em", "b-code-em-1", 7),
            (3, "Quoted", "quoted", 9),
            (4, "", "", 11),
        ]
    );
    // no extra anchor links from the parser
    assert!(!html_of(md, &MarkdownFeatures::default()).contains("aria-label"));
    // the ids are the ones in the HTML
    let html = html_of(md, &MarkdownFeatures::default());
    assert!(html.contains("id=\"b-code-em-1\""), "{html}");
    // each item points at the block holding its line
    assert_eq!(outline[0].block, blocks[0].id);
    assert_eq!(outline[3].block, blocks[4].id);
}

#[test]
fn front_matter_becomes_a_card_or_a_code_block() {
    let md = "---\ntitle: Plan\nowner: me\n---\n# Body\n";
    let (blocks, outline, front, _) = parts(md, &all_features());
    let front = front.unwrap();
    assert_eq!(front.syntax, FrontMatterSyntax::Yaml);
    assert_eq!(front.raw, "title: Plan\nowner: me\n");
    assert_eq!(front.fields.len(), 2);
    // no block for it; the heading keeps its line number
    assert_eq!(blocks.len(), 1);
    assert_eq!(outline[0].line, 5);
    // card plugin off: a code block, same lines
    let (blocks, outline, front, _) = parts(md, &MarkdownFeatures::default());
    assert!(front.is_none());
    assert_eq!(blocks[0].kind, "pre");
    assert!(blocks[0].html.as_ref().unwrap().contains("language-yaml"));
    assert_eq!(outline[0].line, 5);
    // TOML
    let (_, _, front, _) = parts("+++\nname = \"x\"\n+++\ntext", &all_features());
    assert_eq!(front.unwrap().syntax, FrontMatterSyntax::Toml);
    // a rule in the middle is not front matter
    let (blocks, _, front, _) = parts("text\n\n---\n\nmore\n", &all_features());
    assert!(front.is_none());
    assert_eq!(blocks.len(), 3);
}

#[test]
fn prompt_blocks_fold_and_keep_inner_markdown() {
    let md = "<context>\n# Inside\n\n- item\n</context>\n\nAfter\n";
    let (blocks, outline, _, _) = parts(md, &all_features());
    assert_eq!(blocks.len(), 2);
    let html = blocks[0].html.as_ref().unwrap();
    assert!(html.starts_with("<details"), "{html}");
    assert!(html.contains("class=\"prompt-block\"") && html.contains("data-tag=\"context\""));
    assert!(
        html.contains("<summary>context</summary>")
            && html.contains("<h1")
            && html.contains("<li dir=\"auto\">item</li>")
    );
    // lines: the details block covers the tags
    assert_eq!((blocks[0].line_start, blocks[0].line_end), (2, 4));
    assert_eq!(outline[0].line, 2);
    assert_eq!(outline[0].block, blocks[0].id);
    // plugin off: raw tags go to the sanitizer (unknown tags are dropped, text kept)
    let off = plain_html(md);
    assert!(!off.contains("<details") && !off.contains("prompt-block"));
}

#[test]
fn template_variables_and_file_references() {
    let md = "Dear {{ name }}, see @docs/plan.md and @docs/nope.md.\n\n`{{code}}` and [{{link}}](x.md)\n\n```\n{{fence}}\n```\n";
    let html = html_of(md, &all_features());
    assert!(
        html.contains("<span class=\"tvar\">{{name}}</span>"),
        "{html}"
    );
    assert!(
        html.contains("<a class=\"at-ref\" href=\"/docs/plan.md\">@docs/plan.md</a>"),
        "{html}"
    );
    assert!(html.contains("@docs/nope.md"));
    assert!(!html.contains("href=\"/docs/nope.md\""));
    assert!(
        !html.contains("tvar\">{{code}}")
            && !html.contains("tvar\">{{fence}}")
            && !html.contains("tvar\">{{link}}")
    );
    // off: plain text
    let off = plain_html("Dear {{ name }} @docs/plan.md");
    assert!(!off.contains("tvar") && !off.contains("at-ref"));
}

#[test]
fn images_and_media_go_through_the_asset_protocol() {
    let html = plain_html(
        "![alt](<img/a b.png> \"t\") ![w](https://example.com/x.png) <video src=\"v.mp4\"></video>",
    );
    assert!(
        html.contains("src=\"pm-asset:docs/img/a%20b.png\""),
        "{html}"
    );
    assert!(html.contains("src=\"https://example.com/x.png\""));
    assert!(html.contains("src=\"pm-asset:docs/v.mp4\""));
}

#[test]
fn only_changed_blocks_carry_html_when_ids_are_known() {
    let features = MarkdownFeatures::default();
    let first = parts("# A\n\none\n\ntwo\n", &features).0;
    let known: HashSet<String> = first.iter().map(|b| b.id.clone()).collect();
    let rendered = render_with(
        "# A\n\none\n\nTWO changed\n\nnew\n",
        "docs/a.md",
        &features,
        &known,
    );
    let Body::Markdown { blocks, .. } = rendered.body else {
        panic!()
    };
    let have: Vec<_> = blocks.iter().map(|b| b.html.is_some()).collect();
    assert_eq!(have, [false, false, true, true]);
    assert_eq!(blocks[0].id, first[0].id);
    assert_eq!(blocks[1].id, first[1].id);
}

#[test]
fn ids_survive_edits_elsewhere_and_line_shifts() {
    let features = MarkdownFeatures::default();
    let before = parts("# A\n\nsame\n\nlast\n", &features).0;
    let after = parts("intro\n\n# A\n\nsame\n\nlast\n", &features).0;
    assert_eq!(after.len(), 4);
    for (b, a) in before.iter().zip(&after[1..]) {
        assert_eq!(b.id, a.id);
        assert_ne!(b.line_start, a.line_start);
    }
    // equal blocks get distinct ids
    let repeated = parts("same\n\nsame\n\nsame\n", &features).0;
    let ids: HashSet<_> = repeated.iter().map(|b| b.id.clone()).collect();
    assert_eq!(ids.len(), 3);
}

#[test]
fn footnotes_form_a_block_of_their_own() {
    let (blocks, _, _, _) = parts(
        "Text[^a]\n\n[^a]: The note.\n",
        &MarkdownFeatures::default(),
    );
    assert_eq!(blocks.last().unwrap().kind, "section");
    assert!(blocks
        .last()
        .unwrap()
        .html
        .as_ref()
        .unwrap()
        .contains("footnotes"));
}

// ---- adversarial ---------------------------------------------------------------------------------------------------

#[test]
fn hostile_markdown_and_html_never_survive() {
    let hostile = [
        "<script>alert(1)</script>",
        "[x](javascript:alert(1))",
        "[x](  javascript:alert(1))",
        "[x](JaVaScRiPt:alert(1))",
        "[x](data:text/html;base64,PHNjcmlwdD4=)",
        "![x](javascript:alert(1))",
        "<img src=x onerror=alert(1)>",
        "<a href=\"x\" onclick=\"y\">t</a>",
        "<iframe src=\"//evil\"></iframe>",
        "<style>*{display:none}</style>",
        "<form action=x><input name=a><button>go</button></form>",
        "<svg onload=alert(1)><script>1</script></svg>",
        "<base href=\"http://evil/\">",
        "<link rel=stylesheet href=http://evil/x.css>",
        "<math><mtext><table><mglyph><style><!--</style><img title=\"--&gt;&lt;img src=1 onerror=alert(1)&gt;\">",
        "<details open ontoggle=alert(1)><summary>x</summary></details>",
        "<!--pm:open:x\"onclick=alert(1)-->",
        "<video src=x autoplay onplay=alert(1)></video>",
        "<object data=\"x\"></object>",
    ];
    for md in hostile {
        let html = html_of(md, &all_features()).to_lowercase();
        for bad in [
            "<script",
            "javascript:",
            "onerror",
            "onclick",
            "onload",
            "ontoggle",
            "onplay",
            "<iframe",
            "<style",
            "<form",
            "<base",
            "<link",
            "<object",
            "<svg",
            "autoplay",
            "data:text",
        ] {
            assert!(!html.contains(bad), "{md} -> {html}");
        }
    }
}

#[test]
fn nesting_bombs_fall_back_to_plain_text_without_hanging() {
    let started = std::time::Instant::now();
    for md in [
        "> ".repeat(5000) + "x",
        "- ".repeat(5000) + "x",
        "1. ".repeat(3000) + "x",
        "<div>".repeat(5000) + "x",
        "<b>".repeat(100_000) + "x",
        "[".repeat(100_000),
        "*".repeat(100_000),
        "`".repeat(100_000),
        "![[".repeat(30_000),
    ] {
        let (blocks, _, _, notes) = parts(&md, &all_features());
        assert!(!blocks.is_empty());
        let _ = notes;
    }
    let (blocks, _, _, notes) = parts(&("> ".repeat(5000) + "x"), &all_features());
    assert_eq!(notes, [DEEP_NESTING_NOTE]);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].kind, "pre");
    assert!(
        started.elapsed().as_secs() < 30,
        "too slow: {:?}",
        started.elapsed()
    );
}

#[test]
fn nesting_the_tokenizer_reads_falls_back_even_where_a_naive_scan_sees_none() {
    let started = std::time::Instant::now();
    let n = 5000;
    let distinct = (0..n).fold(String::new(), |mut all, i| {
        let _ = write!(all, "<p><b x={i}>a</p>");
        all
    });
    for md in [
        // a self-closing slash is ignored on an HTML element that is not void
        "<div/>".repeat(n) + "x",
        // a quote opens a value only after `=`
        "<div b'c>".to_owned() + &"<div>".repeat(n),
        // a comment also ends at `--!>`
        "<!-- x --!>".to_owned() + &"<div>".repeat(n),
        // an end tag with nothing open to close closes nothing
        "<div></p>".repeat(n),
        // raw text: a `<!--` in a style is text
        "<div><style><!--</style>".to_owned() + &"<div>".repeat(n),
        // script escapes: the first `</script>` after `<!--<script>` does not end the script
        "<div><script><!--<script></script></div></script>".repeat(n),
        // a bogus comment ends at the first `>`, quotes or not
        "<div><?'>".to_owned() + &"<div>".repeat(n),
        // an element the parser closed by itself is not closed again past what came after
        "<p><ul></p>".repeat(n),
        // formatting elements the parser opens again in each paragraph
        distinct,
    ] {
        let (blocks, _, _, notes) = parts(&md, &all_features());
        assert_eq!(notes, [DEEP_NESTING_NOTE], "{}", &md[..60]);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, "pre");
    }
    assert!(
        started.elapsed().as_secs() < 30,
        "too slow: {:?}",
        started.elapsed()
    );
}

#[test]
fn shallow_svg_and_well_formed_html_still_render() {
    let paths = format!(
        "<svg viewBox=\"0 0 10 10\">\n{}</svg>\n\nAfter the drawing.\n",
        "<path d=\"M0 0h1\"/>\n".repeat(1000)
    );
    let label = format!(
        "<svg><foreignObject><div><span><p>label</p></span></div></foreignObject>\n{}</svg>\n\nAfter.\n",
        "<rect/>".repeat(1000)
    );
    let anchors = "<a name=\"x\"/>\n\n## Heading\n\ntext\n\n".repeat(400);
    let sloppy = [
        // an HTML end tag inside SVG closes the HTML element around it
        "<div><svg><g><circle/></div>\n\nAfter.\n".to_owned(),
        // MathML that holds HTML
        "<math><annotation-xml encoding=\"text/html\"><div><p>x</p></div></annotation-xml><mi>y</mi></math>\n\nAfter.\n"
            .to_owned(),
        "<math><mtext><b>x</b></mtext><mi>y</math>\n".to_owned(),
        // a formatting element left open before a drawing with text
        "<b>bold <svg><text>t</text></svg> after\n\nMore.\n".to_owned(),
        // the parser closes these by itself
        "<p>a<p>b<p>c\n".repeat(400),
        "<ul><li>a<li>b<li>c</ul>\n".repeat(400),
    ];
    for md in [paths, label, anchors].into_iter().chain(sloppy) {
        let (blocks, _, _, notes) = parts(&md, &all_features());
        assert!(notes.is_empty(), "{}", md.get(..60).unwrap_or(&md));
        assert!(blocks.iter().all(|b| b.kind != "pre"));
    }
}

/// One open `div`, then rounds of a formatting element left open across blocks: the parser clones it into later blocks
/// and moves the clones around again on each `</b>` (distinct ids: it keeps all of them), so the tree it builds grows
/// with the square of the rounds while the tags of the input never nest more than a dozen deep.
fn cloned_formatting(rounds: usize) -> String {
    let mut html = String::from("<div>\n");
    for i in 0..rounds {
        let _ = write!(
            html,
            "<div><b id=i{i}></div><div>x{}</b>{}",
            "<div>".repeat(9),
            "</div>".repeat(10)
        );
    }
    html
}

#[test]
fn formatting_elements_cloned_by_the_parser_fall_back_fast() {
    let started = std::time::Instant::now();
    let (blocks, _, _, notes) = parts(&cloned_formatting(400), &all_features());
    assert_eq!(notes, [DEEP_NESTING_NOTE]);
    assert_eq!(blocks.len(), 1);
    assert!(started.elapsed().as_secs() < 2, "{:?}", started.elapsed());
}

/// `count` pieces of `each(i)`, one after the other.
fn run(count: usize, each: impl Fn(usize) -> String) -> String {
    (0..count).map(each).collect()
}

#[test]
fn html_the_sanitizer_would_parse_slowly_falls_back_fast() {
    let mega = 1_000_000;
    let attrs = run(200, |i| format!(" a{i}"));
    for (what, md) in [
        // nodes put before a table one by one: each passes every sibling put there before it
        ("foster", format!("<table>{}", "<br>".repeat(mega / 4))),
        // stray `<html>` tags: each looks at every attribute the root already holds
        ("stray html", run(mega / 13, |i| format!("<html a{i}>"))),
        // one tag with a huge number of attributes: each name is checked against every earlier one
        (
            "attributes",
            format!("<div{}>x</div>", run(mega / 7, |i| format!(" a{i}"))),
        ),
        // a formatting element with many attributes copied into each later paragraph
        (
            "rebuilt",
            format!("<p><b{attrs}></p>{}", "<p>x</p>".repeat(mega / 8)),
        ),
        // each new `<b>` compared with 250 open ones
        (
            "compared",
            format!(
                "{}{}",
                run(250, |i| format!("<b x={i}>")),
                "<b></b>".repeat(mega / 7)
            ),
        ),
    ] {
        let started = std::time::Instant::now();
        let (blocks, _, _, notes) = parts(&md, &all_features());
        let elapsed = started.elapsed();
        assert_eq!(notes, [DEEP_NESTING_NOTE], "{what}");
        assert_eq!(blocks.len(), 1, "{what}");
        // unguarded, these inputs take minutes; a slow debug runner needs about 3 s for the guarded walk
        assert!(elapsed.as_secs() < 10, "{what}: {elapsed:?}");
    }
}

#[test]
fn the_guard_depth_holds_where_the_input_is_cut_into_chunks() {
    // the guard hands the parser its input a chunk at a time: a tag, a character reference and a line end cut in two
    // there are read as the sanitizer reads them whole
    for (head, tail) in [
        ("<di", "v class=a><div><p>x</p></div></div>"),
        ("<div title=\"a&no", "tin;b\"><div><b>y</b></div></div>"),
        ("<div>&no", "tin;<ul><li>z</ul></div>"),
        ("<div>\r", "\n<div><p>w\r\n</div></div>"),
    ] {
        let pad = "a".repeat(nesting::CHUNK - head.len());
        let html = format!("{pad}{head}{tail}");
        assert_eq!(&html[nesting::CHUNK - head.len()..nesting::CHUNK], head);
        let found = nesting::deepest(&html, usize::MAX - 1);
        let made = sanitized_depth(&sanitize::sanitize(&html, "docs/a.md"));
        assert_eq!(found, made, "{head}|{tail}");
        assert!(found >= 2, "{head}|{tail}");
    }
}

#[test]
fn a_piece_cut_inside_raw_text_is_measured_as_the_sanitizer_reads_it() {
    // the cuts between pieces come from a plain tag scan, which sees no raw text: the cut after `</p>` falls inside the
    // text area, and the sanitizer parses the rest as markup on its own
    let md = format!(
        "<p><textarea>{}</p>{}</textarea></p>\n",
        "a".repeat(40_000),
        cloned_formatting(400)
    );
    let started = std::time::Instant::now();
    let (blocks, _, _, notes) = parts(&md, &all_features());
    assert!(started.elapsed().as_secs() < 2, "{:?}", started.elapsed());
    // either the deep tree is caught, or what reached the sanitizer was shallow
    let deepest = blocks
        .iter()
        .map(|b| sanitized_depth(b.html.as_deref().unwrap_or_default()))
        .max()
        .unwrap_or(0);
    assert!(notes == [DEEP_NESTING_NOTE] || deepest <= MAX_TREE_DEPTH);
}

/// The deepest element nesting of sanitized HTML (balanced tags, quoted values, no comments); an element without an end
/// tag is one level below its parent.
fn sanitized_depth(html: &str) -> usize {
    const VOID: [&str; 8] = ["area", "br", "col", "hr", "img", "input", "source", "wbr"];
    let (mut depth, mut deepest, mut quote) = (0_usize, 0_usize, None);
    let mut tag: Option<usize> = None;
    for (at, c) in html.char_indices() {
        match (tag, quote) {
            (None, _) if c == '<' => tag = Some(at),
            (Some(_), Some(q)) if c == q => quote = None,
            (Some(_), None) if c == '"' || c == '\'' => quote = Some(c),
            (Some(start), None) if c == '>' => {
                let inner = &html[start + 1..at];
                let name: String = inner
                    .trim_start_matches('/')
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .collect();
                if inner.starts_with('/') {
                    depth = depth.saturating_sub(1);
                } else if VOID.contains(&name.as_str()) {
                    deepest = deepest.max(depth + 1);
                } else {
                    depth += 1;
                    deepest = deepest.max(depth);
                }
                tag = None;
            }
            _ => {}
        }
    }
    deepest
}

#[test]
fn the_guard_depth_is_the_depth_of_the_sanitized_tree() {
    let distinct = (0..40).fold(String::new(), |mut all, i| {
        let _ = write!(all, "<p><b x={i}>a</p>");
        all
    });
    // sloppy markup the parser mends, all with tags the sanitizer keeps: the tree it builds is the tree it hands on
    for html in [
        cloned_formatting(20),
        distinct,
        "<p><b>a</p>".repeat(50),
        "<div></p>".repeat(30),
        "<p><ul></p>".repeat(30),
        "<span><div></span>".repeat(30),
        "<a href=x>1<a href=y>2<div>3</a>4".repeat(20),
        "<b><i><u>x<p>y</b>z</i>w</u>".repeat(20),
        "<table><tr><td>a<b>b<table><td>c</b></table>".repeat(10),
        "<table><div><table><div>x</div></table></div></table>".repeat(10),
        "<table><b>x<tr><td>y</b></td></tr></table>".repeat(10),
        "<ul><li><b>a<li>b</b><li><div>c</ul>".repeat(20),
        "<dl><dt>a<dd><p>b<dt>c</dl>".repeat(20),
        "<h1><b>a<h2>b</h1>c</b>".repeat(20),
        "<pre><code><em>a</pre>b</em>".repeat(20),
        "<p>x<br><img src=a.png></p>".to_owned(),
    ] {
        let found = nesting::deepest(&html, usize::MAX - 1);
        let made = sanitized_depth(&sanitize::sanitize(&html, "docs/a.md"));
        assert_eq!(found, made, "{}", html.get(..60).unwrap_or(&html));
    }
    // past the limit as the rounds grow
    assert_eq!(
        nesting::deepest(&cloned_formatting(400), MAX_TREE_DEPTH),
        usize::MAX
    );
}

#[test]
fn sample_pages_render_without_the_depth_fallback() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut pages = Vec::new();
    for dir in [
        "../demo",
        "../demo/station",
        "e2e/deep/corpus",
        "e2e/deep/corpus/security",
    ] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                pages.push(path);
            }
        }
    }
    assert!(pages.len() >= 20, "{} pages", pages.len());
    for page in pages {
        let Ok(text) = std::fs::read_to_string(&page) else {
            continue;
        };
        let (blocks, _, _, notes) = parts(&text, &all_features());
        // the guard's answer matches the depth of what the sanitizer made
        let deepest = blocks
            .iter()
            .map(|b| sanitized_depth(b.html.as_deref().unwrap_or_default()))
            .max()
            .unwrap_or(0);
        assert!(notes.is_empty(), "{}", page.display());
        assert!(deepest <= MAX_TREE_DEPTH, "{}", page.display());
    }
}

#[test]
fn an_unclosed_link_to_a_heading_keeps_the_text_after_it() {
    let md = "Read <a href=\"#setup\">the setup\n\nMore text.\n\n## Setup\n\nBody.\n";
    let (blocks, outline, _, _) = parts(md, &MarkdownFeatures::default());
    let html = blocks
        .iter()
        .map(|b| b.html.clone().unwrap_or_default())
        .collect::<String>();
    assert!(html.contains("the setup"), "{html}");
    assert!(html.contains("More text."), "{html}");
    // the parser opens the link again around what follows, the heading included
    assert!(html.contains("<h2 id=\"setup\""), "{html}");
    assert_eq!(outline.len(), 1);
    assert!(!outline[0].block.is_empty());
}

/// The outline as it was first written: the first block holding each heading's line, searched from the start.
fn outline_by_search(
    headings: &[Heading],
    parts: &[blocks::RawBlock],
    ids: &[String],
) -> Vec<String> {
    headings
        .iter()
        .map(|heading| {
            parts
                .iter()
                .position(|p| p.line_start <= heading.line && heading.line <= p.line_end)
                .and_then(|at| ids.get(at))
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

fn heading_at(line: u32) -> Heading {
    Heading {
        label: String::new(),
        level: 2,
        id: String::new(),
        line,
    }
}

fn block_over(line_start: u32, line_end: u32) -> blocks::RawBlock {
    blocks::RawBlock {
        html: String::new(),
        tag: "p".to_owned(),
        line_start,
        line_end,
    }
}

#[test]
fn outline_links_each_heading_to_the_first_block_holding_its_line() {
    // a small fixed generator: blocks in any order, overlapping or not; headings in line order, a few out of it
    let mut state = 0x2545_f491_u32;
    let mut next = |below: u32| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state % below
    };
    for _ in 0..300 {
        let parts: Vec<_> = (0..next(30))
            .map(|_| {
                let start = next(60) + 1;
                block_over(start, start + next(4))
            })
            .collect();
        let ids: Vec<String> = (0..parts.len()).map(|i| format!("b{i}")).collect();
        let mut lines: Vec<u32> = (0..next(20)).map(|_| next(66) + 1).collect();
        lines.sort_unstable();
        if next(4) == 0 && lines.len() > 2 {
            lines.swap(0, 2);
        }
        // footnote headings: moved to the end of the document, they keep their (earlier) lines
        if next(3) == 0 {
            let mut notes: Vec<u32> = (0..next(8)).map(|_| next(66) + 1).collect();
            notes.sort_unstable();
            lines.extend(notes);
        }
        let headings: Vec<Heading> = lines.into_iter().map(heading_at).collect();
        let fast: Vec<String> = outline_of(&headings, &parts, &ids)
            .into_iter()
            .map(|item| item.block)
            .collect();
        assert_eq!(fast, outline_by_search(&headings, &parts, &ids));
    }
}

#[test]
fn outline_of_many_headings_stays_linear() {
    let count = 50_000_u32;
    let parts: Vec<_> = (0..count)
        .map(|i| block_over(2 * i + 1, 2 * i + 1))
        .collect();
    let ids: Vec<String> = (0..parts.len()).map(|i| format!("b{i}")).collect();
    let headings: Vec<Heading> = (0..count).map(|i| heading_at(2 * i + 1)).collect();
    let started = std::time::Instant::now();
    let outline = outline_of(&headings, &parts, &ids);
    let elapsed = started.elapsed();
    assert_eq!(outline.len(), 50_000);
    assert_eq!(outline[49_999].block, "b49999");
    // a loose bound for debug builds: the search from the start took seconds here
    assert!(elapsed.as_millis() < 1000, "{elapsed:?}");
}

#[test]
fn headings_in_footnotes_come_last_and_still_find_their_blocks() {
    let md = "x[^1] y[^2]\n\n# Top\n\n[^1]: # One\n\n[^2]: ## Two\n\n# End\n";
    let (blocks, outline, _, _) = parts(md, &all_features());
    // comrak moves the footnote definitions, headings and all, after the rest of the document
    let lines: Vec<u32> = outline.iter().map(|item| item.line).collect();
    assert_eq!(lines, [3, 9, 5, 7]);
    let footnotes = blocks.iter().find(|b| b.kind == "section").map(|b| &b.id);
    assert_eq!(
        outline[2].block.as_str(),
        footnotes.map_or("", |id| id.as_str())
    );
    // 200 000 of them: each found without a search over every block
    let count = 200_000_u32;
    let parts: Vec<_> = (0..count)
        .map(|i| block_over(2 * i + 1, 2 * i + 1))
        .collect();
    let ids: Vec<String> = (0..parts.len()).map(|i| format!("b{i}")).collect();
    let in_text = (0..count).map(|i| heading_at(2 * i + 1));
    let in_notes = (0..count).map(|i| heading_at(2 * i + 2));
    let headings: Vec<Heading> = in_text.chain(in_notes).collect();
    let started = std::time::Instant::now();
    let outline = outline_of(&headings, &parts, &ids);
    let elapsed = started.elapsed();
    assert_eq!(outline.len(), 400_000);
    assert_eq!(outline[199_999].block, "b199999");
    assert_eq!(outline[399_999].block, "");
    assert!(elapsed.as_millis() < 1000, "{elapsed:?}");
}

#[test]
fn odd_line_endings_nul_bidi_and_empty_input() {
    for md in [
        "",
        "\n\n\n",
        "# A\r\n\r\ntext\r\nmore\r\n",
        "# A\r\n\ntext\rmore\n",
        "a\0b\n\n# \u{0}",
        "\u{202e}rtl \u{2066}x\u{2069}\n",
        "```\nunterminated fence",
        "~~~~\n```\n~~~",
        "| a |\n|--|\n| \\| |\n",
        "\u{feff}# bom",
        "---\n---\n",
        "---\nunclosed\n",
    ] {
        let _ = parts(md, &all_features());
    }
    // CRLF headings keep their lines
    let (_, outline, _, _) = parts("# A\r\n\r\n## B\r\n", &MarkdownFeatures::default());
    assert_eq!(outline.iter().map(|o| o.line).collect::<Vec<_>>(), [1, 3]);
    // empty document: no blocks
    assert!(parts("", &MarkdownFeatures::default()).0.is_empty());
}

#[test]
fn a_large_document_renders_with_unique_stable_ids() {
    let mut md = String::new();
    for i in 0..3000 {
        let _ = write!(md, "## Section {i}\n\nParagraph {i} with `code`, **bold** and a [link](other.md#s{i}).\n\n- a\n- b\n\n");
    }
    let started = std::time::Instant::now();
    let (blocks, outline, _, _) = parts(&md, &all_features());
    assert_eq!(blocks.len(), 9000);
    assert_eq!(outline.len(), 3000);
    let ids: HashSet<_> = blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids.len(), blocks.len());
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
    let again = parts(&md, &all_features()).0;
    assert!(blocks.iter().zip(&again).all(|(a, b)| a.id == b.id));
}

#[test]
fn prompt_markers_in_odd_places_stay_safe() {
    for md in [
        "> <context>\n> x\n> </context>\n",
        "- item\n  <context>\n  x\n  </context>\n",
        "<context>\n<task>\nx\n</context>\n</task>\n",
        "<context>\n```\n</context>\n",
        "<context>\n\n</context>\n<context>\n\n</context>",
        "</context>\n<context>\n",
    ] {
        let html = html_of(md, &all_features());
        assert_eq!(
            html.matches("<details").count(),
            html.matches("</details>").count(),
            "{md} -> {html}"
        );
        assert!(!html.contains("pm:"));
    }
}

/// A typical 1 MB document: headings, prose, lists, tables, code, links, quotes.
fn one_megabyte() -> String {
    let unit = "## Section heading\n\nA paragraph with `inline code`, **bold**, *emphasis* and a [link](other.md#part).\nIt goes on for a few lines so that the parser has real work to do.\n\n- first item\n- second item with [a link](docs/a.md)\n- third item\n\n| name | value |\n|------|-------|\n| a    | 1     |\n| b    | 2     |\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\n> A quote that says something worth reading twice.\n\n1. one\n2. two\n\n";
    unit.repeat(1_000_000 / unit.len() + 1)
}

#[test]
fn one_megabyte_of_markdown_renders_in_time() {
    let md = one_megabyte();
    assert!(md.len() >= 1_000_000);
    let started = std::time::Instant::now();
    let (blocks, outline, _, _) = parts(&md, &all_features());
    let elapsed = started.elapsed();
    println!(
        "1 MB Markdown: {} blocks, {} headings, rendered in {elapsed:?}",
        blocks.len(),
        outline.len()
    );
    // a loose bound for debug builds; the release number is what the spec asks for (< 300 ms for the whole first paint)
    assert!(elapsed.as_secs() < 20, "{elapsed:?}");
    assert!(blocks.len() > 3000);
}

// ---- sections (search) ----------------------------------------------------------------------------------------

fn heads(text: &str) -> Vec<(Vec<String>, u32)> {
    sections(text)
        .into_iter()
        .map(|s| (s.heading_path, s.line))
        .collect()
}

#[test]
fn sections_split_at_every_heading_with_a_breadcrumb() {
    let text = "# A\n\nintro\n\n## B\n\nbody b\n\n### C\n\nbody c\n\n## D\n\nbody d\n";
    let found = sections(text);
    let paths: Vec<Vec<&str>> = found
        .iter()
        .map(|s| s.heading_path.iter().map(String::as_str).collect())
        .collect();
    assert_eq!(
        paths,
        [
            vec!["A"],
            vec!["A", "B"],
            vec!["A", "B", "C"],
            vec!["A", "D"]
        ]
    );
    assert_eq!(
        found.iter().map(|s| s.line).collect::<Vec<_>>(),
        [1, 5, 9, 13]
    );
    // flat: a parent never repeats its children
    assert_eq!(found[0].text, "intro");
    assert_eq!(found[1].text, "body b");
    assert_eq!(found[3].anchor.as_deref(), Some("d"));
}

#[test]
fn sections_have_a_preamble_with_front_matter_values_and_no_anchor() {
    let text = "---\ntags: [alpha, beta]\ntitle: Plan\n---\nBefore any heading.\n\n# First\n\nx\n";
    let found = sections(text);
    assert_eq!(found.len(), 2);
    assert!(found[0].heading_path.is_empty());
    assert_eq!(found[0].anchor, None);
    assert!(found[0].text.contains("alpha"), "{:?}", found[0].text);
    assert!(found[0].text.contains("Before any heading."));
    assert_eq!(found[1].line, 7, "front matter lines keep their numbers");
}

#[test]
fn sections_ignore_headings_in_code_and_keep_empty_ones() {
    let text = "# A\n\n```\n# not a heading\n```\n\n## Empty\n\n## Next\n\ntext\n";
    let found = sections(text);
    assert_eq!(found.len(), 3);
    assert!(
        found[0].text.contains("# not a heading"),
        "code text is searchable"
    );
    assert_eq!(found[1].heading_path, ["A", "Empty"]);
    assert_eq!(found[1].text, "");
}

#[test]
fn sections_anchors_match_the_reader_and_repeat_with_a_suffix() {
    let text = "# Same\n\na\n\n# Same\n\nb\n\n# Other heading!\n";
    let ids: Vec<_> = sections(text).into_iter().map(|s| s.anchor).collect();
    assert_eq!(
        ids,
        [
            Some("same".to_owned()),
            Some("same-1".to_owned()),
            Some("other-heading".to_owned())
        ]
    );
    // the outline of the rendered page gives the same ids and lines
    let rendered = render_with(text, "a.md", &MarkdownFeatures::default(), &HashSet::new());
    let Body::Markdown { outline, .. } = rendered.body else {
        panic!("markdown body");
    };
    let from_outline: Vec<_> = outline.iter().map(|o| (o.id.clone(), o.line)).collect();
    let from_sections: Vec<_> = sections(text)
        .into_iter()
        .map(|s| (s.anchor.unwrap_or_default(), s.line))
        .collect();
    assert_eq!(from_outline, from_sections);
}

#[test]
fn sections_text_has_words_not_syntax() {
    let text = "# T\n\n**bold** and [a link](https://x.org/secret-path) ![alt words](p.png)\n\n| h1 | h2 |\n|----|----|\n| c1 | c2 |\n\n- item one\n- [ ] task two\n\n<context>\ninner words\n</context>\n";
    let body = &sections(text)[0].text;
    for word in [
        "bold",
        "a link",
        "alt words",
        "h1",
        "c2",
        "item one",
        "task two",
        "inner words",
    ] {
        assert!(body.contains(word), "{word} in {body:?}");
    }
    for junk in ["https", "secret-path", "p.png", "**", "<context>", "|---"] {
        assert!(!body.contains(junk), "{junk} not in {body:?}");
    }
}

#[test]
fn sections_setext_and_crlf_and_empty_input() {
    assert_eq!(
        heads("Title\n=====\n\ntext\n"),
        [(vec!["Title".to_owned()], 1)]
    );
    assert_eq!(
        heads("# A\r\n\r\ntext\r\n\r\n## B\r\n"),
        [
            (vec!["A".to_owned()], 1),
            (vec!["A".to_owned(), "B".to_owned()], 5)
        ]
    );
    assert!(sections("").is_empty());
    assert!(sections("  \n\n \n").is_empty());
}

#[test]
fn sections_survive_hostile_input() {
    // nesting bombs fall back to one raw section; nothing panics or hangs
    let bomb = ">".repeat(5000) + " x";
    assert_eq!(sections(&bomb).len(), 1);
    let deep = "- a\n".repeat(10) + "  - b\n";
    assert!(!sections(&deep).is_empty());
    for hostile in [
        "\0\0\0",
        "# \u{202e}rtl\n",
        "#\n#\n#",
        "## \n",
        "```\n# open fence",
    ] {
        let _ = sections(hostile);
    }
    let big = format!("# H\n\n{}\n", "word ".repeat(200_000));
    let found = sections(&big);
    assert_eq!(found.len(), 1);
    assert!(found[0].text.len() > 900_000);
}
