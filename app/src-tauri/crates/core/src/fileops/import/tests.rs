use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::error::ErrorKind;
use crate::events::Recorder;
use crate::paths::{Root, Vault};
use crate::settings::ProjectConfig;
use crate::state::{AppDirs, StateStore};
use crate::tree::{Explorer, TreeFilter};
use crate::watch::{Change, ChangeKind, OwnWrites};

struct Fx {
    dir: tempfile::TempDir,
    outside: tempfile::TempDir,
    _state: tempfile::TempDir,
    ops: FileOps,
    own: Arc<OwnWrites>,
}

fn put(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let abs = root.join(path);
        if path.ends_with('/') {
            fs::create_dir_all(&abs).unwrap();
        } else {
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(abs, content).unwrap();
        }
    }
}

/// A project with `inside` files, and a folder outside it with `outside` files.
fn fx(inside: &[(&str, &str)], outside: &[(&str, &str)]) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    put(dir.path(), inside);
    put(out.path(), outside);
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
    let ops = FileOps::new(vault, explorer, Arc::clone(&own), &store);
    Fx {
        dir,
        outside: out,
        _state: state,
        ops,
        own,
    }
}

/// Absolute path (text) of something in the outside folder.
fn src(f: &Fx, rel: &str) -> String {
    f.outside.path().join(rel).display().to_string()
}

fn plan(f: &Fx, target: &str, sources: &[&str]) -> ImportPlan {
    let sources: Vec<String> = sources.iter().map(|s| src(f, s)).collect();
    f.ops.import_plan(target, &sources).unwrap()
}

fn quiet() -> impl Fn(&ImportProgress) {
    |_| {}
}

fn apply(f: &Fx, plan: &ImportPlan, mode: ImportMode, choice: ConflictChoice) -> ImportOutcome {
    f.ops
        .import_apply(plan.plan_id, mode, choice, &quiet())
        .unwrap()
}

fn copy_all(f: &Fx, target: &str, sources: &[&str]) -> ImportOutcome {
    let p = plan(f, target, sources);
    apply(f, &p, ImportMode::Copy, ConflictChoice::KeepBoth)
}

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn read(f: &Fx, rel: &str) -> String {
    fs::read_to_string(f.dir.path().join(rel)).unwrap()
}

fn exists(base: &Path, rel: &str) -> bool {
    fs::symlink_metadata(base.join(rel)).is_ok()
}

fn kind_of(error: FileOpError) -> ErrorKind {
    IpcError::from(error).kind
}

fn has_ext(name: &str, ext: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn no_temps(dir: &Path) {
    let temps: Vec<String> = names(dir)
        .into_iter()
        .filter(|n| n.starts_with('.') && has_ext(n, "part"))
        .collect();
    assert!(temps.is_empty(), "temps left: {temps:?}");
}

/// A rename that says "other volume" whenever the two ends are in different folders (a temp beside its final
/// place still renames).
fn other_volume(from: &Path, to: &Path) -> io::Result<()> {
    if from.parent() == to.parent() {
        fs::rename(from, to)
    } else {
        Err(io::Error::from(io::ErrorKind::CrossesDevices))
    }
}

// ---- plan -------------------------------------------------------------------------------------------------------

#[test]
fn a_plan_lists_the_items_with_their_kind_and_the_taken_names() {
    let f = fx(
        &[("docs/a.md", "mine")],
        &[("a.md", "theirs"), ("b.md", "b"), ("pics/p.png", "png")],
    );
    let p = plan(&f, "docs", &["a.md", "b.md", "pics"]);
    assert_eq!(p.target, "docs");
    assert_eq!(p.refused.len(), 0);
    let summary: Vec<(&str, ImportKind, bool)> = p
        .items
        .iter()
        .map(|i| (i.name.as_str(), i.kind, i.conflict))
        .collect();
    assert_eq!(
        summary,
        [
            ("a.md", ImportKind::File, true),
            ("b.md", ImportKind::File, false),
            ("pics", ImportKind::Folder, false),
        ]
    );
    assert!(p.items.iter().all(|i| !i.in_project));
    assert_eq!(read(&f, "docs/a.md"), "mine", "a plan touches nothing");
}

#[test]
fn names_clash_without_regard_to_case_or_unicode_form() {
    // "é" typed as one character in the project, as "e" + combining accent when dropped
    let f = fx(
        &[("Plan.md", ""), ("caf\u{e9}.md", "")],
        &[("plan.md", ""), ("cafe\u{301}.md", "")],
    );
    let p = plan(&f, "", &["plan.md", "cafe\u{301}.md"]);
    assert!(p.items.iter().all(|i| i.conflict), "{:?}", p.items);
}

#[test]
fn items_dropped_with_the_same_name_clash_with_each_other() {
    let f = fx(&[], &[("a/x.md", "1"), ("b/x.md", "2"), ("b/y.md", "3")]);
    let p = plan(&f, "", &["a/x.md", "b/x.md", "b/y.md"]);
    let flags: Vec<bool> = p.items.iter().map(|i| i.conflict).collect();
    assert_eq!(flags, [false, true, false]);
}

#[test]
fn a_folder_and_what_is_inside_it_collapse_to_the_folder_and_duplicates_to_one() {
    let f = fx(
        &[],
        &[("dir/x.md", "x"), ("dir/sub/y.md", "y"), ("z.md", "z")],
    );
    let p = plan(
        &f,
        "",
        &["dir/x.md", "dir", "z.md", "dir", "dir/sub", "z.md"],
    );
    let names: Vec<&str> = p.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["dir", "z.md"]);
}

#[test]
fn entries_that_cannot_come_in_are_refused_one_by_one_with_a_reason() {
    let f = fx(
        &[("docs/readme.md", "")],
        &[("ok.md", ""), (".git/config", ""), ("sub/", "")],
    );
    let gone = src(&f, "never-was.md");
    let git = src(&f, ".git");
    let ok = src(&f, "ok.md");
    let p = f.ops.import_plan("docs", &[gone, git, ok]).unwrap();
    assert_eq!(p.items.len(), 1);
    let reasons: Vec<(&str, ImportRefusal)> = p
        .refused
        .iter()
        .map(|r| (r.name.as_str(), r.reason))
        .collect();
    assert_eq!(
        reasons,
        [
            ("never-was.md", ImportRefusal::Missing),
            (".git", ImportRefusal::Protected),
        ]
    );
}

#[test]
fn a_folder_cannot_be_dropped_into_itself_or_below_itself() {
    let f = fx(&[("docs/inner/a.md", "")], &[]);
    // the project's own parent folder holds the target
    let parent = f.dir.path().parent().unwrap().display().to_string();
    let here = f.dir.path().display().to_string();
    let docs = f.dir.path().join("docs").display().to_string();
    let p = f
        .ops
        .import_plan("docs/inner", &[parent, here, docs])
        .unwrap();
    assert_eq!(p.items.len(), 0);
    assert_eq!(p.refused.len(), 3);
    assert!(p
        .refused
        .iter()
        .all(|r| r.reason == ImportRefusal::IntoItself));
}

#[test]
fn something_already_in_the_project_is_marked_so() {
    let f = fx(&[("docs/a.md", "x"), ("other/b.md", "y")], &[("c.md", "z")]);
    let inside = f.dir.path().join("other/b.md").display().to_string();
    let outside = src(&f, "c.md");
    let p = f.ops.import_plan("docs", &[inside, outside]).unwrap();
    let flags: Vec<(&str, bool)> = p
        .items
        .iter()
        .map(|i| (i.name.as_str(), i.in_project))
        .collect();
    assert_eq!(flags, [("b.md", true), ("c.md", false)]);
}

#[test]
fn a_target_that_is_protected_missing_or_a_file_is_an_error() {
    let f = fx(
        &[
            ("docs/a.md", ""),
            (".git/HEAD", ""),
            (".permark/config.toml", ""),
        ],
        &[("x.md", "")],
    );
    let x = src(&f, "x.md");
    let sources = std::slice::from_ref(&x);
    for target in [".git", ".git/refs", ".permark", ".permark/local"] {
        let error = f.ops.import_plan(target, sources).unwrap_err();
        assert_eq!(kind_of(error), ErrorKind::Protected, "{target}");
    }
    let error = f.ops.import_plan("nothing-here", sources).unwrap_err();
    assert_eq!(kind_of(error), ErrorKind::NotFound);
    let error = f.ops.import_plan("docs/a.md", sources).unwrap_err();
    assert_eq!(kind_of(error), ErrorKind::NotFound);
    let error = f.ops.import_plan("../elsewhere", sources).unwrap_err();
    assert!(matches!(
        kind_of(error),
        ErrorKind::PathEscape | ErrorKind::InvalidPath
    ));
    let error = f.ops.import_plan("docs", &[]).unwrap_err();
    assert_eq!(kind_of(error), ErrorKind::Unavailable);
}

#[test]
fn hostile_source_paths_are_errors_not_panics() {
    let f = fx(&[], &[]);
    for bad in ["", "relative/path.md", "..", "a\0b", "./x"] {
        let error = f.ops.import_plan("", &[bad.to_owned()]).unwrap_err();
        assert_eq!(kind_of(error), ErrorKind::InvalidPath, "{bad:?}");
    }
}

#[test]
fn a_root_of_a_drive_has_no_name_to_come_in_under() {
    let f = fx(&[], &[]);
    let root = f
        .dir
        .path()
        .ancestors()
        .last()
        .unwrap()
        .display()
        .to_string();
    let p = f.ops.import_plan("", &[root]).unwrap();
    assert_eq!(p.items.len(), 0);
    assert_eq!(p.refused.len(), 1);
    assert!(matches!(
        p.refused[0].reason,
        ImportRefusal::BadName | ImportRefusal::IntoItself
    ));
}

#[test]
fn plans_are_kept_four_at_a_time_and_used_once() {
    let f = fx(&[], &[("a.md", "a")]);
    let first = plan(&f, "", &["a.md"]);
    for _ in 0..4 {
        plan(&f, "", &["a.md"]);
    }
    let error = f
        .ops
        .import_apply(
            first.plan_id,
            ImportMode::Copy,
            ConflictChoice::Skip,
            &quiet(),
        )
        .unwrap_err();
    assert_eq!(kind_of(error), ErrorKind::Unavailable);
    let last = plan(&f, "", &["a.md"]);
    apply(&f, &last, ImportMode::Copy, ConflictChoice::Skip);
    assert!(f
        .ops
        .import_apply(
            last.plan_id,
            ImportMode::Copy,
            ConflictChoice::Skip,
            &quiet()
        )
        .is_err());
}

// ---- copy -------------------------------------------------------------------------------------------------------

#[test]
fn a_file_is_copied_with_its_bytes_and_time_and_the_original_stays() {
    let f = fx(
        &[("docs/x.md", "")],
        &[("note.md", "hello \u{e9}\r\nworld")],
    );
    let before = fs::metadata(f.outside.path().join("note.md"))
        .unwrap()
        .modified()
        .unwrap();
    let out = copy_all(&f, "docs", &["note.md"]);
    assert_eq!((out.copied, out.moved, out.skipped), (1, 0, 0));
    assert!(out.failed.is_empty() && !out.cancelled);
    assert_eq!(read(&f, "docs/note.md"), "hello \u{e9}\r\nworld");
    assert!(exists(f.outside.path(), "note.md"));
    let after = fs::metadata(f.dir.path().join("docs/note.md"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
    assert_eq!((out.files, out.bytes), (1, 15));
    no_temps(&f.dir.path().join("docs"));
}

#[test]
fn a_folder_is_copied_whole_with_its_empty_folders() {
    let f = fx(
        &[],
        &[
            ("pics/a.png", "A"),
            ("pics/deep/er/b.png", "B"),
            ("pics/empty/", ""),
            ("pics/\u{9879}\u{76ee}/c.md", "C"),
        ],
    );
    let out = copy_all(&f, "", &["pics"]);
    assert_eq!((out.copied, out.files, out.bytes), (1, 3, 3));
    assert_eq!(read(&f, "pics/deep/er/b.png"), "B");
    assert!(f.dir.path().join("pics/empty").is_dir());
    assert_eq!(read(&f, "pics/\u{9879}\u{76ee}/c.md"), "C");
    assert!(exists(f.outside.path(), "pics/deep/er/b.png"));
    no_temps(f.dir.path());
}

#[test]
fn the_read_only_flag_is_kept() {
    let f = fx(&[], &[("locked.md", "x")]);
    let path = f.outside.path().join("locked.md");
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    copy_all(&f, "", &["locked.md"]);
    let copy = fs::metadata(f.dir.path().join("locked.md")).unwrap();
    assert!(copy.permissions().readonly());
    // the test folders must be removable afterwards
    for p in [path, f.dir.path().join("locked.md")] {
        let mut permissions = fs::metadata(&p).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&p, permissions).unwrap();
    }
}

#[test]
fn several_items_land_together_and_progress_ends_complete() {
    let f = fx(
        &[],
        &[("a.md", "aaa"), ("dir/b.md", "bb"), ("dir/c.md", "c")],
    );
    let p = plan(&f, "", &["a.md", "dir"]);
    let seen = RefCell::new(Vec::<ImportProgress>::new());
    let out = f
        .ops
        .import_apply(p.plan_id, ImportMode::Copy, ConflictChoice::Skip, &|e| {
            seen.borrow_mut().push(e.clone());
        })
        .unwrap();
    assert_eq!((out.copied, out.files, out.bytes), (2, 3, 6));
    let seen = seen.into_inner();
    assert_eq!(seen.first().map(|e| e.phase), Some(ImportPhase::Counting));
    let last = seen.last().unwrap();
    assert_eq!(last.phase, ImportPhase::Copying);
    assert_eq!(
        (
            last.items_done,
            last.items_total,
            last.files_done,
            last.files_total
        ),
        (2, 2, 3, 3)
    );
    assert_eq!((last.bytes_done, last.bytes_total), (6, 6));
    assert!(seen.iter().all(|e| e.plan_id == p.plan_id));
}

#[test]
fn a_name_that_is_taken_is_skipped_or_kept_both() {
    let f = fx(
        &[("a.md", "mine"), ("a (2).md", "also mine")],
        &[("a.md", "theirs"), ("d/x.md", "x")],
    );
    let p = plan(&f, "", &["a.md"]);
    let out = apply(&f, &p, ImportMode::Copy, ConflictChoice::Skip);
    assert_eq!((out.copied, out.skipped), (0, 1));
    assert_eq!(read(&f, "a.md"), "mine");
    let p = plan(&f, "", &["a.md"]);
    let out = apply(&f, &p, ImportMode::Copy, ConflictChoice::KeepBoth);
    assert_eq!((out.copied, out.skipped), (1, 0));
    assert_eq!(read(&f, "a.md"), "mine");
    assert_eq!(read(&f, "a (3).md"), "theirs", "the first free number");
}

#[test]
fn keep_both_names_folders_dotfiles_and_long_names() {
    let long = format!("{}.md", "n".repeat(251));
    let f = fx(
        &[("pics/", ""), (".env", "1"), (&long, "1")],
        &[("pics/p.png", "p"), (".env", "2"), (&long, "2")],
    );
    let out = copy_all(&f, "", &["pics", ".env", &long]);
    assert_eq!((out.copied, out.failed.len()), (3, 0), "{:?}", out.failed);
    assert!(f.dir.path().join("pics (2)/p.png").is_file());
    assert_eq!(read(&f, ".env (2)"), "2");
    let made: Vec<String> = names(f.dir.path())
        .into_iter()
        .filter(|n| n.contains("(2)") && has_ext(n, "md"))
        .collect();
    assert_eq!(made.len(), 1);
    assert!(made[0].len() <= 255, "{} bytes", made[0].len());
    assert!(made[0].ends_with(" (2).md"));
}

#[test]
fn a_name_taken_after_the_plan_is_never_overwritten() {
    let f = fx(&[], &[("a.md", "theirs")]);
    let p = plan(&f, "", &["a.md"]);
    assert!(!p.items[0].conflict);
    fs::write(f.dir.path().join("a.md"), "written meanwhile").unwrap();
    let out = apply(&f, &p, ImportMode::Copy, ConflictChoice::KeepBoth);
    assert_eq!(out.copied, 1);
    assert_eq!(read(&f, "a.md"), "written meanwhile");
    assert_eq!(read(&f, "a (2).md"), "theirs");
}

#[test]
fn dropped_items_with_one_name_keep_both_in_turn() {
    let f = fx(&[], &[("a/x.md", "1"), ("b/x.md", "2"), ("c/x.md", "3")]);
    let out = copy_all(&f, "", &["a/x.md", "b/x.md", "c/x.md"]);
    assert_eq!(out.copied, 3);
    assert_eq!(read(&f, "x.md"), "1");
    assert_eq!(read(&f, "x (2).md"), "2");
    assert_eq!(read(&f, "x (3).md"), "3");
    assert_eq!(
        out.placed,
        ["x.md", "x (2).md", "x (3).md"],
        "final names, drop order"
    );
}

#[test]
fn placed_names_the_items_in_a_folder_and_leaves_out_skipped_ones() {
    let f = fx(&[("docs/a.md", "old")], &[("a.md", "new"), ("b/c.md", "c")]);
    let p = plan(&f, "docs", &["a.md", "b"]);
    let out = apply(&f, &p, ImportMode::Copy, ConflictChoice::Skip);
    assert_eq!(out.placed, ["docs/b"]);
    assert_eq!(out.skipped, 1);
}

#[test]
fn copying_what_is_already_in_the_project_makes_a_numbered_copy() {
    let f = fx(&[("docs/a.md", "text")], &[]);
    let inside = f.dir.path().join("docs/a.md").display().to_string();
    let p = f.ops.import_plan("docs", &[inside]).unwrap();
    assert!(p.items[0].in_project && p.items[0].conflict);
    let out = apply(&f, &p, ImportMode::Copy, ConflictChoice::KeepBoth);
    assert_eq!(out.copied, 1);
    assert_eq!(read(&f, "docs/a (2).md"), "text");
}

#[test]
fn a_move_of_something_inside_the_project_is_refused() {
    let f = fx(&[("docs/a.md", "text"), ("other/", "")], &[]);
    let inside = f.dir.path().join("docs/a.md").display().to_string();
    let p = f.ops.import_plan("other", &[inside]).unwrap();
    let error = f
        .ops
        .import_apply(p.plan_id, ImportMode::Move, ConflictChoice::Skip, &quiet())
        .unwrap_err();
    assert_eq!(kind_of(error), ErrorKind::Unavailable);
    assert!(exists(f.dir.path(), "docs/a.md") && !exists(f.dir.path(), "other/a.md"));
}

#[test]
fn the_items_announce_themselves_to_the_watch_as_our_own() {
    let f = fx(&[], &[("a.md", "a"), ("dir/b.md", "b")]);
    copy_all(&f, "", &["a.md", "dir"]);
    let change = |path: &str| Change {
        path: path.to_owned(),
        kind: ChangeKind::Created,
        is_dir: false,
        own: false,
    };
    let mut changes = [
        change("a.md"),
        change("dir"),
        change("dir/b.md"),
        change("other.md"),
    ];
    f.own.mark(&mut changes, |_| None);
    let own: Vec<bool> = changes.iter().map(|c| c.own).collect();
    assert_eq!(own, [true, true, true, false]);
}

#[test]
fn the_apply_leaves_one_journal_entry_and_none_when_nothing_came_in() {
    let f = fx(&[("a.md", "mine")], &[("a.md", "theirs"), ("b.md", "b")]);
    let p = plan(&f, "", &["a.md"]);
    apply(&f, &p, ImportMode::Copy, ConflictChoice::Skip);
    assert_eq!(f.ops.undo_depth(), 0, "everything skipped");
    copy_all(&f, "", &["a.md", "b.md"]);
    assert_eq!(f.ops.undo_depth(), 1);
}

// ---- move -------------------------------------------------------------------------------------------------------

#[test]
fn a_move_on_one_volume_is_a_rename() {
    let f = fx(&[("docs/x.md", "")], &[("note.md", "n"), ("dir/b.md", "b")]);
    let p = plan(&f, "docs", &["note.md", "dir"]);
    let out = apply(&f, &p, ImportMode::Move, ConflictChoice::Skip);
    assert_eq!((out.copied, out.moved, out.left_behind), (0, 2, 0));
    assert!(!exists(f.outside.path(), "note.md") && !exists(f.outside.path(), "dir"));
    assert_eq!(read(&f, "docs/note.md"), "n");
    assert_eq!(read(&f, "docs/dir/b.md"), "b");
}

#[test]
fn a_move_to_another_volume_copies_then_deletes_the_originals() {
    let f = fx(
        &[],
        &[
            ("note.md", "n"),
            ("dir/b.md", "bb"),
            ("dir/sub/c.md", "c"),
            ("dir/empty/", ""),
        ],
    );
    let p = plan(&f, "", &["note.md", "dir"]);
    let out = f
        .ops
        .import_apply_with(
            p.plan_id,
            ImportMode::Move,
            ConflictChoice::Skip,
            &quiet(),
            &other_volume,
            TICK,
        )
        .unwrap();
    assert_eq!((out.copied, out.moved, out.left_behind), (0, 2, 0));
    assert_eq!(out.failed.len(), 0);
    assert_eq!(read(&f, "note.md"), "n");
    assert_eq!(read(&f, "dir/sub/c.md"), "c");
    assert!(f.dir.path().join("dir/empty").is_dir());
    assert!(!exists(f.outside.path(), "note.md"), "original file gone");
    assert!(!exists(f.outside.path(), "dir"), "original folder gone");
    no_temps(f.dir.path());
}

#[test]
fn a_file_changed_while_it_was_copied_is_not_deleted_from_its_old_place() {
    let f = fx(&[], &[("dir/a.md", "one"), ("dir/b.md", "two")]);
    let p = plan(&f, "", &["dir"]);
    let changed = f.outside.path().join("dir/b.md");
    // the stage is renamed into place after the copy and before the originals go: edit the original right there
    let rename = |from: &Path, to: &Path| {
        let result = other_volume(from, to);
        if result.is_ok() {
            fs::write(&changed, "two, edited meanwhile").unwrap();
        }
        result
    };
    let out = f
        .ops
        .import_apply_with(
            p.plan_id,
            ImportMode::Move,
            ConflictChoice::Skip,
            &quiet(),
            &rename,
            TICK,
        )
        .unwrap();
    assert_eq!(out.moved, 1);
    assert_eq!(out.left_behind, 1);
    assert!(!exists(f.outside.path(), "dir/a.md"));
    assert_eq!(
        fs::read_to_string(f.outside.path().join("dir/b.md")).unwrap(),
        "two, edited meanwhile"
    );
    assert_eq!(read(&f, "dir/b.md"), "two", "the copy is what was read");
}

#[test]
fn a_move_that_deleted_nothing_is_recorded_as_a_copy() {
    let f = fx(&[], &[("a.md", "one")]);
    let p = plan(&f, "", &["a.md"]);
    let changed = f.outside.path().join("a.md");
    let rename = |from: &Path, to: &Path| {
        let result = other_volume(from, to);
        if result.is_ok() {
            fs::write(&changed, "edited, longer").unwrap();
        }
        result
    };
    let out = f
        .ops
        .import_apply_with(
            p.plan_id,
            ImportMode::Move,
            ConflictChoice::Skip,
            &quiet(),
            &rename,
            TICK,
        )
        .unwrap();
    assert_eq!((out.copied, out.moved, out.left_behind), (1, 0, 1));
    assert!(exists(f.outside.path(), "a.md"));
}

#[test]
fn a_failing_item_does_not_stop_the_others() {
    let f = fx(&[], &[("a.md", "a"), ("b.md", "b"), ("c.md", "c")]);
    let p = plan(&f, "", &["a.md", "b.md", "c.md"]);
    // the original of b vanishes after the plan
    fs::remove_file(f.outside.path().join("b.md")).unwrap();
    let out = apply(&f, &p, ImportMode::Copy, ConflictChoice::Skip);
    assert_eq!(out.copied, 2);
    assert_eq!(out.failed.len(), 1);
    assert_eq!(out.failed[0].name, "b.md");
    assert_eq!(out.failed[0].error.kind, ErrorKind::NotFound);
    assert!(exists(f.dir.path(), "a.md") && exists(f.dir.path(), "c.md"));
    no_temps(f.dir.path());
}

// ---- undo -------------------------------------------------------------------------------------------------------

#[test]
fn undo_takes_copies_out_of_the_project_into_the_bin() {
    let f = fx(&[("keep.md", "k")], &[("a.md", "a"), ("dir/b.md", "b")]);
    copy_all(&f, "", &["a.md", "dir"]);
    let undone = f.ops.undo().unwrap().unwrap();
    assert_eq!(undone.kind, UndoKind::Import);
    assert_eq!((undone.restored, undone.skipped.len()), (2, 0));
    assert!(!exists(f.dir.path(), "a.md") && !exists(f.dir.path(), "dir"));
    assert!(exists(f.dir.path(), "keep.md") && exists(f.outside.path(), "a.md"));
    assert_eq!(f.ops.undo_depth(), 0);
}

#[test]
fn undo_moves_items_back_where_they_came_from() {
    let f = fx(&[], &[("note.md", "n"), ("dir/b.md", "b")]);
    let p = plan(&f, "", &["note.md", "dir"]);
    apply(&f, &p, ImportMode::Move, ConflictChoice::Skip);
    assert!(!exists(f.outside.path(), "note.md"));
    let undone = f.ops.undo().unwrap().unwrap();
    assert_eq!((undone.restored, undone.skipped.len()), (2, 0));
    assert_eq!(
        fs::read_to_string(f.outside.path().join("note.md")).unwrap(),
        "n"
    );
    assert_eq!(
        fs::read_to_string(f.outside.path().join("dir/b.md")).unwrap(),
        "b"
    );
    assert!(!exists(f.dir.path(), "note.md") && !exists(f.dir.path(), "dir"));
}

#[test]
fn undo_leaves_an_item_whose_old_place_is_taken_or_gone() {
    let f = fx(&[], &[("a.md", "a"), ("sub/b.md", "b")]);
    let p = plan(&f, "", &["a.md", "sub/b.md"]);
    apply(&f, &p, ImportMode::Move, ConflictChoice::Skip);
    // a's place is taken again; b's folder is gone
    fs::write(f.outside.path().join("a.md"), "someone else").unwrap();
    fs::remove_dir_all(f.outside.path().join("sub")).unwrap();
    let undone = f.ops.undo().unwrap().unwrap();
    assert_eq!(undone.restored, 0);
    let mut reasons: Vec<(String, SkipReason)> = undone
        .skipped
        .iter()
        .map(|s| (s.path.clone(), s.reason))
        .collect();
    reasons.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        reasons,
        [
            ("a.md".to_owned(), SkipReason::Occupied),
            ("b.md".to_owned(), SkipReason::Missing),
        ]
    );
    assert_eq!(read(&f, "a.md"), "a", "the item stays in the project");
    assert_eq!(read(&f, "b.md"), "b");
}

#[test]
fn undo_of_a_move_across_volumes_goes_back_the_same_way() {
    let f = fx(&[], &[("dir/b.md", "bb"), ("dir/sub/c.md", "c")]);
    let p = plan(&f, "", &["dir"]);
    f.ops
        .import_apply_with(
            p.plan_id,
            ImportMode::Move,
            ConflictChoice::Skip,
            &quiet(),
            &other_volume,
            TICK,
        )
        .unwrap();
    assert!(!exists(f.outside.path(), "dir"));
    let undone = f.ops.undo().unwrap().unwrap();
    assert_eq!((undone.restored, undone.skipped.len()), (1, 0));
    assert_eq!(
        fs::read_to_string(f.outside.path().join("dir/sub/c.md")).unwrap(),
        "c"
    );
}

#[test]
fn a_partly_deleted_source_is_kept_in_the_project_by_undo() {
    let f = fx(&[], &[("dir/a.md", "one"), ("dir/b.md", "two")]);
    let p = plan(&f, "", &["dir"]);
    let changed = f.outside.path().join("dir/b.md");
    let rename = |from: &Path, to: &Path| {
        let result = other_volume(from, to);
        if result.is_ok() {
            fs::write(&changed, "edited meanwhile").unwrap();
        }
        result
    };
    f.ops
        .import_apply_with(
            p.plan_id,
            ImportMode::Move,
            ConflictChoice::Skip,
            &quiet(),
            &rename,
            TICK,
        )
        .unwrap();
    let undone = f.ops.undo().unwrap().unwrap();
    // the old folder still exists (b.md stayed), so the item cannot go back: nothing is lost either way
    assert_eq!(undone.restored, 0);
    assert_eq!(undone.skipped[0].reason, SkipReason::Occupied);
    assert_eq!(read(&f, "dir/a.md"), "one");
}

// ---- cancel -----------------------------------------------------------------------------------------------------

#[test]
fn cancelling_between_items_keeps_the_finished_ones_and_the_journal_matches() {
    let f = fx(&[], &[("a.md", "a"), ("b.md", "b"), ("c.md", "c")]);
    let p = plan(&f, "", &["a.md", "b.md", "c.md"]);
    let id = p.plan_id;
    let out = f
        .ops
        .import_apply(id, ImportMode::Copy, ConflictChoice::Skip, &|e| {
            if e.items_done == 1 {
                assert!(f.ops.import_cancel(id));
            }
        })
        .unwrap();
    assert!(out.cancelled);
    assert_eq!(out.copied, 1);
    assert!(exists(f.dir.path(), "a.md") && !exists(f.dir.path(), "b.md"));
    no_temps(f.dir.path());
    let undone = f.ops.undo().unwrap().unwrap();
    assert_eq!(undone.restored, 1);
    assert!(!exists(f.dir.path(), "a.md"));
    assert!(!f.ops.import_cancel(id), "nothing running any more");
}

#[test]
fn cancelling_inside_a_big_file_leaves_no_trace_of_it() {
    let f = fx(&[("keep.md", "k")], &[]);
    let big = f.outside.path().join("big.bin");
    fs::write(&big, vec![7_u8; 6 * 1024 * 1024]).unwrap();
    let p = plan(&f, "", &["big.bin"]);
    let id = p.plan_id;
    let out = f
        .ops
        .import_apply_with(
            id,
            ImportMode::Copy,
            ConflictChoice::Skip,
            &|e| {
                if e.phase == ImportPhase::Copying && e.bytes_done > 0 {
                    f.ops.import_cancel(id);
                }
            },
            &os_rename,
            Duration::ZERO,
        )
        .unwrap();
    assert!(out.cancelled);
    assert_eq!(out.copied, 0);
    assert_eq!(names(f.dir.path()), ["keep.md"], "no file, no temp");
    assert_eq!(f.ops.undo_depth(), 0, "nothing came in, nothing to undo");
}

#[test]
fn cancelling_inside_a_folder_removes_the_unfinished_folder() {
    let f = fx(&[], &[]);
    for n in 0..300 {
        put(
            f.outside.path(),
            &[(&format!("dir/d{}/f{n}.txt", n % 7), "x")],
        );
    }
    put(f.outside.path(), &[("first.md", "1")]);
    let p = plan(&f, "", &["first.md", "dir"]);
    let id = p.plan_id;
    let files = std::cell::Cell::new(0_u64);
    let out = f
        .ops
        .import_apply_with(
            id,
            ImportMode::Copy,
            ConflictChoice::Skip,
            &|e| {
                files.set(e.files_done);
                if e.phase == ImportPhase::Copying && e.files_done >= 50 {
                    f.ops.import_cancel(id);
                }
            },
            &os_rename,
            Duration::ZERO,
        )
        .unwrap();
    assert!(out.cancelled);
    assert_eq!(out.copied, 1, "the file before the folder finished");
    assert_eq!(names(f.dir.path()), ["first.md"]);
}

// ---- adversarial ------------------------------------------------------------------------------------------------

#[test]
fn unicode_space_and_cjk_names_survive_a_round_trip() {
    let f = fx(
        &[("\u{6587}\u{6863} folder/", "")],
        &[
            ("\u{6d4b}\u{8bd5} \u{6587}\u{6863} \u{fc}.md", "cjk"),
            (
                "\u{65e5}\u{672c}\u{8a9e} \u{30d5}\u{30a9}\u{30eb}\u{30c0}/sub dir/\u{e4} b.txt",
                "nested",
            ),
            ("emoji \u{1f4dd}.md", "pen"),
        ],
    );
    let out = copy_all(
        &f,
        "\u{6587}\u{6863} folder",
        &[
            "\u{6d4b}\u{8bd5} \u{6587}\u{6863} \u{fc}.md",
            "\u{65e5}\u{672c}\u{8a9e} \u{30d5}\u{30a9}\u{30eb}\u{30c0}",
            "emoji \u{1f4dd}.md",
        ],
    );
    assert_eq!((out.copied, out.failed.len()), (3, 0));
    assert_eq!(
        read(
            &f,
            "\u{6587}\u{6863} folder/\u{6d4b}\u{8bd5} \u{6587}\u{6863} \u{fc}.md"
        ),
        "cjk"
    );
    assert_eq!(
        read(
            &f,
            "\u{6587}\u{6863} folder/\u{65e5}\u{672c}\u{8a9e} \u{30d5}\u{30a9}\u{30eb}\u{30c0}/sub dir/\u{e4} b.txt"
        ),
        "nested"
    );
}

#[test]
fn a_path_longer_than_the_classic_limit_is_copied() {
    let deep = ["level-number-"; 24]
        .iter()
        .enumerate()
        .fold(String::new(), |acc, (n, name)| {
            format!("{acc}{name}{n:02}/")
        });
    let rel = format!("tree/{deep}end.md");
    let f = fx(&[], &[(rel.as_str(), "deep")]);
    assert!(f.outside.path().join(&rel).display().to_string().len() > 300);
    let out = copy_all(&f, "", &["tree"]);
    assert_eq!((out.copied, out.files, out.failed.len()), (1, 1, 0));
    assert_eq!(read(&f, &rel), "deep");
}

#[cfg(unix)]
#[test]
fn a_target_folder_that_cannot_be_written_fails_those_items_only() {
    use std::os::unix::fs::PermissionsExt;
    let f = fx(&[("closed/", ""), ("open/", "")], &[("a.md", "a")]);
    let closed = f.dir.path().join("closed");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o555)).unwrap();
    let writable = fs::File::create(closed.join("probe")).is_ok();
    let p = plan(&f, "closed", &["a.md"]);
    let out = apply(&f, &p, ImportMode::Copy, ConflictChoice::Skip);
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        // running as a user the mode does not bind (root): nothing to check
        return;
    }
    assert_eq!(out.copied, 0);
    assert_eq!(out.failed.len(), 1);
    assert_eq!(out.failed[0].error.kind, ErrorKind::Denied);
    no_temps(&closed);
    let p = plan(&f, "open", &["a.md"]);
    assert_eq!(
        apply(&f, &p, ImportMode::Copy, ConflictChoice::Skip).copied,
        1
    );
}

#[cfg(windows)]
#[test]
fn a_file_another_program_holds_open_fails_its_item_and_the_rest_come_in() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = fx(&[], &[("a.md", "a"), ("held.md", "held"), ("z.md", "z")]);
    let holder = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(f.outside.path().join("held.md"))
        .unwrap();
    let out = copy_all(&f, "", &["a.md", "held.md", "z.md"]);
    drop(holder);
    assert_eq!(out.copied, 2);
    assert_eq!(out.failed.len(), 1);
    assert_eq!(out.failed[0].name, "held.md");
    assert!(exists(f.dir.path(), "a.md") && exists(f.dir.path(), "z.md"));
    assert!(!exists(f.dir.path(), "held.md"));
    no_temps(f.dir.path());
}

/// Makes `link` point at `target` (a folder): a symbolic link, or a junction on Windows. `false`: not possible here.
fn make_link(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success())
    }
}

#[test]
fn a_link_inside_a_dropped_folder_is_neither_followed_nor_copied() {
    let f = fx(&[], &[("dir/own.md", "mine")]);
    let secret = tempfile::tempdir().unwrap();
    fs::write(secret.path().join("secret.txt"), "private").unwrap();
    if !make_link(&f.outside.path().join("dir/jump"), secret.path()) {
        eprintln!("skipped: cannot create a link here");
        return;
    }
    let out = copy_all(&f, "", &["dir"]);
    assert_eq!((out.copied, out.files, out.links), (1, 1, 1));
    assert_eq!(read(&f, "dir/own.md"), "mine");
    assert!(!exists(f.dir.path(), "dir/jump"));
    assert_eq!(
        fs::read_to_string(secret.path().join("secret.txt")).unwrap(),
        "private"
    );
}

#[test]
fn a_link_dropped_itself_is_refused() {
    let f = fx(&[], &[("real/x.md", "x")]);
    if !make_link(
        &f.outside.path().join("alias"),
        &f.outside.path().join("real"),
    ) {
        eprintln!("skipped: cannot create a link here");
        return;
    }
    let p = plan(&f, "", &["alias", "real"]);
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.refused[0].reason, ImportRefusal::Link);
}

#[test]
fn ten_thousand_files_come_in_with_exact_totals() {
    let f = fx(&[], &[]);
    for d in 0..100 {
        let dir = f.outside.path().join(format!("big/d{d:03}"));
        fs::create_dir_all(&dir).unwrap();
        for n in 0..100 {
            fs::write(dir.join(format!("f{n:03}.txt")), format!("{d}-{n}")).unwrap();
        }
    }
    // Windows: the first open of a just-written file waits for the virus scan (~2 ms each, 20-60 s for all on a
    // runner); read them once here so the bound below times the copy, not the scanner
    for d in 0..100 {
        for n in 0..100 {
            let path = f.outside.path().join(format!("big/d{d:03}/f{n:03}.txt"));
            fs::read(path).unwrap();
        }
    }
    let started = std::time::Instant::now();
    let out = copy_all(&f, "", &["big"]);
    let took = started.elapsed();
    eprintln!("10 000 files copied in {took:?}");
    assert_eq!((out.copied, out.files, out.failed.len()), (1, 10_000, 0));
    assert_eq!(read(&f, "big/d042/f007.txt"), "42-7");
    // linear: ~2 s on a desktop; a per-file cost that grows with the count would take minutes
    assert!(took.as_secs() < 60, "{took:?}");
    no_temps(f.dir.path());
}

#[test]
fn the_target_cannot_be_dropped_onto_itself_nor_git_or_permark_names_come_in() {
    let f = fx(
        &[("docs/a.md", "")],
        &[
            (".git/config", ""),
            (".permark/config.toml", ""),
            ("fine.md", ""),
        ],
    );
    let docs = f.dir.path().join("docs").display().to_string();
    let p = f.ops.import_plan("docs", &[docs]).unwrap();
    assert_eq!(p.refused[0].reason, ImportRefusal::IntoItself);
    let p = plan(&f, "", &[".git", ".permark", "fine.md"]);
    let refused: Vec<(&str, ImportRefusal)> = p
        .refused
        .iter()
        .map(|r| (r.name.as_str(), r.reason))
        .collect();
    assert_eq!(
        refused,
        [
            (".git", ImportRefusal::Protected),
            (".permark", ImportRefusal::Protected),
        ]
    );
    assert_eq!(p.items.len(), 1);
    // `.permark` is protected at the top of a root only; below it is a plain name
    let p = plan(&f, "docs", &[".permark"]);
    assert_eq!(p.items.len(), 1);
}

#[test]
fn two_plans_can_be_applied_in_any_order() {
    let f = fx(&[], &[("a.md", "a"), ("b.md", "b")]);
    let first = plan(&f, "", &["a.md"]);
    let second = plan(&f, "", &["b.md"]);
    assert_eq!(
        apply(&f, &second, ImportMode::Copy, ConflictChoice::Skip).copied,
        1
    );
    assert_eq!(
        apply(&f, &first, ImportMode::Copy, ConflictChoice::Skip).copied,
        1
    );
    assert_eq!(f.ops.undo_depth(), 2);
}

#[test]
fn a_target_that_disappears_before_the_apply_is_an_error() {
    let f = fx(&[("docs/a.md", "")], &[("x.md", "x")]);
    let p = plan(&f, "docs", &["x.md"]);
    fs::remove_dir_all(f.dir.path().join("docs")).unwrap();
    let error = f
        .ops
        .import_apply(p.plan_id, ImportMode::Copy, ConflictChoice::Skip, &quiet())
        .unwrap_err();
    assert_eq!(kind_of(error), ErrorKind::NotFound);
}

#[test]
fn numbered_names_are_cut_to_fit_and_keep_the_extension() {
    assert_eq!(numbered("a.md", false, 2), "a (2).md");
    assert_eq!(numbered("archive.tar.gz", false, 3), "archive.tar (3).gz");
    assert_eq!(numbered(".env", false, 2), ".env (2)");
    assert_eq!(numbered("v1.2", true, 2), "v1.2 (2)");
    assert_eq!(numbered("noext", false, 10), "noext (10)");
    let long = format!("{}.md", "\u{6587}".repeat(84));
    let cut = numbered(&long, false, 2);
    assert!(
        cut.len() <= MAX_NAME && cut.ends_with(" (2).md"),
        "{}",
        cut.len()
    );
    assert!(cut.is_char_boundary(cut.len()));
}

#[test]
fn the_cancel_flag_of_a_finished_import_is_gone() {
    let f = fx(&[], &[("a.md", "a")]);
    let p = plan(&f, "", &["a.md"]);
    assert!(!f.ops.import_cancel(p.plan_id), "planned, not running");
    apply(&f, &p, ImportMode::Copy, ConflictChoice::Skip);
    assert!(!f.ops.import_cancel(p.plan_id));
    let _: PathBuf = f.dir.path().to_path_buf();
}

// ---- verifier (adversarial) -----------------------------------------------------------------------------------------

/// The project folder spelled another way (other case, verbatim `\?\` prefix, trailing `.`) still holds the target:
/// copying it would copy into itself without end.
#[cfg(windows)]
#[test]
fn verifier_a_folder_spelled_another_way_still_cannot_go_into_itself() {
    let f = fx(&[("docs/inner/a.md", "")], &[]);
    let here = f.dir.path().display().to_string();
    let upper = here.to_uppercase();
    let verbatim = format!(r"\\?\{here}");
    let dotted = format!(r"{here}\.");
    let p = f
        .ops
        .import_plan("docs/inner", &[upper, verbatim, dotted])
        .unwrap();
    assert_eq!(p.items.len(), 0, "items: {:?}", p.items);
    assert!(
        p.refused
            .iter()
            .all(|r| r.reason == ImportRefusal::IntoItself),
        "refused: {:?}",
        p.refused
    );
}

/// The same outside file named twice in other spellings is one item (no `a (2).md` copy of itself).
#[cfg(windows)]
#[test]
fn verifier_one_file_dropped_in_two_spellings_is_one_item() {
    let f = fx(&[("docs/", "")], &[("a.md", "x")]);
    let one = src(&f, "a.md");
    let two = one.to_uppercase();
    let p = f.ops.import_plan("docs", &[one, two]).unwrap();
    assert_eq!(p.items.len(), 1, "items: {:?}", p.items);
}

/// A Move whose numbered name was taken after the plan: the next free number, nothing overwritten.
#[test]
fn verifier_move_of_a_file_already_in_place_twice_never_overwrites() {
    let f = fx(&[("docs/a.md", "mine")], &[("a.md", "theirs")]);
    let p = plan(&f, "docs", &["a.md"]);
    // the name is taken again (and by a different file) after the plan
    fs::write(f.dir.path().join("docs/a (2).md"), "also mine").unwrap();
    let out = apply(&f, &p, ImportMode::Move, ConflictChoice::KeepBoth);
    assert_eq!(read(&f, "docs/a.md"), "mine");
    assert_eq!(read(&f, "docs/a (2).md"), "also mine");
    assert_eq!(read(&f, "docs/a (3).md"), "theirs");
    assert_eq!(out.placed, ["docs/a (3).md"]);
}

// ---- undo of exactly this import (the notice's Undo) -------------------------------------------------------------------

#[test]
fn the_undo_of_an_import_takes_back_that_import_not_the_newest_operation() {
    let f = fx(&[("docs/", "")], &[("a.md", "a"), ("b.md", "b")]);
    let first = copy_all(&f, "docs", &["a.md"]);
    let second = copy_all(&f, "", &["b.md"]);
    let (one, two) = (first.undo.unwrap(), second.undo.unwrap());
    assert_ne!(one, two);
    let out = f.ops.undo_entry(one).unwrap().unwrap();
    assert_eq!(out.kind, UndoKind::Import);
    assert!(
        !f.dir.path().join("docs/a.md").exists(),
        "the first import went"
    );
    assert_eq!(read(&f, "b.md"), "b", "the newer one stays");
    assert!(f.ops.undo_entry(one).unwrap().is_none(), "once only");
    assert!(f.ops.undo_entry(two).unwrap().is_some());
}

#[test]
fn an_import_touched_since_is_not_taken_back_by_its_notice() {
    let f = fx(&[("docs/", "")], &[("a.md", "a")]);
    let out = copy_all(&f, "docs", &["a.md"]);
    f.ops
        .apply_move(
            f.ops
                .plan_move(
                    &[crate::fileops::MoveSpec {
                        from: "docs/a.md".to_owned(),
                        to: "a.md".to_owned(),
                    }],
                    false,
                )
                .unwrap()
                .plan_id,
            false,
        )
        .unwrap();
    assert!(f.ops.undo_entry(out.undo.unwrap()).unwrap().is_none());
    assert_eq!(read(&f, "a.md"), "a", "nothing taken away");
}

#[test]
fn an_import_that_added_nothing_has_no_undo() {
    let f = fx(&[("docs/a.md", "x")], &[("a.md", "y")]);
    let p = plan(&f, "docs", &["a.md"]);
    let out = apply(&f, &p, ImportMode::Copy, ConflictChoice::Skip);
    assert_eq!(out.undo, None);
}

// ---- the tree learns at once (director 2026-10-20: a copied file did not show until a refresh) -------------------------

/// A project whose Explorer events are kept, with one open view "w".
fn fx_seen(inside: &[(&str, &str)], outside: &[(&str, &str)]) -> (Fx, Arc<Recorder>) {
    let f = fx(inside, outside);
    let sink = Arc::new(Recorder::default());
    let vault = Vault::new(Root::open(f.dir.path()).unwrap());
    let explorer = Arc::new(Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::clone(&sink) as Arc<dyn crate::events::EventSink>,
        None,
    ));
    explorer.open_view("w", Some(TreeFilter::default()));
    let state = tempfile::tempdir().unwrap();
    let store = StateStore::for_project(&AppDirs::under(state.path()), vault.main());
    let ops = FileOps::new(vault, explorer, Arc::clone(&f.own), &store);
    std::mem::forget(state);
    (Fx { ops, ..f }, sink)
}

/// Paths the view "w" was told were added since the last look.
fn added(sink: &Recorder) -> Vec<String> {
    sink.take()
        .into_iter()
        .filter_map(|e| match e {
            crate::events::CoreEvent::TreeChanged(c) if c.window == "w" => Some(c.delta),
            _ => None,
        })
        .flat_map(|d| d.added.into_iter().map(|a| a.node.path))
        .collect()
}

#[test]
fn the_view_hears_of_imported_items_right_after_the_apply() {
    for mode in [ImportMode::Copy, ImportMode::Move] {
        let (f, sink) = fx_seen(
            &[
                ("docs/guide.md", "g"),
                ("docs/deep/x.md", "x"),
                ("top.md", "t"),
            ],
            &[
                ("a.md", "a"),
                ("b.md", "b"),
                ("c.md", "c"),
                ("pics/one.md", "1"),
            ],
        );
        let _ = added(&sink);
        for (target, source) in [("docs", "a.md"), ("docs/deep", "b.md"), ("", "c.md")] {
            let p = plan(&f, target, &[source]);
            let out = apply(&f, &p, mode, ConflictChoice::KeepBoth);
            let shown = out.placed[0].clone();
            assert!(
                added(&sink).contains(&shown),
                "{mode:?}: {shown} not told to the view"
            );
        }
        let p = plan(&f, "docs", &["pics"]);
        apply(&f, &p, mode, ConflictChoice::KeepBoth);
        let told = added(&sink);
        assert!(
            told.contains(&"docs/pics".to_owned()),
            "{mode:?}: folder {told:?}"
        );
        assert!(
            told.contains(&"docs/pics/one.md".to_owned()),
            "{mode:?}: inside {told:?}"
        );
    }
}

/// With the watcher running (as in the app): the view's list, built from the deltas it was sent, ends holding the
/// imported items (the watcher's own report of them never takes them away again).
#[test]
fn with_the_watcher_running_the_view_keeps_the_imported_items() {
    use crate::watch::{Timing, WatchHub, Watcher};
    use std::collections::BTreeSet;
    use std::time::{Duration, Instant};

    for mode in [ImportMode::Copy, ImportMode::Move] {
        let f = fx(
            &[("docs/guide.md", "g"), ("docs/deep/x.md", "x")],
            &[("a.md", "a"), ("pics/one.md", "1")],
        );
        let sink = Arc::new(Recorder::default());
        let vault = Vault::new(Root::open(f.dir.path()).unwrap());
        let explorer = Arc::new(Explorer::new(
            &vault,
            &ProjectConfig::default(),
            Arc::clone(&sink) as Arc<dyn crate::events::EventSink>,
            None,
        ));
        let snapshot = explorer.open_view("w", Some(TreeFilter::default()));
        let mut list: BTreeSet<String> = snapshot.nodes.into_iter().map(|n| n.path).collect();
        let hub = WatchHub::new();
        let _sub = hub.subscribe({
            let explorer = Arc::clone(&explorer);
            move |batch| explorer.apply_batch(batch)
        });
        let timing = Timing {
            quiet: Duration::from_millis(30),
            max: Duration::from_millis(250),
            poll: Duration::from_millis(60),
        };
        let watcher = Watcher::start_with(
            &vault,
            explorer.ignorers(),
            hub,
            Arc::clone(&f.own),
            None,
            timing,
            true,
        );
        std::thread::sleep(Duration::from_millis(300));
        let state = tempfile::tempdir().unwrap();
        let store = StateStore::for_project(&AppDirs::under(state.path()), vault.main());
        let ops = FileOps::new(
            vault.clone(),
            Arc::clone(&explorer),
            Arc::clone(&f.own),
            &store,
        );
        let sources = vec![src(&f, "a.md"), src(&f, "pics")];
        let p = ops.import_plan("docs/deep", &sources).unwrap();
        let out = ops
            .import_apply(p.plan_id, mode, ConflictChoice::KeepBoth, &quiet())
            .unwrap();
        assert_eq!(out.placed, ["docs/deep/a.md", "docs/deep/pics"]);
        // the watcher's reports of all this come in
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(1500) {
            std::thread::sleep(Duration::from_millis(50));
        }
        for event in sink.take() {
            if let crate::events::CoreEvent::TreeChanged(c) = event {
                if c.window == "w" {
                    for gone in &c.delta.removed {
                        list.remove(gone);
                    }
                    for added in c.delta.added {
                        list.insert(added.node.path);
                    }
                }
            }
        }
        drop(watcher);
        for path in ["docs/deep/a.md", "docs/deep/pics", "docs/deep/pics/one.md"] {
            assert!(
                list.contains(path),
                "{mode:?}: {path} not in the view: {list:?}"
            );
        }
    }
}
