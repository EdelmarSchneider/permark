//! `PerMark` domain core. Tauri-free: app crate adapts these services to IPC.

pub mod app_info;

pub use app_info::{app_info, AppInfo};
