//! The dictionary store: the dictionaries in the app cache, their downloads, and the loaded ones.
//!
//! Nothing here touches the network until [`DictStore::download`] is called, and the plugin calls that only for a
//! download the person asked for. A dictionary is loaded on its first check and kept for the life of the plugin
//! (loading parses megabytes; checking after that is cheap). Presence = the download engine's install manifest; the
//! SHA-256 of both files is checked again when they are loaded.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use permark_core::downloads::{DownloadError, DownloadKit, Downloads, Unit, Update};
use serde::Serialize;
use sha2::{Digest, Sha256};
use spellbook::Dictionary;
use ts_rs::TS;

use crate::languages::{Language, Pin, AFF, DIC};
use crate::words::readable;

/// Why a dictionary is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum SpellingFailure {
    /// No connection to the source (or it dropped).
    Offline,
    /// The source answered with this status.
    Http { status: u16 },
    /// What arrived (or what is in the cache) is not the official file.
    Hash,
    /// The file could not be written.
    Disk,
    /// The files are the official ones but the spelling engine cannot read them.
    Unreadable,
}

/// Where one dictionary stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum SpellingState {
    Absent,
    Downloading {
        #[ts(type = "number")]
        done: u64,
        #[ts(type = "number")]
        total: u64,
    },
    Ready,
    Failed {
        reason: SpellingFailure,
    },
}

/// What happens to a dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    /// Installed at start.
    Found,
    Started {
        total: u64,
    },
    Progress {
        done: u64,
        total: u64,
    },
    Finished,
    Failed(SpellingFailure),
    Canceled,
}

impl SpellingState {
    /// `(state, event) -> state`; an event that does not fit the state changes nothing.
    fn next(self, event: Event) -> Self {
        match (self, event) {
            (Self::Absent | Self::Failed { .. }, Event::Started { total }) => {
                Self::Downloading { done: 0, total }
            }
            (Self::Downloading { .. }, Event::Progress { done, total }) => {
                Self::Downloading { done, total }
            }
            (Self::Absent, Event::Found) | (Self::Downloading { .. }, Event::Finished) => {
                Self::Ready
            }
            (Self::Downloading { .. } | Self::Ready, Event::Failed(reason)) => {
                Self::Failed { reason }
            }
            (Self::Downloading { .. }, Event::Canceled) => Self::Absent,
            (state, _) => state,
        }
    }
}

/// Why a dictionary cannot be used now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// Not downloaded (or still arriving, or failed, or unreadable).
    NotReady,
}

/// Told after every change of one dictionary (its tag + new state).
pub type Listener = Arc<dyn Fn(&str, SpellingState) + Send + Sync>;

struct Inner {
    states: Vec<SpellingState>,
    loaded: Vec<Option<Arc<Dictionary>>>,
}

pub struct DictStore {
    dir: PathBuf,
    languages: Vec<Language>,
    /// One unit per language (`None`: a bad pin).
    units: Vec<Option<Unit>>,
    /// Engine over `dir` (`None`: the app offers no download service -> a download fails `Offline`).
    downloads: Option<Downloads>,
    listener: Listener,
    inner: Mutex<Inner>,
    /// One parse at a time.
    loading: Mutex<()>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the windows are told of a download failure.
fn failure_of(error: &DownloadError) -> SpellingFailure {
    match error {
        // a refused host reads as "cannot reach the source"; `Cancelled` never reaches here (`heard` turns it into
        // `Event::Canceled` first), listed only to keep the match exhaustive
        DownloadError::Offline | DownloadError::HostNotAllowed | DownloadError::Cancelled => {
            SpellingFailure::Offline
        }
        DownloadError::Http { status } => SpellingFailure::Http { status: *status },
        DownloadError::Integrity => SpellingFailure::Hash,
        DownloadError::NoSpace { .. } | DownloadError::Disk { .. } | DownloadError::Busy => {
            SpellingFailure::Disk
        }
    }
}

/// The file at `path` when it is the pinned one, as text.
fn read_pinned(path: &std::path::Path, pin: Pin) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let official = bytes.len() as u64 == pin.bytes && Sha256::digest(&bytes)[..] == pin.sha256;
    official.then(|| String::from_utf8(bytes).ok()).flatten()
}

impl DictStore {
    /// The store over the cache folder `dir`, downloading through `kit` (the app's service). Installed dictionaries
    /// count as there; a resumable part of an earlier run is kept (the next download goes on from it), anything else
    /// left over is removed. Nothing is created or fetched.
    #[must_use]
    pub fn new(
        dir: PathBuf,
        languages: Vec<Language>,
        kit: Option<&DownloadKit>,
        listener: Listener,
    ) -> Arc<Self> {
        let units: Vec<Option<Unit>> = languages.iter().map(Language::unit).collect();
        let store = Arc::new_cyclic(|weak: &Weak<Self>| {
            // the engine tells the store, which holds the engine: a weak link keeps them from holding each other
            let heard = weak.clone();
            // no thread | folder | request until the first download
            let downloads = kit.map(|kit| {
                kit.downloads(
                    dir.clone(),
                    Arc::new(move |id, update| {
                        if let Some(store) = heard.upgrade() {
                            store.heard(id, update);
                        }
                    }),
                )
            });
            let states = units
                .iter()
                .map(|unit| {
                    let there = downloads
                        .as_ref()
                        .zip(unit.as_ref())
                        .is_some_and(|(downloads, unit)| downloads.is_installed(unit));
                    if there {
                        SpellingState::Absent.next(Event::Found)
                    } else {
                        SpellingState::Absent
                    }
                })
                .collect();
            Self {
                dir,
                inner: Mutex::new(Inner {
                    loaded: vec![None; languages.len()],
                    states,
                }),
                languages,
                units,
                downloads,
                listener,
                loading: Mutex::new(()),
            }
        });
        store.sweep();
        store
    }

    #[must_use]
    pub fn languages(&self) -> &[Language] {
        &self.languages
    }

    /// Index of tag `id` in [`DictStore::languages`].
    #[must_use]
    pub fn index(&self, id: &str) -> Option<usize> {
        self.languages.iter().position(|l| l.id == id)
    }

    #[must_use]
    pub fn states(&self) -> Vec<SpellingState> {
        lock(&self.inner).states.clone()
    }

    /// Engine + unit of language `at` (`None`: no download service | a bad pin).
    fn engine(&self, at: usize) -> Option<(&Downloads, &Unit)> {
        Some((self.downloads.as_ref()?, self.units.get(at)?.as_ref()?))
    }

    /// Applies an event; tells the listener when the state changed. Returns whether it changed.
    fn apply(&self, at: usize, event: Event) -> bool {
        let after = {
            let mut inner = lock(&self.inner);
            let Some(state) = inner.states.get_mut(at) else {
                return false;
            };
            let after = state.next(event);
            if after == *state {
                return false;
            }
            *state = after;
            after
        };
        if let Some(language) = self.languages.get(at) {
            (self.listener)(language.id, after);
        }
        true
    }

    /// Engine news of unit `id`. A cancelled download leaves no partial file; a failed one keeps it for the next try.
    fn heard(&self, id: &str, update: &Update) {
        let Some(at) = self
            .units
            .iter()
            .position(|u| u.as_ref().is_some_and(|u| u.id() == id))
        else {
            return;
        };
        let event = match update {
            Update::Progress(progress) => Event::Progress {
                done: progress.done,
                total: progress.total,
            },
            Update::Done => Event::Finished,
            Update::Failed(DownloadError::Cancelled) => {
                self.delete_part(at);
                Event::Canceled
            }
            Update::Failed(other) => Event::Failed(failure_of(other)),
        };
        self.apply(at, event);
    }

    fn delete_part(&self, at: usize) {
        if let Some((downloads, unit)) = self.engine(at) {
            let _ = downloads.delete_part(unit);
        }
    }

    /// Start: what an earlier run left that matches no pin is removed (resumable parts stay).
    fn sweep(&self) {
        let Some(downloads) = self.downloads.as_ref().filter(|_| self.dir.is_dir()) else {
            return;
        };
        let units: Vec<Unit> = self.units.iter().flatten().cloned().collect();
        let _ = downloads.sweep(&units);
    }

    /// Queues the download of language `at` (one runs at a time). Does nothing while it runs | is queued | it is
    /// there.
    pub fn download(&self, at: usize) {
        let Some(total) = self.languages.get(at).map(Language::bytes) else {
            return;
        };
        // `Started` fits only `Absent` | `Failed`: the check and the move are one step
        if !self.apply(at, Event::Started { total }) {
            return;
        }
        match self.engine(at) {
            Some((downloads, unit)) => downloads.enqueue(unit.clone()),
            None => {
                self.apply(at, Event::Failed(SpellingFailure::Offline));
            }
        }
    }

    /// Asks a running | queued download to stop; its partial files are removed.
    pub fn cancel(&self, at: usize) {
        if let Some((downloads, unit)) = self.engine(at) {
            downloads.cancel(unit.id());
        }
    }

    /// Stops every download and waits for the engine (teardown); the loaded dictionaries are dropped.
    pub fn shutdown(&self) {
        if let Some(downloads) = &self.downloads {
            downloads.shutdown();
        }
        for at in 0..self.languages.len() {
            let downloading = matches!(
                lock(&self.inner).states.get(at),
                Some(SpellingState::Downloading { .. })
            );
            // queued ones are dropped unannounced by the engine
            if downloading {
                self.apply(at, Event::Canceled);
            }
        }
        lock(&self.inner).loaded.fill(None);
    }

    /// The dictionary of language `at`, loaded (and verified) on first use.
    ///
    /// # Errors
    /// `NotReady`: not downloaded; the cached files are not the official ones (removed, the state says so); the
    /// engine cannot read them (the state says so).
    pub fn load(&self, at: usize) -> Result<Arc<Dictionary>, Unavailable> {
        {
            let inner = lock(&self.inner);
            if let Some(Some(dictionary)) = inner.loaded.get(at) {
                return Ok(Arc::clone(dictionary));
            }
            if inner.states.get(at) != Some(&SpellingState::Ready) {
                return Err(Unavailable::NotReady);
            }
        }
        let _parse = lock(&self.loading);
        if let Some(Some(dictionary)) = lock(&self.inner).loaded.get(at) {
            return Ok(Arc::clone(dictionary));
        }
        let (Some(language), Some((downloads, unit))) = (self.languages.get(at), self.engine(at))
        else {
            return Err(Unavailable::NotReady);
        };
        let dir = unit.dir_in(&self.dir).ok();
        let files = dir.and_then(|dir| {
            Some((
                read_pinned(&dir.join(AFF), language.aff)?,
                read_pinned(&dir.join(DIC), language.dic)?,
            ))
        });
        let Some((aff, dic)) = files else {
            let _ = downloads.remove(unit);
            self.apply(at, Event::Failed(SpellingFailure::Hash));
            return Err(Unavailable::NotReady);
        };
        let (aff, dic) = readable(&aff, &dic);
        let Ok(dictionary) = Dictionary::new(&aff, &dic) else {
            self.apply(at, Event::Failed(SpellingFailure::Unreadable));
            return Err(Unavailable::NotReady);
        };
        let dictionary = Arc::new(dictionary);
        if let Some(slot) = lock(&self.inner).loaded.get_mut(at) {
            *slot = Some(Arc::clone(&dictionary));
        }
        Ok(dictionary)
    }
}
