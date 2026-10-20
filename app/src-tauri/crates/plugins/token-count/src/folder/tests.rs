// Test code: counts and sizes here are small, and plain `assert!(x.is_empty())` reads best.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::items_after_statements,
    clippy::too_many_lines
)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use permark_core::conditions::{Context, Distribution};
use permark_core::events::EventSink;
use permark_core::paths::{Root, Vault, VaultPath};
use permark_core::plugins::{
    AppServices, ChangeListener, CxWorkspace, FolderScan, PluginCx, RpcCall, RpcTable, Running,
    ServiceLease, Services, WindowFiles, WindowFilesApi,
};
use permark_core::settings::{AppSettings, ProjectConfig};
use permark_core::state::{AppDirs, StateStore};
use permark_core::tree::{Explorer, ScanFile};
use permark_core::watch::{Change, ChangeKind, FsBatch, Subscription, WatchHub};
use permark_core::IpcError;
use serde_json::{json, Value};

use super::*;
use crate::tests::{pin_of, rank_file, Events, Source};
use crate::tokenizer::Encoding;
use crate::vocab::{Pin, VocabState};
use crate::{plugin_with, Fetch, ID};

const WINDOW: &str = "main";

/// The project of a test: a real Explorer over a temp folder (the ignore rules are the app's), and a watch hub the
/// test publishes to.
struct TestFiles {
    explorer: Explorer,
    vault: Vault,
    root: String,
    hub: Arc<WatchHub>,
    /// Runs right after a scan (a file vanishes between the listing and the read).
    after_scan: Mutex<Option<Box<dyn Fn() + Send>>>,
    scans: AtomicUsize,
}

impl TestFiles {
    fn new(project: &Path) -> Arc<Self> {
        let vault = Vault::new(Root::open(project).unwrap());
        let root = vault.main().path().display().to_string();
        let explorer = Explorer::new(
            &vault,
            &ProjectConfig::default(),
            Arc::new(Events::default()),
            None,
        );
        Arc::new(Self {
            explorer,
            vault,
            root,
            hub: WatchHub::new(),
            after_scan: Mutex::new(None),
            scans: AtomicUsize::new(0),
        })
    }

    fn publish(&self, changes: &[(&str, ChangeKind, bool)], bulk: bool, rescan: bool) {
        self.hub.publish(&FsBatch {
            changes: changes
                .iter()
                .map(|(path, kind, is_dir)| Change {
                    path: (*path).to_owned(),
                    kind: kind.clone(),
                    is_dir: *is_dir,
                    own: false,
                })
                .collect(),
            bulk,
            rescan,
        });
    }

    fn modified(&self, path: &str) {
        self.publish(&[(path, ChangeKind::Modified, false)], false, false);
    }
}

impl WindowFiles for TestFiles {
    fn read(&self, _: &str, _: &str, _: u64) -> Result<String, IpcError> {
        Err(IpcError::new(permark_core::ErrorKind::Unavailable))
    }

    fn write_beside(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, IpcError> {
        Err(IpcError::new(permark_core::ErrorKind::Unavailable))
    }

    fn note(&self, _: &str, _: permark_core::activity::Fact) {}

    fn scan_folder(
        &self,
        _window: &str,
        folder: &str,
        cancel: &AtomicBool,
        stop: &mut dyn FnMut(&ScanFile) -> bool,
    ) -> Result<FolderScan, IpcError> {
        self.scans.fetch_add(1, Ordering::SeqCst);
        let path = VaultPath::parse(folder)?;
        let abs = self.vault.resolve(&path)?;
        let scan = self.explorer.scan_folder(&path, &abs, cancel, stop)?;
        if let Some(hook) = &*self.after_scan.lock().unwrap() {
            hook();
        }
        Ok(FolderScan {
            project: self.root.clone(),
            scan,
        })
    }

    fn watch(&self, _window: &str, listener: ChangeListener) -> Option<Subscription> {
        Some(self.hub.subscribe(move |batch| listener(batch)))
    }
}

fn put(project: &Path, rel: &str, content: &[u8]) {
    let path = project.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// `n` characters: with `chars4`, `n / 4` tokens.
fn text(n: usize) -> Vec<u8> {
    "abcd".repeat(n.div_ceil(4)).as_bytes()[..n].to_vec()
}

fn cx_for(
    dirs: &AppDirs,
    events: &Arc<Events>,
    rpc: &Arc<RpcTable>,
    services: &Arc<Services>,
    settings: &Arc<RwLock<AppSettings>>,
) -> PluginCx {
    let store = StateStore::app(dirs);
    PluginCx::new(
        ID,
        CxWorkspace::App,
        store.plugin_dir(&ID),
        None,
        Context::welcome(Distribution::Direct),
        Arc::clone(events) as Arc<dyn EventSink>,
    )
    .with_store(store)
    .with_app(AppServices::new(
        Arc::clone(rpc),
        Arc::clone(services),
        Arc::clone(settings),
    ))
}

/// The folder count over a real project folder, driven directly (no RPC).
struct Rig {
    state: tempfile::TempDir,
    project: tempfile::TempDir,
    dirs: AppDirs,
    events: Arc<Events>,
    files: Arc<TestFiles>,
    folders: Arc<Folders>,
    vocab: Arc<VocabStore>,
    _lease: Option<ServiceLease>,
    _stop: Stop,
}

/// Ends the jobs of a rig when it goes.
struct Stop(Arc<Folders>);

impl Drop for Stop {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

impl Rig {
    fn new(thresholds: Thresholds) -> Self {
        let state = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        Self::over(state, project, thresholds, true)
    }

    fn over(
        state: tempfile::TempDir,
        project: tempfile::TempDir,
        thresholds: Thresholds,
        with_files: bool,
    ) -> Self {
        let dirs = AppDirs::under(state.path());
        let events = Arc::new(Events::default());
        let rpc = RpcTable::new();
        let services = Services::new();
        let settings = Arc::new(RwLock::new(AppSettings::default()));
        let files = TestFiles::new(project.path());
        let lease = with_files.then(|| {
            services.provide(Arc::new(WindowFilesApi(
                Arc::clone(&files) as Arc<dyn WindowFiles>
            )))
        });
        let cx = cx_for(&dirs, &events, &rpc, &services, &settings);
        let body = rank_file();
        let source = Arc::new(Source {
            body: body.clone(),
            calls: AtomicUsize::new(0),
        });
        let store = StateStore::app(&dirs);
        let cache = store.cache_dir().join("plugins").join(ID.as_str());
        let vocab = VocabStore::new(
            cache.clone(),
            [pin_of(&body), pin_of(&body)],
            source as Arc<dyn Fetch>,
            Arc::new(|_| {}),
        );
        let folders = Folders::new(
            cx.events().clone(),
            AppServices::new(rpc, services, settings),
            Arc::clone(&vocab),
            thresholds,
            cache.join("folders"),
        );
        Self {
            state,
            project,
            dirs,
            events,
            files,
            _stop: Stop(Arc::clone(&folders)),
            folders,
            vocab,
            _lease: lease,
        }
    }

    /// The same state folder and project over again (the app was closed and opened).
    fn reopen(self, thresholds: Thresholds) -> Self {
        let Self {
            state,
            project,
            folders,
            _stop: stop,
            ..
        } = self;
        folders.shutdown();
        drop((folders, stop));
        Self::over(state, project, thresholds, true)
    }

    fn put(&self, rel: &str, content: &[u8]) {
        put(self.project.path(), rel, content);
    }

    fn start(&self, folder: &str, tokenizer: &str) -> FolderJob {
        self.start_as(WINDOW, folder, tokenizer, false)
    }

    fn start_as(&self, window: &str, folder: &str, tokenizer: &str, auto: bool) -> FolderJob {
        self.folders
            .start(
                window,
                &StartArgs {
                    folder: folder.to_owned(),
                    tokenizer: tokenizer.to_owned(),
                    auto,
                },
            )
            .unwrap()
    }

    /// Waits until the window's job is neither scanning nor counting.
    fn settle(&self, window: &str) -> FolderJob {
        self.wait(window, |job| {
            !matches!(job, FolderJob::Scanning { .. } | FolderJob::Counting { .. })
        })
    }

    fn wait(&self, window: &str, pred: impl Fn(&FolderJob) -> bool) -> FolderJob {
        let began = Instant::now();
        loop {
            let job = self.folders.job(window);
            if pred(&job) {
                return job;
            }
            assert!(
                began.elapsed() < Duration::from_secs(120),
                "stuck at {job:?}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Counts a folder and returns its summary (the job ended in `Done`).
    fn count(&self, folder: &str, tokenizer: &str) -> FolderSummary {
        self.start(folder, tokenizer);
        let job = self.settle(WINDOW);
        assert!(matches!(job, FolderJob::Done { .. }), "{job:?}");
        self.dismiss();
        self.summary(folder).unwrap()
    }

    fn dismiss(&self) {
        self.folders.dismiss(WINDOW);
    }

    fn summary(&self, folder: &str) -> Option<FolderSummary> {
        self.folders
            .summary(&self.files.root, folder)
            .map(|s| (*s).clone())
    }

    fn badge(&self, folder: &str) -> Option<FolderBadge> {
        self.folders
            .badges(&self.files.root)
            .folders
            .into_iter()
            .find(|b| b.folder == folder)
    }

    /// Events of one name, in order.
    fn heard(&self, name: &str) -> Vec<Value> {
        self.events
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// States the window's job was announced in, one after the other.
    fn states(&self, window: &str) -> Vec<String> {
        self.heard("token-count:folder")
            .iter()
            .filter(|v| v["window"] == window)
            .map(|v| v["job"]["state"].as_str().unwrap().to_owned())
            .collect()
    }

    /// [`Rig::states`] with the progress announcements folded into one.
    fn phases(&self, window: &str) -> Vec<String> {
        let mut phases = self.states(window);
        phases.dedup();
        phases
    }

    /// Downloads the small test vocabulary of `o200k` and waits for it.
    fn with_vocabulary(&self) {
        self.vocab.download(Encoding::O200k);
        let began = Instant::now();
        while self.vocab.states()[Encoding::O200k.index()] != VocabState::Ready {
            assert!(began.elapsed() < Duration::from_secs(10), "no vocabulary");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

fn roomy() -> Thresholds {
    Thresholds::STANDARD
}

fn tight() -> Thresholds {
    Thresholds {
        confirm_files: 5,
        confirm_bytes: 1_000,
        refuse_files: 10,
        refuse_bytes: 5_000,
        auto_files: 3,
        auto_bytes: 100,
    }
}

// ---- a count ---------------------------------------------------------------------------------------------------------

#[test]
fn a_folder_is_counted_with_its_parts_and_the_result_is_kept() {
    let rig = Rig::new(roomy());
    rig.put("a.md", &text(8));
    rig.put("docs/b.md", &text(12));
    rig.put("docs/c.rs", &text(4));
    rig.put("docs/Makefile", &text(40));
    rig.put("logo.png", b"\x89PNG not read");
    rig.put("blob.dat", b"ab\0cd");
    rig.put("docs/big.txt", &text(5_000_001));
    rig.put("docs/exact.json", &text(5_000_000));
    let summary = rig.count("", "chars4");
    assert_eq!(summary.tokens, 2 + 3 + 1 + 10 + 1_250_000);
    assert_eq!((summary.counted, summary.skipped), (5, 3));
    assert_eq!(
        (
            summary.skipped_binary,
            summary.skipped_large,
            summary.skipped_unreadable
        ),
        (2, 1, 0)
    );
    let listed: Vec<(&str, FolderSkip)> = summary
        .skipped_list
        .iter()
        .map(|s| (s.path.as_str(), s.reason))
        .collect();
    assert_eq!(
        listed,
        [
            ("blob.dat", FolderSkip::Binary),
            ("docs/big.txt", FolderSkip::TooLarge),
            ("logo.png", FolderSkip::Binary)
        ]
    );
    assert_eq!(summary.top[0].path, "docs/exact.json");
    assert_eq!(summary.by_type[0].ext, "json");
    assert_eq!(
        (summary.tokenizer.as_str(), summary.approx),
        ("chars4", true)
    );
    assert_eq!(summary.cached, 0);
    // the badge and the events tell the windows
    let badge = rig.badge("").unwrap();
    assert_eq!(
        (badge.tokens, badge.approx, badge.stale),
        (summary.tokens, true, false)
    );
    assert_eq!(
        rig.phases(WINDOW),
        ["scanning", "counting", "done", "idle"],
        "start, the work, the end, the person's dismissal"
    );
    assert_eq!(rig.heard("token-count:folders").len(), 1);
}

#[test]
fn a_subfolder_is_counted_alone_and_its_name_is_normalized() {
    let rig = Rig::new(roomy());
    rig.put("top.md", &text(400));
    rig.put("docs/a.md", &text(8));
    rig.put("docs/deep/b.md", &text(8));
    let summary = rig.count("docs/", "chars4");
    assert_eq!(
        (summary.folder.as_str(), summary.tokens, summary.counted),
        ("docs", 4, 2)
    );
    let again = rig.count("docs\\deep", "chars4");
    assert_eq!((again.folder.as_str(), again.tokens), ("docs/deep", 2));
    assert_eq!(rig.folders.badges(&rig.files.root).folders.len(), 2);
    assert!(rig.summary("docs").is_some());
    assert!(rig.summary("top").is_none());
    assert!(rig.summary("../x").is_none());
}

#[test]
fn text_is_counted_as_the_app_reads_it() {
    let rig = Rig::new(roomy());
    // BOM, UTF-16 with a BOM, CRLF, accents, spaces and CJK in the path: 8 characters each, 2 tokens
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(b"abcdefgh");
    rig.put("dö cs/日本 語/bom.md", &bom);
    let mut wide = b"\xFF\xFE".to_vec();
    for unit in "abcdefgh".encode_utf16() {
        wide.extend_from_slice(&unit.to_le_bytes());
    }
    rig.put("dö cs/日本 語/wide.txt", &wide);
    rig.put("dö cs/日本 語/crlf.md", b"ab\r\ncd\r\nef\r\n");
    rig.put("dö cs/accents.md", "áéíóúáéíó".as_bytes());
    let summary = rig.count("dö cs", "chars4");
    assert_eq!(summary.counted, 4);
    // 2 + 2 + ceil(12 / 4) + ceil(9 / 4)
    assert_eq!(summary.tokens, 2 + 2 + 3 + 3);
    let nested = rig.count("dö cs/日本 語", "chars4");
    assert_eq!((nested.counted, nested.tokens), (3, 7));
}

#[test]
fn ignored_files_are_not_counted_and_a_folder_that_is_gone_fails_plainly() {
    let rig = Rig::new(roomy());
    rig.put(".gitignore", b"ignored/\n*.log\n");
    rig.put("keep.md", &text(4));
    rig.put("ignored/skip.md", &text(400));
    rig.put("sub/x.log", &text(400));
    rig.put("sub/y.md", &text(4));
    rig.put("node_modules/lib/z.md", &text(400));
    let summary = rig.count("", "chars4");
    // the ignore file itself is a project file
    assert_eq!(summary.counted, 3, "{:?}", summary.top);
    assert_eq!(summary.tokens, 1 + 1 + 4);
    assert!(summary.top.iter().all(|t| !t.path.contains("skip")));
    // not a folder, not there, not a project
    for folder in ["keep.md", "nowhere"] {
        rig.start(folder, "chars4");
        let job = rig.settle(WINDOW);
        assert_eq!(
            job,
            FolderJob::Failed {
                folder: folder.to_owned(),
                reason: FolderFailure::NotFound
            }
        );
        rig.dismiss();
    }
    assert!(rig.badge("nowhere").is_none());
}

#[test]
fn a_window_whose_workspace_offers_no_files_is_unavailable() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let rig = Rig::over(state, project, roomy(), false);
    rig.start("", "chars4");
    let job = rig.settle(WINDOW);
    assert_eq!(
        job,
        FolderJob::Failed {
            folder: String::new(),
            reason: FolderFailure::Unavailable
        }
    );
}

#[test]
fn a_bad_request_is_refused_before_a_job_exists() {
    let rig = Rig::new(roomy());
    let ask = |folder: &str, tokenizer: &str| {
        rig.folders.start(
            WINDOW,
            &StartArgs {
                folder: folder.to_owned(),
                tokenizer: tokenizer.to_owned(),
                auto: false,
            },
        )
    };
    assert!(ask("", "gpt-9").is_err());
    assert!(ask("", "").is_err());
    assert!(ask("../outside", "chars4").is_err());
    assert!(ask("C:\\Windows", "chars4").is_err());
    assert!(ask("a\0b", "chars4").is_err());
    assert_eq!(rig.folders.job(WINDOW), FolderJob::Idle);
    assert_eq!(rig.files.scans.load(Ordering::SeqCst), 0);
}

// ---- the guard -----------------------------------------------------------------------------------------------------

#[test]
fn a_big_folder_asks_first_and_nothing_is_read_until_the_person_agrees() {
    let rig = Rig::new(tight());
    for n in 0..5 {
        rig.put(&format!("five/f{n}.md"), &text(8));
    }
    for n in 0..6 {
        rig.put(&format!("six/f{n}.md"), &text(8));
    }
    // five files are the limit itself: counted at once
    rig.start("five", "chars4");
    assert!(matches!(rig.settle(WINDOW), FolderJob::Done { .. }));
    rig.dismiss();
    // six are not: the person is asked, with the numbers and a time
    rig.start("six", "chars4");
    let job = rig.settle(WINDOW);
    match &job {
        FolderJob::Confirm {
            folder,
            files,
            bytes,
            seconds,
        } => {
            assert_eq!((folder.as_str(), *files, *bytes), ("six", 6, 48));
            assert!(*seconds >= 1);
        }
        other => panic!("{other:?}"),
    }
    assert!(rig.summary("six").is_none());
    // a new request while it waits changes nothing
    assert_eq!(rig.start("five", "chars4"), job);
    // agreeing counts it
    let counting = rig.folders.confirm(WINDOW);
    assert!(
        matches!(counting, FolderJob::Counting { .. }),
        "{counting:?}"
    );
    assert!(matches!(rig.settle(WINDOW), FolderJob::Done { .. }));
    assert_eq!(rig.summary("six").unwrap().tokens, 12);
}

#[test]
fn declining_a_big_folder_leaves_nothing() {
    let rig = Rig::new(tight());
    for n in 0..6 {
        rig.put(&format!("six/f{n}.md"), &text(8));
    }
    rig.start("six", "chars4");
    assert!(matches!(rig.settle(WINDOW), FolderJob::Confirm { .. }));
    assert_eq!(rig.folders.cancel(WINDOW), FolderJob::Idle);
    assert_eq!(rig.folders.job(WINDOW), FolderJob::Idle);
    assert!(rig.badge("six").is_none());
    // the held scan is gone: confirming now does nothing
    assert_eq!(rig.folders.confirm(WINDOW), FolderJob::Idle);
    // and a fresh request works
    rig.start("six", "chars4");
    assert!(matches!(rig.settle(WINDOW), FolderJob::Confirm { .. }));
}

#[test]
fn bytes_ask_too_and_pictures_do_not_count_as_bytes() {
    let rig = Rig::new(tight());
    rig.put("pics/a.png", &vec![1_u8; 4_000]);
    rig.put("pics/b.png", &vec![1_u8; 4_000]);
    rig.put("pics/t.md", &text(40));
    // 8 KB of pictures, 40 bytes of text: under 1 000 bytes to read, nothing asked
    rig.start("pics", "chars4");
    assert!(matches!(rig.settle(WINDOW), FolderJob::Done { .. }));
    rig.dismiss();
    rig.put("text/a.md", &text(600));
    rig.put("text/b.md", &text(600));
    rig.start("text", "chars4");
    match rig.settle(WINDOW) {
        FolderJob::Confirm { files, bytes, .. } => assert_eq!((files, bytes), (2, 1_200)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_folder_over_the_cap_is_refused_and_the_walk_stops_there() {
    let rig = Rig::new(tight());
    for n in 0..30 {
        rig.put(&format!("many/f{n:02}.md"), &text(4));
    }
    rig.start("many", "chars4");
    let job = rig.settle(WINDOW);
    assert_eq!(
        job,
        FolderJob::Refused {
            folder: "many".to_owned(),
            files: 11,
            bytes: 44,
            reason: FolderRefusal::Files,
        },
        "the walk ended at the first file past the limit"
    );
    assert!(rig.summary("many").is_none());
    assert_eq!(rig.folders.confirm(WINDOW), job, "no override");
    // the same by bytes
    rig.dismiss();
    for n in 0..4 {
        rig.put(&format!("heavy/f{n}.md"), &text(2_000));
    }
    rig.start("heavy", "chars4");
    match rig.settle(WINDOW) {
        FolderJob::Refused { reason, bytes, .. } => {
            assert_eq!(reason, FolderRefusal::Bytes);
            assert_eq!(bytes, 6_000);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_automatic_count_never_asks_and_leaves_big_folders_alone() {
    let rig = Rig::new(Thresholds {
        confirm_files: 2,
        ..tight()
    });
    for n in 0..3 {
        rig.put(&format!("small/f{n}.md"), &text(8));
    }
    for n in 0..4 {
        rig.put(&format!("big/f{n}.md"), &text(8));
    }
    // three files are over the confirm limit, but an automatic job has its own: it counts, quietly
    rig.start_as(WINDOW, "small", "chars4", true);
    assert!(matches!(rig.settle(WINDOW), FolderJob::Done { .. }));
    assert_eq!(rig.summary("small").unwrap().tokens, 6);
    rig.dismiss();
    // four files: back to idle, no dialog, no result, no trace in the states
    rig.start_as(WINDOW, "big", "chars4", true);
    assert_eq!(rig.settle(WINDOW), FolderJob::Idle);
    assert!(rig.summary("big").is_none());
    let states = rig.phases(WINDOW);
    assert!(
        !states.iter().any(|s| s == "confirm" || s == "refused"),
        "{states:?}"
    );
    // by bytes
    rig.put("fat/a.md", &text(101));
    rig.start_as(WINDOW, "fat", "chars4", true);
    assert_eq!(rig.settle(WINDOW), FolderJob::Idle);
}

// ---- the job -------------------------------------------------------------------------------------------------------

#[test]
fn a_job_cancelled_half_way_leaves_no_result_but_keeps_what_it_learned() {
    let rig = Rig::new(roomy());
    for n in 0..300 {
        rig.put(&format!("many/f{n:03}.md"), &text(40));
    }
    rig.folders.pace.store(2_000, Ordering::Relaxed);
    rig.start("many", "chars4");
    let job = rig.wait(
        WINDOW,
        |j| matches!(j, FolderJob::Counting { done, .. } if *done >= 20),
    );
    let FolderJob::Counting { done, total, .. } = job else {
        unreachable!()
    };
    assert!(done < total);
    let told = rig.folders.cancel(WINDOW);
    assert!(
        matches!(told, FolderJob::Counting { .. }),
        "the flag is set, the thread ends at its next file: {told:?}"
    );
    let job = rig.settle(WINDOW);
    assert_eq!(
        job,
        FolderJob::Cancelled {
            folder: "many".to_owned()
        }
    );
    assert!(rig.badge("many").is_none(), "no partial badge");
    assert!(rig.summary("many").is_none(), "no partial summary");
    assert!(rig.heard("token-count:folders").is_empty());
    // the next count takes the files counted so far from the cache
    rig.folders.pace.store(0, Ordering::Relaxed);
    rig.dismiss();
    let summary = rig.count("many", "chars4");
    assert_eq!((summary.counted, summary.tokens), (300, 3_000));
    assert!(
        summary.cached >= 20 && summary.cached < 300,
        "{}",
        summary.cached
    );
}

#[test]
fn progress_is_announced_at_most_ten_times_a_second() {
    let rig = Rig::new(roomy());
    for n in 0..200 {
        rig.put(&format!("many/f{n:03}.md"), &text(8));
    }
    rig.folders.pace.store(5_000, Ordering::Relaxed);
    let began = Instant::now();
    rig.start("many", "chars4");
    rig.settle(WINDOW);
    let seconds = began.elapsed().as_secs_f64();
    let counting = rig
        .states(WINDOW)
        .iter()
        .filter(|s| *s == "counting")
        .count();
    assert!(counting >= 3, "progress was announced: {counting}");
    assert!(
        (counting as f64) <= seconds * 10.0 + 3.0,
        "{counting} announcements in {seconds} s"
    );
}

#[test]
fn one_job_per_window_and_the_windows_do_not_disturb_each_other() {
    let rig = Rig::new(roomy());
    for n in 0..100 {
        rig.put(&format!("a/f{n:03}.md"), &text(8));
        rig.put(&format!("b/f{n:03}.md"), &text(8));
    }
    rig.folders.pace.store(2_000, Ordering::Relaxed);
    let first = rig.start_as("one", "a", "chars4", false);
    // the same window asking again gets the job it has
    let second = rig.start_as("one", "b", "chars4", false);
    assert_eq!(first, second);
    // another window has its own slot
    rig.start_as("two", "b", "chars4", false);
    rig.folders.cancel("one");
    assert!(matches!(rig.settle("one"), FolderJob::Cancelled { .. }));
    assert!(matches!(rig.settle("two"), FolderJob::Done { .. }));
    assert!(rig.summary("a").is_none());
    assert_eq!(rig.summary("b").unwrap().counted, 100);
    assert!(rig.phases("one").iter().all(|s| s != "done"));
}

#[test]
fn starting_stopping_and_dismissing_in_any_order_never_hangs_or_corrupts() {
    let rig = Rig::new(roomy());
    for n in 0..40 {
        rig.put(&format!("d/f{n:02}.md"), &text(8));
    }
    // nothing running: harmless
    assert_eq!(rig.folders.cancel(WINDOW), FolderJob::Idle);
    assert_eq!(rig.folders.confirm(WINDOW), FolderJob::Idle);
    assert_eq!(rig.folders.dismiss(WINDOW), FolderJob::Idle);
    rig.folders.pace.store(300, Ordering::Relaxed);
    for round in 0..40 {
        rig.start("d", "chars4");
        if round % 3 == 0 {
            rig.folders.cancel(WINDOW);
        }
        if round % 4 == 0 {
            rig.folders.confirm(WINDOW);
            rig.folders.dismiss(WINDOW);
        }
        if round % 2 == 0 {
            rig.settle(WINDOW);
            rig.folders.dismiss(WINDOW);
        }
    }
    let job = rig.settle(WINDOW);
    assert!(
        matches!(job, FolderJob::Done { .. } | FolderJob::Cancelled { .. }),
        "{job:?}"
    );
    // whatever happened, a count that ends in Done is a full one
    if let Some(summary) = rig.summary("d") {
        assert_eq!((summary.counted, summary.tokens), (40, 80));
    }
}

#[test]
fn shutting_down_ends_a_running_job_and_forgets_everything() {
    let rig = Rig::new(roomy());
    for n in 0..200 {
        rig.put(&format!("d/f{n:03}.md"), &text(8));
    }
    rig.count("d", "chars4");
    rig.folders.pace.store(10_000, Ordering::Relaxed);
    rig.start("d", "chars4");
    rig.wait(WINDOW, |j| matches!(j, FolderJob::Counting { .. }));
    let began = Instant::now();
    rig.folders.shutdown();
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "the thread was joined, not waited out: {:?}",
        began.elapsed()
    );
    assert_eq!(rig.folders.job(WINDOW), FolderJob::Idle);
    assert!(rig.folders.badges(&rig.files.root).folders.is_empty());
    // the watch is gone: a change does not reach a project that is forgotten
    let before = rig.heard("token-count:folders").len();
    rig.files.modified("d/f000.md");
    assert_eq!(rig.heard("token-count:folders").len(), before);
}

#[test]
fn a_file_that_vanishes_between_the_listing_and_the_read_is_skipped_as_unreadable() {
    let rig = Rig::new(roomy());
    rig.put("d/keep.md", &text(8));
    rig.put("d/gone.md", &text(8));
    let doomed = rig.project.path().join("d/gone.md");
    *rig.files.after_scan.lock().unwrap() = Some(Box::new(move || {
        let _ = fs::remove_file(&doomed);
    }));
    let summary = rig.count("d", "chars4");
    assert_eq!((summary.counted, summary.skipped_unreadable), (1, 1));
    assert_eq!(summary.skipped_list[0].path, "d/gone.md");
    assert_eq!(summary.skipped_list[0].reason, FolderSkip::Unreadable);
    assert_eq!(summary.tokens, 2);
}

// ---- the tokenizer ---------------------------------------------------------------------------------------------------

#[test]
fn an_exact_tokenizer_without_its_vocabulary_counts_with_its_stand_in_and_says_so() {
    let rig = Rig::new(roomy());
    rig.put("d/a.md", b"hello world hello world hello world");
    let first = rig.count("d", "o200k");
    assert_eq!(first.tokenizer, "o200k");
    assert!(first.approx, "the interim estimate is marked");
    assert!(first.tokens > 0);
    assert!(rig.badge("d").unwrap().approx);
    let counting: Vec<Value> = rig
        .heard("token-count:folder")
        .into_iter()
        .filter(|v| v["job"]["state"] == "counting")
        .collect();
    assert!(counting.iter().all(|v| v["job"]["approx"] == true));
    // the same words with the vocabulary: exact, not marked, a number of its own
    rig.with_vocabulary();
    let exact = rig.count("d", "o200k");
    assert!(!exact.approx);
    // "hello" and " world" are tokens of the small test vocabulary, " hello" is a space and "hello"
    assert_eq!(exact.tokens, 8);
    assert_eq!(
        exact.cached, 0,
        "the stand-in's counts are not the exact ones"
    );
    assert!(!rig.badge("d").unwrap().approx);
    // and again from the cache
    assert_eq!(rig.count("d", "o200k").cached, 1);
}

#[test]
fn every_estimate_family_is_marked_and_counted_on_its_own() {
    let rig = Rig::new(roomy());
    rig.put("d/a.md", &text(400));
    for tokenizer in ["claude", "gemini", "chars4"] {
        let summary = rig.count("d", tokenizer);
        assert!(summary.approx, "{tokenizer}");
        assert_eq!(summary.tokenizer, tokenizer);
        assert_eq!(
            summary.cached, 0,
            "{tokenizer}: no other tokenizer's numbers"
        );
    }
    assert_eq!(rig.count("d", "chars4").cached, 1);
    assert_eq!(rig.count("d", "chars4").tokens, 100);
}

// ---- the cache -------------------------------------------------------------------------------------------------------

#[test]
fn a_repeat_count_reads_only_what_changed() {
    let rig = Rig::new(roomy());
    for n in 0..6 {
        rig.put(&format!("d/f{n}.md"), &text(40));
    }
    let first = rig.count("d", "chars4");
    assert_eq!((first.counted, first.cached, first.tokens), (6, 0, 60));
    // untouched: everything from the cache
    let second = rig.count("d", "chars4");
    assert_eq!((second.counted, second.cached, second.tokens), (6, 6, 60));
    // a parent reuses the counts of its children
    rig.put("top.md", &text(8));
    let parent = rig.count("", "chars4");
    assert_eq!((parent.counted, parent.cached, parent.tokens), (7, 6, 62));
    // one file grows: that one is counted again
    rig.put("d/f3.md", &text(80));
    let third = rig.count("d", "chars4");
    assert_eq!((third.counted, third.cached, third.tokens), (6, 5, 70));
    // a new file is counted, a removed one leaves
    rig.put("d/new.md", &text(4));
    fs::remove_file(rig.project.path().join("d/f0.md")).unwrap();
    let fourth = rig.count("d", "chars4");
    assert_eq!((fourth.counted, fourth.cached, fourth.tokens), (6, 5, 61));
}

#[test]
fn a_file_rewritten_with_the_same_content_is_taken_from_the_cache_by_its_hash() {
    let rig = Rig::new(roomy());
    rig.put("d/a.md", &text(40));
    rig.put("d/b.md", &text(40));
    rig.count("d", "chars4");
    // touched, not changed: another time, the same bytes
    rig.put("d/a.md", &text(40));
    let later = std::time::SystemTime::now() + Duration::from_secs(5);
    fs::File::options()
        .write(true)
        .open(rig.project.path().join("d/a.md"))
        .unwrap()
        .set_modified(later)
        .unwrap();
    let summary = rig.count("d", "chars4");
    assert_eq!(
        (summary.counted, summary.cached, summary.tokens),
        (2, 2, 20)
    );
    // and the entry was refreshed: the next count does not even read it
    assert_eq!(rig.count("d", "chars4").cached, 2);
}

#[test]
fn a_change_the_watch_reports_is_never_taken_from_the_cache() {
    let rig = Rig::new(roomy());
    rig.put("d/a.md", b"abcdabcd");
    let path = rig.project.path().join("d/a.md");
    let stamp = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(rig.count("d", "chars4").tokens, 2);
    // same length and modified time, other characters: only the watch can tell
    fs::write(&path, "éééé").unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), 8);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    let unseen = rig.count("d", "chars4");
    assert_eq!(
        (unseen.tokens, unseen.cached),
        (2, 1),
        "the metadata says nothing changed"
    );
    rig.files.modified("d/a.md");
    let seen = rig.count("d", "chars4");
    assert_eq!(
        (seen.tokens, seen.cached),
        (1, 0),
        "4 characters are 1 token"
    );
}

#[test]
fn what_a_folder_event_drops_it_drops_below() {
    let rig = Rig::new(roomy());
    rig.put("d/sub/a.md", &text(40));
    rig.put("d/sub/b.md", &text(40));
    rig.put("d/c.md", &text(40));
    rig.count("d", "chars4");
    rig.files
        .publish(&[("d/sub", ChangeKind::Removed, true)], false, false);
    let summary = rig.count("d", "chars4");
    assert_eq!(summary.cached, 1, "only d/c.md stayed in the cache");
    rig.files.publish(
        &[(
            "d/c.md",
            ChangeKind::Renamed {
                from: "d/old.md".to_owned(),
            },
            false,
        )],
        false,
        false,
    );
    assert_eq!(rig.count("d", "chars4").cached, 2);
}

#[test]
fn the_cache_outlives_the_app_and_goes_with_remove_data() {
    let rig = Rig::new(roomy());
    for n in 0..4 {
        rig.put(&format!("d/f{n}.md"), &text(40));
    }
    rig.put("d/blob.dat", b"a\0b");
    let first = rig.count("d", "chars4");
    assert_eq!((first.counted, first.skipped, first.cached), (4, 1, 0));
    let file_dir = rig
        .dirs
        .cache
        .join("plugins")
        .join("token-count")
        .join("folders");
    let files: Vec<_> = fs::read_dir(&file_dir).unwrap().flatten().collect();
    assert_eq!(files.len(), 1, "one cache file for the project");
    // the app is closed and opened again: the second count starts from the file
    let rig = rig.reopen(roomy());
    let second = rig.count("d", "chars4");
    assert_eq!((second.counted, second.skipped, second.cached), (4, 1, 4));
    assert_eq!(second.tokens, first.tokens);
    // remove data deletes it
    StateStore::app(&rig.dirs).remove_plugin_data(&ID).unwrap();
    assert!(!file_dir.exists());
}

#[test]
fn a_damaged_cache_file_is_a_cold_start() {
    let rig = Rig::new(roomy());
    rig.put("d/a.md", &text(40));
    rig.count("d", "chars4");
    let file_dir = rig
        .dirs
        .cache
        .join("plugins")
        .join("token-count")
        .join("folders");
    for entry in fs::read_dir(&file_dir).unwrap().flatten() {
        let mut file = fs::File::create(entry.path()).unwrap();
        file.write_all(b"{ \"v\": 1, \"bases\": [[[[").unwrap();
    }
    let rig = rig.reopen(roomy());
    let summary = rig.count("d", "chars4");
    assert_eq!(
        (summary.counted, summary.cached, summary.tokens),
        (1, 0, 10)
    );
}

// ---- stale results -----------------------------------------------------------------------------------------------------

#[test]
fn a_change_under_a_counted_folder_marks_it_stale_and_nothing_else() {
    let rig = Rig::new(roomy());
    rig.put("docs/a.md", &text(40));
    rig.put("docs/deep/b.md", &text(40));
    rig.put("other/c.md", &text(40));
    rig.put("docsx/d.md", &text(40));
    for folder in ["docs", "docs/deep", "other", ""] {
        rig.count(folder, "chars4");
    }
    let stale = |folder: &str| rig.badge(folder).unwrap().stale;
    assert!(["docs", "docs/deep", "other", ""].iter().all(|f| !stale(f)));
    let told = rig.heard("token-count:folders").len();
    // a file in docs: docs and the project are stale; the folder below and the others are not
    rig.files.modified("docs/a.md");
    assert!(stale("docs") && stale(""));
    assert!(!stale("docs/deep") && !stale("other"));
    assert_eq!(rig.heard("token-count:folders").len(), told + 1);
    let last = rig.heard("token-count:folders").pop().unwrap();
    assert_eq!(last["root"], rig.files.root);
    let said: Vec<(&str, bool)> = last["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["folder"].as_str().unwrap(), f["stale"].as_bool().unwrap()))
        .collect();
    assert!(said.contains(&("docs", true)) && said.contains(&("other", false)));
    // a folder that only starts with the same letters is another folder
    rig.files.modified("docsx/d.md");
    assert!(!stale("other") && !stale("docs/deep"));
    // a second change in the same stale folder says nothing new
    let told = rig.heard("token-count:folders").len();
    rig.files.modified("docs/a.md");
    assert_eq!(rig.heard("token-count:folders").len(), told);
    // counting again clears the mark
    rig.count("docs", "chars4");
    assert!(!stale("docs"));
    // a rename: both ends count
    rig.files.publish(
        &[(
            "elsewhere/x.md",
            ChangeKind::Renamed {
                from: "other/c.md".to_owned(),
            },
            false,
        )],
        false,
        false,
    );
    assert!(stale("other"));
    // events lost, or a bulk change: everything is stale
    rig.count("other", "chars4");
    rig.files.publish(&[], false, true);
    assert!(["docs", "docs/deep", "other", ""].iter().all(|f| stale(f)));
    rig.count("other", "chars4");
    rig.files
        .publish(&[("x.md", ChangeKind::Created, false)], true, false);
    assert!(stale("other"));
}

#[test]
fn a_change_while_the_job_works_makes_its_result_stale_at_once() {
    let rig = Rig::new(roomy());
    for n in 0..100 {
        rig.put(&format!("d/f{n:03}.md"), &text(8));
    }
    rig.put("elsewhere/e.md", &text(8));
    rig.folders.pace.store(2_000, Ordering::Relaxed);
    rig.start("d", "chars4");
    rig.wait(
        WINDOW,
        |j| matches!(j, FolderJob::Counting { done, .. } if *done >= 5),
    );
    rig.files.modified("elsewhere/e.md");
    rig.files.modified("d/f050.md");
    rig.settle(WINDOW);
    assert!(rig.badge("d").unwrap().stale);
    // a change outside the folder would not have
    rig.dismiss();
    rig.folders.pace.store(0, Ordering::Relaxed);
    rig.put("quiet/q.md", &text(8));
    rig.count("quiet", "chars4");
    rig.files.modified("elsewhere/e.md");
    assert!(!rig.badge("quiet").unwrap().stale);
}

#[test]
fn at_most_eight_projects_are_kept_and_the_oldest_goes() {
    let mut projects = Projects::default();
    let made = |_: &str| Project {
        cache: Arc::new(Mutex::new(FileCache::default())),
        results: BTreeMap::new(),
        sub: None,
        runs: Vec::new(),
    };
    for n in 0..PROJECTS_KEPT + 3 {
        let root = format!("root{n}");
        projects.project(&root, || made(&root));
    }
    assert_eq!(projects.by_root.len(), PROJECTS_KEPT);
    assert!(!projects.by_root.contains_key("root0"));
    assert!(projects.by_root.contains_key("root10"));
    // using one keeps it
    projects.project("root3", || made("root3"));
    projects.project("new", || made("new"));
    assert!(projects.by_root.contains_key("root3"));
    assert!(!projects.by_root.contains_key("root4"));
    // a project with a job running is not dropped
    projects.by_root.get_mut("root3").unwrap().runs.push(Run {
        job: 1,
        folder: String::new(),
        touched: false,
    });
    for n in 0..PROJECTS_KEPT * 2 {
        let root = format!("more{n}");
        projects.project(&root, || made(&root));
    }
    assert!(projects.by_root.contains_key("root3"));
    assert_eq!(projects.by_root.len(), PROJECTS_KEPT);
}

#[test]
fn covers_means_the_folder_itself_or_below_it() {
    assert!(covers("", "a.md"));
    assert!(covers("", ""));
    assert!(covers("docs", "docs"));
    assert!(covers("docs", "docs/a.md"));
    assert!(covers("docs", "docs/deep/a.md"));
    assert!(!covers("docs", "docsx/a.md"));
    assert!(!covers("docs", "doc"));
    assert!(!covers("docs/deep", "docs/a.md"));
}

// ---- through the plugin's calls ------------------------------------------------------------------------------------

struct Full {
    _state: tempfile::TempDir,
    project: tempfile::TempDir,
    dirs: AppDirs,
    rpc: Arc<RpcTable>,
    services: Arc<Services>,
    settings: Arc<RwLock<AppSettings>>,
    events: Arc<Events>,
    files: Arc<TestFiles>,
    running: Option<Box<dyn Running>>,
    _lease: ServiceLease,
}

fn start_plugin(
    full_dirs: &AppDirs,
    events: &Arc<Events>,
    rpc: &Arc<RpcTable>,
    services: &Arc<Services>,
    settings: &Arc<RwLock<AppSettings>>,
) -> Box<dyn Running> {
    let cx = cx_for(full_dirs, events, rpc, services, settings);
    let body = rank_file();
    let source = Arc::new(Source {
        body: body.clone(),
        calls: AtomicUsize::new(0),
    });
    let pins: [Pin; 2] = [pin_of(&body), pin_of(&body)];
    plugin_with(source as Arc<dyn Fetch>, pins)
        .start(&cx)
        .unwrap()
}

impl Full {
    fn start() -> Self {
        let state = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(state.path());
        let events = Arc::new(Events::default());
        let rpc = RpcTable::new();
        let services = Services::new();
        let settings = Arc::new(RwLock::new(AppSettings::default()));
        let files = TestFiles::new(project.path());
        let lease = services.provide(Arc::new(WindowFilesApi(
            Arc::clone(&files) as Arc<dyn WindowFiles>
        )));
        let running = start_plugin(&dirs, &events, &rpc, &services, &settings);
        Self {
            _state: state,
            project,
            dirs,
            rpc,
            services,
            settings,
            events,
            files,
            running: Some(running),
            _lease: lease,
        }
    }

    /// Turned off and on again.
    fn restart(&mut self) {
        self.running = None;
        self.running = Some(start_plugin(
            &self.dirs,
            &self.events,
            &self.rpc,
            &self.services,
            &self.settings,
        ));
    }

    fn call(&self, method: &str, args: Value) -> Result<Value, IpcError> {
        self.rpc.call(
            "token-count",
            method,
            &RpcCall {
                window: WINDOW.into(),
                args,
            },
        )
    }

    fn state_of(&self) -> String {
        self.call("folderJob", json!({})).unwrap()["state"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

#[test]
fn the_calls_drive_a_count_and_validate_what_they_get() {
    let full = Full::start();
    put(full.project.path(), "d/a.md", &text(40));
    assert_eq!(full.state_of(), "idle");
    let started = full
        .call(
            "folderStart",
            json!({ "folder": "d", "tokenizer": "chars4" }),
        )
        .unwrap();
    assert_eq!(started["state"], "scanning");
    let began = Instant::now();
    while full.state_of() != "done" {
        assert!(began.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(2));
    }
    let badges = full
        .call("folders", json!({ "root": full.files.root }))
        .unwrap();
    assert_eq!(badges["root"], full.files.root);
    assert_eq!(badges["folders"][0]["folder"], "d");
    assert_eq!(badges["folders"][0]["tokens"], 10);
    assert_eq!(badges["folders"][0]["approx"], true);
    assert_eq!(badges["folders"][0]["stale"], false);
    let summary = full
        .call(
            "folderSummary",
            json!({ "root": full.files.root, "folder": "d" }),
        )
        .unwrap();
    assert_eq!(summary["tokens"], 10);
    assert_eq!(summary["byType"][0]["ext"], "md");
    assert_eq!(summary["skippedList"], json!([]));
    let none = full
        .call(
            "folderSummary",
            json!({ "root": full.files.root, "folder": "nope" }),
        )
        .unwrap();
    assert!(none.is_null());
    assert_eq!(
        full.call("folders", json!({ "root": "somewhere else" }))
            .unwrap()["folders"],
        json!([])
    );
    assert_eq!(
        full.call("folderDismiss", json!({})).unwrap()["state"],
        "idle"
    );
    // arguments are checked
    for (method, args) in [
        ("folderStart", json!({ "folder": "d" })),
        (
            "folderStart",
            json!({ "folder": "d", "tokenizer": "chars4", "extra": 1 }),
        ),
        ("folderStart", json!({ "folder": "d", "tokenizer": "nope" })),
        (
            "folderStart",
            json!({ "folder": "../x", "tokenizer": "chars4" }),
        ),
        ("folders", json!({})),
        ("folderSummary", json!({ "root": "x" })),
        ("setFolderTotals", json!({})),
    ] {
        let error = full.call(method, args.clone()).unwrap_err();
        assert!(
            matches!(
                error.kind,
                permark_core::ErrorKind::InvalidData | permark_core::ErrorKind::PathEscape
            ),
            "{method} {args}: {error:?}"
        );
    }
}

#[test]
fn the_folder_totals_choice_is_kept_and_announced() {
    let mut full = Full::start();
    let state = full.call("state", json!({})).unwrap();
    assert_eq!(state["folderTotals"], false);
    let state = full.call("setFolderTotals", json!({ "on": true })).unwrap();
    assert_eq!(state["folderTotals"], true);
    assert_eq!(full.call("state", json!({})).unwrap()["folderTotals"], true);
    let heard = full.events.0.lock().unwrap().clone();
    assert!(heard
        .iter()
        .any(|(n, v)| n == "token-count:state" && v["folderTotals"] == true));
    // the outline switch keeps its own
    let state = full.call("setOutline", json!({ "on": true })).unwrap();
    assert_eq!(state["outline"], true);
    assert_eq!(state["folderTotals"], true);
    // the choice is in the profile: the plugin started again reads it
    full.restart();
    let state = full.call("state", json!({})).unwrap();
    assert_eq!(
        (&state["folderTotals"], &state["outline"]),
        (&json!(true), &json!(true))
    );
    full.call("setFolderTotals", json!({ "on": false }))
        .unwrap();
    full.restart();
    assert_eq!(
        full.call("state", json!({})).unwrap()["folderTotals"],
        false
    );
}

#[test]
fn turned_off_the_plugin_serves_no_job_listens_to_nothing_and_leaves_no_thread() {
    let mut full = Full::start();
    for n in 0..300 {
        put(full.project.path(), &format!("d/f{n:03}.md"), &text(8));
    }
    full.call(
        "folderStart",
        json!({ "folder": "d", "tokenizer": "chars4" }),
    )
    .unwrap();
    let began = Instant::now();
    while full.state_of() != "done" {
        assert!(began.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(2));
    }
    let told = full.events.0.lock().unwrap().len();
    full.running = None;
    assert_eq!(
        full.call("folderJob", json!({})).unwrap_err().kind,
        permark_core::ErrorKind::Unavailable
    );
    assert_eq!(
        full.call("folders", json!({ "root": full.files.root }))
            .unwrap_err()
            .kind,
        permark_core::ErrorKind::Unavailable
    );
    // a change in the project is not heard
    full.files.modified("d/f000.md");
    assert_eq!(full.events.0.lock().unwrap().len(), told);
    // remove data: the cache of the folder counts goes
    let cache = full
        .dirs
        .cache
        .join("plugins")
        .join("token-count")
        .join("folders");
    assert!(cache.is_dir());
    StateStore::app(&full.dirs).remove_plugin_data(&ID).unwrap();
    assert!(!cache.exists());
}

#[test]
fn a_big_tree_is_counted_without_the_calls_ever_waiting() {
    let full = Full::start();
    // 100 folders of 100 small files: 10 000 files, more than the guard counts without asking
    for folder in 0..100 {
        let dir = full.project.path().join(format!("f{folder:03}"));
        fs::create_dir_all(&dir).unwrap();
        for file in 0..100 {
            fs::write(
                dir.join(format!("n{file:03}.md")),
                format!(
                    "# File {folder}-{file}\n\nSome words to count in this file, for the test.\n"
                ),
            )
            .unwrap();
        }
    }
    let slowest = |latencies: &[Duration]| latencies.iter().max().copied().unwrap_or_default();
    // the request answers at once, whatever the size of the folder
    let began = Instant::now();
    let started = full
        .call(
            "folderStart",
            json!({ "folder": "", "tokenizer": "claude" }),
        )
        .unwrap();
    let start_latency = began.elapsed();
    assert_eq!(started["state"], "scanning");
    assert!(
        start_latency < Duration::from_millis(50),
        "{start_latency:?}"
    );
    // the guard asks (10 000 files)
    let began = Instant::now();
    while full.state_of() != "confirm" {
        assert!(
            began.elapsed() < Duration::from_secs(60),
            "{}",
            full.state_of()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let job = full.call("folderJob", json!({})).unwrap();
    assert_eq!(job["files"], 10_000);
    assert!(job["seconds"].as_u64().unwrap() >= 1);
    // while it counts, other calls keep being answered
    let confirm_began = Instant::now();
    let counting = full.call("folderConfirm", json!({})).unwrap();
    let confirm_latency = confirm_began.elapsed();
    assert_eq!(counting["state"], "counting");
    assert!(
        confirm_latency < Duration::from_millis(50),
        "{confirm_latency:?}"
    );
    let mut latencies = Vec::new();
    let began = Instant::now();
    loop {
        let asked = Instant::now();
        let job = full.call("folderJob", json!({})).unwrap();
        let _ = full
            .call("folders", json!({ "root": full.files.root }))
            .unwrap();
        let _ = full.call("state", json!({})).unwrap();
        latencies.push(asked.elapsed());
        match job["state"].as_str().unwrap() {
            "done" => break,
            "counting" => {}
            other => panic!("{other}: {job}"),
        }
        assert!(began.elapsed() < Duration::from_secs(120), "{job}");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        latencies.len() > 5,
        "the calls ran during the job: {}",
        latencies.len()
    );
    assert!(
        slowest(&latencies) < Duration::from_millis(50),
        "slowest call {:?} of {}",
        slowest(&latencies),
        latencies.len()
    );
    let summary = full
        .call(
            "folderSummary",
            json!({ "root": full.files.root, "folder": "" }),
        )
        .unwrap();
    assert_eq!(summary["counted"], 10_000);
    assert_eq!(summary["skipped"], 0);
    assert!(summary["tokens"].as_u64().unwrap() > 100_000);
    // the progress announcements stayed few
    let announced = full
        .events
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(n, v)| n == "token-count:folder" && v["job"]["state"] == "counting")
        .count();
    let seconds = began.elapsed().as_secs_f64() + 1.0;
    assert!(
        (announced as f64) <= seconds * 10.0 + 5.0,
        "{announced} in {seconds} s"
    );
}
