//! IPC commands: typed adapters over core services. Frontend mirror: `app/src/api/`.
//! `#[tauri::command(async)]` -> runs off the main thread.

use permark_core::AppInfo;

#[tauri::command(async)]
pub fn app_info() -> AppInfo {
    permark_core::app_info()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_adapts_core() {
        assert_eq!(app_info(), permark_core::app_info());
    }
}
