//! Model engine contract: the [`Engine`] trait every model runtime fulfills (feature topics call it), its worker
//! wire protocol ([`wire`]), the worker loop ([`serve`]) and the host proxy ([`WorkerEngine`]). Shared by the app
//! (host) and the engine worker binary; serde only.
//!
//! Rules for implementers:
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
//! - every type crossing the wire = serde; non-finite floats (NaN) NEVER sent (JSON has none).

#[cfg(feature = "llama")]
pub mod llama;
mod prompt;
mod proxy;
mod serve;
pub mod wire;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use prompt::{Piece, Prefixes, PromptProfile, StopScan, Turn};
pub use proxy::{Launch, WorkerEngine, WorkerPipes};
pub use serve::serve;

/// A model runtime (object safe: features hold `Arc<dyn Engine>`).
pub trait Engine: Send + Sync {
    /// What this engine (build + machine) can do.
    fn caps(&self) -> EngineCaps;

    /// Memory `model` would take loaded with `opts` (refines catalog `needs`).
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

/// What a model can do (stable names: stored in assignments, shared with TS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, rename = "ModelCapability")
)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    /// Text -> vectors (find by meaning).
    Embed,
    /// Follows instructions in a conversation (correct, rewrite, translate, answer).
    Chat,
    /// Calls tools from a conversation.
    Tools,
    /// Dedicated translator.
    Translate,
    /// Speech -> text.
    Transcribe,
    /// Text -> speech.
    Speak,
    /// Reads pictures.
    Vision,
}

/// Part a file plays in a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileRole {
    Weights,
    /// Multimodal projector (pictures, audio).
    Mmproj,
    /// Multi-token prediction draft (speed-up).
    Mtp,
    Voice,
}

/// What an engine can do.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEstimate {
    pub ram: u64,
    pub vram: u64,
}

/// One loaded model in an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

/// An installed model's files, ready for [`Engine::load`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelFiles {
    pub model_id: String,
    pub catalog_version: u32,
    /// Absolute paths in the store.
    pub files: Vec<(FileRole, PathBuf)>,
    /// Catalog embedding prefix set (embed models), carried into [`EmbeddingFingerprint`].
    pub prefix_profile: Option<String>,
    /// The prefixes of `prefix_profile` (catalog `prefixProfiles`).
    #[serde(default)]
    pub prefixes: Option<Prefixes>,
    /// Chat markup (chat models; catalog `promptProfiles`); `None` -> `generate` unsupported.
    #[serde(default)]
    pub prompt: Option<PromptProfile>,
    /// Oldest engine release reading the files (catalog `minEngine.version` of this engine family).
    #[serde(default)]
    pub min_engine: Option<String>,
}

impl ModelFiles {
    /// The weights file.
    #[must_use]
    pub fn weights(&self) -> Option<&Path> {
        self.files
            .iter()
            .find(|(role, _)| *role == FileRole::Weights)
            .map(|(_, path)| path.as_path())
    }

    /// Memory lower bound = file bytes, all in RAM (`vram` 0: the engine places layers at load).
    ///
    /// # Errors
    /// [`EngineError::Corrupt`] (no weights) | [`EngineError::Io`].
    pub fn file_bytes(&self) -> Result<MemoryEstimate, EngineError> {
        if self.weights().is_none() {
            return Err(EngineError::Corrupt);
        }
        let mut ram = 0u64;
        for (_, path) in &self.files {
            ram += Path::metadata(path)
                .map_err(|e| EngineError::Io(e.to_string()))?
                .len();
        }
        Ok(MemoryEstimate { ram, vram: 0 })
    }
}

/// How to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadOpts {
    /// Context window, tokens (<= model maximum).
    pub context_len: u32,
    pub device: Device,
    /// CPU threads (`None`: engine picks).
    pub threads: Option<u32>,
}

/// Where a model runs (NO adapter pick: graphics adapter order differs between OS and GPU APIs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Device {
    /// GPU when it fits (engine picks the adapter + layers), else CPU.
    #[default]
    Auto,
    Cpu,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmbedRole {
    Query,
    Document,
    /// Same prefix both sides (similarity, clustering).
    Symmetric,
}

/// Vectors + what made them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Embeddings {
    pub vectors: Vec<Vec<f32>>,
    pub fingerprint: EmbeddingFingerprint,
}

/// Matryoshka shape of a model vector: first `dims` values (`None`: all), then L2 length 1 when `normalize`.
///
/// # Errors
/// [`EngineError::Unsupported`]: `dims` 0 | above the model's; [`EngineError::Corrupt`]: a non-finite value.
pub fn matryoshka(
    vector: &[f32],
    dims: Option<u32>,
    normalize: bool,
) -> Result<Vec<f32>, EngineError> {
    let len = match dims {
        None => vector.len(),
        Some(d) => usize::try_from(d)
            .ok()
            .filter(|&d| d > 0 && d <= vector.len())
            .ok_or_else(|| EngineError::Unsupported {
                what: format!("{d} dimensions"),
            })?,
    };
    let mut out = vector[..len].to_vec();
    if out.iter().any(|v| !v.is_finite()) {
        return Err(EngineError::Corrupt);
    }
    if normalize {
        let norm = out
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>()
            .sqrt();
        if norm > 0.0 {
            for v in &mut out {
                #[allow(clippy::cast_possible_truncation)] // unit-length values, f32 range
                {
                    *v = (f64::from(*v) / norm) as f32;
                }
            }
        }
    }
    Ok(out)
}

/// Engine release `have` reads files needing `needed` (llama.cpp tags `b<N>`); either spelled otherwise -> `true`
/// (the load decides).
#[must_use]
pub fn engine_at_least(have: &str, needed: &str) -> bool {
    let build = |tag: &str| tag.strip_prefix('b').and_then(|n| n.parse::<u32>().ok());
    match (build(have), build(needed)) {
        (Some(have), Some(needed)) => have >= needed,
        _ => true,
    }
}

/// Identity of an embedding space: vectors with different fingerprints NEVER compared (index rebuild).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmbeddingFingerprint {
    pub model_id: String,
    pub catalog_version: u32,
    pub dims: u32,
    pub prefix_profile: Option<String>,
}

/// One chat call.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    /// Result of a tool call.
    Tool,
}

/// A tool the model may call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments.
    pub json_schema: Value,
}

/// Shape the answer must take.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum Constraint {
    #[default]
    None,
    /// JSON matching this schema.
    JsonSchema(Value),
    /// GBNF grammar.
    Grammar(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GenEvent {
    Delta(String),
    ToolCall { name: String, args: Value },
    Done { usage: Usage, finish: Finish },
}

/// Tokens used by one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt: u32,
    pub completion: u32,
}

/// Why an answer ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Finish {
    Stop,
    Length,
    Tool,
    Cancelled,
}

/// Transcribed span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub t0_ms: u64,
    pub t1_ms: u64,
    pub text: String,
}

/// Synthesized audio, mono samples -1..1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PcmChunk {
    pub rate: u32,
    pub samples: Vec<f32>,
}

/// Why an engine call failed (`Display` = plain English).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineError {
    OutOfMemory,
    Unsupported {
        what: String,
    },
    Corrupt,
    Cancelled,
    EngineCrashed,
    EngineTooOld {
        needed: String,
    },
    /// Session unknown (never loaded | unloaded | lost in a crash).
    NotLoaded,
    Io(String),
    /// The system refused to start the worker (host side only; D-14: security software | quarantine).
    Blocked,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfMemory => f.write_str("There is not enough memory to load this model."),
            Self::Unsupported { what } => write!(f, "This is not supported here: {what}."),
            Self::Corrupt => f.write_str("The model files are damaged."),
            Self::Cancelled => f.write_str("Stopped."),
            Self::EngineCrashed => f.write_str("The model stopped unexpectedly."),
            Self::EngineTooOld { needed } => {
                write!(f, "This model needs a newer engine ({needed}).")
            }
            Self::NotLoaded => f.write_str("The model is not loaded."),
            Self::Io(e) => write!(f, "The model files could not be read: {e}"),
            Self::Blocked => f.write_str(
                "The model engine could not start: security software may have blocked it.",
            ),
        }
    }
}

impl std::error::Error for EngineError {}

#[cfg(test)]
mod tests;
