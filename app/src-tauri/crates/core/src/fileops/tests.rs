use std::fs;
use std::path::Path;
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
    let undone = again.undo().unwrap().unwrap();
    assert_eq!(undone.kind, UndoKind::NewFolder);
    assert!(!exists(&f, "start/f24"));
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
