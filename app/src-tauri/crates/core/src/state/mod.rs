//! Where PerMark keeps its own state: the person's profile, or `.permark/local/` of a project that has `.permark/`.
//! Versioned JSON files; corrupt -> moved aside, defaults used, caller warns; unreadable (an IO error, a link | FIFO in
//! `.permark/`, larger than its cap, written by a newer version) -> left as it is, defaults used, never written over by
//! a handle holding those defaults. Plugins get own namespaces.

pub mod context;
pub mod older;
mod private;
mod raw;
mod seal;
pub mod shared_write;

use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{ErrorKind, IpcError};
use crate::fsx::{write_atomic, Zone};
use crate::paths::Root;
use crate::plugins::PluginId;

pub use raw::{RawFile, SHARED_MAX_BYTES};
pub use seal::{Machine, Seal};
pub use shared_write::{merge3, FileLock};

pub const PERMARK_DIR: &str = ".permark";
/// Per-project files that ALWAYS stay in the profile (a project's files must never switch them). Initialize never
/// copies them into `.permark/local/` (a copy there would never be read) and undoing it never carries one back.
pub const PROFILE_ONLY: [&str; 8] = [
    "agent.json",
    "agent-refused.json",
    "plugins.json",
    "pictures.json",
    crate::projects::fsm::MEMBERSHIP_FILE,
    crate::projects::migrate::LEDGER_FILE,
    crate::projects::card::PROFILE_CARD,
    older::TAKEN_OVER,
];
/// Per-project files that live in the shared `.permark/` itself (never in `local/`).
pub const SHARED_FILES: [&str; 1] = ["meta.toml"];
/// Most bytes a state | raw file is read with unless its kind says otherwise ([`StateData::MAX_BYTES`],
/// [`RawFile::capped`]): a larger one is left unread, never taken into memory.
pub const DEFAULT_MAX_BYTES: u64 = 8 * 1024 * 1024;

const PROJECT_GITIGNORE: &str = "# PerMark private state: not shared.\nlocal/\ncache/\n";

/// Written by "Initialize PerMark"; every key optional, commented values = defaults.
pub const DEFAULT_CONFIG_TOML: &str = "\
# PerMark project settings. Shared with your team: safe to commit.
# Everything here is optional; the commented values are the defaults.

# Extra gitignore-style patterns hidden from the live feed and from search.
# ignore = [\"drafts/\", \"*.generated.md\"]

# Which files the Explorer shows by default (each person can change it for themselves).
# [tree]
# show = \"docs-and-assets\"   # \"documents\", \"docs-and-assets\", \"everything\" or \"custom\"
# include = [\"*.sql\"]         # also show these files
# exclude = [\"archive/**\"]    # hide these files

# Agent connection: groups this project never allows (it can only narrow what each person allows).
# [agent]
# deny = [\"notes\"]

# Turn a plugin off for everyone in this project.
# [plugins.comments]
# enabled = false
";

/// Roots of the person's own state. Passed to every service (tests use temp dirs): NEVER a global.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    pub profile: PathBuf,
    pub cache: PathBuf,
    /// This computer's own folder (the install key): never in a roaming profile on Windows.
    pub install: PathBuf,
}

impl AppDirs {
    /// OS-standard folders; debug builds use `PerMark-dev` (never the person's real state).
    #[must_use]
    pub fn system() -> Option<Self> {
        let name = if cfg!(debug_assertions) {
            "PerMark-dev"
        } else {
            "PerMark"
        };
        Some(Self {
            profile: dirs::data_dir()?.join(name),
            cache: dirs::cache_dir()?.join(name),
            install: install_dir(name)?,
        })
    }

    /// `base/profile` + `base/cache` + `base/install` (tests, portable setups).
    #[must_use]
    pub fn under(base: &Path) -> Self {
        Self {
            profile: base.join("profile"),
            cache: base.join("cache"),
            install: base.join("install"),
        }
    }
}

/// `<local data>/<name>/install`; in a Snap, the common folder (the data folder changes with each revision).
fn install_dir(name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(dir) = snap_install(
        std::env::var_os("SNAP").as_deref(),
        std::env::var_os("SNAP_USER_COMMON").as_deref(),
    ) {
        return Some(dir);
    }
    Some(dirs::data_local_dir()?.join(name).join("install"))
}

/// `$SNAP_USER_COMMON/install` when running as a Snap (`SNAP` set) and the common folder is absolute.
#[cfg(any(target_os = "linux", test))]
fn snap_install(
    snap: Option<&std::ffi::OsStr>,
    common: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    let common = Path::new(common?);
    // `has_root`: an absolute path as Linux writes it, also on the computers the tests run on
    (snap.is_some_and(|s| !s.is_empty()) && common.has_root()).then(|| common.join("install"))
}

/// Stable id of a project folder: xxh3 of its `RootKey` as saved ([`crate::paths::RootKey::saved_spelling`]: shares
/// keep 0.6's `//?/unc/` spelling) -> every spelling of one folder, in every version since 0.6, -> one id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProjectId(String);

impl ProjectId {
    #[must_use]
    pub fn of(root: &Root) -> Self {
        Self::of_key(root.key())
    }

    fn of_key(key: &crate::paths::RootKey) -> Self {
        Self(crate::fsx::hash(key.saved_spelling().as_bytes()).hex())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum StateLocation {
    Profile,
    Project,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("cannot write {path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("cannot encode {path}: {source}")]
    Encode {
        path: String,
        source: serde_json::Error,
    },
}

impl StateError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

impl From<StateError> for IpcError {
    fn from(error: StateError) -> Self {
        match error {
            StateError::Io { path, source } => IpcError::io(path, &source),
            StateError::Encode { path, source } => IpcError::new(ErrorKind::Internal)
                .subject(path)
                .detail(source.to_string()),
        }
    }
}

/// A state file's content type: declares its format version (an older one on disk -> [`StateData::upgrade`], else
/// recovered as corrupt; a newer one -> left unread in place, never written over).
pub trait StateData: Serialize + DeserializeOwned {
    const VERSION: u32;
    /// Most bytes a file of this kind is read with (never below the most its writer makes): a larger file is left
    /// unread in place, never taken into memory and never written over.
    const MAX_BYTES: u64 = DEFAULT_MAX_BYTES;

    /// Reads data an older format version (`from` < `VERSION`) wrote; `None` = cannot (the file is moved aside).
    /// A format change ships its upgrade + a fixture test (concept §10: saved data survives updates).
    #[must_use]
    fn upgrade(from: u32, data: serde_json::Value) -> Option<Self> {
        let _ = (from, data);
        None
    }
}

/// Outcome of a load.
#[derive(Debug, PartialEq)]
pub enum Loaded<T> {
    Missing,
    Found(T),
    /// Corrupt | older version it cannot read: moved to `backup`. `None`: left as it is, unread: an IO error kept it
    /// from being read (another program holds it, no permission), it is no plain file of at most its cap (a link | FIFO
    /// in `.permark/`), a newer version wrote it, or it could not be moved; a save of it is refused from this
    /// handle until its own load | update reads it (another handle reading it never unblocks this one), and from
    /// every handle that never read it while it is there, so the defaults a caller uses meanwhile never replace the
    /// person's data. A handle that read the file itself keeps saving: it holds the person's data.
    /// Caller uses default + warns.
    Recovered {
        backup: Option<PathBuf>,
    },
}

impl<T: Default> Loaded<T> {
    /// Value, default when missing | recovered.
    pub fn into_value(self) -> T {
        match self {
            Self::Found(value) => value,
            Self::Missing | Self::Recovered { .. } => T::default(),
        }
    }
}

#[derive(Serialize)]
struct EnvelopeOut<'a, T> {
    version: u32,
    data: &'a T,
}

#[derive(Deserialize)]
struct EnvelopeIn<T> {
    version: u32,
    data: T,
}

/// What a state file's bytes hold.
enum Parsed<T> {
    Data(T),
    /// A newer version's data: left as it is.
    Newer,
    /// Not this layout, or an older version without an upgrade: moved aside.
    Bad,
}

/// Guard of a file inside `.permark/`: the `.permark` folder + the state folder it lives in.
#[derive(Debug, Clone)]
struct Guard {
    permark: PathBuf,
    state: PathBuf,
}

/// One typed state file.
#[derive(Debug)]
pub struct StateFile<T> {
    path: PathBuf,
    guard: Option<Guard>,
    /// What this handle's own last load | update made of the file: [`unread::NEVER`], [`unread::READ`] (read it, found
    /// none, or moved it aside) or [`unread::FAILED`] (left it unread: it holds defaults).
    read: AtomicU8,
    _data: PhantomData<fn() -> T>,
}

impl<T> Clone for StateFile<T> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            guard: self.guard.clone(),
            read: AtomicU8::new(self.read.load(Ordering::SeqCst)),
            _data: PhantomData,
        }
    }
}

impl<T: StateData> StateFile<T> {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn load(&self) -> Loaded<T> {
        // a file held for a moment (antivirus, sync client) is tried again first
        let Ok(found) = self.read_now() else {
            self.failed();
            return Loaded::Recovered { backup: None };
        };
        let Some(bytes) = found else {
            self.has_read();
            return Loaded::Missing;
        };
        match Self::parse(&bytes) {
            Parsed::Data(data) => {
                self.has_read();
                return Loaded::Found(data);
            }
            Parsed::Newer => {
                self.newer();
                return Loaded::Recovered { backup: None };
            }
            Parsed::Bad => {}
        }
        let backup = self.move_aside();
        if backup.is_some() {
            // nothing left to write over: the defaults may be saved
            self.has_read();
        } else {
            // still in place (a file that cannot be moved): never written over either
            self.failed();
        }
        Loaded::Recovered { backup }
    }

    /// The bytes on disk now (`None`: no file): a plain file of at most [`StateData::MAX_BYTES`], never through a
    /// link in `.permark/` (a profile file follows links: the person's own setup).
    fn read_now(&self) -> io::Result<Option<Vec<u8>>> {
        let zone = self
            .guard
            .as_ref()
            .map_or(Zone::Anywhere, |guard| Zone::Inside(&guard.permark));
        raw::read_plain(&self.path, T::MAX_BYTES, zone)
    }

    /// A newer version wrote the file: left in place for it, never written over by this one.
    fn newer(&self) {
        unread::tell(&self.path, &NEWER);
        self.failed();
    }

    /// This handle read the file (or found none, or moved it aside): it holds what is on disk and may save.
    fn has_read(&self) {
        self.read.store(unread::READ, Ordering::SeqCst);
    }

    /// This handle left the file unread (its caller holds defaults): its saves are refused, and those of handles that
    /// never read the file while it is there.
    fn failed(&self) {
        unread::mark(&self.path);
        self.read.store(unread::FAILED, Ordering::SeqCst);
    }

    /// What the file's bytes hold.
    fn parse(bytes: &[u8]) -> Parsed<T> {
        let Ok(envelope) = serde_json::from_slice::<EnvelopeIn<serde_json::Value>>(bytes) else {
            return Parsed::Bad;
        };
        let data = match envelope.version {
            v if v == T::VERSION => serde_json::from_value(envelope.data).ok(),
            v if v < T::VERSION => T::upgrade(v, envelope.data),
            _ => return Parsed::Newer,
        };
        data.map_or(Parsed::Bad, Parsed::Data)
    }

    fn encode(&self, data: &T) -> Result<Vec<u8>, StateError> {
        serde_json::to_vec_pretty(&EnvelopeOut {
            version: T::VERSION,
            data,
        })
        .map_err(|source| StateError::Encode {
            path: self.path.display().to_string(),
            source,
        })
    }

    /// Read-change-write for a file several cores write (`shared_write`): under the file's lock, `next` gets the data
    /// on disk now (`None`: no file; a file this version cannot parse is moved aside first, as [`StateFile::load`]
    /// does; one it must not read, or a newer version's, fails the update untouched) and returns what to write
    /// (`None`: nothing). Written only while the file still holds what `next` saw, else read again and `next` asked
    /// again. Returns the data on disk afterwards. After an `Err`, never save defaults in place of what is on disk: the
    /// file may hold the person's data (a file it could not read is stamped, so a handle that never read it is refused
    /// while the file is there; once it is gone, nothing is left to lose).
    ///
    /// # Errors
    /// Encode | IO failure, a link in place of `.permark/`, a file that kept changing under a writer without the lock,
    /// a file it must not read (link | FIFO in `.permark/`, larger than its cap), a newer version's file.
    pub fn update(
        &self,
        mut next: impl FnMut(Option<&T>) -> Option<T>,
    ) -> Result<Option<T>, StateError> {
        if let (Some(guard), Some(dir)) = (&self.guard, self.path.parent()) {
            private::prepare(&guard.permark, &guard.state, dir)
                .map_err(|e| StateError::io(&self.path, e))?;
        }
        let _lock =
            shared_write::lock_for(&self.path).map_err(|e| StateError::io(&self.path, e))?;
        for _ in 0..raw::UPDATE_ATTEMPTS {
            let bytes = match self.read_now() {
                Ok(bytes) => bytes,
                Err(error) => {
                    // not read: no handle that never read it writes over it; this handle's own state stays (a
                    // handle holding what it read keeps writing)
                    unread::mark(&self.path);
                    return Err(StateError::io(&self.path, error));
                }
            };
            let fresh = match bytes.as_deref().map(Self::parse) {
                None => None,
                Some(Parsed::Data(data)) => Some(data),
                Some(Parsed::Newer) => {
                    self.newer();
                    return Err(StateError::io(&self.path, unread::refused()));
                }
                Some(Parsed::Bad) => {
                    // kept beside it, never written over (the next round sees no file)
                    if self.move_aside().is_some() {
                        continue;
                    }
                    // cannot be moved: left as it is, and no handle writes over it (review S1)
                    self.failed();
                    return Err(StateError::io(&self.path, unread::refused()));
                }
            };
            // read (or gone): what this handle writes next starts from what is on disk
            self.has_read();
            let Some(data) = next(fresh.as_ref()) else {
                return Ok(fresh);
            };
            let out = self.encode(&data)?;
            if bytes.as_deref() == Some(out.as_slice()) {
                return Ok(Some(data));
            }
            let written =
                crate::fsx::write_atomic_checked(
                    &self.path,
                    &out,
                    || Ok(self.read_now()? == bytes),
                )
                .map_err(|e| StateError::io(&self.path, e))?;
            if written {
                return Ok(Some(data));
            }
        }
        Err(StateError::io(&self.path, raw::kept_changing()))
    }

    fn move_aside(&self) -> Option<PathBuf> {
        move_aside(&self.path)
    }

    /// For a handle that never read the file: whatever is there may be replaced (none, one this version reads, or a
    /// corrupt one, as a load would move it aside). Not when no load could read it (a link | FIFO in `.permark/`,
    /// larger than its cap) or a newer version wrote it: stamped then, as a load would leave it. Any other read error
    /// says nothing about the file: the write decides, as before.
    fn replaceable(&self) -> bool {
        match self.read_now() {
            Ok(Some(bytes)) if matches!(Self::parse(&bytes), Parsed::Newer) => {
                unread::tell(&self.path, &NEWER);
                unread::mark(&self.path);
                false
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                unread::mark(&self.path);
                false
            }
            Ok(_) | Err(_) => true,
        }
    }

    /// Atomic write of `{ version, data }`; saves of one file in this process take turns (`shared_write::save_turn`).
    ///
    /// # Errors
    /// Encode failure, IO failure, `.permark/` folder replaced by a link, the saves before this one taking too long.
    pub fn save(&self, data: &T) -> Result<(), StateError> {
        let read = self.read.load(Ordering::SeqCst);
        if !unread::may_write(&self.path, read) {
            return Err(StateError::io(&self.path, unread::refused()));
        }
        let bytes = self.encode(data)?;
        if let (Some(guard), Some(dir)) = (&self.guard, self.path.parent()) {
            private::prepare(&guard.permark, &guard.state, dir)
                .map_err(|e| StateError::io(&self.path, e))?;
        }
        let _turn =
            shared_write::save_turn(&self.path).map_err(|e| StateError::io(&self.path, e))?;
        let stamped = unread::is_stamped(&self.path);
        if read == unread::NEVER && !stamped && !self.replaceable() {
            return Err(StateError::io(&self.path, unread::refused()));
        }
        if !(read == unread::NEVER && stamped) {
            return write_atomic(&self.path, &bytes).map_err(|e| StateError::io(&self.path, e));
        }
        // a file that could not be read once, gone at the check above: still gone right before each rename attempt
        // (one put back meanwhile is kept, as `update` does); written, it holds only PerMark's own data
        let written = crate::fsx::write_atomic_checked(&self.path, &bytes, || {
            Ok(unread::may_write(&self.path, read))
        })
        .map_err(|e| StateError::io(&self.path, e))?;
        if !written {
            return Err(StateError::io(&self.path, unread::refused()));
        }
        unread::clear(&self.path);
        Ok(())
    }
}

/// Why a newer version's state file is left unread.
const NEWER: &str = "written by a newer version of PerMark";

/// `<name>.broken-<ms>` next to `path` (the entry itself: a link is moved, never followed); never overwrites an older
/// backup.
fn move_aside(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    for n in 0..100u32 {
        let suffix = if n == 0 {
            String::new()
        } else {
            format!("-{n}")
        };
        let backup = path.with_file_name(format!("{name}.broken-{ms}{suffix}"));
        if std::fs::symlink_metadata(&backup).is_ok() {
            continue;
        }
        return std::fs::rename(path, &backup).ok().map(|()| backup);
    }
    None
}

/// The state files a load | update could not read (or parse and move aside) and left in place (board item 15): a save
/// that writes the defaults a caller got over the person's data is refused. Refused: a handle whose own last load |
/// update left the file unread (`FAILED`), and a handle that never read a file stamped here (`NEVER`: it may carry
/// the defaults of another handle, as the session save makes a new handle). A handle that read the file itself
/// (`READ`) writes even after another handle failed: it holds the person's data. A file that is gone holds nothing to
/// lose: a `NEVER` handle may write a stamped file while it is not there (only a not-found answer counts as gone; any
/// other error, and any entry, a folder or a link included, counts as there), and that write clears the stamp: the file
/// then holds only PerMark's own data. A `FAILED` handle stays refused. One set for the process (other processes do not
/// see it); otherwise a stamp stays until the process ends.
mod unread {
    use std::collections::HashSet;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock, PoisonError};

    pub(super) const NEVER: u8 = 0;
    pub(super) const READ: u8 = 1;
    pub(super) const FAILED: u8 = 2;

    fn stamped() -> std::sync::MutexGuard<'static, HashSet<String>> {
        static STAMPED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        STAMPED
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn key(path: &Path) -> String {
        let key = crate::paths::simplified(path)
            .to_string_lossy()
            .into_owned();
        if crate::paths::CASE_INSENSITIVE {
            key.to_lowercase()
        } else {
            key
        }
    }

    pub(super) fn mark(path: &Path) {
        stamped().insert(key(path));
    }

    pub(super) fn is_stamped(path: &Path) -> bool {
        stamped().contains(&key(path))
    }

    /// After a `NEVER` handle wrote the file while it was gone: what is there now is PerMark's own data.
    pub(super) fn clear(path: &Path) {
        stamped().remove(&key(path));
    }

    /// A handle whose own last load | update made `read` of the file may write it. One that never read it may, unless
    /// the file could not be read once and is still there: a file that is gone holds nothing to lose (the review keeps
    /// a copy again after removing it as unused).
    pub(super) fn may_write(path: &Path, read: u8) -> bool {
        match read {
            READ => true,
            NEVER => {
                !is_stamped(path)
                    || matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
            }
            _ => false,
        }
    }

    pub(super) fn refused() -> std::io::Error {
        std::io::Error::other("not read yet: kept as it is until PerMark can read it")
    }

    /// Logs why a file is left unread, once per path for the process: the person's data stays as it is, so the reason
    /// is worth one line, not one per load.
    pub(super) fn tell(path: &Path, why: &dyn std::fmt::Display) {
        static TOLD: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        let first = TOLD
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key(path));
        if first {
            eprintln!("PerMark: {} left as it is: {why}", path.display());
        }
    }
}

/// State of one project (or of the app): where its files go.
#[derive(Debug, Clone)]
pub struct StateStore {
    location: StateLocation,
    dir: PathBuf,
    cache: PathBuf,
    /// `<profile>/projects/<id>/`: profile-only files of a project (agent switch, plugin choices).
    profile_dir: PathBuf,
    /// `<profile>/`: app-wide files (defaults a project starts from).
    app_dir: PathBuf,
    /// `.permark/` when the project has it (shared files), else the state folder.
    shared: PathBuf,
    guard: Option<Guard>,
    cache_guard: Option<Guard>,
    /// The person's state roots this store was made from.
    roots: AppDirs,
    /// The project folder; `None`: the app-wide store.
    root: Option<Root>,
    /// This install's data for the project, worked out at first use; clones share it.
    machine: Arc<OnceLock<Option<Machine>>>,
}

impl StateStore {
    /// App-wide state: the profile folder itself.
    #[must_use]
    pub fn app(dirs: &AppDirs) -> Self {
        Self {
            location: StateLocation::Profile,
            dir: dirs.profile.clone(),
            cache: dirs.cache.clone(),
            profile_dir: dirs.profile.clone(),
            app_dir: dirs.profile.clone(),
            shared: dirs.profile.clone(),
            guard: None,
            cache_guard: None,
            roots: dirs.clone(),
            root: None,
            machine: Arc::default(),
        }
    }

    /// `.permark/local/` when `.permark/` is a real folder (and `local/` real or missing), else the profile.
    #[must_use]
    pub fn for_project(dirs: &AppDirs, root: &Root) -> Self {
        let id = ProjectId::of(root);
        let profile_dir = dirs.profile.join("projects").join(id.as_str());
        let profile_cache = dirs.cache.join("projects").join(id.as_str());
        let permark = root.path().join(PERMARK_DIR);
        let local = permark.join("local");
        let cache = permark.join("cache");
        let usable = private::is_real_folder(&permark)
            && !matches!(private::entry(&local), private::Entry::Other);
        if usable {
            let cache_usable = !matches!(private::entry(&cache), private::Entry::Other);
            let guard = |state: &Path| Guard {
                permark: permark.clone(),
                state: state.to_path_buf(),
            };
            Self {
                location: StateLocation::Project,
                dir: local.clone(),
                cache: if cache_usable {
                    cache.clone()
                } else {
                    profile_cache
                },
                profile_dir,
                app_dir: dirs.profile.clone(),
                shared: permark.clone(),
                guard: Some(guard(&local)),
                cache_guard: cache_usable.then(|| guard(&cache)),
                roots: dirs.clone(),
                root: Some(root.clone()),
                machine: Arc::default(),
            }
        } else {
            Self {
                location: StateLocation::Profile,
                dir: profile_dir.clone(),
                cache: profile_cache,
                shared: profile_dir.clone(),
                profile_dir,
                app_dir: dirs.profile.clone(),
                guard: None,
                cache_guard: None,
                roots: dirs.clone(),
                root: Some(root.clone()),
                machine: Arc::default(),
            }
        }
    }

    /// The person's state roots (profile, cache, install) this store was made from.
    #[must_use]
    pub fn app_dirs(&self) -> &AppDirs {
        &self.roots
    }

    /// This install's own data for the project: read only by this install, sealed with its key and bound to the
    /// folder, in `<state dir>/machine/<iid>/`. `None`: the app-wide store, or no usable install key (keep no such
    /// data: fail closed).
    #[must_use]
    pub fn machine(&self) -> Option<&Machine> {
        self.machine
            .get_or_init(|| {
                let root = self.root.as_ref()?;
                let install = seal::install(&self.roots.install)?;
                Some(Machine::new(&install, root, &self.dir))
            })
            .as_ref()
    }

    /// A typed file in [`StateStore::machine`]'s folder, guarded like the other state files.
    #[must_use]
    pub fn machine_file<T: StateData>(&self, name: &str) -> Option<StateFile<T>> {
        self.machine().map(|machine| StateFile {
            path: machine.dir().join(name),
            guard: self.guard.clone(),
            read: AtomicU8::new(unread::NEVER),
            _data: PhantomData,
        })
    }

    /// A raw file in [`StateStore::machine`]'s folder.
    #[must_use]
    pub fn machine_raw_file(&self, name: &str) -> Option<RawFile> {
        self.machine()
            .map(|machine| RawFile::private(machine.dir().join(name), self.guard.clone()))
    }

    /// A typed file in the install folder (this computer's, for every project); `None`: no usable install key.
    #[must_use]
    pub fn install_file<T: StateData>(&self, name: &str) -> Option<StateFile<T>> {
        seal::install(&self.roots.install).map(|_| StateFile {
            path: self.roots.install.join(name),
            guard: None,
            read: AtomicU8::new(unread::NEVER),
            _data: PhantomData,
        })
    }

    #[must_use]
    pub fn location(&self) -> StateLocation {
        self.location
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    #[must_use]
    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }

    #[must_use]
    pub fn file<T: StateData>(&self, name: &str) -> StateFile<T> {
        StateFile {
            path: self.dir.join(name),
            guard: self.guard.clone(),
            read: AtomicU8::new(unread::NEVER),
            _data: PhantomData,
        }
    }

    /// A raw file (the person's data in a layout of an earlier version, no envelope) in the state folder.
    #[must_use]
    pub fn raw_file(&self, name: &str) -> RawFile {
        RawFile::private(self.dir.join(name), self.guard.clone())
    }

    /// A raw file shared with the team: `.permark/<name>` when the project has `.permark/`, else the profile.
    #[must_use]
    pub fn shared_file(&self, name: &str) -> RawFile {
        match (&self.location, &self.guard) {
            (StateLocation::Project, Some(guard)) => {
                RawFile::shared(self.shared.join(name), guard.permark.clone())
            }
            _ => RawFile::private(self.shared.join(name), None),
        }
    }

    /// The profile's copy of a shared file (what the project kept there before it had `.permark/`); `None` while
    /// the project has no `.permark/` (then the profile copy IS the file).
    #[must_use]
    pub fn legacy_shared_file(&self, name: &str) -> Option<RawFile> {
        (self.location == StateLocation::Project)
            .then(|| RawFile::private(self.profile_dir.join(name), None))
    }

    /// File that ALWAYS lives in the profile (`PROFILE_ONLY`), whatever the project holds.
    #[must_use]
    pub fn profile_file<T: StateData>(&self, name: &str) -> StateFile<T> {
        StateFile {
            path: self.profile_dir.join(name),
            guard: None,
            read: AtomicU8::new(unread::NEVER),
            _data: PhantomData,
        }
    }

    /// The profile folder itself (app-wide state).
    #[must_use]
    pub fn app_dir(&self) -> &Path {
        &self.app_dir
    }

    /// App-wide file in the profile folder itself (defaults every project starts from).
    #[must_use]
    pub fn app_file<T: StateData>(&self, name: &str) -> StateFile<T> {
        StateFile {
            path: self.app_dir.join(name),
            guard: None,
            read: AtomicU8::new(unread::NEVER),
            _data: PhantomData,
        }
    }

    /// File in a cache folder (rebuildable).
    #[must_use]
    pub fn cache_file<T: StateData>(&self, name: &str) -> StateFile<T> {
        StateFile {
            path: self.cache.join(name),
            guard: self.cache_guard.clone(),
            read: AtomicU8::new(unread::NEVER),
            _data: PhantomData,
        }
    }

    /// Namespace of a plugin: `<state dir>/plugins/<id>/`.
    #[must_use]
    pub fn plugin_dir(&self, id: &PluginId) -> PathBuf {
        self.dir.join("plugins").join(id.as_str())
    }

    /// A plugin's folder in the profile for this project (`<profile>/projects/<id>/plugins/<plugin>/`), whatever the
    /// project holds: for data that must never sit inside the project folder (a tool walking up from it would find
    /// the project). "Remove data" sweeps it with the rest of the plugin's data.
    #[must_use]
    pub fn profile_plugin_dir(&self, id: &PluginId) -> PathBuf {
        self.profile_dir.join("plugins").join(id.as_str())
    }

    /// A typed file in [`StateStore::profile_plugin_dir`]: ALWAYS in the profile, whatever the project holds.
    #[must_use]
    pub fn profile_plugin_file<T: StateData>(&self, id: &PluginId, name: &str) -> StateFile<T> {
        StateFile {
            path: self.profile_plugin_dir(id).join(name),
            guard: None,
            read: AtomicU8::new(unread::NEVER),
            _data: PhantomData,
        }
    }

    #[must_use]
    pub fn plugin_file<T: StateData>(&self, id: &PluginId, name: &str) -> StateFile<T> {
        StateFile {
            path: self.plugin_dir(id).join(name),
            guard: self.guard.clone(),
            read: AtomicU8::new(unread::NEVER),
            _data: PhantomData,
        }
    }

    /// A raw file in a plugin's namespace (the one-time copy of 0.6 state writes the plugins' files as they are).
    #[must_use]
    pub fn plugin_raw_file(&self, id: &PluginId, name: &str) -> RawFile {
        RawFile::private(self.plugin_dir(id).join(name), self.guard.clone())
    }

    /// Deletes a plugin's namespace (state + cache). Missing = done. A link in its place is removed as a link
    /// (never followed).
    ///
    /// # Errors
    /// IO failure.
    pub fn remove_plugin_data(&self, id: &PluginId) -> Result<(), StateError> {
        for dir in [
            self.plugin_dir(id),
            self.cache.join("plugins").join(id.as_str()),
        ] {
            remove_tree(&dir).map_err(|e| StateError::io(&dir, e))?;
        }
        Ok(())
    }
}

/// Removes a folder tree; a link is removed itself, never followed.
fn remove_tree(path: &Path) -> io::Result<()> {
    match private::entry(path) {
        private::Entry::Missing => Ok(()),
        private::Entry::Folder => std::fs::remove_dir_all(path),
        private::Entry::Other => std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path)),
    }
}

/// Deletes a plugin's data for the app + every project the profile knows + the given open roots.
///
/// # Errors
/// First IO failure (others still attempted).
pub fn remove_plugin_data_everywhere(
    dirs: &AppDirs,
    id: &PluginId,
    open_roots: &[Root],
) -> Result<(), StateError> {
    let mut first: Option<StateError> = None;
    let mut keep = |result: Result<(), StateError>| {
        if let Err(error) = result {
            first.get_or_insert(error);
        }
    };
    keep(StateStore::app(dirs).remove_plugin_data(id));
    for base in [&dirs.profile, &dirs.cache] {
        if let Ok(entries) = std::fs::read_dir(base.join("projects")) {
            for project in entries.flatten() {
                let dir = project.path().join("plugins").join(id.as_str());
                keep(remove_tree(&dir).map_err(|e| StateError::io(&dir, e)));
            }
        }
    }
    for root in open_roots {
        keep(StateStore::for_project(dirs, root).remove_plugin_data(id));
    }
    first.map_or(Ok(()), Err)
}

/// Creates `.permark/` in `root` (config, ignore, `local/`, `cache/`); existing files kept; copies profile state in
/// (never over an existing entry; profile-only files stay). Never auto-called.
///
/// # Errors
/// `.permark` | `local` | `cache` is a link or a file; IO failure.
pub fn init_project(dirs: &AppDirs, root: &Root) -> Result<StateStore, StateError> {
    let permark = root.path().join(PERMARK_DIR);
    let before = StateStore::for_project(dirs, root);
    let fail = |p: &Path| {
        let p = p.to_path_buf();
        move |e| StateError::io(&p, e)
    };
    match private::entry(&permark) {
        private::Entry::Missing => std::fs::create_dir(&permark).map_err(fail(&permark))?,
        private::Entry::Folder => {}
        private::Entry::Other => {
            return Err(StateError::io(
                &permark,
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "a link or a file is in the way",
                ),
            ))
        }
    }
    for sub in ["local", "cache"] {
        let dir = permark.join(sub);
        private::prepare(&permark, &dir, &dir).map_err(fail(&dir))?;
    }
    for (name, content) in [
        ("config.toml", DEFAULT_CONFIG_TOML),
        (".gitignore", PROJECT_GITIGNORE),
    ] {
        let path = permark.join(name);
        private::create_if_missing(&path, content).map_err(fail(&path))?;
    }
    let after = StateStore::for_project(dirs, root);
    if before.location == StateLocation::Profile && after.location == StateLocation::Project {
        copy_missing(&before.dir, &after.dir, true).map_err(fail(&before.dir))?;
    }
    Ok(after)
}

/// Takes back what [`init_project`] made, when it is untouched (`projects.md` §2.4, undo of a first include):
/// `.permark/` holds only `config.toml` + `.gitignore` with the bytes init wrote, `local/`, `cache/`, no link anywhere.
/// The project's private state (`local/`, the live state since init) is carried back into the profile (newer wins),
/// then `.permark/` is removed in one step: renamed away first (a temp name the tree and the watch never show), then
/// deleted; a file held open (antivirus, editor) makes the rename fail and leaves `.permark/` whole, never half
/// removed. The caller stopped the plugins before (their files flushed) and reloads the state after.
/// `Ok(false)`: changed since init, left as it is.
///
/// # Errors
/// IO failure while carrying back | renaming (`.permark/` then stays whole).
pub fn uninit_project(dirs: &AppDirs, root: &Root) -> Result<bool, StateError> {
    let permark = root.path().join(PERMARK_DIR);
    if !private::is_real_folder(&permark) {
        return Ok(false);
    }
    let fail = |p: &Path| {
        let p = p.to_path_buf();
        move |e| StateError::io(&p, e)
    };
    let names: Vec<String> = std::fs::read_dir(&permark)
        .map_err(fail(&permark))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let allowed = ["config.toml", ".gitignore", "local", "cache"];
    if names.iter().any(|n| !allowed.contains(&n.as_str())) {
        return Ok(false);
    }
    let same = |name: &str, want: &str| {
        std::fs::read(permark.join(name)).is_ok_and(|bytes| bytes == want.as_bytes())
    };
    if !same("config.toml", DEFAULT_CONFIG_TOML) || !same(".gitignore", PROJECT_GITIGNORE) {
        return Ok(false);
    }
    for sub in ["local", "cache"] {
        if holds_link(&permark.join(sub)) {
            return Ok(false);
        }
    }
    let profile = dirs
        .profile
        .join("projects")
        .join(ProjectId::of(root).as_str());
    let local = permark.join("local");
    if private::is_real_folder(&local) {
        std::fs::create_dir_all(&profile).map_err(fail(&profile))?;
        copy_over(&local, &profile, true).map_err(fail(&local))?;
    }
    // one step for everyone looking: `.permark/` is there whole or gone; the leftover temp folder is best effort
    let gone = root.path().join(format!(
        ".permark-undo-{}-{:x}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::rename(&permark, &gone).map_err(fail(&permark))?;
    let _ = std::fs::remove_dir_all(&gone);
    Ok(true)
}

/// A project folder moved from `from` to `to` (absolute; `from` may be gone): its profile state folder and its profile
/// cache folder follow the new `ProjectId` (`projects.md` §9.1). Target missing -> renamed; target present -> what it
/// lacks is copied in (the target wins), then the old folder is removed. Same id (case-only rename on a
/// case-insensitive disk) | nothing kept under the old id -> nothing done. Returns whether anything moved.
///
/// # Errors
/// IO failure (the old folder then stays where it is).
pub fn move_profile_state(dirs: &AppDirs, from: &Path, to: &Path) -> Result<bool, StateError> {
    let id = |path: &Path| ProjectId::of_key(&crate::paths::RootKey::of(path)).0;
    let (old_id, new_id) = (id(from), id(to));
    if old_id == new_id {
        return Ok(false);
    }
    let mut moved = false;
    for base in [dirs.profile.join("projects"), dirs.cache.join("projects")] {
        let old = base.join(&old_id);
        if !private::is_real_folder(&old) {
            continue;
        }
        let new = base.join(&new_id);
        if std::fs::symlink_metadata(&new).is_err() {
            std::fs::rename(&old, &new).map_err(|e| StateError::io(&old, e))?;
        } else {
            copy_missing(&old, &new, false).map_err(|e| StateError::io(&new, e))?;
            std::fs::remove_dir_all(&old).map_err(|e| StateError::io(&old, e))?;
        }
        moved = true;
    }
    Ok(moved)
}

/// Whether a folder tree holds a link (symlink | junction) anywhere (the folder itself included).
fn holds_link(dir: &Path) -> bool {
    match private::entry(dir) {
        private::Entry::Missing => false,
        private::Entry::Other => true,
        private::Entry::Folder => std::fs::read_dir(dir).map_or(true, |entries| {
            entries.flatten().any(|e| {
                let path = e.path();
                crate::tree::is_link_meta_of(&path)
                    || e.file_type()
                        .map_or(true, |t| t.is_dir() && holds_link(&path))
            })
        }),
    }
}

/// Copies files + folders of `from` into `to`, the copy winning over what `to` holds (links never followed). `top`:
/// the profile-only and shared files of a project folder stay out (a project's files must never switch them).
fn copy_over(from: &Path, to: &Path, top: bool) -> io::Result<()> {
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        if top && PROFILE_ONLY.iter().chain(&SHARED_FILES).any(|p| name == *p) {
            continue;
        }
        let target = to.join(&name);
        let kind = entry.file_type()?;
        if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_over(&entry.path(), &target, false)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Copies files + folders of `from` into `to`, never over an existing entry (links not followed or copied).
fn copy_missing(from: &Path, to: &Path, top: bool) -> io::Result<()> {
    let Ok(entries) = std::fs::read_dir(from) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if top && PROFILE_ONLY.iter().chain(&SHARED_FILES).any(|p| name == *p) {
            continue;
        }
        let target = to.join(&name);
        if std::fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            std::fs::create_dir(&target)?;
            copy_missing(&entry.path(), &target, false)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
