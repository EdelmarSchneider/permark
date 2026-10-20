//! Documents service: opening, notes, errors, change events (project batches and single-file polling).

#![allow(clippy::assert_is_empty)] // `assert!(x.is_empty())` reads best for "nothing happened"

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::error::ErrorKind;
use crate::events::Recorder;
use crate::format::{
    Contributions, Format, FormatId, FormatRegistry, MarkdownFeature, MarkdownFormat, MediaFormat,
    PromptBlocks, RoleMatch, RoleRule, TextViewer,
};
use crate::paths::{FileScope, Root};
use crate::watch::Change;

fn formats() -> FormatRegistry {
    FormatRegistry::build(
        &Contributions::default()
            .formats(vec![
                Arc::new(MediaFormat),
                Arc::new(TextViewer::json()),
                Arc::new(TextViewer::plain()),
            ])
            .roles(vec![RoleRule {
                id: "agents",
                matches: RoleMatch::Name("AGENTS.md"),
            }])
            .markdown(vec![MarkdownFeature::PromptBlocks(PromptBlocks {
                tags: crate::format::DEFAULT_PROMPT_TAGS,
            })]),
    )
}

struct Project {
    dir: tempfile::TempDir,
    docs: Arc<Documents>,
    events: Arc<Recorder>,
}

fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::open(dir.path()).unwrap();
    let events = Arc::new(Recorder::default());
    let docs = Documents::for_project(DocScope::Project(Vault::new(root)), events.clone());
    Project { dir, docs, events }
}

impl Project {
    fn write(&self, path: &str, bytes: &[u8]) {
        let abs = self.dir.path().join(path);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(abs, bytes).unwrap();
    }

    fn open(&self, window: &str, path: &str) -> Result<DocumentView, IpcError> {
        self.docs.open(window, path, &[], &formats())
    }

    fn batch(&self, changes: Vec<Change>) {
        self.docs.on_batch(&FsBatch {
            changes,
            ..FsBatch::default()
        });
    }

    fn changes(&self) -> Vec<DocumentChanged> {
        self.events
            .take()
            .into_iter()
            .filter_map(|e| match e {
                CoreEvent::DocumentChanged(c) => Some(c),
                _ => None,
            })
            .collect()
    }
}

use crate::paths::Vault;

fn change(path: &str, kind: ChangeKind) -> Change {
    Change {
        path: path.to_owned(),
        kind,
        is_dir: false,
        own: false,
    }
}

#[test]
fn markdown_opens_with_blocks_outline_role_and_hash() {
    let p = project();
    p.write("AGENTS.md", b"# Rules\n\nBe kind.\n");
    let view = p.open("w-1", "AGENTS.md").unwrap();
    assert_eq!(view.format, crate::format::FormatId::MARKDOWN);
    assert_eq!(view.role.as_deref(), Some("agents"));
    assert_eq!(
        view.hash.as_deref(),
        Some(fsx::hash(b"# Rules\n\nBe kind.\n").hex().as_str())
    );
    assert_eq!(view.editable, Editability::Surgical);
    let Body::Markdown {
        blocks, outline, ..
    } = &view.body
    else {
        panic!("{:?}", view.body)
    };
    assert_eq!(blocks.len(), 2);
    assert_eq!(outline[0].label, "Rules");
    // a plain Markdown file has no role
    p.write("docs/a.md", b"x");
    assert_eq!(p.open("w-1", "docs/a.md").unwrap().role, None);
}

#[test]
fn known_blocks_are_not_sent_again() {
    let p = project();
    p.write("a.md", b"one\n\ntwo\n");
    let first = p.open("w", "a.md").unwrap();
    let Body::Markdown { blocks, .. } = first.body else {
        panic!()
    };
    let known: Vec<String> = blocks.iter().map(|b| b.id.clone()).collect();
    p.write("a.md", b"one\n\nTWO\n\nthree\n");
    let second = p.docs.open("w", "a.md", &known, &formats()).unwrap();
    let Body::Markdown { blocks, .. } = second.body else {
        panic!()
    };
    assert_eq!(
        blocks.iter().map(|b| b.html.is_some()).collect::<Vec<_>>(),
        [false, true, true]
    );
}

#[test]
fn other_formats_and_notices() {
    let p = project();
    p.write("data.json", b"{\"a\": 1}");
    p.write("notes.txt", "olá — \u{4e16}\u{754c}".as_bytes());
    p.write("pic.png", &[0x89, b'P', b'N', b'G', 0, 1, 2, 3]);
    p.write("blob.bin", &[1, 2, 0, 3, 4]);
    p.write("empty.md", b"");
    let json = p.open("w", "data.json").unwrap();
    assert!(matches!(&json.body, Body::Text { language: Some(l), .. } if l == "json"));
    assert_eq!(json.format, crate::format::FormatId::JSON);
    let text = p.open("w", "notes.txt").unwrap();
    assert!(matches!(&text.body, Body::Text { text, .. } if text.contains("世界")));
    let pic = p.open("w", "pic.png").unwrap();
    assert!(matches!(&pic.body, Body::Media { mime, .. } if mime == "image/png"));
    assert_eq!(pic.hash, None);
    assert!(matches!(pic.editable, Editability::ReadOnly { .. }));
    let bin = p.open("w", "blob.bin").unwrap();
    assert_eq!(
        bin.body,
        Body::Notice {
            notice: Notice::Binary
        }
    );
    let empty = p.open("w", "empty.md").unwrap();
    assert!(matches!(&empty.body, Body::Markdown { blocks, .. } if blocks.is_empty()));
    // no viewer on (only Markdown): text files get the notice
    let bare = FormatRegistry::core();
    let none = p.docs.open("w", "notes.txt", &[], &bare).unwrap();
    assert_eq!(
        none.body,
        Body::Notice {
            notice: Notice::NoViewer
        }
    );
}

#[test]
fn encodings_leave_notes() {
    let p = project();
    let utf16: Vec<u8> = std::iter::once(0xFF)
        .chain(std::iter::once(0xFE))
        .chain("# T\n".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    p.write("u16.md", &utf16);
    p.write("bad.md", b"caf\xE9\n");
    p.write("bom.md", b"\xEF\xBB\xBF# T\n");
    assert_eq!(p.open("w", "u16.md").unwrap().notes, ["utf16"]);
    assert_eq!(p.open("w", "bad.md").unwrap().notes, ["lossy"]);
    assert!(p.open("w", "bom.md").unwrap().notes.is_empty());
}

/// A text of `lines` numbered lines (a bit over 12 bytes each).
fn numbered(lines: usize) -> String {
    (0..lines)
        .map(|n| format!("line {n:07}\n"))
        .collect::<Vec<_>>()
        .concat()
}

/// Every chunk of a large view, joined with its first part.
fn whole_text(p: &Project, path: &str) -> (DocumentView, String) {
    let view = p.open("w", path).unwrap();
    let Body::Text {
        text,
        partial: Some(partial),
        ..
    } = &view.body
    else {
        panic!("not a partial text: {:?}", view.body);
    };
    let mut all = text.clone();
    let mut next = Some(partial.loaded);
    while let Some(offset) = next {
        let chunk = p.docs.more(path, offset, view.size, view.mtime_ms).unwrap();
        assert!(chunk.text.len() <= MORE_CHUNK + 4);
        assert!(
            chunk.next.is_none() || chunk.text.ends_with('\n') || !chunk.text.contains('\n'),
            "a chunk ends at a line break"
        );
        assert_ne!(chunk.next, Some(offset), "the walk goes on");
        all.push_str(&chunk.text);
        next = chunk.next;
    }
    (view, all)
}

#[test]
fn a_large_text_shows_its_start_at_once_and_the_rest_in_chunks() {
    let p = project();
    let text = numbered(600_000); // about 7.8 MB
    assert!(text.len() as u64 > MAX_TEXT_BYTES);
    p.write("big.log", text.as_bytes());
    let started = Instant::now();
    let view = p.open("w", "big.log").unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    let Body::Text {
        text: first,
        partial: Some(partial),
        language,
        ..
    } = &view.body
    else {
        panic!("{:?}", view.body);
    };
    assert_eq!(language.as_deref(), Some("log"));
    assert!(first.len() <= FIRST_CHUNK && first.len() > FIRST_CHUNK / 2);
    assert!(first.ends_with('\n') && first.starts_with("line 0000000\n"));
    assert_eq!(partial.loaded, first.len() as u64);
    assert_eq!(partial.total, text.len() as u64);
    assert_eq!(view.hash, None);
    assert_eq!(view.size, text.len() as u64);
    assert_eq!(view.format, FormatId::TEXT);
    assert!(matches!(
        &view.editable,
        Editability::ReadOnly { reason } if reason == "tooLarge"
    ));
    // the rest, chunk by chunk, is the rest of the file
    let (_, all) = whole_text(&p, "big.log");
    assert_eq!(all.len(), text.len());
    assert_eq!(fsx::hash(all.as_bytes()), fsx::hash(text.as_bytes()));
}

#[test]
fn a_very_long_line_is_cut_inside_no_character() {
    let p = project();
    // 3 MB of two-byte and four-byte characters on one line, then a short line
    let mut text = "é😀".repeat(900_000);
    text.push('\n');
    text.push_str("end\n");
    assert!(text.len() as u64 > MAX_TEXT_BYTES);
    p.write("one-line.txt", text.as_bytes());
    let (_, all) = whole_text(&p, "one-line.txt");
    assert_eq!(
        fsx::hash(all.as_bytes()),
        fsx::hash(text.as_bytes()),
        "the chunks join to the very text"
    );
}

#[test]
fn a_byte_order_mark_is_not_text_and_other_large_files_are_what_they_are() {
    let p = project();
    let body = numbered(500_000);
    let mut with_bom = b"\xEF\xBB\xBF".to_vec();
    with_bom.extend_from_slice(body.as_bytes());
    p.write("bom.txt", &with_bom);
    let (_, all) = whole_text(&p, "bom.txt");
    assert_eq!(
        fsx::hash(all.as_bytes()),
        fsx::hash(body.as_bytes()),
        "no mark in the text"
    );
    // binary
    let mut binary = vec![b'a'; usize::try_from(MAX_TEXT_BYTES).unwrap() + 10];
    binary[100] = 0;
    p.write("blob.dat", &binary);
    let view = p.open("w", "blob.dat").unwrap();
    assert!(matches!(
        view.body,
        Body::Notice {
            notice: Notice::Binary
        }
    ));
    // UTF-16 is not cut into chunks
    let mut wide = b"\xFF\xFE".to_vec();
    wide.extend(
        "x".repeat(usize::try_from(MAX_TEXT_BYTES).unwrap())
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    p.write("wide.txt", &wide);
    let view = p.open("w", "wide.txt").unwrap();
    assert!(matches!(
        view.body,
        Body::Notice {
            notice: Notice::TooLarge { .. }
        }
    ));
    // a Markdown file over the limit is plain text here
    p.write("big.md", numbered(500_000).as_bytes());
    let view = p.open("w", "big.md").unwrap();
    assert_eq!(view.format, FormatId::TEXT);
    assert!(matches!(
        &view.body,
        Body::Text {
            partial: Some(_),
            ..
        }
    ));
    // the edge: exactly the limit is still read whole
    p.write(
        "edge.txt",
        &vec![b'a'; usize::try_from(MAX_TEXT_BYTES).unwrap()],
    );
    assert!(matches!(
        p.open("w", "edge.txt").unwrap().body,
        Body::Text { partial: None, .. }
    ));
}

#[test]
fn a_chunk_of_a_file_that_is_not_the_one_opened_is_a_conflict() {
    let p = project();
    let text = numbered(500_000);
    p.write("big.log", text.as_bytes());
    let view = p.open("w", "big.log").unwrap();
    let Body::Text {
        partial: Some(partial),
        ..
    } = &view.body
    else {
        panic!()
    };
    // grown since
    p.write("big.log", format!("{text}more\n").as_bytes());
    let error = p
        .docs
        .more("big.log", partial.loaded, view.size, view.mtime_ms)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    // the change is told to the window as for any file (by size and time)
    p.batch(vec![change("big.log", ChangeKind::Modified)]);
    assert_eq!(p.changes().len(), 1);
    // past the end, outside the project, not there, a folder
    let view = p.open("w", "big.log").unwrap();
    let past = p
        .docs
        .more("big.log", view.size + 5, view.size, view.mtime_ms)
        .unwrap();
    assert!(past.text.is_empty() && past.next.is_none());
    for path in ["../outside.txt", "nothing.log"] {
        assert!(p.docs.more(path, 0, 1, 1).is_err(), "{path}");
    }
    p.write("dir/x.txt", b"x");
    assert!(p.docs.more("dir", 0, 1, 1).is_err());
}

fn tiny_card(_syntax: crate::format::FrontMatterSyntax, raw: &str) -> Vec<crate::format::Field> {
    raw.lines()
        .filter_map(|line| line.split_once(": "))
        .map(|(key, value)| crate::format::Field {
            key: key.to_owned(),
            value: crate::format::FieldValue::Text {
                text: value.to_owned(),
            },
        })
        .collect()
}

fn card_formats(card_on: bool) -> FormatRegistry {
    let mut markdown = vec![];
    if card_on {
        markdown.push(MarkdownFeature::FrontMatterCard(tiny_card));
    }
    FormatRegistry::build(
        &Contributions::default()
            .roles(vec![RoleRule {
                id: "agents",
                matches: RoleMatch::Name("AGENTS.md"),
            }])
            .key_roles(vec![crate::format::KeyRole {
                id: "model-card",
                keys: &["pipeline_tag", "base_model"],
                also: &["license"],
            }])
            .markdown(markdown),
    )
}

#[test]
fn a_card_is_known_by_the_keys_of_its_front_matter() {
    let p = project();
    let card = "---
title: T
license: mit
pipeline_tag: text-generation
---
# Model
";
    p.write("README.md", card.as_bytes());
    p.write("AGENTS.md", card.as_bytes());
    p.write(
        "plain.md",
        b"---
title: T
license: mit
---
# Plain
",
    );
    let open = |path: &str, formats: &FormatRegistry| p.docs.open("w", path, &[], formats).unwrap();
    // the keys tell, and the card lists them first
    let view = open("README.md", &card_formats(true));
    assert_eq!(view.role.as_deref(), Some("model-card"));
    let Body::Markdown {
        front_matter: Some(front_matter),
        ..
    } = &view.body
    else {
        panic!("{:?}", view.body);
    };
    let keys: Vec<&str> = front_matter.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["pipeline_tag", "license", "title"]);
    // a name that is a role wins; a front matter with no card key is no card
    assert_eq!(
        open("AGENTS.md", &card_formats(true)).role.as_deref(),
        Some("agents")
    );
    assert_eq!(open("plain.md", &card_formats(true)).role, None);
    // the properties card off: no fields, no role
    assert_eq!(open("README.md", &card_formats(false)).role, None);
}

#[test]
fn paths_outside_or_missing_are_errors() {
    let p = project();
    p.write("a.md", b"x");
    std::fs::create_dir(p.dir.path().join("folder")).unwrap();
    for (path, kind) in [
        ("../outside.md", ErrorKind::PathEscape),
        ("..\\..\\x.md", ErrorKind::PathEscape),
        ("/abs.md", ErrorKind::InvalidPath),
        ("C:\\x.md", ErrorKind::InvalidPath),
        ("missing.md", ErrorKind::NotFound),
        ("folder", ErrorKind::NotFound),
    ] {
        let error = p.open("w", path).unwrap_err();
        assert_eq!(error.kind, kind, "{path}: {error:?}");
    }
    // nothing was registered for the failed opens
    p.batch(vec![change("missing.md", ChangeKind::Created)]);
    assert!(p.changes().is_empty());
}

#[test]
fn unicode_and_space_paths_open() {
    let p = project();
    p.write("café docs/日本語 plan.md", "# 計画\n".as_bytes());
    let view = p.open("w", "café docs/日本語 plan.md").unwrap();
    assert_eq!(view.name, "日本語 plan.md");
    assert!(matches!(&view.body, Body::Markdown { outline, .. } if outline[0].label == "計画"));
}

#[test]
fn a_changed_file_is_told_once_to_the_window_that_shows_it() {
    let p = project();
    p.write("a.md", b"v1");
    p.write("b.md", b"other");
    p.open("w-1", "a.md").unwrap();
    p.open("w-2", "a.md").unwrap();
    p.open("w-3", "b.md").unwrap();
    // nothing changed on disk: a modify event for the same bytes is dropped
    p.batch(vec![change("a.md", ChangeKind::Modified)]);
    assert!(p.changes().is_empty());
    p.write("a.md", b"v2");
    p.batch(vec![change("a.md", ChangeKind::Modified)]);
    let got = p.changes();
    let mut windows: Vec<_> = got.iter().map(|c| c.window.as_str()).collect();
    windows.sort_unstable();
    assert_eq!(windows, ["w-1", "w-2"]);
    assert!(got.iter().all(|c| c.path == "a.md"
        && c.change
            == DocChange::Changed {
                version: fsx::hash(b"v2").hex()
            }));
    // the same event again: nothing new
    p.batch(vec![change("a.md", ChangeKind::Modified)]);
    assert!(p.changes().is_empty());
    // a case-different spelling of the path in the batch still matches
    p.write("a.md", b"v3");
    p.batch(vec![change("a.md", ChangeKind::Created)]);
    assert_eq!(p.changes().len(), 2);
}

#[test]
fn removal_rename_and_return() {
    let p = project();
    p.write("a.md", b"text");
    p.open("w", "a.md").unwrap();
    std::fs::remove_file(p.dir.path().join("a.md")).unwrap();
    p.batch(vec![change("a.md", ChangeKind::Removed)]);
    assert_eq!(p.changes()[0].change, DocChange::Deleted { moved_to: None });
    // told once
    p.batch(vec![change("a.md", ChangeKind::Removed)]);
    assert!(p.changes().is_empty());
    // back (same bytes): the window must hear it
    p.write("a.md", b"text");
    p.batch(vec![change("a.md", ChangeKind::Created)]);
    assert!(matches!(p.changes()[0].change, DocChange::Changed { .. }));
    // renamed: the old path's window hears where it went
    std::fs::rename(p.dir.path().join("a.md"), p.dir.path().join("b.md")).unwrap();
    p.batch(vec![change(
        "b.md",
        ChangeKind::Renamed {
            from: "a.md".to_owned(),
        },
    )]);
    let got = p.changes();
    assert_eq!(got.len(), 1);
    assert_eq!(
        got[0].change,
        DocChange::Deleted {
            moved_to: Some("b.md".to_owned())
        }
    );
    assert_eq!(got[0].path, "a.md");
}

#[test]
fn closing_a_window_stops_its_events() {
    let p = project();
    p.write("a.md", b"1");
    p.open("w", "a.md").unwrap();
    p.docs.close("w");
    p.write("a.md", b"2");
    p.batch(vec![change("a.md", ChangeKind::Modified)]);
    assert!(p.changes().is_empty());
    // opening another document replaces the first
    p.write("b.md", b"1");
    p.open("w", "a.md").unwrap();
    p.open("w", "b.md").unwrap();
    p.write("a.md", b"3");
    p.batch(vec![change("a.md", ChangeKind::Modified)]);
    assert!(p.changes().is_empty());
}

#[test]
fn media_changes_are_told_by_signature_and_a_rescan_checks_everything() {
    let p = project();
    p.write("pic.png", b"1234");
    p.open("w", "pic.png").unwrap();
    p.write("pic.png", b"12345678");
    p.docs.on_batch(&FsBatch {
        rescan: true,
        ..FsBatch::default()
    });
    let got = p.changes();
    assert_eq!(got.len(), 1);
    assert!(matches!(&got[0].change, DocChange::Changed { version } if version.starts_with("8-")));
}

#[test]
fn text_for_copy_respects_the_boundary() {
    let p = project();
    p.write("a.md", b"hello\r\nworld");
    assert_eq!(p.docs.text("a.md").unwrap(), "hello\r\nworld");
    assert_eq!(p.docs.text("../x").unwrap_err().kind, ErrorKind::PathEscape);
}

#[test]
fn links_resolve_against_the_document() {
    let p = project();
    p.write("docs/a.md", b"x");
    p.write("docs/b.md", b"y");
    p.write("img/p.png", b"z");
    let got = p.docs.resolve_links(
        "docs/a.md",
        &[
            "b.md#top".to_owned(),
            "../img/p.png".to_owned(),
            "missing.md".to_owned(),
            "https://example.com".to_owned(),
            "#here".to_owned(),
            "javascript:alert(1)".to_owned(),
        ],
    );
    assert_eq!(
        got[0],
        LinkTarget::Doc {
            path: "docs/b.md".to_owned(),
            anchor: Some("top".to_owned())
        }
    );
    assert_eq!(
        got[1],
        LinkTarget::Asset {
            path: "img/p.png".to_owned()
        }
    );
    assert!(matches!(got[2], LinkTarget::Missing { .. }));
    assert!(matches!(got[3], LinkTarget::Web { .. }));
    assert_eq!(
        got[4],
        LinkTarget::Anchor {
            id: "here".to_owned()
        }
    );
    assert!(matches!(got[5], LinkTarget::Missing { .. }));
    // a bad document path answers "outside" for all
    assert!(p
        .docs
        .resolve_links("../x.md", &["a.md".to_owned()])
        .iter()
        .all(|t| *t == LinkTarget::Outside));
}

#[test]
fn at_references_see_the_project() {
    let p = project();
    p.write("docs/plan.md", b"plan");
    p.write("prompt.md", b"Read @docs/plan.md and @docs/none.md\n");
    let view = p.open("w", "prompt.md").unwrap();
    let Body::Markdown { blocks, .. } = view.body else {
        panic!()
    };
    let html = blocks[0].html.as_ref().unwrap();
    assert!(
        html.contains("class=\"at-ref\" href=\"/docs/plan.md\""),
        "{html}"
    );
    assert!(!html.contains("/docs/none.md\""));
}

// ---- single file ------------------------------------------------------------------------------------------------

fn single(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> (Arc<Documents>, Arc<Recorder>) {
    let file = dir.path().join(name);
    std::fs::write(&file, bytes).unwrap();
    let events = Arc::new(Recorder::default());
    let docs = Documents::polled(
        DocScope::File(FileScope::open(&file).unwrap()),
        events.clone(),
    );
    (docs, events)
}

#[test]
fn a_single_file_workspace_reads_only_its_own_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("other.md"), "o").unwrap();
    let (docs, _) = single(&dir, "note.md", b"# Note\n\n![i](pic.png)\n");
    let view = docs.open("w", "note.md", &[], &formats()).unwrap();
    let Body::Markdown { blocks, .. } = &view.body else {
        panic!()
    };
    // pictures next to the file resolve against its folder
    assert!(blocks[1]
        .html
        .as_ref()
        .unwrap()
        .contains("pm-asset:pic.png"));
    assert_eq!(
        docs.open("w", "other.md", &[], &formats())
            .unwrap_err()
            .kind,
        ErrorKind::NotFound
    );
    assert!(docs.open("w", "../note.md", &[], &formats()).is_err());
    assert_eq!(
        docs.open("w", "NOTE.md", &[], &formats()).is_ok(),
        cfg!(windows)
    );
}

#[test]
fn a_single_file_is_polled_and_the_poller_stops_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let (docs, events) = single(&dir, "note.md", b"v1");
    docs.open("w", "note.md", &[], &formats()).unwrap();
    std::fs::write(dir.path().join("note.md"), b"v2").unwrap();
    let started = Instant::now();
    let mut got = Vec::new();
    while got.is_empty() && started.elapsed() < Duration::from_secs(6) {
        std::thread::sleep(Duration::from_millis(100));
        got = events.take();
    }
    assert!(
        matches!(got.first(), Some(CoreEvent::DocumentChanged(c)) if c.window == "w" && matches!(c.change, DocChange::Changed { .. })),
        "{got:?}"
    );
    let dropped = Instant::now();
    drop(docs);
    assert!(
        dropped.elapsed() < Duration::from_secs(2),
        "the poller did not stop promptly"
    );
}

#[test]
fn threads_opening_and_batches_do_not_deadlock() {
    let p = Arc::new(project());
    for i in 0..4 {
        p.write(&format!("d{i}.md"), format!("# {i}").as_bytes());
    }
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let p = Arc::clone(&p);
            std::thread::spawn(move || {
                for round in 0..30 {
                    let path = format!("d{}.md", (t + round) % 4);
                    let _ = p.docs.open(&format!("w-{t}"), &path, &[], &formats());
                    p.batch(vec![change(&path, ChangeKind::Modified)]);
                    if round % 7 == 0 {
                        p.docs.close(&format!("w-{t}"));
                    }
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn the_core_markdown_format_is_always_there() {
    let registry = FormatRegistry::core();
    assert!(registry
        .pick("a.md", b"")
        .is_some_and(|f| f.id() == MarkdownFormat.id()));
}
