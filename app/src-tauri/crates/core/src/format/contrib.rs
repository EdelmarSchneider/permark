//! What plugins add to the format side of core: data only (formats, role rules, Markdown features).

use std::sync::Arc;

use serde::Serialize;
use ts_rs::TS;

use super::Format;

/// Tags a prompt-blocks plugin folds by default.
pub const DEFAULT_PROMPT_TAGS: &[&str] = &[
    "context",
    "instructions",
    "example",
    "examples",
    "thinking",
    "task",
    "input",
    "output",
    "document",
    "system",
    "rules",
    "constraints",
    "format",
];

/// How a file name picks a role (and so Markdown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleMatch {
    /// The whole file name (case-insensitive).
    Name(&'static str),
    /// The file name ends with (case-insensitive): `.mdc`, `.prompt`.
    Suffix(&'static str),
    /// The whole project path ends with (case-insensitive, `/` separators): `.github/copilot-instructions.md`.
    Path(&'static str),
}

/// A file used in a known way (`AGENTS.md`): a badge in the header, and read as Markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleRule {
    pub id: &'static str,
    pub matches: RoleMatch,
}

impl RoleRule {
    #[must_use]
    pub fn applies(&self, path: &str) -> bool {
        let lower = path.replace('\\', "/").to_lowercase();
        let name = lower.rsplit('/').next().unwrap_or(&lower);
        match self.matches {
            RoleMatch::Name(wanted) => name == wanted.to_lowercase(),
            RoleMatch::Suffix(wanted) => name.ends_with(&wanted.to_lowercase()),
            RoleMatch::Path(wanted) => {
                let wanted = wanted.to_lowercase();
                lower == wanted || lower.ends_with(&format!("/{wanted}"))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FrontMatterSyntax {
    Yaml,
    Toml,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum FieldValue {
    Text { text: String },
    List { items: Vec<String> },
}

/// One property of the front matter card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Field {
    pub key: String,
    pub value: FieldValue,
}

/// Turns the text between the delimiters into card fields.
pub type FrontMatterParser = fn(FrontMatterSyntax, &str) -> Vec<Field>;

/// Prompt tags folded as blocks, `{{name}}` marks and `@path` references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptBlocks {
    pub tags: &'static [&'static str],
}

/// Something a plugin adds to the Markdown pipeline.
#[derive(Debug, Clone, Copy)]
pub enum MarkdownFeature {
    PromptBlocks(PromptBlocks),
    FrontMatterCard(FrontMatterParser),
}

/// The Markdown features that are on.
#[derive(Debug, Clone, Copy, Default)]
pub struct MarkdownFeatures {
    pub prompt_blocks: Option<PromptBlocks>,
    pub front_matter_card: Option<FrontMatterParser>,
}

/// What the enabled plugins add (see `Plugin::contributions`).
#[derive(Clone, Default)]
pub struct Contributions {
    pub formats: Vec<Arc<dyn Format>>,
    pub roles: Vec<RoleRule>,
    pub markdown: Vec<MarkdownFeature>,
}

impl std::fmt::Debug for Contributions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Contributions")
            .field(
                "formats",
                &self.formats.iter().map(|x| x.id()).collect::<Vec<_>>(),
            )
            .field("roles", &self.roles)
            .field("markdown", &self.markdown.len())
            .finish()
    }
}

impl Contributions {
    /// Adds another set after this one (topological order of plugins).
    pub fn extend(&mut self, other: Self) {
        self.formats.extend(other.formats);
        self.roles.extend(other.roles);
        self.markdown.extend(other.markdown);
    }

    #[must_use]
    pub fn formats(mut self, formats: Vec<Arc<dyn Format>>) -> Self {
        self.formats = formats;
        self
    }

    #[must_use]
    pub fn roles(mut self, roles: Vec<RoleRule>) -> Self {
        self.roles = roles;
        self
    }

    #[must_use]
    pub fn markdown(mut self, markdown: Vec<MarkdownFeature>) -> Self {
        self.markdown = markdown;
        self
    }
}
