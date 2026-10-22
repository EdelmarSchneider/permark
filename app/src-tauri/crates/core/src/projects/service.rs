//! `Projects`: the projects service of one `ProjectCore` (`projects.md` §1-§6). Owns the inputs of the membership
//! machine, the per-member resources (card, agent rules, ignore list), the map snapshot readers get, the undo token of
//! the last include. One lock (`Inner`) orders every change; disk discovery runs on the projects worker (the walk is
//! waited for there, never on a command); events, rescans and feed news go out after the lock is released.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, RwLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use super::card::{self, Card, CardChange, CardDto, CardPlace};
use super::fsm::{
    self, AgentChoice, Candidate, Event, Found, Inputs, Membership, MissingReason, Presence,
    ProjectMarker, RecEntry, RootAnswer, MEMBERSHIP_FILE,
};
use super::grace::{self, Came, FolderId, Left, Vanished};
use super::map::{within, AgentRules, AncestorRules, MemberInfo, ProjectMap};
use super::owners::{OwnerSlot, Owners, OwnersChange};
use super::record::{self, HiddenRule, RecordOp, RecordRead, RecordSkip, SkippedEntry};
use super::uid::ProjectUid;
use super::view::{
    CardTarget, IncludeOutcome, InnerLine, LocateRefusal, MemberState, MoveAsk, OfferSummary,
    ProjectEvent, ProjectPick, ProjectRow, ProjectsChanged, ProjectsView, RecordStatus,
    VocabularyEntry, VocabularyStatus, VocabularyView,
};
use super::vocabulary::{self, TagStatus, VocabKind, VocabRead, Vocabularies};
use crate::events::{CoreEvent, EventSink};
use crate::paths::{Root, VaultPath, CASE_INSENSITIVE};
use crate::settings::ProjectConfig;
use crate::state::{AppDirs, StateFile, StateStore, PERMARK_DIR};
use crate::tree::{Explorer, MemberRule, MemberSet};
use crate::watch::{ChangeKind, FsBatch, OwnWrites, WatchHub};
use crate::{ErrorKind, IpcError};

/// The agent's ignore file (gitignore syntax), as the agent plugin reads it.
const AI_IGNORE: &str = ".aiignore";
const BOM: &[u8] = b"\xEF\xBB\xBF";
const MAX_AI_IGNORE: u64 = 1024 * 1024;
/// Levels above the opened folder searched for a record listing it.
const MAX_ANCESTORS: usize = 32;
/// How long closing waits for the worker before leaving it to end on its own.
const STOP_WAIT: Duration = Duration::from_millis(200);
/// Ancestor rules are read again after this long (on an agent call).
const ANCESTORS_TTL: Duration = Duration::from_secs(5);
/// How long the worker waits for the first walk between looks at `stopped` (it never waits inside the walk).
const WALK_POLL: Duration = Duration::from_millis(20);
/// The worker walks itself when no walk started after this long (the tree thread could not start).
const WALK_START_WAIT: Duration = Duration::from_secs(1);

/// Disk touches of the projects service, counted per thread in tests (`projects.md` §13: a plain folder costs nothing
/// more after open).
pub(crate) mod probe {
    #[cfg(test)]
    thread_local! {
        static COUNT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    #[inline]
    pub fn hit() {
        #[cfg(test)]
        COUNT.with(|c| c.set(c.get() + 1));
    }

    #[cfg(test)]
    pub fn count() -> u64 {
        COUNT.with(std::cell::Cell::get)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn is_link(meta: &std::fs::Metadata) -> bool {
    crate::tree::is_link_meta(meta)
}

fn real_dir(path: &Path) -> bool {
    probe::hit();
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !is_link(&m))
}

fn read_record(dir: &Path) -> RecordRead {
    probe::hit();
    record::read(dir)
}

fn abs_of(root: &Path, path: &VaultPath) -> PathBuf {
    let mut abs = root.to_path_buf();
    abs.extend(path.segments());
    abs
}

/// Whether a member's 0.6 state was copied since it was last included (a project left out and included again is
/// copied again: only what the opened folder got since, `projects.md` §14 D50).
fn is_copied(inner: &Inner, member: &MemberInfo) -> bool {
    inner.migrated.iter().any(|u| u == member.uid.as_str())
}

/// Where `path` is now when one of `renames` (`(from, to)`, first that covers it wins) moved it or a folder above it.
fn renamed_path(path: &VaultPath, renames: &[(String, String)]) -> Option<VaultPath> {
    let key = path.key();
    renames.iter().find_map(|(from, to)| {
        if !within(&key, &crate::paths::key_of(from)) {
            return None;
        }
        let depth = from.split('/').count();
        let rest: Vec<&str> = path.as_str().split('/').skip(depth).collect();
        let now = if rest.is_empty() {
            to.clone()
        } else {
            format!("{to}/{}", rest.join("/"))
        };
        VaultPath::parse(&now).ok()
    })
}

fn folder_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

/// What a member brings while included (released at stop).
#[derive(Debug, Clone, Default)]
struct MemberRes {
    card: Card,
    place: Option<CardPlace>,
    /// Its own `.permark/config.toml` `ignore` list.
    ignore: Vec<String>,
    agent: AgentRules,
    /// Entries of its own (dormant) record.
    inner: Vec<(VaultPath, ProjectUid)>,
    has_record: bool,
    /// Its folder on disk (a move split across watch batches pairs by it; `grace.rs`).
    id: Option<FolderId>,
    /// Its own tag | category lists (local to it; `projects.md` §7.1).
    vocab: vocabulary::Vocab,
}

/// Reads what a member brings from its folder (cheap: a few small files).
fn load_member(dirs: &AppDirs, abs: &Path) -> MemberRes {
    probe::hit();
    let config = Root::open(abs)
        .map(|root| ProjectConfig::load(&root))
        .unwrap_or_default();
    let aiignore = crate::fsx::read_bytes(&abs.join(AI_IGNORE), MAX_AI_IGNORE)
        .map(|b| {
            String::from_utf8_lossy(&b)
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let (card, place) = card::file_of(dirs, abs).map_or((Card::default(), None), |file| {
        (file.read(), Some(file.place()))
    });
    let read = read_record(abs);
    let inner = read
        .record()
        .map(|r| {
            r.entries
                .iter()
                .map(|e| (e.path.clone(), e.uid.clone()))
                .collect()
        })
        .unwrap_or_default();
    MemberRes {
        card,
        place,
        ignore: config.ignore,
        agent: AgentRules {
            aiignore,
            deny_paths: config.agent_deny_paths,
            deny: config.agent_deny,
        },
        inner,
        has_record: !matches!(read, RecordRead::Absent),
        id: FolderId::of(abs),
        vocab: vocabulary::read(abs).vocab().cloned().unwrap_or_default(),
    }
}

/// The marker on disk at `abs` (`.git` folder | file; a real `.permark`).
fn marker_at(abs: &Path) -> Option<ProjectMarker> {
    if !real_dir(abs) {
        return None;
    }
    probe::hit();
    let git = std::fs::symlink_metadata(abs.join(".git")).is_ok();
    ProjectMarker::of(git, real_dir(&abs.join(PERMARK_DIR)))
}

/// Whether a segment of `path` below `root` is a link | junction (existing segments only).
fn through_link(root: &Path, path: &VaultPath) -> bool {
    through_link_memo(root, path, &mut HashMap::new())
}

/// [`through_link`] remembering the answer per folder for one pass (entries share their parents).
fn through_link_memo(root: &Path, path: &VaultPath, seen: &mut HashMap<PathBuf, bool>) -> bool {
    let mut abs = root.to_path_buf();
    for segment in path.segments() {
        abs.push(segment);
        let link = if let Some(link) = seen.get(&abs) {
            *link
        } else {
            probe::hit();
            let (link, exists) = match std::fs::symlink_metadata(&abs) {
                Ok(meta) => (is_link(&meta), true),
                Err(_) => (false, false),
            };
            seen.insert(abs.clone(), link);
            if !exists {
                return false;
            }
            link
        };
        if link {
            return true;
        }
    }
    false
}

fn gitignore_of(base: &Path, lines: &[String]) -> Option<Gitignore> {
    if lines.is_empty() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(base);
    let _ = builder.case_insensitive(CASE_INSENSITIVE);
    for line in lines {
        let _ = builder.add_line(None, line);
    }
    builder.build().ok().filter(|g| !g.is_empty())
}

/// The hard rules (`projects.md` §5.1 rules 2-3) for presence checks: `node_modules`, the root's `ignore` list, the
/// `ignore` lists of members.
struct Hard {
    root: Option<Gitignore>,
    /// Member key, its segment count, its `ignore` list.
    members: Vec<(String, usize, Gitignore)>,
}

impl Hard {
    /// Paths are given relative to each rule's folder (never absolute: a matcher asserts its root).
    fn hides(&self, path: &VaultPath) -> bool {
        if path
            .segments()
            .any(|s| crate::paths::fold(s) == crate::paths::fold("node_modules"))
        {
            return true;
        }
        if self.root.as_ref().is_some_and(|g| {
            g.matched_path_or_any_parents(Path::new(path.as_str()), true)
                .is_ignore()
        }) {
            return true;
        }
        let key = path.key();
        self.members.iter().any(|(member, depth, rules)| {
            key != *member && within(&key, member) && {
                let rel: Vec<&str> = path.segments().skip(*depth).collect();
                rules
                    .matched_path_or_any_parents(Path::new(&rel.join("/")), true)
                    .is_ignore()
            }
        })
    }
}

/// The undo of the last include (`projects.md` §2.4).
#[derive(Debug, Clone)]
struct IncludeUndo {
    token: u64,
    uids: Vec<ProjectUid>,
    created_record: bool,
    created_permark: bool,
    answer_before: RootAnswer,
    declined_before: Vec<String>,
}

/// What an undo leaves to the core: `.permark/` to take back when untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UndoPlan {
    pub undone: bool,
    pub created_permark: bool,
}

struct Inner {
    inputs: Inputs,
    record: RecordRead,
    /// Entries skipped for disk reasons (link | outside).
    disk_skipped: Vec<SkippedEntry>,
    res: HashMap<ProjectUid, MemberRes>,
    /// The opened folder's own card, read when first asked for (a plain folder never reads it).
    root_card: Option<(Card, CardPlace)>,
    /// The opened folder's own vocabulary file, read on first use (a plain folder never reads it before a view asks).
    vocab_root: Option<VocabRead>,
    /// The vocabularies as of map `seq` (rebuilt when the map changed).
    vocab: Option<(u64, Arc<Vocabularies>)>,
    member_set: MemberSet,
    ready: bool,
    seq: u64,
    /// Ids given up this session (never drawn again).
    removed: HashSet<ProjectUid>,
    undo: Option<IncludeUndo>,
    gone_told: HashSet<ProjectUid>,
    rejected: HashSet<(ProjectUid, String)>,
    ask: Option<MoveAsk>,
    ancestors: (Vec<AncestorRules>, Option<Instant>),
    /// The root's own `ignore` list (`.permark/config.toml`), read at open and when that file changes.
    root_ignore: Vec<String>,
    /// Uids whose 0.6 state was copied (`projects.md` §9.3): their state lives in their own files.
    migrated: Vec<String>,
    /// Members whose copy failed this session: tried again at the next start, not at every map change.
    copy_failed: HashSet<ProjectUid>,
    /// Members moved since the owners were last told (`(from, to)`): their profile state folder follows once the
    /// plugins wrote what they hold (`projects.md` §9.1).
    pending_moves: Vec<(VaultPath, VaultPath)>,
    /// Root commits of found repositories learned this session (path key -> commit; `None`: none to learn).
    root_commits: HashMap<String, Option<String>>,
    /// Path keys asked of the `git` plugin, answer not back yet (asked once per session while the lease lives).
    asked: HashSet<String>,
    /// Included entries whose folder vanished while open, held as here for a short window (`grace.rs`).
    vanished: BTreeMap<ProjectUid, Vanished>,
    /// New folders kept out of the found set while a window runs (tests look).
    #[cfg_attr(not(test), allow(dead_code))]
    waiting: usize,
    other: serde_json::Map<String, serde_json::Value>,
}

/// What a change leaves to do once the lock is released.
#[derive(Default)]
struct After {
    rescan: bool,
    changed: bool,
    news: Vec<ProjectEvent>,
}

/// A batch, as far as projects care.
#[derive(Debug, Default)]
struct Hint {
    rescan: bool,
    folders: bool,
    record: bool,
    renames: Vec<(String, String)>,
    files: Vec<String>,
}

enum Msg {
    Walk,
    Batch(Hint),
    /// A grace window ended (`grace.rs`): look again.
    Tick,
    Stop,
}

type Listener = Arc<dyn Fn(&ProjectEvent) + Send + Sync>;
type MapListener = Arc<dyn Fn(&Arc<ProjectMap>) + Send + Sync>;
type OwnersFollower = Arc<dyn Fn(OwnersChange<'_>) + Send + Sync>;
/// At most this many uids are remembered as copied (oldest dropped).
const MAX_MIGRATED: usize = 2000;

/// Asks the `git` plugin for the root commits of found repositories (`(path key, folder)`; `projects.md` §4.4, §8).
/// Must return at once: the plugin queues the lookups in its bounded pool and answers later through
/// [`Projects::root_commits_learned`] (never before the first map, never on the projects worker).
pub type RootCommitAsk = Arc<dyn Fn(Vec<(String, PathBuf)>) + Send + Sync>;

pub struct Projects {
    root: Root,
    dirs: AppDirs,
    membership: StateFile<Membership>,
    explorer: Arc<Explorer>,
    own: Arc<OwnWrites>,
    hub: Arc<WatchHub>,
    sink: Arc<dyn EventSink>,
    files_only: bool,
    inner: Mutex<Inner>,
    map: RwLock<Arc<ProjectMap>>,
    listeners: Mutex<Vec<(u64, Listener)>>,
    /// Told the new map after every change (members started | stopped | moved; the `git` plugin follows them).
    map_listeners: Mutex<Vec<(u64, MapListener)>>,
    /// Root commits of found repositories (re-attach by first commit), while the `git` plugin runs.
    identity: Mutex<Option<(u64, RootCommitAsk)>>,
    /// Where state lives now (`projects.md` §9): the opened folder + the included projects whose 0.6 state was copied.
    owners: Mutex<Owners>,
    /// Told before | after the owners change (plugins with per-project state, the journal).
    owner_followers: Mutex<Vec<(u64, OwnersFollower)>>,
    /// One owners change at a time (the worker and a command may both finish a change).
    owners_turn: Mutex<()>,
    next: AtomicU64,
    tx: Mutex<Option<Sender<Msg>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    /// The core is closing: the worker does nothing more (shared with the worker, which holds no `Projects` while it
    /// waits for the walk).
    stopped: Arc<AtomicBool>,
    /// Signalled when the map becomes ready | the core closes (agent calls wait on `inner`).
    ready_cv: Condvar,
    /// `Some`: the time, moved by hand (tests); the worker then never ends a window on its own.
    manual_now: Mutex<Option<Instant>>,
}

impl std::fmt::Debug for Projects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Projects")
            .field("root", &self.root.path())
            .finish_non_exhaustive()
    }
}

/// Listening to project news until dropped (teardown symmetry).
pub struct ProjectsSubscription {
    owner: Weak<Projects>,
    id: u64,
}

impl Drop for ProjectsSubscription {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            lock(&owner.listeners).retain(|(id, _)| *id != self.id);
            lock(&owner.map_listeners).retain(|(id, _)| *id != self.id);
            lock(&owner.owner_followers).retain(|(id, _)| *id != self.id);
            let mut identity = lock(&owner.identity);
            if identity.as_ref().is_some_and(|(id, _)| *id == self.id) {
                *identity = None;
            }
        }
    }
}

impl Projects {
    /// Reads the record, the person's answers and the included members of `root` (cheap: a plain folder costs one
    /// failed open), gives the member set to the Explorer before its first walk. Discovery starts with
    /// [`Projects::start`].
    #[must_use]
    #[allow(clippy::too_many_arguments)] // the core hands its services over once
    pub fn open(
        root: &Root,
        dirs: &AppDirs,
        explorer: Arc<Explorer>,
        own: Arc<OwnWrites>,
        hub: Arc<WatchHub>,
        sink: Arc<dyn EventSink>,
        files_only: bool,
        root_ignore: &[String],
    ) -> Arc<Self> {
        let store = StateStore::for_project(dirs, root);
        let membership = store.profile_file::<Membership>(MEMBERSHIP_FILE);
        let kept = membership.load().into_value();
        let read = read_record(root.path());
        let inputs = Inputs {
            answer: kept.answer,
            declined: kept.declined.clone(),
            agent_off: kept
                .agent_off
                .iter()
                .filter_map(|s| ProjectUid::parse(s))
                .collect(),
            ..Inputs::default()
        };
        let projects = Arc::new(Self {
            root: root.clone(),
            dirs: dirs.clone(),
            membership,
            explorer,
            own,
            hub,
            sink,
            files_only,
            inner: Mutex::new(Inner {
                inputs,
                record: read,
                disk_skipped: Vec::new(),
                res: HashMap::new(),
                root_card: None,
                vocab_root: None,
                vocab: None,
                member_set: MemberSet::default(),
                ready: false,
                seq: 0,
                removed: HashSet::new(),
                undo: None,
                gone_told: HashSet::new(),
                rejected: HashSet::new(),
                ask: None,
                ancestors: (Vec::new(), None),
                root_ignore: root_ignore.to_vec(),
                migrated: kept.migrated,
                copy_failed: HashSet::new(),
                pending_moves: Vec::new(),
                root_commits: HashMap::new(),
                asked: HashSet::new(),
                vanished: BTreeMap::new(),
                waiting: 0,
                other: kept.other,
            }),
            map: RwLock::new(Arc::new(ProjectMap::default())),
            listeners: Mutex::new(Vec::new()),
            map_listeners: Mutex::new(Vec::new()),
            identity: Mutex::new(None),
            owners: Mutex::new(Owners::root_only(store.clone())),
            owner_followers: Mutex::new(Vec::new()),
            owners_turn: Mutex::new(()),
            next: AtomicU64::new(0),
            tx: Mutex::new(None),
            worker: Mutex::new(None),
            stopped: Arc::new(AtomicBool::new(false)),
            ready_cv: Condvar::new(),
            manual_now: Mutex::new(None),
        });
        {
            let mut inner = lock(&projects.inner);
            projects.entries_from_record(&mut inner);
            projects.presence(&mut inner);
            let set = member_set(&inner);
            if set != inner.member_set {
                projects.explorer.set_members(&set);
                inner.member_set = set;
            }
            projects.refresh_ancestors(&mut inner, true);
            projects.rebuild_map(&inner);
        }
        // members included before round 4 get their 0.6 state copied before any plugin reads it
        projects.update_owners();
        projects
    }

    /// Starts the projects worker: it waits for the walk, then knows the found set (and follows the watch). While it
    /// waits it holds neither the service nor the walk: closing the tab stops it at once.
    pub fn start(self: &Arc<Self>) {
        let (tx, rx) = channel::<Msg>();
        let weak = Arc::downgrade(self);
        let explorer = Arc::clone(&self.explorer);
        let stopped = Arc::clone(&self.stopped);
        let handle = std::thread::Builder::new()
            .name("permark-projects".to_owned())
            .spawn(move || loop {
                // a grace window running: wake at its end (no `Projects` held while waiting)
                let wake = weak.upgrade().and_then(|p| p.next_window_end());
                let msg = match wake {
                    Some(after) => match rx.recv_timeout(after) {
                        Ok(msg) => msg,
                        Err(RecvTimeoutError::Timeout) => Msg::Tick,
                        Err(RecvTimeoutError::Disconnected) => return,
                    },
                    None => match rx.recv() {
                        Ok(msg) => msg,
                        Err(_) => return,
                    },
                };
                if matches!(msg, Msg::Stop) {
                    return;
                }
                let Some(markers) = wait_walk(&explorer, &stopped) else {
                    return;
                };
                let Some(projects) = weak.upgrade() else {
                    return;
                };
                match msg {
                    Msg::Stop => return,
                    Msg::Walk => projects.discover(&Hint::default(), true, &markers),
                    Msg::Batch(hint) => projects.on_hint(&hint, &markers),
                    Msg::Tick => projects.discover(&Hint::default(), false, &markers),
                }
            })
            .ok();
        let _ = tx.send(Msg::Walk);
        *lock(&self.tx) = Some(tx);
        *lock(&self.worker) = handle;
    }

    /// Stops the worker (core drop); releases every member; agent calls waiting for the map give up.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.ready_cv.notify_all();
        if let Some(tx) = lock(&self.tx).take() {
            let _ = tx.send(Msg::Stop);
        }
        if let Some(handle) = lock(&self.worker).take() {
            // a worker waiting for the walk sees `stopped` within one poll; one busy with a batch ends right after it
            let until = Instant::now() + STOP_WAIT;
            while !handle.is_finished() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(2));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
        let mut inner = lock(&self.inner);
        inner.res.clear();
        inner.undo = None;
        // the worker (and with it the window's wake-up) is gone: nothing stays held
        inner.vanished.clear();
        inner.waiting = 0;
    }

    fn now(&self) -> Instant {
        lock(&self.manual_now).unwrap_or_else(Instant::now)
    }

    /// Time left until the first grace window ends (`None`: none running, or the time is moved by hand).
    fn next_window_end(&self) -> Option<Duration> {
        if lock(&self.manual_now).is_some() {
            return None;
        }
        let now = Instant::now();
        lock(&self.inner)
            .vanished
            .values()
            .map(|v| v.until.saturating_duration_since(now))
            .min()
    }

    /// Tests: the time stops here; only [`Projects::advance`] moves it.
    #[cfg(test)]
    pub(crate) fn freeze_clock(&self) {
        *lock(&self.manual_now) = Some(Instant::now());
    }

    /// Tests: moves the stopped time by `by`, then lets the worker look again (a window may have ended).
    #[cfg(test)]
    pub(crate) fn advance(&self, by: Duration) {
        if let Some(now) = lock(&self.manual_now).as_mut() {
            *now += by;
        }
        if let Some(tx) = lock(&self.tx).as_ref() {
            let _ = tx.send(Msg::Tick);
        }
    }

    /// Tests: entries held as here while their folder is away.
    #[cfg(test)]
    pub(crate) fn vanished_count(&self) -> usize {
        lock(&self.inner).vanished.len()
    }

    /// Tests: new folders waiting for a window to end.
    #[cfg(test)]
    pub(crate) fn waiting_count(&self) -> usize {
        lock(&self.inner).waiting
    }

    /// Per-member resources held now (teardown tests).
    #[must_use]
    pub fn held_members(&self) -> usize {
        lock(&self.inner).res.len()
    }

    /// The watch callback: hands what matters of a batch to the worker (plain saves cost a name check per change).
    pub fn watch(&self, batch: &FsBatch) {
        let mut hint = Hint {
            rescan: batch.rescan,
            ..Hint::default()
        };
        for change in &batch.changes {
            let name = change.path.rsplit('/').next().unwrap_or(&change.path);
            let structural = !matches!(change.kind, ChangeKind::Modified);
            if change.is_dir && structural {
                hint.folders = true;
            }
            if let ChangeKind::Renamed { from } = &change.kind {
                if change.is_dir {
                    hint.renames.push((from.clone(), change.path.clone()));
                }
            }
            if crate::tree::Ignorer::is_git_name(name)
                || (structural && crate::paths::fold(name) == crate::paths::fold(PERMARK_DIR))
            {
                hint.folders = true;
            }
            let key = crate::paths::key_of(&change.path);
            if key == crate::paths::key_of(".permark/projects.toml") {
                // own writes too: the worker compares with what it read last (an echo changes nothing)
                hint.record = true;
            } else if matches!(
                name,
                ".aiignore" | "project.toml" | "config.toml" | "projects.toml" | "vocabulary.toml"
            ) {
                hint.files.push(change.path.clone());
            }
        }
        // folders holding a project that moved (whatever the ignore rules say), paired by the watch first: their
        // origin beats a later step of the same chain
        for pair in batch.moved.iter().rev() {
            if !hint.renames.contains(pair) {
                hint.renames.insert(0, pair.clone());
            }
        }
        hint.folders |= !hint.renames.is_empty();
        if hint.rescan || hint.folders || hint.record || !hint.files.is_empty() {
            if let Some(tx) = lock(&self.tx).as_ref() {
                let _ = tx.send(Msg::Batch(hint));
            }
        }
    }

    /// The snapshot readers use (not ready before the first walk ended: see [`ProjectMap::ready`]).
    #[must_use]
    pub fn snapshot(&self) -> Arc<ProjectMap> {
        Arc::clone(&self.map.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Waits up to `timeout` for the first walk to end (the found set known). `false`: still walking, or closing.
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let inner = lock(&self.inner);
        let (inner, _) = self
            .ready_cv
            .wait_timeout_while(inner, timeout, |i| {
                !i.ready && !self.stopped.load(Ordering::Relaxed)
            })
            .unwrap_or_else(PoisonError::into_inner);
        inner.ready
    }

    /// The map an agent call uses (`projects.md` §5.2): waits up to `timeout` for the walk (fail closed: `None` while
    /// the found set is unknown); ancestor rules (opened alone) read again when older than 5 s.
    #[must_use]
    pub fn agent_map(&self, timeout: Duration) -> Option<Arc<ProjectMap>> {
        if !self.wait_ready(timeout) {
            return None;
        }
        let mut inner = lock(&self.inner);
        if self.refresh_ancestors(&mut inner, false) {
            self.rebuild_map(&inner);
        }
        drop(inner);
        Some(self.snapshot())
    }

    /// Listens to project news (Changes panel rows) until the subscription goes.
    #[must_use]
    pub fn subscribe(
        self: &Arc<Self>,
        listener: impl Fn(&ProjectEvent) + Send + Sync + 'static,
    ) -> ProjectsSubscription {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        lock(&self.listeners).push((id, Arc::new(listener)));
        ProjectsSubscription {
            owner: Arc::downgrade(self),
            id,
        }
    }

    /// Hears the new map after every change until the subscription goes (the `git` plugin follows members with it).
    #[must_use]
    pub fn subscribe_map(
        self: &Arc<Self>,
        listener: impl Fn(&Arc<ProjectMap>) + Send + Sync + 'static,
    ) -> ProjectsSubscription {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        lock(&self.map_listeners).push((id, Arc::new(listener)));
        ProjectsSubscription {
            owner: Arc::downgrade(self),
            id,
        }
    }

    /// Lets re-attach ask the root commits of found repositories (`projects.md` §4.4) until the lease goes. Asked only
    /// once the first map is ready; the answers arrive later ([`Projects::root_commits_learned`]).
    #[must_use]
    pub fn provide_root_commits(self: &Arc<Self>, ask: RootCommitAsk) -> ProjectsSubscription {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        *lock(&self.identity) = Some((id, ask));
        // a former lease's open questions died with it
        lock(&self.inner).asked.clear();
        self.ask_root_commits();
        ProjectsSubscription {
            owner: Arc::downgrade(self),
            id,
        }
    }

    /// Root commits the `git` plugin found (`(path key, commit)`; `None`: none). A move question they make possible
    /// appears at once (`projects.md` §4.4, D31: never a silent re-attach).
    pub fn root_commits_learned(&self, answers: &[(String, Option<String>)]) {
        if answers.is_empty() || self.stopped.load(Ordering::Relaxed) {
            return;
        }
        let useful = {
            let mut inner = lock(&self.inner);
            for (key, commit) in answers {
                inner.asked.remove(key);
                inner.root_commits.insert(key.clone(), commit.clone());
            }
            inner.ready && answers.iter().any(|(_, c)| c.is_some())
        };
        if useful {
            if let Some(tx) = lock(&self.tx).as_ref() {
                let _ = tx.send(Msg::Batch(Hint {
                    folders: true,
                    ..Hint::default()
                }));
            }
        }
    }

    /// The state store of a project folder below the root (`projects.md` §9.1: the same files as when it is opened
    /// alone). `None`: the folder is not there.
    #[must_use]
    pub fn store_of(&self, path: &VaultPath) -> Option<StateStore> {
        let root = Root::open(&abs_of(self.root.path(), path)).ok()?;
        Some(StateStore::for_project(&self.dirs, &root))
    }

    /// Writes the first commits the `git` plugin learned (`RecordOp::SetFirstCommit`, one record write for all;
    /// `projects.md` D21): only for entries without one, only while the record reads. Others are skipped quietly.
    ///
    /// # Errors
    /// `Conflict`, IO.
    pub fn set_first_commits(&self, learned: &[(ProjectUid, String)]) -> Result<(), IpcError> {
        if self.files_only || self.stopped.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut inner = lock(&self.inner);
        let RecordRead::Read(record) = &inner.record else {
            return Ok(());
        };
        let mut ops: Vec<RecordOp> = Vec::new();
        for (uid, oid) in learned {
            let valid = (4..=64).contains(&oid.len()) && oid.bytes().all(|b| b.is_ascii_hexdigit());
            let open = record
                .entries
                .iter()
                .any(|e| &e.uid == uid && e.first_commit.is_none());
            let twice = ops
                .iter()
                .any(|o| matches!(o, RecordOp::SetFirstCommit { uid: u, .. } if u == uid));
            if valid && open && !twice {
                ops.push(RecordOp::SetFirstCommit {
                    uid: uid.clone(),
                    oid: oid.to_ascii_lowercase(),
                });
            }
        }
        if ops.is_empty() {
            return Ok(());
        }
        self.write_record(&ops, false)?;
        inner.record = read_record(self.root.path());
        inner.seq += 1;
        self.rebuild_map(&inner);
        drop(inner);
        self.finish(&After {
            changed: true,
            ..After::default()
        });
        Ok(())
    }

    /// Asks the `git` plugin for the root commits a move question may need (`projects.md` §4.4): only after the first
    /// map, only while a record entry with a first commit is not at its path, only for found git projects that are
    /// offered (never listed, never refused), once per session per folder. Never waits for the answers.
    fn ask_root_commits(&self) {
        if self.stopped.load(Ordering::Relaxed) {
            return;
        }
        let Some(ask) = lock(&self.identity).as_ref().map(|(_, f)| Arc::clone(f)) else {
            return;
        };
        let need: Vec<(String, PathBuf)> = {
            let mut inner = lock(&self.inner);
            if !inner.ready {
                return;
            }
            let waiting = inner.inputs.entries.iter().any(|e| {
                inner.inputs.presence.get(&e.uid) == Some(&Presence::Gone)
                    && !inner.inputs.refused(&e.path.key())
                    && first_commit_of(&inner, &e.uid).is_some()
            });
            if !waiting {
                return;
            }
            let states = inner.inputs.derive();
            let need: Vec<(String, PathBuf)> = inner
                .inputs
                .found
                .iter()
                .filter(|(k, f)| {
                    f.marker.git()
                        && states.get(*k) == Some(&Candidate::Offered)
                        && !inner.root_commits.contains_key(*k)
                        && !inner.asked.contains(*k)
                })
                .map(|(k, f)| (k.clone(), abs_of(self.root.path(), &f.path)))
                .collect();
            inner.asked.extend(need.iter().map(|(k, _)| k.clone()));
            need
        };
        if !need.is_empty() {
            ask(need);
        }
    }

    // ---- reading the disk --------------------------------------------------------------------------------------

    /// Record entries -> machine entries (valid ones; disk reasons checked here).
    fn entries_from_record(&self, inner: &mut Inner) {
        let Some(record) = inner.record.record().cloned() else {
            // unreadable | absent: the last good entries stay (absent: none)
            if matches!(inner.record, RecordRead::Absent) {
                inner.inputs.entries.clear();
                inner.disk_skipped.clear();
            }
            return;
        };
        inner.disk_skipped.clear();
        let mut seen = HashMap::new();
        inner.inputs.entries = record
            .entries
            .iter()
            .filter(|e| {
                if through_link_memo(self.root.path(), &e.path, &mut seen) {
                    inner.disk_skipped.push(SkippedEntry {
                        line: e.line,
                        reason: RecordSkip::Link,
                    });
                    false
                } else {
                    true
                }
            })
            .map(|e| RecEntry {
                uid: e.uid.clone(),
                path: e.path.clone(),
                hidden: e.hidden,
            })
            .collect();
    }

    /// Presence of every entry, outer first (an outer member's `ignore` list may hide an inner one); members that
    /// start load what they bring, members that stop release it. A path that leads through a link | junction now (made
    /// one after the record was read) counts as gone: nothing is read from beyond it.
    fn presence(&self, inner: &mut Inner) {
        let mut entries = inner.inputs.entries.clone();
        entries.sort_by_key(|e| e.path.segments().count());
        let mut hard = Hard {
            root: gitignore_of(self.root.path(), &inner.root_ignore),
            members: Vec::new(),
        };
        let mut presence = BTreeMap::new();
        let mut seen = HashMap::new();
        let all_out = inner.inputs.answer == RootAnswer::DeclinedAll;
        let declined: HashSet<String> = inner.inputs.declined.iter().cloned().collect();
        // live only (never at open, never files-only): a folder gone while open is held as here for a moment
        let live = inner.ready && !self.files_only;
        let now = self.now();
        let mut vanished = BTreeMap::new();
        for entry in &entries {
            let refused = all_out || declined.contains(&entry.path.key());
            let mut state = if hard.hides(&entry.path) {
                Presence::Ignored
            } else if entry
                .path
                .parent()
                .is_some_and(|up| through_link_memo(self.root.path(), &up, &mut seen))
            {
                // the folder itself: `marker_at` refuses a link
                Presence::Gone
            } else {
                match marker_at(&abs_of(self.root.path(), &entry.path)) {
                    Some(marker) => Presence::Here(marker),
                    None => Presence::Gone,
                }
            };
            if state == Presence::Gone && live && !refused {
                let id = inner.res.get(&entry.uid).and_then(|r| r.id);
                let was = inner.inputs.presence.get(&entry.uid);
                if let Some(v) = grace::hold(inner.vanished.get(&entry.uid), was, id, now) {
                    state = Presence::Here(v.marker);
                    vanished.insert(entry.uid.clone(), v);
                }
            }
            if matches!(state, Presence::Here(_)) && !refused {
                let res = inner.res.entry(entry.uid.clone()).or_insert_with(|| {
                    load_member(&self.dirs, &abs_of(self.root.path(), &entry.path))
                });
                if let Some(rules) =
                    gitignore_of(&abs_of(self.root.path(), &entry.path), &res.ignore)
                {
                    hard.members
                        .push((entry.path.key(), entry.path.segments().count(), rules));
                }
            }
            presence.insert(entry.uid.clone(), state);
        }
        inner.inputs.presence = presence;
        inner.vanished = vanished;
        let here: HashSet<ProjectUid> = inner.inputs.included().into_iter().collect();
        inner.res.retain(|uid, _| here.contains(uid));
    }

    /// Opened alone (`projects.md` §5.2): every ancestor (<= 32 levels up) whose record lists this folder or a folder
    /// containing it adds its own agent rules + those of every listed project on this folder's chain between it and
    /// this folder (as its tab would). Read when older than [`ANCESTORS_TTL`] (`force`: now); any change of a record,
    /// `.aiignore` or config on the chain shows at the next read. Returns whether the rules changed.
    fn refresh_ancestors(&self, inner: &mut Inner, force: bool) -> bool {
        if !force
            && inner
                .ancestors
                .1
                .is_some_and(|at| at.elapsed() < ANCESTORS_TTL)
        {
            return false;
        }
        let found = ancestor_rules(&self.dirs, self.root.path());
        let changed = found != inner.ancestors.0;
        inner.ancestors = (found, Some(Instant::now()));
        changed
    }

    /// The found set from the walk's markers + projects the dormant records of found projects list.
    fn found_now(&self, inner: &Inner, markers: &[crate::tree::Marker]) -> BTreeMap<String, Found> {
        let mut found = BTreeMap::new();
        for marker in markers {
            let Ok(path) = VaultPath::parse(&marker.path) else {
                continue;
            };
            let Some(kind) = ProjectMarker::of(marker.git, marker.permark) else {
                continue;
            };
            found.insert(
                path.key(),
                Found {
                    path,
                    marker: kind,
                    inner_uid: None,
                },
            );
        }
        // inner roots: their own records offer their projects (paths rebased, ids reused)
        let mut inner_offers = Vec::new();
        for f in found.values() {
            if !matches!(f.marker, ProjectMarker::Permark | ProjectMarker::Both) {
                continue;
            }
            let entries = inner
                .inputs
                .entries
                .iter()
                .find(|e| e.path.key() == f.path.key())
                .and_then(|e| inner.res.get(&e.uid))
                .map_or_else(
                    || {
                        read_record(&abs_of(self.root.path(), &f.path))
                            .record()
                            .map(|r| {
                                r.entries
                                    .iter()
                                    .map(|e| (e.path.clone(), e.uid.clone()))
                                    .collect()
                            })
                            .unwrap_or_default()
                    },
                    |res| res.inner.clone(),
                );
            for (rel, uid) in entries {
                let Ok(path) = VaultPath::parse(&format!("{}/{}", f.path.as_str(), rel.as_str()))
                else {
                    continue;
                };
                inner_offers.push((path, uid));
            }
        }
        let hard = Hard {
            root: gitignore_of(self.root.path(), &inner.root_ignore),
            members: Vec::new(),
        };
        for (path, uid) in inner_offers {
            if found.contains_key(&path.key())
                || hard.hides(&path)
                || through_link(self.root.path(), &path)
            {
                continue;
            }
            if let Some(marker) = marker_at(&abs_of(self.root.path(), &path)) {
                found.insert(
                    path.key(),
                    Found {
                        path,
                        marker,
                        inner_uid: Some(uid),
                    },
                );
            }
        }
        found
    }

    fn root_ignore(&self) -> Vec<String> {
        probe::hit();
        ProjectConfig::load(&self.root).ignore
    }

    // ---- worker --------------------------------------------------------------------------------------------------

    fn on_hint(&self, hint: &Hint, markers: &[crate::tree::Marker]) {
        if self.stopped.load(Ordering::Relaxed) {
            return;
        }
        let mut after = After::default();
        {
            let mut inner = lock(&self.inner);
            if hint.record {
                let read = read_record(self.root.path());
                if read != inner.record {
                    let before: HashSet<String> =
                        inner.inputs.entries.iter().map(|e| e.path.key()).collect();
                    inner.record = read;
                    self.entries_from_record(&mut inner);
                    after.news.push(ProjectEvent::ListChanged);
                    // listed now, refused by the person: it stays out; the Changes panel says so
                    for entry in &inner.inputs.entries {
                        let key = entry.path.key();
                        if !before.contains(&key) && inner.inputs.refused(&key) {
                            after.news.push(ProjectEvent::KeptOut {
                                name: folder_name(entry.path.as_str()),
                                path: entry.path.as_str().to_owned(),
                            });
                        }
                    }
                    after.changed = true;
                }
            }
            let mut files = false;
            for path in &hint.files {
                files |= self.reload_files_at(&mut inner, path);
            }
            if files {
                // cards, rules and lists read again: the map carries them
                inner.seq += 1;
                self.rebuild_map(&inner);
                after.changed = true;
            }
        }
        if hint.rescan || hint.folders || hint.record || after.changed {
            self.discover(hint, false, markers);
        }
        self.finish(&after);
    }

    /// A card | config | `.aiignore` | inner record changed: the member (or the root card) reads its files again.
    fn reload_files_at(&self, inner: &mut Inner, path: &str) -> bool {
        let key = crate::paths::key_of(path);
        if key == crate::paths::key_of(".permark/config.toml") {
            let ignore = self.root_ignore();
            let changed = ignore != inner.root_ignore;
            inner.root_ignore = ignore;
            return changed;
        }
        if key == crate::paths::key_of(".permark/vocabulary.toml") {
            inner.vocab_root = Some(vocabulary::read(self.root.path()));
            return true;
        }
        if key == crate::paths::key_of(".permark/project.toml") {
            if let Some(file) = card::file_of(&self.dirs, self.root.path()) {
                inner.root_card = Some((file.read(), file.place()));
                return true;
            }
        }
        let owner = inner
            .inputs
            .entries
            .iter()
            .filter(|e| inner.res.contains_key(&e.uid) && within(&key, &e.path.key()))
            .max_by_key(|e| e.path.key().len())
            .map(|e| (e.uid.clone(), e.path.clone()));
        if let Some((uid, member)) = owner {
            let res = load_member(&self.dirs, &abs_of(self.root.path(), &member));
            inner.res.insert(uid, res);
            return true;
        }
        false
    }

    /// Looks at the disk again: found set, presence, re-attach of moved | missing projects, members started | stopped.
    fn discover(&self, hint: &Hint, first: bool, markers: &[crate::tree::Marker]) {
        if self.stopped.load(Ordering::Relaxed) {
            return;
        }
        let mut after = After::default();
        {
            let mut inner = lock(&self.inner);
            let old = inner.inputs.clone();
            let old_ask = inner.ask.clone();
            let was_ready = inner.ready;
            let found = self.found_now(&inner, markers);
            inner.inputs.found = found;
            self.presence(&mut inner);
            self.reattach(&mut inner, hint, &old.found, &mut after);
            inner.ready = true;
            self.ready_cv.notify_all();
            if was_ready && !first {
                Self::news_between(&mut inner, &old, &mut after);
            }
            let set = member_set(&inner);
            if set != inner.member_set {
                self.explorer.set_members(&set);
                inner.member_set = set;
                after.rescan = true;
            }
            after.changed |= first || !was_ready || inner.inputs != old || inner.ask != old_ask;
            if after.changed {
                inner.seq += 1;
                self.rebuild_map(&inner);
            }
        }
        self.finish(&after);
        // after the map: the answers come back on the git pool and only add a question
        self.ask_root_commits();
    }

    /// Changes rows between two inputs (found while open, gone, back).
    fn news_between(inner: &mut Inner, old: &Inputs, after: &mut After) {
        let before = old.derive();
        let now = inner.inputs.derive();
        for (key, state) in &now {
            let was = before.get(key);
            match (was, state) {
                (None, Candidate::Offered | Candidate::Declined) => {
                    if let Some(f) = inner.inputs.found.get(key) {
                        let path = f.path.as_str().to_owned();
                        let parent = path
                            .rsplit_once('/')
                            .map_or(String::new(), |(p, _)| p.to_owned());
                        after.news.push(ProjectEvent::Appeared {
                            name: folder_name(&path),
                            path: parent,
                        });
                    }
                }
                (
                    Some(Candidate::Included { .. }),
                    Candidate::Missing {
                        uid,
                        reason: MissingReason::Gone,
                    },
                ) => {
                    if inner.gone_told.insert(uid.clone()) {
                        let path = inner
                            .inputs
                            .entry(uid)
                            .map(|e| e.path.as_str().to_owned())
                            .unwrap_or_default();
                        after.news.push(ProjectEvent::Gone {
                            name: Self::name_of(inner, uid, &path),
                            path,
                        });
                    }
                }
                (Some(Candidate::Missing { .. }), Candidate::Included { uid, .. }) => {
                    let path = inner
                        .inputs
                        .entry(uid)
                        .map(|e| e.path.as_str().to_owned())
                        .unwrap_or_default();
                    after.news.push(ProjectEvent::Back {
                        name: Self::name_of(inner, uid, &path),
                        path,
                    });
                }
                _ => {}
            }
        }
    }

    fn name_of(inner: &Inner, uid: &ProjectUid, path: &str) -> String {
        inner
            .res
            .get(uid)
            .and_then(|r| r.card.name.clone())
            .unwrap_or_else(|| folder_name(path))
    }

    /// Re-attach (`projects.md` §4.4, D31): silent only where the disk says it is the same folder (a rename the watch
    /// saw live, the case of its name); anything found at open (card id, first commit, folder name) is a question to
    /// the person, one candidate at a time (clones share card id + first commit). Refused folders are never a
    /// candidate (D28).
    /// A move the OS split across batches pairs by the folder's disk identity inside a grace window (`grace.rs`);
    /// `before`: the found set before this pass.
    fn reattach(
        &self,
        inner: &mut Inner,
        hint: &Hint,
        before: &BTreeMap<String, Found>,
        after: &mut After,
    ) {
        let mut moves: Vec<(ProjectUid, VaultPath)> = Vec::new();
        // a folder renamed to another case of its name (case-insensitive disk): still here; the path follows the disk
        for entry in &inner.inputs.entries {
            if let Some(found) = inner.inputs.found.get(&entry.path.key()) {
                if found.path != entry.path && !inner.inputs.refused(&entry.path.key()) {
                    moves.push((entry.uid.clone(), found.path.clone()));
                }
            }
        }
        // 1. a rename the watch paired covers it (live: the same folder)
        for entry in &inner.inputs.entries {
            let away = inner.inputs.presence.get(&entry.uid) == Some(&Presence::Gone)
                || inner.vanished.contains_key(&entry.uid);
            if !away || inner.inputs.refused(&entry.path.key()) {
                continue;
            }
            if let Some(path) = renamed_path(&entry.path, &hint.renames) {
                if marker_at(&abs_of(self.root.path(), &path)).is_some() {
                    moves.push((entry.uid.clone(), path));
                }
            }
        }
        self.apply_moves(inner, moves, after);
        // 2. inside a grace window: a folder that came is a held entry's own folder (the rest wait while one runs)
        let (moves, waiting) = self.grace_pairs(inner, before);
        self.apply_moves(inner, moves, after);
        inner.waiting = 0;
        if inner.vanished.is_empty() {
            for f in waiting {
                inner.inputs.found.insert(f.path.key(), f);
            }
        } else {
            inner.waiting = waiting.len();
        }
        inner.ask = self.move_question(inner);
    }

    /// While a grace window runs: folders new since `before` (not listed) leave the found set (no row, no strip); one
    /// that is a held entry's folder goes back as where that entry moved. Returns those moves + the folders waiting.
    fn grace_pairs(
        &self,
        inner: &mut Inner,
        before: &BTreeMap<String, Found>,
    ) -> (Vec<(ProjectUid, VaultPath)>, Vec<Found>) {
        if inner.vanished.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let listed: HashSet<String> = inner.inputs.entries.iter().map(|e| e.path.key()).collect();
        let fresh: Vec<String> = inner
            .inputs
            .found
            .keys()
            .filter(|k| !before.contains_key(*k) && !listed.contains(*k))
            .cloned()
            .collect();
        let waiting: Vec<Found> = fresh
            .iter()
            .filter_map(|k| inner.inputs.found.remove(k))
            .collect();
        if waiting.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let ids: Vec<Option<FolderId>> = waiting
            .iter()
            .map(|f| FolderId::of(&abs_of(self.root.path(), &f.path)))
            .collect();
        let firsts: Vec<Option<String>> = inner
            .vanished
            .keys()
            .map(|uid| first_commit_of(inner, uid))
            .collect();
        let pairs = {
            let left: Vec<Left<'_>> = inner
                .vanished
                .iter()
                .zip(&firsts)
                .filter_map(|((uid, v), first)| {
                    Some(Left {
                        uid,
                        path: &inner.inputs.entry(uid)?.path,
                        id: v.id,
                        first_commit: first.as_deref(),
                    })
                })
                .collect();
            let came: Vec<Came<'_>> = waiting
                .iter()
                .zip(&ids)
                .map(|(f, id)| Came {
                    path: &f.path,
                    id: *id,
                    root_commit: inner
                        .root_commits
                        .get(&f.path.key())
                        .and_then(Option::as_deref),
                })
                .collect();
            grace::pair(&left, &came)
        };
        let mut waiting: Vec<Option<Found>> = waiting.into_iter().map(Some).collect();
        let mut moves = Vec::new();
        for (uid, i) in pairs {
            if let Some(f) = waiting.get_mut(i).and_then(Option::take) {
                moves.push((uid, f.path.clone()));
                inner.inputs.found.insert(f.path.key(), f);
            }
        }
        (moves, waiting.into_iter().flatten().collect())
    }

    /// Re-attach moves (the entry follows its folder): record path, profile state, one `moved` row each.
    fn apply_moves(
        &self,
        inner: &mut Inner,
        moves: Vec<(ProjectUid, VaultPath)>,
        after: &mut After,
    ) {
        for (uid, path) in moves {
            if self.files_only {
                break;
            }
            let from = inner.inputs.entry(&uid).map(|e| e.path.clone());
            let (next, effects) =
                fsm::step(&inner.inputs, &Event::SetPath(uid.clone(), path.clone()));
            if effects.is_empty() {
                continue;
            }
            let ops: Vec<RecordOp> = effects
                .iter()
                .filter_map(|e| match e {
                    fsm::Effect::Record(op) => Some(op.clone()),
                    _ => None,
                })
                .collect();
            if self.write_record(&ops, false).is_ok() {
                inner.inputs = next;
                if let Some(from) = &from {
                    self.follow_state(inner, from, &path);
                }
                after.news.push(ProjectEvent::Moved {
                    name: Self::name_of(inner, &uid, path.as_str()),
                    path: path.as_str().to_owned(),
                });
                inner.record = read_record(self.root.path());
                self.presence(inner);
                after.changed = true;
            }
        }
    }

    /// The one move question now (`projects.md` §4.4 steps 2-4), if any: a missing entry (not refused) + one offered
    /// project (found, not listed, not refused) that may be it. Strong signs first (card id; first commit + folder
    /// name): asked one candidate at a time, the next after a No. Weak signs (folder name, or first commit alone):
    /// only when exactly one entry + exactly one project match.
    fn move_question(&self, inner: &Inner) -> Option<MoveAsk> {
        let gone: Vec<&RecEntry> = inner
            .inputs
            .entries
            .iter()
            .filter(|e| inner.inputs.presence.get(&e.uid) == Some(&Presence::Gone))
            .filter(|e| !inner.inputs.refused(&e.path.key()))
            .collect();
        if gone.is_empty() {
            return None;
        }
        let states = inner.inputs.derive();
        let offered: Vec<&Found> = inner
            .inputs
            .found
            .iter()
            .filter(|(k, _)| states.get(*k) == Some(&Candidate::Offered))
            .map(|(_, f)| f)
            .collect();
        if offered.is_empty() {
            return None;
        }
        // card ids of the offered projects, read once per pass
        let ids: HashMap<String, ProjectUid> = offered
            .iter()
            .filter(|f| matches!(f.marker, ProjectMarker::Permark | ProjectMarker::Both))
            .filter_map(|f| {
                let id = card::file_of(&self.dirs, &abs_of(self.root.path(), &f.path))?
                    .read()
                    .id?;
                Some((f.path.key(), id))
            })
            .collect();
        let open =
            |uid: &ProjectUid, f: &Found| !inner.rejected.contains(&(uid.clone(), f.path.key()));
        let named = |path: &str| crate::paths::fold(&folder_name(path));
        let ask = |entry: &RecEntry, f: &Found| MoveAsk {
            uid: entry.uid.clone(),
            name: Self::name_of(inner, &entry.uid, entry.path.as_str()),
            was: entry.path.as_str().to_owned(),
            candidate: f.path.as_str().to_owned(),
        };
        // 2. card id, 3. first commit + folder name
        for entry in &gone {
            let name = named(entry.path.as_str());
            let first = first_commit_of(inner, &entry.uid);
            let strong = |f: &&&Found| {
                ids.get(&f.path.key()) == Some(&entry.uid)
                    || (f.marker.git()
                        && named(f.path.as_str()) == name
                        && first.as_deref().is_some_and(|first| {
                            commit_matches(inner.root_commits.get(&f.path.key()), first)
                        }))
            };
            if let Some(f) = offered.iter().filter(strong).find(|f| open(&entry.uid, f)) {
                return Some(ask(entry, f));
            }
        }
        // 4. exactly one missing entry like exactly one offered project (same folder name, or renamed while closed:
        // same first commit)
        for entry in &gone {
            let name = named(entry.path.as_str());
            let first = first_commit_of(inner, &entry.uid);
            let same: Vec<&&Found> = offered
                .iter()
                .filter(|f| {
                    named(f.path.as_str()) == name
                        || first.as_deref().is_some_and(|first| {
                            commit_matches(inner.root_commits.get(&f.path.key()), first)
                        })
                })
                .collect();
            let others = gone
                .iter()
                .filter(|e| {
                    named(e.path.as_str()) == name
                        || (first.is_some() && first_commit_of(inner, &e.uid) == first)
                })
                .count();
            if let ([candidate], 1) = (same.as_slice(), others) {
                if open(&entry.uid, candidate) {
                    return Some(ask(entry, candidate));
                }
            }
        }
        None
    }

    // ---- state per project (round 4) ------------------------------------------------------------------------------

    /// Where state lives now: the opened folder + every included project whose 0.6 state was copied (`projects.md`
    /// §9.1). A project not copied yet (the copy failed) keeps its state in the opened folder's files meanwhile.
    #[must_use]
    pub fn owners(&self) -> Owners {
        lock(&self.owners).clone()
    }

    /// Hears every owners change until the subscription goes: `Pause` (write what is pending, then hold writes),
    /// the copy of 0.6 state + moved profile folders, then `Resume(owners)` (read again). Called on the thread that
    /// made the change, never under the projects lock.
    #[must_use]
    pub fn follow_owners(
        self: &Arc<Self>,
        follower: impl Fn(OwnersChange<'_>) + Send + Sync + 'static,
    ) -> ProjectsSubscription {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        lock(&self.owner_followers).push((id, Arc::new(follower)));
        ProjectsSubscription {
            owner: Arc::downgrade(self),
            id,
        }
    }

    /// Whether the 0.6 state of `uid` was copied (tests, views).
    #[must_use]
    pub fn is_migrated(&self, uid: &ProjectUid) -> bool {
        let map = self.snapshot();
        let inner = lock(&self.inner);
        inner.migrated.iter().any(|m| m == uid.as_str())
            || map.member(uid).is_some_and(|m| is_copied(&inner, m))
    }

    /// The owners for `map`: members copied (or about to be: `extra`) whose folder is there.
    fn owners_for(&self, map: &ProjectMap, extra: &[ProjectUid]) -> Owners {
        let root = StateStore::for_project(&self.dirs, &self.root);
        let copied: Vec<bool> = {
            let inner = lock(&self.inner);
            map.members().iter().map(|m| is_copied(&inner, m)).collect()
        };
        let slots = map
            .members()
            .iter()
            .zip(copied)
            .filter(|(m, copied)| *copied || extra.contains(&m.uid))
            .map(|(m, _)| m)
            .filter_map(|m| {
                let store = self.store_of(&m.path)?;
                Some(OwnerSlot::member(m.uid.clone(), m.path.as_str(), store))
            })
            .collect();
        Owners::new(root, slots)
    }

    /// The owners follow the map: followers paused, moved profile folders carried (§9.1), 0.6 state of newly
    /// included projects copied once (§9.3), followers resumed with the new owners. Nothing happens while the places
    /// stay the same.
    fn update_owners(&self) {
        let _turn = lock(&self.owners_turn);
        let map = self.snapshot();
        let moves = std::mem::take(&mut lock(&self.inner).pending_moves);
        if !self.files_only {
            self.forget_left(&map);
        }
        let to_copy: Vec<MemberInfo> = if self.files_only {
            Vec::new()
        } else {
            let inner = lock(&self.inner);
            map.members()
                .iter()
                .filter(|m| !is_copied(&inner, m) && !inner.copy_failed.contains(&m.uid))
                .cloned()
                .collect()
        };
        let uids: Vec<ProjectUid> = to_copy.iter().map(|m| m.uid.clone()).collect();
        let wanted = self.owners_for(&map, &uids);
        if wanted.same(&lock(&self.owners)) && moves.is_empty() && to_copy.is_empty() {
            return;
        }
        self.tell_owners(&OwnersChange::Pause);
        for (from, to) in &moves {
            // best effort: a failure leaves the state under the old id, as before round 3
            let _ = crate::state::move_profile_state(
                &self.dirs,
                &abs_of(self.root.path(), from),
                &abs_of(self.root.path(), to),
            );
        }
        if !to_copy.is_empty() {
            self.copy_state(&to_copy);
        }
        let now = self.owners_for(&map, &[]);
        *lock(&self.owners) = now.clone();
        self.tell_owners(&OwnersChange::Resume(&now));
    }

    fn tell_owners(&self, change: &OwnersChange<'_>) {
        let followers: Vec<OwnersFollower> = lock(&self.owner_followers)
            .iter()
            .map(|(_, f)| Arc::clone(f))
            .collect();
        for follower in followers {
            follower(match change {
                OwnersChange::Pause => OwnersChange::Pause,
                OwnersChange::Resume(owners) => OwnersChange::Resume(owners),
            });
        }
    }

    /// Members that were owners and are no longer in the map (left out, removed, declined): their copy mark goes, so
    /// an include later copies what the opened folder got meanwhile (the ledger keeps out what was copied before).
    fn forget_left(&self, map: &ProjectMap) {
        let left: Vec<String> = lock(&self.owners)
            .slots()
            .iter()
            .filter_map(|s| s.uid.as_ref())
            .filter(|uid| map.member(uid).is_none())
            .map(|uid| uid.as_str().to_owned())
            .collect();
        if left.is_empty() {
            return;
        }
        let mut inner = lock(&self.inner);
        let before = inner.migrated.len();
        inner.migrated.retain(|u| !left.contains(u));
        if inner.migrated.len() != before {
            // a failed save keeps the mark on disk: a later include with the same uid then copies at the next start
            let _ = self.save_membership(&inner);
        }
    }

    /// Copies the opened folder's 0.6 state into each member (`projects.md` §9.3, by entry: §14 D50); marks the ones
    /// that succeeded (copy + ledger saved).
    fn copy_state(&self, members: &[MemberInfo]) {
        let root = StateStore::for_project(&self.dirs, &self.root);
        let ledger_file =
            root.profile_file::<super::migrate::LedgerFile>(super::migrate::LEDGER_FILE);
        let mut ledger = ledger_file.load().into_value();
        let mut done = Vec::new();
        let mut failed = Vec::new();
        for member in members {
            let Some(target) = self.store_of(&member.path) else {
                continue;
            };
            let seen = ledger.members.entry(member.key.clone()).or_default();
            match super::migrate::copy_state(&root, &target, member.path.as_str(), seen) {
                Ok(_) => done.push(member),
                Err(_) => failed.push(member.uid.clone()),
            }
        }
        if !done.is_empty() && ledger_file.save(&ledger).is_err() {
            // the copies stand (the project's entries win next time); the marks wait for a ledger that is saved
            failed.extend(done.drain(..).map(|m| m.uid.clone()));
        }
        let mut inner = lock(&self.inner);
        inner.copy_failed.extend(failed);
        if done.is_empty() {
            return;
        }
        for member in done {
            let uid = member.uid.as_str().to_owned();
            if !inner.migrated.contains(&uid) {
                inner.migrated.push(uid);
            }
        }
        let over = inner.migrated.len().saturating_sub(MAX_MIGRATED);
        inner.migrated.drain(..over);
        // not saved: copied again next start (by ledger: nothing twice)
        let _ = self.save_membership(&inner);
    }

    // ---- publishing ------------------------------------------------------------------------------------------------

    fn rebuild_map(&self, inner: &Inner) {
        let states = inner.inputs.derive();
        let mut members = Vec::new();
        let mut closed = Vec::new();
        for (key, state) in &states {
            match state {
                Candidate::Included { uid, marker } => {
                    let Some(entry) = inner.inputs.entry(uid) else {
                        continue;
                    };
                    let res = inner.res.get(uid).cloned().unwrap_or_default();
                    let dto = card::dto(
                        &res.card,
                        &folder_name(entry.path.as_str()),
                        res.place.unwrap_or(CardPlace::Profile),
                    );
                    members.push(MemberInfo {
                        uid: uid.clone(),
                        path: entry.path.clone(),
                        key: key.clone(),
                        marker: *marker,
                        hidden: entry.hidden,
                        name: dto.name,
                        short: dto.short,
                        agent_off: inner.inputs.agent_off.contains(uid),
                        agent: res.agent.clone(),
                        first_commit: inner.record.record().and_then(|r| {
                            r.entries
                                .iter()
                                .find(|e| &e.uid == uid)
                                .and_then(|e| e.first_commit.clone())
                        }),
                    });
                }
                Candidate::Missing { .. } | Candidate::Offered | Candidate::Declined => {
                    closed.push(key.clone());
                }
            }
        }
        let map = ProjectMap::new(
            inner.seq,
            inner.ready,
            members,
            closed,
            inner.ancestors.0.clone(),
        );
        *self.map.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(map);
    }

    /// After the lock: owners (state places) told, rescan (members changed), `projects-changed`, feed news.
    fn finish(&self, after: &After) {
        if after.changed {
            self.update_owners();
        }
        if after.rescan {
            self.hub.publish(&FsBatch {
                rescan: true,
                bulk: true,
                ..FsBatch::default()
            });
        }
        if after.changed {
            let seq = lock(&self.inner).seq;
            self.sink.emit(CoreEvent::ProjectsChanged(ProjectsChanged {
                root: self.root.path().display().to_string(),
                seq,
            }));
            let listeners: Vec<MapListener> = lock(&self.map_listeners)
                .iter()
                .map(|(_, l)| Arc::clone(l))
                .collect();
            if !listeners.is_empty() {
                let map = self.snapshot();
                for listener in &listeners {
                    listener(&map);
                }
            }
        }
        if !after.news.is_empty() {
            let listeners: Vec<Listener> = lock(&self.listeners)
                .iter()
                .map(|(_, l)| Arc::clone(l))
                .collect();
            for event in &after.news {
                for listener in &listeners {
                    listener(event);
                }
            }
        }
    }

    fn save_membership(&self, inner: &Inner) -> Result<(), IpcError> {
        if self.files_only {
            return Ok(());
        }
        let data = Membership {
            answer: inner.inputs.answer,
            declined: inner.inputs.declined.clone(),
            migrated: inner.migrated.clone(),
            agent_off: inner
                .inputs
                .agent_off
                .iter()
                .map(|u| u.as_str().to_owned())
                .collect(),
            other: inner.other.clone(),
        };
        Ok(self.membership.save(&data)?)
    }

    fn write_record(&self, ops: &[RecordOp], create: bool) -> Result<record::Written, IpcError> {
        if ops.is_empty() {
            return Err(IpcError::new(ErrorKind::Unavailable).subject("projects"));
        }
        let own = Arc::clone(&self.own);
        let path = ".permark/projects.toml";
        Ok(record::write(self.root.path(), ops, create, &|bytes| {
            own.expect_write(path, crate::fsx::hash(bytes));
        })?)
    }

    /// Applies a person's event: record ops + profile + members + map, as one change.
    fn apply_event(
        &self,
        inner: &mut Inner,
        event: &Event,
        create: bool,
    ) -> Result<After, IpcError> {
        let (next, effects) = fsm::step(&inner.inputs, event);
        let ops: Vec<RecordOp> = effects
            .iter()
            .filter_map(|e| match e {
                fsm::Effect::Record(op) => Some(op.clone()),
                _ => None,
            })
            .collect();
        // profile-only choices (decline, agent) stay possible while the record cannot be written
        if !ops.is_empty() && !matches!(inner.record, RecordRead::Read(_) | RecordRead::Absent) {
            return Err(IpcError::new(ErrorKind::Unavailable).subject("projects"));
        }
        let mut after = After::default();
        if effects.is_empty() {
            return Ok(after);
        }
        if !ops.is_empty() {
            self.write_record(&ops, create)?;
            inner.record = read_record(self.root.path());
        }
        if let Event::SetPath(uid, to) = event {
            if let Some(from) = inner.inputs.entry(uid).map(|e| e.path.clone()) {
                self.follow_state(inner, &from, to);
            }
        }
        for op in &ops {
            if let RecordOp::Remove { uid } = op {
                inner.removed.insert(uid.clone());
            }
        }
        inner.inputs = next;
        if effects.contains(&fsm::Effect::SaveMembership) {
            self.save_membership(inner)?;
        }
        self.entries_from_record(inner);
        self.presence(inner);
        let set = member_set(inner);
        if set != inner.member_set {
            self.explorer.set_members(&set);
            inner.member_set = set;
            after.rescan = true;
        }
        inner.seq += 1;
        self.rebuild_map(inner);
        after.changed = true;
        Ok(after)
    }

    /// A member moved: its profile state folder follows its new `ProjectId` (`projects.md` §9.1; best effort: a
    /// failure leaves the state under the old id, as before round 3).
    /// Done when the owners are told next (`update_owners`): the plugins write what they hold first, so nothing lands
    /// in the old folder after it moved.
    fn follow_state(&self, inner: &mut Inner, from: &VaultPath, to: &VaultPath) {
        if self.files_only || from.key() == to.key() {
            return;
        }
        inner.pending_moves.push((from.clone(), to.clone()));
    }

    /// The opened folder's own card (read on first use).
    fn root_card<'i>(&self, inner: &'i mut Inner) -> &'i (Card, CardPlace) {
        inner.root_card.get_or_insert_with(|| {
            card::file_of(&self.dirs, self.root.path())
                .map_or((Card::default(), CardPlace::Profile), |file| {
                    (file.read(), file.place())
                })
        })
    }

    // ---- commands ------------------------------------------------------------------------------------------------

    fn keys_of(inner: &Inner, pick: &ProjectPick, wanted: &[Candidate]) -> Vec<String> {
        let states = inner.inputs.derive();
        match pick {
            ProjectPick::All => states
                .iter()
                .filter(|(_, s)| wanted.contains(s))
                .map(|(k, _)| k.clone())
                .collect(),
            ProjectPick::Paths { paths } => paths
                .iter()
                .filter_map(|p| VaultPath::parse(p).ok())
                .map(|p| p.key())
                .filter(|k| states.get(k).is_some_and(|s| wanted.contains(s)))
                .collect(),
        }
    }

    /// Includes projects (`projects.md` §4.2). `init`: Initialize PerMark in the root (when it has no `.permark/`),
    /// called before the record is made; the core reloads its state there. Returns the outcome + whether init ran.
    ///
    /// # Errors
    /// `Unavailable` (record unreadable | newer), `Conflict`, IO.
    pub fn include(
        &self,
        pick: &ProjectPick,
        init: &dyn Fn() -> Result<(), IpcError>,
    ) -> Result<(IncludeOutcome, bool), IpcError> {
        let needs_init = !real_dir(&self.root.path().join(PERMARK_DIR));
        {
            let inner = lock(&self.inner);
            if Self::keys_of(&inner, pick, &[Candidate::Offered, Candidate::Declined]).is_empty() {
                return Ok((
                    IncludeOutcome {
                        included: 0,
                        undo: None,
                    },
                    false,
                ));
            }
        }
        if needs_init {
            init()?;
        }
        let mut inner = lock(&self.inner);
        let wanted: &[Candidate] = match pick {
            // Include all: the offered ones (declined stay declined)
            ProjectPick::All => &[Candidate::Offered],
            ProjectPick::Paths { .. } => &[Candidate::Offered, Candidate::Declined],
        };
        let keys = Self::keys_of(&inner, pick, wanted);
        let mut picks: Vec<(String, ProjectUid)> = Vec::new();
        for key in keys {
            let uid = self.uid_for(&inner, &key, &picks);
            picks.push((key, uid));
        }
        let record_existed = record::record_path(self.root.path()).is_file();
        let answer_before = inner.inputs.answer;
        let declined_before = inner.inputs.declined.clone();
        // listed already (refused before): included by lifting the refusal; undo puts the refusal back, never the entry
        let listed: HashSet<String> = picks
            .iter()
            .filter(|(k, _)| inner.inputs.entry_by_key(k).is_some())
            .map(|(k, _)| k.clone())
            .collect();
        let mut after = self.apply_event(&mut inner, &Event::Include(picks.clone()), true)?;
        let now = inner.inputs.derive();
        let included = picks
            .iter()
            .filter(|(k, _)| {
                matches!(
                    now.get(k),
                    Some(Candidate::Included { .. } | Candidate::Missing { .. })
                )
            })
            .count();
        let uids: Vec<ProjectUid> = picks
            .into_iter()
            .filter(|(k, _)| !listed.contains(k))
            .map(|(_, u)| u)
            .filter(|u| inner.inputs.entry(u).is_some())
            .collect();
        let token = inner.seq;
        let included = u32::try_from(included).unwrap_or(u32::MAX);
        inner.undo = (included > 0).then_some(IncludeUndo {
            token,
            uids,
            created_record: !record_existed,
            created_permark: needs_init,
            answer_before,
            declined_before,
        });
        after.changed = true;
        drop(inner);
        self.finish(&after);
        Ok((
            IncludeOutcome {
                included,
                undo: (included > 0).then_some(token),
            },
            needs_init,
        ))
    }

    /// The uid a project gets at include (`projects.md` §1.2): its card id, the id an inner record gives it, a draw.
    fn uid_for(&self, inner: &Inner, key: &str, picked: &[(String, ProjectUid)]) -> ProjectUid {
        let used = |u: &ProjectUid| {
            inner.inputs.entry(u).is_some()
                || picked.iter().any(|(_, p)| p == u)
                || inner.removed.contains(u)
        };
        if let Some(entry) = inner.inputs.entry_by_key(key) {
            // listed already (refused before): its own id
            return entry.uid.clone();
        }
        let Some(found) = inner.inputs.found.get(key) else {
            return ProjectUid::draw(key, &used);
        };
        let abs = abs_of(self.root.path(), &found.path);
        let card_id = matches!(found.marker, ProjectMarker::Permark | ProjectMarker::Both)
            .then(|| card::file_of(&self.dirs, &abs).and_then(|f| f.read().id))
            .flatten();
        card_id
            .filter(|u| !used(u))
            .or_else(|| found.inner_uid.clone().filter(|u| !used(u)))
            .unwrap_or_else(|| ProjectUid::draw(key, &used))
    }

    /// Takes back the last include when `token` is still its token: entries out, record removed when it made it and
    /// it is empty, the answer as before. `.permark/` is left to the core ([`UndoPlan`]).
    ///
    /// # Errors
    /// Record write failures.
    pub fn undo(&self, token: u64) -> Result<UndoPlan, IpcError> {
        let mut inner = lock(&self.inner);
        let Some(undo) = inner.undo.clone().filter(|u| u.token == token) else {
            return Ok(UndoPlan {
                undone: false,
                created_permark: false,
            });
        };
        let ops: Vec<RecordOp> = undo
            .uids
            .iter()
            .filter(|u| inner.inputs.entry(u).is_some())
            .map(|uid| RecordOp::Remove { uid: uid.clone() })
            .collect();
        if !ops.is_empty() {
            self.write_record(&ops, false)?;
        }
        inner.record = read_record(self.root.path());
        let empty = inner
            .record
            .record()
            .is_some_and(|r| r.entries.is_empty() && r.skipped.is_empty());
        if undo.created_record && empty {
            let path = record::record_path(self.root.path());
            self.own
                .expect_removals(&[".permark/projects.toml".to_owned()]);
            std::fs::remove_file(&path)
                .map_err(|e| IpcError::io(path.display().to_string(), &e))?;
            inner.record = RecordRead::Absent;
        }
        inner.inputs.answer = undo.answer_before;
        inner.inputs.declined.clone_from(&undo.declined_before);
        for uid in &undo.uids {
            inner.inputs.agent_off.remove(uid);
        }
        self.save_membership(&inner)?;
        self.entries_from_record(&mut inner);
        self.presence(&mut inner);
        let mut after = After {
            changed: true,
            ..After::default()
        };
        let set = member_set(&inner);
        if set != inner.member_set {
            self.explorer.set_members(&set);
            inner.member_set = set;
            after.rescan = true;
        }
        inner.undo = None;
        inner.seq += 1;
        self.rebuild_map(&inner);
        drop(inner);
        self.finish(&after);
        Ok(UndoPlan {
            undone: true,
            created_permark: undo.created_permark,
        })
    }

    /// A person's change of the list other than include | undo.
    fn person(&self, event: &Event) -> Result<(), IpcError> {
        let mut inner = lock(&self.inner);
        let after = self.apply_event(&mut inner, event, false)?;
        if after.changed {
            inner.undo = None;
        }
        drop(inner);
        self.finish(&after);
        Ok(())
    }

    /// "No thanks": `All` on the first question declines the root; paths decline those projects.
    ///
    /// # Errors
    /// Profile write failure.
    pub fn decline(&self, pick: &ProjectPick) -> Result<(), IpcError> {
        let event = match pick {
            ProjectPick::All => Event::DeclineAll,
            ProjectPick::Paths { .. } => {
                let inner = lock(&self.inner);
                Event::Decline(Self::keys_of(&inner, pick, &[Candidate::Offered]))
            }
        };
        self.person(&event)
    }

    /// # Errors
    /// `Unavailable`, `Conflict`, IO.
    pub fn leave_out(&self, uids: &[ProjectUid]) -> Result<(), IpcError> {
        self.person(&Event::LeaveOut(uids.to_vec()))
    }

    /// # Errors
    /// `Unavailable`, `Conflict`, IO.
    pub fn set_hidden(&self, uid: &ProjectUid, rule: HiddenRule) -> Result<(), IpcError> {
        self.person(&Event::SetHidden(uid.clone(), rule))
    }

    /// # Errors
    /// Profile write failure.
    pub fn set_agent(&self, uid: &ProjectUid, choice: AgentChoice) -> Result<(), IpcError> {
        self.person(&Event::SetAgent(uid.clone(), choice))
    }

    /// # Errors
    /// `Unavailable`, `Conflict`, IO.
    pub fn remove(&self, uids: &[ProjectUid]) -> Result<(), IpcError> {
        self.person(&Event::Remove(uids.to_vec()))
    }

    /// Locate…: the missing entry `uid` is the project at `abs` (a folder the person picked).
    ///
    /// # Errors
    /// `Unavailable`, `Conflict`, IO.
    pub fn locate(&self, uid: &ProjectUid, abs: &Path) -> Result<Option<LocateRefusal>, IpcError> {
        let vault = crate::paths::Vault::new(self.root.clone());
        let Some(path) = vault
            .relativize(abs)
            .filter(|p| !p.is_root() && !p.as_str().starts_with(crate::paths::WORKTREE_PREFIX))
        else {
            return Ok(Some(LocateRefusal::Outside));
        };
        if through_link(self.root.path(), &path) {
            return Ok(Some(LocateRefusal::Link));
        }
        if marker_at(&abs_of(self.root.path(), &path)).is_none() {
            return Ok(Some(LocateRefusal::NotProject));
        }
        {
            let inner = lock(&self.inner);
            let key = path.key();
            if inner
                .inputs
                .entries
                .iter()
                .any(|e| e.path.key() == key && &e.uid != uid)
            {
                return Ok(Some(LocateRefusal::Taken));
            }
        }
        self.person(&Event::SetPath(uid.clone(), path))?;
        Ok(None)
    }

    /// Where Locate… starts for a project last seen at `path`: its old parent folder when still there (inside the
    /// root, reached without a link), else the opened folder.
    fn locate_from(&self, path: &str) -> String {
        let parent = VaultPath::parse(path)
            .ok()
            .and_then(|p| p.parent())
            .filter(|p| !p.is_root() && !through_link(self.root.path(), p))
            .map(|p| abs_of(self.root.path(), &p))
            .filter(|abs| real_dir(abs));
        parent
            .as_deref()
            .unwrap_or(self.root.path())
            .display()
            .to_string()
    }

    /// The move question's answer: `same` = it moved (the entry follows), else the candidate stays a new project and
    /// is not asked about again this session.
    ///
    /// # Errors
    /// `Unavailable`, `Conflict`, IO.
    pub fn answer_move(&self, uid: &ProjectUid, path: &str, same: bool) -> Result<(), IpcError> {
        let path = VaultPath::parse(path)
            .map_err(|_| IpcError::new(ErrorKind::InvalidPath).subject(path.to_owned()))?;
        if same {
            let mut inner = lock(&self.inner);
            let was_missing = inner.inputs.presence.get(uid) == Some(&Presence::Gone);
            let mut after = self.apply_event(
                &mut inner,
                &Event::SetPath(uid.clone(), path.clone()),
                false,
            )?;
            if after.changed {
                inner.undo = None;
                inner.ask = self.move_question(&inner);
                if was_missing && inner.inputs.included().contains(uid) {
                    after.news.push(ProjectEvent::Back {
                        name: Self::name_of(&inner, uid, path.as_str()),
                        path: path.as_str().to_owned(),
                    });
                }
            }
            drop(inner);
            self.finish(&after);
        } else {
            let mut inner = lock(&self.inner);
            inner.rejected.insert((uid.clone(), path.key()));
            // the next candidate, if any (clones: one at a time)
            inner.ask = self.move_question(&inner);
            inner.seq += 1;
            self.rebuild_map(&inner);
            drop(inner);
            self.finish(&After {
                changed: true,
                ..After::default()
            });
        }
        Ok(())
    }

    /// Writes one field of a card (the root's or a member's); returns the card now.
    ///
    /// # Errors
    /// `NotFound` (no such member), `InvalidData`, `Unavailable`, `Conflict`, IO.
    pub fn card_set(&self, target: &CardTarget, change: &CardChange) -> Result<CardDto, IpcError> {
        let mut inner = lock(&self.inner);
        let (abs, uid, rel) =
            match target {
                CardTarget::Root => {
                    let id =
                        self.root_card(&mut inner).0.id.clone().unwrap_or_else(|| {
                            ProjectUid::draw(self.root.key().as_str(), &|_| false)
                        });
                    (self.root.path().to_path_buf(), id, String::new())
                }
                CardTarget::Member { uid } => {
                    let entry = inner
                        .inputs
                        .entry(uid)
                        .filter(|_| inner.res.contains_key(uid))
                        .ok_or_else(|| {
                            IpcError::new(ErrorKind::NotFound).subject(uid.as_str().to_owned())
                        })?;
                    (
                        abs_of(self.root.path(), &entry.path),
                        uid.clone(),
                        entry.path.as_str().to_owned(),
                    )
                }
            };
        let file = card::file_of(&self.dirs, &abs)
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject("card"))?;
        let own = Arc::clone(&self.own);
        let rel_card = if rel.is_empty() {
            ".permark/project.toml".to_owned()
        } else {
            format!("{rel}/.permark/project.toml")
        };
        file.write(&uid, change, &|_, bytes| {
            own.expect_write(&rel_card, crate::fsx::hash(bytes));
        })?;
        let card = file.read();
        let name = if rel.is_empty() {
            self.root.name()
        } else {
            folder_name(&rel)
        };
        let dto = card::dto(&card, &name, file.place());
        match target {
            CardTarget::Root => inner.root_card = Some((card, file.place())),
            CardTarget::Member { uid } => {
                if let Some(res) = inner.res.get_mut(uid) {
                    res.card = card;
                    res.place = Some(file.place());
                }
            }
        }
        inner.seq += 1;
        self.rebuild_map(&inner);
        drop(inner);
        self.finish(&After {
            changed: true,
            ..After::default()
        });
        Ok(dto)
    }

    /// Everything Settings > Projects and the strip show.
    #[must_use]
    #[allow(clippy::too_many_lines)] // one row per candidate, every field of `ProjectRow`
    pub fn view(&self) -> ProjectsView {
        let mut inner = lock(&self.inner);
        let root_card = self.root_card(&mut inner).clone();
        let states = inner.inputs.derive();
        let keys: HashSet<&str> = states.keys().map(String::as_str).collect();
        let mut rows = Vec::new();
        let mut offered = OfferSummary::default();
        for (key, state) in &states {
            let entry = inner.inputs.entries.iter().find(|e| &e.path.key() == key);
            let found = inner.inputs.found.get(key);
            let path = found
                .map(|f| f.path.as_str().to_owned())
                .or_else(|| entry.map(|e| e.path.as_str().to_owned()))
                .unwrap_or_default();
            let res = entry.and_then(|e| inner.res.get(&e.uid));
            let dto = card::dto(
                &res.map(|r| r.card.clone()).unwrap_or_default(),
                &folder_name(&path),
                res.and_then(|r| r.place).unwrap_or(CardPlace::Profile),
            );
            let (row_state, missing, marker) = match state {
                Candidate::Included { marker, .. } => (MemberState::Included, None, Some(*marker)),
                Candidate::Missing { reason, .. } => (MemberState::Missing, Some(*reason), None),
                Candidate::Offered => (MemberState::Offered, None, found.map(|f| f.marker)),
                Candidate::Declined => (MemberState::Declined, None, found.map(|f| f.marker)),
            };
            let mut parent = None;
            let mut end = 0;
            while let Some(i) = key[end..].find('/') {
                end += i;
                if keys.contains(&key[..end]) {
                    parent = Some(key[..end].to_owned());
                }
                end += 1;
            }
            let inner_count = res
                .filter(|r| r.has_record)
                .map(|r| u32::try_from(r.inner.len()).unwrap_or(u32::MAX))
                .or_else(|| {
                    let n = inner
                        .inputs
                        .found
                        .values()
                        .filter(|f| {
                            f.inner_uid.is_some()
                                && within(&f.path.key(), key)
                                && f.path.key() != *key
                        })
                        .count();
                    (n > 0).then(|| u32::try_from(n).unwrap_or(u32::MAX))
                });
            if row_state == MemberState::Offered {
                offered.count += 1;
                if offered.names.len() < 3 {
                    offered.names.push(dto.name.clone());
                }
                if let Some(count) = inner_count {
                    offered.inner.push(InnerLine {
                        name: dto.name.clone(),
                        count,
                    });
                }
            }
            let locate_from = (row_state == MemberState::Missing).then(|| self.locate_from(&path));
            rows.push(ProjectRow {
                key: key.clone(),
                uid: entry.map(|e| e.uid.clone()),
                path,
                state: row_state,
                missing,
                marker,
                name: dto.name,
                short: dto.short,
                description: dto.description,
                category: dto.category,
                hidden: entry.map_or(HiddenRule::Own, |e| e.hidden),
                hidden_unknown: entry.is_some_and(|e| {
                    inner.record.record().is_some_and(|r| {
                        r.entries.iter().any(|x| x.uid == e.uid && x.hidden_unknown)
                    })
                }),
                agent: if entry.is_some_and(|e| inner.inputs.agent_off.contains(&e.uid)) {
                    AgentChoice::Off
                } else {
                    AgentChoice::Same
                },
                parent,
                inner: inner_count,
                card_place: res.and_then(|r| r.place),
                locate_from,
                category_status: None,
                local_tags: 0,
                local_categories: 0,
            });
        }
        let record = match &inner.record {
            RecordRead::Absent => RecordStatus::Absent,
            RecordRead::Read(_) => RecordStatus::Ok,
            RecordRead::Newer(_) => RecordStatus::Newer,
            RecordRead::Unreadable { line, message } => RecordStatus::Unreadable {
                line: *line,
                message: message.clone(),
            },
        };
        let mut skipped: Vec<SkippedEntry> = inner
            .record
            .record()
            .map(|r| r.skipped.clone())
            .unwrap_or_default();
        skipped.extend(inner.disk_skipped.iter().copied());
        skipped.sort_by_key(|s| s.line);
        let present = !matches!(inner.record, RecordRead::Absent) || !rows.is_empty();
        let view = ProjectsView {
            seq: inner.seq,
            present,
            ready: inner.ready,
            answer: inner.inputs.effective_answer(),
            rows,
            offered,
            record,
            skipped,
            ask: inner.ask.clone(),
            root_card: card::dto(&root_card.0, &self.root.name(), root_card.1),
            root_category_status: None,
            vocabulary: VocabularyView {
                file: VocabularyStatus::Absent,
                active: false,
                tags: Vec::new(),
                categories: Vec::new(),
            },
        };
        drop(inner);
        self.with_vocabulary(view)
    }

    /// Official lists + status marks on a view (`projects.md` §7.2, §12.2 Tags row).
    fn with_vocabulary(&self, mut view: ProjectsView) -> ProjectsView {
        let vocab = self.vocabularies();
        let map = self.snapshot();
        let entries = |kind: VocabKind| -> Vec<VocabularyEntry> {
            vocab
                .official
                .vocab()
                .map(|v| {
                    v.list(kind)
                        .iter()
                        .map(|(name, style)| VocabularyEntry {
                            name: name.clone(),
                            color: style.color.clone(),
                            icon: style.icon.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        view.vocabulary = VocabularyView {
            file: match &vocab.official {
                VocabRead::Absent => VocabularyStatus::Absent,
                VocabRead::Read(_) => VocabularyStatus::Ok,
                VocabRead::Newer(_) => VocabularyStatus::Newer,
                VocabRead::Unreadable { line, message } => VocabularyStatus::Unreadable {
                    line: *line,
                    message: message.clone(),
                },
            },
            active: vocab.active(),
            tags: entries(VocabKind::Tag),
            categories: entries(VocabKind::Category),
        };
        let status = |category: Option<&str>, chain: &[ProjectUid]| -> Option<TagStatus> {
            vocab
                .status(VocabKind::Category, category?, chain)
                .map(|(s, _)| s)
        };
        view.root_category_status = status(view.root_card.category.as_deref(), &[]);
        let official = vocab.official.vocab();
        for row in &mut view.rows {
            if row.state != MemberState::Included {
                continue;
            }
            let chain: Vec<ProjectUid> =
                map.chain(&row.path).iter().map(|m| m.uid.clone()).collect();
            row.category_status = status(row.category.as_deref(), &chain);
            if let Some(own) = row.uid.as_ref().and_then(|u| vocab.local.get(u)) {
                let count = |kind: VocabKind| {
                    let n = own
                        .list(kind)
                        .keys()
                        .filter(|name| official.is_none_or(|o| !o.list(kind).contains_key(*name)))
                        .count();
                    u32::try_from(n).unwrap_or(u32::MAX)
                };
                row.local_tags = count(VocabKind::Tag);
                row.local_categories = count(VocabKind::Category);
            }
        }
        view
    }

    /// The opened folder's official lists + each included project's own (read on first use; rebuilt after a map
    /// change).
    #[must_use]
    pub fn vocabularies(&self) -> Arc<Vocabularies> {
        let mut inner = lock(&self.inner);
        if let Some((seq, vocab)) = &inner.vocab {
            if *seq == inner.seq {
                return Arc::clone(vocab);
            }
        }
        if inner.vocab_root.is_none() {
            inner.vocab_root = Some(vocabulary::read(self.root.path()));
        }
        let map = self.snapshot();
        let local = map
            .members()
            .iter()
            .filter_map(|m| {
                let res = inner.res.get(&m.uid)?;
                (!res.vocab.is_empty()).then(|| (m.uid.clone(), res.vocab.clone()))
            })
            .collect();
        let vocab = Arc::new(Vocabularies {
            official: inner.vocab_root.clone().unwrap_or_default(),
            local,
        });
        inner.vocab = Some((inner.seq, Arc::clone(&vocab)));
        vocab
    }

    /// Adds a tag | category to the opened folder's official list (`projects.md` §7.2 promote): `[tags.<name>]` |
    /// `[categories.<name>]` appended (the local look copied when a project lists it with one); nothing else of the
    /// file changes. Already official: nothing to do.
    ///
    /// # Errors
    /// `InvalidData` (empty name); `Unavailable` (no `.permark/` here, list unreadable | newer, a link in the way,
    /// a reader with no window); `Conflict` (changed while saving); IO.
    pub fn promote(&self, kind: VocabKind, name: &str) -> Result<(), IpcError> {
        let unavailable = || IpcError::new(ErrorKind::Unavailable).subject("vocabulary");
        let name = crate::organizing::normalize::normalize_tag(name)
            .ok_or_else(|| IpcError::new(ErrorKind::InvalidData).subject("name"))?;
        if self.files_only || !real_dir(&self.root.path().join(PERMARK_DIR)) {
            return Err(unavailable());
        }
        let vocab = self.vocabularies();
        if !matches!(vocab.official, VocabRead::Absent | VocabRead::Read(_)) {
            return Err(unavailable());
        }
        let style = vocab.local_style(kind, &name).cloned().unwrap_or_default();
        let path = self
            .root
            .path()
            .join(PERMARK_DIR)
            .join(vocabulary::VOCABULARY_FILE);
        if crate::tree::is_link_meta_of(&path) {
            return Err(unavailable());
        }
        let old = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(IpcError::io("vocabulary.toml", &e)),
        };
        let (bom, text) = match old.as_deref() {
            None => (false, ""),
            Some(bytes) => {
                let (bom, rest) = bytes
                    .strip_prefix(BOM)
                    .map_or((false, bytes), |rest| (true, rest));
                (bom, std::str::from_utf8(rest).map_err(|_| unavailable())?)
            }
        };
        let Some(new_text) = vocabulary::promote_text(text, kind, &name, &style)
            .map_err(|m| unavailable().detail(m))?
        else {
            return Ok(());
        };
        let mut bytes = Vec::with_capacity(new_text.len() + 3);
        if bom {
            bytes.extend_from_slice(BOM);
        }
        bytes.extend_from_slice(new_text.as_bytes());
        let hash = crate::fsx::hash(&bytes);
        self.own.expect_write(".permark/vocabulary.toml", hash);
        let written =
            crate::fsx::write_atomic_checked(&path, &bytes, || match std::fs::read(&path) {
                Ok(now) => Ok(old.as_deref() == Some(now.as_slice())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(old.is_none()),
                Err(e) => Err(e),
            });
        let written = match written {
            Ok(written) => written,
            Err(e) => {
                self.own.retract_write(".permark/vocabulary.toml", hash);
                return Err(IpcError::io("vocabulary.toml", &e));
            }
        };
        if !written {
            self.own.retract_write(".permark/vocabulary.toml", hash);
            return Err(IpcError::new(ErrorKind::Conflict).subject("vocabulary"));
        }
        {
            let mut inner = lock(&self.inner);
            inner.vocab_root = Some(vocabulary::parse(&bytes));
            inner.seq += 1;
            self.rebuild_map(&inner);
        }
        self.finish(&After {
            changed: true,
            ..After::default()
        });
        Ok(())
    }
}

/// Waits for the first walk without walking (the tree thread does) and without holding the service: `None` when the
/// core closes meanwhile. Walks itself only when no walk started within [`WALK_START_WAIT`]. Once built, the markers
/// are read at once (a rescan running holds the worker, never the service).
fn wait_walk(explorer: &Explorer, stopped: &AtomicBool) -> Option<Vec<crate::tree::Marker>> {
    let start = Instant::now();
    while !explorer.wait_built(WALK_POLL) {
        if stopped.load(Ordering::Relaxed) {
            return None;
        }
        if start.elapsed() > WALK_START_WAIT && !explorer.walking() {
            break;
        }
    }
    if stopped.load(Ordering::Relaxed) {
        return None;
    }
    Some(explorer.markers())
}

/// The agent rules of the folders above `root` that govern it (`projects.md` §5.2 opened alone): for each ancestor
/// whose record lists `root` or a folder containing it, the ancestor's own rules + the rules of each listed folder on
/// the chain (strictly above `root`). One stat per level; records + rule files read only where a record exists.
fn ancestor_rules(dirs: &AppDirs, root: &Path) -> Vec<AncestorRules> {
    let mut found: Vec<AncestorRules> = Vec::new();
    let mut rel: Vec<String> = Vec::new();
    let mut dir = root.to_path_buf();
    let push = |found: &mut Vec<AncestorRules>, base: PathBuf, rel: String| {
        if found.iter().any(|a| a.base == base) {
            return;
        }
        let rules = load_member(dirs, &base).agent;
        found.push(AncestorRules { base, rel, rules });
    };
    for _ in 0..MAX_ANCESTORS {
        let Some(name) = dir.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            break;
        };
        rel.insert(0, name);
        let Some(parent) = dir.parent().map(Path::to_path_buf) else {
            break;
        };
        dir = parent;
        probe::hit();
        if !record::record_path(&dir).is_file() {
            continue;
        }
        let joined = rel.join("/");
        let Ok(path) = VaultPath::parse(&joined) else {
            continue;
        };
        let key = path.key();
        let read = read_record(&dir);
        let Some(record) = read.record() else {
            continue;
        };
        let chain: Vec<&VaultPath> = record
            .entries
            .iter()
            .map(|e| &e.path)
            .filter(|p| within(&key, &p.key()))
            .collect();
        if chain.is_empty() {
            continue;
        }
        push(&mut found, dir.clone(), joined.clone());
        for member in chain {
            let depth = member.segments().count();
            let below: Vec<&str> = joined.split('/').skip(depth).collect();
            if below.is_empty() {
                // the opened folder itself: its own rules hold already
                continue;
            }
            push(&mut found, abs_of(&dir, member), below.join("/"));
        }
    }
    found
}

/// The first commit the record keeps for an entry.
fn first_commit_of(inner: &Inner, uid: &ProjectUid) -> Option<String> {
    inner
        .record
        .record()?
        .entries
        .iter()
        .find(|e| &e.uid == uid)?
        .first_commit
        .clone()
}

/// Whether a learned root commit (full id) is the record's first commit (full or abbreviated, any case).
fn commit_matches(learned: Option<&Option<String>>, first: &str) -> bool {
    learned.and_then(Option::as_deref).is_some_and(|full| {
        first.len() >= 4
            && full
                .to_ascii_lowercase()
                .starts_with(&first.to_ascii_lowercase())
    })
}

/// The member set the ignore rules need, from the included members.
fn member_set(inner: &Inner) -> MemberSet {
    let mut members: Vec<(String, MemberRule)> = inner
        .inputs
        .entries
        .iter()
        .filter(|e| !inner.inputs.refused(&e.path.key()))
        .filter_map(|e| match inner.inputs.presence.get(&e.uid) {
            Some(Presence::Here(marker)) => Some((
                // the disk spelling when the walk saw it (the member's own ignore list is matched under it)
                inner.inputs.found.get(&e.path.key()).map_or_else(
                    || e.path.as_str().to_owned(),
                    |f| f.path.as_str().to_owned(),
                ),
                MemberRule {
                    hidden: e.hidden,
                    git: marker.git(),
                    ignore: inner
                        .res
                        .get(&e.uid)
                        .map(|r| r.ignore.clone())
                        .unwrap_or_default(),
                },
            )),
            _ => None,
        })
        .collect();
    members.sort_by(|a, b| a.0.cmp(&b.0));
    MemberSet { members }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
