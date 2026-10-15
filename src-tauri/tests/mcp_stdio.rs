//! `trailmark mcp` as an agent starts it: the real program with piped standard streams, no window, and
//! newline-delimited JSON-RPC on standard input and output. It must find the project from the folder it runs in,
//! answer, keep standard output free of anything else, and exit by itself when its input closes.
//!
//! The window's side of the local socket is covered by the unit tests of `bridge`, which run the same server and
//! client code over a real socket on every platform; these tests have no window, which is also a case that must work.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const WAIT: Duration = Duration::from_secs(30);

struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Session {
    /// Starts `trailmark mcp` in the project, with the data folder of the fixture (never the person's own).
    fn start(fixture: &Fixture) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_trailmark"))
            .arg("mcp")
            .current_dir(fixture.project.path())
            .env(
                trailmark_lib::test_support::DATA_DIR_VARIABLE,
                fixture.data.path(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start trailmark mcp");
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (sender, lines) = channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut session = Self {
            child,
            stdin,
            lines,
            next_id: 1,
        };
        let init = session.request(
            "initialize",
            &json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "stdio-test", "version": "0.0.0" }
            }),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "trailmark");
        session.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        session
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").expect("write to the server");
        self.stdin.flush().expect("flush to the server");
    }

    /// Sends a request and returns its reply; every line the server writes must be one JSON message.
    fn request(&mut self, method: &str, params: &Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(left)
                .expect("the server answered in time");
            let message: Value = serde_json::from_str(&line).unwrap_or_else(|error| {
                panic!("standard output carried a non-JSON line ({error}): {line}")
            });
            if message["id"] == id {
                return message;
            }
        }
    }

    /// The structured answer of a tool call, and whether it was an error.
    fn call(&mut self, tool: &str, arguments: &Value) -> (Value, bool) {
        let reply = self.request(
            "tools/call",
            &json!({ "name": tool, "arguments": arguments }),
        );
        let result = &reply["result"];
        (
            result["structuredContent"].clone(),
            result["isError"] == true,
        )
    }

    /// Closes the server's input and waits for it to exit by itself.
    fn finish(mut self) -> std::process::ExitStatus {
        drop(self.stdin);
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(status) = self.child.try_wait().expect("poll the server") {
                return status;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!("the server did not exit after its input closed");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A project and the data folder the program uses for it.
struct Fixture {
    project: tempfile::TempDir,
    data: tempfile::TempDir,
}

/// A project with a document and a `.trailmark/` folder (which makes TrailMark know it and keeps its settings with
/// it), and a data folder of its own in which the agent connection is turned on or off, as the app does.
fn project(connection: Option<bool>) -> Fixture {
    let project = tempfile::tempdir().expect("project folder");
    let data = tempfile::tempdir().expect("data folder");
    std::fs::create_dir_all(project.path().join("docs")).expect("docs folder");
    std::fs::write(
        project.path().join("docs/plan.md"),
        "# Plan\n\nIntro.\n\n## Rollout\n\nShip the beacon on Friday.\n",
    )
    .expect("a document");
    std::fs::create_dir_all(project.path().join(".trailmark/local")).expect("state folder");
    if let Some(on) = connection {
        trailmark_lib::test_support::set_agent_connection(data.path(), project.path(), on)
            .expect("remember the switch");
    }
    Fixture { project, data }
}

#[test]
fn the_program_serves_the_tools_for_a_project_without_a_window_and_exits_when_its_input_closes() {
    let project = project(Some(true));
    let mut session = Session::start(&project);

    let tools = session.request("tools/list", &json!({}));
    let mut names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .expect("a list of tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("a name").to_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "get_context",
            "list_comments",
            "list_notes",
            "list_unread",
            "open",
            "read_note",
            "resolve_comment",
            "search"
        ]
    );

    // No window is open: what needs it says so, what does not still works.
    let (context, is_error) = session.call("get_context", &json!({}));
    assert!(!is_error);
    assert_eq!(context["status"], "notOpen");
    assert!(context["message"]
        .as_str()
        .expect("a message")
        .starts_with("TrailMark is not open on this project."));

    let (found, is_error) = session.call("search", &json!({ "query": "beacon" }));
    assert!(!is_error, "{found}");
    assert_eq!(found["status"], "ok");
    assert_eq!(found["source"], "files");
    assert_eq!(found["hits"][0]["path"], "docs/plan.md");
    assert_eq!(found["hits"][0]["headingPath"], json!(["Plan", "Rollout"]));
    assert_eq!(found["hits"][0]["line"], 5);

    let (unread, is_error) = session.call("list_unread", &json!({}));
    assert!(!is_error, "{unread}");
    assert_eq!(unread["status"], "ok");
    assert_eq!(unread["source"], "files");
    assert_eq!(
        unread["unread"],
        json!([]),
        "a project never looked at has everything read"
    );

    // Nothing outside the project is ever shown, and nothing is started for it.
    let (refused, is_error) = session.call("open", &json!({ "path": "../elsewhere.md" }));
    assert!(is_error);
    assert_eq!(refused["status"], "error");

    assert!(session.finish().success());
}

#[test]
fn comments_are_listed_and_resolved_in_the_projects_files_without_a_window() {
    let project = project(Some(true));
    let comments = project
        .project
        .path()
        .join(".trailmark/local/comments.json");
    std::fs::write(
        &comments,
        json!({ "version": 1, "comments": [{
            "id": "c00000001",
            "path": "docs/plan.md",
            "text": "Which beacon?",
            "anchor": {
                "quote": "Ship the beacon",
                "headingPath": ["Plan", "Rollout"],
                "line": 1,
                "endLine": 1
            },
            "createdMs": 1_760_520_600_000_u64,
            "done": null
        }]})
        .to_string(),
    )
    .expect("a comment");
    let mut session = Session::start(&project);

    // the passage is found again in the document, which is not where the comment remembers it
    let (listed, is_error) = session.call("list_comments", &json!({}));
    assert!(!is_error, "{listed}");
    assert_eq!(listed["status"], "ok");
    assert_eq!(listed["source"], "files");
    assert_eq!(listed["total"], 1);
    let comment = &listed["comments"][0];
    assert_eq!(comment["id"], "c00000001");
    assert_eq!(comment["text"], "Which beacon?");
    assert_eq!(comment["state"], "anchored");
    assert_eq!(comment["line"], 7);
    assert_eq!(comment["created"], "2025-10-15T09:30:00Z");

    let (resolved, is_error) = session.call(
        "resolve_comment",
        &json!({ "id": "c00000001", "reply": "Named it" }),
    );
    assert!(!is_error, "{resolved}");
    assert_eq!(resolved["comment"]["done"], true);
    assert_eq!(resolved["comment"]["reply"], "Named it");

    let (open, _) = session.call("list_comments", &json!({}));
    assert_eq!(open["total"], 0);
    let (done, _) = session.call("list_comments", &json!({ "which": "done" }));
    assert_eq!(done["comments"][0]["doneBy"], "agent");

    let (missing, is_error) = session.call("resolve_comment", &json!({ "id": "c99999999" }));
    assert!(is_error, "{missing}");
    assert_eq!(missing["status"], "error");
    assert!(session.finish().success());

    // what the agent wrote is in the file the window reads
    let kept = std::fs::read_to_string(comments).expect("the comments file");
    assert!(kept.contains("Named it"), "{kept}");
}

/// The person's notes in the data folder of the fixture (where a debug build keeps them): two notes, one of them shared.
fn notes(fixture: &Fixture) {
    let notes = fixture.data.path().join("notes");
    std::fs::create_dir_all(notes.join(".trailmark")).expect("notes folder");
    std::fs::write(
        notes.join("deploy.md"),
        "# The deploy checklist\n\nbackups first\n",
    )
    .expect("a shared note");
    std::fs::write(notes.join("diary.md"), "# My diary\n\nprivate\n").expect("a private note");
    std::fs::write(
        notes.join(".trailmark/notes.json"),
        json!({ "shared": ["deploy.md"] }).to_string(),
    )
    .expect("notes.json");
}

#[test]
fn the_notes_the_person_shared_are_read_from_their_files_and_no_other_note_is() {
    let project = project(Some(true));
    notes(&project);
    let mut session = Session::start(&project);

    let (listed, is_error) = session.call("list_notes", &json!({}));
    assert!(!is_error, "{listed}");
    assert_eq!(listed["status"], "ok");
    assert_eq!(listed["source"], "files");
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["notes"][0]["note"], "deploy.md");
    assert_eq!(listed["notes"][0]["title"], "The deploy checklist");

    let (read, is_error) = session.call("read_note", &json!({ "note": "deploy.md" }));
    assert!(!is_error, "{read}");
    assert_eq!(read["text"], "# The deploy checklist\n\nbackups first\n");

    // a note that was not shared, and names that try to reach other files, are all the same answer
    for note in ["diary.md", "../diary.md", "notes.json", "..\\diary.md"] {
        let (refused, is_error) = session.call("read_note", &json!({ "note": note }));
        assert!(is_error, "{note}: {refused}");
        assert!(
            refused["message"]
                .as_str()
                .expect("a message")
                .contains("no shared note"),
            "{note}: {refused}"
        );
    }
    assert!(session.finish().success());
}

/// Every tool, with arguments that would do something if the connection were on.
fn every_tool() -> [(&'static str, Value); 8] {
    [
        ("get_context", json!({})),
        ("list_unread", json!({})),
        ("search", json!({ "query": "beacon" })),
        ("open", json!({ "path": "docs/plan.md" })),
        ("list_comments", json!({})),
        ("resolve_comment", json!({ "id": "c00000001" })),
        ("list_notes", json!({})),
        ("read_note", json!({ "note": "deploy.md" })),
    ]
}

#[test]
fn with_the_connection_off_every_tool_says_how_to_turn_it_on() {
    for connection in [None, Some(false)] {
        let project = project(connection);
        let mut session = Session::start(&project);
        for (tool, arguments) in every_tool() {
            let (answer, is_error) = session.call(tool, &arguments);
            assert!(!is_error, "{tool}: {answer}");
            assert_eq!(answer["status"], "off", "{tool}");
            let message = answer["message"].as_str().expect("a message");
            assert!(message.contains("Agent connection"), "{message}");
        }
        assert!(session.finish().success());
    }
}

/// A repository can carry any `.trailmark/` folder, so what is in it cannot be what turns the connection on.
#[test]
fn a_project_folder_that_says_the_connection_is_on_is_still_off() {
    let project = project(None);
    let local = project.project.path().join(".trailmark/local");
    std::fs::write(
        local.join("session.json"),
        json!({ "agentConnection": true }).to_string(),
    )
    .expect("a session");
    std::fs::write(
        local.join("agent-connection.json"),
        json!({ "enabled": true }).to_string(),
    )
    .expect("a switch");
    let mut session = Session::start(&project);
    for (tool, arguments) in every_tool() {
        let (answer, is_error) = session.call(tool, &arguments);
        assert!(!is_error, "{tool}: {answer}");
        assert_eq!(answer["status"], "off", "{tool}: {answer}");
    }
    assert!(session.finish().success());
}
