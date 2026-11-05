//! Host side of [`crate::wire`]: [`WorkerEngine`] = [`Engine`] over worker pipes.
//!
//! - one worker per loaded session: `load` launches one, reads [`Hello`] (proto != [`PROTO`] -> `EngineTooOld`),
//!   sends `Load`; `unload` sends `Unload`, closes stdin, kills. Drop = every worker gone.
//! - per worker: reader thread pushes stdout lines into a channel; calls poll it ([`POLL`]) + check the cancel
//!   token -> `Cancel` sent once (load: kill instead -> `Cancelled`).
//! - token cancelled before a call -> answered host-side (`Cancelled`; `generate`: `Done` + `Cancelled`), worker
//!   not asked.
//! - worker gone (EOF | bad line | write failure) -> `EngineCrashed`, session forgotten; next `load` = fresh worker.
//! - host session ids own (unique per proxy); worker ids stay on the wire.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;

use crate::wire::{Hello, Probe, Request, Response, PROTO};
use crate::{
    CancelToken, ChatRequest, EmbedRole, Embeddings, Engine, EngineCaps, EngineError, FileRole,
    Finish, GenEvent, LoadOpts, MemoryEstimate, ModelFiles, PcmChunk, Segment, SessionId, Usage,
};

/// Cancel | worker-death check interval while a call waits.
const POLL: Duration = Duration::from_millis(50);
/// Worker silent this long before its [`Hello`] -> treated as crashed.
const HELLO_WAIT: Duration = Duration::from_secs(30);

/// A started worker's stdio + how to stop it.
pub struct WorkerPipes {
    /// Worker stdin.
    pub to_worker: Box<dyn Write + Send>,
    /// Worker stdout.
    pub from_worker: Box<dyn BufRead + Send>,
    /// Ensures the worker is gone (may wait briefly for its own exit first); called once, after stdin closed.
    pub kill: Box<dyn FnMut() + Send>,
}

/// Starts one worker.
pub type Launch = Box<dyn Fn() -> io::Result<WorkerPipes> + Send + Sync>;

/// [`Engine`] running each loaded model in its own worker (see module doc).
pub struct WorkerEngine {
    launch: Launch,
    /// From the last [`Hello`].
    caps: Mutex<Option<EngineCaps>>,
    next: AtomicU64,
    workers: Mutex<HashMap<SessionId, Arc<Mutex<Worker>>>>,
}

/// One running worker holding one session.
struct Worker {
    to: Box<dyn Write + Send>,
    lines: Receiver<String>,
    kill: Box<dyn FnMut() + Send>,
    /// Session id on the worker's side.
    session: SessionId,
}

/// What a waiting call does when its token is cancelled.
#[derive(Clone, Copy)]
enum OnCancel {
    /// Send `Cancel` once, keep reading (worker answers per trait rules).
    Forward,
    /// Stop waiting -> `Cancelled` (caller kills the worker).
    Abort,
}

/// One response's meaning to a waiting call.
enum Step<T> {
    More,
    Done(T),
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Worker {
    fn start(pipes: WorkerPipes) -> Self {
        let WorkerPipes {
            to_worker,
            from_worker,
            kill,
        } = pipes;
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in from_worker.lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        Self {
            to: to_worker,
            lines,
            kill,
            session: SessionId(0),
        }
    }

    /// Reads [`Hello`]; wrong protocol -> `EngineTooOld`.
    fn hello(&mut self, cancel: &CancelToken) -> Result<Hello, EngineError> {
        let started = Instant::now();
        let line = loop {
            if cancel.is_cancelled() {
                return Err(EngineError::Cancelled);
            }
            match self.lines.recv_timeout(POLL) {
                Ok(line) => break line,
                Err(RecvTimeoutError::Timeout) if started.elapsed() < HELLO_WAIT => {}
                Err(_) => return Err(EngineError::EngineCrashed),
            }
        };
        let probe: Probe = parse(&line)?;
        if probe.proto != PROTO {
            return Err(EngineError::EngineTooOld {
                needed: format!("protocol {PROTO}"),
            });
        }
        parse(&line)
    }

    fn send(&mut self, request: &Request) -> Result<(), EngineError> {
        let mut line = serde_json::to_vec(request).map_err(|_| EngineError::EngineCrashed)?;
        line.push(b'\n');
        self.to
            .write_all(&line)
            .and_then(|()| self.to.flush())
            .map_err(|_| EngineError::EngineCrashed)
    }

    /// Sends `request`, feeds responses to `step` until it is done; `Response::Error` -> that error.
    fn call<T>(
        &mut self,
        request: &Request,
        cancel: &CancelToken,
        on_cancel: OnCancel,
        mut step: impl FnMut(Response) -> Result<Step<T>, EngineError>,
    ) -> Result<T, EngineError> {
        self.send(request)?;
        let mut cancel_sent = false;
        loop {
            if cancel.is_cancelled() {
                match on_cancel {
                    OnCancel::Abort => return Err(EngineError::Cancelled),
                    OnCancel::Forward if !cancel_sent => {
                        cancel_sent = true;
                        self.send(&Request::Cancel)?;
                    }
                    OnCancel::Forward => {}
                }
            }
            let line = match self.lines.recv_timeout(POLL) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Err(EngineError::EngineCrashed),
            };
            match parse(&line)? {
                Response::Error { error } => return Err(error),
                response => {
                    if let Step::Done(value) = step(response)? {
                        return Ok(value);
                    }
                }
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // stdin EOF first: the worker exits by itself
        self.to = Box::new(io::sink());
        (self.kill)();
    }
}

fn parse<T: DeserializeOwned>(line: &str) -> Result<T, EngineError> {
    serde_json::from_str(line).map_err(|_| EngineError::EngineCrashed)
}

/// Response the call did not ask for = protocol broken.
fn unexpected<T>() -> Result<T, EngineError> {
    Err(EngineError::EngineCrashed)
}

impl WorkerEngine {
    #[must_use]
    pub fn new(launch: Launch) -> Self {
        Self {
            launch,
            caps: Mutex::new(None),
            next: AtomicU64::new(0),
            workers: Mutex::new(HashMap::new()),
        }
    }

    fn start(&self, cancel: &CancelToken) -> Result<Worker, EngineError> {
        let pipes = (self.launch)().map_err(|e| EngineError::Io(e.to_string()))?;
        let mut worker = Worker::start(pipes);
        let hello = worker.hello(cancel)?;
        *lock(&self.caps) = Some(hello.caps);
        Ok(worker)
    }

    /// Runs a call on `session`'s worker; worker gone -> forgotten + killed.
    fn on<T>(
        &self,
        session: SessionId,
        request: impl FnOnce(SessionId) -> Request,
        cancel: &CancelToken,
        step: impl FnMut(Response) -> Result<Step<T>, EngineError>,
    ) -> Result<T, EngineError> {
        let Some(worker) = lock(&self.workers).get(&session).cloned() else {
            return Err(EngineError::NotLoaded);
        };
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let result = {
            let mut guard = lock(&worker);
            let request = request(guard.session);
            guard.call(&request, cancel, OnCancel::Forward, step)
        };
        if matches!(result, Err(EngineError::EngineCrashed)) {
            let mut workers = lock(&self.workers);
            if workers
                .get(&session)
                .is_some_and(|w| Arc::ptr_eq(w, &worker))
            {
                workers.remove(&session);
            }
        }
        result
    }

    /// Calls with a single final response.
    fn ask<T>(
        &self,
        session: SessionId,
        request: impl FnOnce(SessionId) -> Request,
        cancel: &CancelToken,
        pick: impl Fn(Response) -> Option<T>,
    ) -> Result<T, EngineError> {
        self.on(session, request, cancel, |r| {
            pick(r).map_or_else(unexpected, |v| Ok(Step::Done(v)))
        })
    }
}

impl Engine for WorkerEngine {
    /// From the last [`Hello`]; none yet -> a worker started only to say hello (failure -> no capabilities).
    fn caps(&self) -> EngineCaps {
        if let Some(caps) = lock(&self.caps).clone() {
            return caps;
        }
        // worker dropped = gone
        match self.start(&CancelToken::new()) {
            Ok(_) => lock(&self.caps).clone().unwrap_or_default(),
            Err(_) => EngineCaps::default(),
        }
    }

    /// Host side, no worker: lower bound = file bytes, all in RAM (`vram` 0; engine places layers at load).
    fn estimate_memory(
        &self,
        model: &ModelFiles,
        _opts: &LoadOpts,
    ) -> Result<MemoryEstimate, EngineError> {
        if !model
            .files
            .iter()
            .any(|(role, _)| *role == FileRole::Weights)
        {
            return Err(EngineError::Corrupt);
        }
        let mut ram = 0u64;
        for (_, path) in &model.files {
            ram += Path::metadata(path)
                .map_err(|e| EngineError::Io(e.to_string()))?
                .len();
        }
        Ok(MemoryEstimate { ram, vram: 0 })
    }

    fn load(
        &self,
        model: ModelFiles,
        opts: LoadOpts,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<SessionId, EngineError> {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        // failure | cancel below drops (= kills) the worker
        let mut worker = self.start(cancel)?;
        worker.session = worker.call(
            &Request::Load { model, opts },
            cancel,
            OnCancel::Abort,
            |r| match r {
                Response::Progress { value } => {
                    progress(value);
                    Ok(Step::More)
                }
                Response::Loaded { session } => Ok(Step::Done(session)),
                _ => unexpected(),
            },
        )?;
        let id = SessionId(self.next.fetch_add(1, Ordering::Relaxed) + 1);
        lock(&self.workers).insert(id, Arc::new(Mutex::new(worker)));
        Ok(id)
    }

    fn unload(&self, session: SessionId) {
        let Some(worker) = lock(&self.workers).remove(&session) else {
            return;
        };
        let mut worker = lock(&worker);
        let request = Request::Unload {
            session: worker.session,
        };
        // worker gone already -> kill below anyway
        let _ = worker.send(&request);
        // last Arc dropped after the guard -> Drop kills
    }

    fn tokenize(&self, session: SessionId, text: &str) -> Result<Vec<u32>, EngineError> {
        let text = text.to_owned();
        self.ask(
            session,
            |session| Request::Tokenize { session, text },
            &CancelToken::new(),
            |r| match r {
                Response::Tokens { ids } => Some(ids),
                _ => None,
            },
        )
    }

    fn count_tokens(&self, session: SessionId, text: &str) -> Result<usize, EngineError> {
        let text = text.to_owned();
        self.ask(
            session,
            |session| Request::CountTokens { session, text },
            &CancelToken::new(),
            |r| match r {
                Response::Count { tokens } => Some(tokens),
                _ => None,
            },
        )
    }

    fn context_len(&self, session: SessionId) -> Result<u32, EngineError> {
        self.ask(
            session,
            |session| Request::ContextLen { session },
            &CancelToken::new(),
            |r| match r {
                Response::ContextLen { tokens } => Some(tokens),
                _ => None,
            },
        )
    }

    fn embed(
        &self,
        session: SessionId,
        texts: &[&str],
        role: EmbedRole,
        dims: Option<u32>,
        normalize: bool,
        cancel: &CancelToken,
    ) -> Result<Embeddings, EngineError> {
        let texts = texts.iter().map(|&t| t.to_owned()).collect();
        self.ask(
            session,
            |session| Request::Embed {
                session,
                texts,
                role,
                dims,
                normalize,
            },
            cancel,
            |r| match r {
                Response::Embeddings { embeddings } => Some(embeddings),
                _ => None,
            },
        )
    }

    fn generate(
        &self,
        session: SessionId,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        if cancel.is_cancelled() && lock(&self.workers).contains_key(&session) {
            sink(GenEvent::Done {
                usage: Usage::default(),
                finish: Finish::Cancelled,
            });
            return Ok(());
        }
        let request = request.clone();
        self.on(
            session,
            |session| Request::Generate { session, request },
            cancel,
            |r| match r {
                Response::Gen { event } => {
                    sink(event);
                    Ok(Step::More)
                }
                Response::Done => Ok(Step::Done(())),
                _ => unexpected(),
            },
        )
    }

    fn transcribe(
        &self,
        session: SessionId,
        pcm_16k_mono: &[f32],
        lang: Option<&str>,
        sink: &mut dyn FnMut(Segment),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        let pcm_16k_mono = pcm_16k_mono.to_vec();
        let lang = lang.map(str::to_owned);
        self.on(
            session,
            |session| Request::Transcribe {
                session,
                pcm_16k_mono,
                lang,
            },
            cancel,
            |r| match r {
                Response::Segment { segment } => {
                    sink(segment);
                    Ok(Step::More)
                }
                Response::Done => Ok(Step::Done(())),
                _ => unexpected(),
            },
        )
    }

    fn synthesize(
        &self,
        session: SessionId,
        text: &str,
        voice: Option<&str>,
        lang: Option<&str>,
        sink: &mut dyn FnMut(PcmChunk),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        let text = text.to_owned();
        let voice = voice.map(str::to_owned);
        let lang = lang.map(str::to_owned);
        self.on(
            session,
            |session| Request::Synthesize {
                session,
                text,
                voice,
                lang,
            },
            cancel,
            |r| match r {
                Response::Pcm { chunk } => {
                    sink(chunk);
                    Ok(Step::More)
                }
                Response::Done => Ok(Step::Done(())),
                _ => unexpected(),
            },
        )
    }
}
