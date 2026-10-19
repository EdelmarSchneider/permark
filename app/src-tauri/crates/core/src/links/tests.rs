use std::fs;

use super::*;
use crate::paths::{Root, Vault, VaultPath};

fn raws(text: &str) -> Vec<String> {
    parse(text).iter().map(|d| d.raw(text).to_owned()).collect()
}

fn kinds(text: &str) -> Vec<Kind> {
    parse(text).iter().map(|d| d.kind).collect()
}

// ---- parse: spec --------------------------------------------------------------------------------------------------------

#[test]
fn inline_links_and_images_give_their_destination() {
    let text = "See [the guide](docs/guide.md) and ![logo](img/logo.png \"Logo\").\n";
    assert_eq!(raws(text), ["docs/guide.md", "img/logo.png"]);
    assert_eq!(kinds(text), [Kind::Inline, Kind::Image]);
    let d = &parse(text)[0];
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
    assert_eq!(parse(text)[2].form, Form::Angle);
    assert_eq!(parse(text)[2].inner(text), "my file.md");
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
    let found = parse(text);
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
    assert!(parse(text).iter().all(|d| d.form == Form::Html));
}

#[test]
fn offsets_are_exact_with_multibyte_text_a_bom_and_every_line_ending() {
    for ending in ["\n", "\r\n", "\r"] {
        let text = format!(
            "\u{feff}日本語 ação{ending}[ü](ünï/cødé.md){ending}{ending}![i](p.png){ending}{ending}[r]: ref.md{ending}"
        );
        let found = parse(&text);
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
        for d in parse(&text) {
            assert!(
                text.get(d.range.clone()).is_some(),
                "range on char boundaries"
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
    assert_eq!(
        resolve(&vault, &from, &abs.display().to_string()),
        Target::Outside
    );
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
