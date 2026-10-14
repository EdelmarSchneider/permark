use std::cell::RefCell;
use std::collections::BTreeMap;

use super::*;

/// A project in memory.
struct MemWorld {
    files: BTreeMap<String, Vec<u8>>,
    reads: RefCell<Vec<String>>,
}

impl MemWorld {
    fn new(files: &[(&str, &str)]) -> Self {
        Self {
            files: files
                .iter()
                .map(|(p, t)| ((*p).to_owned(), t.as_bytes().to_vec()))
                .collect(),
            reads: RefCell::new(Vec::new()),
        }
    }

    fn with_bytes(mut self, path: &str, bytes: &[u8]) -> Self {
        self.files.insert(path.to_owned(), bytes.to_vec());
        self
    }
}

impl World for MemWorld {
    fn documents(&self) -> Vec<String> {
        self.files
            .keys()
            .filter(|p| {
                std::path::Path::new(p)
                    .extension()
                    .is_some_and(|e| e == "md")
            })
            .cloned()
            .collect()
    }

    fn read(&self, rel: &str) -> io::Result<Vec<u8>> {
        self.reads.borrow_mut().push(rel.to_owned());
        self.files
            .get(rel)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn kind(&self, rel: &str) -> Option<EntryKind> {
        let k = key(rel);
        if self.files.keys().any(|f| key(f) == k) {
            return Some(EntryKind::File);
        }
        let dir = format!("{k}/");
        self.files
            .keys()
            .any(|f| key(f).starts_with(&dir))
            .then_some(EntryKind::Dir)
    }
}

fn moves(list: &[(&str, &str)]) -> Vec<Move> {
    list.iter()
        .map(|(from, to)| Move {
            from: (*from).to_owned(),
            to: (*to).to_owned(),
        })
        .collect()
}

fn plan_of(world: &MemWorld, list: &[(&str, &str)]) -> LinkPlan {
    let moves = moves(list);
    Planner::new(world, &moves).plan()
}

/// The text of `doc` after the plan for `list` is applied to it (the text itself when nothing changes).
fn after(files: &[(&str, &str)], list: &[(&str, &str)], doc: &str) -> String {
    let world = MemWorld::new(files);
    let plan = plan_of(&world, list);
    let text = std::str::from_utf8(&world.files[doc])
        .expect("utf8")
        .to_owned();
    match plan.docs.iter().find(|d| d.path == doc) {
        Some(d) => apply_edits(&text, &d.edits),
        None => text,
    }
}

#[test]
fn a_link_to_a_renamed_file_is_rewritten_and_the_rest_of_the_link_stays() {
    let files = [
        (
            "README.md",
            "See [the guide](docs/guide.md#setup \"Setup\") now.\n",
        ),
        ("docs/guide.md", "# Guide\n"),
    ];
    assert_eq!(
        after(&files, &[("docs/guide.md", "docs/manual.md")], "README.md"),
        "See [the guide](docs/manual.md#setup \"Setup\") now.\n"
    );
}

#[test]
fn links_in_documents_that_do_not_mention_the_file_are_not_even_read() {
    let world = MemWorld::new(&[
        ("a.md", "[x](docs/guide.md)\n"),
        ("b.md", "nothing here\n"),
        ("docs/guide.md", "# G\n"),
    ]);
    let plan = plan_of(&world, &[("docs/guide.md", "docs/manual.md")]);
    assert_eq!(plan.docs.len(), 1);
    assert!(world.reads.borrow().contains(&"a.md".to_owned()));
    // b.md is read to look at it (the plan cannot know), but a document that stays put and
    // does not contain the name produces nothing
    assert!(plan.docs.iter().all(|d| d.path != "b.md"));
}

#[test]
fn query_anchor_and_title_are_kept() {
    let files = [
        (
            "a.md",
            "[x](docs/g.md?plain=1#intro 'T') ![i](img/p.png \"Pic\")\n",
        ),
        ("docs/g.md", "x\n"),
        ("img/p.png", "x"),
    ];
    assert_eq!(
        after(
            &files,
            &[
                ("docs/g.md", "guides/g.md"),
                ("img/p.png", "assets/pic.png")
            ],
            "a.md"
        ),
        "[x](guides/g.md?plain=1#intro 'T') ![i](assets/pic.png \"Pic\")\n"
    );
}

#[test]
fn relative_paths_are_computed_from_each_document() {
    let files = [
        (
            "a/b/c.md",
            "[up](../../docs/x.md) [same](../../docs/x.md#s)\n",
        ),
        ("docs/x.md", "x\n"),
        ("docs/sub/y.md", "[near](../x.md) [down](./sub/y.md)\n"),
    ];
    let m = [("docs/x.md", "archive/2024/x.md")];
    assert_eq!(
        after(&files, &m, "a/b/c.md"),
        "[up](../../archive/2024/x.md) [same](../../archive/2024/x.md#s)\n"
    );
    assert_eq!(
        after(&files, &m, "docs/sub/y.md"),
        "[near](../../archive/2024/x.md) [down](./sub/y.md)\n"
    );
}

#[test]
fn a_dot_slash_prefix_is_kept_when_the_path_does_not_climb() {
    let files = [("docs/a.md", "[b](./b.md)\n"), ("docs/b.md", "x\n")];
    assert_eq!(
        after(&files, &[("docs/b.md", "docs/c.md")], "docs/a.md"),
        "[b](./c.md)\n"
    );
    assert_eq!(
        after(&files, &[("docs/b.md", "other/b.md")], "docs/a.md"),
        "[b](../other/b.md)\n"
    );
}

#[test]
fn moving_a_folder_rewrites_links_to_its_files_and_to_the_folder() {
    let files = [
        (
            "README.md",
            "[a](docs/a.md) [b](docs/deep/b.md#x) [dir](docs/) [dir2](docs) ![i](docs/img/i.png)\n",
        ),
        ("docs/a.md", "a\n"),
        ("docs/deep/b.md", "b\n"),
        ("docs/img/i.png", "x"),
    ];
    assert_eq!(
        after(&files, &[("docs", "manual")], "README.md"),
        "[a](manual/a.md) [b](manual/deep/b.md#x) [dir](manual/) [dir2](manual) ![i](manual/img/i.png)\n"
    );
}

#[test]
fn a_moved_document_keeps_working_links_by_recomputing_its_relative_paths() {
    let files = [
        (
            "docs/guide.md",
            "[home](../README.md) [sib](sibling.md) ![l](../img/logo.png) [web](https://x.dev/a.md) [top](#top)\n",
        ),
        ("README.md", "x\n"),
        ("docs/sibling.md", "x\n"),
        ("img/logo.png", "x"),
    ];
    assert_eq!(
        after(&files, &[("docs/guide.md", "docs/deep/er/guide.md")], "docs/guide.md"),
        "[home](../../../README.md) [sib](../../sibling.md) ![l](../../../img/logo.png) [web](https://x.dev/a.md) [top](#top)\n"
    );
}

#[test]
fn documents_that_move_together_keep_their_links() {
    let files = [
        ("docs/a.md", "[b](b.md) [c](sub/c.md) [r](../README.md)\n"),
        ("docs/b.md", "[a](a.md)\n"),
        ("docs/sub/c.md", "[a](../a.md)\n"),
        ("README.md", "[a](docs/a.md)\n"),
    ];
    let world = MemWorld::new(&files);
    let plan = plan_of(&world, &[("docs", "manual")]);
    // only what points into the moved folder from outside changes: the documents inside keep their
    // relative layout, and `../README.md` still reaches the same file from the new place
    let paths: Vec<_> = plan.docs.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, ["README.md"]);
    assert_eq!(
        after(&files, &[("docs", "manual")], "README.md"),
        "[a](manual/a.md)\n"
    );
}

#[test]
fn moving_a_document_one_level_deeper_fixes_what_points_out_of_it() {
    let files = [
        ("docs/a.md", "[r](../README.md) [b](b.md)\n"),
        ("docs/b.md", "x\n"),
        ("README.md", "x\n"),
    ];
    assert_eq!(
        after(&files, &[("docs/a.md", "docs/x/a.md")], "docs/a.md"),
        "[r](../../README.md) [b](../b.md)\n"
    );
}

#[test]
fn reference_definitions_are_rewritten_and_their_uses_are_not_touched() {
    let files = [
        (
            "a.md",
            "Read [the guide][g] and [g] and ![logo][l].\n\n[g]: docs/guide.md \"Guide\"\n[l]: <img/my logo.png>\n",
        ),
        ("docs/guide.md", "x\n"),
        ("img/my logo.png", "x"),
    ];
    assert_eq!(
        after(
            &files,
            &[("docs/guide.md", "docs/manual.md"), ("img/my logo.png", "img/brand logo.png")],
            "a.md"
        ),
        "Read [the guide][g] and [g] and ![logo][l].\n\n[g]: docs/manual.md \"Guide\"\n[l]: <img/brand logo.png>\n"
    );
}

#[test]
fn spaces_are_percent_encoded_in_bare_destinations_and_kept_in_angle_brackets() {
    let files = [
        (
            "a.md",
            "[x](docs/guide.md) [y](<docs/guide.md>) [z](docs/guide.md)\n",
        ),
        ("docs/guide.md", "x\n"),
    ];
    assert_eq!(
        after(&files, &[("docs/guide.md", "docs/user guide.md")], "a.md"),
        "[x](docs/user%20guide.md) [y](<docs/user guide.md>) [z](docs/user%20guide.md)\n"
    );
}

#[test]
fn existing_percent_encoding_and_non_ascii_escapes_are_followed() {
    let files = [
        (
            "a.md",
            "[x](my%20file.md) [y](%C3%A1rea/x.md) [z](ção/y.md)\n",
        ),
        ("my file.md", "x\n"),
        ("área/x.md", "x\n"),
        ("ção/y.md", "x\n"),
    ];
    assert_eq!(
        after(
            &files,
            &[
                ("my file.md", "the file.md"),
                ("área", "região"),
                ("ção", "ação")
            ],
            "a.md"
        ),
        "[x](the%20file.md) [y](regi%C3%A3o/x.md) [z](ação/y.md)\n"
    );
}

#[test]
fn characters_that_would_end_a_destination_are_always_encoded() {
    let files = [("a.md", "[x](one.md)\n"), ("one.md", "x\n")];
    assert_eq!(
        after(&files, &[("one.md", "notes #1 (final) 100%.md")], "a.md"),
        "[x](notes%20%231%20%28final%29%20100%25.md)\n"
    );
}

#[test]
fn backslash_separators_are_kept_where_that_is_safe() {
    let files = [("a.md", "[x](docs\\guide.md)\n"), ("docs/guide.md", "x\n")];
    assert_eq!(
        after(&files, &[("docs/guide.md", "manual/guide.md")], "a.md"),
        "[x](manual\\guide.md)\n"
    );
    // a backslash before punctuation would turn into an escape: use slashes
    assert_eq!(
        after(&files, &[("docs/guide.md", "manual/_guide.md")], "a.md"),
        "[x](manual/_guide.md)\n"
    );
}

#[test]
fn a_link_without_the_md_extension_stays_without_it() {
    let files = [
        (
            "a.md",
            "[x](docs/guide) [y](docs/guide#s) [z](docs/guide.md)\n",
        ),
        ("docs/guide.md", "x\n"),
    ];
    assert_eq!(
        after(&files, &[("docs/guide.md", "docs/manual.md")], "a.md"),
        "[x](docs/manual) [y](docs/manual#s) [z](docs/manual.md)\n"
    );
    // renamed to another extension: the extension has to be written
    assert_eq!(
        after(&files, &[("docs/guide.md", "docs/guide.txt")], "a.md"),
        "[x](docs/guide.txt) [y](docs/guide.txt#s) [z](docs/guide.txt)\n"
    );
}

#[test]
fn root_relative_links_stay_root_relative() {
    let files = [
        (
            "deep/er/a.md",
            "[x](/docs/guide.md) [y](/docs/guide.md#s)\n",
        ),
        ("docs/guide.md", "x\n"),
    ];
    assert_eq!(
        after(
            &files,
            &[("docs/guide.md", "manual/guide.md")],
            "deep/er/a.md"
        ),
        "[x](/manual/guide.md) [y](/manual/guide.md#s)\n"
    );
    // moving the document itself does not change a root-relative link
    assert_eq!(
        after(&files, &[("deep/er/a.md", "a.md")], "deep/er/a.md"),
        "[x](/docs/guide.md) [y](/docs/guide.md#s)\n"
    );
}

#[test]
fn code_spans_code_blocks_and_front_matter_are_never_rewritten() {
    let text = "---\nfile: [x](docs/guide.md)\n---\n\nReal [link](docs/guide.md) and `[code](docs/guide.md)`.\n\n```md\n[block](docs/guide.md)\n[ref]: docs/guide.md\n```\n\n    [indented](docs/guide.md)\n";
    let files = [("a.md", text), ("docs/guide.md", "x\n")];
    assert_eq!(
        after(&files, &[("docs/guide.md", "docs/manual.md")], "a.md"),
        text.replacen("[link](docs/guide.md)", "[link](docs/manual.md)", 1)
    );
}

#[test]
fn broken_external_and_anchor_links_are_left_alone() {
    let text = "[gone](docs/missing.md) [web](https://example.com/docs/guide.md) [mail](mailto:a@b.dev) [top](#guide) [file](file:///x/docs/guide.md) [win](C:\\docs\\guide.md) [net](//host/docs/guide.md) [up](../../../outside.md)\n";
    let files = [("a.md", text), ("docs/guide.md", "x\n")];
    let world = MemWorld::new(&files);
    let plan = plan_of(&world, &[("docs/guide.md", "docs/manual.md")]);
    assert!(plan.docs.is_empty(), "{plan:?}");
}

#[test]
fn raw_html_images_and_links_are_rewritten() {
    let files = [
        (
            "README.md",
            "<p align=\"center\"><img src=\"docs/logo.png\" width=\"120\"></p>\n\nSee <a href='docs/guide.md#x'>the guide</a>.\n",
        ),
        ("docs/logo.png", "x"),
        ("docs/guide.md", "x\n"),
    ];
    assert_eq!(
        after(
            &files,
            &[("docs/logo.png", "assets/my logo.png"), ("docs/guide.md", "docs/manual.md")],
            "README.md"
        ),
        "<p align=\"center\"><img src=\"assets/my%20logo.png\" width=\"120\"></p>\n\nSee <a href='docs/manual.md#x'>the guide</a>.\n"
    );
}

#[test]
fn an_apostrophe_in_a_new_name_does_not_end_a_quoted_html_attribute() {
    let files = [
        (
            "a.md",
            "<a href='b.md'>single</a> <a href=\"b.md\">double</a> [md](b.md)\n",
        ),
        ("b.md", "x\n"),
    ];
    // encoded in both kinds of quotes, left as it is in Markdown where it is harmless
    assert_eq!(
        after(&files, &[("b.md", "it's.md")], "a.md"),
        "<a href='it%27s.md'>single</a> <a href=\"it%27s.md\">double</a> [md](it's.md)\n"
    );
}

#[test]
fn every_edit_knows_which_moves_it_comes_from() {
    let files = [
        ("a.md", "[x](docs/b.md) [y](c.md)\n"),
        ("docs/b.md", "[back](../a.md) [z](../c.md)\n"),
        ("c.md", "x\n"),
    ];
    let world = MemWorld::new(&files);
    // 0: b.md is renamed, 1: c.md is renamed
    let plan = plan_of(
        &world,
        &[("docs/b.md", "docs/b2.md"), ("c.md", "docs/c.md")],
    );
    let causes = |doc: &str| -> Vec<Vec<usize>> {
        plan.docs
            .iter()
            .find(|d| d.path == doc)
            .map(|d| d.edits.iter().map(|e| e.causes.clone()).collect())
            .unwrap_or_default()
    };
    // a.md stays: each of its links is about the move of what it points at
    assert_eq!(causes("a.md"), [vec![0], vec![1]]);
    // b.md moves itself (0) and its link to c.md follows c.md (1) too; a link to a file that does not move
    // is not touched by a rename within the same folder
    assert_eq!(causes("docs/b.md"), [vec![0, 1]]);
}

#[test]
fn a_case_only_rename_rewrites_links_to_the_new_case() {
    let files = [
        ("a.md", "[x](docs/readme.md) [y](docs/readme.md)\n"),
        ("docs/readme.md", "x\n"),
    ];
    assert_eq!(
        after(&files, &[("docs/readme.md", "docs/README.md")], "a.md"),
        "[x](docs/README.md) [y](docs/README.md)\n"
    );
}

#[test]
fn several_moves_at_once_are_planned_together() {
    let files = [
        ("a.md", "[b](b.md) [c](c.md) [d](sub/d.md)\n"),
        ("b.md", "[a](a.md) [c](c.md)\n"),
        ("c.md", "x\n"),
        ("sub/d.md", "[a](../a.md)\n"),
    ];
    let m = [("b.md", "sub/b.md"), ("c.md", "sub/c.md")];
    assert_eq!(
        after(&files, &m, "a.md"),
        "[b](sub/b.md) [c](sub/c.md) [d](sub/d.md)\n"
    );
    // b moves next to c: its link to c is still "c.md", its link to a climbs out
    assert_eq!(after(&files, &m, "b.md"), "[a](../a.md) [c](c.md)\n");
}

#[test]
fn a_moved_document_that_links_to_itself_is_followed() {
    let files = [("a.md", "[me](a.md#top) [me2](./a.md)\n")];
    assert_eq!(
        after(&files, &[("a.md", "b.md")], "a.md"),
        "[me](b.md#top) [me2](./b.md)\n"
    );
}

#[test]
fn links_in_worktree_documents_stay_inside_the_worktree() {
    let files = [
        ("@wt/feat/docs/a.md", "[b](b.md) [r](../README.md)\n"),
        ("@wt/feat/docs/b.md", "x\n"),
        ("@wt/feat/README.md", "[a](docs/a.md)\n"),
        ("docs/a.md", "[b](b.md)\n"),
        ("docs/b.md", "x\n"),
    ];
    let m = [("@wt/feat/docs/b.md", "@wt/feat/docs/c.md")];
    assert_eq!(
        after(&files, &m, "@wt/feat/docs/a.md"),
        "[b](c.md) [r](../README.md)\n"
    );
    // the main project's documents are not looked at
    let world = MemWorld::new(&files);
    let plan = plan_of(&world, &m);
    assert!(plan.docs.iter().all(|d| d.path.starts_with("@wt/feat/")));
}

#[test]
fn links_in_tables_lists_headings_and_quotes_are_rewritten() {
    let files = [
        (
            "a.md",
            "# See [g](g.md)\n\n- item [g](g.md)\n\n> quote [g](g.md)\n\n| a | b |\n|---|---|\n| [g](g.md) | x |\n",
        ),
        ("g.md", "x\n"),
    ];
    let expected = "# See [g](h.md)\n\n- item [g](h.md)\n\n> quote [g](h.md)\n\n| a | b |\n|---|---|\n| [g](h.md) | x |\n";
    assert_eq!(after(&files, &[("g.md", "h.md")], "a.md"), expected);
}

#[test]
fn offsets_are_right_with_multibyte_text_a_bom_and_crlf() {
    let text = "\u{FEFF}# Olá\r\n\r\nVeja [guia](docs/guía.md#ação) e ![ícone](img/ícone.png).\r\n";
    let files = [
        ("a.md", text),
        ("docs/guía.md", "x"),
        ("img/ícone.png", "x"),
    ];
    assert_eq!(
        after(
            &files,
            &[
                ("docs/guía.md", "docs/manual.md"),
                ("img/ícone.png", "img/logo.png")
            ],
            "a.md"
        ),
        "\u{FEFF}# Olá\r\n\r\nVeja [guia](docs/manual.md#ação) e ![ícone](img/logo.png).\r\n"
    );
}

#[test]
fn edits_report_the_line_the_old_text_and_the_new_text() {
    let files = [("a.md", "one\n\ntwo [x](g.md)\n"), ("g.md", "x\n")];
    let world = MemWorld::new(&files);
    let plan = plan_of(&world, &[("g.md", "h.md")]);
    let edit = &plan.docs[0].edits[0];
    assert_eq!(
        (edit.line, edit.old.as_str(), edit.new.as_str()),
        (3, "g.md", "h.md")
    );
    assert_eq!(plan.link_count(), 1);
    assert_eq!(plan.docs[0].hash, hash_hex(files[0].1.as_bytes()));
}

#[test]
fn inverted_edits_give_back_the_original_text() {
    let files = [
        (
            "a.md",
            "[a](one.md) text [b](two/three.md#x) ![c](one.md)\n",
        ),
        ("one.md", "x"),
        ("two/three.md", "x"),
    ];
    let world = MemWorld::new(&files);
    let plan = plan_of(
        &world,
        &[
            ("one.md", "a/very/long/new/name.md"),
            ("two/three.md", "t.md"),
        ],
    );
    let doc = &plan.docs[0];
    let rewritten = apply_edits(files[0].1, &doc.edits);
    assert_ne!(rewritten, files[0].1);
    assert_eq!(
        apply_edits(&rewritten, &invert_edits(&doc.edits)),
        files[0].1
    );
}

#[test]
fn documents_that_cannot_be_read_as_text_are_reported() {
    let big = vec![b'a'; usize::try_from(MAX_RENDER_BYTES).expect("fits") + 1];
    let world = MemWorld::new(&[("g.md", "x\n")])
        .with_bytes("latin.md", b"[x](g.md) caf\xE9")
        .with_bytes("big.md", &big);
    let plan = plan_of(&world, &[("g.md", "h.md")]);
    let mut skipped: Vec<_> = plan
        .skipped
        .iter()
        .map(|s| (s.path.as_str(), s.reason))
        .collect();
    skipped.sort_by_key(|(p, _)| *p);
    assert_eq!(
        skipped,
        [
            ("big.md", SkipReason::TooLarge),
            ("latin.md", SkipReason::NotUtf8)
        ]
    );
    assert!(plan.docs.is_empty(), "{plan:?}");
}

#[test]
fn nothing_to_move_is_an_empty_plan() {
    let world = MemWorld::new(&[("a.md", "[x](b.md)\n"), ("b.md", "x\n")]);
    assert_eq!(plan_of(&world, &[]), LinkPlan::default());
}

#[test]
fn unmoved_links_to_unmoved_files_in_a_moved_document_are_recomputed_even_when_extensionless() {
    let files = [
        ("docs/a.md", "[b](b) [c](../c)\n"),
        ("docs/b.md", "x"),
        ("c.md", "x"),
    ];
    assert_eq!(
        after(&files, &[("docs/a.md", "docs/n/a.md")], "docs/a.md"),
        "[b](../b) [c](../../c)\n"
    );
}

#[test]
fn a_folder_link_to_a_moved_folder_keeps_its_trailing_slash_and_dot_slash() {
    let files = [
        ("a.md", "[x](./docs/) [y](docs/sub/)\n"),
        ("docs/sub/b.md", "x"),
    ];
    assert_eq!(
        after(&files, &[("docs", "manual")], "a.md"),
        "[x](./manual/) [y](manual/sub/)\n"
    );
}

#[test]
fn an_entity_like_name_is_encoded_in_html_and_markdown() {
    let files = [
        ("a.md", "[x](one.md) <a href=\"one.md\">y</a>\n"),
        ("one.md", "x"),
    ];
    assert_eq!(
        after(&files, &[("one.md", "R&D.md")], "a.md"),
        "[x](R&D.md) <a href=\"R&amp;D.md\">y</a>\n"
    );
    assert_eq!(
        after(&files, &[("one.md", "&copy;.md")], "a.md"),
        "[x](%26copy;.md) <a href=\"%26copy;.md\">y</a>\n"
    );
}
