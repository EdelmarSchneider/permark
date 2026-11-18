//! JSON-RPC 2.0 with an agent process, one JSON message per line (Agent Client Protocol framing), on plain threads.
//!
//! - one reader thread dispatches each line by its shape: answer to one of our calls (by id) | notification | the
//!   agent's own request (the [`Inbox`] answers it, at once or later through [`Connection::reply`]).
//! - each message is decoded on its own: a line that is no JSON-RPC message, or one longer than [`LINE_CAP`], is
//!   skipped, never fatal (agents add kinds of news faster than any client learns them).
//! - an error answer keeps its JSON-RPC code ([`CallError::Refused`]): `auth_required` asks the person to sign in.
//! - writes go through one lock (a whole line at a time, flushed).
//! - the agent's stderr is drained into a short tail ([`TAIL_CAP`]) for error reports; it is never written anywhere.
//! - end of the agent's output (it exited) | [`Connection::close`] -> every waiting call answers `Closed`.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use agent_client_protocol_schema::v1::{
    Error as RpcError, ErrorCode, JsonRpcMessage, Notification, Request, RequestId, Response,
};
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
    fn notification(&self, method: &str, params: Option<Value>);
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
    /// The agent's stdin; `None` once closed.
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    waiting: Mutex<Waiting>,
    next: AtomicI64,
    tail: Tail,
}

impl Connection {
    /// Starts the reader and stderr threads.
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
        let connection = Arc::new(Self {
            writer: Mutex::new(Some(to_agent)),
            waiting: Mutex::new(Waiting::default()),
            next: AtomicI64::new(1),
            tail: Tail {
                lines: Mutex::new((VecDeque::new(), 0)),
                done: Mutex::new(done_rx),
            },
        });
        let reader = Arc::clone(&connection);
        std::thread::Builder::new()
            .name("acp-agent-out".into())
            .spawn(move || reader.read_loop(from_agent, inbox.as_ref()))?;
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
            (Some(method), None) => inbox.notification(&method, message.params),
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
        let answer = match wait {
            Some(wait) => rx.recv_timeout(wait).map_err(|e| match e {
                RecvTimeoutError::Timeout => CallError::TimedOut,
                RecvTimeoutError::Disconnected => CallError::Closed,
            }),
            None => rx.recv().map_err(|_| CallError::Closed),
        };
        if answer.is_err() {
            lock(&self.waiting).calls.remove(&id);
        }
        match answer? {
            Answer::Done(value) => serde_json::from_value(value).map_err(|_| CallError::Unreadable),
            Answer::Failed { code, message } => Err(CallError::Refused { code, message }),
        }
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

    fn send(&self, message: &impl Serialize) -> io::Result<()> {
        let mut line = serde_json::to_vec(message).map_err(io::Error::other)?;
        line.push(b'\n');
        let mut writer = lock(&self.writer);
        let to = writer
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?;
        to.write_all(&line).and_then(|()| to.flush())
    }

    /// Closes the agent's stdin (an ACP agent exits on it) and ends every waiting call (`Closed`), even while the
    /// agent's output stays open (a grandchild holding it).
    pub fn close(&self) {
        drop(lock(&self.writer).take());
        self.close_calls();
    }

    fn close_calls(&self) {
        let mut waiting = lock(&self.waiting);
        waiting.closed = true;
        waiting.calls.clear();
    }

    /// The newest `max_chars` characters of the agent's stderr, after waiting up to `wait` for the stream to end
    /// (an exiting agent writes its last words just before).
    #[must_use]
    pub fn stderr_tail(&self, max_chars: usize, wait: Duration) -> String {
        self.tail.settle(wait);
        self.tail.text(max_chars)
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
        let mut guard = lock(&self.lines);
        let (lines, bytes) = &mut *guard;
        *bytes += line.len() + 1;
        lines.push_back(line.to_owned());
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

    fn text(&self, max_chars: usize) -> String {
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
        let count = all.chars().count();
        all.chars().skip(count.saturating_sub(max_chars)).collect()
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
    }
}
