use std::collections::{BTreeSet, HashSet};
use std::fs;

use super::*;
use crate::format::{
    Body, DocEnv, Field, Format, FrontMatterSyntax, MarkdownFeatures, MarkdownFormat, PromptBlocks,
    RenderCx, DEFAULT_PROMPT_TAGS,
};
use crate::paths::{Root, Vault, VaultPath};

const OFF: MarkdownFeatures = MarkdownFeatures {
    prompt_blocks: None,
    front_matter_card: None,
};
const PROMPTS: MarkdownFeatures = MarkdownFeatures {
    prompt_blocks: Some(PromptBlocks {
        tags: DEFAULT_PROMPT_TAGS,
    }),
    front_matter_card: None,
};

fn raws(text: &str) -> Vec<String> {
    parse(text, &OFF)
        .iter()
        .map(|d| d.raw(text).to_owned())
        .collect()
}

fn kinds(text: &str) -> Vec<Kind> {
    parse(text, &OFF).iter().map(|d| d.kind).collect()
}

// ---- parse: spec --------------------------------------------------------------------------------------------------------

#[test]
fn inline_links_and_images_give_their_destination() {
    let text = "See [the guide](docs/guide.md) and ![logo](img/logo.png \"Logo\").\n";
    assert_eq!(raws(text), ["docs/guide.md", "img/logo.png"]);
    assert_eq!(kinds(text), [Kind::Inline, Kind::Image]);
    let d = &parse(text, &OFF)[0];
    assert_eq!(d.line, 1);
    assert_eq!(d.form, Form::Bare);
}

#[test]
fn anchors_queries_and_titles_stay_with_the_destination_token() {
    let text = "[a](x.md#sec \"t\") [b](y.md?q=1#z) [c](<my file.md> 'title') [d](#only)";
    assert_eq!(
        raws(text),
        ["x.md#sec", "y.md?q=1#z", "<my file.md>", "#only"]
    );
    assert_eq!(parse(text, &OFF)[2].form, Form::Angle);
    assert_eq!(parse(text, &OFF)[2].inner(text), "my file.md");
}

#[test]
fn parentheses_and_escapes_inside_a_destination() {
    let text = "[a](a(b)c.md) [b](x\\)y.md) [c](<a(b.md>)";
    assert_eq!(raws(text), ["a(b)c.md", "x\\)y.md", "<a(b.md>"]);
}

#[test]
fn text_that_is_not_a_link_is_left_alone() {
    for text in [
        "just text (not.md) and [bracket] and ]( stray",
        "[ref][label] and [shortcut] without definitions",
        "an <https://example.com/auto> autolink and https://bare.example/x",
        "",
        "\n\n",
        "[]()",
        "[empty]()",
    ] {
        assert!(raws(text).is_empty(), "{text:?} -> {:?}", raws(text));
    }
}

#[test]
fn code_math_and_front_matter_are_never_scanned() {
    let text = "---\nsource: [x](front.md)\n---\n\n`[a](code.md)` and\n\n```\n[b](block.md)\n```\n\n    [c](indented.md)\n\n$[d](math.md)$ real [e](real.md)\n";
    assert_eq!(raws(text), ["real.md"]);
}

#[test]
fn an_unclosed_backtick_run_is_text() {
    let text = "a ` [x](y.md) never closed";
    assert_eq!(raws(text), ["y.md"]);
}

#[test]
fn nested_links_follow_commonmark() {
    // a link cannot contain a link: the inner one wins, the outer is text
    let text = "[outer [inner](in.md) text](out.md)";
    assert_eq!(raws(text), ["in.md"]);
    // an image inside a link is both (in text order of the destinations)
    let text = "[![alt](pic.png)](page.md)";
    assert_eq!(raws(text), ["pic.png", "page.md"]);
}

#[test]
fn reference_definitions_are_found_with_their_titles() {
    let text =
        "[a]: first.md\n[b]:   <my file.md>   \"Title\"\n[c]: third.md 'T'\n[d]: fourth.md (T)\n";
    assert_eq!(
        raws(text),
        ["first.md", "<my file.md>", "third.md", "fourth.md"]
    );
    assert!(kinds(text).iter().all(|k| *k == Kind::Definition));
}

#[test]
fn definitions_in_lists_and_quotes_are_found_and_footnotes_are_not() {
    let text = "> [q]: quoted.md\n\n- [l]: listed.md\n\n1. [n]: numbered.md\n\n[^1]: a footnote, not a link target\n";
    assert_eq!(raws(text), ["quoted.md", "listed.md", "numbered.md"]);
}

#[test]
fn a_definition_cannot_interrupt_a_paragraph_and_junk_after_one_means_it_is_not_one() {
    let text =
        "Some text\n[a]: not-a-definition.md\n\n[b]: real.md\n\n[c]: junk.md trailing words\n";
    assert_eq!(raws(text), ["real.md"]);
}

#[test]
fn definitions_inside_code_are_left_alone() {
    let text = "```\n[a]: fenced.md\n```\n\n    [b]: indented.md\n\n[c]: real.md\n";
    assert_eq!(raws(text), ["real.md"]);
}

#[test]
fn a_definition_right_before_paragraph_text_is_found_with_the_links_of_that_text() {
    // the parser takes the definition off the paragraph but keeps the paragraph's first line as its start
    let text = "[guide]: docs/guide.md\nSee the [guide] and [x](x.md) <a href=\"h.md\">h</a>.\n";
    assert_eq!(
        placed(text, &OFF),
        [("docs/guide.md", 1), ("x.md", 2), ("h.md", 2)]
    );
    assert_eq!(kinds(text), [Kind::Definition, Kind::Inline, Kind::Html]);
}

#[test]
fn several_definitions_right_before_paragraph_text_keep_exact_offsets_with_every_line_ending() {
    for ending in ["\n", "\r\n", "\r"] {
        let text = format!(
            "\u{feff}[a]: a.md{ending}[b]: <b c.md> \"T\"{ending}See [x](x.md) `[c](code.md)`{ending}   more ![ü](ÿ.png){ending}"
        );
        assert_eq!(
            placed(&text, &OFF),
            [("a.md", 1), ("<b c.md>", 2), ("x.md", 3), ("ÿ.png", 4)],
            "{ending:?}"
        );
    }
}

#[test]
fn a_definition_right_before_text_in_a_quote_or_a_list_item_is_found() {
    let text = "> [q]: q.md\n> quote [x](x.md)\nlazy [z](z.md)\n\n- [l]: l.md\n  item [y](y.md)\n";
    assert_eq!(
        placed(text, &OFF),
        [
            ("q.md", 1),
            ("x.md", 2),
            ("z.md", 3),
            ("l.md", 5),
            ("y.md", 6)
        ]
    );
}

#[test]
fn a_later_line_is_text_and_links_after_a_two_line_definition_are_placed() {
    // only the leading definitions leave the paragraph: a later line is never one
    let text = "[a]: a.md\ntext\n[b]: not-a-definition.md\n";
    assert_eq!(placed(text, &OFF), [("a.md", 1)]);
    // a definition over two lines is not read here, but the links after it are still placed exactly
    let text = "[a]:\n  a.md\nthen [x](x.md)\n";
    assert_eq!(placed(text, &OFF), [("x.md", 3)]);
}

#[test]
fn links_in_headings_lists_quotes_and_tables() {
    let text = "# Title [h](head.md)\n\n- item [l](list.md)\n  - nested [n](nested.md)\n\n> quote [q](quote.md)\n\n| a | b |\n|---|---|\n| [t](cell.md) | x |\n";
    assert_eq!(
        raws(text),
        ["head.md", "list.md", "nested.md", "quote.md", "cell.md"]
    );
}

#[test]
fn an_escaped_pipe_does_not_split_a_table_cell_or_shift_the_links_after_it() {
    let text = "| a | b |\n|---|---|\n| [t](cell.md) | x \\| y \\| [e](esc.md) z |\n| 日本 \\| [j](jp.md) | [k](k.md) |\n";
    assert_eq!(raws(text), ["cell.md", "esc.md", "jp.md", "k.md"]);
    let found = parse(text, &OFF);
    for (d, want) in found.iter().zip(["cell.md", "esc.md", "jp.md", "k.md"]) {
        assert_eq!(d.raw(text), want);
    }
}

#[test]
fn html_attributes_are_destinations_and_comments_are_not() {
    let text = "<img src=\"a/b.png\" alt='x'>\n\n<a href='c.md'>t</a> and <video src=d.mp4 poster=\"p.png\"></video>\n\n<!-- <a href=\"hidden.md\"> -->\n\n<div data-href=\"nope.md\" HREF=\"upper.md\"></div>\n";
    assert_eq!(
        raws(text),
        ["a/b.png", "c.md", "d.mp4", "p.png", "upper.md"]
    );
    assert!(kinds(text).iter().all(|k| *k == Kind::Html));
    assert!(parse(text, &OFF).iter().all(|d| d.form == Form::Html));
}

#[test]
fn non_ascii_text_in_html_blocks_and_comments_does_not_panic() {
    // Regression: the HTML scan sliced the `&str` at byte offsets, so a multibyte
    // character inside an HTML block (which runs to the next blank line) aborted the app.
    let text = "中文\n\n<div>\n中文内容\n</div>\n\n<!-- 注释：<a href=\"隐藏.md\"> -->\n\n<img src=\"图片/logo.png\" alt=\"中文\">\n";
    assert_eq!(raws(text), ["图片/logo.png"]);
    assert!(kinds(text).iter().all(|k| *k == Kind::Html));
    assert!(parse(text, &OFF).iter().all(|d| d.form == Form::Html));
}

#[test]
fn offsets_are_exact_with_multibyte_text_a_bom_and_every_line_ending() {
    for ending in ["\n", "\r\n", "\r"] {
        let text = format!(
            "\u{feff}日本語 ação{ending}[ü](ünï/cødé.md){ending}{ending}![i](p.png){ending}{ending}[r]: ref.md{ending}"
        );
        let found = parse(&text, &OFF);
        let raw: Vec<&str> = found.iter().map(|d| d.raw(&text)).collect();
        assert_eq!(raw, ["ünï/cødé.md", "p.png", "ref.md"], "{ending:?}");
        let lines: Vec<usize> = found.iter().map(|d| d.line).collect();
        assert_eq!(lines, [2, 4, 6], "{ending:?}");
    }
}

#[test]
fn a_destination_with_entities_is_taken_as_written() {
    let text = "[a](x&amp;y.md)";
    assert_eq!(raws(text), ["x&amp;y.md"]);
}

#[test]
fn hostile_input_never_panics() {
    let nasty = [
        "[a](".repeat(5000),
        "![".repeat(5000),
        "[x]: ".repeat(2000),
        "<a href=\"".repeat(2000),
        "[".repeat(10000) + &"](x)".repeat(10000),
        "\u{feff}\u{feff}[a](b.md)".to_owned(),
        "[a](<".to_owned() + &"x".repeat(100_000),
        "\0[a](b)\0".to_owned(),
        "[a](b \"unclosed".to_owned(),
        "[\u{202e}a](b.md)".to_owned(),
        "> ".repeat(500) + "[a]: b.md",
        "- ".repeat(300) + "[a](b.md)",
    ];
    for text in nasty {
        for features in [OFF, PROMPTS] {
            for d in parse(&text, &features) {
                assert!(
                    text.get(d.range.clone()).is_some(),
                    "range on char boundaries"
                );
            }
        }
    }
}

// ---- parse: prompt blocks -------------------------------------------------------------------------------------------

/// The bytes each destination's range covers, with its line.
fn placed<'t>(text: &'t str, features: &MarkdownFeatures) -> Vec<(&'t str, usize)> {
    parse(text, features)
        .iter()
        .map(|d| (d.raw(text), d.line))
        .collect()
}

fn no_card(_: FrontMatterSyntax, _: &str) -> Vec<Field> {
    Vec::new()
}

#[test]
fn links_inside_prompt_blocks_are_found_with_the_prompt_features_on() {
    // no blank line after the tags: without the prompt features each tag line opens an HTML block
    let text = "<context>\n[a](a.md) and ![i](i.png)\n</context>\n\n<rules>\n\n- [b](<b c.md>)\n\n</rules>\n<examples>\n<example>\n[ref]: r.md\n\n[r][ref] <a href=\"h.md\">h</a>\n</example>\n</examples>\n";
    assert_eq!(
        placed(text, &PROMPTS),
        [
            ("a.md", 2),
            ("i.png", 2),
            ("<b c.md>", 7),
            ("r.md", 12),
            ("h.md", 14)
        ]
    );
}

#[test]
fn offsets_inside_prompt_blocks_are_exact_with_a_bom_crlf_and_code_left_alone() {
    let text = "\u{feff}<task>\r\n<input>\r\n`[c](code.md)` [x](x.md) ü [y](ÿ.md)\r\n```\r\n[f](fenced.md)\r\n```\r\n</input>\r\n</task>\r\n";
    assert_eq!(placed(text, &PROMPTS), [("x.md", 3), ("ÿ.md", 3)]);
}

#[test]
fn a_definition_right_before_text_inside_a_prompt_block_is_found_with_the_prompt_features_on() {
    let text =
        "<context>\r\n[guide]: docs/guide.md\r\nSee the [guide] and [x](x.md).\r\n</context>\r\n";
    assert_eq!(placed(text, &PROMPTS), [("docs/guide.md", 2), ("x.md", 3)]);
}

#[test]
fn without_the_prompt_features_a_tag_line_opens_an_html_block_as_before() {
    let text = "<context>\n[a](a.md)\n</context>\n\n<context>\n\n[b](b.md)\n\n</context>\n";
    assert_eq!(placed(text, &OFF), [("b.md", 7)]);
    assert_eq!(placed(text, &PROMPTS), [("a.md", 2), ("b.md", 7)]);
}

#[test]
fn front_matter_holds_no_destination_with_the_prompt_features_on() {
    let text = "---\r\n[d]: fm.md\r\nsee: \"[y](y.md)\"\r\n---\r\n<context>\r\n[z](z.md)\r\n</context>\r\n";
    let card = MarkdownFeatures {
        front_matter_card: Some(no_card),
        ..PROMPTS
    };
    assert_eq!(placed(text, &OFF), []);
    assert_eq!(placed(text, &PROMPTS), [("z.md", 6)]);
    assert_eq!(placed(text, &card), [("z.md", 6)]);
}

/// Reading asks for no file: no `@path` reference becomes a link.
struct NoFiles;

impl DocEnv for NoFiles {
    fn file_exists(&self, _: &str) -> bool {
        false
    }
}

/// The local destinations reading renders as `<a href>` (footnote marks left out), decoded, each once.
fn rendered_hrefs(text: &str, features: &MarkdownFeatures) -> BTreeSet<String> {
    let known = HashSet::new();
    let rendered = MarkdownFormat.render(
        text.strip_prefix('\u{feff}').unwrap_or(text),
        &RenderCx {
            path: "doc.md",
            env: &NoFiles,
            features,
            known: &known,
        },
    );
    let Body::Markdown { blocks, .. } = rendered.body else {
        panic!("not markdown");
    };
    let mut out = BTreeSet::new();
    for block in blocks {
        let html = block.html.unwrap_or_default();
        for rest in html.split("<a ").skip(1) {
            let tag = &rest[..rest.find('>').unwrap_or(rest.len())];
            let Some((_, value)) = tag.split_once("href=\"") else {
                continue;
            };
            let href = percent_decode(&value[..value.find('"').unwrap()].replace("&amp;", "&"));
            if !tag.contains("data-footnote") && scheme_of(&href).is_none() {
                out.insert(href);
            }
        }
    }
    out
}

/// The local destinations `parse` finds that reading makes `<a href>` of, each once.
fn parsed_hrefs(text: &str, features: &MarkdownFeatures) -> BTreeSet<String> {
    parse(text, features)
        .iter()
        .filter(|d| match d.kind {
            Kind::Inline | Kind::Definition => true,
            Kind::Html => text[..d.range.start].ends_with("<a href=\""),
            Kind::Image => false,
        })
        .map(|d| d.inner(text).to_owned())
        .filter(|href| scheme_of(href).is_none())
        .collect()
}

#[test]
fn every_link_reading_renders_is_one_parse_finds() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/api/fixtures");
    let mut docs: Vec<(String, String)> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "md"))
        .map(|path| {
            (
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            )
        })
        .collect();
    assert!(docs.len() >= 3, "fixtures at {}", dir.display());
    for text in [
        "<context>\n[a](a.md) and ![i](i.png)\n</context>\n\n<rules>\n\n- [b](<b c.md>)\n\n</rules>\n",
        "<examples>\n<example>\n[ref]: r.md\n\n[r][ref] <a href=\"h.md\">h</a> `[c](code.md)`\n</example>\n</examples>\n",
        "\u{feff}---\r\nsee: \"[y](y.md)\"\r\n---\r\n<task>\r\n[x](x.md)\r\n```\r\n[f](fenced.md)\r\n```\r\n</task>\r\n",
        "<context>\r[lone](lone.md)\r</context>\r",
        "<context>\n[open](open.md)\n",
        "[guide]: docs/guide.md\nSee the [guide] and [x](x.md).\n",
        "> [a]: a.md\n> [b]: b.md\n> quote [a] [x](x.md) [b]\nlazy [z](z.md)\n\n- [l]: l.md\n  item [l][] [y](y.md)\n",
        "<context>\r\n[guide]: docs/guide.md\r\nSee the [guide] and [x](x.md).\r\n</context>\r\n",
    ] {
        docs.push((format!("{text:?}"), text.to_owned()));
    }
    let card = MarkdownFeatures {
        front_matter_card: Some(no_card),
        ..PROMPTS
    };
    for (name, text) in &docs {
        for features in [OFF, PROMPTS, card] {
            assert_eq!(
                parsed_hrefs(text, &features),
                rendered_hrefs(text, &features),
                "{name} (prompt features on: {})",
                features.prompt_blocks.is_some()
            );
        }
    }
}

// ---- resolve ------------------------------------------------------------------------------------------------------------

fn vault_with(files: &[&str]) -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    for f in files {
        let path = dir.path().join(f);
        if f.ends_with('/') {
            fs::create_dir_all(&path).unwrap();
        } else {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
    }
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    (dir, vault)
}

fn vp(path: &str) -> VaultPath {
    VaultPath::parse(path).unwrap()
}

#[test]
fn resolve_finds_documents_assets_folders_and_missing_targets() {
    let (_dir, vault) = vault_with(&[
        "a.md",
        "docs/guide.md",
        "docs/img/p.png",
        "My Notes.md",
        "folder/README.md",
        "plain/",
        "idx/index.md",
    ]);
    let from = vp("docs/guide.md");
    let t = |href: &str| resolve(&vault, &from, href);
    assert_eq!(t("../a.md"), Target::Doc(vp("a.md"), None));
    assert_eq!(
        t("../a"),
        Target::Doc(vp("a.md"), None),
        "missing extension"
    );
    assert_eq!(
        t("../a.md#sec%20one"),
        Target::Doc(vp("a.md"), Some("sec one".to_owned()))
    );
    assert_eq!(t("img/p.png"), Target::Asset(vp("docs/img/p.png")));
    assert_eq!(t("../My%20Notes.md"), Target::Doc(vp("My Notes.md"), None));
    assert_eq!(
        t("../My Notes.md"),
        Target::Doc(vp("My Notes.md"), None),
        "raw space"
    );
    assert_eq!(t("..\\a.md"), Target::Doc(vp("a.md"), None), "backslash");
    assert_eq!(t("/a.md"), Target::Doc(vp("a.md"), None), "root-relative");
    assert_eq!(t("../folder"), Target::Doc(vp("folder/README.md"), None));
    assert_eq!(t("../idx/"), Target::Doc(vp("idx/index.md"), None));
    assert_eq!(t("../plain"), Target::Folder(vp("plain")));
    assert_eq!(
        t("../a.md?x=1"),
        Target::Doc(vp("a.md"), None),
        "query dropped"
    );
    assert_eq!(t("#intro"), Target::Anchor("intro".to_owned()));
    assert_eq!(t(""), Target::Anchor(String::new()));
    assert_eq!(t("nope.md"), Target::Missing("docs/nope.md".to_owned()));
}

#[test]
fn resolve_reads_schemes_and_blocks_the_dangerous_ones() {
    let (_dir, vault) = vault_with(&["a.md"]);
    let from = vp("a.md");
    assert_eq!(
        resolve(&vault, &from, "https://example.com/x"),
        Target::Web("https://example.com/x".to_owned())
    );
    assert_eq!(
        resolve(&vault, &from, "mailto:me@x.org"),
        Target::Web("mailto:me@x.org".to_owned())
    );
    for bad in [
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        "data:text/html;base64,AAAA",
        "vbscript:x",
    ] {
        assert!(
            matches!(resolve(&vault, &from, bad), Target::Missing(_)),
            "{bad}"
        );
    }
}

#[test]
fn resolve_never_leaves_the_project() {
    let (_dir, vault) = vault_with(&["a.md", "docs/b.md"]);
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.md"), "x").unwrap();
    let from = vp("docs/b.md");
    assert!(matches!(
        resolve(&vault, &from, "../../outside.md"),
        Target::Missing(_)
    ));
    assert!(matches!(
        resolve(&vault, &from, "../../../x.md"),
        Target::Missing(_)
    ));
    let abs = outside.path().join("secret.md");
    let plain = resolve(&vault, &from, &abs.display().to_string());
    if cfg!(windows) {
        assert_eq!(plain, Target::Outside);
    } else {
        // `/x/y.md` is a path from the project's root on Unix: looked up inside, never outside
        assert!(matches!(plain, Target::Missing(_)), "{plain:?}");
    }
    let url = format!("file:///{}", abs.display().to_string().replace('\\', "/"));
    assert_eq!(resolve(&vault, &from, &url), Target::Outside);
}

#[test]
fn absolute_paths_inside_the_project_resolve_to_its_files() {
    let (_dir, vault) = vault_with(&["a.md", "docs/b.md"]);
    let from = vp("docs/b.md");
    let abs = vault.main().path().join("a.md");
    assert_eq!(
        resolve(&vault, &from, &abs.display().to_string()),
        Target::Doc(vp("a.md"), None)
    );
    let url = format!("file:///{}", abs.display().to_string().replace('\\', "/"));
    assert_eq!(resolve(&vault, &from, &url), Target::Doc(vp("a.md"), None));
}

#[test]
fn join_relative_stays_inside_one_root() {
    assert_eq!(
        join_relative("docs/a.md", "../b.md").as_deref(),
        Some("b.md")
    );
    assert_eq!(
        join_relative("docs/a.md", "./x/../y.md").as_deref(),
        Some("docs/y.md")
    );
    assert_eq!(join_relative("docs/a.md", "../../b.md"), None);
    assert_eq!(
        join_relative("docs/a.md", "/top.md").as_deref(),
        Some("top.md")
    );
    assert_eq!(
        join_relative("@wt/w/docs/a.md", "../b.md").as_deref(),
        Some("@wt/w/b.md")
    );
    assert_eq!(
        join_relative("@wt/w/a.md", "../b.md"),
        None,
        "never climbs out of its worktree"
    );
}

#[test]
fn percent_decode_and_scheme_basics() {
    assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
    assert_eq!(percent_decode("100%"), "100%");
    assert_eq!(percent_decode("%zz%4"), "%zz%4");
    assert_eq!(percent_decode("%E6%97%A5"), "日");
    assert_eq!(scheme_of("https://x").as_deref(), Some("https"));
    assert_eq!(scheme_of("C:\\x").as_deref(), Some("c"));
    assert_eq!(scheme_of("a b:c"), None);
    assert_eq!(scheme_of("./x:y"), None);
}

#[test]
fn local_target_names_a_project_path_without_the_disk() {
    let t = |from: &str, href: &str| local_target(from, href);
    assert_eq!(
        t("docs/a.md", "img/p.png").as_deref(),
        Some("docs/img/p.png")
    );
    assert_eq!(t("docs/a.md", "../img/p.png").as_deref(), Some("img/p.png"));
    assert_eq!(t("docs/a.md", "/img/p.png").as_deref(), Some("img/p.png"));
    assert_eq!(t("docs/a.md", "b.md#sec").as_deref(), Some("docs/b.md"));
    assert_eq!(t("docs/a.md", "b.md?x=1").as_deref(), Some("docs/b.md"));
    assert_eq!(t("a.md", "my%20pic.png").as_deref(), Some("my pic.png"));
    assert_eq!(
        t("docs/a.md", "..\\img\\p.png").as_deref(),
        Some("img/p.png")
    );
    assert_eq!(t("docs/a.md", "  ./p.png  ").as_deref(), Some("docs/p.png"));
    assert_eq!(t("@wt/w/a.md", "x/p.png").as_deref(), Some("@wt/w/x/p.png"));
}

#[test]
fn local_target_ignores_what_is_not_a_project_path() {
    for href in [
        "",
        "#sec",
        "https://x.org/a.png",
        "mailto:a@b.c",
        "javascript:alert(1)",
        "data:image/png;base64,AAAA",
        "file:///etc/passwd",
        "C:\\x\\y.png",
        "?only=query",
    ] {
        assert_eq!(local_target("docs/a.md", href), None, "{href}");
    }
    assert_eq!(local_target("a.md", "../../up.png"), None, "climbs out");
}
