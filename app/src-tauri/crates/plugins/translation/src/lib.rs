//! Plugin `translation`: translates Markdown text with the local model assigned to job "translation" (plugin
//! `local-ai`) into a language the person picks. The window reviews every translated piece before it lands, or saves
//! the whole translation as a new file beside the original (TS side: segment, markers, guards, review, language list).
//!
//! Rust side is thin: prompt data + the translation job ([`prompt`]) run by `local-ai`'s piece runner
//! ([`PieceRunner`]: model call + piece results stream, warm session, cancel), the remembered targets ([`prefs`]) and
//! the write of the new file through the window's files. Depends on `local-ai` (model service `ModelHost`, looked up
//! per call: loads only through it). No network code: the text stays on this computer.
//!
//! On by default but dormant: start registers the RPC only (no thread, no load, no file read); the window hides the
//! commands until `status` says a model is assigned.
//!
//! RPC (app scope):
//! - `status {}` -> [`TranslationStatus`].
//! - `run { run, pieces: [{ id, text }], target: { tag, name }, source: { tag, name } | null }` -> `null` (news as
//!   event `translation:run` = the runner's [`RunNews`](permark_plugin_local_ai::pieces::RunNews); a new run cancels
//!   the running one). `name` = the language's English name; both languages checked before they enter the prompt
//!   ([`Language`], [`Pair`]): refused -> `invalidData`, detail `tag` | `name` | `sameLanguage`, nothing started.
//! - `cancel { run }` -> `null`.
//! - `prefs {}` -> [`TranslationPrefs`]; `setPrefs { target: tag }` -> [`TranslationPrefs`] (target to the front of
//!   the recent ones).
//! - `save { beside, name, text }` -> [`TranslationSaved`]: a NEW file `name` next to `beside` in the workspace of
//!   the calling window, never over a file (a taken name gets ` (2)`).

pub mod prefs;
pub mod prompt;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running, WindowFilesApi,
};
use permark_core::{ErrorKind, IpcError};
use permark_plugin_local_ai::pieces::{Piece, PieceRunner};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use prefs::TranslationPrefs;
pub use prompt::{Language, Pair, Refused, Tag, TranslationJob};

const ID: PluginId = PluginId::of("translation");

/// Translation runs of one plugin instance.
type Translator = PieceRunner<TranslationJob>;

/// The plugin.
#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(Translation {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::App,
            requires: Requirement::NONE,
            depends_on: Cow::Owned(vec![PluginId::of("local-ai")]),
            default_enabled: true,
        },
    })
}

struct Translation {
    manifest: Manifest,
}

struct Started {
    _rpc: Vec<RpcRegistration>,
    translator: Arc<Translator>,
}

impl Started {
    /// The plugin's RPC over `translator` (production: the hosted one; tests: one over a scripted model source).
    fn over(cx: &PluginCx, translator: Arc<Translator>) -> Self {
        Self {
            _rpc: register(cx, &translator),
            translator,
        }
    }
}

impl Running for Started {}

impl Drop for Started {
    fn drop(&mut self) {
        // off: run cancelled, warm session unloaded, worker joined (later calls refused); RPC dropped next
        self.translator.shutdown();
    }
}

/// Answer of `translation.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranslationStatus {
    /// A model is assigned to job "translation" (state alone; it may since be gone -> the run fails `noModel`).
    pub available: bool,
}

/// A language of a run as the window names it (unchecked: [`Language::parse`] decides).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct TranslationLanguage {
    /// Language tag (`pt-BR`, `zh-Hans`, `es-419`).
    pub tag: String,
    /// English name the prompt shows next to the tag ("Brazilian Portuguese").
    pub name: String,
}

impl TranslationLanguage {
    fn checked(&self) -> Result<Language, Refused> {
        Language::parse(&self.tag, &self.name)
    }
}

/// Answer of `translation.save`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranslationSaved {
    /// Path of the new file in the window's workspace (the name asked for, or the free one picked instead).
    pub path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArgs {
    run: String,
    pieces: Vec<Piece>,
    target: TranslationLanguage,
    source: Option<TranslationLanguage>,
}

impl RunArgs {
    /// The languages of the run, checked.
    fn pair(&self) -> Result<Pair, Refused> {
        let source = self
            .source
            .as_ref()
            .map(TranslationLanguage::checked)
            .transpose()?;
        Pair::new(self.target.checked()?, source)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelArgs {
    run: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetPrefsArgs {
    target: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveArgs {
    /// Path of the original in the window's workspace: the new file goes into its folder.
    beside: String,
    /// File name of the new file (no folder).
    name: String,
    text: String,
}

/// RPC of one instance over `translator`.
fn register(cx: &PluginCx, translator: &Arc<Translator>) -> Vec<RpcRegistration> {
    let mut rpc = Vec::new();
    let t = Arc::clone(translator);
    rpc.extend(cx.rpc(
        "status",
        typed(move |_: NoArgs, _| {
            Ok::<_, IpcError>(TranslationStatus {
                available: t.available(),
            })
        }),
    ));
    let t = Arc::clone(translator);
    rpc.extend(cx.rpc(
        "run",
        typed(move |args: RunArgs, _| {
            let pair = args.pair()?;
            t.start(args.run, pair, args.pieces)
        }),
    ));
    let t = Arc::clone(translator);
    rpc.extend(cx.rpc(
        "cancel",
        typed(move |args: CancelArgs, _| {
            t.cancel(&args.run);
            Ok::<_, IpcError>(())
        }),
    ));
    let chosen = Arc::new(prefs::Store::new(cx.state_file("prefs.json")));
    let c = Arc::clone(&chosen);
    rpc.extend(cx.rpc(
        "prefs",
        typed(move |_: NoArgs, _| Ok::<_, IpcError>(c.get())),
    ));
    rpc.extend(cx.rpc(
        "setPrefs",
        typed(move |args: SetPrefsArgs, _| chosen.choose(&Tag::parse(&args.target)?)),
    ));
    let services = cx.app().map(|app| Arc::clone(&app.services));
    rpc.extend(cx.rpc(
        "save",
        typed(move |args: SaveArgs, call| {
            // the window's own files: its boundary, a new file only (never over one)
            let files = services
                .as_ref()
                .and_then(|services| services.get::<WindowFilesApi>())
                .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).detail("files"))?;
            let path = files
                .0
                .write_beside(&call.window, &args.beside, &args.name, &args.text)?;
            Ok(TranslationSaved { path })
        }),
    ));
    rpc
}

impl Plugin for Translation {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let translator = Arc::new(Translator::hosted(cx));
        Ok(Box::new(Started::over(cx, translator)))
    }
}

#[cfg(test)]
mod tests;
