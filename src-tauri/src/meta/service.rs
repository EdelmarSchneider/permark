//! The metadata of one open project: the app's tags and descriptions (kept in memory, written to
//! `meta.toml` a moment after the last change), the tags found in front matter, and the answers the
//! UI asks for.
//!
//! Where the file lives follows the state store: `.trailmark/meta.toml` once the project has a
//! `.trailmark/` folder, a file in AppData before that. The watcher sees `.trailmark/meta.toml`: its
//! own writes are recognised by their content and hidden, an edit made by anyone else is read in.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::catalog::{combine_tags, Catalog, IndexEntry, Row, TagCount, TagRef, TaggedFile};
use super::filetags;
use super::store::{Entries, Problem};
use crate::util::{hash_bytes, modified_ms, write_atomic};
use crate::vault::{key, NodeKind, Vault, VaultError, WORKTREE_PREFIX};
use crate::watch::FsChange;

/// `meta.toml` inside a project that has a `.trailmark/` folder, as a vault path.
pub const PROJECT_META_PATH: &str = ".trailmark/meta.toml";

#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    /// The file cannot be read (a merge conflict, a bad edit): the app's tags are read-only.
    #[error("{0}")]
    Unreadable(Problem),
    #[error("tags are not stored for files of linked worktrees: {0}")]
    Worktree(String),
    #[error("cannot access {path}: {source}")]
    Io { path: String, source: io::Error },
    #[error(transparent)]
    Vault(#[from] VaultError),
}

impl MetaError {
    /// A short stable identifier for the UI.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Unreadable(_) => "metaUnreadable",
            Self::Worktree(_) => "metaWorktree",
            Self::Io { .. } => "io",
            Self::Vault(e) => e.kind(),
        }
    }

    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "meta.toml can't be read (line {line}): {}", self.message),
            None => write!(f, "meta.toml can't be read: {}", self.message),
        }
    }
}

/// How long writes wait for more changes.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Written this long after the last change...
    pub quiet: Duration,
    /// ...but never later than this after the first one.
    pub max_wait: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(300),
            max_wait: Duration::from_secs(2),
        }
    }
}

/// Matches the UI's `FileMeta`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    pub path: String,
    pub tags: Vec<TagRef>,
    pub description: String,
    /// False when the app's tags cannot be changed for this file right now.
    pub writable: bool,
}

/// Matches the UI's `MetaStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaStatus {
    /// Set when `meta.toml` cannot be read: the app's tags are read-only until it is fixed.
    pub problem: Option<Problem>,
    /// Set when the last attempt to write `meta.toml` failed.
    pub write_error: Option<String>,
}

/// Matches the UI's `TagOverview`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagOverview {
    pub tags: Vec<TagCount>,
    pub untagged: usize,
    /// Entries kept for files that no longer exist.
    pub missing: usize,
    pub status: MetaStatus,
}

/// What a look at `meta.toml` after the watcher reported it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskEvent {
    /// The file holds what this app wrote or read last: nothing happened.
    Echo,
    /// Someone else changed it, and it has been read again.
    Reloaded,
    /// Not watched, or it could not be looked at.
    Ignored,
}

struct Inner {
    path: PathBuf,
    /// Inside the project, where the watcher reports changes.
    watched: bool,
    entries: Entries,
    problem: Option<Problem>,
    write_error: Option<String>,
    /// Hash of the file's content as last read or written (`None`: there was no file).
    disk_hash: Option<u64>,
    /// Paths changed here and not written yet.
    dirty: BTreeSet<String>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The file's bytes, or `None` when it does not exist.
fn read_disk(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match crate::util::read_with_retry(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// What the bytes of a `meta.toml` mean: the entries, or the problem with them, and their hash.
fn parse_bytes(bytes: Option<&[u8]>) -> (Entries, Option<Problem>, Option<u64>) {
    let Some(bytes) = bytes else {
        return (Entries::default(), None, None);
    };
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    match Entries::parse(text) {
        Ok(entries) => (entries, None, Some(hash_bytes(bytes))),
        Err(problem) => (Entries::default(), Some(problem), Some(hash_bytes(bytes))),
    }
}

impl Inner {
    fn load(path: PathBuf, watched: bool) -> Self {
        let mut inner = Self {
            path,
            watched,
            entries: Entries::default(),
            problem: None,
            write_error: None,
            disk_hash: None,
            dirty: BTreeSet::new(),
        };
        match read_disk(&inner.path) {
            Ok(bytes) => inner.adopt(bytes.as_deref()),
            Err(e) => {
                inner.problem = Some(Problem {
                    message: e.to_string(),
                    line: None,
                });
            }
        }
        inner
    }

    /// Takes the content of the file as the truth, then lays what was changed here and not written
    /// yet on top of it.
    fn adopt(&mut self, bytes: Option<&[u8]>) {
        let mine = std::mem::take(&mut self.entries);
        let (entries, problem, hash) = parse_bytes(bytes);
        self.entries = entries;
        self.problem = problem;
        self.disk_hash = hash;
        if self.problem.is_some() {
            self.dirty.clear();
            return;
        }
        for path in &self.dirty {
            match mine.get(path) {
                Some(entry) => self.entries.put(path, entry.clone()),
                None => {
                    self.entries.remove(path);
                }
            }
        }
    }

    fn check_writable(&self) -> Result<(), MetaError> {
        match &self.problem {
            Some(p) => Err(MetaError::Unreadable(p.clone())),
            None => Ok(()),
        }
    }
}

/// Writes `meta.toml` if anything changed. Returns whether the UI should hear about it (the file had
/// been changed by someone else in the meantime, or writing failed or recovered).
fn flush(shared: &Mutex<Inner>) -> bool {
    let mut g = lock(shared);
    if g.dirty.is_empty() {
        return false;
    }
    if g.problem.is_some() {
        g.dirty.clear();
        return false;
    }
    let had_error = g.write_error.is_some();
    // An edit made outside the app since we last looked wins the race with our delay: take it in,
    // then lay our own changes on top.
    let on_disk = match read_disk(&g.path) {
        Ok(bytes) => bytes,
        Err(e) => {
            g.write_error = Some(e.to_string());
            return !had_error;
        }
    };
    let mut changed = false;
    if on_disk.as_deref().map(hash_bytes) != g.disk_hash {
        g.adopt(on_disk.as_deref());
        changed = true;
        if g.problem.is_some() {
            return true;
        }
    }
    let text = g.entries.to_toml();
    match write_atomic(&g.path, text.as_bytes()) {
        Ok(()) => {
            g.disk_hash = Some(hash_bytes(text.as_bytes()));
            g.write_error = None;
            g.dirty.clear();
        }
        Err(e) => g.write_error = Some(e.to_string()),
    }
    changed || had_error != g.write_error.is_some()
}

enum Msg {
    /// Something changed: write after the quiet period.
    Touch,
    Stop,
}

/// The thread that writes `meta.toml` once changes have settled.
fn writer(
    rx: &mpsc::Receiver<Msg>,
    shared: &Mutex<Inner>,
    notify: &(dyn Fn() + Send + Sync),
    timing: Timing,
) {
    let settle = || {
        if flush(shared) {
            notify();
        }
    };
    'idle: while let Ok(first) = rx.recv() {
        if matches!(first, Msg::Stop) {
            break;
        }
        let began = Instant::now();
        loop {
            let left = timing
                .max_wait
                .saturating_sub(began.elapsed())
                .min(timing.quiet);
            match rx.recv_timeout(left) {
                Ok(Msg::Touch) if began.elapsed() < timing.max_wait => {}
                Ok(Msg::Touch) | Err(RecvTimeoutError::Timeout) => {
                    settle();
                    break;
                }
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break 'idle,
            }
        }
    }
    settle();
}

/// A file's front matter tags, as they were when its size and modification time were last seen.
struct Seen {
    len: u64,
    mtime_ms: u64,
    tags: Vec<String>,
}

pub struct MetaService {
    vault: Arc<Vault>,
    inner: Arc<Mutex<Inner>>,
    notify: Arc<dyn Fn() + Send + Sync>,
    tx: Sender<Msg>,
    handle: Mutex<Option<JoinHandle<()>>>,
    seen: Mutex<HashMap<String, Seen>>,
    catalog: Mutex<Option<Arc<Catalog>>>,
}

impl MetaService {
    /// Loads `path` (a missing file is an empty store) and starts the writer thread. `watched` says
    /// the file is inside the project, where the watcher reports changes to it. `notify` is called
    /// whenever the metadata changes.
    pub fn open(
        vault: Arc<Vault>,
        path: PathBuf,
        watched: bool,
        timing: Timing,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let inner = Arc::new(Mutex::new(Inner::load(path, watched)));
        let (tx, rx) = mpsc::channel();
        let handle = {
            let (inner, notify) = (Arc::clone(&inner), Arc::clone(&notify));
            thread::Builder::new()
                .name("trailmark-meta".to_owned())
                .spawn(move || writer(&rx, &inner, notify.as_ref(), timing))
                .ok()
        };
        Self {
            vault,
            inner,
            notify,
            tx,
            handle: Mutex::new(handle),
            seen: Mutex::new(HashMap::new()),
            catalog: Mutex::new(None),
        }
    }

    // ---- reading ------------------------------------------------------------------------------

    pub fn status(&self) -> MetaStatus {
        let g = lock(&self.inner);
        MetaStatus {
            problem: g.problem.clone(),
            write_error: g.write_error.clone(),
        }
    }

    /// The front matter tags of a file, read again only when its size or time changed.
    fn file_tags(&self, rel: &str, abs: &Path) -> Vec<String> {
        if !filetags::has_front_matter(rel) {
            return Vec::new();
        }
        let Ok(meta) = std::fs::metadata(abs) else {
            return Vec::new();
        };
        let (len, mtime_ms) = (meta.len(), modified_ms(&meta));
        let mut seen = lock(&self.seen);
        if let Some(s) = seen
            .get(&key(rel))
            .filter(|s| s.len == len && s.mtime_ms == mtime_ms)
        {
            return s.tags.clone();
        }
        let tags = filetags::read(abs);
        seen.insert(
            key(rel),
            Seen {
                len,
                mtime_ms,
                tags: tags.clone(),
            },
        );
        tags
    }

    /// The tags and description of one file (a path of the UI, resolved here).
    pub fn file_meta(&self, path: &str) -> Result<FileMeta, MetaError> {
        let resolved = self.vault.resolve(path)?;
        let in_worktree = resolved.root != 0;
        let (app_tags, description, readable) = {
            let g = lock(&self.inner);
            let known = g.entries.get(&resolved.rel).filter(|_| !in_worktree);
            (
                known.map(|e| e.tags.clone()).unwrap_or_default(),
                known.map(|e| e.description.clone()).unwrap_or_default(),
                g.problem.is_none(),
            )
        };
        let file_tags = self.file_tags(&resolved.rel, &resolved.abs);
        Ok(FileMeta {
            path: resolved.rel,
            tags: combine_tags(&file_tags, &app_tags),
            description,
            writable: !in_worktree && readable,
        })
    }

    // ---- changing -----------------------------------------------------------------------------

    /// A path of the main folder, ready to be a key of the store.
    fn own_path(&self, path: &str) -> Result<String, MetaError> {
        let resolved = self.vault.resolve(path)?;
        if resolved.root != 0 {
            return Err(MetaError::Worktree(resolved.rel));
        }
        Ok(resolved.rel)
    }

    /// Replaces the tags the app keeps for a file (the front matter's are not touched).
    pub fn set_tags(&self, path: &str, tags: &[String]) -> Result<FileMeta, MetaError> {
        let rel = self.own_path(path)?;
        self.change(&rel, |entries| entries.set_tags(&rel, tags))?;
        self.file_meta(&rel)
    }

    /// Replaces the description of a file.
    pub fn set_description(&self, path: &str, description: &str) -> Result<FileMeta, MetaError> {
        let rel = self.own_path(path)?;
        self.change(&rel, |entries| entries.set_description(&rel, description))?;
        self.file_meta(&rel)
    }

    fn change(&self, rel: &str, apply: impl FnOnce(&mut Entries)) -> Result<(), MetaError> {
        {
            let mut g = lock(&self.inner);
            g.check_writable()?;
            apply(&mut g.entries);
            g.dirty.insert(rel.to_owned());
        }
        self.changed();
        Ok(())
    }

    /// Schedules the write and tells the UI.
    fn changed(&self) {
        self.invalidate();
        let _ = self.tx.send(Msg::Touch);
        (self.notify)();
    }

    /// The tags of a file that was renamed or moved go with it.
    pub fn rename(&self, from: &str, to: &str) {
        let moved = {
            let mut g = lock(&self.inner);
            if g.problem.is_some() || !g.entries.rename(from, to) {
                false
            } else {
                g.dirty.insert(from.to_owned());
                g.dirty.insert(to.to_owned());
                true
            }
        };
        if moved {
            self.changed();
        }
    }

    /// Entries of files that are gone (kept, because a checkout may bring the files back).
    fn missing_paths(&self) -> Vec<String> {
        let paths: Vec<String> = lock(&self.inner)
            .entries
            .iter()
            .map(|(p, _)| p.to_owned())
            .collect();
        paths
            .into_iter()
            .filter(|p| {
                self.vault
                    .resolve(p)
                    .map_or(true, |r| std::fs::symlink_metadata(r.abs).is_err())
            })
            .collect()
    }

    /// Removes the entries of files that are gone. Returns how many.
    pub fn clean_up(&self) -> Result<usize, MetaError> {
        let missing = self.missing_paths();
        if missing.is_empty() {
            return Ok(0);
        }
        {
            let mut g = lock(&self.inner);
            g.check_writable()?;
            for path in &missing {
                g.entries.remove(path);
                g.dirty.insert(path.clone());
            }
        }
        self.changed();
        Ok(missing.len())
    }

    // ---- the whole project --------------------------------------------------------------------

    /// Forgets the answers computed from the files (they changed, or may have).
    pub fn invalidate(&self) {
        *lock(&self.catalog) = None;
    }

    /// Every file of the folder with its tags. Computed once, until [`MetaService::invalidate`].
    pub fn catalog(&self) -> Arc<Catalog> {
        if let Some(c) = lock(&self.catalog).as_ref() {
            return Arc::clone(c);
        }
        let catalog = Arc::new(self.scan());
        *lock(&self.catalog) = Some(Arc::clone(&catalog));
        catalog
    }

    fn scan(&self) -> Catalog {
        let entries = lock(&self.inner).entries.clone();
        let mut rows = Vec::new();
        let mut present = HashSet::new();
        for item in self.vault.walk() {
            if item.kind != NodeKind::File || item.rel.starts_with(WORKTREE_PREFIX) {
                continue;
            }
            let entry = entries.get(&item.rel);
            let file_tags = self.file_tags(&item.rel, &item.abs);
            let modified = if entry.is_some() || !file_tags.is_empty() {
                std::fs::metadata(&item.abs).map_or(0, |m| modified_ms(&m))
            } else {
                0
            };
            present.insert(key(&item.rel));
            rows.push(Row {
                app_tags: entry.map(|e| e.tags.clone()).unwrap_or_default(),
                description: entry.map(|e| e.description.clone()).unwrap_or_default(),
                file_tags,
                modified_ms: modified,
                path: item.rel,
            });
        }
        lock(&self.seen).retain(|k, _| present.contains(k));
        Catalog::new(rows)
    }

    pub fn overview(&self) -> TagOverview {
        let catalog = self.catalog();
        TagOverview {
            tags: catalog.tag_counts(),
            untagged: catalog.untagged_count(),
            missing: self.missing_paths().len(),
            status: self.status(),
        }
    }

    pub fn files_with_tags(&self, tags: &[String], untagged: bool) -> Vec<TaggedFile> {
        self.catalog().files_with(tags, untagged)
    }

    pub fn index(&self) -> Vec<IndexEntry> {
        self.catalog().index()
    }

    // ---- the file on disk ---------------------------------------------------------------------

    /// Whether `path` is the project's `meta.toml`.
    fn is_meta_file(path: &str) -> bool {
        key(path) == key(PROJECT_META_PATH)
    }

    /// Looks at `meta.toml` after the watcher said something happened to it.
    pub fn on_disk_event(&self) -> DiskEvent {
        let event = {
            let mut g = lock(&self.inner);
            if !g.watched {
                return DiskEvent::Ignored;
            }
            let Ok(bytes) = read_disk(&g.path) else {
                return DiskEvent::Ignored;
            };
            if bytes.as_deref().map(hash_bytes) == g.disk_hash {
                DiskEvent::Echo
            } else {
                g.adopt(bytes.as_deref());
                DiskEvent::Reloaded
            }
        };
        if event == DiskEvent::Reloaded {
            self.changed();
        }
        event
    }

    /// What the watcher reported means for the metadata: renamed files take their tags along, the
    /// file tags may have changed, and an edit of `meta.toml` is read in. Returns whether the changes
    /// to `meta.toml` were only this app's own writes, which are not news.
    pub fn on_fs_changes(&self, changes: &[FsChange]) -> bool {
        self.invalidate();
        for change in changes {
            if let Some(from) = &change.from {
                self.rename(from, &change.path);
            }
        }
        let touched = changes.iter().any(|c| {
            Self::is_meta_file(&c.path) || c.from.as_deref().is_some_and(Self::is_meta_file)
        });
        touched && self.on_disk_event() == DiskEvent::Echo
    }

    /// Drops the changes to `meta.toml` from a list.
    pub fn without_meta_file(changes: &mut Vec<FsChange>) {
        changes.retain(|c| !Self::is_meta_file(&c.path));
    }

    /// The project got a `.trailmark/` folder: the metadata moves from AppData to `new_path`. What a
    /// teammate may already have committed there is kept, and what was added here is merged in.
    pub fn migrate_to(&self, new_path: PathBuf, watched: bool) -> Result<(), MetaError> {
        if flush(&self.inner) {
            (self.notify)();
        }
        {
            let mut g = lock(&self.inner);
            if g.path == new_path {
                g.watched = watched;
                return Ok(());
            }
            let existing = read_disk(&new_path).map_err(|e| MetaError::io(&new_path, e))?;
            let (mut merged, problem, existing_hash) = parse_bytes(existing.as_deref());
            if let Some(problem) = problem {
                return Err(MetaError::Unreadable(problem));
            }
            let theirs = merged.clone();
            merged.merge(g.entries.clone());
            let hash = if merged == theirs {
                existing_hash // nothing to add: the file stays as it is (or stays absent)
            } else {
                let text = merged.to_toml();
                write_atomic(&new_path, text.as_bytes())
                    .map_err(|e| MetaError::io(&new_path, e))?;
                Some(hash_bytes(text.as_bytes()))
            };
            let old = std::mem::replace(&mut g.path, new_path);
            if g.problem.is_none() {
                // Moved, not copied. A file that could not be read is left where it is.
                let _ = std::fs::remove_file(old);
            }
            g.watched = watched;
            g.entries = merged;
            g.problem = None;
            g.disk_hash = hash;
            g.dirty.clear();
        }
        self.changed();
        Ok(())
    }

    /// Writes what is pending and stops the writer thread. Safe to call more than once.
    pub fn stop(&self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(handle) = lock(&self.handle).take() {
            let _ = handle.join();
        }
        flush(&self.inner);
    }
}

impl Drop for MetaService {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

#[cfg(test)]
mod tests;
