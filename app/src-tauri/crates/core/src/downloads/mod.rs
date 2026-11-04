//! Verified downloads: a pinned file (source + path, size, sha256) the person asked for, fetched over an allowlisted
//! `https` source into a store folder the caller names. Pure over the [`Transport`] seam (app crate supplies the real
//! client) and the [`SecretStore`] seam (OS credential store in the app crate). Nothing here touches the network by
//! itself: a plugin calls it after a person's action.
//!
//! Engine: [`Downloads`] = one worker thread per store, FIFO queue of install [`Unit`]s, store lock vs a second app
//! instance, `.part` + `.part.meta` resume, startup [`Downloads::sweep`].

mod error;
mod part;
mod secret;
mod transport;
mod unit;

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::state::shared_write::{lock_within, FileLock};

pub use error::DownloadError;
pub use secret::{MemorySecrets, Secret, SecretError, SecretStore};
use transport::open;
pub use transport::{Asked, Request, Response, Scripted, ScriptedTransport, Transport};
pub use unit::Unit;

/// Where pinned files come from: a fixed `https` base (no free URLs from callers) + host suffixes every hop of a
/// redirect chain must match. Defined in code (pins are covered by the app signature).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    key: &'static str,
    base: &'static str,
    hosts: &'static [&'static str],
    credential: bool,
}

impl Source {
    /// Hugging Face: `https://huggingface.co`; its CDN redirects stay under `huggingface.co` | `hf.co`. Accepts a
    /// credential (token for gated models).
    pub const HUGGING_FACE: Self = Self::new(
        "huggingface",
        "https://huggingface.co",
        &["huggingface.co", "hf.co"],
    )
    .with_credential();

    /// A source: `key` names it (credential slot), `base` = `https://host` (no path, no trailing `/`), `hosts` =
    /// allowed host suffixes (a host matches itself | any subdomain). A base outside `hosts` | not `https` is refused
    /// at request time ([`DownloadError::HostNotAllowed`]). No credential: the credential store is never read for it
    /// (no OS keychain prompt); see [`Source::with_credential`].
    #[must_use]
    pub const fn new(
        key: &'static str,
        base: &'static str,
        hosts: &'static [&'static str],
    ) -> Self {
        Self {
            key,
            base,
            hosts,
            credential: false,
        }
    }

    /// Same source, its credential (slot [`Source::key`]) read + sent to the first host.
    #[must_use]
    pub const fn with_credential(mut self) -> Self {
        self.credential = true;
        self
    }

    /// Whether a credential is read + sent for this source.
    #[must_use]
    pub const fn accepts_credential(&self) -> bool {
        self.credential
    }

    /// Credential slot name (`SecretStore` key).
    #[must_use]
    pub const fn key(&self) -> &'static str {
        self.key
    }

    /// Address of `path` (validated by [`Asset::new`]: starts with `/`).
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Host + port of the base: the only origin a credential is sent to.
    fn first_origin(&self) -> Option<(String, u16)> {
        origin_of(self.base)
    }

    /// `host` equals an allowed suffix or is a subdomain of one (case-insensitive, one trailing dot ignored).
    fn allows(&self, host: &str) -> bool {
        self.hosts.iter().any(|suffix| {
            let suffix = suffix.to_ascii_lowercase();
            host == suffix
                || host
                    .strip_suffix(suffix.as_str())
                    .is_some_and(|head| head.ends_with('.'))
        })
    }
}

/// Lowercase host (one trailing dot dropped) + port (none: 443) of an `https://` address. `None`: not `https://`, a
/// host holding anything but ASCII letters | digits | `-` | `.` (escapes, backslashes, bracketed IP literals), a port
/// not all digits.
fn origin_of(url: &str) -> Option<(String, u16)> {
    let rest = url
        .get(..8)
        .filter(|s| s.eq_ignore_ascii_case("https://"))
        .map(|_| &url[8..])?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, port.parse().ok()?)
        }
        Some(_) => return None,
        None => (authority, 443),
    };
    let host = host.strip_suffix('.').unwrap_or(host);
    let plain = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'.';
    (!host.is_empty() && host.bytes().all(plain)).then(|| (host.to_ascii_lowercase(), port))
}

/// Why an [`Asset`] cannot be built (a bug in the pin, never person input).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidAsset {
    /// Path not `/...`, or holding `..`, `\`, `?`, `#`, whitespace, control characters.
    #[error("invalid asset path")]
    Path,
    /// Name not a plain name on every system: empty, a path, `.`-leading (scratch names are hidden + swept), ending in
    /// `.` | space, holding `<>:"|?*` | control characters, a Windows device name (`CON`, `NUL.txt`, `COM1`, ...).
    #[error("invalid asset file name")]
    Name,
    /// Unit id | folder segment not a plain name, no files, two files of one name (case-insensitive), a file name
    /// ending `.part` | `.part.meta` (the store's own temp names) | named `<unit id>.installed.json` (its manifest).
    #[error("invalid install unit")]
    Unit,
}

/// Plain file | folder name on every system: not empty, not `.`-leading (refuses `.` + `..` too), not ending in `.` |
/// space, no separator | drive colon | `<>"|?*` | control character, not a Windows device name (`CON`, `PRN`, `AUX`,
/// `NUL`, `COM0-9`, `LPT0-9`, `CONIN$`, `CONOUT$`; with any extension, any case).
fn plain_name(name: &str) -> bool {
    const DEVICES: [&str; 6] = ["con", "prn", "aux", "nul", "conin$", "conout$"];
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_lowercase();
    let numbered = stem.len() == 4
        && (stem.starts_with("com") || stem.starts_with("lpt"))
        && stem.as_bytes().get(3).is_some_and(u8::is_ascii_digit);
    !(name.is_empty()
        || name.starts_with('.')
        || name.ends_with(['.', ' '])
        || name.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        || name.chars().any(char::is_control)
        || numbered
        || DEVICES.contains(&stem.as_str()))
}

/// One pinned file: where it comes from, what it must be, what it is called in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    source: Source,
    path: String,
    name: String,
    bytes: u64,
    sha256: [u8; 32],
    revision: Option<String>,
}

impl Asset {
    /// Pin of `source` + `path` (e.g. `/org/repo/resolve/<commit>/model.gguf`) of exactly `bytes` bytes hashing to
    /// `sha256`, stored as `name`.
    ///
    /// # Errors
    /// [`InvalidAsset`]: bad path | name.
    pub fn new(
        source: Source,
        path: impl Into<String>,
        name: impl Into<String>,
        bytes: u64,
        sha256: [u8; 32],
    ) -> Result<Self, InvalidAsset> {
        let (path, name) = (path.into(), name.into());
        let bad_char = |c: char| c.is_whitespace() || c.is_control();
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains(['\\', '?', '#'])
            || path.chars().any(bad_char)
            || path.split('/').any(|segment| segment == "..")
        {
            return Err(InvalidAsset::Path);
        }
        if !plain_name(&name) {
            return Err(InvalidAsset::Name);
        }
        Ok(Self {
            source,
            path,
            name,
            bytes,
            sha256,
            revision: None,
        })
    }

    /// Records the pinned revision (e.g. a commit sha) in the part meta; not part of the address.
    #[must_use]
    pub fn with_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
    }

    #[must_use]
    pub const fn source(&self) -> &Source {
        &self.source
    }

    /// File name in the store.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Pinned size.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Pinned sha256.
    #[must_use]
    pub const fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    #[must_use]
    pub fn revision(&self) -> Option<&str> {
        self.revision.as_deref()
    }

    /// First address asked (the source's base + path).
    #[must_use]
    pub fn url(&self) -> String {
        self.source.url(&self.path)
    }
}

/// Step of one download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "DownloadPhase")]
pub enum Phase {
    Downloading,
    /// Rehashing an existing part before resuming | the final check.
    Verifying,
}

/// Progress of one download (plugin UIs show it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "DownloadProgress")]
pub struct Progress {
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number")]
    pub total: u64,
    pub phase: Phase,
}

/// What happened to a queued unit (listener). `Done` | `Failed` end it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// Bytes of the whole unit, throttled (~100 ms).
    Progress(Progress),
    /// Every file verified + manifest written (or already installed).
    Done,
    /// Stopped; `Cancelled` | `Offline` | `Http` | `NoSpace` | `Disk` keep the part for resume, `Integrity` removed it.
    Failed(DownloadError),
}

/// Called from the worker thread with the unit id; keep it short (forward to a channel | event sink).
pub type Listener = Arc<dyn Fn(&str, &Update) + Send + Sync>;

/// A resumable part found by [`Downloads::sweep`]: listed, NEVER restarted by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paused {
    pub unit: String,
    /// Asset file name.
    pub name: String,
    pub done: u64,
    pub total: u64,
}

/// The app's real network + credential store, offered as an app service (`services.get::<DownloadKit>()`, app crate
/// provides it): a plugin builds its own [`Downloads`] over its store root with [`DownloadKit::downloads`].
#[derive(Clone)]
pub struct DownloadKit {
    pub transport: Arc<dyn Transport>,
    pub secrets: Arc<dyn SecretStore>,
}

impl std::fmt::Debug for DownloadKit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadKit").finish_non_exhaustive()
    }
}

impl DownloadKit {
    /// Engine over `store` with this kit's transport + credentials ([`Downloads::new`]).
    #[must_use]
    pub fn downloads(&self, store: PathBuf, listener: Listener) -> Downloads {
        Downloads::new(
            store,
            Arc::clone(&self.transport),
            Some(Arc::clone(&self.secrets)),
            listener,
        )
    }
}

/// Free bytes on the volume of a path (seam: tests inject a full disk).
type FreeSpace = fn(&Path) -> io::Result<u64>;
/// Store lock within a wait ([`lock_within`]; seam: tests inject a system without locks).
type StoreLocker = fn(&Path, Duration) -> io::Result<Option<FileLock>>;

/// System hooks of the engine (real ones: [`Seams::SYSTEM`]).
#[derive(Clone, Copy)]
struct Seams {
    free_space: FreeSpace,
    lock: StoreLocker,
}

impl Seams {
    const SYSTEM: Self = Self {
        free_space: crate::fsx::free_space,
        lock: lock_within,
    };
}

/// Least room kept free beyond the remaining bytes (also at least 1 % of them).
const MARGIN: u64 = 64 << 20;
/// Worker's wait between tries for a store lock another instance holds (enqueue | cancel | shutdown wake it sooner).
const LOCK_POLL: Duration = Duration::from_millis(200);
/// Unit refused: no store lock to be had on this system (no lock folder | file system without locks). Downloading
/// unlocked could let a second instance write the same part.
const NO_LOCK: &str = "store lock unavailable";

/// Verified-download engine over one store folder (caller's root: plugin cache | profile data). One worker thread
/// (started by the first [`Downloads::enqueue`]), one unit at a time, FIFO. Store lock ([`lock_within`] on the root)
/// held per running unit: on a store another app instance downloads into, queued units wait their turn and cleanup
/// calls answer [`DownloadError::Busy`]. Drop = [`Downloads::shutdown`].
pub struct Downloads {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct Shared {
    store: PathBuf,
    transport: Arc<dyn Transport>,
    secrets: Option<Arc<dyn SecretStore>>,
    listener: Listener,
    seams: Seams,
    queue: Mutex<Queue>,
    wake: Condvar,
}

#[derive(Default)]
struct Queue {
    waiting: VecDeque<Unit>,
    /// Running unit; set + cleared under the queue mutex together with the store lock it holds, so cleanup calls
    /// read "this worker holds the store" exactly while it does.
    active: Option<Active>,
    stopping: bool,
}

struct Active {
    id: String,
    cancel: Arc<AtomicBool>,
    /// `delete_part` asked meanwhile: worker removes the parts once the unit stops.
    discard: bool,
    /// Store lock, released when the unit stops (`None`: none to be had; the unit is refused, [`NO_LOCK`]).
    _lock: Option<FileLock>,
}

impl std::fmt::Debug for Downloads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Downloads")
            .field("store", &self.shared.store)
            .finish_non_exhaustive()
    }
}

impl Downloads {
    /// Engine over `store`: MUST be a folder dedicated to this engine (sweep + remove act on its unit folders; no
    /// worker thread, folder, request until [`Downloads::enqueue`]). `secrets`: per-source credential, read ONLY for a
    /// source that accepts one ([`Source::with_credential`]) and sent to its first host only; store unreachable ->
    /// asked without one.
    #[must_use]
    pub fn new(
        store: PathBuf,
        transport: Arc<dyn Transport>,
        secrets: Option<Arc<dyn SecretStore>>,
        listener: Listener,
    ) -> Self {
        Self::with_seams(store, transport, secrets, listener, Seams::SYSTEM)
    }

    fn with_seams(
        store: PathBuf,
        transport: Arc<dyn Transport>,
        secrets: Option<Arc<dyn SecretStore>>,
        listener: Listener,
        seams: Seams,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                store,
                transport,
                secrets,
                listener,
                seams,
                queue: Mutex::new(Queue::default()),
                wake: Condvar::new(),
            }),
            worker: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn store(&self) -> &Path {
        &self.shared.store
    }

    /// Worker thread started (test seam).
    #[cfg(test)]
    fn started(&self) -> bool {
        self.worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Queues `unit` (FIFO); the first call starts the worker thread. Already queued | running | shutting down:
    /// nothing. Already installed: `Done` at its turn. Thread cannot start: `Failed(Disk)`.
    pub fn enqueue(&self, unit: Unit) {
        // worker slot first, queue inside: `shutdown` (queue, then slot) never sees a worker started after it
        let mut worker = self.worker.lock().unwrap_or_else(PoisonError::into_inner);
        {
            let mut queue = self.shared.queue();
            let known = queue.active.as_ref().is_some_and(|a| a.id == unit.id())
                || queue.waiting.iter().any(|u| u.id() == unit.id());
            if queue.stopping || known {
                return;
            }
            queue.waiting.push_back(unit);
        }
        self.shared.wake.notify_all();
        if worker.is_some() {
            return;
        }
        let shared = Arc::clone(&self.shared);
        match std::thread::Builder::new()
            .name("downloads".into())
            .spawn(move || shared.work())
        {
            Ok(handle) => *worker = Some(handle),
            Err(error) => {
                drop(worker);
                // no worker yet: the queue holds only this unit (unless a cancel took it, already told)
                let unit = self.shared.queue().waiting.pop_back();
                if let Some(unit) = unit {
                    (self.shared.listener)(unit.id(), &Update::Failed(DownloadError::disk(&error)));
                }
            }
        }
    }

    /// Stops unit `id`: running -> stops before its next read | redirect hop (part kept); queued -> dropped. Both
    /// answer `Failed(Cancelled)`. Latency bound: see [`Downloads::shutdown`].
    pub fn cancel(&self, id: &str) {
        let dropped = {
            let mut queue = self.shared.queue();
            match queue.active.as_ref().filter(|a| a.id == id) {
                Some(active) => {
                    active.cancel.store(true, Ordering::Relaxed);
                    false
                }
                None => queue.drop_waiting(id),
            }
        };
        if dropped {
            (self.shared.listener)(id, &Update::Failed(DownloadError::Cancelled));
        }
    }

    /// Removes the `.part` + `.part.meta` files of `unit` (installed files untouched). Running -> stopped, the worker
    /// removes them (`Failed(Cancelled)`); queued -> dropped first.
    ///
    /// # Errors
    /// `Busy` (another instance holds the store), `Disk` (incl. a unit folder that is a link).
    pub fn delete_part(&self, unit: &Unit) -> Result<(), DownloadError> {
        let store = &self.shared.store;
        self.while_quiet(unit, true, || {
            part::discard_all(&unit.dir_in(store)?, unit.assets())
        })
    }

    /// Uninstalls `unit`: manifest first (no longer installed), then its files + parts; Windows locks retried.
    ///
    /// # Errors
    /// `Busy` (running here | another instance holds the store), `Disk` (incl. a unit folder that is a link).
    pub fn remove(&self, unit: &Unit) -> Result<(), DownloadError> {
        self.while_quiet(unit, false, || unit::remove(&self.shared.store, unit))
    }

    /// Manifest present, listing exactly `unit`'s pins, every file of its pinned size (a linked unit folder: no).
    #[must_use]
    pub fn is_installed(&self, unit: &Unit) -> bool {
        unit::is_installed(&self.shared.store, unit)
    }

    /// Startup cleanup in the folders of `units`: removes a part without meta, a meta without part, a pair matching
    /// no pin of `units` (or longer than it), a linked part; lists the valid parts as paused (NEVER restarted).
    /// Scratch names ([`crate::fsx::is_scratch_name`]) + folders untouched. Call before queueing.
    ///
    /// # Errors
    /// `Busy` (units queued | running here, or another instance holds the store), `Disk` (incl. a unit folder that is
    /// a link).
    pub fn sweep(&self, units: &[Unit]) -> Result<Vec<Paused>, DownloadError> {
        let queue = self.shared.queue();
        if queue.active.is_some() || !queue.waiting.is_empty() {
            return Err(DownloadError::Busy);
        }
        let store = &self.shared.store;
        let lock =
            (self.shared.seams.lock)(store, Duration::ZERO).map_err(|_| DownloadError::Busy)?;
        let mut paused = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();
        for first in units {
            let dir = first.dir_in(store).map_err(|e| DownloadError::disk(&e))?;
            if seen.contains(&dir) {
                continue;
            }
            let pins: Vec<(&Unit, &Asset)> = units
                .iter()
                .filter(|u| u.path_in(store) == dir)
                .flat_map(|u| u.assets().iter().map(move |a| (u, a)))
                .collect();
            let assets: Vec<&Asset> = pins.iter().map(|(_, a)| *a).collect();
            let found = part::sweep(&dir, &assets).map_err(|e| DownloadError::disk(&e))?;
            paused.extend(found.into_iter().map(|(at, done)| Paused {
                unit: pins[at].0.id().to_owned(),
                name: pins[at].1.name().to_owned(),
                done,
                total: pins[at].1.bytes(),
            }));
            seen.push(dir);
        }
        drop(lock);
        drop(queue);
        Ok(paused)
    }

    /// Stops the worker: queue dropped, running unit cancelled (part kept), thread joined. Idempotent.
    ///
    /// Latency: cancel is checked before every body read and every redirect hop; one blocked transport call is not
    /// interrupted. Bound with the app transport: connect timeout (15 s) | one idle wait (`DOWNLOAD_IDLE`, 10 s).
    pub fn shutdown(&self) {
        {
            let mut queue = self.shared.queue();
            queue.stopping = true;
            queue.waiting.clear();
            if let Some(active) = &queue.active {
                active.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.shared.wake.notify_all();
        let handle = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        // a listener dropping the last handle runs on the worker itself: never join oneself
        if let Some(handle) = handle.filter(|h| h.thread().id() != std::thread::current().id()) {
            let _ = handle.join();
        }
    }

    /// Runs `op` on `unit`'s files while no one else writes them: this worker holds the store lock on another unit,
    /// or the lock is free now. `unit` running: `defer` -> stopped + parts removed by the worker; else `Busy`. Queued
    /// -> dropped first (`Failed(Cancelled)`).
    fn while_quiet(
        &self,
        unit: &Unit,
        defer: bool,
        op: impl FnOnce() -> io::Result<()>,
    ) -> Result<(), DownloadError> {
        let mut queue = self.shared.queue();
        if let Some(active) = queue.active.as_mut().filter(|a| a.id == unit.id()) {
            if !defer {
                return Err(DownloadError::Busy);
            }
            active.discard = true;
            active.cancel.store(true, Ordering::Relaxed);
            return Ok(());
        }
        let dropped = queue.drop_waiting(unit.id());
        // another unit active: this worker holds the store lock (taken + released under this mutex) and writes only
        // that unit's files
        let result = if queue.active.is_some() {
            op().map_err(|e| DownloadError::disk(&e))
        } else {
            match (self.shared.seams.lock)(&self.shared.store, Duration::ZERO) {
                Ok(lock) => {
                    let done = op().map_err(|e| DownloadError::disk(&e));
                    drop(lock);
                    done
                }
                Err(_) => Err(DownloadError::Busy),
            }
        };
        drop(queue);
        if dropped {
            (self.shared.listener)(unit.id(), &Update::Failed(DownloadError::Cancelled));
        }
        result
    }
}

impl Drop for Downloads {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Queue {
    /// Drops queued unit `id`; whether it was queued.
    fn drop_waiting(&mut self, id: &str) -> bool {
        let before = self.waiting.len();
        self.waiting.retain(|u| u.id() != id);
        self.waiting.len() != before
    }
}

impl Shared {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Worker loop: next unit (with the store lock) -> run -> release -> report. Ends on shutdown.
    fn work(&self) {
        while let Some((unit, cancel, locked)) = self.next() {
            let result = if locked {
                self.run(&unit, &cancel)
            } else {
                Err(DownloadError::Disk {
                    detail: NO_LOCK.to_owned(),
                })
            };
            self.finish(&unit);
            let update = match result {
                Ok(()) => Update::Done,
                Err(error) => Update::Failed(error),
            };
            (self.listener)(unit.id(), &update);
        }
    }

    /// Waits for queued work + the store lock (another instance downloading: tried again each [`LOCK_POLL`] | wake),
    /// then makes the first unit active with the lock, both under the queue mutex. `(unit, cancel, locked)`; `None`:
    /// shutting down.
    fn next(&self) -> Option<(Unit, Arc<AtomicBool>, bool)> {
        let mut queue = self.queue();
        loop {
            while queue.waiting.is_empty() && !queue.stopping {
                queue = self
                    .wake
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            if queue.stopping {
                return None;
            }
            if let Ok(lock) = (self.seams.lock)(&self.store, Duration::ZERO) {
                let Some(unit) = queue.waiting.pop_front() else {
                    continue;
                };
                let cancel = Arc::new(AtomicBool::new(false));
                let locked = lock.is_some();
                queue.active = Some(Active {
                    id: unit.id().to_owned(),
                    cancel: Arc::clone(&cancel),
                    discard: false,
                    _lock: lock,
                });
                return Some((unit, cancel, locked));
            }
            queue = self
                .wake
                .wait_timeout(queue, LOCK_POLL)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Unit stopped: parts removed when `delete_part` asked, then active cleared + store lock released, under the
    /// queue mutex.
    fn finish(&self, unit: &Unit) {
        let mut queue = self.queue();
        if queue.active.as_ref().is_some_and(|a| a.discard) {
            if let Ok(dir) = unit.dir_in(&self.store) {
                let _ = part::discard_all(&dir, unit.assets());
            }
        }
        queue.active = None;
    }

    /// One unit: installed -> done; free space; every file fetched + verified; manifest LAST.
    fn run(&self, unit: &Unit, cancel: &AtomicBool) -> Result<(), DownloadError> {
        if unit::is_installed(&self.store, unit) {
            return Ok(());
        }
        let dir = unit
            .dir_in(&self.store)
            .map_err(|e| DownloadError::disk(&e))?;
        let remaining: u64 = unit.assets().iter().map(|a| part::remaining(&dir, a)).sum();
        if remaining > 0 {
            let needed = remaining.saturating_add(MARGIN.max(remaining / 100));
            let free = (self.seams.free_space)(&dir).map_err(|e| DownloadError::disk(&e))?;
            if free < needed {
                return Err(DownloadError::NoSpace { needed, free });
            }
        }
        std::fs::create_dir_all(&dir).map_err(|e| DownloadError::disk(&e))?;
        let emit = |progress: Progress| (self.listener)(unit.id(), &Update::Progress(progress));
        let total = unit.assets().iter().map(Asset::bytes).sum();
        let mut meter = part::Meter::new(total, &emit);
        for asset in unit.assets() {
            // read ONLY for a source that takes one (no keychain prompt otherwise); store unreachable: asked without
            let secret = self
                .secrets
                .as_ref()
                .filter(|_| asset.source().accepts_credential())
                .and_then(|s| s.get(asset.source()).ok().flatten());
            let cx = part::Fetch {
                transport: &*self.transport,
                secret: secret.as_ref(),
                cancel,
            };
            part::fetch(&cx, asset, &dir, &mut meter)?;
            meter.next_file(asset.bytes());
        }
        unit::write_manifest(&self.store, unit).map_err(|e| DownloadError::disk(&e))
    }
}

#[cfg(test)]
mod tests;
