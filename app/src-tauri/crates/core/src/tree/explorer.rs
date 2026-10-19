//! `Explorer`: the tree service of a `ProjectCore`. Owns the model (built on first use), the per-window views and
//! the deltas sent to them.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ignore::gitignore::Gitignore;

use super::filter::{apply, Compiled, Visible};
use super::ignore::Ignorer;
use super::model::{key_of, RootCtx, TreeModel};
use super::{
    Added, NodeKind, Renamed, RootKind, RootNode, TreeChanged, TreeDelta, TreeError, TreeFilter,
    TreeNode, TreeSnapshot,
};
use crate::events::{CoreEvent, EventSink};
use crate::paths::{Vault, VaultPath};
use crate::settings::ProjectConfig;
use crate::watch::{ChangeKind, FsBatch, BULK_CHANGES};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One window's view of the tree: its filter and the list it was last told.
struct View {
    compiled: Compiled,
    keep: Option<String>,
    list: Vec<TreeNode>,
    total: u32,
    shown: u32,
    seq: u64,
}

impl View {
    fn snapshot(&self, roots: Vec<RootNode>) -> TreeSnapshot {
        TreeSnapshot {
            roots,
            nodes: self.list.clone(),
            filter: self.compiled.filter.clone(),
            seq: self.seq,
            invalid_patterns: self.compiled.invalid.clone(),
            total_files: self.total,
            shown_files: self.shown,
        }
    }
}

/// Difference between two lists: paths to drop, nodes to add (each after its predecessor in `new`).
pub(crate) fn diff(old: &[TreeNode], new: &[TreeNode]) -> (Vec<String>, Vec<Added>) {
    let old_map: HashMap<&str, &TreeNode> = old.iter().map(|n| (n.path.as_str(), n)).collect();
    let new_map: HashMap<&str, &TreeNode> = new.iter().map(|n| (n.path.as_str(), n)).collect();
    let removed = old
        .iter()
        .filter(|n| new_map.get(n.path.as_str()) != Some(n))
        .map(|n| n.path.clone())
        .collect();
    let added = new
        .iter()
        .enumerate()
        .filter(|(_, n)| old_map.get(n.path.as_str()) != Some(n))
        .map(|(at, n)| Added {
            node: n.clone(),
            after: at.checked_sub(1).map(|before| new[before].path.clone()),
        })
        .collect();
    (removed, added)
}

pub struct Explorer {
    roots: Vec<RootCtx>,
    model: Mutex<Option<TreeModel>>,
    views: Mutex<HashMap<String, View>>,
    /// Keys of folders made in the Explorer: shown while empty.
    kept_dirs: Mutex<HashSet<String>>,
    default_filter: TreeFilter,
    sink: Arc<dyn EventSink>,
}

impl std::fmt::Debug for Explorer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Explorer").finish_non_exhaustive()
    }
}

impl Explorer {
    /// `global`: the user's global excludes (None: none, as in tests).
    #[must_use]
    pub fn new(
        vault: &Vault,
        config: &ProjectConfig,
        sink: Arc<dyn EventSink>,
        global: Option<&Gitignore>,
    ) -> Self {
        let main = vault.main();
        let mut roots = vec![RootCtx {
            name: main.name(),
            kind: RootKind::Main,
            prefix: String::new(),
            abs: main.path().to_path_buf(),
            ignorer: Arc::new(Ignorer::new(main.path(), &config.ignore, global.cloned())),
        }];
        for worktree in vault.worktrees() {
            roots.push(RootCtx {
                name: worktree.name.clone(),
                kind: RootKind::Worktree,
                prefix: format!("{}{}", crate::paths::WORKTREE_PREFIX, worktree.name),
                abs: worktree.root.path().to_path_buf(),
                ignorer: Arc::new(Ignorer::new(
                    worktree.root.path(),
                    &config.ignore,
                    global.cloned(),
                )),
            });
        }
        Self {
            roots,
            model: Mutex::new(None),
            views: Mutex::new(HashMap::new()),
            kept_dirs: Mutex::new(HashSet::new()),
            default_filter: TreeFilter::from_config(&config.tree),
            sink,
        }
    }

    /// Ignore rules per root as the watch needs them: (folded root prefix, rules); the project folder first.
    #[must_use]
    pub fn ignorers(&self) -> Vec<(String, Arc<Ignorer>)> {
        self.roots
            .iter()
            .map(|r| (key_of(&r.prefix), Arc::clone(&r.ignorer)))
            .collect()
    }

    /// The default filter of the project (`[tree]`).
    #[must_use]
    pub fn default_filter(&self) -> &TreeFilter {
        &self.default_filter
    }

    fn model(&self) -> MutexGuard<'_, Option<TreeModel>> {
        let mut guard = lock(&self.model);
        if guard.is_none() {
            let mut model = TreeModel::new(self.roots.clone());
            model.rebuild();
            *guard = Some(model);
        }
        guard
    }

    fn root_nodes(&self) -> Vec<RootNode> {
        self.roots
            .iter()
            .map(|r| RootNode {
                name: r.name.clone(),
                kind: r.kind,
                path: r.prefix.clone(),
            })
            .collect()
    }

    fn visible(&self, model: &TreeModel, view: &View) -> Visible {
        apply(
            model,
            &view.compiled,
            view.keep.as_deref(),
            &lock(&self.kept_dirs),
        )
    }

    // ---- views -------------------------------------------------------------------------------------------------

    /// Registers (or replaces) the window's view and returns the whole list. `None`: the project's default filter.
    pub fn open_view(&self, window: &str, filter: Option<TreeFilter>) -> TreeSnapshot {
        let guard = self.model();
        let Some(model) = guard.as_ref() else {
            return TreeSnapshot::empty(self.root_nodes());
        };
        let mut views = lock(&self.views);
        let keep = views.get(window).and_then(|v| v.keep.clone());
        let seq = views.get(window).map_or(0, |v| v.seq + 1);
        let mut view = View {
            compiled: Compiled::new(filter.unwrap_or_else(|| self.default_filter.clone())),
            keep,
            list: Vec::new(),
            total: 0,
            shown: 0,
            seq,
        };
        let visible = self.visible(model, &view);
        view.list = visible.nodes;
        view.total = visible.total_files;
        view.shown = visible.shown_files;
        let snapshot = view.snapshot(self.root_nodes());
        views.insert(window.to_owned(), view);
        snapshot
    }

    /// Replaces the window's filter; returns the whole list (the one case a snapshot follows the first).
    ///
    /// # Errors
    /// `NoView`: the window has no tree open.
    pub fn set_filter(&self, window: &str, filter: TreeFilter) -> Result<TreeSnapshot, TreeError> {
        if !lock(&self.views).contains_key(window) {
            return Err(TreeError::NoView);
        }
        Ok(self.open_view(window, Some(filter)))
    }

    /// The window's view goes (window closed, workspace changed).
    pub fn close_view(&self, window: &str) {
        lock(&self.views).remove(window);
    }

    /// Open document of a window: shown even when the filter hides it.
    ///
    /// # Errors
    /// `NoView`.
    pub fn keep(&self, window: &str, path: Option<&str>) -> Result<(), TreeError> {
        let guard = self.model();
        let Some(model) = guard.as_ref() else {
            return Ok(());
        };
        let mut views = lock(&self.views);
        let view = views.get_mut(window).ok_or(TreeError::NoView)?;
        if view.keep.as_deref() == path {
            return Ok(());
        }
        view.keep = path.map(str::to_owned);
        let visible = self.visible(model, view);
        self.update_view(window, view, visible, &[], false);
        Ok(())
    }

    /// Lists a folder that was left unopened (large | ignored).
    ///
    /// # Errors
    /// `NotAFolder`: no such listed folder.
    pub fn open_folder(&self, path: &str) -> Result<(), TreeError> {
        let parsed = VaultPath::parse(path)?;
        let mut guard = self.model();
        let Some(model) = guard.as_mut() else {
            return Ok(());
        };
        if !model.open_folder(parsed.as_str()) {
            return Err(TreeError::NotAFolder(path.to_owned()));
        }
        self.refresh_views(model, &[], false);
        Ok(())
    }

    /// Folder made in the Explorer: stays visible while empty.
    pub fn keep_folder(&self, path: &str) {
        lock(&self.kept_dirs).insert(key_of(path));
    }

    // ---- changes -----------------------------------------------------------------------------------------------

    /// A watch batch: the model learns what the disk says, windows are told what changed for them.
    pub fn apply_batch(&self, batch: &FsBatch) {
        let mut guard = lock(&self.model);
        // not built yet: the first walk will see the disk as it is
        let Some(model) = guard.as_mut() else {
            return;
        };
        let mut changed = false;
        if batch.rescan || batch.changes.iter().any(|c| is_ignore_file(&c.path)) {
            model.rebuild();
            changed = true;
        } else {
            for change in &batch.changes {
                if let ChangeKind::Renamed { from } = &change.kind {
                    changed |= model.sync_path(from);
                }
                changed |= model.sync_path(&change.path);
            }
        }
        if !changed {
            return;
        }
        let renames: Vec<Renamed> = batch
            .changes
            .iter()
            .filter_map(|c| match &c.kind {
                ChangeKind::Renamed { from } => Some(Renamed {
                    from: from.clone(),
                    to: c.path.clone(),
                }),
                _ => None,
            })
            .collect();
        self.refresh_views(
            model,
            &renames,
            batch.bulk || batch.changes.len() > BULK_CHANGES,
        );
    }

    /// The app changed these paths itself (a move, a delete, a new file): the model syncs at once, without waiting for
    /// the watcher.
    pub fn sync_paths(&self, paths: &[String], renames: &[Renamed]) {
        let mut guard = self.model();
        let Some(model) = guard.as_mut() else {
            return;
        };
        let mut changed = false;
        for path in paths {
            changed |= model.sync_path(path);
        }
        if changed {
            self.refresh_views(model, renames, paths.len() > BULK_CHANGES);
        }
    }

    fn refresh_views(&self, model: &TreeModel, renames: &[Renamed], bulk: bool) {
        let mut views = lock(&self.views);
        for (window, view) in views.iter_mut() {
            let visible = self.visible(model, view);
            self.update_view(window, view, visible, renames, bulk);
        }
    }

    /// Moves a view to `visible` and tells its window the difference.
    fn update_view(
        &self,
        window: &str,
        view: &mut View,
        visible: Visible,
        renames: &[Renamed],
        bulk: bool,
    ) {
        let (removed, added) = diff(&view.list, &visible.nodes);
        let counts_changed = view.total != visible.total_files || view.shown != visible.shown_files;
        if removed.is_empty() && added.is_empty() && !counts_changed {
            return;
        }
        let old_paths: HashSet<&str> = view.list.iter().map(|n| n.path.as_str()).collect();
        let hints = renames
            .iter()
            .filter(|r| old_paths.contains(r.from.as_str()))
            .cloned()
            .collect();
        view.seq += 1;
        view.total = visible.total_files;
        view.shown = visible.shown_files;
        view.list = visible.nodes;
        self.sink.emit(CoreEvent::TreeChanged(TreeChanged {
            window: window.to_owned(),
            delta: TreeDelta {
                seq: view.seq,
                removed,
                added,
                renamed: hints,
                total_files: view.total,
                shown_files: view.shown,
                bulk,
            },
        }));
    }

    // ---- reading the model (file operations) -------------------------------------------------------------------

    /// Listed Markdown documents of the root `prefix` (`""` | `@wt/<name>`), not ignored.
    #[must_use]
    pub fn documents(&self, prefix: &str) -> Vec<String> {
        self.listed_files(prefix, true)
    }

    /// Every listed file of the root `prefix`, not ignored (link completion).
    #[must_use]
    pub fn files(&self, prefix: &str) -> Vec<String> {
        self.listed_files(prefix, false)
    }

    fn listed_files(&self, prefix: &str, markdown_only: bool) -> Vec<String> {
        let guard = self.model();
        let Some(model) = guard.as_ref() else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let mut stack = vec![key_of(prefix)];
        while let Some(folder) = stack.pop() {
            for child in model.children_of(&folder) {
                let Some(node) = model.node(child) else {
                    continue;
                };
                match node.kind {
                    NodeKind::File { category } => {
                        if !node.ignored
                            && (!markdown_only || category == super::Category::Markdown)
                        {
                            found.push(node.path.clone());
                        }
                    }
                    NodeKind::Dir { unlisted: None } if !node.ignored => stack.push(child.clone()),
                    NodeKind::Dir { .. } => {}
                }
            }
        }
        found.sort();
        found
    }

    /// Listed files (not ignored) by exact name: name -> project paths (code references).
    #[must_use]
    pub fn files_named(&self, names: &HashSet<String>) -> HashMap<String, Vec<String>> {
        let guard = self.model();
        guard
            .as_ref()
            .map_or_else(HashMap::new, |model| model.files_named(names))
    }

    /// Whether a project path is listed (folder or file), and what.
    #[must_use]
    pub fn listed(&self, path: &str) -> Option<NodeKind> {
        let guard = self.model();
        guard.as_ref()?.node(&key_of(path)).map(|n| n.kind)
    }
}

/// `.gitignore` | `.ignore` changed: what the walk read from ignore files is stale.
fn is_ignore_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    Ignorer::is_ignore_file(name) && name != "exclude"
}

impl TreeSnapshot {
    fn empty(roots: Vec<RootNode>) -> Self {
        Self {
            roots,
            nodes: Vec::new(),
            filter: TreeFilter::default(),
            seq: 0,
            invalid_patterns: Vec::new(),
            total_files: 0,
            shown_files: 0,
        }
    }
}
