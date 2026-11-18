//! JSON-RPC 2.0 with an agent process, one JSON message per line (Agent Client Protocol framing), on plain threads.
//!
//! - one reader thread dispatches each line by its shape: answer to one of our calls (by id) | notification | the
//!   agent's own request (the [`Inbox`] answers it, at once or later through [`Connection::reply`]).
//! - each message is decoded on its own: a line that is no JSON-RPC message, or one longer than [`LINE_CAP`], is
//!   skipped, never fatal (agents add kinds of news faster than any client learns them).
//! - an error answer keeps its JSON-RPC code ([`CallError::Refused`]): `auth_required` asks the person to sign in.
//! - writes: one writer thread takes whole lines from a queue, in order, and flushes each; a caller only queues (a full
//!   pipe of an agent that stopped reading never holds up a stop, a cancel or the reader thread's replies).
//! - the agent's stderr is drained into a short tail ([`TAIL_CAP`]) for error reports, cleaned as it comes: terminal
//!   color | cursor codes and other control characters removed, lines naming account news dropped; it is never
//!   written anywhere.
//! - end of the agent's output (it exited) | [`Connection::close`] -> every waiting call answers `Closed`.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use agent_client_protocol_schema::v1::{
    Error as RpcError, ErrorCode, JsonRpcMessage, Notification, Request, RequestId, Response,
};
use permark_core::settings::is_unsafe_char;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Longest line read from the agent (a file the agent read can ride along in a tool update); longer -> skipped.
pub const LINE_CAP: u64 = 16 << 20;
/// Bytes of the agent's stderr kept (the newest).
pub const TAIL_CAP: usize = 8 << 10;
/// Longest stderr line kept as one (longer ones are cut into several).
const TAIL_LINE_CAP: u64 = 4 << 10;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A started agent's standard streams + how to stop it.
pub struct AgentPipes {
    pub to_agent: Box<dyn Write + Send>,
    pub from_agent: Box<dyn Read + Send>,
    pub stderr: Box<dyn Read + Send>,
    /// Ends the agent, called once after its stdin closed: waits a short grace for its own exit, then kills it.
    pub stop: Box<dyn FnOnce() + Send>,
}

/// What the reader thread hands on. Runs on the reader thread: quick, never waiting on the agent.
pub trait Inbox: Send + Sync + 'static {
    /// The agent tells something (`connection`: to answer a request the notice withdraws).
    fn notification(&self, connection: &Connection, method: &str, params: Option<Value>);
    /// The agent asks something; every request MUST get one [`Connection::reply`] (now or later).
    fn request(&self, connection: &Connection, id: RequestId, method: &str, params: Option<Value>);
    /// The agent's output ended (it exited | was stopped).
    fn closed(&self);
}

/// Why a call has no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// The agent is gone (exited | stopped) before it answered.
    Closed,
    /// No answer within the wait.
    TimedOut,
    /// The agent answered with an error: its JSON-RPC code + message.
    Refused { code: i32, message: String },
    /// The answer is not the expected shape.
    Unreadable,
}

impl CallError {
    /// The agent wants the person to sign in first (`auth_required`).
    #[must_use]
    pub fn needs_login(&self) -> bool {
        matches!(self, Self::Refused { code, .. } if *code == i32::from(ErrorCode::AuthRequired))
    }
}

/// Answer to one call.
enum Answer {
    Done(Value),
    Failed { code: i32, message: String },
}

#[derive(Default)]
struct Waiting {
    closed: bool,
    calls: HashMap<i64, SyncSender<Answer>>,
}

/// One line from the agent, read by shape only (the inbox decodes `params` by method).
#[derive(Deserialize)]
struct Incoming {
    #[serde(default)]
    id: Option<RequestId>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    /// Kept loose: an odd error object still ends the call it answers.
    #[serde(default)]
    error: Option<Value>,
}

/// The connection to one running agent.
pub struct Connection {
    /// Lines for the writer thread (the only writer of the agent's stdin); `None` once closed.
    outbox: Mutex<Option<Sender<Vec<u8>>>>,
    waiting: Mutex<Waiting>,
    next: AtomicI64,
    tail: Tail,
}

impl Connection {
    /// Starts the writer, reader and stderr threads.
    ///
    /// # Errors
    /// A thread cannot be started.
    pub fn start(
        to_agent: Box<dyn Write + Send>,
        from_agent: Box<dyn Read + Send>,
        stderr: Box<dyn Read + Send>,
        inbox: Arc<dyn Inbox>,
    ) -> io::Result<Arc<Self>> {
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let (lines, queued) = mpsc::channel();
        let connection = Arc::new(Self {
            outbox: Mutex::new(Some(lines)),
            waiting: Mutex::new(Waiting::default()),
            next: AtomicI64::new(1),
            tail: Tail {
                lines: Mutex::new((VecDeque::new(), 0)),
                done: Mutex::new(done_rx),
            },
        });
        let writer = Arc::downgrade(&connection);
        std::thread::Builder::new()
            .name("acp-agent-in".into())
            .spawn(move || write_loop(&queued, to_agent, &writer))?;
        let reader = Arc::clone(&connection);
        let read = std::thread::Builder::new()
            .name("acp-agent-out".into())
            .spawn(move || reader.read_loop(from_agent, inbox.as_ref()));
        if let Err(error) = read {
            connection.close();
            return Err(error);
        }
        let drain = Arc::clone(&connection);
        let drained = std::thread::Builder::new()
            .name("acp-agent-err".into())
            .spawn(move || {
                drain.tail.drain(stderr);
                // dropping the sender tells `settle` the stream ended
                drop(done_tx);
            });
        if let Err(error) = drained {
            connection.close();
            return Err(error);
        }
        Ok(connection)
    }

    fn read_loop(&self, from: Box<dyn Read + Send>, inbox: &dyn Inbox) {
        let mut from = BufReader::new(from);
        let mut line = Vec::new();
        loop {
            match next_line(&mut from, &mut line) {
                Ok(Line::Full) => {}
                Ok(Line::TooLong) => continue,
                Ok(Line::End) | Err(_) => break,
            }
            // not a JSON-RPC message: skipped (its content is never logged: it may carry the person's account)
            let Ok(message) = serde_json::from_slice::<Incoming>(&line) else {
                continue;
            };
            self.dispatch(message, inbox);
        }
        self.close_calls();
        inbox.closed();
    }

    fn dispatch(&self, message: Incoming, inbox: &dyn Inbox) {
        match (message.method, message.id) {
            (Some(method), Some(id)) => inbox.request(self, id, &method, message.params),
            (Some(method), None) => inbox.notification(self, &method, message.params),
            (None, Some(RequestId::Number(id))) => {
                let answer = match message.error {
                    Some(error) => Answer::Failed {
                        // no | odd code: an internal error of the agent
                        code: error
                            .get("code")
                            .and_then(Value::as_i64)
                            .and_then(|code| i32::try_from(code).ok())
                            .unwrap_or_else(|| i32::from(ErrorCode::InternalError)),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("error")
                            .to_owned(),
                    },
                    // `"result": null` reads as absent: still an answer
                    None => Answer::Done(message.result.unwrap_or(Value::Null)),
                };
                if let Some(waiter) = lock(&self.waiting).calls.remove(&id) {
                    let _ = waiter.send(answer);
                }
            }
            // an answer to no call of ours
            _ => {}
        }
    }

    /// Calls `method` and waits for its answer (`None`: as long as the agent runs).
    ///
    /// # Errors
    /// [`CallError`].
    pub fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
        wait: Option<Duration>,
    ) -> Result<R, CallError> {
        self.request(method, params)?.wait(wait)
    }

    /// Queues the call of `method` (never waits on the agent: the caller may hold a lock that orders it against
    /// other lines); [`Pending::wait`] takes its answer.
    ///
    /// # Errors
    /// `Closed`: the agent is gone | its stdin closed.
    pub fn request<P: Serialize>(&self, method: &str, params: P) -> Result<Pending<'_>, CallError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut waiting = lock(&self.waiting);
            if waiting.closed {
                return Err(CallError::Closed);
            }
            waiting.calls.insert(id, tx);
        }
        let request = Request {
            id: RequestId::Number(id),
            method: method.into(),
            params: Some(params),
        };
        if self.send(&JsonRpcMessage::wrap(request)).is_err() {
            lock(&self.waiting).calls.remove(&id);
            return Err(CallError::Closed);
        }
        Ok(Pending {
            connection: self,
            id,
            answer: rx,
        })
    }

    /// Sends a notification.
    ///
    /// # Errors
    /// The agent's stdin is closed | broken.
    pub fn notify<P: Serialize>(&self, method: &str, params: P) -> io::Result<()> {
        self.send(&JsonRpcMessage::wrap(Notification {
            method: method.into(),
            params: Some(params),
        }))
    }

    /// Answers the agent's request `id`.
    ///
    /// # Errors
    /// The agent's stdin is closed | broken.
    pub fn reply<R: Serialize>(
        &self,
        id: RequestId,
        answer: Result<R, RpcError>,
    ) -> io::Result<()> {
        self.send(&JsonRpcMessage::wrap(Response::new(id, answer)))
    }

    /// Queues one line for the writer thread (never waits on the agent).
    fn send(&self, message: &impl Serialize) -> io::Result<()> {
        let mut line = serde_json::to_vec(message).map_err(io::Error::other)?;
        line.push(b'\n');
        let closed = || io::Error::from(io::ErrorKind::BrokenPipe);
        lock(&self.outbox)
            .as_ref()
            .ok_or_else(closed)?
            .send(line)
            .map_err(|_| closed())
    }

    /// Ends every waiting call (`Closed`), even while the agent's output stays open (a grandchild holding it), and
    /// closes the agent's stdin once the lines already queued are written (an ACP agent exits on it). Never waits on
    /// the agent: a writer stuck on a full pipe ends when the agent is stopped.
    pub fn close(&self) {
        drop(lock(&self.outbox).take());
        self.close_calls();
    }

    fn close_calls(&self) {
        let mut waiting = lock(&self.waiting);
        waiting.closed = true;
        waiting.calls.clear();
    }

    /// The newest `max_bytes` (whole characters) of the agent's cleaned stderr, after waiting up to `wait` for the
    /// stream to end (an exiting agent writes its last words just before).
    #[must_use]
    pub fn stderr_tail(&self, max_bytes: usize, wait: Duration) -> String {
        self.tail.settle(wait);
        self.tail.text(max_bytes)
    }
}

/// A call on its way: its answer comes once.
pub struct Pending<'a> {
    connection: &'a Connection,
    id: i64,
    answer: Receiver<Answer>,
}

impl Pending<'_> {
    /// Waits for the answer (`None`: as long as the agent runs).
    ///
    /// # Errors
    /// [`CallError`].
    pub fn wait<R: DeserializeOwned>(self, wait: Option<Duration>) -> Result<R, CallError> {
        let answer = match wait {
            Some(wait) => self.answer.recv_timeout(wait).map_err(|e| match e {
                RecvTimeoutError::Timeout => CallError::TimedOut,
                RecvTimeoutError::Disconnected => CallError::Closed,
            }),
            None => self.answer.recv().map_err(|_| CallError::Closed),
        };
        if answer.is_err() {
            // a late answer finds no call: dropped
            lock(&self.connection.waiting).calls.remove(&self.id);
        }
        match answer? {
            Answer::Done(value) => serde_json::from_value(value).map_err(|_| CallError::Unreadable),
            Answer::Failed { code, message } => Err(CallError::Refused { code, message }),
        }
    }
}

/// The writer thread: each queued line in order, flushed. Ends once the queue's sender is gone ([`Connection::close`])
/// and the queue is written, then closes the agent's stdin | when the stdin broke (every waiting call then answers
/// `Closed`: nothing more reaches the agent).
fn write_loop(
    queued: &Receiver<Vec<u8>>,
    mut to: Box<dyn Write + Send>,
    connection: &Weak<Connection>,
) {
    for line in queued {
        if to.write_all(&line).and_then(|()| to.flush()).is_err() {
            if let Some(connection) = connection.upgrade() {
                connection.close();
            }
            return;
        }
    }
}

/// Outcome of reading one line.
enum Line {
    Full,
    /// Past [`LINE_CAP`]: dropped whole.
    TooLong,
    End,
}

/// Reads one line (without its `\n`) into `line`.
fn next_line(from: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<Line> {
    line.clear();
    let read = from.by_ref().take(LINE_CAP).read_until(b'\n', line)?;
    if read == 0 {
        return Ok(Line::End);
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        return Ok(Line::Full);
    }
    if u64::try_from(read).unwrap_or(u64::MAX) < LINE_CAP {
        // cut off by the end of the stream
        return Ok(Line::End);
    }
    // past the cap: the rest of the line goes unread
    loop {
        let buffer = from.fill_buf()?;
        if buffer.is_empty() {
            return Ok(Line::End);
        }
        if let Some(at) = buffer.iter().position(|b| *b == b'\n') {
            from.consume(at + 1);
            return Ok(Line::TooLong);
        }
        let all = buffer.len();
        from.consume(all);
    }
}

/// The newest stderr lines, at most [`TAIL_CAP`] bytes.
struct Tail {
    /// Lines + their bytes (each counted with its break).
    lines: Mutex<(VecDeque<String>, usize)>,
    /// Disconnected once the stream ended.
    done: Mutex<Receiver<()>>,
}

impl Tail {
    fn drain(&self, stderr: Box<dyn Read + Send>) {
        let mut reader = BufReader::new(stderr);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader
                .by_ref()
                .take(TAIL_LINE_CAP)
                .read_until(b'\n', &mut line)
            {
                Ok(0) | Err(_) => return,
                Ok(_) => self.add(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n'])),
            }
        }
    }

    fn add(&self, line: &str) {
        let Some(line) = readable(line) else {
            return;
        };
        let mut guard = lock(&self.lines);
        let (lines, bytes) = &mut *guard;
        *bytes += line.len() + 1;
        lines.push_back(line);
        while *bytes > TAIL_CAP {
            let Some(old) = lines.pop_front() else {
                break;
            };
            *bytes -= old.len() + 1;
        }
    }

    fn settle(&self, wait: Duration) {
        let _ = lock(&self.done).recv_timeout(wait);
    }

    fn text(&self, max_bytes: usize) -> String {
        let all = lock(&self.lines)
            .0
            .iter()
            .fold(String::new(), |mut all, line| {
                if !all.is_empty() {
                    all.push('\n');
                }
                all.push_str(line);
                all
            });
        let mut from = all.len().saturating_sub(max_bytes);
        while !all.is_char_boundary(from) {
            from += 1;
        }
        all[from..].to_owned()
    }
}

/// Words only account news carries: a line holding one is never kept (it may name the person's account).
const ACCOUNT_WORDS: [&str; 2] = ["status_update", "email"];

/// A stderr line as the person may read it: terminal escape sequences (colors, cursor moves, titles) and control
/// characters removed, tabs kept (`None`: a line naming account news, never kept).
fn readable(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    if ACCOUNT_WORDS.iter().any(|word| lower.contains(word)) {
        return None;
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            // CSI (`ESC [` | its one-character form): parameters up to a final character `@`..`~`
            '\u{1b}' if chars.clone().next() == Some('[') => {
                chars.next();
                skip_csi(&mut chars);
            }
            '\u{9b}' => skip_csi(&mut chars),
            // OSC (a window title, a link): up to BEL | ESC + `\`
            '\u{1b}' if chars.clone().next() == Some(']') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next().is_some()) {
                        break;
                    }
                }
            }
            // any other escape: its intermediate characters (space..`/`), then its final one
            '\u{1b}' => while chars.next().is_some_and(|c| (' '..='/').contains(&c)) {},
            '\t' => out.push(c),
            c if is_unsafe_char(c) => {}
            c => out.push(c),
        }
    }
    Some(out)
}

fn skip_csi(chars: &mut std::str::Chars<'_>) {
    for c in chars.by_ref() {
        if ('@'..='~').contains(&c) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn lines_of(input: &[u8]) -> Vec<Result<String, &'static str>> {
        let mut from = Cursor::new(input.to_vec());
        let mut line = Vec::new();
        let mut out = Vec::new();
        loop {
            match next_line(&mut from, &mut line).unwrap() {
                Line::Full => out.push(Ok(String::from_utf8(line.clone()).unwrap())),
                Line::TooLong => out.push(Err("too long")),
                Line::End => return out,
            }
        }
    }

    #[test]
    fn lines_are_split_on_breaks_and_a_cut_off_tail_is_the_end() {
        assert_eq!(
            lines_of(b"{\"a\":1}\n{\"b\":2}\n{\"c\""),
            vec![Ok("{\"a\":1}".to_owned()), Ok("{\"b\":2}".to_owned())]
        );
    }

    #[test]
    fn a_line_past_the_cap_is_skipped_and_reading_goes_on() {
        let cap = usize::try_from(LINE_CAP).unwrap();
        let mut input = vec![b'x'; cap + 10];
        input.extend_from_slice(b"\n{\"next\":true}\n");
        assert_eq!(
            lines_of(&input),
            vec![Err("too long"), Ok("{\"next\":true}".to_owned())]
        );
    }

    #[test]
    fn the_tail_keeps_the_newest_bytes() {
        let (_tx, rx) = mpsc::sync_channel(1);
        let tail = Tail {
            lines: Mutex::new((VecDeque::new(), 0)),
            done: Mutex::new(rx),
        };
        for n in 0..2000 {
            tail.add(&format!("line {n}"));
        }
        let kept = lock(&tail.lines).1;
        assert!(kept <= TAIL_CAP, "{kept}");
        assert!(tail.text(500).ends_with("line 1999"));
        assert_eq!(tail.text(9), "line 1999");
        // never half a character
        tail.add("é");
        assert_eq!(tail.text(1), "");
        assert_eq!(tail.text(2), "é");
    }

    #[test]
    fn stderr_lines_lose_terminal_codes_and_account_news() {
        assert_eq!(
            readable("\u{1b}[1;31mfatal:\u{1b}[0m the model\u{7} is gone\r").as_deref(),
            Some("fatal: the model is gone")
        );
        assert_eq!(
            readable("\u{1b}]0;window title\u{7}after\u{1b}]8;;https://x\u{1b}\\link").as_deref(),
            Some("afterlink")
        );
        assert_eq!(
            readable("\u{9b}2Kcleared\u{1b}(B\tkept").as_deref(),
            Some("cleared\tkept")
        );
        assert_eq!(
            readable("\u{202e}reversed\u{2066}").as_deref(),
            Some("reversed")
        );
        assert_eq!(
            readable("{\"method\":\"_auth/status_update\",\"params\":{}}"),
            None
        );
        assert_eq!(readable("  Email: person@example.com"), None);
    }

    /// Longest wait for the other side of the pipes.
    const WAIT: Duration = Duration::from_secs(20);

    /// What the reader thread handed on: `notification <method>` | `request <method> <id>` | `closed`.
    #[derive(Default)]
    struct Heard {
        list: Mutex<Vec<String>>,
        changed: std::sync::Condvar,
    }

    impl Heard {
        fn push(&self, what: String) {
            lock(&self.list).push(what);
            self.changed.notify_all();
        }

        fn wait_for(&self, what: &str) {
            let started = std::time::Instant::now();
            let mut list = lock(&self.list);
            while !list.iter().any(|seen| seen == what) {
                let left = WAIT.saturating_sub(started.elapsed());
                assert!(!left.is_zero(), "no {what}; heard {list:?}");
                list = self
                    .changed
                    .wait_timeout(list, left)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
    }

    impl Inbox for Heard {
        fn notification(&self, _: &Connection, method: &str, _: Option<Value>) {
            self.push(format!("notification {method}"));
        }

        fn request(&self, connection: &Connection, id: RequestId, method: &str, _: Option<Value>) {
            self.push(format!(
                "request {method} {}",
                serde_json::to_string(&id).unwrap()
            ));
            connection
                .reply::<Value>(id, Err(RpcError::method_not_found()))
                .unwrap();
        }

        fn closed(&self) {
            self.push("closed".to_owned());
        }
    }

    /// A connection over pipes with the agent's ends.
    struct Wire {
        connection: Arc<Connection>,
        to_client: io::PipeWriter,
        from_client: BufReader<io::PipeReader>,
        heard: Arc<Heard>,
        _stderr: io::PipeWriter,
    }

    impl Wire {
        fn new() -> Self {
            let (agent_in, client_out) = io::pipe().unwrap();
            let (client_in, agent_out) = io::pipe().unwrap();
            let (client_err, agent_err) = io::pipe().unwrap();
            let heard = Arc::new(Heard::default());
            let connection = Connection::start(
                Box::new(client_out),
                Box::new(client_in),
                Box::new(client_err),
                Arc::clone(&heard) as Arc<dyn Inbox>,
            )
            .unwrap();
            Self {
                connection,
                to_client: agent_out,
                from_client: BufReader::new(agent_in),
                heard,
                _stderr: agent_err,
            }
        }

        fn write(&mut self, text: &str) {
            self.to_client.write_all(text.as_bytes()).unwrap();
            self.to_client.flush().unwrap();
        }

        /// The client's next line (`None`: its stdin closed).
        fn next(&mut self) -> Option<Value> {
            let mut line = String::new();
            (self.from_client.read_line(&mut line).unwrap() > 0)
                .then(|| serde_json::from_str(&line).unwrap())
        }

        /// Calls `method` on a thread of its own; answers the id the agent sees.
        fn call(
            &mut self,
            method: &'static str,
            wait: Duration,
        ) -> (i64, std::thread::JoinHandle<Result<Value, CallError>>) {
            let connection = Arc::clone(&self.connection);
            let call = std::thread::spawn(move || connection.call(method, json!({}), Some(wait)));
            let request = self.next().unwrap();
            assert_eq!(request["method"], method);
            (request["id"].as_i64().unwrap(), call)
        }
    }

    use serde_json::json;

    #[test]
    fn an_answer_finds_its_call_in_every_shape_it_comes() {
        let mut wire = Wire::new();
        // split over two writes; an answer with no id | a string id | an unknown id answers nothing
        let (id, call) = wire.call("one", WAIT);
        wire.write(&format!(
            "{{\"jsonrpc\":\"2.0\",\"result\":1}}\n{{\"jsonrpc\":\"2.0\",\"id\":\"{id}\",\"result\":2}}\n"
        ));
        wire.write("{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":3}\n{\"jsonrpc\":\"2.0\",\"id\":");
        wire.write(&format!("{id},\"result\":null}}\n"));
        assert_eq!(call.join().unwrap(), Ok(Value::Null));

        // an error without a code: the agent's internal error
        let (id, call) = wire.call("two", WAIT);
        wire.write(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"message\":\"boom\"}}}}\n"
        ));
        assert_eq!(
            call.join().unwrap(),
            Err(CallError::Refused {
                code: i32::from(ErrorCode::InternalError),
                message: "boom".to_owned()
            })
        );
    }

    /// Ids are never reused: an answer that comes after its call gave up never reaches a later call, nor does a
    /// second answer to the same id.
    #[test]
    fn a_late_answer_never_reaches_a_later_call() {
        let mut wire = Wire::new();
        let (late, call) = wire.call("slow", Duration::from_millis(50));
        assert_eq!(call.join().unwrap(), Err(CallError::TimedOut));
        let (id, call) = wire.call("next", WAIT);
        assert_ne!(id, late);
        wire.write(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{late},\"result\":\"late\"}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":\"fresh\"}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":\"again\"}}\n"
        ));
        assert_eq!(call.join().unwrap(), Ok(json!("fresh")));
    }

    #[test]
    fn unknown_lines_are_skipped_and_the_rest_reaches_the_inbox() {
        let mut wire = Wire::new();
        let cap = usize::try_from(LINE_CAP).unwrap();
        let mut long = "x".repeat(cap + 1);
        long.push('\n');
        wire.write(&long);
        wire.write("not json\n{}\n[1,2]\n");
        wire.write("{\"jsonrpc\":\"2.0\",\"method\":\"future/news\",\"params\":{}}\n");
        wire.write("{\"jsonrpc\":\"2.0\",\"id\":\"q-1\",\"method\":\"fs/read_text_file\"}\n");
        wire.heard.wait_for("notification future/news");
        wire.heard.wait_for("request fs/read_text_file \"q-1\"");
        // the request got its one answer
        let reply = wire.next().unwrap();
        assert_eq!(reply["id"], "q-1");
        assert_eq!(reply["error"]["code"], -32601);
    }

    /// Close: every waiting call ends at once even while the agent's output stays open; the agent's stdin closes
    /// after the lines already queued.
    #[test]
    fn close_ends_every_waiting_call_and_the_agents_stdin() {
        let mut wire = Wire::new();
        let connection = Arc::clone(&wire.connection);
        let waiting =
            std::thread::spawn(move || connection.call::<_, Value>("hang", json!({}), None));
        assert_eq!(wire.next().unwrap()["method"], "hang");
        wire.connection.notify("last", json!({})).unwrap();
        wire.connection.close();
        assert_eq!(waiting.join().unwrap(), Err(CallError::Closed));
        assert_eq!(
            wire.connection.call::<_, Value>("after", json!({}), None),
            Err(CallError::Closed)
        );
        assert!(wire.connection.notify("after", json!({})).is_err());
        assert_eq!(wire.next().unwrap()["method"], "last");
        assert_eq!(wire.next(), None);
        // the output ending is told once
        drop(wire.to_client);
        wire.heard.wait_for("closed");
    }
}
