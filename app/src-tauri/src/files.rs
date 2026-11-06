//! The files of a window's workspace for App-scope plugins (the converter): read under a cap, write a new file next
//! to one. Each call goes through the window's own editor (its scope, its boundary, its own-write announcements).
//! Also the project folders (open + recent) a plugin's own folder must stay clear of, and showing a plugin's folder in
//! the file manager.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use permark_core::paths::VaultPath;
use permark_core::plugins::{
    ChangeListener, FolderReveal, FolderRevealApi, FolderScan, ProjectPlaces, ProjectPlacesApi,
    ServiceLease, WindowFiles, WindowFilesApi,
};
use permark_core::tree::ScanFile;
use permark_core::watch::Subscription;
use permark_core::workspace::{CoreEnv, ProjectCore, Workspace};
use permark_core::{ErrorKind, IpcError};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::commands::with_editor;

struct ShellFiles {
    handle: AppHandle,
}

impl ShellFiles {
    fn window(&self, label: &str) -> Result<tauri::Webview, IpcError> {
        self.handle
            .get_webview(label)
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject("window"))
    }

    /// The project the window shows.
    fn project(&self, label: &str) -> Option<Arc<ProjectCore>> {
        match crate::shell::state(&self.handle)
            .registry
            .workspace_of(label)
        {
            Some((_, Workspace::Project(core))) => Some(core),
            _ => None,
        }
    }
}

impl WindowFiles for ShellFiles {
    fn read(&self, window: &str, path: &str, cap: u64) -> Result<String, IpcError> {
        let window = self.window(window)?;
        with_editor(&self.handle, &window, |editor, _| {
            editor.read_capped(path, cap)
        })
    }

    fn write_beside(
        &self,
        window: &str,
        beside: &str,
        name: &str,
        text: &str,
    ) -> Result<String, IpcError> {
        let window = self.window(window)?;
        with_editor(&self.handle, &window, |editor, _| {
            editor.write_beside(beside, name, text)
        })
    }

    fn note(&self, window: &str, fact: permark_core::activity::Fact) {
        if let Some(core) = self.project(window) {
            core.activity().person(fact);
        }
    }

    fn scan_folder(
        &self,
        window: &str,
        folder: &str,
        cancel: &AtomicBool,
        stop: &mut dyn FnMut(&ScanFile) -> bool,
    ) -> Result<FolderScan, IpcError> {
        let core = self
            .project(window)
            .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).subject("project"))?;
        let path = VaultPath::parse(folder)?;
        let abs = core.vault().resolve(&path)?;
        let scan = core.explorer().scan_folder(&path, &abs, cancel, stop)?;
        Ok(FolderScan {
            project: core.root().path().display().to_string(),
            scan,
        })
    }

    fn watch(&self, window: &str, listener: ChangeListener) -> Option<Subscription> {
        let core = self.project(window)?;
        Some(core.watcher().hub().subscribe(move |batch| listener(batch)))
    }
}

impl ProjectPlaces for ShellFiles {
    fn folders(&self) -> Vec<PathBuf> {
        let shell = crate::shell::state(&self.handle);
        let open = shell
            .registry
            .cores()
            .into_iter()
            .map(|core| core.root().path().to_path_buf());
        let recent = shell
            .recent
            .list()
            .into_iter()
            .map(|folder| PathBuf::from(folder.path));
        open.chain(recent).collect()
    }
}

impl FolderReveal for ShellFiles {
    fn reveal(&self, folder: &Path) -> Result<(), IpcError> {
        if !folder.is_dir() {
            return Err(IpcError::new(ErrorKind::NotFound).subject(folder.display().to_string()));
        }
        self.handle
            .opener()
            .reveal_item_in_dir(folder)
            .map_err(|e| {
                IpcError::new(ErrorKind::Io)
                    .subject(folder.display().to_string())
                    .detail(e.to_string())
            })
    }
}

/// Offers the services for the app's whole run (window files, project places, folder reveal).
pub fn provide(env: &CoreEnv, handle: &AppHandle) -> Vec<ServiceLease> {
    let files = Arc::new(ShellFiles {
        handle: handle.clone(),
    });
    let services = env.services();
    vec![
        services.provide(Arc::new(WindowFilesApi(Arc::clone(&files) as _))),
        services.provide(Arc::new(ProjectPlacesApi(Arc::clone(&files) as _))),
        services.provide(Arc::new(FolderRevealApi(files))),
    ]
}
