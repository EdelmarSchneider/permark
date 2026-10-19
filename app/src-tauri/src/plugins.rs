//! The plugins compiled into this build: one Cargo feature per plugin crate (`plugin-<id>`, all default).
//! Order matters for ties between formats: `media` before `xml-view` (an SVG is a picture), `text-view` last (weakest).

use std::sync::Arc;

use permark_core::plugins::{Plugin, PluginRegistry};

/// Every plugin of this build, in registration order.
#[allow(clippy::vec_init_then_push)] // each push is behind its feature
pub fn all() -> Vec<Arc<dyn Plugin>> {
    #[allow(unused_mut)]
    let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();
    #[cfg(feature = "plugin-git")]
    plugins.push(permark_plugin_git::plugin());
    #[cfg(feature = "plugin-live-feed")]
    plugins.push(permark_plugin_live_feed::plugin());
    #[cfg(feature = "plugin-follow")]
    plugins.push(permark_plugin_follow::plugin());
    #[cfg(feature = "plugin-unread")]
    plugins.push(permark_plugin_unread::plugin());
    #[cfg(feature = "plugin-review")]
    plugins.push(permark_plugin_review::plugin());
    #[cfg(feature = "plugin-tags")]
    plugins.push(permark_plugin_tags::plugin());
    #[cfg(feature = "plugin-quiet")]
    plugins.push(permark_plugin_quiet::plugin());
    #[cfg(feature = "plugin-comments")]
    plugins.push(permark_plugin_comments::plugin());
    #[cfg(feature = "plugin-broken-links")]
    plugins.push(permark_plugin_broken_links::plugin());
    #[cfg(feature = "plugin-md-family")]
    plugins.push(permark_plugin_md_family::plugin());
    #[cfg(feature = "plugin-word-count")]
    plugins.push(permark_plugin_word_count::plugin());
    #[cfg(feature = "plugin-prompt-blocks")]
    plugins.push(permark_plugin_prompt_blocks::plugin());
    #[cfg(feature = "plugin-frontmatter-card")]
    plugins.push(permark_plugin_frontmatter_card::plugin());
    #[cfg(feature = "plugin-highlight")]
    plugins.push(permark_plugin_highlight::plugin());
    #[cfg(feature = "plugin-math")]
    plugins.push(permark_plugin_math::plugin());
    #[cfg(feature = "plugin-mermaid")]
    plugins.push(permark_plugin_mermaid::plugin());
    #[cfg(feature = "plugin-plantuml")]
    plugins.push(permark_plugin_plantuml::plugin());
    #[cfg(feature = "plugin-media")]
    plugins.push(permark_plugin_media::plugin());
    #[cfg(feature = "plugin-json-view")]
    plugins.push(permark_plugin_json_view::plugin());
    #[cfg(feature = "plugin-xml-view")]
    plugins.push(permark_plugin_xml_view::plugin());
    #[cfg(feature = "plugin-text-view")]
    plugins.push(permark_plugin_text_view::plugin());
    plugins
}

/// The registry of this build. A broken plugin set is a build bug: loud in debug, survivable in release.
pub fn registry() -> PluginRegistry {
    match PluginRegistry::new(all()) {
        Ok(registry) => registry,
        Err(error) => {
            debug_assert!(false, "plugin registry: {error}");
            eprintln!("PerMark: plugins disabled: {error}");
            PluginRegistry::empty()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use permark_core::format::{Body, FormatId, FormatRegistry};
    use permark_core::plugins::PluginId;

    use super::*;

    fn registry_of_all() -> PluginRegistry {
        PluginRegistry::new(all()).expect("the plugin set is valid")
    }

    fn on(ids: &[&str]) -> BTreeSet<PluginId> {
        ids.iter().filter_map(|i| PluginId::parse(i)).collect()
    }

    #[test]
    fn the_whole_set_is_valid_and_complete() {
        let registry = registry_of_all();
        let ids: Vec<_> = registry.ordered().map(|m| m.id.to_string()).collect();
        for expected in [
            "git",
            "live-feed",
            "follow",
            "unread",
            "review",
            "tags",
            "quiet",
            "comments",
            "broken-links",
            "md-family",
            "prompt-blocks",
            "frontmatter-card",
            "highlight",
            "math",
            "mermaid",
            "plantuml",
            "media",
            "json-view",
            "xml-view",
            "text-view",
        ] {
            assert!(
                ids.iter().any(|i| i == expected),
                "{expected} missing in {ids:?}"
            );
        }
    }

    #[test]
    fn formats_follow_the_plugins_that_are_on() {
        let registry = registry_of_all();
        let all_on: BTreeSet<PluginId> = registry.ordered().map(|m| m.id.clone()).collect();
        let formats = FormatRegistry::build(&registry.contributions(&all_on));
        let id = |path: &str| formats.pick(path, b"").map(|f| f.id());
        assert_eq!(id("a.md"), Some(FormatId::MARKDOWN));
        assert_eq!(id("AGENTS.md"), Some(FormatId::MARKDOWN));
        assert_eq!(id("rules/x.mdc"), Some(FormatId::MARKDOWN));
        assert_eq!(id("a.json"), Some(FormatId::JSON));
        assert_eq!(id("a.xml"), Some(FormatId::XML));
        assert_eq!(id("logo.svg"), Some(FormatId::MEDIA), "an SVG is a picture");
        assert_eq!(id("clip.mp4"), Some(FormatId::MEDIA));
        assert_eq!(id("notes.txt"), Some(FormatId::TEXT));
        assert_eq!(formats.role_of("CLAUDE.md"), Some("claude"));
        assert!(formats.features().prompt_blocks.is_some());
        assert!(formats.features().front_matter_card.is_some());
        // each switched off: it leaves nothing behind
        let without = |off: &str| {
            let mut set = all_on.clone();
            set.remove(&PluginId::parse(off).unwrap());
            FormatRegistry::build(&registry.contributions(&set))
        };
        assert!(without("prompt-blocks").features().prompt_blocks.is_none());
        assert!(without("frontmatter-card")
            .features()
            .front_matter_card
            .is_none());
        assert_eq!(without("md-family").role_of("AGENTS.md"), None);
        assert_eq!(
            without("json-view").pick("a.json", b"").map(|f| f.id()),
            Some(FormatId::TEXT)
        );
        assert!(without("text-view").pick("a.txt", b"").is_none());
        let none = FormatRegistry::build(&registry.contributions(&on(&[])));
        assert!(none.pick("a.json", b"").is_none());
        assert!(none.pick("a.md", b"").is_some());
    }

    #[test]
    fn a_full_render_through_the_real_plugins() {
        use permark_core::document::{DocScope, Documents};
        use permark_core::events::{CoreEvent, EventSink};
        use permark_core::paths::{Root, Vault};

        struct Quiet;
        impl EventSink for Quiet {
            fn emit(&self, _event: CoreEvent) {}
        }

        let registry = registry_of_all();
        let all_on: BTreeSet<PluginId> = registry.ordered().map(|m| m.id.clone()).collect();
        let formats = FormatRegistry::build(&registry.contributions(&all_on));
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("CLAUDE.md"),
            "---\ntitle: Rules\ntags: [a, b]\n---\n# Rules\n\n<context>\nHello {{name}}\n</context>\n",
        )
        .unwrap();
        let docs = Documents::for_project(
            DocScope::Project(Vault::new(Root::open(dir.path()).unwrap())),
            Arc::new(Quiet),
        );
        let view = docs.open("w", "CLAUDE.md", &[], &formats).unwrap();
        assert_eq!(view.role.as_deref(), Some("claude"));
        let Body::Markdown {
            blocks,
            front_matter,
            outline,
        } = view.body
        else {
            panic!("not markdown")
        };
        let front = front_matter.expect("front matter card on");
        assert_eq!(front.fields.len(), 2);
        assert_eq!(outline[0].label, "Rules");
        let html: String = blocks.iter().filter_map(|b| b.html.clone()).collect();
        assert!(
            html.contains("prompt-block") && html.contains("tvar"),
            "{html}"
        );
    }

    /// Renders the fixture documents (`src/api/fixtures/`) with every plugin on and writes the views the mock backend
    /// (`dev:mock`, UI tests) serves: `src/api/generated/mock-documents.json`. `cargo test` regenerates it; CI fails when
    /// regeneration changes the file.
    #[test]
    fn export_mock_documents() {
        use permark_core::document::{DocScope, Documents};
        use permark_core::events::{CoreEvent, EventSink};
        use permark_core::paths::{Root, Vault};

        struct Quiet;
        impl EventSink for Quiet {
            fn emit(&self, _event: CoreEvent) {}
        }

        let generated = std::path::Path::new(env!("TS_RS_EXPORT_DIR"));
        let fixtures = generated.parent().unwrap().join("fixtures");
        let registry = registry_of_all();
        let all_on: BTreeSet<PluginId> = registry.ordered().map(|m| m.id.clone()).collect();
        let formats = FormatRegistry::build(&registry.contributions(&all_on));
        // the project the documents live in: the same paths as the mock project
        let project = tempfile::tempdir().unwrap();
        let place = |path: &str, text: &[u8]| {
            let abs = project.path().join(path);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(abs, text).unwrap();
        };
        let documents = [
            ("README.md", "README.md"),
            ("docs/guide.md", "guide.md"),
            ("notes/ideas.md", "ideas.md"),
            ("CLAUDE.md", "claude-rules.md"),
        ];
        for (path, file) in documents {
            place(path, &std::fs::read(fixtures.join(file)).unwrap());
        }
        place("src/main.rs", b"fn main() {}");
        let docs = Documents::for_project(
            DocScope::Project(Vault::new(Root::open(project.path()).unwrap())),
            Arc::new(Quiet),
        );
        let mut views = serde_json::Map::new();
        for (path, _) in documents {
            let mut view = docs.open("mock", path, &[], &formats).unwrap();
            // the time of a file is not part of the fixture (the generated file must not change with the clock)
            view.mtime_ms = 0;
            views.insert(path.to_owned(), serde_json::to_value(view).unwrap());
        }
        let text = format!("{}\n", serde_json::to_string_pretty(&views).unwrap());
        let target = generated.join("mock-documents.json");
        if std::fs::read_to_string(&target).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(target, text).unwrap();
        }
        // what the editors start from (mock `edit_source`)
        let editor = permark_core::edit::Editor {
            scope: docs.scope(),
            formats: &formats,
            own: None,
        };
        let mut sources = serde_json::Map::new();
        for (path, _) in documents {
            let source = editor.source(path).unwrap();
            sources.insert(path.to_owned(), serde_json::to_value(source).unwrap());
        }
        let text = format!("{}\n", serde_json::to_string_pretty(&sources).unwrap());
        let target = generated.join("mock-edit-sources.json");
        if std::fs::read_to_string(&target).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(target, text).unwrap();
        }
    }
}
