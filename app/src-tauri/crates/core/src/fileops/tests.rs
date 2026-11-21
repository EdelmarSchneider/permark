use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::events::Recorder;
use crate::paths::Root;
use crate::settings::ProjectConfig;
use crate::state::{AppDirs, StateStore};
use crate::tree::{Explorer, TreeFilter};
use crate::watch::{Change, ChangeKind, OwnWrites};

struct Fx {
    dir: tempfile::TempDir,
    state: tempfile::TempDir,
    ops: FileOps,
    explorer: Arc<Explorer>,
    own: Arc<OwnWrites>,
}

fn fx(files: &[(&str, &str)]) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files {
        let abs = dir.path().join(path);
        if path.ends_with('/') {
            fs::create_dir_all(&abs).unwrap();
        } else {
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(abs, content).unwrap();
        }
    }
    fx_in(dir)
}

fn fx_in(dir: tempfile::TempDir) -> Fx {
    let state = tempfile::tempdir().unwrap();
    let root = Root::open(dir.path()).unwrap();
    let vault = Vault::new(root.clone());
    let store = StateStore::for_project(&AppDirs::under(state.path()), &root);
    let explorer = Arc::new(Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    ));
    explorer.open_view("w", Some(TreeFilter::default()));
    let own = Arc::new(OwnWrites::new());
    let ops = FileOps::new(vault, Arc::clone(&explorer), Arc::clone(&own), &store);
    Fx {
        dir,
        state,
        ops,
        explorer,
        own,
    }
}

fn spec(from: &str, to: &str) -> MoveSpec {
    MoveSpec {
        from: from.to_owned(),
        to: to.to_owned(),
    }
}

fn read(fx: &Fx, rel: &str) -> String {
    fs::read_to_string(fx.dir.path().join(rel)).unwrap()
}

fn exists(fx: &Fx, rel: &str) -> bool {
    fs::symlink_metadata(fx.dir.path().join(rel)).is_ok()
}

/// Plans with links and applies at once (the folders moved into exist, as when a drag ends on a folder).
fn mv(fx: &Fx, moves: &[(&str, &str)]) -> MoveOutcome {
    for (_, to) in moves {
        if let Some(parent) = Path::new(to).parent() {
            fs::create_dir_all(fx.dir.path().join(parent)).unwrap();
        }
    }
    let specs: Vec<MoveSpec> = moves.iter().map(|(a, b)| spec(a, b)).collect();
    let plan = fx.ops.plan_move(&specs, true).unwrap();
    fx.ops.apply_move(plan.plan_id, true).unwrap()
}

fn kind_of(error: FileOpError) -> ErrorKind {
    IpcError::from(error).kind
}

// ---- link updates -------------------------------------------------------------------------------------------------------

#[test]
fn a_link_to_a_renamed_file_is_rewritten_and_the_rest_of_the_link_stays() {
    let f = fx(&[
        (
            "a.md",
            "see [x](b.md#sec \"title\") and [y](b.md?q=1) and ![i](img/p.png)\n",
        ),
        ("b.md", "# b\n"),
        ("img/p.png", "png"),
    ]);
    let outcome = mv(&f, &[("b.md", "renamed.md")]);
    assert_eq!(outcome.links, 2);
    assert_eq!(outcome.documents, 1);
    assert_eq!(
        read(&f, "a.md"),
        "see [x](renamed.md#sec \"title\") and [y](renamed.md?q=1) and ![i](img/p.png)\n"
    );
    assert!(exists(&f, "renamed.md") && !exists(&f, "b.md"));
}

#[test]
fn relative_paths_are_computed_from_each_document() {
    let f = fx(&[
        ("top.md", "[t](docs/guide.md)\n"),
        ("docs/sibling.md", "[s](guide.md) and [up](../top.md)\n"),
        ("docs/deep/inner.md", "[i](../guide.md)\n"),
        ("docs/guide.md", "x"),
    ]);
    mv(&f, &[("docs/guide.md", "ref/guide.md")]);
    assert_eq!(read(&f, "top.md"), "[t](ref/guide.md)\n");
    assert_eq!(
        read(&f, "docs/sibling.md"),
        "[s](../ref/guide.md) and [up](../top.md)\n"
    );
    assert_eq!(read(&f, "docs/deep/inner.md"), "[i](../../ref/guide.md)\n");
}

#[test]
fn a_dot_slash_prefix_is_kept_when_the_path_does_not_climb() {
    let f = fx(&[
        ("a.md", "[b](./b.md) [c](./sub/c.md)\n"),
        ("b.md", ""),
        ("sub/c.md", ""),
    ]);
    mv(&f, &[("b.md", "moved/b.md")]);
    assert_eq!(read(&f, "a.md"), "[b](./moved/b.md) [c](./sub/c.md)\n");
}

#[test]
fn moving_a_folder_rewrites_links_to_its_files_and_to_the_folder() {
    let f = fx(&[
        (
            "a.md",
            "[f](docs/x.md) [d](docs/) [e](docs) [s](docs/sub/y.md)\n",
        ),
        ("docs/x.md", "[sib](sub/y.md) [out](../a.md)\n"),
        ("docs/sub/y.md", "[x](../x.md)\n"),
    ]);
    mv(&f, &[("docs", "archive/docs")]);
    assert_eq!(
        read(&f, "a.md"),
        "[f](archive/docs/x.md) [d](archive/docs/) [e](archive/docs) [s](archive/docs/sub/y.md)\n"
    );
    // the moved documents keep their way to each other and to what stayed
    assert_eq!(
        read(&f, "archive/docs/x.md"),
        "[sib](sub/y.md) [out](../../a.md)\n"
    );
    assert_eq!(read(&f, "archive/docs/sub/y.md"), "[x](../x.md)\n");
}

#[test]
fn a_moved_document_keeps_working_links_by_recomputing_its_relative_paths() {
    let f = fx(&[
        ("docs/a.md", "[b](b.md) [img](../img/p.png) [web](https://x.org) [anchor](#top) [up](../README.md)\n"),
        ("docs/b.md", ""),
        ("img/p.png", ""),
        ("README.md", ""),
    ]);
    mv(&f, &[("docs/a.md", "docs/deeper/a.md")]);
    assert_eq!(
        read(&f, "docs/deeper/a.md"),
        "[b](../b.md) [img](../../img/p.png) [web](https://x.org) [anchor](#top) [up](../../README.md)\n"
    );
}

#[test]
fn documents_that_move_together_keep_their_links() {
    let f = fx(&[("a.md", "[b](b.md)\n"), ("b.md", "[a](a.md)\n")]);
    mv(&f, &[("a.md", "new/a.md"), ("b.md", "new/b.md")]);
    assert_eq!(read(&f, "new/a.md"), "[b](b.md)\n");
    assert_eq!(read(&f, "new/b.md"), "[a](a.md)\n");
}

#[test]
fn reference_definitions_are_rewritten_and_their_uses_are_not_touched() {
    let f = fx(&[
        (
            "a.md",
            "see [ref][one] and [two]\n\n[one]: b.md \"T\"\n[two]: <b.md#s>\n",
        ),
        ("b.md", ""),
    ]);
    mv(&f, &[("b.md", "c/b.md")]);
    assert_eq!(
        read(&f, "a.md"),
        "see [ref][one] and [two]\n\n[one]: c/b.md \"T\"\n[two]: <c/b.md#s>\n"
    );
}

#[test]
fn definitions_over_lines_and_heading_links_are_rewritten_and_undone_exactly() {
    let doc = "[one]: b.md\nsee [one] and [x](b.md#s)\n---\n\n[two]:\r\n  <b.md>\r\n  \"T\"\r\nthen [two] ![i](b.md)\r\n\n[three]: b.md\n  'T'\n\n> [four]:\n>   b.md\n> quote [y](b.md)\n> ===\n";
    let f = fx(&[("a.md", doc), ("b.md", "")]);
    let outcome = mv(&f, &[("b.md", "c/b.md")]);
    assert_eq!(outcome.links, 7);
    assert_eq!(
        read(&f, "a.md"),
        "[one]: c/b.md\nsee [one] and [x](c/b.md#s)\n---\n\n[two]:\r\n  <c/b.md>\r\n  \"T\"\r\nthen [two] ![i](c/b.md)\r\n\n[three]: c/b.md\n  'T'\n\n> [four]:\n>   c/b.md\n> quote [y](c/b.md)\n> ===\n"
    );
    f.ops.undo().unwrap().unwrap();
    assert_eq!(read(&f, "a.md"), doc, "byte for byte");
}

#[test]
fn spaces_and_special_characters_are_written_the_way_the_link_was() {
    let f = fx(&[
        ("a.md", "[bare](x.md) [angle](<x.md>) [pct](x.md)\n"),
        ("x.md", ""),
    ]);
    mv(&f, &[("x.md", "My Notes (1).md")]);
    assert_eq!(
        read(&f, "a.md"),
        "[bare](My%20Notes%20%281%29.md) [angle](<My Notes (1).md>) [pct](My%20Notes%20%281%29.md)\n"
    );
    // existing percent-encoding is followed, including non-ASCII escapes
    let g = fx(&[("a.md", "[x](caf%C3%A9.md)\n"), ("café.md", "")]);
    mv(&g, &[("café.md", "thé.md")]);
    assert_eq!(read(&g, "a.md"), "[x](th%C3%A9.md)\n");
    let h = fx(&[("a.md", "[x](cafe.md)\n"), ("cafe.md", "")]);
    mv(&h, &[("cafe.md", "thé.md")]);
    assert_eq!(read(&h, "a.md"), "[x](thé.md)\n", "plain text stays plain");
}

#[test]
fn backslash_separators_and_missing_extensions_are_kept() {
    let f = fx(&[
        ("a.md", "[w](sub\\b.md) [n](sub/b) [r](/sub/b.md)\n"),
        ("sub/b.md", ""),
    ]);
    mv(&f, &[("sub/b.md", "other/b.md")]);
    assert_eq!(
        read(&f, "a.md"),
        "[w](other\\b.md) [n](other/b) [r](/other/b.md)\n"
    );
}

#[test]
fn a_repair_token_keeps_the_suffix_and_the_written_style_and_refuses_another_root() {
    use crate::links::Form;
    let doc = "a/note.md";
    let target = "b/plan.md";
    assert_eq!(
        replacement_href(doc, "old/plan.md#section", Form::Bare, target),
        Some("../b/plan.md#section".to_owned())
    );
    assert_eq!(
        replacement_href(doc, "old/plan.md?q=1", Form::Bare, target),
        Some("../b/plan.md?q=1".to_owned())
    );
    assert_eq!(
        replacement_href(doc, "old/plan.md?q=1#section", Form::Bare, target),
        Some("../b/plan.md?q=1#section".to_owned())
    );
    assert_eq!(
        replacement_href(doc, "old/plan#section", Form::Bare, target),
        Some("../b/plan#section".to_owned()),
        "a missing .md stays missing"
    );
    assert_eq!(
        replacement_href(doc, "pics/shot.png", Form::Bare, "img/shot.png"),
        Some("../img/shot.png".to_owned())
    );
    assert_eq!(
        replacement_href(doc, "old/plan.md#s", Form::Angle, target),
        Some("<../b/plan.md#s>".to_owned())
    );
    assert_eq!(
        replacement_href(doc, "old/my%20plan.md", Form::Bare, "b/my plan.md"),
        Some("../b/my%20plan.md".to_owned())
    );
    assert_eq!(
        replacement_href("a.md", "sub\\b.md", Form::Bare, "other/b.md"),
        Some("other\\b.md".to_owned())
    );
    assert_eq!(
        replacement_href(doc, "./plan.md", Form::Bare, "a/plan.md"),
        Some("./plan.md".to_owned())
    );
    assert_eq!(
        replacement_href(doc, "old/plan.md#s", Form::Bare, "@wt/other/b/plan.md"),
        None
    );
    assert_eq!(
        replacement_href("@wt/mine/a/note.md", "old/plan.md", Form::Bare, "b/plan.md"),
        None
    );
    assert_eq!(
        replacement_href(
            "@wt/mine/a/note.md",
            "old/plan.md",
            Form::Bare,
            "@wt/other/b/plan.md"
        ),
        None
    );
    assert_eq!(
        replacement_href(
            "@wt/mine/a/note.md",
            "old/plan.md#section",
            Form::Bare,
            "@wt/mine/b/plan.md"
        ),
        Some("../b/plan.md#section".to_owned())
    );
}

#[test]
fn code_front_matter_math_broken_external_and_anchor_links_are_left_alone() {
    let text = "---\nlink: b.md\n---\n`[c](b.md)` ```b.md``` \n\n```\n[d](b.md)\n```\n\n$[m](b.md)$ [gone](missing.md) [web](https://b.md) [mail](mailto:b.md) [self](#b.md)\n";
    let f = fx(&[("a.md", text), ("b.md", ""), ("z/", "")]);
    let plan = f.ops.plan_move(&[spec("b.md", "z/b.md")], true).unwrap();
    assert_eq!(plan.link_count, 0);
    f.ops.apply_move(plan.plan_id, true).unwrap();
    assert_eq!(read(&f, "a.md"), text, "not one byte changed");
}

#[test]
fn html_images_and_links_are_rewritten_and_an_apostrophe_does_not_end_a_quoted_attribute() {
    let f = fx(&[
        (
            "a.md",
            "<img src=\"pics/p.png\" width=\"10\">\n\n<a href='pics/p.png'>link</a>\n",
        ),
        ("pics/p.png", ""),
    ]);
    mv(&f, &[("pics/p.png", "it's.png")]);
    let text = read(&f, "a.md");
    // the apostrophe is encoded in both attributes: it would end a single-quoted value
    assert!(text.contains("src=\"it%27s.png\""), "{text}");
    assert!(text.contains("href='it%27s.png'"), "{text}");
}

#[test]
fn a_case_only_rename_works_and_rewrites_links_to_the_new_case() {
    if !crate::paths::CASE_INSENSITIVE {
        return;
    }
    let f = fx(&[("a.md", "[x](readme.md)\n"), ("readme.md", "")]);
    mv(&f, &[("readme.md", "README.md")]);
    assert_eq!(read(&f, "a.md"), "[x](README.md)\n");
    let names: Vec<String> = fs::read_dir(f.dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    assert!(names.contains(&"README.md".to_owned()), "{names:?}");
}

#[test]
fn a_moved_document_that_links_to_itself_is_followed() {
    let f = fx(&[
        ("docs/a.md", "[me](a.md) [me2](./a.md#x) [top](../top.md)\n"),
        ("top.md", ""),
    ]);
    mv(&f, &[("docs/a.md", "docs/b/a.md")]);
    assert_eq!(
        read(&f, "docs/b/a.md"),
        "[me](a.md) [me2](./a.md#x) [top](../../top.md)\n"
    );
}

#[test]
fn crlf_bom_and_multibyte_text_survive_the_edit() {
    let text = "\u{feff}日本語 [x](b.md)\r\nação [y](b.md#z)\r\n";
    let f = fx(&[("a.md", text), ("b.md", "")]);
    mv(&f, &[("b.md", "sub/ü b.md")]);
    assert_eq!(
        read(&f, "a.md"),
        "\u{feff}日本語 [x](sub/ü%20b.md)\r\nação [y](sub/ü%20b.md#z)\r\n"
    );
}

#[test]
fn documents_that_cannot_be_read_as_text_are_reported() {
    let f = fx(&[("ok.md", "[b](b.md)\n"), ("b.md", "")]);
    fs::write(
        f.dir.path().join("utf16.md"),
        b"\xff\xfe[\0b\0]\0(\0b\0.\0m\0d\0)\0",
    )
    .unwrap();
    f.explorer.sync_paths(&["utf16.md".to_owned()], &[]);
    let plan = f.ops.plan_move(&[spec("b.md", "c.md")], true).unwrap();
    assert!(
        plan.skipped
            .iter()
            .any(|s| s.path == "utf16.md" && s.reason == SkipReason::NotUtf8),
        "{:?}",
        plan.skipped
    );
    assert_eq!(plan.link_count, 1);
}

#[test]
fn a_preview_lists_every_link_with_its_line_and_a_plan_without_links_reads_nothing() {
    let f = fx(&[("a.md", "x\ny [l](b.md)\n"), ("b.md", "")]);
    let plan = f.ops.plan_move(&[spec("b.md", "c.md")], true).unwrap();
    assert_eq!(plan.file_count, 1);
    assert_eq!(plan.files[0].path, "a.md");
    assert_eq!(plan.files[0].changes[0].line, 2);
    assert_eq!(
        (
            plan.files[0].changes[0].old.as_str(),
            plan.files[0].changes[0].new.as_str()
        ),
        ("b.md", "c.md")
    );
    let bare = f.ops.plan_move(&[spec("b.md", "c.md")], false).unwrap();
    assert_eq!((bare.link_count, bare.files.len()), (0, 0));
    f.ops.apply_move(bare.plan_id, false).unwrap();
    assert_eq!(
        read(&f, "a.md"),
        "x\ny [l](b.md)\n",
        "move without updating links"
    );
}

#[test]
fn a_document_with_unsaved_edits_is_skipped_and_said_so() {
    let f = fx(&[
        ("open doc.md", "[l](b.md)\n"),
        ("other.md", "[l](b.md)\n"),
        ("b.md", ""),
    ]);
    let unsaved: std::collections::HashSet<String> =
        [crate::paths::VaultPath::parse("open doc.md").unwrap().key()].into();
    let plan = f
        .ops
        .plan_move_with(&[spec("b.md", "c.md")], true, &unsaved)
        .unwrap();
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].path, "other.md");
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, SkipReason::Unsaved);
    f.ops.apply_move(plan.plan_id, true).unwrap();
    assert_eq!(read(&f, "open doc.md"), "[l](b.md)\n", "left to the editor");
    assert_eq!(read(&f, "other.md"), "[l](c.md)\n");
}

// ---- apply: guards -------------------------------------------------------------------------------------------------------

#[test]
fn a_preview_can_be_applied_once() {
    let f = fx(&[("a.md", ""), ("b.md", "")]);
    let plan = f.ops.plan_move(&[spec("a.md", "c.md")], true).unwrap();
    f.ops.apply_move(plan.plan_id, true).unwrap();
    assert_eq!(
        kind_of(f.ops.apply_move(plan.plan_id, true).unwrap_err()),
        ErrorKind::Unavailable
    );
    assert_eq!(
        kind_of(f.ops.apply_move(9999, true).unwrap_err()),
        ErrorKind::Unavailable
    );
}

#[test]
fn a_document_that_changed_after_the_preview_is_skipped_and_reported() {
    let f = fx(&[
        ("a.md", "[b](b.md)\n"),
        ("c.md", "[b](b.md)\n"),
        ("b.md", ""),
    ]);
    let plan = f.ops.plan_move(&[spec("b.md", "moved.md")], true).unwrap();
    // an agent saves a.md between the preview and the apply
    fs::write(f.dir.path().join("a.md"), "[b](b.md) and new words\n").unwrap();
    let outcome = f.ops.apply_move(plan.plan_id, true).unwrap();
    assert_eq!(
        outcome.skipped,
        [Skip {
            path: "a.md".to_owned(),
            reason: SkipReason::Changed
        }]
    );
    assert_eq!(
        read(&f, "a.md"),
        "[b](b.md) and new words\n",
        "never overwrites a version it has not seen"
    );
    assert_eq!(read(&f, "c.md"), "[b](moved.md)\n");
    assert!(exists(&f, "moved.md"));
}

#[test]
fn something_created_between_preview_and_apply_is_not_overwritten() {
    let f = fx(&[("a.md", "A"), ("b.md", "B")]);
    let plan = f.ops.plan_move(&[spec("a.md", "c.md")], true).unwrap();
    fs::write(f.dir.path().join("c.md"), "precious").unwrap();
    assert_eq!(
        kind_of(f.ops.apply_move(plan.plan_id, true).unwrap_err()),
        ErrorKind::Exists
    );
    assert_eq!(read(&f, "c.md"), "precious");
    assert_eq!(read(&f, "a.md"), "A");
}

#[test]
fn own_writes_and_moves_are_announced_to_the_watch() {
    let f = fx(&[("a.md", "[b](b.md)\n"), ("b.md", ""), ("dir/x.md", "")]);
    mv(&f, &[("b.md", "c.md"), ("dir", "dir2")]);
    let new_hash = crate::fsx::hash(read(&f, "a.md").as_bytes());
    let mut changes = [
        Change {
            path: "a.md".to_owned(),
            kind: ChangeKind::Modified,
            is_dir: false,
            own: false,
        },
        Change {
            path: "b.md".to_owned(),
            kind: ChangeKind::Removed,
            is_dir: false,
            own: false,
        },
        Change {
            path: "c.md".to_owned(),
            kind: ChangeKind::Created,
            is_dir: false,
            own: false,
        },
        Change {
            path: "dir2/x.md".to_owned(),
            kind: ChangeKind::Created,
            is_dir: false,
            own: false,
        },
        Change {
            path: "other.md".to_owned(),
            kind: ChangeKind::Created,
            is_dir: false,
            own: false,
        },
    ];
    f.own
        .mark(&mut changes, |p| (p == "a.md").then_some(new_hash.0));
    let own: Vec<bool> = changes.iter().map(|c| c.own).collect();
    assert_eq!(own, [true, true, true, true, false]);
}

#[test]
fn the_tree_follows_at_once() {
    let f = fx(&[("a.md", ""), ("docs/x.md", "")]);
    mv(&f, &[("a.md", "docs/a.md")]);
    let snap = f.explorer.open_view("w", Some(TreeFilter::default()));
    let paths: Vec<_> = snap.nodes.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(paths, ["docs", "docs/a.md", "docs/x.md"]);
}

// ---- refusals -------------------------------------------------------------------------------------------------------------

#[test]
fn moves_are_refused_for_every_reason_and_touch_nothing() {
    let f = fx(&[
        ("a.md", ""),
        ("b.md", ""),
        ("docs/x.md", ""),
        (".git/HEAD", ""),
        (".permark/config.toml", ""),
        ("sub/", ""),
    ]);
    let plan = |moves: &[(&str, &str)]| {
        let specs: Vec<MoveSpec> = moves.iter().map(|(a, b)| spec(a, b)).collect();
        f.ops.plan_move(&specs, true).map(|_| ()).map_err(kind_of)
    };
    assert_eq!(plan(&[("a.md", "b.md")]), Err(ErrorKind::Exists));
    assert_eq!(plan(&[("missing.md", "z.md")]), Err(ErrorKind::NotFound));
    assert_eq!(plan(&[("docs", "docs/inner")]), Err(ErrorKind::IntoItself));
    assert_eq!(
        plan(&[("docs", "docs")]),
        Err(ErrorKind::Unavailable),
        "nothing to do"
    );
    assert_eq!(plan(&[("a.md", "../out.md")]), Err(ErrorKind::PathEscape));
    assert_eq!(plan(&[("a.md", "/abs.md")]), Err(ErrorKind::InvalidPath));
    assert_eq!(plan(&[("a.md", "C:/x.md")]), Err(ErrorKind::InvalidPath));
    assert_eq!(plan(&[(".git", "g")]), Err(ErrorKind::Protected));
    assert_eq!(plan(&[(".git/HEAD", "h")]), Err(ErrorKind::Protected));
    assert_eq!(plan(&[("a.md", ".git/a.md")]), Err(ErrorKind::Protected));
    assert_eq!(plan(&[(".permark", "p")]), Err(ErrorKind::Protected));
    assert_eq!(
        plan(&[("a.md", ".permark/a.md")]),
        Err(ErrorKind::Protected)
    );
    assert_eq!(
        plan(&[("", "x")]),
        Err(ErrorKind::Protected),
        "the project folder"
    );
    assert_eq!(plan(&[("a.md", "nodir/a.md")]), Err(ErrorKind::NotFound));
    assert_eq!(
        plan(&[("a.md", "c.md"), ("b.md", "c.md")]),
        Err(ErrorKind::Overlap)
    );
    assert_eq!(
        plan(&[("a.md", "C.md"), ("b.md", "c.md")]),
        Err(ErrorKind::Overlap),
        "compared without case"
    );
    assert_eq!(
        plan(&[("docs", "d2"), ("docs/x.md", "x.md")]),
        Err(ErrorKind::Overlap)
    );
    // a bad name is refused as a name (or, where the path syntax itself rejects it, as a path)
    for bad in ["bad:name.md", "con.md", "trailing.", "star*.md", "pipe|.md"] {
        let refused = plan(&[("a.md", bad)]);
        assert!(
            matches!(
                refused,
                Err(ErrorKind::InvalidName | ErrorKind::InvalidPath)
            ),
            "{bad}: {refused:?}"
        );
    }
    assert_eq!(plan(&[("a.md", "")]), Err(ErrorKind::Protected));
    assert_eq!(
        f.ops.plan_move(&[], true).map(|_| ()).map_err(kind_of),
        Err(ErrorKind::Unavailable)
    );
    assert!(exists(&f, "a.md") && exists(&f, "b.md") && exists(&f, "docs/x.md"));
}

#[test]
fn names_are_judged_by_one_rule_everywhere() {
    let f = fx(&[("a.md", "")]);
    for bad in [
        "a|b.md",
        "a?.md",
        "a*.md",
        "a<.md",
        "a\".md",
        " lead.md",
        "trail .md ",
    ] {
        let err = f
            .ops
            .plan_move(&[spec("a.md", bad)], true)
            .map(|_| ())
            .map_err(kind_of);
        assert!(err.is_err(), "{bad:?}");
    }
    for good in [
        "日本語.md",
        "with space.md",
        ".hidden.md",
        "ünï.md",
        "😀.md",
    ] {
        f.ops.plan_move(&[spec("a.md", good)], true).unwrap();
    }
}

#[test]
fn a_move_that_fails_half_way_puts_back_what_it_moved() {
    let f = fx(&[("a.md", "A"), ("b.md", "B"), ("c.md", "C")]);
    let vault = Vault::new(Root::open(f.dir.path()).unwrap());
    let pairs = [
        (
            VaultPath::parse("a.md").unwrap(),
            VaultPath::parse("a2.md").unwrap(),
        ),
        (
            VaultPath::parse("b.md").unwrap(),
            VaultPath::parse("b2.md").unwrap(),
        ),
        (
            VaultPath::parse("c.md").unwrap(),
            VaultPath::parse("c2.md").unwrap(),
        ),
    ];
    let valid = ops::check_moves(&vault, &pairs).unwrap();
    let calls = std::cell::Cell::new(0);
    let result = ops::run_moves(&valid, |from: &Path, to: &Path| {
        calls.set(calls.get() + 1);
        if to.ends_with("c2.md") {
            Err(std::io::Error::other("disk on fire"))
        } else {
            fs::rename(from, to)
        }
    });
    assert!(result.is_err());
    assert!(exists(&f, "a.md") && exists(&f, "b.md") && exists(&f, "c.md"));
    assert!(!exists(&f, "a2.md") && !exists(&f, "b2.md"));
}

#[test]
fn what_cannot_be_put_back_is_named() {
    let f = fx(&[("a.md", "A"), ("b.md", "B")]);
    let vault = Vault::new(Root::open(f.dir.path()).unwrap());
    let pairs = [
        (
            VaultPath::parse("a.md").unwrap(),
            VaultPath::parse("a2.md").unwrap(),
        ),
        (
            VaultPath::parse("b.md").unwrap(),
            VaultPath::parse("b2.md").unwrap(),
        ),
    ];
    let valid = ops::check_moves(&vault, &pairs).unwrap();
    let dir = f.dir.path().to_path_buf();
    let result = ops::run_moves(&valid, |from: &Path, to: &Path| {
        if to.ends_with("b2.md") {
            // something takes the first file's old place, then the second move fails
            fs::write(dir.join("a.md"), "squatter").unwrap();
            Err(std::io::Error::other("nope"))
        } else {
            fs::rename(from, to)
        }
    });
    let Err(FileOpError::PartlyMoved { left, .. }) = result else {
        panic!("expected PartlyMoved");
    };
    assert_eq!(left, [("a.md".to_owned(), "a2.md".to_owned())]);
    assert_eq!(read(&f, "a.md"), "squatter", "nothing was overwritten");
    assert_eq!(read(&f, "a2.md"), "A");
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_is_moved_itself_not_its_target_and_cannot_reach_outside() {
    let f = fx(&[("a.md", "")]);
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.md"), "S").unwrap();
    std::os::unix::fs::symlink(outside.path(), f.dir.path().join("escape")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.md"),
        f.dir.path().join("link.md"),
    )
    .unwrap();
    // moving through the link out of the project is refused
    assert!(f
        .ops
        .plan_move(&[spec("escape/secret.md", "stolen.md")], true)
        .is_err());
    assert!(f
        .ops
        .plan_move(&[spec("a.md", "escape/in.md")], true)
        .is_err());
    // the link itself moves; its target stays
    mv(&f, &[("link.md", "renamed-link.md")]);
    assert!(fs::symlink_metadata(f.dir.path().join("renamed-link.md"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::read_to_string(outside.path().join("secret.md")).unwrap(),
        "S"
    );
}

// ---- delete / new --------------------------------------------------------------------------------------------------------

#[test]
fn new_files_and_folders_are_made_checked_and_never_over_something() {
    let f = fx(&[("docs/a.md", "keep"), (".git/HEAD", "")]);
    assert_eq!(
        f.ops.create("docs", "notes", NewKind::File).unwrap(),
        "docs/notes.md"
    );
    assert_eq!(read(&f, "docs/notes.md"), "");
    assert_eq!(
        f.ops.create("docs", "todo.txt", NewKind::File).unwrap(),
        "docs/todo.txt"
    );
    assert_eq!(f.ops.create("", "area", NewKind::Folder).unwrap(), "area");
    assert!(f.dir.path().join("area").is_dir());
    let err = |dir: &str, name: &str, kind| f.ops.create(dir, name, kind).map_err(kind_of);
    assert_eq!(err("docs", "a.md", NewKind::File), Err(ErrorKind::Exists));
    if crate::paths::CASE_INSENSITIVE {
        assert_eq!(err("docs", "A.md", NewKind::File), Err(ErrorKind::Exists));
    }
    assert_eq!(
        err("docs", "run.sh", NewKind::File),
        Err(ErrorKind::InvalidName)
    );
    assert_eq!(
        err("docs", "a/b", NewKind::File),
        Err(ErrorKind::InvalidName)
    );
    assert_eq!(
        err("docs", "con", NewKind::Folder),
        Err(ErrorKind::InvalidName)
    );
    assert_eq!(err("nodir", "x", NewKind::File), Err(ErrorKind::NotFound));
    assert_eq!(
        err("../out", "x", NewKind::File),
        Err(ErrorKind::PathEscape)
    );
    assert_eq!(err(".git", "x", NewKind::File), Err(ErrorKind::Protected));
    assert_eq!(
        err("docs/a.md", "x", NewKind::File),
        Err(ErrorKind::NotFound),
        "a file is no folder"
    );
    assert_eq!(read(&f, "docs/a.md"), "keep");
}

#[test]
fn deleting_goes_to_the_bin_and_nested_paths_collapse() {
    let f = fx(&[
        ("a.md", "A"),
        ("docs/x.md", "X"),
        ("docs/sub/y.md", "Y"),
        ("keep.md", "K"),
    ]);
    match f.ops.delete(&[
        "docs".to_owned(),
        "docs/sub/y.md".to_owned(),
        "a.md".to_owned(),
        "a.md".to_owned(),
    ]) {
        Ok(outcome) => {
            assert_eq!(outcome.deleted, ["docs", "a.md"]);
            assert!(!exists(&f, "docs") && !exists(&f, "a.md") && exists(&f, "keep.md"));
            let snap = f.explorer.open_view("w", Some(TreeFilter::default()));
            assert_eq!(snap.nodes.len(), 1, "the tree follows");
        }
        // a build machine without a recycle bin: nothing may have been deleted
        Err(error) => {
            eprintln!("recycle bin not usable here: {error}");
            assert!(exists(&f, "a.md") && exists(&f, "docs/x.md"));
        }
    }
}

#[test]
fn deleting_things_that_do_not_exist_or_cannot_be_touched_is_refused() {
    let f = fx(&[
        ("a.md", ""),
        (".git/HEAD", ""),
        (".permark/config.toml", ""),
    ]);
    let del = |p: &str| f.ops.delete(&[p.to_owned()]).map(|_| ()).map_err(kind_of);
    assert_eq!(del("missing.md"), Err(ErrorKind::NotFound));
    assert_eq!(del("../x.md"), Err(ErrorKind::PathEscape));
    assert_eq!(del(".git"), Err(ErrorKind::Protected));
    assert_eq!(del(".permark/config.toml"), Err(ErrorKind::Protected));
    assert_eq!(del(""), Err(ErrorKind::Protected));
    assert_eq!(
        f.ops.delete(&[]).map(|_| ()).map_err(kind_of),
        Err(ErrorKind::Unavailable)
    );
    assert!(exists(&f, "a.md"));
}

// ---- undo -----------------------------------------------------------------------------------------------------------------

#[test]
fn undo_moves_everything_back_and_restores_the_links() {
    let f = fx(&[
        ("a.md", "see [b](b.md#s) \"x\"\n"),
        ("b.md", "back to [a](a.md)\n"),
        ("c.md", "[b](b.md)\n"),
    ]);
    let before: Vec<String> = ["a.md", "b.md", "c.md"]
        .iter()
        .map(|p| read(&f, p))
        .collect();
    mv(&f, &[("b.md", "docs/b.md")]);
    assert_ne!(read(&f, "a.md"), before[0]);
    let outcome = f.ops.undo().unwrap().unwrap();
    assert_eq!(outcome.kind, UndoKind::Move);
    assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    let after: Vec<String> = ["a.md", "b.md", "c.md"]
        .iter()
        .map(|p| read(&f, p))
        .collect();
    assert_eq!(after, before, "byte for byte");
    assert!(!exists(&f, "docs/b.md"));
    assert!(f.ops.undo().unwrap().is_none(), "nothing left to undo");
}

#[test]
fn undo_of_a_moved_document_with_rewritten_links_restores_both() {
    let f = fx(&[
        ("docs/a.md", "[img](../img/p.png) [b](b.md)\n"),
        ("docs/b.md", ""),
        ("img/p.png", ""),
    ]);
    let original = read(&f, "docs/a.md");
    mv(&f, &[("docs/a.md", "x/y/a.md")]);
    assert_ne!(read(&f, "x/y/a.md"), original);
    f.ops.undo().unwrap().unwrap();
    assert_eq!(read(&f, "docs/a.md"), original);
}

// ---- `@path` references (prompt text) -------------------------------------------------------------------------------

/// [`fx`] with the prompt features on (the plugin that marks `@path` references in reading).
fn fx_prompts(files: &[(&str, &str)]) -> Fx {
    use crate::format::{Contributions, FormatRegistry, MarkdownFeature, PromptBlocks};
    let Fx {
        dir,
        state,
        ops,
        explorer,
        own,
    } = fx(files);
    let formats = FormatRegistry::build(&Contributions::default().markdown(vec![
        MarkdownFeature::PromptBlocks(PromptBlocks {
            tags: crate::format::DEFAULT_PROMPT_TAGS,
        }),
    ]));
    Fx {
        dir,
        state,
        ops: ops.with_formats(Arc::new(std::sync::RwLock::new(Arc::new(formats)))),
        explorer,
        own,
    }
}

const PROMPT_TEXT: &str = "Read @docs/guide.md first.\r\n`@docs/guide.md` stays, [@docs/guide.md](other.md) too\r\n<context>\r\n- see @docs/guide.md.\r\n</context>\r\n";

#[test]
fn references_to_a_moved_file_are_rewritten_in_place_counted_and_undone() {
    let f = fx_prompts(&[
        ("README.md", PROMPT_TEXT),
        ("docs/guide.md", "# Guide\n"),
        ("other.md", ""),
    ]);
    let plan = f
        .ops
        .plan_move(&[spec("docs/guide.md", "docs/manual.md")], true)
        .unwrap();
    assert_eq!((plan.link_count, plan.file_count), (2, 1));
    let changes: Vec<(u32, &str, &str)> = plan.files[0]
        .changes
        .iter()
        .map(|c| (c.line, c.old.as_str(), c.new.as_str()))
        .collect();
    assert_eq!(
        changes,
        [
            (1, "docs/guide.md", "docs/manual.md"),
            (4, "docs/guide.md", "docs/manual.md")
        ]
    );
    let outcome = f.ops.apply_move(plan.plan_id, true).unwrap();
    assert_eq!((outcome.links, outcome.documents), (2, 1));
    // only the two paths changed: code, the link's text, the line endings and every other byte stay
    assert_eq!(
        read(&f, "README.md"),
        PROMPT_TEXT
            .replacen("Read @docs/guide.md", "Read @docs/manual.md", 1)
            .replacen("see @docs/guide.md.", "see @docs/manual.md.", 1)
    );
    f.ops.undo().unwrap().unwrap();
    assert_eq!(read(&f, "README.md"), PROMPT_TEXT, "byte for byte");
    assert!(exists(&f, "docs/guide.md"));
}

#[test]
fn references_follow_a_moved_folder_and_stay_when_the_new_name_cannot_be_one() {
    let text = "@docs/guide.md and @/docs/a.md\n";
    let f = fx_prompts(&[
        ("README.md", text),
        ("docs/guide.md", ""),
        ("docs/a.md", ""),
    ]);
    mv(&f, &[("docs", "handbook")]);
    assert_eq!(
        read(&f, "README.md"),
        "@handbook/guide.md and @/handbook/a.md\n"
    );
    // a name with a space ends a reference: written in, the text would name another file
    let f = fx_prompts(&[
        ("README.md", text),
        ("docs/guide.md", ""),
        ("docs/a.md", ""),
    ]);
    let plan = f
        .ops
        .plan_move(&[spec("docs/guide.md", "docs/my guide.md")], true)
        .unwrap();
    assert_eq!(plan.link_count, 0);
}

#[test]
fn a_reference_below_front_matter_moves_and_the_front_matter_stays() {
    let text =
        "\u{feff}---\r\nsee: \"@docs/guide.md\"\r\n---\r\nÉ @docs/guide.md, ü @docs/guide.md\r\n";
    let f = fx_prompts(&[("README.md", text), ("docs/guide.md", "")]);
    let plan = f
        .ops
        .plan_move(&[spec("docs/guide.md", "docs/manual.md")], true)
        .unwrap();
    let lines: Vec<u32> = plan.files[0].changes.iter().map(|c| c.line).collect();
    assert_eq!(lines, [4, 4]);
    f.ops.apply_move(plan.plan_id, true).unwrap();
    // reading marks no reference in front matter: it stays, with the byte order mark and the line endings
    assert_eq!(
        read(&f, "README.md"),
        text.replace(
            "É @docs/guide.md, ü @docs/guide.md",
            "É @docs/manual.md, ü @docs/manual.md"
        )
    );
}

#[test]
fn references_stay_as_written_when_the_prompt_features_are_off() {
    let f = fx(&[
        ("README.md", PROMPT_TEXT),
        ("docs/guide.md", ""),
        ("other.md", ""),
    ]);
    let plan = f
        .ops
        .plan_move(&[spec("docs/guide.md", "docs/manual.md")], true)
        .unwrap();
    assert_eq!(plan.link_count, 0);
    f.ops.apply_move(plan.plan_id, true).unwrap();
    assert_eq!(read(&f, "README.md"), PROMPT_TEXT);
}

/// Prompt blocks with no blank line after their tags: without the prompt features the lines from the first tag to the
/// blank line are one HTML block, and only the `href` after it is a destination.
const BLOCK_TEXT: &str = "\u{feff}<context>\r\n- see [guide](docs/guide.md) and `[code](docs/guide.md)`\r\n</context>\r\n<rules>\r\n<example>\r\n[g]: docs/guide.md\r\n\r\n<a href=\"docs/guide.md\">g</a>\r\n</example>\r\n</rules>\r\n";

#[test]
fn links_inside_prompt_blocks_follow_a_moved_file_and_undo_is_exact() {
    let f = fx_prompts(&[("README.md", BLOCK_TEXT), ("docs/guide.md", "# Guide\n")]);
    let plan = f
        .ops
        .plan_move(&[spec("docs/guide.md", "docs/manual.md")], true)
        .unwrap();
    let lines: Vec<u32> = plan.files[0].changes.iter().map(|c| c.line).collect();
    assert_eq!(lines, [2, 6, 8]);
    f.ops.apply_move(plan.plan_id, true).unwrap();
    // the code span, the tags, the byte order mark and the line endings stay
    assert_eq!(
        read(&f, "README.md"),
        BLOCK_TEXT
            .replacen("[guide](docs/guide.md)", "[guide](docs/manual.md)", 1)
            .replacen("[g]: docs/guide.md", "[g]: docs/manual.md", 1)
            .replacen("href=\"docs/guide.md\"", "href=\"docs/manual.md\"", 1)
    );
    f.ops.undo().unwrap().unwrap();
    assert_eq!(read(&f, "README.md"), BLOCK_TEXT, "byte for byte");
}

#[test]
fn links_inside_prompt_blocks_are_read_as_before_when_the_prompt_features_are_off() {
    let f = fx(&[("README.md", BLOCK_TEXT), ("docs/guide.md", "# Guide\n")]);
    let plan = f
        .ops
        .plan_move(&[spec("docs/guide.md", "docs/manual.md")], true)
        .unwrap();
    let lines: Vec<u32> = plan.files[0].changes.iter().map(|c| c.line).collect();
    assert_eq!(lines, [8]);
}

/// Definitions with paragraph text right after them (no blank line between): the parser takes them off the
/// paragraph, so their lines are read apart from it.
const DEFINITIONS_TEXT: &str = "[guide]: docs/guide.md\r\n[home]: README.md\r\nSee the [guide] or [the manual](docs/guide.md) and `[c](docs/guide.md)`.\r\n\r\n> [q]: <docs/guide.md> \"Quoted\"\r\n> quoted [guide][q]\r\n";

#[test]
fn a_definition_right_before_paragraph_text_follows_a_renamed_file_and_undo_is_exact() {
    let f = fx(&[
        ("README.md", DEFINITIONS_TEXT),
        ("docs/guide.md", "# Guide\n"),
    ]);
    let plan = f
        .ops
        .plan_move(&[spec("docs/guide.md", "docs/manual.md")], true)
        .unwrap();
    let lines: Vec<u32> = plan.files[0].changes.iter().map(|c| c.line).collect();
    assert_eq!(lines, [1, 3, 5]);
    f.ops.apply_move(plan.plan_id, true).unwrap();
    // the code span, the titles and the line endings stay
    assert_eq!(
        read(&f, "README.md"),
        DEFINITIONS_TEXT
            .replacen("[guide]: docs/guide.md", "[guide]: docs/manual.md", 1)
            .replacen("(docs/guide.md)", "(docs/manual.md)", 1)
            .replacen("<docs/guide.md>", "<docs/manual.md>", 1)
    );
    f.ops.undo().unwrap().unwrap();
    assert_eq!(read(&f, "README.md"), DEFINITIONS_TEXT, "byte for byte");
}

#[test]
fn undo_reports_what_changed_since_instead_of_overwriting_it() {
    let f = fx(&[("a.md", "[b](b.md)\n"), ("b.md", "")]);
    mv(&f, &[("b.md", "c.md")]);
    fs::write(f.dir.path().join("a.md"), "[b](c.md) plus an agent edit\n").unwrap();
    let outcome = f.ops.undo().unwrap().unwrap();
    assert_eq!(
        outcome.skipped,
        [Skip {
            path: "a.md".to_owned(),
            reason: SkipReason::Changed
        }]
    );
    assert_eq!(read(&f, "a.md"), "[b](c.md) plus an agent edit\n");
    assert!(exists(&f, "b.md"), "the file still went back");
}

#[test]
fn undo_does_not_put_a_file_where_another_one_has_appeared() {
    let f = fx(&[("a.md", "[b](b.md)\n"), ("b.md", "B")]);
    mv(&f, &[("b.md", "c.md")]);
    fs::write(f.dir.path().join("b.md"), "squatter").unwrap();
    let outcome = f.ops.undo().unwrap().unwrap();
    assert_eq!(outcome.restored, 0);
    assert_eq!(read(&f, "b.md"), "squatter");
    assert_eq!(read(&f, "c.md"), "B");
    assert_eq!(
        read(&f, "a.md"),
        "[b](c.md)\n",
        "links stay as the move left them when the file could not go back"
    );
    assert!(outcome
        .skipped
        .iter()
        .any(|s| s.reason == SkipReason::Occupied));
}

#[test]
fn undo_puts_back_the_links_of_the_files_that_go_back_and_only_those() {
    let f = fx(&[
        ("a.md", "[b](b.md) [c](c.md)\n"),
        ("b.md", ""),
        ("c.md", ""),
    ]);
    mv(&f, &[("b.md", "nb.md"), ("c.md", "nc.md")]);
    assert_eq!(read(&f, "a.md"), "[b](nb.md) [c](nc.md)\n");
    // one of the two places is taken meanwhile
    fs::write(f.dir.path().join("c.md"), "squatter").unwrap();
    let outcome = f.ops.undo().unwrap().unwrap();
    assert_eq!(outcome.restored, 1);
    // all-or-nothing per document: a.md depends on a move that did not go back
    assert!(
        outcome
            .skipped
            .iter()
            .any(|s| s.path == "a.md" && s.reason == SkipReason::Occupied),
        "{:?}",
        outcome.skipped
    );
    assert!(exists(&f, "b.md") && exists(&f, "nc.md"));
}

#[test]
fn undo_of_new_things_removes_only_what_is_still_empty() {
    let f = fx(&[("docs/a.md", "")]);
    f.ops.create("docs", "empty", NewKind::File).unwrap();
    f.ops.create("docs", "folder", NewKind::Folder).unwrap();
    f.ops.create("docs", "written", NewKind::File).unwrap();
    fs::write(f.dir.path().join("docs/written.md"), "words").unwrap();
    // newest first: the file with words stays, and says so
    let outcome = f.ops.undo().unwrap().unwrap();
    assert_eq!(outcome.kind, UndoKind::NewFile);
    assert_eq!(outcome.restored, 0);
    assert!(exists(&f, "docs/written.md"));
    let outcome = f.ops.undo().unwrap().unwrap();
    assert_eq!((outcome.kind, outcome.restored), (UndoKind::NewFolder, 1));
    assert!(!exists(&f, "docs/folder"));
    let outcome = f.ops.undo().unwrap().unwrap();
    assert_eq!((outcome.kind, outcome.restored), (UndoKind::NewFile, 1));
    assert!(!exists(&f, "docs/empty.md"));
}

#[test]
fn the_journal_keeps_twenty_and_survives_a_restart() {
    let f = fx(&[("start/", "")]);
    for n in 0..25 {
        f.ops
            .create("start", &format!("f{n}"), NewKind::Folder)
            .unwrap();
    }
    assert_eq!(f.ops.undo_depth(), 20);
    // a second service on the same state sees the same journal
    let root = Root::open(f.dir.path()).unwrap();
    let again = FileOps::new(
        Vault::new(root.clone()),
        Arc::clone(&f.explorer),
        Arc::clone(&f.own),
        &StateStore::for_project(&AppDirs::under(f.state.path()), &root),
    );
    assert_eq!(again.undo_depth(), 20);
    // what makes the journal readable again: the install key kept in the install folder
    assert!(f.state.path().join("install/install.key").is_file());
    let undone = again.undo().unwrap().unwrap();
    assert_eq!(undone.kind, UndoKind::NewFolder);
    assert!(!exists(&f, "start/f24"));
}

/// A second service on the same project and state folders: the app started again.
fn restarted(f: &Fx) -> FileOps {
    let root = Root::open(f.dir.path()).unwrap();
    FileOps::new(
        Vault::new(root.clone()),
        Arc::clone(&f.explorer),
        Arc::clone(&f.own),
        &StateStore::for_project(&AppDirs::under(f.state.path()), &root),
    )
}

#[test]
fn every_kind_of_operation_is_undone_after_a_restart() {
    let f = fx(&[
        ("a.md", "[b](b.md)\n"),
        ("b.md", "b"),
        ("docs/", ""),
        ("gone.md", "bye"),
    ]);
    mv(&f, &[("b.md", "docs/b.md")]);
    f.ops.create("docs", "new", NewKind::File).unwrap();
    f.ops.create("docs", "folder", NewKind::Folder).unwrap();
    let deleted = f.ops.delete(&["gone.md".to_owned()]).is_ok();
    let again = restarted(&f);
    assert_eq!(again.undo_depth(), if deleted { 4 } else { 3 });
    if deleted {
        let out = again.undo().unwrap().unwrap();
        assert_eq!(out.kind, UndoKind::Delete);
        if out.restored == 1 {
            assert_eq!(read(&f, "gone.md"), "bye");
        }
    }
    let out = again.undo().unwrap().unwrap();
    assert_eq!((out.kind, out.restored), (UndoKind::NewFolder, 1));
    assert!(!exists(&f, "docs/folder"));
    let out = again.undo().unwrap().unwrap();
    assert_eq!((out.kind, out.restored), (UndoKind::NewFile, 1));
    assert!(!exists(&f, "docs/new.md"));
    let out = again.undo().unwrap().unwrap();
    assert_eq!((out.kind, out.restored), (UndoKind::Move, 1));
    assert_eq!(read(&f, "b.md"), "b");
    assert_eq!(read(&f, "a.md"), "[b](b.md)\n");
    assert!(again.undo().unwrap().is_none());
}

#[test]
fn a_journal_kept_in_the_profile_still_undoes_after_initialize() {
    let f = fx(&[("docs/", "")]);
    f.ops.create("docs", "made", NewKind::Folder).unwrap();
    let root = Root::open(f.dir.path()).unwrap();
    let dirs = AppDirs::under(f.state.path());
    assert_eq!(
        StateStore::for_project(&dirs, &root).location(),
        crate::state::StateLocation::Profile
    );
    let store = crate::state::init_project(&dirs, &root).unwrap();
    assert_eq!(store.location(), crate::state::StateLocation::Project);
    f.ops.use_store(&store);
    assert_eq!(f.ops.undo_depth(), 1);
    let out = f.ops.undo().unwrap().unwrap();
    assert_eq!((out.kind, out.restored), (UndoKind::NewFolder, 1));
    assert!(!exists(&f, "docs/made"));
}

/// A journal that came with the folder (a zip, a clone, a sync partner): written where PerMark kept its journal.
fn plant_journal(f: &Fx, json: &str) {
    let root = Root::open(f.dir.path()).unwrap();
    let store = StateStore::for_project(&AppDirs::under(f.state.path()), &root);
    let path = store.dir().join("fileops").join("undo.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, json).unwrap();
}

/// Undoes until nothing is left (or the journal refuses).
fn undo_all(ops: &FileOps) {
    for _ in 0..25 {
        if !matches!(ops.undo(), Ok(Some(_))) {
            break;
        }
    }
}

#[test]
fn a_journal_that_came_with_the_folder_never_moves_a_file_out() {
    let f = fx(&[("a.md", "mine")]);
    // stands in for a folder whose files run at sign-in
    let startup = tempfile::tempdir().unwrap();
    let evil = startup.path().join("evil.md");
    let from = serde_json::to_string(&evil.display().to_string()).unwrap();
    plant_journal(
        &f,
        &format!(
            r#"{{"version":1,"data":{{"next":2,"entries":[{{"id":1,"at_ms":1,"op":{{"op":"import","copied":[],"moved":[{{"from":{from},"to":"a.md"}}]}}}}]}}}}"#
        ),
    );
    let again = restarted(&f);
    undo_all(&again);
    assert_eq!(read(&f, "a.md"), "mine");
    assert!(!evil.exists());
    // the journal of the earlier version is kept aside under another name, never read
    let root = Root::open(f.dir.path()).unwrap();
    let folder = StateStore::for_project(&AppDirs::under(f.state.path()), &root)
        .dir()
        .join("fileops");
    assert!(!folder.join("undo.json").exists());
    assert!(fs::read_to_string(folder.join("undo.json.pre-1.0"))
        .unwrap()
        .contains("evil.md"));
}

#[test]
fn the_journal_of_an_earlier_version_is_never_renamed_over_one_kept_before() {
    let f = fx(&[("a.md", "mine")]);
    plant_journal(&f, "older");
    drop(restarted(&f));
    plant_journal(&f, "newer");
    drop(restarted(&f));
    let root = Root::open(f.dir.path()).unwrap();
    let folder = StateStore::for_project(&AppDirs::under(f.state.path()), &root)
        .dir()
        .join("fileops");
    assert_eq!(read_text(&folder.join("undo.json")), "newer");
    assert_eq!(read_text(&folder.join("undo.json.pre-1.0")), "older");
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// This install's journal of the fixture's project, and the project's own install data (key + folder id).
fn journal_of(
    f: &Fx,
) -> (
    crate::state::StateFile<undo::Journal>,
    crate::state::Machine,
) {
    let root = Root::open(f.dir.path()).unwrap();
    let store = StateStore::for_project(&AppDirs::under(f.state.path()), &root);
    (
        store.machine_file(undo::JOURNAL_FILE).unwrap(),
        store.machine().unwrap().clone(),
    )
}

/// Puts `op` in place of the newest entry's operation (its id, so its mark, stays), sealed under `machine` (`None`:
/// unsealed).
fn swap_newest(f: &Fx, op: Op, machine: Option<&crate::state::Machine>) {
    let (file, _) = journal_of(f);
    let mut journal = file.load().into_value();
    let entry = journal.entries.last_mut().unwrap();
    entry.op = op;
    let seal = machine.map(|m| entry.seal_with(m)).unwrap_or_default();
    entry.seal = seal;
    file.save(&journal).unwrap();
}

fn import_op(copied: &[&str], moved: &[(&str, &str)]) -> Op {
    Op::Import {
        copied: copied.iter().map(|p| (*p).to_owned()).collect(),
        moved: moved
            .iter()
            .map(|(from, to)| undo::ImportMoved {
                from: (*from).to_owned(),
                to: (*to).to_owned(),
            })
            .collect(),
    }
}

#[test]
fn an_entry_this_install_did_not_seal_for_this_folder_or_with_a_bad_path_never_moves_a_file() {
    let f = fx(&[
        ("a.md", "mine"),
        (".git/HEAD", "ref"),
        ("sub/.permark/x.json", "{}"),
        ("sub/.permark/empty/", ""),
        ("docs/", ""),
    ]);
    // stands in for a folder whose files run at sign-in
    let startup = tempfile::tempdir().unwrap();
    let evil = startup.path().join("evil.md").display().to_string();
    f.ops.create("docs", "made", NewKind::Folder).unwrap();
    let (_, mine) = journal_of(&f);
    let other_state = tempfile::tempdir().unwrap();
    let root = Root::open(f.dir.path()).unwrap();
    let other_store = StateStore::for_project(&AppDirs::under(other_state.path()), &root);
    let other = other_store.machine().unwrap();
    let out = import_op(&[], &[(&evil, "a.md")]);
    let cases: Vec<(&str, Op, Option<&crate::state::Machine>)> = vec![
        ("unsealed", out.clone(), None),
        ("sealed by another install", out, Some(other)),
        (
            "a repository folder",
            import_op(&[".git"], &[]),
            Some(&mine),
        ),
        (
            "a member's state",
            import_op(&["sub/.permark/x.json"], &[]),
            Some(&mine),
        ),
        (
            "a member's state folder",
            Op::NewFolder {
                path: "sub/.permark/empty".to_owned(),
            },
            Some(&mine),
        ),
        (
            "a link put back into a state folder",
            Op::Move {
                moves: vec![("a.md".to_owned(), "docs/a.md".to_owned())],
                docs: vec![DocUndo {
                    path: "sub/.permark/x.json".to_owned(),
                    original: "sub/.permark/x.json".to_owned(),
                    hash_after: hash(b"{}").hex(),
                    edits: vec![UndoEdit {
                        start: 0,
                        end: 2,
                        old: "pwned".to_owned(),
                    }],
                    causes: Vec::new(),
                    moved_by: None,
                }],
            },
            Some(&mine),
        ),
    ];
    for (case, op, machine) in cases {
        swap_newest(&f, op, machine);
        assert_eq!(f.ops.undo_depth(), 0, "{case}");
        assert!(f.ops.undo_entry(1).unwrap().is_none(), "{case}");
        assert!(f.ops.undo().unwrap().is_none(), "{case}");
        assert!(restarted(&f).undo().unwrap().is_none(), "{case}");
        assert_eq!(read(&f, "a.md"), "mine", "{case}");
        assert!(!Path::new(&evil).exists(), "{case}");
        assert_eq!(read(&f, ".git/HEAD"), "ref", "{case}");
        assert_eq!(read(&f, "sub/.permark/x.json"), "{}", "{case}");
        assert!(exists(&f, "sub/.permark/empty"), "{case}");
    }
    // the same entry as this install wrote it is still taken back: its mark was live all along
    swap_newest(
        &f,
        Op::NewFolder {
            path: "docs/made".to_owned(),
        },
        Some(&mine),
    );
    let out = f.ops.undo().unwrap().unwrap();
    assert_eq!((out.kind, out.restored), (UndoKind::NewFolder, 1));
    assert!(!exists(&f, "docs/made"));
}

#[test]
fn a_step_taken_back_once_never_comes_back_with_an_older_copy_of_the_journal() {
    let f = fx(&[("docs/", "")]);
    f.ops.create("docs", "made", NewKind::Folder).unwrap();
    let (file, _) = journal_of(&f);
    let before = fs::read(file.path()).unwrap();
    assert!(f.ops.undo().unwrap().is_some());
    // made again by hand, then the older journal comes back (a sync partner, a restore)
    fs::create_dir(f.dir.path().join("docs/made")).unwrap();
    fs::write(file.path(), &before).unwrap();
    assert_eq!(f.ops.undo_depth(), 0);
    assert!(f.ops.undo().unwrap().is_none());
    assert!(f.ops.undo_entry(1).unwrap().is_none());
    assert!(restarted(&f).undo().unwrap().is_none());
    assert!(exists(&f, "docs/made"));
}

#[test]
fn an_import_is_put_back_only_on_a_local_drive_outside_the_project_and_the_state_folders() {
    let f = fx(&[("a.md", "mine"), ("docs/", "")]);
    let dirs = AppDirs::under(f.state.path());
    let (_, mine) = journal_of(&f);
    let inside = f.dir.path().join("back.md").display().to_string();
    let install = dirs.install.join("a.md").display().to_string();
    let profile = dirs.profile.join("a.md").display().to_string();
    let refused = [
        "//127.0.0.1/share/a.md",
        r"\\127.0.0.1\share\a.md",
        r"\\?\C:\a.md",
        r"\\.\C:\a.md",
        "out/a.md",
        &inside,
        &install,
        &profile,
    ];
    for (n, from) in refused.iter().enumerate() {
        f.ops
            .create("docs", &format!("m{n}"), NewKind::Folder)
            .unwrap();
        swap_newest(&f, import_op(&[], &[(from, "a.md")]), Some(&mine));
        let out = f.ops.undo().unwrap().unwrap();
        assert_eq!(out.kind, UndoKind::Import, "{from}");
        assert_eq!(out.restored, 0, "{from}");
        assert_eq!(
            out.skipped,
            [Skip {
                path: "a.md".to_owned(),
                reason: SkipReason::Missing
            }],
            "{from}"
        );
        assert_eq!(read(&f, "a.md"), "mine", "{from}");
    }
    assert!(!Path::new(&inside).exists() && !Path::new(&install).exists());
    // a place outside all of them: put back
    let outside = tempfile::tempdir().unwrap();
    let back = outside.path().join("a.md");
    f.ops.create("docs", "last", NewKind::Folder).unwrap();
    let from = back.display().to_string();
    swap_newest(&f, import_op(&[], &[(&from, "a.md")]), Some(&mine));
    let out = f.ops.undo().unwrap().unwrap();
    assert_eq!((out.restored, out.skipped.len()), (1, 0));
    assert_eq!(read_text(&back), "mine");
}

#[test]
fn ids_go_on_after_the_journal_is_lost() {
    let f = fx(&[("docs/", "")]);
    f.ops.create("docs", "one", NewKind::Folder).unwrap();
    f.ops.create("docs", "two", NewKind::Folder).unwrap();
    let (file, _) = journal_of(&f);
    fs::remove_file(file.path()).unwrap();
    assert_eq!(f.ops.undo_depth(), 0);
    f.ops.create("docs", "three", NewKind::Folder).unwrap();
    let journal = file.load().into_value();
    assert_eq!(
        journal.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
        [3]
    );
    assert!(f.ops.undo_entry(1).unwrap().is_none());
    assert!(f.ops.undo_entry(3).unwrap().is_some());
    assert!(!exists(&f, "docs/three"));
}

#[test]
fn a_next_id_written_into_the_journal_file_never_overflows_nor_gives_an_id_twice() {
    let f = fx(&[("docs/", "")]);
    f.ops.create("docs", "one", NewKind::Folder).unwrap();
    // the journal file sits in the project's state folder, where a sync partner can write it
    let (file, _) = journal_of(&f);
    let mut journal = file.load().into_value();
    journal.next = u64::MAX;
    file.save(&journal).unwrap();
    f.ops.create("docs", "two", NewKind::Folder).unwrap();
    f.ops.create("docs", "three", NewKind::Folder).unwrap();
    let ids: Vec<u64> = file
        .load()
        .into_value()
        .entries
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(ids, [1, 2, 3]);
    for name in ["three", "two", "one"] {
        let out = f.ops.undo().unwrap().unwrap();
        assert_eq!((out.kind, out.restored), (UndoKind::NewFolder, 1), "{name}");
        assert!(!exists(&f, &format!("docs/{name}")), "{name}");
    }
}

#[test]
fn an_undo_never_acts_through_a_folder_link_into_a_repository_or_state_folder() {
    let f = fx(&[
        ("a.md", "mine"),
        ("notes/", ""),
        ("docs/", ""),
        (".git/x/a.md", "repository data"),
        ("sub/.permark/y/made.md", ""),
    ]);
    mv(&f, &[("a.md", "notes/a.md")]);
    f.ops.create("docs", "made", NewKind::File).unwrap();
    // each folder swapped for a link into a protected folder holding the same names
    let root = f.dir.path();
    for (folder, target) in [
        ("notes", root.join(".git").join("x")),
        ("docs", root.join("sub").join(".permark").join("y")),
    ] {
        fs::rename(root.join(folder), root.join(format!("{folder}-was"))).unwrap();
        if !crate::tree::tests::link_out(&root.join(folder), &target) {
            eprintln!("links could not be made here; skipped");
            return;
        }
    }
    let out = f.ops.undo().unwrap().unwrap();
    assert_eq!((out.kind, out.restored), (UndoKind::NewFile, 0));
    let out = f.ops.undo().unwrap().unwrap();
    assert_eq!((out.kind, out.restored), (UndoKind::Move, 0));
    assert_eq!(read(&f, ".git/x/a.md"), "repository data");
    assert!(exists(&f, "sub/.permark/y/made.md"));
    assert!(!exists(&f, "a.md"));
}

#[test]
fn without_a_usable_install_key_operations_work_and_nothing_is_journaled() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();
    let state = tempfile::tempdir().unwrap();
    // a file where the install folder should be: no key can be kept
    let dirs = AppDirs {
        install: state.path().join("install"),
        ..AppDirs::under(state.path())
    };
    fs::write(&dirs.install, "in the way").unwrap();
    let root = Root::open(dir.path()).unwrap();
    let vault = Vault::new(root.clone());
    let explorer = Arc::new(Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    ));
    let store = StateStore::for_project(&dirs, &root);
    let ops = FileOps::new(vault, explorer, Arc::new(OwnWrites::new()), &store);
    ops.create("docs", "made", NewKind::Folder).unwrap();
    assert!(dir.path().join("docs/made").is_dir());
    assert_eq!(ops.undo_depth(), 0);
    assert!(ops.undo().unwrap().is_none());
    assert!(!store.dir().join("machine").exists());
}

#[test]
fn an_undo_never_changes_a_root_a_repository_folder_or_any_state_folder() {
    let undoable = |path: &str| ops::undoable(&VaultPath::parse(path).unwrap());
    for path in [
        "a.md",
        "docs/a.md",
        "@wt/n/a.md",
        "permark/x",
        "sub/.permarks/x",
    ] {
        assert!(undoable(path), "{path}");
    }
    for path in [
        "",
        "@wt/n",
        ".git",
        "sub/.git/config",
        ".permark",
        ".permark/local/x",
        "sub/.permark",
        "sub/.permark/x",
        "@wt/n/.permark/x",
        "@wt/n/sub/.permark/x",
    ] {
        assert!(!undoable(path), "{path}");
    }
}

#[test]
fn undo_of_delete_restores_from_the_bin_where_the_os_allows() {
    let f = fx(&[("gone.md", "bye"), ("stay.md", "")]);
    if f.ops.delete(&["gone.md".to_owned()]).is_err() {
        eprintln!("recycle bin not usable here; skipped");
        return;
    }
    assert!(!exists(&f, "gone.md"));
    let outcome = f.ops.undo().unwrap().unwrap();
    assert_eq!(outcome.kind, UndoKind::Delete);
    if outcome.restored == 1 {
        assert_eq!(read(&f, "gone.md"), "bye");
    } else {
        // macOS (no restore API) or a bin that cannot be listed: said, not hidden
        assert_eq!(outcome.skipped.len(), 1);
    }
}

// ---- adversarial ----------------------------------------------------------------------------------------------------------

#[test]
fn unicode_spaces_and_cjk_names_move_and_keep_their_links() {
    let f = fx(&[
        ("日本語/ノート 1.md", "[b](../b%20c.md) [自](自分.md)\n"),
        ("日本語/自分.md", ""),
        ("b c.md", ""),
    ]);
    mv(&f, &[("日本語", "日本/語 folder")]);
    assert_eq!(
        read(&f, "日本/語 folder/ノート 1.md"),
        "[b](../../b%20c.md) [自](自分.md)\n"
    );
}

#[test]
fn a_concurrent_external_edit_during_apply_never_loses_the_edit() {
    let f = fx(&[("a.md", "[b](b.md)\n"), ("b.md", "")]);
    let plan = f.ops.plan_move(&[spec("b.md", "c.md")], true).unwrap();
    let doc = f.dir.path().join("a.md");
    // the external writer runs right up to the apply
    let writer = std::thread::spawn(move || {
        for n in 0..30 {
            fs::write(&doc, format!("[b](b.md) edit {n}\n")).unwrap();
        }
    });
    writer.join().unwrap();
    let outcome = f.ops.apply_move(plan.plan_id, true).unwrap();
    let text = read(&f, "a.md");
    assert!(
        text.contains("edit 29"),
        "the external version is what is on disk: {text}"
    );
    assert_eq!(outcome.skipped.len(), 1);
}

#[test]
fn hostile_documents_do_not_break_a_plan() {
    let f = fx(&[
        ("a.md", &"[a](".repeat(3000)),
        ("b.md", &format!("[x](b.md){}", "<a href=\"".repeat(1000))),
        ("big.md", &"[x](target.md)\n".repeat(30_000)),
        ("target.md", ""),
        ("moved/", ""),
    ]);
    let plan = f
        .ops
        .plan_move(&[spec("target.md", "moved/target.md")], true)
        .unwrap();
    assert!(plan.link_count >= 30_000);
    assert!(plan.truncated, "the preview is cut off, the plan is not");
    let outcome = f.ops.apply_move(plan.plan_id, true).unwrap();
    assert!(outcome.links >= 30_000);
    assert!(read(&f, "big.md").starts_with("[x](moved/target.md)\n"));
}

#[test]
fn documents_over_the_size_limit_are_reported_not_read() {
    let f = fx(&[("b.md", "")]);
    let big = f.dir.path().join("huge.md");
    fs::write(
        &big,
        vec![b'a'; usize::try_from(LINK_SCAN_LIMIT).unwrap() + 1],
    )
    .unwrap();
    f.explorer.sync_paths(&["huge.md".to_owned()], &[]);
    let plan = f.ops.plan_move(&[spec("b.md", "c.md")], true).unwrap();
    assert!(plan
        .skipped
        .iter()
        .any(|s| s.path == "huge.md" && s.reason == SkipReason::TooLarge));
}

#[test]
fn links_in_worktree_documents_stay_inside_the_worktree() {
    let main = tempfile::tempdir().unwrap();
    let wt = tempfile::tempdir().unwrap();
    fs::write(main.path().join("a.md"), "[b](b.md)\n").unwrap();
    fs::write(main.path().join("b.md"), "").unwrap();
    fs::write(wt.path().join("a.md"), "[b](b.md)\n").unwrap();
    fs::write(wt.path().join("b.md"), "").unwrap();
    let state = tempfile::tempdir().unwrap();
    let root = Root::open(main.path()).unwrap();
    let vault =
        Vault::new(root.clone()).with_worktrees([("w".to_owned(), wt.path().to_path_buf())]);
    let explorer = Arc::new(Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    ));
    explorer.open_view("w", None);
    let ops = FileOps::new(
        vault,
        explorer,
        Arc::new(OwnWrites::new()),
        &StateStore::for_project(&AppDirs::under(state.path()), &root),
    );
    let plan = ops
        .plan_move(&[spec("@wt/w/b.md", "@wt/w/c.md")], true)
        .unwrap();
    ops.apply_move(plan.plan_id, true).unwrap();
    assert_eq!(
        fs::read_to_string(wt.path().join("a.md")).unwrap(),
        "[b](c.md)\n"
    );
    assert_eq!(
        fs::read_to_string(main.path().join("a.md")).unwrap(),
        "[b](b.md)\n",
        "the main folder is another root"
    );
    let across = ops
        .plan_move(&[spec("a.md", "@wt/w/a2.md")], true)
        .map(|_| ())
        .map_err(kind_of);
    assert_eq!(across, Err(ErrorKind::OtherRoot));
}

// ---- documents the tree does not list -------------------------------------------------------------------------------------

#[test]
fn the_plan_reaches_documents_in_folders_the_tree_leaves_unlisted_for_their_size() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("docs/target.md"), "# T\n").unwrap();
    let big = dir.path().join("big");
    fs::create_dir_all(&big).unwrap();
    for n in 0..(crate::tree::LARGE_FOLDER + 5) {
        fs::write(big.join(format!("n{n}.md")), "x").unwrap();
    }
    fs::write(big.join("links.md"), "[to](../docs/target.md)\n").unwrap();
    let fx = fx_in(dir);
    // the tree does not list the folder
    assert!(fx.explorer.listed("big/links.md").is_none());
    let plan = fx
        .ops
        .plan_move(&[spec("docs/target.md", "docs/renamed.md")], true)
        .unwrap();
    assert_eq!(plan.link_count, 1, "{plan:?}");
    assert_eq!(plan.files[0].path, "big/links.md");
    fx.ops.apply_move(plan.plan_id, true).unwrap();
    assert_eq!(read(&fx, "big/links.md"), "[to](../docs/renamed.md)\n");
}

#[test]
fn the_plan_names_the_ignored_folders_it_did_not_look_into() {
    let fx = fx(&[
        (".gitignore", "build/\nout/\n"),
        ("docs/target.md", "# T\n"),
        ("docs/a.md", "[t](target.md)\n"),
        ("build/b.md", "[t](../docs/target.md)\n"),
        ("src/x.md", "x"),
    ]);
    let plan = fx
        .ops
        .plan_move(&[spec("docs/target.md", "docs/new.md")], true)
        .unwrap();
    assert_eq!(plan.link_count, 1, "the ignored document is not touched");
    assert_eq!(
        plan.unchecked,
        ["build"],
        "only folders that exist and are ignored"
    );
    let without = fx
        .ops
        .plan_move(&[spec("docs/target.md", "docs/new2.md")], false)
        .unwrap();
    assert!(
        without.unchecked.is_empty(),
        "no links planned, nothing to say"
    );
}

// ---- integrity: names that compare equal, mixed link spellings, saves that slip in ----------------------------------

#[test]
fn a_rename_that_only_changes_normalization_never_replaces_another_file() {
    let nfc = "caf\u{e9}.md";
    let nfd = "cafe\u{301}.md";
    let f = fx(&[(nfc, "mine")]);
    fs::write(f.dir.path().join(nfd), "theirs").unwrap();
    if fs::read_dir(f.dir.path()).unwrap().count() < 2 {
        // this file system folds normalization: both names are one file
        return;
    }
    assert_eq!(
        kind_of(f.ops.plan_move(&[spec(nfc, nfd)], true).unwrap_err()),
        ErrorKind::Exists
    );
    assert_eq!(read(&f, nfc), "mine");
    assert_eq!(read(&f, nfd), "theirs");
    // the other spelling appears between the preview and the apply
    fs::remove_file(f.dir.path().join(nfd)).unwrap();
    let plan = f.ops.plan_move(&[spec(nfc, nfd)], true).unwrap();
    fs::write(f.dir.path().join(nfd), "theirs again").unwrap();
    assert_eq!(
        kind_of(f.ops.apply_move(plan.plan_id, true).unwrap_err()),
        ErrorKind::Exists
    );
    assert_eq!(read(&f, nfc), "mine");
    assert_eq!(read(&f, nfd), "theirs again");
}

#[test]
fn links_spelled_with_mixed_escapes_are_found_and_rewritten() {
    let f = fx(&[
        (
            "a.md",
            "[p](My%20Notes%20(1).md) [e](My%20Notes%20\\(1\\).md)\n",
        ),
        ("b.md", "[r](Relat\u{f3}rio%20final.md)\n"),
        ("My Notes (1).md", ""),
        ("Relat\u{f3}rio final.md", ""),
    ]);
    let outcome = mv(
        &f,
        &[
            ("My Notes (1).md", "Done (2).md"),
            ("Relat\u{f3}rio final.md", "Relat\u{f3}rio 2.md"),
        ],
    );
    assert_eq!(outcome.links, 3);
    assert_eq!(
        read(&f, "a.md"),
        "[p](Done%20%282%29.md) [e](Done%20%282%29.md)\n"
    );
    assert_eq!(read(&f, "b.md"), "[r](Relat\u{f3}rio%202.md)\n");
}

#[test]
fn the_candidate_filter_matches_any_normalization_and_escape_of_a_moved_name() {
    let f = fx(&[("Relato\u{301}rio.md", "")]);
    let vault = Vault::new(Root::open(f.dir.path()).unwrap());
    let moves = [("Relato\u{301}rio.md".to_owned(), "x.md".to_owned())];
    let planner = Planner::new(&vault, &moves);
    for text in [
        "[r](Relat\u{f3}rio.md)",
        "[r](RELAT%C3%93RIO.md)",
        "[r](relato%CC%81rio.md)",
        "[r](<Relato\u{301}rio.md>)",
    ] {
        assert!(mentions(text, planner.needles()), "{text}");
    }
    assert!(!mentions("[r](other.md)", planner.needles()));
}

#[test]
fn a_case_change_the_drive_cannot_prove_is_one_item_is_refused_with_its_reason() {
    let error = IpcError::from(FileOpError::MaybeSame("Plan.md".to_owned()));
    assert_eq!(error.kind, ErrorKind::Exists);
    assert_eq!(error.subject.as_deref(), Some("Plan.md"));
    assert_eq!(error.detail.as_deref(), Some(IDENTITY_UNKNOWN));
}

#[test]
fn the_candidate_filter_matches_plain_ascii_links_in_any_case() {
    let f = fx(&[("docs/Plan.md", "")]);
    let vault = Vault::new(Root::open(f.dir.path()).unwrap());
    let moves = [("docs/Plan.md".to_owned(), "docs/x.md".to_owned())];
    let planner = Planner::new(&vault, &moves);
    for text in ["[p](docs/Plan.md)", "[p](PLAN.md)", "see [[plan]]"] {
        assert!(mentions(text, planner.needles()), "{text}");
    }
    assert!(!mentions("[p](docs/other.md)", planner.needles()));
}

/// Another program saves `text` to `path` after an atomic write of `path` made its temp and before that write replaces
/// the file. Windows: the program holds `path` without delete sharing (a rename over it waits), writes through its
/// handle as soon as the temp shows up, and lets go a moment later. Elsewhere nothing outside holds a rename back: the
/// write's own seam saves at that point. The returned call waits until the save landed.
#[cfg(windows)]
fn slip_in_save(path: &Path, text: &'static str) -> impl FnOnce() {
    let writer = hold_and_save(path, text);
    move || writer.join().unwrap()
}

#[cfg(not(windows))]
fn slip_in_save(path: &Path, text: &'static str) -> impl FnOnce() {
    let saved = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let landed = Arc::clone(&saved);
    let target = path.to_path_buf();
    crate::fsx::slip::before_check(path, move || {
        fs::write(&target, text).unwrap();
        landed.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    move || {
        assert!(
            saved.load(std::sync::atomic::Ordering::SeqCst),
            "the save landed"
        );
    }
}

#[cfg(windows)]
fn hold_and_save(path: &Path, text: &'static str) -> std::thread::JoinHandle<()> {
    use std::io::Write as _;
    use std::os::windows::fs::OpenOptionsExt;
    const SHARE_READ_WRITE: u32 = 0x1 | 0x2;
    let mut held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(SHARE_READ_WRITE)
        .open(path)
        .unwrap();
    let dir = path.parent().unwrap().to_path_buf();
    let prefix = format!(".{}.", path.file_name().unwrap().to_string_lossy());
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(10) {
            let temp = fs::read_dir(&dir).unwrap().flatten().any(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with(&prefix) && crate::fsx::is_scratch_name(&name)
            });
            if temp {
                break;
            }
            std::thread::yield_now();
        }
        held.set_len(0).unwrap();
        held.write_all(text.as_bytes()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(held);
    })
}

#[test]
fn a_link_rewrite_never_replaces_a_save_that_slips_in_before_its_write() {
    let f = fx(&[
        ("a.md", "[b](b.md)\n"),
        ("c.md", "[b](b.md)\n"),
        ("b.md", ""),
    ]);
    let plan = f.ops.plan_move(&[spec("b.md", "moved.md")], true).unwrap();
    let landed = slip_in_save(&f.dir.path().join("a.md"), "their save\n");
    let outcome = f.ops.apply_move(plan.plan_id, true).unwrap();
    landed();
    assert_eq!(read(&f, "a.md"), "their save\n");
    assert_eq!(
        outcome.skipped,
        [Skip {
            path: "a.md".to_owned(),
            reason: SkipReason::Changed
        }]
    );
    assert_eq!(read(&f, "c.md"), "[b](moved.md)\n");
}

#[test]
fn an_undo_never_replaces_a_save_that_slips_in_before_its_write() {
    let f = fx(&[("a.md", "[b](b.md)\n"), ("b.md", "")]);
    mv(&f, &[("b.md", "c.md")]);
    let landed = slip_in_save(&f.dir.path().join("a.md"), "their save\n");
    let outcome = f.ops.undo().unwrap().unwrap();
    landed();
    assert_eq!(read(&f, "a.md"), "their save\n");
    assert!(
        outcome
            .skipped
            .iter()
            .any(|s| s.path == "a.md" && s.reason == SkipReason::Changed),
        "{:?}",
        outcome.skipped
    );
    assert!(exists(&f, "b.md"), "the file still went back");
}

// ---- a bin that stops part way --------------------------------------------------------------------------------------------

#[test]
fn a_delete_the_bin_stops_part_way_journals_what_went_so_undo_brings_it_back() {
    let f = fx(&[("a.md", "A"), ("b.md", "B")]);
    // the bin takes the first item, then refuses the next (the freedesktop bin stops at its first failure)
    let partly = |places: &[PathBuf]| -> Result<(), Unrecycled> {
        let taken = crate::fsx::recycle(&places[..1]).is_ok();
        Err(Unrecycled {
            reason: "refused".to_owned(),
            gone: vec![taken, false],
        })
    };
    let refused = f
        .ops
        .delete_with(&["a.md".to_owned(), "b.md".to_owned()], partly);
    if exists(&f, "a.md") {
        eprintln!("recycle bin not usable here; skipped");
        return;
    }
    assert_eq!(refused.map(|_| ()).map_err(kind_of), Err(ErrorKind::Trash));
    assert_eq!(read(&f, "b.md"), "B");
    let outcome = f.ops.undo().unwrap().expect("what went is journaled");
    assert_eq!(outcome.kind, UndoKind::Delete);
    if outcome.restored == 1 {
        assert_eq!(read(&f, "a.md"), "A");
        assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    } else {
        // macOS (no restore API) or a bin that cannot be listed: said, not hidden
        assert_eq!(
            outcome.skipped,
            [Skip {
                path: "a.md".to_owned(),
                reason: SkipReason::Missing
            }]
        );
    }
    assert_eq!(read(&f, "b.md"), "B");
}

/// The Windows bin cannot take a file held open without sharing; it goes on with the others and then fails.
#[cfg(windows)]
#[test]
fn the_items_the_bin_took_before_refusing_a_held_one_come_back_with_undo() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = fx(&[("a.md", "A"), ("held.md", "H"), ("c.md", "C")]);
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(f.dir.path().join("held.md"))
        .unwrap();
    let refused = f
        .ops
        .delete(&["a.md".to_owned(), "held.md".to_owned(), "c.md".to_owned()]);
    drop(held);
    if refused.is_ok() {
        eprintln!("the bin took a held file here; skipped");
        return;
    }
    let gone: Vec<(&str, &str)> = [("a.md", "A"), ("c.md", "C")]
        .into_iter()
        .filter(|(path, _)| !exists(&f, path))
        .collect();
    if gone.is_empty() {
        eprintln!("the bin took nothing here; skipped");
        return;
    }
    assert_eq!(read(&f, "held.md"), "H");
    let outcome = f.ops.undo().unwrap().expect("what went is journaled");
    assert_eq!(
        usize::try_from(outcome.restored).unwrap(),
        gone.len(),
        "{:?}",
        outcome.skipped
    );
    for (path, text) in gone {
        assert_eq!(read(&f, path), text);
    }
}

/// The freedesktop bin cannot take an item out of a read-only folder; it stops there, after the items before it.
/// (Not on macOS: the Finder may ask for a password there.)
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn the_items_the_bin_took_before_an_item_it_cannot_take_come_back_with_undo() {
    use std::os::unix::fs::PermissionsExt;
    let f = fx(&[("a.md", "A"), ("locked/b.md", "B")]);
    let locked = f.dir.path().join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    let refused = f.ops.delete(&["a.md".to_owned(), "locked/b.md".to_owned()]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if refused.is_ok() {
        eprintln!("the bin took an item of a read-only folder here; skipped");
        return;
    }
    if exists(&f, "a.md") {
        eprintln!("recycle bin not usable here; skipped");
        return;
    }
    assert_eq!(read(&f, "locked/b.md"), "B");
    let outcome = f.ops.undo().unwrap().expect("what went is journaled");
    assert_eq!(outcome.restored, 1, "{:?}", outcome.skipped);
    assert_eq!(read(&f, "a.md"), "A");
}

#[test]
fn undo_of_a_delete_brings_back_what_it_can_and_never_replaces_what_took_a_place() {
    let f = fx(&[("a.md", "A"), ("b.md", "B"), ("c.md", "C")]);
    if f.ops
        .delete(&["a.md".to_owned(), "b.md".to_owned(), "c.md".to_owned()])
        .is_err()
    {
        eprintln!("recycle bin not usable here; skipped");
        return;
    }
    fs::write(f.dir.path().join("b.md"), "new B").unwrap();
    let outcome = f.ops.undo().unwrap().unwrap();
    assert_eq!(read(&f, "b.md"), "new B");
    if !cfg!(windows) && outcome.restored == 0 {
        // macOS (no restore API) or a bin that cannot be listed: said, not hidden
        assert_eq!(outcome.skipped.len(), 3);
        return;
    }
    assert_eq!(outcome.restored, 2);
    assert_eq!(
        outcome.skipped,
        [Skip {
            path: "b.md".to_owned(),
            reason: SkipReason::Occupied
        }]
    );
    assert_eq!(
        (read(&f, "a.md"), read(&f, "c.md")),
        ("A".into(), "C".into())
    );
}

// ---- many items: the answers of the pairwise checks, in about the time of the count ----------------------------------------

mod many_items {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use proptest::prelude::*;

    use super::super::ops::{check_together, outermost, ValidMove};
    use super::super::plan::Mover;
    use super::super::FileOpError;
    use crate::paths::VaultPath;
    use crate::tree::key_of;

    /// Segments that meet in every way the checks tell apart: case, Unicode normalization, one name inside another.
    const SEGMENTS: &[&str] = &["a", "A", "b", "ab", "\u{e9}", "e\u{301}", "x y"];

    fn path() -> impl Strategy<Value = String> {
        proptest::collection::vec(proptest::sample::select(SEGMENTS), 1..4)
            .prop_map(|parts| parts.join("/"))
    }

    fn valid(from: &str, to: &str) -> ValidMove {
        ValidMove {
            from: VaultPath::parse(from).unwrap(),
            to: VaultPath::parse(to).unwrap(),
            abs_from: PathBuf::from(from),
            abs_to: PathBuf::from(to),
            is_dir: false,
            case_only: false,
        }
    }

    /// Every pair in order: the reference the keyed check answers like.
    fn pairwise_check_together(moves: &[ValidMove]) -> Result<(), FileOpError> {
        for (at, a) in moves.iter().enumerate() {
            for b in &moves[at + 1..] {
                if a.from.is_within(&b.from) || b.from.is_within(&a.from) {
                    return Err(FileOpError::Overlap(a.from.to_string()));
                }
                let (ta, tb) = (a.to.key().to_lowercase(), b.to.key().to_lowercase());
                if ta == tb
                    || ta.starts_with(&format!("{tb}/"))
                    || tb.starts_with(&format!("{ta}/"))
                {
                    return Err(FileOpError::Overlap(a.to.to_string()));
                }
            }
        }
        Ok(())
    }

    /// Every path against every other: the reference of the collapse of nested deletes.
    fn pairwise_outermost(checked: &[(VaultPath, PathBuf)]) -> Vec<(VaultPath, PathBuf)> {
        let keys: Vec<String> = checked.iter().map(|(p, _)| p.key()).collect();
        let mut kept = Vec::new();
        for (at, (path, abs)) in checked.iter().enumerate() {
            let covered = keys.iter().enumerate().any(|(other, key)| {
                other != at
                    && (path.key().starts_with(&format!("{key}/"))
                        || (path.key() == *key && other < at))
            });
            if !covered {
                kept.push((path.clone(), abs.clone()));
            }
        }
        kept
    }

    /// The longest `from` first (the first given among equals), compared segment by segment: the reference mover.
    fn linear_find(moves: &[(String, String)], path: &str) -> Option<(usize, String)> {
        let mut entries: Vec<(Vec<String>, &String, usize)> = moves
            .iter()
            .enumerate()
            .map(|(at, (from, to))| (from.split('/').map(key_of).collect(), to, at))
            .collect();
        entries.sort_by_key(|(from, _, _)| std::cmp::Reverse(from.len()));
        let parts: Vec<&str> = path.split('/').collect();
        entries.into_iter().find_map(|(from, to, at)| {
            (parts.len() >= from.len() && parts.iter().zip(&from).all(|(p, f)| key_of(p) == *f))
                .then(|| {
                    let mut out = to.clone();
                    for rest in &parts[from.len()..] {
                        out.push('/');
                        out.push_str(rest);
                    }
                    (at, out)
                })
        })
    }

    fn debug(result: Result<(), FileOpError>) -> Result<(), String> {
        result.map_err(|e| format!("{e:?}"))
    }

    proptest! {
        #[test]
        fn overlapping_moves_are_refused_naming_the_item_the_pairwise_check_named(
            pairs in proptest::collection::vec((path(), path()), 0..10),
        ) {
            let moves: Vec<ValidMove> = pairs.iter().map(|(from, to)| valid(from, to)).collect();
            prop_assert_eq!(debug(check_together(&moves)), debug(pairwise_check_together(&moves)));
        }

        #[test]
        fn nested_and_repeated_deletes_collapse_into_the_same_paths_in_the_same_order(
            paths in proptest::collection::vec(path(), 0..12),
        ) {
            let checked: Vec<(VaultPath, PathBuf)> = paths
                .iter()
                .enumerate()
                .map(|(at, p)| (VaultPath::parse(p).unwrap(), PathBuf::from(at.to_string())))
                .collect();
            prop_assert_eq!(outermost(checked.clone()), pairwise_outermost(&checked));
        }

        #[test]
        fn the_mover_takes_every_path_where_the_linear_search_took_it(
            moves in proptest::collection::vec((path(), path()), 0..8),
            paths in proptest::collection::vec(path(), 1..8),
        ) {
            let mover = Mover::new(&moves);
            for path in &paths {
                let expected = linear_find(&moves, path);
                prop_assert_eq!(mover.move_index_of(path), expected.as_ref().map(|(at, _)| *at));
                prop_assert_eq!(mover.map(path), expected.map(|(_, to)| to));
            }
        }
    }

    /// Files spread over shared folders, each moved to its own place.
    fn many(n: usize) -> Vec<(String, String)> {
        (0..n)
            .map(|i| {
                let folder = i % 40;
                (
                    format!("src/part{folder}/d{i}.md"),
                    format!("dst/part{folder}/d{i}.md"),
                )
            })
            .collect()
    }

    // debug builds: checking every pair took minutes for these
    const BOUND: Duration = Duration::from_secs(2);

    #[test]
    fn the_checks_of_thousands_of_moves_and_deletes_grow_with_the_count_not_its_square() {
        let moves: Vec<ValidMove> = many(5000).iter().map(|(a, b)| valid(a, b)).collect();
        let started = Instant::now();
        assert!(check_together(&moves).is_ok());
        let checked = moves
            .iter()
            .map(|m| (m.from.clone(), m.abs_from.clone()))
            .collect();
        assert_eq!(outermost(checked).len(), moves.len());
        let took = started.elapsed();
        assert!(took < BOUND, "{took:?}");
    }

    #[test]
    fn the_mover_of_thousands_of_moves_finds_each_place_without_a_search() {
        let pairs = many(5000);
        let mover = Mover::new(&pairs);
        let started = Instant::now();
        for (at, (from, to)) in pairs.iter().enumerate() {
            assert_eq!(mover.move_index_of(from), Some(at));
            assert_eq!(mover.map(&format!("{from}/x")), Some(format!("{to}/x")));
            assert_eq!(mover.map(&format!("other/{from}")), None);
        }
        let took = started.elapsed();
        assert!(took < BOUND, "{took:?}");
    }
}
