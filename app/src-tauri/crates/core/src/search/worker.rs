//! The index worker: one thread per project. Builds the index (reading documents on a few helper threads, inserting
//! in batches so queries interleave), then keeps it current from watch batches. Nothing else writes the index.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use super::harvest::DocTags;
use super::prepare::{head_tags, read_and_prepare, reads_content, Prepared, Sig};
use super::{IndexPhase, IndexProgress, Shared};
use crate::events::{CoreEvent, EventSink};
use crate::format::FormatRegistry;
use crate::paths::{Vault, VaultPath};
use crate::tree::{category_of, Category, Explorer, ScanFile};
use crate::watch::{Change, ChangeKind, FsBatch};

/// Stack of the threads that parse untrusted text.
pub const STACK: usize = 8 * 1024 * 1024;
/// Documents inserted per lock hold.
const BATCH: usize = 64;
/// Files added to the list per lock hold.
const LIST_BATCH: usize = 2000;
/// Progress events are at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

pub enum Msg {
    Changes {
        changes: Vec<Change>,
        reconcile: bool,
    },
    /// The formats that are on changed: config files are read again.
    FormatsChanged,
    Stop,
}

pub struct Worker {
    pub shared: Arc<Shared>,
    pub vault: Vault,
    pub explorer: Arc<Explorer>,
    pub sink: Arc<dyn EventSink>,
    pub root: String,
    pub stop: Arc<AtomicBool>,
    pub formats: Arc<std::sync::RwLock<Arc<FormatRegistry>>>,
}

/// What a helper thread found for one document.
enum Outcome {
    Unchanged,
    Read(Prepared, Category),
    /// Not readable as text (binary, too large, vanished): listed by name only (a Markdown file too large for the
    /// index still gives its front matter tags, read from its head).
    NameOnly(String, Option<DocTags>),
}

/// The harvest entry of a prepared Markdown document.
fn harvested(prepared: &mut Prepared) -> Option<(String, DocTags)> {
    let tags = prepared.fm_tags.take()?;
    Some((
        prepared.path.to_string(),
        DocTags {
            tags,
            len: prepared.sig.len,
            mtime_ms: prepared.sig.mtime_ms,
        },
    ))
}

impl Worker {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn run(&self, rx: &Receiver<Msg>) {
        self.reconcile(true);
        loop {
            if self.stopped() {
                self.shared.harvest.write_cache();
                return;
            }
            // the harvest cache is written once changes rest (`projects.md` §7.3: 2 s after the last)
            let first = match self.shared.harvest.cache_due() {
                Some(due) => match rx.recv_timeout(due) {
                    Ok(msg) => msg,
                    Err(RecvTimeoutError::Timeout) => {
                        self.shared.harvest.write_cache();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match rx.recv() {
                    Ok(msg) => msg,
                    Err(_) => return,
                },
            };
            let mut changes: Vec<Change> = Vec::new();
            let mut reconcile = false;
            let mut next = Some(first);
            while let Some(msg) = next {
                match msg {
                    Msg::Stop => {
                        self.shared.harvest.write_cache();
                        return;
                    }
                    Msg::FormatsChanged => {
                        self.forget_config_content();
                        reconcile = true;
                    }
                    Msg::Changes {
                        changes: more,
                        reconcile: force,
                    } => {
                        changes.extend(more);
                        reconcile |= force;
                    }
                }
                next = rx.try_recv().ok();
            }
            if reconcile {
                self.reconcile(false);
            } else {
                self.apply(&changes);
            }
            self.publish_linked();
        }
    }

    /// The registry now: one snapshot per batch of reads.
    fn formats(&self) -> Arc<FormatRegistry> {
        Arc::clone(&self.formats.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Config files were indexed by the formats of that time: forget their content so the next pass reads them again.
    fn forget_config_content(&self) {
        let mut index = self
            .shared
            .index
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let config: Vec<String> = index
            .files
            .iter()
            .filter(|(_, f)| f.category == Category::Config)
            .map(|(p, _)| p.clone())
            .collect();
        for path in config {
            index.drop_content(&path);
        }
    }

    fn emit(&self, phase: IndexPhase, done: usize, total: usize) {
        self.sink.emit(CoreEvent::IndexProgress(IndexProgress {
            root: self.root.clone(),
            phase,
            done: u32::try_from(done).unwrap_or(u32::MAX),
            total: u32::try_from(total).unwrap_or(u32::MAX),
        }));
    }

    fn publish_linked(&self) {
        let linked = self
            .shared
            .index
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .take_linked();
        if let Some(set) = linked {
            self.explorer.set_linked(set);
        }
    }

    // ---- reconcile: the index becomes what the disk is --------------------------------------------------------------

    fn reconcile(&self, initial: bool) {
        let scans = self.explorer.scan(&self.stop);
        if self.stopped() {
            return;
        }
        let files: Vec<&ScanFile> = scans
            .iter()
            .flat_map(|(_, scan)| scan.files.iter())
            .collect();
        let on_disk: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
        let complete = scans.iter().all(|(_, scan)| !scan.cut_off);

        // 1. files that vanished leave (only when the walk saw everything)
        if complete {
            let vanished: Vec<String> = {
                let index = self
                    .shared
                    .index
                    .read()
                    .unwrap_or_else(PoisonError::into_inner);
                index
                    .files
                    .keys()
                    .filter(|path| !on_disk.contains(path.as_str()))
                    .cloned()
                    .collect()
            };
            for chunk in vanished.chunks(LIST_BATCH) {
                let mut index = self
                    .shared
                    .index
                    .write()
                    .unwrap_or_else(PoisonError::into_inner);
                for path in chunk {
                    index.remove_file(path);
                }
            }
            // entries known from the cache only (files gone while closed)
            self.shared.harvest.keep_only(&on_disk);
        }
        // 2. new files join the list
        for chunk in files.chunks(LIST_BATCH) {
            if self.stopped() {
                return;
            }
            let mut index = self
                .shared
                .index
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            for file in chunk {
                index.add_file(&file.path, file.category);
            }
        }
        // 3. documents are read (the ones whose length or time changed)
        let formats = self.formats();
        let todo: Vec<(&str, &PathBuf, Category)> = files
            .iter()
            .filter(|f| reads_content(f.category) && wanted(&f.path, f.category, &formats))
            .filter_map(|f| f.abs.as_ref().map(|abs| (f.path.as_str(), abs, f.category)))
            .collect();
        let total = self.read_documents(&todo, initial);
        if self.stopped() {
            return;
        }
        if initial {
            self.shared.set_ready();
            self.shared.harvest.set_ready();
            self.emit(IndexPhase::Ready, total, total);
        }
        self.publish_linked();
    }

    /// Reads the documents of `todo` (the ones that changed); returns how many there are.
    fn read_documents(&self, todo: &[(&str, &PathBuf, Category)], initial: bool) -> usize {
        let total = todo.len();
        if initial {
            self.shared.set_building(0, total);
            self.emit(IndexPhase::Building, 0, total);
        }
        if total == 0 {
            return 0;
        }
        let threads = std::thread::available_parallelism()
            .map_or(2, std::num::NonZero::get)
            .saturating_sub(1)
            .clamp(1, 6)
            .min(total);
        let next = AtomicUsize::new(0);
        let formats = self.formats();
        let (tx, rx) = sync_channel::<(usize, Outcome)>(256);
        std::thread::scope(|scope| {
            for _ in 0..threads {
                let tx = tx.clone();
                let next = &next;
                let shared = &self.shared;
                let stop = &self.stop;
                let formats = &formats;
                let spawned =
                    std::thread::Builder::new()
                        .stack_size(STACK)
                        .spawn_scoped(scope, move || loop {
                            if stop.load(Ordering::Relaxed) {
                                return;
                            }
                            let at = next.fetch_add(1, Ordering::Relaxed);
                            let Some((path, abs, category)) = todo.get(at) else {
                                return;
                            };
                            let known = shared
                                .index
                                .read()
                                .unwrap_or_else(PoisonError::into_inner)
                                .sig_of(path);
                            let outcome = outcome_of(path, abs, *category, known, formats);
                            if tx.send((at, outcome)).is_err() {
                                return;
                            }
                        });
                if spawned.is_err() {
                    break;
                }
            }
            drop(tx);
            self.insert_outcomes(&rx, total, initial);
        });
        total
    }

    fn insert_outcomes(&self, rx: &Receiver<(usize, Outcome)>, total: usize, initial: bool) {
        let mut pending: Vec<Outcome> = Vec::with_capacity(BATCH);
        let mut done = 0_usize;
        let mut last_event = Instant::now();
        loop {
            let received = rx.recv_timeout(Duration::from_millis(20));
            let finished = matches!(received, Err(RecvTimeoutError::Disconnected));
            let idle = matches!(received, Err(RecvTimeoutError::Timeout));
            if let Ok((_, outcome)) = received {
                done += 1;
                pending.push(outcome);
            }
            if pending.len() >= BATCH || ((idle || finished) && !pending.is_empty()) {
                self.insert(std::mem::take(&mut pending), initial);
            }
            if initial && (last_event.elapsed() >= PROGRESS_EVERY || finished) {
                self.shared.set_building(done, total);
                self.emit(IndexPhase::Building, done, total);
                last_event = Instant::now();
            }
            if finished || self.stopped() {
                break;
            }
        }
        // stopped mid-read: readers blocked on the full queue are let go (each sees `stop` before its next file and
        // drops its sender); without this the scope never ends and the index drop hangs (CI 39698724615)
        for _ in rx {}
        if !pending.is_empty() {
            self.insert(pending, initial);
        }
    }

    fn insert(&self, batch: Vec<Outcome>, building: bool) {
        let mut tags = Vec::new();
        {
            let mut index = self
                .shared
                .index
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            for outcome in batch {
                match outcome {
                    Outcome::Unchanged => {}
                    Outcome::Read(mut prepared, category) => {
                        tags.extend(harvested(&mut prepared));
                        index.upsert_doc(prepared, category);
                    }
                    Outcome::NameOnly(path, doc) => {
                        index.drop_content(&path);
                        if let Some(doc) = doc {
                            tags.push((path, doc));
                        }
                    }
                }
            }
        }
        self.shared.harvest.put(tags, building);
        self.shared.harvest.tell(building);
    }

    // ---- incremental: one watch batch ------------------------------------------------------------------------------

    fn apply(&self, changes: &[Change]) {
        for change in changes {
            if self.stopped() {
                return;
            }
            if crate::tree::path_always_out(&change.path) {
                continue;
            }
            match &change.kind {
                ChangeKind::Removed => self.forget(&change.path, change.is_dir),
                ChangeKind::Renamed { from } => {
                    self.forget(from, change.is_dir);
                    if !change.is_dir {
                        self.learn(&change.path);
                    }
                }
                ChangeKind::Created | ChangeKind::Modified => {
                    if !change.is_dir {
                        self.learn(&change.path);
                    }
                }
            }
        }
    }

    fn forget(&self, path: &str, is_dir: bool) {
        {
            let mut index = self
                .shared
                .index
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            if is_dir {
                index.remove_under(path);
            } else {
                index.remove_file(path);
            }
        }
        self.shared.harvest.remove(path, is_dir);
    }

    /// A file appeared or changed: it joins the list; documents are read again when their length or time differ.
    fn learn(&self, path: &str) {
        let name = path.rsplit('/').next().unwrap_or(path);
        let category = category_of(name);
        let abs = VaultPath::parse(path)
            .ok()
            .and_then(|vp| self.vault.resolve(&vp).ok());
        let Some(abs) = abs else {
            return;
        };
        let Ok(meta) = std::fs::metadata(&abs) else {
            // gone before we looked
            self.forget(path, false);
            return;
        };
        if !meta.is_file() {
            return;
        }
        let sig = Sig::of(&meta);
        {
            let mut index = self
                .shared
                .index
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            index.add_file(path, category);
            index.set_mtime(path, sig.mtime_ms);
            if !reads_content(category)
                || !wanted(path, category, &self.formats())
                || index.sig_of(path) == Some(sig)
            {
                return;
            }
        }
        // read outside the lock
        let outcome = outcome_of(path, &abs, category, None, &self.formats());
        self.insert(vec![outcome], false);
    }
}

/// Whether the content of a file is read: documents and text always; a config file only when a config format that is
/// on claims it (else it stays in the list by name, unread).
fn wanted(path: &str, category: Category, formats: &FormatRegistry) -> bool {
    category != Category::Config || formats.pick(path, &[]).is_some()
}

fn outcome_of(
    path: &str,
    abs: &std::path::Path,
    category: Category,
    known: Option<Sig>,
    formats: &FormatRegistry,
) -> Outcome {
    if let (Some(known), Ok(meta)) = (known, std::fs::metadata(abs)) {
        if Sig::of(&meta) == known {
            return Outcome::Unchanged;
        }
    }
    match read_and_prepare(path, abs, category, formats) {
        Some(prepared) => Outcome::Read(prepared, category),
        None => Outcome::NameOnly(
            path.to_owned(),
            (category == Category::Markdown)
                .then(|| head_tags(abs))
                .flatten()
                .map(|(tags, sig)| DocTags {
                    tags,
                    len: sig.len,
                    mtime_ms: sig.mtime_ms,
                }),
        ),
    }
}

/// Changes of a watch batch -> a message (any change of a folder other than its removal needs a look at the disk).
pub fn message_of(batch: &FsBatch) -> Msg {
    let reconcile = batch.bulk
        || batch.rescan
        || batch
            .changes
            .iter()
            .any(|c| c.is_dir && !matches!(c.kind, ChangeKind::Removed));
    Msg::Changes {
        changes: batch.changes.clone(),
        reconcile,
    }
}
