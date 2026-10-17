use std::cell::Cell;
use std::fs;

use super::*;
use crate::vault::Worktree;
use crate::watch::{ChangeKind, FsChange, SigStore};

fn vault_with(files: &[&str]) -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    for f in files {
        let p = dir.path().join(f);
        fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        fs::write(p, "x").expect("write");
    }
    let vault = Vault::open(dir.path()).expect("open");
    (dir, vault)
}

fn mv(from: &str, to: &str) -> Move {
    Move {
        from: from.to_owned(),
        to: to.to_owned(),
    }
}

#[test]
fn a_move_is_checked_at_both_ends() {
    let (_d, vault) = vault_with(&["a.md", "dir/b.md"]);
    let ok = check_moves(&vault, &[mv("a.md", "dir/a.md")]).expect("valid");
    assert_eq!(
        (ok[0].from.as_str(), ok[0].to.as_str()),
        ("a.md", "dir/a.md")
    );
    assert!(!ok[0].is_dir);
    assert!(matches!(
        check_moves(&vault, &[mv("a.md", "dir/b.md")]),
        Err(FileOpError::Exists(_))
    ));
    assert!(matches!(
        check_moves(&vault, &[mv("missing.md", "x.md")]),
        Err(FileOpError::Vault(VaultError::NotFound(_)))
    ));
    assert!(matches!(
        check_moves(&vault, &[mv("dir", "dir/sub")]),
        Err(FileOpError::IntoItself(_))
    ));
}

#[test]
fn roots_and_protected_names_are_recognised() {
    assert!(is_root_path(""));
    assert!(is_root_path("@wt/feature"));
    assert!(!is_root_path("@wt/feature/a.md"));
    assert!(!is_root_path("docs"));
    assert!(is_protected(".git"));
    assert!(is_protected("sub/.GIT/config"));
    assert!(is_protected(".permark/meta.toml"));
    assert!(!is_protected("docs/.gitignore"));
}

#[test]
fn inside_compares_whole_segments() {
    assert!(inside("docs/a.md", "docs"));
    assert!(inside("docs", "docs"));
    assert!(!inside("docs-old/a.md", "docs"));
    assert!(!inside("doc", "docs"));
}

#[test]
fn locate_joins_the_name_to_the_resolved_parent() {
    let (_d, vault) = vault_with(&["Docs/a.md"]);
    // the folder is spelled as it is on disk, the name as it was given
    let found = locate(&vault, "Docs/New.md").expect("locate");
    assert_eq!(found.rel, "Docs/New.md");
    assert_eq!(found.name, "New.md");
    assert_eq!(found.root, 0);
    let top = locate(&vault, "top.md").expect("locate");
    assert_eq!(top.rel, "top.md");
    // backslashes and trailing slashes are normalised
    assert_eq!(
        locate(&vault, "Docs\\x.md/").expect("locate").rel,
        "Docs/x.md"
    );
}

#[test]
fn locate_refuses_what_could_leave_the_project() {
    let (_d, vault) = vault_with(&["a.md"]);
    for bad in [
        "../x.md",
        "docs/../../x.md",
        "/abs.md",
        "C:/x.md",
        "a\0b",
        "docs/..",
        ".",
    ] {
        assert!(locate(&vault, bad).is_err(), "{bad:?}");
    }
}

#[test]
fn only_one_plain_component_is_a_name() {
    for bad in ["", ".", "..", "a/b", "/a", "a/"] {
        assert!(!plain_name(bad), "{bad:?}");
    }
    for good in ["a.md", ".editorconfig", "two words.md", "it's.md", "a..b"] {
        assert!(plain_name(good), "{good:?}");
    }
}

#[test]
fn what_windows_reads_as_another_place_is_recognised() {
    for bad in [
        "D:secret.docx",
        "C:x.md",
        "a:stream",
        ".git.",
        ".git ",
        "x. ",
    ] {
        assert!(windows_would_misread(bad), "{bad:?}");
    }
    for fine in ["a.md", ".git", ".gitignore", "a b.md", "a.b"] {
        assert!(!windows_would_misread(fine), "{fine:?}");
    }
}

/// `PathBuf::join` of `D:x` replaces the folder: the name has to be refused before it is joined.
#[cfg(windows)]
#[test]
fn a_drive_relative_name_cannot_leave_the_project() {
    let (_d, vault) = vault_with(&["a.md", "docs/b.md"]);
    for bad in ["C:x.md", "D:secret.docx", "F:Cargo.toml", "docs/D:x.md"] {
        assert!(locate(&vault, bad).is_err(), "locate {bad:?}");
        assert!(
            check_deletes(&vault, &[bad.to_owned()]).is_err(),
            "delete {bad:?}"
        );
        assert!(
            check_moves(&vault, &[mv(bad, "stolen.md")]).is_err(),
            "move from {bad:?}"
        );
        assert!(
            check_moves(&vault, &[mv("a.md", bad)]).is_err(),
            "move to {bad:?}"
        );
    }
}

/// Windows drops a trailing dot or space, so these are the protected `.git` under another spelling.
#[cfg(windows)]
#[test]
fn another_spelling_of_a_protected_folder_is_protected_too() {
    let (_d, vault) = vault_with(&["a.md", ".git/config"]);
    for bad in [".git.", ".git ", ".git./config", ".GIT./hooks"] {
        assert!(locate(&vault, bad).is_err(), "{bad:?}");
        assert!(check_deletes(&vault, &[bad.to_owned()]).is_err(), "{bad:?}");
    }
    assert!(check_moves(&vault, &[mv("a.md", ".git./a.md")]).is_err());
    assert!(vault.root().join(".git/config").exists());
}

#[test]
fn the_last_check_before_the_disk_is_touched_keeps_everything_inside_the_root() {
    let (_d, vault) = vault_with(&["a.md", "docs/b.md"]);
    // paths in the app are built from the vault's canonical root (not the temp dir's own spelling, which can be
    // an 8.3 short name on Windows)
    let root = vault.root().to_path_buf();
    let inside_path = root.join("docs").join("b.md");
    assert!(confine(&vault, 0, &inside_path).is_ok());
    // the root itself, something beside it, something that climbs out of it
    assert!(confine(&vault, 0, &root).is_err());
    let outside = tempfile::tempdir().expect("outside");
    assert!(confine(&vault, 0, &outside.path().join("x.md")).is_err());
    assert!(confine(
        &vault,
        0,
        &root.join("docs").join("..").join("..").join("x")
    )
    .is_err());
    // a root that does not exist
    assert!(confine(&vault, 3, &inside_path).is_err());
}

/// A move whose ends are not inside the root (a forged one, or one built from a path that was valid when it
/// was checked) is refused right before the rename.
#[test]
fn execute_refuses_a_move_that_leaves_the_root_and_touches_nothing() {
    let (d, vault) = vault_with(&["a.md"]);
    let outside = tempfile::tempdir().expect("outside");
    let forged = ValidMove {
        from: "a.md".to_owned(),
        to: "x.md".to_owned(),
        abs_from: d.path().join("a.md"),
        abs_to: outside.path().join("x.md"),
        is_dir: false,
        root: 0,
        case_only: false,
    };
    let err = execute_moves(&vault, &OwnWrites::new(), &[forged]).expect_err("refused");
    assert_eq!(err.kind(), "pathEscape");
    assert!(d.path().join("a.md").exists() && !outside.path().join("x.md").exists());
}

/// A recycle bin that must never be asked.
struct NeverCalled;

impl Recycler for NeverCalled {
    fn recycle(&self, _: &[PathBuf]) -> Result<(), String> {
        panic!("the bin must not be asked");
    }
}

#[test]
fn execute_refuses_to_delete_what_is_outside_the_root() {
    let (_d, vault) = vault_with(&["a.md"]);
    let outside = tempfile::tempdir().expect("outside");
    fs::write(outside.path().join("secret.md"), "x").expect("write");
    let forged = ValidDelete {
        rel: "secret.md".to_owned(),
        abs: outside.path().join("secret.md"),
        is_dir: false,
        root: 0,
    };
    let err =
        execute_deletes(&vault, &OwnWrites::new(), &NeverCalled, &[forged]).expect_err("refused");
    assert_eq!(err.kind(), "pathEscape");
    assert!(outside.path().join("secret.md").exists());
}

#[test]
fn two_sources_that_differ_only_in_case_cannot_land_in_one_folder() {
    let (_d, vault) = vault_with(&["notes/Plan.md", "drafts/plan.md", "archive/keep.md"]);
    let both = [
        mv("notes/Plan.md", "archive/Plan.md"),
        mv("drafts/plan.md", "archive/plan.md"),
    ];
    assert!(matches!(
        check_moves(&vault, &both),
        Err(FileOpError::Exists(_))
    ));
    // one at a time each is fine
    assert!(check_moves(&vault, &both[..1]).is_ok());
    assert!(check_moves(&vault, &both[1..]).is_ok());
}

#[test]
fn destinations_are_compared_without_case_on_every_system() {
    // two different sources: the names of the sources do not differ only by case
    let at = |from: &str, to: &str| ValidMove {
        from: from.to_owned(),
        to: to.to_owned(),
        abs_from: PathBuf::from(from),
        abs_to: PathBuf::from(to),
        is_dir: false,
        root: 0,
        case_only: false,
    };
    assert!(matches!(
        check_together(&[
            at("notes/one.md", "archive/Plan.md"),
            at("drafts/two.md", "archive/plan.md")
        ]),
        Err(FileOpError::Exists(_))
    ));
    assert!(check_together(&[
        at("notes/one.md", "archive/Plan.md"),
        at("drafts/two.md", "archive/Other.md")
    ])
    .is_ok());
}

#[test]
fn a_destination_that_appears_after_the_check_is_never_replaced() {
    let (d, vault) = vault_with(&["a.md", "docs/b.md"]);
    let valid = check_moves(&vault, &[mv("a.md", "docs/c.md")]).expect("valid");
    // another program creates the file in the meantime
    fs::write(d.path().join("docs/c.md"), "theirs").expect("write");
    let err = execute_moves(&vault, &OwnWrites::new(), &valid).expect_err("refused");
    assert_eq!(err.kind(), "exists");
    assert_eq!(
        fs::read_to_string(d.path().join("docs/c.md")).expect("read"),
        "theirs"
    );
    assert!(d.path().join("a.md").exists());
}

#[test]
fn when_a_later_move_finds_its_destination_taken_the_earlier_ones_go_back() {
    let (d, vault) = vault_with(&["a.md", "b.md", "docs/x.md"]);
    let mut valid =
        check_moves(&vault, &[mv("a.md", "docs/a.md"), mv("b.md", "docs/b.md")]).expect("valid");
    // both end up wanting the same name (two sources that differ only in case on a volume that folds it)
    let (first_to, first_abs_to) = (valid[0].to.clone(), valid[0].abs_to.clone());
    valid[1].to = first_to;
    valid[1].abs_to = first_abs_to;
    let err = execute_moves(&vault, &OwnWrites::new(), &valid).expect_err("refused");
    assert_eq!(err.kind(), "exists");
    assert!(d.path().join("a.md").exists() && d.path().join("b.md").exists());
    assert!(!d.path().join("docs/a.md").exists());
}

#[test]
fn what_cannot_be_put_back_is_named_in_the_error() {
    let (d, vault) = vault_with(&["a.md", "b.md", "docs/x.md"]);
    let valid =
        check_moves(&vault, &[mv("a.md", "docs/a.md"), mv("b.md", "docs/b.md")]).expect("valid");
    let own = OwnWrites::new();
    // the second rename fails, and so does the one that would put the first back
    let calls = Cell::new(0);
    let flaky = |from: &Path, to: &Path| {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            fs::rename(from, to)
        } else {
            Err(io::Error::other("locked"))
        }
    };
    let err = run_moves(&vault, &own, &valid, &flaky).expect_err("failed");
    assert_eq!(err.kind(), "partlyMoved");
    let text = err.to_string();
    assert!(text.contains("docs/a.md"), "{text}");
    assert!(
        !d.path().join("a.md").exists() && d.path().join("docs/a.md").exists(),
        "a.md really stayed moved"
    );
    assert!(d.path().join("b.md").exists());
    // the announcement for the file that stayed moved is kept, the other one is taken back
    let removed = |path: &str| FsChange {
        kind: ChangeKind::Removed,
        path: path.to_owned(),
        from: None,
        at_ms: 1,
    };
    let mut changes = vec![removed("a.md"), removed("b.md")];
    own.strip(&mut changes, &SigStore::new());
    assert_eq!(changes, [removed("b.md")]);
}

#[test]
fn a_folder_that_holds_a_linked_worktree_cannot_be_moved_or_deleted() {
    let dir = tempfile::tempdir().expect("tempdir");
    for f in [
        "a.md",
        "agents/notes.md",
        "agents/trees/x/code.rs",
        "other/o.md",
    ] {
        let p = dir.path().join(f);
        fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        fs::write(p, "x").expect("write");
    }
    let vault = Vault::open(dir.path())
        .expect("open")
        .with_worktrees(vec![Worktree {
            name: "wt-x".to_owned(),
            path: dir.path().join("agents/trees/x"),
            branch: None,
        }]);
    assert_eq!(vault.worktrees().len(), 1);
    // every folder above the worktree
    for folder in ["agents", "agents/trees"] {
        let moved = check_moves(&vault, &[mv(folder, "elsewhere")]).expect_err(folder);
        assert_eq!(moved.kind(), "holdsWorktree", "{folder}");
        assert!(moved.to_string().contains("wt-x"), "{moved}");
        let deleted = check_deletes(&vault, &[folder.to_owned()]).expect_err(folder);
        assert_eq!(deleted.kind(), "holdsWorktree", "{folder}");
    }
    // folders and files beside it are free
    assert!(check_moves(&vault, &[mv("other", "other2")]).is_ok());
    assert!(check_moves(&vault, &[mv("agents/notes.md", "n.md")]).is_ok());
    assert!(check_deletes(&vault, &["other".to_owned()]).is_ok());
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_is_moved_itself_not_its_target() {
    let (d, vault) = vault_with(&["target.md"]);
    std::os::unix::fs::symlink(d.path().join("target.md"), d.path().join("link.md"))
        .expect("symlink");
    let valid = check_moves(&vault, &[mv("link.md", "renamed.md")]).expect("valid");
    // paths come from the vault's canonical root: the temp folder itself may be behind a link (`/var`)
    assert_eq!(valid[0].abs_from, vault.root().join("link.md"));
    assert!(!valid[0].is_dir);
}

#[cfg(unix)]
#[test]
fn a_link_that_leaves_the_project_cannot_be_used_to_reach_outside() {
    let (d, vault) = vault_with(&["a.md"]);
    let outside = tempfile::tempdir().expect("outside");
    fs::write(outside.path().join("secret.md"), "x").expect("write");
    std::os::unix::fs::symlink(outside.path(), d.path().join("out")).expect("symlink");
    assert!(check_moves(&vault, &[mv("a.md", "out/a.md")]).is_err());
    assert!(check_moves(&vault, &[mv("out/secret.md", "s.md")]).is_err());
    assert!(check_deletes(&vault, &["out/secret.md".to_owned()]).is_err());
}

#[test]
fn deletes_inside_a_deleted_folder_are_dropped_and_duplicates_collapse() {
    let (_d, vault) = vault_with(&["docs/a.md", "docs/sub/b.md", "c.md"]);
    let paths: Vec<String> = ["docs/sub/b.md", "docs", "docs/a.md", "c.md", "c.md"]
        .iter()
        .map(|p| (*p).to_owned())
        .collect();
    let valid = check_deletes(&vault, &paths).expect("valid");
    let rels: Vec<&str> = valid.iter().map(|d| d.rel.as_str()).collect();
    assert_eq!(rels, ["docs", "c.md"]);
    assert!(valid[0].is_dir && !valid[1].is_dir);
}

#[test]
fn the_files_of_a_folder_are_listed_with_their_vault_paths() {
    let (d, _v) = vault_with(&["docs/a.md", "docs/sub/b.md", "docs/sub/deep/c.md"]);
    let mut found = list_files(&d.path().join("docs"), "docs");
    found.sort();
    assert_eq!(found, ["docs/a.md", "docs/sub/b.md", "docs/sub/deep/c.md"]);
}

/// The real recycle bin of the system running the tests: ignored by default because it leaves a
/// (tiny) file in the user's bin. Run it with `cargo test -- --ignored os_recycle_bin`.
#[test]
#[ignore = "puts a file in the real recycle bin"]
fn os_recycle_bin_takes_a_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("permark-recycle-test.md");
    fs::write(&file, "test").expect("write");
    OsRecycler
        .recycle(std::slice::from_ref(&file))
        .expect("recycle");
    assert!(!file.exists());
}
