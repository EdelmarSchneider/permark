//! The desktop around PerMark: which file types it offers to open, and the person's choice of default app.
//!
//! PerMark becomes the default app of a file type only when the person asks. Where the system lets a program set it
//! (macOS `LaunchServices`, Linux `mimeapps.list`), the handler it replaces is recorded first and given back when the
//! person unchecks the type. Windows protects the choice: PerMark only lists itself ("Open with") and hands the person to
//! the system's Default Apps page.

mod choice;
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
mod mimeapps;
#[cfg(windows)]
pub mod windows;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use choice::{choose, is_ours, DefaultAppState, Handlers};
pub use mimeapps::{default_of, with_default};

/// A family of file types offered together ("Open these files with PerMark").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FileGroup {
    Markdown,
    Text,
    Json,
    Xml,
    Yaml,
    Toml,
    Ini,
    Properties,
}

impl FileGroup {
    /// Every group, in the order the person sees them.
    pub const ALL: [Self; 8] = [
        Self::Markdown,
        Self::Text,
        Self::Json,
        Self::Xml,
        Self::Yaml,
        Self::Toml,
        Self::Ini,
        Self::Properties,
    ];

    /// File extensions, with the dot.
    #[must_use]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Markdown => &[".md", ".markdown", ".mdown", ".mkd"],
            Self::Text => &[".txt"],
            Self::Json => &[".json", ".jsonl"],
            Self::Xml => &[".xml"],
            Self::Yaml => &[".yaml", ".yml"],
            Self::Toml => &[".toml"],
            Self::Ini => &[".ini", ".cfg"],
            Self::Properties => &[".properties"],
        }
    }

    /// MIME types (freedesktop shared-mime-info names, aliases included).
    #[must_use]
    pub const fn mime_types(self) -> &'static [&'static str] {
        match self {
            Self::Markdown => &["text/markdown", "text/x-markdown"],
            Self::Text => &["text/plain"],
            Self::Json => &["application/json"],
            Self::Xml => &["application/xml", "text/xml"],
            Self::Yaml => &["application/yaml", "application/x-yaml"],
            Self::Toml => &["application/toml"],
            Self::Ini => &["text/x-ini", "application/x-wine-extension-ini"],
            Self::Properties => &["text/x-java-properties"],
        }
    }

    /// The Windows `ProgID` the installer registers for the group.
    #[must_use]
    pub const fn prog_id(self) -> &'static str {
        match self {
            Self::Markdown => "PerMark.Markdown",
            Self::Text => "PerMark.Text",
            Self::Json => "PerMark.Json",
            Self::Xml => "PerMark.Xml",
            Self::Yaml => "PerMark.Yaml",
            Self::Toml => "PerMark.Toml",
            Self::Ini => "PerMark.Ini",
            Self::Properties => "PerMark.Properties",
        }
    }

    /// Checked when PerMark asks: Markdown only.
    #[must_use]
    pub const fn checked_by_default(self) -> bool {
        matches!(self, Self::Markdown)
    }
}

/// How this copy of PerMark can become the default app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase", tag = "mode")]
#[ts(export)]
pub enum DefaultAppMode {
    /// PerMark sets it (macOS, an installed Linux package).
    Direct,
    /// The system's settings set it (Windows).
    Handoff,
    /// Neither: the person uses the system's settings.
    Unsupported { reason: Unsupported },
}

/// Why this copy cannot set the default app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Unsupported {
    /// Not installed with a desktop entry (a Linux AppImage started from a folder).
    NotInstalled,
    /// A confined package (snap) that cannot write the person's settings.
    Confined,
}

/// One group as the settings show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GroupView {
    pub group: FileGroup,
    pub extensions: Vec<String>,
    /// PerMark opens these files now (every type of the group).
    pub ours: bool,
}

/// The default-app part of the settings, and whether to ask now (first run).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DefaultAppView {
    pub mode: DefaultAppMode,
    pub groups: Vec<GroupView>,
    /// Ask the person once (first run of a copy that did not ask at install).
    pub ask: bool,
}

/// Whether the first-run question is due: never asked, not an older PerMark's computer (its data folder is there: the
/// person chose before), and a way to act on the answer.
#[must_use]
pub fn should_ask(state: &DefaultAppState, older_data: bool, mode: &DefaultAppMode) -> bool {
    !state.asked && !older_data && !matches!(mode, DefaultAppMode::Unsupported { .. })
}

#[cfg(test)]
mod tests;
