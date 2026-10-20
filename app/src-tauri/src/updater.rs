//! The updater's IO for the plugin `updates`: the signed update server (tauri-plugin-updater, endpoint + public key from
//! `tauri.conf.json`), the downloaded update, the unsaved check across windows, the installer and the restart.
//!
//! A development build checks only when `PERMARK_UPDATER_ENDPOINT` names a server (a local fake `latest.json`), with
//! `PERMARK_UPDATER_PUBKEY` as its key; a release build never reads those.

use std::sync::{Arc, Mutex, PoisonError};

use permark_core::plugins::ServiceLease;
use permark_core::workspace::{CoreEnv, Workspace};
use permark_plugin_updates::{DriverService, Found, UpdateDriver};
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::shell;

struct Driver {
    handle: AppHandle,
    found: Mutex<Option<Update>>,
    bytes: Mutex<Option<Vec<u8>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The development override of the server (`None` in release builds).
fn dev_override(name: &str) -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var(name).ok().filter(|v| !v.trim().is_empty())
    } else {
        None
    }
}

impl Driver {
    fn updater(&self) -> Result<tauri_plugin_updater::Updater, String> {
        let mut builder = self.handle.updater_builder();
        if let Some(endpoint) = dev_override("PERMARK_UPDATER_ENDPOINT") {
            let url = endpoint.parse().map_err(|e| format!("{e}"))?;
            builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
        }
        if let Some(key) = dev_override("PERMARK_UPDATER_PUBKEY") {
            builder = builder.pubkey(key);
        }
        builder.build().map_err(|e| e.to_string())
    }
}

impl UpdateDriver for Driver {
    fn automatic(&self) -> bool {
        !cfg!(debug_assertions) || dev_override("PERMARK_UPDATER_ENDPOINT").is_some()
    }

    fn check(&self) -> Result<Option<Found>, String> {
        let updater = self.updater()?;
        let update = tauri::async_runtime::block_on(updater.check()).map_err(|e| e.to_string())?;
        let found = update.as_ref().map(|u| Found {
            version: u.version.clone(),
            notes: u.body.clone().filter(|b| !b.trim().is_empty()),
        });
        *lock(&self.found) = update;
        *lock(&self.bytes) = None;
        Ok(found)
    }

    fn download(&self, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<(), String> {
        let update = lock(&self.found)
            .clone()
            .ok_or_else(|| "no update was found".to_owned())?;
        let mut done = 0u64;
        // verified against the public key by the updater before it returns
        let bytes = tauri::async_runtime::block_on(update.download(
            |chunk, total| {
                done = done.saturating_add(chunk as u64);
                progress(done, total);
            },
            || {},
        ))
        .map_err(|e| e.to_string())?;
        *lock(&self.bytes) = Some(bytes);
        Ok(())
    }

    fn unsaved(&self) -> Vec<String> {
        let shell = shell::state(&self.handle);
        let mut paths = Vec::new();
        for label in self.handle.webview_windows().into_keys() {
            let Some((_, workspace)) = shell.registry.workspace_of(&label) else {
                continue;
            };
            let unsaved = match &workspace {
                Workspace::Project(core) => core.documents().unsaved(&label),
                Workspace::SingleFile(file) => file.documents.unsaved(&label),
                Workspace::Notes(core) => core.documents().unsaved(&label),
            };
            paths.extend(unsaved);
        }
        paths
    }

    fn restart(&self) -> Result<(), String> {
        let update = lock(&self.found)
            .clone()
            .ok_or_else(|| "no update was found".to_owned())?;
        let bytes = lock(&self.bytes)
            .clone()
            .ok_or_else(|| "the update is not downloaded".to_owned())?;
        shell::state(&self.handle).save_session_now(&self.handle);
        // Windows: starts the installer (passive, `/UPDATE`) and exits; elsewhere: replaces the program, then restart
        update.install(bytes).map_err(|e| e.to_string())?;
        self.handle.restart()
    }
}

/// Hands the driver to the plugin (kept while the lease lives: the app's whole run).
#[must_use]
pub fn provide(env: &CoreEnv, handle: &AppHandle) -> ServiceLease {
    env.services()
        .provide(Arc::new(DriverService(Arc::new(Driver {
            handle: handle.clone(),
            found: Mutex::new(None),
            bytes: Mutex::new(None),
        }))))
}
