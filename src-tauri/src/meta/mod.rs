//! Tags and descriptions for files.
//!
//! Two sources feed the same chips: tags written in a file's front matter (read-only for the app) and
//! tags added in the app, which are kept outside the files in a `meta.toml` (`.permark/meta.toml`
//! once the project has a `.permark/` folder, a file in AppData before that). The pieces:
//! [`normalize`] (how tags and descriptions are cleaned), [`filetags`] (front matter), [`store`] (the
//! app's entries and the `meta.toml` format), [`catalog`] (counting and finding across the project),
//! [`style`] (the optional color and icon of a tag, kept in the same file), and [`service`] (one project's live metadata: delayed writes, the watcher, the answers for the UI).

pub mod catalog;
pub mod filetags;
pub mod normalize;
pub mod service;
pub mod store;
pub mod style;

pub use service::{MetaError, MetaService, Timing};
