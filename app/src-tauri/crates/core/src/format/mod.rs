//! Formats: how a file becomes something to read. Core owns the API and Markdown; viewers, roles and Markdown
//! features arrive from plugins as data ([`Contributions`]). Editing parts (`edit`, `secrets`, parts of a
//! structure) join the trait with the editing area.

mod analysis;
mod contrib;
mod markdown;
mod registry;
mod viewers;

use std::collections::HashSet;

use serde::Serialize;
use ts_rs::TS;

pub use analysis::{Analysis, Measure, MeasureUnit, TextKind};
pub use contrib::{
    Contributions, Field, FieldValue, FrontMatterParser, FrontMatterSyntax, MarkdownFeature,
    MarkdownFeatures, PromptBlocks, RoleMatch, RoleRule, DEFAULT_PROMPT_TAGS,
};
pub use markdown::{segments, LockReason, MarkdownFormat, Segment, SegmentKind};
pub use registry::FormatRegistry;
pub use viewers::{media_of, MediaFormat, TextViewer, MEDIA_EXTENSIONS};

/// Identity of a format (`markdown | text | json | xml | media`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct FormatId(pub &'static str);

impl FormatId {
    pub const MARKDOWN: Self = Self("markdown");
    pub const TEXT: Self = Self("text");
    pub const JSON: Self = Self("json");
    pub const XML: Self = Self("xml");
    pub const MEDIA: Self = Self("media");
}

/// How strongly a format claims a file (by name > by extension > by sniffing the head).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Claim {
    No,
    Weak,
    Strong,
}

/// What a render may ask of the document's surroundings.
pub trait DocEnv {
    /// Whether the project path names an existing file (for `@path` references).
    fn file_exists(&self, project_path: &str) -> bool;
}

/// Everything a render needs besides the text.
pub struct RenderCx<'a> {
    /// Project path of the document (breadcrumb | relative links | asset paths).
    pub path: &'a str,
    pub env: &'a dyn DocEnv,
    pub features: &'a MarkdownFeatures,
    /// Block ids the caller already holds (their HTML is left out).
    pub known: &'a HashSet<String>,
}

/// One format.
pub trait Format: Send + Sync {
    fn id(&self) -> FormatId;
    fn claims(&self, name: &str, ext: Option<&str>, head: &[u8]) -> Claim;
    /// `false`: the file is never read as text (media; the asset protocol streams it).
    fn reads_text(&self) -> bool {
        true
    }
    /// The viewer data of `text`.
    fn render(&self, text: &str, cx: &RenderCx<'_>) -> Rendered;
}

/// A render: the body and the notes about it (`utf16 | lossy | deepNesting` words the UI knows).
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub body: Body,
    pub notes: Vec<&'static str>,
}

/// What a viewer shows.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Body {
    Markdown {
        blocks: Vec<Block>,
        outline: Vec<OutlineItem>,
        front_matter: Option<FrontMatter>,
    },
    Text {
        text: String,
        language: Option<String>,
    },
    Media {
        media: MediaKind,
        mime: String,
    },
    Notice {
        notice: Notice,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
}

/// Why there is no body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "notice", rename_all = "camelCase")]
#[ts(export)]
pub enum Notice {
    Binary,
    TooLarge {
        #[ts(type = "number")]
        limit: u64,
    },
    NoViewer,
}

/// One top-level block of a Markdown document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Block {
    /// Content id: equal HTML -> equal id (plus an occurrence number for repeats).
    pub id: String,
    /// Tag of the block's element.
    pub kind: String,
    pub line_start: u32,
    pub line_end: u32,
    /// Absent when the caller listed the id as known.
    pub html: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OutlineItem {
    pub label: String,
    pub level: u8,
    /// Anchor id of the heading in the HTML.
    pub id: String,
    pub line: u32,
    /// Id of the block holding the heading.
    pub block: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FrontMatter {
    pub syntax: FrontMatterSyntax,
    /// The text between the delimiters.
    pub raw: String,
    pub fields: Vec<Field>,
}

/// Whether a document can be edited and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "editable", rename_all = "camelCase")]
#[ts(export)]
pub enum Editability {
    Surgical,
    ReadOnly { reason: String },
}

/// Lowercase extension of the last path segment, without dot (none for dotfiles and names ending in a dot).
#[must_use]
pub fn ext_of(name: &str) -> Option<String> {
    crate::tree::extension_of(name)
}

/// Last path segment.
#[must_use]
pub fn name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}
