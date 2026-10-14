//! An open folder, with everything running for it: watcher, git worker, search index, state.
//!
//! This is the Tauri-free core behind the commands: [`Workspace::open`] wires the modules together and
//! reports through an [`EventSink`], which the app implements with Tauri events and tests implement
//! with a recorder.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};

use serde::Serialize;

use crate::error::AppError;
use crate::git::{
    self, BaselineKind, Discovery, GitContext, GitHandle, GitSettings, GitSnapshot, GitState,
};
use crate::links::{self, LinkTarget};
use crate::render::{self, DocumentView, Registry};
use crate::search::indexer;
use crate::search::{Bm25Index, SearchHit, SearchProvider};
use crate::session::{LastVisit, RecentStore, SessionState, StateLocation, StateStore};
use crate::util::{modified_ms, now_ms};
use crate::vault::{NodeKind, TreeNode, Vault, VaultMode};
use crate::watch::{FsBatch, GitProbe, PathFilter, SigStore, Verdict, WatchConfig, WatchService};

/// "Since your last visit" lists at most this many files.
const MAX_SINCE_FILES: usize = 500;
const DEFAULT_SEARCH_LIMIT: usize = 30;
const MAX_SEARCH_LIMIT: usize = 200;

/// Where the core reports what happens in the background.
pub trait EventSink: Send + Sync + 'static {
    /// Files changed (`fs-change`).
    fn fs_change(&self, batch: &FsBatch);
    /// Files or folders appeared or disappeared (`tree-changed`).
    fn tree_changed(&self);
    /// The git picture changed (`git-status-changed`).
    fn git_status(&self, snapshot: &GitSnapshot);
    /// Search indexing advanced (`index-progress`).
    fn index_progress(&self, indexed: usize, total: usize);
}

/// Forwards events only while its workspace is the current one, so a folder that is being replaced
/// cannot leak late events into the UI that is already showing the next folder.
pub struct GuardedSink {
    inner: Arc<dyn EventSink>,
    epoch: u64,
    current: Arc<AtomicU64>,
}

impl GuardedSink {
    /// `epoch` identifies one workspace; events pass while `current` still holds it.
    pub fn new(inner: Arc<dyn EventSink>, epoch: u64, current: Arc<AtomicU64>) -> Self {
        Self {
            inner,
            epoch,
            current,
        }
    }

    fn live(&self) -> bool {
        self.current.load(Ordering::SeqCst) == self.epoch
    }
}

impl EventSink for GuardedSink {
    fn fs_change(&self, batch: &FsBatch) {
        if self.live() {
            self.inner.fs_change(batch);
        }
    }

    fn tree_changed(&self) {
        if self.live() {
            self.inner.tree_changed();
        }
    }

    fn git_status(&self, snapshot: &GitSnapshot) {
        if self.live() {
            self.inner.git_status(snapshot);
        }
    }

    fn index_progress(&self, indexed: usize, total: usize) {
        if self.live() {
            self.inner.index_progress(indexed, total);
        }
    }
}

/// Matches the UI's `VaultInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub root: String,
    pub name: String,
    pub mode: VaultMode,
    pub initial_file: Option<String>,
    pub worktrees: Vec<WorktreeInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: String,
}

/// Matches the UI's `ChangedSince`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedSince {
    pub since_ms: Option<u64>,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    pub state: GitState,
    pub modified_ms: u64,
}

pub struct Workspace {
    vault: Arc<Vault>,
    registry: Registry,
    store: Mutex<StateStore>,
    config: crate::session::Config,
    git_ctx: Option<GitContext>,
    git: GitHandle,
    watcher: Mutex<Option<WatchService>>,
    index: Arc<RwLock<Bm25Index>>,
    filter: Arc<PathFilter>,
    cancel: Arc<AtomicBool>,
    index_thread: Mutex<Option<JoinHandle<()>>>,
    last_visit: Option<LastVisit>,
    closed: AtomicBool,
}

/// State shared between a workspace and its background threads.
#[derive(Clone)]
struct Shared {
    vault: Arc<Vault>,
    filter: Arc<PathFilter>,
    sigs: Arc<SigStore>,
    index: Arc<RwLock<Bm25Index>>,
    sink: Arc<dyn EventSink>,
}

/// Watches the roots; every flush updates the index, tells the UI, and wakes the git worker.
fn start_watcher(
    shared: &Shared,
    git_ctx: Option<&GitContext>,
    trigger_git: Arc<dyn Fn() + Send + Sync>,
) -> Result<WatchService, AppError> {
    let probe = GitProbe::new(
        git_ctx.map(|c| c.git_dir.clone()),
        git_ctx.map(|c| c.common_dir.clone()),
    );
    let s = shared.clone();
    Ok(WatchService::start(
        Arc::clone(&shared.vault),
        Arc::clone(&shared.filter),
        Arc::clone(&shared.sigs),
        probe,
        WatchConfig::default(),
        move |out| {
            if !out.all_changes.is_empty() {
                indexer::apply_changes(&s.vault, &s.index, &s.sigs, &out.all_changes);
            }
            if out.has_batch() {
                s.sink.fs_change(&out.batch);
            }
            if out.tree_changed {
                s.sink.tree_changed();
            }
            trigger_git();
        },
    )?)
}

/// Builds the search index in the background, reporting progress; `cancel` stops it early.
fn start_indexer(shared: &Shared, cancel: Arc<AtomicBool>) -> Option<JoinHandle<()>> {
    let s = shared.clone();
    thread::Builder::new()
        .name("trailmark-index".to_owned())
        .spawn(move || {
            indexer::build(
                &s.vault,
                &s.filter,
                &s.index,
                &s.sigs,
                &cancel,
                &mut |done, total| {
                    s.sink.index_progress(done, total);
                },
            );
        })
        .ok()
}

impl Workspace {
    /// Opens a folder (IDE mode) or a file (Simple mode) and starts everything for it.
    pub fn open(
        path: &Path,
        app_data_dir: &Path,
        home: Option<&Path>,
        sink: &Arc<dyn EventSink>,
    ) -> Result<Self, AppError> {
        let vault = Vault::open(path)?;
        let store = StateStore::open(app_data_dir, vault.root());
        let config = store.config();
        let last_visit = store.load_last_visit();

        // Git first: linked worktrees become extra roots of the vault.
        let discovery = git::discover(vault.root(), home);
        let git_ctx = match &discovery {
            Discovery::Repo(ctx) => Some(ctx.clone()),
            _ => None,
        };
        let vault = match &git_ctx {
            Some(ctx) => vault.with_worktrees(git::list_worktrees(ctx)),
            None => vault,
        };
        let vault = Arc::new(vault);

        let info_exclude = git_ctx.as_ref().and_then(GitContext::info_exclude);
        let roots: Vec<(PathBuf, Option<PathBuf>)> = vault
            .root_paths()
            .into_iter()
            .map(|r| (r.to_path_buf(), info_exclude.clone()))
            .collect();
        let filter = Arc::new(PathFilter::new(&roots, &config.ignore));
        let sigs = Arc::new(SigStore::new());
        let index = Arc::new(RwLock::new(Bm25Index::new()));
        let cancel = Arc::new(AtomicBool::new(false));

        let git_sink = Arc::clone(sink);
        let git = GitHandle::start(
            GitSettings {
                vault_root: vault.root().to_path_buf(),
                home: home.map(Path::to_path_buf),
                baseline: config.baseline,
                last_visit_oid: last_visit.as_ref().and_then(|v| v.oid.clone()),
                timings: git::GitTimings::default(),
            },
            discovery,
            Arc::new(move |snap: &GitSnapshot| git_sink.git_status(snap)),
        );

        let shared = Shared {
            vault: Arc::clone(&vault),
            filter: Arc::clone(&filter),
            sigs,
            index: Arc::clone(&index),
            sink: Arc::clone(sink),
        };
        let watcher = start_watcher(&shared, git_ctx.as_ref(), git.trigger_fn())?;
        // The watcher is live now: ask git to look once more so nothing slipped through between
        // its first computation and the watcher start.
        git.trigger();
        let index_thread = start_indexer(&shared, Arc::clone(&cancel));

        // A failure to remember the folder must not stop it from opening.
        let _ = RecentStore::new(app_data_dir).touch(
            &vault.root().display().to_string(),
            &vault.name(),
            now_ms(),
        );

        Ok(Self {
            vault,
            registry: Registry::with_defaults(),
            store: Mutex::new(store),
            config,
            git_ctx,
            git,
            watcher: Mutex::new(Some(watcher)),
            index,
            filter,
            cancel,
            index_thread: Mutex::new(index_thread),
            last_visit,
            closed: AtomicBool::new(false),
        })
    }

    #[cfg(test)]
    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    /// Every root folder (main first, then worktrees); the asset protocol is allowed for each.
    pub fn root_paths(&self) -> Vec<PathBuf> {
        self.vault
            .root_paths()
            .into_iter()
            .map(Path::to_path_buf)
            .collect()
    }

    pub fn info(&self) -> VaultInfo {
        VaultInfo {
            root: self.vault.root().display().to_string(),
            name: self.vault.name(),
            mode: self.vault.mode(),
            initial_file: self.vault.initial_file().map(str::to_owned),
            worktrees: self
                .vault
                .worktrees()
                .iter()
                .map(|w| WorktreeInfo {
                    name: w.name.clone(),
                    path: w.path.display().to_string(),
                })
                .collect(),
        }
    }

    pub fn tree(&self) -> TreeNode {
        self.vault.tree()
    }

    pub fn read_document(&self, path: &str) -> Result<DocumentView, AppError> {
        let resolved = self.vault.resolve(path)?;
        Ok(render::load_document(
            &self.registry,
            &resolved.abs,
            &resolved.rel,
        )?)
    }

    pub fn resolve_link(&self, from: &str, href: &str) -> LinkTarget {
        links::resolve(&self.vault, from, href)
    }

    pub fn absolute_path(&self, path: &str) -> Result<String, AppError> {
        Ok(self.vault.resolve(path)?.abs.display().to_string())
    }

    pub fn git_snapshot(&self) -> GitSnapshot {
        self.git.snapshot()
    }

    pub fn set_git_baseline(&self, kind: BaselineKind) -> GitSnapshot {
        self.git.set_baseline(kind)
    }

    /// Asks the git worker to look again (window focus, manual refresh).
    pub fn refresh_git(&self) {
        self.git.trigger();
    }

    pub fn search(&self, query: &str, limit: Option<usize>) -> Vec<SearchHit> {
        let limit = limit.unwrap_or(DEFAULT_SEARCH_LIMIT).min(MAX_SEARCH_LIMIT);
        self.index.search(query, limit)
    }

    pub fn save_session(&self, session: &SessionState) -> Result<(), AppError> {
        self.lock_store()?.save_session(session)?;
        Ok(())
    }

    /// The saved session. A `ref_template` set in `.trailmark/config.toml` overrides the saved one,
    /// so a shared project setting wins over what a teammate's UI remembered.
    pub fn load_session(&self) -> Result<Option<SessionState>, AppError> {
        let mut session = self.lock_store()?.load_session();
        if let (Some(s), Some(template)) = (session.as_mut(), &self.config.ref_template) {
            s.ref_template.clone_from(template);
        }
        Ok(session)
    }

    pub fn init_project_folder(&self) -> Result<(), AppError> {
        self.lock_store()?.init_project_folder()?;
        Ok(())
    }

    pub fn state_location(&self) -> Result<StateLocation, AppError> {
        Ok(self.lock_store()?.location())
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, StateStore>, AppError> {
        self.store
            .lock()
            .map_err(|_| AppError::Internal("state store is unavailable".to_owned()))
    }

    /// Files changed since the project was last closed. Empty (with `since_ms: None`) on a first visit.
    pub fn since_last_visit(&self) -> Result<ChangedSince, AppError> {
        let Some(visit) = &self.last_visit else {
            return Ok(ChangedSince {
                since_ms: None,
                files: Vec::new(),
            });
        };
        let mut files = match self.git.since_last_visit()? {
            Some(list) => list
                .into_iter()
                .map(|(path, state)| {
                    let modified_ms = self
                        .vault
                        .resolve(&path)
                        .ok()
                        .and_then(|r| std::fs::metadata(r.abs).ok())
                        .map_or(visit.time_ms, |m| modified_ms(&m));
                    ChangedFile {
                        path,
                        state,
                        modified_ms,
                    }
                })
                .collect(),
            None => self.modified_since(visit.time_ms),
        };
        files.sort_by(|a, b| {
            b.modified_ms
                .cmp(&a.modified_ms)
                .then_with(|| a.path.cmp(&b.path))
        });
        files.truncate(MAX_SINCE_FILES);
        Ok(ChangedSince {
            since_ms: Some(visit.time_ms),
            files,
        })
    }

    /// Folders without git: files whose modification time is later than the last visit.
    fn modified_since(&self, since_ms: u64) -> Vec<ChangedFile> {
        self.vault
            .walk()
            .into_iter()
            .filter(|item| item.kind == NodeKind::File)
            .filter(|item| {
                self.vault
                    .locate(&item.abs)
                    .is_some_and(|l| self.filter.classify(l.root, &l.inner, false) == Verdict::Emit)
            })
            .filter_map(|item| {
                let modified = std::fs::metadata(&item.abs).ok().map(|m| modified_ms(&m))?;
                (modified > since_ms).then_some(ChangedFile {
                    path: item.rel,
                    state: GitState::Modified,
                    modified_ms: modified,
                })
            })
            .collect()
    }

    /// Remembers "now" and the current HEAD as the last visit (called on close and on exit).
    pub fn save_last_visit(&self) {
        let oid = self.git_ctx.as_ref().and_then(git::read_head_oid);
        let visit = LastVisit {
            time_ms: now_ms(),
            oid,
        };
        if let Ok(store) = self.store.lock() {
            let _ = store.save_last_visit(&visit);
        }
    }

    /// Saves the last visit and stops every background task. Safe to call more than once.
    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.save_last_visit();
        self.cancel.store(true, Ordering::Relaxed);
        if let Ok(mut w) = self.watcher.lock() {
            if let Some(w) = w.take() {
                w.stop();
            }
        }
        self.git.shutdown();
        if let Ok(mut t) = self.index_thread.lock() {
            if let Some(t) = t.take() {
                let _ = t.join();
            }
        }
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        // The watcher and the git worker stop with their handles; the index loop needs a nudge.
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::{commit_all, init_repo, write};
    use crate::watch::ChangeKind;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct Recorder {
        fs: Mutex<Vec<FsBatch>>,
        tree: AtomicUsize,
        git: Mutex<Vec<GitSnapshot>>,
        progress: Mutex<Vec<(usize, usize)>>,
    }

    impl EventSink for Recorder {
        fn fs_change(&self, batch: &FsBatch) {
            self.fs.lock().expect("lock").push(batch.clone());
        }
        fn tree_changed(&self) {
            self.tree.fetch_add(1, Ordering::SeqCst);
        }
        fn git_status(&self, snapshot: &GitSnapshot) {
            self.git.lock().expect("lock").push(snapshot.clone());
        }
        fn index_progress(&self, indexed: usize, total: usize) {
            self.progress.lock().expect("lock").push((indexed, total));
        }
    }

    fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if cond() {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("timed out waiting for: {what}");
    }

    struct Fx {
        project: tempfile::TempDir,
        app: tempfile::TempDir,
        rec: Arc<Recorder>,
    }

    impl Fx {
        fn new() -> Self {
            Self {
                project: tempfile::tempdir().expect("tempdir"),
                app: tempfile::tempdir().expect("tempdir"),
                rec: Arc::new(Recorder::default()),
            }
        }

        fn open(&self) -> Workspace {
            self.open_path(self.project.path())
        }

        fn open_path(&self, path: &Path) -> Workspace {
            let sink: Arc<dyn EventSink> = self.rec.clone();
            Workspace::open(path, self.app.path(), None, &sink).expect("open")
        }

        fn indexed(&self) {
            wait_until("indexing to finish", || {
                self.rec
                    .progress
                    .lock()
                    .expect("lock")
                    .last()
                    .is_some_and(|(done, total)| done == total)
            });
        }
    }

    #[test]
    fn guarded_sink_drops_events_once_another_workspace_is_current() {
        let rec = Arc::new(Recorder::default());
        let current = Arc::new(AtomicU64::new(1));
        let guarded = GuardedSink::new(rec.clone(), 1, Arc::clone(&current));
        guarded.tree_changed();
        guarded.index_progress(1, 2);
        assert_eq!(rec.tree.load(Ordering::SeqCst), 1);
        assert_eq!(rec.progress.lock().expect("lock").len(), 1);

        current.store(2, Ordering::SeqCst);
        guarded.tree_changed();
        guarded.index_progress(2, 2);
        guarded.fs_change(&FsBatch {
            changes: vec![],
            bulk: None,
            rescan: true,
        });
        guarded.git_status(&GitSnapshot::without_repo(BaselineKind::Head, None));
        assert_eq!(rec.tree.load(Ordering::SeqCst), 1);
        assert_eq!(rec.progress.lock().expect("lock").len(), 1);
        assert!(rec.fs.lock().expect("lock").is_empty());
        assert!(rec.git.lock().expect("lock").is_empty());
    }

    #[test]
    fn opens_a_folder_and_serves_the_basic_commands() {
        let fx = Fx::new();
        write(
            fx.project.path(),
            "README.md",
            "# Home\n\nSee [guide](docs/guide.md#setup).\n",
        );
        write(
            fx.project.path(),
            "docs/guide.md",
            "# Guide\n\n## Setup\n\nInstall the thing.\n",
        );
        let ws = fx.open();

        let info = ws.info();
        assert_eq!(info.mode, VaultMode::Ide);
        assert_eq!(info.initial_file, None);
        assert!(info.worktrees.is_empty());
        assert_eq!(info.name, ws.vault().name());

        let tree = ws.tree();
        assert_eq!(tree.kind, NodeKind::Dir);
        assert_eq!(tree.children.as_ref().map(Vec::len), Some(2));

        let doc = ws.read_document("README.md").expect("doc");
        assert_eq!(doc.title.as_deref(), Some("Home"));
        assert!(doc.html.contains("href=\"docs/guide.md#setup\""));

        assert_eq!(
            ws.resolve_link("README.md", "docs/guide.md#setup"),
            LinkTarget::Internal {
                path: "docs/guide.md".to_owned(),
                anchor: Some("setup".to_owned())
            }
        );
        assert!(ws
            .absolute_path("docs/guide.md")
            .expect("abs")
            .ends_with("guide.md"));
        assert_eq!(ws.state_location().expect("loc"), StateLocation::AppData);

        fx.indexed();
        let hits = ws.search("install", None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "docs/guide.md");
        assert_eq!(hits[0].heading_path, ["Guide", "Setup"]);

        let snap = ws.git_snapshot();
        assert!(!snap.is_repo);
        ws.close();
    }

    #[test]
    fn paths_from_the_ui_are_validated() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "x");
        let ws = fx.open();
        for bad in [
            "../outside.md",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "a/../../b",
        ] {
            let err = ws.read_document(bad).expect_err(bad);
            assert!(
                matches!(err.kind(), "pathEscape" | "invalidPath"),
                "{bad}: {}",
                err.kind()
            );
            assert!(ws.absolute_path(bad).is_err(), "{bad}");
        }
        assert_eq!(
            ws.read_document("missing.md").expect_err("missing").kind(),
            "notFound"
        );
        ws.close();
    }

    #[test]
    fn file_argument_opens_simple_mode() {
        let fx = Fx::new();
        write(fx.project.path(), "notes/todo.md", "# Todo\n");
        let ws = fx.open_path(&fx.project.path().join("notes").join("todo.md"));
        let info = ws.info();
        assert_eq!(info.mode, VaultMode::Simple);
        assert_eq!(info.initial_file.as_deref(), Some("todo.md"));
        assert!(ws.read_document("todo.md").is_ok());
        ws.close();
    }

    #[test]
    fn simple_mode_does_not_index_or_watch_the_whole_folder() {
        let fx = Fx::new();
        write(
            fx.project.path(),
            "docs/todo.md",
            "# Todo

siblingword
",
        );
        write(
            fx.project.path(),
            "docs/sub/deep.md",
            "# Deep

deepword
",
        );
        let ws = fx.open_path(&fx.project.path().join("docs").join("todo.md"));
        fx.indexed();
        assert_eq!(ws.search("siblingword", None).len(), 1);
        assert!(ws.search("deepword", None).is_empty());

        // Nested changes are not watched; changes next to the file are.
        write(fx.project.path(), "docs/sub/new.md", "x");
        write(fx.project.path(), "docs/next.md", "x");
        wait_until("sibling change", || {
            fx.rec
                .fs
                .lock()
                .expect("lock")
                .iter()
                .flat_map(|b| &b.changes)
                .any(|c| c.path == "next.md")
        });
        let nested = fx
            .rec
            .fs
            .lock()
            .expect("lock")
            .iter()
            .flat_map(|b| &b.changes)
            .any(|c| c.path.starts_with("sub/"));
        assert!(!nested);
        ws.close();
    }

    #[test]
    fn live_changes_reach_the_sink_the_index_and_git() {
        let fx = Fx::new();
        let repo = init_repo(fx.project.path());
        write(fx.project.path(), "a.md", "# A\n\nfirst\n");
        commit_all(&repo, "init");
        let ws = fx.open();
        fx.indexed();
        assert!(ws.git_snapshot().is_repo);

        write(fx.project.path(), "new.md", "# New\n\nbrandnewword\n");
        wait_until("fs-change for new.md", || {
            fx.rec
                .fs
                .lock()
                .expect("lock")
                .iter()
                .flat_map(|b| &b.changes)
                .any(|c| c.path == "new.md" && c.kind == ChangeKind::Created)
        });
        assert!(
            fx.rec.tree.load(Ordering::SeqCst) >= 1,
            "tree-changed on create"
        );
        wait_until("search sees the new document", || {
            !ws.search("brandnewword", None).is_empty()
        });
        wait_until("git sees the untracked file", || {
            fx.rec
                .git
                .lock()
                .expect("lock")
                .iter()
                .any(|s| s.files.get("new.md").map(|f| f.state) == Some(GitState::Untracked))
        });
        let latest = fx
            .rec
            .git
            .lock()
            .expect("lock")
            .last()
            .cloned()
            .expect("snap");
        assert!(latest.generation > 1);

        // Editing updates the index in place.
        write(fx.project.path(), "new.md", "# New\n\nreplacedword\n");
        wait_until("index update", || {
            ws.search("brandnewword", None).is_empty()
                && !ws.search("replacedword", None).is_empty()
        });
        ws.close();
    }

    #[test]
    fn since_last_visit_uses_the_commit_stored_at_the_last_close() {
        let fx = Fx::new();
        let repo = init_repo(fx.project.path());
        write(fx.project.path(), "a.md", "one\n");
        commit_all(&repo, "init");

        // First visit: nothing to compare with.
        let ws = fx.open();
        let first = ws.since_last_visit().expect("since");
        assert_eq!(
            first,
            ChangedSince {
                since_ms: None,
                files: vec![]
            }
        );
        ws.close();

        // The agent works while the app is closed, committing along the way.
        write(fx.project.path(), "a.md", "two\n");
        write(fx.project.path(), "b.md", "new\n");
        commit_all(&repo, "agent");
        write(fx.project.path(), "c.md", "uncommitted\n");

        let ws = fx.open();
        let since = ws.since_last_visit().expect("since");
        assert!(since.since_ms.is_some());
        let got: Vec<(&str, GitState)> = since
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.state))
            .collect();
        let mut got = got;
        got.sort_by_key(|(p, _)| *p);
        assert_eq!(
            got,
            [
                ("a.md", GitState::Modified),
                ("b.md", GitState::Added),
                ("c.md", GitState::Untracked),
            ]
        );
        assert!(since.files.iter().all(|f| f.modified_ms > 0));

        // The default baseline is the last visit; HEAD can be selected instead.
        let snap = ws.git_snapshot();
        assert_eq!(snap.baseline.kind, BaselineKind::LastVisit);
        assert_eq!(snap.files.len(), 3);
        let head = ws.set_git_baseline(BaselineKind::Head);
        assert_eq!(head.files.keys().collect::<Vec<_>>(), ["c.md"]);
        ws.close();
    }

    #[test]
    fn since_last_visit_without_git_uses_modification_times() {
        let fx = Fx::new();
        write(fx.project.path(), "old.md", "old\n");
        let ws = fx.open();
        ws.close();
        thread::sleep(Duration::from_millis(30));
        write(fx.project.path(), "later.md", "new\n");
        write(fx.project.path(), "node_modules/dep/x.md", "ignored\n");

        let ws = fx.open();
        let since = ws.since_last_visit().expect("since");
        let paths: Vec<&str> = since.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["later.md"]);
        assert_eq!(since.files[0].state, GitState::Modified);
        ws.close();
    }

    #[test]
    fn linked_worktrees_become_extra_roots() {
        let fx = Fx::new();
        let repo = init_repo(fx.project.path());
        write(fx.project.path(), "a.md", "# Main\n");
        commit_all(&repo, "init");
        let wt_parent = tempfile::tempdir().expect("tempdir");
        let wt_path = wt_parent.path().join("feature-x");
        repo.worktree("feature-x", &wt_path, None)
            .expect("worktree");
        write(&wt_path, "plan.md", "# Plan in worktree\n\nwtuniqueword\n");

        let ws = fx.open();
        let info = ws.info();
        assert_eq!(info.worktrees.len(), 1);
        assert_eq!(info.worktrees[0].name, "feature-x");
        assert_eq!(ws.root_paths().len(), 2);
        let tree = ws.tree();
        let wt = tree
            .children
            .as_ref()
            .and_then(|c| c.iter().find(|n| n.kind == NodeKind::Worktree))
            .expect("worktree node");
        assert_eq!(wt.path, "@wt/feature-x");
        let doc = ws.read_document("@wt/feature-x/plan.md").expect("doc");
        assert_eq!(doc.title.as_deref(), Some("Plan in worktree"));
        fx.indexed();
        assert_eq!(
            ws.search("wtuniqueword", None)[0].path,
            "@wt/feature-x/plan.md"
        );

        // Changes inside the worktree are reported with the prefixed path.
        write(&wt_path, "plan.md", "# Plan in worktree\n\nedited\n");
        wait_until("worktree change", || {
            fx.rec
                .fs
                .lock()
                .expect("lock")
                .iter()
                .flat_map(|b| &b.changes)
                .any(|c| c.path == "@wt/feature-x/plan.md")
        });
        ws.close();
    }

    #[test]
    fn session_round_trip_and_project_folder_initialization() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "x");
        let ws = fx.open();
        assert_eq!(ws.load_session().expect("load"), None);
        let session = SessionState {
            last_doc: Some("a.md".to_owned()),
            scroll_top: 99.0,
            ..SessionState::default()
        };
        ws.save_session(&session).expect("save");
        assert_eq!(ws.load_session().expect("load"), Some(session.clone()));

        ws.init_project_folder().expect("init");
        assert_eq!(ws.state_location().expect("loc"), StateLocation::Project);
        assert!(fx.project.path().join(".trailmark/config.toml").is_file());
        assert!(fx.project.path().join(".trailmark/.gitignore").is_file());
        assert_eq!(ws.load_session().expect("load"), Some(session));

        // Our own writes under .trailmark/local do not show up as changes.
        ws.save_session(&SessionState::default()).expect("save");
        thread::sleep(Duration::from_millis(600));
        let leaked = fx
            .rec
            .fs
            .lock()
            .expect("lock")
            .iter()
            .flat_map(|b| &b.changes)
            .any(|c| c.path.contains("local") || c.path.contains("cache"));
        assert!(!leaked);
        ws.close();
    }

    #[test]
    fn project_config_ref_template_overrides_the_saved_one() {
        let fx = Fx::new();
        write(
            fx.project.path(),
            ".trailmark/config.toml",
            "ref_template = \"see {path}\"\n",
        );
        let ws = fx.open();
        assert_eq!(ws.state_location().expect("loc"), StateLocation::Project);
        ws.save_session(&SessionState::default()).expect("save");
        assert_eq!(
            ws.load_session().expect("load").map(|s| s.ref_template),
            Some("see {path}".to_owned())
        );
        ws.close();
    }

    #[test]
    fn recents_are_updated_on_open() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "x");
        let ws = fx.open();
        let list = RecentStore::new(fx.app.path()).load();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].root, ws.info().root);
        ws.close();
    }

    #[test]
    fn close_stops_everything_and_reopening_works() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n\nword\n");
        for _ in 0..2 {
            let ws = fx.open();
            fx.indexed();
            ws.close();
        }
        // Nothing is left running that would keep emitting after close.
        let before = fx.rec.fs.lock().expect("lock").len();
        write(fx.project.path(), "after.md", "x");
        thread::sleep(Duration::from_millis(500));
        assert_eq!(fx.rec.fs.lock().expect("lock").len(), before);
    }
}
