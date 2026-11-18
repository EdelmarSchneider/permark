//! The session against a fake agent over pipes, scripted after messages of a real ACP agent (Claude's adapter).

use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use permark_core::events::{CoreEvent, EventSink};
use serde_json::{json, Value};

use super::*;
use crate::agents::{platforms, Choice, Plan, CHOICE_FILE};
use crate::testing;

/// Longest wait for something a fake agent does.
const WAIT: Duration = Duration::from_secs(20);

#[derive(Default)]
struct Events {
    list: Mutex<Vec<Value>>,
    changed: Condvar,
}

impl EventSink for Events {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, payload } = event {
            assert_eq!(name, "acp:update");
            self.list.lock().unwrap().push(payload);
            self.changed.notify_all();
        }
    }
}

impl Events {
    fn all(&self) -> Vec<Value> {
        self.list.lock().unwrap().clone()
    }

    /// Waits until `test` holds for the events so far; answers them.
    fn wait_for(&self, what: &str, wait: Duration, test: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let started = Instant::now();
        let mut list = self.list.lock().unwrap();
        while !test(&list) {
            let left = wait.saturating_sub(started.elapsed());
            assert!(!left.is_zero(), "no {what}; events: {list:#?}");
            list = self.changed.wait_timeout(list, left).unwrap().0;
        }
        list.clone()
    }

    fn of_type(&self, kind: &str) -> Vec<Value> {
        self.all()
            .into_iter()
            .filter(|e| e["type"] == kind)
            .collect()
    }

    /// The last turn's end, once the status that follows it came too.
    fn turn_end(&self) -> Value {
        let list = self.wait_for("turn end", WAIT, |list| {
            list.iter()
                .rposition(|e| e["type"] == "turnEnd")
                .is_some_and(|at| list[at..].iter().any(|e| e["type"] == "status"))
        });
        list.into_iter().rfind(|e| e["type"] == "turnEnd").unwrap()
    }
}

fn wait_until(what: &str, test: impl Fn() -> bool) {
    let started = Instant::now();
    while !test() {
        assert!(started.elapsed() < WAIT, "no {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The fake agent's side of the pipes.
struct Peer {
    from_client: BufReader<PipeReader>,
    to_client: PipeWriter,
    stderr: PipeWriter,
    /// Every message the client sent, in order.
    seen: Arc<Mutex<Vec<Value>>>,
    /// The session's folder, from `session/new`.
    cwd: PathBuf,
    auth: Arc<Auth>,
}

/// What the fake agent asks of sign-ins.
#[derive(Default)]
struct Auth {
    /// `authMethods` of its `initialize` answer.
    methods: Mutex<Vec<Value>>,
    /// `session/new` is refused with `auth_required` until an `authenticate`.
    required: AtomicBool,
}

const SESSION: &str = "44453ef7-44fa-476b-a0cb-7768c9987d6c";

impl Peer {
    fn send(&mut self, message: &Value) {
        let mut line = serde_json::to_vec(message).unwrap();
        line.push(b'\n');
        self.to_client.write_all(&line).unwrap();
    }

    fn raw(&mut self, line: &str) {
        self.to_client.write_all(line.as_bytes()).unwrap();
        self.to_client.write_all(b"\n").unwrap();
    }

    /// The client's next message (`None`: its stdin closed).
    fn next(&mut self) -> Option<Value> {
        let mut line = String::new();
        if self.from_client.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let message: Value = serde_json::from_str(&line).unwrap();
        self.seen.lock().unwrap().push(message.clone());
        Some(message)
    }

    fn update(&mut self, update: &Value) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": { "sessionId": SESSION, "update": update },
        }));
    }

    fn text(&mut self, text: &str) {
        self.update(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": text },
            "messageId": "msg_011Ch9NdR9bmdm64DafX2LtF",
        }));
    }

    /// Asks permission for a tool call with Claude's three options.
    fn ask(&mut self, id: &Value, tool_call: &Value) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/request_permission",
            "params": {
                "sessionId": SESSION,
                "toolCall": tool_call,
                "options": [
                    { "optionId": "allow-once", "name": "Yes", "kind": "allow_once" },
                    { "optionId": "allow-with-updates", "name": "Yes, allow all edits during this session", "kind": "allow_always" },
                    { "optionId": "reject", "name": "No", "kind": "reject_once" },
                ],
            },
        }));
    }

    fn in_copy(&self, path: &str) -> String {
        self.cwd.join(path).display().to_string()
    }
}

/// What the fake does with one prompt: `Some(stop reason)` answers it; `None` exits without answering.
type Turn = Arc<dyn Fn(&mut Peer, &Value) -> Option<&'static str> + Send + Sync>;

fn agent_loop(mut peer: Peer, turn: &Turn) {
    while let Some(message) = peer.next() {
        let Some(method) = message["method"].as_str().map(str::to_owned) else {
            continue;
        };
        let id = message["id"].clone();
        if method == "session/new" && peer.auth.required.load(Ordering::SeqCst) {
            peer.send(&json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32000, "message": "Authentication required" },
            }));
            continue;
        }
        let result = match method.as_str() {
            // answers as Claude's adapter 0.108.0 did
            "initialize" => json!({
                "protocolVersion": 1,
                "agentCapabilities": {
                    "promptCapabilities": { "image": true, "embeddedContext": true },
                    "mcpCapabilities": { "http": true, "sse": true },
                    "loadSession": true,
                },
                "agentInfo": { "name": "@agentclientprotocol/claude-agent-acp", "title": "Claude Agent", "version": "0.108.0" },
                "authMethods": peer.auth.methods.lock().unwrap().clone(),
            }),
            "authenticate" => {
                peer.auth.required.store(false, Ordering::SeqCst);
                json!({})
            }
            "session/new" => {
                peer.cwd = PathBuf::from(message["params"]["cwd"].as_str().unwrap());
                json!({
                    "sessionId": SESSION,
                    "modes": {
                        "currentModeId": "auto",
                        "availableModes": [
                            { "id": "default", "name": "Manual", "description": "Always ask before making changes" },
                            { "id": "acceptEdits", "name": "Accept edits" },
                            { "id": "auto", "name": "Auto" },
                        ],
                    },
                })
            }
            "session/set_mode" => json!({}),
            "session/prompt" => match turn(&mut peer, &message) {
                Some(reason) => json!({ "stopReason": reason }),
                None => return,
            },
            _ => continue,
        };
        peer.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }
}

/// What the test sees of the fake agents it started.
#[derive(Clone, Default)]
struct Fake {
    seen: Arc<Mutex<Vec<Value>>>,
    launches: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
    /// What each start was asked to run.
    plans: Arc<Mutex<Vec<Plan>>>,
    auth: Arc<Auth>,
}

impl Fake {
    fn seen(&self) -> Vec<Value> {
        self.seen.lock().unwrap().clone()
    }

    fn plans(&self) -> Vec<Plan> {
        self.plans.lock().unwrap().clone()
    }

    fn methods(&self) -> Vec<String> {
        self.seen()
            .iter()
            .filter_map(|m| m["method"].as_str().map(str::to_owned))
            .collect()
    }

    fn reply_to(&self, id: &Value) -> Value {
        self.seen()
            .into_iter()
            .find(|m| m["id"] == *id && m.get("method").is_none())
            .unwrap_or_else(|| panic!("no reply to {id}: {:#?}", self.seen()))
    }

    fn launch(&self, turn: Turn) -> Launch {
        let fake = self.clone();
        Arc::new(move |plan: &Plan, cwd: &Path| {
            fake.launches.fetch_add(1, Ordering::SeqCst);
            fake.plans.lock().unwrap().push(plan.clone());
            let (agent_in, client_out) = std::io::pipe()?;
            let (client_in, agent_out) = std::io::pipe()?;
            let (client_err, agent_err) = std::io::pipe()?;
            let peer = Peer {
                from_client: BufReader::new(agent_in),
                to_client: agent_out,
                stderr: agent_err,
                seen: Arc::clone(&fake.seen),
                cwd: cwd.to_path_buf(),
                auth: Arc::clone(&fake.auth),
            };
            let turn = Arc::clone(&turn);
            let agent = std::thread::spawn(move || agent_loop(peer, &turn));
            let stops = Arc::clone(&fake.stops);
            Ok(AgentPipes {
                to_agent: Box::new(client_out),
                from_agent: Box::new(client_in),
                stderr: Box::new(client_err),
                stop: Box::new(move || {
                    // its stdin is closed by now: the fake's loop ends
                    let _ = agent.join();
                    stops.fetch_add(1, Ordering::SeqCst);
                }),
            })
        })
    }
}

struct Fx {
    project: tempfile::TempDir,
    profile: tempfile::TempDir,
    /// The only folder of the fake `PATH` (holds `npx`).
    bin: tempfile::TempDir,
    events: Arc<Events>,
    copy: SharedCopy,
    session: Session,
    fake: Fake,
}

const NOTES: &str = "Line one of the notes.\nLine two of the notes.\nLine three of the notes.\n";
/// How the chosen Claude agent starts with `npx` on the fake `PATH`.
const CLAUDE: &str = "npx -y @agentclientprotocol/claude-agent-acp@0.108.0";

type TurnFn = dyn Fn(&mut Peer, &Value) -> Option<&'static str> + Send + Sync;

impl Fx {
    /// Claude's agent chosen (through `npx`).
    fn new(
        turn: impl Fn(&mut Peer, &Value) -> Option<&'static str> + Send + Sync + 'static,
    ) -> Self {
        Self::choosing(
            Some(&Choice::Agent("claude-acp".to_owned())),
            Arc::new(turn),
        )
    }

    fn choosing(choice: Option<&Choice>, turn: Arc<TurnFn>) -> Self {
        let fake = Fake::default();
        let launch = fake.launch(turn);
        Self::with(launch, fake, choice, false)
    }

    /// `real_path`: programs found on this process's own `PATH`.
    fn with(launch: Launch, fake: Fake, choice: Option<&Choice>, real_path: bool) -> Self {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("notes.md"), NOTES).unwrap();
        let profile = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        testing::program(bin.path(), "npx");
        let file = testing::app_store(profile.path()).app_file(CHOICE_FILE);
        if let Some(choice) = choice {
            file.save(choice).unwrap();
        }
        let agents = if real_path {
            Agents::new(Some(file))
        } else {
            Agents::with(Some(file), testing::find_in(bin.path()), platforms())
        };
        let (vault, documents) = testing::open(project.path());
        let events = Arc::new(Events::default());
        let copy: SharedCopy = Arc::new(Mutex::new(None));
        let session = Session::new(Host {
            agents,
            launch,
            project: testing::project(&vault, &documents),
            place: Some(profile.path().join("work")),
            copy: Arc::clone(&copy),
            events: testing::events(events.clone()),
        });
        Self {
            project,
            profile,
            bin,
            events,
            copy,
            session,
            fake,
        }
    }

    fn ask(&self, text: &str) {
        self.session
            .prompt(PromptArgs {
                text: text.to_owned(),
                selection: None,
                document: None,
            })
            .unwrap();
    }

    fn copy_dir(&self) -> PathBuf {
        self.copy
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .dir()
            .to_path_buf()
    }

    fn permission(&self) -> Value {
        let list = self.events.wait_for("permission card", WAIT, |list| {
            list.iter().any(|e| e["type"] == "permission")
        });
        list.into_iter()
            .find(|e| e["type"] == "permission")
            .unwrap()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        self.session.shutdown();
    }
}

fn edit_of(path: &str) -> Value {
    json!({
        "toolCallId": "toolu_018EiDtwiFoUJHFFTXK1xgJ7",
        "name": "Edit",
        "status": "pending",
        "rawInput": { "file_path": path, "old_string": "Line two of the notes.", "new_string": "Line two, edited.", "replace_all": false },
        "title": "Edit notes.md",
        "kind": "edit",
        "content": [{ "type": "diff", "path": path, "oldText": "Line two of the notes.", "newText": "Line two, edited." }],
        "locations": [{ "path": path }],
    })
}

#[test]
fn the_first_prompt_starts_the_agent_in_the_copy_and_streams_the_reply() {
    let fx = Fx::new(|peer, _| {
        peer.text("Hel");
        peer.text("lo");
        Some("end_turn")
    });
    assert_eq!(fx.session.status().state, AcpState::Idle);
    fx.session
        .prompt(PromptArgs {
            text: "Shorten this.".to_owned(),
            selection: Some(PromptSelection {
                path: "notes.md".to_owned(),
                from_line: 2,
                to_line: 2,
                text: "Line two of the notes.".to_owned(),
            }),
            document: Some("notes.md".to_owned()),
        })
        .unwrap();
    let end = fx.events.turn_end();
    assert_eq!(
        end,
        json!({ "type": "turnEnd", "stopReason": "end_turn", "changed": 0 })
    );
    let events = fx.events.wait_for("ready", WAIT, |list| {
        list.last()
            .is_some_and(|e| e["type"] == "status" && e["state"] == "ready")
    });
    let kinds: Vec<Value> = events
        .iter()
        .map(|e| match e["type"].as_str() {
            Some("status") => json!(["status", e["state"]]),
            _ => json!([e["type"], e.get("text").cloned().unwrap_or(Value::Null)]),
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            json!(["status", "starting"]),
            json!(["status", "working"]),
            json!(["text", "Hel"]),
            json!(["text", "lo"]),
            json!(["turnEnd", null]),
            json!(["status", "ready"]),
        ]
    );
    assert_eq!(fx.session.status().state, AcpState::Ready);
    assert_eq!(fx.session.status().agent, CLAUDE);

    assert_eq!(
        fx.fake.methods(),
        vec![
            "initialize",
            "session/new",
            "session/set_mode",
            "session/prompt"
        ]
    );
    let seen = fx.fake.seen();
    let init = &seen[0]["params"];
    assert_eq!(init["protocolVersion"], 1);
    assert_eq!(init["clientInfo"]["name"], "PerMark");
    // no file system, no terminal offered
    let caps = &init["clientCapabilities"];
    assert_ne!(caps["terminal"], true);
    assert_ne!(caps["fs"]["readTextFile"], true);
    assert_ne!(caps["fs"]["writeTextFile"], true);
    let new = &seen[1]["params"];
    let dir = fx.copy_dir();
    assert_eq!(PathBuf::from(new["cwd"].as_str().unwrap()), dir);
    assert!(!dir.starts_with(fx.project.path()));
    assert_eq!(new["mcpServers"], json!([]));
    assert_eq!(
        seen[2]["params"],
        json!({ "sessionId": SESSION, "modeId": "default" })
    );

    // the passage names its file in the copy + its lines; the open document is linked
    let prompt = &seen[3]["params"]["prompt"];
    let file = url::Url::from_file_path(dir.join("notes.md")).unwrap();
    assert_eq!(
        prompt[0],
        json!({ "type": "text", "text": "Shorten this." })
    );
    assert_eq!(
        prompt[1],
        json!({
            "type": "resource",
            "resource": {
                "uri": format!("{file}#L2:2"),
                "mimeType": "text/markdown",
                "text": "Line two of the notes.",
            },
        })
    );
    assert_eq!(prompt[2]["type"], "resource_link");
    assert_eq!(prompt[2]["uri"], file.to_string());
    assert_eq!(prompt[2]["name"], "notes.md");
}

#[test]
fn an_edit_inside_the_copy_is_allowed_once_without_a_card() {
    let fx = Fx::new(|peer, _| {
        let path = peer.in_copy("notes.md");
        peer.update(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "toolu_018EiDtwiFoUJHFFTXK1xgJ7",
            "name": "Edit", "title": "Edit", "kind": "edit", "status": "pending",
            "rawInput": {}, "content": [], "locations": [],
        }));
        // request id 0, as the real agent sends it
        peer.ask(&json!(0), &edit_of(&path));
        peer.next()?;
        std::fs::write(
            &path,
            NOTES.replace("Line two of the notes.", "Line two, edited."),
        )
        .unwrap();
        peer.update(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "toolu_018EiDtwiFoUJHFFTXK1xgJ7",
            "title": "Edit notes.md",
        }));
        peer.update(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "toolu_018EiDtwiFoUJHFFTXK1xgJ7",
            "rawInput": { "file_path": path },
        }));
        peer.update(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "toolu_018EiDtwiFoUJHFFTXK1xgJ7",
            "status": "completed",
        }));
        Some("end_turn")
    });
    fx.ask("Edit line two.");
    assert_eq!(fx.events.turn_end()["changed"], 1);
    assert_eq!(
        fx.fake.reply_to(&json!(0))["result"],
        json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } })
    );
    assert_eq!(fx.events.of_type("permission"), Vec::<Value>::new());
    // one line per visible change: created, titled, completed (the input stream shows nothing)
    let tools: Vec<(Value, Value)> = fx
        .events
        .of_type("tool")
        .iter()
        .map(|e| (e["title"].clone(), e["status"].clone()))
        .collect();
    assert_eq!(
        tools,
        vec![
            (json!("Edit"), json!("pending")),
            (json!("Edit notes.md"), json!("pending")),
            (json!("Edit notes.md"), json!("completed")),
        ]
    );
    assert_eq!(fx.events.of_type("tool")[0]["kind"], "edit");
    // the real file is untouched
    assert_eq!(
        std::fs::read_to_string(fx.project.path().join("notes.md")).unwrap(),
        NOTES
    );
}

#[test]
fn a_command_gets_a_card_and_the_persons_answer_reaches_the_agent() {
    let fx = Fx::new(|peer, _| {
        peer.ask(
            &json!(7),
            &json!({
                "toolCallId": "toolu_bash", "name": "Bash", "status": "pending",
                "title": "Run the tests", "kind": "execute",
                "rawInput": { "command": "npm test", "description": "Run the tests" },
            }),
        );
        peer.next()?;
        peer.text("Tests pass.");
        Some("end_turn")
    });
    fx.ask("Run the tests.");
    let card = fx.permission();
    assert_eq!(
        card,
        json!({
            "type": "permission",
            "requestId": "7",
            "title": "Run the tests",
            "kind": "execute",
            "detail": "npm test",
            "options": [
                { "id": "allow-once", "name": "Yes", "kind": "allow_once" },
                { "id": "allow-with-updates", "name": "Yes, allow all edits during this session", "kind": "allow_always" },
                { "id": "reject", "name": "No", "kind": "reject_once" },
            ],
        })
    );
    // a second prompt waits for this one
    let busy = fx.session.prompt(PromptArgs {
        text: "And this?".to_owned(),
        selection: None,
        document: None,
    });
    assert_eq!(busy.unwrap_err().kind, ErrorKind::Conflict);
    let wrong = fx.session.answer(&AnswerArgs {
        request_id: "7".to_owned(),
        option_id: Some("maybe".to_owned()),
    });
    assert_eq!(wrong.unwrap_err().kind, ErrorKind::InvalidData);
    fx.session
        .answer(&AnswerArgs {
            request_id: "7".to_owned(),
            option_id: Some("allow-once".to_owned()),
        })
        .unwrap();
    assert_eq!(fx.events.turn_end()["stopReason"], "end_turn");
    assert_eq!(
        fx.fake.reply_to(&json!(7))["result"],
        json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } })
    );
    assert_eq!(
        fx.events.of_type("permissionDone"),
        vec![json!({ "type": "permissionDone", "requestId": "7" })]
    );
    let again = fx.session.answer(&AnswerArgs {
        request_id: "7".to_owned(),
        option_id: None,
    });
    assert_eq!(again.unwrap_err().kind, ErrorKind::NotFound);
}

#[test]
fn an_edit_outside_the_copy_gets_a_card() {
    let project = tempfile::tempdir().unwrap();
    let outside = project.path().join("notes.md").display().to_string();
    let fx = Fx::new(move |peer, _| {
        // a string request id, and paths: one outside, one inside
        let inside = peer.in_copy("notes.md");
        let mut call = edit_of(&outside);
        call["locations"] = json!([{ "path": inside }, { "path": outside }]);
        peer.ask(&json!("ask-1"), &call);
        peer.next()?;
        Some("end_turn")
    });
    fx.ask("Edit the real notes.");
    let card = fx.permission();
    assert_eq!(card["requestId"], "\"ask-1\"");
    assert_eq!(card["kind"], "edit");
    assert_eq!(
        card["detail"],
        format!("notes.md\n{}", project.path().join("notes.md").display())
    );
    // dismissed: the agent hears "cancelled"
    fx.session
        .answer(&AnswerArgs {
            request_id: "\"ask-1\"".to_owned(),
            option_id: None,
        })
        .unwrap();
    fx.events.turn_end();
    assert_eq!(
        fx.fake.reply_to(&json!("ask-1"))["result"],
        json!({ "outcome": { "outcome": "cancelled" } })
    );
}

#[test]
fn only_paths_inside_the_copy_count_as_inside() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "a").unwrap();
    let copy = Vault::new(Root::open(dir.path()).unwrap());
    let root = copy.main().path().to_path_buf();
    let other = tempfile::tempdir().unwrap();
    assert!(inside(&copy, &root));
    assert!(inside(&copy, &root.join("a.md")));
    assert!(inside(&copy, &root.join("new").join("b.md")));
    assert!(!inside(&copy, &root.join("..").join("x.md")));
    assert!(!inside(
        &copy,
        &root.join("sub").join("..").join("..").join("x.md")
    ));
    assert!(!inside(&copy, Path::new("a.md")));
    assert!(!inside(&copy, other.path()));
    if cfg!(windows) {
        let upper = PathBuf::from(root.display().to_string().to_uppercase());
        assert!(inside(&copy, &upper.join("A.MD")));
    }
}

#[test]
fn stop_tells_the_agent_and_drops_the_waiting_question() {
    let fx = Fx::new(|peer, _| {
        peer.ask(
            &json!(3),
            &json!({
                "toolCallId": "toolu_fetch", "status": "pending",
                "title": "Fetch https://example.com", "kind": "fetch",
            }),
        );
        // the cancel notification, then the answer to the question
        let mut heard = (false, false);
        while heard != (true, true) {
            let message = peer.next()?;
            if message["method"] == "session/cancel" {
                heard.0 = true;
            }
            if message["id"] == 3 {
                heard.1 = true;
            }
        }
        Some("cancelled")
    });
    fx.ask("Look this up.");
    let card = fx.permission();
    assert_eq!(card["detail"], "");
    fx.session.cancel();
    assert_eq!(fx.events.turn_end()["stopReason"], "cancelled");
    assert_eq!(
        fx.events.of_type("permissionDone"),
        vec![json!({ "type": "permissionDone", "requestId": "3" })]
    );
    let cancel = fx
        .fake
        .seen()
        .into_iter()
        .find(|m| m["method"] == "session/cancel")
        .unwrap();
    assert_eq!(cancel["params"], json!({ "sessionId": SESSION }));
    assert!(cancel.get("id").is_none());
    assert_eq!(
        fx.fake.reply_to(&json!(3))["result"],
        json!({ "outcome": { "outcome": "cancelled" } })
    );
    assert_eq!(fx.session.status().state, AcpState::Ready);
}

#[test]
fn unknown_news_and_account_status_never_reach_the_window() {
    let fx = Fx::new(|peer, _| {
        let email = "person@example.com";
        peer.send(&json!({
            "jsonrpc": "2.0", "method": "_auth/status_update",
            "params": { "authStatus": { "kind": "account", "label": "Plan", "account": { "plan": "max", "email": email } } },
        }));
        peer.send(&json!({
            "jsonrpc": "2.0", "method": "auth/status_update",
            "params": { "state": "authenticated", "accounts": [{ "label": "Plan", "active": true, "email": email }] },
        }));
        peer.update(&json!({ "sessionUpdate": "future_kind", "who": email }));
        peer.update(&json!({
            "sessionUpdate": "available_commands_update",
            "availableCommands": [{ "name": "design", "description": "Design", "input": null }],
        }));
        peer.update(&json!({ "sessionUpdate": "usage_update", "used": 23356, "size": 1_000_000 }));
        peer.update(&json!({ "sessionUpdate": "session_info_update", "title": "Notes", "updatedAt": "2026-11-17T23:32:36.340Z" }));
        peer.update(&json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "hm" } }));
        peer.update(&json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "hm" } }));
        peer.raw("not json at all");
        peer.raw("{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":{}}");
        // an agent request the client does not serve
        peer.send(&json!({
            "jsonrpc": "2.0", "id": 41, "method": "fs/read_text_file",
            "params": { "sessionId": SESSION, "path": "/etc/hosts" },
        }));
        peer.next()?;
        peer.text("ok");
        Some("end_turn")
    });
    fx.ask("Say ok.");
    fx.events.turn_end();
    let events = fx.events.all();
    assert!(!serde_json::to_string(&events)
        .unwrap()
        .contains("example.com"));
    let types: Vec<&str> = events.iter().filter_map(|e| e["type"].as_str()).collect();
    assert_eq!(
        types,
        vec!["status", "status", "thinking", "text", "turnEnd", "status"]
    );
    assert_eq!(fx.events.of_type("text")[0]["text"], "ok");
    assert_eq!(fx.fake.reply_to(&json!(41))["error"]["code"], -32601);
}

#[test]
fn an_agent_that_exits_mid_turn_reports_its_last_words() {
    let fx = Fx::new(|peer, _| {
        peer.stderr
            .write_all(b"[session/create] phase=validate-cwd\nfatal: the model is gone\n")
            .unwrap();
        None
    });
    fx.ask("Hello?");
    let list = fx.events.wait_for("error status", WAIT, |list| {
        list.iter()
            .any(|e| e["type"] == "status" && e["state"] == "error")
    });
    let error = list
        .iter()
        .find(|e| e["state"] == "error")
        .and_then(|e| e["error"].as_str())
        .unwrap()
        .to_owned();
    assert!(error.contains("fatal: the model is gone"), "{error}");
    let status = fx.session.status();
    assert_eq!(status.state, AcpState::Error);
    assert_eq!(status.error.as_deref(), Some(error.as_str()));
    wait_until("agent stopped", || {
        fx.fake.stops.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn new_conversation_stops_the_agent_and_the_next_prompt_starts_a_fresh_one() {
    let fx = Fx::new(|peer, _| {
        peer.text("ok");
        Some("end_turn")
    });
    fx.ask("One.");
    fx.events.turn_end();
    let dir = fx.copy_dir();
    fx.session.reset();
    assert_eq!(fx.session.status().state, AcpState::Idle);
    wait_until("agent stopped", || {
        fx.fake.stops.load(Ordering::SeqCst) == 1
    });
    fx.ask("Two.");
    fx.events.wait_for("second turn", WAIT, |list| {
        list.iter().filter(|e| e["type"] == "turnEnd").count() == 2
    });
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 2);
    assert_eq!(fx.copy_dir(), dir);
    assert_eq!(
        fx.fake
            .methods()
            .iter()
            .filter(|m| *m == "initialize")
            .count(),
        2
    );
}

#[test]
fn shutdown_stops_the_agent_before_answering() {
    let fx = Fx::new(|peer, _| {
        peer.text("ok");
        Some("end_turn")
    });
    fx.ask("One.");
    fx.events.turn_end();
    fx.session.shutdown();
    assert_eq!(fx.fake.stops.load(Ordering::SeqCst), 1);
    assert_eq!(fx.session.status().state, AcpState::Idle);
}

#[test]
fn prompts_that_cannot_be_sent_are_refused_before_anything_starts() {
    let fx = Fx::new(|_, _| Some("end_turn"));
    let refused = |args: PromptArgs| fx.session.prompt(args).unwrap_err().kind;
    let passage = |path: &str, from_line: u32, to_line: u32| PromptArgs {
        text: "Fix.".to_owned(),
        selection: Some(PromptSelection {
            path: path.to_owned(),
            from_line,
            to_line,
            text: "x".to_owned(),
        }),
        document: None,
    };
    assert_eq!(
        refused(PromptArgs {
            text: "  ".to_owned(),
            selection: None,
            document: None,
        }),
        ErrorKind::InvalidData
    );
    assert!(matches!(
        refused(passage("../outside.md", 1, 1)),
        ErrorKind::InvalidPath | ErrorKind::PathEscape
    ));
    assert_eq!(refused(passage("notes.md", 0, 1)), ErrorKind::InvalidData);
    assert_eq!(refused(passage("notes.md", 3, 2)), ErrorKind::InvalidData);
    assert_eq!(fx.session.status().state, AcpState::Idle);
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 0);
}

/// The real Claude agent over the real program: by hand only (first run downloads the adapter; needs Node.js and the
/// person's own agent login).
#[test]
#[ignore = "starts the real Claude agent; run by hand"]
fn the_real_claude_agent_answers() {
    let fx = Fx::with(
        crate::agents::person_launch(),
        Fake::default(),
        Some(&Choice::Agent("claude-acp".to_owned())),
        true,
    );
    fx.ask("Reply with one word: ok");
    let list = fx
        .events
        .wait_for("turn end", Duration::from_secs(300), |list| {
            list.iter()
                .any(|e| e["type"] == "turnEnd" || e["state"] == "error")
        });
    let text: String = list
        .iter()
        .filter_map(|e| (e["type"] == "text").then(|| e["text"].as_str()).flatten())
        .collect();
    eprintln!("events: {list:#?}");
    assert!(
        list.iter().any(|e| e["type"] == "turnEnd"),
        "{:?}",
        fx.session.status()
    );
    assert!(text.to_lowercase().contains("ok"), "{text}");
}

fn choose_agent(id: &str) -> ChooseArgs {
    ChooseArgs {
        agent: Some(id.to_owned()),
        command: None,
    }
}

fn turns_ended(fx: &Fx, count: usize) {
    fx.events.wait_for("turn ends", WAIT, |list| {
        list.iter().filter(|e| e["type"] == "turnEnd").count() == count
    });
}

#[test]
fn without_a_choice_the_prompt_is_refused_until_the_person_chooses() {
    let fx = Fx::choosing(
        None,
        Arc::new(|peer: &mut Peer, _: &Value| {
            peer.text("ok");
            Some("end_turn")
        }),
    );
    let status = fx.session.status();
    assert!(!status.chosen);
    assert_eq!(status.agent_name, None);
    assert_eq!(status.agent, "");
    let refused = fx
        .session
        .prompt(PromptArgs {
            text: "Hello.".to_owned(),
            selection: None,
            document: None,
        })
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Unavailable);
    assert_eq!(refused.detail.as_deref(), Some("noAgent"));
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 0);
    assert!(fx.session.agents().iter().all(|entry| !entry.chosen));

    let list = fx.session.choose(&choose_agent("codex-acp")).unwrap();
    let chosen: Vec<&str> = list
        .iter()
        .filter(|entry| entry.chosen)
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(chosen, ["codex-acp"]);
    // the profile folder, never the project
    assert!(fx.profile.path().join(CHOICE_FILE).is_file());
    assert!(!fx.project.path().join(CHOICE_FILE).exists());
    let status = fx.session.status();
    assert!(status.chosen);
    assert_eq!(status.agent_name.as_deref(), Some("Codex"));
    assert_eq!(status.agent, "npx -y @agentclientprotocol/codex-acp@2.7.0");
    fx.ask("Hello.");
    fx.events.turn_end();
    assert_eq!(
        fx.fake.plans()[0].args,
        ["-y", "@agentclientprotocol/codex-acp@2.7.0"]
    );
}

/// A new choice stops the agent (the copy stays) and the next prompt starts the new one; an own command runs its
/// program as named.
#[test]
fn a_new_choice_applies_at_the_next_start() {
    let fx = Fx::new(|peer, _| {
        peer.text("ok");
        Some("end_turn")
    });
    fx.ask("One.");
    fx.events.turn_end();
    let status = fx.session.status();
    assert_eq!(status.agent_name.as_deref(), Some("Claude Agent"));
    assert!(status.chosen);
    assert_eq!(status.methods, Vec::<LoginMethod>::new());
    let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
    let first = &fx.fake.plans()[0];
    assert_eq!(
        first.program.to_string_lossy().to_lowercase(),
        fx.bin.path().join(npx).to_string_lossy().to_lowercase()
    );
    assert_eq!(first.shown, CLAUDE);
    let dir = fx.copy_dir();
    fx.session.choose(&choose_agent("gemini")).unwrap();
    assert_eq!(fx.session.status().state, AcpState::Idle);
    wait_until("agent stopped", || {
        fx.fake.stops.load(Ordering::SeqCst) == 1
    });
    fx.ask("Two.");
    turns_ended(&fx, 2);
    let plans = fx.fake.plans();
    assert_eq!(
        plans[0].args,
        ["-y", "@agentclientprotocol/claude-agent-acp@0.108.0"]
    );
    assert_eq!(plans[1].args, ["-y", "@google/gemini-cli@0.68.0", "--acp"]);
    assert_eq!(fx.copy_dir(), dir);

    let own = testing::program(fx.bin.path(), "my-agent");
    fx.session
        .choose(&ChooseArgs {
            agent: None,
            command: Some(format!("{} --acp", own.display())),
        })
        .unwrap();
    let status = fx.session.status();
    assert_eq!(
        status.agent_name.as_deref(),
        own.file_name().and_then(|n| n.to_str())
    );
    fx.ask("Three.");
    turns_ended(&fx, 3);
    let plan = &fx.fake.plans()[2];
    assert_eq!(plan.program, own);
    assert_eq!(plan.args, ["--acp"]);
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 3);
}

#[test]
fn choosing_is_refused_while_the_agent_works() {
    let fx = Fx::new(|peer, _| {
        peer.ask(
            &json!(5),
            &json!({
                "toolCallId": "toolu_bash", "status": "pending",
                "title": "Run the tests", "kind": "execute",
                "rawInput": { "command": "npm test" },
            }),
        );
        peer.next()?;
        Some("end_turn")
    });
    fx.ask("Run the tests.");
    fx.permission();
    let busy = fx.session.choose(&choose_agent("gemini")).unwrap_err();
    assert_eq!(busy.kind, ErrorKind::Conflict);
    assert_eq!(busy.detail.as_deref(), Some("busy"));
    let saved = std::fs::read_to_string(fx.profile.path().join(CHOICE_FILE)).unwrap();
    assert!(saved.contains("claude-acp"), "{saved}");
    fx.session
        .answer(&AnswerArgs {
            request_id: "5".to_owned(),
            option_id: Some("reject".to_owned()),
        })
        .unwrap();
    fx.events.turn_end();
    assert_eq!(fx.fake.stops.load(Ordering::SeqCst), 0);
}

/// `auth_required` at `session/new`: the person sees the agent's ways to sign in; a terminal way only tries again,
/// an agent way has the agent sign in, then the waiting prompt goes on.
#[test]
fn an_agent_that_needs_a_sign_in_asks_for_it_and_the_prompt_goes_on_after() {
    let fx = Fx::new(|peer, _| {
        peer.text("Signed in.");
        Some("end_turn")
    });
    *fx.fake.auth.methods.lock().unwrap() = vec![
        json!({ "id": "oauth", "name": "Sign in with the browser", "description": "Opens your browser" }),
        json!({ "type": "terminal", "id": "claude-ai-login", "name": "Claude account", "args": ["--cli", "auth", "login", "--claudeai"] }),
        json!({ "type": "env_var", "id": "key", "name": "API key", "vars": [{ "name": "ANTHROPIC_API_KEY" }] }),
        json!({ "type": "future_kind", "id": "later", "name": "Later" }),
    ];
    fx.fake.auth.required.store(true, Ordering::SeqCst);
    fx.ask("Hello.");
    let list = fx.events.wait_for("sign-in", WAIT, |list| {
        list.iter().any(|e| e["state"] == "needsLogin")
    });
    let asked = list.iter().find(|e| e["state"] == "needsLogin").unwrap();
    assert_eq!(asked["error"], Value::Null);
    assert_eq!(
        asked["methods"],
        json!([
            { "id": "oauth", "name": "Sign in with the browser", "description": "Opens your browser", "kind": "agent", "hint": null },
            { "id": "claude-ai-login", "name": "Claude account", "description": null, "kind": "terminal", "hint": format!("{CLAUDE} --cli auth login --claudeai") },
            { "id": "key", "name": "API key", "description": null, "kind": "env", "hint": "ANTHROPIC_API_KEY" },
        ])
    );
    let status = fx.session.status();
    assert_eq!(status.state, AcpState::NeedsLogin);
    assert_eq!(status.methods.len(), 3);
    // terminal sign-in methods are welcome (the person's own terminal); still no terminal of the app's
    let caps = &fx.fake.seen()[0]["params"]["clientCapabilities"];
    assert_eq!(caps["auth"]["terminal"], true);
    assert_ne!(caps["terminal"], true);
    // other states carry no methods
    assert!(list
        .iter()
        .filter(|e| e["state"] != "needsLogin")
        .all(|e| e.get("methods").is_none()));

    let unknown = fx
        .session
        .login(&LoginArgs {
            method_id: Some("nope".to_owned()),
        })
        .unwrap_err();
    assert_eq!(unknown.detail.as_deref(), Some("unknownMethod"));
    assert_eq!(fx.session.status().state, AcpState::NeedsLogin);

    // a terminal way: the person signs in outside; only tried again (still refused here)
    fx.session
        .login(&LoginArgs {
            method_id: Some("claude-ai-login".to_owned()),
        })
        .unwrap();
    let list = fx.events.wait_for("sign-in again", WAIT, |list| {
        list.iter().filter(|e| e["state"] == "needsLogin").count() == 2
    });
    let again = list.iter().rfind(|e| e["state"] == "needsLogin").unwrap();
    assert_eq!(again["error"], "The agent still needs you to sign in.");
    assert!(!fx.fake.methods().contains(&"authenticate".to_owned()));

    // an agent way: the agent signs in, then the prompt goes on in a new session
    fx.session
        .login(&LoginArgs {
            method_id: Some("oauth".to_owned()),
        })
        .unwrap();
    assert_eq!(fx.events.turn_end()["stopReason"], "end_turn");
    assert_eq!(
        fx.fake.methods(),
        [
            "initialize",
            "session/new",
            "session/new",
            "authenticate",
            "session/new",
            "session/set_mode",
            "session/prompt"
        ]
    );
    let seen = fx.fake.seen();
    let auth = seen.iter().find(|m| m["method"] == "authenticate").unwrap();
    assert_eq!(auth["params"], json!({ "methodId": "oauth" }));
    let prompt = seen
        .iter()
        .find(|m| m["method"] == "session/prompt")
        .unwrap();
    assert_eq!(prompt["params"]["prompt"][0]["text"], "Hello.");
    assert_eq!(fx.events.of_type("text")[0]["text"], "Signed in.");
    assert_eq!(fx.session.status().state, AcpState::Ready);
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 1);
    let none = fx.session.login(&LoginArgs::default()).unwrap_err();
    assert_eq!(none.detail.as_deref(), Some("noLogin"));
}
