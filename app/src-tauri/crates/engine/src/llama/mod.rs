//! [`LlamaEngine`]: [`Engine`] over llama.cpp (`llama-cpp-2`, vendored build [`LLAMA_CPP`]); runs inside the worker
//! binary (one loaded model per process).
//!
//! - text only: Chat + Embed (tokenize | count | context length on any model); transcribe | synthesize | tool calls
//!   -> `Unsupported` (D-1, D-9).
//! - devices (D-7): `Cpu` = no GPU device, 0 layers, no op offload; `Auto` = the GPU with most memory (none -> CPU),
//!   split none, `fit_params` picks layers -> no fit = `OutOfMemory`.
//! - catalog `min_engine` above [`LLAMA_CPP`] -> `EngineTooOld` before any load (unknown architectures never parsed).
//! - load on a scoped thread (llama progress callback must be `'static`): progress polled + forwarded here; cancel
//!   aborts the load (callback answers `false`).
//! - model leaked for the worker's life (contexts borrow it): unload = worker exit (crate rule) frees everything.
//! - one context per session, kind follows the call: embed (`n_batch` = `n_ubatch` = `n_ctx`, pooling from GGUF, D-12) |
//!   generate (prompt decoded in `n_batch` chunks, cancel between chunks + per token); switching kind rebuilds it.
//! - prompts: catalog [`crate::PromptProfile`] | [`crate::Prefixes`] (crate rule); markup tokenized with special
//!   tokens, message text without.
//! - logs: llama.cpp default logger (stderr; the worker sends fd 1 to stderr too).

use std::ffi::CString;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::{FitError, LlamaModelParams, LlamaSplitMode};
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::{
    json_schema_to_grammar, list_llama_ggml_backend_devices, max_devices, DecodeError,
    LlamaBackendDevice, LlamaBackendDeviceType,
};

use crate::proxy::lock;
use crate::{
    engine_at_least, matryoshka, CancelToken, Capability, ChatRequest, Constraint, Device,
    EmbedRole, EmbeddingFingerprint, Embeddings, Engine, EngineCaps, EngineError, Finish, GenEvent,
    LoadOpts, MemoryEstimate, ModelFiles, PcmChunk, Segment, SessionId, StopScan, Usage,
};

/// llama.cpp build vendored by `llama-cpp-2` =0.1.161 (submodule `d812350`, tag b11429); compared with catalog
/// `minEngine`. Bump together with the crate pin.
pub const LLAMA_CPP: &str = "b11429";

/// Free memory kept per GPU when fitting layers (llama.cpp `--fit-target` default).
const FIT_MARGIN: usize = 1 << 30;
/// `fit_params` log level (warnings and up).
const FIT_LOG_WARN: i32 = 3;
/// Generation batch, tokens.
const GEN_BATCH: u32 = 512;
/// Candidates kept before top-p (llama.cpp default).
const TOP_K: i32 = 40;
/// `dist` seed meaning "random" (llama.cpp `LLAMA_DEFAULT_SEED`).
const RANDOM_SEED: u32 = u32::MAX;
const PROGRESS_POLL: Duration = Duration::from_millis(50);

/// llama.cpp engine (rules: module doc).
pub struct LlamaEngine {
    backend: LlamaBackend,
    devices: Vec<LlamaBackendDevice>,
    next: AtomicU64,
    session: Mutex<Option<Session>>,
}

/// The loaded model.
struct Session {
    id: SessionId,
    model: &'static LlamaModel,
    files: ModelFiles,
    n_ctx: u32,
    threads: i32,
    /// No GPU in use (no op offload).
    cpu: bool,
    context: Option<(Kind, LlamaContext<'static>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Embed,
    Generate,
}

impl LlamaEngine {
    /// Backends loaded (dynamic builds: from the worker's folder), devices listed. Once per process.
    ///
    /// # Errors
    /// [`EngineError::Io`]: backend already started | worker folder unreadable.
    pub fn new() -> Result<Self, EngineError> {
        #[cfg(feature = "dynamic-backends")]
        {
            let exe = std::env::current_exe().map_err(|e| EngineError::Io(e.to_string()))?;
            let dir = exe
                .parent()
                .filter(|dir| dir.to_str().is_some())
                .ok_or_else(|| EngineError::Io("engine folder name not readable".into()))?;
            llama_cpp_2::llama_backend::load_backends_from_path(dir);
        }
        let backend = LlamaBackend::init().map_err(|e| EngineError::Io(e.to_string()))?;
        Ok(Self {
            backend,
            devices: list_llama_ggml_backend_devices(),
            next: AtomicU64::new(1),
            session: Mutex::new(None),
        })
    }

    /// GPU with the most memory (D-7: one device, no adapter picker).
    fn gpu(&self) -> Option<&LlamaBackendDevice> {
        self.devices
            .iter()
            .filter(|d| {
                matches!(
                    d.device_type,
                    LlamaBackendDeviceType::Gpu | LlamaBackendDeviceType::IntegratedGpu
                )
            })
            .max_by_key(|d| d.memory_total)
    }

    /// Runs `f` on the loaded session `id`.
    fn with<T>(
        &self,
        id: SessionId,
        f: impl FnOnce(&mut Session) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let mut slot = lock(&self.session);
        let session = slot
            .as_mut()
            .filter(|s| s.id == id)
            .ok_or(EngineError::NotLoaded)?;
        f(session)
    }
}

impl Session {
    /// Context of `kind` (the other kind freed first).
    fn context(
        &mut self,
        backend: &LlamaBackend,
        kind: Kind,
    ) -> Result<&mut LlamaContext<'static>, EngineError> {
        if self.context.as_ref().is_none_or(|(k, _)| *k != kind) {
            self.context = None;
            let params = context_params(self.n_ctx, self.threads, self.cpu);
            let params = match kind {
                Kind::Embed => params
                    .with_embeddings(true)
                    .with_n_batch(self.n_ctx)
                    .with_n_ubatch(self.n_ctx),
                Kind::Generate => params.with_n_batch(GEN_BATCH.min(self.n_ctx)),
            };
            let context = self
                .model
                .new_context(backend, params)
                .map_err(|_| EngineError::OutOfMemory)?;
            self.context = Some((kind, context));
        }
        match &mut self.context {
            Some((_, context)) => Ok(context),
            None => Err(EngineError::OutOfMemory),
        }
    }
}

fn context_params(n_ctx: u32, threads: i32, cpu: bool) -> LlamaContextParams {
    LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx))
        .with_n_threads(threads)
        .with_n_threads_batch(threads)
        .with_op_offload(!cpu)
}

/// Half the logical CPUs (~ physical cores), at least 1.
fn default_threads() -> i32 {
    thread::available_parallelism()
        .ok()
        .and_then(|n| i32::try_from(n.get() / 2).ok())
        .unwrap_or(1)
        .max(1)
}

fn run_error(e: &DecodeError) -> EngineError {
    match e {
        DecodeError::NoKvCacheSlot => EngineError::OutOfMemory,
        e => EngineError::Io(format!("model run failed: {e}")),
    }
}

fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::Io(e.to_string())
}

/// Loads `path` with `gpu` (`None`: CPU only); runs on the load thread (params never cross threads).
fn load_model(
    backend: &LlamaBackend,
    path: &Path,
    gpu: Option<usize>,
    n_ctx: u32,
    threads: i32,
    progress: Arc<AtomicU32>,
    cancel: CancelToken,
) -> Result<LlamaModel, EngineError> {
    let c_path = path
        .to_str()
        .and_then(|p| CString::new(p).ok())
        .ok_or_else(|| EngineError::Io("model path not readable".into()))?;
    let params = LlamaModelParams::default().with_progress_callback(move |value| {
        progress.store(value.to_bits(), Ordering::Relaxed);
        !cancel.is_cancelled()
    });
    let mut params = match gpu {
        None => params
            .with_devices(&[])
            .map_err(internal)?
            .with_n_gpu_layers(0),
        Some(index) => params
            .with_devices(&[index])
            .map_err(internal)?
            .with_split_mode(LlamaSplitMode::None),
    };
    if gpu.is_some() {
        let mut cparams = context_params(n_ctx, threads, false);
        let mut margins = vec![FIT_MARGIN; max_devices()];
        std::pin::Pin::new(&mut params)
            .fit_params(&c_path, &mut cparams, &mut margins, n_ctx, FIT_LOG_WARN)
            .map_err(|e| match e {
                FitError::Failure => EngineError::OutOfMemory,
                FitError::Error => EngineError::Corrupt,
            })?;
    }
    // llama.cpp cannot tell damaged files from unknown ones (catalog `min_engine` caught those before)
    LlamaModel::load_from_file(backend, path, &params).map_err(|_| EngineError::Corrupt)
}

impl Engine for LlamaEngine {
    fn caps(&self) -> EngineCaps {
        EngineCaps {
            formats: vec!["gguf".into()],
            capabilities: vec![Capability::Chat, Capability::Embed],
            gpu: self.gpu().is_some(),
            version: LLAMA_CPP.into(),
        }
    }

    fn estimate_memory(
        &self,
        model: &ModelFiles,
        _opts: &LoadOpts,
    ) -> Result<MemoryEstimate, EngineError> {
        model.file_bytes()
    }

    fn load(
        &self,
        model: ModelFiles,
        opts: LoadOpts,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<SessionId, EngineError> {
        if let Some(needed) = model
            .min_engine
            .as_deref()
            .filter(|needed| !engine_at_least(LLAMA_CPP, needed))
        {
            return Err(EngineError::EngineTooOld {
                needed: needed.to_owned(),
            });
        }
        let mut slot = lock(&self.session);
        if slot.is_some() {
            return Err(EngineError::Unsupported {
                what: "a second model in one engine worker".into(),
            });
        }
        let path = model.weights().ok_or(EngineError::Corrupt)?.to_path_buf();
        if !path.is_file() {
            return Err(EngineError::Io(format!("{} not found", path.display())));
        }
        let threads = opts
            .threads
            .and_then(|t| i32::try_from(t).ok())
            .filter(|&t| t > 0)
            .unwrap_or_else(default_threads);
        let gpu = match opts.device {
            Device::Cpu => None,
            Device::Auto => self.gpu().map(|d| d.index),
        };
        let shared = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let backend = &self.backend;
        let loaded = thread::scope(|scope| {
            let (seen, stop) = (Arc::clone(&shared), cancel.clone());
            let path = &path;
            let job = scope.spawn(move || {
                load_model(backend, path, gpu, opts.context_len, threads, seen, stop)
            });
            let mut sent = f32::NEG_INFINITY;
            while !job.is_finished() {
                let value = f32::from_bits(shared.load(Ordering::Relaxed));
                if value > sent {
                    sent = value;
                    progress(value);
                }
                thread::sleep(PROGRESS_POLL);
            }
            job.join()
        });
        let loaded = match loaded {
            Ok(result) => result,
            Err(_) => Err(EngineError::EngineCrashed),
        };
        let llama = match loaded {
            Err(_) if cancel.is_cancelled() => return Err(EngineError::Cancelled),
            other => other?,
        };
        progress(1.0);
        let llama: &'static LlamaModel = Box::leak(Box::new(llama));
        let train = llama.n_ctx_train();
        let n_ctx = match opts.context_len {
            0 => train,
            n => n.min(train.max(1)),
        };
        let id = SessionId(self.next.fetch_add(1, Ordering::Relaxed));
        *slot = Some(Session {
            id,
            model: llama,
            files: model,
            n_ctx,
            threads,
            cpu: gpu.is_none(),
            context: None,
        });
        Ok(id)
    }

    fn unload(&self, session: SessionId) {
        let mut slot = lock(&self.session);
        if slot.as_ref().is_some_and(|s| s.id == session) {
            *slot = None;
        }
    }

    fn tokenize(&self, session: SessionId, text: &str) -> Result<Vec<u32>, EngineError> {
        self.with(session, |s| {
            s.model
                .vocab()
                .tokenize(text.as_bytes(), false, false)
                .into_iter()
                .map(|t| u32::try_from(t.0).map_err(|_| EngineError::Corrupt))
                .collect()
        })
    }

    fn count_tokens(&self, session: SessionId, text: &str) -> Result<usize, EngineError> {
        self.with(session, |s| {
            Ok(s.model
                .vocab()
                .tokenize(text.as_bytes(), false, false)
                .len())
        })
    }

    fn context_len(&self, session: SessionId) -> Result<u32, EngineError> {
        self.with(session, |s| Ok(s.n_ctx))
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
        self.with(session, |s| {
            let model = s.model;
            let prefix = s
                .files
                .prefixes
                .as_ref()
                .map_or(String::new(), |p| p.of(role).to_owned());
            let n_ctx = usize::try_from(s.n_ctx).map_err(internal)?;
            let fingerprint = EmbeddingFingerprint {
                model_id: s.files.model_id.clone(),
                catalog_version: s.files.catalog_version,
                dims: dims.unwrap_or_else(|| u32::try_from(model.n_embd_out()).unwrap_or(0)),
                prefix_profile: s.files.prefix_profile.clone(),
            };
            let context = s.context(&self.backend, Kind::Embed)?;
            let mut batch = LlamaBatch::new(n_ctx, 1);
            let mut vectors = Vec::with_capacity(texts.len());
            for text in texts {
                if cancel.is_cancelled() {
                    return Err(EngineError::Cancelled);
                }
                // person's text: special tokens never parsed; longer than the context -> cut (embedding practice)
                let mut tokens =
                    model
                        .vocab()
                        .tokenize(format!("{prefix}{text}").as_bytes(), true, false);
                tokens.truncate(n_ctx);
                context.clear_kv_cache();
                batch.clear();
                batch.add_sequence(&tokens, 0, false).map_err(internal)?;
                context.decode(&mut batch).map_err(|e| run_error(&e))?;
                let raw = context
                    .embeddings_seq_ith(0)
                    .map_err(|_| EngineError::Unsupported {
                        what: "embeddings from this model".into(),
                    })?;
                vectors.push(matryoshka(raw, dims, normalize)?);
            }
            Ok(Embeddings {
                vectors,
                fingerprint,
            })
        })
    }

    fn generate(
        &self,
        session: SessionId,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        if !request.tools.is_empty() {
            return Err(EngineError::Unsupported {
                what: "tool calls".into(),
            });
        }
        self.with(session, |s| {
            let model = s.model;
            let profile = s.files.prompt.clone().ok_or(EngineError::Unsupported {
                what: "chat with this model".into(),
            })?;
            let vocab = model.vocab();
            let mut prompt: Vec<LlamaToken> = Vec::new();
            for (i, piece) in profile.render(&request.messages)?.iter().enumerate() {
                // first piece adds BOS (GGUF `add_bos_token`)
                prompt.extend(vocab.tokenize(piece.text.as_bytes(), i == 0, piece.markup));
            }
            let n_ctx = usize::try_from(s.n_ctx).map_err(internal)?;
            if prompt.is_empty() || prompt.len() >= n_ctx {
                return Err(EngineError::Unsupported {
                    what: "a prompt longer than the model context".into(),
                });
            }
            let mut sampler = sampler(model, request)?;
            let context = s.context(&self.backend, Kind::Generate)?;
            context.clear_kv_cache();
            let n_batch = usize::try_from(context.n_batch()).map_err(internal)?.max(1);
            let mut batch = LlamaBatch::new(n_batch, 1);
            let prompt_tokens = u32::try_from(prompt.len()).map_err(internal)?;
            let done = |sink: &mut dyn FnMut(GenEvent), completion, finish| {
                sink(GenEvent::Done {
                    usage: Usage {
                        prompt: prompt_tokens,
                        completion,
                    },
                    finish,
                });
                Ok(())
            };
            let last = prompt.len() - 1;
            for (c, chunk) in prompt.chunks(n_batch).enumerate() {
                if cancel.is_cancelled() {
                    return done(sink, 0, Finish::Cancelled);
                }
                batch.clear();
                for (j, token) in chunk.iter().enumerate() {
                    let at = c * n_batch + j;
                    let pos = i32::try_from(at).map_err(internal)?;
                    batch.add(*token, pos, &[0], at == last).map_err(internal)?;
                }
                context.decode(&mut batch).map_err(|e| run_error(&e))?;
            }
            let mut scan = StopScan::new(profile.stop.iter().chain(&request.stop));
            let mut bytes = Vec::new();
            let mut at = prompt.len();
            let max = match request.max_tokens {
                0 => u32::MAX,
                n => n,
            };
            let mut completion = 0u32;
            let finish = loop {
                if cancel.is_cancelled() {
                    break Finish::Cancelled;
                }
                if completion >= max || at >= n_ctx {
                    break Finish::Length;
                }
                let token = sampler.sample(context, -1);
                if vocab.is_eog(token) {
                    break Finish::Stop;
                }
                completion += 1;
                bytes.extend(vocab.token_to_piece(token, false, None));
                let text = take_utf8(&mut bytes);
                if !text.is_empty() {
                    let (out, stopped) = scan.push(&text);
                    if !out.is_empty() {
                        sink(GenEvent::Delta(out));
                    }
                    if stopped {
                        break Finish::Stop;
                    }
                }
                batch.clear();
                let pos = i32::try_from(at).map_err(internal)?;
                batch.add(token, pos, &[0], true).map_err(internal)?;
                context.decode(&mut batch).map_err(|e| run_error(&e))?;
                at += 1;
            };
            let rest = scan.finish();
            if !rest.is_empty() && finish != Finish::Cancelled {
                sink(GenEvent::Delta(rest));
            }
            done(sink, completion, finish)
        })
    }

    fn transcribe(
        &self,
        _session: SessionId,
        _pcm_16k_mono: &[f32],
        _lang: Option<&str>,
        _sink: &mut dyn FnMut(Segment),
        _cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        Err(EngineError::Unsupported {
            what: "speech to text".into(),
        })
    }

    fn synthesize(
        &self,
        _session: SessionId,
        _text: &str,
        _voice: Option<&str>,
        _lang: Option<&str>,
        _sink: &mut dyn FnMut(PcmChunk),
        _cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        Err(EngineError::Unsupported {
            what: "text to speech".into(),
        })
    }
}

/// Sampler chain: constraint grammar first, then top-k + top-p + temperature + seeded draw (temperature 0: greedy).
fn sampler(model: &LlamaModel, request: &ChatRequest) -> Result<LlamaSampler, EngineError> {
    let shape = |e: &dyn std::fmt::Display| EngineError::Unsupported {
        what: format!("this answer shape ({e})"),
    };
    let mut chain = Vec::new();
    let grammar = match &request.constraint {
        Constraint::None => None,
        Constraint::Grammar(grammar) => Some(grammar.clone()),
        Constraint::JsonSchema(schema) => {
            Some(json_schema_to_grammar(&schema.to_string()).map_err(|e| shape(&e))?)
        }
    };
    if let Some(grammar) = grammar {
        chain.push(LlamaSampler::grammar(model, &grammar, "root").map_err(|e| shape(&e))?);
    }
    let sampling = request.sampling;
    if sampling.temperature <= 0.0 {
        chain.push(LlamaSampler::greedy());
    } else {
        let seed = sampling.seed.map_or(RANDOM_SEED, |seed| {
            u32::try_from(seed & u64::from(u32::MAX)).unwrap_or(RANDOM_SEED)
        });
        chain.extend([
            LlamaSampler::top_k(TOP_K),
            LlamaSampler::top_p(sampling.top_p, 1),
            LlamaSampler::temp(sampling.temperature),
            LlamaSampler::dist(seed),
        ]);
    }
    Ok(LlamaSampler::chain_simple(chain))
}

/// Complete UTF-8 text at the front of `bytes` (token pieces may split a character); invalid bytes -> U+FFFD.
fn take_utf8(bytes: &mut Vec<u8>) -> String {
    let valid = match std::str::from_utf8(bytes) {
        Ok(text) => text.len(),
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => return String::from_utf8_lossy(&std::mem::take(bytes)).into_owned(),
    };
    let text = String::from_utf8_lossy(&bytes[..valid]).into_owned();
    bytes.drain(..valid);
    text
}
