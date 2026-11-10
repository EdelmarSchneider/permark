//! The targets the person chose (`prefs.json` in the plugin's own state namespace, profile): the last one = default
//! of the next run, the recent ones = top of the language list. Tags only: names are the window's (shown in the app's
//! language). Read on the first call, never at plugin start (dormant).

use std::sync::{Mutex, MutexGuard, PoisonError};

use permark_core::state::{StateData, StateFile};
use permark_core::IpcError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::prompt::Tag;

/// Recent targets kept.
pub const RECENT_MAX: usize = 5;

/// Answer of `translation.prefs` | `translation.setPrefs`; also the content of the state file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranslationPrefs {
    /// Tag of the target chosen last (`null`: none yet -> the window asks before its first run).
    pub target: Option<String>,
    /// Tags of the targets chosen, newest first, at most [`RECENT_MAX`], each once.
    pub recent: Vec<String>,
}

impl StateData for TranslationPrefs {
    const VERSION: u32 = 1;
}

impl TranslationPrefs {
    /// `target` chosen: the default now, first of the recent ones (its older entry, in any case, dropped).
    fn choose(&mut self, target: &Tag) {
        let tag = target.as_str();
        self.recent.retain(|known| !known.eq_ignore_ascii_case(tag));
        self.recent.insert(0, tag.to_owned());
        self.recent.truncate(RECENT_MAX);
        self.target = Some(tag.to_owned());
    }
}

/// The choices of one plugin instance: in memory once read, written through on every change.
pub struct Store {
    /// `None`: no state store given (kept in memory only).
    file: Option<StateFile<TranslationPrefs>>,
    /// `None`: not read yet.
    now: Mutex<Option<TranslationPrefs>>,
}

impl Store {
    /// Over `file`; nothing read yet.
    #[must_use]
    pub fn new(file: Option<StateFile<TranslationPrefs>>) -> Self {
        Self {
            file,
            now: Mutex::new(None),
        }
    }

    fn now(&self) -> MutexGuard<'_, Option<TranslationPrefs>> {
        self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn read(&self) -> TranslationPrefs {
        self.file
            .as_ref()
            .map(|file| file.load().into_value())
            .unwrap_or_default()
    }

    /// The choices now (missing | unreadable file: none).
    #[must_use]
    pub fn get(&self) -> TranslationPrefs {
        self.now().get_or_insert_with(|| self.read()).clone()
    }

    /// Remembers `target` as chosen; answers the choices after it.
    ///
    /// # Errors
    /// The state file could not be written (the choice still holds until the app closes).
    pub fn choose(&self, target: &Tag) -> Result<TranslationPrefs, IpcError> {
        let mut now = self.now();
        let prefs = now.get_or_insert_with(|| self.read());
        prefs.choose(target);
        if let Some(file) = &self.file {
            file.save(prefs)?;
        }
        Ok(prefs.clone())
    }
}
