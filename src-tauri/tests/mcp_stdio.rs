//! `trailmark mcp` as an agent starts it: the real program with piped standard streams, no window, and
//! newline-delimited JSON-RPC on standard input and output. It must find the project from the folder it runs in,
//! answer, keep standard output free of anything else, and exit by itself when its input closes.
//!
//! The window's side of the local socket is covered by the unit tests of `bridge`, which run the same server and
//! client code over a real socket on every platform; these tests have no window, which is also a case that must work.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
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
    fn start(project: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_trailmark"))
            .arg("mcp")
            .current_dir(project)
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

/// A project with a document, a `.trailmark/` folder (which makes TrailMark know it and keeps its settings with it),
/// and the agent connection turned on or off in the saved session.
fn project(connection: Option<bool>) -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project folder");
    std::fs::create_dir_all(project.path().join("docs")).expect("docs folder");
    std::fs::write(
        project.path().join("docs/plan.md"),
        "# Plan\n\nIntro.\n\n## Rollout\n\nShip the beacon on Friday.\n",
    )
    .expect("a document");
    std::fs::create_dir_all(project.path().join(".trailmark/local")).expect("state folder");
    if let Some(on) = connection {
        std::fs::write(
            project.path().join(".trailmark/local/session.json"),
            json!({ "agentConnection": on }).to_string(),
        )
        .expect("a session");
    }
    project
}

#[test]
fn the_program_serves_the_tools_for_a_project_without_a_window_and_exits_when_its_input_closes() {
    let project = project(Some(true));
    let mut session = Session::start(project.path());

    let tools = session.request("tools/list", &json!({}));
    let mut names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .expect("a list of tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("a name").to_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["get_context", "list_unread", "open", "search"]);

    // No window is open: what needs it says so, what does not still works.
    let (context, is_error) = session.call("get_context", &json!({}));
    assert!(!is_error);
    assert_eq!(context["status"], "notOpen");
    assert_eq!(context["message"], "TrailMark is not open on this project.");

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
fn with_the_connection_off_every_tool_says_how_to_turn_it_on() {
    for connection in [None, Some(false)] {
        let project = project(connection);
        let mut session = Session::start(project.path());
        for (tool, arguments) in [
            ("get_context", json!({})),
            ("list_unread", json!({})),
            ("search", json!({ "query": "beacon" })),
            ("open", json!({ "path": "docs/plan.md" })),
        ] {
            let (answer, is_error) = session.call(tool, &arguments);
            assert!(!is_error, "{tool}: {answer}");
            assert_eq!(answer["status"], "off", "{tool}");
            let message = answer["message"].as_str().expect("a message");
            assert!(message.contains("Agent connection"), "{message}");
        }
        assert!(session.finish().success());
    }
}
