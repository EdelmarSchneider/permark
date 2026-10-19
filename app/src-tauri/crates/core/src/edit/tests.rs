//! Editing service: sources, surgical saves, conflicts, the person's copy, pictures.

use std::sync::Arc;

use super::*;
use crate::format::{
    Contributions, LockReason, MarkdownFeature, PromptBlocks, SegmentKind, TextViewer,
};
use crate::paths::{FileScope, Root, Vault};
use crate::watch::{Change, ChangeKind};

pub(super) fn formats() -> FormatRegistry {
    FormatRegistry::build(
        &Contributions::default()
            .formats(vec![
                Arc::new(TextViewer::json()),
                Arc::new(TextViewer::plain()),
            ])
            .markdown(vec![MarkdownFeature::PromptBlocks(PromptBlocks {
                tags: crate::format::DEFAULT_PROMPT_TAGS,
            })]),
    )
}

pub(super) struct Fx {
    pub dir: tempfile::TempDir,
    pub scope: DocScope,
    pub formats: FormatRegistry,
    pub own: OwnWrites,
}

impl Fx {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let scope = DocScope::Project(Vault::new(Root::open(dir.path()).unwrap()));
        Self {
            dir,
            scope,
            formats: formats(),
            own: OwnWrites::new(),
        }
    }

    pub fn editor(&self) -> Editor<'_> {
        Editor {
            scope: &self.scope,
            formats: &self.formats,
            own: Some(&self.own),
        }
    }

    pub fn write(&self, rel: &str, bytes: &[u8]) {
        let path = self.dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    pub fn read(&self, rel: &str) -> Vec<u8> {
        std::fs::read(self.dir.path().join(rel)).unwrap()
    }

    /// Whether a watch report of `rel` with its current content would be news (not our own write).
    fn is_own(&self, rel: &str) -> bool {
        let mut changes = [Change {
            path: rel.to_owned(),
            kind: ChangeKind::Modified,
            is_dir: false,
            own: false,
        }];
        let hash = fsx::hash(&self.read(rel)).0;
        self.own.mark(&mut changes, |_| Some(hash));
        changes[0].own
    }
}

fn edit(at: u32, remove: u32, insert: &[&str]) -> LineEdit {
    LineEdit {
        at,
        remove,
        insert: insert.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn saved_hash(outcome: &SaveOutcome) -> &str {
    match outcome {
        SaveOutcome::Saved { hash, .. } => hash,
        SaveOutcome::Conflict { .. } => panic!("conflict: {outcome:?}"),
    }
}

#[test]
fn the_source_is_the_text_with_lf_breaks_its_hash_and_its_blocks() {
    let fx = Fx::new();
    fx.write("a b/doc ç.md", b"\xEF\xBB\xBF# T\r\n\r\npara\r\n");
    let source = fx.editor().source("a b/doc ç.md").unwrap();
    assert_eq!(source.text, "# T\n\npara\n");
    assert_eq!(source.hash, fsx::hash(&fx.read("a b/doc ç.md")).hex());
    assert_eq!(source.encoding, Encoding::Utf8Bom);
    assert_eq!(source.line_ending, LineEnding::CrLf);
    let segments = source.segments.unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].kind, SegmentKind::Heading);
    assert_eq!((segments[1].line_start, segments[1].line_end), (3, 3));
}

#[test]
fn text_files_have_no_blocks_and_other_files_are_refused_with_a_reason() {
    let fx = Fx::new();
    fx.write("notes.txt", b"plain\n");
    fx.write("data.json", b"{}");
    fx.write("bin.md", b"a\0b");
    fx.write("bad.md", b"caf\xE9\n");
    assert!(fx.editor().source("notes.txt").unwrap().segments.is_none());
    assert!(fx.editor().source("data.json").unwrap().segments.is_none());
    assert_eq!(
        fx.editor().source("bin.md").unwrap_err().kind,
        ErrorKind::Binary
    );
    assert_eq!(
        fx.editor().source("bad.md").unwrap_err().kind,
        ErrorKind::Encoding
    );
    assert_eq!(
        fx.editor().source("../outside.md").unwrap_err().kind,
        ErrorKind::PathEscape
    );
    assert_eq!(
        fx.editor().source("missing.md").unwrap_err().kind,
        ErrorKind::NotFound
    );
}

#[test]
fn a_file_over_the_limit_is_too_large_to_edit() {
    let fx = Fx::new();
    let big = vec![b'a'; usize::try_from(MAX_TEXT_BYTES).unwrap() + 1];
    fx.write("big.md", &big);
    assert_eq!(
        fx.editor().source("big.md").unwrap_err().kind,
        ErrorKind::TooLarge
    );
}

#[test]
fn a_save_changes_only_the_edited_lines_and_keeps_each_line_ending() {
    let fx = Fx::new();
    let original = b"one\r\ntwo\nthree\r\nfour\r\n";
    fx.write("m.md", original);
    let seen = fsx::hash(original).hex();
    let out = fx
        .editor()
        .save("m.md", &seen, &[edit(2, 1, &["THREE"])])
        .unwrap();
    assert_eq!(fx.read("m.md"), b"one\r\ntwo\nTHREE\r\nfour\r\n");
    assert_eq!(saved_hash(&out), fsx::hash(&fx.read("m.md")).hex());
    assert!(fx.is_own("m.md"), "the save was announced to the watch");
}

#[test]
fn utf16_and_bom_files_are_written_back_in_their_encoding() {
    let fx = Fx::new();
    for (name, encoding) in [
        ("le.md", Encoding::Utf16Le),
        ("be.md", Encoding::Utf16Be),
        ("bom.md", Encoding::Utf8Bom),
    ] {
        let bytes = lines::encode("ação\r\nlinha 文\r\n", encoding);
        fx.write(name, &bytes);
        let source = fx.editor().source(name).unwrap();
        fx.editor()
            .save(name, &source.hash, &[edit(1, 1, &["nova"])])
            .unwrap();
        assert_eq!(
            fx.read(name),
            lines::encode("ação\r\nnova\r\n", encoding),
            "{name}"
        );
    }
}

#[test]
fn saving_nothing_new_writes_and_announces_nothing() {
    let fx = Fx::new();
    fx.write("same.md", b"a\nb\n");
    let seen = fsx::hash(b"a\nb\n").hex();
    let before = std::fs::metadata(fx.dir.path().join("same.md"))
        .unwrap()
        .modified()
        .unwrap();
    let out = fx
        .editor()
        .save("same.md", &seen, &[edit(1, 1, &["b"])])
        .unwrap();
    assert_eq!(saved_hash(&out), seen);
    let after = std::fs::metadata(fx.dir.path().join("same.md"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
    assert!(!fx.is_own("same.md"));
}

#[test]
fn a_file_changed_behind_the_editor_is_a_conflict_and_is_left_alone() {
    let fx = Fx::new();
    fx.write("c.md", b"theirs\n");
    let out = fx
        .editor()
        .save("c.md", "0000000000000000", &[edit(0, 1, &["mine"])])
        .unwrap();
    assert_eq!(
        out,
        SaveOutcome::Conflict {
            disk_hash: Some(fsx::hash(b"theirs\n").hex())
        }
    );
    assert_eq!(fx.read("c.md"), b"theirs\n");
}

#[test]
fn a_writer_that_gets_in_after_the_first_check_is_not_overwritten() {
    let fx = Fx::new();
    fx.write("race.md", b"v1\n");
    let seen = fsx::hash(b"v1\n").hex();
    let path = fx.dir.path().join("race.md");
    let out = fx
        .editor()
        .save_after("race.md", &seen, &[edit(0, 1, &["mine"])], || {
            std::fs::write(&path, b"agent\n").unwrap();
        })
        .unwrap();
    assert_eq!(
        out,
        SaveOutcome::Conflict {
            disk_hash: Some(fsx::hash(b"agent\n").hex())
        }
    );
    assert_eq!(fx.read("race.md"), b"agent\n");
    assert!(!fx.is_own("race.md"), "the announcement was taken back");
    let names: Vec<_> = std::fs::read_dir(fx.dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["race.md"], "no temp left");
}

#[test]
fn a_file_that_vanishes_is_a_conflict_without_a_hash_and_is_not_recreated() {
    let fx = Fx::new();
    let out = fx
        .editor()
        .save("gone.md", "abc", &[edit(0, 0, &["x"])])
        .unwrap();
    assert_eq!(out, SaveOutcome::Conflict { disk_hash: None });
    fx.write("late.md", b"v1\n");
    let path = fx.dir.path().join("late.md");
    let out = fx
        .editor()
        .save_after(
            "late.md",
            &fsx::hash(b"v1\n").hex(),
            &[edit(0, 1, &["x"])],
            || std::fs::remove_file(&path).unwrap(),
        )
        .unwrap();
    assert_eq!(out, SaveOutcome::Conflict { disk_hash: None });
    assert!(!path.exists());
}

#[test]
fn a_read_only_file_is_refused() {
    let fx = Fx::new();
    fx.write("ro.md", b"x\n");
    let path = fx.dir.path().join("ro.md");
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms.clone()).unwrap();
    let err = fx
        .editor()
        .save("ro.md", &fsx::hash(b"x\n").hex(), &[edit(0, 1, &["y"])])
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Denied);
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&path, perms).unwrap();
    assert_eq!(fx.read("ro.md"), b"x\n");
}

#[test]
fn bad_edits_are_refused_and_nothing_is_written() {
    let fx = Fx::new();
    fx.write("e.md", b"a\nb\n");
    let seen = fsx::hash(b"a\nb\n").hex();
    for edits in [
        vec![edit(9, 1, &[])],
        vec![edit(1, 1, &[]), edit(0, 1, &[])],
        vec![edit(0, 1, &["x\ry"])],
    ] {
        let err = fx.editor().save("e.md", &seen, &edits).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidData);
    }
    assert_eq!(fx.read("e.md"), b"a\nb\n");
    assert_eq!(
        fx.editor().save("../x.md", &seen, &[]).unwrap_err().kind,
        ErrorKind::PathEscape
    );
}

#[test]
fn save_as_makes_a_new_file_next_to_the_document_and_never_overwrites() {
    let fx = Fx::new();
    fx.write("dir/doc.md", b"theirs\r\n");
    let first = fx
        .editor()
        .save_as("dir/doc.md", "mine\nline\n", false)
        .unwrap();
    assert_eq!(first, "dir/doc-mine.md");
    assert_eq!(
        fx.read("dir/doc-mine.md"),
        b"mine\r\nline\r\n",
        "CRLF of the original"
    );
    let second = fx.editor().save_as("dir/doc.md", "again", false).unwrap();
    assert_eq!(second, "dir/doc-mine-2.md");
    assert!(fx.is_own("dir/doc-mine-2.md"));
    assert_eq!(
        fx.editor()
            .save_as("dir/doc.md", "x", true)
            .unwrap_err()
            .kind,
        ErrorKind::Exists
    );
    std::fs::remove_file(fx.dir.path().join("dir/doc.md")).unwrap();
    assert_eq!(
        fx.editor().save_as("dir/doc.md", "back\n", true).unwrap(),
        "dir/doc.md"
    );
    assert_eq!(fx.read("dir/doc.md"), b"back\n");
}

#[test]
fn pictures_are_saved_next_to_the_document_with_the_next_free_number() {
    let fx = Fx::new();
    fx.write("notes/my doc.md", b"x");
    fx.write("notes/my doc-1.png", b"taken");
    let png = b"\x89PNG\r\n\x1a\nDATA";
    let path = fx
        .editor()
        .write_asset(
            "notes/my doc.md",
            "image/png".rsplit('/').next().unwrap(),
            png,
        )
        .unwrap();
    assert_eq!(path, "notes/my doc-2.png");
    assert_eq!(fx.read("notes/my doc-2.png"), png);
    assert_eq!(fx.read("notes/my doc-1.png"), b"taken");
    assert!(fx.is_own("notes/my doc-2.png"));
}

#[test]
fn pictures_of_the_wrong_kind_or_too_large_are_refused() {
    let fx = Fx::new();
    fx.write("d.md", b"x");
    let e = fx.editor().write_asset("d.md", "exe", b"MZ").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported);
    let e = fx
        .editor()
        .write_asset("d.md", "png", b"GIF89a")
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported);
    let big = vec![0_u8; usize::try_from(MAX_ASSET_BYTES).unwrap() + 1];
    let e = fx.editor().write_asset("d.md", "png", &big).unwrap_err();
    assert_eq!(e.kind, ErrorKind::TooLarge);
    let e = fx
        .editor()
        .write_asset("../d.md", "png", b"\x89PNG\r\n\x1a\n")
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::PathEscape);
}

#[test]
fn a_single_file_workspace_edits_its_file_and_puts_pictures_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("only.md");
    std::fs::write(&file, b"# Only\n").unwrap();
    std::fs::write(dir.path().join("other.md"), b"no").unwrap();
    let scope = DocScope::File(FileScope::open(&file).unwrap());
    let formats = formats();
    let editor = Editor {
        scope: &scope,
        formats: &formats,
        own: None,
    };
    let source = editor.source("only.md").unwrap();
    editor
        .save("only.md", &source.hash, &[edit(1, 0, &["more"])])
        .unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"# Only\nmore\n");
    assert_eq!(
        editor.source("other.md").unwrap_err().kind,
        ErrorKind::NotFound
    );
    let pic = editor.write_asset("only.md", "gif", b"GIF89a").unwrap();
    assert_eq!(pic, "only-1.gif");
    assert!(dir.path().join("only-1.gif").exists());
}

#[test]
fn headings_have_the_ids_of_the_reading_view() {
    let fx = Fx::new();
    fx.write("h.md", b"# Hello World\n\n## Hello World\n");
    let ids: Vec<String> = fx
        .editor()
        .headings("h.md")
        .unwrap()
        .into_iter()
        .map(|h| h.id)
        .collect();
    assert_eq!(ids, ["hello-world", "hello-world-1"]);
}

// ---- block map -------------------------------------------------------------------------------------------------

fn kinds(text: &str) -> Vec<(u32, u32, SegmentKind, Option<LockReason>)> {
    crate::format::segments(text, formats().features())
        .into_iter()
        .map(|s| (s.line_start, s.line_end, s.kind, s.lock))
        .collect()
}

#[test]
fn blocks_have_kinds_and_the_ones_to_keep_say_why() {
    use LockReason as L;
    use SegmentKind as K;
    let text = "---\ntitle: x\n---\n\n# H\n\npara[^1]\n\n- a\n- b\n\n```mermaid\ngraph\n```\n\n$$\nx\n$$\n\n<div>\nhi\n</div>\n\n[^1]: note\n\n> [!NOTE]\n> alert\n\n| a |\n|---|\n| 1 |\n\n***\n\n[ref]: http://x\n";
    let got = kinds(text);
    assert_eq!(
        got,
        [
            (1, 3, K::FrontMatter, Some(L::FrontMatter)),
            (5, 5, K::Heading, None),
            (7, 7, K::Paragraph, None),
            (9, 10, K::List, None),
            (12, 14, K::Code, Some(L::Diagram)),
            (16, 18, K::Math, Some(L::Math)),
            (20, 22, K::Html, Some(L::Html)),
            (24, 24, K::Footnote, Some(L::Footnote)),
            (26, 27, K::Alert, Some(L::Kind)),
            (29, 31, K::Table, None),
            (33, 33, K::Rule, None),
            (35, 35, K::Raw, Some(L::Definition)),
        ]
    );
}

#[test]
fn prompt_tags_are_their_own_locked_lines_and_the_inside_stays_editable() {
    let got = kinds("<context>\nInside **bold**\n</context>\n");
    assert_eq!(
        got.iter().map(|g| g.2).collect::<Vec<_>>(),
        [SegmentKind::Html, SegmentKind::Paragraph, SegmentKind::Html]
    );
    assert_eq!(got[1].3, None);
}

#[test]
fn an_unclosed_fence_leaves_trailing_blank_lines_to_the_separator() {
    let got = kinds("```\ncode\n\n\n");
    assert_eq!(got, [(1, 2, SegmentKind::Code, None)]);
}

#[test]
fn deep_nesting_is_one_locked_block() {
    let text = format!("\n{}x\n\n", "> ".repeat(150));
    let got = kinds(&text);
    assert_eq!(got, [(2, 2, SegmentKind::Other, Some(LockReason::Parse))]);
    assert_eq!(kinds("").len(), 0);
    assert_eq!(kinds("\n\n  \n").len(), 0);
}
