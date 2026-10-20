//! The vocabulary store: the rank files of the exact encodings in the app cache, and the download that puts them there.
//!
//! A file is only ever the official one: its size and SHA-256 are pinned in code and checked when it arrives and again
//! when it is loaded. Nothing here touches the network until [`VocabStore::download`] is called, and the plugin calls
//! that only for a download the person confirmed.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::bpe::{Bpe, Ranks};
use crate::fetch::{Fetch, FetchError};
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

/// What the official file must be.
#[derive(Debug, Clone)]
pub struct Pin {
    pub url: String,
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
            url: encoding.url().to_owned(),
            bytes: encoding.bytes(),
            sha256: hex32(encoding.sha256()),
        }
    }
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
    cancels: [Option<Arc<AtomicBool>>; 2],
    loaded: [Option<Arc<Bpe>>; 2],
    threads: Vec<JoinHandle<()>>,
}

pub struct VocabStore {
    dir: PathBuf,
    pins: [Pin; 2],
    fetch: Arc<dyn Fetch>,
    listener: Listener,
    inner: Mutex<Inner>,
    /// One parse at a time.
    loading: Mutex<()>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A file being downloaded: removed again unless it was kept.
struct PartFile {
    path: PathBuf,
    keep: bool,
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// How a download job ended, apart from success.
enum Stop {
    Canceled,
    Failed(VocabFailure),
}

/// The least time between two progress announcements.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

impl VocabStore {
    /// The store over the cache folder `dir`. A file of the pinned size counts as there (its hash is checked when it is
    /// loaded); stale `.part` files are removed. Nothing is created or fetched.
    #[must_use]
    pub fn new(
        dir: PathBuf,
        pins: [Pin; 2],
        fetch: Arc<dyn Fetch>,
        listener: Listener,
    ) -> Arc<Self> {
        let mut states = [VocabState::Absent, VocabState::Absent];
        for encoding in Encoding::ALL {
            let _ = fs::remove_file(dir.join(format!("{}.part", encoding.file())));
            let there = fs::metadata(dir.join(encoding.file()))
                .is_ok_and(|m| m.is_file() && m.len() == pins[encoding.index()].bytes);
            if there {
                states[encoding.index()] = states[encoding.index()].next(Event::Found);
            }
        }
        Arc::new(Self {
            dir,
            pins,
            fetch,
            listener,
            inner: Mutex::new(Inner {
                states,
                cancels: [None, None],
                loaded: [None, None],
                threads: Vec::new(),
            }),
            loading: Mutex::new(()),
        })
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

    /// Starts the download of `encoding` on a thread of its own. Does nothing while one runs or the file is there.
    pub fn download(self: &Arc<Self>, encoding: Encoding) {
        let total = self.pins[encoding.index()].bytes;
        let cancel = Arc::new(AtomicBool::new(false));
        let snapshot = {
            let mut inner = lock(&self.inner);
            let at = encoding.index();
            if !matches!(
                inner.states[at],
                VocabState::Absent | VocabState::Failed { .. }
            ) {
                return;
            }
            inner.cancels[at] = Some(Arc::clone(&cancel));
            inner.states[at] = inner.states[at].next(Event::Started { total });
            inner.states.clone()
        };
        (self.listener)(&snapshot);
        let store = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("permark-token-vocab".to_owned())
            .spawn(move || store.run_download(encoding, &cancel));
        if let Ok(handle) = spawned {
            lock(&self.inner).threads.push(handle);
        } else {
            lock(&self.inner).cancels[encoding.index()] = None;
            self.apply(encoding, Event::Failed(VocabFailure::Disk));
        }
    }

    /// Asks a running download to stop; its file is removed.
    pub fn cancel(&self, encoding: Encoding) {
        if let Some(flag) = &lock(&self.inner).cancels[encoding.index()] {
            flag.store(true, Ordering::Relaxed);
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
        match fs::remove_file(self.dir.join(encoding.file())) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        self.apply(encoding, Event::Removed);
        Ok(())
    }

    /// Stops every download and waits for the threads (teardown).
    pub fn shutdown(&self) {
        let threads = {
            let mut inner = lock(&self.inner);
            for flag in inner.cancels.iter().flatten() {
                flag.store(true, Ordering::Relaxed);
            }
            std::mem::take(&mut inner.threads)
        };
        for thread in threads {
            let _ = thread.join();
        }
        let mut inner = lock(&self.inner);
        inner.loaded = [None, None];
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
            let _ = fs::remove_file(&path);
            self.apply(encoding, Event::Failed(VocabFailure::Hash));
            Err(Unavailable::NoVocab)
        }
    }

    fn run_download(&self, encoding: Encoding, cancel: &AtomicBool) {
        let event = match self.fetch_and_keep(encoding, cancel) {
            Ok(()) => Event::Finished,
            Err(Stop::Canceled) => Event::Canceled,
            Err(Stop::Failed(reason)) => Event::Failed(reason),
        };
        lock(&self.inner).cancels[encoding.index()] = None;
        self.apply(encoding, event);
    }

    fn fetch_and_keep(&self, encoding: Encoding, cancel: &AtomicBool) -> Result<(), Stop> {
        let pin = &self.pins[encoding.index()];
        let disk = |_| Stop::Failed(VocabFailure::Disk);
        fs::create_dir_all(&self.dir).map_err(disk)?;
        let mut part = PartFile {
            path: self.dir.join(format!("{}.part", encoding.file())),
            keep: false,
        };
        let mut out = File::create(&part.path).map_err(disk)?;
        let mut hasher = Sha256::new();
        let mut done = 0_u64;
        let mut last = Instant::now();
        let mut stop: Option<Stop> = None;
        let fetched = self.fetch.get(&pin.url, &mut |chunk| {
            if cancel.load(Ordering::Relaxed) {
                stop = Some(Stop::Canceled);
                return false;
            }
            done += chunk.len() as u64;
            if done > pin.bytes {
                stop = Some(Stop::Failed(VocabFailure::Hash));
                return false;
            }
            hasher.update(chunk);
            if out.write_all(chunk).is_err() {
                stop = Some(Stop::Failed(VocabFailure::Disk));
                return false;
            }
            if last.elapsed() >= PROGRESS_EVERY {
                last = Instant::now();
                self.apply(
                    encoding,
                    Event::Progress {
                        done,
                        total: pin.bytes,
                    },
                );
            }
            true
        });
        if let Some(stop) = stop {
            return Err(stop);
        }
        match fetched {
            Err(FetchError::Offline) => return Err(Stop::Failed(VocabFailure::Offline)),
            Err(FetchError::Status(status)) => {
                return Err(Stop::Failed(VocabFailure::Http { status }))
            }
            Ok(()) => {}
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(Stop::Canceled);
        }
        if done != pin.bytes || hasher.finalize()[..] != pin.sha256 {
            return Err(Stop::Failed(VocabFailure::Hash));
        }
        out.sync_all().map_err(disk)?;
        drop(out);
        let target = self.dir.join(encoding.file());
        let _ = fs::remove_file(&target);
        fs::rename(&part.path, &target).map_err(disk)?;
        part.keep = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
