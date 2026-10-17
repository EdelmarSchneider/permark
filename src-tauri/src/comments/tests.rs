//! The comments of a project, on real files.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use crate::vault::Vault;

const PLAN: &str = "\
# Plan

Intro paragraph.

## Rollout

Ship it on Friday, after the review.

## Risks

Many things can go wrong.
";

struct Fx {
    project: tempfile::TempDir,
    state: tempfile::TempDir,
    book: CommentBook,
    heard: Arc<AtomicUsize>,
}

impl Fx {
    fn write(&self, rel: &str, text: &str) {
        let path = self.project.path().join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }

    fn file(&self) -> PathBuf {
        self.state.path().join("comments.json")
    }

    fn comment(&self, path: &str, quote: &str, text: &str) -> CommentView {
        self.book
            .add(NewComment {
                path: path.to_owned(),
                quote: quote.to_owned(),
                text: text.to_owned(),
                heading_path: Vec::new(),
                line: 0,
                end_line: 0,
            })
            .expect("a comment")
    }
}

fn fx() -> Fx {
    let project = tempfile::tempdir().expect("project");
    let state = tempfile::tempdir().expect("state");
    std::fs::create_dir_all(project.path().join("docs")).expect("mkdir");
    std::fs::write(project.path().join("docs/plan.md"), PLAN).expect("write");
    let heard = Arc::new(AtomicUsize::new(0));
    let listener: Listener = {
        let heard = Arc::clone(&heard);
        Arc::new(move || {
            heard.fetch_add(1, Ordering::SeqCst);
        })
    };
    let vault = Arc::new(Vault::open(project.path()).expect("vault"));
    let book = CommentBook::new(vault, state.path().join("comments.json")).with_listener(listener);
    Fx {
        project,
        state,
        book,
        heard,
    }
}

#[test]
fn a_comment_takes_its_place_from_the_document() {
    let fx = fx();
    // the window's idea of where the selection was is only a hint
    let view = fx
        .book
        .add(NewComment {
            path: "docs/plan.md".to_owned(),
            quote: "after the review".to_owned(),
            text: "  Which review?  ".to_owned(),
            heading_path: vec!["Wrong".to_owned()],
            line: 1,
            end_line: 1,
        })
        .expect("a comment");
    assert_eq!(view.path, "docs/plan.md");
    assert_eq!(view.text, "Which review?");
    assert_eq!(view.quote, "after the review");
    assert_eq!(view.heading_path, ["Plan", "Rollout"]);
    assert_eq!((view.line, view.end_line), (7, 7));
    assert_eq!(view.state, PassageState::Anchored);
    assert!(!view.done);
    assert!(
        view.id.starts_with('c') && view.id.len() == 9,
        "{}",
        view.id
    );
    assert!(view.created_ms > 0);
}

#[test]
fn a_comment_on_text_the_document_does_not_show_keeps_the_hints_of_the_window() {
    let fx = fx();
    let view = fx
        .book
        .add(NewComment {
            path: "docs/plan.md".to_owned(),
            quote: "a label the reading view drew".to_owned(),
            text: "Odd".to_owned(),
            heading_path: vec!["Plan".to_owned(), "Risks".to_owned()],
            line: 11,
            end_line: 11,
        })
        .expect("a comment");
    // not found, so the section it was said to be in is where it points
    assert_eq!(view.state, PassageState::PassageChanged);
    assert_eq!((view.line, view.end_line), (9, 9));
    assert_eq!(view.heading_path, ["Plan", "Risks"]);
}

#[test]
fn a_comment_needs_a_passage_and_some_words() {
    let fx = fx();
    for (quote, text) in [
        ("", "words"),
        ("   ", "words"),
        ("passage", ""),
        ("passage", "  "),
    ] {
        let result = fx.book.add(NewComment {
            path: "docs/plan.md".to_owned(),
            quote: quote.to_owned(),
            text: text.to_owned(),
            heading_path: Vec::new(),
            line: 0,
            end_line: 0,
        });
        assert!(
            matches!(result, Err(CommentError::Empty)),
            "{quote:?} {text:?}"
        );
    }
    assert!(!fx.file().exists());
}

#[test]
fn only_markdown_documents_of_the_project_take_comments() {
    let fx = fx();
    fx.write("docs/readme.txt", "plain text");
    for path in [
        "docs/readme.txt",
        "docs/missing.md",
        "docs",
        "../outside.md",
        "/etc/hosts",
        "C:/Windows/win.ini",
    ] {
        let result = fx.book.add(NewComment {
            path: path.to_owned(),
            quote: "plain".to_owned(),
            text: "comment".to_owned(),
            heading_path: Vec::new(),
            line: 1,
            end_line: 1,
        });
        assert!(
            matches!(result, Err(CommentError::NotADocument(_))),
            "{path}: {result:?}"
        );
    }
    assert!(!fx.file().exists());
}

#[test]
fn a_single_file_has_no_comments() {
    let fx = fx();
    let vault = Arc::new(Vault::open(&fx.project.path().join("docs/plan.md")).expect("vault"));
    let book = CommentBook::new(vault, fx.file());
    let result = book.add(NewComment {
        path: "plan.md".to_owned(),
        quote: "Intro".to_owned(),
        text: "comment".to_owned(),
        heading_path: Vec::new(),
        line: 1,
        end_line: 1,
    });
    assert!(matches!(result, Err(CommentError::SingleFile)));
}

#[test]
fn a_project_keeps_a_limited_number_of_comments() {
    let fx = fx();
    let comments: Vec<Comment> = (0..MAX_COMMENTS)
        .map(|n| Comment {
            id: format!("c{n:08x}"),
            path: "docs/plan.md".to_owned(),
            text: "x".to_owned(),
            anchor: Anchor::default(),
            created_ms: 1,
            done: None,
        })
        .collect();
    let contents = serde_json::json!({ "version": 1, "comments": comments });
    std::fs::write(fx.file(), contents.to_string()).expect("write");
    let result = fx.book.add(NewComment {
        path: "docs/plan.md".to_owned(),
        quote: "Intro".to_owned(),
        text: "one too many".to_owned(),
        heading_path: Vec::new(),
        line: 1,
        end_line: 1,
    });
    assert!(matches!(result, Err(CommentError::TooMany)), "{result:?}");
}

#[test]
fn a_long_quote_and_a_long_comment_are_cut() {
    let fx = fx();
    let view = fx
        .book
        .add(NewComment {
            path: "docs/plan.md".to_owned(),
            quote: format!("Intro paragraph. {}", "é".repeat(MAX_QUOTE_CHARS * 2)),
            text: "ü".repeat(MAX_COMMENT_CHARS * 2),
            heading_path: Vec::new(),
            line: 3,
            end_line: 3,
        })
        .expect("a comment");
    assert_eq!(view.quote.chars().count(), MAX_QUOTE_CHARS);
    assert_eq!(view.text.chars().count(), MAX_COMMENT_CHARS);
}

#[test]
fn the_passage_is_found_again_when_the_document_changes() {
    let fx = fx();
    let comment = fx.comment("docs/plan.md", "after the review", "Which review?");
    assert_eq!(comment.line, 7);

    // text is added above the passage: it moved down
    fx.write(
        "docs/plan.md",
        &PLAN.replace(
            "Intro paragraph.",
            "Intro paragraph.\n\nA new paragraph.\n\nAnd another.",
        ),
    );
    let listed = fx.book.list(true);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].state, PassageState::Anchored);
    assert_eq!((listed[0].line, listed[0].end_line), (11, 11));
    assert_eq!(listed[0].id, comment.id);

    // the place that moved is written down
    let kept = std::fs::read_to_string(fx.file()).expect("read");
    assert!(kept.contains("\"line\": 11"), "{kept}");
}

#[test]
fn a_rewritten_passage_is_kept_and_says_so_with_the_best_place() {
    let fx = fx();
    fx.comment("docs/plan.md", "after the review", "Which review?");
    fx.write(
        "docs/plan.md",
        &PLAN.replace("Ship it on Friday, after the review.", "Ship it next week."),
    );
    // read without writing anything down
    let before = std::fs::read_to_string(fx.file()).expect("read");
    let listed = fx.book.list(false);
    assert_eq!(listed[0].state, PassageState::PassageChanged);
    // its section is still there
    assert_eq!(listed[0].line, 5);
    assert_eq!(listed[0].heading_path, ["Plan", "Rollout"]);
    assert_eq!(listed[0].quote, "after the review");
    assert_eq!(std::fs::read_to_string(fx.file()).expect("read"), before);

    // the section is renamed too: the line it was on, inside the document
    fx.write("docs/plan.md", "# Other\n\nShort.\n");
    let listed = fx.book.list(true);
    assert_eq!(listed[0].state, PassageState::PassageChanged);
    assert_eq!(listed[0].line, 3);
    assert_eq!(std::fs::read_to_string(fx.file()).expect("read"), before);
}

#[test]
fn a_deleted_document_leaves_its_comments_and_a_restored_one_gets_them_back() {
    let fx = fx();
    fx.comment("docs/plan.md", "after the review", "Which review?");
    std::fs::remove_file(fx.project.path().join("docs/plan.md")).expect("rm");
    let listed = fx.book.list(true);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].state, PassageState::DocumentGone);
    assert_eq!(listed[0].line, 7);
    fx.write("docs/plan.md", PLAN);
    assert_eq!(fx.book.list(true)[0].state, PassageState::Anchored);
}

#[test]
fn a_document_that_cannot_be_read_as_text_does_not_lose_its_comments() {
    let fx = fx();
    fx.comment("docs/plan.md", "after the review", "Which review?");
    std::fs::write(
        fx.project.path().join("docs/plan.md"),
        [0_u8, 159, 146, 150, 0],
    )
    .expect("write");
    let listed = fx.book.list(true);
    assert_eq!(listed[0].state, PassageState::PassageChanged);
    assert_eq!(listed[0].line, 7);
}

#[test]
fn comments_are_listed_by_document_then_line() {
    let fx = fx();
    fx.write("a.md", "# A\n\nFirst words here.\n");
    let late = fx.comment("docs/plan.md", "Many things can go wrong", "late");
    let early = fx.comment("docs/plan.md", "Intro paragraph", "early");
    let other = fx.comment("a.md", "First words", "other");
    let ids: Vec<_> = fx.book.list(false).into_iter().map(|n| n.id).collect();
    assert_eq!(ids, [other.id, early.id, late.id]);
}

#[test]
fn the_person_marks_a_comment_done_and_open_again() {
    let fx = fx();
    let comment = fx.comment("docs/plan.md", "Intro paragraph", "Rewrite");
    let done = fx.book.set_done(&comment.id, true).expect("done");
    assert!(done.done);
    assert_eq!(done.done_by, Some(DoneBy::Person));
    assert!(done.done_ms.is_some());
    assert_eq!(done.reply, None);
    let open = fx.book.set_done(&comment.id, false).expect("open");
    assert!(!open.done);
    assert_eq!(open.done_by, None);
    assert!(matches!(
        fx.book.set_done("nope", true),
        Err(CommentError::NotFound(_))
    ));
}

#[test]
fn an_agent_resolves_a_comment_and_leaves_a_reply() {
    let fx = fx();
    let comment = fx.comment("docs/plan.md", "Intro paragraph", "Rewrite");
    let view = fx
        .book
        .resolve(&comment.id, Some("  Rewritten in 3f2a1c  "))
        .expect("resolved");
    assert!(view.done);
    assert_eq!(view.done_by, Some(DoneBy::Agent));
    assert_eq!(view.reply.as_deref(), Some("Rewritten in 3f2a1c"));
    // the reply and the time are in the file
    let kept = std::fs::read_to_string(fx.file()).expect("read");
    assert!(kept.contains("\"by\": \"agent\""), "{kept}");
    assert!(kept.contains("Rewritten in 3f2a1c"), "{kept}");

    // resolving again is not an error, and keeps who did it first
    let again = fx.book.resolve(&comment.id, None).expect("again");
    assert_eq!(again.reply.as_deref(), Some("Rewritten in 3f2a1c"));
    let replaced = fx.book.resolve(&comment.id, Some("Fixed")).expect("again");
    assert_eq!(replaced.reply.as_deref(), Some("Fixed"));
    assert!(matches!(
        fx.book.resolve("nope", None),
        Err(CommentError::NotFound(_))
    ));
}

#[test]
fn an_agent_adds_its_reply_to_a_comment_the_person_already_closed() {
    let fx = fx();
    let comment = fx.comment("docs/plan.md", "Intro paragraph", "Rewrite");
    fx.book.set_done(&comment.id, true).expect("done");
    let view = fx
        .book
        .resolve(&comment.id, Some("Already so"))
        .expect("ok");
    assert_eq!(view.done_by, Some(DoneBy::Person));
    assert_eq!(view.reply.as_deref(), Some("Already so"));
}

#[test]
fn a_reply_is_short_and_never_empty() {
    let fx = fx();
    let comment = fx.comment("docs/plan.md", "Intro paragraph", "Rewrite");
    let long = "r".repeat(MAX_REPLY_CHARS * 3);
    let view = fx.book.resolve(&comment.id, Some(&long)).expect("resolved");
    assert_eq!(view.reply.map(|r| r.chars().count()), Some(MAX_REPLY_CHARS));
    let other = fx.comment("docs/plan.md", "Many things", "Another");
    let view = fx.book.resolve(&other.id, Some("   ")).expect("resolved");
    assert_eq!(view.reply, None);
}

#[test]
fn a_comment_can_be_deleted() {
    let fx = fx();
    let one = fx.comment("docs/plan.md", "Intro paragraph", "one");
    let two = fx.comment("docs/plan.md", "Many things", "two");
    fx.book.delete(&one.id).expect("deleted");
    let left: Vec<_> = fx.book.list(false).into_iter().map(|n| n.id).collect();
    assert_eq!(left, [two.id]);
    assert!(matches!(
        fx.book.delete(&one.id),
        Err(CommentError::NotFound(_))
    ));
}

#[test]
fn comments_follow_a_renamed_document_and_a_renamed_folder() {
    let fx = fx();
    fx.write("docs/sub/b.md", "# B\n\nSome words.\n");
    fx.comment("docs/plan.md", "Intro paragraph", "plan");
    fx.comment("docs/sub/b.md", "Some words", "b");

    std::fs::rename(
        fx.project.path().join("docs/plan.md"),
        fx.project.path().join("docs/roadmap.md"),
    )
    .expect("mv");
    fx.book.renamed("docs/plan.md", "docs/roadmap.md", false);
    // a folder that merely starts with the same letters is not the folder
    fx.book.renamed("docs/su", "elsewhere", true);
    std::fs::rename(
        fx.project.path().join("docs/sub"),
        fx.project.path().join("docs/part"),
    )
    .expect("mv");
    fx.book.renamed("docs/sub", "docs/part", true);

    let listed = fx.book.list(false);
    let paths: Vec<_> = listed.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(paths, ["docs/part/b.md", "docs/roadmap.md"]);
    assert!(listed.iter().all(|n| n.state == PassageState::Anchored));
}

#[test]
fn a_listener_hears_every_change_and_no_read() {
    let fx = fx();
    let comment = fx.comment("docs/plan.md", "Intro paragraph", "one");
    assert_eq!(fx.heard.load(Ordering::SeqCst), 1);
    fx.book.list(true);
    fx.book.list(false);
    assert_eq!(fx.heard.load(Ordering::SeqCst), 1, "reading is silent");
    fx.book.set_done(&comment.id, true).expect("done");
    assert_eq!(fx.heard.load(Ordering::SeqCst), 2);
    // nothing changed: nobody is told
    fx.book.set_done(&comment.id, true).expect("done");
    assert_eq!(fx.heard.load(Ordering::SeqCst), 2);
    fx.book.resolve(&comment.id, Some("ok")).expect("resolved");
    assert_eq!(fx.heard.load(Ordering::SeqCst), 3);
    fx.book.resolve(&comment.id, Some("ok")).expect("resolved");
    assert_eq!(fx.heard.load(Ordering::SeqCst), 3);
    fx.book.renamed("elsewhere.md", "else.md", false);
    assert_eq!(fx.heard.load(Ordering::SeqCst), 3);
    fx.book.renamed("docs/plan.md", "docs/p.md", false);
    assert_eq!(fx.heard.load(Ordering::SeqCst), 4);
    fx.book.delete(&comment.id).expect("deleted");
    assert_eq!(fx.heard.load(Ordering::SeqCst), 5);
}

#[test]
fn another_comment_book_on_the_same_file_sees_the_same_comments() {
    // `permark mcp` reads and resolves the comments of a project whose window is closed
    let fx = fx();
    let comment = fx.comment("docs/plan.md", "Intro paragraph", "one");
    let vault = Arc::new(Vault::open(fx.project.path()).expect("vault"));
    let offline = CommentBook::new(vault, fx.file());
    assert_eq!(offline.list(false).len(), 1);
    offline
        .resolve(&comment.id, Some("done"))
        .expect("resolved");
    let seen = fx.book.list(false);
    assert_eq!(seen[0].reply.as_deref(), Some("done"));
}

#[test]
fn the_comment_book_follows_the_state_to_its_new_place() {
    let fx = fx();
    fx.comment("docs/plan.md", "Intro paragraph", "one");
    let moved = fx.state.path().join("local").join("comments.json");
    fs_copy(&fx.file(), &moved);
    fx.book.relocate(moved.clone());
    assert_eq!(fx.book.list(false).len(), 1);
    fx.comment("docs/plan.md", "Many things", "two");
    assert!(std::fs::read_to_string(moved)
        .expect("read")
        .contains("two"));
    assert!(!std::fs::read_to_string(fx.file())
        .expect("read")
        .contains("two"));
}

fn fs_copy(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to.parent().expect("parent")).expect("mkdir");
    std::fs::copy(from, to).expect("copy");
}
