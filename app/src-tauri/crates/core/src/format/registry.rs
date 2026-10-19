//! `FormatRegistry`: the formats of the enabled plugins + core Markdown; picks one per file.

use std::sync::Arc;

use super::{
    ext_of, name_of, Claim, Contributions, Format, MarkdownFeature, MarkdownFeatures,
    MarkdownFormat, RoleRule,
};

pub struct FormatRegistry {
    /// Core Markdown first, then plugin formats in plugin order.
    formats: Vec<Arc<dyn Format>>,
    roles: Vec<RoleRule>,
    features: MarkdownFeatures,
    analyses: Vec<Arc<dyn super::Analysis>>,
}

impl std::fmt::Debug for FormatRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormatRegistry")
            .field(
                "formats",
                &self.formats.iter().map(|x| x.id()).collect::<Vec<_>>(),
            )
            .field("roles", &self.roles)
            .finish_non_exhaustive()
    }
}

impl FormatRegistry {
    /// Core Markdown alone (no plugin on).
    #[must_use]
    pub fn core() -> Self {
        Self::build(&Contributions::default())
    }

    #[must_use]
    pub fn build(contributions: &Contributions) -> Self {
        let mut formats: Vec<Arc<dyn Format>> = vec![Arc::new(MarkdownFormat)];
        formats.extend(contributions.formats.iter().cloned());
        let mut features = MarkdownFeatures::default();
        for feature in &contributions.markdown {
            match feature {
                MarkdownFeature::PromptBlocks(blocks) => features.prompt_blocks = Some(*blocks),
                MarkdownFeature::FrontMatterCard(parser) => {
                    features.front_matter_card = Some(*parser);
                }
            }
        }
        Self {
            formats,
            roles: contributions.roles.clone(),
            features,
            analyses: contributions.analyses.clone(),
        }
    }

    /// Measures of `text` by every enabled analysis, in plugin order.
    #[must_use]
    pub fn analyze(&self, text: &str, kind: super::TextKind) -> Vec<super::Measure> {
        self.analyses
            .iter()
            .flat_map(|a| a.measure(text, kind))
            .collect()
    }

    #[must_use]
    pub fn features(&self) -> &MarkdownFeatures {
        &self.features
    }

    /// The role of a project path (`agents`, `claude`, ...), if a rule matches.
    #[must_use]
    pub fn role_of(&self, path: &str) -> Option<&'static str> {
        self.roles.iter().find(|r| r.applies(path)).map(|r| r.id)
    }

    /// The format with the strongest claim; ties go to the earlier one. A role makes the file Markdown.
    #[must_use]
    pub fn pick(&self, path: &str, head: &[u8]) -> Option<&Arc<dyn Format>> {
        let name = name_of(path);
        let ext = ext_of(name);
        let mut best: Option<(&Arc<dyn Format>, Claim)> = None;
        for format in &self.formats {
            let mut claim = format.claims(name, ext.as_deref(), head);
            if claim < Claim::Strong
                && format.id() == super::FormatId::MARKDOWN
                && self.role_of(path).is_some()
            {
                claim = Claim::Strong;
            }
            if claim != Claim::No && best.is_none_or(|(_, c)| claim > c) {
                best = Some((format, claim));
            }
        }
        best.map(|(format, _)| format)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FormatId, RoleMatch, TextViewer};

    fn registry() -> FormatRegistry {
        let contributions = Contributions::default()
            .formats(vec![
                Arc::new(TextViewer::json()),
                Arc::new(TextViewer::plain()),
            ])
            .roles(vec![
                RoleRule {
                    id: "agents",
                    matches: RoleMatch::Name("AGENTS.md"),
                },
                RoleRule {
                    id: "cursor-rule",
                    matches: RoleMatch::Suffix(".mdc"),
                },
                RoleRule {
                    id: "copilot",
                    matches: RoleMatch::Path(".github/copilot-instructions.md"),
                },
            ]);
        FormatRegistry::build(&contributions)
    }

    fn picked(registry: &FormatRegistry, path: &str) -> Option<FormatId> {
        registry.pick(path, b"").map(|f| f.id())
    }

    #[test]
    fn strongest_claim_wins_and_text_is_the_fallback() {
        let r = registry();
        assert_eq!(picked(&r, "docs/a.md"), Some(FormatId::MARKDOWN));
        assert_eq!(picked(&r, "x/data.JSON"), Some(FormatId::JSON));
        assert_eq!(picked(&r, "x/notes.log"), Some(FormatId::TEXT));
        assert_eq!(picked(&r, "Makefile"), Some(FormatId::TEXT));
    }

    #[test]
    fn roles_make_markdown_and_match_case_insensitively() {
        let r = registry();
        assert_eq!(picked(&r, "agents.MD"), Some(FormatId::MARKDOWN));
        assert_eq!(r.role_of("sub/AGENTS.md"), Some("agents"));
        assert_eq!(r.role_of("rules/style.MDC"), Some("cursor-rule"));
        assert_eq!(picked(&r, "rules/style.mdc"), Some(FormatId::MARKDOWN));
        assert_eq!(
            r.role_of(".github/copilot-instructions.md"),
            Some("copilot")
        );
        assert_eq!(
            r.role_of("docs/.github/copilot-instructions.md"),
            Some("copilot")
        );
        assert_eq!(r.role_of("copilot-instructions.md"), None);
        assert_eq!(r.role_of("AGENTS.md.bak"), None);
    }

    #[test]
    fn nothing_picked_when_no_viewer_is_on() {
        let r = FormatRegistry::core();
        assert_eq!(picked(&r, "a.md"), Some(FormatId::MARKDOWN));
        assert_eq!(picked(&r, "a.json"), None);
        assert_eq!(r.role_of("AGENTS.md"), None);
    }
}
