//! The vocabulary store: the rank files of the exact encodings in the app cache, and the download that puts them there.
//!
//! A file is only ever the official one: its size and SHA-256 are pinned in code and checked when it arrives (core
//! `downloads` engine) and again when it is loaded. Nothing here touches the network until [`VocabStore::download`] is
//! called, and the plugin calls that only for a download the person confirmed.
//!
//! Presence = file of the pinned size (hash checked on load), NOT the engine's install manifest: files downloaded
//! before the engine have none and are never fetched again. No partial file outlives a download that did not finish
//! (cancel, failure, shutdown, app start) — as before the engine.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use permark_core::downloads::{Asset, DownloadError, DownloadKit, Downloads, Source, Unit, Update};
use serde::Serialize;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::bpe::{Bpe, Ranks};
use crate::tokenizer::Encoding;

/// Why a vocabulary is not there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum VocabFailure {
    /// No connection to the source (or it dropped).
    Offline,
    /// The source answered with this status.
    Http { status: u16 },
    /// What arrived (or what is in the cache) is not the official file.
    Hash,
    /// The file could not be written.
    Disk,
}

/// Where one vocabulary stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum VocabState {
    Absent,
    Downloading {
        #[ts(type = "number")]
        done: u64,
        #[ts(type = "number")]
        total: u64,
    },
    Ready,
    Failed {
        reason: VocabFailure,
    },
}

/// What happens to a vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A file of the right size was found at start.
    Found,
    Started {
        total: u64,
    },
    Progress {
        done: u64,
        total: u64,
    },
    Finished,
    Failed(VocabFailure),
    Canceled,
    Removed,
}

impl VocabState {
    /// `(state, event) -> state`; an event that does not fit the state changes nothing.
    #[must_use]
    pub fn next(&self, event: Event) -> Self {
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
            (Self::Downloading { .. }, Event::Canceled)
            | (Self::Ready | Self::Failed { .. }, Event::Removed) => Self::Absent,
            (state, _) => state.clone(),
        }
    }
}

/// What the official file must be: its path at [`SOURCE`], size and SHA-256.
#[derive(Debug, Clone)]
pub struct Pin {
    /// Path at [`SOURCE`] (`/encodings/<file>`).
    pub path: String,
    pub bytes: u64,
    pub sha256: [u8; 32],
}

fn hex32(text: &str) -> [u8; 32] {
    let mut out = [0_u8; 32];
    for (at, pair) in text.as_bytes().chunks(2).take(32).enumerate() {
        let digit = |b: u8| u8::try_from(char::from(b).to_digit(16).unwrap_or(0)).unwrap_or(0);
        out[at] = (digit(pair[0]) << 4) | digit(*pair.get(1).unwrap_or(&b'0'));
    }
    out
}

impl Pin {
    /// The official file of an encoding.
    #[must_use]
    pub fn official(encoding: Encoding) -> Self {
        Self {
            path: encoding.path().to_owned(),
            bytes: encoding.bytes(),
            sha256: hex32(encoding.sha256()),
        }
    }
}

/// Where the rank files come from: the public blob store of `OpenAI`; a redirect to any other host is refused.
pub const SOURCE: Source = Source::new(
    "openai-public",
    "https://openaipublic.blob.core.windows.net",
    &["openaipublic.blob.core.windows.net"],
);

/// Install unit of `encoding` (id = encoding id, file = its cache name, store root). `None`: a bad pin (a bug).
fn unit_of(encoding: Encoding, pin: &Pin) -> Option<Unit> {
    let asset = Asset::new(
        SOURCE,
        pin.path.clone(),
        encoding.file(),
        pin.bytes,
        pin.sha256,
    )
    .ok()?;
    Unit::new(encoding.id(), "", vec![asset]).ok()
}

/// Why a vocabulary cannot be used now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// Not downloaded (or still arriving, or failed).
    NoVocab,
}

/// Told after every change: the states of the vocabularies (`Encoding::index` order).
pub type Listener = Arc<dyn Fn(&[VocabState; 2]) + Send + Sync>;

struct Inner {
    states: [VocabState; 2],
    loaded: [Option<Arc<Bpe>>; 2],
}

pub struct VocabStore {
    dir: PathBuf,
    pins: [Pin; 2],
    /// Engine over `dir` (`None`: the app offers no download service -> a download fails `Offline`).
    downloads: Option<Downloads>,
    /// One unit per encoding (`Encoding::index` order).
    units: [Option<Unit>; 2],
    listener: Listener,
    inner: Mutex<Inner>,
    /// One parse at a time.
    loading: Mutex<()>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the windows are told of a download failure (the `VocabFailure` type stays as before the engine).
fn failure_of(error: &DownloadError) -> VocabFailure {
    match error {
        // a refused host | address reads as "cannot reach the source"; `Cancelled` never reaches here (`heard` turns
        // it into `Event::Canceled` first), listed only to keep the match exhaustive
        DownloadError::Offline | DownloadError::HostNotAllowed | DownloadError::Cancelled => {
            VocabFailure::Offline
        }
        DownloadError::Http { status } => VocabFailure::Http { status: *status },
        DownloadError::Integrity => VocabFailure::Hash,
        DownloadError::NoSpace { .. } | DownloadError::Disk { .. } | DownloadError::Busy => {
            VocabFailure::Disk
        }
    }
}

impl VocabStore {
    /// The store over the cache folder `dir`, downloading through `kit` (the app's service). A file of the pinned size
    /// counts as there (its hash is checked when it is loaded); partial downloads left over are removed. Nothing is
    /// created or fetched.
    #[must_use]
    pub fn new(
        dir: PathBuf,
        pins: [Pin; 2],
        kit: Option<&DownloadKit>,
        listener: Listener,
    ) -> Arc<Self> {
        let mut states = [VocabState::Absent, VocabState::Absent];
        for encoding in Encoding::ALL {
            let there = fs::metadata(dir.join(encoding.file()))
                .is_ok_and(|m| m.is_file() && m.len() == pins[encoding.index()].bytes);
            if there {
                states[encoding.index()] = states[encoding.index()].next(Event::Found);
            }
        }
        let units = Encoding::ALL.map(|encoding| unit_of(encoding, &pins[encoding.index()]));
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
            Self {
                dir,
                pins,
                downloads,
                units,
                listener,
                inner: Mutex::new(Inner {
                    states,
                    loaded: [None, None],
                }),
                loading: Mutex::new(()),
            }
        });
        store.remove_leftovers();
        store
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    #[must_use]
    pub fn pin(&self, encoding: Encoding) -> &Pin {
        &self.pins[encoding.index()]
    }

    #[must_use]
    pub fn states(&self) -> [VocabState; 2] {
        lock(&self.inner).states.clone()
    }

    /// Engine + unit of `encoding` (`None`: no download service | a bad pin).
    fn engine(&self, encoding: Encoding) -> Option<(&Downloads, &Unit)> {
        Some((
            self.downloads.as_ref()?,
            self.units[encoding.index()].as_ref()?,
        ))
    }

    /// Applies an event; tells the listener when the state changed. Returns whether it changed.
    fn apply(&self, encoding: Encoding, event: Event) -> bool {
        let snapshot = {
            let mut inner = lock(&self.inner);
            let at = encoding.index();
            let after = inner.states[at].next(event);
            if after == inner.states[at] {
                return false;
            }
            inner.states[at] = after;
            inner.states.clone()
        };
        (self.listener)(&snapshot);
        true
    }

    /// Engine news of unit `id`. A download that did not finish leaves no partial file (removed before it is told).
    fn heard(&self, id: &str, update: &Update) {
        let Some(encoding) = Encoding::ALL
            .into_iter()
            .find(|e| self.units[e.index()].as_ref().is_some_and(|u| u.id() == id))
        else {
            return;
        };
        let event = match update {
            Update::Progress(progress) => Event::Progress {
                done: progress.done,
                total: progress.total,
            },
            Update::Done => Event::Finished,
            Update::Failed(error) => {
                self.delete_part(encoding);
                match error {
                    DownloadError::Cancelled => Event::Canceled,
                    other => Event::Failed(failure_of(other)),
                }
            }
        };
        self.apply(encoding, event);
    }

    fn delete_part(&self, encoding: Encoding) {
        if let Some((downloads, unit)) = self.engine(encoding) {
            let _ = downloads.delete_part(unit);
        }
    }

    /// Start: partial downloads of an earlier run are removed (never resumed), as before the engine.
    fn remove_leftovers(&self) {
        let Some(downloads) = self.downloads.as_ref().filter(|_| self.dir.is_dir()) else {
            return;
        };
        let units: Vec<Unit> = self.units.iter().flatten().cloned().collect();
        let Ok(paused) = downloads.sweep(&units) else {
            return;
        };
        for unit in units
            .iter()
            .filter(|u| paused.iter().any(|p| p.unit == u.id()))
        {
            let _ = downloads.delete_part(unit);
        }
    }

    /// Queues the download of `encoding` (one runs at a time). Does nothing while it runs | is queued | the file is
    /// there.
    pub fn download(&self, encoding: Encoding) {
        let total = self.pins[encoding.index()].bytes;
        // `Started` fits only `Absent` | `Failed`: the check and the move are one step
        if !self.apply(encoding, Event::Started { total }) {
            return;
        }
        match self.engine(encoding) {
            Some((downloads, unit)) => downloads.enqueue(unit.clone()),
            None => {
                self.apply(encoding, Event::Failed(VocabFailure::Offline));
            }
        }
    }

    /// Asks a running | queued download to stop; its partial file is removed.
    pub fn cancel(&self, encoding: Encoding) {
        if let Some((downloads, unit)) = self.engine(encoding) {
            downloads.cancel(unit.id());
        }
    }

    /// Deletes the cached file (not while it downloads).
    ///
    /// # Errors
    /// The file could not be removed.
    pub fn remove(&self, encoding: Encoding) -> std::io::Result<()> {
        if matches!(
            lock(&self.inner).states[encoding.index()],
            VocabState::Downloading { .. }
        ) {
            return Ok(());
        }
        let _parse = lock(&self.loading);
        lock(&self.inner).loaded[encoding.index()] = None;
        self.discard(encoding)?;
        self.apply(encoding, Event::Removed);
        Ok(())
    }

    /// Deletes the cached file of `encoding` (absent: fine), then the engine's record + parts of it.
    fn discard(&self, encoding: Encoding) -> std::io::Result<()> {
        match fs::remove_file(self.dir.join(encoding.file())) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        if let Some((downloads, unit)) = self.engine(encoding) {
            let _ = downloads.remove(unit);
        }
        Ok(())
    }

    /// Stops every download and waits for the engine (teardown); nothing partial is left.
    pub fn shutdown(&self) {
        if let Some(downloads) = &self.downloads {
            downloads.shutdown();
        }
        for encoding in Encoding::ALL {
            let downloading = matches!(
                lock(&self.inner).states[encoding.index()],
                VocabState::Downloading { .. }
            );
            // queued ones are dropped unannounced by the engine
            if downloading {
                self.delete_part(encoding);
                self.apply(encoding, Event::Canceled);
            }
        }
        lock(&self.inner).loaded = [None, None];
    }

    /// The tables of `encoding`, loaded (and verified) on first use.
    ///
    /// # Errors
    /// `NoVocab`: not downloaded, or the cached file is not the official one (it is deleted and the state says so).
    pub fn load(&self, encoding: Encoding) -> Result<Arc<Bpe>, Unavailable> {
        let at = encoding.index();
        {
            let inner = lock(&self.inner);
            if let Some(bpe) = &inner.loaded[at] {
                return Ok(Arc::clone(bpe));
            }
            if inner.states[at] != VocabState::Ready {
                return Err(Unavailable::NoVocab);
            }
        }
        let _parse = lock(&self.loading);
        if let Some(bpe) = &lock(&self.inner).loaded[at] {
            return Ok(Arc::clone(bpe));
        }
        let path = self.dir.join(encoding.file());
        let pin = &self.pins[at];
        let bpe = fs::read(&path)
            .ok()
            .filter(|bytes| {
                bytes.len() as u64 == pin.bytes && Sha256::digest(bytes)[..] == pin.sha256
            })
            .and_then(|bytes| Ranks::parse(&bytes).ok())
            .map(|ranks| Arc::new(Bpe::new(ranks, encoding.rules())));
        if let Some(bpe) = bpe {
            lock(&self.inner).loaded[at] = Some(Arc::clone(&bpe));
            Ok(bpe)
        } else {
            let _ = self.discard(encoding);
            self.apply(encoding, Event::Failed(VocabFailure::Hash));
            Err(Unavailable::NoVocab)
        }
    }
}

#[cfg(test)]
mod tests;
