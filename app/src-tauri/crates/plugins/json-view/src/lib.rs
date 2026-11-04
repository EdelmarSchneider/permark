//! Plugin `json-view`: JSON as a read-only collapsible tree, and JSON Lines as one record per line.

use std::sync::Arc;

use permark_core::format::Contributions;
use permark_core::format::TextViewer;
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "json-view",
        PluginKind::Format,
        Contributions::default().formats(vec![
            Arc::new(TextViewer::json()),
            Arc::new(TextViewer::json_lines()),
        ]),
    )
}

#[cfg(test)]
mod tests {
    use permark_core::format::{Claim, FormatId};

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

    #[test]
    fn contributes_json_and_json_lines() {
        let formats = plugin().contributions().formats;
        let ids: Vec<_> = formats.iter().map(|format| format.id()).collect();
        assert_eq!(ids, [FormatId::JSON, FormatId::JSONL]);
        let lines = formats.get(1).expect("json lines");
        assert_eq!(
            lines.claims("session.jsonl", Some("jsonl"), b""),
            Claim::Strong
        );
        assert_eq!(
            lines.claims("session.ndjson", Some("ndjson"), b""),
            Claim::Strong
        );
        assert_eq!(
            lines.claims("session.jsonlines", Some("jsonlines"), b""),
            Claim::Strong
        );
        assert_eq!(lines.claims("session.json", Some("json"), b""), Claim::No);
        assert_eq!(
            formats
                .first()
                .expect("json")
                .claims("session.jsonl", Some("jsonl"), b""),
            Claim::No
        );
    }
}
