//! Plugin `shell-integration`: "Open with PerMark" on folders, on the background of a folder and on Markdown files, and
//! "New Markdown file" on folders, in the Windows file manager. Everything lives under `HKEY_CURRENT_USER` (no admin);
//! the installer writes the same entries and both remember the person's choice under `HKCU\Software\PerMark`, so an
//! update keeps it. Windows + direct builds only: a Store package declares its entries in its manifest.
//!
//! Switched off, the plugin removes every entry it made ([`withdraw`]); the app quitting leaves them in place.

pub mod entries;
#[cfg(windows)]
mod notify;
#[cfg(windows)]
pub mod registry;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::{Distribution, Need, Platform, Requirement};
use permark_core::plugins::{
    Manifest, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope, Running,
};
use serde::Serialize;
use ts_rs::TS;

pub use entries::{Entry, Language};

const ID: PluginId = PluginId::of("shell-integration");

const NEEDS: &[Need] = &[
    Need::Platform {
        platforms: Cow::Borrowed(&[Platform::Windows]),
    },
    Need::Distribution {
        distribution: Distribution::Direct,
    },
];

/// Which entries are there now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShellState {
    pub menu: bool,
    pub new_file: bool,
}

struct ShellPlugin {
    manifest: Manifest,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(ShellPlugin {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::App,
            requires: Requirement {
                needs: Cow::Borrowed(NEEDS),
                prefers: Cow::Borrowed(&[]),
            },
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
    })
}

impl Plugin for ShellPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        imp::start(cx)
    }
}

/// Removes every entry and records both choices off (the plugin was switched off). Nothing to do off Windows.
///
/// # Errors
/// The registry refused a delete (text for the log).
pub fn withdraw() -> Result<(), String> {
    imp::withdraw()
}

#[cfg(windows)]
mod imp {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use permark_core::plugins::{typed, NoArgs, PluginCx, PluginError, RpcRegistration, Running};
    use permark_core::settings::AppSettings;
    use permark_core::{ErrorKind, IpcError};

    use serde::Deserialize;

    use super::{Entry, Language, ID};
    use crate::notify::display_languages;
    use crate::registry::Registry;

    /// The real registry; a development build works on `HKCU\Software\PerMark-dev` (never the person's entries).
    fn registry() -> Registry {
        if cfg!(debug_assertions) {
            Registry::under(r"Software\PerMark-dev")
        } else {
            Registry::current_user()
        }
    }

    fn exe() -> Result<PathBuf, IpcError> {
        let exe = std::env::current_exe().map_err(|e| IpcError::io("the program", &e))?;
        Ok(dunce::simplified(&exe).to_path_buf())
    }

    #[derive(Debug, Deserialize)]
    struct SetArgs {
        entry: Entry,
        on: bool,
    }

    fn language(settings: &AppSettings) -> Language {
        Language::resolve(settings.language, &display_languages())
    }

    struct ShellRunning {
        _registrations: Vec<RpcRegistration>,
        language: Mutex<Option<Language>>,
    }

    impl Running for ShellRunning {
        /// The app's language changed: the entries that are there say it in the new one (off the caller's thread:
        /// Explorer may be slow to hear about it).
        fn settings_changed(&self, settings: &AppSettings) {
            let now = language(settings);
            let mut last = self
                .language
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *last == Some(now) {
                return;
            }
            *last = Some(now);
            std::thread::spawn(move || {
                // best effort: an entry that keeps the old words is not worth an error
                let _ = registry().relabel(now);
            });
        }
    }

    pub(super) fn start(cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let Some(app) = cx.app() else {
            return Err(PluginError::Start {
                id: ID,
                reason: "needs the app services".to_owned(),
            });
        };
        let mut registrations = Vec::new();
        registrations.extend(cx.rpc("state", typed(|_: NoArgs, _| Ok(registry().state()))));
        let services = app.clone();
        registrations.extend(cx.rpc(
            "set",
            typed(move |args: SetArgs, _| {
                let registry = registry();
                let language = language(&services.settings());
                registry
                    .set(args.entry, args.on, &exe()?, language)
                    .map_err(|e| IpcError::new(ErrorKind::Denied).detail(e.to_string()))?;
                Ok(registry.state())
            }),
        ));
        Ok(Box::new(ShellRunning {
            _registrations: registrations,
            language: Mutex::new(Some(language(&app.settings()))),
        }))
    }

    pub(super) fn withdraw() -> Result<(), String> {
        let exe = exe().map_err(|e| e.to_string())?;
        registry().withdraw(&exe).map_err(|e| e.to_string())
    }
}

#[cfg(not(windows))]
mod imp {
    use permark_core::plugins::{PluginCx, PluginError, Running};

    struct Nothing;
    impl Running for Nothing {}

    #[allow(clippy::unnecessary_wraps)] // same signature as on Windows
    pub(super) fn start(_cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        Ok(Box::new(Nothing))
    }

    #[allow(clippy::unnecessary_wraps)]
    pub(super) fn withdraw() -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
