//! `.trailmark/config.toml`: project settings that are safe to commit and share.

use serde::{Deserialize, Serialize};

use crate::git::BaselineKind;

pub const DEFAULT_REF_TEMPLATE: &str = "@{path}";

/// Written by "Initialize TrailMark in this project"; every key is optional.
pub const DEFAULT_CONFIG_TOML: &str = "\
# TrailMark project settings. Shared with your team: safe to commit.
# Everything here is optional; the commented values are the defaults.

# How \"Copy reference\" writes a path. {path} is the file path relative to the project folder.
# ref_template = \"@{path}\"

# What the file badges compare against: \"lastVisit\" (the project as you last left it) or \"head\".
# baseline = \"lastVisit\"

# Extra gitignore-style patterns hidden from the live feed and from search.
# ignore = [\"drafts/\", \"*.generated.md\"]

# Which files the Explorer shows by default (each person can change it for themselves).
# [tree]
# show = \"docs-and-assets\"   # \"documents\", \"docs-and-assets\", \"everything\" or \"custom\"
# include = [\"*.sql\"]         # also show these files (gitignore-style patterns)
# exclude = [\"archive/**\"]    # hide these files
";

/// What the Explorer shows: the presets of the filter in its header. `Custom` starts from nothing, so only
/// the `include` patterns of the project are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TreeShow {
    Documents,
    #[default]
    DocsAndAssets,
    Everything,
    Custom,
}

/// The `[tree]` table: the project's default for the Explorer filter. Each person's own choice wins.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeConfig {
    /// `None` when the project does not pick a preset (the app's default then applies).
    pub show: Option<TreeShow>,
    /// Patterns for files to show in addition to the preset.
    pub include: Vec<String>,
    /// Patterns for files to hide even when the preset shows them.
    pub exclude: Vec<String>,
}

/// Parsed `config.toml`. Missing keys take their defaults.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// `None` when the project does not set one (the user's own setting then applies).
    pub ref_template: Option<String>,
    pub baseline: BaselineKind,
    pub ignore: Vec<String>,
    /// `None` when the file has no `[tree]` table.
    pub tree: Option<TreeConfig>,
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_comment_only_files_give_defaults() {
        assert_eq!(Config::parse("").expect("parse"), Config::default());
        assert_eq!(
            Config::parse(DEFAULT_CONFIG_TOML).expect("parse"),
            Config::default()
        );
        let c = Config::default();
        assert_eq!(c.ref_template, None);
        assert_eq!(c.baseline, BaselineKind::LastVisit);
        assert_eq!(c.ignore.len(), 0);
        assert_eq!(c.tree, None);
    }

    #[test]
    fn keys_are_read_with_serde_defaults_for_the_rest() {
        let c = Config::parse("ref_template = \"see {path}\"\nbaseline = \"head\"\nignore = [\"drafts/\", \"*.tmp.md\"]\n")
            .expect("parse");
        assert_eq!(c.ref_template.as_deref(), Some("see {path}"));
        assert_eq!(c.baseline, BaselineKind::Head);
        assert_eq!(c.ignore, ["drafts/", "*.tmp.md"]);

        let c = Config::parse("ignore = [\"x/\"]").expect("parse");
        assert_eq!(c.ref_template, None);
        assert_eq!(c.baseline, BaselineKind::LastVisit);
    }

    #[test]
    fn baseline_accepts_snake_case_too() {
        let c = Config::parse("baseline = \"last_visit\"").expect("parse");
        assert_eq!(c.baseline, BaselineKind::LastVisit);
    }

    #[test]
    fn unknown_keys_are_tolerated_and_bad_values_are_errors() {
        assert!(Config::parse("future_key = 1").is_ok());
        assert!(Config::parse("baseline = \"nonsense\"").is_err());
        assert!(Config::parse("ignore = 5").is_err());
        assert!(Config::parse("not toml at all [[[").is_err());
    }

    #[test]
    fn the_tree_table_gives_the_explorer_defaults() {
        let c = Config::parse(
            "[tree]
show = \"docs-and-assets\"
include = [\"*.sql\"]
exclude = [\"archive/**\", \"*.tmp\"]
",
        )
        .expect("parse");
        let tree = c.tree.expect("tree table");
        assert_eq!(tree.show, Some(TreeShow::DocsAndAssets));
        assert_eq!(tree.include, ["*.sql"]);
        assert_eq!(tree.exclude, ["archive/**", "*.tmp"]);
    }

    #[test]
    fn every_preset_name_is_accepted_and_keys_are_optional() {
        for (name, show) in [
            ("documents", TreeShow::Documents),
            ("docs-and-assets", TreeShow::DocsAndAssets),
            ("everything", TreeShow::Everything),
            ("custom", TreeShow::Custom),
        ] {
            let c = Config::parse(&format!(
                "[tree]
show = \"{name}\""
            ))
            .expect("parse");
            assert_eq!(c.tree.expect("tree").show, Some(show));
        }
        let c = Config::parse(
            "[tree]
exclude = [\"x/\"]",
        )
        .expect("parse");
        let tree = c.tree.expect("tree");
        assert_eq!(tree.show, None);
        assert!(tree.include.is_empty());
        assert_eq!(tree.exclude, ["x/"]);
        // an empty table is still "the project has a tree section"
        assert_eq!(
            Config::parse("[tree]").expect("parse").tree,
            Some(TreeConfig::default())
        );
    }

    #[test]
    fn bad_tree_values_are_errors_like_the_other_keys() {
        assert!(Config::parse(
            "[tree]
show = \"all\""
        )
        .is_err());
        assert!(Config::parse(
            "[tree]
include = \"*.sql\""
        )
        .is_err());
        assert!(Config::parse("tree = 3").is_err());
    }

    #[test]
    fn the_template_documents_the_tree_table_in_comments_only() {
        assert!(DEFAULT_CONFIG_TOML.contains("# [tree]"));
        assert!(DEFAULT_CONFIG_TOML.contains("# show = "));
        assert_eq!(
            Config::parse(DEFAULT_CONFIG_TOML).expect("parse").tree,
            None
        );
    }
}
