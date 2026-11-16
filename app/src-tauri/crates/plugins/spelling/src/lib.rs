//! Plugin `spelling`: which words of a text no dictionary of its languages knows, and what they may have meant.
//!
//! On by default and dormant: no dictionary is bundled, none is downloaded until the person asks for one (RPC
//! `download`), and none is loaded until a window checks words in its language. A word is accepted when any of the
//! requested dictionaries knows it, or the person's own dictionary (profile) holds it. The windows split the text into
//! words and send each distinct word once; the answers come back in the same order.
//!
//! The language of a document: declared by the person (kept with the project, or in the profile outside one), else
//! detected from a sample of its text, else the app's default language (module `docs`).
//!
//! RPC: `languages`, `download`, `cancel`, `check`, `suggest`, `personal.add`, `personal.remove`, `personal.list`,
//! `language`, `declare`, `defaults.get`, `defaults.set`.
//! Events: `download` (one dictionary's new state), `personal` (the personal words after a change).

mod detect;
mod docs;
mod languages;
mod store;
mod words;

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use permark_core::conditions::Requirement;
use permark_core::downloads::DownloadKit;
use permark_core::plugins::{
    typed, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginEvents, PluginId, PluginKind,
    PluginScope, RpcRegistration, Running,
};
use permark_core::state::{StateData, StateError, StateFile};
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use spellbook::Dictionary;
use ts_rs::TS;

pub use docs::{SpellingDefault, SpellingDocLanguage, SpellingLanguageSource, SHARED_FILE};
pub use languages::{Language, Pin, COMMIT, OFFICIAL, SOURCE};
pub use store::{DictStore, SpellingFailure, SpellingState, Unavailable};
pub use words::{
    keep_suggestion, normalize, over_cap, personal_accepts, personal_form, sanitize_dic, FlagKind,
    MAX_WORD_CHARS,
};

const ID: PluginId = PluginId::of("spelling");

/// Most words one `check` call takes (the windows send the distinct words of what changed).
pub const MAX_WORDS: usize = 5000;
/// Most suggestions taken from each dictionary.
pub const SUGGESTIONS_PER_LANGUAGE: usize = 5;

/// Why a call is refused.
#[derive(Debug, thiserror::Error)]
pub enum SpellError {
    #[error("no dictionary of the requested languages is ready")]
    NotReady,
    #[error("more than {MAX_WORDS} words in one call")]
    TooMany,
    #[error("no dictionary for language {0}")]
    UnknownLanguage(String),
    #[error("not a single word")]
    NotAWord,
    #[error(transparent)]
    State(#[from] StateError),
}

impl From<SpellError> for IpcError {
    fn from(error: SpellError) -> Self {
        match error {
            SpellError::NotReady => IpcError::new(ErrorKind::Unavailable).detail("notReady"),
            SpellError::TooMany => IpcError::new(ErrorKind::TooLarge).detail("words"),
            SpellError::UnknownLanguage(lang) => IpcError::new(ErrorKind::InvalidData)
                .subject(lang)
                .detail("lang"),
            SpellError::NotAWord => IpcError::new(ErrorKind::InvalidData).detail("word"),
            SpellError::State(error) => error.into(),
        }
    }
}

/// The person's own words (profile, every document).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Personal {
    words: BTreeSet<String>,
}

impl StateData for Personal {
    const VERSION: u32 = 1;
}

/// One language as the windows see it (its name comes from the window's own language files).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpellingLanguage {
    /// BCP-47 tag.
    pub id: String,
    pub state: SpellingState,
    /// License of the dictionary (SPDX expression).
    pub license: String,
    /// Size of the download.
    #[ts(type = "number")]
    pub bytes: u64,
}

/// Event `spelling:download`: one dictionary's new state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpellingDownload {
    pub lang: String,
    pub state: SpellingState,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LangArgs {
    lang: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckArgs {
    langs: Vec<String>,
    words: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestArgs {
    langs: Vec<String>,
    word: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WordArgs {
    word: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DocArgs {
    /// The document in the window's workspace.
    path: String,
    /// Prose of the document (the windows leave code out); only its first `detect::MAX_SAMPLE_CHARS` are read.
    sample: String,
    /// The window's language (the default until the person picks one).
    ui_locale: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DeclareArgs {
    path: String,
    /// `None` clears the declared language.
    lang: Option<String>,
    /// For the answer once cleared (detection again).
    #[serde(default)]
    sample: String,
    #[serde(default)]
    ui_locale: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LocaleArgs {
    ui_locale: String,
}

/// The plugin with the official dictionaries (downloaded through the app's `DownloadKit` service).
#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    plugin_with(OFFICIAL.to_vec())
}

/// The plugin with other languages | pins (tests).
#[must_use]
pub fn plugin_with(languages: Vec<Language>) -> Arc<dyn Plugin> {
    Arc::new(Spelling {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::App,
            requires: Requirement::NONE,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
        languages,
    })
}

struct Spelling {
    manifest: Manifest,
    languages: Vec<Language>,
}

struct Started {
    _rpc: Vec<RpcRegistration>,
    store: Arc<DictStore>,
}

impl Running for Started {}

impl Drop for Started {
    fn drop(&mut self) {
        // downloads stop and their thread is joined; the loaded dictionaries go
        self.store.shutdown();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether `word` is spelled right: the person's own words | any of `dictionaries` know it. A word over the cap is
/// accepted unjudged (an engine panic would end the app: `panic = "abort"`; no dictionary holds such a word anyway).
fn accepted(
    dictionaries: &[(&str, Arc<Dictionary>)],
    personal: &BTreeSet<String>,
    word: &str,
) -> bool {
    over_cap(word)
        || personal_accepts(personal, word)
        || dictionaries
            .iter()
            .any(|(lang, d)| d.check(&normalize(word, lang)))
}

/// Up to [`SUGGESTIONS_PER_LANGUAGE`] corrections of `word` from `dictionary` (none for a word over the cap).
fn suggestions_in(dictionary: &Dictionary, lang: &str, word: &str) -> Vec<String> {
    if over_cap(word) {
        return Vec::new();
    }
    let mut found = Vec::new();
    dictionary.suggest(&normalize(word, lang), &mut found);
    found
        .into_iter()
        .filter(|s| keep_suggestion(s, |part| dictionary.check(part)))
        .take(SUGGESTIONS_PER_LANGUAGE)
        .collect()
}

/// Everything the handlers share.
struct Shared {
    store: Arc<DictStore>,
    personal: Mutex<BTreeSet<String>>,
    personal_file: Option<StateFile<Personal>>,
    events: PluginEvents,
    docs: docs::DocLanguages,
}

impl Shared {
    fn languages(&self) -> Vec<SpellingLanguage> {
        self.store
            .languages()
            .iter()
            .zip(self.store.states())
            .map(|(language, state)| SpellingLanguage {
                id: language.id.to_owned(),
                state,
                license: language.license.to_owned(),
                bytes: language.bytes(),
            })
            .collect()
    }

    fn index(&self, lang: &str) -> Result<usize, SpellError> {
        self.store
            .index(lang)
            .ok_or_else(|| SpellError::UnknownLanguage(lang.to_owned()))
    }

    /// The loaded dictionaries of `langs` (each once, in order); languages not ready are left out (never fetched).
    ///
    /// # Errors
    /// `UnknownLanguage`; `NotReady` when none is ready.
    fn dictionaries<'a>(
        &self,
        langs: &'a [String],
    ) -> Result<Vec<(&'a str, Arc<Dictionary>)>, SpellError> {
        let mut found: Vec<(&'a str, Arc<Dictionary>)> = Vec::new();
        for lang in langs {
            let at = self.index(lang)?;
            if found.iter().any(|(id, _)| id == lang) {
                continue;
            }
            if let Ok(dictionary) = self.store.load(at) {
                found.push((lang.as_str(), dictionary));
            }
        }
        if found.is_empty() {
            return Err(SpellError::NotReady);
        }
        Ok(found)
    }

    fn check(&self, args: &CheckArgs) -> Result<Vec<bool>, SpellError> {
        if args.words.len() > MAX_WORDS {
            return Err(SpellError::TooMany);
        }
        let dictionaries = self.dictionaries(&args.langs)?;
        let personal = lock(&self.personal);
        Ok(args
            .words
            .iter()
            .map(|word| accepted(&dictionaries, &personal, word))
            .collect())
    }

    /// Up to [`SUGGESTIONS_PER_LANGUAGE`] from each dictionary of `langs`, in order, each once.
    fn suggest(&self, args: &SuggestArgs) -> Result<Vec<String>, SpellError> {
        let dictionaries = self.dictionaries(&args.langs)?;
        let mut merged: Vec<String> = Vec::new();
        for (lang, dictionary) in &dictionaries {
            for suggestion in suggestions_in(dictionary, lang, &args.word) {
                if !merged.contains(&suggestion) {
                    merged.push(suggestion);
                }
            }
        }
        Ok(merged)
    }

    fn personal_list(&self) -> Vec<String> {
        lock(&self.personal).iter().cloned().collect()
    }

    /// Changes the personal words: saved first, then kept and told (a refused save changes nothing).
    fn change_personal(
        &self,
        word: &str,
        change: impl FnOnce(&mut BTreeSet<String>, String),
    ) -> Result<Vec<String>, SpellError> {
        let word = personal_form(word).ok_or(SpellError::NotAWord)?;
        let list = {
            let mut personal = lock(&self.personal);
            let mut words = personal.clone();
            change(&mut words, word);
            if let Some(file) = &self.personal_file {
                file.save(&Personal {
                    words: words.clone(),
                })?;
            }
            *personal = words;
            personal.iter().cloned().collect::<Vec<String>>()
        };
        if let Ok(payload) = serde_json::to_value(&list) {
            self.events.emit("personal", payload);
        }
        Ok(list)
    }
}

/// The calls of the plugin.
fn register(cx: &PluginCx, shared: &Arc<Shared>) -> Vec<RpcRegistration> {
    let mut rpc = Vec::new();
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc("languages", typed(move |_: NoArgs, _| Ok(s.languages()))));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "download",
        typed(move |args: LangArgs, _| {
            s.store.download(s.index(&args.lang)?);
            Ok(s.languages())
        }),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "cancel",
        typed(move |args: LangArgs, _| {
            s.store.cancel(s.index(&args.lang)?);
            Ok(s.languages())
        }),
    ));
    // the app runs plugin calls on its blocking pool: a first check loads a dictionary, a suggestion may take most of
    // a second, neither holds a window
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "check",
        typed(move |args: CheckArgs, _| Ok(s.check(&args)?)),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "suggest",
        typed(move |args: SuggestArgs, _| Ok(s.suggest(&args)?)),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "personal.add",
        typed(move |args: WordArgs, _| {
            Ok(s.change_personal(&args.word, |words, word| {
                words.insert(word);
            })?)
        }),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "personal.remove",
        typed(move |args: WordArgs, _| {
            Ok(s.change_personal(&args.word, |words, word| {
                words.remove(&word);
            })?)
        }),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "personal.list",
        typed(move |_: NoArgs, _| Ok(s.personal_list())),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "language",
        typed(move |args: DocArgs, call| {
            Ok(s.docs
                .language(&call.window, &args.path, &args.sample, &args.ui_locale))
        }),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "declare",
        typed(move |args: DeclareArgs, call| {
            s.docs
                .declare(&call.window, &args.path, args.lang.as_deref())?;
            Ok(s.docs
                .language(&call.window, &args.path, &args.sample, &args.ui_locale))
        }),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "defaults.get",
        typed(move |args: LocaleArgs, _| Ok(s.docs.default_lang(&args.ui_locale))),
    ));
    let s = Arc::clone(shared);
    rpc.extend(cx.rpc(
        "defaults.set",
        typed(move |args: LangArgs, _| s.docs.set_default(&args.lang)),
    ));
    rpc
}

impl Plugin for Spelling {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let store = cx.store().ok_or_else(|| PluginError::Start {
            id: ID,
            reason: "needs the state store".to_owned(),
        })?;
        let dir = store.cache_dir().join("plugins").join(ID.as_str());
        let personal_file = cx.state_file::<Personal>("personal.json");
        let personal = personal_file
            .as_ref()
            .map(|file| file.load().into_value().words)
            .unwrap_or_default();

        // the store tells the shared state, which holds the store: a weak link keeps them from holding each other
        let cell: Arc<Mutex<Weak<Shared>>> = Arc::default();
        let heard = Arc::clone(&cell);
        let listener: store::Listener = Arc::new(move |lang, state| {
            let shared = lock(&heard).upgrade();
            let payload = serde_json::to_value(SpellingDownload {
                lang: lang.to_owned(),
                state,
            });
            if let (Some(shared), Ok(payload)) = (shared, payload) {
                shared.events.emit("download", payload);
            }
        });
        // no download service in this app: a download fails `offline`
        let kit = cx.app().and_then(|app| app.services.get::<DownloadKit>());
        let dicts = DictStore::new(dir, self.languages.clone(), kit.as_deref(), listener);
        let docs = docs::DocLanguages::new(
            self.languages.iter().map(|language| language.id).collect(),
            cx.app().map(|app| Arc::clone(&app.services)),
            cx.state_file("languages.json"),
        );
        let shared = Arc::new(Shared {
            store: Arc::clone(&dicts),
            personal: Mutex::new(personal),
            personal_file,
            events: cx.events().clone(),
            docs,
        });
        *lock(&cell) = Arc::downgrade(&shared);
        Ok(Box::new(Started {
            _rpc: register(cx, &shared),
            store: dicts,
        }))
    }
}

#[cfg(test)]
mod tests;
