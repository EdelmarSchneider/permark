//! `PerMark` app crate: Tauri shell. Wires core services to windows + IPC; holds no domain logic.

mod commands;

/// Builds + runs the app until the last window closes.
///
/// # Errors
/// Tauri fails to start (window | webview creation, runtime).
pub fn run() -> Result<(), tauri::Error> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::app_info])
        .run(tauri::generate_context!())
}
