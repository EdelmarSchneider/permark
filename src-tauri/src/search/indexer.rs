//! Building the search index from the vault, and keeping it current from watcher output.
//!
//! One worker thread ([`IndexWorker`]) owns every update of the index. It builds the index first and then
//! applies, in order, what the watcher reports. A rescan (events were lost, or a moved-in folder was only
//! partly listed) is a reconciliation with the disk. Because nothing else writes the index, a slow initial
//! build can never put an older version of a file over a newer one the watcher already indexed.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;

use super::chunk::{chunk_markdown, Chunk};
use super::Bm25Index;
use crate::render::{decode_text, MAX_RENDER_BYTES};
use crate::util::{hash_bytes, modified_ms, parser_thread, read_with_retry};
use crate::vault::{key, NodeKind, Vault, WalkItem};
use crate::watch::{ChangeKind, FileSig, FsChange, PathFilter, SigStore, Verdict};

/// How often `build` reports progress (in files).
const PROGRESS_EVERY: usize = 25;

/// Only Markdown is indexed.
pub fn is_indexable(rel: &str) -> bool {
    Path::new(rel).extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_ascii_lowercase();
        matches!(e.as_str(), "md" | "markdown" | "mdown" | "mkd")
    })
}

/// A Markdown file as read from disk.
struct Read {
    sig: FileSig,
    chunks: Vec<Chunk>,
}

/// Reads and chunks one file. `None` for files that cannot be read, are too large or binary.
fn read_markdown(abs: &Path) -> Option<Read> {
    let meta = std::fs::metadata(abs).ok()?;
    if !meta.is_file() || meta.len() > MAX_RENDER_BYTES {
        return None;
    }
    let bytes = read_with_retry(abs).ok()?;
    let sig = FileSig {
        len: bytes.len() as u64,
        mtime_ms: modified_ms(&meta),
        hash: hash_bytes(&bytes),
    };
    Some(Read {
        sig,
        chunks: chunk_markdown(&decode_text(&bytes)?),
    })
}

/// What to do with the watcher's remembered signature of a file that was just read.
#[derive(Clone, Copy)]
enum SigPolicy {
    /// Remember it unless the watcher already has one (the initial build: it gives the watcher a
    /// baseline to recognise no-op rewrites, but must never override something the watcher saw).
    Seed,
    /// Replace it: a rescan reconciles the signatures with the disk.
    Overwrite,
    /// Leave it alone: the watcher keeps its own signatures current while it reports changes.
    Keep,
}

/// The index updates, to be run by exactly one thread at a time.
pub struct Indexer {
    vault: Arc<Vault>,
    filter: Arc<PathFilter>,
    index: Arc<RwLock<Bm25Index>>,
    sigs: Arc<SigStore>,
    /// (size, mtime) of each indexed file when it was read, so a rescan only re-reads what changed.
    stamps: HashMap<String, (u64, u64)>,
}

impl Indexer {
    pub fn new(
        vault: Arc<Vault>,
        filter: Arc<PathFilter>,
        index: Arc<RwLock<Bm25Index>>,
        sigs: Arc<SigStore>,
    ) -> Self {
        Self {
            vault,
            filter,
            index,
            sigs,
            stamps: HashMap::new(),
        }
    }

    /// Reads one file and (re)indexes it, or removes it from the index when it cannot be indexed.
    /// Returns whether it is indexed.
    fn index_one(&mut self, abs: &Path, rel: &str, policy: SigPolicy) -> bool {
        let k = key(rel);
        let chunks = if let Some(Read { sig, chunks }) = read_markdown(abs) {
            match policy {
                SigPolicy::Seed => self.sigs.seed(k.clone(), rel, sig),
                SigPolicy::Overwrite => self.sigs.set(k.clone(), rel, sig),
                SigPolicy::Keep => {}
            }
            self.stamps.insert(k, (sig.len, sig.mtime_ms));
            Some(chunks)
        } else {
            self.stamps.remove(&k);
            None
        };
        let Ok(mut index) = self.index.write() else {
            return false;
        };
        if let Some(chunks) = chunks {
            index.add_chunks(rel, chunks);
            true
        } else {
            index.remove_doc(rel);
            false
        }
    }

    /// Every Markdown file of the vault that the watcher would report.
    fn files(&self) -> Vec<WalkItem> {
        self.vault
            .walk()
            .into_iter()
            .filter(|item| item.kind == NodeKind::File && is_indexable(&item.rel))
            .filter(|item| {
                self.vault
                    .locate(&item.abs)
                    .is_some_and(|l| self.filter.classify(l.root, &l.inner, false) == Verdict::Emit)
            })
            .collect()
    }

    /// Indexes every Markdown file of the vault. `progress(indexed, total)` is called every few files
    /// and at the end; the loop stops early when `cancel` is set (a different folder was opened).
    pub fn build(&mut self, cancel: &AtomicBool, progress: &mut dyn FnMut(usize, usize)) {
        let files = self.files();
        let total = files.len();
        progress(0, total);
        for (i, item) in files.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            self.index_one(&item.abs, &item.rel, SigPolicy::Seed);
            let done = i + 1;
            if done % PROGRESS_EVERY == 0 || done == total {
                progress(done, total);
            }
        }
    }

    /// Applies watcher output to the index.
    pub fn apply(&mut self, changes: &[FsChange]) {
        for change in changes {
            match change.kind {
                ChangeKind::Removed => self.forget_tree(&change.path),
                ChangeKind::Renamed | ChangeKind::Created | ChangeKind::Modified => {
                    if let Some(from) = &change.from {
                        self.forget_tree(from);
                    }
                    if !is_indexable(&change.path) {
                        continue;
                    }
                    match self.vault.resolve(&change.path) {
                        Ok(r) if r.abs.is_file() => {
                            self.index_one(&r.abs, &r.rel, SigPolicy::Keep);
                        }
                        _ => self.forget_tree(&change.path),
                    }
                }
            }
        }
    }

    /// Removes a file, or every file below a folder (the watcher reports a deleted folder as one
    /// removal when it never saw its files).
    fn forget_tree(&mut self, path: &str) {
        let prefix = format!("{}/", key(path).trim_end_matches('/'));
        self.stamps
            .retain(|k, _| k != &key(path) && !k.starts_with(&prefix));
        if let Ok(mut index) = self.index.write() {
            index.remove_doc(path);
            index.remove_under(path);
        }
    }

    /// Reconciles the index with the disk: documents whose file is gone leave, new and changed files
    /// are read again, and the watcher's signatures are refreshed on the way.
    pub fn rescan(&mut self, cancel: &AtomicBool) {
        let files = self.files();
        let present: HashSet<String> = files.iter().map(|f| key(&f.rel)).collect();
        let gone: Vec<String> = self
            .index
            .read()
            .map(|index| index.paths())
            .unwrap_or_default()
            .into_iter()
            .filter(|p| !present.contains(&key(p)))
            .collect();
        for path in gone {
            self.forget_tree(&path);
            self.sigs.remove(&key(&path));
        }
        for item in &files {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let k = key(&item.rel);
            let stamp = std::fs::metadata(&item.abs)
                .ok()
                .map(|m| (m.len(), modified_ms(&m)));
            if stamp.is_some() && self.stamps.get(&k).copied() == stamp {
                continue; // read before, and untouched since
            }
            self.index_one(&item.abs, &item.rel, SigPolicy::Overwrite);
        }
    }
}

/// What the worker is asked to do.
enum Msg {
    /// Watcher output to apply.
    Changes(Vec<FsChange>),
    /// Events were lost: reconcile with the disk.
    Rescan,
    Stop,
}

/// What a burst of queued messages comes down to.
#[derive(Default)]
struct Pending {
    changes: Vec<FsChange>,
    rescan: bool,
    stop: bool,
}

impl Pending {
    fn absorb(&mut self, msg: Msg) {
        match msg {
            Msg::Changes(mut c) => self.changes.append(&mut c),
            Msg::Rescan => self.rescan = true,
            Msg::Stop => self.stop = true,
        }
    }
}

/// The thread that builds the index and then keeps it current. Dropping it asks the thread to stop;
/// [`IndexWorker::stop`] also waits for it.
pub struct IndexWorker {
    tx: Sender<Msg>,
    cancel: Arc<AtomicBool>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl IndexWorker {
    /// Starts the thread: it builds the index (reporting `progress(indexed, total)`), then waits for
    /// work. The thread has a large stack because it parses untrusted documents.
    pub fn start(
        mut indexer: Indexer,
        mut progress: impl FnMut(usize, usize) + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<Msg>();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let handle = parser_thread("trailmark-index")
            .spawn(move || {
                indexer.build(&stop, &mut progress);
                run(&mut indexer, &rx, &stop);
            })
            .ok();
        Self {
            tx,
            cancel,
            handle: Mutex::new(handle),
        }
    }

    /// Queues watcher output. Changes are applied after the initial build, in the order they arrive.
    pub fn apply(&self, changes: Vec<FsChange>) {
        let _ = self.tx.send(Msg::Changes(changes));
    }

    /// Queues a reconciliation with the disk (it also covers every change queued before it).
    pub fn rescan(&self) {
        let _ = self.tx.send(Msg::Rescan);
    }

    /// Stops the thread and waits for it. Safe to call more than once.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Msg::Stop);
        let handle = self.handle.lock().ok().and_then(|mut h| h.take());
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

impl Drop for IndexWorker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Msg::Stop);
    }
}

fn run(indexer: &mut Indexer, rx: &Receiver<Msg>, cancel: &AtomicBool) {
    while let Ok(first) = rx.recv() {
        let mut pending = Pending::default();
        pending.absorb(first);
        while let Ok(more) = rx.try_recv() {
            pending.absorb(more);
        }
        if pending.stop || cancel.load(Ordering::Relaxed) {
            return;
        }
        if pending.rescan {
            // Reads the disk as it is now, which covers every queued change.
            indexer.rescan(cancel);
        } else {
            indexer.apply(&pending.changes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchProvider;
    use std::fs;
    use std::time::{Duration, Instant};

    struct Fx {
        dir: tempfile::TempDir,
        indexer: Indexer,
        index: Arc<RwLock<Bm25Index>>,
        sigs: Arc<SigStore>,
    }

    fn fx(files: &[(&str, &str)]) -> Fx {
        let dir = tempfile::tempdir().expect("tempdir");
        for (rel, content) in files {
            let p = dir.path().join(rel);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(p, content).expect("write");
        }
        let vault = Arc::new(Vault::open(dir.path()).expect("open"));
        let filter = Arc::new(PathFilter::new(
            &[(vault.root().to_path_buf(), None)],
            &["private/".to_owned()],
        ));
        let index = Arc::new(RwLock::new(Bm25Index::new()));
        let sigs = Arc::new(SigStore::new());
        let indexer = Indexer::new(vault, filter, Arc::clone(&index), Arc::clone(&sigs));
        Fx {
            dir,
            indexer,
            index,
            sigs,
        }
    }

    impl Fx {
        fn build(&mut self) -> Vec<(usize, usize)> {
            let mut calls = Vec::new();
            self.indexer
                .build(&AtomicBool::new(false), &mut |a, b| calls.push((a, b)));
            calls
        }

        fn rescan(&mut self) {
            self.indexer.rescan(&AtomicBool::new(false));
        }

        fn write(&self, rel: &str, content: &str) {
            let p = self.dir.path().join(rel);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(p, content).expect("write");
        }

        fn found(&self, word: &str) -> Vec<String> {
            self.index
                .search(word, 20)
                .into_iter()
                .map(|h| h.path)
                .collect()
        }
    }

    fn change(kind: ChangeKind, path: &str) -> FsChange {
        FsChange {
            kind,
            path: path.to_owned(),
            from: None,
            at_ms: 0,
        }
    }

    #[test]
    fn build_indexes_markdown_only_honoring_ignores_and_reports_progress() {
        let mut f = fx(&[
            ("a.md", "# Alpha\n\nuniquealphaword\n"),
            ("docs/b.markdown", "# Beta\n\nbeta text\n"),
            ("notes.txt", "# Not markdown\n\nplainonlyword\n"),
            ("private/c.md", "# Hidden\n\nhiddenword\n"),
            ("node_modules/pkg/readme.md", "# Dep\n\ndepword\n"),
            (".gitignore", "ignored.md\n"),
            ("ignored.md", "# Ign\n\nignoredword\n"),
        ]);
        let calls = f.build();
        assert_eq!(calls.first(), Some(&(0, 2)));
        assert_eq!(calls.last(), Some(&(2, 2)));
        let i = f.index.read().expect("lock");
        assert_eq!(i.paths(), ["a.md", "docs/b.markdown"]);
        assert!(i.search("plainonlyword", 3).is_empty());
        assert!(i.search("hiddenword", 3).is_empty());
        assert!(i.search("ignoredword", 3).is_empty());
        assert_eq!(i.search("uniquealphaword", 3).len(), 1);
        drop(i);
        assert_eq!(f.sigs.len(), 2, "signatures are seeded for the watcher");
    }

    #[test]
    fn cancel_stops_the_build() {
        let mut f = fx(&[("a.md", "# A\n\nx\n"), ("b.md", "# B\n\ny\n")]);
        let cancel = AtomicBool::new(true);
        f.indexer.build(&cancel, &mut |_, _| {});
        assert!(f.index.read().expect("lock").is_empty());
    }

    #[test]
    fn a_pathologically_nested_file_does_not_stop_the_build() {
        let bomb = format!("{} bombword\n", ">".repeat(100_000));
        let mut f = fx(&[("bomb.md", &bomb), ("a.md", "# A\n\nnormalword\n")]);
        let started = Instant::now();
        f.build();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(f.found("normalword"), ["a.md"]);
        assert_eq!(f.found("bombword"), ["bomb.md"]);
    }

    #[test]
    fn seeding_keeps_a_newer_signature_the_watcher_already_has() {
        let mut f = fx(&[("a.md", "# A\n\nfirst\n")]);
        let newer = FileSig {
            len: 99,
            mtime_ms: 99,
            hash: 99,
        };
        f.sigs.set(key("a.md"), "a.md", newer);
        f.build();
        assert_eq!(f.sigs.get(&key("a.md")), Some(newer));
        // ... and changes applied later do not touch the watcher's signature either.
        f.write("a.md", "# A\n\nsecond\n");
        f.indexer.apply(&[change(ChangeKind::Modified, "a.md")]);
        assert_eq!(f.found("second"), ["a.md"]);
        assert_eq!(f.sigs.get(&key("a.md")), Some(newer));
    }

    #[test]
    fn changes_update_the_index_incrementally() {
        let mut f = fx(&[("a.md", "# A\n\nfirstversion\n")]);
        f.build();
        assert_eq!(f.found("firstversion"), ["a.md"]);

        f.write("a.md", "# A\n\nsecondversion\n");
        f.write("new.md", "# New\n\nbrandnewword\n");
        f.indexer.apply(&[
            change(ChangeKind::Modified, "a.md"),
            change(ChangeKind::Created, "new.md"),
        ]);
        assert!(f.found("firstversion").is_empty());
        assert_eq!(f.found("secondversion"), ["a.md"]);
        assert_eq!(f.found("brandnewword"), ["new.md"]);

        fs::remove_file(f.dir.path().join("new.md")).expect("rm");
        f.indexer.apply(&[change(ChangeKind::Removed, "new.md")]);
        assert!(f.found("brandnewword").is_empty());
    }

    #[test]
    fn renames_move_the_document_in_the_index() {
        let mut f = fx(&[("old.md", "# T\n\nrenamemeword\n")]);
        f.build();
        fs::create_dir_all(f.dir.path().join("sub")).expect("mkdir");
        fs::rename(f.dir.path().join("old.md"), f.dir.path().join("sub/new.md")).expect("mv");
        f.indexer.apply(&[FsChange {
            kind: ChangeKind::Renamed,
            path: "sub/new.md".to_owned(),
            from: Some("old.md".to_owned()),
            at_ms: 0,
        }]);
        assert_eq!(f.found("renamemeword"), ["sub/new.md"]);
    }

    #[test]
    fn removing_a_folder_removes_every_document_below_it() {
        // The watcher reports a deleted folder as one removal when it never saw the files inside.
        let mut f = fx(&[
            ("docs/a.md", "# A\n\nfolderword\n"),
            ("docs/deep/b.md", "# B\n\nfolderword\n"),
            ("docs2/c.md", "# C\n\nfolderword\n"),
        ]);
        f.build();
        assert_eq!(f.found("folderword").len(), 3);
        fs::remove_dir_all(f.dir.path().join("docs")).expect("rm");
        f.indexer.apply(&[change(ChangeKind::Removed, "docs")]);
        assert_eq!(f.found("folderword"), ["docs2/c.md"]);
    }

    #[test]
    fn unreadable_or_binary_files_leave_the_index() {
        let mut f = fx(&[("a.md", "# A\n\nsomeword\n")]);
        f.build();
        f.write("a.md", "\0\0binary\0");
        f.indexer.apply(&[change(ChangeKind::Modified, "a.md")]);
        assert!(f.found("someword").is_empty());
        f.indexer
            .apply(&[change(ChangeKind::Created, "missing.md")]);
        assert!(f.found("binary").is_empty());
    }

    #[test]
    fn a_rescan_repairs_everything_that_happened_while_events_were_lost() {
        let mut f = fx(&[
            ("keep.md", "# Keep\n\nkeepword\n"),
            ("edit.md", "# Edit\n\noldword\n"),
            ("gone.md", "# Gone\n\ngoneword\n"),
            ("dir/in.md", "# In\n\ninsideword\n"),
        ]);
        f.build();
        // Nothing of this was reported to the index.
        std::thread::sleep(Duration::from_millis(20));
        f.write("edit.md", "# Edit\n\nnewword and more\n");
        f.write("fresh.md", "# Fresh\n\nfreshword\n");
        f.write("deep/er/moved.md", "# Moved\n\nmovedword\n");
        fs::remove_file(f.dir.path().join("gone.md")).expect("rm");
        fs::remove_dir_all(f.dir.path().join("dir")).expect("rm");

        f.rescan();
        assert_eq!(f.found("keepword"), ["keep.md"]);
        assert!(f.found("oldword").is_empty());
        assert_eq!(f.found("newword"), ["edit.md"]);
        assert_eq!(f.found("freshword"), ["fresh.md"]);
        assert_eq!(f.found("movedword"), ["deep/er/moved.md"]);
        assert!(f.found("goneword").is_empty());
        assert!(f.found("insideword").is_empty());
        assert!(
            f.sigs.get(&key("gone.md")).is_none(),
            "its signature goes too"
        );
        assert!(
            f.sigs.get(&key("fresh.md")).is_some(),
            "and new files are seeded"
        );
    }

    #[test]
    fn a_rescan_does_not_read_files_that_did_not_change() {
        let mut f = fx(&[("a.md", "# A\n\noriginalword\n")]);
        f.build();
        // Same size and mtime as far as the index knows: sneak different content in behind its back.
        let path = f.dir.path().join("a.md");
        let mtime = fs::metadata(&path)
            .expect("meta")
            .modified()
            .expect("mtime");
        fs::write(&path, "# A\n\nchangedwords\n").expect("write");
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("open")
            .set_modified(mtime)
            .expect("set mtime");
        f.rescan();
        assert_eq!(
            f.found("originalword"),
            ["a.md"],
            "untouched files are skipped"
        );
    }

    #[test]
    fn the_worker_builds_first_and_applies_what_arrived_meanwhile_in_order() {
        let f = fx(&[("a.md", "# A\n\nstartword\n")]);
        let Fx {
            dir,
            indexer,
            index,
            ..
        } = f;
        // Queue a change before the worker even starts: it must be applied after the build.
        fs::write(dir.path().join("a.md"), "# A\n\nlaterword\n").expect("write");
        let (tx, rx) = mpsc::channel::<Msg>();
        tx.send(Msg::Changes(vec![change(ChangeKind::Modified, "a.md")]))
            .expect("send");
        tx.send(Msg::Stop).expect("send");
        let mut indexer = indexer;
        let cancel = AtomicBool::new(false);
        indexer.build(&cancel, &mut |_, _| {});
        run(&mut indexer, &rx, &cancel);
        assert!(index.search("startword", 3).is_empty());
        assert_eq!(index.search("laterword", 3).len(), 1);
    }

    #[test]
    fn queued_messages_collapse_into_one_rescan_that_covers_the_changes() {
        let mut p = Pending::default();
        p.absorb(Msg::Changes(vec![change(ChangeKind::Modified, "a.md")]));
        p.absorb(Msg::Rescan);
        p.absorb(Msg::Changes(vec![change(ChangeKind::Created, "b.md")]));
        assert!(p.rescan && !p.stop);
        assert_eq!(p.changes.len(), 2);
    }

    #[test]
    fn the_worker_thread_applies_changes_rescans_and_stops() {
        let f = fx(&[("a.md", "# A\n\nstartword\n")]);
        let Fx {
            dir,
            indexer,
            index,
            ..
        } = f;
        let done = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&done);
        let worker = IndexWorker::start(indexer, move |indexed, total| {
            if indexed == total {
                signal.store(true, Ordering::SeqCst);
            }
        });
        let wait = |what: &str, cond: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !cond() {
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        wait("the build", &|| done.load(Ordering::SeqCst));
        assert_eq!(index.search("startword", 3).len(), 1);

        fs::write(dir.path().join("a.md"), "# A\n\nsecondword\n").expect("write");
        worker.apply(vec![change(ChangeKind::Modified, "a.md")]);
        wait("the change", &|| index.search("secondword", 3).len() == 1);

        fs::write(dir.path().join("b.md"), "# B\n\nthirdword\n").expect("write");
        worker.rescan();
        wait("the rescan", &|| index.search("thirdword", 3).len() == 1);

        worker.stop();
        worker.stop(); // idempotent
    }

    #[test]
    fn indexable_extensions() {
        assert!(is_indexable("a/B.MD"));
        assert!(is_indexable("x.markdown"));
        assert!(!is_indexable("x.txt"));
        assert!(!is_indexable("md"));
    }
}
