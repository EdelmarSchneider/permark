//! Where PerMark keeps its own state: the person's profile, or `.permark/local/` of a project that has `.permark/`.
//! Versioned JSON files; corrupt -> moved aside, defaults used, caller warns; unreadable (an IO error) -> left as it is,
//! defaults used, never written over until read again. Plugins get own namespaces.

pub mod older;
mod private;
mod raw;
pub mod shared_write;

use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{ErrorKind, IpcError};
use crate::fsx::write_atomic;
use crate::paths::Root;
use crate::plugins::PluginId;

pub use raw::RawFile;
pub use shared_write::{merge3, FileLock};

pub const PERMARK_DIR: &str = ".permark";
/// Per-project files that ALWAYS stay in the profile (a project's files must never switch them).
pub const PROFILE_ONLY: [&str; 3] = ["agent.json", "plugins.json", "pictures.json"];
/// Per-project files that live in the shared `.permark/` itself (never in `local/`).
pub const SHARED_FILES: [&str; 1] = ["meta.toml"];

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
# [plugins.word-count]
# enabled = false
";

/// Roots of the person's own state. Passed to every service (tests use temp dirs): NEVER a global.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    pub profile: PathBuf,
    pub cache: PathBuf,
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
        })
    }

    /// `base/profile` + `base/cache` (tests, portable setups).
    #[must_use]
    pub fn under(base: &Path) -> Self {
        Self {
            profile: base.join("profile"),
            cache: base.join("cache"),
        }
    }
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

/// A state file's content type: declares its format version (other version on disk -> recovered as corrupt).
pub trait StateData: Serialize + DeserializeOwned {
    const VERSION: u32;

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
    /// Corrupt | other version: moved to `backup`. `None`: an IO error kept it from being read (another program
    /// holds it, no permission), or it could not be moved: left as it is, and every save of it is refused until a
    /// load | update reads it again, so the defaults the caller uses meanwhile never replace the person's data.
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
    _data: PhantomData<fn() -> T>,
}

impl<T> Clone for StateFile<T> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            guard: self.guard.clone(),
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
        let bytes = match crate::fsx::retry(&crate::fsx::RETRY_DELAYS, || std::fs::read(&self.path))
        {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                unread::clear(&self.path);
                return Loaded::Missing;
            }
            Err(_) => {
                unread::mark(&self.path);
                return Loaded::Recovered { backup: None };
            }
        };
        unread::clear(&self.path);
        if let Some(data) = Self::parse(&bytes) {
            return Loaded::Found(data);
        }
        let backup = self.move_aside();
        if backup.is_none() {
            // still in place (newer data, or a file that cannot be moved): never written over either
            unread::mark(&self.path);
        }
        Loaded::Recovered { backup }
    }

    /// The data of the file's bytes (`None`: not this layout, or a newer version).
    fn parse(bytes: &[u8]) -> Option<T> {
        serde_json::from_slice::<EnvelopeIn<serde_json::Value>>(bytes)
            .ok()
            .and_then(|envelope| match envelope.version {
                v if v == T::VERSION => serde_json::from_value(envelope.data).ok(),
                v if v < T::VERSION => T::upgrade(v, envelope.data),
                _ => None,
            })
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
    /// on disk now (`None`: no file; a file this version cannot read is moved aside first, as [`StateFile::load`]
    /// does) and returns what to write (`None`: nothing). Written only while the file still holds what `next` saw,
    /// else read again and `next` asked again. Returns the data on disk afterwards.
    ///
    /// # Errors
    /// Encode | IO failure, a link in place of `.permark/`, a file that kept changing under a writer without the lock.
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
            let bytes = match std::fs::read(&self.path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(StateError::io(&self.path, error)),
            };
            // read (or gone): what is written next starts from what is on disk
            unread::clear(&self.path);
            let fresh = match bytes.as_deref().map(Self::parse) {
                None => None,
                Some(Some(data)) => Some(data),
                Some(None) => {
                    // kept beside it, never written over (the next round sees no file)
                    let _ = self.move_aside();
                    continue;
                }
            };
            let Some(data) = next(fresh.as_ref()) else {
                return Ok(fresh);
            };
            let out = self.encode(&data)?;
            if bytes.as_deref() == Some(out.as_slice()) {
                return Ok(Some(data));
            }
            let written =
                crate::fsx::write_atomic_checked(&self.path, &out, || {
                    match std::fs::read(&self.path) {
                        Ok(now) => Ok(bytes.as_deref() == Some(now.as_slice())),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            Ok(bytes.is_none())
                        }
                        Err(error) => Err(error),
                    }
                })
                .map_err(|e| StateError::io(&self.path, e))?;
            if written {
                return Ok(Some(data));
            }
        }
        Err(StateError::io(&self.path, raw::kept_changing()))
    }

    /// `<name>.broken-<ms>` next to it; never overwrites an older backup.
    fn move_aside(&self) -> Option<PathBuf> {
        let name = self.path.file_name()?.to_string_lossy().into_owned();
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        for n in 0..100u32 {
            let suffix = if n == 0 {
                String::new()
            } else {
                format!("-{n}")
            };
            let backup = self
                .path
                .with_file_name(format!("{name}.broken-{ms}{suffix}"));
            if std::fs::symlink_metadata(&backup).is_ok() {
                continue;
            }
            return std::fs::rename(&self.path, &backup).ok().map(|()| backup);
        }
        None
    }

    /// Atomic write of `{ version, data }`.
    ///
    /// # Errors
    /// Encode failure, IO failure, `.permark/` folder replaced by a link.
    pub fn save(&self, data: &T) -> Result<(), StateError> {
        if unread::is(&self.path) {
            return Err(StateError::io(&self.path, unread::refused()));
        }
        let bytes = self.encode(data)?;
        if let (Some(guard), Some(dir)) = (&self.guard, self.path.parent()) {
            private::prepare(&guard.permark, &guard.state, dir)
                .map_err(|e| StateError::io(&self.path, e))?;
        }
        write_atomic(&self.path, &bytes).map_err(|e| StateError::io(&self.path, e))
    }
}

/// The state files whose last load could not read them and left them in place (board item 15): a save of one would
/// write the defaults its caller got over the person's data, so it is refused until a load | update reads it again.
/// One set for the process: every handle on the same file sees it.
mod unread {
    use std::collections::HashSet;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock, PoisonError};

    fn set() -> std::sync::MutexGuard<'static, HashSet<String>> {
        static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        SET.get_or_init(Mutex::default)
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
        set().insert(key(path));
    }

    pub(super) fn clear(path: &Path) {
        set().remove(&key(path));
    }

    pub(super) fn is(path: &Path) -> bool {
        set().contains(&key(path))
    }

    pub(super) fn refused() -> std::io::Error {
        std::io::Error::other("not read yet: kept as it is until PerMark can read it")
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
            }
        }
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
            _data: PhantomData,
        }
    }

    /// File in a cache folder (rebuildable).
    #[must_use]
    pub fn cache_file<T: StateData>(&self, name: &str) -> StateFile<T> {
        StateFile {
            path: self.cache.join(name),
            guard: self.cache_guard.clone(),
            _data: PhantomData,
        }
    }

    /// Namespace of a plugin: `<state dir>/plugins/<id>/`.
    #[must_use]
    pub fn plugin_dir(&self, id: &PluginId) -> PathBuf {
        self.dir.join("plugins").join(id.as_str())
    }

    #[must_use]
    pub fn plugin_file<T: StateData>(&self, id: &PluginId, name: &str) -> StateFile<T> {
        StateFile {
            path: self.plugin_dir(id).join(name),
            guard: self.guard.clone(),
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
        copy_over(&local, &profile).map_err(fail(&local))?;
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

/// Copies files + folders of `from` into `to`, the copy winning over what `to` holds (links never followed).
fn copy_over(from: &Path, to: &Path) -> io::Result<()> {
    for entry in std::fs::read_dir(from)?.flatten() {
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_over(&entry.path(), &target)?;
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
