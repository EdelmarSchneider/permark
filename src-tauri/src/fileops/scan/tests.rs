use std::collections::BTreeSet;

use super::*;

/// The destinations of `text` as `(kind, raw token)`.
fn found(text: &str) -> Vec<(Kind, String)> {
    find_destinations(text)
        .into_iter()
        .map(|d| (d.kind, d.raw(text).to_owned()))
        .collect()
}

/// Only the raw tokens.
fn raws(text: &str) -> Vec<String> {
    found(text).into_iter().map(|(_, raw)| raw).collect()
}

fn inline(raw: &str) -> (Kind, String) {
    (Kind::Inline, raw.to_owned())
}

#[test]
fn inline_links_and_images_give_their_destination() {
    let text =
        "See [guide](docs/guide.md) and ![logo](img/logo.png \"Logo\") and [x](<my file.md>).\n";
    assert_eq!(
        found(text),
        [
            inline("docs/guide.md"),
            inline("img/logo.png"),
            inline("<my file.md>")
        ]
    );
    let dests = find_destinations(text);
    assert_eq!(dests[2].form, Form::Angle);
    assert_eq!(dests[2].inner(text), "my file.md");
    assert_eq!(dests[0].form, Form::Bare);
}

#[test]
fn anchors_queries_and_titles_stay_with_the_destination_token() {
    assert_eq!(
        raws("[a](docs/guide.md#setup) [b](guide.md?plain=1#x) [c](#local) [d](a.md 'Title') [e](a.md (Title))"),
        ["docs/guide.md#setup", "guide.md?plain=1#x", "#local", "a.md", "a.md"]
    );
}

#[test]
fn parentheses_and_escapes_inside_a_destination() {
    assert_eq!(
        raws("[a](wiki/Foo_(bar).md) [b](a\\)b.md) [c](x.md)"),
        ["wiki/Foo_(bar).md", "a\\)b.md", "x.md"]
    );
    assert_eq!(unescape("a\\)b.md"), "a)b.md");
    assert_eq!(unescape("docs\\guide.md"), "docs\\guide.md");
    assert_eq!(unescape("docs\\_x.md"), "docs_x.md");
}

#[test]
fn text_that_is_not_a_link_is_left_alone() {
    // a space inside a bare destination, an unclosed link, an escaped bracket, a bracket without a paren
    assert_eq!(
        raws("[a](my file.md) [b](x.md [c] \\[d](e.md) [f]"),
        Vec::<String>::new()
    );
    assert_eq!(raws("[a]() [b]( )"), Vec::<String>::new());
}

#[test]
fn code_spans_and_blocks_are_never_scanned() {
    let text = [
        "Use `[a](one.md)` or ``[b](two.md)`` here, but [c](three.md).",
        "",
        "```md",
        "[d](four.md)",
        "```",
        "",
        "~~~",
        "[e](five.md)",
        "~~~",
        "",
        "    [f](six.md)",
        "",
        "- item",
        "",
        "      [g](seven.md)",
        "",
        "Then [h](eight.md).",
        "",
    ]
    .join("\n");
    assert_eq!(raws(&text), ["three.md", "eight.md"]);
}

#[test]
fn front_matter_is_not_scanned() {
    let text = "---\ntitle: [x](front.md)\n---\n\n[y](body.md)\n";
    assert_eq!(raws(text), ["body.md"]);
}

#[test]
fn an_unclosed_backtick_run_is_text() {
    assert_eq!(raws("a ` [b](x.md) c"), ["x.md"]);
    // a longer run does not close a shorter one
    assert_eq!(raws("`` a ` [b](x.md) ``  [c](y.md)"), ["y.md"]);
}

#[test]
fn nested_links_follow_the_commonmark_rules() {
    // a link cannot contain a link: the inner one wins
    assert_eq!(raws("[a [b](inner.md) c](outer.md)"), ["inner.md"]);
    // an image can hold a link, and the image is itself a link target
    assert_eq!(raws("[![alt](img.png)](page.md)"), ["img.png", "page.md"]);
    assert_eq!(
        raws("![alt [b](inner.md)](img.png)"),
        ["inner.md", "img.png"]
    );
}

#[test]
fn reference_definitions_are_found_with_their_titles() {
    let text = "[guide]: docs/guide.md\n[logo]: <img/my logo.png> \"Logo\"\n[ext]: https://example.com 'x'\n\nText [guide] and [the logo][logo].\n";
    let got = found(text);
    assert_eq!(
        got,
        [
            (Kind::Definition, "docs/guide.md".to_owned()),
            (Kind::Definition, "<img/my logo.png>".to_owned()),
            (Kind::Definition, "https://example.com".to_owned()),
        ]
    );
}

#[test]
fn definitions_in_lists_and_quotes_are_found_and_footnotes_are_not() {
    let text = "- [a]: one.md\n- item\n\n> [b]: two.md\n> quote\n\n[^1]: a footnote with [link](three.md)\n\nBody[^1].\n";
    assert_eq!(
        found(text),
        [
            (Kind::Definition, "one.md".to_owned()),
            (Kind::Definition, "two.md".to_owned()),
            inline("three.md"),
        ]
    );
}

#[test]
fn a_definition_cannot_interrupt_a_paragraph() {
    assert_eq!(
        raws("text\n[a]: not-a-definition.md\n"),
        Vec::<String>::new()
    );
    // but it can follow another definition at the top of a paragraph
    assert_eq!(
        raws("[a]: one.md\n[b]: two.md\nThen text [a] and [c](three.md)\n"),
        ["one.md", "two.md", "three.md"]
    );
}

#[test]
fn junk_after_a_definition_means_it_is_not_one() {
    assert_eq!(raws("[a]: one.md extra words\n"), Vec::<String>::new());
    assert_eq!(raws("[a]: one.md \"title\" extra\n"), Vec::<String>::new());
}

#[test]
fn definitions_inside_code_are_left_alone() {
    assert_eq!(raws("```\n[a]: one.md\n```\n"), Vec::<String>::new());
}

#[test]
fn a_shortcut_reference_inside_link_text_stops_the_outer_link_only_when_defined() {
    // [foo] is defined, so it forms a link and the outer one cannot be a link
    assert_eq!(
        raws("[foo]: def.md\n\n[see [foo] here](outer.md)\n"),
        ["def.md"]
    );
    // not defined: [foo] is text, the outer link is a link
    assert_eq!(raws("[see [foo] here](outer.md)\n"), ["outer.md"]);
}

#[test]
fn links_in_headings_lists_quotes_and_tables() {
    let text =
        "# Title [a](h.md)\n\n- item [b](l.md)\n  1. nested [c](n.md)\n\n> quote [d](q.md)\n\n\
                | col | other |\n|-----|-------|\n| [e](t1.md) | `[x](no.md)` and [f](t2.md) |\n";
    assert_eq!(
        raws(text),
        ["h.md", "l.md", "n.md", "q.md", "t1.md", "t2.md"]
    );
}

#[test]
fn an_escaped_pipe_does_not_split_a_table_cell() {
    let text = "| a | b |\n|---|---|\n| [x](p\\|q.md) | y |\n";
    assert_eq!(raws(text), ["p\\|q.md"]);
}

#[test]
fn raw_html_attributes_are_found_and_the_rest_of_the_tag_is_skipped() {
    let text = "Inline <img src=\"img/a.png\" width=\"20\"> and <a href='docs/b.md'>b</a> and <a href=plain.md>c</a>.\n\n\
                <div>\n  <img src=\"img/c.png\">\n  <!-- <a href=\"hidden.md\"> -->\n</div>\n";
    assert_eq!(
        found(text),
        [
            (Kind::Html, "img/a.png".to_owned()),
            (Kind::Html, "docs/b.md".to_owned()),
            (Kind::Html, "plain.md".to_owned()),
            (Kind::Html, "img/c.png".to_owned()),
        ]
    );
    let dests = find_destinations(text);
    assert!(dests.iter().all(|d| d.form == Form::Html));
}

#[test]
fn a_tag_swallows_a_markdown_link_that_sits_inside_an_attribute() {
    assert_eq!(
        raws("<span title=\"[a](x.md)\">t</span> [b](y.md)"),
        ["y.md"]
    );
}

#[test]
fn autolinks_are_not_destinations() {
    assert_eq!(
        raws("<https://example.com/a](b)> <me@example.com> [c](d.md)"),
        ["d.md"]
    );
}

#[test]
fn offsets_are_exact_in_text_with_multibyte_characters_a_bom_and_crlf() {
    let text = "\u{FEFF}# Olá, mundo\r\n\r\nVeja [guia](docs/guía.md#ação) e ![ícone](img/ícone.png).\r\n\r\n[ref]: <café/menu.md>\r\n";
    let dests = find_destinations(text);
    assert_eq!(
        dests.iter().map(|d| d.raw(text)).collect::<Vec<_>>(),
        ["docs/guía.md#ação", "img/ícone.png", "<café/menu.md>"]
    );
    for d in &dests {
        assert!(text.is_char_boundary(d.start) && text.is_char_boundary(d.end));
    }
}

#[test]
fn a_lone_cr_line_ending_is_a_line_ending() {
    let text = "# T\r\r[a](x.md)\r\r```\r[b](no.md)\r```\r";
    assert_eq!(raws(text), ["x.md"]);
}

#[test]
fn a_title_may_start_on_the_next_line() {
    assert_eq!(raws("[a](x.md\n  \"A title\")\n"), ["x.md"]);
    assert_eq!(raws("[a](\n  x.md\n)\n"), ["x.md"]);
}

#[test]
fn a_blank_text_and_a_text_without_links_have_no_destinations() {
    assert!(find_destinations("").is_empty(), "an empty text");
    assert!(
        find_destinations(
            "

"
        )
        .is_empty(),
        "a blank text"
    );
    assert!(
        find_destinations(
            "Just words, a [bracket] and (parens).
"
        )
        .is_empty(),
        "no links"
    );
}

#[test]
fn hostile_input_does_not_panic() {
    for text in [
        "[",
        "]",
        "[](",
        "[a](<",
        "[a](<b",
        "![",
        "<",
        "<a",
        "<a href=",
        "<a href=\"",
        "`",
        "[a]: ",
        "[a]:<",
        "\\",
        "[a][",
        "| a |\n|---|\n| [x](",
        "<img src=\"a\" src=\"b\"",
        "[\u{00e9}](\u{00e9}",
    ] {
        let _ = find_destinations(text);
    }
}

/// The destinations the scanner finds must be the ones comrak sees, for real documents: every link and
/// image of the project's own documents (a frozen copy, see `edit::corpus`) with a path as destination.
/// Reference-style links use a definition several times (or never), and raw HTML is not parsed by comrak,
/// so the comparison is between sets: nothing comrak links to is missed, and every inline destination
/// found is a link comrak sees too.
#[test]
fn the_scanner_agrees_with_comrak_on_real_documents() {
    use comrak::nodes::NodeValue;

    let mut docs = frozen_docs();
    assert!(docs.len() >= 4, "the frozen documents are missing");
    // the hostile documents of the editor's round-trip corpus
    let inline = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("src/lib/editor/corpus/inline.json");
    if let Ok(json) = std::fs::read_to_string(inline) {
        let map: std::collections::BTreeMap<String, String> =
            serde_json::from_str(&json).expect("inline.json is a map from names to texts");
        assert!(map.len() > 20, "the hostile documents are missing");
        docs.extend(map);
    }
    for (name, text) in docs {
        let arena = comrak::Arena::new();
        let opts = crate::render::markdown::options();
        let root = comrak::parse_document(&arena, &normalize_newlines(&text), &opts);
        let comrak: BTreeSet<String> = root
            .descendants()
            .filter_map(|node| match &node.data.borrow().value {
                NodeValue::Link(l) | NodeValue::Image(l) => Some(l.url.clone()),
                _ => None,
            })
            .filter(|url| is_path(url))
            .collect();
        let found = find_destinations(&text);
        let urls = |wanted: &dyn Fn(Kind) -> bool| -> BTreeSet<String> {
            found
                .iter()
                .filter(|d| wanted(d.kind))
                .map(|d| unescape(d.inner(&text)).into_owned())
                .filter(|url| is_path(url))
                .collect()
        };
        let inline = urls(&|k| k == Kind::Inline);
        let everything = urls(&|k| k != Kind::Html);
        let invented: Vec<_> = inline.difference(&comrak).collect();
        let missed: Vec<_> = comrak.difference(&everything).collect();
        assert!(
            invented.is_empty(),
            "{name}: not links to comrak: {invented:?}"
        );
        assert!(
            missed.is_empty(),
            "{name}: links the scanner missed: {missed:?}"
        );
    }
}

/// A destination that points inside the project: no scheme, not a bare fragment.
fn is_path(url: &str) -> bool {
    !url.is_empty() && !url.starts_with('#') && !url.contains("://") && !url.starts_with("mailto:")
}

fn frozen_docs() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "md") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((path.display().to_string(), text));
                }
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("src/lib/editor/corpus/docs");
    let mut out = Vec::new();
    walk(&root, &mut out);
    out
}
