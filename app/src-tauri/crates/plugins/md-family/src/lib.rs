//! Plugin `md-family`: Markdown-family files of the AI era recognized with no setup: agent instructions, skills, rules, llms.txt, prompt files.

use std::sync::Arc;

use permark_core::format::Contributions;
use permark_core::format::{RoleMatch, RoleRule};
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

/// Names and suffixes that make a file a known kind of Markdown.
const ROLES: [RoleRule; 9] = [
    RoleRule {
        id: "agents",
        matches: RoleMatch::Name("AGENTS.md"),
    },
    RoleRule {
        id: "claude",
        matches: RoleMatch::Name("CLAUDE.md"),
    },
    RoleRule {
        id: "skill",
        matches: RoleMatch::Name("SKILL.md"),
    },
    RoleRule {
        id: "cursor-rule",
        matches: RoleMatch::Suffix(".mdc"),
    },
    RoleRule {
        id: "llms",
        matches: RoleMatch::Name("llms.txt"),
    },
    RoleRule {
        id: "llms",
        matches: RoleMatch::Name("llms-full.txt"),
    },
    RoleRule {
        id: "prompt",
        matches: RoleMatch::Suffix(".prompt"),
    },
    RoleRule {
        id: "prompt",
        matches: RoleMatch::Suffix(".prompty"),
    },
    RoleRule {
        id: "copilot",
        matches: RoleMatch::Path(".github/copilot-instructions.md"),
    },
];

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "md-family",
        PluginKind::Role,
        Contributions::default().roles(ROLES.to_vec()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_a_valid_app_plugin() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "md-family");
        assert_eq!(manifest.kind, PluginKind::Role);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }

    #[test]
    fn known_names_get_their_role() {
        use permark_core::format::{Contributions as C, FormatRegistry};
        let registry = FormatRegistry::build(&{
            let mut all = C::default();
            all.extend(plugin().contributions());
            all
        });
        for (path, role) in [
            ("AGENTS.md", "agents"),
            ("sub/CLAUDE.md", "claude"),
            ("skills/x/SKILL.md", "skill"),
            (".cursor/rules/style.mdc", "cursor-rule"),
            ("llms.txt", "llms"),
            ("docs/llms-full.txt", "llms"),
            ("review.prompt", "prompt"),
            ("review.prompty", "prompt"),
            (".github/copilot-instructions.md", "copilot"),
        ] {
            assert_eq!(registry.role_of(path), Some(role), "{path}");
        }
        assert_eq!(registry.role_of("README.md"), None);
        // the copilot file is matched by its whole path, a same-named file elsewhere is not
        assert_eq!(registry.role_of("copilot-instructions.md"), None);
        assert!(registry
            .pick("llms.txt", b"")
            .is_some_and(|f| f.id() == permark_core::format::FormatId::MARKDOWN));
    }
}
