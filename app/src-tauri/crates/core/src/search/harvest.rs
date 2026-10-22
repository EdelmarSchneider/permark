//! Front matter tags taken from the index pass (`projects.md` §7.3): the index reads every Markdown document once in
//! the background; its front matter tags are kept here, so the `tags` plugin never reads the documents itself (no
//! second full read, nothing on the UI path). A small cache per owning project (`<cache>/index/fm-tags.json`) shows
//! tags at once on the next open, before the pass ends; only changed files are read again (watch).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, RwLock, Weak};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::paths::{key_of, WORKTREE_PREFIX};
use crate::projects::Owners;
use crate::state::{Loaded, StateData};

/// The cache file, below each owner's cache folder.
pub const CACHE_FILE: &str = "index/fm-tags.json";
/// While the first pass runs, listeners hear at most this often.
const BUILD_CADENCE: Duration = Duration::from_secs(1);
/// The cache is written this long after the last change.
pub const CACHE_QUIET: Duration = Duration::from_secs(2);

/// The front matter tags of one Markdown document, with the stamp they were read at.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocTags {
    pub tags: Vec<String>,
    pub len: u64,
    pub mtime_ms: u64,
}

/// `fm-tags.json`: owner-relative path -> tags (droppable cache).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CacheFile(BTreeMap<String, DocTags>);

impl StateData for CacheFile {
    const VERSION: u32 = 1;
}

/// The documents with their front matter tags, by root-relative path (main root only).
pub type HarvestDocs = HashMap<String, DocTags>;

type Listener = Arc<dyn Fn() + Send + Sync>;
/// Where the cache goes: the owners now (the opened folder + included projects).
pub type OwnersSource = Arc<dyn Fn() -> Owners + Send + Sync>;

#[derive(Default)]
struct Cache {
    source: Option<OwnersSource>,
    dirty: bool,
    changed_at: Option<Instant>,
}

/// The harvest of one project (owned by its search index).
pub struct Harvest {
    docs: RwLock<Arc<HarvestDocs>>,
    ready: Mutex<bool>,
    ready_cv: Condvar,
    rev: AtomicU64,
    listeners: Mutex<Vec<(u64, Listener)>>,
    next: AtomicU64,
    last_told: Mutex<Option<Instant>>,
    pending_tell: AtomicBool,
    cache: Mutex<Cache>,
}

impl std::fmt::Debug for Harvest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Harvest").finish_non_exhaustive()
    }
}

/// Hearing harvest changes until dropped.
pub struct HarvestSubscription {
    owner: Weak<Harvest>,
    id: u64,
}

impl Drop for HarvestSubscription {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            lock(&owner.listeners).retain(|(id, _)| *id != self.id);
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Default for Harvest {
    fn default() -> Self {
        Self {
            docs: RwLock::new(Arc::new(HashMap::new())),
            ready: Mutex::new(false),
            ready_cv: Condvar::new(),
            rev: AtomicU64::new(1),
            listeners: Mutex::new(Vec::new()),
            next: AtomicU64::new(1),
            last_told: Mutex::new(None),
            pending_tell: AtomicBool::new(false),
            cache: Mutex::new(Cache::default()),
        }
    }
}

impl Harvest {
    // ---- readers (the `tags` plugin) -------------------------------------------------------------------------------

    /// Every Markdown document of the main root with its front matter tags (an empty list = none). Cheap (shared).
    #[must_use]
    pub fn snapshot(&self) -> Arc<HarvestDocs> {
        Arc::clone(&self.docs.read().unwrap_or_else(PoisonError::into_inner))
    }

    #[must_use]
    pub fn tags_of(&self, path: &str) -> Option<Vec<String>> {
        self.docs
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path)
            .map(|d| d.tags.clone())
    }

    /// Goes up at every change.
    #[must_use]
    pub fn rev(&self) -> u64 {
        self.rev.load(Ordering::Relaxed)
    }

    /// The first pass ended (until then the harvest is what the cache and the pass found so far).
    #[must_use]
    pub fn ready(&self) -> bool {
        *lock(&self.ready)
    }

    /// Waits up to `timeout` for the first pass.
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let ready = lock(&self.ready);
        let (ready, _) = self
            .ready_cv
            .wait_timeout_while(ready, timeout, |r| !*r)
            .unwrap_or_else(PoisonError::into_inner);
        *ready
    }

    /// Hears changes (while the first pass runs: at most once a second) until the subscription goes. While anyone
    /// listens, the cache is kept on disk.
    #[must_use]
    pub fn subscribe(
        self: &Arc<Self>,
        listener: impl Fn() + Send + Sync + 'static,
    ) -> HarvestSubscription {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        lock(&self.listeners).push((id, Arc::new(listener)));
        HarvestSubscription {
            owner: Arc::downgrade(self),
            id,
        }
    }

    // ---- the index pass ------------------------------------------------------------------------------------------

    /// The cache's places (`None`: no cache, a reader with no window). The cache of every owner is read now: what it
    /// holds shows at once, the pass replaces it as it reads.
    pub fn use_cache(&self, source: Option<OwnersSource>) {
        if let Some(source) = &source {
            let owners = source();
            let mut found: Vec<(String, DocTags)> = Vec::new();
            for (i, slot) in owners.slots().iter().enumerate() {
                if let Loaded::Found(file) = slot.store.cache_file::<CacheFile>(CACHE_FILE).load() {
                    found.extend(
                        file.0
                            .into_iter()
                            .map(|(rel, doc)| (owners.join(i, &rel), doc))
                            .filter(|(path, _)| owners.owner_of(path) == i),
                    );
                }
            }
            if !found.is_empty() {
                let mut docs = self.docs.write().unwrap_or_else(PoisonError::into_inner);
                let map = Arc::make_mut(&mut docs);
                for (path, doc) in found {
                    map.entry(path).or_insert(doc);
                }
                drop(docs);
                self.bumped(false);
            }
        }
        lock(&self.cache).source = source;
    }

    /// Documents the pass read (Markdown, main root): their tags now.
    pub(crate) fn put(&self, batch: Vec<(String, DocTags)>, building: bool) {
        let batch: Vec<(String, DocTags)> = batch
            .into_iter()
            .filter(|(p, _)| !p.starts_with(WORKTREE_PREFIX))
            .collect();
        if batch.is_empty() {
            return;
        }
        let changed = {
            let mut docs = self.docs.write().unwrap_or_else(PoisonError::into_inner);
            let differs = batch.iter().any(|(p, d)| docs.get(p) != Some(d));
            if differs {
                let map = Arc::make_mut(&mut docs);
                for (path, doc) in batch {
                    map.insert(path, doc);
                }
            }
            differs
        };
        if changed {
            self.bumped(building);
        }
    }

    /// A file | folder left (or was renamed away).
    pub(crate) fn remove(&self, path: &str, is_dir: bool) {
        let changed = {
            let mut docs = self.docs.write().unwrap_or_else(PoisonError::into_inner);
            if is_dir {
                let key = key_of(path);
                let below: Vec<String> = docs
                    .keys()
                    .filter(|p| crate::projects::map::within(&key_of(p), &key))
                    .cloned()
                    .collect();
                if below.is_empty() {
                    false
                } else {
                    let map = Arc::make_mut(&mut docs);
                    for p in below {
                        map.remove(&p);
                    }
                    true
                }
            } else if docs.contains_key(path) {
                Arc::make_mut(&mut docs).remove(path);
                true
            } else {
                false
            }
        };
        if changed {
            self.bumped(false);
        }
    }

    /// The first pass saw the whole disk: entries of files that are not there (known from the cache only) go.
    pub(crate) fn keep_only(&self, on_disk: &HashSet<&str>) {
        let gone: Vec<String> = self
            .snapshot()
            .keys()
            .filter(|p| !on_disk.contains(p.as_str()))
            .cloned()
            .collect();
        if gone.is_empty() {
            return;
        }
        {
            let mut docs = self.docs.write().unwrap_or_else(PoisonError::into_inner);
            let map = Arc::make_mut(&mut docs);
            for p in gone {
                map.remove(&p);
            }
        }
        self.bumped(true);
    }

    /// The first pass ended: listeners hear it at once; the cache is written.
    pub(crate) fn set_ready(&self) {
        *lock(&self.ready) = true;
        self.ready_cv.notify_all();
        self.pending_tell.store(true, Ordering::Relaxed);
        self.tell(false);
        self.write_cache();
    }

    fn bumped(&self, building: bool) {
        self.rev.fetch_add(1, Ordering::Relaxed);
        {
            let mut cache = lock(&self.cache);
            cache.dirty = true;
            cache.changed_at = Some(Instant::now());
        }
        self.pending_tell.store(true, Ordering::Relaxed);
        self.tell(building);
    }

    /// Tells the listeners (while building: at most once per [`BUILD_CADENCE`]; the rest at the next batch | ready).
    pub(crate) fn tell(&self, building: bool) {
        if !self.pending_tell.load(Ordering::Relaxed) {
            return;
        }
        {
            let mut last = lock(&self.last_told);
            if building && last.is_some_and(|at| at.elapsed() < BUILD_CADENCE) {
                return;
            }
            *last = Some(Instant::now());
        }
        self.pending_tell.store(false, Ordering::Relaxed);
        let listeners: Vec<Listener> = lock(&self.listeners)
            .iter()
            .map(|(_, l)| Arc::clone(l))
            .collect();
        for listener in listeners {
            listener();
        }
    }

    /// How long until the cache is due (`None`: nothing to write, or nobody listens).
    pub(crate) fn cache_due(&self) -> Option<Duration> {
        let cache = lock(&self.cache);
        if !cache.dirty || cache.source.is_none() || lock(&self.listeners).is_empty() {
            return None;
        }
        Some(cache.changed_at.map_or(Duration::ZERO, |at| {
            CACHE_QUIET.saturating_sub(at.elapsed())
        }))
    }

    /// Writes the cache, split by owner (best effort: a cache that cannot be written is rebuilt next open). Only while
    /// someone listens (the `tags` plugin off leaves no cache written).
    pub(crate) fn write_cache(&self) {
        let source = {
            let mut cache = lock(&self.cache);
            if !cache.dirty || lock(&self.listeners).is_empty() {
                return;
            }
            let Some(source) = cache.source.clone() else {
                return;
            };
            cache.dirty = false;
            source
        };
        let owners = source();
        let docs = self.snapshot();
        let mut parts: Vec<BTreeMap<String, DocTags>> =
            (0..owners.slots().len()).map(|_| BTreeMap::new()).collect();
        for (path, doc) in docs.iter() {
            let i = owners.owner_of(path);
            parts[i].insert(owners.rel(i, path), doc.clone());
        }
        for (slot, part) in owners.slots().iter().zip(parts) {
            let file = slot.store.cache_file::<CacheFile>(CACHE_FILE);
            if part.is_empty() && !file.path().exists() {
                continue;
            }
            let _ = file.save(&CacheFile(part));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppDirs, StateStore};

    fn doc(tags: &[&str], len: u64) -> DocTags {
        DocTags {
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            len,
            mtime_ms: 1,
        }
    }

    #[test]
    fn put_remove_keep_only_and_listeners_cadence() {
        let h = Arc::new(Harvest::default());
        let heard = Arc::new(AtomicU64::new(0));
        let _sub = h.subscribe({
            let heard = Arc::clone(&heard);
            move || {
                heard.fetch_add(1, Ordering::Relaxed);
            }
        });
        h.put(
            vec![
                ("a.md".into(), doc(&["x"], 1)),
                ("@wt/f/b.md".into(), doc(&["y"], 1)),
            ],
            true,
        );
        assert_eq!(h.tags_of("a.md").unwrap(), ["x"]);
        assert!(h.tags_of("@wt/f/b.md").is_none(), "main root only");
        h.put(vec![("d/c.md".into(), doc(&[], 2))], true);
        assert_eq!(heard.load(Ordering::Relaxed), 1, "building: once a second");
        let rev = h.rev();
        h.put(vec![("d/c.md".into(), doc(&[], 2))], false);
        assert_eq!(h.rev(), rev, "same content: no change");
        h.set_ready();
        assert_eq!(heard.load(Ordering::Relaxed), 2);
        h.remove("d", true);
        assert!(h.tags_of("d/c.md").is_none());
        assert_eq!(heard.load(Ordering::Relaxed), 3, "after ready: at once");
        h.put(vec![("gone.md".into(), doc(&["z"], 1))], false);
        h.keep_only(&["a.md"].into_iter().collect());
        assert_eq!(h.snapshot().len(), 1);
    }

    #[test]
    fn cache_split_by_owner_and_read_back() {
        let base = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(base.path());
        let root_dir = base.path().join("r");
        std::fs::create_dir_all(root_dir.join("m/.permark")).unwrap();
        let store = |p: &std::path::Path| {
            StateStore::for_project(&dirs, &crate::paths::Root::open(p).unwrap())
        };
        let owners = Owners::new(
            store(&root_dir),
            vec![crate::projects::OwnerSlot::member(
                crate::projects::ProjectUid::parse("aaaaaaaaaa").unwrap(),
                "m",
                store(&root_dir.join("m")),
            )],
        );
        let source: OwnersSource = {
            let owners = owners.clone();
            Arc::new(move || owners.clone())
        };
        let h = Arc::new(Harvest::default());
        h.use_cache(Some(Arc::clone(&source)));
        h.put(
            vec![
                ("top.md".into(), doc(&["a"], 1)),
                ("m/in.md".into(), doc(&["b"], 2)),
            ],
            false,
        );
        h.write_cache();
        assert!(
            !root_dir
                .join("m/.permark/cache/index/fm-tags.json")
                .exists(),
            "nobody listens: no cache"
        );
        let _sub = h.subscribe(|| {});
        h.put(vec![("m/in2.md".into(), doc(&["c"], 3))], false);
        h.write_cache();
        let member_cache = root_dir.join("m/.permark/cache/index/fm-tags.json");
        let text = std::fs::read_to_string(&member_cache).unwrap();
        assert!(
            text.contains("\"in.md\"") && !text.contains("top.md"),
            "{text}"
        );
        // a new harvest shows the cached tags at once
        let again = Harvest::default();
        again.use_cache(Some(source));
        assert_eq!(again.tags_of("m/in.md").unwrap(), ["b"]);
        assert_eq!(again.tags_of("top.md").unwrap(), ["a"]);
        assert!(!again.ready());
    }
}
