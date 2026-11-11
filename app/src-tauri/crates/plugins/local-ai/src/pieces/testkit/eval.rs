//! Eval harness of a piece job, step 1 of an eval set: the piece of every case through the production piece path
//! ([`answer`] of the job: its prompt, sampling and answer cap; too-long + length + thinking discards) on one model
//! -> [`Outputs`], written as JSON for the plugin's report (TS: the plugin's own `restore` + guards + scores). What a
//! case is, what its record holds and the checks of the cases file stay with the plugin.
//!
//! - [`Model::fake`]: a session of the in-process [`Fake`] (the harness without a model; always run).
//! - [`Model::real`], by hand only: the REAL worker + a local chat GGUF, named by the environment. `PERMARK_WORKER` =
//!   worker build (debug override), `PERMARK_EVAL_GGUF` = chat model, `PERMARK_EVAL_PROFILE` = catalog prompt profile
//!   (default `gemma-4`), `PERMARK_EVAL_OUT` = outputs JSON path, `PERMARK_SMOKE_DEVICE=auto` -> GPU (default CPU).

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use permark_core::fsx;
use serde::Serialize;

use super::Fake;
use crate::catalog::{self, Needs};
use crate::engine::files::{self, WorkerFiles};
use crate::engine::{
    CancelToken, ChatRequest, Device, EngineError, FileRole, GenEvent, LoadOpts, ModelFiles,
    SessionId, Usage,
};
use crate::pieces::{answer, preamble_tokens, Models, PieceJob, PieceResult, Session};
use crate::runtime::{Runtime, WorkerLog};

/// Prompt profile of a real model when the environment names none.
const PROFILE: &str = "gemma-4";

/// A loaded model for the cases of an eval + the facts of its load (Drop unloads it).
pub struct Model {
    session: Box<dyn Session>,
    profile: String,
    name: String,
    on_gpu: Option<bool>,
    load: Duration,
}

impl Model {
    /// A session of `source`, the in-process fake: it echoes the last user turn word by word.
    ///
    /// # Panics
    /// The fake refuses the session.
    #[must_use]
    pub fn fake(source: &Fake) -> Self {
        let session = source
            .open(&|_| {}, &CancelToken::new())
            .unwrap_or_else(|e| panic!("fake session: {e:?}"));
        Self {
            session,
            profile: "fake".into(),
            name: "fake".into(),
            on_gpu: None,
            load: Duration::ZERO,
        }
    }

    /// The REAL worker with the chat model the environment names (module doc), loaded with the context window of
    /// job `J`, + the path its outputs go to. `dir`: an empty folder for the worker's store and log.
    ///
    /// # Panics
    /// `PERMARK_WORKER` | `PERMARK_EVAL_GGUF` | `PERMARK_EVAL_OUT` not set, no such prompt profile, the load fails.
    #[must_use]
    pub fn real<J: PieceJob>(dir: &Path) -> (Self, PathBuf) {
        let env = |key| std::env::var_os(key).map(PathBuf::from);
        let (Some(_), Some(gguf), Some(out)) = (
            env("PERMARK_WORKER"),
            env("PERMARK_EVAL_GGUF"),
            env("PERMARK_EVAL_OUT"),
        ) else {
            panic!("set PERMARK_WORKER, PERMARK_EVAL_GGUF and PERMARK_EVAL_OUT");
        };
        let profile = std::env::var("PERMARK_EVAL_PROFILE").unwrap_or_else(|_| PROFILE.into());
        let auto = std::env::var("PERMARK_SMOKE_DEVICE").is_ok_and(|d| d == "auto");
        let runtime = Arc::new(Runtime::new(
            Arc::new(WorkerFiles::new(
                None,
                dir.join(files::DIR),
                Arc::new(|_, _| {}),
            )),
            Arc::new(WorkerLog::new(dir.join("worker.log"))),
        ));
        let prompt = catalog::bundled()
            .unwrap_or_else(|e| panic!("catalog: {e:?}"))
            .prompt_profiles
            .get(&profile)
            .cloned()
            .unwrap_or_else(|| panic!("no prompt profile {profile}"));
        let model = ModelFiles {
            model_id: "eval".into(),
            catalog_version: 1,
            files: vec![(FileRole::Weights, gguf.clone())],
            prompt: Some(prompt),
            ..ModelFiles::default()
        };
        let opts = LoadOpts {
            context_len: J::CONTEXT,
            device: if auto { Device::Auto } else { Device::Cpu },
            threads: None,
        };
        let small = Needs {
            ram_min: 1,
            ram_rec: 1,
            vram_rec: 1,
        };
        let started = Instant::now();
        let (id, _) = runtime
            .load("eval", &small, model, || opts, &|_| {}, &CancelToken::new())
            .unwrap_or_else(|e| panic!("load: {e:?}"));
        let load = started.elapsed();
        let on_gpu = runtime.engine().on_gpu(id);
        let name = gguf
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let model = Self {
            session: Box::new(Worker { runtime, id }),
            profile,
            name,
            on_gpu: Some(on_gpu),
            load,
        };
        (model, out)
    }
}

/// Real worker session (Drop unloads).
struct Worker {
    runtime: Arc<Runtime>,
    id: SessionId,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.runtime.unload(self.id);
    }
}

impl Session for Worker {
    fn serves(&self) -> bool {
        true
    }

    fn count_tokens(&self, text: &str) -> Result<usize, EngineError> {
        self.runtime.engine().count_tokens(self.id, text)
    }

    fn generate(
        &self,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        self.runtime
            .engine()
            .generate(self.id, request, sink, cancel)
    }
}

/// Session that keeps the token counts of its last `generate`: the answer cap asked + the usage from `Done`.
struct Metered {
    inner: Box<dyn Session>,
    max_tokens: Cell<u32>,
    usage: Cell<Usage>,
}

impl Session for Metered {
    fn serves(&self) -> bool {
        self.inner.serves()
    }

    fn count_tokens(&self, text: &str) -> Result<usize, EngineError> {
        self.inner.count_tokens(text)
    }

    fn generate(
        &self,
        request: &ChatRequest,
        sink: &mut dyn FnMut(GenEvent),
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        self.max_tokens.set(request.max_tokens);
        self.inner.generate(
            request,
            &mut |event| {
                if let GenEvent::Done { usage, .. } = &event {
                    self.usage.set(*usage);
                }
                sink(event);
            },
            cancel,
        )
    }
}

/// What one case got: the runner's result + the facts of its request (token counts 0: the piece never reached the
/// model, too long for the context).
#[derive(Debug)]
pub struct Answered {
    /// The answer as the runner passes it on | why it dropped it.
    pub result: PieceResult,
    /// Time of the answer, ms.
    pub ms: u64,
    /// Tokens of the whole prompt, as the engine counted them.
    pub prompt_tokens: u32,
    /// Answer tokens.
    pub tokens: u32,
    /// Answer cap asked.
    pub max_tokens: u32,
}

/// Outputs file of an eval run (read by the plugin's report): the facts of the run + one record per case, in order.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outputs<R> {
    /// Prompt profile the chat was rendered with.
    pub profile: String,
    /// File name of the model.
    pub model: String,
    /// `None`: no real model.
    pub on_gpu: Option<bool>,
    /// Time of the model load.
    pub load_ms: u64,
    /// Time of every answer together.
    pub total_ms: u64,
    /// Answer tokens per second over `total_ms`.
    pub tok_per_sec: f64,
    /// One per case, in case order: the plugin's own shape.
    pub records: Vec<R>,
}

impl<R> Outputs<R> {
    /// The run in one line, for the console.
    #[must_use]
    pub fn summary(&self) -> String {
        let Self {
            model,
            on_gpu,
            load_ms,
            total_ms,
            tok_per_sec,
            records,
            ..
        } = self;
        let cases = records.len();
        format!(
            "eval: {cases} cases on {model}, gpu {on_gpu:?}, load {load_ms} ms, generate {total_ms} ms, \
             {tok_per_sec:.1} tok/s"
        )
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Every case in order through [`answer`] of job `J` on `model`: `asked` gives the run parameters + the piece text
/// of a case, `record` makes its line of the outputs.
///
/// # Errors
/// Engine failure (an answer cancelled counts as one: nothing cancels an eval).
pub fn evaluate<J: PieceJob, C, R>(
    model: Model,
    cases: &[C],
    asked: impl Fn(&C) -> (&J::Params, &str),
    record: impl Fn(&C, Answered) -> R,
) -> Result<Outputs<R>, EngineError> {
    let session = Metered {
        inner: model.session,
        max_tokens: Cell::new(0),
        usage: Cell::default(),
    };
    let cancel = CancelToken::new();
    let (mut records, mut total, mut tokens) =
        (Vec::with_capacity(cases.len()), Duration::ZERO, 0u32);
    for case in cases {
        let (params, text) = asked(case);
        let preamble = preamble_tokens::<J>(&session, params)?;
        session.max_tokens.set(0);
        session.usage.set(Usage::default());
        let started = Instant::now();
        let result = answer::<J>(&session, params, preamble, text, &cancel)?
            .ok_or(EngineError::Cancelled)?;
        let elapsed = started.elapsed();
        let usage = session.usage.get();
        total += elapsed;
        tokens = tokens.saturating_add(usage.completion);
        let answered = Answered {
            result,
            ms: millis(elapsed),
            prompt_tokens: usage.prompt,
            tokens: usage.completion,
            max_tokens: session.max_tokens.get(),
        };
        records.push(record(case, answered));
    }
    let secs = total.as_secs_f64();
    Ok(Outputs {
        profile: model.profile,
        model: model.name,
        on_gpu: model.on_gpu,
        load_ms: millis(model.load),
        total_ms: millis(total),
        tok_per_sec: if secs > 0.0 {
            f64::from(tokens) / secs
        } else {
            0.0
        },
        records,
    })
}

/// `outputs` as pretty JSON at `path`, written whole or not at all.
///
/// # Panics
/// `outputs` does not serialize, the file cannot be written.
pub fn write(path: &Path, outputs: &impl Serialize) {
    let json = serde_json::to_vec_pretty(outputs).unwrap_or_else(|e| panic!("outputs: {e}"));
    fsx::write_atomic(path, &json).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
