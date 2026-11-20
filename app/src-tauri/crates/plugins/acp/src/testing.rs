//! Fixtures shared by the crate's tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use permark_core::conditions::{Context, Distribution};
use permark_core::document::{DocScope, Documents};
use permark_core::events::{EventSink, NoEvents};
use permark_core::fsx;
use permark_core::paths::{Root, Vault};
use permark_core::plugins::{CxWorkspace, PluginCx, PluginEvents};
use permark_core::process;
use permark_core::settings::ProjectConfig;
use permark_core::state::{AppDirs, StateStore};
use permark_core::tree::Explorer;
use permark_core::watch::OwnWrites;

use crate::agents::{platforms, Agents, Find, CHOICE_FILE};
use crate::work::Project;
use crate::ID;

/// The project of the folder `root` with its documents service.
pub(crate) fn open(root: &Path) -> (Vault, Arc<Documents>) {
    let vault = Vault::new(Root::open(root).unwrap());
    let documents = Documents::for_project(DocScope::Project(vault.clone()), Arc::new(NoEvents));
    (vault, documents)
}

/// What the work copy needs of `vault` (a fresh tree service, no projects inside).
pub(crate) fn project(vault: &Vault, documents: &Arc<Documents>) -> Project {
    let explorer = Explorer::new(
        vault,
        &ProjectConfig::load(vault.main()),
        Arc::new(NoEvents),
        None,
    );
    Project::new(
        vault.clone(),
        Arc::clone(documents),
        Arc::new(explorer),
        Arc::new(OwnWrites::new()),
    )
}

/// The plugin's event sender into `sink` (payloads without a project wrapper).
pub(crate) fn events(sink: Arc<dyn EventSink>) -> PluginEvents {
    PluginCx::new(
        ID,
        CxWorkspace::App,
        PathBuf::new(),
        None,
        Context::welcome(Distribution::Direct),
        sink,
    )
    .events()
    .clone()
}

/// The profile folders of `profile` (app scope: its choice file sits in `profile` itself).
pub(crate) fn app_store(profile: &Path) -> StateStore {
    StateStore::app(&AppDirs {
        profile: profile.to_path_buf(),
        cache: profile.join("cache"),
        install: profile.join("install"),
    })
}

/// Programs found only in the folder `bin` (as if it were the whole `PATH`).
pub(crate) fn find_in(bin: &Path) -> Find {
    let path = bin.as_os_str().to_owned();
    Arc::new(move |name| process::resolve_program_in(name, &path, None))
}

/// The agents with the choice file in `profile` and programs found only in `bin`.
pub(crate) fn agents(profile: &Path, bin: &Path) -> Agents {
    Agents::with(
        Some(app_store(profile).app_file(CHOICE_FILE)),
        find_in(bin),
        platforms(),
    )
}

/// A program file named `name` in `bin` (Windows: `name` + `.cmd` unless it has an extension; unix: executable).
pub(crate) fn program(bin: &Path, name: &str) -> PathBuf {
    let file = if cfg!(windows) && Path::new(name).extension().is_none() {
        bin.join(format!("{name}.cmd"))
    } else {
        bin.join(name)
    };
    fsx::write_atomic(&file, b"x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    file
}
