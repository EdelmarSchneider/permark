//! Deterministic [`Engine`] for tests (no files read, no worker process).
//!
//! Behavior:
//! - embed: per-dimension `fsx::hash` of (role, text, index); native dims [`FakeEngine::NATIVE_DIMS`].
//! - generate: echoes the last user message word by word ([`GenEvent::Delta`]), honors `max_tokens` + `stop`;
//!   `call:<name>` in the last user message + tool `<name>` offered -> [`GenEvent::ToolCall`] (args `{}`).
//!   Constraint accepted, not applied.
//! - transcribe: one segment per started second (`"<lang> second <n>"`); synthesize: one 100 ms silent chunk per
//!   word.
//! - tokens = whitespace-separated words; [`FakeEngine::crash`] -> sessions lost, calls answer `EngineCrashed` until
//!   next `load` (worker restart).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use permark_core::fsx;
use serde_json::json;

use super::{
    CancelToken, ChatRequest, Device, EmbedRole, EmbeddingFingerprint, Embeddings, Engine,
    EngineCaps, EngineError, Finish, GenEvent, LoadOpts, MemoryEstimate, ModelFiles, PcmChunk,
    Role, Segment, SessionId, Usage,
};
use crate::catalog::{Capability, FileRole};

/// See module doc.
#[derive(Debug)]
pub struct FakeEngine {
    caps: EngineCaps,
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    next: u64,
    sessions: HashMap<SessionId, Loaded>,
    crashed: bool,
}

#[derive(Debug)]
struct Loaded {
    model: ModelFiles,
    context_len: u32,
}

const SPEECH_RATE: u32 = 24_000;
const PCM_16K: usize = 16_000;

impl Default for FakeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeEngine {
    /// Vector length when `dims` is `None`.
    pub const NATIVE_DIMS: u32 = 16;

    /// Every capability, with GPU.
    #[must_use]
    pub fn new() -> Self {
        Self::with_caps(
            vec![
                Capability::Embed,
                Capability::Chat,
                Capability::Tools,
                Capability::Translate,
                Capability::Transcribe,
                Capability::Speak,
                Capability::Vision,
            ],
            true,
        )
    }

    /// Only `capabilities` (others -> `Unsupported`).
    #[must_use]
    pub fn with_caps(capabilities: Vec<Capability>, gpu: bool) -> Self {
        Self {
            caps: EngineCaps {
                formats: vec!["gguf".to_owned()],
                capabilities,
                gpu,
                version: "fake-1".to_owned(),
            },
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Simulates a worker crash: every session lost.
    pub fn crash(&self) {
        let mut inner = self.inner();
        inner.sessions.clear();
        inner.crashed = true;
    }

    /// Sessions loaded now.
    #[must_use]
    pub fn loaded(&self) -> usize {
        self.inner().sessions.len()
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn need(&self, capability: Capability) -> Result<(), EngineError> {
        if self.caps.capabilities.contains(&capability) {
            Ok(())
        } else {
            Err(unsupported(&format!("{capability:?}").to_lowercase()))
        }
    }

    /// Runs `f` on a live session.
    fn with<T>(&self, session: SessionId, f: impl FnOnce(&Loaded) -> T) -> Result<T, EngineError> {
        let inner = self.inner();
        match inner.sessions.get(&session) {
            Some(loaded) => Ok(f(loaded)),
            None if inner.crashed => Err(EngineError::EngineCrashed),
            None => Err(EngineError::NotLoaded),
        }
    }
}

fn unsupported(what: &str) -> EngineError {
    EngineError::Unsupported {
        what: what.to_owned(),
    }
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
}

fn count(text: &str) -> u32 {
    u32::try_from(words(text).count()).unwrap_or(u32::MAX)
}

/// Value in -1..1 from a hash.
#[allow(clippy::cast_precision_loss)]
fn unit(bytes: &[u8]) -> f32 {
    let h = fsx::hash(bytes).0;
    ((h >> 40) as f32 / (1u64 << 24) as f32).mul_add(2.0, -1.0)
}

impl Engine for FakeEngine {
    fn caps(&self) -> EngineCaps {
        self.caps.clone()
    }

    fn estimate_memory(
        &self,
        model: &ModelFiles,
        opts: &LoadOpts,
    ) -> Result<MemoryEstimate, EngineError> {
        if !model
            .files
            .iter()
            .any(|(role, _)| *role == FileRole::Weights)
        {
            return Err(EngineError::Corrupt);
        }
        let ram = (model.files.len() as u64) * (1 << 20) + u64::from(opts.context_len) * 1024;
        let vram = if self.caps.gpu && opts.device != Device::Cpu {
            ram
        } else {
            0
        };
        Ok(MemoryEstimate { ram, vram })
    }

    fn load(
        &self,
        model: ModelFiles,
        opts: LoadOpts,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<SessionId, EngineError> {
        if !model
            .files
            .iter()
            .any(|(role, _)| *role == FileRole::Weights)
        {
            return Err(EngineError::Corrupt);
        }
        if matches!(opts.device, Device::Gpu(_)) && !self.caps.gpu {
            return Err(unsupported("gpu"));
        }
        for step in 0..=4u8 {
            if cancel.is_cancelled() {
                return Err(EngineError::Cancelled);
            }
            progress(f32::from(step) / 4.0);
        }
        let mut inner = self.inner();
        inner.crashed = false;
        inner.next += 1;
        let id = SessionId(inner.next);
        inner.sessions.insert(
            id,
            Loaded {
                model,
                context_len: opts.context_len,
            },
        );
        Ok(id)
    }

    fn unload(&self, session: SessionId) {
        self.inner().sessions.remove(&session);
    }

    fn tokenize(&self, session: SessionId, text: &str) -> Result<Vec<u32>, EngineError> {
        self.with(session, |_| {
            words(text)
                .map(|w| u32::try_from(fsx::hash(w.as_bytes()).0 & 0xffff_ffff).unwrap_or(0))
                .collect()
        })
    }

    fn count_tokens(&self, session: SessionId, text: &str) -> Result<usize, EngineError> {
        self.with(session, |_| words(text).count())
    }

    fn context_len(&self, session: SessionId) -> Result<u32, EngineError> {
        self.with(session, |l| l.context_len)
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
        self.need(Capability::Embed)?;
        let model = self.with(session, |l| l.model.clone())?;
        let dims = dims.unwrap_or(Self::NATIVE_DIMS);
        if dims == 0 || dims > Self::NATIVE_DIMS {
            return Err(unsupported(&format!("{dims} dimensions")));
        }
        let mut vectors = Vec::with_capacity(texts.len());
        for text in texts {
            if cancel.is_cancelled() {
                return Err(EngineError::Cancelled);
            }
            let mut v: Vec<f32> = (0..dims)
                .map(|i| unit(format!("{role:?}\u{0}{text}\u{0}{i}").as_bytes()))
                .collect();
            let len = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            if normalize && len > 0.0 {
                for x in &mut v {
                    *x /= len;
                }
            }
            vectors.push(v);
        }
        Ok(Embeddings {
            vectors,
            fingerprint: EmbeddingFingerprint {
                model_id: model.model_id,
                catalog_version: model.catalog_version,
                dims,
                prefix_profile: model.prefix_profile,
            },
        })
    }

    fn generate(
        &self,
        session: SessionId,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        self.need(Capability::Chat)?;
        if !request.tools.is_empty() {
            self.need(Capability::Tools)?;
        }
        self.with(session, |_| ())?;
        let prompt = request
            .messages
            .iter()
            .map(|m| count(&m.content))
            .fold(0u32, u32::saturating_add);
        let last = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map_or("", |m| m.content.as_str());
        let done = |completion, finish| GenEvent::Done {
            usage: Usage { prompt, completion },
            finish,
        };
        if let Some(tool) = request
            .tools
            .iter()
            .find(|t| last.contains(&format!("call:{}", t.name)))
        {
            sink(GenEvent::ToolCall {
                name: tool.name.clone(),
                args: json!({}),
            });
            sink(done(1, Finish::Tool));
            return Ok(());
        }
        let stop_at = request
            .stop
            .iter()
            .filter_map(|s| last.find(s.as_str()))
            .min();
        let echo = stop_at.map_or(last, |at| &last[..at]);
        let mut completion = 0u32;
        for (i, word) in words(echo).enumerate() {
            if cancel.is_cancelled() {
                sink(done(completion, Finish::Cancelled));
                return Ok(());
            }
            if completion >= request.max_tokens {
                sink(done(completion, Finish::Length));
                return Ok(());
            }
            sink(GenEvent::Delta(if i == 0 {
                word.to_owned()
            } else {
                format!(" {word}")
            }));
            completion += 1;
        }
        sink(done(completion, Finish::Stop));
        Ok(())
    }

    fn transcribe(
        &self,
        session: SessionId,
        pcm_16k_mono: &[f32],
        lang: Option<&str>,
        sink: &mut dyn FnMut(Segment),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        self.need(Capability::Transcribe)?;
        self.with(session, |_| ())?;
        let lang = lang.unwrap_or("und");
        for (n, chunk) in (0u64..).zip(pcm_16k_mono.chunks(PCM_16K)) {
            if cancel.is_cancelled() {
                return Err(EngineError::Cancelled);
            }
            let len = chunk.len() as u64 * 1000 / PCM_16K as u64;
            sink(Segment {
                t0_ms: n * 1000,
                t1_ms: n * 1000 + len,
                text: format!("{lang} second {n}"),
            });
        }
        Ok(())
    }

    fn synthesize(
        &self,
        session: SessionId,
        text: &str,
        _voice: Option<&str>,
        _lang: Option<&str>,
        sink: &mut dyn FnMut(PcmChunk),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        self.need(Capability::Speak)?;
        self.with(session, |_| ())?;
        for _ in words(text) {
            if cancel.is_cancelled() {
                return Err(EngineError::Cancelled);
            }
            sink(PcmChunk {
                rate: SPEECH_RATE,
                samples: vec![0.0; (SPEECH_RATE / 10) as usize],
            });
        }
        Ok(())
    }
}
