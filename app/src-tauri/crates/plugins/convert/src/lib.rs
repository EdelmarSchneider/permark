//! Plugin `convert`: data from JSON, JSON Lines, YAML, XML, CSV | TSV, TOON and TOML turned into TOON and other
//! compact formats (and back), with the tokens each choice would take and an honest word when one does not help.
//!
//! RPC for the windows: `analyze` (read the source, write every target, count, judge), `result` (one target's
//! text), `save` (a new file next to the source, never over one), `cancel`, `close`. The work runs on the caller's
//! worker thread and stops at its next check when cancelled. Files are read and written through the shell's
//! [`WindowFilesApi`] (the window's own workspace and boundary). Nothing leaves the computer.

mod read;
mod shape;
mod write;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use permark_core::activity::Fact;
use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, AppServices, Manifest, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running, WindowFilesApi,
};
use permark_core::settings::ProjectConfig;
use permark_core::tokens::{estimate, TokenCounterApi};
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

pub use read::read;
pub use shape::{shape, Shape};
pub use write::write;

const ID: PluginId = PluginId::of("convert");
const FMT_TOON: PluginId = PluginId::of("fmt-toon");

/// The largest source read (bytes).
pub const MAX_SOURCE: u64 = 50 * 1024 * 1024;
/// Outputs up to this size are kept for Copy | Save; bigger ones are written again when asked.
const KEEP_OUTPUT: usize = 8 * 1024 * 1024;
/// Lines of a preview.
const PREVIEW_LINES: usize = 40;

/// What a source is read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Json,
    JsonLines,
    Yaml,
    Xml,
    Csv,
    Tsv,
    Toon,
    Toml,
}

impl Source {
    pub const ALL: [Self; 8] = [
        Self::Json,
        Self::JsonLines,
        Self::Yaml,
        Self::Xml,
        Self::Csv,
        Self::Tsv,
        Self::Toon,
        Self::Toml,
    ];

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::JsonLines => "jsonl",
            Self::Yaml => "yaml",
            Self::Xml => "xml",
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Toon => "toon",
            Self::Toml => "toml",
        }
    }

    /// By id | extension | fence language.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "json" | "jsonc" | "geojson" | "webmanifest" => Self::Json,
            "jsonl" | "ndjson" | "jsonlines" => Self::JsonLines,
            "yaml" | "yml" => Self::Yaml,
            "xml" => Self::Xml,
            "csv" => Self::Csv,
            "tsv" | "tab" => Self::Tsv,
            "toon" => Self::Toon,
            "toml" => Self::Toml,
            _ => return None,
        })
    }

    /// What a text looks like when nothing names it.
    #[must_use]
    pub fn sniff(text: &str) -> Self {
        let t = text.trim_start_matches('\u{feff}').trim_start();
        if t.starts_with('<') {
            return Self::Xml;
        }
        let first = t.lines().next().unwrap_or("");
        let toml_header = |l: &str| {
            let l = l.trim();
            l.len() > 2
                && l.starts_with('[')
                && l.ends_with(']')
                && l.trim_matches(['[', ']'])
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '"' | ' '))
        };
        if toml_header(first) && serde_json::from_str::<serde::de::IgnoredAny>(t).is_err() {
            return Self::Toml;
        }
        if t.starts_with('{') || t.starts_with('[') {
            if serde_json::from_str::<serde::de::IgnoredAny>(t).is_ok() {
                return Self::Json;
            }
            let lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).collect();
            if lines.len() > 1
                && lines
                    .iter()
                    .all(|l| serde_json::from_str::<serde::de::IgnoredAny>(l).is_ok())
            {
                return Self::JsonLines;
            }
            if permark_plugin_fmt_toon::decode(t).is_ok() {
                return Self::Toon;
            }
            return Self::Json;
        }
        // a TOON header: `key[N]...:` before any other colon
        if t.lines().any(|l| {
            let l = l.trim_start();
            let bracket = l.find('[');
            let colon = l.find(':');
            matches!((bracket, colon), (Some(b), Some(c)) if b < c && l[b + 1..].starts_with(|ch: char| ch.is_ascii_digit()))
        }) {
            return Self::Toon;
        }
        if first.contains('\t') {
            return Self::Tsv;
        }
        if first.contains(',') && !first.contains(": ") {
            return Self::Csv;
        }
        if t.starts_with('[') || t.lines().any(|l| l.trim_start().starts_with("[[")) {
            return Self::Toml;
        }
        if t.lines().any(|l| {
            let l = l.trim();
            !l.starts_with('#') && l.contains(" = ") && !l.contains(": ")
        }) {
            return Self::Toml;
        }
        Self::Yaml
    }
}

/// What data is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    Toon,
    Json,
    JsonLines,
    Csv,
    Tsv,
    Yaml,
    JsonPretty,
}

impl Target {
    pub const ALL: [Self; 7] = [
        Self::Toon,
        Self::Json,
        Self::JsonLines,
        Self::Csv,
        Self::Tsv,
        Self::Yaml,
        Self::JsonPretty,
    ];

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Toon => "toon",
            Self::Json => "json",
            Self::JsonLines => "jsonl",
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Yaml => "yaml",
            Self::JsonPretty => "jsonPretty",
        }
    }

    #[must_use]
    pub fn ext(self) -> &'static str {
        match self {
            Self::Toon => "toon",
            Self::Json | Self::JsonPretty => "json",
            Self::JsonLines => "jsonl",
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Yaml => "yaml",
        }
    }

    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }
}

/// Why the work stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    Cancelled,
    TooLarge,
    Invalid { line: u32, detail: String },
}

/// The words of what a conversion lost, once each, in the order they happened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Losses(Vec<&'static str>);

impl Losses {
    pub fn add(&mut self, word: &'static str) {
        if !self.0.contains(&word) {
            self.0.push(word);
        }
    }

    #[must_use]
    pub fn words(&self) -> Vec<String> {
        self.0.iter().map(|w| (*w).to_owned()).collect()
    }
}

/// The cancel flag of one job.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Counts work and looks at the cancel flag every 4 096 steps.
pub struct Tick<'a> {
    cancel: &'a Cancel,
    n: u32,
}

impl<'a> Tick<'a> {
    #[must_use]
    pub fn new(cancel: &'a Cancel) -> Self {
        Self { cancel, n: 0 }
    }

    /// # Errors
    /// The job was cancelled.
    pub fn tick(&mut self) -> Result<(), Failure> {
        self.n = self.n.wrapping_add(1);
        if self.n.is_multiple_of(4096) && self.cancel.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        Ok(())
    }

    /// # Errors
    /// The job was cancelled.
    pub fn check(&self) -> Result<(), Failure> {
        if self.cancel.is_cancelled() {
            Err(Failure::Cancelled)
        } else {
            Ok(())
        }
    }
}

// ---- answers ------------------------------------------------------------------------------------------------------

/// One target of an analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TargetView {
    pub id: String,
    pub ext: String,
    /// `None`: not available for this data (`reason` says why).
    #[ts(type = "number | null")]
    pub tokens: Option<u64>,
    /// `good` | `poor` | `no` (not available).
    pub fit: String,
    /// `tabular` | `smaller` | `compact` | `lines` | `readable` | `noGain` | `deep` | `notUniform` | `notTable` |
    /// `notArray` | `failed`.
    pub reason: String,
    /// Fewest tokens among the targets that fit.
    pub best: bool,
    /// The first lines.
    pub preview: String,
    /// What writing this target loses.
    pub notes: Vec<String>,
}

impl TargetView {
    fn not_available(target: Target, reason: &str) -> Self {
        Self {
            id: target.id().to_owned(),
            ext: target.ext().to_owned(),
            tokens: None,
            fit: "no".to_owned(),
            reason: reason.to_owned(),
            best: false,
            preview: String::new(),
            notes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Analysis {
    /// The source as read (`json` | `jsonl` | `yaml` | `xml` | `csv` | `tsv` | `toon` | `toml`).
    pub from: String,
    #[ts(type = "number")]
    pub bytes: u64,
    #[ts(type = "number")]
    pub source_tokens: u64,
    /// The tokenizer the numbers belong to (`None`: the app's own estimate).
    pub tokenizer: Option<String>,
    /// The numbers are estimates.
    pub approx: bool,
    /// What reading the source lost.
    pub notes: Vec<String>,
    pub targets: Vec<TargetView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "ConvertAnswer")]
pub enum Answer {
    Ready {
        analysis: Analysis,
    },
    /// `tooLarge` | `invalid` (with the line, 0 = unknown, and the reader's message).
    Refused {
        reason: String,
        line: u32,
        detail: String,
    },
    Cancelled,
}

// ---- the work -----------------------------------------------------------------------------------------------------

/// Tokens of a text: the token count plugin's primary tokenizer when it serves, else the app's estimate.
pub type Counter<'a> = &'a dyn Fn(&str) -> (u64, Option<String>, bool);

/// The counter of the app: the token count plugin's primary tokenizer (app choice) when ready, else the estimate.
fn counter_of(app: Option<&AppServices>) -> impl Fn(&str) -> (u64, Option<String>, bool) + '_ {
    let api = app.and_then(|a| a.services.get::<TokenCounterApi>());
    let config = ProjectConfig::default();
    move |text: &str| {
        if let Some(counted) = api.as_ref().and_then(|api| api.0.primary(&config, text)) {
            return (
                counted.value,
                Some(counted.tokenizer.to_owned()),
                counted.approx,
            );
        }
        (estimate(text), None, true)
    }
}

fn preview(text: &str) -> String {
    let mut out = String::new();
    for (i, line) in text.lines().enumerate() {
        if i == PREVIEW_LINES {
            out.push_str("…\n");
            break;
        }
        // very long lines are cut: a preview, not the result
        if line.chars().count() > 400 {
            out.extend(line.chars().take(400));
            out.push('…');
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// A finished analysis and what it keeps for Copy | Save.
pub struct Done {
    pub analysis: Analysis,
    pub value: Value,
    pub outputs: HashMap<Target, String>,
}

/// Reads `text` as `source`, writes every target that fits the data, counts and judges each.
///
/// # Errors
/// The text is too large or not valid for the source, or the work was cancelled.
pub fn analyze(
    text: &str,
    source: Source,
    cancel: &Cancel,
    count: Counter<'_>,
) -> Result<Done, Failure> {
    if text.len() as u64 > MAX_SOURCE {
        return Err(Failure::TooLarge);
    }
    let mut tick = Tick::new(cancel);
    let mut losses = Losses::default();
    let value = read(source, text, &mut tick, &mut losses)?;
    tick.check()?;
    let (source_tokens, tokenizer, approx) = count(text);
    let data = shape(&value);
    let mut outputs = HashMap::new();
    let mut targets = Vec::new();
    let mut compact_tokens = None;
    for target in Target::ALL {
        tick.check()?;
        let unavailable = match target {
            Target::Csv | Target::Tsv if !data.table => Some("notTable"),
            Target::Csv | Target::Tsv if data.mixed => Some("notTable"),
            Target::JsonLines if !data.array => Some("notArray"),
            _ => None,
        };
        if let Some(reason) = unavailable {
            targets.push(TargetView::not_available(target, reason));
            continue;
        }
        let mut target_losses = Losses::default();
        let output = match write(target, &value, &mut tick, &mut target_losses) {
            Ok(output) => output,
            Err(Failure::Cancelled) => return Err(Failure::Cancelled),
            Err(_) => {
                targets.push(TargetView::not_available(target, "failed"));
                continue;
            }
        };
        let (tokens, _, _) = count(&output);
        if target == Target::Json {
            compact_tokens = Some(tokens);
        }
        let shown = preview(&output);
        let (fit, reason) = judge(target, tokens, source_tokens, compact_tokens, &data);
        targets.push(TargetView {
            id: target.id().to_owned(),
            ext: target.ext().to_owned(),
            tokens: Some(tokens),
            fit: fit.to_owned(),
            reason: reason.to_owned(),
            best: false,
            preview: shown,
            notes: target_losses.words(),
        });
        if output.len() <= KEEP_OUTPUT {
            outputs.insert(target, output);
        }
    }
    // TOON was judged before compact JSON existed: judge it again against it
    if let (Some(compact), Some(toon)) = (
        compact_tokens,
        targets.iter_mut().find(|t| t.id == Target::Toon.id()),
    ) {
        if let Some(tokens) = toon.tokens {
            let (fit, reason) = judge(Target::Toon, tokens, source_tokens, Some(compact), &data);
            fit.clone_into(&mut toon.fit);
            reason.clone_into(&mut toon.reason);
        }
    }
    if let Some(best) = targets
        .iter_mut()
        .filter(|t| t.fit == "good")
        .min_by_key(|t| t.tokens.unwrap_or(u64::MAX))
    {
        best.best = true;
    }
    Ok(Done {
        analysis: Analysis {
            from: source.id().to_owned(),
            bytes: text.len() as u64,
            source_tokens,
            tokenizer,
            approx,
            notes: losses.words(),
            targets,
        },
        value,
        outputs,
    })
}

/// Whether a target fits the data, and the word that says why: honest about formats that do not help.
#[must_use]
pub fn judge(
    target: Target,
    tokens: u64,
    source_tokens: u64,
    compact: Option<u64>,
    data: &Shape,
) -> (&'static str, &'static str) {
    let no_gain = tokens >= source_tokens;
    match target {
        Target::Toon => {
            let beats_compact = compact.is_none_or(|c| tokens < c);
            if beats_compact && !no_gain {
                (
                    "good",
                    if data.tabular_share >= 0.3 {
                        "tabular"
                    } else {
                        "smaller"
                    },
                )
            } else if data.non_uniform {
                ("poor", "notUniform")
            } else if data.depth > 3 && data.tabular_share < 0.3 {
                ("poor", "deep")
            } else {
                ("poor", "noGain")
            }
        }
        Target::Csv | Target::Tsv => {
            if no_gain {
                ("poor", "noGain")
            } else {
                ("good", "tabular")
            }
        }
        Target::Json => {
            if no_gain {
                ("poor", "noGain")
            } else {
                ("good", "compact")
            }
        }
        Target::JsonLines => {
            if no_gain {
                ("poor", "noGain")
            } else {
                ("good", "lines")
            }
        }
        Target::Yaml | Target::JsonPretty => {
            if no_gain {
                ("poor", "readable")
            } else {
                ("good", "readable")
            }
        }
    }
}

// ---- the plugin ---------------------------------------------------------------------------------------------------

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    static DEPENDS: [PluginId; 1] = [FMT_TOON];
    Arc::new(Convert {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::App,
            requires: Requirement::NONE,
            depends_on: Cow::Borrowed(&DEPENDS),
            default_enabled: true,
        },
    })
}

struct Convert {
    manifest: Manifest,
}

/// The job a window works on: one at a time per window.
struct Job {
    window: String,
    id: u64,
    value: Value,
    outputs: HashMap<Target, String>,
    /// The source file (none: a text from the page).
    path: Option<String>,
}

#[derive(Default)]
struct Shared {
    jobs: Mutex<Vec<Job>>,
    running: Mutex<HashMap<(String, u64), Cancel>>,
}

struct Started {
    _rpc: Vec<RpcRegistration>,
}

impl Running for Started {}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum SourceArg {
    File { path: String },
    Text { text: String },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AnalyzeArgs {
    job: u64,
    source: SourceArg,
    /// The source format (id | extension | fence language); none: by the file's extension, else by sniffing.
    #[serde(default)]
    from: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JobArgs {
    job: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResultArgs {
    job: u64,
    target: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveArgs {
    job: u64,
    target: String,
    /// Where a text from the page is saved next to (the open document); a file source saves next to itself.
    #[serde(default)]
    beside: Option<String>,
}

fn bad(what: &str) -> IpcError {
    IpcError::new(ErrorKind::InvalidData).detail(what.to_owned())
}

fn ext_of(path: &str) -> Option<&str> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, e)| e)
        .filter(|e| !e.is_empty())
}

fn stem_of(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

impl Shared {
    fn output(
        &self,
        window: &str,
        job: u64,
        target: Target,
    ) -> Result<(String, Option<String>), IpcError> {
        let jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        let found = jobs
            .iter()
            .find(|j| j.window == window && j.id == job)
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).detail("job"))?;
        let text = if let Some(text) = found.outputs.get(&target) {
            text.clone()
        } else {
            let cancel = Cancel::default();
            write(
                target,
                &found.value,
                &mut Tick::new(&cancel),
                &mut Losses::default(),
            )
            .map_err(|_| bad("failed"))?
        };
        Ok((text, found.path.clone()))
    }

    fn analyze(
        &self,
        app: Option<&AppServices>,
        args: AnalyzeArgs,
        window: &str,
    ) -> Result<Answer, IpcError> {
        let refused = |reason: &str, line: u32, detail: String| Answer::Refused {
            reason: reason.to_owned(),
            line,
            detail,
        };
        let (text, path) = match args.source {
            SourceArg::Text { text } => (text, None),
            SourceArg::File { path } => {
                let files = app
                    .and_then(|a| a.services.get::<WindowFilesApi>())
                    .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).detail("files"))?;
                match files.0.read(window, &path, MAX_SOURCE) {
                    Ok(text) => (text, Some(path)),
                    Err(e) if e.kind == ErrorKind::TooLarge => {
                        return Ok(refused("tooLarge", 0, String::new()))
                    }
                    Err(e) => return Err(e),
                }
            }
        };
        let source = args
            .from
            .as_deref()
            .and_then(Source::parse)
            .or_else(|| path.as_deref().and_then(ext_of).and_then(Source::parse))
            .unwrap_or_else(|| Source::sniff(&text));
        let cancel = Cancel::default();
        let key = (window.to_owned(), args.job);
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone(), cancel.clone());
        let count = counter_of(app);
        let outcome = analyze(&text, source, &cancel, &count);
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&key);
        Ok(match outcome {
            Ok(done) => {
                let mut jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
                // one job per window: a new analysis lets go of the one before
                jobs.retain(|j| j.window != window);
                jobs.push(Job {
                    window: window.to_owned(),
                    id: args.job,
                    value: done.value,
                    outputs: done.outputs,
                    path,
                });
                Answer::Ready {
                    analysis: done.analysis,
                }
            }
            Err(Failure::Cancelled) => Answer::Cancelled,
            Err(Failure::TooLarge) => refused("tooLarge", 0, String::new()),
            Err(Failure::Invalid { line, detail }) => refused("invalid", line, detail),
        })
    }
}

impl Plugin for Convert {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let app = cx.app().cloned();
        let shared = Arc::new(Shared::default());
        let mut rpc = Vec::new();

        let (s, a) = (Arc::clone(&shared), app.clone());
        rpc.extend(cx.rpc(
            "analyze",
            typed(move |args: AnalyzeArgs, call| s.analyze(a.as_ref(), args, &call.window)),
        ));

        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc(
            "cancel",
            typed(move |args: JobArgs, call| {
                if let Some(cancel) = s
                    .running
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&(call.window.clone(), args.job))
                {
                    cancel.cancel();
                }
                Ok(())
            }),
        ));

        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc(
            "close",
            typed(move |args: JobArgs, call| {
                s.jobs
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .retain(|j| !(j.window == call.window && j.id == args.job));
                Ok(())
            }),
        ));

        let (s, a) = (Arc::clone(&shared), app.clone());
        rpc.extend(cx.rpc(
            "result",
            typed(move |args: ResultArgs, call| {
                let target = Target::parse(&args.target).ok_or_else(|| bad("target"))?;
                let (text, path) = s.output(&call.window, args.job, target)?;
                // the person copies the result: the journal of the project notes it (a file source only)
                if let (Some(path), Some(files)) = (
                    path,
                    a.as_ref().and_then(|a| a.services.get::<WindowFilesApi>()),
                ) {
                    files.0.note(
                        &call.window,
                        Fact::Convert {
                            path,
                            to: target.id().to_owned(),
                            saved: None,
                        },
                    );
                }
                Ok(text)
            }),
        ));

        let (s, a) = (Arc::clone(&shared), app);
        rpc.extend(cx.rpc(
            "save",
            typed(move |args: SaveArgs, call| {
                let target = Target::parse(&args.target).ok_or_else(|| bad("target"))?;
                let (text, path) = s.output(&call.window, args.job, target)?;
                let beside = path.or(args.beside).ok_or_else(|| bad("beside"))?;
                let name = format!("{}.{}", stem_of(&beside), target.ext());
                let files = a
                    .as_ref()
                    .and_then(|a| a.services.get::<WindowFilesApi>())
                    .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).detail("files"))?;
                let saved = files.0.write_beside(&call.window, &beside, &name, &text)?;
                files.0.note(
                    &call.window,
                    Fact::Convert {
                        path: beside,
                        to: target.id().to_owned(),
                        saved: Some(saved.clone()),
                    },
                );
                Ok(saved)
            }),
        ));

        Ok(Box::new(Started { _rpc: rpc }))
    }
}

#[cfg(test)]
mod tests;
