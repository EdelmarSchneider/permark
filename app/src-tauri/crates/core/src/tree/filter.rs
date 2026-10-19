//! The Explorer filter: which entries of the model a window shows. Pure over the model.

use std::collections::HashSet;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::model::{below_root, key_of, parent_of, Node, TreeModel};
use super::{Category, NodeKind, TreeNode};
use crate::settings::{TreeConfig, TreeShow};

/// Preset of the filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum FilterShow {
    Documents,
    DocsAndAssets,
    Everything,
    Custom,
}

/// One window's filter (kept in the project session; the project default comes from `[tree]`).
#[allow(clippy::struct_excessive_bools)] // independent switches of a settings form, not a state machine
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TreeFilter {
    pub show: FilterShow,
    /// Used by `Custom` (the presets bring their own).
    pub categories: Vec<Category>,
    /// `+pattern` shows, `-pattern` hides (gitignore syntax), separated by white space or commas.
    pub patterns: String,
    pub assets_near_docs_only: bool,
    pub hide_empty_dirs: bool,
    pub show_hidden: bool,
    pub show_ignored: bool,
}

impl Default for TreeFilter {
    fn default() -> Self {
        Self {
            show: FilterShow::DocsAndAssets,
            categories: preset_categories(FilterShow::DocsAndAssets),
            patterns: String::new(),
            assets_near_docs_only: true,
            hide_empty_dirs: true,
            show_hidden: false,
            show_ignored: false,
        }
    }
}

fn preset_categories(show: FilterShow) -> Vec<Category> {
    match show {
        FilterShow::Documents => vec![Category::Markdown, Category::Text],
        FilterShow::DocsAndAssets => vec![
            Category::Markdown,
            Category::Text,
            Category::Config,
            Category::Data,
            Category::Image,
            Category::Audio,
            Category::Video,
        ],
        FilterShow::Everything => Category::ALL.to_vec(),
        FilterShow::Custom => Vec::new(),
    }
}

impl TreeFilter {
    /// The filter a project asks for in `[tree]`. Patterns turn a preset into `Custom` with that preset's categories.
    #[must_use]
    pub fn from_config(config: &TreeConfig) -> Self {
        let show = match config.show {
            Some(TreeShow::Documents) => FilterShow::Documents,
            Some(TreeShow::Everything) => FilterShow::Everything,
            Some(TreeShow::Custom) => FilterShow::Custom,
            Some(TreeShow::DocsAndAssets) | None => FilterShow::DocsAndAssets,
        };
        let patterns: Vec<String> = config
            .include
            .iter()
            .map(|p| format!("+{p}"))
            .chain(config.exclude.iter().map(|p| format!("-{p}")))
            .collect();
        let custom = show == FilterShow::Custom || !patterns.is_empty();
        Self {
            show: if custom { FilterShow::Custom } else { show },
            categories: preset_categories(show),
            patterns: patterns.join(" "),
            ..Self::default()
        }
    }

    /// Switching to `Custom` keeps what the previous preset showed.
    #[must_use]
    pub fn with_show(mut self, show: FilterShow) -> Self {
        if show == FilterShow::Custom && self.show != FilterShow::Custom {
            self.categories = preset_categories(self.show);
        }
        self.show = show;
        self
    }

    /// Categories in force.
    #[must_use]
    pub fn active_categories(&self) -> Vec<Category> {
        match self.show {
            FilterShow::Custom => self.categories.clone(),
            other => preset_categories(other),
        }
    }
}

/// Folder names that hold pictures, sound or video by convention.
const ASSET_FOLDERS: &[&str] = &[
    "assets",
    "images",
    "img",
    "media",
    "attachments",
    "screenshots",
    "figures",
    "diagrams",
    "pictures",
    "audio",
    "sounds",
    "sound",
    "music",
    "recordings",
    "video",
    "videos",
    "movies",
    "clips",
    "screencasts",
];

/// Most pattern entries read from the field.
const MAX_PATTERNS: usize = 200;

/// Splits the pattern field at white space and at commas outside `{...}`.
#[must_use]
pub fn split_patterns(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth = 0_usize;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c.is_whitespace() || (c == ',' && depth == 0) {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens.truncate(MAX_PATTERNS);
    tokens
}

struct Rule {
    include: bool,
    matcher: Gitignore,
}

/// A filter ready to run: patterns compiled, categories as a set.
pub(crate) struct Compiled {
    pub filter: TreeFilter,
    categories: HashSet<Category>,
    rules: Vec<Rule>,
    pub invalid: Vec<String>,
}

impl Compiled {
    pub(crate) fn new(filter: TreeFilter) -> Self {
        let mut rules = Vec::new();
        let mut invalid = Vec::new();
        if filter.show == FilterShow::Custom {
            for token in split_patterns(&filter.patterns) {
                let include = !token.starts_with('-');
                let body = token.trim_start_matches(['+', '-']);
                let mut builder = GitignoreBuilder::new("");
                // patterns match ignoring case, on every system
                let _ = builder.case_insensitive(true);
                let added = if body.is_empty() || body.starts_with(['!', '#']) {
                    None
                } else {
                    builder.add_line(None, body).ok().map(|_| ())
                };
                match (added, builder.build()) {
                    (Some(()), Ok(matcher)) if !matcher.is_empty() => {
                        rules.push(Rule { include, matcher });
                    }
                    _ => invalid.push(token),
                }
            }
        }
        Self {
            categories: filter.active_categories().into_iter().collect(),
            filter,
            rules,
            invalid,
        }
    }

    /// Last matching rule wins; `None` when no rule matches.
    fn rule_says(&self, rel: &str) -> Option<bool> {
        let mut says = None;
        for rule in &self.rules {
            if rule
                .matcher
                .matched_path_or_any_parents(rel, false)
                .is_ignore()
            {
                says = Some(rule.include);
            }
        }
        says
    }
}

/// What a filter shows of a model.
#[derive(Debug, Default)]
pub(crate) struct Visible {
    pub nodes: Vec<TreeNode>,
    pub total_files: u32,
    pub shown_files: u32,
}

/// Facts shared by one pass.
struct Pass<'a> {
    model: &'a TreeModel,
    c: &'a Compiled,
    doc_folders: HashSet<&'a str>,
    keep: Option<String>,
    keep_ancestors: HashSet<String>,
    kept_dirs: &'a HashSet<String>,
    /// Keys of the files some document links to.
    linked: &'a HashSet<String>,
    out: Visible,
}

/// Result of visiting a folder: something kept inside, a file kept inside.
#[derive(Clone, Copy, Default)]
struct Inside {
    any: bool,
    file: bool,
}

fn dto(node: &Node, forced: bool) -> TreeNode {
    TreeNode {
        path: node.path.clone(),
        name: node.name.clone(),
        kind: node.kind,
        hidden: node.hidden,
        ignored: node.ignored,
        forced,
    }
}

/// Runs `c` over `model`. `keep`: the open document, shown even when the filter hides it. `kept_dirs`: keys of
/// folders made in the Explorer, shown while empty.
pub(crate) fn apply(
    model: &TreeModel,
    c: &Compiled,
    keep: Option<&str>,
    kept_dirs: &HashSet<String>,
    linked: &HashSet<String>,
) -> Visible {
    let doc_folders = model_doc_folders(model, c);
    let mut keep_ancestors = HashSet::new();
    if let Some(path) = keep {
        let mut at = parent_of(path);
        while !at.is_empty() {
            keep_ancestors.insert(key_of(at));
            at = parent_of(at);
        }
    }
    let mut pass = Pass {
        model,
        c,
        doc_folders,
        keep: keep.map(key_of),
        keep_ancestors,
        kept_dirs,
        linked,
        out: Visible::default(),
    };
    for root in &model.roots {
        pass.visit(&key_of(&root.prefix), false, false);
    }
    pass.out
}

/// Keys of the folders that directly hold a document (only when "near documents" is in force).
fn model_doc_folders<'a>(model: &'a TreeModel, c: &Compiled) -> HashSet<&'a str> {
    let mut folders = HashSet::new();
    if !(c.filter.assets_near_docs_only && c.filter.show != FilterShow::Documents) {
        return folders;
    }
    for root in &model.roots {
        collect_doc_folders(model, &key_of(&root.prefix), &mut folders);
    }
    folders
}

fn collect_doc_folders<'a>(model: &'a TreeModel, folder: &str, out: &mut HashSet<&'a str>) {
    for child in model.children_of(folder) {
        let Some(node) = model.node(child) else {
            continue;
        };
        match node.kind {
            NodeKind::File { category } if category.is_document() => {
                out.insert(node.parent_key());
            }
            NodeKind::File { .. } => {}
            NodeKind::Dir { .. } => collect_doc_folders(model, child, out),
        }
    }
}

impl Pass<'_> {
    /// Appends what the folder `folder_key` shows; returns what was kept inside.
    fn visit(&mut self, folder_key: &str, in_hidden: bool, in_asset_folder: bool) -> Inside {
        let mut inside = Inside::default();
        let model = self.model;
        for child in model.children_of(folder_key) {
            let Some(node) = model.node(child) else {
                continue;
            };
            match node.kind {
                NodeKind::File { category } => {
                    let hidden_path = in_hidden || node.hidden;
                    let load_visible = (self.c.filter.show_hidden || !hidden_path)
                        && (self.c.filter.show_ignored || !node.ignored);
                    let shown = self.file_shown(node, category, hidden_path, in_asset_folder);
                    if load_visible || shown {
                        self.out.total_files += 1;
                    }
                    if shown {
                        self.out.shown_files += 1;
                        self.out.nodes.push(dto(node, false));
                        inside = Inside {
                            any: true,
                            file: true,
                        };
                    } else if (!node.ignored || self.c.filter.show_ignored)
                        && self.keep.as_deref() == Some(node.key())
                    {
                        self.out.nodes.push(dto(node, true));
                        inside = Inside {
                            any: true,
                            file: true,
                        };
                    }
                }
                NodeKind::Dir { unlisted } => {
                    if node.ignored && !self.c.filter.show_ignored {
                        continue;
                    }
                    let mark = self.out.nodes.len();
                    self.out.nodes.push(dto(node, false));
                    let hidden_path = in_hidden || node.hidden;
                    let asset_folder = in_asset_folder
                        || ASSET_FOLDERS.contains(&node.name.to_lowercase().as_str());
                    let below = if unlisted.is_none() {
                        self.visit(child, hidden_path, asset_folder)
                    } else {
                        Inside::default()
                    };
                    let keep_dir = if node.hidden && !self.c.filter.show_hidden {
                        // a hidden folder is listed for a file in it that is shown, not for being there
                        below.file
                    } else {
                        below.any
                            || !self.c.filter.hide_empty_dirs
                            || unlisted.is_some()
                            || self.kept_dirs.contains(node.key())
                            || self.keep_ancestors.contains(node.key())
                    };
                    if keep_dir {
                        inside.any = true;
                        inside.file |= below.file;
                    } else {
                        self.out.nodes.truncate(mark);
                    }
                }
            }
        }
        inside
    }

    fn file_shown(
        &self,
        node: &Node,
        category: Category,
        hidden_path: bool,
        in_asset_folder: bool,
    ) -> bool {
        // ignored files are shown on request only; patterns cannot reveal them
        if node.ignored && !self.c.filter.show_ignored {
            return false;
        }
        let f = &self.c.filter;
        let visible = f.show_hidden || !hidden_path;
        let by_category = if f.show == FilterShow::Everything {
            visible
        } else if !visible || !self.c.categories.contains(&category) {
            false
        } else if f.assets_near_docs_only && f.show != FilterShow::Documents && category.is_asset()
        {
            self.near_docs(node)
                || (category.is_media() && in_asset_folder)
                || self.linked.contains(node.key())
        } else {
            true
        };
        if self.c.rules.is_empty() {
            return by_category;
        }
        self.c
            .rule_says(below_root(&node.path))
            .unwrap_or(by_category)
    }

    /// A document sits in the file's own folder or the one above it (a root counts as its own folder only).
    fn near_docs(&self, node: &Node) -> bool {
        let own = node.parent_key();
        if self.doc_folders.contains(own) {
            return true;
        }
        // a root has no folder above that counts, and a top-level folder's parent is the root
        let Some(folder) = self.model.node(own) else {
            return false;
        };
        let above = folder.parent_key();
        self.model.node(above).is_some() && self.doc_folders.contains(above)
    }
}
