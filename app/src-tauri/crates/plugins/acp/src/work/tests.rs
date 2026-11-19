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

/// The keys of hunks `at` of `path` in `changes`.
fn keys(changes: &[FileChange], path: &str, at: &[usize]) -> Vec<String> {
    let file = changes.iter().find(|c| c.path == path).unwrap();
    at.iter().map(|i| file.hunks[*i].key.clone()).collect()
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
        // what grants tools | modes | servers or runs a program: subagents, skills, commands, hooks
        ".claude/agents/helper.md",
        "docs/.claude/skills/tidy/SKILL.md",
        ".claude/commands/review.md",
        ".claude/hooks/pre.sh",
        ".factory/droids/reviewer.md",
        ".factory/commands/x.md",
        ".kiro/agents/a.json",
    ] {
        assert!(kept_out(path), "{path}");
    }
    for path in [
        "CLAUDE.md",
        ".claude/rules/style.md",
        "docs/settings.json",
        "vscode/notes.md",
        "settings.local.json",
        "mcp.json",
        // their instructions are documents
        ".kiro/steering/style.md",
        ".junie/guidelines.md",
        ".kilocode/rules/style.md",
        ".clinerules/style.md",
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
        (".claude/rules/x.md", b"x\n"),
        (
            ".claude/agents/helper.md",
            b"---\npermissionMode: bypassPermissions\n---\n",
        ),
        (
            ".claude/skills/tidy/SKILL.md",
            b"---\nallowed-tools: Bash\n---\n",
        ),
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
        ".claude/rules/x.md",
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
        ".claude/agents/helper.md",
        ".claude/skills/tidy/SKILL.md",
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
        copied(&work, "left-over.md").is_none() && work.refresh().unwrap().files.is_empty()
    }
}

#[test]
fn a_copy_inside_the_project_is_refused() {
    let f = fx(&[("a.md", b"a\n")]);
    let inside = f.project.path().join("work");
    let error = WorkCopy::build(f.access(), inside).err().unwrap();
    assert_eq!(error.kind, ErrorKind::InvalidPath);
}

/// A copy folder spelled through a link (macOS temp folders sit behind /var -> /private/var; Windows temp folders can
/// carry 8.3 names) is still recognised as inside the project.
#[test]
fn a_copy_inside_the_project_through_a_link_is_refused() {
    let f = fx(&[("a.md", b"a\n")]);
    let alias = f.profile.path().join("alias");
    if link(f.project.path(), &alias).is_err() {
        return; // the system refused a folder link here: nothing to spell through
    }
    let error = WorkCopy::build(f.access(), alias.join("work")).err();
    assert_eq!(error.map(|e| e.kind), Some(ErrorKind::InvalidPath));
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
    assert_none(&work.changes().unwrap().files);
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
    // the list is computed once, then kept until asked again
    assert_none(&work.changes().unwrap().files);
    let list = work.refresh().unwrap();
    assert_eq!(list.more, 0);
    let changes = list.files;
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
    assert_eq!(work.changes().unwrap().files, changes);
    let edited = &changes[1].hunks;
    assert_eq!(edited.len(), 1);
    assert!(!edited[0].key.is_empty() && !edited[0].truncated);
    assert_eq!(edited[0].old_start, 2);
    assert_eq!(edited[0].old_lines, ["two"]);
    assert_eq!(edited[0].new_lines, ["TWO"]);
    assert_eq!(changes[3].hunks[0].new_lines[0], "hello");
    assert_eq!(changes[2].hunks[0].old_lines[0], "bye");
    assert!(changes[0].hunks.is_empty() && changes[4].hunks.is_empty());
}

/// Each run carries the real file's line above and below it; the empty piece after a final line break is no line.
#[test]
fn a_hunk_shows_one_real_line_around_it() {
    let f = fx(&[
        ("notes.md", b"one\ntwo\nthree\nfour\nfive\n"),
        ("open.md", b"x\ny"),
        ("gone.md", b"bye\n"),
    ]);
    let mut work = f.build();
    edit_copy(&work, "notes.md", b"ONE\ntwo\nTHREE\nfour\nfive\nsix\n");
    edit_copy(&work, "open.md", b"x\nY");
    edit_copy(&work, "new.md", b"hello\n");
    std::fs::remove_file(work.copy_path("gone.md").unwrap()).unwrap();
    let changes = work.refresh().unwrap().files;
    let around = |path: &str| -> Vec<(Option<String>, Option<String>)> {
        let file = changes.iter().find(|c| c.path == path).unwrap();
        file.hunks
            .iter()
            .map(|h| (h.context_before.clone(), h.context_after.clone()))
            .collect()
    };
    let line = |s: &str| Some(s.to_owned());
    assert_eq!(
        around("notes.md"),
        [
            (None, line("two")),
            (line("two"), line("four")),
            (line("five"), None),
        ]
    );
    assert_eq!(around("open.md"), [(line("x"), None)]);
    // a whole new | deleted file has no real line around it
    assert_eq!(around("new.md"), [(None, None)]);
    assert_eq!(around("gone.md"), [(None, None)]);
}

/// One hunk of two lands: every other byte of the real file stays, CRLF breaks included (the agent wrote LF).
#[test]
fn accepting_one_hunk_keeps_every_other_byte() {
    let real = b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n";
    let f = fx(&[("notes.md", real)]);
    let mut work = f.build();
    edit_copy(&work, "notes.md", b"one\nTWO\nthree\nfour\nFIVE\nsix\n");
    let changes = work.refresh().unwrap().files;
    assert_eq!(changes[0].hunks.len(), 2);
    let after = work
        .accept("notes.md", Some(&keys(&changes, "notes.md", &[1])))
        .unwrap()
        .files;
    assert_eq!(
        f.real("notes.md"),
        b"one\r\ntwo\r\nthree\r\nfour\r\nFIVE\r\nsix\r\n"
    );
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].hunks.len(), 1);
    assert_eq!(after[0].hunks[0].new_lines, ["TWO"]);
    // the hunk above the accepted one kept its place: same key
    assert_eq!(after[0].hunks[0].key, changes[0].hunks[0].key);
    let after = work.accept("notes.md", None).unwrap();
    assert_none(&after.files);
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
    let changes = work.refresh().unwrap().files;
    assert_eq!(kinds(&changes), [("w.txt", Kind::Modified)]);
    work.accept("w.txt", Some(&keys(&changes, "w.txt", &[0])))
        .unwrap();
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
    assert_eq!(
        kinds(&work.refresh().unwrap().files),
        [("old.txt", Kind::Binary)]
    );
    // no hunk to name: a key can only come from an older list
    let error = work
        .accept("old.txt", Some(&["0123456789abcdef".to_owned()]))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(error.detail.as_deref(), Some("outdated"));
    assert_eq!(f.real("old.txt"), b"caf\xe9\n");
    assert_none(&work.accept("old.txt", None).unwrap().files);
    assert_eq!(f.real("old.txt"), "café!\n".as_bytes());
}

#[test]
fn accept_is_refused_while_the_document_is_unsaved() {
    let f = fx(&[("a.md", b"a\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"A\n");
    let changes = work.refresh().unwrap().files;
    let key = keys(&changes, "a.md", &[0]);
    f.documents.set_unsaved("w1", Some("a.md".to_owned()));
    let error = work.accept("a.md", Some(&key)).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(error.detail.as_deref(), Some("unsaved"));
    assert_eq!(f.real("a.md"), b"a\n");
    f.documents.set_unsaved("w1", None);
    work.accept("a.md", Some(&key)).unwrap();
    assert_eq!(f.real("a.md"), b"A\n");
}

/// The real file changed after the copy was taken: no hunk lands; the whole file replaces only the version the
/// person saw listed.
#[test]
fn accept_is_refused_when_the_real_file_changed() {
    let f = fx(&[("a.md", b"a\nb\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"a\nB\n");
    let changes = work.refresh().unwrap().files;
    assert_eq!(kinds(&changes), [("a.md", Kind::Modified)]);
    f.put_real("a.md", b"x\nb\n");
    let error = work
        .accept("a.md", Some(&keys(&changes, "a.md", &[0])))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(f.real("a.md"), b"x\nb\n");
    // listed as a conflict now (the refusal updated the kept list); a later change of the real file refuses the
    // whole-file accept too
    assert_eq!(
        kinds(&work.changes().unwrap().files),
        [("a.md", Kind::Conflict)]
    );
    f.put_real("a.md", b"y\nb\n");
    let error = work.accept("a.md", None).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(f.real("a.md"), b"y\nb\n");
    assert_none(&work.accept("a.md", None).unwrap().files);
    assert_eq!(f.real("a.md"), b"a\nB\n");
}

#[test]
fn reject_puts_the_real_lines_back_in_the_copy() {
    let f = fx(&[("a.md", b"1\n2\n3\n4\n5\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"1\nTWO\n3\n4\nFIVE\n");
    let changes = work.refresh().unwrap().files;
    let after = work
        .reject("a.md", Some(&keys(&changes, "a.md", &[0])))
        .unwrap()
        .files;
    assert_eq!(copied(&work, "a.md").unwrap(), b"1\n2\n3\n4\nFIVE\n");
    assert_eq!(after[0].hunks.len(), 1);
    assert_eq!(after[0].hunks[0].new_lines, ["FIVE"]);
    assert_none(&work.reject("a.md", None).unwrap().files);
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
    work.refresh().unwrap();
    assert_eq!(
        kinds(&work.accept("docs/new.md", None).unwrap().files).len(),
        2
    );
    assert_eq!(f.real("docs/new.md"), b"new\n");
    // the person made a file at that path since: never replaced
    f.put_real("late.md", b"person\n");
    let error = work.accept("late.md", None).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Exists);
    assert_eq!(f.real("late.md"), b"person\n");
    let after = work.reject("drop.md", None).unwrap().files;
    assert_eq!(copied(&work, "drop.md"), None);
    assert_eq!(kinds(&after), [("late.md", Kind::Added)]);
}

#[test]
fn a_deleted_file_goes_to_the_recycle_bin_on_accept() {
    let f = fx(&[("gone.md", b"bye\n"), ("back.md", b"back\n")]);
    let mut work = f.build();
    std::fs::remove_file(work.copy_path("gone.md").unwrap()).unwrap();
    std::fs::remove_file(work.copy_path("back.md").unwrap()).unwrap();
    work.refresh().unwrap();
    let after = work.accept("gone.md", None).unwrap().files;
    assert!(!f.project.path().join("gone.md").exists());
    assert_eq!(kinds(&after), [("back.md", Kind::Deleted)]);
    assert_none(&work.reject("back.md", None).unwrap().files);
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
        kinds(&work.refresh().unwrap().files),
        [("both.md", Kind::Conflict), ("dropped.md", Kind::Conflict)]
    );
    // rejecting a conflict takes the person's version
    assert_eq!(work.reject("both.md", None).unwrap().files.len(), 1);
    assert_eq!(copied(&work, "both.md").unwrap(), b"person\n");
    assert_none(&work.reject("dropped.md", None).unwrap().files);
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
        ".claude/skills/tidy/SKILL.md",
        ".claude/rules/tidy.md",
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
            ".claude/rules/tidy.md",
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
    if link(target.path(), &dir.join(".claude")).is_err() {
        // links need a privilege this machine does not give
        return;
    }
    link(target.path(), &dir.join("docs")).unwrap();
    work.purge().unwrap();
    assert!(std::fs::symlink_metadata(dir.join(".claude")).is_err());
    assert!(std::fs::symlink_metadata(dir.join("docs")).is_ok());
    assert!(target.path().join("settings.json").exists());
    assert!(target.path().join(".mcp.json").exists());
}

/// A hunk is named by its content and place: an older list's key never lands on another hunk.
#[test]
fn a_key_from_an_older_list_is_refused() {
    let f = fx(&[("a.md", b"1\n2\n3\n4\n5\n6\n7\n8\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"1\n2\n3\n4\n5\n6\n7\nEIGHT\n");
    let old = work.refresh().unwrap().files;
    // the agent works on: a new change above, and the one shown changed again
    edit_copy(&work, "a.md", b"ONE\n2\n3\n4\n5\n6\n7\nVIII\n");
    let error = work
        .accept("a.md", Some(&keys(&old, "a.md", &[0])))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(error.detail.as_deref(), Some("outdated"));
    assert_eq!(f.real("a.md"), b"1\n2\n3\n4\n5\n6\n7\n8\n");
    // the kept list was brought up to date for that file: its keys apply
    let fresh = work.changes().unwrap().files;
    assert_eq!(fresh[0].hunks.len(), 2);
    assert_eq!(fresh[0].hunks[1].new_lines, ["VIII"]);
    work.accept("a.md", Some(&keys(&fresh, "a.md", &[1])))
        .unwrap();
    assert_eq!(f.real("a.md"), b"1\n2\n3\n4\n5\n6\n7\nVIII\n");
    // an empty choice names nothing
    let error = work.reject("a.md", Some(&[])).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidData);
}

/// A long hunk reaches the window cut to its first lines; a choice still applies all of it.
#[test]
fn a_long_hunk_is_cut_for_the_window_and_applied_whole() {
    let lines = |word: &str, count: usize| -> String {
        let lines: Vec<String> = (0..count).map(|i| format!("{word} {i}\n")).collect();
        lines.concat()
    };
    let old = lines("old", 600);
    let new = lines("new", 900);
    let f = fx(&[("long.md", old.as_bytes())]);
    let mut work = f.build();
    edit_copy(&work, "long.md", new.as_bytes());
    edit_copy(&work, "added.md", new.as_bytes());
    let changes = work.refresh().unwrap().files;
    for file in &changes {
        let hunk = &file.hunks[0];
        assert!(hunk.truncated, "{}", file.path);
        assert!(hunk.old_lines.len() <= MAX_HUNK_LINES);
        assert_eq!(hunk.new_lines.len(), MAX_HUNK_LINES);
        assert_eq!(hunk.new_lines[0], "new 0");
    }
    work.accept("long.md", Some(&keys(&changes, "long.md", &[0])))
        .unwrap();
    assert_eq!(f.real("long.md"), new.as_bytes());
    work.accept("added.md", Some(&keys(&changes, "added.md", &[0])))
        .unwrap();
    assert_eq!(f.real("added.md"), new.as_bytes());
}

/// A very long line reaches the window cut to its first bytes (whole characters); a choice still applies all of it.
#[test]
fn a_long_line_is_cut_for_the_window_and_applied_whole() {
    let long = format!("{}\n", "é".repeat(MAX_LINE_BYTES));
    let f = fx(&[("a.md", b"short\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", long.as_bytes());
    edit_copy(&work, "added.md", long.as_bytes());
    let changes = work.refresh().unwrap().files;
    assert_eq!(changes.len(), 2);
    for file in &changes {
        let hunk = &file.hunks[0];
        assert!(hunk.truncated, "{}", file.path);
        let shown = &hunk.new_lines[0];
        assert!(shown.len() <= MAX_LINE_BYTES, "{}", shown.len());
        assert!(shown.len() > MAX_LINE_BYTES - 4, "{}", shown.len());
        assert!(long.starts_with(shown.as_str()));
    }
    let short = changes.iter().find(|c| c.path == "a.md").unwrap();
    assert_eq!(short.hunks[0].old_lines, ["short"]);
    work.accept("a.md", Some(&keys(&changes, "a.md", &[0])))
        .unwrap();
    assert_eq!(f.real("a.md"), long.as_bytes());
}

/// At most [`MAX_LISTED`] files are listed; the rest are counted.
#[test]
fn a_long_list_is_cut_and_counted() {
    let f = fx(&[("a.md", b"a\n")]);
    let mut work = f.build();
    for i in 0..MAX_LISTED + 3 {
        edit_copy(&work, &format!("new/{i:04}.md"), b"x\n");
    }
    let list = work.refresh().unwrap();
    assert_eq!(list.files.len(), MAX_LISTED);
    assert_eq!(list.more, 3);
    assert_eq!(list.count(), u32::try_from(MAX_LISTED + 3).unwrap());
    let list = work.reject("new/0000.md", None).unwrap();
    assert_eq!(list.files.len(), MAX_LISTED);
    assert_eq!(list.more, 2);
    assert_eq!(list.files[0].path, "new/0001.md");
}

/// A copy of long new files: their name by number, the text of each (one run of lines as long as a hunk shows), the
/// work copy holding the first, and how many such files one list carries.
fn long_files() -> (Fx, WorkCopy, String, usize) {
    let big = format!("{}\n", "x".repeat(4000)).repeat(MAX_HUNK_LINES);
    let f = fx(&[("a.md", b"a\n")]);
    let mut work = f.build();
    edit_copy(&work, &long_name(0), big.as_bytes());
    let fit = MAX_LISTED_BYTES / carried(&work.refresh().unwrap().files[0]);
    assert!(fit > 1 && fit < MAX_LISTED, "{fit}");
    (f, work, big, fit)
}

fn long_name(i: usize) -> String {
    format!("big/{i:02}.md")
}

fn listed_paths(list: &ChangeList) -> Vec<String> {
    list.files.iter().map(|f| f.path.clone()).collect()
}

fn listed_bytes(list: &ChangeList) -> usize {
    list.files.iter().map(carried).sum()
}

/// At most [`MAX_LISTED_BYTES`] of lines are listed: the first files by path, the rest counted; a choice that frees
/// room lets the next file move up.
#[test]
fn a_list_of_long_files_is_cut_by_its_bytes_and_counted() {
    let (_f, mut work, big, fit) = long_files();
    for i in 1..fit + 3 {
        edit_copy(&work, &long_name(i), big.as_bytes());
    }
    let list = work.refresh().unwrap();
    assert_eq!(
        listed_paths(&list),
        (0..fit).map(long_name).collect::<Vec<_>>()
    );
    assert_eq!(list.more, 3);
    assert!(listed_bytes(&list) <= MAX_LISTED_BYTES);
    let list = work.reject(&long_name(0), None).unwrap();
    assert_eq!(
        listed_paths(&list),
        (1..=fit).map(long_name).collect::<Vec<_>>()
    );
    assert_eq!(list.more, 2);
}

/// A file a choice brings back into a full list (Undo) takes it past its bytes: the list is computed whole and the
/// file past them is counted.
#[test]
fn a_file_listed_again_past_the_bytes_of_the_list_is_counted() {
    let (_f, mut work, big, fit) = long_files();
    for i in 1..fit {
        edit_copy(&work, &long_name(i), big.as_bytes());
    }
    assert_eq!(work.refresh().unwrap().files.len(), fit);
    work.accept(&long_name(0), None).unwrap();
    edit_copy(&work, "big/zz.md", big.as_bytes());
    let list = work.refresh().unwrap();
    assert_eq!((list.files.len(), list.more), (fit, 0));
    let list = work.undo(&long_name(0)).unwrap();
    assert!(listed_bytes(&list) <= MAX_LISTED_BYTES);
    assert_eq!(list.files[0].path, long_name(0));
    assert_eq!((list.files.len(), list.more), (fit, 1));
}

/// After a choice only that file is looked at again: the rest of the kept list stays as it was computed.
#[test]
fn a_choice_updates_only_its_own_file() {
    let f = fx(&[("a.md", b"a\n"), ("b.md", b"b\n"), ("c.md", b"c\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"A\n");
    edit_copy(&work, "b.md", b"B\n");
    work.refresh().unwrap();
    edit_copy(&work, "c.md", b"C\n");
    let after = work.accept("a.md", None).unwrap();
    assert_eq!(kinds(&after.files), [("b.md", Kind::Modified)]);
    assert_eq!(
        kinds(&work.refresh().unwrap().files),
        [("b.md", Kind::Modified), ("c.md", Kind::Modified)]
    );
}

/// Undo puts back what the last accept of a file wrote, then lists the change again.
#[test]
fn undo_puts_back_what_an_accept_did() {
    let real = b"one\r\ntwo\r\nthree\r\n";
    let f = fx(&[("a.md", real), ("gone.md", b"bye\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"one\nTWO\nthree\n");
    edit_copy(&work, "new.md", b"new\n");
    std::fs::remove_file(work.copy_path("gone.md").unwrap()).unwrap();
    let before = work.refresh().unwrap().files;
    work.accept("a.md", Some(&keys(&before, "a.md", &[0])))
        .unwrap();
    work.accept("new.md", None).unwrap();
    work.accept("gone.md", None).unwrap();
    assert_none(&work.changes().unwrap().files);
    work.undo("a.md").unwrap();
    assert_eq!(f.real("a.md"), real);
    work.undo("new.md").unwrap();
    assert!(!f.project.path().join("new.md").exists());
    let after = work.undo("gone.md").unwrap().files;
    assert_eq!(f.real("gone.md"), b"bye\n");
    assert_eq!(after, before);
    // one step only
    let error = work.undo("a.md").unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert_eq!(error.detail.as_deref(), Some("noUndo"));
}

/// Undo never writes over a file changed since the accept, nor under unsaved edits.
#[test]
fn undo_is_refused_when_the_file_changed_since() {
    let f = fx(&[("a.md", b"a\n"), ("b.md", b"b\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"A\n");
    edit_copy(&work, "b.md", b"B\n");
    work.refresh().unwrap();
    work.accept("a.md", None).unwrap();
    work.accept("b.md", None).unwrap();
    f.put_real("a.md", b"person\n");
    let error = work.undo("a.md").unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(error.detail.as_deref(), Some("changed"));
    assert_eq!(f.real("a.md"), b"person\n");
    f.documents.set_unsaved("w1", Some("b.md".to_owned()));
    let error = work.undo("b.md").unwrap_err();
    assert_eq!(error.detail.as_deref(), Some("unsaved"));
    assert_eq!(f.real("b.md"), b"B\n");
    f.documents.set_unsaved("w1", None);
    work.undo("b.md").unwrap();
    assert_eq!(f.real("b.md"), b"b\n");
}

/// Undo of a reject gives the copy back the agent's version.
#[test]
fn undo_puts_back_what_a_reject_dropped() {
    let f = fx(&[("a.md", b"1\n2\n3\n")]);
    let mut work = f.build();
    edit_copy(&work, "a.md", b"1\nTWO\n3\n");
    edit_copy(&work, "new.md", b"agent\n");
    let before = work.refresh().unwrap().files;
    work.reject("a.md", Some(&keys(&before, "a.md", &[0])))
        .unwrap();
    work.reject("new.md", None).unwrap();
    assert_eq!(copied(&work, "new.md"), None);
    work.undo("a.md").unwrap();
    let after = work.undo("new.md").unwrap().files;
    assert_eq!(copied(&work, "a.md").unwrap(), b"1\nTWO\n3\n");
    assert_eq!(copied(&work, "new.md").unwrap(), b"agent\n");
    assert_eq!(after, before);
    assert_eq!(f.real("a.md"), b"1\n2\n3\n");
    // the agent changed the copy again since the reject: left as it is
    work.reject("new.md", None).unwrap();
    edit_copy(&work, "new.md", b"newer\n");
    assert_eq!(
        work.undo("new.md").unwrap_err().detail.as_deref(),
        Some("changed")
    );
    assert_eq!(copied(&work, "new.md").unwrap(), b"newer\n");
}

/// A path kept out | hidden is never written, as sent and as the folder spells it.
#[test]
fn accept_never_writes_a_kept_out_or_hidden_path() {
    let f = fx(&[
        ("a.md", b"a\n"),
        (".aiignore", b"secret/\n"),
        ("secret/s.md", b"s\n"),
    ]);
    let mut work = f.build();
    // written by the agent and named straight over IPC
    edit_copy(&work, ".git/hooks/pre-commit", b"#!/bin/sh\n");
    edit_copy(&work, "secret/new.md", b"agent\n");
    work.refresh().unwrap();
    for path in [".git/hooks/pre-commit", "secret/new.md"] {
        let error = work.accept(path, None).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Denied, "{path}");
        assert_eq!(error.detail.as_deref(), Some("denied"));
        assert!(!f.project.path().join(path).exists(), "{path}");
    }
    // the refusals never list what is kept out (a hidden path stays listed: Reject cleans it from the copy)
    assert_eq!(
        kinds(&work.changes().unwrap().files),
        [("secret/new.md", Kind::Added)]
    );
}

/// Windows: a short (8.3) name of `.git` made in the copy would reach the real `.git` only through its spelling:
/// refused on the real location.
#[cfg(windows)]
#[test]
fn a_short_name_never_reaches_git() {
    let f = fx(&[("a.md", b"a\n"), (".git/hooks/x.sample", b"x\n")]);
    let short = f.project.path().join("GIT~1");
    let same = std::fs::canonicalize(&short).ok()
        == std::fs::canonicalize(f.project.path().join(".git")).ok();
    if !short.exists() || !same {
        // this volume makes no short names
        return;
    }
    let mut work = f.build();
    edit_copy(&work, "GIT~1/hooks/pre-commit", b"#!/bin/sh\n");
    let listed = work.refresh().unwrap().files;
    assert_eq!(kinds(&listed), [("GIT~1/hooks/pre-commit", Kind::Added)]);
    let error = work.accept("GIT~1/hooks/pre-commit", None).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Denied);
    assert!(!f.project.path().join(".git/hooks/pre-commit").exists());
}

/// A link at `link` to `target`; Windows: a folder link falls back to a junction (needs no privilege).
#[cfg(windows)]
pub(crate) fn link(target: &Path, link: &Path) -> std::io::Result<()> {
    if !target.is_dir() {
        return std::os::windows::fs::symlink_file(target, link);
    }
    if std::os::windows::fs::symlink_dir(target, link).is_ok() {
        return Ok(());
    }
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()?;
    if made.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("no junction"))
    }
}

#[cfg(unix)]
pub(crate) fn link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// A link the agent made in the copy is listed as a link: never read through, never accepted; Reject removes it
/// (a tracked file gets the real bytes back) and leaves what it leads to untouched.
#[test]
fn a_link_in_the_copy_is_listed_never_read() {
    let f = fx(&[("notes.md", b"real\n")]);
    let outside = tempfile::tempdir().unwrap();
    put(outside.path(), "secret.txt", b"secret\n");
    let mut work = f.build();
    let notes = work.copy_path("notes.md").unwrap();
    std::fs::remove_file(&notes).unwrap();
    // folder links: where the file was and at a new path
    if link(outside.path(), &notes).is_err() {
        return;
    }
    link(outside.path(), &work.copy_path("away").unwrap()).unwrap();
    // a file link, where this machine lets one be made
    let secret = work.copy_path("secret.md").unwrap();
    let file_link = link(&outside.path().join("secret.txt"), &secret).is_ok();
    let changes = work.refresh().unwrap().files;
    let mut expected = vec![("away", Kind::Link), ("notes.md", Kind::Link)];
    if file_link {
        expected.push(("secret.md", Kind::Link));
    }
    assert_eq!(kinds(&changes), expected);
    assert!(changes.iter().all(|c| c.hunks.is_empty()));
    let error = work.accept("notes.md", None).unwrap_err();
    assert_eq!(error.detail.as_deref(), Some("link"));
    assert_eq!(f.real("notes.md"), b"real\n");
    work.reject("notes.md", None).unwrap();
    assert_eq!(copied(&work, "notes.md").unwrap(), b"real\n");
    if file_link {
        assert_eq!(
            work.accept("secret.md", None)
                .unwrap_err()
                .detail
                .as_deref(),
            Some("link")
        );
        assert!(!f.project.path().join("secret.md").exists());
        work.reject("secret.md", None).unwrap();
    }
    assert_none(&work.reject("away", None).unwrap().files);
    assert!(std::fs::symlink_metadata(work.copy_path("away").unwrap()).is_err());
    assert_eq!(
        std::fs::read(outside.path().join("secret.txt")).unwrap(),
        b"secret\n"
    );
}

/// A folder of the copy replaced by a link: its files count as gone, and nothing is written | removed through it.
#[test]
fn a_linked_folder_in_the_copy_is_never_written_through() {
    let f = fx(&[("docs/a.md", b"a\n"), ("b.md", b"b\n")]);
    let outside = tempfile::tempdir().unwrap();
    put(outside.path(), "a.md", b"outside\n");
    let mut work = f.build();
    let docs = work.copy_path("docs").unwrap();
    std::fs::remove_dir_all(&docs).unwrap();
    if link(outside.path(), &docs).is_err() {
        return;
    }
    f.put_real("docs/new.md", b"fresh\n");
    work.sync().unwrap();
    assert_eq!(
        kinds(&work.refresh().unwrap().files),
        [("docs", Kind::Link), ("docs/a.md", Kind::Deleted)]
    );
    let error = work.reject("docs/a.md", None).unwrap_err();
    assert_eq!(error.detail.as_deref(), Some("linkInPath"));
    let mut left: Vec<String> = std::fs::read_dir(outside.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["a.md"]);
    assert_eq!(
        std::fs::read(outside.path().join("a.md")).unwrap(),
        b"outside\n"
    );
    // the link goes first; then the folder's files come back
    work.reject("docs", None).unwrap();
    work.reject("docs/a.md", None).unwrap();
    assert_eq!(copied(&work, "docs/a.md").unwrap(), b"a\n");
    assert_eq!(
        std::fs::read(outside.path().join("a.md")).unwrap(),
        b"outside\n"
    );
}

/// Unix: a special file (here a socket) in the copy is listed as a link and never opened.
#[cfg(unix)]
#[test]
fn a_special_file_in_the_copy_is_never_opened() {
    let f = fx(&[("a.md", b"a\n")]);
    let mut work = f.build();
    let socket = work.copy_path("a.md").unwrap();
    std::fs::remove_file(&socket).unwrap();
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    assert_eq!(
        kinds(&work.refresh().unwrap().files),
        [("a.md", Kind::Link)]
    );
    assert_none(&work.reject("a.md", None).unwrap().files);
    assert_eq!(copied(&work, "a.md").unwrap(), b"a\n");
}
