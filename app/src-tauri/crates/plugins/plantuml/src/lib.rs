//! Plugin `plantuml`: PlantUML diagrams drawn in the page (UI only).

use std::sync::Arc;

use permark_core::format::Contributions;
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app("plantuml", PluginKind::Embed, Contributions::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_a_valid_app_plugin() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "plantuml");
        assert_eq!(manifest.kind, PluginKind::Embed);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }
}
