//! `.trailmark/config.toml`: project settings that are safe to commit and share.

use serde::Deserialize;

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
";

/// Parsed `config.toml`. Missing keys take their defaults.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// `None` when the project does not set one (the user's own setting then applies).
    pub ref_template: Option<String>,
    pub baseline: BaselineKind,
    pub ignore: Vec<String>,
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
}
