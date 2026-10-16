//! An open folder, with everything running for it: watcher, git worker, search index, state.
//!
//! This is the Tauri-free core behind the commands: [`Workspace::open`] wires the modules together and
//! reports through an [`EventSink`], which the app implements with Tauri events and tests implement
//! with a recorder. [`WorkspaceHost`] holds the folder that is open and makes switching folders safe.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::Serialize;

use crate::bridge::context::AgentContext;
use crate::bridge::hub::{AgentEvents, AgentStatus, Backend};
use crate::bridge::protocol::Opened;
use crate::bridge::service::AgentBridge;
use crate::bridge::socket::Endpoint;
use crate::comments::{
    CommentBook, CommentError, CommentView, Listener as CommentsListener, NewComment,
};
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
use crate::media::{self, Window};
use crate::meta::catalog::{IndexEntry, TaggedFile};
use crate::meta::service::{FileMeta, TagOverview};
use crate::meta::{MetaService, Timing};
use crate::notes::{
    self, LooseDoc, NoteSaved, NotesError, NotesService, OpenReport, ReadOnlyReason,
};
use crate::refs::ImageRefs;
use crate::render::{self, DocumentView, Registry};
use crate::search::indexer::{IndexWorker, Indexer};
use crate::search::{Bm25Index, SearchHit, SearchProvider};
use crate::session::{
    FileStamp, LastVisit, RecentStore, SessionState, StateLocation, StateStore, TreeConfig,
    WindowState,
};
use crate::unread::{Timing as UnreadTiming, UnreadService};
use crate::util::{modified_ms, now_ms, on_parser_stack};
use crate::vault::{NodeKind, Resolved, TreeNode, TreeOptions, Vault, VaultError, VaultMode};
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
    /// The set of images that documents link to changed (`image-refs-changed`).
    fn image_refs_changed(&self);
    /// The documents that are unread changed (`unread-changed`); `unread` is all of them, sorted.
    fn unread_changed(&self, unread: &[String]);
    /// The agent connection changed: turned on or off, an agent came or went, or one asked something (`agent-status`).
    fn agent_status(&self, status: &AgentStatus);
    /// An agent asked to show a place (`agent-open`); the UI picks it up with `take_agent_open`.
    fn agent_open(&self);
    /// The comments changed: one was added, completed or deleted, or a document moved (`comments-changed`).
    fn comments_changed(&self);
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

    fn image_refs_changed(&self) {
        if self.live() {
            self.inner.image_refs_changed();
        }
    }

    fn unread_changed(&self, unread: &[String]) {
        if self.live() {
            self.inner.unread_changed(unread);
        }
    }

    fn agent_status(&self, status: &AgentStatus) {
        if self.live() {
            self.inner.agent_status(status);
        }
    }

    fn agent_open(&self) {
        if self.live() {
            self.inner.agent_open();
        }
    }

    fn comments_changed(&self) {
        if self.live() {
            self.inner.comments_changed();
        }
    }
}

/// What an open folder is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceKind {
    /// A project: a folder (or a file) the person reads and follows, with git, unread markers and the agent connection.
    Project,
    /// The notes folder: a vault root with no git, no unread worker, no last visit, no entry among the recent folders
    /// and never an agent connection.
    Notes,
}

/// Matches the UI's `VaultInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub root: String,
    pub name: String,
    pub kind: WorkspaceKind,
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

/// Which part of a file an [`Asset`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// All of it.
    Whole,
    /// The bytes `start` to `end`, both included, because a player asked for them.
    Range { start: u64, end: u64 },
    /// The request starts after the end of the file: nothing is carried.
    Unsatisfiable,
}

/// A picture, sound or video of the open folder (or a part of one), ready to be served to the document view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    /// The size of the whole file.
    pub total: u64,
    pub part: Part,
}

/// Extensions of the files that "Open in external editor" hands to the system's default application when no editor
/// command is set. Files that can run code when opened (programs, scripts, shortcuts, installers) are not documents and
/// are left out; so are source files, because some systems run those on open too. Pictures, sound and video are
/// allowed by their kind (see [`media::asset_kind`]).
const EXTERNAL_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mdown", "mkd", "mdx", "txt", "text", "log", "rst", "adoc", "org", "tex",
    "bib", "json", "jsonc", "yaml", "yml", "toml", "xml", "xsd", "csv", "tsv", "ini", "cfg",
    "conf", "html", "htm", "css", "pdf",
];

/// How long a closing window waits for the unread markers' file to be written (see [`Workspace::save_last_visit`]).
const CLOSE_WAIT: Duration = Duration::from_millis(500);

/// The notes folder opened as a workspace, and what opening it found.
pub struct NotesPart {
    service: NotesService,
    opened: OpenReport,
}

pub struct Workspace {
    kind: WorkspaceKind,
    /// The notes, for the workspace of the notes folder.
    notes: Option<NotesPart>,
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
    /// The images that documents link to, kept by the index worker.
    image_refs: Arc<RwLock<ImageRefs>>,
    indexer: Arc<IndexWorker>,
    filter: Arc<PathFilter>,
    meta: Arc<MetaService>,
    /// Which documents changed since they were last read.
    unread: Arc<UnreadService>,
    /// The comments the person left on passages.
    comments: Arc<CommentBook>,
    /// Content the editor is writing, so the watcher does not report it back as an external change.
    own_writes: Arc<OwnWrites>,
    /// Rename, move, delete and new folder in the Explorer.
    fileops: FileOpsService,
    /// What an AI agent can ask of this folder while the person has the agent connection on.
    agent: AgentBridge,
    /// The agent connection was on when the folder was opened (see [`Workspace::activate`]).
    agent_wanted: bool,
    sink: Arc<dyn EventSink>,
    last_visit: Option<LastVisit>,
    closed: AtomicBool,
}

/// What the app keeps for a file follows it when it is renamed or moved in the Explorer: its tags, and whether
/// it was read. A document whose links the app rewrote stays as read as it was.
struct AppFollower {
    meta: Arc<MetaService>,
    unread: Arc<UnreadService>,
    comments: Arc<CommentBook>,
}

impl Followers for AppFollower {
    fn renamed(&self, from: &str, to: &str, is_dir: bool) {
        if is_dir {
            self.meta.rename_tree(from, to);
        } else {
            self.meta.rename(from, to);
        }
        self.unread.renamed(from, to);
        self.comments.renamed(from, to, is_dir);
    }

    fn rewritten(&self, path: &str, old_hash: &str, new_hash: &str) {
        self.unread.rewritten(path, old_hash, new_hash);
    }
}

/// What only the open folder can tell an agent: the live search index, the live unread markers and the comments.
struct LiveBackend {
    index: Arc<RwLock<Bm25Index>>,
    unread: Arc<UnreadService>,
    comments: Arc<CommentBook>,
    /// The data folder of the program: where the person's choice of the notes folder is kept.
    app_data: PathBuf,
}

impl Backend for LiveBackend {
    fn notes_folder(&self) -> Option<PathBuf> {
        Some(notes::share::notes_root(&self.app_data))
    }

    fn unread(&self) -> Vec<String> {
        self.unread.unread()
    }

    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        self.index.search(query, limit)
    }

    fn comments(&self) -> Vec<CommentView> {
        self.comments.list(true)
    }

    fn resolve_comment(&self, id: &str, reply: Option<&str>) -> Result<CommentView, CommentError> {
        self.comments.resolve(id, reply)
    }
}

/// Tells the UI what happens to the agent connection.
struct AgentSink(Arc<dyn EventSink>);

impl AgentEvents for AgentSink {
    fn status_changed(&self, status: &AgentStatus) {
        self.0.agent_status(status);
    }

    fn open_requested(&self) {
        self.0.agent_open();
    }
}

/// The agent connection of a folder, off until [`Workspace::activate`] starts it, and whether the person had it on
/// the last time (a single file is no project and never connects).
fn agent_bridge(
    vault: &Arc<Vault>,
    store: &StateStore,
    index: &Arc<RwLock<Bm25Index>>,
    unread: &Arc<UnreadService>,
    comments: &Arc<CommentBook>,
    sink: &Arc<dyn EventSink>,
    app_data: &Path,
) -> (AgentBridge, bool) {
    let bridge = AgentBridge::new(
        Arc::clone(vault),
        Endpoint::for_project(vault.root()),
        store.agent_path(),
        Arc::new(LiveBackend {
            index: Arc::clone(index),
            unread: Arc::clone(unread),
            comments: Arc::clone(comments),
            app_data: app_data.to_path_buf(),
        }),
        Arc::new(AgentSink(Arc::clone(sink))),
    );
    // the notes folder is private even where it lies inside the project: no tool of the connection reaches it
    bridge.hide(crate::bridge::answers::PrivateTree::of(
        app_data,
        vault.root(),
    ));
    let wanted = !vault.is_shallow() && store.agent_connection();
    (bridge, wanted)
}

/// What the watcher thread needs to do its work.
struct WatchWiring {
    vault: Arc<Vault>,
    filter: Arc<PathFilter>,
    sigs: Arc<SigStore>,
    sink: Arc<dyn EventSink>,
    indexer: Arc<IndexWorker>,
    meta: Arc<MetaService>,
    unread: Arc<UnreadService>,
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
        unread,
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
                unread.rescan();
            } else if !all_changes.is_empty() {
                unread.apply(&all_changes);
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

/// Starts the index worker: the only writer of the search index and of the set of images that documents
/// use. It builds them first, then follows the watcher. Returns the worker and the set.
fn start_indexer(
    vault: &Arc<Vault>,
    filter: &Arc<PathFilter>,
    index: &Arc<RwLock<Bm25Index>>,
    sigs: &Arc<SigStore>,
    sink: &Arc<dyn EventSink>,
) -> (Arc<IndexWorker>, Arc<RwLock<ImageRefs>>) {
    let progress_sink = Arc::clone(sink);
    let refs_sink = Arc::clone(sink);
    let indexer = Indexer::new(
        Arc::clone(vault),
        Arc::clone(filter),
        Arc::clone(index),
        Arc::clone(sigs),
    )
    .on_image_refs_changed(Arc::new(move || refs_sink.image_refs_changed()));
    let image_refs = indexer.image_refs();
    let worker = IndexWorker::start(indexer, move |done, total| {
        progress_sink.index_progress(done, total);
    });
    (Arc::new(worker), image_refs)
}

/// Starts the unread markers: they tell the UI through the sink whenever the unread documents change.
fn start_unread(
    vault: &Arc<Vault>,
    filter: &Arc<PathFilter>,
    read_path: PathBuf,
    sink: &Arc<dyn EventSink>,
) -> Arc<UnreadService> {
    let sink = Arc::clone(sink);
    Arc::new(UnreadService::open(
        Arc::clone(vault),
        Arc::clone(filter),
        read_path,
        UnreadTiming::default(),
        Arc::new(move |paths: &[String]| sink.unread_changed(paths)),
        None,
    ))
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
        Self::open_with(
            path,
            app_data_dir,
            home,
            sink,
            start_watcher,
            WorkspaceKind::Project,
        )
    }

    /// Opens the notes folder: a vault root with no git, no unread markers, no last visit, no entry among the recent
    /// folders, and never an agent connection (see [`WorkspaceKind::Notes`]). The folder must be there: nothing is
    /// created here.
    pub fn open_notes(
        folder: &Path,
        app_data_dir: &Path,
        home: Option<&Path>,
        sink: &Arc<dyn EventSink>,
    ) -> Result<Self, AppError> {
        Self::open_with(
            folder,
            app_data_dir,
            home,
            sink,
            start_watcher,
            WorkspaceKind::Notes,
        )
    }

    /// [`Workspace::open`] with the way the watcher is started as a parameter (tests make it fail).
    // It wires every part of an open folder together, one after the other; each part has its own helper already.
    #[allow(clippy::too_many_lines)]
    fn open_with(
        path: &Path,
        app_data_dir: &Path,
        home: Option<&Path>,
        sink: &Arc<dyn EventSink>,
        watch: StartWatcher,
        kind: WorkspaceKind,
    ) -> Result<Self, AppError> {
        let vault = Vault::open(path)?;
        let notes_kind = kind == WorkspaceKind::Notes;
        if notes_kind && vault.is_shallow() {
            return Err(VaultError::Invalid(format!("not a folder: {}", path.display())).into());
        }
        // The notes folder keeps this computer's settings in the app's own data; a project may keep them in its folder.
        let store = if notes_kind {
            StateStore::open_in_app_data(app_data_dir, vault.root())
        } else {
            StateStore::open(app_data_dir, vault.root())
        };
        // A `config.toml` is a project's: the notes folder has none (it may be synced to other computers).
        let config = if notes_kind {
            crate::session::Config::default()
        } else {
            store.config()
        };
        let last_visit = if notes_kind {
            None
        } else {
            store.load_last_visit()
        };

        // Git first: linked worktrees become extra roots of the vault. The notes folder is not a repository, whatever
        // folder it happens to be inside.
        let discovery = if notes_kind {
            Discovery::NoRepo
        } else {
            git::discover(vault.root(), home)
        };
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
        // The notes are private: a notes folder inside the project is not part of what is indexed, searched or listed as unread
        // (the agent connection answers from those).
        let mut ignore = config.ignore.clone();
        if !notes_kind {
            ignore.extend(crate::notes::folder::ignore_inside(
                app_data_dir,
                crate::session::AppSettingsStore::new(app_data_dir)
                    .load()
                    .notes_folder
                    .as_deref(),
                vault.root(),
            ));
        }
        let filter = Arc::new(PathFilter::new(&roots, &ignore));
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

        let unread = if notes_kind {
            Arc::new(UnreadService::off(Arc::clone(&vault)))
        } else {
            start_unread(&vault, &filter, store.read_path(), sink)
        };
        let comments_sink = Arc::clone(sink);
        let comments_heard: CommentsListener = Arc::new(move || comments_sink.comments_changed());
        let comments = Arc::new(
            CommentBook::new(Arc::clone(&vault), store.comments_path())
                .with_listener(comments_heard),
        );

        let own_writes = Arc::new(OwnWrites::new());
        let fileops = FileOpsService::new(
            Arc::clone(&vault),
            Arc::clone(&own_writes),
            Arc::new(AppFollower {
                meta: Arc::clone(&meta),
                unread: Arc::clone(&unread),
                comments: Arc::clone(&comments),
            }),
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

        let (indexer, image_refs) = start_indexer(&vault, &filter, &index, &sigs, sink);

        // No watcher is not fatal: the folder is perfectly readable, it just is not live.
        let watcher = watch(
            WatchWiring {
                vault: Arc::clone(&vault),
                filter: Arc::clone(&filter),
                sigs,
                sink: Arc::clone(sink),
                indexer: Arc::clone(&indexer),
                meta: Arc::clone(&meta),
                unread: Arc::clone(&unread),
                own_writes: Arc::clone(&own_writes),
            },
            git_ctx.as_ref(),
            git.trigger_fn(),
        )
        .ok();
        // The watcher is live now: ask git to look once more so nothing slipped through between
        // its first computation and the watcher start.
        git.trigger();

        // A failure to remember the folder must not stop it from opening. The notes are not a recent folder.
        if !notes_kind {
            let _ = RecentStore::new(app_data_dir).touch(
                &vault.root().display().to_string(),
                &vault.name(),
                now_ms(),
            );
        }

        // Started by `activate`, once this folder has replaced the previous one (which must let go of the socket first).
        // The notes never connect to an agent: they are private.
        let (agent, agent_wanted) = agent_bridge(
            &vault,
            &store,
            &index,
            &unread,
            &comments,
            sink,
            app_data_dir,
        );
        let agent_wanted = agent_wanted && !notes_kind;
        let notes = notes_kind.then(|| {
            let (service, opened) = NotesService::open(
                Arc::clone(&vault),
                Arc::clone(&own_writes),
                Arc::new(OsRecycler),
                home.map(Path::to_path_buf),
                now_ms(),
                &notes::process_is_running,
            );
            NotesPart { service, opened }
        });
        Ok(Self {
            kind,
            notes,
            vault,
            registry: Registry::with_defaults(),
            store: Mutex::new(store),
            config,
            git_ctx,
            git,
            live_updates: watcher.is_some(),
            watcher: Mutex::new(watcher),
            index,
            image_refs,
            indexer,
            filter,
            meta,
            unread,
            comments,
            own_writes,
            fileops,
            agent,
            agent_wanted,
            sink: Arc::clone(sink),
            last_visit,
            closed: AtomicBool::new(false),
        })
    }

    /// Called once this folder is the one that is open: starts the agent connection when the person had it on.
    pub fn activate(&self) {
        if self.agent_wanted {
            self.agent.set_enabled(true);
        } else {
            self.agent.remove_stale_file();
        }
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
            kind: self.kind,
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

    /// Whether `path` is the path of a loose file of this window's notes.
    fn is_loose(&self, path: &str) -> bool {
        self.notes.is_some() && notes::is_loose_path(path)
    }

    /// The scope of a loose file, for a UI path of one (`@file/<id>/<name>`) in the Notes window; `None` for any other path.
    /// A loose file is reached through its id only: the file, and the images next to it.
    fn loose_doc(&self, path: &str) -> Result<Option<LooseDoc>, AppError> {
        match &self.notes {
            Some(part) if notes::is_loose_path(path) => Ok(Some(part.service.loose_doc(path)?)),
            _ => Ok(None),
        }
    }

    /// [`Vault::resolve`] for the paths of the folder, and for the paths of a loose file the file's own place.
    fn resolve_path(&self, path: &str) -> Result<Resolved, AppError> {
        if let Some(doc) = self.loose_doc(path)? {
            let target = doc.asset.as_deref().unwrap_or(&doc.name);
            let mut resolved = doc.vault.resolve(target)?;
            resolved.rel = notes::loose_ui_path(&doc.id, target);
            return Ok(resolved);
        }
        Ok(self.vault.resolve(path)?)
    }

    pub fn read_document(&self, path: &str) -> Result<DocumentView, AppError> {
        if let Some(doc) = self.loose_doc(path)? {
            if doc.asset.is_some() {
                return Err(VaultError::NotFound(path.to_owned()).into());
            }
            let resolved = doc.vault.resolve(&doc.name)?;
            // looked at first: a change after this point is a change to the version that is shown
            self.notes()?.mark_seen(&doc.id);
            return Ok(on_parser_stack(|| {
                render::load_document(&self.registry, &resolved.abs, &doc.ui_path)
            })?);
        }
        let resolved = self.vault.resolve(path)?;
        // A picture, sound or video is not read: the view holds its size and time, and the UI draws the file itself.
        if let Some(kind) = media::asset_kind(&resolved.rel) {
            return Ok(render::asset_view(&resolved.abs, &resolved.rel, kind)?);
        }
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
        if let Some(doc) = self.loose_doc(path)? {
            if doc.asset.is_some() {
                return Err(VaultError::NotFound(path.to_owned()).into());
            }
            self.notes()?.mark_seen(&doc.id);
            // a loose file's editor takes plain text too, and nothing it does is the notes watcher's business
            let own = OwnWrites::new();
            let mut source = Editor::new(&doc.vault, &own)
                .with_text_files()
                .source(&doc.name)?;
            source.path = doc.ui_path;
            return Ok(source);
        }
        Ok(self.editor().source(path)?)
    }

    /// Saves the editor's text unless the file changed since `expected_hash`.
    pub fn save_document(
        &self,
        path: &str,
        text: &str,
        expected_hash: &str,
    ) -> Result<SaveOutcome, AppError> {
        // a loose file is written only here, when the person asks, and nothing of it is news or unread
        if self.is_loose(path) {
            return Ok(self.notes()?.save_file(path, text, expected_hash)?);
        }
        self.require_writable()?;
        let outcome = self.editor().save(path, text, expected_hash)?;
        // What the person wrote is read by definition.
        if let SaveOutcome::Saved { hash, .. } = &outcome {
            self.unread.mark_written(path, hash);
        }
        Ok(outcome)
    }

    /// Creates a new Markdown file (never overwrites).
    pub fn create_document(&self, path: &str, text: &str) -> Result<CreatedDoc, AppError> {
        // the only file a loose file's path can create is the file itself, again, where it was
        if self.is_loose(path) {
            return Ok(self.notes()?.recreate_file(path, text)?);
        }
        self.require_writable()?;
        let created = self.editor().create(path, text)?;
        self.unread.mark_written(&created.path, &created.hash);
        Ok(created)
    }

    /// Writes a pasted or dropped image into the `assets` folder next to a document.
    pub fn write_asset(
        &self,
        doc_path: &str,
        name_hint: &str,
        data: &[u8],
    ) -> Result<AssetWritten, AppError> {
        self.require_writable()?;
        // a loose file takes no images: its folder is not ours to add to (the vault of one file says so)
        if let Some(doc) = self.loose_doc(doc_path)? {
            let own = OwnWrites::new();
            return Ok(Editor::new(&doc.vault, &own).write_asset(&doc.name, name_hint, data)?);
        }
        Ok(self.editor().write_asset(doc_path, name_hint, data)?)
    }

    // ---- notes ----------------------------------------------------------------------------------

    /// The notes of this window; an error for a window that shows a project.
    pub fn notes(&self) -> Result<&NotesService, AppError> {
        self.notes
            .as_ref()
            .map(|part| &part.service)
            .ok_or_else(|| NotesError::NotOpen.into())
    }

    /// What opening the notes found: whether they can only be read, and the notes made from a crash's leftovers.
    pub fn notes_opened(&self) -> Option<&OpenReport> {
        self.notes.as_ref().map(|part| &part.opened)
    }

    /// Where a file of the disk is in this project, when this window shows a folder and the file is inside it: its path in the
    /// project. A file anywhere else (and every file in the Notes window) is not in a project.
    pub fn locate_in_project(&self, path: &Path) -> Option<String> {
        if self.notes.is_some() || self.vault.is_shallow() {
            return None;
        }
        let canonical = dunce::canonicalize(path).ok()?;
        if !canonical.is_file() {
            return None;
        }
        self.vault.locate(&canonical).map(|located| located.rel)
    }

    /// Whether this workspace is the notes, and in the folder `folder`.
    pub fn shows_notes_of(&self, folder: &Path) -> bool {
        self.notes.is_some()
            && dunce::canonicalize(folder).is_ok_and(|folder| folder == self.vault.root())
    }

    /// Saves a note over the file it came from (see [`NotesService::save`]).
    pub fn save_note(
        &self,
        path: &str,
        text: &str,
        expected_hash: &str,
    ) -> Result<NoteSaved, AppError> {
        Ok(self
            .notes()?
            .save(path, text, expected_hash, now_ms(), notes::Moment::now())?)
    }

    /// Writing is refused in the notes of a second TrailMark window (see [`NotesService::open`]).
    fn require_writable(&self) -> Result<(), AppError> {
        match &self.notes {
            Some(part) => Ok(part.service.writable()?),
            None => Ok(()),
        }
    }

    /// Why the notes can only be read in this window, if they can.
    pub fn notes_read_only(&self) -> Option<ReadOnlyReason> {
        self.notes.as_ref().and_then(|part| part.opened.read_only)
    }

    // ---- file operations ------------------------------------------------------------------------

    /// The Explorer's operations need a folder, not a single opened file (whose folder may be Downloads).
    fn file_ops(&self) -> Result<&FileOpsService, AppError> {
        if self.vault.is_shallow() {
            return Err(FileOpError::SimpleMode.into());
        }
        self.require_writable()?;
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
        if let Ok(Some(doc)) = self.loose_doc(from) {
            // a loose file is read by itself: what it links to in its folder is not part of any project
            return match links::resolve(&doc.vault, &doc.name, href) {
                LinkTarget::Internal { path, .. } => LinkTarget::Missing { path },
                other => other,
            };
        }
        links::resolve(&self.vault, from, href)
    }

    pub fn absolute_path(&self, path: &str) -> Result<String, AppError> {
        Ok(self.resolve_path(path)?.abs.display().to_string())
    }

    /// A picture, sound or video for the document view, or the part of a sound or video that `range` (the value of a
    /// `Range` header) asks for. It is looked up like any other path of the folder, so nothing outside it can be read,
    /// and only a folder that is open now can be served. In Simple mode only the files next to the opened file are
    /// served, because that folder is often Downloads or the home folder.
    pub fn read_asset(&self, path: &str, range: Option<&str>) -> Result<Asset, AppError> {
        // the files of a loose file are the ones next to it, and no others
        if let Some(doc) = self.loose_doc(path)? {
            let Some(name) = doc.asset.as_deref() else {
                return Err(VaultError::NotFound(path.to_owned()).into());
            };
            return Self::read_media(&doc.vault, name, range);
        }
        Self::read_media(&self.vault, path, range)
    }

    /// A picture, sound or video of `vault`, looked up like any other path of it, so nothing outside it can be read. A
    /// vault of one file (Simple mode, a loose file) serves the files next to the file only.
    ///
    /// A picture is read whole (up to [`media::MAX_WHOLE`]). A sound or video is cut: only the part asked for is read, at
    /// most [`media::MAX_CHUNK`] bytes, so a long film is never held in memory.
    fn read_media(vault: &Vault, path: &str, range: Option<&str>) -> Result<Asset, AppError> {
        let not_media = || VaultError::Invalid(format!("not a picture, sound or video: {path}"));
        // the name asked for says a picture, a sound or a video (nothing else is looked up)...
        media::asset_kind(path).ok_or_else(not_media)?;
        let resolved = vault.resolve(path)?;
        if vault.is_shallow() && resolved.rel.contains('/') {
            return Err(VaultError::Escapes(path.to_owned()).into());
        }
        // ...and so does the name of the file it leads to: a link `a.png` that points at `.env` serves nothing
        let (kind, mime) = media::served_as(path, &resolved.rel).ok_or_else(not_media)?;
        let io_err = |e| VaultError::io(&resolved.abs, e);
        let meta = std::fs::metadata(&resolved.abs).map_err(io_err)?;
        if !meta.is_file() {
            return Err(VaultError::NotFound(path.to_owned()).into());
        }
        let total = meta.len();
        let window = if kind.is_streamed() {
            match media::window(range, total, media::MAX_CHUNK) {
                // no range asked for: a big file starts with its first part, a small one is sent whole
                Window::Whole if total > media::MAX_WHOLE => {
                    media::window(Some("bytes=0-"), total, media::MAX_CHUNK)
                }
                other => other,
            }
        } else if total > media::MAX_WHOLE {
            return Err(VaultError::NotFound(path.to_owned()).into());
        } else {
            Window::Whole
        };
        let (bytes, part) = match window {
            // at most what the file measured above, which is what the size limit was checked on
            Window::Whole => (
                media::read_whole(&resolved.abs, total).map_err(io_err)?,
                Part::Whole,
            ),
            Window::Part { start, end } => {
                let bytes = media::read_part(&resolved.abs, start, end).map_err(io_err)?;
                // the file got shorter than the answer was planned for: say what is really carried
                match u64::try_from(bytes.len()) {
                    Ok(len) if len > 0 => (
                        bytes,
                        Part::Range {
                            start,
                            end: start + len - 1,
                        },
                    ),
                    _ => (Vec::new(), Part::Unsatisfiable),
                }
            }
            Window::Unsatisfiable => (Vec::new(), Part::Unsatisfiable),
        };
        Ok(Asset {
            bytes,
            mime,
            total,
            part,
        })
    }

    /// The file to hand to the system's default application for "Open externally".
    ///
    /// The path must stay inside the folder, name an existing file, and be a document, a picture, a sound or a video (or
    /// a plain file without an extension): a program or script in the folder is never started from here, and neither is
    /// a file without an extension that the system would run (see [`media::has_execute_bit`]).
    pub fn external_target(&self, path: &str) -> Result<PathBuf, AppError> {
        let resolved = self.resolve_path(path)?;
        if !resolved.abs.is_file() {
            return Err(VaultError::NotFound(path.to_owned()).into());
        }
        let allowed = resolved.abs.extension().map_or_else(
            // No extension: a plain file (`LICENSE`, `Makefile`) is a document, a program is not.
            || !media::has_execute_bit(&resolved.abs),
            |e| {
                let e = e.to_string_lossy().to_ascii_lowercase();
                EXTERNAL_EXTENSIONS.contains(&e.as_str())
                    || media::asset_kind(&resolved.rel).is_some()
            },
        );
        if !allowed {
            return Err(VaultError::Invalid(path.to_owned()).into());
        }
        Ok(resolved.abs)
    }

    /// The file to hand to the system's default application for "Open with default app", which any file of the folder
    /// can go to except a program, script, installer, shortcut or macro document: such a file is refused with its kind
    /// named (see [`media::open_policy`]). The path must stay inside the folder and name an existing file.
    pub fn default_app_target(&self, path: &str) -> Result<PathBuf, AppError> {
        let resolved = self.resolve_path(path)?;
        if !resolved.abs.is_file() {
            return Err(VaultError::NotFound(path.to_owned()).into());
        }
        media::check_openable(&resolved.abs)?;
        Ok(resolved.abs)
    }

    /// The file to hand to the person's editor command. It must stay inside the folder and name an existing file; any
    /// kind of file will do, because it is only ever an argument to the program the person chose (the program
    /// itself comes from their settings, never from the folder).
    pub fn editor_target(&self, path: &str) -> Result<PathBuf, AppError> {
        let resolved = self.resolve_path(path)?;
        if !resolved.abs.is_file() {
            return Err(VaultError::NotFound(path.to_owned()).into());
        }
        Ok(resolved.abs)
    }

    /// The folder (or file) to show in the system's file manager for "Reveal in folder".
    pub fn reveal_target(&self, path: &str) -> Result<PathBuf, AppError> {
        let resolved = self.resolve_path(path)?;
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
            self.unread.rescan();
            self.sink.tree_changed();
        }
    }

    pub fn search(&self, query: &str, limit: Option<usize>) -> Vec<SearchHit> {
        let limit = limit.unwrap_or(DEFAULT_SEARCH_LIMIT).min(MAX_SEARCH_LIMIT);
        self.index.search(query, limit)
    }

    /// The images that some document of the folder links to, embeds or defines a reference for, as paths
    /// of the folder (`@wt/<name>/...` in a worktree). Documents are read once, when the folder opens,
    /// and again one at a time as they change.
    pub fn referenced_images(&self) -> Vec<String> {
        self.image_refs
            .read()
            .map(|refs| refs.images())
            .unwrap_or_default()
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

    /// Remembers where the window is, with this project's state. Simple mode (one file) has no state of its
    /// own to keep it in: such a window is only remembered app-wide.
    pub fn save_window(&self, window: &WindowState) -> Result<(), AppError> {
        if self.vault.mode() == VaultMode::Simple {
            return Ok(());
        }
        self.lock_store()?.save_window(window)?;
        Ok(())
    }

    pub fn init_project_folder(&self) -> Result<(), AppError> {
        let mut store = self.lock_store()?;
        let was_in_project = store.location() == StateLocation::Project;
        store.init_project_folder()?;
        if !was_in_project {
            // The tags and descriptions added so far move into `.trailmark/meta.toml`.
            self.meta.migrate_to(store.meta_path(), true)?;
            self.unread.relocate(store.read_path());
            self.comments.relocate(store.comments_path());
            self.agent.relocate_file(store.agent_path());
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

    // ---- unread markers -------------------------------------------------------------------------

    /// The documents that changed since they were last read, sorted. Empty until the folder has been looked at.
    pub fn unread(&self) -> Vec<String> {
        self.unread.unread()
    }

    /// The person read a document: what it holds now counts as read.
    pub fn mark_read(&self, path: &str) -> Result<(), AppError> {
        Ok(self.unread.mark_read(path)?)
    }

    pub fn mark_unread(&self, path: &str) -> Result<(), AppError> {
        Ok(self.unread.mark_unread(path)?)
    }

    pub fn mark_all_read(&self) {
        self.unread.mark_all_read();
    }

    /// Everything in a folder counts as read.
    pub fn mark_folder_read(&self, path: &str) -> Result<(), AppError> {
        Ok(self.unread.mark_folder_read(path)?)
    }

    // ---- comments ---------------------------------------------------------------------------

    /// Writes a comment on a passage of a document.
    pub fn add_comment(&self, comment: NewComment) -> Result<CommentView, AppError> {
        Ok(self.comments.add(comment)?)
    }

    /// Every comment with its passage found again in the document as it is now.
    pub fn list_comments(&self) -> Vec<CommentView> {
        self.comments.list(true)
    }

    /// The person checks a comment off, or opens it again.
    pub fn set_comment_done(&self, id: &str, done: bool) -> Result<CommentView, AppError> {
        Ok(self.comments.set_done(id, done)?)
    }

    pub fn delete_comment(&self, id: &str) -> Result<(), AppError> {
        Ok(self.comments.delete(id)?)
    }

    // ---- agent connection -----------------------------------------------------------------------

    /// Turns the agent connection of this folder on or off: while it is on, an agent can ask this window what the
    /// person is looking at. The switch is remembered first, in the app's own data (the programs that talk to the
    /// window read it there, and nothing in the project can change it); if that fails nothing changes. A single file
    /// (Simple mode) is no project, so it never connects.
    pub fn set_agent_connection(&self, enabled: bool) -> Result<AgentStatus, AppError> {
        if self.vault.is_shallow() || self.kind == WorkspaceKind::Notes {
            return Ok(self.agent.status());
        }
        self.lock_store()?.set_agent_connection(enabled)?;
        Ok(self.agent.set_enabled(enabled))
    }

    pub fn agent_status(&self) -> AgentStatus {
        self.agent.status()
    }

    /// What the window shows now, for an agent that asks.
    pub fn publish_agent_context(&self, context: AgentContext) {
        if !self.vault.is_shallow() && self.kind == WorkspaceKind::Project {
            self.agent.publish(context);
        }
    }

    /// The place an agent asked to be shown, if the window has not picked it up yet.
    pub fn take_agent_open(&self) -> Option<Opened> {
        self.agent.take_open()
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

    /// Remembers "now", the current HEAD and the uncommitted files as the last visit.
    fn record_visit(&self) {
        // the notes are not visited: there is nothing to come back to and compare with
        if self.kind == WorkspaceKind::Notes {
            return;
        }
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

    /// Remembers the last visit when the window closes. The unread markers' file is written too, if it can be
    /// soon: the thread that keeps it may be in the middle of a look at a big project, and a closing window must
    /// not wait for that. What is not written then is written when the workspace is closed on exit.
    pub fn save_last_visit(&self) {
        self.record_visit();
        self.unread.flush_within(CLOSE_WAIT);
    }

    /// Saves the last visit and stops every background task. Safe to call more than once.
    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // Agents are let go first, and the socket is free for the next window of this folder.
        self.agent.shutdown();
        // No flush of the unread markers here: stopping them writes the file, and sets the flag that cuts a
        // look at the disk short, which a flush queued behind it would not.
        self.record_visit();
        // The watcher goes first: what it reports last (a rename that moves tags) is still written.
        if let Ok(mut w) = self.watcher.lock() {
            if let Some(w) = w.take() {
                w.stop();
            }
        }
        self.unread.stop();
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
        let previous = self.replace(Some(Arc::clone(&workspace)));
        // From here on the new workspace is the one that may emit.
        self.emitting.store(id, Ordering::SeqCst);
        if let Some(previous) = previous {
            previous.close();
        }
        // After the previous folder let go: the same folder opened again takes over the agent connection's socket.
        workspace.activate();
        Ok(info)
    }

    /// Installs a workspace and returns the previous one.
    fn replace(&self, next: Option<Arc<Workspace>>) -> Option<Arc<Workspace>> {
        let mut slot = self.current.lock().ok()?;
        std::mem::replace(&mut *slot, next)
    }

    /// A picture, sound or video of the open folder, or the part of one a `Range` header asks for (see
    /// [`Workspace::read_asset`]).
    pub fn read_asset(&self, path: &str, range: Option<&str>) -> Result<Asset, AppError> {
        self.current()?.read_asset(path, range)
    }

    /// Remembers "now" as the last visit without closing anything (window close).
    pub fn save_last_visit(&self) {
        if let Ok(ws) = self.current() {
            ws.save_last_visit();
        }
    }

    /// Remembers where the window is with the open project (none is open on the welcome screen).
    pub fn save_window(&self, window: &WindowState) {
        if let Ok(ws) = self.current() {
            // Best effort, like the rest of the window's memory: a failed write must not bother anyone.
            let _ = ws.save_window(window);
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

    const NOTHING: [&str; 0] = [];
    use crate::watch::ChangeKind;
    use std::sync::atomic::AtomicUsize;
    use std::thread;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct Recorder {
        fs: Mutex<Vec<FsBatch>>,
        tree: AtomicUsize,
        meta: AtomicUsize,
        image_refs: AtomicUsize,
        unread: Mutex<Vec<Vec<String>>>,
        agent: Mutex<Vec<AgentStatus>>,
        agent_opens: AtomicUsize,
        comments: AtomicUsize,
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
        fn image_refs_changed(&self) {
            self.image_refs.fetch_add(1, Ordering::SeqCst);
        }
        fn unread_changed(&self, unread: &[String]) {
            self.unread.lock().expect("lock").push(unread.to_vec());
        }
        fn agent_status(&self, status: &AgentStatus) {
            self.agent.lock().expect("lock").push(status.clone());
        }
        fn agent_open(&self) {
            self.agent_opens.fetch_add(1, Ordering::SeqCst);
        }
        fn comments_changed(&self) {
            self.comments.fetch_add(1, Ordering::SeqCst);
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
    fn the_images_documents_use_follow_the_files_live() {
        let fx = Fx::new();
        write(
            fx.project.path(),
            "README.md",
            "![logo](docs/assets/logo.png)\n",
        );
        write(fx.project.path(), "docs/guide.md", "No images yet.\n");
        let ws = fx.open();
        fx.indexed();
        wait_until("the build to find the logo", || {
            ws.referenced_images() == ["docs/assets/logo.png"]
        });
        assert!(
            fx.rec.image_refs.load(Ordering::SeqCst) >= 1,
            "the UI is told"
        );

        // Another tool edits a document: only that document is read again.
        let told = fx.rec.image_refs.load(Ordering::SeqCst);
        write(
            fx.project.path(),
            "docs/guide.md",
            "![s](../site/shot.png)\n",
        );
        wait_until("the edit to be followed", || {
            ws.referenced_images() == ["docs/assets/logo.png", "site/shot.png"]
        });
        wait_until("the UI to be told", || {
            fx.rec.image_refs.load(Ordering::SeqCst) > told
        });

        std::fs::remove_file(fx.project.path().join("README.md")).expect("rm");
        wait_until("the removal to be followed", || {
            ws.referenced_images() == ["site/shot.png"]
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
        write(
            fx.project.path(),
            "a.md",
            "# Main\n\n![m](assets/main.png)\n",
        );
        commit_all(&repo, "init");
        let wt_parent = tempfile::tempdir().expect("tempdir");
        let wt_path = wt_parent.path().join("feature-x");
        repo.worktree("feature-x", &wt_path, None)
            .expect("worktree");
        write(
            &wt_path,
            "plan.md",
            "# Plan in worktree\n\nwtuniqueword\n\n![w](shots/w.png)\n",
        );

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
        // Images are resolved inside the root of the document that uses them (the worktree is a checkout
        // of the same commit, so it has `a.md` too).
        wait_until("the images of both roots", || {
            ws.referenced_images()
                == [
                    "@wt/feature-x/assets/main.png",
                    "@wt/feature-x/shots/w.png",
                    "assets/main.png",
                ]
        });

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

    /// The folder's own files are not the person's choice: a repository can carry a `.trailmark/` folder with any
    /// content, and opening it must not share the person's reading with an agent.
    #[test]
    fn a_project_that_says_the_agent_connection_is_on_does_not_turn_it_on() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        write(
            fx.project.path(),
            ".trailmark/local/session.json",
            r#"{"agentConnection": true}"#,
        );
        write(
            fx.project.path(),
            ".trailmark/local/agent-connection.json",
            r#"{"enabled": true}"#,
        );
        let ws = fx.open();
        assert!(!ws.agent_wanted, "nothing in the project turns it on");
        ws.activate();
        let status = ws.agent_status();
        assert!(!status.enabled && !status.listening, "{status:?}");
        assert!(
            !fx.project
                .path()
                .join(".trailmark/local/agent.json")
                .exists(),
            "no context is written"
        );
        ws.close();
    }

    #[test]
    fn the_agent_connection_is_the_persons_choice_and_is_remembered_by_the_app() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        let ws = fx.open();
        assert!(!ws.agent_wanted);
        let status = ws.set_agent_connection(true).expect("on");
        assert!(status.enabled, "{status:?}");
        ws.close();

        // opened again, it is on without anything in the project saying so
        let ws = fx.open();
        assert!(ws.agent_wanted, "remembered in the app's data");
        assert!(!fx.project.path().join(".trailmark").exists());
        assert!(!ws.set_agent_connection(false).expect("off").enabled);
        ws.close();
        let ws = fx.open();
        assert!(!ws.agent_wanted);
        ws.close();
    }

    /// Whether the app kept an agent connection switch anywhere in `dir`.
    fn remembered(dir: &Path) -> bool {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry.file_name() == "agent-connection.json" || remembered(&entry.path())
            })
        })
    }

    #[test]
    fn a_single_file_never_connects_and_remembers_nothing() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        let ws = fx.open_path(&fx.project.path().join("a.md"));
        let status = ws.set_agent_connection(true).expect("answers");
        assert!(!status.enabled);
        ws.close();
        assert!(!remembered(fx.app.path()));
    }

    impl Fx {
        fn open_notes(&self) -> Workspace {
            let sink: Arc<dyn EventSink> = self.rec.clone();
            Workspace::open_notes(self.project.path(), self.app.path(), None, &sink)
                .expect("open notes")
        }
    }

    /// The notes folder is a vault root and nothing else: no git (not even in a repository), no unread markers, no
    /// last visit, no recent folder, never an agent connection and no `agent.json`.
    #[test]
    fn the_notes_are_a_workspace_with_no_git_no_unread_no_visits_no_recents_and_no_agent() {
        let fx = Fx::new();
        let repo = init_repo(fx.project.path());
        write(fx.project.path(), "a.md", "# A\n");
        write(fx.project.path(), "b.md", "# B\n");
        commit_all(&repo, "init");
        let ws = fx.open_notes();
        assert_eq!(ws.info().kind, WorkspaceKind::Notes);
        assert_eq!(ws.info().mode, VaultMode::Ide);
        assert!(
            !ws.git_snapshot().is_repo,
            "the folder is inside a repository, the notes are not"
        );
        assert_eq!(ws.notes().expect("notes").list().notes.len(), 2);

        // somebody else changes a note: nothing is unread (the markers have no worker, so nothing can ever be)
        write(
            fx.project.path(),
            "a.md",
            "# A, changed by another program\n",
        );
        write(fx.project.path(), "c.md", "# C\n");
        assert_eq!(ws.unread(), Vec::<String>::new());
        ws.mark_unread("a.md").expect("answers");
        assert_eq!(ws.unread(), Vec::<String>::new());

        // no agent
        let status = ws.set_agent_connection(true).expect("answers");
        assert!(!status.enabled);
        ws.publish_agent_context(AgentContext::default());
        ws.activate();
        ws.close();
        assert!(!remembered(fx.app.path()));
        assert!(!fx.project.path().join(".trailmark").join("local").exists());
        assert!(!fx
            .project
            .path()
            .join(".trailmark")
            .join("local")
            .join("agent.json")
            .exists());

        // no recent folder, no last visit; the settings of the window are the computer's, not the notes'
        assert_eq!(RecentStore::new(fx.app.path()).load_existing(), Vec::new());
        let again = fx.open_notes();
        assert_eq!(again.since_last_visit().expect("since").since_ms, None);
        assert!(!fx
            .project
            .path()
            .join(".trailmark")
            .join("last_visit.json")
            .exists());
        let store = StateStore::open_in_app_data(fx.app.path(), fx.project.path());
        assert!(store.load_last_visit().is_none());
        assert_eq!(store.location(), StateLocation::AppData);
        assert!(store.agent_path().is_none());
        again.close();
    }

    /// A notes folder chosen inside a project is the person's private notes: the search the agent connection answers from
    /// (the index of the open project) holds none of their text.
    #[test]
    fn a_notes_folder_inside_the_project_is_not_in_its_index() {
        let fx = Fx::new();
        write(fx.project.path(), "README.md", "# Home\n\nlighthouseword\n");
        write(
            fx.project.path(),
            "my-notes/idea.md",
            "# Idea\n\nlighthouseword and a pelicanword\n",
        );
        let notes = dunce::canonicalize(fx.project.path())
            .expect("canonical")
            .join("my-notes");
        crate::session::AppSettingsStore::new(fx.app.path())
            .set_notes_folder(&notes.display().to_string())
            .expect("choose the notes folder");
        let ws = fx.open();
        fx.indexed();
        let hits = ws.search("lighthouseword", None);
        assert_eq!(
            hits.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(),
            ["README.md"]
        );
        assert_eq!(ws.search("pelicanword", None).len(), 0);
        ws.close();
    }

    #[test]
    fn a_notes_workspace_needs_a_folder_and_a_project_has_no_notes() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        let sink: Arc<dyn EventSink> = fx.rec.clone();
        let file = fx.project.path().join("a.md");
        assert!(Workspace::open_notes(&file, fx.app.path(), None, &sink).is_err());
        assert!(
            Workspace::open_notes(&fx.project.path().join("nope"), fx.app.path(), None, &sink)
                .is_err()
        );
        let project = fx.open();
        assert_eq!(project.info().kind, WorkspaceKind::Project);
        assert_eq!(
            project.notes().err().map(|e| e.kind()),
            Some("notesNotOpen")
        );
        assert_eq!(project.notes_read_only(), None);
    }

    /// A folder that is not the notes folder, with a file someone sent and the image it shows.
    fn sent_files() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            "proposal.md",
            "# Proposal\n\n![the plan](pic.png)\n\n[next](other.md) and [web](https://example.com)\n",
        );
        write(dir.path(), "pic.png", "not really a picture");
        write(dir.path(), "other.md", "# Other\n");
        write(dir.path(), "todo.txt", "buy milk\n");
        dir
    }

    fn open_loose(ws: &Workspace, file: &Path) -> String {
        ws.notes()
            .expect("notes")
            .open_file(file, now_ms())
            .expect("open the file")
            .path
    }

    /// A loose file is read, edited, saved and served through its own paths, and nothing of the notes folder is involved.
    #[test]
    fn a_loose_file_is_read_edited_and_saved_through_paths_of_its_own() {
        let fx = Fx::new();
        let ws = fx.open_notes();
        let sent = sent_files();
        let path = open_loose(&ws, &sent.path().join("proposal.md"));
        let id = path
            .strip_prefix("@file/")
            .and_then(|rest| rest.split('/').next())
            .expect("an id")
            .to_owned();

        let view = ws.read_document(&path).expect("read");
        assert_eq!(view.path, path);
        assert!(view.html.contains("Proposal") && view.html.contains("pic.png"));
        let source = ws.document_source(&path).expect("source");
        assert_eq!(source.path, path);
        assert!(source.text.starts_with("# Proposal"));

        // the image next to it is served, nothing else of its folder is
        let image = ws
            .read_asset(&format!("@file/{id}/pic.png"), None)
            .expect("image");
        assert_eq!(image.mime, "image/png");
        assert!(ws
            .read_asset(&format!("@file/{id}/sub/pic.png"), None)
            .is_err());
        assert!(ws
            .read_asset(&format!("@file/{id}/../pic.png"), None)
            .is_err());
        assert!(
            ws.read_asset(&path, None).is_err(),
            "the document is not an image"
        );
        assert!(ws.read_document(&format!("@file/{id}/pic.png")).is_err());
        assert!(ws.read_document("@file/ffffffff/proposal.md").is_err());

        // saving writes the file only when asked, and nothing else
        let outcome = ws
            .save_document(&path, "# Proposal\nedited\n", &source.hash)
            .expect("save");
        assert!(matches!(outcome, SaveOutcome::Saved { .. }));
        assert_eq!(
            std::fs::read_to_string(sent.path().join("proposal.md")).expect("read"),
            "# Proposal\nedited\n"
        );
        let in_notes: Vec<String> = std::fs::read_dir(fx.project.path())
            .expect("ls")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .filter(|name| name != ".trailmark")
            .collect();
        assert_eq!(in_notes, Vec::<String>::new(), "no file made in the notes");
        // a save based on a version that is not the file's any more writes nothing
        let stale = ws
            .save_document(&path, "# Mine\n", &source.hash)
            .expect("save");
        assert!(matches!(stale, SaveOutcome::Conflict { .. }));

        // images cannot be added next to a file that is not ours
        assert!(ws.write_asset(&path, "x.png", &[1, 2, 3]).is_err());
        assert!(!sent.path().join("assets").exists());

        // what its links lead to
        assert!(matches!(
            ws.resolve_link(&path, "other.md"),
            LinkTarget::Missing { .. }
        ));
        assert!(matches!(
            ws.resolve_link(&path, "https://example.com"),
            LinkTarget::External { .. }
        ));
        assert!(matches!(
            ws.resolve_link(&path, "#top"),
            LinkTarget::Anchor { .. }
        ));

        // the places the system is handed
        let file = dunce::canonicalize(sent.path().join("proposal.md")).expect("canonical");
        assert_eq!(
            ws.absolute_path(&path).expect("abs"),
            file.display().to_string()
        );
        assert_eq!(ws.external_target(&path).expect("external"), file);
        assert_eq!(ws.reveal_target(&path).expect("reveal"), file);
        assert_eq!(ws.editor_target(&path).expect("editor"), file);
        ws.close();
    }

    #[test]
    fn a_text_file_opens_for_editing_in_the_notes_and_stays_read_only_in_a_project() {
        let fx = Fx::new();
        let ws = fx.open_notes();
        let sent = sent_files();
        let path = open_loose(&ws, &sent.path().join("todo.txt"));
        let source = ws.document_source(&path).expect("a text file is edited");
        assert_eq!(source.text, "buy milk\n");
        ws.save_document(&path, "buy milk\nand eggs\n", &source.hash)
            .expect("saved");
        assert_eq!(
            std::fs::read_to_string(sent.path().join("todo.txt")).expect("read"),
            "buy milk\nand eggs\n"
        );
        ws.close();

        let project = Fx::new();
        write(project.project.path(), "todo.txt", "buy milk\n");
        let ws = project.open();
        assert_eq!(
            ws.document_source("todo.txt").err().map(|e| e.kind()),
            Some("notMarkdown")
        );
        ws.close();
    }

    #[test]
    fn a_loose_file_that_is_gone_is_written_again_by_the_create_that_a_project_uses_for_new_files()
    {
        let fx = Fx::new();
        let ws = fx.open_notes();
        let sent = sent_files();
        let file = sent.path().join("proposal.md");
        let path = open_loose(&ws, &file);
        std::fs::remove_file(&file).expect("remove");
        let created = ws
            .create_document(&path, "# Back again\n")
            .expect("written again");
        assert_eq!(created.path, path);
        assert_eq!(
            std::fs::read_to_string(&file).expect("read"),
            "# Back again\n"
        );
        assert!(ws.create_document(&path, "# Over it\n").is_err());
        ws.close();
    }

    #[test]
    fn a_file_is_in_the_project_only_when_the_open_folder_holds_it() {
        let fx = Fx::new();
        write(fx.project.path(), "docs/a.md", "# A\n");
        let ws = fx.open();
        let inside = fx.project.path().join("docs").join("a.md");
        assert_eq!(ws.locate_in_project(&inside).as_deref(), Some("docs/a.md"));
        assert_eq!(ws.locate_in_project(&fx.project.path().join("docs")), None);
        let sent = sent_files();
        assert_eq!(ws.locate_in_project(&sent.path().join("proposal.md")), None);
        assert_eq!(
            ws.locate_in_project(&fx.project.path().join("gone.md")),
            None
        );
        ws.close();
        // the Notes window has no project
        let notes_window = Fx::new();
        let notes = notes_window.open_notes();
        write(notes_window.project.path(), "n.md", "# N\n");
        assert_eq!(
            notes.locate_in_project(&notes_window.project.path().join("n.md")),
            None
        );
        notes.close();
    }

    #[test]
    fn a_new_note_is_an_empty_file_at_once_and_its_first_text_is_saved_over_it() {
        let fx = Fx::new();
        let ws = fx.open_notes();
        let created = ws
            .notes()
            .expect("notes")
            .create_new(notes::Moment::now(), now_ms())
            .expect("a new note");
        let name = created.path;
        // the file is there, empty, and in the list as a note with no title
        assert_eq!(
            std::fs::read(fx.project.path().join(&name)).expect("read"),
            b""
        );
        let listed = ws.notes().expect("notes").list();
        assert_eq!(
            listed
                .notes
                .iter()
                .map(|n| (n.path.as_str(), n.title.as_str()))
                .collect::<Vec<_>>(),
            [(name.as_str(), "")]
        );
        // the editor opens it like any document
        let view = ws.read_document(&name).expect("an empty document");
        assert_eq!(
            (view.kind, view.html.trim()),
            (crate::render::DocKind::Markdown, "")
        );
        let source = ws.document_source(&name).expect("an empty source");
        assert_eq!(
            (source.text.as_str(), source.hash.as_str()),
            ("", created.hash.as_str())
        );

        // blank text is saved as it is: nothing is skipped and nothing is removed
        let NoteSaved::Saved { hash, .. } = ws
            .save_note(&name, "  ", &created.hash)
            .expect("blank text")
        else {
            panic!("saved");
        };
        let NoteSaved::Saved { hash, path, .. } =
            ws.save_note(&name, "# First", &hash).expect("saved")
        else {
            panic!("saved");
        };
        assert_eq!(path, name);
        assert_eq!(ws.document_source(&name).expect("source").hash, hash);
        assert_eq!(
            std::fs::read_to_string(fx.project.path().join(&name)).expect("read"),
            "# First"
        );
    }

    #[test]
    fn the_notes_of_a_second_window_are_read_only() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        let first = fx.open_notes();
        assert_eq!(first.notes_read_only(), None);
        let second = fx.open_notes();
        assert_eq!(second.notes_read_only(), Some(ReadOnlyReason::Locked));
        let hash = second.document_source("a.md").expect("source").hash;
        for refused in [
            second
                .save_document("a.md", "# Changed", &hash)
                .err()
                .map(|e| e.kind()),
            second.create_document("b.md", "x").err().map(|e| e.kind()),
            second
                .save_note("a.md", "# Changed", &hash)
                .err()
                .map(|e| e.kind()),
            second
                .delete_entries(&["a.md".to_owned()])
                .err()
                .map(|e| e.kind()),
        ] {
            assert_eq!(refused, Some("notesLocked"));
        }
        assert_eq!(
            std::fs::read_to_string(fx.project.path().join("a.md")).expect("read"),
            "# A\n"
        );
        // it still reads
        assert_eq!(
            second.read_document("a.md").expect("read").title.as_deref(),
            Some("A")
        );
    }

    fn new_comment(path: &str, quote: &str, text: &str) -> NewComment {
        NewComment {
            path: path.to_owned(),
            quote: quote.to_owned(),
            text: text.to_owned(),
            heading_path: Vec::new(),
            line: 0,
            end_line: 0,
        }
    }

    #[test]
    fn comments_are_kept_with_the_local_state_follow_their_document_and_tell_the_ui() {
        let fx = Fx::new();
        write(
            fx.project.path(),
            "docs/guide.md",
            "# Guide

The zebra crosses.
",
        );
        let ws = fx.open();
        let comment = ws
            .add_comment(new_comment(
                "docs/guide.md",
                "zebra crosses",
                "Which zebra?",
            ))
            .expect("a comment");
        assert_eq!(
            (comment.line, comment.heading_path.clone()),
            (3, vec!["Guide".to_owned()])
        );
        assert_eq!(fx.rec.comments.load(Ordering::SeqCst), 1);
        // the state is in the app's data folder until the project is initialized
        let before = ws.lock_store().expect("store").comments_path();
        assert!(before.starts_with(fx.app.path()), "{before:?}");
        assert!(before.is_file());

        // a rename in the Explorer takes the comment along, and the UI is told
        let rename = [Move {
            from: "docs/guide.md".to_owned(),
            to: "docs/manual.md".to_owned(),
        }];
        let preview = ws.preview_move(&rename, false, &[]).expect("preview");
        ws.apply_move(preview.plan_id, false, &[]).expect("apply");
        let listed = ws.list_comments();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].path, "docs/manual.md");
        assert_eq!(listed[0].state, crate::comments::PassageState::Anchored);
        assert_eq!(fx.rec.comments.load(Ordering::SeqCst), 2);

        // Initialize moves the comments into the project's local folder, and the next ones go there
        ws.init_project_folder().expect("init");
        let local = fx.project.path().join(".trailmark/local/comments.json");
        assert!(local.is_file(), "carried over");
        ws.set_comment_done(&comment.id, true).expect("done");
        assert!(std::fs::read_to_string(&local)
            .expect("read")
            .contains("person"));
        assert_eq!(ws.list_comments().len(), 1);
        ws.delete_comment(&comment.id).expect("deleted");
        assert!(ws.list_comments().is_empty(), "no comments left");
        ws.close();
    }

    #[test]
    fn a_single_file_takes_no_comments() {
        let fx = Fx::new();
        write(
            fx.project.path(),
            "a.md",
            "# A

words
",
        );
        let ws = fx.open_path(&fx.project.path().join("a.md"));
        let error = ws
            .add_comment(new_comment("a.md", "words", "comment"))
            .expect_err("no project");
        assert_eq!(error.kind(), "commentSingleFile");
        ws.close();
    }

    #[test]
    fn the_window_is_remembered_with_a_folder_but_not_with_a_single_file() {
        let window = WindowState {
            x: 10,
            y: 20,
            width: 1280,
            height: 800,
            scale: 1.5,
            maximized: true,
        };
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "x");
        let ws = fx.open();
        ws.save_window(&window).expect("save");
        ws.init_project_folder().expect("init");
        ws.close();
        // it moved into the project's own (private) state with everything else
        let store = StateStore::open(fx.app.path(), fx.project.path());
        assert_eq!(store.location(), StateLocation::Project);
        assert_eq!(store.load_window(), Some(window));
        assert!(fx
            .project
            .path()
            .join(".trailmark/local/window.json")
            .is_file());

        // a single file shares the app-wide window instead: nothing is kept for its folder
        let single = Fx::new();
        write(single.project.path(), "note.md", "x");
        let ws = single.open_path(&single.project.path().join("note.md"));
        ws.save_window(&window).expect("save");
        ws.close();
        let store = StateStore::open(single.app.path(), single.project.path());
        assert_eq!(store.load_window(), None);
        assert!(
            !store.dir().join("window.json").exists(),
            "no entry of its own"
        );
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
        // the UI is told after the edit is read in, not at the same moment
        wait_until("the UI to be told", || {
            fx.rec.meta.load(Ordering::SeqCst) > told
        });
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
        // The index build gives the watcher its first look at every file. A rename made before it is done can be
        // swallowed (the build sees the new name already and the watcher takes it for known), so the test waits.
        fx.indexed();
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

    /// What the UI heard last about the unread documents.
    fn unread_heard(rec: &Recorder) -> Option<Vec<String>> {
        rec.unread.lock().expect("lock").last().cloned()
    }

    #[test]
    fn documents_changed_by_others_become_unread_live_and_the_editor_reads_its_own() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        write(fx.project.path(), "b.md", "# B\n");
        let ws = fx.open();
        ws.unread.flush();
        assert!(ws.unread().is_empty(), "the first visit reads everything");

        // an agent writes a new document: unread, and the UI is told
        write_until_reported(&fx.rec, fx.project.path(), "c.md", "the new document");
        wait_until("c.md to be unread", || ws.unread() == ["c.md"]);
        assert_eq!(unread_heard(&fx.rec), Some(vec!["c.md".to_owned()]));

        // an agent changes b.md
        write(fx.project.path(), "b.md", "# B by an agent\n");
        wait_until("b.md to be unread", || ws.unread() == ["b.md", "c.md"]);

        // the person edits b.md and saves: read. A new file of theirs is read too.
        let source = ws.document_source("b.md").expect("source");
        ws.save_document("b.md", "# B merged\n", &source.hash)
            .expect("save");
        ws.create_document("mine.md", "# Mine\n").expect("create");
        assert_eq!(ws.unread(), ["c.md"]);

        // the watcher reports those writes too (they are the app's own), and changes nothing: an agent's
        // later write is seen after them, so once it shows the earlier reports have been dealt with
        write(fx.project.path(), "d.md", "# D\n");
        wait_until("d.md to be unread", || ws.unread() == ["c.md", "d.md"]);

        ws.mark_read("c.md").expect("mark");
        ws.mark_unread("a.md").expect("mark");
        assert_eq!(ws.unread(), ["a.md", "d.md"]);
        ws.mark_folder_read("").expect("folder");
        assert_eq!(ws.unread(), NOTHING);
        ws.mark_all_read();
        assert_eq!(unread_heard(&fx.rec), Some(Vec::new()));
        assert!(ws.mark_read("../outside.md").is_err());
        ws.close();
    }

    #[test]
    fn what_changed_while_the_folder_was_closed_is_unread_when_it_opens_again() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        write(fx.project.path(), "b.md", "# B\n");
        let ws = fx.open();
        ws.unread.flush(); // the first look at the folder is done
        ws.close();

        write(fx.project.path(), "b.md", "# B changed\n");
        write(fx.project.path(), "c.md", "# C\n");
        let ws = fx.open();
        ws.unread.flush();
        assert_eq!(ws.unread(), ["b.md", "c.md"]);
        ws.mark_all_read();
        ws.close();

        // what was marked read before closing stays read
        let ws = fx.open();
        ws.unread.flush();
        assert_eq!(ws.unread(), NOTHING);
        ws.close();
    }

    #[test]
    fn moves_in_the_explorer_keep_what_was_read_even_where_links_are_rewritten() {
        let fx = Fx::new();
        write(
            fx.project.path(),
            "README.md",
            "# Home\n\nSee [guide](docs/guide.md).\n",
        );
        write(fx.project.path(), "docs/guide.md", "# Guide\n");
        write(fx.project.path(), "docs/extra/api.md", "# API\n");
        let ws = fx.open();
        ws.unread.flush();
        fx.indexed();
        ws.mark_unread("docs/extra/api.md").expect("mark");

        let rename = [Move {
            from: "docs/guide.md".to_owned(),
            to: "docs/manual.md".to_owned(),
        }];
        let preview = ws.preview_move(&rename, true, &[]).expect("preview");
        ws.apply_move(preview.plan_id, true, &[]).expect("apply");
        let folder = [Move {
            from: "docs".to_owned(),
            to: "manual".to_owned(),
        }];
        let preview = ws.preview_move(&folder, true, &[]).expect("preview");
        ws.apply_move(preview.plan_id, true, &[]).expect("apply");
        let readme = std::fs::read_to_string(fx.project.path().join("README.md")).expect("readme");
        assert!(
            readme.contains("(manual/manual.md)"),
            "the link was rewritten"
        );

        // the rewritten README and the moved documents are as read as they were; the unread one still is
        assert_eq!(ws.unread(), ["manual/extra/api.md"]);

        // the watcher reports all of it a moment later, and it changes nothing
        write(fx.project.path(), "later.md", "# Later\n");
        wait_until("the watcher to catch up", || {
            ws.unread() == ["later.md", "manual/extra/api.md"]
        });
        ws.unread.flush();
        let saved: BTreeMap<String, String> = serde_json::from_slice(
            &std::fs::read(ws.lock_store().expect("store").read_path()).expect("read.json"),
        )
        .expect("json");
        assert_eq!(
            saved.keys().map(String::as_str).collect::<Vec<_>>(),
            ["README.md", "manual/manual.md"]
        );
        ws.close();
    }

    #[test]
    fn the_read_state_is_written_into_the_project_when_it_is_initialized() {
        let fx = Fx::new();
        write(fx.project.path(), "a.md", "# A\n");
        let ws = fx.open();
        ws.unread.flush();
        let before = ws.lock_store().expect("store").read_path();
        assert!(before.starts_with(fx.app.path()) && before.is_file());

        ws.init_project_folder().expect("init");
        // like the session, the last visit and the window, it is copied: the app's data folder keeps its file
        assert!(before.is_file());
        ws.mark_unread("a.md").expect("mark");
        ws.unread.flush();
        let local = fx.project.path().join(".trailmark/local/read.json");
        let saved: BTreeMap<String, String> =
            serde_json::from_slice(&std::fs::read(&local).expect("read.json in the project"))
                .expect("json");
        assert!(
            saved.is_empty(),
            "the state written there is the current one"
        );
        ws.close();
    }

    #[test]
    fn a_single_file_has_no_unread_markers() {
        let fx = Fx::new();
        write(fx.project.path(), "notes/todo.md", "# Todo\n");
        write(fx.project.path(), "notes/other.md", "# Other\n");
        let ws = fx.open_path(&fx.project.path().join("notes").join("todo.md"));
        ws.unread.flush();
        assert_eq!(ws.unread(), NOTHING);
        ws.mark_read("todo.md").expect("mark");
        ws.mark_unread("todo.md").expect("mark");
        ws.mark_all_read();
        assert_eq!(ws.unread(), NOTHING);
        assert!(fx.rec.unread.lock().expect("lock").is_empty());
        let read_path = ws.lock_store().expect("store").read_path();
        ws.close();
        assert!(!read_path.exists(), "no state is kept for a single file");
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
        let ws = Workspace::open_with(
            fx.project.path(),
            fx.app.path(),
            None,
            &sink,
            |_, _, _| {
                Err(
                    crate::watch::WatchError::Notify("no change notification here".to_owned())
                        .into(),
                )
            },
            WorkspaceKind::Project,
        )
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

        let a = ws.read_asset("img/a.png", None).expect("png");
        assert_eq!((a.mime, a.bytes.as_slice()), ("image/png", &b"PNGDATA"[..]));
        assert_eq!((a.total, a.part), (7, Part::Whole));
        assert_eq!(
            ws.read_asset("photo.JPG", None).expect("jpg").mime,
            "image/jpeg"
        );

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
            assert!(
                ws.read_asset(bad, None).is_err(),
                "{bad} must not be served"
            );
        }
        assert_eq!(
            ws.read_asset("notes.md", None)
                .expect_err("not an image")
                .kind(),
            "invalidPath"
        );
        ws.close();
    }

    #[test]
    fn simple_mode_serves_only_the_files_next_to_the_file() {
        let fx = Fx::new();
        write(fx.project.path(), "notes/todo.md", "# Todo\n");
        write(fx.project.path(), "notes/a.png", "PNGDATA");
        write(fx.project.path(), "notes/clip.mp4", "VIDEODATA");
        write(fx.project.path(), "notes/img/b.png", "PNGDATA");
        write(fx.project.path(), "notes/media/c.mp4", "VIDEODATA");
        let ws = fx.open_path(&fx.project.path().join("notes").join("todo.md"));
        assert!(ws.read_asset("a.png", None).is_ok());
        assert!(ws.read_asset("clip.mp4", Some("bytes=0-4")).is_ok());
        for nested in ["img/b.png", "media/c.mp4"] {
            assert_eq!(
                ws.read_asset(nested, None).expect_err("nested").kind(),
                "pathEscape",
                "{nested}"
            );
        }
        ws.close();
    }

    /// A sound or video is served in parts, so that a long film is never held whole in memory.
    #[test]
    fn a_video_is_served_in_parts_and_only_from_the_folder() {
        let fx = Fx::new();
        let bytes: Vec<u8> = (0..=255_u8).cycle().take(6 * 1024 * 1024).collect();
        std::fs::create_dir_all(fx.project.path().join("clips")).expect("mkdir");
        std::fs::write(fx.project.path().join("clips/demo.mp4"), &bytes).expect("write");
        std::fs::write(fx.project.path().join("tone.WAV"), b"0123456789").expect("write");
        let outside = tempfile::tempdir().expect("tempdir");
        write(outside.path(), "secret.mp4", "SECRET");
        let ws = fx.open();

        // the first part, as a player asks for it
        let first = ws
            .read_asset("clips/demo.mp4", Some("bytes=0-"))
            .expect("part");
        assert_eq!(first.mime, "video/mp4");
        assert_eq!(first.total, 6 * 1024 * 1024);
        let chunk = usize::try_from(media::MAX_CHUNK).expect("fits");
        assert_eq!(first.bytes.len(), chunk);
        assert_eq!(
            first.part,
            Part::Range {
                start: 0,
                end: media::MAX_CHUNK - 1
            }
        );
        assert_eq!(first.bytes, bytes[..chunk]);

        // a seek into the middle, and the end of the file
        let middle = ws
            .read_asset("clips/demo.mp4", Some("bytes=1000-1999"))
            .expect("middle");
        assert_eq!(middle.bytes, bytes[1000..2000]);
        assert_eq!(
            middle.part,
            Part::Range {
                start: 1000,
                end: 1999
            }
        );
        let tail = ws
            .read_asset("clips/demo.mp4", Some("bytes=-100"))
            .expect("tail");
        assert_eq!(tail.bytes, bytes[bytes.len() - 100..]);

        // past the end: nothing is carried, and the size is still said
        let past = ws
            .read_asset("clips/demo.mp4", Some("bytes=999999999-"))
            .expect("past");
        assert_eq!(
            (past.part, past.bytes.len(), past.total),
            (Part::Unsatisfiable, 0, 6 * 1024 * 1024)
        );

        // a small file asked for without a range comes whole; a sound is cut the same way
        let whole = ws.read_asset("tone.WAV", None).expect("whole");
        assert_eq!(
            (whole.mime, whole.part, whole.bytes.as_slice()),
            ("audio/wav", Part::Whole, &b"0123456789"[..])
        );
        let cut = ws.read_asset("tone.WAV", Some("bytes=2-4")).expect("cut");
        assert_eq!(cut.bytes, b"234");

        // nothing outside the folder, and nothing that is not a picture, sound or video
        let outside_name = outside.path().file_name().expect("name").to_string_lossy();
        let escape = format!("../{outside_name}/secret.mp4");
        for bad in [
            escape.as_str(),
            "clips",
            "missing.mp4",
            "/etc/x.mp4",
            "C:/x.mp4",
        ] {
            assert!(
                ws.read_asset(bad, Some("bytes=0-")).is_err(),
                "{bad} must not be served"
            );
        }
        ws.close();
    }

    /// A big film asked for without a range starts with its first part; a big picture is not served at all.
    #[test]
    fn a_file_over_the_limit_is_cut_when_it_streams_and_refused_when_it_is_a_picture() {
        let fx = Fx::new();
        for name in ["film.mkv", "huge.png"] {
            let file = std::fs::File::create(fx.project.path().join(name)).expect("create");
            file.set_len(media::MAX_WHOLE + 1).expect("set_len");
        }
        let ws = fx.open();
        let film = ws.read_asset("film.mkv", None).expect("film");
        assert_eq!(film.total, media::MAX_WHOLE + 1);
        assert_eq!(
            film.part,
            Part::Range {
                start: 0,
                end: media::MAX_CHUNK - 1
            }
        );
        assert_eq!(
            ws.read_asset("huge.png", None).expect_err("too big").kind(),
            "notFound"
        );
        ws.close();
    }

    /// A link inside the folder (a symbolic link, or a junction on Windows) that leads out of it is not followed to a
    /// file elsewhere, for a folder link and, where the system allows one without privileges, for a file link.
    #[test]
    fn a_link_that_leaves_the_folder_does_not_serve_the_file_it_points_at() {
        let fx = Fx::new();
        let outside = tempfile::tempdir().expect("tempdir");
        write(outside.path(), "secret.mp4", "SECRET");
        write(outside.path(), "secret.png", "SECRET");
        crate::util::testing::dir_link(&fx.project.path().join("dir"), outside.path());
        #[cfg(unix)]
        let leaving = {
            std::os::unix::fs::symlink(
                outside.path().join("secret.mp4"),
                fx.project.path().join("link.mp4"),
            )
            .expect("symlink");
            ["dir/secret.png", "dir/secret.mp4", "link.mp4"]
        };
        #[cfg(not(unix))]
        let leaving = ["dir/secret.png", "dir/secret.mp4"];
        let ws = fx.open();
        for bad in leaving {
            assert_eq!(
                ws.read_asset(bad, Some("bytes=0-"))
                    .expect_err("outside")
                    .kind(),
                "pathEscape",
                "{bad}"
            );
            assert_eq!(
                ws.default_app_target(bad).expect_err("outside").kind(),
                "pathEscape",
                "{bad}"
            );
        }
        ws.close();
    }

    /// What is served is judged by the name of the file a link leads to, not by the name of the link: `a.png` that points
    /// at `.env` is not a picture. Where a file link cannot be made without privileges (Windows) the test has nothing to
    /// make a link with and says so by passing without one.
    #[test]
    fn a_link_named_like_a_picture_that_leads_to_another_file_serves_nothing() {
        let fx = Fx::new();
        write(fx.project.path(), ".env", "TOKEN=secret");
        write(fx.project.path(), "notes/key.txt", "SECRET");
        write(fx.project.path(), "real.png", "PNGDATA");
        write(fx.project.path(), "real.mp4", "VIDEODATA");
        let link = |from: &str, to: &str| -> bool {
            let (from, to) = (fx.project.path().join(from), fx.project.path().join(to));
            #[cfg(unix)]
            let made = std::os::unix::fs::symlink(&to, &from).is_ok();
            #[cfg(windows)]
            let made = std::os::windows::fs::symlink_file(&to, &from).is_ok();
            made
        };
        let made = [
            link("a.png", ".env"),
            link("clip.mp4", "notes/key.txt"),
            // a link between pictures is served as the picture it leads to
            link("cover.jpg", "real.png"),
            link("latest.mp4", "real.mp4"),
        ];
        let ws = fx.open();
        let linked = made.iter().all(|made| *made);
        // macOS and Linux always allow a symbolic link; Windows only with a privilege most accounts do not have
        #[cfg(unix)]
        assert!(linked, "a symbolic link could not be made");
        if linked {
            for bad in ["a.png", "clip.mp4"] {
                assert_eq!(
                    ws.read_asset(bad, Some("bytes=0-"))
                        .expect_err("not a picture")
                        .kind(),
                    "invalidPath",
                    "{bad}"
                );
            }
            let cover = ws.read_asset("cover.jpg", None).expect("a picture");
            assert_eq!(
                (cover.mime, cover.bytes.as_slice()),
                ("image/png", &b"PNGDATA"[..])
            );
            let latest = ws.read_asset("latest.mp4", None).expect("a video");
            assert_eq!(latest.mime, "video/mp4");
        }
        // whatever the system allows, the real files are served as what they are
        assert_eq!(
            ws.read_asset("real.png", None).expect("png").mime,
            "image/png"
        );
        assert!(ws.read_asset(".env", None).is_err());
        ws.close();
    }

    #[test]
    fn a_picture_sound_or_video_is_opened_as_such_and_never_read_as_text() {
        let fx = Fx::new();
        write(fx.project.path(), "logo.png", "\u{0}PNG not text");
        write(fx.project.path(), "demo.mp4", "\u{0}video");
        write(fx.project.path(), "song.flac", "\u{0}audio");
        write(fx.project.path(), "data.bin", "\u{0}binary");
        let ws = fx.open();
        for (name, kind) in [
            ("logo.png", render::DocKind::Image),
            ("demo.mp4", render::DocKind::Video),
            ("song.flac", render::DocKind::Audio),
            ("data.bin", render::DocKind::Binary),
        ] {
            let view = ws.read_document(name).expect(name);
            assert_eq!((view.kind, view.path.as_str()), (kind, name));
            assert!(view.html.is_empty() && view.modified_ms > 0 && view.size_bytes > 0);
        }
        assert!(ws.read_document("missing.png").is_err());
        assert!(ws.read_document("../outside.png").is_err());
        ws.close();
    }

    /// "Open with default app" takes any file of the folder, programs and scripts excepted.
    #[test]
    fn the_default_app_is_asked_to_open_files_of_the_folder_but_never_a_program() {
        let fx = Fx::new();
        for name in [
            "report.pdf",
            "clip.mkv",
            "model.stl",
            "sheet.xlsx",
            "a.md",
            "setup.exe",
            "run.bat",
            "tool.ps1",
            "app.jar",
            "link.lnk",
            "web.url",
            "script.py",
        ] {
            write(fx.project.path(), name, "x");
        }
        std::fs::create_dir(fx.project.path().join("dir")).expect("mkdir");
        let ws = fx.open();
        for ok in ["report.pdf", "clip.mkv", "model.stl", "sheet.xlsx", "a.md"] {
            assert!(ws.default_app_target(ok).expect(ok).is_file(), "{ok}");
        }
        for (bad, label) in [
            ("setup.exe", ".exe"),
            ("run.bat", ".bat"),
            ("tool.ps1", ".ps1"),
            ("app.jar", ".jar"),
            ("link.lnk", ".lnk"),
            ("web.url", ".url"),
            ("script.py", ".py"),
        ] {
            let error = ws.default_app_target(bad).expect_err(bad);
            assert_eq!(error.kind(), "blockedType", "{bad}");
            assert_eq!(error.details().subject.as_deref(), Some(label), "{bad}");
        }
        for bad in ["dir", "missing.pdf", "../outside.pdf", "/etc/passwd"] {
            assert!(ws.default_app_target(bad).is_err(), "{bad}");
        }
        assert_ne!(
            ws.default_app_target("dir").expect_err("dir").kind(),
            "blockedType"
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
        // what the UI words the refusal with is the file, not an English sentence
        assert_eq!(
            ws.external_target("run.exe")
                .expect_err("exe")
                .details()
                .subject
                .as_deref(),
            Some("run.exe")
        );

        // An editor command opens any file of the folder, but still only files of the folder.
        for ok in ["a.md", "main.py", "run.exe", "Makefile", "script.ps1"] {
            assert!(ws.editor_target(ok).expect(ok).is_file(), "{ok}");
        }
        for bad in [
            "dir",
            "missing.md",
            "../outside.md",
            "/etc/passwd",
            "C:\\x.rs",
        ] {
            assert!(ws.editor_target(bad).is_err(), "{bad} must not be passed");
        }
        assert_eq!(
            ws.editor_target("../outside.md")
                .expect_err("escape")
                .kind(),
            "pathEscape"
        );

        // Revealing is harmless for anything inside the folder, folders included.
        assert!(ws.reveal_target("dir").expect("dir").is_dir());
        assert!(ws.reveal_target("run.exe").is_ok());
        assert!(ws.reveal_target("../outside.md").is_err());
        assert!(ws.reveal_target("missing.md").is_err());
        ws.close();
    }

    /// macOS opens an executable without an extension in Terminal and runs it: a cloned repository's `gradlew` or
    /// `bin/setup` must not be started by "Open in external editor" with no editor command.
    #[cfg(unix)]
    #[test]
    fn a_program_without_an_extension_is_not_handed_to_the_system() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new();
        write(
            fx.project.path(),
            "gradlew",
            "#!/bin/sh
",
        );
        write(
            fx.project.path(),
            "bin/setup",
            "#!/bin/sh
",
        );
        write(
            fx.project.path(),
            "LICENSE",
            "text
",
        );
        write(
            fx.project.path(),
            "script.sh",
            "#!/bin/sh
",
        );
        let chmod = |rel: &str, mode: u32| {
            std::fs::set_permissions(
                fx.project.path().join(rel),
                std::fs::Permissions::from_mode(mode),
            )
            .expect("chmod");
        };
        chmod("gradlew", 0o755);
        chmod("bin/setup", 0o700);
        chmod("LICENSE", 0o644);
        chmod("script.sh", 0o755);
        let ws = fx.open();

        for bad in ["gradlew", "bin/setup", "script.sh"] {
            let error = ws.external_target(bad).expect_err(bad);
            assert_eq!(error.kind(), "invalidPath", "{bad}");
            assert_eq!(error.details().subject.as_deref(), Some(bad));
        }
        assert!(ws.external_target("LICENSE").expect("plain file").is_file());
        // the person's own editor command takes any file as an argument
        assert!(ws.editor_target("gradlew").is_ok());
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
    fn the_window_is_saved_with_whichever_folder_is_open() {
        let fx = HostFx::new();
        let window = WindowState {
            x: 1,
            y: 2,
            width: 900,
            height: 600,
            scale: 1.0,
            maximized: false,
        };
        // the welcome screen has no folder: nothing to save with, and nothing goes wrong
        fx.host.save_window(&window);
        fx.open(fx.a.path()).expect("open A");
        fx.host.save_window(&window);
        let moved = WindowState { x: 99, ..window };
        fx.open(fx.b.path()).expect("open B");
        fx.host.save_window(&moved);
        let stored = |dir: &Path| StateStore::open(fx.app.path(), dir).load_window();
        assert_eq!(stored(fx.a.path()), Some(window));
        assert_eq!(stored(fx.b.path()), Some(moved));
        fx.host.shutdown();
    }

    #[test]
    fn opening_a_folder_replaces_the_previous_one_and_closes_it() {
        let fx = HostFx::new();
        let info = fx.open(fx.a.path()).expect("open A");
        assert_eq!(info.root, root_of(&fx.a));
        assert!(fx.host.read_asset("only-in-a.png", None).is_ok());

        fx.open(fx.b.path()).expect("open B");
        assert_eq!(fx.current_root(), root_of(&fx.b));
        // The folder that was closed can no longer be read through the image protocol.
        assert_eq!(
            fx.host
                .read_asset("only-in-a.png", None)
                .expect_err("closed")
                .kind(),
            "notFound"
        );
        // A change in the old folder is not reported any more; one in the new folder is.
        write(fx.a.path(), "late-a.md", "x");
        write_until_reported(&fx.rec, fx.b.path(), "late-b.md", "B reports its change");
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
        write_until_reported(&fx.rec, fx.b.path(), "later.md", "B reports its change");
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
