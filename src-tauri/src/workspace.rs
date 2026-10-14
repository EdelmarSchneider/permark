//! An open folder, with everything running for it: watcher, git worker, search index, state.
//!
//! This is the Tauri-free core behind the commands: [`Workspace::open`] wires the modules together and
//! reports through an [`EventSink`], which the app implements with Tauri events and tests implement
//! with a recorder. [`WorkspaceHost`] holds the folder that is open and makes switching folders safe.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;

use crate::edit::{AssetWritten, CreatedDoc, Editor, SaveOutcome, SourceDoc};
use crate::error::AppError;
use crate::fileops::{
    BufferText, CreatedFolder, DeleteOutcome, FileOpError, FileOpsService, Followers, Move,
    MoveOutcome, MovePreview, OsRecycler, UndoOutcome,
};
use crate::git::{
    self, BaselineKind, Discovery, GitContext, GitHandle, GitSettings, GitSnapshot, GitState,
};
use crate::links::{self, LinkTarget};
use crate::meta::catalog::{IndexEntry, TaggedFile};
use crate::meta::service::{FileMeta, TagOverview};
use crate::meta::{MetaService, Timing};
use crate::render::{self, DocumentView, Registry};
use crate::search::indexer::{IndexWorker, Indexer};
use crate::search::{Bm25Index, SearchHit, SearchProvider};
use crate::session::{
    FileStamp, LastVisit, RecentStore, SessionState, StateLocation, StateStore, TreeConfig,
};
use crate::util::{modified_ms, now_ms, on_parser_stack, read_with_retry};
use crate::vault::{NodeKind, TreeNode, TreeOptions, Vault, VaultError, VaultMode};
use crate::watch::service::WatchOutput;
use crate::watch::sigs::hash_file;
use crate::watch::{
    FsBatch, GitProbe, OwnWrites, PathFilter, SigStore, Verdict, WatchConfig, WatchService,
};

/// "Since your last visit" lists at most this many files.
const MAX_SINCE_FILES: usize = 500;
const DEFAULT_SEARCH_LIMIT: usize = 30;
const MAX_SEARCH_LIMIT: usize = 200;
/// At most this many uncommitted files are remembered (with their content) when a project is closed.
const MAX_DIRTY_TRACKED: usize = 500;
/// Images larger than this are not served to the document view.
const MAX_ASSET_BYTES: u64 = 32 * 1024 * 1024;

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
    /// Tags or descriptions changed (`meta-changed`).
    fn meta_changed(&self);
}

/// Forwards events only while its workspace is the one that may emit, so a folder that is being
/// replaced, or one that is not installed yet, cannot leak events into the UI that shows another one.
pub struct GuardedSink {
    inner: Arc<dyn EventSink>,
    id: u64,
    emitting: Arc<AtomicU64>,
}

impl GuardedSink {
    /// `id` identifies one workspace; events pass while `emitting` holds it.
    pub fn new(inner: Arc<dyn EventSink>, id: u64, emitting: Arc<AtomicU64>) -> Self {
        Self {
            inner,
            id,
            emitting,
        }
    }

    fn live(&self) -> bool {
        self.emitting.load(Ordering::SeqCst) == self.id
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

    fn meta_changed(&self) {
        if self.live() {
            self.inner.meta_changed();
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
    /// Whether changes in the folder are followed live. False when the file system cannot report
    /// them (some network and virtual drives): everything else keeps working, refreshed on focus.
    pub live_updates: bool,
    /// The project's default for the Explorer filter (`[tree]` in `.trailmark/config.toml`), if it sets one.
    pub tree_default: Option<TreeConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: String,
    pub branch: Option<String>,
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

/// An image of the open folder, ready to be served to the document view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

/// The MIME type of an image file, from its extension.
fn image_mime(rel: &str) -> Option<&'static str> {
    let ext = Path::new(rel)
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

/// Extensions of the files that may be handed to the system's default application. Files that can
/// run code when opened (programs, scripts, shortcuts, installers) are not documents and are left out;
/// so are source files, because some systems run those on open too.
const EXTERNAL_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mdown", "mkd", "mdx", "txt", "text", "log", "rst", "adoc", "org", "tex",
    "bib", "json", "jsonc", "yaml", "yml", "toml", "xml", "xsd", "csv", "tsv", "ini", "cfg",
    "conf", "html", "htm", "css", "svg", "png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "ico",
    "pdf",
];

pub struct Workspace {
    vault: Arc<Vault>,
    registry: Registry,
    store: Mutex<StateStore>,
    config: crate::session::Config,
    git_ctx: Option<GitContext>,
    git: GitHandle,
    watcher: Mutex<Option<WatchService>>,
    /// Whether the watcher runs (see [`VaultInfo::live_updates`]).
    live_updates: bool,
    index: Arc<RwLock<Bm25Index>>,
    indexer: Arc<IndexWorker>,
    filter: Arc<PathFilter>,
    meta: Arc<MetaService>,
    /// Content the editor is writing, so the watcher does not report it back as an external change.
    own_writes: Arc<OwnWrites>,
    /// Rename, move, delete and new folder in the Explorer.
    fileops: FileOpsService,
    sink: Arc<dyn EventSink>,
    last_visit: Option<LastVisit>,
    closed: AtomicBool,
}

/// Tags follow the files that are renamed or moved in the Explorer.
struct MetaFollower(Arc<MetaService>);

impl Followers for MetaFollower {
    fn renamed(&self, from: &str, to: &str, is_dir: bool) {
        if is_dir {
            self.0.rename_tree(from, to);
        } else {
            self.0.rename(from, to);
        }
    }
}

/// What the watcher thread needs to do its work.
struct WatchWiring {
    vault: Arc<Vault>,
    filter: Arc<PathFilter>,
    sigs: Arc<SigStore>,
    sink: Arc<dyn EventSink>,
    indexer: Arc<IndexWorker>,
    meta: Arc<MetaService>,
    own_writes: Arc<OwnWrites>,
}

type StartWatcher = fn(
    WatchWiring,
    Option<&GitContext>,
    Arc<dyn Fn() + Send + Sync>,
) -> Result<WatchService, AppError>;

/// Watches the roots; every flush goes to the index worker, tells the UI, and wakes the git worker.
fn start_watcher(
    wiring: WatchWiring,
    git_ctx: Option<&GitContext>,
    trigger_git: Arc<dyn Fn() + Send + Sync>,
) -> Result<WatchService, AppError> {
    let probe = GitProbe::new(
        git_ctx.map(|c| c.git_dir.clone()),
        git_ctx.map(|c| c.common_dir.clone()),
    );
    let WatchWiring {
        vault,
        filter,
        sigs,
        sink,
        indexer,
        meta,
        own_writes,
    } = wiring;
    let known = Arc::clone(&sigs);
    Ok(WatchService::start(
        vault,
        filter,
        sigs,
        probe,
        WatchConfig::default(),
        move |mut out: WatchOutput| {
            // Tags follow renamed files; file tags may have changed; an edit of `meta.toml` is read in,
            // and the app's own writes to it are not news.
            if meta.on_fs_changes(&out.all_changes) {
                MetaService::without_meta_file(&mut out.all_changes);
                MetaService::without_meta_file(&mut out.batch.changes);
            }
            // Lost events or a folder that appeared: what the files hold has to be looked at again.
            if out.batch.rescan || out.tree_changed {
                meta.invalidate();
            }
            // What the editor saved or created is no news to the person who did it. The index and git
            // below still learn about it (`all_changes`), the UI does not hear about it.
            if out.batch.bulk.is_some() {
                // A bulk batch lists at most 200 of its changes, so what the app announced is matched against
                // all of them: a big folder moved in the Explorer is not a "Large batch" row.
                let mut everything = out.all_changes.clone();
                own_writes.strip(&mut everything, &known);
                if everything.is_empty() && !out.all_changes.is_empty() {
                    out.batch.bulk = None;
                    out.batch.changes.clear();
                }
            } else {
                own_writes.strip(&mut out.batch.changes, &known);
            }
            let has_batch = out.has_batch();
            let WatchOutput {
                batch,
                all_changes,
                tree_changed,
                ..
            } = out;
            // Lost events or a partly listed folder: the index reconciles with the disk instead.
            if batch.rescan {
                indexer.rescan();
            } else if !all_changes.is_empty() {
                indexer.apply(all_changes);
            }
            if has_batch {
                sink.fs_change(&batch);
            }
            if tree_changed {
                sink.tree_changed();
            }
            trigger_git();
        },
    )?)
}

impl Workspace {
    /// Opens a folder (IDE mode) or a file (Simple mode) and starts everything for it.
    ///
    /// A folder whose changes cannot be watched still opens: [`VaultInfo::live_updates`] is false then.
    pub fn open(
        path: &Path,
        app_data_dir: &Path,
        home: Option<&Path>,
        sink: &Arc<dyn EventSink>,
    ) -> Result<Self, AppError> {
        Self::open_with(path, app_data_dir, home, sink, start_watcher)
    }

    /// [`Workspace::open`] with the way the watcher is started as a parameter (tests make it fail).
    fn open_with(
        path: &Path,
        app_data_dir: &Path,
        home: Option<&Path>,
        sink: &Arc<dyn EventSink>,
        watch: StartWatcher,
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

        let meta_sink = Arc::clone(sink);
        let meta = Arc::new(MetaService::open(
            Arc::clone(&vault),
            store.meta_path(),
            store.location() == StateLocation::Project,
            store.legacy_meta_path(),
            Timing::default(),
            Arc::new(move || meta_sink.meta_changed()),
        ));

        let own_writes = Arc::new(OwnWrites::new());
        let fileops = FileOpsService::new(
            Arc::clone(&vault),
            Arc::clone(&own_writes),
            Arc::new(MetaFollower(Arc::clone(&meta))),
            Arc::new(OsRecycler),
        );

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

        // The index worker is the only writer of the index; it builds first, then follows the watcher.
        let progress_sink = Arc::clone(sink);
        let indexer = Arc::new(IndexWorker::start(
            Indexer::new(
                Arc::clone(&vault),
                Arc::clone(&filter),
                Arc::clone(&index),
                Arc::clone(&sigs),
            ),
            move |done, total| progress_sink.index_progress(done, total),
        ));

        // No watcher is not fatal: the folder is perfectly readable, it just is not live.
        let watcher = watch(
            WatchWiring {
                vault: Arc::clone(&vault),
                filter: Arc::clone(&filter),
                sigs,
                sink: Arc::clone(sink),
                indexer: Arc::clone(&indexer),
                meta: Arc::clone(&meta),
                own_writes: Arc::clone(&own_writes),
            },
            git_ctx.as_ref(),
            git.trigger_fn(),
        )
        .ok();
        // The watcher is live now: ask git to look once more so nothing slipped through between
        // its first computation and the watcher start.
        git.trigger();

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
            live_updates: watcher.is_some(),
            watcher: Mutex::new(watcher),
            index,
            indexer,
            filter,
            meta,
            own_writes,
            fileops,
            sink: Arc::clone(sink),
            last_visit,
            closed: AtomicBool::new(false),
        })
    }

    #[cfg(test)]
    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    /// Every root folder (main first, then worktrees).
    #[cfg(test)]
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
                    branch: w.branch.clone(),
                })
                .collect(),
            live_updates: self.live_updates,
            tree_default: self.config.tree.clone(),
        }
    }

    pub fn tree(&self, opts: TreeOptions) -> TreeNode {
        self.vault.tree_with(opts)
    }

    pub fn read_document(&self, path: &str) -> Result<DocumentView, AppError> {
        let resolved = self.vault.resolve(path)?;
        // Parsing untrusted documents happens on a thread with a large stack (see `on_parser_stack`).
        Ok(on_parser_stack(|| {
            render::load_document(&self.registry, &resolved.abs, &resolved.rel)
        })?)
    }

    // ---- editing --------------------------------------------------------------------------------

    fn editor(&self) -> Editor<'_> {
        Editor::new(&self.vault, &self.own_writes)
    }

    /// A Markdown file as the editor needs it: the exact text, its hash and its blocks.
    pub fn document_source(&self, path: &str) -> Result<SourceDoc, AppError> {
        Ok(self.editor().source(path)?)
    }

    /// Saves the editor's text unless the file changed since `expected_hash`.
    pub fn save_document(
        &self,
        path: &str,
        text: &str,
        expected_hash: &str,
    ) -> Result<SaveOutcome, AppError> {
        Ok(self.editor().save(path, text, expected_hash)?)
    }

    /// Creates a new Markdown file (never overwrites).
    pub fn create_document(&self, path: &str, text: &str) -> Result<CreatedDoc, AppError> {
        Ok(self.editor().create(path, text)?)
    }

    /// Writes a pasted or dropped image into the `assets` folder next to a document.
    pub fn write_asset(
        &self,
        doc_path: &str,
        name_hint: &str,
        data: &[u8],
    ) -> Result<AssetWritten, AppError> {
        Ok(self.editor().write_asset(doc_path, name_hint, data)?)
    }

    // ---- file operations ------------------------------------------------------------------------

    /// The Explorer's operations need a folder, not a single opened file (whose folder may be Downloads).
    fn file_ops(&self) -> Result<&FileOpsService, AppError> {
        if self.vault.is_shallow() {
            return Err(FileOpError::SimpleMode.into());
        }
        Ok(&self.fileops)
    }

    /// Checks a rename or move and finds the links it would change (`scan_links`), leaving out the
    /// documents in `held_back`.
    pub fn preview_move(
        &self,
        moves: &[Move],
        scan_links: bool,
        held_back: &[String],
    ) -> Result<MovePreview, AppError> {
        Ok(self
            .file_ops()?
            .preview_move(moves, scan_links, held_back)?)
    }

    /// Makes the move of a preview, rewriting links when asked to.
    pub fn apply_move(
        &self,
        plan_id: u64,
        update_links: bool,
        buffers: &[BufferText],
    ) -> Result<MoveOutcome, AppError> {
        Ok(self
            .file_ops()?
            .apply_move(plan_id, update_links, buffers)?)
    }

    /// Sends files and folders to the recycle bin.
    pub fn delete_entries(&self, paths: &[String]) -> Result<DeleteOutcome, AppError> {
        Ok(self.file_ops()?.delete(paths)?)
    }

    pub fn new_folder(&self, path: &str) -> Result<CreatedFolder, AppError> {
        Ok(self.file_ops()?.new_folder(path)?)
    }

    pub fn undo_file_op(&self, op_id: u64) -> Result<UndoOutcome, AppError> {
        Ok(self.file_ops()?.undo(op_id)?)
    }

    pub fn resolve_link(&self, from: &str, href: &str) -> LinkTarget {
        links::resolve(&self.vault, from, href)
    }

    pub fn absolute_path(&self, path: &str) -> Result<String, AppError> {
        Ok(self.vault.resolve(path)?.abs.display().to_string())
    }

    /// An image for the document view. It is looked up like any other path of the folder, so nothing
    /// outside it can be read, and only a folder that is open now can be served. In Simple mode only
    /// the images next to the opened file are served, because that folder is often Downloads or the
    /// home folder.
    pub fn read_asset(&self, path: &str) -> Result<Asset, AppError> {
        let mime =
            image_mime(path).ok_or_else(|| VaultError::Invalid(format!("not an image: {path}")))?;
        let resolved = self.vault.resolve(path)?;
        if self.vault.is_shallow() && resolved.rel.contains('/') {
            return Err(VaultError::Escapes(path.to_owned()).into());
        }
        let meta =
            std::fs::metadata(&resolved.abs).map_err(|e| VaultError::io(&resolved.abs, e))?;
        if !meta.is_file() || meta.len() > MAX_ASSET_BYTES {
            return Err(VaultError::NotFound(path.to_owned()).into());
        }
        let bytes = read_with_retry(&resolved.abs).map_err(|e| VaultError::io(&resolved.abs, e))?;
        Ok(Asset { bytes, mime })
    }

    /// The file to hand to the system's default application for "Open externally".
    ///
    /// The path must stay inside the folder, name an existing file, and be a document or an image:
    /// a program or script in the folder is never started from here.
    pub fn external_target(&self, path: &str) -> Result<PathBuf, AppError> {
        let resolved = self.vault.resolve(path)?;
        if !resolved.abs.is_file() {
            return Err(VaultError::NotFound(path.to_owned()).into());
        }
        let allowed = resolved.abs.extension().is_none_or(|e| {
            let e = e.to_string_lossy().to_ascii_lowercase();
            EXTERNAL_EXTENSIONS.contains(&e.as_str())
        });
        if !allowed {
            return Err(VaultError::Invalid(format!(
                "{path} is not a document or an image, so it is not opened from here"
            ))
            .into());
        }
        Ok(resolved.abs)
    }

    /// The folder (or file) to show in the system's file manager for "Reveal in folder".
    pub fn reveal_target(&self, path: &str) -> Result<PathBuf, AppError> {
        let resolved = self.vault.resolve(path)?;
        if !resolved.abs.exists() {
            return Err(VaultError::NotFound(path.to_owned()).into());
        }
        Ok(resolved.abs)
    }

    pub fn git_snapshot(&self) -> GitSnapshot {
        self.git.snapshot()
    }

    pub fn set_git_baseline(&self, kind: BaselineKind) -> GitSnapshot {
        self.git.set_baseline(kind)
    }

    /// The window is back in the foreground: other tools may have changed things meanwhile.
    pub fn refresh(&self) {
        self.git.trigger();
        self.meta.invalidate();
        if !self.live_updates {
            // Nothing tells us about changes in this folder, so look at the disk now.
            self.indexer.rescan();
            self.sink.tree_changed();
        }
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
        let mut store = self.lock_store()?;
        let was_in_project = store.location() == StateLocation::Project;
        store.init_project_folder()?;
        if !was_in_project {
            // The tags and descriptions added so far move into `.trailmark/meta.toml`.
            self.meta.migrate_to(store.meta_path(), true)?;
        }
        Ok(())
    }

    // ---- tags and descriptions ------------------------------------------------------------------

    pub fn file_meta(&self, path: &str) -> Result<FileMeta, AppError> {
        Ok(self.meta.file_meta(path)?)
    }

    pub fn set_file_tags(&self, path: &str, tags: &[String]) -> Result<FileMeta, AppError> {
        Ok(self.meta.set_tags(path, tags)?)
    }

    pub fn set_file_description(
        &self,
        path: &str,
        description: &str,
    ) -> Result<FileMeta, AppError> {
        Ok(self.meta.set_description(path, description)?)
    }

    pub fn list_tags(&self) -> TagOverview {
        self.meta.overview()
    }

    pub fn files_with_tags(&self, tags: &[String], untagged: bool) -> Vec<TaggedFile> {
        self.meta.files_with_tags(tags, untagged)
    }

    pub fn meta_index(&self) -> Vec<IndexEntry> {
        self.meta.index()
    }

    /// Removes the tags and descriptions kept for files that no longer exist. Returns how many.
    pub fn clean_up_meta(&self) -> Result<usize, AppError> {
        Ok(self.meta.clean_up()?)
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
    ///
    /// With git these are the files that differ from the commit stored at the last close, except the
    /// ones that were already uncommitted then and still have the content they had.
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
                .filter(|(path, state)| !self.unchanged_since(visit, path, *state))
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

    /// Whether a file was already uncommitted when the project was last closed and still has the
    /// content it had then (even if it has been committed meanwhile): the user has seen it that way.
    fn unchanged_since(&self, visit: &LastVisit, path: &str, state: GitState) -> bool {
        state != GitState::Deleted
            && visit
                .dirty
                .get(path)
                .is_some_and(|then| self.file_stamp(path).as_ref() == Some(then))
    }

    /// Size and content hash of a file of the folder; `None` when it is missing or not a file.
    fn file_stamp(&self, rel: &str) -> Option<FileStamp> {
        let resolved = self.vault.resolve(rel).ok()?;
        let meta = std::fs::metadata(&resolved.abs).ok()?;
        if !meta.is_file() {
            return None;
        }
        let sig = hash_file(&resolved.abs, &meta).ok()?;
        Some(FileStamp {
            len: sig.len,
            hash: sig.hash,
        })
    }

    /// The uncommitted files right now, with their content stamps.
    fn dirty_stamps(&self) -> BTreeMap<String, FileStamp> {
        let Ok(Some(paths)) = self.git.dirty_files() else {
            return BTreeMap::new();
        };
        paths
            .into_iter()
            .take(MAX_DIRTY_TRACKED)
            .filter_map(|path| {
                let stamp = self.file_stamp(&path)?;
                Some((path, stamp))
            })
            .collect()
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

    /// Remembers "now", the current HEAD and the uncommitted files as the last visit (called on
    /// close and on exit).
    pub fn save_last_visit(&self) {
        let oid = self.git_ctx.as_ref().and_then(git::read_head_oid);
        let visit = LastVisit {
            time_ms: now_ms(),
            oid,
            dirty: self.dirty_stamps(),
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
        // The watcher goes first: what it reports last (a rename that moves tags) is still written.
        if let Ok(mut w) = self.watcher.lock() {
            if let Some(w) = w.take() {
                w.stop();
            }
        }
        self.meta.stop();
        self.indexer.stop();
        self.git.shutdown();
    }
}

/// Identifies one request to open a folder, in the order the requests arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenRequest(u64);

/// The folder that is open, and the rules for replacing it.
///
/// Folders are opened strictly one at a time, and when several requests pile up only the newest one
/// is carried out, so the folder the user asked for last is the one that ends up open. Only the
/// installed workspace may emit events: one that is still starting (or was already replaced) is muted.
#[derive(Default, Clone)]
pub struct WorkspaceHost {
    current: Arc<Mutex<Option<Arc<Workspace>>>>,
    /// The id of the workspace allowed to emit events; 0 while none is installed.
    emitting: Arc<AtomicU64>,
    next_id: Arc<AtomicU64>,
    /// Held for the whole of an open, so two folders are never started at once.
    gate: Arc<Mutex<()>>,
    /// The newest request taken with [`WorkspaceHost::request`].
    newest: Arc<AtomicU64>,
}

impl WorkspaceHost {
    /// The open workspace, or [`AppError::NoVault`].
    pub fn current(&self) -> Result<Arc<Workspace>, AppError> {
        self.current
            .lock()
            .map_err(|_| AppError::Internal("application state is unavailable".to_owned()))?
            .clone()
            .ok_or(AppError::NoVault)
    }

    /// Registers a request to open a folder. Call it as soon as the request arrives, then pass it
    /// to [`WorkspaceHost::open`].
    pub fn request(&self) -> OpenRequest {
        OpenRequest(self.newest.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Opens a folder with `opener`, makes it the current workspace and closes the previous one.
    ///
    /// `opener` receives the sink the new workspace must report to. If a newer request arrived while
    /// this one waited its turn, nothing is opened and [`AppError::Superseded`] is returned. When the
    /// open fails, the previous folder stays open and keeps emitting.
    pub fn open(
        &self,
        request: OpenRequest,
        sink: &Arc<dyn EventSink>,
        opener: impl FnOnce(&Arc<dyn EventSink>) -> Result<Workspace, AppError>,
    ) -> Result<VaultInfo, AppError> {
        let _turn = self
            .gate
            .lock()
            .map_err(|_| AppError::Internal("application state is unavailable".to_owned()))?;
        if self.newest.load(Ordering::SeqCst) != request.0 {
            return Err(AppError::Superseded);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let guarded: Arc<dyn EventSink> = Arc::new(GuardedSink::new(
            Arc::clone(sink),
            id,
            Arc::clone(&self.emitting),
        ));
        let workspace = Arc::new(opener(&guarded)?);
        let info = workspace.info();
        let previous = self.replace(Some(workspace));
        // From here on the new workspace is the one that may emit.
        self.emitting.store(id, Ordering::SeqCst);
        if let Some(previous) = previous {
            previous.close();
        }
        Ok(info)
    }

    /// Installs a workspace and returns the previous one.
    fn replace(&self, next: Option<Arc<Workspace>>) -> Option<Arc<Workspace>> {
        let mut slot = self.current.lock().ok()?;
        std::mem::replace(&mut *slot, next)
    }

    /// An image of the open folder (see [`Workspace::read_asset`]).
    pub fn read_asset(&self, path: &str) -> Result<Asset, AppError> {
        self.current()?.read_asset(path)
    }

    /// Remembers "now" as the last visit without closing anything (window close).
    pub fn save_last_visit(&self) {
        if let Ok(ws) = self.current() {
            ws.save_last_visit();
        }
    }

    /// Window focus: external tools may have changed things while we were in the background.
    pub fn refresh_on_focus(&self) {
        if let Ok(ws) = self.current() {
            ws.refresh();
        }
    }

    /// Application exit: save the last visit and stop everything.
    pub fn shutdown(&self) {
        if let Some(ws) = self.replace(None) {
            self.emitting.store(0, Ordering::SeqCst);
            ws.close();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::{commit_all, init_repo, write};
    use crate::watch::ChangeKind;
    use std::sync::atomic::AtomicUsize;
    use std::thread;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct Recorder {
        fs: Mutex<Vec<FsBatch>>,
        tree: AtomicUsize,
        meta: AtomicUsize,
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
        fn meta_changed(&self) {
            self.meta.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Writes `name` until the watcher reports it. A watcher can still be starting for a moment after a folder
    /// opens (inotify adds its watches asynchronously), and a write in that moment is never reported; the
    /// initial scan covers such files in the app, but a test waiting for the event would hang.
    fn write_until_reported(rec: &Recorder, dir: &Path, name: &str, what: &str) {
        let mut n = 0u32;
        wait_until(what, || {
            if fs_paths(rec).contains(&name.to_owned()) {
                return true;
            }
            if n.is_multiple_of(20) {
                write(dir, name, &format!("x{n}"));
            }
            n += 1;
            false
        });
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
        assert_eq!(info.worktrees.len(), 0);
        assert!(info.live_updates);
        assert_eq!(info.name, ws.vault().name());

        let tree = ws.tree(TreeOptions::default());
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
        assert_eq!(ws.search("deepword", None).len(), 0);

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
        assert_eq!(info.worktrees[0].branch.as_deref(), Some("feature-x"));
        assert_eq!(ws.root_paths().len(), 2);
        let tree = ws.tree(TreeOptions::default());
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
    fn repositories_the_project_ignores_are_part_of_the_workspace() {
        // A folder that git-ignores three sibling repositories, with a script next to the documents.
        let fx = Fx::new();
        let root = fx.project.path();
        let outer = init_repo(root);
        write(root, ".gitignore", "Design/\nEngineering/\n");
        write(root, "README.md", "# Home\n");
        write(root, "start-team.bat", "@echo off\n");
        for sub in ["Design", "Engineering"] {
            let inner = init_repo(&root.join(sub));
            write(
                root,
                &format!("{sub}/notes.md"),
                &format!("# {sub}\n\n{sub}uniqueword\n"),
            );
            write(root, &format!("{sub}/.gitignore"), "secret.md\n");
            write(root, &format!("{sub}/secret.md"), "# Secret\n");
            commit_all(&inner, "init");
        }
        commit_all(&outer, "init");

        let ws = fx.open();
        let tree = ws.tree(TreeOptions::default());
        let top: Vec<_> = tree
            .children
            .iter()
            .flatten()
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(
            top,
            [
                "Design",
                "Engineering",
                ".gitignore",
                "README.md",
                "start-team.bat"
            ]
        );
        let design = tree
            .children
            .iter()
            .flatten()
            .find(|n| n.name == "Design")
            .expect("Design");
        let inside: Vec<_> = design
            .children
            .iter()
            .flatten()
            .map(|n| n.path.as_str())
            .collect();
        assert_eq!(inside, ["Design/.gitignore", "Design/notes.md"]);

        // reading, searching (the repository's own ignore rules apply), tagging and git all work, without errors
        assert_eq!(
            ws.read_document("Design/notes.md")
                .expect("doc")
                .title
                .as_deref(),
            Some("Design")
        );
        fx.indexed();
        assert_eq!(
            ws.search("Designuniqueword", None)[0].path,
            "Design/notes.md"
        );
        assert_eq!(
            ws.search("Secret", None).len(),
            0,
            "the repository ignores secret.md"
        );
        ws.set_file_tags("Engineering/notes.md", &["spec".to_owned()])
            .expect("tags");
        let tagged = ws.files_with_tags(&["spec".to_owned()], false);
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].path, "Engineering/notes.md");
        let snapshot = ws.git_snapshot();
        assert_eq!(snapshot.git_error, None);
        assert!(
            snapshot.files.keys().all(|p| !p.starts_with("Design")),
            "{:?}",
            snapshot.files.keys().collect::<Vec<_>>()
        );

        // the watcher reports changes inside a repository the project ignores
        write_until_reported(&fx.rec, root, "Design/live.md", "a change in Design");
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
    fn tags_move_into_the_project_on_init_and_the_apps_own_writes_are_not_changes() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        let ws = fx.open();
        ws.set_file_tags("a.md", &["before".to_owned()])
            .expect("tags");
        ws.set_file_description("a.md", "first")
            .expect("description");
        let appdata_meta = StateStore::open(fx.app.path(), fx.project.path()).meta_path();
        wait_until("the first write", || appdata_meta.exists());

        ws.init_project_folder().expect("init");
        let meta_file = fx.project.path().join(".trailmark/meta.toml");
        let text = std::fs::read_to_string(&meta_file).expect("meta.toml");
        assert!(
            text.contains("\"before\"") && text.contains("first"),
            "{text}"
        );
        assert!(!appdata_meta.exists(), "moved, not copied");

        ws.set_file_tags("a.md", &["before".to_owned(), "after".to_owned()])
            .expect("tags");
        wait_until("the delayed write", || {
            std::fs::read_to_string(&meta_file).is_ok_and(|t| t.contains("\"after\""))
        });
        // A negative proof: wait longer than the watcher's quiet period, then look. On a slow machine
        // this can pass without the watcher having reported yet, but it cannot fail without a leak.
        thread::sleep(Duration::from_millis(700));
        let seen = fs_paths(&fx.rec);
        assert!(
            !seen.iter().any(|p| p.contains("meta.toml")),
            "own writes must not reach the feed: {seen:?}"
        );
        assert!(fx.rec.meta.load(Ordering::SeqCst) >= 2, "the UI is told");

        // An agent edits the file: it is read in, and it is news.
        let told = fx.rec.meta.load(Ordering::SeqCst);
        std::fs::write(
            &meta_file,
            "[files.\"a.md\"]\ntags = [\"by-agent\"]\ndescription = \"edited\"\n",
        )
        .expect("write");
        wait_until("the edit to be read in", || {
            ws.file_meta("a.md")
                .is_ok_and(|m| m.description == "edited" && m.tags.len() == 1)
        });
        assert!(fx.rec.meta.load(Ordering::SeqCst) > told);
        wait_until("the edit in the feed", || {
            fs_paths(&fx.rec).iter().any(|p| p.ends_with("meta.toml"))
        });
        ws.close();
    }

    #[test]
    fn a_project_that_gets_a_trailmark_folder_from_elsewhere_keeps_the_tags_added_before() {
        let fx = Fx::new();
        write(
            fx.project.path(),
            "a.md",
            "# A
",
        );
        let appdata_meta = StateStore::open(fx.app.path(), fx.project.path()).meta_path();
        let ws = fx.open();
        ws.set_file_tags("a.md", &["early".to_owned()])
            .expect("tags");
        ws.close();
        assert!(appdata_meta.exists());

        // a teammate commits .trailmark/, and it arrives with a pull
        std::fs::create_dir_all(fx.project.path().join(".trailmark")).expect("mkdir");
        std::fs::write(
            fx.project.path().join(".trailmark/meta.toml"),
            "[files.\"a.md\"]
tags = [\"team\"]
",
        )
        .expect("write");

        let ws = fx.open();
        let meta = ws.file_meta("a.md").expect("meta");
        let mut names: Vec<&str> = meta.tags.iter().map(|t| t.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["early", "team"]);
        let text = std::fs::read_to_string(fx.project.path().join(".trailmark/meta.toml"))
            .expect("meta.toml");
        assert!(
            text.contains("\"early\"") && text.contains("\"team\""),
            "{text}"
        );
        assert!(!appdata_meta.exists(), "the AppData copy is retired");
        ws.close();
    }

    #[test]
    fn a_renamed_file_keeps_its_tags() {
        let fx = Fx::new();
        write(fx.project.path(), "old.md", "# Same content\n");
        let ws = fx.open();
        ws.set_file_tags("old.md", &["keep".to_owned()])
            .expect("tags");
        std::fs::rename(
            fx.project.path().join("old.md"),
            fx.project.path().join("new.md"),
        )
        .expect("rename");
        wait_until("the tags to follow the rename", || {
            ws.file_meta("new.md")
                .is_ok_and(|m| m.tags.iter().any(|t| t.name == "keep"))
        });
        assert_eq!(
            ws.file_meta("old.md").expect("old").tags,
            Vec::<crate::meta::catalog::TagRef>::new()
        );
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
    fn project_config_tree_defaults_reach_the_ui_through_the_vault_info() {
        let fx = Fx::new();
        let ws = fx.open();
        assert_eq!(ws.info().tree_default, None);
        ws.close();

        write(
            fx.project.path(),
            ".trailmark/config.toml",
            "[tree]
show = \"documents\"
exclude = [\"archive/**\"]
",
        );
        let ws = fx.open();
        let tree = ws.info().tree_default.expect("tree default");
        assert_eq!(tree.show, Some(crate::session::TreeShow::Documents));
        assert_eq!(tree.exclude, ["archive/**"]);
        assert_eq!(tree.include, Vec::<String>::new());
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
    fn the_editors_own_writes_are_not_news_but_an_agents_are() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        write(fx.project.path(), "b.md", "# B\n");
        let ws = fx.open();
        fx.indexed();

        // The editor saves a.md: the index and git still learn about it, the UI does not.
        let source = ws.document_source("a.md").expect("source");
        let saved = ws
            .save_document("a.md", "# A\n\nzebra stripes\n", &source.hash)
            .expect("save");
        assert!(matches!(saved, SaveOutcome::Saved { .. }), "{saved:?}");
        wait_until("the index to learn the new text", || {
            ws.search("zebra", None).len() == 1
        });

        // It creates a new file: the tree is refreshed, but there is no feed entry for it.
        let trees = fx.rec.tree.load(Ordering::SeqCst);
        ws.create_document("c.md", "# C\n").expect("create");
        wait_until("the tree to refresh", || {
            fx.rec.tree.load(Ordering::SeqCst) > trees
        });

        // A negative proof: wait longer than the watcher's quiet period, then look.
        thread::sleep(Duration::from_millis(700));
        let seen = fs_paths(&fx.rec);
        assert!(
            !seen.iter().any(|p| p == "a.md" || p == "c.md"),
            "own writes must not reach the feed: {seen:?}"
        );

        // An agent saves b.md, and then a.md over our version: both are news.
        std::fs::write(fx.project.path().join("b.md"), "# B changed\n").expect("write");
        std::fs::write(fx.project.path().join("a.md"), "# A by an agent\n").expect("write");
        wait_until("the agent's saves in the feed", || {
            let seen = fs_paths(&fx.rec);
            seen.iter().any(|p| p == "b.md") && seen.iter().any(|p| p == "a.md")
        });
        ws.close();
    }

    #[test]
    fn moves_in_the_explorer_are_not_news_and_tags_links_and_the_index_follow() {
        let fx = Fx::new();
        write(
            fx.project.path(),
            "README.md",
            "# Home\n\nSee [guide](docs/guide.md).\n",
        );
        write(fx.project.path(), "docs/guide.md", "# Guide\n\nzebra\n");
        write(fx.project.path(), "docs/extra/api.md", "# API\n");
        let ws = fx.open();
        fx.indexed();
        let keep = vec!["keep".to_owned()];
        ws.set_file_tags("docs/guide.md", &keep).expect("tags");
        ws.set_file_tags("docs/extra/api.md", &keep).expect("tags");

        // a rename with the links rewritten, then the folder with everything in it
        let rename = [Move {
            from: "docs/guide.md".to_owned(),
            to: "docs/manual.md".to_owned(),
        }];
        let preview = ws.preview_move(&rename, true, &[]).expect("preview");
        assert_eq!((preview.link_count, preview.file_count), (1, 1));
        ws.apply_move(preview.plan_id, true, &[]).expect("apply");
        let folder = [Move {
            from: "docs".to_owned(),
            to: "manual".to_owned(),
        }];
        let preview = ws.preview_move(&folder, true, &[]).expect("preview");
        ws.apply_move(preview.plan_id, true, &[]).expect("apply");

        let on_disk = |rel: &str| std::fs::read_to_string(fx.project.path().join(rel)).expect(rel);
        assert!(on_disk("README.md").contains("(manual/manual.md)"));
        assert!(on_disk("manual/manual.md").contains("zebra"));

        // the tags went along, for the file and for what was inside the folder
        for moved in ["manual/manual.md", "manual/extra/api.md"] {
            let meta = ws.file_meta(moved).expect("meta");
            assert_eq!(meta.tags.len(), 1, "{moved}");
        }
        // the search index learned the new place
        wait_until("the index to follow the move", || {
            ws.search("zebra", None)
                .first()
                .is_some_and(|hit| hit.path == "manual/manual.md")
        });
        wait_until("the tree to refresh", || {
            fx.rec.tree.load(Ordering::SeqCst) >= 1
        });

        // A negative proof: wait longer than the watcher's quiet period, then look. Nothing of this
        // reached the feed: no removed and created rows for every file, no row for the rewritten README.
        thread::sleep(Duration::from_millis(900));
        let seen = fs_paths(&fx.rec);
        assert!(
            seen.is_empty(),
            "the person's own moves must not reach the feed: {seen:?}"
        );

        // An agent saving a moved file afterwards is news.
        std::fs::write(
            fx.project.path().join("manual/manual.md"),
            "# Guide\n\nby an agent\n",
        )
        .expect("write");
        wait_until("the agent's save in the feed", || {
            fs_paths(&fx.rec).iter().any(|p| p == "manual/manual.md")
        });
        ws.close();
    }

    #[test]
    fn a_big_folder_moved_in_the_explorer_is_not_a_bulk_row() {
        let fx = Fx::new();
        for n in 0..70 {
            write(
                fx.project.path(),
                &format!("docs/note-{n}.md"),
                &format!("# Note {n}\n"),
            );
        }
        let ws = fx.open();
        fx.indexed();
        let folder = [Move {
            from: "docs".to_owned(),
            to: "manual".to_owned(),
        }];
        let preview = ws.preview_move(&folder, true, &[]).expect("preview");
        ws.apply_move(preview.plan_id, true, &[]).expect("apply");
        assert!(fx.project.path().join("manual/note-69.md").is_file());
        wait_until("the tree to refresh", || {
            fx.rec.tree.load(Ordering::SeqCst) >= 1
        });

        thread::sleep(Duration::from_millis(900));
        let batches = fx.rec.fs.lock().expect("lock").clone();
        assert!(
            batches.is_empty(),
            "70 files moved by the person must not reach the feed: {} batch(es), bulk {:?}",
            batches.len(),
            batches
                .first()
                .and_then(|b| b.bulk.as_ref().map(|x| x.count))
        );
        ws.close();
    }

    fn fs_paths(rec: &Recorder) -> Vec<String> {
        rec.fs
            .lock()
            .expect("lock")
            .iter()
            .flat_map(|b| b.changes.iter().map(|c| c.path.clone()))
            .collect()
    }

    fn sink_of(rec: &Arc<Recorder>) -> Arc<dyn EventSink> {
        rec.clone()
    }

    #[test]
    fn a_folder_that_cannot_be_watched_still_opens_without_live_updates() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n\nearlyword\n");
        let sink = sink_of(&fx.rec);
        let ws = Workspace::open_with(fx.project.path(), fx.app.path(), None, &sink, |_, _, _| {
            Err(crate::watch::WatchError::Notify("no change notification here".to_owned()).into())
        })
        .expect("the folder opens anyway");
        assert!(!ws.info().live_updates);
        assert_eq!(
            ws.read_document("a.md").expect("doc").title.as_deref(),
            Some("A")
        );
        fx.indexed();
        assert_eq!(ws.search("earlyword", None).len(), 1);

        // Nothing reports changes, so coming back to the window looks at the disk instead.
        write(fx.project.path(), "later.md", "# L\n\nlateword\n");
        ws.refresh();
        wait_until("the refresh finds the new file", || {
            !ws.search("lateword", None).is_empty()
        });
        assert!(fx.rec.tree.load(Ordering::SeqCst) >= 1, "tree refresh");
        ws.close();
    }

    #[test]
    fn a_watched_folder_reports_live_updates_and_does_not_refresh_on_focus() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        let ws = fx.open();
        assert!(ws.info().live_updates);
        let before = fx.rec.tree.load(Ordering::SeqCst);
        ws.refresh();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(fx.rec.tree.load(Ordering::SeqCst), before);
        ws.close();
    }

    #[test]
    fn since_last_visit_skips_files_that_were_already_uncommitted_and_have_not_changed() {
        let fx = Fx::new();
        let p = fx.project.path();
        let repo = init_repo(p);
        write(p, "a.md", "one\n");
        write(p, "b.md", "one\n");
        commit_all(&repo, "init");
        // Uncommitted when the project is closed (an agent leaves its work like this).
        write(p, "a.md", "work in progress\n");
        write(p, "c.md", "scratch\n");
        let ws = fx.open();
        ws.close();

        // While the app is closed one more file is edited and one is created.
        write(p, "b.md", "two\n");
        write(p, "e.md", "new\n");
        let ws = fx.open();
        let listed = |ws: &Workspace| -> Vec<String> {
            let mut paths: Vec<String> = ws
                .since_last_visit()
                .expect("since")
                .files
                .into_iter()
                .map(|f| f.path)
                .collect();
            paths.sort();
            paths
        };
        assert_eq!(
            listed(&ws),
            ["b.md", "e.md"],
            "a.md and c.md did not change"
        );
        ws.close();

        // Everything was seen at the last close; only an edit made afterwards counts.
        write(p, "a.md", "work in progress, edited again\n");
        let ws = fx.open();
        assert_eq!(listed(&ws), ["a.md"]);
        ws.close();
    }

    #[test]
    fn since_last_visit_skips_uncommitted_files_that_were_committed_unchanged_meanwhile() {
        let fx = Fx::new();
        let p = fx.project.path();
        let repo = init_repo(p);
        write(p, "a.md", "one\n");
        commit_all(&repo, "init");
        write(p, "a.md", "work in progress\n");
        let ws = fx.open();
        ws.close();

        commit_all(&repo, "the agent commits the work in progress");
        let ws = fx.open();
        assert_eq!(ws.since_last_visit().expect("since").files.len(), 0);
        ws.close();
    }

    #[test]
    fn moving_a_folder_moves_its_documents_in_the_search_index() {
        let fx = Fx::new();
        write(fx.project.path(), "docs/a.md", "# A\n\nmovedword\n");
        write(fx.project.path(), "docs/deep/b.md", "# B\n\nmovedword\n");
        let ws = fx.open();
        fx.indexed();
        assert_eq!(ws.search("movedword", None).len(), 2);

        std::fs::create_dir_all(fx.project.path().join("archive")).expect("mkdir");
        std::fs::rename(
            fx.project.path().join("docs"),
            fx.project.path().join("archive").join("docs"),
        )
        .expect("move the folder");
        wait_until("the index follows the move", || {
            let hits = ws.search("movedword", None);
            hits.len() == 2 && hits.iter().all(|h| h.path.starts_with("archive/docs/"))
        });
        ws.close();
    }

    #[test]
    fn deleting_a_folder_removes_its_documents_from_search() {
        let fx = Fx::new();
        write(fx.project.path(), "docs/a.md", "# A\n\ngoneword\n");
        write(fx.project.path(), "keep.md", "# K\n\ngoneword\n");
        let ws = fx.open();
        fx.indexed();
        assert_eq!(ws.search("goneword", None).len(), 2);
        std::fs::remove_dir_all(fx.project.path().join("docs")).expect("rm");
        wait_until("the folder's documents leave the index", || {
            ws.search("goneword", None).len() == 1
        });
        ws.close();
    }

    #[test]
    fn images_are_served_from_the_open_folder_only() {
        let fx = Fx::new();
        write(fx.project.path(), "img/a.png", "PNGDATA");
        write(fx.project.path(), "photo.JPG", "JPGDATA");
        write(fx.project.path(), "notes.md", "# N\n");
        let outside = tempfile::tempdir().expect("tempdir");
        write(outside.path(), "secret.png", "SECRET");
        let ws = fx.open();

        let a = ws.read_asset("img/a.png").expect("png");
        assert_eq!((a.mime, a.bytes.as_slice()), ("image/png", &b"PNGDATA"[..]));
        assert_eq!(ws.read_asset("photo.JPG").expect("jpg").mime, "image/jpeg");

        let outside_name = outside.path().file_name().expect("name").to_string_lossy();
        let escape = format!("../{outside_name}/secret.png");
        for bad in [
            escape.as_str(),
            "notes.md",
            "img",
            "missing.png",
            "/etc/hosts.png",
            "C:/Windows/x.png",
        ] {
            assert!(ws.read_asset(bad).is_err(), "{bad} must not be served");
        }
        assert_eq!(
            ws.read_asset("notes.md").expect_err("not an image").kind(),
            "invalidPath"
        );
        ws.close();
    }

    #[test]
    fn simple_mode_serves_only_the_images_next_to_the_file() {
        let fx = Fx::new();
        write(fx.project.path(), "notes/todo.md", "# Todo\n");
        write(fx.project.path(), "notes/a.png", "PNGDATA");
        write(fx.project.path(), "notes/img/b.png", "PNGDATA");
        let ws = fx.open_path(&fx.project.path().join("notes").join("todo.md"));
        assert!(ws.read_asset("a.png").is_ok());
        assert_eq!(
            ws.read_asset("img/b.png").expect_err("nested").kind(),
            "pathEscape"
        );
        ws.close();
    }

    #[test]
    fn external_and_reveal_targets_are_checked_like_every_other_path() {
        let fx = Fx::new();
        for name in [
            "a.md",
            "Notes.MD",
            "img.png",
            "Makefile",
            "run.exe",
            "tool.bat",
            "link.lnk",
            "script.ps1",
            "main.py",
        ] {
            write(fx.project.path(), name, "x");
        }
        std::fs::create_dir(fx.project.path().join("dir")).expect("mkdir");
        let ws = fx.open();

        for ok in ["a.md", "Notes.MD", "img.png", "Makefile"] {
            let target = ws.external_target(ok).expect(ok);
            assert!(target.is_file(), "{ok}");
        }
        for bad in [
            "run.exe",
            "tool.bat",
            "link.lnk",
            "script.ps1",
            "main.py",
            "dir",
            "missing.md",
            "../outside.md",
            "/etc/passwd",
        ] {
            assert!(ws.external_target(bad).is_err(), "{bad} must not be opened");
        }
        assert_eq!(
            ws.external_target("run.exe").expect_err("exe").kind(),
            "invalidPath"
        );

        // Revealing is harmless for anything inside the folder, folders included.
        assert!(ws.reveal_target("dir").expect("dir").is_dir());
        assert!(ws.reveal_target("run.exe").is_ok());
        assert!(ws.reveal_target("../outside.md").is_err());
        assert!(ws.reveal_target("missing.md").is_err());
        ws.close();
    }

    // ---- switching folders --------------------------------------------------------------------------

    struct HostFx {
        a: tempfile::TempDir,
        b: tempfile::TempDir,
        app: tempfile::TempDir,
        rec: Arc<Recorder>,
        host: WorkspaceHost,
    }

    impl HostFx {
        fn new() -> Self {
            let fx = Self {
                a: tempfile::tempdir().expect("tempdir"),
                b: tempfile::tempdir().expect("tempdir"),
                app: tempfile::tempdir().expect("tempdir"),
                rec: Arc::new(Recorder::default()),
                host: WorkspaceHost::default(),
            };
            write(fx.a.path(), "a.md", "# A\n");
            write(fx.a.path(), "only-in-a.png", "A");
            write(fx.b.path(), "b.md", "# B\n");
            fx
        }

        fn open(&self, dir: &Path) -> Result<VaultInfo, AppError> {
            let request = self.host.request();
            self.host.open(request, &sink_of(&self.rec), |sink| {
                Workspace::open(dir, self.app.path(), None, sink)
            })
        }

        fn current_root(&self) -> String {
            self.host.current().expect("current").info().root
        }
    }

    fn root_of(dir: &tempfile::TempDir) -> String {
        dunce::canonicalize(dir.path())
            .expect("canonicalize")
            .display()
            .to_string()
    }

    #[test]
    fn opening_a_folder_replaces_the_previous_one_and_closes_it() {
        let fx = HostFx::new();
        let info = fx.open(fx.a.path()).expect("open A");
        assert_eq!(info.root, root_of(&fx.a));
        assert!(fx.host.read_asset("only-in-a.png").is_ok());

        fx.open(fx.b.path()).expect("open B");
        assert_eq!(fx.current_root(), root_of(&fx.b));
        // The folder that was closed can no longer be read through the image protocol.
        assert_eq!(
            fx.host
                .read_asset("only-in-a.png")
                .expect_err("closed")
                .kind(),
            "notFound"
        );
        // A change in the old folder is not reported any more; one in the new folder is.
        write(fx.a.path(), "late-a.md", "x");
        write(fx.b.path(), "late-b.md", "x");
        wait_until("B reports its change", || {
            fs_paths(&fx.rec).contains(&"late-b.md".to_owned())
        });
        assert!(!fs_paths(&fx.rec).contains(&"late-a.md".to_owned()));
        fx.host.shutdown();
    }

    #[test]
    fn an_open_request_that_a_newer_one_overtook_is_dropped() {
        let fx = HostFx::new();
        let first = fx.host.request();
        let second = fx.host.request();
        let sink = sink_of(&fx.rec);
        let opener = |dir: PathBuf| {
            let app = fx.app.path().to_path_buf();
            move |sink: &Arc<dyn EventSink>| Workspace::open(&dir, &app, None, sink)
        };
        let err = fx
            .host
            .open(first, &sink, opener(fx.a.path().to_path_buf()))
            .expect_err("superseded");
        assert_eq!(err.kind(), "superseded");
        assert!(matches!(fx.host.current(), Err(AppError::NoVault)));
        fx.host
            .open(second, &sink, opener(fx.b.path().to_path_buf()))
            .expect("the newest request opens");
        assert_eq!(fx.current_root(), root_of(&fx.b));
        fx.host.shutdown();
    }

    #[test]
    fn a_slow_open_that_finishes_after_a_newer_request_started_cannot_mute_the_newest_folder() {
        let fx = HostFx::new();
        let sink = sink_of(&fx.rec);
        let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();

        // A asked first, and is slow.
        let request_a = fx.host.request();
        let slow = {
            let (host, sink) = (fx.host.clone(), Arc::clone(&sink));
            let (dir, app) = (fx.a.path().to_path_buf(), fx.app.path().to_path_buf());
            thread::spawn(move || {
                host.open(request_a, &sink, |sink| {
                    started_tx.send(()).expect("send");
                    go_rx.recv().expect("go");
                    Workspace::open(&dir, &app, None, sink)
                })
            })
        };
        started_rx.recv().expect("A is opening");

        // B is asked for while A is still opening, and has to wait for its turn.
        let request_b = fx.host.request();
        let newest = {
            let (host, sink) = (fx.host.clone(), Arc::clone(&sink));
            let (dir, app) = (fx.b.path().to_path_buf(), fx.app.path().to_path_buf());
            thread::spawn(move || {
                host.open(request_b, &sink, |sink| {
                    Workspace::open(&dir, &app, None, sink)
                })
            })
        };
        go_tx.send(()).expect("go");
        slow.join().expect("A thread").expect("A opened");
        newest.join().expect("B thread").expect("B opened");

        assert_eq!(fx.current_root(), root_of(&fx.b), "the last request wins");
        // The folder on screen keeps receiving its events (it used to be muted for good).
        write(fx.b.path(), "later.md", "x");
        wait_until("B reports its change", || {
            fs_paths(&fx.rec).contains(&"later.md".to_owned())
        });
        fx.host.shutdown();
    }

    #[test]
    fn a_failed_open_leaves_the_current_folder_open_and_emitting() {
        let fx = HostFx::new();
        fx.open(fx.a.path()).expect("open A");
        let err = fx
            .open(&fx.a.path().join("no-such-folder"))
            .expect_err("missing");
        assert_eq!(err.kind(), "notFound");
        assert_eq!(fx.current_root(), root_of(&fx.a));
        write_until_reported(
            &fx.rec,
            fx.a.path(),
            "after.md",
            "A still reports its changes",
        );
        fx.host.shutdown();
    }

    #[test]
    fn a_workspace_that_is_still_starting_emits_nothing_until_it_is_installed() {
        let fx = HostFx::new();
        let request = fx.host.request();
        let rec = Arc::clone(&fx.rec);
        fx.host
            .open(request, &sink_of(&fx.rec), |sink| {
                let ws = Workspace::open(fx.a.path(), fx.app.path(), None, sink)?;
                // The index worker has reported its progress by now, but nobody may hear about it yet.
                thread::sleep(Duration::from_millis(300));
                sink.tree_changed();
                assert!(rec.progress.lock().expect("lock").is_empty());
                assert_eq!(rec.tree.load(Ordering::SeqCst), 0);
                Ok(ws)
            })
            .expect("open");
        write_until_reported(
            &fx.rec,
            fx.a.path(),
            "later.md",
            "events flow once the folder is installed",
        );
        fx.host.shutdown();
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
