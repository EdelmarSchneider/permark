//! `trailmark mcp` as an agent starts it: the real program with piped standard streams, no window, and
//! newline-delimited JSON-RPC on standard input and output. It must answer, keep standard output free of anything
//! else, and exit by itself when its input closes.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const WAIT: Duration = Duration::from_secs(20);

struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Session {
    fn start(project: &std::path::Path) -> Self {
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
        Self {
            child,
            stdin,
            lines,
        }
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").expect("write to the server");
        self.stdin.flush().expect("flush to the server");
    }

    /// The reply to request `id`; every line the server writes must be one JSON message.
    fn reply(&self, id: u64) -> Value {
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

#[test]
fn the_program_answers_ping_over_stdio_and_exits_when_its_input_closes() {
    let project = tempfile::tempdir().expect("project folder");
    let mut session = Session::start(project.path());

    session.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "stdio-test", "version": "0.0.0" }
        }
    }));
    let init = session.reply(1);
    assert_eq!(init["result"]["serverInfo"]["name"], "trailmark");

    session.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    session.send(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }));
    let tools = session.reply(2);
    assert_eq!(tools["result"]["tools"][0]["name"], "ping");

    session.send(&json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "ping", "arguments": {} }
    }));
    let ping = session.reply(3);
    let answer = &ping["result"]["structuredContent"];
    assert_eq!(answer["ok"], true);
    assert_eq!(answer["version"], env!("CARGO_PKG_VERSION"));
    // The project is the folder the program was started in.
    let reported = std::fs::canonicalize(answer["project"].as_str().expect("a project path"))
        .expect("the project exists");
    assert_eq!(
        reported,
        std::fs::canonicalize(project.path()).expect("the project exists")
    );

    assert!(session.finish().success());
}
