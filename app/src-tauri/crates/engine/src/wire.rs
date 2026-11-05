//! Worker wire protocol: JSON lines over the worker's stdin | stdout (one `serde_json` object per line, flushed).
//!
//! Rules:
//! - first line worker -> host = [`Hello`]; host checks `proto` BEFORE reading the rest ([`Probe`]): != [`PROTO`]
//!   -> [`crate::EngineError::EngineTooOld`].
//! - host sends ONE [`Request`] at a time and reads [`Response`]s until the call's final one; only
//!   [`Request::Cancel`] may come while a call runs.
//! - final responses: `estimate_memory` -> [`Response::Memory`]; `load` -> [`Response::Loaded`] (after
//!   [`Response::Progress`]*); `tokenize` -> [`Response::Tokens`]; `count_tokens` -> [`Response::Count`];
//!   `context_len` -> [`Response::ContextLen`]; `embed` -> [`Response::Embeddings`]; `generate` | `transcribe` |
//!   `synthesize` -> [`Response::Done`] (after [`Response::Gen`] | [`Response::Segment`] | [`Response::Pcm`]*); any
//!   call -> [`Response::Error`]. [`Request::Unload`] has no answer: the worker exits.
//! - one loaded session per worker; stdin EOF = exit.

use serde::{Deserialize, Serialize};

use crate::{
    ChatRequest, EmbedRole, Embeddings, EngineCaps, EngineError, GenEvent, LoadOpts,
    MemoryEstimate, ModelFiles, PcmChunk, Segment, SessionId,
};

/// Protocol version; bump on any incompatible change of these types.
pub const PROTO: u32 = 1;

/// Worker's first line: protocol + what the engine can do (`caps.version` = engine release).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub proto: u32,
    pub caps: EngineCaps,
}

/// The part of any [`Hello`] every protocol version keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Probe {
    pub proto: u32,
}

/// Host -> worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    EstimateMemory {
        model: ModelFiles,
        opts: LoadOpts,
    },
    Load {
        model: ModelFiles,
        opts: LoadOpts,
    },
    /// Frees `session`, then the worker exits.
    Unload {
        session: SessionId,
    },
    Tokenize {
        session: SessionId,
        text: String,
    },
    CountTokens {
        session: SessionId,
        text: String,
    },
    ContextLen {
        session: SessionId,
    },
    Embed {
        session: SessionId,
        texts: Vec<String>,
        role: EmbedRole,
        dims: Option<u32>,
        normalize: bool,
    },
    Generate {
        session: SessionId,
        request: ChatRequest,
    },
    Transcribe {
        session: SessionId,
        pcm_16k_mono: Vec<f32>,
        lang: Option<String>,
    },
    Synthesize {
        session: SessionId,
        text: String,
        voice: Option<String>,
        lang: Option<String>,
    },
    /// Stops the running call (trait cancel rules); none running -> nothing.
    Cancel,
}

/// Worker -> host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "re", rename_all = "snake_case")]
pub enum Response {
    Progress {
        value: f32,
    },
    Loaded {
        session: SessionId,
    },
    Memory {
        estimate: MemoryEstimate,
    },
    Tokens {
        ids: Vec<u32>,
    },
    Count {
        tokens: usize,
    },
    ContextLen {
        tokens: u32,
    },
    Embeddings {
        embeddings: Embeddings,
    },
    Gen {
        event: GenEvent,
    },
    Segment {
        segment: Segment,
    },
    Pcm {
        chunk: PcmChunk,
    },
    /// Streaming call ended `Ok`.
    Done,
    Error {
        error: EngineError,
    },
}
