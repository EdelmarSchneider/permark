//! The tree model: every listed entry of every root, unfiltered, kept sorted. Built by a walk (never follows links),
//! kept current by syncing changed paths with the disk ("disk is truth": idempotent, order of events irrelevant).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use unicode_normalization::UnicodeNormalization;

use super::ignore::{DirHints, Frames, Ignorer};
use super::{category_of, NodeKind, RootKind, Unlisted, LARGE_FOLDER};
use crate::fsx::is_scratch_name;
use crate::paths::{fold, WORKTREE_PREFIX};

/// Folders deeper than this are listed unopened.
pub(crate) const MAX_DEPTH: usize = 100;
/// Entries one walk lists; the rest of the walk is cut off (folders left unopened).
pub(crate) const MAX_ENTRIES: usize = 200_000;

/// Identity key of a project path: NFC + case folded per OS.
pub(crate) fn key_of(path: &str) -> String {
    fold(&path.nfc().collect::<String>())
}

/// Folder of a project path (`""` for a top-level entry of the project folder).
pub(crate) fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// Path below its root: `@wt/<name>/a/b` -> `a/b`.
pub(crate) fn below_root(path: &str) -> &str {
    match path.strip_prefix(WORKTREE_PREFIX) {
        Some(rest) => rest.split_once('/').map_or("", |(_, rest)| rest),
        None => path,
    }
}

/// One root of the vault with what the model needs to walk it.
#[derive(Debug, Clone)]
pub(crate) struct RootCtx {
    pub name: String,
    pub kind: RootKind,
    /// `""` or `@wt/<name>`.
    pub prefix: String,
    pub abs: PathBuf,
    pub ignorer: Arc<Ignorer>,
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub path: String,
    pub name: String,
    pub kind: NodeKind,
    pub hidden: bool,
    pub ignored: bool,
    key: String,
    /// Key of the folder it is in.
    parent: String,
    sort: (bool, String),
}

impl Node {
    pub(crate) fn is_dir(&self) -> bool {
        matches!(self.kind, NodeKind::Dir { .. })
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    /// Key of the folder this entry is in (a root's key: `""` | `@wt/<name>` folded).
    pub(crate) fn parent_key(&self) -> &str {
        &self.parent
    }

    fn new(path: String, name: String, kind: NodeKind, ignored: bool) -> Self {
        let sort = (
            !matches!(kind, NodeKind::Dir { .. }),
            name.nfc().collect::<String>().to_lowercase(),
        );
        Self {
            hidden: name.starts_with('.'),
            key: key_of(&path),
            parent: key_of(parent_of(&path)),
            path,
            name,
            kind,
            ignored,
            sort,
        }
    }
}

/// Entry found on disk.
struct Found {
    name: String,
    is_dir: bool,
    /// Size and modified time (ms since the epoch) of a file, when the caller asked for them.
    stat: Option<(u64, i64)>,
}

fn is_reparse_point(entry: &fs::DirEntry) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        entry
            .metadata()
            .is_ok_and(|m| m.file_attributes() & 0x400 != 0)
    }
    #[cfg(not(windows))]
    {
        let _ = entry;
        false
    }
}

/// Whether the model never holds this entry: git's folder, the app's private state folders, scratch files.
pub(crate) fn always_out(parent_path: &str, name: &str) -> bool {
    name == ".git"
        || (below_root(parent_path) == ".permark" && matches!(name, "local" | "cache"))
        || is_scratch_name(name)
}

/// [`always_out`] for the entry itself or any folder above it.
pub(crate) fn path_always_out(path: &str) -> bool {
    let mut parent = String::new();
    for part in path.split('/') {
        if always_out(&parent, part) {
            return true;
        }
        if !parent.is_empty() {
            parent.push('/');
        }
        parent.push_str(part);
    }
    false
}

/// Direct entries of a folder (links are never followed: a link to a folder is a leaf) + what the listing says about
/// the folder itself. `None`: unreadable.
fn read_entries(abs: &Path, parent_path: &str, want_stat: bool) -> Option<(Vec<Found>, DirHints)> {
    let mut found = Vec::new();
    let mut hints = DirHints::default();
    for entry in fs::read_dir(abs).ok()?.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue; // a name that is not text cannot be a project path
        };
        match name.as_str() {
            ".gitignore" => hints.gitignore = true,
            ".ignore" => hints.ignore = true,
            ".git" => hints.git = true,
            _ => {}
        }
        if always_out(parent_path, &name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let link = file_type.is_symlink() || is_reparse_point(&entry);
        let is_dir = file_type.is_dir() && !link;
        if want_stat && link && !is_dir {
            // a folder scan reads files: through a link (or a cloud placeholder) it would leave the folder
            continue;
        }
        let stat = (want_stat && !is_dir)
            .then(|| entry.metadata().ok().map(|m| (m.len(), modified_ms(&m))))
            .flatten();
        found.push(Found { name, is_dir, stat });
    }
    Some((found, hints))
}

/// Modified time of a file in milliseconds since the epoch (0 when the system does not tell).
pub(crate) fn modified_ms(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

fn large(entries: usize) -> NodeKind {
    NodeKind::Dir {
        unlisted: Some(Unlisted::Large {
            entries: u32::try_from(entries).unwrap_or(u32::MAX),
        }),
    }
}

/// Folders searched for a nested repository below an ignored folder.
const REPO_SEARCH_DEPTH: usize = 4;
/// Entries one such search looks at.
const REPO_SEARCH_ENTRIES: usize = 3000;

/// Whether a repository (a folder with its own `.git`) sits somewhere below `dir`: an ignored folder that leads to
/// one is still shown, so the way to the repository is not cut (the root repo may ignore `hubs/` while every hub is
/// a repo of its own).
fn holds_repo(dir: &Path, depth: usize, budget: &mut usize) -> bool {
    if depth == 0 {
        return false;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    let mut folders = Vec::new();
    for entry in entries.flatten() {
        *budget = budget.saturating_sub(1);
        if *budget == 0 {
            return false;
        }
        let name = entry.file_name();
        if name == ".git" {
            return true;
        }
        let plain_folder = entry.file_type().is_ok_and(|t| t.is_dir()) && !is_reparse_point(&entry);
        if plain_folder && name != "node_modules" {
            folders.push(entry.path());
        }
    }
    folders
        .iter()
        .any(|folder| holds_repo(folder, depth - 1, budget))
}

/// What a walk of one folder works with.
struct Walk<'a> {
    root: &'a RootCtx,
    budget: usize,
}

#[derive(Debug, Default)]
pub(crate) struct TreeModel {
    pub(crate) roots: Vec<RootCtx>,
    nodes: HashMap<String, Node>,
    /// Folder key -> child keys, sorted (folders first, then by name).
    children: HashMap<String, Vec<String>>,
}

impl TreeModel {
    pub(crate) fn new(roots: Vec<RootCtx>) -> Self {
        Self {
            roots,
            nodes: HashMap::new(),
            children: HashMap::new(),
        }
    }

    pub(crate) fn node(&self, key: &str) -> Option<&Node> {
        self.nodes.get(key)
    }

    /// Listed files (not ignored) by exact name: name -> project paths.
    pub(crate) fn files_named(
        &self,
        names: &std::collections::HashSet<String>,
    ) -> HashMap<String, Vec<String>> {
        let mut found: HashMap<String, Vec<String>> = HashMap::new();
        for node in self.nodes.values() {
            if !node.is_dir() && !node.ignored && names.contains(&node.name) {
                found
                    .entry(node.name.clone())
                    .or_default()
                    .push(node.path.clone());
            }
        }
        found
    }

    pub(crate) fn children_of(&self, key: &str) -> &[String] {
        self.children.get(key).map_or(&[], Vec::as_slice)
    }

    /// Root a project path belongs to.
    fn root_of(&self, path: &str) -> Option<&RootCtx> {
        if path.starts_with(WORKTREE_PREFIX) {
            let own = key_of(path);
            self.roots.iter().skip(1).find(|r| {
                let key = key_of(&r.prefix);
                own == key || own.starts_with(&format!("{key}/"))
            })
        } else {
            self.roots.first()
        }
    }

    /// Absolute path of a project path (`None`: unknown worktree).
    pub(crate) fn abs_of(&self, path: &str) -> Option<PathBuf> {
        let root = self.root_of(path)?;
        let mut abs = root.abs.clone();
        abs.extend(below_root(path).split('/').filter(|s| !s.is_empty()));
        Some(abs)
    }

    // ---- structure ---------------------------------------------------------------------------------------------

    /// Adds a node in its place among its siblings (replaces a node of the same key, subtree included).
    fn insert(&mut self, node: Node) {
        if self.nodes.contains_key(node.key()) {
            self.remove_subtree(&node.key.clone());
        }
        let key = node.key.clone();
        let nodes = &self.nodes;
        let list = self.children.entry(node.parent.clone()).or_default();
        let at = list.partition_point(|k| {
            nodes
                .get(k)
                .is_some_and(|n| (&n.sort, &n.name) < (&node.sort, &node.name))
        });
        list.insert(at, key.clone());
        self.nodes.insert(key, node);
    }

    /// Adds a node at the end of its folder's list: a walk sorts once when the folder is done.
    fn push(&mut self, node: Node) {
        if self.nodes.contains_key(node.key()) {
            self.remove_subtree(&node.key.clone());
        }
        self.children
            .entry(node.parent.clone())
            .or_default()
            .push(node.key.clone());
        self.nodes.insert(node.key.clone(), node);
    }

    fn sort_folder(&mut self, folder_key: &str) {
        let nodes = &self.nodes;
        if let Some(list) = self.children.get_mut(folder_key) {
            list.sort_by(|a, b| match (nodes.get(a), nodes.get(b)) {
                (Some(x), Some(y)) => x.sort.cmp(&y.sort).then_with(|| x.name.cmp(&y.name)),
                _ => std::cmp::Ordering::Equal,
            });
        }
    }

    /// Removes a node and everything below it. Returns how many nodes went.
    pub(crate) fn remove_subtree(&mut self, key: &str) -> usize {
        let Some(node) = self.nodes.remove(key) else {
            return 0;
        };
        if let Some(list) = self.children.get_mut(&node.parent) {
            list.retain(|k| k != key);
        }
        1 + self.drop_below(key)
    }

    fn drop_below(&mut self, key: &str) -> usize {
        let mut gone = 0;
        if let Some(kids) = self.children.remove(key) {
            for kid in kids {
                if self.nodes.remove(&kid).is_some() {
                    gone += 1 + self.drop_below(&kid);
                }
            }
        }
        gone
    }

    // ---- walk --------------------------------------------------------------------------------------------------

    /// Forgets everything and walks every root again.
    pub(crate) fn rebuild(&mut self) {
        self.nodes.clear();
        self.children.clear();
        let mut budget = MAX_ENTRIES;
        for index in 0..self.roots.len() {
            let root = self.roots[index].clone();
            root.ignorer.clear();
            let frames = root.ignorer.root_frames();
            let mut walk = Walk {
                root: &root,
                budget,
            };
            // the root of a walk is exempt from the large-folder guard
            let _ = self.list_dir(&mut walk, &root.abs, &root.prefix, &frames, false, 0, false);
            budget = walk.budget;
        }
    }

    /// Lists the entries of `dir_abs` (project path `dir_path`) into the model. `parent_frames` are the ignore
    /// layers of the folder above (at the root of a walk: the root's own). `Err(n)`: the folder holds `n` entries,
    /// more than [`LARGE_FOLDER`]: nothing listed (the root of a walk is exempt, and so is a folder `asked` for).
    #[allow(clippy::too_many_arguments)] // one recursive walk step; the arguments are its position + mode
    fn list_dir(
        &mut self,
        walk: &mut Walk<'_>,
        dir_abs: &Path,
        dir_path: &str,
        parent_frames: &Frames,
        in_ignored: bool,
        depth: usize,
        asked: bool,
    ) -> Result<(), usize> {
        let root = walk.root;
        let Some((found, hints)) = read_entries(dir_abs, dir_path, false) else {
            return Ok(());
        };
        if depth > 0 && !asked && found.len() > LARGE_FOLDER {
            return Err(found.len());
        }
        let frames = if depth == 0 {
            parent_frames.clone()
        } else {
            root.ignorer.enter_hinted(parent_frames, dir_abs, hints)
        };
        for entry in found {
            let path = if dir_path.is_empty() {
                entry.name.clone()
            } else {
                format!("{dir_path}/{}", entry.name)
            };
            let abs = dir_abs.join(&entry.name);
            // a nested repository is walked under its own rules, whatever surrounds it
            let nested = entry.is_dir && root.ignorer.is_nested_repo(&abs);
            let ignored =
                !nested && (in_ignored || root.ignorer.is_ignored(&frames, &abs, entry.is_dir));
            if !entry.is_dir {
                walk.budget = walk.budget.saturating_sub(1);
                let kind = NodeKind::File {
                    category: category_of(&entry.name),
                };
                self.push(Node::new(path, entry.name, kind, ignored));
                continue;
            }
            // an ignored folder that leads to a nested repository is a container: shown, its own content ignored
            let mut budget = REPO_SEARCH_ENTRIES;
            let container = ignored
                && depth < MAX_DEPTH
                && walk.budget > 0
                && holds_repo(&abs, REPO_SEARCH_DEPTH, &mut budget);
            if container {
                walk.budget = walk.budget.saturating_sub(1);
                self.push(Node::new(
                    path.clone(),
                    entry.name.clone(),
                    NodeKind::Dir { unlisted: None },
                    false,
                ));
                let inner = root
                    .ignorer
                    .enter_hinted(&frames, &abs, DirHints::default());
                let _ = self.list_dir(walk, &abs, &path, &inner, true, depth + 1, true);
                continue;
            }
            if ignored || depth + 1 > MAX_DEPTH || walk.budget == 0 {
                // ignored (or too deep, or past the budget): listed, not entered
                let unlisted = if ignored {
                    Unlisted::Ignored
                } else {
                    Unlisted::Large { entries: 0 }
                };
                let kind = NodeKind::Dir {
                    unlisted: Some(unlisted),
                };
                self.push(Node::new(path, entry.name, kind, ignored));
                continue;
            }
            walk.budget = walk.budget.saturating_sub(1);
            let dir_node = Node::new(
                path.clone(),
                entry.name.clone(),
                NodeKind::Dir { unlisted: None },
                false,
            );
            self.push(dir_node);
            if let Err(entries) = self.list_dir(walk, &abs, &path, &frames, false, depth + 1, false)
            {
                self.push(Node::new(path, entry.name, large(entries), false));
            }
        }
        self.sort_folder(&key_of(dir_path));
        Ok(())
    }

    /// Lists the entries of an unlisted folder (a large one, an ignored one). Returns whether anything changed.
    pub(crate) fn open_folder(&mut self, path: &str) -> bool {
        let key = key_of(path);
        let Some(node) = self.nodes.get(&key) else {
            return false;
        };
        let NodeKind::Dir {
            unlisted: Some(unlisted),
        } = node.kind
        else {
            return false;
        };
        let (Some(root), Some(abs)) = (self.root_of(path).cloned(), self.abs_of(path)) else {
            return false;
        };
        let (name, ignored) = (node.name.clone(), node.ignored);
        let in_ignored = ignored || matches!(unlisted, Unlisted::Ignored);
        let frames = root.ignorer.frames_at(abs.parent().unwrap_or(&abs));
        let mut walk = Walk {
            root: &root,
            budget: MAX_ENTRIES,
        };
        // the folder becomes listed first; a folder too large puts its marker back
        self.insert(Node::new(
            path.to_owned(),
            name.clone(),
            NodeKind::Dir { unlisted: None },
            ignored,
        ));
        // asked for by the person: the folder itself is listed whatever its size (its sub-folders keep the guard)
        let _ = self.list_dir(&mut walk, &abs, path, &frames, in_ignored, 1, true);
        true
    }

    // ---- sync with the disk ------------------------------------------------------------------------------------

    /// Brings the model in line with the disk at `path`. Returns whether the model changed.
    pub(crate) fn sync_path(&mut self, path: &str) -> bool {
        let Some(root) = self.root_of(path).cloned() else {
            return false;
        };
        let key = key_of(path);
        // roots themselves and everything the model never holds
        if below_root(path).is_empty() || path_always_out(path) {
            return false;
        }
        let name = path.rsplit('/').next().unwrap_or(path);
        let Some(abs) = self.abs_of(path) else {
            return false;
        };
        let Ok(meta) = fs::symlink_metadata(&abs) else {
            return self.remove_subtree(&key) > 0;
        };
        // parents first: a folder that appeared with its files
        let parent_path = parent_of(path);
        let parent_key = key_of(parent_path);
        let parent_is_root = below_root(parent_path).is_empty();
        let mut changed = false;
        if !parent_is_root && !self.nodes.contains_key(&parent_key) {
            changed |= self.sync_path(parent_path);
        }
        if !parent_is_root {
            match self.nodes.get(&parent_key).map(|n| n.kind) {
                Some(NodeKind::Dir { unlisted: None }) => {}
                // parent unknown (gone, ignored, never listed) or unopened: not listed
                _ => return changed,
            }
        }
        let link = meta.file_type().is_symlink() || {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                meta.file_attributes() & 0x400 != 0
            }
            #[cfg(not(windows))]
            {
                false
            }
        };
        let is_dir = meta.is_dir() && !link;
        let ignored = self.nodes.get(&parent_key).is_some_and(|n| n.ignored)
            || root.ignorer.path_ignored(&abs, is_dir);
        if !is_dir {
            let kind = NodeKind::File {
                category: category_of(name),
            };
            if self
                .nodes
                .get(&key)
                .is_some_and(|old| old.kind == kind && old.ignored == ignored && old.name == name)
            {
                return changed;
            }
            self.insert(Node::new(path.to_owned(), name.to_owned(), kind, ignored));
            return true;
        }
        if self
            .nodes
            .get(&key)
            .is_some_and(|old| old.is_dir() && old.ignored == ignored && old.name == name)
        {
            // known folder: its children report themselves
            return changed;
        }
        if ignored {
            let kind = NodeKind::Dir {
                unlisted: Some(Unlisted::Ignored),
            };
            self.insert(Node::new(path.to_owned(), name.to_owned(), kind, true));
            return true;
        }
        self.insert(Node::new(
            path.to_owned(),
            name.to_owned(),
            NodeKind::Dir { unlisted: None },
            false,
        ));
        // a folder that appeared (created, moved in): walk it
        let frames = root.ignorer.frames_at(abs.parent().unwrap_or(&abs));
        let mut walk = Walk {
            root: &root,
            budget: MAX_ENTRIES,
        };
        let depth = below_root(path).matches('/').count() + 1;
        if let Err(entries) = self.list_dir(&mut walk, &abs, path, &frames, false, depth, false) {
            self.insert(Node::new(
                path.to_owned(),
                name.to_owned(),
                large(entries),
                false,
            ));
        }
        true
    }
}

// ---- scan ---------------------------------------------------------------------------------------------------

/// A file found by [`scan_root`] or [`scan_folder`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanFile {
    /// Project path.
    pub path: String,
    pub category: super::Category,
    /// Where it is on disk; a root scan gives it only for the categories whose content is read (Markdown | Text |
    /// Config), a folder scan for every file.
    pub abs: Option<PathBuf>,
    /// Size in bytes (a folder scan only; 0 otherwise).
    pub size: u64,
    /// Modified time, ms since the epoch (a folder scan only; 0 otherwise).
    pub modified_ms: i64,
}

/// What a fresh walk of one root found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    pub files: Vec<ScanFile>,
    /// Ignored folders the walk did not enter (project paths).
    pub unchecked: Vec<String>,
    /// The walk hit a limit (depth, entries) or was cancelled: the list may be incomplete.
    pub cut_off: bool,
}

struct ScanCx<'a> {
    root: &'a RootCtx,
    cancel: &'a std::sync::atomic::AtomicBool,
    budget: usize,
    out: Scan,
    /// A folder scan: size, modified time and absolute path of every file; its callback may end the walk.
    folder: Option<&'a mut dyn FnMut(&ScanFile) -> bool>,
}

/// Walks `root` afresh with the tree's ignore rules and without its large-folder guard: every file that is not
/// ignored (links never followed). Ignored folders are not entered; the ones that are not containers are named in
/// `unchecked`.
pub(crate) fn scan_root(root: &RootCtx, cancel: &std::sync::atomic::AtomicBool) -> Scan {
    root.ignorer.clear();
    let frames = root.ignorer.root_frames();
    let mut cx = ScanCx {
        root,
        cancel,
        budget: MAX_ENTRIES,
        out: Scan::default(),
        folder: None,
    };
    scan_dir(&mut cx, &root.abs, &root.prefix, &frames, false, 0);
    cx.out
}

/// [`scan_root`] started inside a folder of the root (`abs` is where `folder`, a project path, is on disk): the rules
/// of the folders above it apply, and a folder that is itself ignored gives only the nested repositories below it.
/// Every file comes with its size, modified time and absolute path; `stop` sees each file and ends the walk by
/// answering `true` (the scan is then cut off).
pub(crate) fn scan_folder(
    root: &RootCtx,
    folder: &str,
    abs: &Path,
    cancel: &std::sync::atomic::AtomicBool,
    stop: &mut dyn FnMut(&ScanFile) -> bool,
) -> Scan {
    root.ignorer.clear();
    let frames = root.ignorer.frames_at(abs);
    let in_ignored = abs != root.abs && root.ignorer.path_ignored(abs, true);
    let mut cx = ScanCx {
        root,
        cancel,
        budget: MAX_ENTRIES,
        out: Scan::default(),
        folder: Some(stop),
    };
    scan_dir(&mut cx, abs, folder, &frames, in_ignored, 0);
    cx.out
}

fn scan_dir(
    cx: &mut ScanCx<'_>,
    dir_abs: &Path,
    dir_path: &str,
    parent_frames: &Frames,
    in_ignored: bool,
    depth: usize,
) {
    if cx.cancel.load(std::sync::atomic::Ordering::Relaxed) {
        cx.out.cut_off = true;
        return;
    }
    let Some((found, hints)) = read_entries(dir_abs, dir_path, cx.folder.is_some()) else {
        return;
    };
    let root = cx.root;
    let frames = if depth == 0 {
        parent_frames.clone()
    } else {
        root.ignorer.enter_hinted(parent_frames, dir_abs, hints)
    };
    for entry in found {
        let path = if dir_path.is_empty() {
            entry.name.clone()
        } else {
            format!("{dir_path}/{}", entry.name)
        };
        let abs = dir_abs.join(&entry.name);
        // a nested repository is walked under its own rules, whatever surrounds it
        let nested = entry.is_dir && root.ignorer.is_nested_repo(&abs);
        let ignored =
            !nested && (in_ignored || root.ignorer.is_ignored(&frames, &abs, entry.is_dir));
        if cx.budget == 0 {
            cx.out.cut_off = true;
            return;
        }
        cx.budget -= 1;
        if !entry.is_dir {
            if !ignored {
                let category = category_of(&entry.name);
                let (size, modified_ms) = entry.stat.unwrap_or((0, 0));
                let reads_content = cx.folder.is_some()
                    || matches!(
                        category,
                        super::Category::Markdown | super::Category::Text | super::Category::Config
                    );
                let file = ScanFile {
                    path,
                    category,
                    abs: reads_content.then_some(abs),
                    size,
                    modified_ms,
                };
                let end = cx.folder.as_mut().is_some_and(|stop| stop(&file));
                cx.out.files.push(file);
                if end {
                    // the budget at zero ends the walk in every folder above this one too
                    cx.budget = 0;
                    cx.out.cut_off = true;
                    return;
                }
            }
            continue;
        }
        if depth + 1 > MAX_DEPTH {
            cx.out.cut_off = true;
            continue;
        }
        if ignored {
            // an ignored folder that leads to a nested repository is a container: entered, its own files stay out
            let mut budget = REPO_SEARCH_ENTRIES;
            if holds_repo(&abs, REPO_SEARCH_DEPTH, &mut budget) {
                scan_dir(cx, &abs, &path, &frames, true, depth + 1);
            } else {
                cx.out.unchecked.push(path);
            }
            continue;
        }
        scan_dir(cx, &abs, &path, &frames, false, depth + 1);
    }
}
