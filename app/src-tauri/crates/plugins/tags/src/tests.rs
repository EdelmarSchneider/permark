use std::fmt::Write as _;

use crate::catalog::{self, DocTags, Docs};
use crate::frontmatter;
use crate::meta::{edit_text, Entries, Op, HEADER};
use crate::normalize::{
    normalize_description, normalize_tag, normalize_tags, MAX_DESCRIPTION_CHARS, MAX_TAG_CHARS,
};

/// A byte copy of a real `.permark/meta.toml` written by version 0.5 (CRLF).
const REAL: &str = include_str!("fixtures/real-meta-0.5.toml");

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

// ---- normalization ------------------------------------------------------------------------------------------------

#[test]
fn tags_are_trimmed_lowercased_and_hyphenated() {
    assert_eq!(
        normalize_tag("  Needs Review "),
        Some("needs-review".into())
    );
    assert_eq!(normalize_tag("a \t\n b"), Some("a-b".into()));
    assert_eq!(normalize_tag("area/api"), Some("area/api".into()));
    assert_eq!(normalize_tag("Ünï 档案"), Some("ünï-档案".into()));
    assert_eq!(normalize_tag("   "), None);
    assert_eq!(normalize_tag(""), None);
}

#[test]
fn tags_are_cut_by_characters_not_bytes() {
    let long = "é".repeat(60);
    assert_eq!(
        normalize_tag(&long).map(|t| t.chars().count()),
        Some(MAX_TAG_CHARS)
    );
}

#[test]
fn repeated_tags_collapse_in_order_and_are_capped() {
    assert_eq!(
        normalize_tags(&["Spec", "feed", "spec", "FEED", "new idea"]),
        ["spec", "feed", "new-idea"]
    );
    let many: Vec<String> = (0..500).map(|n| format!("t{n}")).collect();
    assert_eq!(normalize_tags(&many).len(), 100);
}

#[test]
fn descriptions_are_one_line_and_cut() {
    assert_eq!(normalize_description("  A\nB\r\n\r\nC\tD  "), "A B C D");
    assert_eq!(
        normalize_description(&"日".repeat(500)).chars().count(),
        MAX_DESCRIPTION_CHARS
    );
}

// ---- reading the file of 0.5 --------------------------------------------------------------------------------------

#[test]
fn the_real_0_5_file_reads_as_it_is() {
    let entries = Entries::parse(REAL).unwrap();
    assert_eq!(entries.len(), 1);
    let entry = entries.get("design/2-concept/sync/concept.md").unwrap();
    assert_eq!(entry.tags, ["approved"]);
    assert_eq!(entry.description, "");
}

#[test]
fn a_full_0_5_file_reads_tags_descriptions_styles_and_ignores_the_unknown() {
    let text = "# PerMark metadata\n\n[files.\"docs/a.md\"]\ntags = [\"Spec\", \"needs-review\"]\ndescription = \"How it\\ngroups.\"\nfuture = 3\n\n[files.\"b c/é 档.md\"]\ntags = [\"x\"]\n\n[tag_styles.spec]\ncolor = \"BLUE\"\nicon = \"book-open\"\nmore = true\n\n[tag_styles.none]\ncolor = \"magenta\"\n\n[future_table]\nk = 1\n";
    let e = Entries::parse(text).unwrap();
    assert_eq!(e.get("docs/a.md").unwrap().tags, ["spec", "needs-review"]);
    assert_eq!(e.get("docs/a.md").unwrap().description, "How it groups.");
    assert_eq!(e.get("b c/é 档.md").unwrap().tags, ["x"]);
    let spec = e.style("spec").unwrap();
    assert_eq!((spec.color, spec.icon), (Some("blue"), Some("book-open")));
    assert!(
        e.style("none").is_none(),
        "a color outside the palette is none"
    );
    assert_eq!(e.styles().count(), 1);
}

#[test]
fn bad_files_report_the_line_and_odd_shapes_are_ignored() {
    let problem = Entries::parse("[files.\"a.md\"]\ntags = [\"x\"\n").unwrap_err();
    assert!(
        problem.line.is_some_and(|l| (2..=3).contains(&l)),
        "{problem:?}"
    );
    let conflict = "<<<<<<< HEAD\n[files.\"a.md\"]\ntags = [\"x\"]\n=======\n";
    assert_eq!(Entries::parse(conflict).unwrap_err().line, Some(1));
    // wrong types are skipped, not fatal
    let odd = "[files]\n\"a.md\" = 3\n\"b.md\" = { tags = [\"ok\", 4, true] }\n[tag_styles]\nx = \"red\"\n";
    let e = Entries::parse(odd).unwrap();
    assert_eq!(e.get("b.md").unwrap().tags, ["ok"]);
    assert!(e.get("a.md").is_none());
}

#[test]
fn paths_that_differ_in_case_find_the_entry_on_systems_that_ignore_case() {
    let e = Entries::parse("[files.\"Docs/A.md\"]\ntags = [\"x\"]\n").unwrap();
    assert!(e.get("Docs/A.md").is_some());
    if permark_core::paths::CASE_INSENSITIVE {
        assert!(e.get("docs/a.md").is_some());
    } else {
        assert!(e.get("docs/a.md").is_none());
    }
}

// ---- writing: only the touched keys change ------------------------------------------------------------------------

#[test]
fn editing_the_real_file_touches_only_what_changed_and_keeps_crlf() {
    let edited = edit_text(REAL, &[Op::set_description("README.md", "The start")]).unwrap();
    assert!(edited.starts_with(REAL.trim_end()), "{edited:?}");
    assert!(edited.contains("[files.\"README.md\"]"));
    assert!(edited.contains("description = \"The start\""));
    assert!(
        !edited.replace("\r\n", "").contains('\n'),
        "all CRLF: {edited:?}"
    );
    // the other way round: nothing to do = the same bytes
    assert_eq!(edit_text(REAL, &[]).unwrap(), REAL);
    // read back
    let e = Entries::parse(&edited).unwrap();
    assert_eq!(e.get("README.md").unwrap().description, "The start");
    assert_eq!(
        e.get("design/2-concept/sync/concept.md").unwrap().tags,
        ["approved"]
    );
}

#[test]
fn a_new_file_gets_the_header_and_entries_in_path_order() {
    let text = edit_text(
        "",
        &[
            Op::set_tags("z.md", &["b"]),
            Op::set_tags("a/x.md", &["a"]),
            Op::set_description("m.md", "mid"),
        ],
    )
    .unwrap();
    assert!(text.starts_with(HEADER));
    let a = text.find("[files.\"a/x.md\"]").unwrap();
    let m = text.find("[files.\"m.md\"]").unwrap();
    let z = text.find("[files.\"z.md\"]").unwrap();
    assert!(a < m && m < z, "{text}");
    assert_eq!(
        text.matches("\n\n\n").count(),
        0,
        "no stacked blank lines: {text:?}"
    );
}

#[test]
fn comments_order_spacing_unknown_keys_and_other_tables_survive_every_edit() {
    let text = "# my notes about this file\r\n[files.\"a.md\"]\r\ntags = [\"x\", \"y\"]   # keep\r\nfuture = \"v2\"\r\n\r\n\r\n[files.\"b.md\"]\r\ndescription = \"old\"\r\n\r\n[other]\r\nkeep = true\r\n";
    let edited = edit_text(
        text,
        &[
            Op::set_tags("a.md", &["x", "y", "z"]),
            Op::set_description("b.md", ""),
        ],
    )
    .unwrap();
    assert!(edited.contains("# my notes about this file"));
    assert!(edited.contains("future = \"v2\""));
    assert!(edited.contains("[other]\r\nkeep = true"));
    assert!(
        edited.contains("tags = [\"x\", \"y\", \"z\"]   # keep"),
        "{edited:?}"
    );
    assert!(
        !edited.contains("[files.\"b.md\"]"),
        "a blank entry is dropped"
    );
    assert!(!edited.replace("\r\n", "").contains('\n'));
    // the entry with an unknown key stays when its tags go
    let again = edit_text(&edited, &[Op::set_tags("a.md", &[] as &[&str])]).unwrap();
    assert!(again.contains("[files.\"a.md\"]") && again.contains("future = \"v2\""));
    assert!(!again.contains("tags ="));
}

#[test]
fn styles_are_set_replaced_and_removed_without_touching_unknown_keys() {
    let text = "[tag_styles.spec]\ncolor = \"red\"\nfuture = 1\n";
    let blue = edit_text(
        text,
        &[Op::set_style("Spec", Some("blue"), Some("star")).unwrap()],
    )
    .unwrap();
    let e = Entries::parse(&blue).unwrap();
    let style = e.style("spec").unwrap();
    assert_eq!((style.color, style.icon), (Some("blue"), Some("star")));
    assert!(blue.contains("future = 1"));
    let none = edit_text(&blue, &[Op::set_style("spec", None, None).unwrap()]).unwrap();
    assert!(none.contains("future = 1") && !none.contains("color") && !none.contains("icon"));
    // a tag that had only known keys loses its table, and the section goes with it
    let plain = edit_text(
        "[tag_styles.a]\ncolor = \"red\"\n",
        &[Op::set_style("a", None, None).unwrap()],
    )
    .unwrap();
    assert!(!plain.contains("tag_styles"), "{plain:?}");
    // a name outside the sets means none
    assert!(matches!(
        Op::set_style("a", Some("magenta"), Some("rocket")),
        Some(Op::SetStyle {
            color: None,
            icon: None,
            ..
        })
    ));
    assert!(Op::set_style("  ", Some("red"), None).is_none());
}

#[test]
fn rename_moves_an_entry_and_merges_into_an_existing_one() {
    let text = "[files.\"old.md\"]\ntags = [\"a\"]\ndescription = \"from old\"\nkeep = 1\n\n[files.\"new.md\"]\ntags = [\"b\"]\n";
    let moved = edit_text(
        text,
        &[Op::Rename {
            from: "old.md".into(),
            to: "fresh.md".into(),
        }],
    )
    .unwrap();
    assert!(moved.contains("[files.\"fresh.md\"]") && moved.contains("keep = 1"));
    assert!(!moved.contains("old.md"));
    let merged = edit_text(
        text,
        &[Op::Rename {
            from: "old.md".into(),
            to: "new.md".into(),
        }],
    )
    .unwrap();
    let e = Entries::parse(&merged).unwrap();
    assert_eq!(e.get("new.md").unwrap().tags, ["b", "a"]);
    assert_eq!(e.get("new.md").unwrap().description, "from old");
    assert!(e.get("old.md").is_none());
    let tree = edit_text(
        "[files.\"docs/a.md\"]\ntags = [\"a\"]\n[files.\"docs/sub/b.md\"]\ntags = [\"b\"]\n[files.\"docsx/c.md\"]\ntags = [\"c\"]\n",
        &[Op::RenameTree { from: "docs".into(), to: "guide".into() }],
    )
    .unwrap();
    let e = Entries::parse(&tree).unwrap();
    assert!(e.get("guide/a.md").is_some() && e.get("guide/sub/b.md").is_some());
    assert!(
        e.get("docsx/c.md").is_some(),
        "a sibling with the same start stays"
    );
}

#[test]
fn ops_in_memory_and_on_the_text_agree() {
    // a deterministic walk over many operations: the text, read back, equals the memory model
    let paths = ["a.md", "docs/b.md", "docs/sub/c.md", "d e/ü.md", "z.md"];
    let tags = ["x", "y", "needs-review", "档"];
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % (n as u64)).unwrap_or(0)
    };
    let mut text = REAL.to_owned();
    let mut model = Entries::parse(&text).unwrap();
    for _ in 0..300 {
        let op = match next(7) {
            0 | 1 => Op::set_tags(paths[next(5)], &[tags[next(4)], tags[next(4)]][..=next(2)]),
            2 => Op::set_description(paths[next(5)], if next(2) == 0 { "d" } else { "" }),
            3 => Op::Remove {
                path: paths[next(5)].into(),
            },
            4 => Op::Rename {
                from: paths[next(5)].into(),
                to: paths[next(5)].into(),
            },
            5 => Op::RenameTree {
                from: "docs".into(),
                to: ["docs2", "docs", "x/docs"][next(3)].into(),
            },
            _ => Op::set_style(tags[next(4)], Some(["red", "blue"][next(2)]), None).unwrap(),
        };
        model.apply(&op);
        text = edit_text(&text, std::slice::from_ref(&op)).unwrap();
        assert_eq!(Entries::parse(&text).unwrap(), model, "after {op:?}");
    }
}

#[test]
fn bad_text_and_wrong_shapes_are_refused_not_written_over() {
    assert!(edit_text("[files\n", &[Op::set_tags("a.md", &["x"])]).is_err());
    assert!(edit_text("files = 3\n", &[Op::set_tags("a.md", &["x"])]).is_err());
    assert!(edit_text("[files]\n\"a.md\" = 3\n", &[Op::set_tags("a.md", &["x"])]).is_err());
    assert!(edit_text(
        "tag_styles = \"x\"\n",
        &[Op::set_style("a", Some("red"), None).unwrap()]
    )
    .is_err());
}

#[test]
fn many_entries_edit_quickly_and_hostile_keys_are_quoted() {
    let mut text = String::from(HEADER);
    for n in 0..10_000 {
        let _ = write!(
            text,
            "[files.\"dir{}/file{n}.md\"]\ntags = [\"t{}\"]\n\n",
            n % 50,
            n % 7
        );
    }
    let started = std::time::Instant::now();
    let edited = edit_text(&text, &[Op::set_tags("dir1/file1.md", &["changed"])]).unwrap();
    assert!(started.elapsed().as_secs() < 5, "{:?}", started.elapsed());
    assert_eq!(edited.len(), text.len() - "t1".len() + "changed".len());
    let hostile = "a\"b\\c\n[x]\u{0}.md";
    let out = edit_text("", &[Op::set_tags(hostile, &["x"])]).unwrap();
    assert_eq!(
        Entries::parse(&out).unwrap().get(hostile).unwrap().tags,
        ["x"]
    );
}

// ---- front matter ------------------------------------------------------------------------------------------------

#[test]
fn front_matter_tags_in_every_shape() {
    let tags = |t: &str| frontmatter::from_text(t);
    assert_eq!(
        tags("---\ntags: [Spec, \"needs review\", c]\n---\nbody"),
        ["spec", "needs-review", "c"]
    );
    assert_eq!(
        tags(
            "---\ntitle: x\ntags:\n  - one\n  - \"two words\"   # c\n  -   three\nother: 1\n---\n"
        ),
        ["one", "two-words", "three"]
    );
    assert_eq!(tags("---\ntags: a, b c ,d\n---\n"), ["a", "b-c", "d"]);
    assert_eq!(tags("---\ntags: alpha beta\n---\n"), ["alpha", "beta"]);
    assert_eq!(tags("---\r\ntags:\r\n- x\r\n- y\r\n---\r\n"), ["x", "y"]);
    assert_eq!(tags("+++\ntags = [\"a\", \"b\"]\n+++\n"), ["a", "b"]);
    assert_eq!(
        tags("+++\ntags = [\n  \"a\",\n  \"b\",\n]\n+++\n"),
        ["a", "b"]
    );
    assert_eq!(tags("\u{feff}---\ntags: [z]\n---\n"), ["z"]);
    assert!(tags("no front matter\ntags: [a]\n").is_empty());
    assert!(tags("---\ntags: [a]\nnever closed").is_empty());
    assert!(tags("---\ntitle: x\n---\n").is_empty());
    assert!(tags("---\nnottags: [a]\n---\n").is_empty());
    assert!(tags("").is_empty());
    assert_eq!(tags("---\ntags: []\n---\n"), Vec::<String>::new());
}

// ---- catalog -----------------------------------------------------------------------------------------------------

fn docs(items: &[(&str, &[&str])]) -> Docs {
    items
        .iter()
        .map(|(path, tags)| {
            (
                (*path).to_owned(),
                DocTags {
                    tags: strs(tags),
                    len: 1,
                    mtime_ms: 1,
                },
            )
        })
        .collect()
}

#[test]
fn counts_combine_front_matter_and_app_tags_once_per_file() {
    let entries = Entries::parse(
        "[files.\"a.md\"]\ntags = [\"spec\", \"app-only\"]\n[files.\"logo.png\"]\ntags = [\"spec\"]\ndescription = \"the logo\"\n",
    )
    .unwrap();
    let docs = docs(&[("a.md", &["spec", "fm"]), ("b.md", &["fm"]), ("c.md", &[])]);
    let counts = catalog::tag_counts(&entries, &docs);
    let get = |n: &str| counts.iter().find(|c| c.name == n).unwrap();
    assert_eq!(get("spec").count, 2);
    assert!(!get("spec").only_in_files);
    assert_eq!(get("fm").count, 2);
    assert!(get("fm").only_in_files);
    assert_eq!(get("app-only").count, 1);
    assert_eq!(counts[0].count, 2, "most files first");
    assert_eq!(catalog::untagged(&entries, &docs), ["c.md"]);
    assert_eq!(
        catalog::files_with(&entries, &docs, &strs(&["spec"])),
        ["a.md", "logo.png"]
    );
    assert_eq!(
        catalog::files_with(&entries, &docs, &strs(&["spec", "fm"])),
        ["a.md"]
    );
    assert!(catalog::files_with(&entries, &docs, &[]).is_empty());
    assert!(catalog::files_with(&entries, &docs, &strs(&["nope"])).is_empty());
    assert_eq!(
        catalog::files_with(&entries, &docs, &strs(&["SPEC"])),
        ["a.md", "logo.png"]
    );
    let index = catalog::index(&entries, &docs);
    assert_eq!(index.len(), 3, "c.md has nothing to show");
    assert_eq!(
        index[0].tags,
        ["spec", "fm", "app-only"],
        "front matter first"
    );
    assert_eq!(catalog::tags_of(&entries, &docs, "logo.png"), ["spec"]);
    let combined = catalog::combine(&strs(&["a"]), &strs(&["a", "b"]));
    assert_eq!(
        (combined[0].file, combined[0].app, combined[1].file),
        (true, true, false)
    );
}

#[test]
fn a_file_of_only_comments_keeps_them_in_front_of_the_first_entry() {
    let only = "# one\r\n\r\n# two\r\n";
    let text = edit_text(only, &[Op::set_tags("a.md", &["x"])]).unwrap();
    assert!(text.starts_with(only), "{text:?}");
    assert!(text.contains("[files.\"a.md\"]"));
    // taking the last entry away leaves the comments, as they were
    let back = edit_text(&text, &[Op::set_tags("a.md", &[] as &[&str])]).unwrap();
    assert_eq!(back, only);
}

#[test]
fn removing_the_first_entry_keeps_the_comments_written_above_it() {
    // the real file: its two header lines stand in front of its only entry
    let gone = edit_text(
        REAL,
        &[Op::Remove {
            path: "design/2-concept/sync/concept.md".into(),
        }],
    )
    .unwrap();
    assert!(gone.contains("# PerMark metadata: tags and descriptions added in the app."));
    assert!(!gone.contains("[files"));
    // with a second entry the comments move to the entry that is first now
    let two =
        "# top note\n\n[files.\"a.md\"]\ntags = [\"a\"]\n\n[files.\"b.md\"]\ntags = [\"b\"]\n";
    let edited = edit_text(two, &[Op::set_tags("a.md", &[] as &[&str])]).unwrap();
    assert_eq!(edited, "# top note\n\n[files.\"b.md\"]\ntags = [\"b\"]\n");
}
