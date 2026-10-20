//! Plugin `md-family`: Markdown-family files of the AI era recognized with no setup: agent instructions, skills, rules, llms.txt, prompt files.

use std::sync::Arc;

use permark_core::format::Contributions;
use permark_core::format::{KeyRole, RoleMatch, RoleRule};
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

/// Model and dataset cards: told by the keys of their front matter, never by prose. A dataset key wins.
const KEY_ROLES: [KeyRole; 2] = [
    KeyRole {
        id: "dataset-card",
        keys: &[
            "dataset_info",
            "task_categories",
            "task_ids",
            "size_categories",
            "annotations_creators",
            "language_creators",
            "source_datasets",
            "multilinguality",
            "pretty_name",
        ],
        also: &["license", "language", "tags", "configs"],
    },
    KeyRole {
        id: "model-card",
        keys: &[
            "model-index",
            "base_model",
            "pipeline_tag",
            "library_name",
            "datasets",
            "metrics",
            "inference",
        ],
        also: &["license", "language", "tags"],
    },
];

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "md-family",
        PluginKind::Role,
        Contributions::default()
            .roles(ROLES.to_vec())
            .key_roles(KEY_ROLES.to_vec()),
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
    fn cards_get_their_role_by_the_keys_of_the_front_matter_and_list_them_first() {
        use permark_core::format::{Contributions as C, Field, FieldValue, FormatRegistry};
        let registry = FormatRegistry::build(&{
            let mut all = C::default();
            all.extend(plugin().contributions());
            all
        });
        let fields = |keys: &[&str]| -> Vec<Field> {
            keys.iter()
                .map(|key| Field {
                    key: (*key).to_owned(),
                    value: FieldValue::Text {
                        text: "x".to_owned(),
                    },
                })
                .collect()
        };
        let keys =
            |fields: &[Field]| -> Vec<String> { fields.iter().map(|f| f.key.clone()).collect() };
        // a model card
        let mut model = fields(&[
            "title",
            "license",
            "pipeline_tag",
            "tags",
            "base_model",
            "note",
        ]);
        assert_eq!(
            registry.role_of_front_matter(&mut model),
            Some("model-card")
        );
        assert_eq!(
            keys(&model),
            [
                "base_model",
                "pipeline_tag",
                "license",
                "tags",
                "title",
                "note"
            ]
        );
        // a dataset card, a nested key counts by its first part
        let mut dataset = fields(&["license", "dataset_info.features", "size_categories"]);
        assert_eq!(
            registry.role_of_front_matter(&mut dataset),
            Some("dataset-card")
        );
        assert_eq!(
            keys(&dataset)[..2],
            ["dataset_info.features", "size_categories"]
        );
        // a dataset key wins over a model key
        let mut both = fields(&["pipeline_tag", "task_categories"]);
        assert_eq!(
            registry.role_of_front_matter(&mut both),
            Some("dataset-card")
        );
        // license, language and tags alone decide nothing; no keys, no role
        let mut plain = fields(&["title", "license", "language", "tags", "date"]);
        assert_eq!(registry.role_of_front_matter(&mut plain), None);
        assert_eq!(
            keys(&plain),
            ["title", "license", "language", "tags", "date"]
        );
        assert_eq!(registry.role_of_front_matter(&mut []), None);
        // a key that only starts like one
        let mut near = fields(&["datasets_used", "pipeline"]);
        assert_eq!(registry.role_of_front_matter(&mut near), None);
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
