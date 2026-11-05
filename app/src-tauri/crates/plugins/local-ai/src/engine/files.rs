//! Engine worker files (D-6): one unit per platform (worker exe + llama/ggml libs + backend libs), each file pinned
//! url + bytes + sha256, built + published by PerMark CI (`worker-v*` releases), downloaded only when the person acts
//! (core `downloads`, own [`Downloads`] over `<plugin data>/engine/`). Pins compiled into the app (updater-signed):
//! the ONLY programs the runtime spawns.
//!
//! - [`PINS`] empty until the first `worker-v*` release -> [`FilesError::NotAvailable`] ("not available yet for this
//!   computer").
//! - Pin per (platform, variant); table order = preference (first pin of this platform wins).
//! - Unit folder `<tag>-<platform>-<variant>`: a new pin = new folder (old one left for a later sweep).
//! - Unix: exec bit set on the worker exe when it is resolved ([`WorkerFiles::exe`]).
//! - Debug builds only: `PERMARK_WORKER` = path to a local worker build (used instead of the pinned unit).
//! - Dormant: nothing created | fetched until [`WorkerFiles::enqueue`].

use std::io;
use std::path::{Path, PathBuf};

use permark_core::downloads::{Asset, DownloadKit, Downloads, Listener, Source, Unit};

/// Folder of the engine store in the plugin's data folder.
pub const DIR: &str = "engine";

/// PerMark's public release assets on GitHub; downloads redirect to `*.githubusercontent.com`.
pub const SOURCE: Source = Source::new(
    "github-releases",
    "https://github.com",
    &["github.com", "githubusercontent.com"],
);

/// Release assets path at [`SOURCE`] (`/<owner>/<repo>/releases/download`).
pub const RELEASES: &str = "/EdelmarSchneider/permark/releases/download";

/// Compute backend a worker build carries (CPU always included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Cpu,
    Vulkan,
    Metal,
    Cuda,
}

impl Variant {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Vulkan => "vulkan",
            Self::Metal => "metal",
            Self::Cuda => "cuda",
        }
    }
}

/// One pinned file of a worker unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerFile {
    /// File name in the unit folder.
    pub name: &'static str,
    /// Path at [`SOURCE`] (`RELEASES` + `/<tag>/<asset>`).
    pub path: &'static str,
    pub bytes: u64,
    pub sha256: [u8; 32],
}

/// Worker unit of one platform + variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerPin {
    /// Release tag (`worker-v<semver>`).
    pub tag: &'static str,
    /// [`platform`] spelling (`<os>-<arch>`).
    pub platform: &'static str,
    pub variant: Variant,
    /// Name of the worker program among [`WorkerPin::files`].
    pub exe: &'static str,
    pub files: &'static [WorkerFile],
}

/// Pinned worker units (filled by the first `worker-v*` release).
pub const PINS: &[WorkerPin] = &[];

/// This computer as pins spell it: `<os>-<arch>` (`windows-x86_64`, `macos-aarch64`, `linux-x86_64`).
#[must_use]
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Preferred pin of this computer (`None`: no worker built for it yet).
#[must_use]
pub fn pinned() -> Option<&'static WorkerPin> {
    let here = platform();
    PINS.iter().find(|p| p.platform == here)
}

/// Why the worker cannot be fetched | used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilesError {
    /// No worker built for this computer yet (no pin).
    #[error("the model engine is not available yet for this computer")]
    NotAvailable,
    /// A pin that does not make a unit (a build bug).
    #[error("bad engine pin")]
    BadPin,
    /// The app offers no download service.
    #[error("no download service")]
    NoDownloads,
}

/// Install unit of `pin` (id = folder = `<tag>-<platform>-<variant>`).
///
/// # Errors
/// [`FilesError::BadPin`].
pub fn unit_of(pin: &WorkerPin) -> Result<Unit, FilesError> {
    let folder = format!("{}-{}-{}", pin.tag, pin.platform, pin.variant.as_str());
    let assets = pin
        .files
        .iter()
        .map(|f| Asset::new(SOURCE, f.path, f.name, f.bytes, f.sha256))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| FilesError::BadPin)?;
    if !pin.files.iter().any(|f| f.name == pin.exe) {
        return Err(FilesError::BadPin);
    }
    Unit::new(folder.clone(), &folder, assets).map_err(|_| FilesError::BadPin)
}

/// Engine files of one plugin instance (dormant until [`WorkerFiles::enqueue`]).
pub struct WorkerFiles {
    pin: Option<&'static WorkerPin>,
    /// Engine over the engine store (`None`: no download service).
    downloads: Option<Downloads>,
    store: PathBuf,
    /// Debug builds: local worker build (`PERMARK_WORKER`).
    dev: Option<PathBuf>,
}

impl WorkerFiles {
    /// Files over `store` for this computer's pin; `listener` hears the worker unit's downloads. Nothing created.
    #[must_use]
    pub fn new(kit: Option<&DownloadKit>, store: PathBuf, listener: Listener) -> Self {
        let mut files = Self::with_pin(pinned(), kit, store, listener);
        files.dev = dev_override();
        files
    }

    /// Same over `pin` (tests), no debug override.
    #[must_use]
    pub fn with_pin(
        pin: Option<&'static WorkerPin>,
        kit: Option<&DownloadKit>,
        store: PathBuf,
        listener: Listener,
    ) -> Self {
        Self {
            pin,
            downloads: kit.map(|kit| kit.downloads(store.clone(), listener)),
            store,
            dev: None,
        }
    }

    /// Unit of this computer's pin.
    ///
    /// # Errors
    /// [`FilesError::NotAvailable`] (no pin), [`FilesError::BadPin`].
    pub fn unit(&self) -> Result<Unit, FilesError> {
        unit_of(self.pin.ok_or(FilesError::NotAvailable)?)
    }

    /// Worker usable now: debug override, else the pinned unit installed (manifest + every file of its size).
    #[must_use]
    pub fn is_installed(&self) -> bool {
        self.dev.is_some()
            || self.unit().is_ok_and(|unit| {
                self.downloads
                    .as_ref()
                    .is_some_and(|d| d.is_installed(&unit))
            })
    }

    /// Queues the worker unit (listener: progress, `Done` | `Failed`; already installed: `Done`).
    ///
    /// # Errors
    /// [`FilesError::NotAvailable`] | [`FilesError::BadPin`] | [`FilesError::NoDownloads`].
    pub fn enqueue(&self) -> Result<(), FilesError> {
        let unit = self.unit()?;
        let downloads = self.downloads.as_ref().ok_or(FilesError::NoDownloads)?;
        downloads.enqueue(unit);
        Ok(())
    }

    /// Stops the worker unit's download (part kept for resume).
    pub fn cancel(&self) {
        if let (Ok(unit), Some(downloads)) = (self.unit(), self.downloads.as_ref()) {
            downloads.cancel(unit.id());
        }
    }

    /// Path of the worker program (Unix: made executable).
    ///
    /// # Errors
    /// `NotFound`: not installed | no pin; IO errors reading the store.
    pub fn exe(&self) -> io::Result<PathBuf> {
        if let Some(dev) = &self.dev {
            return Ok(dev.clone());
        }
        let missing = || io::Error::new(io::ErrorKind::NotFound, "model engine not installed");
        let pin = self.pin.ok_or_else(missing)?;
        let unit = unit_of(pin).map_err(|_| missing())?;
        if !self.is_installed() {
            return Err(missing());
        }
        let exe = unit.dir_in(&self.store)?.join(pin.exe);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(exe)
    }

    /// Stops the download engine (plugin off; parts kept). Idempotent.
    pub fn shutdown(&self) {
        if let Some(downloads) = &self.downloads {
            downloads.shutdown();
        }
    }
}

impl std::fmt::Debug for WorkerFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerFiles")
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

#[cfg(debug_assertions)]
fn dev_override() -> Option<PathBuf> {
    std::env::var_os("PERMARK_WORKER")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

/// Release builds: never an override.
#[cfg(not(debug_assertions))]
const fn dev_override() -> Option<PathBuf> {
    None
}

/// Bytes of the files in the engine store `store` (unit folders one level down: installed files + parts + manifests;
/// links not followed).
#[must_use]
pub fn bytes(store: &Path) -> u64 {
    fn sum(dir: &Path, depth: u8) -> u64 {
        let Ok(read) = std::fs::read_dir(dir) else {
            return 0;
        };
        read.flatten()
            .filter_map(|e| {
                std::fs::symlink_metadata(e.path())
                    .ok()
                    .map(|m| (e.path(), m))
            })
            .map(|(path, meta)| {
                if meta.is_file() {
                    meta.len()
                } else if meta.is_dir() && depth > 0 {
                    sum(&path, depth - 1)
                } else {
                    0
                }
            })
            .sum()
    }
    sum(store, 1)
}

#[cfg(test)]
pub(crate) mod tests;
