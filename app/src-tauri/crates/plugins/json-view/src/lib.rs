//! Plugin `json-view`: JSON files as a read-only collapsible tree (Source for the text).

use std::sync::Arc;

use permark_core::format::Contributions;
use permark_core::format::TextViewer;
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "json-view",
        PluginKind::Format,
        Contributions::default().formats(vec![Arc::new(TextViewer::json())]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_a_valid_app_plugin() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "json-view");
        assert_eq!(manifest.kind, PluginKind::Format);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }
}
