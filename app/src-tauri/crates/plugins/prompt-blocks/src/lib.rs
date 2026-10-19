//! Plugin `prompt-blocks`: Prompt tags as foldable blocks, `{{name}}` marks and `@path` references in Markdown.

use std::sync::Arc;

use permark_core::format::Contributions;
use permark_core::format::{MarkdownFeature, PromptBlocks, DEFAULT_PROMPT_TAGS};
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "prompt-blocks",
        PluginKind::Embed,
        Contributions::default().markdown(vec![MarkdownFeature::PromptBlocks(PromptBlocks {
            tags: DEFAULT_PROMPT_TAGS,
        })]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_a_valid_app_plugin() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "prompt-blocks");
        assert_eq!(manifest.kind, PluginKind::Embed);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }
}
