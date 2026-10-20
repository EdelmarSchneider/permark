//! The files of a window's workspace for App-scope plugins (the converter): read under a cap, write a new file next
//! to one. Each call goes through the window's own editor (its scope, its boundary, its own-write announcements).

use std::sync::Arc;

use permark_core::plugins::{ServiceLease, WindowFiles, WindowFilesApi};
use permark_core::workspace::CoreEnv;
use permark_core::{ErrorKind, IpcError};
use tauri::{AppHandle, Manager};

use crate::commands::with_editor;

struct ShellFiles {
    handle: AppHandle,
}

impl ShellFiles {
    fn window(&self, label: &str) -> Result<tauri::WebviewWindow, IpcError> {
        self.handle
            .get_webview_window(label)
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject("window"))
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
}

/// Offers the service for the app's whole run.
pub fn provide(env: &CoreEnv, handle: &AppHandle) -> ServiceLease {
    env.services()
        .provide(Arc::new(WindowFilesApi(Arc::new(ShellFiles {
            handle: handle.clone(),
        }))))
}
