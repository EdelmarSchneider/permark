//! Worker side of [`crate::wire`]: answers requests with any [`Engine`].
//!
//! - main loop runs ONE request at a time; a reader thread reads `reader` lines: [`Request::Cancel`] -> cancels the
//!   running call's token, others -> main loop (each with a fresh token).
//! - returns on stdin EOF | [`Request::Unload`] (Ok) | write failure (Err). Reader thread may outlive the call: the
//!   worker binary hard-exits right after.
//! - stdin EOF | read error (host gone): the reader thread calls `on_host_gone` AT ONCE, even while a call runs
//!   (worker: hard exit, D-10 -> cancel during load | plugin off never waits for llama.cpp).

use std::cell::RefCell;
use std::io::{self, BufRead, Write};
use std::sync::mpsc;
use std::thread;

use serde::Serialize;

use crate::wire::{Hello, Request, Response, PROTO};
use crate::{CancelToken, Engine, EngineError};

/// Speaks the protocol on `reader` | `writer` for `engine` until the host is gone or unloads; `on_host_gone` runs
/// on the reader thread when `reader` ends | fails (tests: no-op).
///
/// # Errors
/// Writing to `writer` failed (host gone).
pub fn serve(
    reader: impl BufRead + Send + 'static,
    writer: impl Write,
    engine: &dyn Engine,
    on_host_gone: impl FnOnce() + Send + 'static,
) -> io::Result<()> {
    let out = Out {
        writer: RefCell::new(writer),
        error: RefCell::new(None),
    };
    out.line(&Hello {
        proto: PROTO,
        caps: engine.caps(),
    });
    out.result()?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if read_requests(reader, &tx) {
            on_host_gone();
        }
    });
    for (request, cancel) in rx {
        if !answer(&out, engine, request, &cancel) {
            break;
        }
        out.result()?;
    }
    out.result()
}

/// Reader thread: forwards requests (bad line -> `Err` text), handles `Cancel` itself. `true`: host gone (EOF | read
/// error); `false`: main loop ended first.
fn read_requests(
    reader: impl BufRead,
    tx: &mpsc::Sender<(Result<Request, String>, CancelToken)>,
) -> bool {
    let mut current = CancelToken::new();
    for line in reader.lines() {
        let Ok(line) = line else { return true };
        if line.trim().is_empty() {
            continue;
        }
        let request = serde_json::from_str::<Request>(&line).map_err(|e| e.to_string());
        if matches!(request, Ok(Request::Cancel)) {
            current.cancel();
            continue;
        }
        current = CancelToken::new();
        if tx.send((request, current.clone())).is_err() {
            return false;
        }
    }
    true
}

/// Lines to the host; first write error kept (later lines dropped).
struct Out<W: Write> {
    writer: RefCell<W>,
    error: RefCell<Option<io::Error>>,
}

impl<W: Write> Out<W> {
    fn line(&self, value: &impl Serialize) {
        if self.error.borrow().is_some() {
            return;
        }
        let mut writer = self.writer.borrow_mut();
        let written = serde_json::to_writer(&mut *writer, value)
            .map_err(io::Error::other)
            .and_then(|()| writer.write_all(b"\n"))
            .and_then(|()| writer.flush());
        if let Err(e) = written {
            *self.error.borrow_mut() = Some(e);
        }
    }

    fn failed(&self) -> bool {
        self.error.borrow().is_some()
    }

    fn result(&self) -> io::Result<()> {
        self.error.borrow_mut().take().map_or(Ok(()), Err)
    }
}

/// Runs one request; `false` -> stop serving (unload).
fn answer<W: Write>(
    out: &Out<W>,
    engine: &dyn Engine,
    request: Result<Request, String>,
    cancel: &CancelToken,
) -> bool {
    // host gone mid-stream -> stop the work too
    let stream = |response| {
        out.line(&response);
        if out.failed() {
            cancel.cancel();
        }
    };
    let reply = match request {
        Err(e) => Err(EngineError::Io(format!("bad request: {e}"))),
        Ok(Request::Cancel) => return true,
        Ok(Request::Unload { session }) => {
            engine.unload(session);
            return false;
        }
        Ok(Request::EstimateMemory { model, opts }) => engine
            .estimate_memory(&model, &opts)
            .map(|estimate| Response::Memory { estimate }),
        Ok(Request::Load { model, opts }) => engine
            .load(
                model,
                opts,
                &|value| stream(Response::Progress { value }),
                cancel,
            )
            .map(|session| Response::Loaded {
                session,
                gpu: engine.on_gpu(session),
            }),
        Ok(Request::Tokenize { session, text }) => engine
            .tokenize(session, &text)
            .map(|ids| Response::Tokens { ids }),
        Ok(Request::CountTokens { session, text }) => engine
            .count_tokens(session, &text)
            .map(|tokens| Response::Count { tokens }),
        Ok(Request::ContextLen { session }) => engine
            .context_len(session)
            .map(|tokens| Response::ContextLen { tokens }),
        Ok(Request::Embed {
            session,
            texts,
            role,
            dims,
            normalize,
        }) => {
            let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
            engine
                .embed(session, &texts, role, dims, normalize, cancel)
                .map(|embeddings| Response::Embeddings { embeddings })
        }
        Ok(Request::Generate { session, request }) => engine
            .generate(
                session,
                &request,
                &mut |event| stream(Response::Gen { event }),
                cancel,
            )
            .map(|()| Response::Done),
        Ok(Request::Transcribe {
            session,
            pcm_16k_mono,
            lang,
        }) => engine
            .transcribe(
                session,
                &pcm_16k_mono,
                lang.as_deref(),
                &mut |segment| stream(Response::Segment { segment }),
                cancel,
            )
            .map(|()| Response::Done),
        Ok(Request::Synthesize {
            session,
            text,
            voice,
            lang,
        }) => engine
            .synthesize(
                session,
                &text,
                voice.as_deref(),
                lang.as_deref(),
                &mut |chunk| stream(Response::Pcm { chunk }),
                cancel,
            )
            .map(|()| Response::Done),
    };
    out.line(&reply.unwrap_or_else(|error| Response::Error { error }));
    true
}
