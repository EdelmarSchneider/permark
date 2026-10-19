//! Plugin `follow`: follow mode opens the document an agent is writing and never loses the reader's place. The work is
//! in the interface (it needs the window's state: what is open, whether the person is typing); the backend part is the
//! manifest, so the plugin can be switched, depends on `live-feed` (which tells what changed), and leaves nothing behind.

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::format::Contributions;
use permark_core::plugins::{Manifest, Plugin, PluginId, PluginKind, PluginScope, StaticPlugin};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::new(
        Manifest {
            id: PluginId::of("follow"),
            kind: PluginKind::Feature,
            scope: PluginScope::Project,
            requires: Requirement::PROJECT,
            depends_on: Cow::Owned(vec![PluginId::of("live-feed")]),
            default_enabled: true,
        },
        Contributions::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_needs_the_feed_and_a_project() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "follow");
        assert_eq!(manifest.depends_on.as_ref(), [PluginId::of("live-feed")]);
        assert_eq!(manifest.scope, PluginScope::Project);
        assert_eq!(manifest.requires, Requirement::PROJECT);
    }
}
