//! `PerMark` domain core. Tauri-free: app crate adapts these services to IPC.

pub mod app_info;
pub mod conditions;
pub mod document;
pub mod edit;
pub mod error;
pub mod events;
pub mod fileops;
pub mod format;
pub mod fsx;
pub mod ipc_local;
pub mod links;
pub mod open;
pub mod paths;
pub mod plugins;
pub mod search;
pub mod session;
pub mod settings;
pub mod state;
pub mod tree;
pub mod watch;
pub mod workspace;

pub use app_info::{app_info, AppInfo};
pub use error::{ErrorKind, IpcError};
