//! Ignore rules of a tree: `.gitignore` + `.ignore` per folder, `.git/info/exclude`, the user's global excludes and
//! the project's own `ignore` list. Works without a repository. A folder with its own `.git` is a nested repository:
//! never hidden by the enclosing rules, walked under its own.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::paths::CASE_INSENSITIVE;

/// Dependency folders are ignored whatever the ignore files say (listed unopened when ignored files are shown).
const DEPENDENCY_DIR: &str = "node_modules";
/// Levels above the project folder searched for the enclosing repository.
const MAX_ANCESTORS: usize = 32;

/// Ignore layers in force inside one folder: folder files (shallowest first) + repository-wide ones (lowest priority).
#[derive(Debug, Clone, Default)]
pub struct Frames {
    layers: Vec<Arc<Gitignore>>,
    tail: Vec<Arc<Gitignore>>,
}

/// What a folder listing already told about the folder: saves a look at the disk per folder.
#[derive(Debug, Clone, Copy, Default)]
pub struct DirHints {
    pub gitignore: bool,
    pub ignore: bool,
    pub git: bool,
}

#[derive(Debug)]
pub struct Ignorer {
    root: PathBuf,
    extra: Option<Gitignore>,
    global: Option<Arc<Gitignore>>,
    /// Folder -> matcher of its `.gitignore` + `.ignore` (None: it has neither).
    folders: Mutex<HashMap<PathBuf, Option<Arc<Gitignore>>>>,
    /// Folder -> has its own `.git`.
    repos: Mutex<HashMap<PathBuf, bool>>,
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
    /// `extra`: the project's `ignore` patterns; `global`: the user's global excludes (None: none).
    #[must_use]
    pub fn new(root: &Path, extra: &[String], global: Option<Gitignore>) -> Self {
        let extra = if extra.is_empty() {
            None
        } else {
            let mut b = builder(root);
            for line in extra {
                let _ = b.add_line(None, line);
            }
            b.build().ok().filter(|g| !g.is_empty())
        };
        Self {
            root: root.to_path_buf(),
            extra,
            global: global.filter(|g| !g.is_empty()).map(Arc::new),
            folders: Mutex::new(HashMap::new()),
            repos: Mutex::new(HashMap::new()),
        }
    }

    /// The user's global excludes file as git reads it.
    #[must_use]
    pub fn global_excludes() -> Option<Gitignore> {
        let (global, _) = Gitignore::global();
        (!global.is_empty()).then_some(global)
    }

    /// Forgets what was read from ignore files (one changed).
    pub fn clear(&self) {
        lock(&self.folders).clear();
        lock(&self.repos).clear();
    }

    fn is_repo(&self, dir: &Path) -> bool {
        if let Some(found) = lock(&self.repos).get(dir) {
            return *found;
        }
        // disk read outside the lock
        let found = dir.join(".git").exists();
        lock(&self.repos).insert(dir.to_path_buf(), found);
        found
    }

    /// Folder is the root of a repository nested in the project (not the project folder itself).
    #[must_use]
    pub fn is_nested_repo(&self, dir: &Path) -> bool {
        dir != self.root && self.is_repo(dir)
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

    /// Repository-wide layers of the repo rooted at `repo`: `.git/info/exclude`, then the global excludes.
    fn repo_tail(&self, repo: Option<&Path>) -> Vec<Arc<Gitignore>> {
        let mut tail = Vec::new();
        if let Some(repo) = repo {
            let exclude = repo.join(".git").join("info").join("exclude");
            if let Some(layer) = matcher_of(&[exclude], repo) {
                tail.push(layer);
            }
        }
        if let Some(global) = &self.global {
            tail.push(Arc::clone(global));
        }
        tail
    }

    /// Frames at the project folder: enclosing repository's folder files (when the project sits inside one) + its own.
    #[must_use]
    pub fn root_frames(&self) -> Frames {
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

    /// Frames inside `dir` (a folder below the project folder), entered from the top.
    #[must_use]
    pub fn frames_at(&self, dir: &Path) -> Frames {
        let mut frames = self.root_frames();
        let mut current = self.root.clone();
        if let Ok(rest) = dir.strip_prefix(&self.root) {
            for part in rest.components() {
                current.push(part);
                frames = self.enter(&frames, &current);
            }
        }
        frames
    }

    /// [`Ignorer::enter`] for a folder whose listing is at hand: no look at the disk.
    #[must_use]
    pub fn enter_hinted(&self, parent: &Frames, dir: &Path, hints: DirHints) -> Frames {
        lock(&self.repos).insert(dir.to_path_buf(), hints.git);
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

    /// Whether `path` (child of the folder `frames` describes) is ignored there. Nested repositories never are.
    #[must_use]
    pub fn is_ignored(&self, frames: &Frames, path: &Path, is_dir: bool) -> bool {
        let ignored = (is_dir && path.file_name().is_some_and(|n| n == DEPENDENCY_DIR))
            || self
                .extra
                .as_ref()
                .and_then(|g| verdict(g, path, is_dir))
                .unwrap_or(false)
            || frames
                .layers
                .iter()
                .rev()
                .chain(frames.tail.iter())
                .find_map(|layer| verdict(layer, path, is_dir))
                .unwrap_or(false);
        // a folder with its own `.git` is never hidden by the enclosing rules (looked at only when they hide it)
        ignored && !(is_dir && self.is_nested_repo(path))
    }

    /// Whether `abs` (inside the project folder) or one of its folders is ignored: for watch events.
    #[must_use]
    pub fn path_ignored(&self, abs: &Path, is_dir: bool) -> bool {
        let Ok(rest) = abs.strip_prefix(&self.root) else {
            return false;
        };
        let parts: Vec<_> = rest.components().collect();
        let mut frames = self.root_frames();
        let mut current = self.root.clone();
        for (at, part) in parts.iter().enumerate() {
            current.push(part);
            let last = at + 1 == parts.len();
            let dir = !last || is_dir;
            if self.is_ignored(&frames, &current, dir) {
                return true;
            }
            if dir && !last {
                frames = self.enter(&frames, &current);
            }
        }
        false
    }

    /// [`Ignorer::path_ignored`] for a path given below the root (`a/b/c.md`).
    #[must_use]
    pub fn rel_ignored(&self, rel: &str, is_dir: bool) -> bool {
        let mut abs = self.root.clone();
        abs.extend(rel.split('/').filter(|s| !s.is_empty()));
        self.path_ignored(&abs, is_dir)
    }

    /// Name of a file that, when it changes, makes everything read from ignore files stale.
    #[must_use]
    pub fn is_ignore_file(name: &str) -> bool {
        matches!(name, ".gitignore" | ".ignore" | "exclude")
    }
}
