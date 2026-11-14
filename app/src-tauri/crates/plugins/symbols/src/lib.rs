//! Plugin `symbols`: a panel of symbols and emoji beside the document. The panel, its search and the insert at the
//! caret live in the frontend (`app/src/plugins/symbols/`); the groups come from the core settings payload, so this
//! crate only declares the plugin (its switch in the Plugins page).

use std::sync::Arc;

use permark_core::format::Contributions;
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app("symbols", PluginKind::Feature, Contributions::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_a_valid_app_plugin() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "symbols");
        assert_eq!(manifest.kind, PluginKind::Feature);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }
}
