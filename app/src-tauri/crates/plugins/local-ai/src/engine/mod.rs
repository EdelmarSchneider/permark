//! Engine seam: the contract every model runtime fulfills (C4 implements it; feature topics call it). Contract only:
//! no inference here; [`FakeEngine`] answers deterministically for tests of future features.
//!
//! Rules for implementers (C4):
//! - process model: llama.cpp family runs in its OWN worker process, talking over stdio pipes; NEVER a TCP | other
//!   socket port. Worker crash -> [`EngineError::EngineCrashed`], app keeps running; next `load` starts a new worker.
//! - unload = worker process exit (memory back to the OS for sure); NEVER rely on in-process free.
//! - blocking API: callers run it off the UI thread; streaming results go to the `sink` as they come; `cancel` checked
//!   between steps (token, segment, chunk, batch).
//! - prompt formats (embedding prefixes per [`EmbedRole`], chat templates) = catalog data (`prefix_profile`,
//!   `prompt_profile`), NEVER hardcoded per model in engine code.
//! - cancel: `generate` ends with [`GenEvent::Done`] + [`Finish::Cancelled`] (`Ok`); every other call answers
//!   [`EngineError::Cancelled`]. Anything already sent to a sink stays valid.
//! - missing capability (engine | model) -> [`EngineError::Unsupported`], never a silent fallback.

mod fake;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;

use crate::catalog::{Capability, Catalog, FileRole};
use crate::installs::AssignedModel;

pub use fake::FakeEngine;

/// A model runtime (object safe: features hold `Arc<dyn Engine>`).
pub trait Engine: Send + Sync {
    /// What this engine (build + machine) can do.
    fn caps(&self) -> EngineCaps;

    /// Memory `model` would take loaded with `opts` (refines catalog `needs`, D-3).
    ///
    /// # Errors
    /// [`EngineError::Corrupt`] | [`EngineError::Unsupported`] | [`EngineError::Io`].
    fn estimate_memory(
        &self,
        model: &ModelFiles,
        opts: &LoadOpts,
    ) -> Result<MemoryEstimate, EngineError>;

    /// Loads `model`; `progress` gets 0.0..=1.0.
    ///
    /// # Errors
    /// [`EngineError::OutOfMemory`] | [`EngineError::Corrupt`] | [`EngineError::EngineTooOld`] |
    /// [`EngineError::Unsupported`] | [`EngineError::Cancelled`] | [`EngineError::Io`].
    fn load(
        &self,
        model: ModelFiles,
        opts: LoadOpts,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<SessionId, EngineError>;

    /// Frees the session (worker exit); unknown session -> nothing.
    fn unload(&self, session: SessionId);

    /// Token ids of `text` in the session's vocabulary.
    ///
    /// # Errors
    /// [`EngineError::NotLoaded`] | [`EngineError::EngineCrashed`].
    fn tokenize(&self, session: SessionId, text: &str) -> Result<Vec<u32>, EngineError>;

    /// Number of tokens of `text` (same as `tokenize(..).len()`, maybe cheaper).
    ///
    /// # Errors
    /// [`EngineError::NotLoaded`] | [`EngineError::EngineCrashed`].
    fn count_tokens(&self, session: SessionId, text: &str) -> Result<usize, EngineError>;

    /// Context window of the session, tokens.
    ///
    /// # Errors
    /// [`EngineError::NotLoaded`] | [`EngineError::EngineCrashed`].
    fn context_len(&self, session: SessionId) -> Result<u32, EngineError>;

    /// One vector per text; `dims` = Matryoshka truncation (`None`: model native); `normalize` -> L2 length 1.
    ///
    /// # Errors
    /// [`EngineError::Unsupported`] (no embed | dims not offered) | [`EngineError::Cancelled`] | not loaded | crash.
    fn embed(
        &self,
        session: SessionId,
        texts: &[&str],
        role: EmbedRole,
        dims: Option<u32>,
        normalize: bool,
        cancel: &CancelToken,
    ) -> Result<Embeddings, EngineError>;

    /// Streams the answer to `request` into `sink`; last event = [`GenEvent::Done`].
    ///
    /// # Errors
    /// [`EngineError::Unsupported`] (no chat | tools | constraint kind) | not loaded | crash.
    fn generate(
        &self,
        session: SessionId,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError>;

    /// Speech (`pcm_16k_mono`: 16 kHz mono samples -1..1) -> text segments; `lang` BCP-47 (`None`: detect).
    ///
    /// # Errors
    /// [`EngineError::Unsupported`] | [`EngineError::Cancelled`] | not loaded | crash.
    fn transcribe(
        &self,
        session: SessionId,
        pcm_16k_mono: &[f32],
        lang: Option<&str>,
        sink: &mut dyn FnMut(Segment),
        cancel: &CancelToken,
    ) -> Result<(), EngineError>;

    /// Text -> speech chunks; `voice` | `lang` `None`: model default.
    ///
    /// # Errors
    /// [`EngineError::Unsupported`] | [`EngineError::Cancelled`] | not loaded | crash.
    fn synthesize(
        &self,
        session: SessionId,
        text: &str,
        voice: Option<&str>,
        lang: Option<&str>,
        sink: &mut dyn FnMut(PcmChunk),
        cancel: &CancelToken,
    ) -> Result<(), EngineError>;
}

/// What an engine can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCaps {
    /// Model file formats read (`gguf`, `ggml`).
    pub formats: Vec<String>,
    pub capabilities: Vec<Capability>,
    /// A usable GPU backend (Vulkan | Metal) on this machine.
    pub gpu: bool,
    /// Engine release tag (compared with catalog `min_engine`).
    pub version: String,
}

/// Memory a load takes, bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryEstimate {
    pub ram: u64,
    pub vram: u64,
}

/// One loaded model in an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// An installed model's files, ready for [`Engine::load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFiles {
    pub model_id: String,
    pub catalog_version: u32,
    /// Absolute paths in the store.
    pub files: Vec<(FileRole, PathBuf)>,
    /// Catalog embedding prefix set (embed models), carried into [`EmbeddingFingerprint`].
    pub prefix_profile: Option<String>,
}

impl ModelFiles {
    /// Files of the installed version `model` (from [`crate::Installs::model_for`]); prefix profile from `catalog`
    /// entry of the same id (lineage-stable).
    #[must_use]
    pub fn of(model: &AssignedModel, catalog: &Catalog) -> Self {
        Self {
            model_id: model.id.clone(),
            catalog_version: model.catalog_version,
            files: model
                .files
                .iter()
                .map(|f| {
                    let name = f.path.rsplit('/').next().unwrap_or(&f.path);
                    (f.role, model.dir.join(name))
                })
                .collect(),
            prefix_profile: catalog
                .model(&model.id)
                .and_then(|m| m.embedding.as_ref())
                .map(|e| e.prefix_profile.clone()),
        }
    }
}

/// How to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadOpts {
    /// Context window, tokens (<= model maximum).
    pub context_len: u32,
    pub device: Device,
    /// CPU threads (`None`: engine picks).
    pub threads: Option<u32>,
}

/// Where a model runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Device {
    /// GPU when it fits, else CPU.
    #[default]
    Auto,
    Cpu,
    /// GPU adapter index (machine profile order).
    Gpu(u32),
}

/// Stop flag shared between caller and a running engine call.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// The raw flag (core APIs taking `&AtomicBool`).
    #[must_use]
    pub fn flag(&self) -> &AtomicBool {
        &self.0
    }
}

/// What a text is embedded as (selects the catalog prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmbedRole {
    Query,
    Document,
    /// Same prefix both sides (similarity, clustering).
    Symmetric,
}

/// Vectors + what made them.
#[derive(Debug, Clone, PartialEq)]
pub struct Embeddings {
    pub vectors: Vec<Vec<f32>>,
    pub fingerprint: EmbeddingFingerprint,
}

/// Identity of an embedding space: vectors with different fingerprints NEVER compared (index rebuild, C3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmbeddingFingerprint {
    pub model_id: String,
    pub catalog_version: u32,
    pub dims: u32,
    pub prefix_profile: Option<String>,
}

/// One chat call.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    /// Non-empty -> model may answer [`GenEvent::ToolCall`].
    pub tools: Vec<ToolSpec>,
    pub constraint: Constraint,
    pub sampling: Sampling,
    /// Answer length cap, tokens.
    pub max_tokens: u32,
    /// Answer ends before the first of these.
    pub stop: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    /// Result of a tool call.
    Tool,
}

/// A tool the model may call.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments.
    pub json_schema: Value,
}

/// Shape the answer must take.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Constraint {
    #[default]
    None,
    /// JSON matching this schema.
    JsonSchema(Value),
    /// GBNF grammar.
    Grammar(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampling {
    pub temperature: f32,
    pub top_p: f32,
    /// Same seed + input -> same answer.
    pub seed: Option<u64>,
}

impl Default for Sampling {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.95,
            seed: None,
        }
    }
}

/// One step of a streamed answer.
#[derive(Debug, Clone, PartialEq)]
pub enum GenEvent {
    Delta(String),
    ToolCall { name: String, args: Value },
    Done { usage: Usage, finish: Finish },
}

/// Tokens used by one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub prompt: u32,
    pub completion: u32,
}

/// Why an answer ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    Stop,
    Length,
    Tool,
    Cancelled,
}

/// Transcribed span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub t0_ms: u64,
    pub t1_ms: u64,
    pub text: String,
}

/// Synthesized audio, mono samples -1..1.
#[derive(Debug, Clone, PartialEq)]
pub struct PcmChunk {
    pub rate: u32,
    pub samples: Vec<f32>,
}

/// Why an engine call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("There is not enough memory to load this model.")]
    OutOfMemory,
    #[error("This is not supported here: {what}.")]
    Unsupported { what: String },
    #[error("The model files are damaged.")]
    Corrupt,
    #[error("Stopped.")]
    Cancelled,
    #[error("The model stopped unexpectedly.")]
    EngineCrashed,
    #[error("This model needs a newer engine ({needed}).")]
    EngineTooOld { needed: String },
    /// Session unknown (never loaded | unloaded | lost in a crash).
    #[error("The model is not loaded.")]
    NotLoaded,
    #[error("The model files could not be read: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests;
