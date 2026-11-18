use std::path::{Path, PathBuf};
use std::sync::Arc;

use permark_core::document::Documents;
use permark_core::edit::lines;
use permark_core::fsx::Encoding;
use permark_core::paths::Vault;
use permark_core::ErrorKind;

use super::*;
use crate::dto::FileChangeKind as Kind;

struct Fx {
    project: tempfile::TempDir,
    profile: tempfile::TempDir,
    vault: Vault,
    documents: Arc<Documents>,
}

fn fx(files: &[(&str, &[u8])]) -> Fx {
    let project = tempfile::tempdir().unwrap();
    for (path, bytes) in files {
        put(project.path(), path, bytes);
    }
    let (vault, documents) = crate::testing::open(project.path());
    Fx {
        project,
        profile: tempfile::tempdir().unwrap(),
        vault,
        documents,
    }
}

fn put(base: &Path, path: &str, bytes: &[u8]) {
    let abs = base.join(path);
    std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
    std::fs::write(abs, bytes).unwrap();
}

impl Fx {
    fn access(&self) -> Project {
        crate::testing::project(&self.vault, &self.documents)
    }

    fn dir(&self) -> PathBuf {
        copy_dir(&self.profile.path().join("work"), self.vault.main())
    }

    fn build(&self) -> WorkCopy {
        WorkCopy::build(self.access(), self.dir()).unwrap()
    }

    fn real(&self, path: &str) -> Vec<u8> {
        std::fs::read(self.project.path().join(path)).unwrap()
    }

    fn put_real(&self, path: &str, bytes: &[u8]) {
        put(self.project.path(), path, bytes);
    }
}

fn copied(work: &WorkCopy, path: &str) -> Option<Vec<u8>> {
    std::fs::read(work.copy_path(path).unwrap()).ok()
}

fn edit_copy(work: &WorkCopy, path: &str, bytes: &[u8]) {
    put(work.dir(), path, bytes);
}

fn kinds(changes: &[FileChange]) -> Vec<(&str, Kind)> {
    changes.iter().map(|c| (c.path.as_str(), c.kind)).collect()
}

/// No change left to offer (the list shows on failure).
fn assert_none(changes: &[FileChange]) {
    let none: &[FileChange] = &[];
    assert_eq!(changes, none);
}

#[test]
fn agent_settings_and_machinery_are_kept_out() {
    for path in [
        ".claude/settings.json",
        "docs/.claude/settings.local.json",
        ".CLAUDE/Settings.json",
        ".mcp.json",
        "sub/.mcp.json",
        ".vscode/settings.json",
        "a/.codex/config.toml",
        ".gemini/x",
        ".cursor/rules/x.mdc",
        ".opencode/agent.md",
        "opencode.json",
        "tools/opencode.jsonc",
        ".git/config",
        ".git",
        "nested/.permark/config.toml",
        ".aiignore",
        "notes/.AIIGNORE",
        // settings | servers | hooks of other registry agents
        ".qwen/settings.json",
        "a/.goose/config.yaml",
        ".kilo/kilo.json",
        ".cline/x.json",
        ".amp/settings.json",
        ".kilocode/mcp.json",
        ".junie/mcp/mcp.json",
        ".kiro/settings/mcp.json",
        "docs/.kiro/hooks/on-save.kiro.hook",
        ".clinerules/hooks/pre.sh",
        ".factory/settings.json",
        ".factory/mcp.json",
        ".factory/hooks/x.sh",
        ".augment/settings.json",
        ".vibe/config.toml",
    ] {
        assert!(kept_out(path), "{path}");
    }
    for path in [
        "CLAUDE.md",
        ".claude/commands/review.md",
        "docs/settings.json",
        "vscode/notes.md",
        "settings.local.json",
        "mcp.json",
        // their instructions are documents
        ".kiro/steering/style.md",
        ".junie/guidelines.md",
        ".kilocode/rules/style.md",
        ".clinerules/style.md",
        ".factory/droids/reviewer.md",
        ".augment/rules/a.md",
        "QWEN.md",
        ".goosehints",
        "kiro/settings/x.md",
    ] {
        assert!(!kept_out(path), "{path}");
    }
}

/// `.aiignore`, deny paths, the project's machinery, agent settings and files past the size cap never reach the copy;
/// the copy lives in the profile, outside the project.
#[test]
fn the_copy_holds_only_what_an_agent_may_see() {
    let big = vec![b'x'; usize::try_from(MAX_FILE_BYTES).unwrap() + 1];
    let f = fx(&[
        ("README.md", b"# Hi\n"),
        ("docs/a.md", b"a\n"),
        (".aiignore", b"secret/\n"),
        ("secret/key.md", b"k\n"),
        ("private/p.md", b"p\n"),
        (
            ".permark/config.toml",
            b"[agent]\ndeny_paths = [\"private/\"]\n",
        ),
        (".git/config", b"[core]\n"),
        (".claude/settings.json", b"{}"),
        (".claude/commands/x.md", b"x\n"),
        (".mcp.json", b"{}"),
        (".vscode/settings.json", b"{}"),
        (".kiro/settings/mcp.json", b"{}"),
        (
            ".kiro/steering/style.md",
            b"s
",
        ),
        (".qwen/settings.json", b"{}"),
        ("big.txt", &big),
    ]);
    let work = f.build();
    assert!(work.dir().starts_with(f.profile.path()));
    assert!(!work.dir().starts_with(f.project.path()));
    for kept in [
        "README.md",
        "docs/a.md",
        ".claude/commands/x.md",
        ".kiro/steering/style.md",
    ] {
        assert_eq!(copied(&work, kept), Some(f.real(kept)), "{kept}");
    }
    for out in [
        ".aiignore",
        "secret/key.md",
        "private/p.md",
        ".permark/config.toml",
        ".git/config",
        ".claude/settings.json",
        ".mcp.json",
        ".vscode/settings.json",
        ".kiro/settings/mcp.json",
        ".qwen/settings.json",
        "big.txt",
    ] {
        assert_eq!(copied(&work, out), None, "{out}");
    }
    assert_eq!(work.skipped(), 1);
    assert!(f.build_again_is_clean());
}

impl Fx {
    /// A second build over a copy left behind starts from scratch (a crash leaves the folder).
    fn build_again_is_clean(&self) -> bool {
        put(&self.dir(), "left-over.md", b"old\n");
        let mut work = self.build();
        copied(&work, "left-over.md").is_none() && work.changes().unwrap().is_empty()
    }
}

#[test]
fn a_copy_inside_the_project_is_refused() {
    let f = fx(&[("a.md", b"a\n")]);
    let inside = f.project.path().join("work");
    let error = WorkCopy::build(f.access(), inside).err().unwrap();
    assert_eq!(error.kind, ErrorKind::InvalidPath);
}

#[test]
fn copy_paths_stay_inside_the_copy() {
    let f = fx(&[("docs/a.md", b"a\n")]);
    let work = f.build();
    assert_eq!(
        work.copy_path("docs/a.md").unwrap(),
        work.dir().join("docs").join("a.md")
    );
    assert!(work.copy_path("../outside.md").is_err());
    assert!(work.copy_path("").is_err());
}

#[test]
fn each_kind_of_change_is_listed() {
    let f = fx(&[
        ("edited.md", b"one\ntwo\n"),
        ("gone.md", b"bye\n"),
        ("both.md", b"base\n"),
        ("pic.bin", b"\x00\x01\x02"),
        ("same.md", b"keep\n"),
    ]);
    let mut work = f.build();
    assert_none(&work.changes().unwrap());
    edit_copy(&work, "edited.md", b"one\nTWO\n");
    edit_copy(&work, "new.md", b"hello\n");
    std::fs::remove_file(work.copy_path("gone.md").unwrap()).unwrap();
    edit_copy(&work, "both.md", b"agent\n");
    f.put_real("both.md", b"person\n");
    edit_copy(&work, "pic.bin", b"\x00\x09");
    // line endings alone are no change: the real file keeps its own
    edit_copy(&work, "same.md", b"keep\r\n");
    // the agent's own settings and machinery made in the copy are never offered back
    edit_copy(&work, ".claude/settings.local.json", b"{}");
    edit_copy(&work, ".git/hooks/post-checkout", b"#!/bin/sh\n");
    let changes = work.changes().unwrap();
    assert_eq!(
        kinds(&changes),
        [
            ("both.md", Kind::Conflict),
            ("edited.md", Kind::Modified),
            ("gone.md", Kind::Deleted),
            ("new.md", Kind::Added),
            ("pic.bin", Kind::Binary),
        ]
    );
    let edited = &changes[1].hunks;
    assert_eq!(edited.len(), 1);
    assert_eq!(edited[0].id, 0);
    assert_eq!(edited[0].old_start, 2);
    assert_eq!(edited[0].old_lines, ["two"]);
    assert_eq!(edited[0].new_lines, ["TWO"]);
    assert_eq!(changes[3].hunks[0].new_lines[0], "hello");
    assert_eq!(changes[2].hunks[0].old_lines[0], "bye");
    assert!(changes[0].hunks.is_empty() && changes[4].hunks.is_empty());
}

/// One hunk of two lands: every other byte of the real file stays, CRLF breaks included (the agent wrote LF).
#[test]
fn accepting_one_hunk_keeps_every_other_byte() {
    let real = b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n";
    let f = fx(&[("notes.md", real)]);
    let mut work = f.build();
    edit_copy(&work, "notes.md", b"one\nTWO\nthree\nfour\nFIVE\nsix\n");
    let changes = work.changes().unwrap();
    assert_eq!(changes[0].hunks.len(), 2);
    let after = work.accept("notes.md", Some(&[1])).unwrap();
    assert_eq!(
        f.real("notes.md"),
        b"one\r\ntwo\r\nthree\r\nfour\r\nFIVE\r\nsix\r\n"
    );
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].hunks.len(), 1);
    assert_eq!(after[0].hunks[0].new_lines, ["TWO"]);
    let after = work.accept("notes.md", None).unwrap();
    assert_none(&after);
    assert_eq!(
        f.real("notes.md"),
        b"one\r\nTWO\r\nthree\r\nfour\r\nFIVE\r\nsix\r\n"
    );
    // the copy took the real bytes: nothing left to offer, nothing to sync
    assert_eq!(copied(&work, "notes.md"), Some(f.real("notes.md")));
}

#[test]
fn a_utf16_file_keeps_its_encoding() {
    let real = lines::encode("a\r\nb\r\n", Encoding::Utf16Le);
    let f = fx(&[("w.txt", &real)]);
    let mut work = f.build();
    edit_copy(&work, "w.txt", b"a\nB\n");
    let changes = work.changes().unwrap();
    assert_eq!(kinds(&changes), [("w.txt", Kind::Modified)]);
    work.accept("w.txt", Some(&[0])).unwrap();
    assert_eq!(
        f.real("w.txt"),
        lines::encode("a\r\nB\r\n", Encoding::Utf16Le)
    );
}

/// Text the app cannot decode without loss (Latin-1) is changed as a whole file only.
#[test]
fn undecodable_text_is_a_whole_file_choice() {
    let f = fx(&[("old.txt", b"caf\xe9\n")]);
    let mut work = f.build();
    edit_copy(&work, "old.txt", "café!\n".as_bytes());
    assert_eq!(kinds(&work.changes().unwrap()), [("old.txt", Kind::Binary)]);
    let error = work.accept("old.txt", Some(&[0])).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidData);
    assert_eq!(f.real("old.txt"), b"caf\xe9\n");
    assert_none(&work.accept("old.txt", None).unwrap());
    assert_eq!(f.real("old.txt"), "café!\n".as_bytes());
}

#[test]
fn accept_is_refused_while_the_document_is_unsaved() {
    let f = fx(&[("a.md", b"a\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"A\n");
    work.changes().unwrap();
    f.documents.set_unsaved("w1", Some("a.md".to_owned()));
    let error = work.accept("a.md", Some(&[0])).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(error.detail.as_deref(), Some("unsaved"));
    assert_eq!(f.real("a.md"), b"a\n");
    f.documents.set_unsaved("w1", None);
    work.accept("a.md", Some(&[0])).unwrap();
    assert_eq!(f.real("a.md"), b"A\n");
}

/// The real file changed after the copy was taken: no hunk lands; the whole file replaces only the version the
/// person saw listed.
#[test]
fn accept_is_refused_when_the_real_file_changed() {
    let f = fx(&[("a.md", b"a\nb\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"a\nB\n");
    assert_eq!(kinds(&work.changes().unwrap()), [("a.md", Kind::Modified)]);
    f.put_real("a.md", b"x\nb\n");
    let error = work.accept("a.md", Some(&[0])).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(f.real("a.md"), b"x\nb\n");
    // listed as a conflict now; a later change of the real file refuses the whole-file accept too
    assert_eq!(kinds(&work.changes().unwrap()), [("a.md", Kind::Conflict)]);
    f.put_real("a.md", b"y\nb\n");
    let error = work.accept("a.md", None).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(f.real("a.md"), b"y\nb\n");
    work.changes().unwrap();
    assert_none(&work.accept("a.md", None).unwrap());
    assert_eq!(f.real("a.md"), b"a\nB\n");
}

#[test]
fn reject_puts_the_real_lines_back_in_the_copy() {
    let f = fx(&[("a.md", b"1\n2\n3\n4\n5\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"1\nTWO\n3\n4\nFIVE\n");
    let after = work.reject("a.md", Some(&[0])).unwrap();
    assert_eq!(copied(&work, "a.md").unwrap(), b"1\n2\n3\n4\nFIVE\n");
    assert_eq!(after[0].hunks.len(), 1);
    assert_eq!(after[0].hunks[0].new_lines, ["FIVE"]);
    assert_none(&work.reject("a.md", None).unwrap());
    assert_eq!(copied(&work, "a.md"), Some(f.real("a.md")));
    assert_eq!(f.real("a.md"), b"1\n2\n3\n4\n5\n");
}

#[test]
fn an_added_file_lands_only_where_nothing_is() {
    let f = fx(&[("a.md", b"a\n")]);
    let mut work = f.build();
    edit_copy(&work, "docs/new.md", b"new\n");
    edit_copy(&work, "late.md", b"agent\n");
    edit_copy(&work, "drop.md", b"drop\n");
    work.changes().unwrap();
    assert_eq!(kinds(&work.accept("docs/new.md", None).unwrap()).len(), 2);
    assert_eq!(f.real("docs/new.md"), b"new\n");
    // the person made a file at that path since: never replaced
    f.put_real("late.md", b"person\n");
    let error = work.accept("late.md", None).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Exists);
    assert_eq!(f.real("late.md"), b"person\n");
    let after = work.reject("drop.md", None).unwrap();
    assert_eq!(copied(&work, "drop.md"), None);
    assert_eq!(kinds(&after), [("late.md", Kind::Added)]);
}

#[test]
fn a_deleted_file_goes_to_the_recycle_bin_on_accept() {
    let f = fx(&[("gone.md", b"bye\n"), ("back.md", b"back\n")]);
    let mut work = f.build();
    std::fs::remove_file(work.copy_path("gone.md").unwrap()).unwrap();
    std::fs::remove_file(work.copy_path("back.md").unwrap()).unwrap();
    work.changes().unwrap();
    let after = work.accept("gone.md", None).unwrap();
    assert!(!f.project.path().join("gone.md").exists());
    assert_eq!(kinds(&after), [("back.md", Kind::Deleted)]);
    assert_none(&work.reject("back.md", None).unwrap());
    assert_eq!(copied(&work, "back.md").unwrap(), b"back\n");
}

/// Before a prompt: the person's edits reach the copy where the agent did not touch the file; where both changed,
/// the agent's version stays and shows as a conflict.
#[test]
fn sync_brings_the_persons_edits_and_keeps_both_changed() {
    let f = fx(&[
        ("edited.md", b"v1\n"),
        ("removed.md", b"r\n"),
        ("both.md", b"base\n"),
        ("dropped.md", b"old\n"),
    ]);
    let mut work = f.build();
    f.put_real("edited.md", b"v2\n");
    f.put_real("added.md", b"fresh\n");
    std::fs::remove_file(f.project.path().join("removed.md")).unwrap();
    edit_copy(&work, "both.md", b"agent\n");
    f.put_real("both.md", b"person\n");
    // the agent deleted what the person edited
    std::fs::remove_file(work.copy_path("dropped.md").unwrap()).unwrap();
    f.put_real("dropped.md", b"new\n");
    work.sync().unwrap();
    assert_eq!(copied(&work, "edited.md").unwrap(), b"v2\n");
    assert_eq!(copied(&work, "added.md").unwrap(), b"fresh\n");
    assert_eq!(copied(&work, "removed.md"), None);
    assert_eq!(copied(&work, "both.md").unwrap(), b"agent\n");
    assert_eq!(copied(&work, "dropped.md"), None);
    assert_eq!(
        kinds(&work.changes().unwrap()),
        [("both.md", Kind::Conflict), ("dropped.md", Kind::Conflict)]
    );
    // rejecting a conflict takes the person's version
    assert_eq!(work.reject("both.md", None).unwrap().len(), 1);
    assert_eq!(copied(&work, "both.md").unwrap(), b"person\n");
    assert_none(&work.reject("dropped.md", None).unwrap());
    assert_eq!(copied(&work, "dropped.md").unwrap(), b"new\n");
}

#[test]
fn removing_the_copy_leaves_nothing() {
    let f = fx(&[("a.md", b"a\n")]);
    let work = f.build();
    let dir = work.dir().to_path_buf();
    assert!(dir.join("a.md").exists());
    work.remove().unwrap();
    assert!(!dir.exists());
    assert_eq!(f.real("a.md"), b"a\n");
}

/// Agent settings | machinery the agent made in the copy go, at any depth, read-only ones too; everything else
/// stays (its instructions, its new files).
#[test]
fn purge_removes_what_is_kept_out_and_nothing_else() {
    let f = fx(&[("a.md", b"a\n")]);
    let work = f.build();
    let dir = work.dir().to_path_buf();
    for path in [
        ".claude/settings.json",
        ".claude/settings.local.json",
        ".claude/commands/tidy.md",
        ".mcp.json",
        "docs/opencode.json",
        "docs/.gemini/settings.json",
        ".codex/config.toml",
        ".kiro/hooks/run.json",
        ".kiro/steering/style.md",
        "sub/.git/hooks/pre-commit",
        ".permark/config.toml",
        "notes/.aiignore",
        "new.md",
    ] {
        put(&dir, path, b"x");
    }
    let locked = dir.join(".mcp.json");
    let mut permissions = std::fs::metadata(&locked).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&locked, permissions).unwrap();
    work.purge().unwrap();
    let mut left: Vec<String> = ignore::WalkBuilder::new(&dir)
        .standard_filters(false)
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| {
            let rel = e.path().strip_prefix(&dir).unwrap();
            rel.to_string_lossy().replace('\\', "/")
        })
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            ".claude/commands/tidy.md",
            ".kiro/steering/style.md",
            "a.md",
            "new.md"
        ]
    );
    for gone in [".codex", ".git", "sub/.git", "docs/.gemini", ".permark"] {
        assert!(!dir.join(gone).exists(), "{gone}");
    }
}

/// A link in the copy is never followed: one named like an agent's settings folder is removed itself, and what any
/// link leads to stays untouched.
#[test]
fn purge_never_follows_a_link() {
    let f = fx(&[("a.md", b"a\n")]);
    let work = f.build();
    let dir = work.dir().to_path_buf();
    let target = tempfile::tempdir().unwrap();
    put(target.path(), "settings.json", b"{}");
    put(target.path(), ".mcp.json", b"{}");
    #[cfg(windows)]
    let linked = |link: &Path| std::os::windows::fs::symlink_dir(target.path(), link);
    #[cfg(unix)]
    let linked = |link: &Path| std::os::unix::fs::symlink(target.path(), link);
    if linked(&dir.join(".claude")).is_err() {
        // links need a privilege this machine does not give
        return;
    }
    linked(&dir.join("docs")).unwrap();
    work.purge().unwrap();
    assert!(std::fs::symlink_metadata(dir.join(".claude")).is_err());
    assert!(std::fs::symlink_metadata(dir.join("docs")).is_ok());
    assert!(target.path().join("settings.json").exists());
    assert!(target.path().join(".mcp.json").exists());
}
