//! Plugin `token-count`: tokens of a document, a selection or a section by the tokenizer the person chooses.
//!
//! A count belongs to a tokenizer. Exact ones (`o200k`, `cl100k`) need the public rank file of their encoding, which is
//! never bundled: the person confirms a download, it is checked against a pinned SHA-256 and kept in the app cache.
//! The others (`claude`, `gemini`, `chars4`) are estimates and are marked as such wherever their number shows. Nothing
//! the person writes leaves the computer to count it.
//!
//! The plugin serves an RPC for the windows (`state`, `download`, `cancel`, `removeVocab`, `setOutline`, `count`) and
//! offers the core service [`permark_core::tokens::TokenCounterApi`] to the agent connection while it runs.

mod bpe;
mod counter;
mod estimate;
mod fetch;
mod folder;
mod tokenizer;
mod vocab;

use std::borrow::Cow;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, Manifest, Plugin, PluginCx, PluginError, PluginEvents, PluginId, PluginKind,
    PluginScope, RpcRegistration, Running, ServiceLease,
};
use permark_core::state::{StateData, StateFile};
use permark_core::tokens::TokenCounterApi;
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use counter::{line_starts, lines_of, Counter};
pub use estimate::{chars4, estimate};
pub use fetch::{Fetch, FetchError, UreqFetch};
pub use folder::{
    FolderBadge, FolderBadges, FolderFailure, FolderJob, FolderRefusal, FolderSkip, FolderSkipped,
    FolderSummary, FolderTopFile, FolderTypeRow, Folders,
};
pub use tokenizer::{effective, Encoding, Tokenizer};
pub use vocab::{Pin, Unavailable, VocabFailure, VocabState, VocabStore};

const ID: PluginId = PluginId::of("token-count");

/// What the plugin keeps of the person's choices (profile, its own namespace).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Prefs {
    /// Show the token column in the outline.
    outline: bool,
    /// Count the folders the person opens in the Explorer, when they are small.
    #[serde(default)]
    folder_totals: bool,
}

impl StateData for Prefs {
    const VERSION: u32 = 1;
}

/// One vocabulary as the windows see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VocabEntry {
    pub state: VocabState,
    /// Size of the download, from the pin.
    #[ts(type = "number")]
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VocabEntries {
    pub o200k: VocabEntry,
    pub cl100k: VocabEntry,
}

/// Everything the windows need to show: the vocabularies and the outline switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TokenCountState {
    pub vocab: VocabEntries,
    pub outline: bool,
    /// Folders the person opens in the Explorer are counted when they are small.
    pub folder_totals: bool,
}

/// The count of one tokenizer, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum CountResult {
    Ok {
        tokenizer: String,
        /// The whole text.
        #[ts(type = "number")]
        whole: u64,
        /// One count per requested line range, in order.
        #[ts(type = "number[]")]
        parts: Vec<u64>,
    },
    Unavailable {
        tokenizer: String,
        /// `noVocab`.
        reason: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CountArgs {
    tokenizers: Vec<String>,
    text: String,
    /// Line ranges (1-based, both ends included) counted on their own.
    #[serde(default)]
    parts: Vec<(u32, u32)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenizerArgs {
    tokenizer: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutlineArgs {
    on: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TotalsArgs {
    on: bool,
}

fn invalid(what: &str) -> IpcError {
    IpcError::new(ErrorKind::InvalidData).detail(what)
}

fn exact(args: &TokenizerArgs) -> Result<Encoding, IpcError> {
    Tokenizer::parse(&args.tokenizer)
        .and_then(Tokenizer::encoding)
        .ok_or_else(|| invalid("tokenizer"))
}

fn state_of(vocab: &[VocabState; 2], store: &VocabStore, prefs: &Prefs) -> TokenCountState {
    let entry = |encoding: Encoding| VocabEntry {
        state: vocab[encoding.index()].clone(),
        bytes: store.pin(encoding).bytes,
    };
    TokenCountState {
        vocab: VocabEntries {
            o200k: entry(Encoding::O200k),
            cl100k: entry(Encoding::Cl100k),
        },
        outline: prefs.outline,
        folder_totals: prefs.folder_totals,
    }
}

/// The plugin with the real network source for the vocabularies.
#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    plugin_with(
        Arc::new(UreqFetch::new()),
        [
            Pin::official(Encoding::O200k),
            Pin::official(Encoding::Cl100k),
        ],
    )
}

/// The plugin with another source and other pins (tests).
#[must_use]
pub fn plugin_with(fetch: Arc<dyn Fetch>, pins: [Pin; 2]) -> Arc<dyn Plugin> {
    Arc::new(TokenCount {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Analysis,
            scope: PluginScope::App,
            requires: Requirement::NONE,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
        fetch,
        pins,
    })
}

struct TokenCount {
    manifest: Manifest,
    fetch: Arc<dyn Fetch>,
    pins: [Pin; 2],
}

struct Started {
    _rpc: Vec<RpcRegistration>,
    _service: ServiceLease,
    store: Arc<VocabStore>,
    folders: Arc<Folders>,
}

impl Running for Started {}

impl Drop for Started {
    fn drop(&mut self) {
        // folder counts and downloads stop and their threads are joined; the tables go with the store
        self.folders.shutdown();
        self.store.shutdown();
    }
}

/// Everything the handlers share.
struct Shared {
    store: Arc<VocabStore>,
    counter: Arc<Counter>,
    prefs: Mutex<Prefs>,
    prefs_file: Option<StateFile<Prefs>>,
    events: PluginEvents,
}

impl Shared {
    fn set_pref(&self, change: impl FnOnce(&mut Prefs)) -> Result<(), IpcError> {
        {
            let mut prefs = self.prefs.lock().unwrap_or_else(PoisonError::into_inner);
            change(&mut prefs);
            if let Some(file) = &self.prefs_file {
                file.save(&prefs)?;
            }
        }
        self.announce();
        Ok(())
    }
}

impl Shared {
    fn prefs(&self) -> Prefs {
        self.prefs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn state(&self) -> TokenCountState {
        state_of(&self.store.states(), &self.store, &self.prefs())
    }

    fn announce(&self) {
        if let Ok(payload) = serde_json::to_value(self.state()) {
            self.events.emit("state", payload);
        }
    }

    fn count(&self, args: &CountArgs) -> Result<Vec<CountResult>, IpcError> {
        let starts = if args.parts.is_empty() {
            Vec::new()
        } else {
            line_starts(&args.text)
        };
        let mut results = Vec::with_capacity(args.tokenizers.len());
        for id in &args.tokenizers {
            let tokenizer = Tokenizer::parse(id).ok_or_else(|| invalid("tokenizer"))?;
            let outcome = self.counter.count(tokenizer, &args.text).and_then(|whole| {
                let parts = args
                    .parts
                    .iter()
                    .map(|&(from, to)| {
                        self.counter
                            .count(tokenizer, lines_of(&args.text, &starts, from, to))
                    })
                    .collect::<Result<Vec<u64>, Unavailable>>()?;
                Ok((whole, parts))
            });
            results.push(match outcome {
                Ok((whole, parts)) => CountResult::Ok {
                    tokenizer: id.clone(),
                    whole,
                    parts,
                },
                Err(Unavailable::NoVocab) => CountResult::Unavailable {
                    tokenizer: id.clone(),
                    reason: "noVocab".to_owned(),
                },
            });
        }
        Ok(results)
    }
}

/// The calls of the plugin that work on its shared state: the vocabularies, the choices, counting a text.
fn register(cx: &PluginCx, shared: &Arc<Shared>) -> Vec<RpcRegistration> {
    let mut rpc = Vec::new();
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "state",
        typed(move |_: permark_core::plugins::NoArgs, _| Ok(s.state())),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "download",
        typed(move |args: TokenizerArgs, _| {
            s.store.download(exact(&args)?);
            Ok(s.state())
        }),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "cancel",
        typed(move |args: TokenizerArgs, _| {
            s.store.cancel(exact(&args)?);
            Ok(s.state())
        }),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "removeVocab",
        typed(move |args: TokenizerArgs, _| {
            s.store
                .remove(exact(&args)?)
                .map_err(|e| IpcError::io(String::new(), &e))?;
            Ok(s.state())
        }),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "setOutline",
        typed(move |args: OutlineArgs, _| {
            s.set_pref(|prefs| prefs.outline = args.on)?;
            Ok(s.state())
        }),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "setFolderTotals",
        typed(move |args: TotalsArgs, _| {
            s.set_pref(|prefs| prefs.folder_totals = args.on)?;
            Ok(s.state())
        }),
    ));
    rpc.extend(cx.rpc(
        "count",
        typed({
            let s = Arc::clone(shared);
            move |args: CountArgs, _| s.count(&args)
        }),
    ));
    rpc
}

impl Plugin for TokenCount {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let fail = |reason: &str| PluginError::Start {
            id: ID,
            reason: reason.to_owned(),
        };
        let app = cx
            .app()
            .ok_or_else(|| fail("needs the app services"))?
            .clone();
        let store = cx.store().ok_or_else(|| fail("needs the state store"))?;
        let dir = store.cache_dir().join("plugins").join(ID.as_str());
        let prefs_file = cx.state_file::<Prefs>("prefs.json");
        let prefs = prefs_file
            .as_ref()
            .map(|file| file.load().into_value())
            .unwrap_or_default();

        // the store tells the shared state, which holds the store: a weak link keeps them from holding each other
        let cell: Arc<Mutex<Weak<Shared>>> = Arc::default();
        let heard = Arc::clone(&cell);
        let listener: vocab::Listener = Arc::new(move |_| {
            let shared = heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .upgrade();
            if let Some(shared) = shared {
                shared.announce();
            }
        });
        let vocab = VocabStore::new(
            dir.clone(),
            self.pins.clone(),
            Arc::clone(&self.fetch),
            listener,
        );
        let counter = Counter::new(Arc::clone(&vocab));
        let shared = Arc::new(Shared {
            store: Arc::clone(&vocab),
            counter: Arc::clone(&counter),
            prefs: Mutex::new(prefs),
            prefs_file,
            events: cx.events().clone(),
        });
        *cell.lock().unwrap_or_else(PoisonError::into_inner) = Arc::downgrade(&shared);

        let mut rpc = register(cx, &shared);
        let folders = Folders::new(
            cx.events().clone(),
            app.clone(),
            Arc::clone(&vocab),
            folder::Thresholds::STANDARD,
            dir.join("folders"),
        );
        rpc.extend(folder::register(cx, &folders));

        let service = app.services.provide(Arc::new(TokenCounterApi(Arc::new(
            counter::CounterService {
                counter,
                app: app.clone(),
            },
        ))));
        Ok(Box::new(Started {
            _rpc: rpc,
            _service: service,
            store: vocab,
            folders,
        }))
    }
}

#[cfg(test)]
mod tests;
