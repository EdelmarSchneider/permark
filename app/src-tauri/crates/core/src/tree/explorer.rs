//! `Explorer`: the tree service of a `ProjectCore`. Owns the model (built on first use), the per-window views and
//! the deltas sent to them.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ignore::gitignore::Gitignore;

use super::filter::{apply, Compiled, Visible};
use super::ignore::Ignorer;
use super::model::{key_of, scan_folder, scan_root, RootCtx, TreeModel};
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
    /// The person chose this filter (else the view follows the project's default).
    own: bool,
    /// The filter changed by itself: the next delta carries it.
    filter_moved: bool,
    keep: Option<String>,
    list: Vec<TreeNode>,
    total: u32,
    shown: u32,
    code_hidden: u32,
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
            code_hidden: self.code_hidden,
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
    /// Keys of the project paths some document links to (search link index): assets shown wherever they sit.
    linked: Mutex<Arc<HashSet<String>>>,
    default_filter: Mutex<TreeFilter>,
    sink: Arc<dyn EventSink>,
}

impl std::fmt::Debug for Explorer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Explorer").finish_non_exhaustive()
    }
}

impl Explorer {
    /// `global`: the global excludes file of git (None: none, as in tests).
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
            linked: Mutex::new(Arc::new(HashSet::new())),
            default_filter: Mutex::new(TreeFilter::from_config(&config.tree)),
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
    pub fn default_filter(&self) -> TreeFilter {
        lock(&self.default_filter).clone()
    }

    /// The project's config changed: the default filter + the `ignore` patterns count from now on. Views that follow the
    /// default move to it; the caller then publishes a rescan batch (the tree is walked again and windows are told).
    pub fn reconfigure(&self, config: &ProjectConfig) {
        let filter = TreeFilter::from_config(&config.tree);
        *lock(&self.default_filter) = filter.clone();
        for root in &self.roots {
            root.ignorer.set_extra(&config.ignore);
            root.ignorer.clear();
        }
        for view in lock(&self.views).values_mut() {
            if !view.own && view.compiled.filter != filter {
                view.compiled = Compiled::new(filter.clone());
                view.filter_moved = true;
            }
        }
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
        let linked = Arc::clone(&lock(&self.linked));
        apply(
            model,
            &view.compiled,
            view.keep.as_deref(),
            &lock(&self.kept_dirs),
            &linked,
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
        let own = filter.is_some();
        let mut view = View {
            compiled: Compiled::new(filter.unwrap_or_else(|| self.default_filter())),
            own,
            filter_moved: false,
            keep,
            list: Vec::new(),
            total: 0,
            shown: 0,
            code_hidden: 0,
            seq,
        };
        let visible = self.visible(model, &view);
        view.list = visible.nodes;
        view.total = visible.total_files;
        view.shown = visible.shown_files;
        view.code_hidden = visible.code_hidden;
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
        let filter = std::mem::take(&mut view.filter_moved).then(|| view.compiled.filter.clone());
        if removed.is_empty() && added.is_empty() && !counts_changed && filter.is_none() {
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
        view.code_hidden = visible.code_hidden;
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
                filter,
            },
        }));
    }

    // ---- reading the model (file operations) -------------------------------------------------------------------

    /// A fresh walk of every root without the large-folder guard (the search index reads it).
    #[must_use]
    pub fn scan(&self, cancel: &std::sync::atomic::AtomicBool) -> Vec<(String, super::Scan)> {
        self.roots
            .iter()
            .map(|root| (root.prefix.clone(), scan_root(root, cancel)))
            .collect()
    }

    /// A fresh walk inside one folder (`folder`, a project path; `abs`, where the vault resolved it) with the tree's
    /// ignore rules and no large-folder guard: every file that is not ignored, with its size and modified time.
    /// `stop` sees each file and ends the walk by answering `true` (the scan is then cut off).
    ///
    /// # Errors
    /// `NotAFolder`: no root holds the folder, or it is not a folder on disk.
    pub fn scan_folder(
        &self,
        folder: &VaultPath,
        abs: &std::path::Path,
        cancel: &std::sync::atomic::AtomicBool,
        stop: &mut dyn FnMut(&super::ScanFile) -> bool,
    ) -> Result<super::Scan, TreeError> {
        let not_a_folder = || TreeError::NotAFolder(folder.as_str().to_owned());
        let root = self.root_of(folder.as_str()).ok_or_else(not_a_folder)?;
        if !std::fs::metadata(abs).is_ok_and(|m| m.is_dir()) {
            return Err(not_a_folder());
        }
        Ok(scan_folder(root, folder.as_str(), abs, cancel, stop))
    }

    /// The root a project path belongs to: a worktree by its prefix, else the project folder.
    fn root_of(&self, path: &str) -> Option<&RootCtx> {
        if path.starts_with(crate::paths::WORKTREE_PREFIX) {
            let key = key_of(path);
            self.roots.iter().skip(1).find(|r| {
                let own = key_of(&r.prefix);
                key == own || key.starts_with(&format!("{own}/"))
            })
        } else {
            self.roots.first()
        }
    }

    /// Markdown documents of the root `prefix` (`""` | `@wt/<name>`) found by a fresh walk (also the ones in folders the
    /// tree leaves unlisted for their size), plus the ignored folders that were not looked into.
    #[must_use]
    pub fn scan_documents(&self, prefix: &str) -> (Vec<String>, Vec<String>) {
        let never = std::sync::atomic::AtomicBool::new(false);
        let key = key_of(prefix);
        let Some(root) = self.roots.iter().find(|r| key_of(&r.prefix) == key) else {
            return (Vec::new(), Vec::new());
        };
        let scan = scan_root(root, &never);
        let mut docs: Vec<String> = scan
            .files
            .into_iter()
            .filter(|f| f.category == super::Category::Markdown)
            .map(|f| f.path)
            .collect();
        docs.sort();
        (docs, scan.unchecked)
    }

    /// The files documents link to (keys): assets among them are shown by the "near documents" filter wherever they
    /// sit. Views are refreshed when the set differs.
    pub fn set_linked(&self, linked: Arc<HashSet<String>>) {
        {
            let mut current = lock(&self.linked);
            if **current == *linked {
                return;
            }
            *current = linked;
        }
        let guard = lock(&self.model);
        if let Some(model) = guard.as_ref() {
            self.refresh_views(model, &[], false);
        }
    }

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

    /// Whether the tree, when it is built, holds `path` as a folder (never walks the disk to find out).
    #[must_use]
    pub fn holds_folder(&self, path: &str) -> bool {
        lock(&self.model)
            .as_ref()
            .and_then(|model| model.node(&key_of(path)))
            .is_some_and(|n| matches!(n.kind, NodeKind::Dir { .. }))
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
            code_hidden: 0,
        }
    }
}
