//! File operations in the Explorer: rename, move, delete, new file | folder, with the links of the documents kept
//! working and every operation undoable.
//!
//! * [`names`]: what a name may be;
//! * [`ops`]: checks the file system side (nothing is overwritten, nothing leaves its root, delete goes to the
//!   recycle bin) and does it, putting back what moved when a move fails half way;
//! * [`plan`]: decides which links of which documents change, as exact byte edits;
//! * [`undo`]: the journal;
//! * [`FileOps`]: ties them together for a project: preview, apply (hash-guarded writes, own writes announced to the
//!   watch, the tree synced at once) and undo.

mod external;
mod names;
mod ops;
mod plan;
#[cfg(test)]
mod tests;
mod undo;

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use external::{external_target, OpenMode};
pub use names::{check_name, new_file_name, NameProblem};

use self::ops::{
    announced_pairs, check_deletes, check_moves, execute_moves, io_error, is_protected, locate,
    ValidMove,
};
use self::plan::{apply_edits, invert_edits, mentions, split_root, DocPlan, Planner};
use self::undo::{DocUndo, Entry, Journal, Op, UndoEdit, JOURNAL_FILE};
use crate::error::{ErrorKind, IpcError};
use crate::fsx::{hash, write_atomic, ContentHash};
use crate::paths::{Vault, VaultError, VaultPath};
use crate::state::StateFile;
use crate::tree::{Explorer, Renamed};
use crate::watch::OwnWrites;

/// Documents larger than this are not looked at for links.
const LINK_SCAN_LIMIT: u64 = 4 * 1024 * 1024;
/// Plans kept for `apply_move`.
const KEPT_PLANS: usize = 4;
/// A preview lists at most this many links.
const MAX_PREVIEW_LINKS: usize = 2000;

#[derive(Debug, thiserror::Error)]
pub enum FileOpError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("{0} already exists")]
    Exists(String),
    #[error("invalid name: {}", .0.as_str())]
    InvalidName(NameProblem),
    #[error("{0} can't be moved into itself")]
    IntoItself(String),
    #[error("{0} is in another root: files can't be moved between the project and a worktree")]
    OtherRoot(String),
    #[error("{0} can't be changed here")]
    Protected(String),
    #[error("{path} holds the linked worktree {worktree}, so it can't be moved or deleted here")]
    HoldsWorktree { path: String, worktree: String },
    #[error("{0} overlaps another item of the same operation")]
    Overlap(String),
    #[error("there is nothing to do")]
    Nothing,
    #[error("that preview is no longer available; try again")]
    UnknownPlan,
    #[error("couldn't change {path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("{reason}. These could not be put back and stay where they were moved to: {}", .left.iter().map(|(_, to)| to.as_str()).collect::<Vec<_>>().join(", "))]
    PartlyMoved {
        reason: Box<FileOpError>,
        left: Vec<(String, String)>,
    },
    #[error("couldn't move to the recycle bin: {0}")]
    Trash(String),
}

impl From<FileOpError> for IpcError {
    fn from(error: FileOpError) -> Self {
        match error {
            FileOpError::Vault(e) => e.into(),
            FileOpError::Exists(path) => IpcError::new(ErrorKind::Exists).subject(path),
            FileOpError::InvalidName(problem) => {
                IpcError::new(ErrorKind::InvalidName).detail(problem.as_str())
            }
            FileOpError::IntoItself(path) => IpcError::new(ErrorKind::IntoItself).subject(path),
            FileOpError::OtherRoot(path) => IpcError::new(ErrorKind::OtherRoot).subject(path),
            FileOpError::Protected(path) => IpcError::new(ErrorKind::Protected).subject(path),
            FileOpError::HoldsWorktree { path, worktree } => IpcError::new(ErrorKind::Protected)
                .subject(path)
                .detail(worktree),
            FileOpError::Overlap(path) => IpcError::new(ErrorKind::Overlap).subject(path),
            FileOpError::Nothing | FileOpError::UnknownPlan => {
                IpcError::new(ErrorKind::Unavailable)
            }
            FileOpError::Io { path, source } => IpcError::io(path, &source),
            FileOpError::PartlyMoved { reason, left } => {
                let detail = left
                    .iter()
                    .map(|(_, to)| to.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let inner: IpcError = (*reason).into();
                IpcError::new(ErrorKind::PartlyMoved)
                    .subject(inner.subject.unwrap_or_default())
                    .detail(detail)
            }
            FileOpError::Trash(message) => IpcError::new(ErrorKind::Trash).detail(message),
        }
    }
}

// ---- DTOs --------------------------------------------------------------------------------------------------------

/// A file or folder that moves (or is renamed): project paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MoveSpec {
    pub from: String,
    pub to: String,
}

/// Why a document's links were not updated (or an undo could not restore it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SkipReason {
    /// Too large to look at.
    TooLarge,
    NotUtf8,
    Unreadable,
    /// The file changed after the preview (or since the operation), so it was left alone.
    Changed,
    /// The new version could not be written.
    WriteFailed,
    Missing,
    /// Something else is where the file has to go back to (an undo), or a move it depends on did not go back.
    Occupied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Skip {
    pub path: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanChange {
    pub line: u32,
    pub old: String,
    pub new: String,
}

/// The links of one document that change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanFile {
    pub path: String,
    /// Where the document will be, when it moves too.
    pub moved_to: Option<String>,
    pub changes: Vec<PlanChange>,
}

/// What a move would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MovePlan {
    #[ts(type = "number")]
    pub plan_id: u64,
    pub moves: Vec<MoveSpec>,
    pub link_count: u32,
    pub file_count: u32,
    /// The first documents and links (cut off after a few thousand links).
    pub files: Vec<PlanFile>,
    pub truncated: bool,
    pub skipped: Vec<Skip>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MoveOutcome {
    pub moved: u32,
    /// Links rewritten, in how many documents.
    pub links: u32,
    pub documents: u32,
    pub skipped: Vec<Skip>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeleteOutcome {
    pub deleted: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NewKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum UndoKind {
    Move,
    NewFile,
    NewFolder,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UndoOutcome {
    pub kind: UndoKind,
    /// Items put back.
    pub restored: u32,
    /// What could not be put back, and why.
    pub skipped: Vec<Skip>,
}

// ---- service -----------------------------------------------------------------------------------------------------

struct StoredPlan {
    id: u64,
    moves: Vec<(String, String)>,
    docs: Vec<DocPlan>,
}

#[derive(Default)]
struct Plans {
    next: u64,
    kept: VecDeque<StoredPlan>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The file operations of one project.
pub struct FileOps {
    vault: Vault,
    explorer: Arc<Explorer>,
    own: Arc<OwnWrites>,
    journal: StateFile<Journal>,
    /// One journal read-modify-write at a time.
    journal_lock: Mutex<()>,
    plans: Mutex<Plans>,
}

impl std::fmt::Debug for FileOps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileOps").finish_non_exhaustive()
    }
}

impl FileOps {
    #[must_use]
    pub fn new(
        vault: Vault,
        explorer: Arc<Explorer>,
        own: Arc<OwnWrites>,
        store: &crate::state::StateStore,
    ) -> Self {
        Self {
            vault,
            explorer,
            own,
            journal: store.file::<Journal>(JOURNAL_FILE),
            journal_lock: Mutex::new(()),
            plans: Mutex::new(Plans::default()),
        }
    }

    fn parse(path: &str) -> Result<VaultPath, FileOpError> {
        Ok(VaultPath::parse(path)?)
    }

    fn record(&self, op: Op) {
        let _guard = lock(&self.journal_lock);
        let mut journal = self.journal.load().into_value();
        journal.push(Entry {
            at_ms: now_ms(),
            op,
        });
        // best effort: an operation that cannot be journaled is done all the same, just not undoable
        let _ = self.journal.save(&journal);
    }

    /// Entries the next undo would take back (newest first count).
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        let _guard = lock(&self.journal_lock);
        self.journal.load().into_value().entries.len()
    }

    // ---- move ------------------------------------------------------------------------------------------------

    /// Checks the moves and, with `update_links`, finds the links that have to change. Nothing is touched.
    ///
    /// # Errors
    /// A refusal of [`FileOpError`].
    pub fn plan_move(
        &self,
        moves: &[MoveSpec],
        update_links: bool,
    ) -> Result<MovePlan, FileOpError> {
        let parsed = parse_moves(moves)?;
        let valid = check_moves(&self.vault, &parsed)?;
        let pairs: Vec<(String, String)> = valid
            .iter()
            .map(|m| (m.from.to_string(), m.to.to_string()))
            .collect();
        let (docs, skipped) = if update_links {
            self.plan_links(&pairs)
        } else {
            (Vec::new(), Vec::new())
        };
        let mut plans = lock(&self.plans);
        plans.next += 1;
        let id = plans.next;
        let plan = build_dto(id, &pairs, &docs, skipped);
        plans.kept.push_back(StoredPlan {
            id,
            moves: pairs,
            docs,
        });
        while plans.kept.len() > KEPT_PLANS {
            plans.kept.pop_front();
        }
        Ok(plan)
    }

    fn plan_links(&self, pairs: &[(String, String)]) -> (Vec<DocPlan>, Vec<Skip>) {
        let Some((first, _)) = pairs.first() else {
            return (Vec::new(), Vec::new());
        };
        let root = split_root(first).0.trim_end_matches('/').to_owned();
        let planner = Planner::new(&self.vault, pairs);
        let mut docs = Vec::new();
        let mut skipped = Vec::new();
        for doc in self.explorer.documents(&root) {
            let moved_to = planner.mover().map(&doc);
            let Ok(path) = VaultPath::parse(&doc) else {
                continue;
            };
            let read = locate(&self.vault, &path)
                .ok()
                .map(|abs| read_limited(&abs, LINK_SCAN_LIMIT));
            let bytes = match read {
                Some(Ok(Some(bytes))) => bytes,
                Some(Ok(None)) => {
                    skipped.push(Skip {
                        path: doc,
                        reason: SkipReason::TooLarge,
                    });
                    continue;
                }
                _ => {
                    skipped.push(Skip {
                        path: doc,
                        reason: SkipReason::Unreadable,
                    });
                    continue;
                }
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                skipped.push(Skip {
                    path: doc,
                    reason: SkipReason::NotUtf8,
                });
                continue;
            };
            // a document that stays put matters only when it mentions something that moves
            if moved_to.is_none() && !mentions(text, planner.needles()) {
                continue;
            }
            let edits = planner.edits_for(&doc, text);
            if !edits.is_empty() {
                docs.push(DocPlan {
                    path: doc,
                    moved_to,
                    hash: hash(&bytes),
                    edits,
                });
            }
        }
        (docs, skipped)
    }

    /// Makes the moves of a preview, then rewrites the links (when asked): each document only if it still is what the
    /// preview saw.
    ///
    /// # Errors
    /// `UnknownPlan`, or a refusal | failure of the moves (nothing moved, or what moved is put back).
    pub fn apply_move(&self, plan_id: u64, update_links: bool) -> Result<MoveOutcome, FileOpError> {
        let stored = {
            let mut plans = lock(&self.plans);
            let at = plans.kept.iter().position(|p| p.id == plan_id);
            at.and_then(|at| plans.kept.remove(at))
                .ok_or(FileOpError::UnknownPlan)?
        };
        let parsed: Vec<(VaultPath, VaultPath)> = stored
            .moves
            .iter()
            .map(|(a, b)| Ok((Self::parse(a)?, Self::parse(b)?)))
            .collect::<Result<_, FileOpError>>()?;
        // checked again: the disk may have changed since the preview
        let valid = check_moves(&self.vault, &parsed)?;
        let announced: Vec<(String, String)> = valid.iter().flat_map(announced_pairs).collect();
        self.own.expect_moves(&announced);
        if let Err(error) = execute_moves(&valid) {
            let paths: Vec<String> = announced
                .iter()
                .flat_map(|(a, b)| [a.clone(), b.clone()])
                .collect();
            self.own.retract_moves(&paths);
            self.sync_after_moves(&valid);
            return Err(error);
        }
        let mut skipped = Vec::new();
        let mut undo_docs = Vec::new();
        let mut links = 0_usize;
        if update_links {
            let planner = Planner::new(&self.vault, &stored.moves);
            for doc in &stored.docs {
                self.rewrite_document(doc, &planner, &mut links, &mut undo_docs, &mut skipped);
            }
        }
        let moved = valid.len();
        self.record(Op::Move {
            moves: stored.moves.clone(),
            docs: undo_docs.clone(),
        });
        self.sync_after_moves(&valid);
        Ok(MoveOutcome {
            moved: count(moved),
            links: count(links),
            documents: count(undo_docs.len()),
            skipped,
        })
    }

    fn rewrite_document(
        &self,
        doc: &DocPlan,
        planner: &Planner<'_>,
        links: &mut usize,
        undo_docs: &mut Vec<DocUndo>,
        skipped: &mut Vec<Skip>,
    ) {
        let now_path = planner.mover().map_or_same(&doc.path);
        let skip = |reason| Skip {
            path: now_path.clone(),
            reason,
        };
        let Ok(path) = VaultPath::parse(&now_path) else {
            skipped.push(skip(SkipReason::Missing));
            return;
        };
        let Ok(abs) = locate(&self.vault, &path) else {
            skipped.push(skip(SkipReason::Missing));
            return;
        };
        let bytes = match fs::read(&abs) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                skipped.push(skip(SkipReason::Missing));
                return;
            }
            Err(_) => {
                skipped.push(skip(SkipReason::Unreadable));
                return;
            }
        };
        if hash(&bytes) != doc.hash {
            skipped.push(skip(SkipReason::Changed));
            return;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            skipped.push(skip(SkipReason::NotUtf8));
            return;
        };
        let new_text = apply_edits(text, &doc.edits);
        let new_hash = hash(new_text.as_bytes());
        self.own.expect_write(&now_path, new_hash);
        if write_atomic(&abs, new_text.as_bytes()).is_err() {
            self.own.retract_write(&now_path, new_hash);
            skipped.push(skip(SkipReason::WriteFailed));
            return;
        }
        *links += doc.edits.len();
        let mut causes: Vec<usize> = doc.edits.iter().flat_map(|e| e.causes.clone()).collect();
        causes.sort_unstable();
        causes.dedup();
        undo_docs.push(DocUndo {
            path: now_path,
            original: doc.path.clone(),
            hash_after: new_hash.hex(),
            edits: invert_edits(&doc.edits)
                .into_iter()
                .map(|e| UndoEdit {
                    start: e.start,
                    end: e.end,
                    old: e.new,
                })
                .collect(),
            causes,
            moved_by: planner.mover().move_index_of(&doc.path),
        });
    }

    fn sync_after_moves(&self, valid: &[ValidMove]) {
        let paths: Vec<String> = valid
            .iter()
            .flat_map(|m| [m.from.to_string(), m.to.to_string()])
            .collect();
        let renames: Vec<Renamed> = valid
            .iter()
            .map(|m| Renamed {
                from: m.from.to_string(),
                to: m.to.to_string(),
            })
            .collect();
        self.explorer.sync_paths(&paths, &renames);
    }

    // ---- delete ----------------------------------------------------------------------------------------------

    /// Sends files and folders to the recycle bin.
    ///
    /// # Errors
    /// A refusal, or `Trash` (the files stay where they are).
    pub fn delete(&self, paths: &[String]) -> Result<DeleteOutcome, FileOpError> {
        let parsed: Vec<VaultPath> = paths
            .iter()
            .map(|p| Self::parse(p))
            .collect::<Result<_, _>>()?;
        let checked = check_deletes(&self.vault, &parsed)?;
        let announced: Vec<String> = checked.iter().map(|(p, _)| p.to_string()).collect();
        self.own.expect_removals(&announced);
        let result = trash::delete_all(checked.iter().map(|(_, abs)| abs));
        let shown: Vec<String> = announced.clone();
        // whatever the bin did, the tree shows what the disk now holds
        self.explorer.sync_paths(&shown, &[]);
        if let Err(error) = result {
            self.own.retract_moves(&announced);
            return Err(FileOpError::Trash(error.to_string()));
        }
        self.record(Op::Delete {
            paths: announced.clone(),
        });
        Ok(DeleteOutcome { deleted: announced })
    }

    // ---- new -------------------------------------------------------------------------------------------------

    /// Makes an empty file (`.md` | `.txt`) or a folder (one level) in `dir`. Returns its project path.
    ///
    /// # Errors
    /// Name problems, `Exists`, a missing | protected folder.
    pub fn create(&self, dir: &str, name: &str, kind: NewKind) -> Result<String, FileOpError> {
        let dir = Self::parse(dir)?;
        let name = match kind {
            NewKind::File => new_file_name(name),
            NewKind::Folder => check_name(name).map(|()| name.to_owned()),
        }
        .map_err(FileOpError::InvalidName)?;
        if is_protected(&dir) {
            return Err(FileOpError::Protected(dir.to_string()));
        }
        let parent = self.vault.resolve(&dir)?;
        if !fs::metadata(&parent).is_ok_and(|m| m.is_dir()) {
            return Err(FileOpError::Vault(VaultError::NotFound(dir.to_string())));
        }
        let path = dir.join_name(&name);
        let abs = parent.join(&name);
        if fs::symlink_metadata(&abs).is_ok() {
            return Err(FileOpError::Exists(path.to_string()));
        }
        let shown = path.to_string();
        self.own.expect_creations(std::slice::from_ref(&shown));
        let made = match kind {
            NewKind::File => fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&abs)
                .map(|_| ()),
            NewKind::Folder => fs::create_dir(&abs),
        };
        if let Err(error) = made {
            self.own.retract_moves(std::slice::from_ref(&shown));
            return Err(io_error(&path, error));
        }
        if kind == NewKind::Folder {
            self.explorer.keep_folder(&shown);
        }
        self.explorer.sync_paths(std::slice::from_ref(&shown), &[]);
        self.record(match kind {
            NewKind::File => Op::NewFile {
                path: shown.clone(),
            },
            NewKind::Folder => Op::NewFolder {
                path: shown.clone(),
            },
        });
        Ok(shown)
    }

    // ---- undo ------------------------------------------------------------------------------------------------

    /// Takes the newest operation back. `None`: nothing to undo.
    ///
    /// # Errors
    /// Only when the journal cannot be written; what could not be put back is in the outcome.
    pub fn undo(&self) -> Result<Option<UndoOutcome>, FileOpError> {
        let entry = {
            let _guard = lock(&self.journal_lock);
            let mut journal = self.journal.load().into_value();
            let Some(entry) = journal.entries.pop() else {
                return Ok(None);
            };
            self.journal.save(&journal).map_err(|e| FileOpError::Io {
                path: JOURNAL_FILE.to_owned(),
                source: io::Error::other(e.to_string()),
            })?;
            entry
        };
        Ok(Some(match entry.op {
            Op::Move { moves, docs } => self.undo_move(&moves, &docs),
            Op::NewFile { path } => self.undo_new(&path, UndoKind::NewFile),
            Op::NewFolder { path } => self.undo_new(&path, UndoKind::NewFolder),
            Op::Delete { paths } => self.undo_delete(&paths),
        }))
    }

    fn undo_move(&self, moves: &[(String, String)], docs: &[DocUndo]) -> UndoOutcome {
        let mut skipped = Vec::new();
        let mut back = vec![false; moves.len()];
        let mut done: Vec<(VaultPath, VaultPath)> = Vec::new();
        for (at, (from, to)) in moves.iter().enumerate().rev() {
            let (Ok(from_p), Ok(to_p)) = (VaultPath::parse(from), VaultPath::parse(to)) else {
                continue;
            };
            // the reverse move: `to` goes back to `from`
            let pair = [(to_p.clone(), from_p.clone())];
            match check_moves(&self.vault, &pair) {
                Ok(valid) => {
                    let announced: Vec<(String, String)> =
                        valid.iter().flat_map(announced_pairs).collect();
                    self.own.expect_moves(&announced);
                    if execute_moves(&valid).is_ok() {
                        back[at] = true;
                        done.push((to_p, from_p));
                    } else {
                        let paths: Vec<String> = announced
                            .iter()
                            .flat_map(|(a, b)| [a.clone(), b.clone()])
                            .collect();
                        self.own.retract_moves(&paths);
                        skipped.push(Skip {
                            path: to.clone(),
                            reason: SkipReason::Occupied,
                        });
                    }
                }
                Err(_) => skipped.push(Skip {
                    path: to.clone(),
                    reason: if fs::symlink_metadata(locate(&self.vault, &to_p).unwrap_or_default())
                        .is_err()
                    {
                        SkipReason::Missing
                    } else {
                        SkipReason::Occupied
                    },
                }),
            }
        }
        let mut links_back = 0;
        for doc in docs {
            if !doc
                .causes
                .iter()
                .all(|c| back.get(*c).copied().unwrap_or(false))
            {
                skipped.push(Skip {
                    path: doc.path.clone(),
                    reason: SkipReason::Occupied,
                });
                continue;
            }
            // the document is where it started when its own move went back
            let place = if doc
                .moved_by
                .is_some_and(|m| back.get(m).copied().unwrap_or(false))
            {
                &doc.original
            } else {
                &doc.path
            };
            match self.revert_links(place, doc) {
                Ok(()) => links_back += 1,
                Err(reason) => skipped.push(Skip {
                    path: place.clone(),
                    reason,
                }),
            }
        }
        let _ = links_back;
        let paths: Vec<String> = done
            .iter()
            .flat_map(|(a, b)| [a.to_string(), b.to_string()])
            .collect();
        let renames: Vec<Renamed> = done
            .iter()
            .map(|(a, b)| Renamed {
                from: a.to_string(),
                to: b.to_string(),
            })
            .collect();
        self.explorer.sync_paths(&paths, &renames);
        UndoOutcome {
            kind: UndoKind::Move,
            restored: count(done.len()),
            skipped,
        }
    }

    fn revert_links(&self, place: &str, doc: &DocUndo) -> Result<(), SkipReason> {
        let path = VaultPath::parse(place).map_err(|_| SkipReason::Missing)?;
        let abs = locate(&self.vault, &path).map_err(|_| SkipReason::Missing)?;
        let bytes = fs::read(&abs).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                SkipReason::Missing
            } else {
                SkipReason::Unreadable
            }
        })?;
        if hash(&bytes).hex() != doc.hash_after {
            return Err(SkipReason::Changed);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| SkipReason::NotUtf8)?;
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for edit in &doc.edits {
            let (Some(before), Some(_)) =
                (text.get(at..edit.start), text.get(edit.start..edit.end))
            else {
                return Err(SkipReason::Changed);
            };
            out.push_str(before);
            out.push_str(&edit.old);
            at = edit.end;
        }
        out.push_str(text.get(at..).ok_or(SkipReason::Changed)?);
        let new_hash: ContentHash = hash(out.as_bytes());
        self.own.expect_write(place, new_hash);
        if write_atomic(&abs, out.as_bytes()).is_err() {
            self.own.retract_write(place, new_hash);
            return Err(SkipReason::WriteFailed);
        }
        Ok(())
    }

    fn undo_new(&self, path: &str, kind: UndoKind) -> UndoOutcome {
        let mut skipped = Vec::new();
        let mut restored = 0;
        let removed = VaultPath::parse(path)
            .ok()
            .and_then(|p| locate(&self.vault, &p).ok())
            .map(|abs| match kind {
                UndoKind::NewFile => match fs::metadata(&abs) {
                    // only a file nobody has written to is taken back
                    Ok(m) if m.len() == 0 => fs::remove_file(&abs).is_ok(),
                    Ok(_) => false,
                    Err(_) => true,
                },
                _ => fs::remove_dir(&abs).is_ok() || !abs.exists(),
            });
        if removed == Some(true) {
            restored = 1;
            self.own.expect_removals(&[path.to_owned()]);
        } else {
            skipped.push(Skip {
                path: path.to_owned(),
                reason: SkipReason::Changed,
            });
        }
        self.explorer.sync_paths(&[path.to_owned()], &[]);
        UndoOutcome {
            kind,
            restored,
            skipped,
        }
    }

    fn undo_delete(&self, paths: &[String]) -> UndoOutcome {
        let abs: Vec<PathBuf> = paths
            .iter()
            .filter_map(|p| VaultPath::parse(p).ok())
            .filter_map(|p| locate(&self.vault, &p).ok())
            .collect();
        let restored = restore_from_bin(&abs);
        self.own.expect_creations(paths);
        self.explorer.sync_paths(paths, &[]);
        let skipped = if restored {
            Vec::new()
        } else {
            paths
                .iter()
                .map(|p| Skip {
                    path: p.clone(),
                    reason: SkipReason::Missing,
                })
                .collect()
        };
        UndoOutcome {
            kind: UndoKind::Delete,
            restored: if restored { count(paths.len()) } else { 0 },
            skipped,
        }
    }
}

/// The bytes of a file; `None` when it is larger than `limit`.
fn read_limited(path: &std::path::Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    if fs::metadata(path)?.len() > limit {
        return Ok(None);
    }
    fs::read(path).map(Some)
}

fn parse_moves(moves: &[MoveSpec]) -> Result<Vec<(VaultPath, VaultPath)>, FileOpError> {
    moves
        .iter()
        .map(|m| Ok((VaultPath::parse(&m.from)?, VaultPath::parse(&m.to)?)))
        .collect()
}

fn build_dto(
    id: u64,
    pairs: &[(String, String)],
    docs: &[DocPlan],
    skipped: Vec<Skip>,
) -> MovePlan {
    let mut shown = 0;
    let mut truncated = false;
    let mut files = Vec::new();
    for doc in docs {
        let room = MAX_PREVIEW_LINKS.saturating_sub(shown);
        if room == 0 {
            truncated = true;
            break;
        }
        let changes: Vec<PlanChange> = doc
            .edits
            .iter()
            .take(room)
            .map(|e| PlanChange {
                line: count(e.line),
                old: e.old.clone(),
                new: e.new.clone(),
            })
            .collect();
        truncated |= changes.len() < doc.edits.len();
        shown += changes.len();
        files.push(PlanFile {
            path: doc.path.clone(),
            moved_to: doc.moved_to.clone(),
            changes,
        });
    }
    MovePlan {
        plan_id: id,
        moves: pairs
            .iter()
            .map(|(from, to)| MoveSpec {
                from: from.clone(),
                to: to.clone(),
            })
            .collect(),
        link_count: count(docs.iter().map(|d| d.edits.len()).sum()),
        file_count: count(docs.len()),
        files,
        truncated,
        skipped,
    }
}

/// Puts deleted items back from the system's recycle bin (Windows, Linux). `false`: not possible here.
#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn restore_from_bin(paths: &[PathBuf]) -> bool {
    let Ok(items) = trash::os_limited::list() else {
        return false;
    };
    let wanted: Vec<trash::TrashItem> = paths
        .iter()
        .filter_map(|path| {
            items
                .iter()
                .filter(|item| item.original_path() == *path)
                .max_by_key(|item| item.time_deleted)
                .cloned()
        })
        .collect();
    wanted.len() == paths.len()
        && !wanted.is_empty()
        && trash::os_limited::restore_all(wanted).is_ok()
}

#[cfg(not(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
fn restore_from_bin(_paths: &[PathBuf]) -> bool {
    false
}
