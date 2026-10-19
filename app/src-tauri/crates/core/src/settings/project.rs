//! `.permark/config.toml`: project settings shared with the team. Read only when `.permark/` is a real folder.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use toml_edit::{DocumentMut, Item};
use ts_rs::TS;

use super::{plugin_tables, read_doc, RawTable};
use crate::paths::Root;
use crate::plugins::PluginId;
use crate::state::PERMARK_DIR;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TreeShow {
    Documents,
    DocsAndAssets,
    Everything,
    Custom,
}

impl TreeShow {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "documents" => Some(Self::Documents),
            "docs-and-assets" => Some(Self::DocsAndAssets),
            "everything" => Some(Self::Everything),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// `[tree]`: the project's default Explorer filter (each person's own choice wins).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, TS)]
#[ts(export)]
pub struct TreeConfig {
    pub show: Option<TreeShow>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectConfig {
    pub tree: TreeConfig,
    /// Extra gitignore-style patterns hidden from feed + search.
    pub ignore: Vec<String>,
    /// `[agent] deny`: permission groups this project never allows (narrowing only).
    pub agent_deny: BTreeSet<String>,
    /// `[plugins.<id>] enabled = false` (a project may only disable).
    pub plugins_off: BTreeSet<PluginId>,
    pub plugin_tables: BTreeMap<PluginId, RawTable>,
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

impl ProjectConfig {
    /// Reads `<root>/.permark/config.toml`; defaults when `.permark/` is no real folder | file missing | invalid.
    #[must_use]
    pub fn load(root: &Root) -> Self {
        let permark = root.path().join(PERMARK_DIR);
        let real = std::fs::symlink_metadata(&permark).is_ok_and(|m| m.file_type().is_dir());
        if !real {
            return Self::default();
        }
        read_doc(&permark.join("config.toml"))
            .map_or_else(Self::default, |doc| Self::from_doc(&doc))
    }

    /// Lenient: each key on its own; wrong types read as defaults.
    #[must_use]
    pub fn from_doc(doc: &DocumentMut) -> Self {
        let tree = doc.get("tree").and_then(Item::as_table_like);
        let tree = TreeConfig {
            show: tree
                .and_then(|t| t.get("show"))
                .and_then(Item::as_str)
                .and_then(TreeShow::parse),
            include: strings(tree.and_then(|t| t.get("include"))),
            exclude: strings(tree.and_then(|t| t.get("exclude"))),
        };
        let agent = doc.get("agent").and_then(Item::as_table_like);
        let plugin_tables = plugin_tables(doc);
        let plugins_off = plugin_tables
            .iter()
            .filter(|(_, table)| table.0.get("enabled").and_then(Item::as_bool) == Some(false))
            .map(|(id, _)| id.clone())
            .collect();
        Self {
            tree,
            ignore: strings(doc.get("ignore")),
            agent_deny: strings(agent.and_then(|t| t.get("deny")))
                .into_iter()
                .collect(),
            plugins_off,
            plugin_tables,
        }
    }
}
