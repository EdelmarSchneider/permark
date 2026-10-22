//! The activity trail of a project: a private journal of what the person did in PerMark (files created, moved or
//! deleted, sections saved, conversions, comments, tags, review choices), the changes made outside PerMark (from the
//! watch, never with a guessed author) and the hand-off checkpoints. Facts only.
//!
//! * [`ActivityLog`]: the service of one project (`ProjectCore`; every window on the folder shares it). Plugins append
//!   through it; a plugin that is off appends nothing.
//! * [`net`]: what a run of entries amounts to (create + delete -> nothing, rename chains, one `edited` per file) and
//!   the check of that against the disk now. Pure.
//! * [`handoff`]: sessions, staleness and the default scope of a hand-off (S1..S7). Pure.

pub mod handoff;
pub mod net;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::state::{Loaded, StateData, StateFile, StateStore};
use crate::watch::{ChangeKind, FsBatch};

pub const HOUR_MS: u64 = 60 * 60 * 1000;
pub const DAY_MS: u64 = 24 * HOUR_MS;
/// Entries older than this are dropped.
pub const KEEP_MS: u64 = 30 * DAY_MS;
/// At most this many entries are kept (changes made outside go first, then the oldest).
pub const KEEP_ENTRIES: usize = 5_000;
/// A pause longer than this between two of the person's actions starts a new session.
pub const SESSION_GAP_MS: u64 = 2 * HOUR_MS;
/// A session that ended this long before a hand-off is stale (offered unchecked).
pub const STALE_MS: u64 = 7 * DAY_MS;
/// Up to this many files changed outside are listed one by one; more are counted per folder.
pub const OUTSIDE_DETAIL_FILES: usize = 50;
/// The journal file (the project's private state folder).
pub const JOURNAL_FILE: &str = "activity.json";
/// How long the journal waits for more entries before it writes them.
const FLUSH_DELAY: Duration = Duration::from_millis(500);

/// Milliseconds since the epoch, now (tests give their own clock).
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The system clock.
#[must_use]
pub fn system_clock() -> Clock {
    Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    })
}

/// Who did it: the person in PerMark, or something outside PerMark (an agent, an editor, git; never guessed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ActivityBy")]
pub enum By {
    Person,
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ActivityFileOp")]
pub enum FileOp {
    New,
    /// To `path` from `from` (same or another folder).
    Move,
    Delete,
    Import,
    /// A deleted file came back (undo of a delete).
    Restore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ActivityCommentOp")]
pub enum CommentOp {
    Add,
    Reply,
    Done,
    Reopen,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ActivityReviewOp")]
pub enum ReviewOp {
    Keep,
    Revert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ActivityOutside")]
pub enum OutsideChange {
    Created,
    Modified,
    Deleted,
    /// To `path` from `from`.
    Renamed,
}

/// One fact. Paths are project paths (`/`), relative: a project moved on disk keeps its journal valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, rename = "ActivityFact")]
pub enum Fact {
    File {
        op: FileOp,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        /// Done by an undo (the operation it takes back is in the journal before it).
        #[serde(default, skip_serializing_if = "is_false")]
        undo: bool,
    },
    /// The person saved a file: the sections the change touched (heading paths `A > B`; `(top)` before the first
    /// heading) and how many changed blocks the save carried.
    Save {
        path: String,
        sections: Vec<String>,
        blocks: u32,
    },
    /// A conversion of `path` to the format `to`: copied (`saved` none) or saved as a new file.
    Convert {
        path: String,
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        saved: Option<String>,
    },
    Comment {
        op: CommentOp,
        path: String,
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heading: Option<String>,
    },
    Tag {
        added: bool,
        path: String,
        tag: String,
    },
    Review {
        op: ReviewOp,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        section: Option<String>,
    },
    Outside {
        change: OutsideChange,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
    /// The person handed off to an agent: what came before is in the package.
    HandOff {
        /// Rows the package held.
        rows: u32,
    },
    /// The project was closed (a session never spans it).
    Closed,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde hands the field by reference
fn is_false(value: &bool) -> bool {
    !*value
}

impl Fact {
    /// The project path the fact is about (none for markers).
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::File { path, .. }
            | Self::Save { path, .. }
            | Self::Convert { path, .. }
            | Self::Comment { path, .. }
            | Self::Tag { path, .. }
            | Self::Review { path, .. }
            | Self::Outside { path, .. } => Some(path),
            Self::HandOff { .. } | Self::Closed => None,
        }
    }

    /// A marker (hand-off, close) rather than an action.
    #[must_use]
    pub fn is_marker(&self) -> bool {
        matches!(self, Self::HandOff { .. } | Self::Closed)
    }

    /// The same fact with its paths seen from another folder (`rebase` answers `None` for a path outside it): `None`
    /// when the main path is outside; a `from` outside is dropped (a move into the folder reads as new there).
    #[must_use]
    pub fn rebased(&self, rebase: &dyn Fn(&str) -> Option<String>) -> Option<Self> {
        let mut fact = self.clone();
        match &mut fact {
            Self::File { op, path, from, .. } => {
                *path = rebase(path)?;
                if let Some(old) = from.take() {
                    match rebase(&old) {
                        Some(rel) => *from = Some(rel),
                        None if *op == FileOp::Move => *op = FileOp::New,
                        None => {}
                    }
                }
            }
            Self::Save { path, .. }
            | Self::Comment { path, .. }
            | Self::Tag { path, .. }
            | Self::Review { path, .. } => *path = rebase(path)?,
            Self::Convert { path, saved, .. } => {
                *path = rebase(path)?;
                *saved = saved.as_deref().and_then(rebase);
            }
            Self::Outside { change, path, from } => {
                *path = rebase(path)?;
                if let Some(old) = from.take() {
                    match rebase(&old) {
                        Some(rel) => *from = Some(rel),
                        None => *change = OutsideChange::Created,
                    }
                }
            }
            Self::HandOff { .. } | Self::Closed => return None,
        }
        Some(fact)
    }
}

/// One journal entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ActivityEntry")]
pub struct Entry {
    #[ts(type = "number")]
    pub seq: u64,
    #[ts(type = "number")]
    pub at: u64,
    pub by: By,
    pub fact: Fact,
}

/// Where the last hand-off put its mark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Seq of the `HandOff` entry: everything up to it was handed off.
    pub seq: u64,
    pub at: u64,
    /// HEAD of the repository at the hand-off (git facts of the next one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
}

/// What the person handed off, frozen at confirm: the agent reads exactly what the person saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub seq: u64,
    pub at: u64,
    /// The rows as the agent receives them (rendered by the agent connection).
    pub text: String,
    pub rows: u32,
}

/// `activity.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    /// The next entry's seq (from 1).
    #[serde(default)]
    pub next: u64,
    #[serde(default)]
    pub entries: Vec<Entry>,
    /// Highest seq retention dropped (0: none).
    #[serde(default)]
    pub trimmed_through: u64,
    /// The journal on disk had this older format: its entries were kept aside, read-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgraded_from: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<Checkpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<Package>,
}

impl StateData for Journal {
    const VERSION: u32 = 1;
}

impl Journal {
    /// Entries after the last hand-off (all of them before the first one).
    #[must_use]
    pub fn since_checkpoint(&self) -> &[Entry] {
        let from = self.checkpoint.as_ref().map_or(0, |c| c.seq);
        let at = self.entries.partition_point(|e| e.seq <= from);
        &self.entries[at..]
    }

    fn push(&mut self, at: u64, by: By, fact: Fact) -> u64 {
        let seq = self.next.max(1);
        self.next = seq + 1;
        self.entries.push(Entry { seq, at, by, fact });
        seq
    }

    /// A change outside merges into the one of the same path since the person's last action: created + deleted ->
    /// nothing, created + modified -> created, deleted + created -> modified, renames chained.
    fn push_outside(&mut self, at: u64, change: OutsideChange, path: &str, from: Option<&str>) {
        let key = if change == OutsideChange::Renamed {
            from.unwrap_or(path)
        } else {
            path
        };
        // only the tail of changes outside (since the last action of the person | marker) is merged
        let tail = self
            .entries
            .iter()
            .rposition(|e| e.by != By::Outside)
            .map_or(0, |i| i + 1);
        let found = self.entries[tail..]
            .iter()
            .rposition(|e| e.fact.path() == Some(key))
            .map(|i| i + tail);
        let Some(index) = found else {
            self.push(
                at,
                By::Outside,
                Fact::Outside {
                    change,
                    path: path.to_owned(),
                    from: from.map(str::to_owned),
                },
            );
            return;
        };
        let Fact::Outside {
            change: before,
            from: before_from,
            ..
        } = self.entries[index].fact.clone()
        else {
            return;
        };
        let merged: Option<(OutsideChange, String, Option<String>)> = match (before, change) {
            (OutsideChange::Created, OutsideChange::Deleted) => None,
            (OutsideChange::Created, _) => Some((OutsideChange::Created, path.to_owned(), None)),
            (OutsideChange::Deleted, OutsideChange::Created | OutsideChange::Modified) => {
                Some((OutsideChange::Modified, path.to_owned(), None))
            }
            (OutsideChange::Renamed, OutsideChange::Renamed) => {
                let origin = before_from.unwrap_or_else(|| key.to_owned());
                (origin != path).then(|| (OutsideChange::Renamed, path.to_owned(), Some(origin)))
            }
            (OutsideChange::Renamed, OutsideChange::Deleted) => Some((
                OutsideChange::Deleted,
                before_from.unwrap_or_else(|| path.to_owned()),
                None,
            )),
            (OutsideChange::Renamed, _) => {
                Some((OutsideChange::Renamed, path.to_owned(), before_from))
            }
            (OutsideChange::Modified, OutsideChange::Renamed) => Some((
                OutsideChange::Renamed,
                path.to_owned(),
                Some(key.to_owned()),
            )),
            (_, change) => Some((change, path.to_owned(), from.map(str::to_owned))),
        };
        self.entries.remove(index);
        if let Some((change, path, from)) = merged {
            self.push(at, By::Outside, Fact::Outside { change, path, from });
        }
    }

    /// Retention: older than [`KEEP_MS`] goes; above [`KEEP_ENTRIES`] the changes outside go first, then the oldest.
    fn trim(&mut self, now: u64) {
        let cutoff = now.saturating_sub(KEEP_MS);
        let mut dropped = 0_u64;
        self.entries.retain(|e| {
            let keep = e.at >= cutoff;
            if !keep {
                dropped = dropped.max(e.seq);
            }
            keep
        });
        let mut over = self.entries.len().saturating_sub(KEEP_ENTRIES);
        if over > 0 {
            self.entries.retain(|e| {
                if over > 0 && e.by == By::Outside {
                    over -= 1;
                    dropped = dropped.max(e.seq);
                    false
                } else {
                    true
                }
            });
        }
        let over = self.entries.len().saturating_sub(KEEP_ENTRIES);
        if over > 0 {
            for e in self.entries.drain(..over) {
                dropped = dropped.max(e.seq);
            }
        }
        self.trimmed_through = self.trimmed_through.max(dropped);
    }
}

/// Largest file a save fact reads back for its sections.
pub const SAVE_READ_CAP: u64 = 8 * 1024 * 1024;

fn is_markdown(path: &str) -> bool {
    matches!(
        crate::format::ext_of(path).as_deref(),
        Some("md" | "markdown" | "mdown" | "mkd" | "mdx" | "mdc")
    )
}

/// The fact of a save: `text` = the file after it, `edits` = the line edits it carried (0-based base lines). The
/// sections are the headings around the changed lines of the new text (Markdown only), in document order.
#[must_use]
pub fn save_fact(path: &str, text: &str, edits: &[crate::edit::LineEdit]) -> Fact {
    let mut sorted: Vec<&crate::edit::LineEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| e.at);
    // 1-based ranges of the new text the edits touched
    let mut delta: i64 = 0;
    let mut ranges: Vec<(u32, u32)> = Vec::with_capacity(sorted.len());
    for edit in &sorted {
        let at = i64::from(edit.at) + delta;
        let inserted = i64::try_from(edit.insert.len()).unwrap_or(i64::MAX);
        let first = at.max(0) + 1;
        let last = (at + inserted).max(first);
        ranges.push((
            u32::try_from(first).unwrap_or(u32::MAX),
            u32::try_from(last).unwrap_or(u32::MAX),
        ));
        delta += inserted - i64::from(edit.remove);
    }
    let mut sections: Vec<String> = Vec::new();
    if is_markdown(path) && !ranges.is_empty() {
        let all = crate::format::sections(text);
        let label = |s: &crate::format::Section| {
            if s.heading_path.is_empty() {
                net::TOP.to_owned()
            } else {
                s.heading_path.join(" > ")
            }
        };
        for (first, last) in &ranges {
            // the section the first line is in, then every one that starts inside the range
            let containing = all.iter().rev().find(|s| s.line <= *first);
            let inside = all.iter().filter(|s| s.line > *first && s.line <= *last);
            for s in containing.into_iter().chain(inside) {
                let l = label(s);
                if !sections.contains(&l) {
                    sections.push(l);
                }
            }
            if containing.is_none() && !sections.iter().any(|x| x == net::TOP) {
                sections.insert(0, net::TOP.to_owned());
            }
        }
    }
    Fact::Save {
        path: path.to_owned(),
        sections,
        blocks: u32::try_from(edits.len()).unwrap_or(u32::MAX),
    }
}

/// Hears new entries (the Changes panel).
pub type Listener = Arc<dyn Fn(&Entry) + Send + Sync>;

struct State {
    journal: Journal,
    dirty: bool,
    file: StateFile<Journal>,
    /// The included projects whose own journal also gets the facts about their files (`projects.md` §9.2); `None`:
    /// only this journal (a plain folder, or the owners are changing).
    owners: Option<crate::projects::Owners>,
    /// Facts waiting for the next write, per project journal (its file).
    mirror: Vec<(StateFile<Journal>, Mirrored)>,
}

/// A fact for an included project's own journal, its paths relative to that project.
#[derive(Debug, Clone)]
enum Mirrored {
    Fact {
        at: u64,
        by: By,
        fact: Fact,
    },
    Outside {
        at: u64,
        change: OutsideChange,
        path: String,
        from: Option<String>,
    },
}

struct Inner {
    state: Mutex<State>,
    wake: Condvar,
    stop: AtomicBool,
    clock: Clock,
    listeners: Mutex<Vec<(u64, Listener)>>,
    next_listener: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The version a journal file on disk says it has (`None`: missing | unreadable).
fn version_on_disk(file: &StateFile<Journal>) -> Option<u32> {
    let bytes = std::fs::read(file.path()).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}

/// Reads the journal; another format version is moved aside (kept, never read again) and noted.
fn load(file: &StateFile<Journal>) -> Journal {
    let older = version_on_disk(file).filter(|v| *v != Journal::VERSION);
    match file.load() {
        Loaded::Found(journal) => journal,
        Loaded::Missing => Journal::default(),
        Loaded::Recovered { .. } => Journal {
            upgraded_from: older,
            ..Journal::default()
        },
    }
}

/// How the journal of a project is kept.
enum Mode {
    /// The app: appends, writes (a thread writes shortly after the last change; Drop writes what is left).
    Writing { flusher: Option<JoinHandle<()>> },
    /// `permark mcp` with the app closed: read fresh from the file at every look, never written.
    Reading,
}

/// The activity journal of one project.
pub struct ActivityLog {
    inner: Arc<Inner>,
    mode: Mode,
}

impl std::fmt::Debug for ActivityLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivityLog").finish_non_exhaustive()
    }
}

impl ActivityLog {
    /// The journal of the app's project: read once, written shortly after each change.
    #[must_use]
    pub fn open(store: &StateStore, clock: Clock) -> Self {
        let file = store.file::<Journal>(JOURNAL_FILE);
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                journal: load(&file),
                dirty: false,
                file,
                owners: None,
                mirror: Vec::new(),
            }),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            clock,
            listeners: Mutex::new(Vec::new()),
            next_listener: AtomicU64::new(1),
        });
        let weak = Arc::downgrade(&inner);
        let flusher = std::thread::Builder::new()
            .name("activity-journal".to_owned())
            .spawn(move || flush_loop(&weak))
            .ok();
        Self {
            inner,
            mode: Mode::Writing { flusher },
        }
    }

    /// The journal for a reader with no window: never written.
    #[must_use]
    pub fn read_only(store: &StateStore, clock: Clock) -> Self {
        let file = store.file::<Journal>(JOURNAL_FILE);
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    journal: Journal::default(),
                    dirty: false,
                    file,
                    owners: None,
                    mirror: Vec::new(),
                }),
                wake: Condvar::new(),
                stop: AtomicBool::new(true),
                clock,
                listeners: Mutex::new(Vec::new()),
                next_listener: AtomicU64::new(1),
            }),
            mode: Mode::Reading,
        }
    }

    fn writing(&self) -> bool {
        matches!(self.mode, Mode::Writing { .. })
    }

    /// Now, by the journal's clock.
    #[must_use]
    pub fn now(&self) -> u64 {
        (self.inner.clock)()
    }

    /// The project's state folder moved (PerMark was initialized): the journal goes on there (written soon).
    pub fn use_store(&self, store: &StateStore) {
        let mut state = lock(&self.inner.state);
        state.file = store.file::<Journal>(JOURNAL_FILE);
        state.dirty = self.writing();
        self.inner.wake.notify_all();
    }

    /// The owners of state changed (`Projects::follow_owners`): before, what waits is written; after, facts about an
    /// included project's files also go to that project's own journal (the one it uses opened alone). This journal
    /// keeps every fact too: the hand-off and the agent's `activity` read it as before (merging = round 5).
    pub fn follow_owners(&self, change: &crate::projects::OwnersChange<'_>) {
        if !self.writing() {
            return;
        }
        match change {
            crate::projects::OwnersChange::Pause => {
                lock(&self.inner.state).owners = None;
                flush(&self.inner);
            }
            crate::projects::OwnersChange::Resume(owners) => {
                lock(&self.inner.state).owners = (!owners.is_single()).then(|| (*owners).clone());
            }
        }
    }

    fn append(&self, by: By, fact: Fact) -> Option<u64> {
        if !self.writing() {
            return None;
        }
        let now = self.now();
        let entry = {
            let mut state = lock(&self.inner.state);
            mirror_fact(&mut state, now, by, &fact);
            let seq = state.journal.push(now, by, fact);
            state.journal.trim(now);
            state.dirty = true;
            self.inner.wake.notify_all();
            state
                .journal
                .entries
                .last()
                .filter(|e| e.seq == seq)
                .cloned()
        };
        if let Some(entry) = &entry {
            self.announce(entry);
        }
        entry.map(|e| e.seq)
    }

    fn announce(&self, entry: &Entry) {
        let listeners: Vec<Listener> = lock(&self.inner.listeners)
            .iter()
            .map(|(_, l)| Arc::clone(l))
            .collect();
        for listener in listeners {
            listener(entry);
        }
    }

    /// Something the person did in PerMark (a reader that never writes ignores it).
    pub fn person(&self, fact: Fact) {
        if fact.is_marker() || matches!(fact, Fact::Outside { .. }) {
            return;
        }
        self.append(By::Person, fact);
    }

    /// The project closes: the next action starts a new session.
    pub fn closed(&self) {
        let last_is_close = lock(&self.inner.state)
            .journal
            .entries
            .last()
            .is_none_or(|e| e.fact == Fact::Closed);
        if !last_is_close {
            self.append(By::Person, Fact::Closed);
        }
    }

    /// Changes the watch verified: the ones made outside PerMark (not the app's own writes) are noted, files only.
    pub fn outside(&self, batch: &FsBatch) {
        if !self.writing() {
            return;
        }
        let now = self.now();
        let mut noted = false;
        {
            let mut state = lock(&self.inner.state);
            for change in batch.changes.iter().filter(|c| !c.own && !c.is_dir) {
                if is_private(&change.path) {
                    continue;
                }
                let (kind, from) = match &change.kind {
                    ChangeKind::Created => (OutsideChange::Created, None),
                    ChangeKind::Modified => (OutsideChange::Modified, None),
                    ChangeKind::Removed => (OutsideChange::Deleted, None),
                    ChangeKind::Renamed { from } => (OutsideChange::Renamed, Some(from.as_str())),
                };
                mirror_outside(&mut state, now, kind, &change.path, from);
                state.journal.push_outside(now, kind, &change.path, from);
                noted = true;
            }
            if noted {
                state.journal.trim(now);
                state.dirty = true;
                self.inner.wake.notify_all();
            }
        }
        if noted {
            let last = lock(&self.inner.state).journal.entries.last().cloned();
            if let Some(entry) = last.filter(|e| e.by == By::Outside) {
                self.announce(&entry);
            }
        }
    }

    /// The journal as it is now (a reader with no window reads the file again).
    #[must_use]
    pub fn snapshot(&self) -> Journal {
        let state = lock(&self.inner.state);
        match self.mode {
            Mode::Writing { .. } => state.journal.clone(),
            Mode::Reading => load(&state.file),
        }
    }

    /// The person confirmed a hand-off of everything up to `upto` (the seq the preview was built from): the checkpoint
    /// and the frozen package. Entries that came after the preview stay for the next hand-off.
    ///
    /// Returns the seq of the hand-off marker (`None`: a reader that never writes).
    #[must_use]
    pub fn hand_off(
        &self,
        upto: u64,
        head: Option<String>,
        text: String,
        rows: u32,
    ) -> Option<u64> {
        if !self.writing() {
            return None;
        }
        let now = self.now();
        let entry = {
            let mut state = lock(&self.inner.state);
            let journal = &mut state.journal;
            // the marker goes at the end: entries that came after the preview (seq > upto) stay for the next hand-off
            let seq = journal.push(now, By::Person, Fact::HandOff { rows });
            let entry = journal.entries.last().cloned().unwrap_or(Entry {
                seq,
                at: now,
                by: By::Person,
                fact: Fact::HandOff { rows },
            });
            // the checkpoint covers what the person saw (seq <= upto)
            journal.checkpoint = Some(Checkpoint {
                seq: upto.max(journal.checkpoint.as_ref().map_or(0, |c| c.seq)),
                at: now,
                head,
            });
            journal.package = Some(Package {
                seq,
                at: now,
                text,
                rows,
            });
            journal.upgraded_from = None;
            journal.trim(now);
            state.dirty = true;
            self.inner.wake.notify_all();
            entry
        };
        self.announce(&entry);
        Some(entry.seq)
    }

    /// Hears every new entry until the returned value is dropped.
    #[must_use]
    pub fn listen(&self, listener: Listener) -> Listening {
        let id = self.inner.next_listener.fetch_add(1, Ordering::Relaxed);
        lock(&self.inner.listeners).push((id, listener));
        Listening {
            inner: Arc::downgrade(&self.inner),
            id,
        }
    }

    /// Writes what is pending now (tests; Drop does it too).
    pub fn flush(&self) {
        flush(&self.inner);
    }
}

/// A listener of the journal; dropping it stops it.
pub struct Listening {
    inner: Weak<Inner>,
    id: u64,
}

impl Drop for Listening {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            lock(&inner.listeners).retain(|(id, _)| *id != self.id);
        }
    }
}

/// `.permark/` and `.git/` are the project's machinery, not its files.
fn is_private(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or_default();
    first.eq_ignore_ascii_case(".permark") || first.eq_ignore_ascii_case(".git")
}

/// The journal file of the included project owning `path` + its slot (`None`: the opened folder owns it).
fn owner_file(state: &State, path: &str) -> Option<(StateFile<Journal>, usize)> {
    let owners = state.owners.as_ref()?;
    let i = owners.owner_of(path);
    (i != 0).then(|| (owners.slots()[i].store.file::<Journal>(JOURNAL_FILE), i))
}

fn mirror_fact(state: &mut State, at: u64, by: By, fact: &Fact) {
    let Some((file, i)) = fact.path().and_then(|p| owner_file(state, p)) else {
        return;
    };
    let Some(owners) = state.owners.clone() else {
        return;
    };
    let rebase = |p: &str| (owners.owner_of(p) == i).then(|| owners.rel(i, p));
    if let Some(fact) = fact.rebased(&rebase) {
        state.mirror.push((file, Mirrored::Fact { at, by, fact }));
        state.dirty = true;
    }
}

fn mirror_outside(
    state: &mut State,
    at: u64,
    change: OutsideChange,
    path: &str,
    from: Option<&str>,
) {
    let Some((file, i)) = owner_file(state, path) else {
        return;
    };
    let Some(owners) = state.owners.clone() else {
        return;
    };
    let rel = owners.rel(i, path);
    let from = from
        .filter(|f| owners.owner_of(f) == i)
        .map(|f| owners.rel(i, f));
    let change = if change == OutsideChange::Renamed && from.is_none() {
        OutsideChange::Created
    } else {
        change
    };
    state.mirror.push((
        file,
        Mirrored::Outside {
            at,
            change,
            path: rel,
            from,
        },
    ));
    state.dirty = true;
}

/// Writes the facts waiting for included projects' journals: each read fresh, appended to, trimmed, written.
fn flush_mirror(mirror: Vec<(StateFile<Journal>, Mirrored)>) {
    let mut by_file: Vec<(StateFile<Journal>, Vec<Mirrored>)> = Vec::new();
    for (file, item) in mirror {
        match by_file.iter_mut().find(|(f, _)| f.path() == file.path()) {
            Some((_, items)) => items.push(item),
            None => by_file.push((file, vec![item])),
        }
    }
    for (file, items) in by_file {
        let mut journal = load(&file);
        let mut last = 0;
        for item in items {
            match item {
                Mirrored::Fact { at, by, fact } => {
                    journal.push(at, by, fact);
                    last = at;
                }
                Mirrored::Outside {
                    at,
                    change,
                    path,
                    from,
                } => {
                    journal.push_outside(at, change, &path, from.as_deref());
                    last = at;
                }
            }
        }
        journal.trim(last);
        // best effort, as this journal's own write
        let _ = file.save(&journal);
    }
}

fn flush(inner: &Inner) {
    let (journal, file, mirror) = {
        let mut state = lock(&inner.state);
        if !state.dirty {
            return;
        }
        state.dirty = false;
        let mirror = std::mem::take(&mut state.mirror);
        (state.journal.clone(), state.file.clone(), mirror)
    };
    // best effort: a journal that cannot be written loses entries, never an action
    let _ = file.save(&journal);
    if !mirror.is_empty() {
        flush_mirror(mirror);
    }
}

fn flush_loop(inner: &Weak<Inner>) {
    let Some(strong) = inner.upgrade() else {
        return;
    };
    let stopped = |s: &Inner| s.stop.load(Ordering::SeqCst);
    loop {
        {
            let state = lock(&strong.state);
            // wait for something to write (or the stop)
            let state = strong
                .wake
                .wait_while(state, |s| !s.dirty && !stopped(&strong))
                .unwrap_or_else(PoisonError::into_inner);
            // a short pause gathers a burst of entries into one write; the stop cuts it short
            let (state, _) = strong
                .wake
                .wait_timeout_while(state, FLUSH_DELAY, |_| !stopped(&strong))
                .unwrap_or_else(PoisonError::into_inner);
            drop(state);
        }
        if stopped(&strong) {
            return;
        }
        flush(&strong);
    }
}

impl Drop for ActivityLog {
    fn drop(&mut self) {
        if let Mode::Writing { flusher } = &mut self.mode {
            self.inner.stop.store(true, Ordering::SeqCst);
            self.inner.wake.notify_all();
            if let Some(thread) = flusher.take() {
                let _ = thread.join();
            }
            flush(&self.inner);
        }
    }
}
