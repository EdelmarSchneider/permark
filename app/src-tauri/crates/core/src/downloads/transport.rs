//! The network as downloads see it: one GET per hop over [`Transport`]; redirects followed here by hand under the
//! source's allowlist. Credential only to the source's first origin; resolved (signed) addresses never leave [`open`],
//! NEVER stored (resume resolves again).

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use super::{origin_of, Asset, DownloadError, Secret, Source};
use crate::remote::{check_url, next_url, MAX_REDIRECTS};

/// One GET.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub url: &'a str,
    /// Resume offset -> `Range: bytes=N-`.
    pub from: Option<u64>,
    /// Credential -> `Authorization: Bearer <value>`; NEVER logged.
    pub auth: Option<&'a Secret>,
}

/// What one GET answered (headers the download needs + streaming body).
pub struct Response {
    pub status: u16,
    pub location: Option<String>,
    pub content_range: Option<String>,
    pub content_length: Option<u64>,
    pub body: Box<dyn Read + Send>,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("location", &self.location)
            .field("content_range", &self.content_range)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

/// HTTP client seam (blocking, object-safe): one GET, NO redirects followed, no cookies, no body decompression, only
/// public addresses connected to, per-read body timeout (no global timeout: large files).
pub trait Transport: Send + Sync {
    /// # Errors
    /// [`DownloadError::Offline`] (no answer), [`DownloadError::HostNotAllowed`] (name resolves to a non-public
    /// address; nothing connected).
    fn get(&self, request: &Request<'_>) -> Result<Response, DownloadError>;
}

/// Asks for `asset` (from byte `from` when resuming), following up to [`MAX_REDIRECTS`] redirects by hand: every hop
/// passes [`check_url`] (`https`, public host) + the source's host suffixes. `secret` goes ONLY to a hop on the
/// source's first origin (host + port; never to a CDN redirect host), and only for a source that accepts one.
/// `cancel` checked before every hop. The `200` | `206` answer is returned; resolved (signed) addresses stay here.
///
/// # Errors
/// `HostNotAllowed` (a hop off `https` | off the allowlist | to a non-public address), `Http { status }` (status other
/// than 200 | 206 | redirect, a redirect without `Location`, more than [`MAX_REDIRECTS`] redirects: last status),
/// `Offline`, `Cancelled`.
pub(crate) fn open(
    transport: &dyn Transport,
    asset: &Asset,
    from: Option<u64>,
    secret: Option<&Secret>,
    cancel: &AtomicBool,
) -> Result<Response, DownloadError> {
    let source = asset.source();
    let first = source.first_origin();
    let secret = secret.filter(|_| source.accepts_credential());
    let mut current = allowed(source, asset.url())?;
    let mut last = 0;
    for _ in 0..=MAX_REDIRECTS {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let on_first = first.is_some() && origin_of(&current) == first;
        let response = transport.get(&Request {
            url: &current,
            from,
            auth: secret.filter(|_| on_first),
        })?;
        match response.status {
            200 | 206 => return Ok(response),
            status @ (301 | 302 | 303 | 307 | 308) => {
                last = status;
                let location = response
                    .location
                    .as_deref()
                    .ok_or(DownloadError::Http { status })?;
                let next =
                    next_url(&current, location).map_err(|_| DownloadError::HostNotAllowed)?;
                current = allowed(source, next)?;
            }
            status => return Err(DownloadError::Http { status }),
        }
    }
    Err(DownloadError::Http { status: last })
}

/// `url` passes [`check_url`], its host is plain ([`origin_of`]: no escapes | backslashes) and under one of the
/// source's suffixes.
fn allowed(source: &Source, url: String) -> Result<String, DownloadError> {
    check_url(&url).map_err(|_| DownloadError::HostNotAllowed)?;
    match origin_of(&url) {
        Some((host, _)) if source.allows(&host) => Ok(url),
        _ => Err(DownloadError::HostNotAllowed),
    }
}

/// One canned answer of [`ScriptedTransport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scripted {
    pub status: u16,
    pub location: Option<String>,
    pub content_range: Option<String>,
    pub content_length: Option<u64>,
    pub body: Vec<u8>,
    /// Body read fails (`TimedOut`) after this many bytes: a dropped transfer.
    pub cut_after: Option<usize>,
}

impl Scripted {
    /// `200` with `body`.
    #[must_use]
    pub fn ok(body: &[u8]) -> Self {
        Self {
            content_length: Some(body.len() as u64),
            body: body.to_vec(),
            ..Self::status(200)
        }
    }

    /// `206` with `body` from byte `from` of a `total`-byte file (`Content-Range: bytes from-end/total`).
    #[must_use]
    pub fn partial(from: u64, total: u64, body: &[u8]) -> Self {
        let end = (from + body.len() as u64).saturating_sub(1);
        let mut answer = Self::ok(body);
        answer.status = 206;
        answer.content_range = Some(format!("bytes {from}-{end}/{total}"));
        answer
    }

    /// A redirect (`301 | 302 | 303 | 307 | 308`) to `location`.
    #[must_use]
    pub fn redirect(status: u16, location: &str) -> Self {
        Self {
            location: Some(location.to_owned()),
            ..Self::status(status)
        }
    }

    /// Bare `status`, empty body.
    #[must_use]
    pub fn status(status: u16) -> Self {
        Self {
            status,
            location: None,
            content_range: None,
            content_length: None,
            body: Vec::new(),
            cut_after: None,
        }
    }

    /// Body fails after `bytes` bytes.
    #[must_use]
    pub fn cut_after(mut self, bytes: usize) -> Self {
        self.cut_after = Some(bytes);
        self
    }
}

/// One request [`ScriptedTransport`] got. Credential recorded as a flag only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub url: String,
    pub from: Option<u64>,
    pub authorized: bool,
}

/// Test transport (core + plugin tests): canned answers per address, in order (the last one repeats); an address
/// without answers is `Offline`. Records every request.
#[derive(Debug, Default)]
pub struct ScriptedTransport {
    answers: Mutex<HashMap<String, VecDeque<Scripted>>>,
    asked: Mutex<Vec<Asked>>,
}

impl ScriptedTransport {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues `answer` for `url`.
    #[must_use]
    pub fn answer(self, url: &str, answer: Scripted) -> Self {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(url.to_owned())
            .or_default()
            .push_back(answer);
        self
    }

    /// Requests so far, in order.
    #[must_use]
    pub fn asked(&self) -> Vec<Asked> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Transport for ScriptedTransport {
    fn get(&self, request: &Request<'_>) -> Result<Response, DownloadError> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Asked {
                url: request.url.to_owned(),
                from: request.from,
                authorized: request.auth.is_some(),
            });
        let scripted = {
            let mut answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
            let queue = answers.get_mut(request.url).ok_or(DownloadError::Offline)?;
            if queue.len() > 1 {
                queue.pop_front()
            } else {
                queue.front().cloned()
            }
        }
        .ok_or(DownloadError::Offline)?;
        let end = scripted
            .cut_after
            .map_or(scripted.body.len(), |cut| cut.min(scripted.body.len()));
        Ok(Response {
            status: scripted.status,
            location: scripted.location,
            content_range: scripted.content_range,
            content_length: scripted.content_length,
            body: Box::new(CutBody {
                fail: scripted.cut_after.is_some(),
                bytes: scripted.body,
                at: 0,
                end,
            }),
        })
    }
}

/// Body of a scripted answer: `bytes[..end]`, then end of file | `TimedOut` (`fail`).
struct CutBody {
    bytes: Vec<u8>,
    at: usize,
    end: usize,
    fail: bool,
}

impl Read for CutBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.at >= self.end {
            return if self.fail {
                Err(io::Error::from(io::ErrorKind::TimedOut))
            } else {
                Ok(0)
            };
        }
        let n = buf.len().min(self.end - self.at);
        buf[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}
