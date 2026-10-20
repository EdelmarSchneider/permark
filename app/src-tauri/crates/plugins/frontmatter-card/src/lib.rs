//! Plugin `frontmatter-card`: the properties card above a Markdown document. Core cuts the front matter out; this
//! plugin turns its text into fields (YAML subset | TOML). Off: core shows the block as a code block.

mod toml;
mod yaml;

use permark_core::format::{Contributions, Field, FrontMatterSyntax, MarkdownFeature};
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};
use std::sync::Arc;

/// Most fields a card shows.
pub const MAX_FIELDS: usize = 200;
/// Longest value kept (characters).
pub const MAX_VALUE: usize = 2000;

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "frontmatter-card",
        PluginKind::Embed,
        Contributions::default().markdown(vec![MarkdownFeature::FrontMatterCard(parse)]),
    )
}

/// The card fields of a front matter text.
#[must_use]
pub fn parse(syntax: FrontMatterSyntax, raw: &str) -> Vec<Field> {
    let mut fields = match syntax {
        FrontMatterSyntax::Yaml => yaml::parse(raw),
        FrontMatterSyntax::Toml => toml::parse(raw),
    };
    fields.truncate(MAX_FIELDS);
    fields
}

pub(crate) fn clip(text: &str) -> String {
    text.chars().take(MAX_VALUE).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use permark_core::format::FieldValue;

    #[test]
    fn manifest_is_a_valid_app_embed() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "frontmatter-card");
        assert_eq!(manifest.kind, PluginKind::Embed);
        assert!(manifest.default_enabled);
    }

    #[test]
    fn contributes_the_parser() {
        let contributions = plugin().contributions();
        assert!(matches!(
            contributions.markdown.as_slice(),
            [MarkdownFeature::FrontMatterCard(_)]
        ));
    }

    #[test]
    fn never_more_than_the_limits() {
        let many = "k: v\n".repeat(500);
        assert_eq!(parse(FrontMatterSyntax::Yaml, &many).len(), MAX_FIELDS);
        let long = format!("k: {}\n", "x".repeat(10_000));
        let fields = parse(FrontMatterSyntax::Yaml, &long);
        assert!(
            matches!(&fields[0].value, FieldValue::Text { text } if text.chars().count() == MAX_VALUE)
        );
    }
}
