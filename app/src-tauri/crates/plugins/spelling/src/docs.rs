//! The language a document is checked in: the one the person declared for it, else the one its text shows, else the
//! app's default language.
//!
//! A declared language of a project's document travels with the project: `.permark/spelling.json` (shared with the
//! team, keyed by the document's project path). A document outside a project (a single file, the Notes) keeps it in
//! the profile, keyed by where the file is on disk. A renamed | moved document loses its declared language (detection
//! takes over again). The default language lives in the profile; until the person picks one it follows the window's
//! language.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use permark_core::paths::VaultPath;
use permark_core::plugins::{Services, WindowFiles, WindowFilesApi};
use permark_core::state::{RawFile, StateData, StateFile};
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::detect::{detect, locale_default, Detection};
use crate::lock;

/// Name of the project's shared file (in `.permark/`).
pub const SHARED_FILE: &str = "spelling.json";
/// Format version of the shared file (a file of another version is never written over).
const SHARED_VERSION: u32 = 1;

/// Where the language of a document comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SpellingLanguageSource {
    /// The person chose it for this document.
    Declared,
    /// The document's text shows it.
    Detected,
    /// The app's default language (the text is too short | unclear).
    Default,
    /// The text is written in a script no dictionary covers.
    Unsupported,
}

/// The languages a document is checked in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpellingDocLanguage {
    /// Dictionary ids (empty when unsupported).
    pub langs: Vec<String>,
    pub source: SpellingLanguageSource,
    /// What the text showed: the language found, or the best guess the default won over.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detected: Option<String>,
    /// The script of an unsupported text (`han`, `kana`, `hangul`, `cyrillic`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub script: Option<String>,
}

/// The app's default spelling language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpellingDefault {
    pub lang: String,
    /// The person picked it (else it follows the window's language).
    pub chosen: bool,
}

/// What the profile keeps: the default language and the declared languages of documents outside a project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ProfileLanguages {
    default: Option<String>,
    /// File on disk -> dictionary id.
    docs: BTreeMap<String, String>,
}

impl StateData for ProfileLanguages {
    const VERSION: u32 = 1;
}

/// The project's shared file, in the envelope the other state files use.
#[derive(Debug, Serialize, Deserialize)]
struct SharedEnvelope {
    version: u32,
    data: SharedDocs,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct SharedDocs {
    /// Project path -> dictionary id.
    docs: BTreeMap<String, String>,
}

/// The project's declared languages; `Err`: a file this version cannot read (never written over).
fn read_shared(bytes: Option<&[u8]>) -> Result<SharedDocs, ()> {
    let Some(bytes) = bytes else {
        return Ok(SharedDocs::default());
    };
    match serde_json::from_slice::<SharedEnvelope>(bytes) {
        Ok(envelope) if envelope.version == SHARED_VERSION => Ok(envelope.data),
        _ => Err(()),
    }
}

/// Pretty and sorted: the file is read in reviews and merged by git.
fn write_shared(docs: SharedDocs) -> Option<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(&SharedEnvelope {
        version: SHARED_VERSION,
        data: docs,
    })
    .ok()?;
    bytes.push(b'\n');
    Some(bytes)
}

/// Where a document's declared language is kept.
enum Place {
    Project { file: RawFile, key: String },
    Profile { key: String },
}

/// The document languages of the plugin.
pub(crate) struct DocLanguages {
    ids: Vec<&'static str>,
    services: Option<Arc<Services>>,
    profile: Mutex<ProfileLanguages>,
    profile_file: Option<StateFile<ProfileLanguages>>,
}

impl DocLanguages {
    pub(crate) fn new(
        ids: Vec<&'static str>,
        services: Option<Arc<Services>>,
        profile_file: Option<StateFile<ProfileLanguages>>,
    ) -> Self {
        let profile = profile_file
            .as_ref()
            .map(|file| file.load().into_value())
            .unwrap_or_default();
        Self {
            ids,
            services,
            profile: Mutex::new(profile),
            profile_file,
        }
    }

    fn files(&self) -> Option<Arc<dyn WindowFiles>> {
        self.services
            .as_ref()
            .and_then(|services| services.get::<WindowFilesApi>())
            .map(|api| Arc::clone(&api.0))
    }

    fn known(&self, lang: &str) -> Option<&'static str> {
        self.ids.iter().copied().find(|id| *id == lang)
    }

    fn known_or_refused(&self, lang: &str) -> Result<&'static str, IpcError> {
        self.known(lang).ok_or_else(|| {
            IpcError::new(ErrorKind::InvalidData)
                .subject(lang)
                .detail("lang")
        })
    }

    /// Where the declared language of `path` (a document of the window's workspace) is kept; `located`: the file
    /// was found inside the window's boundary (a project document is keyed without asking the disk).
    fn place(
        files: &dyn WindowFiles,
        window: &str,
        path: &str,
        located: impl FnOnce() -> Result<String, IpcError>,
    ) -> Result<Place, IpcError> {
        if let Some(file) = files.shared_file(window, SHARED_FILE) {
            let key = VaultPath::parse(path)?.as_str().to_owned();
            if key.is_empty() {
                return Err(IpcError::new(ErrorKind::InvalidData).subject(path));
            }
            return Ok(Place::Project { file, key });
        }
        Ok(Place::Profile { key: located()? })
    }

    /// The language the person declared for `path`, when it still has a dictionary.
    fn declared(&self, window: &str, path: &str) -> Option<&'static str> {
        let files = self.files()?;
        let located = || {
            files
                .locate(window, path)
                .map(|abs| abs.display().to_string())
        };
        let lang = match Self::place(files.as_ref(), window, path, located).ok()? {
            Place::Project { file, key } => {
                let bytes = file.read().ok()?;
                read_shared(bytes.as_deref()).ok()?.docs.remove(&key)?
            }
            Place::Profile { key } => lock(&self.profile).docs.get(&key)?.clone(),
        };
        self.known(&lang)
    }

    /// The default language now (`ui_locale`: the window's language, used until the person picks one).
    pub(crate) fn default_lang(&self, ui_locale: &str) -> SpellingDefault {
        let chosen = lock(&self.profile)
            .default
            .as_deref()
            .and_then(|lang| self.known(lang));
        SpellingDefault {
            lang: chosen
                .unwrap_or_else(|| locale_default(ui_locale, &self.ids))
                .to_owned(),
            chosen: chosen.is_some(),
        }
    }

    /// Picks the default language (profile).
    ///
    /// # Errors
    /// No dictionary for `lang`; the profile cannot be written.
    pub(crate) fn set_default(&self, lang: &str) -> Result<SpellingDefault, IpcError> {
        let lang = self.known_or_refused(lang)?;
        self.change_profile(|profile| profile.default = Some(lang.to_owned()))?;
        Ok(SpellingDefault {
            lang: lang.to_owned(),
            chosen: true,
        })
    }

    /// Saved first, then kept (a refused save changes nothing).
    fn change_profile(&self, change: impl FnOnce(&mut ProfileLanguages)) -> Result<(), IpcError> {
        let mut profile = lock(&self.profile);
        let mut next = profile.clone();
        change(&mut next);
        if next == *profile {
            return Ok(());
        }
        let file = self
            .profile_file
            .as_ref()
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("state"))?;
        file.save(&next)?;
        *profile = next;
        Ok(())
    }

    /// The languages `path` is checked in: declared, else detected from `sample`, else the default.
    pub(crate) fn language(
        &self,
        window: &str,
        path: &str,
        sample: &str,
        ui_locale: &str,
    ) -> SpellingDocLanguage {
        if let Some(lang) = self.declared(window, path) {
            return SpellingDocLanguage {
                langs: vec![lang.to_owned()],
                source: SpellingLanguageSource::Declared,
                detected: None,
                script: None,
            };
        }
        let default = self.default_lang(ui_locale).lang;
        match detect(sample, &self.ids, &default) {
            Detection::Found(lang) => SpellingDocLanguage {
                langs: vec![lang.to_owned()],
                source: SpellingLanguageSource::Detected,
                detected: Some(lang.to_owned()),
                script: None,
            },
            Detection::Unsure(guess) => SpellingDocLanguage {
                langs: vec![default],
                source: SpellingLanguageSource::Default,
                detected: guess.map(str::to_owned),
                script: None,
            },
            Detection::Unsupported(script) => SpellingDocLanguage {
                langs: Vec::new(),
                source: SpellingLanguageSource::Unsupported,
                detected: None,
                script: Some(script.to_owned()),
            },
        }
    }

    /// Declares (`Some`) | clears (`None`) the language of `path`, a file inside the window's workspace.
    ///
    /// # Errors
    /// No dictionary for `lang`; no such window | outside | missing file; a project file this version cannot read; a
    /// refused write.
    pub(crate) fn declare(
        &self,
        window: &str,
        path: &str,
        lang: Option<&str>,
    ) -> Result<(), IpcError> {
        let lang = lang.map(|lang| self.known_or_refused(lang)).transpose()?;
        let files = self
            .files()
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("files"))?;
        // the window's boundary: only an existing document of its workspace gets a language
        let abs = files.locate(window, path)?;
        let located = || Ok(abs.display().to_string());
        match Self::place(files.as_ref(), window, path, located)? {
            Place::Project { file, key } => {
                let mut unreadable = false;
                file.update(|now| {
                    let Ok(mut shared) = read_shared(now) else {
                        unreadable = true;
                        return None;
                    };
                    let before = shared.docs.clone();
                    match lang {
                        Some(lang) => shared.docs.insert(key.clone(), lang.to_owned()),
                        None => shared.docs.remove(&key),
                    };
                    if shared.docs == before {
                        return None;
                    }
                    write_shared(shared)
                })?;
                if unreadable {
                    return Err(IpcError::new(ErrorKind::InvalidData)
                        .subject(SHARED_FILE)
                        .detail("unreadable"));
                }
            }
            Place::Profile { key } => self.change_profile(|profile| {
                match lang {
                    Some(lang) => profile.docs.insert(key, lang.to_owned()),
                    None => profile.docs.remove(&key),
                };
            })?,
        }
        Ok(())
    }
}
