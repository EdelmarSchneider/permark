//! The index worker: one thread per project. Builds the index (reading documents on a few helper threads, inserting
//! in batches so queries interleave), then keeps it current from watch batches. Nothing else writes the index.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use super::harvest::DocTags;
use super::prepare::{head_tags, read_and_prepare, reads_content, Prepared, Sig};
use super::{IndexPhase, IndexProgress, Shared};
use crate::events::{CoreEvent, EventSink};
use crate::format::FormatRegistry;
use crate::links::same_links;
use crate::paths::{Vault, VaultPath};
use crate::tree::{category_of, is_ignore_file, Category, Explorer, ScanFile};
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
    /// The formats that are on changed: config files are read again, Markdown documents too when the Markdown
    /// features that decide their links changed.
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
        // the Markdown features the documents' links were read with
        let mut features = *self.formats().features();
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
                        let now = *self.formats().features();
                        self.forget_content(!same_links(&features, &now));
                        features = now;
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

    /// Config files were indexed by the formats of that time, the links of Markdown documents by the Markdown features
    /// of that time: forget the content of config files, and of Markdown documents too when `links` (those features
    /// changed), so the next pass reads them again.
    fn forget_content(&self, links: bool) {
        let mut index = self
            .shared
            .index
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let stale: Vec<String> = index
            .files
            .iter()
            .filter(|(_, f)| {
                f.category == Category::Config || (links && f.category == Category::Markdown)
            })
            .map(|(p, _)| p.clone())
            .collect();
        for path in stale {
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
        // the first pass compares documents by the size and time the folder listings show; later passes (events were
        // lost, many changes, other rules) ask each file, as a listing may lag behind a file still being written
        let scans = if initial {
            self.explorer.scan_with_listed_stats(&self.stop)
        } else {
            self.explorer.scan(&self.stop)
        };
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
        let Some(total) = self.take_files(&files, initial) else {
            return;
        };
        if initial {
            self.shared.set_ready();
            self.shared.harvest.set_ready();
            self.emit(IndexPhase::Ready, total, total);
        }
        self.publish_linked();
    }

    /// Files a walk found: new ones join the list, documents whose length or time changed are read. Returns how many
    /// documents were looked at; `None`: stopped.
    fn take_files(&self, files: &[&ScanFile], initial: bool) -> Option<usize> {
        // a link (the scan gives it no place on disk) keeps no text, nor tags, from before
        let mut links = Vec::new();
        for chunk in files.chunks(LIST_BATCH) {
            if self.stopped() {
                return None;
            }
            let mut index = self
                .shared
                .index
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            for file in chunk {
                index.add_file(&file.path, file.category);
                if file.abs.is_none() && reads_content(file.category) {
                    index.drop_content(&file.path);
                    links.push(file.path.as_str());
                }
            }
        }
        for path in links {
            self.shared.harvest.remove(path, false);
        }
        // documents are read (the ones whose length or time changed)
        let formats = self.formats();
        let todo: Vec<Todo<'_>> = files
            .iter()
            .filter(|f| reads_content(f.category) && wanted(&f.path, f.category, &formats))
            .filter_map(|f| {
                f.abs.as_ref().map(|abs| Todo {
                    path: f.path.as_str(),
                    abs,
                    category: f.category,
                    stat: listed_sig(f),
                })
            })
            .collect();
        #[cfg(test)]
        self.shared.listed.fetch_add(
            todo.iter().filter(|doc| doc.stat.is_some()).count(),
            Ordering::Relaxed,
        );
        let total = self.read_documents(&todo, initial);
        (!self.stopped()).then_some(total)
    }

    /// Reads the documents of `todo` (the ones that changed); returns how many there are.
    fn read_documents(&self, todo: &[Todo<'_>], initial: bool) -> usize {
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
                            let Some(doc) = todo.get(at) else {
                                return;
                            };
                            let known = shared
                                .index
                                .read()
                                .unwrap_or_else(PoisonError::into_inner)
                                .sig_of(doc.path);
                            let outcome = outcome_of(doc, known, formats);
                            #[cfg(test)]
                            if !matches!(outcome, Outcome::Unchanged) {
                                shared.reads.fetch_add(1, Ordering::Relaxed);
                            }
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
                    if change.is_dir {
                        self.learn_folder(&change.path);
                    } else {
                        self.learn(&change.path, false);
                    }
                }
                ChangeKind::Created if change.is_dir => self.learn_folder(&change.path),
                ChangeKind::Created => self.learn(&change.path, false),
                // the watch saw other bytes: read again even when length and time stayed (a coarse clock)
                ChangeKind::Modified if !change.is_dir => self.learn(&change.path, true),
                // a folder's own change: its files report themselves
                ChangeKind::Modified => {}
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

    /// A folder appeared (created, moved in): it is walked on its own, as a walk of the whole project would find its
    /// files; what the index held below it and is gone leaves.
    fn learn_folder(&self, path: &str) {
        let Ok(folder) = VaultPath::parse(path) else {
            return;
        };
        let Ok(abs) = self.vault.resolve(&folder) else {
            return;
        };
        // `resolve` follows links: a folder reached through one (or a link itself) is listed by name, never walked
        let through_link = std::fs::symlink_metadata(&abs)
            .is_ok_and(|m| m.file_type().is_symlink())
            || self
                .vault
                .relativize(&abs)
                .is_none_or(|real| real.key() != folder.key());
        if through_link {
            self.learn(path, false);
            return;
        }
        let Ok(scan) = self.explorer.scan_below(&folder, &abs, &self.stop) else {
            return;
        };
        if !scan.cut_off {
            let found: HashSet<&str> = scan.files.iter().map(|f| f.path.as_str()).collect();
            let gone: Vec<String> = {
                let index = self
                    .shared
                    .index
                    .read()
                    .unwrap_or_else(PoisonError::into_inner);
                let prefix = format!("{path}/");
                index
                    .files
                    .range(prefix.clone()..)
                    .take_while(|(p, _)| p.starts_with(&prefix))
                    .filter(|(p, _)| !found.contains(p.as_str()))
                    .map(|(p, _)| p.clone())
                    .collect()
            };
            for file in gone {
                self.forget(&file, false);
            }
        }
        let files: Vec<&ScanFile> = scan.files.iter().collect();
        let _ = self.take_files(&files, false);
    }

    /// A file appeared or changed: it joins the list; documents are read again when their length or time differ, or
    /// always (`changed`: the watch saw other bytes).
    fn learn(&self, path: &str, changed: bool) {
        let name = path.rsplit('/').next().unwrap_or(path);
        let category = category_of(name);
        let Ok(hinted) = VaultPath::parse(path) else {
            return;
        };
        let Ok(abs) = self.vault.resolve(&hinted) else {
            return;
        };
        let Ok(meta) = std::fs::symlink_metadata(&abs) else {
            // gone before we looked
            self.forget(path, false);
            return;
        };
        // `resolve` follows links: a real place under another project path means a link on the way (the file | a
        // folder above it); a link left as is leads nowhere
        let through_link = meta.file_type().is_symlink()
            || self
                .vault
                .relativize(&abs)
                .is_none_or(|real| real.key() != hinted.key());
        if through_link {
            // listed by name like the scan lists a link; its target is never read
            {
                let mut index = self
                    .shared
                    .index
                    .write()
                    .unwrap_or_else(PoisonError::into_inner);
                index.add_file(path, category);
                index.drop_content(path);
            }
            self.shared.harvest.remove(path, false);
            return;
        }
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
                || (!changed && index.sig_of(path) == Some(sig))
            {
                return;
            }
        }
        // read outside the lock
        let doc = Todo {
            path,
            abs: &abs,
            category,
            stat: None,
        };
        let outcome = outcome_of(&doc, None, &self.formats());
        #[cfg(test)]
        self.shared.reads.fetch_add(1, Ordering::Relaxed);
        self.insert(vec![outcome], false);
    }
}

/// Whether the content of a file is read: documents and text always; a config file only when a config format that is
/// on claims it (else it stays in the list by name, unread).
fn wanted(path: &str, category: Category, formats: &FormatRegistry) -> bool {
    category != Category::Config || formats.pick(path, &[]).is_some()
}

/// A document to look at: where it is, and the length + time its walk saw (when the listing told them).
struct Todo<'a> {
    path: &'a str,
    abs: &'a std::path::Path,
    category: Category,
    stat: Option<Sig>,
}

/// Length + modified time a walk read from the folder listing (`None`: it did not, or the system told nothing).
fn listed_sig(file: &ScanFile) -> Option<Sig> {
    let mtime_ms = u64::try_from(file.modified_ms).ok()?;
    ((file.size, mtime_ms) != (0, 0)).then_some(Sig {
        len: file.size,
        mtime_ms,
    })
}

fn outcome_of(doc: &Todo<'_>, known: Option<Sig>, formats: &FormatRegistry) -> Outcome {
    let Todo {
        path,
        abs,
        category,
        stat,
    } = *doc;
    if let Some(known) = known {
        let now = stat.or_else(|| std::fs::symlink_metadata(abs).ok().map(|m| Sig::of(&m)));
        if now == Some(known) {
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

/// Changes of a watch batch -> a message: lost events, many changes at once and other ignore rules make the index
/// reconcile with the disk; a folder that appeared is walked on its own.
pub fn message_of(batch: &FsBatch) -> Msg {
    let reconcile = batch.bulk
        || batch.rescan
        || batch.changes.iter().any(|c| {
            is_ignore_file(&c.path)
                || matches!(&c.kind, ChangeKind::Renamed { from } if is_ignore_file(from))
        });
    Msg::Changes {
        changes: batch.changes.clone(),
        reconcile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::prepare::tests::linked_document;

    #[test]
    fn a_link_is_listed_by_name_only_and_never_gives_its_targets_text_or_tags() {
        let (dir, outside) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let Some((link, far)) = linked_document(dir.path(), outside.path()) else {
            eprintln!("links could not be made here; skipped");
            return;
        };
        let formats = FormatRegistry::core();
        let doc = |path, abs| Todo {
            path,
            abs,
            category: Category::Markdown,
            stat: None,
        };
        let read = outcome_of(&doc("far.md", &far), None, &formats);
        assert!(matches!(read, Outcome::Read(..)), "the target itself reads");
        match outcome_of(&doc("alias.md", &link), None, &formats) {
            Outcome::NameOnly(path, None) => assert_eq!(path, "alias.md"),
            Outcome::NameOnly(_, Some(tags)) => {
                panic!("tags read through the link: {:?}", tags.tags)
            }
            Outcome::Read(..) | Outcome::Unchanged => panic!("text read through the link"),
        }
    }
}
