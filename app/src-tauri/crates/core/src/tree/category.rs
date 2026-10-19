//! File category from name: one table, used by the filter, quick open and the viewers' claims.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Category {
    Markdown,
    Text,
    Config,
    Data,
    Image,
    Audio,
    Video,
    Office,
    Other,
}

impl Category {
    pub const ALL: [Self; 9] = [
        Self::Markdown,
        Self::Text,
        Self::Config,
        Self::Data,
        Self::Image,
        Self::Audio,
        Self::Video,
        Self::Office,
        Self::Other,
    ];

    /// Companions of documents: the "only assets next to documents" option applies to them.
    #[must_use]
    pub fn is_asset(self) -> bool {
        matches!(
            self,
            Self::Config | Self::Data | Self::Image | Self::Audio | Self::Video
        )
    }

    /// Pictures, sound and video: also shown inside asset-named folders.
    #[must_use]
    pub fn is_media(self) -> bool {
        matches!(self, Self::Image | Self::Audio | Self::Video)
    }

    /// Documents: a folder holding one is a place for reading.
    #[must_use]
    pub fn is_document(self) -> bool {
        matches!(self, Self::Markdown | Self::Text)
    }
}

const MARKDOWN: &[&str] = &["md", "markdown", "mdown", "mkd", "mdx", "mdc"];
const TEXT: &[&str] = &["txt", "text", "rst", "adoc", "asciidoc", "org", "log"];
const CONFIG: &[&str] = &[
    "toml",
    "yaml",
    "yml",
    "ini",
    "cfg",
    "conf",
    "properties",
    "env",
    "editorconfig",
];
const DATA: &[&str] = &[
    "json", "jsonc", "json5", "jsonl", "ndjson", "csv", "tsv", "xml", "xsd", "xsl", "xslt", "rss",
    "atom",
];
const IMAGE: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "ico", "svg",
];
const AUDIO: &[&str] = &[
    "mp3", "wav", "ogg", "oga", "opus", "flac", "m4a", "aac", "weba", "wma", "aif", "aiff", "mka",
];
const VIDEO: &[&str] = &[
    "mp4", "m4v", "webm", "ogv", "mov", "mkv", "avi", "wmv", "flv", "mpg", "mpeg", "3gp",
];
const OFFICE: &[&str] = &[
    "doc", "docx", "dot", "dotx", "xls", "xlsx", "xlsm", "ppt", "pptx", "odt", "ods", "odp", "rtf",
    "pdf",
];

/// Lower-case extension of a file name, without the dot; `None` for no extension and for a dotfile (`.env`).
#[must_use]
pub fn extension_of(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    if dot == 0 || dot + 1 == name.len() {
        return None;
    }
    Some(name[dot + 1..].to_lowercase())
}

/// Category of a file from its name (extension, or well-known dotfile names).
#[must_use]
pub fn category_of(name: &str) -> Category {
    let lower = name.to_lowercase();
    if lower == ".env"
        || lower.starts_with(".env.")
        || lower == ".gitattributes"
        || lower == ".editorconfig"
        || (lower.starts_with('.') && lower.ends_with("ignore"))
    {
        return Category::Config;
    }
    if let Some(ext) = extension_of(name) {
        let ext = ext.as_str();
        for (list, category) in [
            (MARKDOWN, Category::Markdown),
            (TEXT, Category::Text),
            (CONFIG, Category::Config),
            (DATA, Category::Data),
            (IMAGE, Category::Image),
            (AUDIO, Category::Audio),
            (VIDEO, Category::Video),
            (OFFICE, Category::Office),
        ] {
            if list.contains(&ext) {
                return category;
            }
        }
        return Category::Other;
    }
    Category::Other
}

/// Extensions a document opened by the system's default program may have: never executables | scripts.
/// Deny list (wins over everything) for the "open with default app" action.
const NEVER_OPEN: &[&str] = &[
    "exe", "com", "bat", "cmd", "msi", "msp", "scr", "pif", "lnk", "url", "reg", "ps1", "psm1",
    "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "cpl", "jar", "sh", "bash", "zsh", "command",
    "app", "desktop", "appimage", "run", "bin", "dll", "sys", "py", "pl", "rb", "php", "scpt",
    "workflow", "action", "apk", "deb", "rpm", "dmg", "pkg", "gadget", "inf", "ins", "isp", "mht",
    "svg", "mhtml", "chm", "swf", "svgz",
];

/// Whether the system's default program may be asked to open this file name.
#[must_use]
pub fn may_open_with_default_app(name: &str) -> bool {
    let Some(ext) = extension_of(name) else {
        return false;
    };
    if NEVER_OPEN.contains(&ext.as_str()) {
        return false;
    }
    !matches!(category_of(name), Category::Other)
}

/// `.html` | `.htm`: opened in the browser by its own action.
#[must_use]
pub fn is_html(name: &str) -> bool {
    matches!(extension_of(name).as_deref(), Some("html" | "htm"))
}
