use std::fs;

use super::*;

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
    assert!(is_protected(".trailmark/meta.toml"));
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

#[cfg(unix)]
#[test]
fn a_symbolic_link_is_moved_itself_not_its_target() {
    let (d, vault) = vault_with(&["target.md"]);
    std::os::unix::fs::symlink(d.path().join("target.md"), d.path().join("link.md"))
        .expect("symlink");
    let valid = check_moves(&vault, &[mv("link.md", "renamed.md")]).expect("valid");
    assert_eq!(valid[0].abs_from, d.path().join("link.md"));
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
    let file = dir.path().join("trailmark-recycle-test.md");
    fs::write(&file, "test").expect("write");
    OsRecycler
        .recycle(std::slice::from_ref(&file))
        .expect("recycle");
    assert!(!file.exists());
}
