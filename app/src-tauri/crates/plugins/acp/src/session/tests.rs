//! The session against a fake agent over pipes, scripted after messages of a real ACP agent (Claude's adapter).

use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use permark_core::events::{CoreEvent, EventSink};
use serde_json::{json, Value};

use super::permission::inside;
use super::*;
use crate::agents::{platforms, Choice, Launch, Plan, CHOICE_FILE};
use crate::dto::{AcpReach, AgentNeed, PermissionDiff};
use crate::testing;
use permark_core::state::{Loaded, StateFile};

/// Longest wait for something a fake agent does.
const WAIT: Duration = Duration::from_secs(20);

/// A value tests wait on (set by other threads).
#[derive(Default)]
struct Watched<T> {
    value: Mutex<T>,
    changed: Condvar,
}

impl<T: Clone + std::fmt::Debug> Watched<T> {
    // poison-tolerant: a failed wait must not fail the teardown that still emits
    fn get(&self) -> T {
        lock(&self.value).clone()
    }

    fn update(&self, change: impl FnOnce(&mut T)) {
        change(&mut lock(&self.value));
        self.changed.notify_all();
    }

    /// Waits until `test` holds; answers the value then.
    fn wait_for(&self, what: &str, wait: Duration, test: impl Fn(&T) -> bool) -> T {
        let started = Instant::now();
        let mut value = lock(&self.value);
        while !test(&value) {
            let left = wait.saturating_sub(started.elapsed());
            if left.is_zero() {
                let now = format!("{value:#?}");
                drop(value);
                panic!("no {what}; now: {now}");
            }
            value = self
                .changed
                .wait_timeout(value, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        value.clone()
    }
}

/// Runs on the emitting thread before an event is kept.
type Spy = Box<dyn Fn(&Value) + Send + Sync>;

/// The events the window got (+ a spy a test may set).
#[derive(Default)]
struct Events {
    list: Watched<Vec<Value>>,
    spy: Mutex<Option<Spy>>,
}

impl std::ops::Deref for Events {
    type Target = Watched<Vec<Value>>;

    fn deref(&self) -> &Self::Target {
        &self.list
    }
}

impl EventSink for Events {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, payload } = event {
            assert_eq!(name, "acp:update");
            if let Some(spy) = lock(&self.spy).as_ref() {
                spy(&payload);
            }
            self.update(|list| list.push(payload));
        }
    }
}

impl Events {
    fn all(&self) -> Vec<Value> {
        self.get()
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

/// The fake agent's side of the pipes.
struct Peer {
    from_client: BufReader<PipeReader>,
    to_client: PipeWriter,
    stderr: PipeWriter,
    /// Every message the client sent, in order.
    seen: Arc<Watched<Vec<Value>>>,
    /// Its `initialize` answer says it takes embedded resources.
    embedded: bool,
    /// The session's folder, from `session/new`.
    cwd: PathBuf,
    auth: Arc<Auth>,
    /// `Some`: stops reading its stdin once the session is set up, until it is stopped (this ends).
    deaf: Option<mpsc::Receiver<()>>,
    /// Its `session/new` answer offers the modes only as a config option (no `modes`).
    config_modes: bool,
    story: Arc<Story>,
}

/// What the fake agent keeps of earlier conversations.
#[derive(Default)]
struct Story {
    /// `initialize` answers `loadSession: false`.
    no_load: AtomicBool,
    /// `initialize` offers `sessionCapabilities.resume`.
    resume: AtomicBool,
    /// `session/load` | `session/resume` is refused: the agent no longer has the conversation.
    gone: AtomicBool,
    /// Messages a `session/load` sends before its answer (the conversation replayed).
    replay: Mutex<Vec<Value>>,
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
        self.seen.update(|seen| seen.push(message.clone()));
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

    /// Reads the client's messages until one `test` accepts (`None`: its stdin closed first).
    fn until(&mut self, test: impl Fn(&Value) -> bool) -> Option<Value> {
        loop {
            let message = self.next()?;
            if test(&message) {
                return Some(message);
            }
        }
    }

    /// Writes into the copy as the agent's own tools do.
    fn write(&self, path: &str, text: &str) {
        let abs = self.cwd.join(path);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(abs, text).unwrap();
    }
}

/// A config option of Claude's adapter that is no mode.
static MODEL_OPTION: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| {
    json!({
        "id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "default",
        "options": [{ "value": "default", "name": "Default" }, { "value": "opus", "name": "Opus" }],
    })
});

/// The mode choice as Claude's adapter also offers it: a config option (`currentValue` = `current`).
fn mode_option(current: &str) -> Value {
    json!({
        "id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": current,
        "options": [
            { "value": "default", "name": "Manual", "description": "Always ask before making changes" },
            { "value": "acceptEdits", "name": "Accept edits" },
            { "value": "auto", "name": "Auto" },
        ],
    })
}

/// What a turn returns when it answered the prompt itself.
const ANSWERED: &str = "(answered)";

/// The text of a prompt message.
fn prompt_text(message: &Value) -> &str {
    message["params"]["prompt"][0]["text"]
        .as_str()
        .unwrap_or_default()
}

/// What the fake does with one prompt: `Some(stop reason)` answers it ([`ANSWERED`]: it did itself); `None` exits
/// without answering.
type Turn = Arc<dyn Fn(&mut Peer, &Value) -> Option<&'static str> + Send + Sync>;

fn agent_loop(mut peer: Peer, turn: &Turn) {
    while let Some(message) = peer.next() {
        let Some(method) = message["method"].as_str().map(str::to_owned) else {
            continue;
        };
        let id = message["id"].clone();
        let opens = matches!(
            method.as_str(),
            "session/new" | "session/load" | "session/resume"
        );
        if opens && peer.auth.required.load(Ordering::SeqCst) {
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
                    "promptCapabilities": { "image": true, "embeddedContext": peer.embedded },
                    "mcpCapabilities": { "http": true, "sse": true },
                    "loadSession": !peer.story.no_load.load(Ordering::SeqCst),
                    "sessionCapabilities": if peer.story.resume.load(Ordering::SeqCst) {
                        json!({ "list": {}, "resume": {}, "close": {} })
                    } else {
                        json!({ "list": {} })
                    },
                },
                "agentInfo": { "name": "@agentclientprotocol/claude-agent-acp", "title": "Claude Agent", "version": "0.108.0" },
                "authMethods": peer.auth.methods.lock().unwrap().clone(),
            }),
            "authenticate" => {
                peer.auth.required.store(false, Ordering::SeqCst);
                json!({})
            }
            "session/load" | "session/resume" if peer.story.gone.load(Ordering::SeqCst) => {
                peer.send(&json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32002, "message": "Resource not found: no conversation 44453ef7" },
                }));
                continue;
            }
            "session/new" | "session/load" | "session/resume" => {
                peer.cwd = PathBuf::from(message["params"]["cwd"].as_str().unwrap());
                if method == "session/load" {
                    let replay = peer.story.replay.lock().unwrap().clone();
                    for message in &replay {
                        peer.send(message);
                    }
                }
                if peer.config_modes {
                    json!({ "sessionId": SESSION, "configOptions": [MODEL_OPTION.clone(), mode_option("auto")] })
                } else {
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
            }
            "session/set_config_option" => {
                let value = message["params"]["value"].as_str().unwrap_or_default();
                json!({ "configOptions": [MODEL_OPTION.clone(), mode_option(value)] })
            }
            "session/set_mode" => {
                peer.send(&json!({ "jsonrpc": "2.0", "id": id, "result": {} }));
                if let Some(stopped) = peer.deaf.take() {
                    let _ = stopped.recv();
                    return;
                }
                continue;
            }
            "session/prompt" => match turn(&mut peer, &message) {
                Some(ANSWERED) => continue,
                Some(reason) => json!({ "stopReason": reason }),
                None => return,
            },
            _ => continue,
        };
        peer.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }
}

/// A start's folder + the names in it at the start.
type StartFolder = (PathBuf, Vec<String>);

/// What the test sees of the fake agents it started.
#[derive(Clone, Default)]
struct Fake {
    seen: Arc<Watched<Vec<Value>>>,
    launches: Arc<AtomicUsize>,
    stops: Arc<Watched<usize>>,
    /// Paths looked at when each agent starts.
    watch: Arc<Mutex<Vec<PathBuf>>>,
    /// Of `watch`, those present at each start.
    present: Arc<Mutex<Vec<Vec<PathBuf>>>>,
    /// The agents started from now on take no embedded resources.
    plain: Arc<AtomicBool>,
    /// What each start was asked to run.
    plans: Arc<Mutex<Vec<Plan>>>,
    /// Each start's folder.
    folders: Arc<Mutex<Vec<StartFolder>>>,
    /// The agents started from now on stop reading once their session is set up.
    deaf: Arc<AtomicBool>,
    /// Writes toward a deaf agent that began with more than a pipe holds: each one cannot end until the agent stops.
    stuck: Arc<Watched<usize>>,
    /// The agents started from now on offer their modes only as a config option.
    config_modes: Arc<AtomicBool>,
    auth: Arc<Auth>,
    story: Arc<Story>,
}

impl Fake {
    fn seen(&self) -> Vec<Value> {
        self.seen.get()
    }

    fn stopped(&self, count: usize) {
        self.stops
            .wait_for("agent stopped", WAIT, |stops| *stops == count);
    }

    fn count(&self, method: &str) -> usize {
        self.methods().iter().filter(|m| *m == method).count()
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

    /// The client's answer to the agent's request `id` (waits for it).
    fn reply_to(&self, id: &Value) -> Value {
        let is_reply = |m: &Value| m["id"] == *id && m.get("method").is_none();
        self.seen
            .wait_for(&format!("reply to {id}"), WAIT, |seen| {
                seen.iter().any(is_reply)
            })
            .into_iter()
            .find(is_reply)
            .unwrap()
    }

    fn launch(&self, turn: Turn) -> Launch {
        let fake = self.clone();
        Arc::new(move |plan: &Plan, cwd: &Path| {
            fake.launches.fetch_add(1, Ordering::SeqCst);
            fake.plans.lock().unwrap().push(plan.clone());
            let names = std::fs::read_dir(cwd)?
                .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
                .collect::<std::io::Result<Vec<_>>>()?;
            fake.folders
                .lock()
                .unwrap()
                .push((cwd.to_path_buf(), names));
            let present = fake
                .watch
                .lock()
                .unwrap()
                .iter()
                .filter(|path| path.exists())
                .cloned()
                .collect();
            fake.present.lock().unwrap().push(present);
            let (stop_tx, stop_rx) = mpsc::channel::<()>();
            let deaf = fake.deaf.load(Ordering::SeqCst);
            let (agent_in, client_out) = std::io::pipe()?;
            let (client_in, agent_out) = std::io::pipe()?;
            let (client_err, agent_err) = std::io::pipe()?;
            let peer = Peer {
                from_client: BufReader::new(agent_in),
                to_client: agent_out,
                stderr: agent_err,
                seen: Arc::clone(&fake.seen),
                embedded: !fake.plain.load(Ordering::SeqCst),
                cwd: cwd.to_path_buf(),
                auth: Arc::clone(&fake.auth),
                deaf: deaf.then_some(stop_rx),
                config_modes: fake.config_modes.load(Ordering::SeqCst),
                story: Arc::clone(&fake.story),
            };
            let turn = Arc::clone(&turn);
            let agent = std::thread::spawn(move || agent_loop(peer, &turn));
            let stops = Arc::clone(&fake.stops);
            let to_agent: Box<dyn Write + Send> = if deaf {
                Box::new(Watching {
                    pipe: client_out,
                    stuck: Arc::clone(&fake.stuck),
                })
            } else {
                Box::new(client_out)
            };
            Ok(AgentPipes {
                to_agent,
                from_agent: Box::new(client_in),
                stderr: Box::new(client_err),
                stop: Box::new(move || {
                    // its stdin is closed by now: the fake's loop ends (a deaf one: on this signal)
                    drop(stop_tx);
                    let _ = agent.join();
                    stops.update(|stops| *stops += 1);
                }),
            })
        })
    }
}

/// Most bytes a pipe holds unread here (Windows | Linux pipes hold far less).
const PIPE_HOLDS: usize = 1 << 20;

/// The client's end of a deaf agent's stdin: counts each write larger than the pipe holds as it begins (the agent no
/// longer reads, so that write cannot end until the agent is stopped).
struct Watching {
    pipe: PipeWriter,
    stuck: Arc<Watched<usize>>,
}

impl Write for Watching {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.len() > PIPE_HOLDS {
            self.stuck.update(|stuck| *stuck += 1);
        }
        self.pipe.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.pipe.flush()
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

type MakeAgents = fn(StateFile<Choice>, &Path) -> Agents;

/// The file of the project's last conversation in the profile `profile`.
fn saved_file(profile: &Path) -> StateFile<Remembered> {
    testing::app_store(profile).profile_plugin_file(&crate::ID, SAVED_FILE)
}

/// Programs found only on the fake `PATH`.
fn fake_path(file: StateFile<Choice>, bin: &Path) -> Agents {
    Agents::with(Some(file), testing::find_in(bin), platforms())
}

/// Programs found on this process's own `PATH`.
fn real_path(file: StateFile<Choice>, _: &Path) -> Agents {
    Agents::new(Some(file))
}

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
        Self::with(launch, fake, choice, fake_path)
    }

    /// `agents`: the catalog over the choice file and the fake `PATH`'s folder.
    fn with(launch: Launch, fake: Fake, choice: Option<&Choice>, agents: MakeAgents) -> Self {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("notes.md"), NOTES).unwrap();
        let profile = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        testing::program(bin.path(), "npx");
        let file = testing::app_store(profile.path()).app_file(CHOICE_FILE);
        if let Some(choice) = choice {
            file.save(choice).unwrap();
        }
        let agents = agents(file, bin.path());
        let (vault, documents) = testing::open(project.path());
        let events = Arc::new(Events::default());
        let copy: SharedCopy = Arc::new(Mutex::new(None));
        let session = Session::new(Host {
            agents,
            launch,
            project: testing::project(&vault, &documents),
            place: Some(profile.path().join("work")),
            run: Some(profile.path().join("run")),
            copy: Arc::clone(&copy),
            saved: Some(saved_file(profile.path())),
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

    /// The project's last conversation as kept on disk (`None`: none | forgotten).
    fn saved(&self) -> Option<Saved> {
        match saved_file(self.profile.path()).load() {
            Loaded::Found(file) => file.last,
            _ => None,
        }
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

    /// The card of the agent's question `request_id` (the JSON text of its id).
    fn card(&self, request_id: &str) -> Value {
        let is_card = |e: &Value| e["type"] == "permission" && e["requestId"] == request_id;
        self.events
            .wait_for(&format!("card {request_id}"), WAIT, |list| {
                list.iter().any(is_card)
            })
            .into_iter()
            .find(is_card)
            .unwrap()
    }

    fn reject(&self, request_id: &str) {
        self.session
            .answer(&AnswerArgs {
                request_id: request_id.to_owned(),
                option_id: Some("reject".to_owned()),
            })
            .unwrap();
    }

    fn turns_ended(&self, count: usize) -> Vec<Value> {
        self.events.wait_for("turn ends", WAIT, |list| {
            list.iter().filter(|e| e["type"] == "turnEnd").count() == count
                && list.last().is_some_and(|e| e["type"] == "status")
        })
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

/// The turn's end computes the change list once and keeps it for the window; the status counts the files the copy
/// left out for their size.
#[test]
fn the_turn_end_keeps_the_list_and_the_status_counts_skipped_files() {
    let fx = Fx::new(|_, message| {
        let blocks = message["params"]["prompt"].as_array().unwrap();
        let link = blocks
            .iter()
            .find(|b| b["type"] == "resource_link")
            .unwrap();
        let file = url::Url::parse(link["uri"].as_str().unwrap())
            .unwrap()
            .to_file_path()
            .unwrap();
        std::fs::write(file, "Changed by the agent.\n").unwrap();
        Some("end_turn")
    });
    let big = vec![b'x'; usize::try_from(crate::work::MAX_FILE_BYTES).unwrap() + 1];
    std::fs::write(fx.project.path().join("big.txt"), big).unwrap();
    assert_eq!(fx.session.status().skipped_files, 0);
    fx.session
        .prompt(PromptArgs {
            text: "Edit.".to_owned(),
            selection: None,
            document: Some("notes.md".to_owned()),
        })
        .unwrap();
    assert_eq!(fx.events.turn_end()["changed"], 1);
    assert_eq!(fx.session.status().skipped_files, 1);
    let mut held = fx.copy.lock().unwrap();
    let copy = held.as_mut().unwrap();
    // kept: a later edit of the copy shows only once the list is asked again
    std::fs::write(copy.dir().join("other.md"), "new\n").unwrap();
    assert_eq!(copy.changes().unwrap().files.len(), 1);
    assert_eq!(copy.refresh().unwrap().files.len(), 2);
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
    assert_eq!(fx.session.status().needs, None);

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
            "requestId": "1:7",
            "title": "Run the tests",
            "kind": "execute",
            "reach": "outside",
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
        request_id: "1:7".to_owned(),
        option_id: Some("maybe".to_owned()),
    });
    assert_eq!(wrong.unwrap_err().kind, ErrorKind::InvalidData);
    fx.session
        .answer(&AnswerArgs {
            request_id: "1:7".to_owned(),
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
        vec![json!({ "type": "permissionDone", "requestId": "1:7" })]
    );
    let again = fx.session.answer(&AnswerArgs {
        request_id: "1:7".to_owned(),
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
    assert_eq!(card["requestId"], "1:\"ask-1\"");
    assert_eq!(card["kind"], "edit");
    assert_eq!(card["reach"], "outside");
    assert_eq!(
        card["detail"],
        format!("notes.md\n{}", project.path().join("notes.md").display())
    );
    assert_eq!(
        card["diff"],
        json!({
            "path": project.path().join("notes.md").display().to_string(),
            "oldLines": ["Line two of the notes."],
            "newLines": ["Line two, edited."],
            "truncated": false,
        })
    );
    // dismissed: the agent hears "cancelled"
    fx.session
        .answer(&AnswerArgs {
            request_id: "1:\"ask-1\"".to_owned(),
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
        vec![json!({ "type": "permissionDone", "requestId": "1:3" })]
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

/// The agent's last words come as they would read in a terminal (no color codes), never with account news, and the
/// window words the error itself (a code).
#[test]
fn an_agent_that_exits_mid_turn_reports_its_last_words() {
    let fx = Fx::new(|peer, _| {
        peer.stderr
            .write_all(
                b"[session/create] phase=validate-cwd\n\
                  {\"method\":\"_auth/status_update\",\"params\":{\"email\":\"person@example.com\"}}\n\
                  \x1b[31mfatal:\x1b[0m the model is gone\r\n",
            )
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
        .map(|e| e["error"].clone())
        .unwrap();
    assert_eq!(
        error,
        json!({
            "code": "exited",
            "stderr": "[session/create] phase=validate-cwd\nfatal: the model is gone",
        })
    );
    let status = fx.session.status();
    assert_eq!(status.state, AcpState::Error);
    assert_eq!(serde_json::to_value(status.error).unwrap(), error);
    fx.fake.stopped(1);
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
    fx.fake.stopped(1);
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
    assert_eq!(fx.fake.stops.get(), 1);
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
    // a passage past the cap: refused with its reason; one at the cap goes
    let sized = |bytes: usize| {
        let mut args = passage("notes.md", 1, 1);
        if let Some(selection) = &mut args.selection {
            selection.text = "x".repeat(bytes);
        }
        args
    };
    let large = fx.session.prompt(sized(MAX_PASSAGE_BYTES + 1)).unwrap_err();
    assert_eq!(
        (large.kind, large.detail.as_deref()),
        (ErrorKind::TooLarge, Some("passageTooLarge"))
    );
    assert!(check(&sized(MAX_PASSAGE_BYTES)).is_ok());
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
        real_path,
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
    fx.fake.stopped(1);
    fx.ask("Two.");
    fx.turns_ended(2);
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
    // an own command has no name of the backend's: the window words it
    assert!(status.chosen);
    assert_eq!(status.agent_name, None);
    fx.ask("Three.");
    fx.turns_ended(3);
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
            request_id: "1:5".to_owned(),
            option_id: Some("reject".to_owned()),
        })
        .unwrap();
    fx.events.turn_end();
    assert_eq!(fx.fake.stops.get(), 0);
}

/// `auth_required` at `session/new`: the person sees the agent's ways to sign in; after a terminal way a fresh agent
/// starts (it reads the new sign-in at its start); an agent way has the agent sign in, then the waiting prompt goes on.
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

    // a terminal way: the person signs in outside; a fresh agent starts (still refused here)
    fx.session
        .login(&LoginArgs {
            method_id: Some("claude-ai-login".to_owned()),
        })
        .unwrap();
    let list = fx.events.wait_for("sign-in again", WAIT, |list| {
        list.iter().filter(|e| e["state"] == "needsLogin").count() == 2
    });
    let again = list.iter().rfind(|e| e["state"] == "needsLogin").unwrap();
    assert_eq!(again["error"], json!({ "code": "stillNeedsLogin" }));
    assert!(!fx.fake.methods().contains(&"authenticate".to_owned()));
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 2);
    fx.fake.stopped(1);

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
            "initialize",
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
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 2);
    let none = fx.session.login(&LoginArgs::default()).unwrap_err();
    assert_eq!(none.detail.as_deref(), Some("noLogin"));
}

/// The agent's program starts in an empty folder of the plugin's own, never in the copy: a runner (`npx`, `uvx`) reads
/// its settings (`.npmrc`, `uv.toml`, `package.json`) and a Windows script its `node` from the folder it starts in, and
/// the copy holds the project's files. The copy is only the session's folder.
#[test]
fn the_agent_program_starts_in_an_empty_folder_never_in_the_copy() {
    let fx = Fx::new(|_, _| Some("end_turn"));
    for (name, text) in [
        (
            ".npmrc",
            "registry=https://example.invalid/
",
        ),
        (
            "package.json",
            "{}
",
        ),
        (
            "uv.toml",
            "index-url = \"https://example.invalid/\"
",
        ),
        (
            "node.cmd",
            "@echo off
",
        ),
    ] {
        std::fs::write(fx.project.path().join(name), text).unwrap();
    }
    // whatever an earlier run left in the start folder goes first
    let run = fx.profile.path().join("run");
    std::fs::create_dir_all(run.join("node_modules")).unwrap();
    std::fs::write(
        run.join(".npmrc"),
        "registry=https://example.invalid/
",
    )
    .unwrap();
    fx.ask("Hello.");
    fx.events.turn_end();
    let folders = fx.fake.folders.lock().unwrap().clone();
    assert_eq!(folders, [(run.clone(), Vec::<String>::new())]);
    let copy = fx.copy_dir();
    assert_ne!(folders[0].0, copy);
    assert!(!run.starts_with(&copy) && !copy.starts_with(&run));
    assert!(!run.starts_with(fx.project.path()));
    let new = fx
        .fake
        .seen()
        .into_iter()
        .find(|m| m["method"] == "session/new")
        .unwrap();
    assert_eq!(PathBuf::from(new["params"]["cwd"].as_str().unwrap()), copy);
}

/// The profile reached through a link (macOS temp folders sit behind /var -> /private/var, Windows ones can carry 8.3
/// names, a junction can lead to another volume): the session opens on the copy's real spelling, so the places the
/// agent names in it match the copy's own root; places named through the link still count as inside.
#[test]
fn the_session_opens_on_the_real_spelling_of_the_copy() {
    let fx = Fx::new(|peer, _| {
        let notes = peer.in_copy("notes.md");
        peer.update(&json!({
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit notes.md", "kind": "edit",
            "status": "pending", "locations": [{ "path": notes }], "rawInput": { "file_path": notes },
        }));
        peer.ask(&json!(1), &json!({ "toolCallId": "t1" }));
        peer.until(|m| m["id"] == 1)?;
        Some("end_turn")
    });
    let real = fx.profile.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    let alias = fx.profile.path().join("work");
    if crate::work::tests::link(&real, &alias).is_err() {
        return; // the system refused a folder link here: nothing to spell through
    }
    fx.ask("Edit the notes.");
    fx.events.turn_end();
    let spelled = alias.join(fx.project.path().file_name().unwrap());
    let copy = Root::open(&spelled).unwrap();
    let new = fx
        .fake
        .seen()
        .into_iter()
        .find(|m| m["method"] == "session/new")
        .unwrap();
    let cwd = PathBuf::from(new["params"]["cwd"].as_str().unwrap());
    assert_eq!(cwd, copy.path());
    assert_ne!(cwd, spelled);
    assert_eq!(
        fx.fake.reply_to(&json!(1))["result"],
        json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } })
    );
    assert!(inside(&Vault::new(copy), &spelled.join("notes.md")));
}

/// Claude's adapter turns on its mode that skips every question unless the client opts out: the session asks it never
/// to, whatever a settings file | a subagent asks for.
#[test]
fn the_session_opts_out_of_skipping_the_persons_questions() {
    let fx = Fx::new(|_, _| Some("end_turn"));
    fx.ask("Hello.");
    fx.events.turn_end();
    let new = fx
        .fake
        .seen()
        .into_iter()
        .find(|m| m["method"] == "session/new")
        .unwrap();
    assert_eq!(
        new["params"]["_meta"]["claudeCode"]["options"]["allowDangerouslySkipPermissions"],
        json!(false)
    );
}

/// An agent that stops reading its stdin while a prompt larger than a pipe holds is being written: the shutdown
/// never waits on the write, and the agent is stopped.
#[test]
fn an_agent_that_stops_reading_never_holds_up_the_shutdown() {
    let fx = Fx::new(|_, _| Some("end_turn"));
    fx.fake.deaf.store(true, Ordering::SeqCst);
    fx.ask(&"x".repeat(4 << 20));
    // the prompt's write began toward the agent that no longer reads: it cannot end
    fx.fake
        .stuck
        .wait_for("a stuck write", WAIT, |stuck| *stuck >= 1);
    let session = fx.session.clone();
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        session.shutdown();
        let _ = done_tx.send(());
    });
    assert!(done_rx.recv_timeout(WAIT).is_ok(), "the shutdown hangs");
    assert_eq!(fx.fake.stops.get(), 1);
}

/// Inside a strict Snap no program of the computer can start: the status says so, the prompt fails with the reason.
#[test]
fn inside_a_snap_the_status_says_no_agent_can_start() {
    let fake = Fake::default();
    let launch = fake.launch(Arc::new(|_: &mut Peer, _: &Value| Some("end_turn")));
    let fx = Fx::with(
        launch,
        fake,
        Some(&Choice::Agent("claude-acp".to_owned())),
        |file, bin| fake_path(file, bin).in_snap(true),
    );
    let status = fx.session.status();
    assert_eq!(status.needs, Some(AgentNeed::Snap));
    assert!(status.chosen);
    fx.ask("Hello.");
    let list = fx.events.wait_for("error", WAIT, |list| {
        list.iter().any(|e| e["state"] == "error")
    });
    let error = list.iter().find(|e| e["state"] == "error").unwrap();
    // nothing to start it with: the status's `needs` (snap) says why
    assert_eq!(error["error"], json!({ "code": "notFound" }));
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 0);
}

/// Claude's three options, without ids of its own: the request's.
fn reject_reply(fx: &Fx, id: &Value) {
    assert_eq!(
        fx.fake.reply_to(id)["result"],
        json!({ "outcome": { "outcome": "selected", "optionId": "reject" } })
    );
}

/// Writing an agent's own settings in the copy is never allowed by rule, even where the path is inside the copy: they
/// would load at its next start. Named by a location or only by the tool's input, the person decides.
#[test]
fn an_edit_of_agent_settings_in_the_copy_gets_a_card() {
    let fx = Fx::new(|peer, _| {
        let settings = peer.in_copy(".claude/settings.local.json");
        let mut call = edit_of(&settings);
        call["title"] = json!("Write .claude/settings.local.json");
        peer.ask(&json!(1), &call);
        peer.until(|m| m["id"] == 1)?;
        // a move named inside the copy whose input puts the file where servers are read from
        let notes = peer.in_copy("notes.md");
        peer.ask(
            &json!(2),
            &json!({
                "toolCallId": "toolu_move", "title": "Move notes.md", "kind": "move",
                "locations": [{ "path": notes }],
                "rawInput": { "source": notes, "destination": ".mcp.json" },
            }),
        );
        peer.until(|m| m["id"] == 2)?;
        Some("end_turn")
    });
    fx.ask("Allow every command.");
    let card = fx.card("1:1");
    assert_eq!(card["kind"], "edit");
    assert_eq!(card["reach"], "agentSettings");
    assert_eq!(card["detail"], ".claude/settings.local.json");
    assert_eq!(card["diff"]["path"], ".claude/settings.local.json");
    fx.reject("1:1");
    let card = fx.card("1:2");
    assert_eq!(card["kind"], "move");
    assert_eq!(card["reach"], "agentSettings");
    fx.reject("1:2");
    fx.events.turn_end();
    reject_reply(&fx, &json!(1));
    reject_reply(&fx, &json!(2));
}

/// Agent settings the agent wrote into the copy (any depth, files and folders) are gone before the next agent starts
/// and before the next prompt reaches the running one; the agent's other files stay.
#[test]
fn agent_settings_in_the_copy_are_gone_before_each_start_and_prompt() {
    let fx = Fx::new(|peer, message| {
        match prompt_text(message) {
            "Plant." => {
                peer.write(
                    ".claude/settings.json",
                    "{\"permissions\":{\"allow\":[\"Bash(*)\"]}}",
                );
                peer.write(".mcp.json", "{\"mcpServers\":{}}");
                peer.write("docs/.gemini/settings.json", "{}");
                peer.write(".codex/config.toml", "x = 1");
                peer.write(
                    ".claude/agents/helper.md",
                    "permissionMode: bypassPermissions",
                );
                peer.write(".claude/rules/tidy.md", "Tidy the notes.");
            }
            "Plant again." => peer.write(".mcp.json", "{}"),
            _ => {
                let left = peer.cwd.join(".mcp.json").exists();
                peer.text(if left { "present" } else { "absent" });
            }
        }
        Some("end_turn")
    });
    fx.ask("Plant.");
    fx.events.turn_end();
    let copy = fx.copy_dir();
    let planted: Vec<PathBuf> = [
        ".claude/settings.json",
        ".mcp.json",
        "docs/.gemini/settings.json",
        ".codex",
        ".claude/agents/helper.md",
        ".claude/rules/tidy.md",
    ]
    .iter()
    .map(|path| copy.join(path))
    .collect();
    assert!(planted.iter().all(|path| path.exists()));
    *fx.fake.watch.lock().unwrap() = planted.clone();
    fx.session.reset();
    fx.ask("Plant again.");
    fx.turns_ended(2);
    // at the second start only the agent's instructions were left
    assert_eq!(
        fx.fake.present.lock().unwrap()[1],
        [copy.join(".claude/rules/tidy.md")]
    );
    // the same agent's next prompt finds the copy cleared again
    fx.ask("Look.");
    fx.turns_ended(3);
    assert_eq!(fx.events.of_type("text").last().unwrap()["text"], "absent");
}

/// A question is routed on everything the agent told of its tool call: a bare question about an edit of the copy
/// it announced before is allowed; a search naming no place, and the same edit once an update moved it out of the
/// copy, get cards showing what was told.
#[test]
fn questions_are_routed_on_everything_told_of_the_tool_call() {
    let project = tempfile::tempdir().unwrap();
    let outside = project.path().join("notes.md").display().to_string();
    let fx = Fx::new(move |peer, _| {
        let notes = peer.in_copy("notes.md");
        peer.update(&json!({
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit notes.md", "kind": "edit",
            "status": "pending", "locations": [{ "path": notes }], "rawInput": { "file_path": notes },
        }));
        peer.ask(&json!(1), &json!({ "toolCallId": "t1" }));
        peer.until(|m| m["id"] == 1)?;
        peer.ask(
            &json!(2),
            &json!({ "toolCallId": "t2", "title": "Grep token", "kind": "search" }),
        );
        peer.until(|m| m["id"] == 2)?;
        peer.update(&json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "t1",
            "locations": [{ "path": outside }], "rawInput": { "file_path": outside },
        }));
        peer.ask(&json!(3), &json!({ "toolCallId": "t1" }));
        peer.until(|m| m["id"] == 3)?;
        Some("end_turn")
    });
    fx.ask("Edit the notes.");
    let search = fx.card("1:2");
    assert_eq!(
        (&search["title"], &search["kind"], &search["detail"]),
        (&json!("Grep token"), &json!("search"), &json!(""))
    );
    fx.reject("1:2");
    let moved = fx.card("1:3");
    assert_eq!(
        (&moved["title"], &moved["kind"]),
        (&json!("Edit notes.md"), &json!("edit"))
    );
    assert_eq!(
        moved["detail"],
        project.path().join("notes.md").display().to_string()
    );
    fx.reject("1:3");
    fx.events.turn_end();
    assert_eq!(
        fx.fake.reply_to(&json!(1))["result"],
        json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } })
    );
    assert_eq!(fx.events.of_type("permission").len(), 2);
}

/// Allowed by rule only: a thought, or a read | search | edit | delete | move that names a place and keeps every
/// place it names (a move's destination among them) inside the copy and off agent settings.
#[test]
fn only_plain_work_on_the_copy_is_allowed_by_rule() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "a").unwrap();
    let copy = Vault::new(Root::open(dir.path()).unwrap());
    let root = copy.main().path().to_path_buf();
    let other = tempfile::tempdir().unwrap();
    let out = other.path().join("x.md");
    let tool = |kind: Option<ToolKind>, locations: &[&Path], input: Value| ToolLine {
        kind,
        locations: locations.iter().map(|p| p.to_path_buf()).collect(),
        raw_input: Some(input),
        ..ToolLine::default()
    };
    let a = root.join("a.md");
    let none = json!({});
    let allowed = |line: &ToolLine| plain_work(line, &copy);

    // a think tool can start a subagent with tools of its own: the person decides
    assert!(!allowed(&tool(Some(ToolKind::Think), &[], none.clone())));
    assert!(!allowed(&tool(Some(ToolKind::Think), &[&a], none.clone())));
    for kind in [
        ToolKind::Read,
        ToolKind::Search,
        ToolKind::Edit,
        ToolKind::Delete,
        ToolKind::Move,
    ] {
        assert!(allowed(&tool(Some(kind), &[&a], none.clone())), "{kind:?}");
        assert!(
            !allowed(&tool(Some(kind), &[], none.clone())),
            "{kind:?} nowhere"
        );
        assert!(
            !allowed(&tool(Some(kind), &[&out], none.clone())),
            "{kind:?} out"
        );
    }
    for kind in [
        None,
        Some(ToolKind::Execute),
        Some(ToolKind::Fetch),
        Some(ToolKind::SwitchMode),
        Some(ToolKind::Other),
    ] {
        assert!(!allowed(&tool(kind, &[&a], none.clone())), "{kind:?}");
    }
    // agent settings | machinery, as a location or in the input, whatever the kind
    for kept in [
        ".claude/settings.json",
        ".mcp.json",
        "sub/.git/hooks/pre-commit",
        ".codex/config.toml",
        ".aiignore",
    ] {
        let path = root.join(kept);
        assert!(
            !allowed(&tool(Some(ToolKind::Edit), &[&path], none.clone())),
            "{kept}"
        );
        assert!(
            !allowed(&tool(Some(ToolKind::Think), &[&path], none.clone())),
            "{kept}"
        );
        assert!(
            !allowed(&tool(Some(ToolKind::Edit), &[&a], json!({ "path": kept }))),
            "{kept} in the input"
        );
    }
    // a move: its destination counts (relative: from the copy's folder)
    let moving = |to: Value| tool(Some(ToolKind::Move), &[&a], json!({ "destination": to }));
    assert!(allowed(&moving(json!("b.md"))));
    assert!(allowed(&moving(json!(root.join("sub/b.md")))));
    assert!(!allowed(&moving(json!(out))));
    assert!(!allowed(&moving(json!("../b.md"))));
    // a search of a folder outside the copy, named only in its input
    assert!(!allowed(&tool(
        Some(ToolKind::Search),
        &[&a],
        json!({ "path": other.path() })
    )));
    // subagents, skills, commands: they grant tools | modes when loaded
    for kept in [".claude/agents/helper.md", ".claude/skills/x/SKILL.md"] {
        let path = root.join(kept);
        assert!(
            !allowed(&tool(Some(ToolKind::Edit), &[&path], none.clone())),
            "{kept}"
        );
    }
}

/// Windows reaches one file under several spellings: a trailing dot | space, a `::$DATA` stream name, an 8.3 short
/// name. Each spelling of agent settings in the copy gets a card.
#[cfg(windows)]
#[test]
fn every_spelling_of_agent_settings_in_the_copy_gets_a_card() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".claude")).unwrap();
    std::fs::write(dir.path().join(".claude").join("rules.md"), "r").unwrap();
    let copy = Vault::new(Root::open(dir.path()).unwrap());
    let root = copy.main().path().to_path_buf();
    let edit = |path: PathBuf| ToolLine {
        kind: Some(ToolKind::Edit),
        locations: vec![path],
        raw_input: Some(json!({})),
        ..ToolLine::default()
    };
    let mut spellings = vec![
        root.join(".claude.").join("settings.local.json"),
        root.join(".claude").join("settings.local.json::$DATA"),
        root.join(".claude ").join("x"),
        root.join(".claude").join("settings.local.json."),
    ];
    // the short name exists only where the volume makes 8.3 names
    let short = root.join("CLAUDE~1");
    if short.exists() {
        spellings.push(short.join("settings.local.json"));
    }
    for path in spellings {
        assert!(
            !plain_work(&edit(path.clone()), &copy),
            "{}",
            path.display()
        );
    }
    // the plain file beside them is still work
    assert!(plain_work(
        &edit(root.join(".claude").join("rules.md")),
        &copy
    ));
}

/// A card warns of where allowing acts: the computer for anything not known to be work on files or naming a file
/// outside the copy, the agent's own settings for a write to them in the copy, nothing more for the copy's files.
#[test]
fn a_card_says_where_the_call_acts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "a").unwrap();
    let copy = Vault::new(Root::open(dir.path()).unwrap());
    let root = copy.main().path().to_path_buf();
    let other = tempfile::tempdir().unwrap();
    let out = other.path().join("x.md");
    let tool = |kind: Option<ToolKind>, locations: &[&Path], content: Value| ToolLine {
        kind,
        locations: locations.iter().map(|p| p.to_path_buf()).collect(),
        raw_input: Some(json!({})),
        content: serde_json::from_value(content).unwrap(),
        ..ToolLine::default()
    };
    let a = root.join("a.md");
    let settings = root.join(".claude").join("settings.json");
    let none = json!([]);
    let at = |line: &ToolLine| reach(line, &copy);

    assert_eq!(
        at(&tool(Some(ToolKind::Edit), &[&a], none.clone())),
        AcpReach::Copy
    );
    assert_eq!(
        at(&tool(Some(ToolKind::Think), &[], none.clone())),
        AcpReach::Copy
    );
    assert_eq!(
        at(&tool(Some(ToolKind::Edit), &[&settings], none.clone())),
        AcpReach::AgentSettings
    );
    for kind in [
        Some(ToolKind::Execute),
        Some(ToolKind::Fetch),
        Some(ToolKind::Other),
        None,
    ] {
        assert_eq!(
            at(&tool(kind, &[&a], none.clone())),
            AcpReach::Outside,
            "{kind:?}"
        );
    }
    assert_eq!(
        at(&tool(Some(ToolKind::Read), &[&a, &out], none.clone())),
        AcpReach::Outside
    );
    // a file it would write counts like a place it names
    let writes = |path: &Path| json!([{ "type": "diff", "path": path, "newText": "x" }]);
    assert_eq!(
        at(&tool(Some(ToolKind::Edit), &[&a], writes(&out))),
        AcpReach::Outside
    );
    assert_eq!(
        at(&tool(Some(ToolKind::Edit), &[&a], writes(&settings))),
        AcpReach::AgentSettings
    );
}

/// A card shows the first file the call would write (its project path, cut to a few lines of bounded length) and the
/// agent's own words about the call (cut).
#[test]
fn a_card_shows_what_the_call_would_write_and_the_agents_words() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "a").unwrap();
    let copy = Vault::new(Root::open(dir.path()).unwrap());
    let root = copy.main().path().to_path_buf();
    let other = tempfile::tempdir().unwrap();
    let out = other.path().join("x.md");
    let tool = |content: Value| ToolLine {
        kind: Some(ToolKind::Edit),
        content: serde_json::from_value(content).unwrap(),
        ..ToolLine::default()
    };
    let a = root.join("a.md");

    let edit = tool(json!([
        { "type": "content", "content": { "type": "text", "text": " Fix the title " } },
        { "type": "diff", "path": a, "oldText": "# Old\r\nbody\n", "newText": "# New\r\nbody\n" },
    ]));
    assert_eq!(
        diff(&edit, &copy),
        Some(PermissionDiff {
            path: "a.md".to_owned(),
            old_lines: vec!["# Old".to_owned(), "body".to_owned()],
            new_lines: vec!["# New".to_owned(), "body".to_owned()],
            truncated: false,
        })
    );
    assert_eq!(summary(&edit).as_deref(), Some("Fix the title"));

    // a new file outside the copy, long and wide, and a second file: cut, said so
    let long = (0..100)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let wide = "w".repeat(2000);
    let big = tool(json!([
        { "type": "diff", "path": out, "newText": format!("{wide}\n{long}") },
        { "type": "diff", "path": a, "oldText": "a", "newText": "b" },
    ]));
    let shown = diff(&big, &copy).unwrap();
    assert_eq!(shown.path, out.display().to_string());
    assert_eq!(shown.old_lines, Vec::<String>::new());
    assert_eq!(shown.new_lines.len(), 40);
    assert_eq!(shown.new_lines[0].chars().count(), 500);
    assert!(shown.truncated);
    let wide_only = tool(json!([{ "type": "diff", "path": a, "oldText": "a", "newText": wide }]));
    assert!(diff(&wide_only, &copy).unwrap().truncated);

    let said = tool(json!([
        { "type": "content", "content": { "type": "text", "text": "é".repeat(1500) } },
    ]));
    let words = summary(&said).unwrap();
    assert_eq!(words.chars().count(), 1001);
    assert!(words.ends_with('…'));
    assert_eq!(diff(&said, &copy), None);
    assert_eq!(summary(&tool(json!([]))), None);
}

#[test]
fn a_card_shows_the_command_else_the_places_else_the_address() {
    let dir = tempfile::tempdir().unwrap();
    let copy = Vault::new(Root::open(dir.path()).unwrap());
    let root = copy.main().path().to_path_buf();
    let line = |locations: Vec<PathBuf>, input: Value| ToolLine {
        locations,
        raw_input: Some(input),
        ..ToolLine::default()
    };
    assert_eq!(
        detail(
            &line(
                vec![root.join("a.md")],
                json!({ "command": ["git", "status"] })
            ),
            &copy
        ),
        "git status"
    );
    assert_eq!(
        detail(
            &line(vec![root.join("docs").join("a.md")], json!({})),
            &copy
        ),
        "docs/a.md"
    );
    assert_eq!(
        detail(&line(Vec::new(), json!({ "file_path": "notes.md" })), &copy),
        "notes.md"
    );
    assert_eq!(
        detail(
            &line(Vec::new(), json!({ "url": "https://example.com/" })),
            &copy
        ),
        "https://example.com/"
    );
}

/// An agent that does not take embedded resources gets the passage as text and a link to its lines.
#[test]
fn without_embedded_context_the_passage_goes_as_text_and_a_link() {
    let fx = Fx::new(|_, _| Some("end_turn"));
    fx.fake.plain.store(true, Ordering::SeqCst);
    fx.session
        .prompt(PromptArgs {
            text: "Shorten this.".to_owned(),
            selection: Some(PromptSelection {
                path: "notes.md".to_owned(),
                from_line: 2,
                to_line: 3,
                text: "Line two of the notes.\nLine three of the notes.\n".to_owned(),
            }),
            document: Some("notes.md".to_owned()),
        })
        .unwrap();
    fx.events.turn_end();
    let file = url::Url::from_file_path(fx.copy_dir().join("notes.md")).unwrap();
    let prompt = fx
        .fake
        .seen()
        .into_iter()
        .find(|m| m["method"] == "session/prompt")
        .unwrap()["params"]["prompt"]
        .clone();
    assert_eq!(
        prompt,
        json!([
            { "type": "text", "text": "Shorten this." },
            {
                "type": "text",
                "text": "Passage from notes.md, lines 2-3:\n```\nLine two of the notes.\nLine three of the notes.\n```",
            },
            { "type": "resource_link", "name": "notes.md", "uri": format!("{file}#L2:3") },
            { "type": "resource_link", "name": "notes.md", "uri": file.to_string() },
        ])
    );
}

#[test]
fn a_passage_is_fenced_longer_than_any_backticks_in_it() {
    assert_eq!(
        passage_text("a.md", 1, 1, "plain"),
        "Passage from a.md, lines 1-1:\n```\nplain\n```"
    );
    assert_eq!(
        passage_text("a.md", 4, 6, "```js\nx\n```\n"),
        "Passage from a.md, lines 4-6:\n````\n```js\nx\n```\n````"
    );
}

/// The agent withdraws a question (`$/cancel_request`): it is answered "request cancelled" and its card goes.
#[test]
fn a_withdrawn_question_is_answered_and_its_card_goes() {
    let fx = Fx::new(|peer, _| {
        peer.ask(
            &json!(9),
            &json!({ "toolCallId": "toolu_bash", "title": "Run the tests", "kind": "execute",
                     "rawInput": { "command": "npm test" } }),
        );
        peer.send(&json!({
            "jsonrpc": "2.0", "method": "$/cancel_request", "params": { "requestId": 9 },
        }));
        peer.until(|m| m["id"] == 9)?;
        peer.text("Never mind.");
        Some("end_turn")
    });
    fx.ask("Run the tests.");
    fx.card("1:9");
    fx.events.turn_end();
    assert_eq!(
        fx.events.of_type("permissionDone"),
        [json!({ "type": "permissionDone", "requestId": "1:9" })]
    );
    assert_eq!(fx.fake.reply_to(&json!(9))["error"]["code"], -32800);
    let late = fx.session.answer(&AnswerArgs {
        request_id: "1:9".to_owned(),
        option_id: Some("allow-once".to_owned()),
    });
    assert_eq!(late.unwrap_err().detail.as_deref(), Some("noQuestion"));
    // exactly one answer
    assert_eq!(
        fx.fake
            .seen()
            .iter()
            .filter(|m| m["id"] == 9 && m.get("method").is_none())
            .count(),
        1
    );
}

/// A question still open when the turn ends is answered `cancelled`: every request of the agent gets one answer.
#[test]
fn a_question_still_open_at_the_turn_end_is_answered_cancelled() {
    let fx = Fx::new(|peer, _| {
        peer.ask(
            &json!(4),
            &json!({ "toolCallId": "toolu_fetch", "title": "Fetch", "kind": "fetch" }),
        );
        Some("end_turn")
    });
    fx.ask("Look this up.");
    fx.events.turn_end();
    assert_eq!(
        fx.events.of_type("permissionDone"),
        [json!({ "type": "permissionDone", "requestId": "1:4" })]
    );
    assert_eq!(
        fx.fake.reply_to(&json!(4))["result"],
        json!({ "outcome": { "outcome": "cancelled" } })
    );
}

/// Stop while the copy is brought up to date, before the prompt is on its way: the prompt never reaches the agent and
/// the turn ends `cancelled` (no `session/cancel` for a prompt it never had).
#[test]
fn stop_before_the_prompt_is_sent_keeps_it_from_the_agent() {
    let fx = Fx::new(|peer, _| {
        peer.text("ok");
        Some("end_turn")
    });
    fx.ask("One.");
    fx.events.turn_end();
    // the copy busy: the turn waits before its prompt
    let held = fx.copy.lock().unwrap();
    fx.ask("Two.");
    fx.events.wait_for("second working", WAIT, |list| {
        list.iter().filter(|e| e["state"] == "working").count() == 2
    });
    fx.session.cancel();
    drop(held);
    let list = fx.turns_ended(2);
    assert_eq!(
        list.iter().rfind(|e| e["type"] == "turnEnd").unwrap()["stopReason"],
        "cancelled"
    );
    assert_eq!(fx.session.status().state, AcpState::Ready);
    assert_eq!(fx.fake.count("session/prompt"), 1);
    assert_eq!(fx.fake.count("session/cancel"), 0);
}

/// Stop during the start: the agent stops, the turn ends `cancelled` and the prompt never reaches any agent.
#[test]
fn stop_during_the_start_ends_the_turn_and_stops_the_agent() {
    let fx = Fx::new(|peer, message| {
        peer.text(prompt_text(message));
        Some("end_turn")
    });
    // the copy busy: the start waits before the agent's program
    let held = fx.copy.lock().unwrap();
    fx.ask("One.");
    fx.session.cancel();
    drop(held);
    let states: Vec<Value> = fx
        .events
        .all()
        .iter()
        .map(|e| e.get("state").cloned().unwrap_or_else(|| e["type"].clone()))
        .collect();
    assert_eq!(states, [json!("starting"), json!("turnEnd"), json!("idle")]);
    assert_eq!(
        fx.events.of_type("turnEnd")[0],
        json!({ "type": "turnEnd", "stopReason": "cancelled", "changed": 0 })
    );
    // the next prompt starts the one agent that ever ran
    fx.ask("Two.");
    fx.turns_ended(2);
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 1);
    let texts: Vec<Value> = fx
        .events
        .of_type("text")
        .iter()
        .map(|e| e["text"].clone())
        .collect();
    assert_eq!(texts, [json!("Two.")]);
}

/// After Stop the agent may still ask before it ends its turn: answered `cancelled` at once, no card.
#[test]
fn a_question_after_stop_is_answered_cancelled_without_a_card() {
    let fx = Fx::new(|peer, _| {
        peer.text("On it.");
        peer.until(|m| m["method"] == "session/cancel")?;
        peer.ask(
            &json!(11),
            &json!({ "toolCallId": "toolu_bash", "title": "Clean up", "kind": "execute",
                     "rawInput": { "command": "rm -rf build" } }),
        );
        peer.until(|m| m["id"] == 11)?;
        Some("cancelled")
    });
    fx.ask("Build it.");
    fx.events.wait_for("reply", WAIT, |list| {
        list.iter().any(|e| e["type"] == "text")
    });
    fx.session.cancel();
    assert_eq!(fx.events.turn_end()["stopReason"], "cancelled");
    assert_eq!(fx.events.of_type("permission"), Vec::<Value>::new());
    assert_eq!(
        fx.fake.reply_to(&json!(11))["result"],
        json!({ "outcome": { "outcome": "cancelled" } })
    );
}

/// An agent answering a cancelled prompt with an error instead of its stop reason: the turn still ends `cancelled`.
#[test]
fn an_error_answer_to_a_cancelled_prompt_ends_it_as_cancelled() {
    let fx = Fx::new(|peer, message| {
        let id = message["id"].clone();
        peer.text("On it.");
        peer.until(|m| m["method"] == "session/cancel")?;
        peer.send(&json!({
            "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": "aborted" },
        }));
        Some(ANSWERED)
    });
    fx.ask("Build it.");
    fx.events.wait_for("reply", WAIT, |list| {
        list.iter().any(|e| e["type"] == "text")
    });
    fx.session.cancel();
    assert_eq!(fx.events.turn_end()["stopReason"], "cancelled");
    let last = fx.events.all().last().cloned().unwrap();
    assert_eq!(
        last,
        json!({ "type": "status", "state": "ready", "error": null })
    );
}

/// A stop reason, a tool kind of a later protocol release: the turn ends normally (`other`), the tool shows as
/// `other`; a refused prompt says the agent's own words.
#[test]
fn news_of_a_later_release_reads_as_other_and_a_refusal_keeps_its_words() {
    let fx = Fx::new(|peer, message| {
        if prompt_text(message) == "Refuse." {
            let id = message["id"].clone();
            peer.send(&json!({
                "jsonrpc": "2.0", "id": id, "error": { "code": -32602, "message": "Too long." },
            }));
            return Some(ANSWERED);
        }
        peer.update(&json!({
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Plan", "kind": "future_kind",
        }));
        Some("future_reason")
    });
    fx.ask("One.");
    let end = fx.events.turn_end();
    assert_eq!(end["stopReason"], "other");
    assert_eq!(fx.events.of_type("tool")[0]["kind"], "other");
    assert_eq!(fx.events.all().last().unwrap()["error"], Value::Null);
    fx.ask("Refuse.");
    let list = fx.turns_ended(2);
    assert_eq!(
        list.iter().rfind(|e| e["type"] == "turnEnd").unwrap()["stopReason"],
        "error"
    );
    assert_eq!(
        list.last().unwrap()["error"],
        json!({ "code": "refused", "message": "Too long." })
    );
}

/// A passage of a file agents may not see never reaches the agent (refused before anything starts); such an open
/// document | one the copy leaves out is not named.
#[test]
fn a_passage_of_a_hidden_file_is_refused_and_a_hidden_document_is_not_named() {
    let fx = Fx::new(|_, _| Some("end_turn"));
    std::fs::write(fx.project.path().join(".aiignore"), "secret.md\n").unwrap();
    std::fs::write(fx.project.path().join("secret.md"), "the key\n").unwrap();
    let passage = |path: &str| PromptArgs {
        text: "What is this?".to_owned(),
        selection: Some(PromptSelection {
            path: path.to_owned(),
            from_line: 1,
            to_line: 1,
            text: "the key".to_owned(),
        }),
        document: None,
    };
    for hidden in ["secret.md", ".git/config"] {
        let refused = fx.session.prompt(passage(hidden)).unwrap_err();
        assert_eq!(refused.kind, ErrorKind::Denied, "{hidden}");
        assert_eq!(refused.detail.as_deref(), Some("hiddenFromAgents"));
    }
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 0);

    for (round, document) in ["secret.md", ".claude/settings.json"].iter().enumerate() {
        fx.session
            .prompt(PromptArgs {
                text: "Hello.".to_owned(),
                selection: None,
                document: Some((*document).to_owned()),
            })
            .unwrap();
        fx.turns_ended(round + 1);
    }
    let prompts: Vec<Value> = fx
        .fake
        .seen()
        .into_iter()
        .filter(|m| m["method"] == "session/prompt")
        .map(|m| m["params"]["prompt"].clone())
        .collect();
    assert_eq!(
        prompts,
        [
            json!([{ "type": "text", "text": "Hello." }]),
            json!([{ "type": "text", "text": "Hello." }])
        ]
    );
}

/// An option kind of a later protocol release does not refuse the question: it shows as `other`, the person may pick
/// it, and the rule never does.
#[test]
fn an_option_kind_of_a_later_release_reads_as_other_and_is_never_chosen_by_rule() {
    let fx = Fx::new(|peer, _| {
        let notes = peer.in_copy("notes.md");
        for (id, offered) in [
            (
                1,
                [("always-here", "allow_project"), ("reject", "reject_once")],
            ),
            (
                2,
                [
                    ("always-here", "allow_project"),
                    ("allow-once", "allow_once"),
                ],
            ),
        ] {
            let options: Vec<Value> = offered
                .iter()
                .map(|(option, kind)| json!({ "optionId": option, "name": option, "kind": kind }))
                .collect();
            peer.send(&json!({
                "jsonrpc": "2.0", "id": id, "method": "session/request_permission",
                "params": { "sessionId": SESSION, "toolCall": edit_of(&notes), "options": options },
            }));
            peer.until(|m| m["id"] == id && m.get("method").is_none())?;
        }
        Some("end_turn")
    });
    fx.ask("Edit the notes.");
    // plain work, but no option of a known allowing kind: a card
    let card = fx.card("1:1");
    assert_eq!(
        card["options"],
        json!([
            { "id": "always-here", "name": "always-here", "kind": "other" },
            { "id": "reject", "name": "reject", "kind": "reject_once" },
        ])
    );
    fx.session
        .answer(&AnswerArgs {
            request_id: "1:1".to_owned(),
            option_id: Some("always-here".to_owned()),
        })
        .unwrap();
    fx.events.turn_end();
    assert_eq!(
        fx.fake.reply_to(&json!(1))["result"],
        json!({ "outcome": { "outcome": "selected", "optionId": "always-here" } })
    );
    // beside an allow-once option it is never the one chosen by rule
    assert_eq!(
        fx.fake.reply_to(&json!(2))["result"],
        json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } })
    );
    assert_eq!(fx.events.of_type("permission").len(), 1);
}

/// A card is told while its question is held under the session's lock: a Stop | a turn's end that answers the
/// question takes that lock first, so its `permissionDone` always comes after the card.
#[test]
fn a_card_is_told_before_anything_can_answer_its_question() {
    let fx = Fx::new(|peer, _| {
        peer.ask(
            &json!(6),
            &json!({ "toolCallId": "toolu_bash", "title": "Run the tests", "kind": "execute",
                     "rawInput": { "command": "npm test" } }),
        );
        peer.until(|m| m["id"] == 6 && m.get("method").is_none())?;
        Some("end_turn")
    });
    let core = Arc::downgrade(&fx.session.core);
    let held = Arc::new(Watched::<Vec<bool>>::default());
    let seen = Arc::clone(&held);
    *lock(&fx.events.spy) = Some(Box::new(move |event| {
        if event["type"] == "permission" {
            let locked = core
                .upgrade()
                .is_some_and(|core| core.inner.try_lock().is_err());
            seen.update(|list| list.push(locked));
        }
    }));
    fx.ask("Run the tests.");
    fx.card("1:6");
    fx.session.cancel();
    fx.events.turn_end();
    assert_eq!(held.get(), [true]);
    let cards: Vec<Value> = fx
        .events
        .all()
        .iter()
        .filter(|e| e["type"] == "permission" || e["type"] == "permissionDone")
        .map(|e| e["type"].clone())
        .collect();
    assert_eq!(cards, [json!("permission"), json!("permissionDone")]);
}

/// The card ids of two agents never meet: a fresh agent numbers its questions from the start again.
#[test]
fn question_ids_differ_across_agent_restarts() {
    let fx = Fx::new(|peer, _| {
        peer.ask(
            &json!(0),
            &json!({ "toolCallId": "toolu_bash", "title": "Run the tests", "kind": "execute",
                     "rawInput": { "command": "npm test" } }),
        );
        peer.until(|m| m["id"] == 0 && m.get("method").is_none())?;
        Some("end_turn")
    });
    fx.ask("One.");
    assert_eq!(fx.permission()["requestId"], "1:0");
    fx.session.reset();
    fx.fake.stopped(1);
    fx.ask("Two.");
    let list = fx.events.wait_for("second card", WAIT, |list| {
        list.iter().filter(|e| e["type"] == "permission").count() == 2
    });
    let second = list
        .iter()
        .filter(|e| e["type"] == "permission")
        .nth(1)
        .unwrap()["requestId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(second, "1:0");
    assert!(second.ends_with(":0"), "{second}");
    // the first agent's question is long answered: only the second one's card takes the answer
    let stale = fx.session.answer(&AnswerArgs {
        request_id: "1:0".to_owned(),
        option_id: Some("reject".to_owned()),
    });
    assert_eq!(stale.unwrap_err().detail.as_deref(), Some("noQuestion"));
    fx.reject(&second);
    fx.events.turn_end();
}

/// An agent offering its modes only as a config option (no `modes`) is set to the asking mode through it.
#[test]
fn an_agent_with_its_modes_as_a_config_option_is_set_to_ask() {
    let fx = Fx::new(|_, _| Some("end_turn"));
    fx.fake.config_modes.store(true, Ordering::SeqCst);
    fx.ask("Hello.");
    fx.events.turn_end();
    assert_eq!(
        fx.fake.methods(),
        [
            "initialize",
            "session/new",
            "session/set_config_option",
            "session/prompt"
        ]
    );
    let set = fx
        .fake
        .seen()
        .into_iter()
        .find(|m| m["method"] == "session/set_config_option")
        .unwrap();
    assert_eq!(
        set["params"],
        json!({ "sessionId": SESSION, "configId": "mode", "value": "default" })
    );
}

#[test]
fn the_passage_mime_type_follows_the_file() {
    assert_eq!(passage_mime("notes.md"), "text/markdown");
    assert_eq!(passage_mime("docs/Notes.MARKDOWN"), "text/markdown");
    assert_eq!(passage_mime("data.json"), "text/plain");
    assert_eq!(passage_mime("notes.md.txt"), "text/plain");
    assert_eq!(passage_mime("README"), "text/plain");
}

/// A conversation as an agent replays it at `session/load`: the person's message (its text, then the attached
/// passage), a thought, a tool line, the reply, its title; and a question it asks during the replay.
fn replay_of_one_turn() -> Vec<Value> {
    let update = |update: Value| {
        json!({
            "jsonrpc": "2.0", "method": "session/update",
            "params": { "sessionId": SESSION, "update": update },
        })
    };
    vec![
        update(
            json!({ "sessionUpdate": "user_message_chunk", "content": { "type": "text", "text": "One." } }),
        ),
        update(json!({
            "sessionUpdate": "user_message_chunk",
            "content": { "type": "resource", "resource": { "uri": "file:///copy/notes.md#L1:1", "text": "Line one." } },
        })),
        update(
            json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "hm" } }),
        ),
        update(json!({
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Read notes.md", "kind": "read",
            "status": "completed",
        })),
        update(
            json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "ok" } }),
        ),
        update(json!({ "sessionUpdate": "session_info_update", "title": "Notes again" })),
        json!({
            "jsonrpc": "2.0", "id": 77, "method": "session/request_permission",
            "params": {
                "sessionId": SESSION,
                "toolCall": { "toolCallId": "t2", "title": "Run it", "kind": "execute" },
                "options": [{ "optionId": "allow-once", "name": "Yes", "kind": "allow_once" }],
            },
        }),
    ]
}

impl Fx {
    /// Continues the last conversation; answers what the window was told until the agent is ready again.
    fn continue_last(&self) -> Vec<Value> {
        let before = self.events.all().len();
        self.session.continue_last().unwrap();
        let list = self.events.wait_for("ready again", WAIT, |list| {
            list.len() > before
                && list
                    .last()
                    .is_some_and(|e| e["type"] == "status" && e["state"] == "ready")
        });
        list[before..].to_vec()
    }

    /// Ends the conversation (New conversation): its agent stops (the `count`th stop).
    fn leave(&self, count: usize) {
        self.session.reset();
        self.fake.stopped(count);
    }
}

fn status(state: &str) -> Value {
    json!({ "type": "status", "state": state, "error": null })
}

/// The project's last conversation is kept in the profile (its agent, id, title) and continued with `session/load`:
/// the agent replays it (the person's messages, its replies, tool lines; never a question to answer again), the
/// conversation is set to ask again, and no message is sent.
#[test]
fn the_last_conversation_is_kept_and_continued_with_its_history() {
    let fx = Fx::new(|peer, _| {
        peer.update(&json!({
            "sessionUpdate": "session_info_update", "title": "Notes", "updatedAt": "2026-11-17T23:32:36.340Z",
        }));
        peer.text("ok");
        Some("end_turn")
    });
    assert_eq!(fx.session.status().last, None);
    fx.ask("One.");
    fx.events.turn_end();
    let saved = fx.saved().unwrap();
    assert_eq!(saved.agent, Choice::Agent("claude-acp".to_owned()));
    assert_eq!(saved.session_id, SESSION);
    assert_eq!(saved.title.as_deref(), Some("Notes"));
    assert!(!saved.had_changes);
    assert_eq!(saved.way, Some(ContinueWay::Load));
    assert!(saved_file(fx.profile.path())
        .path()
        .starts_with(fx.profile.path()));
    let last = fx.session.status().last.unwrap();
    assert_eq!(last.title.as_deref(), Some("Notes"));
    assert_eq!(last.supports, ContinueWay::Load);
    assert!(last.at > 0);
    // after a new conversation, the one just left is offered
    fx.leave(1);
    assert_eq!(
        fx.session.status().last.map(|last| last.supports),
        Some(ContinueWay::Load)
    );

    *fx.fake.story.replay.lock().unwrap() = replay_of_one_turn();
    let told = fx.continue_last();
    assert_eq!(
        told,
        [
            status("starting"),
            json!({ "type": "you", "text": "One." }),
            json!({ "type": "tool", "id": "t1", "title": "Read notes.md", "kind": "read", "status": "completed" }),
            json!({ "type": "text", "text": "ok" }),
            json!({ "type": "restored", "replayed": true, "changesLost": false }),
            status("ready"),
        ]
    );
    assert_eq!(
        fx.fake.reply_to(&json!(77))["result"],
        json!({ "outcome": { "outcome": "cancelled" } })
    );
    assert_eq!(fx.session.status().state, AcpState::Ready);
    let methods = fx.fake.methods();
    assert_eq!(
        methods[methods.len() - 3..],
        ["initialize", "session/load", "session/set_mode"]
    );
    let load = fx
        .fake
        .seen()
        .into_iter()
        .find(|m| m["method"] == "session/load")
        .unwrap();
    assert_eq!(load["params"]["sessionId"], SESSION);
    assert_eq!(
        PathBuf::from(load["params"]["cwd"].as_str().unwrap()),
        fx.copy_dir()
    );
    assert_eq!(load["params"]["mcpServers"], json!([]));
    assert_eq!(
        load["params"]["_meta"]["claudeCode"]["options"]["allowDangerouslySkipPermissions"],
        json!(false)
    );
    // a title told during the replay is kept
    assert_eq!(fx.saved().unwrap().title.as_deref(), Some("Notes again"));

    // the next message goes on in the same conversation
    fx.ask("Two.");
    fx.turns_ended(2);
    let prompt = fx
        .fake
        .seen()
        .into_iter()
        .rfind(|m| m["method"] == "session/prompt")
        .unwrap();
    assert_eq!(prompt["params"]["sessionId"], SESSION);
    assert_eq!(prompt_text(&prompt), "Two.");
    assert_eq!(fx.fake.launches.load(Ordering::SeqCst), 2);
    assert_eq!(fx.fake.count("session/new"), 1);
}

/// An agent that cannot replay a conversation but can resume one: `session/resume`, and the window is told that
/// nothing was replayed.
#[test]
fn an_agent_without_load_resumes_the_conversation() {
    let fx = Fx::new(|_, _| Some("end_turn"));
    fx.fake.story.no_load.store(true, Ordering::SeqCst);
    fx.fake.story.resume.store(true, Ordering::SeqCst);
    fx.ask("One.");
    fx.events.turn_end();
    let last = fx.session.status().last.unwrap();
    assert_eq!(last.supports, ContinueWay::Resume);
    assert_eq!(last.title, None);
    fx.leave(1);
    // never sent: a resume replays nothing
    *fx.fake.story.replay.lock().unwrap() = replay_of_one_turn();
    assert_eq!(
        fx.continue_last(),
        [
            status("starting"),
            json!({ "type": "restored", "replayed": false, "changesLost": false }),
            status("ready"),
        ]
    );
    let methods = fx.fake.methods();
    assert_eq!(
        methods[methods.len() - 3..],
        ["initialize", "session/resume", "session/set_mode"]
    );
    assert_eq!(fx.fake.count("session/prompt"), 1);
}

/// A conversation the agent no longer has: said with the agent's words, forgotten; the next message starts a new
/// one, which is kept instead.
#[test]
fn a_conversation_the_agent_no_longer_has_is_forgotten() {
    let fx = Fx::new(|peer, _| {
        peer.text("ok");
        Some("end_turn")
    });
    fx.ask("One.");
    fx.events.turn_end();
    fx.leave(1);
    fx.fake.story.gone.store(true, Ordering::SeqCst);
    fx.session.continue_last().unwrap();
    let list = fx.events.wait_for("error", WAIT, |list| {
        list.iter().any(|e| e["state"] == "error")
    });
    let failed = list.iter().find(|e| e["state"] == "error").unwrap();
    assert_eq!(
        failed["error"],
        json!({ "code": "sessionGone", "message": "Resource not found: no conversation 44453ef7" })
    );
    fx.fake.stopped(2);
    assert_eq!(fx.saved(), None);
    assert_eq!(fx.session.status().last, None);
    let refused = fx.session.continue_last().unwrap_err();
    assert_eq!(refused.kind, ErrorKind::NotFound);
    assert_eq!(refused.detail.as_deref(), Some("noSaved"));
    fx.ask("Two.");
    fx.turns_ended(2);
    assert_eq!(fx.saved().unwrap().session_id, SESSION);
}

/// An agent that wants a sign-in before it continues a conversation: the sign-in, then the conversation is continued
/// (no message sent).
#[test]
fn continuing_waits_for_a_sign_in_and_goes_on_after_it() {
    let fx = Fx::new(|_, _| Some("end_turn"));
    fx.ask("One.");
    fx.events.turn_end();
    fx.leave(1);
    *fx.fake.auth.methods.lock().unwrap() =
        vec![json!({ "id": "oauth", "name": "Sign in with the browser" })];
    fx.fake.auth.required.store(true, Ordering::SeqCst);
    fx.session.continue_last().unwrap();
    fx.events.wait_for("sign-in", WAIT, |list| {
        list.iter().any(|e| e["state"] == "needsLogin")
    });
    assert_eq!(fx.session.status().state, AcpState::NeedsLogin);
    fx.session
        .login(&LoginArgs {
            method_id: Some("oauth".to_owned()),
        })
        .unwrap();
    fx.events.wait_for("ready", WAIT, |list| {
        list.last()
            .is_some_and(|e| e["type"] == "status" && e["state"] == "ready")
    });
    let methods = fx.fake.methods();
    assert_eq!(
        methods[methods.len() - 5..],
        [
            "initialize",
            "session/load",
            "authenticate",
            "session/load",
            "session/set_mode"
        ]
    );
    assert_eq!(fx.fake.count("session/prompt"), 1);
    assert_eq!(fx.events.of_type("restored").len(), 1);
}

/// The last conversation is offered only while its agent is the chosen one and can continue it; a new conversation
/// takes its place at its first message.
#[test]
fn the_last_conversation_is_offered_only_for_its_agent() {
    let fx = Fx::new(|_, _| Some("end_turn"));
    let refused = fx.session.continue_last().unwrap_err();
    assert_eq!(refused.kind, ErrorKind::NotFound);
    assert_eq!(refused.detail.as_deref(), Some("noSaved"));
    fx.ask("One.");
    fx.events.turn_end();
    fx.session.choose(&choose_agent("codex-acp")).unwrap();
    assert_eq!(fx.session.status().last, None);
    let refused = fx.session.continue_last().unwrap_err();
    assert_eq!(refused.detail.as_deref(), Some("noSaved"));
    fx.session.choose(&choose_agent("claude-acp")).unwrap();
    assert!(fx.session.status().last.is_some());
    // an agent that can neither load nor resume: its conversation replaces the one kept and is never offered
    fx.fake.story.no_load.store(true, Ordering::SeqCst);
    fx.ask("Two.");
    fx.turns_ended(2);
    assert_eq!(fx.saved().unwrap().way, None);
    assert_eq!(fx.session.status().last, None);
}

/// Changes the agent left for the person are noted with the conversation. Continued on the same copy they are still
/// there; continued on a copy made anew (the plugin stopped meanwhile), the window is told they were not kept, once.
#[test]
fn unaccepted_changes_of_a_conversation_continued_on_a_new_copy_are_said_lost() {
    let fx = Fx::new(|peer, _| {
        peer.write("notes.md", "Changed by the agent.\n");
        Some("end_turn")
    });
    fx.ask("Edit.");
    assert_eq!(fx.events.turn_end()["changed"], 1);
    assert!(fx.saved().unwrap().had_changes);
    fx.leave(1);
    let restored = |told: &[Value]| {
        told.iter()
            .find(|e| e["type"] == "restored")
            .map(|e| e["changesLost"].clone())
    };
    assert_eq!(restored(&fx.continue_last()), Some(json!(false)));
    assert!(fx.saved().unwrap().had_changes);
    // the plugin stopped: the copy went with its changes
    fx.session.shutdown();
    fx.copy.lock().unwrap().take().unwrap().remove().unwrap();
    assert_eq!(restored(&fx.continue_last()), Some(json!(true)));
    assert!(!fx.saved().unwrap().had_changes);
    fx.leave(3);
    assert_eq!(restored(&fx.continue_last()), Some(json!(false)));
}
