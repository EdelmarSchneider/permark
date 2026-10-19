use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::events::{CoreEvent, EventSink};
use permark_core::format::units;
use permark_core::paths::Root;
use permark_core::plugins::{PluginId, PluginRegistry, RpcCall};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use permark_core::ErrorKind;
use serde_json::{json, Value};

use super::*;
use crate::anchor::{fold, Analysis, Probe, Resolution};
use crate::model::{Anchor, Book, MAX_COMMENTS};

/// A byte copy of a real `.permark/local/comments.json` written by version 0.5.
const REAL: &str = include_str!("fixtures/real-comments-0.5.json");

fn analysis(markdown: &str) -> Analysis {
    let (parts, last) = units(markdown);
    Analysis::of(&parts, last)
}

fn anchor(quote: &str, line: u32) -> Anchor {
    Anchor {
        quote: quote.to_owned(),
        line,
        end_line: line,
        ..Anchor::default()
    }
}

fn found(a: &Analysis, quote: &str, line: u32) -> Option<(u32, u32, Vec<String>)> {
    match a.resolve(&anchor(quote, line)) {
        Resolution::Found(p) => Some((p.start_line, p.end_line, p.heading_path)),
        Resolution::Changed { .. } => None,
    }
}

// ---- finding a passage again ---------------------------------------------------------------------------------------

#[test]
fn folding_keeps_letters_and_digits_only() {
    assert_eq!(
        fold("A **bold** claim, “quoted” – 12!"),
        "aboldclaimquoted12"
    );
    assert_eq!(fold("日本語 Ünï"), "日本語ünï");
    assert_eq!(fold("  \n\t"), "");
}

#[test]
fn a_quote_from_the_reading_view_is_found_in_the_source_behind_it() {
    let a = analysis("# Plan\n\nThe **first** step is a [link](https://example.com/x) to `code`.\n\nsecond paragraph\n");
    assert_eq!(
        found(&a, "The first step is a link to code.", 1),
        Some((3, 3, vec!["Plan".to_owned()]))
    );
    assert_eq!(found(&a, "second paragraph", 1).map(|f| f.0), Some(5));
}

#[test]
fn a_quote_across_two_blocks_spans_their_lines() {
    let a = analysis("intro\n\nfirst block ends here\n\nsecond block starts here\n\nlast\n");
    assert_eq!(
        found(&a, "ends here second block starts", 1).map(|f| (f.0, f.1)),
        Some((3, 5))
    );
}

#[test]
fn tables_code_html_and_wide_scripts_are_text_too() {
    let a = analysis("| a | b |\n|---|---|\n| cell one | cell two |\n\n```rust\nlet answer = 42;\n```\n\n<div>raw <b>html</b> text</div>\n\n日本語のテキスト 🎉 done\n");
    assert_eq!(found(&a, "cell two", 1).map(|f| f.0), Some(3));
    assert_eq!(
        found(&a, "let answer = 42;", 1).map(|f| (f.0, f.1)),
        Some((5, 7)),
        "a code block is one place: its fences included"
    );
    assert_eq!(found(&a, "raw html text", 1).map(|f| f.0), Some(9));
    assert_eq!(found(&a, "日本語のテキスト", 1).map(|f| f.0), Some(11));
}

#[test]
fn the_words_around_a_repeated_quote_decide_then_the_nearest_line() {
    let md = "alpha beta gamma\n\none two three\n\nmiddle\n\nalpha beta gamma\n\nfour five six\n";
    let a = analysis(md);
    let mut c = anchor("beta", 7);
    // the same words twice: no context, the nearest line to where it was wins
    assert_eq!(found(&a, "beta", 7).map(|f| f.0), Some(7));
    assert_eq!(found(&a, "beta", 1).map(|f| f.0), Some(1));
    // context beats the line
    c.before = fold("one two three middle alpha");
    c.after = fold("gamma four five six");
    match a.resolve(&c) {
        Resolution::Found(p) => assert_eq!(p.start_line, 7, "context says the second one"),
        Resolution::Changed { .. } => panic!("found"),
    }
    // a quote that is everywhere is looked at in the first places only, and never loops
    let wall = "the ".repeat(20_000);
    let big = analysis(&wall);
    assert!(found(&big, "the", 1).is_some());
}

#[test]
fn a_vanished_quote_leads_to_its_section_then_to_its_line() {
    let a = analysis("# Plan\n\nstays\n\n## Rollout\n\nnew words only\n\n# Other\n\ntext\n");
    let mut c = anchor("this sentence was deleted", 99);
    c.heading_path = vec!["Plan".to_owned(), "Rollout".to_owned()];
    assert_eq!(a.resolve(&c), Resolution::Changed { line: 5 });
    // the heading was renamed: the same last heading is looked for, then the line
    c.heading_path = vec!["Gone".to_owned(), "Rollout".to_owned()];
    assert_eq!(a.resolve(&c), Resolution::Changed { line: 5 });
    c.heading_path = vec!["Gone".to_owned()];
    assert_eq!(
        a.resolve(&c),
        Resolution::Changed { line: 11 },
        "kept inside the document"
    );
    c.line = 4;
    assert_eq!(a.resolve(&c), Resolution::Changed { line: 4 });
    c.heading_path = vec!["  OTHER ".to_owned()];
    assert_eq!(
        a.resolve(&c),
        Resolution::Changed { line: 9 },
        "folded like the quote"
    );
}

#[test]
fn nothing_to_find_in_and_nothing_to_find_are_both_fine() {
    let empty = analysis("");
    assert!(found(&empty, "anything", 1).is_none());
    assert_eq!(
        empty.resolve(&anchor("x", 50)),
        Resolution::Changed { line: 1 }
    );
    let a = analysis("some text\n");
    assert!(a.locate("   ", &Probe::default()).is_none());
    assert!(
        a.locate("!!!", &Probe::default()).is_none(),
        "no letters, nothing to look for"
    );
}

#[test]
fn hostile_documents_never_panic() {
    let nested = analysis(&">".repeat(10_000));
    assert!(found(&nested, "x", 1).is_none());
    let lines = analysis(&"> ".repeat(5_000));
    let _ = lines.resolve(&anchor("a", 1));
    let nul = analysis("a\u{0}b\r\nc\u{202e}d\r\n\r\n# \u{0}\n");
    let _ = nul.resolve(&anchor("cd", 1));
    let huge = analysis(&"word ".repeat(300_000));
    assert!(found(&huge, "word word word", 1).is_some());
}

// ---- the file of 0.5 -----------------------------------------------------------------------------------------------

#[test]
fn the_real_0_5_file_reads_and_writes_back_byte_for_byte() {
    let book: Book = serde_json::from_str(REAL).unwrap();
    assert!(book.comments.is_empty());
    assert_eq!(serde_json::to_string_pretty(&book).unwrap(), REAL);
}

#[test]
fn a_file_with_comments_keeps_every_field_including_the_unknown_ones() {
    let text = r#"{
  "version": 1,
  "comments": [
    {
      "id": "c1",
      "path": "docs/a.md",
      "text": "Say this better",
      "anchor": {
        "quote": "the quote",
        "before": "abc",
        "after": "def",
        "headingPath": ["Plan", "Rollout"],
        "line": 12,
        "endLine": 14,
        "future": [1, 2]
      },
      "createdMs": 1760000000000,
      "done": { "atMs": 1760000100000, "by": "agent", "reply": "Rewritten", "note": "x" },
      "pin": true
    },
    {
      "id": "c2",
      "path": "b.md",
      "text": "open one",
      "anchor": { "quote": "q", "line": 1, "endLine": 1 },
      "createdMs": 1,
      "done": null
    }
  ],
  "later": { "k": 1 }
}"#;
    let book: Book = serde_json::from_str(text).unwrap();
    assert_eq!(book.comments.len(), 2);
    assert_eq!(book.comments[0].anchor.heading_path, ["Plan", "Rollout"]);
    assert_eq!(
        book.comments[0].done.as_ref().unwrap().by,
        crate::model::By::Agent
    );
    assert!(book.comments[1].done.is_none() && book.comments[1].replies.is_empty());
    let back: Value = serde_json::to_value(&book).unwrap();
    let original: Value = serde_json::from_str(text).unwrap();
    // equal but for the explicit nulls and empty lists 0.5 wrote and this version leaves out
    assert_eq!(back["later"], original["later"]);
    assert_eq!(back["comments"][0], original["comments"][0]);
    assert_eq!(
        back["comments"][1]["anchor"],
        json!({ "quote": "q", "line": 1, "endLine": 1, "headingPath": [] }),
        "the list 0.5 always wrote is written"
    );
    assert_eq!(original["comments"][1]["anchor"]["quote"], "q");
}

// ---- in a project --------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Sink(Mutex<Vec<String>>);

impl EventSink for Sink {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, .. } = event {
            self.0.lock().unwrap().push(name);
        }
    }
}

struct Harness {
    base: tempfile::TempDir,
    project: tempfile::TempDir,
    sink: Arc<Sink>,
    env: CoreEnv,
    core: Option<Arc<ProjectCore>>,
}

impl Harness {
    fn new() -> Self {
        let sink = Arc::new(Sink::default());
        let base = tempfile::tempdir().unwrap();
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
            sink.clone(),
            Distribution::Direct,
            AppSettings::default(),
        );
        Self {
            base,
            project: tempfile::tempdir().unwrap(),
            sink,
            env,
            core: None,
        }
    }

    fn root(&self) -> PathBuf {
        dunce::canonicalize(self.project.path()).unwrap()
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn open(&mut self) {
        self.core = Some(ProjectCore::open(
            Root::open(&self.root()).unwrap(),
            &self.env,
        ));
    }

    fn close(&mut self) {
        self.core = None;
    }

    fn core(&self) -> &Arc<ProjectCore> {
        self.core.as_ref().unwrap()
    }

    fn call(&self, method: &str, args: Value) -> Result<Value, permark_core::IpcError> {
        self.core().rpc().call(
            "comments",
            method,
            &RpcCall {
                window: "w".to_owned(),
                args,
            },
        )
    }

    fn file(&self) -> PathBuf {
        self.core().store().dir().join(FILE)
    }

    fn list(&self, scope: &str) -> Vec<Value> {
        self.call("list", json!({ "scope": scope }))
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    fn add(&self, path: &str, quote: &str, text: &str) -> Value {
        self.call(
            "add",
            json!({ "path": path, "quote": quote, "text": text, "line": 1, "endLine": 1 }),
        )
        .unwrap()
    }

    fn state_of(&self, id: &str) -> String {
        self.list("all")
            .iter()
            .find(|c| c["id"] == id)
            .map(|c| c["state"].as_str().unwrap().to_owned())
            .unwrap_or_default()
    }
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(15));
    }
}

const DOC: &str = "# Plan\n\nThe first paragraph.\n\n## Rollout\n\nWe ship on Friday after the review.\n\nLast words.\n";

#[test]
fn a_comment_is_placed_by_the_document_and_stored_in_the_local_state() {
    let mut h = Harness::new();
    h.write("docs/a.md", DOC);
    h.open();
    let view = h
        .call(
            "add",
            json!({
                "path": "docs/a.md",
                "quote": "  We ship on Friday  ",
                "text": "  Why Friday?  ",
                "headingPath": ["wrong"],
                "line": 99,
                "endLine": 99
            }),
        )
        .unwrap();
    assert_eq!(view["line"], 7, "the document decides, not the window");
    assert_eq!(view["endLine"], 7);
    assert_eq!(view["headingPath"], json!(["Plan", "Rollout"]));
    assert_eq!(view["state"], "anchored");
    assert_eq!(view["text"], "Why Friday?");
    assert_eq!(view["quote"], "We ship on Friday");
    let book: Book = serde_json::from_slice(&std::fs::read(h.file()).unwrap()).unwrap();
    assert_eq!(book.version, 1);
    assert_eq!(book.comments[0].anchor.line, 7);
    assert!(
        !book.comments[0].anchor.before.is_empty() && !book.comments[0].anchor.after.is_empty()
    );
    // never in the project's files
    assert!(!h.root().join(".permark").exists());
    assert!(h
        .sink
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|n| n == "comments:changed"));
}

#[test]
fn the_window_s_values_are_the_fallback_when_the_quote_is_not_in_the_document() {
    let mut h = Harness::new();
    h.write("a.md", DOC);
    h.open();
    let view = h
        .call(
            "add",
            json!({ "path": "a.md", "quote": "text of an embed", "text": "t", "headingPath": ["Plan"], "line": 3, "endLine": 4 }),
        )
        .unwrap();
    assert_eq!(view["state"], "passageChanged");
    assert_eq!(view["line"], 1, "the section's heading");
}

#[test]
fn editing_moves_the_passage_and_the_new_place_is_written_down() {
    let mut h = Harness::new();
    h.write("a.md", DOC);
    h.open();
    let id = h.add("a.md", "We ship on Friday", "when?")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    // lines are added above it
    h.write("a.md", &format!("new top line\n\nanother one\n\n{DOC}"));
    wait_until("the passage moved", || h.list("all")[0]["line"] == 11);
    let book: Book = serde_json::from_slice(&std::fs::read(h.file()).unwrap()).unwrap();
    wait_until("written down", || {
        let b: Book = serde_json::from_slice(&std::fs::read(h.file()).unwrap()).unwrap();
        b.comments[0].anchor.line == 11
    });
    assert_eq!(book.comments[0].id, id);
    assert_eq!(h.state_of(&id), "anchored");
    // then the sentence is rewritten: the comment stays, the section is the place
    h.write(
        "a.md",
        &format!(
            "new top line\n\nanother one\n\n{}",
            DOC.replace(
                "We ship on Friday after the review.",
                "Shipping is on hold."
            )
        ),
    );
    wait_until("the passage changed", || {
        h.state_of(&id) == "passageChanged"
    });
    assert_eq!(h.list("all")[0]["line"], 9, "the Rollout heading");
    assert!(h
        .sink
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|n| n == "comments:changed"));
    // and back: found again
    h.write("a.md", DOC);
    wait_until("found again", || h.state_of(&id) == "anchored");
}

#[test]
fn a_document_that_is_gone_keeps_its_comments_and_says_so() {
    let mut h = Harness::new();
    h.write("a.md", DOC);
    h.write("keep.md", DOC);
    h.open();
    let id = h.add("a.md", "Last words", "x")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.add("keep.md", "Last words", "y");
    std::fs::remove_file(h.root().join("a.md")).unwrap();
    wait_until("missing", || h.state_of(&id) == "documentMissing");
    assert_eq!(h.list("open").len(), 2, "nothing is deleted");
    h.write("a.md", DOC);
    wait_until("back", || h.state_of(&id) == "anchored");
}

#[test]
fn moving_a_document_or_a_folder_takes_its_comments_along() {
    let mut h = Harness::new();
    h.write("old.md", DOC);
    h.write("dir/x.md", DOC);
    h.write("dir/sub/y.md", DOC);
    h.open();
    h.add("old.md", "Last words", "1");
    h.add("dir/x.md", "Last words", "2");
    h.add("dir/sub/y.md", "Last words", "3");
    let mv = |from: &str, to: &str| {
        let ops = h.core().fileops();
        let plan = ops
            .plan_move(
                &[permark_core::fileops::MoveSpec {
                    from: from.to_owned(),
                    to: to.to_owned(),
                }],
                false,
            )
            .unwrap();
        ops.apply_move(plan.plan_id, false).unwrap();
    };
    mv("old.md", "dir/new.md");
    wait_until("the document moved", || {
        h.list("all").iter().any(|c| c["path"] == "dir/new.md")
    });
    mv("dir", "guide");
    wait_until("moved", || {
        let mut paths: Vec<String> = h
            .list("all")
            .iter()
            .map(|c| c["path"].as_str().unwrap().to_owned())
            .collect();
        paths.sort();
        paths == ["guide/new.md", "guide/sub/y.md", "guide/x.md"]
    });
    assert!(h.list("all").iter().all(|c| c["state"] == "anchored"));
}

#[test]
fn done_reopen_reply_and_delete() {
    let mut h = Harness::new();
    h.write("a.md", DOC);
    h.open();
    let id = h.add("a.md", "The first paragraph", "one")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let other = h.add("a.md", "Last words", "two")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.call("setDone", json!({ "id": id, "done": true }))
        .unwrap();
    assert_eq!(h.list("open").len(), 1);
    assert_eq!(h.list("done").len(), 1);
    assert_eq!(h.list("all").len(), 2);
    let done = &h.list("done")[0];
    assert_eq!(
        (done["done"] == true, done["doneBy"].as_str()),
        (true, Some("user"))
    );
    h.call("setDone", json!({ "id": id, "done": false }))
        .unwrap();
    assert_eq!(h.list("done").len(), 0);
    h.call("reply", json!({ "id": id, "text": "  got it  " }))
        .unwrap();
    assert_eq!(h.list("all")[0]["replies"][0]["text"], "got it");
    assert_eq!(h.list("all")[0]["replies"][0]["by"], "user");
    h.call("remove", json!({ "id": other })).unwrap();
    assert_eq!(h.list("all").len(), 1);
    for (method, args) in [
        ("setDone", json!({ "id": "nope", "done": true })),
        ("remove", json!({ "id": "nope" })),
        ("reply", json!({ "id": "nope", "text": "x" })),
    ] {
        assert_eq!(
            h.call(method, args).unwrap_err().kind,
            ErrorKind::NotFound,
            "{method}"
        );
    }
    assert_eq!(
        h.call("reply", json!({ "id": id, "text": "   " }))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
}

#[test]
fn a_change_made_outside_is_picked_up_and_not_lost() {
    let mut h = Harness::new();
    h.write("a.md", DOC);
    h.open();
    let first = h.add("a.md", "Last words", "mine")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    // `permark mcp` with the window closed resolves it: the file changes under us
    let mut book: Value = serde_json::from_slice(&std::fs::read(h.file()).unwrap()).unwrap();
    book["comments"][0]["done"] = json!({ "atMs": 5, "by": "agent", "reply": "done in 3f2a1c" });
    book["comments"][0]["pin"] = json!(true);
    std::fs::write(h.file(), serde_json::to_vec_pretty(&book).unwrap()).unwrap();
    let done = h.list("done");
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["doneBy"], "agent");
    assert_eq!(done[0]["reply"], "done in 3f2a1c");
    // our next change reads the file again first and keeps what it wrote
    h.add("a.md", "The first paragraph", "second");
    let after: Value = serde_json::from_slice(&std::fs::read(h.file()).unwrap()).unwrap();
    assert_eq!(after["comments"].as_array().unwrap().len(), 2);
    assert_eq!(after["comments"][0]["id"], first.as_str());
    assert_eq!(
        after["comments"][0]["pin"], true,
        "keys of other versions stay"
    );
    assert_eq!(after["comments"][0]["done"]["by"], "agent");
}

#[test]
fn a_file_that_is_not_comments_is_put_aside_never_overwritten() {
    for broken in [
        "{ not json",
        "[1,2]",
        r#"{ "version": 99, "comments": [] }"#,
    ] {
        let mut h = Harness::new();
        h.write("a.md", DOC);
        let store = permark_core::state::StateStore::for_project(
            &AppDirs::under(h.base.path()),
            &Root::open(&h.root()).unwrap(),
        );
        store.raw_file(FILE).write(broken.as_bytes()).unwrap();
        h.open();
        assert!(h.list("all").is_empty(), "nothing to show: {broken}");
        h.add("a.md", "Last words", "new");
        let aside: Vec<PathBuf> = std::fs::read_dir(h.file().parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("comments.unreadable-")
            })
            .collect();
        assert_eq!(aside.len(), 1, "{broken}");
        assert_eq!(std::fs::read_to_string(&aside[0]).unwrap(), broken);
        assert_eq!(h.list("all").len(), 1);
    }
}

#[test]
fn limits_and_hostile_input() {
    let mut h = Harness::new();
    h.write("a.md", DOC);
    h.write("pic.png", "not text \u{0} at all");
    h.write("worktree-like/b.md", "# B\n\nbody words\n");
    h.open();
    for (path, quote, text, kind) in [
        ("../outside.md", "x", "y", ErrorKind::PathEscape),
        ("missing.md", "x y", "y", ErrorKind::NotFound),
        ("pic.png", "x y", "y", ErrorKind::NotFound),
        ("a.md", "   ", "y", ErrorKind::InvalidData),
        ("a.md", "!!! ???", "y", ErrorKind::InvalidData),
        ("a.md", "Last words", "   ", ErrorKind::InvalidData),
    ] {
        let error = h
            .call(
                "add",
                json!({ "path": path, "quote": quote, "text": text, "line": 1 }),
            )
            .unwrap_err();
        assert_eq!(error.kind, kind, "{path} {quote:?}");
    }
    assert!(!h.file().exists(), "refused calls write nothing");
    // long quote and text are cut, not refused
    let long = "word ".repeat(2000);
    let view = h.add("a.md", &format!("Last words {long}"), &"é".repeat(5000));
    assert_eq!(view["text"].as_str().unwrap().chars().count(), 2000);
    assert_eq!(view["quote"].as_str().unwrap().chars().count(), 1000);
    // the most comments a project keeps
    let mut book: Book = serde_json::from_slice(&std::fs::read(h.file()).unwrap()).unwrap();
    let template = book.comments[0].clone();
    for n in 0..MAX_COMMENTS {
        let mut c = template.clone();
        c.id = format!("fill{n}");
        book.comments.push(c);
    }
    std::fs::write(h.file(), serde_json::to_vec_pretty(&book).unwrap()).unwrap();
    assert_eq!(
        h.call(
            "add",
            json!({ "path": "a.md", "quote": "Last words", "text": "one too many" })
        )
        .unwrap_err()
        .kind,
        ErrorKind::TooLarge
    );
}

#[test]
fn many_writers_at_once_lose_nothing() {
    let mut h = Harness::new();
    h.write("a.md", DOC);
    h.open();
    let core = h.core().clone();
    let mut threads = Vec::new();
    for t in 0..4 {
        let core = Arc::clone(&core);
        threads.push(std::thread::spawn(move || {
            for n in 0..25 {
                core.rpc()
                    .call(
                        "comments",
                        "add",
                        &RpcCall {
                            window: "w".to_owned(),
                            args: json!({ "path": "a.md", "quote": "Last words", "text": format!("{t}-{n}") }),
                        },
                    )
                    .unwrap();
            }
        }));
    }
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(h.list("all").len(), 100);
    let ids: std::collections::BTreeSet<String> = h
        .list("all")
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids.len(), 100, "ids are unique");
}

#[test]
fn off_leaves_the_file_and_the_comments_wait() {
    let mut h = Harness::new();
    h.write("a.md", DOC);
    h.open();
    h.add("a.md", "Last words", "x");
    let before = std::fs::read(h.file()).unwrap();
    let core = h.core().clone();
    h.env.set_settings(AppSettings {
        plugins: BTreeMap::from([(PluginId::of("comments"), false)]),
        ..AppSettings::default()
    });
    core.apply_plugins(&h.env);
    assert_eq!(core.rpc().served(), Vec::<String>::new());
    assert_eq!(std::fs::read(h.file()).unwrap(), before);
    h.env.set_settings(AppSettings::default());
    core.apply_plugins(&h.env);
    assert_eq!(h.list("all").len(), 1);
    drop(core);
    h.close();
}
