//! The review of code and plain text against a real project: what the person last looked at, the hunks, keep and
//! revert byte for byte, stale versions, the commit, the journal.

use std::time::{Duration, Instant};

use git2::{IndexAddOption, Repository, Signature};
use permark_core::activity::{Fact, ReviewOp};
use permark_core::ErrorKind;
use serde_json::{json, Value};

use super::tests::{wait_until, Harness};

const CODE: &str = "fn main() {\n    println!(\"a\");\n    println!(\"b\");\n}\n\nfn helper() -> u32 {\n    1\n}\n";

/// The person looks at the file as it is on disk.
fn look(h: &Harness, rel: &str) {
    h.review("seen", json!({"path": rel, "hash": h.hash(rel)}))
        .unwrap();
}

fn open(h: &Harness, rel: &str) -> Value {
    h.review("open", json!({"path": rel})).unwrap()
}

fn state(h: &Harness, rel: &str) -> String {
    h.state(rel)["state"].as_str().unwrap().to_owned()
}

fn change_args(rel: &str, view: &Value, index: usize) -> Value {
    json!({"path": rel, "hash": view["hash"], "index": index})
}

fn all_args(rel: &str, view: &Value) -> Value {
    json!({"path": rel, "hash": view["hash"]})
}

#[test]
fn a_code_file_is_compared_with_the_version_last_looked_at() {
    let mut h = Harness::new(false);
    h.write("src/main.rs", CODE);
    h.open();
    // never looked at: nothing to compare with
    assert_eq!(state(&h, "src/main.rs"), "noBasis");
    look(&h, "src/main.rs");
    assert_eq!(state(&h, "src/main.rs"), "same");

    let changed = CODE
        .replace("\"b\"", "\"B\"")
        .replace("    1\n", "    1\n    + 1\n");
    h.write("src/main.rs", &changed);
    assert_eq!(h.state("src/main.rs")["count"], 2);

    let view = open(&h, "src/main.rs");
    assert_eq!(view["unit"], "lines");
    assert_eq!(view["canKeep"], true);
    assert_eq!(view["basis"], "read");
    let hunks = view["changes"].as_array().unwrap();
    assert_eq!(hunks.len(), 2);
    // line 3 was replaced: the new line is index 2, the old one comes along
    assert_eq!(hunks[0]["kind"], "edited");
    assert_eq!(
        (hunks[0]["start"].as_u64(), hunks[0]["len"].as_u64()),
        (Some(2), Some(1))
    );
    assert_eq!(hunks[0]["old"], json!(["    println!(\"b\");"]));
    assert_eq!(hunks[0]["whitespace"], false);
    assert_eq!(hunks[0]["ghosts"], json!([]));
    // a line added after `1`
    assert_eq!(hunks[1]["kind"], "added");
    assert_eq!(hunks[1]["old"], json!([]));

    // revert the first one only
    let after = h
        .review("revert", change_args("src/main.rs", &view, 0))
        .unwrap();
    assert_eq!(after["count"], 1);
    assert_eq!(
        String::from_utf8(h.read("src/main.rs")).unwrap(),
        CODE.replace("    1\n", "    1\n    + 1\n")
    );
    // undo puts the agent's line back
    let undone = h.review("undo", json!({"path": "src/main.rs"})).unwrap();
    assert_eq!(undone["count"], 2);
    assert_eq!(String::from_utf8(h.read("src/main.rs")).unwrap(), changed);
    // revert everything
    let view = open(&h, "src/main.rs");
    h.review("revertAll", all_args("src/main.rs", &view))
        .unwrap();
    assert_eq!(h.read("src/main.rs"), CODE.as_bytes());
    assert_eq!(state(&h, "src/main.rs"), "same");
}

/// Every way a text can end its lines: the revert gives back the very bytes.
#[test]
fn reverting_gives_back_the_bytes_whatever_the_endings() {
    let cases: [(&str, &[u8], &[u8]); 12] = [
        ("lf", b"a\nb\nc\n", b"a\nB\nc\nd\n"),
        ("crlf", b"a\r\nb\r\nc\r\n", b"a\r\nB\r\nc\r\nd\r\n"),
        ("no newline at the end", b"a\nb\nc", b"a\nB\nc\nd"),
        ("newline added at the end", b"a\nb", b"a\nb\n"),
        ("newline removed at the end", b"a\nb\n", b"a\nb"),
        (
            "bom and crlf",
            b"\xef\xbb\xbfa\r\nb\r\n",
            b"\xef\xbb\xbfa\r\nX\r\nb\r\n",
        ),
        ("lines gone", b"a\r\nb\r\nc\r\nd\r\n", b"a\r\nd\r\n"),
        (
            "a replaced line keeps its own ending",
            b"a\r\nb\nc\r\nd\n",
            b"a\r\nB\nc\r\nD\n",
        ),
        (
            "unicode and tabs",
            "é\tx\r\n日本語\r\n😀\r\n".as_bytes(),
            "é\ty\r\n日本\r\n😀\r\nnew\r\n".as_bytes(),
        ),
        (
            "whitespace only",
            b"def f():\n    return 1\n",
            b"def f():\n\treturn 1\n",
        ),
        ("everything", b"one\ntwo\nthree\n", b"x\ny\n"),
        ("emptied", b"one\ntwo\n", b""),
    ];
    for (name, original, changed) in cases {
        let mut h = Harness::new(false);
        h.write_bytes("f.txt", original);
        h.open();
        look(&h, "f.txt");
        h.write_bytes("f.txt", changed);
        let view = open(&h, "f.txt");
        assert!(view["count"].as_u64().unwrap() >= 1, "{name}");
        h.review("revertAll", all_args("f.txt", &view)).unwrap();
        assert_eq!(
            h.read("f.txt"),
            original,
            "{name}: {:?}",
            String::from_utf8_lossy(&h.read("f.txt"))
        );
        // and one hunk at a time, from the last to the first, gives the same bytes
        h.write_bytes("f.txt", changed);
        loop {
            let view = open(&h, "f.txt");
            let count = usize::try_from(view["count"].as_u64().unwrap()).unwrap();
            if count == 0 {
                break;
            }
            h.review("revert", change_args("f.txt", &view, count - 1))
                .unwrap();
        }
        assert_eq!(h.read("f.txt"), original, "{name}, one by one");
    }
}

#[test]
fn mixed_endings_keep_every_line_that_is_not_in_a_hunk_as_it_was() {
    let mut h = Harness::new(false);
    let original = b"keep1\r\nkeep2\nold\r\nkeep3\nkeep4\r\n";
    h.write_bytes("m.txt", original);
    h.open();
    look(&h, "m.txt");
    // the agent deletes the middle line; it comes back with the ending most lines use (here the line feed
    // and the carriage return pair are tied: the line feed wins), all other lines are the same bytes
    h.write_bytes("m.txt", b"keep1\r\nkeep2\nkeep3\nkeep4\r\n");
    let view = open(&h, "m.txt");
    h.review("revertAll", all_args("m.txt", &view)).unwrap();
    let back = String::from_utf8(h.read("m.txt")).unwrap();
    assert!(back.starts_with("keep1\r\nkeep2\n"));
    assert!(back.ends_with("keep3\nkeep4\r\n"));
    assert!(back.contains("old"));
}

#[test]
fn a_file_that_changed_meanwhile_is_never_overwritten() {
    let mut h = Harness::new(false);
    h.write("a.rs", CODE);
    h.open();
    look(&h, "a.rs");
    h.write("a.rs", &CODE.replace("\"a\"", "\"A\""));
    let view = open(&h, "a.rs");
    // another writer comes in after the view was drawn
    let theirs = CODE.replace("\"a\"", "\"THEIRS\"");
    h.write("a.rs", &theirs);
    for (method, args) in [
        ("revert", change_args("a.rs", &view, 0)),
        ("revertAll", all_args("a.rs", &view)),
        ("keep", change_args("a.rs", &view, 0)),
        ("keepAll", all_args("a.rs", &view)),
    ] {
        let error = h.review(method, args).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Conflict, "{method}");
        assert_eq!(error.detail.as_deref(), Some("stale"), "{method}");
        assert_eq!(h.read("a.rs"), theirs.as_bytes(), "{method} left the file");
    }
    // undo of a revert that was made on a version that is gone
    let view = open(&h, "a.rs");
    h.review("revert", change_args("a.rs", &view, 0)).unwrap();
    h.write("a.rs", "somebody else\n");
    let error = h.review("undo", json!({"path": "a.rs"})).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(h.read("a.rs"), b"somebody else\n");
}

#[test]
fn keeping_moves_the_base_and_looking_at_a_changed_file_keeps_the_review_open() {
    let mut h = Harness::new(false);
    h.write("k.rs", CODE);
    h.open();
    look(&h, "k.rs");
    let changed = CODE
        .replace("\"b\"", "\"B\"")
        .replace("    1\n", "    1\n    + 1\n");
    h.write("k.rs", &changed);
    // the person looks at the changed file: the version before it stays the base of the review
    look(&h, "k.rs");
    assert_eq!(h.state("k.rs")["count"], 2, "held");
    // keep the first change: it becomes part of the base
    let view = open(&h, "k.rs");
    let kept = h.review("keep", change_args("k.rs", &view, 0)).unwrap();
    assert_eq!(kept["count"], 1);
    assert_eq!(h.read("k.rs"), changed.as_bytes(), "keeping writes nothing");
    // undo takes the keep back
    let undone = h.review("undo", json!({"path": "k.rs"})).unwrap();
    assert_eq!(undone["count"], 2);
    // keep all: the file is the version seen, nothing is held
    let view = open(&h, "k.rs");
    let all = h.review("keepAll", all_args("k.rs", &view)).unwrap();
    assert_eq!(all["count"], 0);
    assert_eq!(state(&h, "k.rs"), "same");
    // another change shows against the kept version
    h.write("k.rs", &changed.replace("fn main", "fn start"));
    assert_eq!(h.state("k.rs")["count"], 1);
}

#[test]
fn leaving_the_document_ends_the_review_of_what_was_looked_at() {
    let mut h = Harness::new(false);
    h.write("r.rs", CODE);
    h.open();
    look(&h, "r.rs");
    h.write("r.rs", &CODE.replace("\"a\"", "\"A\""));
    look(&h, "r.rs");
    assert_eq!(state(&h, "r.rs"), "changes");
    h.review("release", json!({"path": "r.rs"})).unwrap();
    assert_eq!(state(&h, "r.rs"), "same");
}

#[test]
fn the_persons_own_save_is_not_news_to_the_person() {
    let mut h = Harness::new(false);
    h.write("o.rs", CODE);
    h.open();
    look(&h, "o.rs");
    // the person saves a change in Source: the window tells the documents service
    let saved = CODE.replace("fn helper", "fn assistant");
    h.write("o.rs", &saved);
    h.core
        .as_ref()
        .unwrap()
        .documents()
        .saved("w", "o.rs", &h.hash("o.rs"));
    look(&h, "o.rs");
    assert_eq!(state(&h, "o.rs"), "same");
    // an agent's change after it is news again
    h.write("o.rs", &saved.replace("fn main", "fn start"));
    look(&h, "o.rs");
    assert_eq!(h.state("o.rs")["count"], 1);
}

#[test]
fn what_was_looked_at_is_remembered_between_sessions() {
    let mut h = Harness::new(false);
    h.write("p.rs", CODE);
    h.open();
    look(&h, "p.rs");
    drop(h.core.take());
    h.write("p.rs", &CODE.replace("\"a\"", "\"A\""));
    h.open();
    wait_until("the review to answer", || {
        h.review("basis", Value::Null).is_ok()
    });
    assert_eq!(h.state("p.rs")["count"], 1);
    let status = h.review("status", json!({"path": "p.rs"})).unwrap();
    assert_eq!(status["basis"], "read");
}

#[test]
fn a_look_at_a_version_that_is_not_on_disk_counts_for_nothing() {
    let mut h = Harness::new(false);
    h.write("s.rs", CODE);
    h.open();
    // the window drew an older version: the next look tells
    h.review("seen", json!({"path": "s.rs", "hash": "0000000000000000"}))
        .unwrap();
    assert_eq!(state(&h, "s.rs"), "noBasis");
    // a bad hash, a Markdown file, a file that is not there
    for (path, hash) in [("s.rs", "not hex"), ("a.md", "0"), ("gone.rs", "0")] {
        h.write("a.md", "# A\n");
        h.review("seen", json!({"path": path, "hash": hash}))
            .unwrap();
    }
    assert!(h
        .review("seen", json!({"path": "../x.rs", "hash": "0"}))
        .is_err());
}

#[test]
fn the_states_of_a_text_without_a_review() {
    let mut h = Harness::new(false);
    h.write("t.txt", "text\n");
    h.write_bytes("b.bin", b"\x00\x01binary\x00");
    h.write("big.log", &"x".repeat(2 * 1024 * 1024 + 1));
    h.write("lossy.txt", "ok");
    h.write_bytes("lossy.txt", b"ok \xff\xfe broken");
    h.open();
    assert_eq!(state(&h, "t.txt"), "noBasis");
    assert_eq!(state(&h, "b.bin"), "unsupported");
    assert_eq!(state(&h, "big.log"), "tooLarge");
    assert_eq!(state(&h, "lossy.txt"), "unsupported");
    assert_eq!(
        h.review("open", json!({"path": "big.log"}))
            .unwrap_err()
            .kind,
        ErrorKind::TooLarge
    );
    assert_eq!(
        h.review("open", json!({"path": "t.txt"})).unwrap_err().kind,
        ErrorKind::Unavailable
    );
    assert_eq!(
        h.review("open", json!({"path": "nothing.rs"}))
            .unwrap_err()
            .kind,
        ErrorKind::NotFound
    );
    // a text that is the same as the one looked at
    look(&h, "t.txt");
    assert_eq!(state(&h, "t.txt"), "same");
    // line endings alone are no change
    h.write_bytes("t.txt", b"text\r\n");
    assert_eq!(state(&h, "t.txt"), "same");
}

#[test]
fn the_last_commit_is_a_basis_for_code_and_a_commit_cannot_be_kept() {
    let mut h = Harness::new(true);
    let repo = Repository::init(h.root()).unwrap();
    h.write("c.rs", CODE);
    {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("T", "t@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "first", &tree, &[])
            .unwrap();
    }
    h.open();
    wait_until("git", || {
        h.call("git", "snapshot", Value::Null)
            .is_ok_and(|v| !v.is_null())
    });
    // never looked at, so the read basis has nothing; the commit is asked for by name
    h.write("c.rs", &CODE.replace("\"b\"", "\"B\""));
    assert_eq!(state(&h, "c.rs"), "noBasis");
    let asked = h
        .review("status", json!({"path": "c.rs", "basis": "commit"}))
        .unwrap();
    assert_eq!(asked["state"]["state"], "changes");
    assert_eq!(asked["basis"], "commit");
    let view = h
        .review("open", json!({"path": "c.rs", "basis": "commit"}))
        .unwrap();
    assert_eq!(
        (view["basis"].as_str(), view["canKeep"].as_bool()),
        (Some("commit"), Some(false))
    );
    assert_eq!(view["count"], 1);
    let keep = json!({"path": "c.rs", "hash": view["hash"], "index": 0, "basis": "commit"});
    assert!(h.review("keep", keep.clone()).is_err());
    h.review("revert", keep).unwrap();
    assert_eq!(h.read("c.rs"), CODE.as_bytes());
}

#[test]
fn the_journal_notes_each_choice_with_the_lines_it_was_about() {
    let mut h = Harness::new(false);
    h.write("j.rs", CODE);
    h.open();
    look(&h, "j.rs");
    h.write(
        "j.rs",
        &CODE
            .replace("\"b\"", "\"B\"")
            .replace("    1\n", "    1\n    + 1\n    + 2\n"),
    );
    let view = open(&h, "j.rs");
    h.review("revert", change_args("j.rs", &view, 0)).unwrap();
    let view = open(&h, "j.rs");
    h.review("keep", change_args("j.rs", &view, 0)).unwrap();
    let entries = h.core.as_ref().unwrap().activity().snapshot().entries;
    let reviews: Vec<(ReviewOp, Option<String>)> = entries
        .iter()
        .filter_map(|e| match &e.fact {
            Fact::Review { op, section, .. } => Some((*op, section.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        reviews,
        [
            (ReviewOp::Revert, Some("L3".to_owned())),
            (ReviewOp::Keep, Some("L8-9".to_owned())),
        ]
    );
}

#[test]
fn a_file_of_ten_thousand_lines_is_reviewed_and_reverted_quickly_and_exactly() {
    let mut h = Harness::new(false);
    let original = (0..10_000)
        .map(|i| format!("line {i} of the file\r\n"))
        .collect::<Vec<_>>()
        .concat();
    h.write_bytes("big.txt", original.as_bytes());
    h.open();
    look(&h, "big.txt");
    let mut lines: Vec<String> = original.lines().map(str::to_owned).collect();
    for i in (0..10_000).step_by(97) {
        lines[i] = format!("edited {i}");
    }
    lines.insert(5_000, "inserted".to_owned());
    lines.remove(7_000);
    let changed = lines
        .iter()
        .map(|l| format!("{l}\r\n"))
        .collect::<Vec<_>>()
        .concat();
    h.write_bytes("big.txt", changed.as_bytes());

    let started = Instant::now();
    let view = open(&h, "big.txt");
    assert!(view["count"].as_u64().unwrap() > 100);
    h.review("revertAll", all_args("big.txt", &view)).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(h.read("big.txt"), original.as_bytes());
}

#[test]
fn hostile_calls_about_a_text_are_refused() {
    let mut h = Harness::new(false);
    h.write("h.rs", CODE);
    h.open();
    look(&h, "h.rs");
    h.write("h.rs", &CODE.replace("\"a\"", "\"A\""));
    let view = open(&h, "h.rs");
    let hash = view["hash"].clone();
    for (method, args) in [
        ("revert", json!({"path": "h.rs", "hash": hash, "index": 99})),
        ("keep", json!({"path": "h.rs", "hash": hash, "index": 99})),
        ("revert", json!({"path": "h.rs", "hash": "zz", "index": 0})),
        ("status", json!({"path": "../outside.rs"})),
        ("open", json!({"path": "/etc/passwd"})),
        ("undo", json!({"path": "h.rs"})),
        ("restore", json!({"path": "h.rs"})),
    ] {
        let error = h.review(method, args.clone()).unwrap_err();
        assert!(
            matches!(
                error.kind,
                ErrorKind::InvalidData
                    | ErrorKind::PathEscape
                    | ErrorKind::InvalidPath
                    | ErrorKind::Conflict
                    | ErrorKind::Unavailable
                    | ErrorKind::Unsupported
            ),
            "{method} {args}: {error}"
        );
    }
    assert_eq!(h.read("h.rs"), CODE.replace("\"a\"", "\"A\"").as_bytes());
}
