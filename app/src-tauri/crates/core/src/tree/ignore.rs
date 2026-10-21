//! Ignore rules of a tree: `.gitignore` + `.ignore` per folder, `.git/info/exclude`, git's global excludes file and
//! the project's own `ignore` list. Works without a repository. A folder with its own `.git` (folder | file) is a nested
//! repository: un-hides from ignore files (`.gitignore` | `.ignore` | `exclude` | global), walked under its own rules
//! only; never from the person's own `ignore` list or `node_modules` (hard rules). A `.permark` folder never un-hides (it
//! can ship inside third-party content; `.git` cannot be committed). An ignored folder with a nested repository below
//! is a container. One decision for the walk ([`Ignorer::classify`]) and for single paths ([`Ignorer::locate`],
//! the watch).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::paths::CASE_INSENSITIVE;

/// Dependency folders are ignored whatever the ignore files say (listed unopened when ignored files are shown).
const DEPENDENCY_DIR: &str = "node_modules";
/// Levels above the project folder searched for the enclosing repository.
const MAX_ANCESTORS: usize = 32;
/// Folders searched for a nested project below an ignored folder.
const PROJECT_SEARCH_DEPTH: usize = 4;
/// Entries one such search looks at.
const PROJECT_SEARCH_ENTRIES: usize = 3000;

/// Ignore layers in force inside one folder: folder files (shallowest first) + repository-wide ones (lowest priority).
#[derive(Debug, Clone, Default)]
pub struct Frames {
    layers: Vec<Arc<Gitignore>>,
    tail: Vec<Arc<Gitignore>>,
}

/// What a folder listing already told about the folder: saves a look at the disk per folder.
#[allow(clippy::struct_excessive_bools)] // independent facts of one folder listing
#[derive(Debug, Clone, Copy, Default)]
pub struct DirHints {
    pub gitignore: bool,
    pub ignore: bool,
    pub git: bool,
}

/// Whether `dir` is a folder of its own, not a link to one (a walk never follows links; neither do its probes).
fn plain_dir(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && !super::model::is_link(&m))
}

/// Whether `dir` is a repository of its own: a plain folder (not a link) holding `.git` (folder | file). 2 stats.
fn repo_on_disk(dir: &Path) -> bool {
    plain_dir(dir) && dir.join(".git").exists()
}

/// Whether a folder entry `name` is `wanted` under the platform's case rule (Windows | macOS: case-insensitive, as
/// their file systems and git there). One rule for listings, probes and watch hints.
pub(crate) fn named(name: &str, wanted: &str) -> bool {
    if CASE_INSENSITIVE {
        name.eq_ignore_ascii_case(wanted)
    } else {
        name == wanted
    }
}

/// What a walk makes of one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entry {
    /// Listed under the rules in force.
    Shown,
    /// Hidden by the enclosing rules, but a nested repository: shown, walked under its own rules only.
    Project,
    /// Ignored folder with a nested repository below: shown and entered; its own files are ignored.
    Container,
    Ignored,
}

#[derive(Debug)]
pub struct Ignorer {
    root: PathBuf,
    /// The project's own `ignore` patterns (changes while the project is open).
    extra: Mutex<Option<Arc<Gitignore>>>,
    global: Option<Arc<Gitignore>>,
    /// Folder -> matcher of its `.gitignore` + `.ignore` (None: it has neither).
    folders: Mutex<HashMap<PathBuf, Option<Arc<Gitignore>>>>,
    /// Repository folder -> matcher of its `.git/info/exclude` (None: none).
    excludes: Mutex<HashMap<PathBuf, Option<Arc<Gitignore>>>>,
    /// Frames at the project folder as last built ([`Ignorer::root_frames`]).
    top: Mutex<Option<Frames>>,
    /// Folder -> whether it has its own `.git`, as last read. Kept across [`Ignorer::clear`]: the watch compares it
    /// with the disk to see repositories come and go ([`Ignorer::project_changed`]).
    marks: Mutex<HashMap<PathBuf, bool>>,
    /// Folders among `marks` with a `.git`.
    projects: Mutex<HashSet<PathBuf>>,
    /// Ignored folder -> holds a nested repository below.
    containers: Mutex<HashMap<PathBuf, bool>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn builder(base: &Path) -> GitignoreBuilder {
    let mut builder = GitignoreBuilder::new(base);
    // must come before any pattern; a failure leaves the platform default
    let _ = builder.case_insensitive(CASE_INSENSITIVE);
    builder
}

fn matcher_of(files: &[PathBuf], base: &Path) -> Option<Arc<Gitignore>> {
    let mut builder = builder(base);
    let mut any = false;
    for file in files {
        if file.is_file() {
            // a malformed line is skipped by the crate; the rest of the file still counts
            let _ = builder.add(file);
            any = true;
        }
    }
    if !any {
        return None;
    }
    builder.build().ok().filter(|g| !g.is_empty()).map(Arc::new)
}

/// The matcher of the project's own `ignore` patterns (None: no pattern).
fn extra_layer(root: &Path, patterns: &[String]) -> Option<Arc<Gitignore>> {
    if patterns.is_empty() {
        return None;
    }
    let mut b = builder(root);
    for line in patterns {
        let _ = b.add_line(None, line);
    }
    b.build().ok().filter(|g| !g.is_empty()).map(Arc::new)
}

/// Verdict of one layer: `Some(true)` ignored, `Some(false)` whitelisted, `None` no opinion.
fn verdict(layer: &Gitignore, path: &Path, is_dir: bool) -> Option<bool> {
    let found = layer.matched(path, is_dir);
    if found.is_ignore() {
        Some(true)
    } else if found.is_whitelist() {
        Some(false)
    } else {
        None
    }
}

impl Ignorer {
    /// `extra`: the project's `ignore` patterns; `global`: git's global excludes file (None: none).
    #[must_use]
    pub fn new(root: &Path, extra: &[String], global: Option<Gitignore>) -> Self {
        Self {
            root: root.to_path_buf(),
            extra: Mutex::new(extra_layer(root, extra)),
            global: global.filter(|g| !g.is_empty()).map(Arc::new),
            folders: Mutex::new(HashMap::new()),
            excludes: Mutex::new(HashMap::new()),
            top: Mutex::new(None),
            marks: Mutex::new(HashMap::new()),
            projects: Mutex::new(HashSet::new()),
            containers: Mutex::new(HashMap::new()),
        }
    }

    /// The project's `ignore` patterns changed: the new ones count from now on (the caller walks the tree again).
    pub fn set_extra(&self, patterns: &[String]) {
        *lock(&self.extra) = extra_layer(&self.root, patterns);
    }

    /// Git's global excludes file (`core.excludesFile`, else its default place).
    #[must_use]
    pub fn global_excludes() -> Option<Gitignore> {
        let (global, _) = Gitignore::global();
        (!global.is_empty()).then_some(global)
    }

    /// Forgets what was read from ignore files (one changed) and which ignored folders hold repositories.
    pub fn clear(&self) {
        lock(&self.folders).clear();
        lock(&self.excludes).clear();
        *lock(&self.top) = None;
        lock(&self.containers).clear();
    }

    /// Forgets everything read from the disk, `.git` marks too (repositories came or went, or events were lost).
    pub fn forget_projects(&self) {
        self.clear();
        lock(&self.marks).clear();
        lock(&self.projects).clear();
    }

    fn set_marks(&self, dir: &Path, repo: bool) {
        lock(&self.marks).insert(dir.to_path_buf(), repo);
        let mut projects = lock(&self.projects);
        if repo {
            projects.insert(dir.to_path_buf());
        } else {
            projects.remove(dir);
        }
    }

    fn is_repo(&self, dir: &Path) -> bool {
        if let Some(found) = lock(&self.marks).get(dir) {
            return *found;
        }
        // disk read outside the lock
        let found = repo_on_disk(dir);
        self.set_marks(dir, found);
        found
    }

    /// Folder is the root of a repository nested in the project (not the project folder itself).
    fn is_nested_repo(&self, dir: &Path) -> bool {
        dir != self.root && self.is_repo(dir)
    }

    /// Ignored folder `dir` leads to a nested repository (searched [`PROJECT_SEARCH_DEPTH`] levels down within
    /// [`PROJECT_SEARCH_ENTRIES`] entries, `node_modules` skipped): the way to it is not cut (the root repo may ignore
    /// `hubs/` while every hub is a repo of its own).
    fn is_container(&self, dir: &Path) -> bool {
        if let Some(found) = lock(&self.containers).get(dir) {
            return *found;
        }
        let mut budget = PROJECT_SEARCH_ENTRIES;
        let extra = lock(&self.extra).clone();
        let found =
            plain_dir(dir) && holds_repo(dir, PROJECT_SEARCH_DEPTH, &mut budget, extra.as_deref());
        lock(&self.containers).insert(dir.to_path_buf(), found);
        found
    }

    /// Whether `dir`'s `.git` on disk differs from the one last read (a repository came or went); a folder never looked
    /// at counts when it has one. Then what was read about repositories is stale.
    #[must_use]
    pub fn project_changed(&self, dir: &Path) -> bool {
        if !dir.starts_with(&self.root) {
            return false;
        }
        let known = lock(&self.marks).get(dir).copied();
        let now = repo_on_disk(dir);
        known.map_or(now, |known| known != now)
    }

    /// Whether a nested repository was known at or below `dir` (a folder that went: repositories may have gone too).
    #[must_use]
    pub fn held_project(&self, dir: &Path) -> bool {
        dir != self.root.as_path() && lock(&self.projects).iter().any(|p| p.starts_with(dir))
    }

    fn folder_layer(&self, dir: &Path) -> Option<Arc<Gitignore>> {
        if let Some(found) = lock(&self.folders).get(dir) {
            return found.clone();
        }
        let layer = matcher_of(&[dir.join(".gitignore"), dir.join(".ignore")], dir);
        lock(&self.folders)
            .entry(dir.to_path_buf())
            .or_insert(layer)
            .clone()
    }

    fn exclude_layer(&self, repo: &Path) -> Option<Arc<Gitignore>> {
        if let Some(found) = lock(&self.excludes).get(repo) {
            return found.clone();
        }
        let layer = matcher_of(&[repo.join(".git").join("info").join("exclude")], repo);
        lock(&self.excludes)
            .entry(repo.to_path_buf())
            .or_insert(layer)
            .clone()
    }

    /// Repository-wide layers of the repo rooted at `repo`: `.git/info/exclude`, then the global excludes.
    fn repo_tail(&self, repo: Option<&Path>) -> Vec<Arc<Gitignore>> {
        let mut tail = Vec::new();
        if let Some(layer) = repo.and_then(|repo| self.exclude_layer(repo)) {
            tail.push(layer);
        }
        if let Some(global) = &self.global {
            tail.push(Arc::clone(global));
        }
        tail
    }

    /// Frames at the project folder: enclosing repository's folder files (when the project sits inside one) + its own.
    /// Kept until [`Ignorer::clear`].
    #[must_use]
    pub fn root_frames(&self) -> Frames {
        if let Some(found) = lock(&self.top).as_ref() {
            return found.clone();
        }
        let frames = self.build_root_frames();
        lock(&self.top).get_or_insert(frames).clone()
    }

    fn build_root_frames(&self) -> Frames {
        let mut chain: Vec<&Path> = Vec::new();
        let mut repo: Option<&Path> = None;
        for dir in self.root.ancestors().take(MAX_ANCESTORS + 1) {
            chain.push(dir);
            if self.is_repo(dir) {
                repo = Some(dir);
                break;
            }
        }
        let mut frames = Frames {
            layers: Vec::new(),
            tail: self.repo_tail(repo),
        };
        // no repository found: the project folder's own files only
        let dirs: Vec<&Path> = if repo.is_some() {
            chain.into_iter().rev().collect()
        } else {
            vec![self.root.as_path()]
        };
        for dir in dirs {
            if let Some(layer) = self.folder_layer(dir) {
                frames.layers.push(layer);
            }
        }
        frames
    }

    /// Frames a nested repository hidden by the enclosing rules starts from: none of theirs, git's global excludes only.
    fn fresh(&self) -> Frames {
        Frames {
            layers: Vec::new(),
            tail: self.repo_tail(None),
        }
    }

    /// Frames a walk hands to a folder it classified `entry` (inside `frames`); its own layers come on entering it.
    pub(crate) fn base_for(&self, entry: Entry, frames: &Frames) -> Frames {
        if entry == Entry::Project {
            self.fresh()
        } else {
            frames.clone()
        }
    }

    /// Frames inside `dir`, a child folder of the folder `parent` describes.
    #[must_use]
    pub fn enter(&self, parent: &Frames, dir: &Path) -> Frames {
        let mut frames = if self.is_nested_repo(dir) {
            Frames {
                layers: Vec::new(),
                tail: self.repo_tail(Some(dir)),
            }
        } else {
            parent.clone()
        };
        if let Some(layer) = self.folder_layer(dir) {
            frames.layers.push(layer);
        }
        frames
    }

    /// [`Ignorer::enter`] for a folder whose listing is at hand: no look at the disk.
    #[must_use]
    pub fn enter_hinted(&self, parent: &Frames, dir: &Path, hints: DirHints) -> Frames {
        self.set_marks(dir, hints.git);
        let mut frames = if dir != self.root && hints.git {
            Frames {
                layers: Vec::new(),
                tail: self.repo_tail(Some(dir)),
            }
        } else {
            parent.clone()
        };
        if hints.gitignore || hints.ignore {
            if let Some(layer) = self.folder_layer(dir) {
                frames.layers.push(layer);
            }
        } else {
            lock(&self.folders).entry(dir.to_path_buf()).or_insert(None);
        }
        frames
    }

    /// Whether the person's own `ignore` list or `node_modules` hide `path`: no nested repository un-hides these.
    fn hard_hidden(&self, path: &Path, is_dir: bool) -> bool {
        hard_hidden_by(lock(&self.extra).as_deref(), path, is_dir)
    }

    /// Whether the rules of `frames` (+ the project's list, `node_modules`) hide `path`, a child of their folder.
    fn rules_hide(&self, frames: &Frames, path: &Path, is_dir: bool) -> bool {
        self.hard_hidden(path, is_dir)
            || frames
                .layers
                .iter()
                .rev()
                .chain(frames.tail.iter())
                .find_map(|layer| verdict(layer, path, is_dir))
                .unwrap_or(false)
    }

    /// What a walk makes of `path`, a child of the folder `frames` describes. `in_ignored`: that folder is a container
    /// (its own entries are ignored). `may_contain: false` makes no container (a walk at its limits).
    pub(crate) fn classify(
        &self,
        frames: &Frames,
        path: &Path,
        is_dir: bool,
        in_ignored: bool,
        may_contain: bool,
    ) -> Entry {
        // `.git` marks are looked at only for what the rules hide
        if !(in_ignored || self.rules_hide(frames, path, is_dir)) {
            Entry::Shown
        } else if !is_dir || self.hard_hidden(path, true) {
            Entry::Ignored
        } else if self.is_nested_repo(path) {
            Entry::Project
        } else if may_contain && self.is_container(path) {
            Entry::Container
        } else {
            Entry::Ignored
        }
    }

    /// Follows `abs` (below the project folder) down from the top as a walk reaches it: what it is there + the frames
    /// a walk hands to it ([`Ignorer::base_for`]). `None`: the project folder itself, or outside it.
    pub(crate) fn locate(&self, abs: &Path, is_dir: bool) -> Option<(Entry, Frames)> {
        let rest = abs.strip_prefix(&self.root).ok()?;
        let parts: Vec<_> = rest.components().collect();
        let (last, above) = parts.split_last()?;
        let mut frames = self.root_frames();
        let mut in_ignored = false;
        let mut current = self.root.clone();
        for part in above {
            current.push(part);
            match self.classify(&frames, &current, true, in_ignored, true) {
                Entry::Ignored => return Some((Entry::Ignored, frames)),
                // inside a container only nested repositories count: its frames are not needed
                Entry::Container => in_ignored = true,
                entry @ (Entry::Shown | Entry::Project) => {
                    frames = self.enter(&self.base_for(entry, &frames), &current);
                    in_ignored = false;
                }
            }
        }
        current.push(last);
        let entry = self.classify(&frames, &current, is_dir, in_ignored, true);
        Some((entry, self.base_for(entry, &frames)))
    }

    /// Whether `abs` (inside the project folder) is ignored as the walk decides it (containers and nested repositories
    /// are not): for watch events.
    #[must_use]
    pub fn path_ignored(&self, abs: &Path, is_dir: bool) -> bool {
        matches!(self.locate(abs, is_dir), Some((Entry::Ignored, _)))
    }

    /// [`Ignorer::path_ignored`] for a path given below the root (`a/b/c.md`).
    #[must_use]
    pub fn rel_ignored(&self, rel: &str, is_dir: bool) -> bool {
        self.path_ignored(&self.abs_of(rel), is_dir)
    }

    /// Where a path given below the root (`a/b/c.md`) is on disk.
    #[must_use]
    pub fn abs_of(&self, rel: &str) -> PathBuf {
        let mut abs = self.root.clone();
        abs.extend(rel.split('/').filter(|s| !s.is_empty()));
        abs
    }

    /// Name of a file that, when it changes, makes everything read from ignore files stale.
    #[must_use]
    pub fn is_ignore_file(name: &str) -> bool {
        matches!(name, ".gitignore" | ".ignore" | "exclude")
    }

    /// Whether a folder entry `name` is git's folder | gitfile: the one name that makes a nested repository.
    #[must_use]
    pub fn is_git_name(name: &str) -> bool {
        named(name, ".git")
    }
}

/// [`Ignorer::hard_hidden`] with the person's list at hand.
fn hard_hidden_by(extra: Option<&Gitignore>, path: &Path, is_dir: bool) -> bool {
    (is_dir && path.file_name().is_some_and(|n| n == DEPENDENCY_DIR))
        || extra
            .and_then(|g| verdict(g, path, is_dir))
            .unwrap_or(false)
}

/// Whether a nested repository (a folder with its own `.git`) sits somewhere below `dir`, at most `depth` levels down,
/// within `budget` entries; folders the person's list (`extra`) or `node_modules` hide are skipped. Links are never
/// followed.
fn holds_repo(dir: &Path, depth: usize, budget: &mut usize, extra: Option<&Gitignore>) -> bool {
    if depth == 0 {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut folders = Vec::new();
    for entry in entries.flatten() {
        *budget = budget.saturating_sub(1);
        if *budget == 0 {
            return false;
        }
        let name = entry.file_name();
        if name.to_str().is_some_and(Ignorer::is_git_name) {
            return true;
        }
        let plain_folder =
            entry.file_type().is_ok_and(|t| t.is_dir()) && !super::model::is_reparse_point(&entry);
        if plain_folder {
            let path = entry.path();
            if !hard_hidden_by(extra, &path, true) {
                folders.push(path);
            }
        }
    }
    folders
        .iter()
        .any(|folder| holds_repo(folder, depth - 1, budget, extra))
}
