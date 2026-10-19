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

// ---- spec ---------------------------------------------------------------------------------------------------------

#[test]
fn gfm_features_render() {
    let md = "# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n~~strike~~ https://example.com\n\nText[^1]\n\n[^1]: Note.\n\n> [!WARNING]\n> careful\n\nterm\n: definition\n\n$a^2$ and\n\n$$\nx\n$$\n\n```rust\nfn main() {}\n```\n";
    let html = plain_html(md);
    for needle in [
        "<h1",
        "<table",
        "<th>a</th>",
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
            && html.contains("<li>item</li>")
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
