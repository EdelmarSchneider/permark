//! Search: the index of one project (file list, sections by BM25, link index), kept by a background worker, and the
//! queries over it.

mod bm25;
mod fold;
mod fuzzy;
pub mod harvest;
mod index;
mod prepare;
#[cfg(test)]
mod tests;
mod worker;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use bm25::{Bm25, SectionInput};
pub use fold::{fold, is_cjk, stem, tokenize, Token, TokenKind};
pub use fuzzy::{fold_aligned, Match, Needle};
pub use prepare::{prepare, read_and_prepare, Prepared, Sig, MARKDOWN_LIMIT, TEXT_LIMIT};

use crate::events::EventSink;
use crate::format::FormatRegistry;
use crate::organizing::SilenceLookup;
use crate::paths::Vault;
use crate::tree::{Category, Explorer};
use crate::watch::FsBatch;
use index::Index;
use worker::{message_of, Msg, Worker, STACK};

/// Who reads the search answer. `Agent`: ranked as if the secret values were not in the files (`agent.md` Agent
/// search): a masked value is not guessable by asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reader {
    Person,
    Agent,
}

/// Most hits one query returns.
pub const MAX_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum IndexPhase {
    /// The first build is running: answers are partial.
    Building,
    Ready,
}

/// Where the index is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IndexStatus {
    pub phase: IndexPhase,
    /// Documents read so far / to read (the first build).
    pub done: u32,
    pub total: u32,
    /// Files in the list.
    pub files: u32,
    /// Sections indexed.
    pub sections: u32,
}

/// Event `index-progress` (all windows; a window compares `root` with its project's).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IndexProgress {
    pub root: String,
    pub phase: IndexPhase,
    pub done: u32,
    pub total: u32,
}

/// A piece of a snippet; `mark` = a word the query matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SnippetPart {
    pub text: String,
    pub mark: bool,
}

/// One matching section.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SectionHit {
    pub path: String,
    /// Ancestor headings, own heading last; empty for a preamble or a plain text file.
    pub heading_path: Vec<String>,
    /// Id of the heading in the reader.
    pub anchor: Option<String>,
    /// 1-based line of the heading (or of the first block of a preamble).
    pub line: u32,
    pub snippet: Vec<SnippetPart>,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SectionResults {
    pub hits: Vec<SectionHit>,
    /// The first build is still running.
    pub partial: bool,
}

/// A file found by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileHit {
    pub path: String,
    pub category: Category,
    /// Indices (characters) of the matched characters in `path`.
    pub positions: Vec<u32>,
    /// A dot file or a file inside a dot folder ([`is_hidden_path`]).
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderHit {
    pub path: String,
    pub positions: Vec<u32>,
    pub hidden: bool,
}

/// Whether a project path is hidden as the Explorer's switch means it: a segment below its root starts with `.` (the
/// `@wt/<name>/` prefix of a linked worktree is not a segment of the file's own path).
#[must_use]
pub fn is_hidden_path(path: &str) -> bool {
    let own = path
        .strip_prefix(crate::paths::WORKTREE_PREFIX)
        .and_then(|rest| rest.split_once('/').map(|(_, inner)| inner))
        .unwrap_or(path);
    own.split('/').any(|segment| segment.starts_with('.'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PathResults {
    pub files: Vec<FileHit>,
    pub folders: Vec<FolderHit>,
    pub partial: bool,
}

/// What Quick open lists before anything is typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Recent {
    pub opened: Vec<FileHit>,
    pub changed: Vec<FileHit>,
}

/// What the person asked of a search besides the words (the silence and tags plugins answer these).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchOptions {
    pub include_silenced: bool,
    pub tags: Vec<String>,
    /// Hidden paths (dot files | folders) rank after every other match: the window's Explorer does not show them.
    #[serde(default)]
    pub hidden_last: bool,
    /// "This project": only paths inside this included project (its subtree, projects inside it included).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project: Option<crate::projects::ProjectUid>,
}

/// Which documents may answer: the typed seam for the tags (`only`) and silence (`skip`, `muted`) plugins.
#[derive(Clone, Default)]
pub struct SearchFilter {
    pub only: Option<HashSet<String>>,
    pub skip: HashSet<String>,
    /// The silence plugin's answer: silenced paths do not answer either (asked per path, so a folder mark covers
    /// the files that appear later).
    pub muted: Option<Arc<dyn SilenceLookup>>,
    /// File and folder names: hidden paths after every other match.
    pub hidden_last: bool,
    /// Only paths at | below this key (a project's subtree).
    pub within: Option<String>,
}

impl std::fmt::Debug for SearchFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchFilter")
            .field("only", &self.only)
            .field("skip", &self.skip)
            .field("muted", &self.muted.is_some())
            .field("hidden_last", &self.hidden_last)
            .field("within", &self.within)
            .finish()
    }
}

impl SearchFilter {
    fn allows(&self, path: &str) -> bool {
        self.within
            .as_ref()
            .is_none_or(|base| crate::projects::map::within(&crate::paths::key_of(path), base))
            && self.only.as_ref().is_none_or(|only| only.contains(path))
            && !self.skip.contains(path)
            && !self.muted.as_ref().is_some_and(|m| m.is_silenced(path))
    }

    fn is_open(&self) -> bool {
        self.only.is_none() && self.skip.is_empty() && self.muted.is_none()
    }
}

#[derive(Debug)]
struct StatusInner {
    phase: IndexPhase,
    done: usize,
    total: usize,
}

/// What the worker and the queries share.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) index: RwLock<Index>,
    status: Mutex<StatusInner>,
    ready: Condvar,
    /// Front matter tags of the Markdown documents, from the same reads (`projects.md` §7.3).
    pub(crate) harvest: Arc<harvest::Harvest>,
}

impl Shared {
    fn new() -> Self {
        Self {
            harvest: Arc::new(harvest::Harvest::default()),
            index: RwLock::new(Index::new()),
            status: Mutex::new(StatusInner {
                phase: IndexPhase::Building,
                done: 0,
                total: 0,
            }),
            ready: Condvar::new(),
        }
    }

    fn status_lock(&self) -> std::sync::MutexGuard<'_, StatusInner> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn set_building(&self, done: usize, total: usize) {
        let mut status = self.status_lock();
        status.phase = IndexPhase::Building;
        status.done = done;
        status.total = total;
    }

    pub(crate) fn set_ready(&self) {
        let mut status = self.status_lock();
        status.phase = IndexPhase::Ready;
        status.done = status.total;
        self.ready.notify_all();
    }

    fn partial(&self) -> bool {
        self.status_lock().phase == IndexPhase::Building
    }
}

/// The index of one project: a worker thread builds it and follows the watch; queries read it.
pub struct SearchIndex {
    shared: Arc<Shared>,
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for SearchIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchIndex").finish_non_exhaustive()
    }
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl SearchIndex {
    /// Starts the worker: it walks the project and builds the index at once. `root` names the project in
    /// `index-progress` events. The Explorer receives the set of linked files as the worker learns it.
    #[must_use]
    pub fn start(
        vault: Vault,
        explorer: Arc<Explorer>,
        sink: Arc<dyn EventSink>,
        root: String,
    ) -> Self {
        let formats = Arc::new(std::sync::RwLock::new(Arc::new(FormatRegistry::core())));
        Self::start_with(vault, explorer, sink, root, formats)
    }

    /// [`SearchIndex::start`] with the formats of the project (config files are searched by their keys when a config
    /// format is on); the cell is read at every pass, so a change of plugins is seen after [`Self::formats_changed`].
    #[must_use]
    pub fn start_with(
        vault: Vault,
        explorer: Arc<Explorer>,
        sink: Arc<dyn EventSink>,
        root: String,
        formats: Arc<std::sync::RwLock<Arc<FormatRegistry>>>,
    ) -> Self {
        let shared = Arc::new(Shared::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel::<Msg>();
        let worker = Worker {
            shared: Arc::clone(&shared),
            vault,
            explorer,
            sink,
            root,
            stop: Arc::clone(&stop),
            formats,
        };
        let thread = std::thread::Builder::new()
            .name("permark-index".to_owned())
            .stack_size(STACK)
            .spawn(move || worker.run(&rx))
            .ok();
        Self {
            shared,
            tx,
            stop,
            thread: Mutex::new(thread),
        }
    }

    /// Front matter tags of the Markdown documents, taken from the index pass (`projects.md` §7.3 `HarvestApi`).
    #[must_use]
    pub fn harvest(&self) -> &Arc<harvest::Harvest> {
        &self.shared.harvest
    }

    /// The formats that are on changed: config files are read again for their keys.
    pub fn formats_changed(&self) {
        let _ = self.tx.send(Msg::FormatsChanged);
    }

    /// The function to subscribe to the project's watch with: batches go to the worker.
    pub fn feeder(&self) -> impl Fn(&FsBatch) + Send + Sync + 'static {
        let tx = self.tx.clone();
        move |batch: &FsBatch| {
            if !batch.is_empty() {
                let _ = tx.send(message_of(batch));
            }
        }
    }

    /// Waits until the first build ends (tests, measurements). `false`: timed out.
    #[must_use]
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let guard = self.shared.status_lock();
        let (guard, _) = self
            .shared
            .ready
            .wait_timeout_while(guard, timeout, |s| s.phase != IndexPhase::Ready)
            .unwrap_or_else(PoisonError::into_inner);
        guard.phase == IndexPhase::Ready
    }

    #[must_use]
    pub fn status(&self) -> IndexStatus {
        let (files, sections) = {
            let index = self.read();
            (index.files.len(), index.bm25.len())
        };
        let status = self.shared.status_lock();
        IndexStatus {
            phase: status.phase,
            done: to_u32(status.done),
            total: to_u32(status.total),
            files: to_u32(files),
            sections: to_u32(sections),
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Index> {
        self.shared
            .index
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Ranked sections for the words, as the person sees them. Empty query: nothing.
    #[must_use]
    pub fn sections(&self, query: &str, limit: usize, filter: &SearchFilter) -> SectionResults {
        self.sections_for(Reader::Person, query, limit, filter)
    }

    /// Ranked sections for the words, as `reader` sees them. Empty query: nothing.
    #[must_use]
    pub fn sections_for(
        &self,
        reader: Reader,
        query: &str,
        limit: usize,
        filter: &SearchFilter,
    ) -> SectionResults {
        let limit = limit.clamp(1, MAX_LIMIT);
        let allow = |path: &str| filter.allows(path);
        let hits = self.read().bm25.search_for(
            reader,
            query,
            limit,
            if filter.is_open() { None } else { Some(&allow) },
        );
        SectionResults {
            hits,
            partial: self.shared.partial(),
        }
    }

    /// Files and folders whose path matches the (fuzzy) words, best first.
    #[must_use]
    pub fn paths(
        &self,
        query: &str,
        limit: usize,
        folder_limit: usize,
        filter: &SearchFilter,
    ) -> PathResults {
        let limit = limit.clamp(1, MAX_LIMIT);
        let Some(needle) = Needle::new(query) else {
            return PathResults {
                files: Vec::new(),
                folders: Vec::new(),
                partial: self.shared.partial(),
            };
        };
        let index = self.read();
        // hidden paths second (before the limit cuts), when the person's tree does not show them
        let last = |path: &str| filter.hidden_last && is_hidden_path(path);
        let mut found: Vec<(f32, u64, &str, &index::FileEntry, Vec<u32>)> = Vec::new();
        for (path, entry) in &index.files {
            if !filter.allows(path) {
                continue;
            }
            let folded = entry.folded.as_deref().unwrap_or(path);
            if let Some(m) = needle.matches(folded) {
                found.push((m.score, entry.mtime_ms, path, entry, m.positions));
            }
        }
        found.sort_by(|a, b| {
            last(a.2)
                .cmp(&last(b.2))
                .then(b.0.total_cmp(&a.0))
                .then(b.1.cmp(&a.1))
                .then(a.2.len().cmp(&b.2.len()))
                .then(a.2.cmp(b.2))
        });
        found.truncate(limit);
        let files = found
            .into_iter()
            .map(|(_, _, path, entry, positions)| FileHit {
                path: path.to_owned(),
                category: entry.category,
                positions,
                hidden: is_hidden_path(path),
            })
            .collect();

        let mut folders: Vec<(f32, &str, Vec<u32>)> = Vec::new();
        if folder_limit > 0 {
            for folder in index.folders.keys() {
                let folded = fuzzy::fold_aligned(folder);
                if let Some(m) = needle.matches(&folded) {
                    folders.push((m.score, folder, m.positions));
                }
            }
            folders.sort_by(|a, b| {
                last(a.1)
                    .cmp(&last(b.1))
                    .then(b.0.total_cmp(&a.0))
                    .then(a.1.len().cmp(&b.1.len()))
                    .then(a.1.cmp(b.1))
            });
            folders.truncate(folder_limit);
        }
        PathResults {
            files,
            folders: folders
                .into_iter()
                .map(|(_, path, positions)| FolderHit {
                    path: path.to_owned(),
                    positions,
                    hidden: is_hidden_path(path),
                })
                .collect(),
            partial: self.shared.partial(),
        }
    }

    /// The files Quick open lists for an empty box: the given (opened) paths that exist, in order, and the files with
    /// the newest modification time.
    #[must_use]
    pub fn recent(&self, opened: &[String], limit: usize, filter: &SearchFilter) -> Recent {
        let limit = limit.clamp(1, MAX_LIMIT);
        let index = self.read();
        let hit = |path: &str, entry: &index::FileEntry| FileHit {
            path: path.to_owned(),
            category: entry.category,
            positions: Vec::new(),
            hidden: is_hidden_path(path),
        };
        let opened_hits: Vec<FileHit> = opened
            .iter()
            .filter(|p| filter.allows(p))
            .filter_map(|p| index.files.get_key_value(p.as_str()))
            .take(limit)
            .map(|(path, entry)| hit(path, entry))
            .collect();
        let mut newest: Vec<(&String, &index::FileEntry)> = index
            .files
            .iter()
            .filter(|(path, entry)| entry.mtime_ms > 0 && filter.allows(path))
            .collect();
        let order = |a: &(&String, &index::FileEntry), b: &(&String, &index::FileEntry)| {
            b.1.mtime_ms.cmp(&a.1.mtime_ms).then(a.0.cmp(b.0))
        };
        if newest.len() > limit {
            newest.select_nth_unstable_by(limit, order);
            newest.truncate(limit);
        }
        newest.sort_by(order);
        Recent {
            opened: opened_hits,
            changed: newest.into_iter().map(|(p, e)| hit(p, e)).collect(),
        }
    }

    /// The files of the project below `folder` (`""`: all), sorted by path. Ignored files are not in the index.
    #[must_use]
    pub fn files_under(&self, folder: &str) -> Vec<String> {
        let index = self.read();
        if folder.is_empty() {
            return index.files.keys().cloned().collect();
        }
        let prefix = format!("{folder}/");
        index
            .files
            .range(prefix.clone()..)
            .take_while(|(p, _)| p.starts_with(&prefix))
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// The documents that link to `path` (their own links, each document once), sorted by path.
    #[must_use]
    pub fn linking_to(&self, path: &str) -> Vec<String> {
        let mut found = self.read().linking_to(&crate::tree::key_of(path));
        found.sort();
        found
    }

    /// Link targets (keys) documents point at: tests and measurements.
    #[must_use]
    pub fn linked(&self) -> HashSet<String> {
        self.read().linked_now()
    }

    /// Number of documents whose content is indexed.
    #[must_use]
    pub fn doc_count(&self) -> usize {
        self.read().doc_count()
    }
}

impl Drop for SearchIndex {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Msg::Stop);
        let handle = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}
