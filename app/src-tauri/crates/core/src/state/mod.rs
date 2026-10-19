//! Where PerMark keeps its own state: the person's profile, or `.permark/local/` of a project that has `.permark/`.
//! Versioned JSON files; corrupt -> moved aside, defaults used, caller warns. Plugins get own namespaces.

mod private;
mod raw;

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

pub const PERMARK_DIR: &str = ".permark";
/// Per-project files that ALWAYS stay in the profile (a project's files must never switch them).
pub const PROFILE_ONLY: [&str; 2] = ["agent.json", "plugins.json"];
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

/// Stable id of a project folder: xxh3 of its `RootKey` (every spelling of one folder -> one id).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProjectId(String);

impl ProjectId {
    #[must_use]
    pub fn of(root: &Root) -> Self {
        Self(crate::fsx::hash(root.key().as_str().as_bytes()).hex())
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
}

/// Outcome of a load.
#[derive(Debug, PartialEq)]
pub enum Loaded<T> {
    Missing,
    Found(T),
    /// Unreadable | other version: moved to `backup` (None: could not be moved); caller uses default + warns.
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

impl<T: StateData> StateFile<T> {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn load(&self) -> Loaded<T> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Loaded::Missing,
            Err(_) => return Loaded::Recovered { backup: None },
        };
        match serde_json::from_slice::<EnvelopeIn<T>>(&bytes) {
            Ok(envelope) if envelope.version == T::VERSION => Loaded::Found(envelope.data),
            Ok(_) | Err(_) => Loaded::Recovered {
                backup: self.move_aside(),
            },
        }
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
        let bytes = serde_json::to_vec_pretty(&EnvelopeOut {
            version: T::VERSION,
            data,
        })
        .map_err(|source| StateError::Encode {
            path: self.path.display().to_string(),
            source,
        })?;
        if let (Some(guard), Some(dir)) = (&self.guard, self.path.parent()) {
            private::prepare(&guard.permark, &guard.state, dir)
                .map_err(|e| StateError::io(&self.path, e))?;
        }
        write_atomic(&self.path, &bytes).map_err(|e| StateError::io(&self.path, e))
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
