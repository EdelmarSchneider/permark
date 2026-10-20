//! What plugins add to the format side of core: data only (formats, role rules, Markdown features).

use std::sync::Arc;

use serde::Serialize;
use ts_rs::TS;

use super::{Analysis, Format};

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

/// A Markdown file used in a known way, told by the keys of its front matter (a model card, a dataset card): a badge
/// in the header, and the card lists the keys of the role first. Facts only: key names, never prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyRole {
    pub id: &'static str,
    /// Top-level keys of which one makes the file this role (listed first in the card, in this order).
    pub keys: &'static [&'static str],
    /// Keys the card lists right after those; they decide nothing alone.
    pub also: &'static [&'static str],
}

impl KeyRole {
    /// The place of a front matter key in the card: before the others when it belongs to the role.
    fn rank(&self, key: &str) -> Option<usize> {
        let top = key.split('.').next().unwrap_or(key);
        self.keys
            .iter()
            .chain(self.also)
            .position(|wanted| *wanted == top)
    }

    #[must_use]
    pub fn applies(&self, fields: &[Field]) -> bool {
        fields.iter().any(|field| {
            let top = field.key.split('.').next().unwrap_or(&field.key);
            self.keys.contains(&top)
        })
    }

    /// Moves the fields of the role to the front (in the role's order; the others keep theirs).
    pub fn put_first(&self, fields: &mut [Field]) {
        fields.sort_by_key(|field| self.rank(&field.key).unwrap_or(usize::MAX));
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
    pub key_roles: Vec<KeyRole>,
    pub markdown: Vec<MarkdownFeature>,
    pub analyses: Vec<Arc<dyn Analysis>>,
}

impl std::fmt::Debug for Contributions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Contributions")
            .field(
                "formats",
                &self.formats.iter().map(|x| x.id()).collect::<Vec<_>>(),
            )
            .field("roles", &self.roles)
            .field("key_roles", &self.key_roles)
            .field("markdown", &self.markdown.len())
            .field(
                "analyses",
                &self.analyses.iter().map(|a| a.id()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Contributions {
    /// Adds another set after this one (topological order of plugins).
    pub fn extend(&mut self, other: Self) {
        self.formats.extend(other.formats);
        self.roles.extend(other.roles);
        self.key_roles.extend(other.key_roles);
        self.markdown.extend(other.markdown);
        self.analyses.extend(other.analyses);
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
    pub fn key_roles(mut self, key_roles: Vec<KeyRole>) -> Self {
        self.key_roles = key_roles;
        self
    }

    #[must_use]
    pub fn analyses(mut self, analyses: Vec<Arc<dyn Analysis>>) -> Self {
        self.analyses = analyses;
        self
    }

    #[must_use]
    pub fn markdown(mut self, markdown: Vec<MarkdownFeature>) -> Self {
        self.markdown = markdown;
        self
    }
}
