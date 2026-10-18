//! The Explorer's operations as one service per open project: preview a move (which links would change),
//! apply it (move the files, rewrite the links with guarded atomic writes), delete to the recycle bin,
//! make folders, and undo what was done.
//!
//! Nothing here knows about the window. The parts of the project that follow a rename (the tags the app
//! keeps) are told through [`Followers`].

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::broken::{self, Prepared, RepairOutcome, RepairStatus};
use super::ops::{
    check_deletes, check_moves, create_folder, execute_deletes, execute_moves, remove_empty_folder,
    Recycler, ValidMove,
};
use super::plan::{apply_edits, invert_edits, LinkEdit, LinkPlan, Planner};
use super::world::VaultWorld;
use super::{
    BufferEdits, BufferText, CreatedFolder, DeleteOutcome, FileOpError, Move, MoveOutcome,
    MovePreview, PreviewChange, PreviewFile, SkipReason, SkippedFile, TextEdit, UndoOutcome,
};
use crate::util::{hash_bytes, hash_hex, read_with_retry, write_atomic_guarded, Guarded};
use crate::vault::{key, Vault};
use crate::watch::OwnWrites;

/// The previews kept for `apply_move`, oldest dropped first.
const KEPT_PLANS: usize = 8;
/// The operations that can still be undone, oldest dropped first.
const KEPT_OPERATIONS: usize = 20;
/// A preview lists at most this many links (the counts are always complete).
const MAX_PREVIEW_LINKS: usize = 2_000;

/// What follows the changes the app makes to files: the tags it keeps for them and whether they were read.
pub trait Followers: Send + Sync {
    /// A file or folder was renamed or moved.
    fn renamed(&self, from: &str, to: &str, is_dir: bool);
    /// The links in a document were rewritten, or put back by an undo: its hash (see [`hash_hex`]) was
    /// `old_hash` and is `new_hash`. `path` is where the document is after the change.
    fn rewritten(&self, path: &str, old_hash: &str, new_hash: &str);
}

/// A preview waiting to be applied.
struct StoredPlan {
    moves: Vec<ValidMove>,
    links: LinkPlan,
}

/// A document whose links were rewritten, with what it takes to put them back.
struct Rewrite {
    /// Where the document is after the operation.
    path: String,
    old_hash: String,
    new_hash: String,
    /// The edits that turn the new text into the old one.
    inverse: Vec<LinkEdit>,
    links: usize,
}

/// What an operation did, for undoing it.
struct Operation {
    id: u64,
    moves: Vec<Move>,
    rewrites: Vec<Rewrite>,
    folders: Vec<String>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    plans: Vec<(u64, StoredPlan)>,
    operations: Vec<Operation>,
}

pub struct FileOpsService {
    vault: Arc<Vault>,
    own: Arc<OwnWrites>,
    followers: Arc<dyn Followers>,
    recycler: Arc<dyn Recycler>,
    state: Mutex<State>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl FileOpsService {
    pub fn new(
        vault: Arc<Vault>,
        own: Arc<OwnWrites>,
        followers: Arc<dyn Followers>,
        recycler: Arc<dyn Recycler>,
    ) -> Self {
        Self {
            vault,
            own,
            followers,
            recycler,
            state: Mutex::new(State::default()),
        }
    }

    fn next_id(&self) -> u64 {
        let mut state = lock(&self.state);
        state.next_id += 1;
        state.next_id
    }

    // ---- move and rename ---------------------------------------------------------------------------

    /// Checks the moves and, with `scan_links`, finds the links that would have to change. The documents in
    /// `held_back` are open with unsaved edits that cannot be patched: they are left out of the plan, and the
    /// ones that had links to change are listed as skipped ([`SkipReason::UnsavedVisual`]).
    pub fn preview_move(
        &self,
        moves: &[Move],
        scan_links: bool,
        held_back: &[String],
    ) -> Result<MovePreview, FileOpError> {
        let valid = check_moves(&self.vault, moves)?;
        let real: Vec<Move> = valid.iter().map(ValidMove::as_move).collect();
        let links = if scan_links {
            let world = VaultWorld::new(&self.vault);
            let mut links = Planner::new(&world, &real).plan();
            links.hold_back(&held_back.iter().map(|p| key(p)).collect());
            links
        } else {
            LinkPlan::default()
        };
        let preview = describe(&real, &links);
        let plan_id = self.next_id();
        let mut state = lock(&self.state);
        state.plans.push((
            plan_id,
            StoredPlan {
                moves: valid,
                links,
            },
        ));
        let excess = state.plans.len().saturating_sub(KEPT_PLANS);
        state.plans.drain(..excess);
        Ok(MovePreview { plan_id, ..preview })
    }

    /// Makes the moves of a preview and, with `update_links`, rewrites the links it found. Documents with
    /// unsaved edits (`buffers`) are not written: the changes for their text are returned instead.
    pub fn apply_move(
        &self,
        plan_id: u64,
        update_links: bool,
        buffers: &[BufferText],
    ) -> Result<MoveOutcome, FileOpError> {
        let stored = {
            let mut state = lock(&self.state);
            let at = state
                .plans
                .iter()
                .position(|(id, _)| *id == plan_id)
                .ok_or(FileOpError::UnknownPlan)?;
            state.plans.remove(at).1
        };
        // The disk may have changed since the preview: every move is checked again.
        let wanted: Vec<Move> = stored.moves.iter().map(ValidMove::as_move).collect();
        let valid = check_moves(&self.vault, &wanted)?;
        let moves: Vec<Move> = valid.iter().map(ValidMove::as_move).collect();

        // Before anything moves, while the planner still sees the project as it is.
        let (buffer_edits, in_buffers) = if update_links {
            self.buffer_edits(&moves, buffers)
        } else {
            (Vec::new(), HashSet::new())
        };

        if let Err(error) = execute_moves(&self.vault, &self.own, &valid) {
            if let FileOpError::PartlyMoved { left, .. } = &error {
                // what could not be put back did move, and what follows files has to know
                for m in left {
                    if let Some(v) = valid.iter().find(|v| v.from == m.from) {
                        self.followers.renamed(&v.from, &v.to, v.is_dir);
                    }
                }
            }
            return Err(error);
        }
        for m in &valid {
            self.followers.renamed(&m.from, &m.to, m.is_dir);
        }

        let (rewrites, skipped) = if update_links {
            self.rewrite_documents(&stored.links, &in_buffers)
        } else {
            (Vec::new(), Vec::new())
        };
        let mut skipped = skipped;
        skipped.extend(stored.links.skipped.iter().map(|s| SkippedFile {
            path: s.path.clone(),
            reason: s.reason,
        }));

        let outcome = MoveOutcome {
            op_id: self.next_id(),
            moved: moves.clone(),
            files_updated: rewrites.len(),
            links_updated: rewrites.iter().map(|r| r.links).sum(),
            buffers: buffer_edits,
            skipped,
        };
        self.remember(Operation {
            id: outcome.op_id,
            moves,
            rewrites,
            folders: Vec::new(),
        });
        Ok(outcome)
    }

    /// The changes for the text of documents that are being edited, and the set of their paths.
    fn buffer_edits(
        &self,
        moves: &[Move],
        buffers: &[BufferText],
    ) -> (Vec<BufferEdits>, HashSet<String>) {
        let world = VaultWorld::new(&self.vault);
        let planner = Planner::new(&world, moves);
        let mut out = Vec::new();
        let mut paths = HashSet::new();
        for buffer in buffers {
            paths.insert(key(&buffer.path));
            let edits = planner.edits_for(&buffer.path, &buffer.text);
            if !edits.is_empty() {
                out.push(BufferEdits {
                    path: buffer.path.clone(),
                    new_path: planner.new_path(&buffer.path),
                    edits: to_text_edits(&buffer.text, &edits),
                });
            }
        }
        (out, paths)
    }

    /// Writes the planned changes into the documents, which are where the moves put them. A document
    /// that is not the version the plan was made from is left alone.
    fn rewrite_documents(
        &self,
        links: &LinkPlan,
        in_buffers: &HashSet<String>,
    ) -> (Vec<Rewrite>, Vec<SkippedFile>) {
        let mut done = Vec::new();
        let mut skipped = Vec::new();
        for doc in &links.docs {
            if in_buffers.contains(&key(&doc.path)) {
                continue;
            }
            let path = doc.moved_to.clone().unwrap_or_else(|| doc.path.clone());
            match self.rewrite_one(&path, &doc.hash, &doc.edits) {
                Ok(rewrite) => done.push(rewrite),
                Err(reason) => skipped.push(SkippedFile { path, reason }),
            }
        }
        (done, skipped)
    }

    /// Replaces the destinations in one document if it still holds the version with `old_hash`.
    fn rewrite_one(
        &self,
        path: &str,
        old_hash: &str,
        edits: &[LinkEdit],
    ) -> Result<Rewrite, SkipReason> {
        let resolved = self.vault.resolve(path).map_err(|_| SkipReason::Missing)?;
        let bytes = read_with_retry(&resolved.abs).map_err(|_| SkipReason::Unreadable)?;
        if hash_hex(&bytes) != old_hash {
            return Err(SkipReason::Changed);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| SkipReason::NotUtf8)?;
        let rewritten = apply_edits(text, edits).into_bytes();
        let new_hash = hash_bytes(&rewritten);
        self.own.expect(&resolved.rel, new_hash);
        let written = write_atomic_guarded(&resolved.abs, &rewritten, || {
            let on_disk = read_with_retry(&resolved.abs)?;
            Ok((hash_hex(&on_disk) != old_hash).then_some(()))
        });
        match written {
            Ok(Guarded::Written) => {
                let new_hash = format!("{new_hash:016x}");
                self.followers.rewritten(&resolved.rel, old_hash, &new_hash);
                Ok(Rewrite {
                    path: resolved.rel,
                    old_hash: old_hash.to_owned(),
                    new_hash,
                    inverse: invert_edits(edits),
                    links: edits.len(),
                })
            }
            Ok(Guarded::Refused(())) => {
                self.own.retract(&resolved.rel, new_hash);
                Err(SkipReason::Changed)
            }
            Err(_) => {
                self.own.retract(&resolved.rel, new_hash);
                Err(SkipReason::WriteFailed)
            }
        }
    }

    // ---- broken links -------------------------------------------------------------------------------

    /// Points one broken link at `target`, a file with the name the link asks for. The document is
    /// left on disk when `buffer` is its unsaved text: the change for the editor comes back instead.
    pub fn repair_link(
        &self,
        path: &str,
        start: usize,
        end: usize,
        target: &str,
        buffer: Option<&BufferText>,
    ) -> Result<RepairOutcome, FileOpError> {
        if let Some(buf) = buffer.filter(|b| key(&b.path) == key(path)) {
            let outcome = match broken::prepare(&self.vault, path, &buf.text, start, end, target) {
                Prepared::Edit(edit) => RepairOutcome {
                    status: RepairStatus::Repaired,
                    op_id: None,
                    buffer: Some(BufferEdits {
                        path: path.to_owned(),
                        new_path: path.to_owned(),
                        edits: to_text_edits(&buf.text, std::slice::from_ref(&edit)),
                    }),
                },
                Prepared::Stale => RepairOutcome::status(RepairStatus::Stale),
                Prepared::NotAMatch => RepairOutcome::status(RepairStatus::NotAMatch),
            };
            return Ok(outcome);
        }
        let resolved = self.vault.resolve(path)?;
        let bytes = read_with_retry(&resolved.abs).map_err(|source| FileOpError::Io {
            path: path.to_owned(),
            source,
        })?;
        if bytes.len() as u64 > crate::render::MAX_RENDER_BYTES {
            return Ok(RepairOutcome::status(RepairStatus::Stale));
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return Ok(RepairOutcome::status(RepairStatus::Stale));
        };
        let edit = match broken::prepare(&self.vault, path, text, start, end, target) {
            Prepared::Edit(edit) => edit,
            Prepared::Stale => return Ok(RepairOutcome::status(RepairStatus::Stale)),
            Prepared::NotAMatch => return Ok(RepairOutcome::status(RepairStatus::NotAMatch)),
        };
        self.write_repair(&resolved.rel, &bytes, &edit)
    }

    /// Writes one link edit when the file is still `bytes`, and remembers it for undo.
    fn write_repair(
        &self,
        path: &str,
        bytes: &[u8],
        edit: &LinkEdit,
    ) -> Result<RepairOutcome, FileOpError> {
        let resolved = self.vault.resolve(path)?;
        let text = std::str::from_utf8(bytes).map_err(|_| FileOpError::Io {
            path: path.to_owned(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, "not utf-8"),
        })?;
        let old_hash = hash_hex(bytes);
        let rewritten = apply_edits(text, std::slice::from_ref(edit)).into_bytes();
        let new_hash_num = hash_bytes(&rewritten);
        self.own.expect(&resolved.rel, new_hash_num);
        let written = write_atomic_guarded(&resolved.abs, &rewritten, || {
            let on_disk = read_with_retry(&resolved.abs)?;
            Ok((hash_hex(&on_disk) != old_hash).then_some(()))
        });
        match written {
            Ok(Guarded::Written) => {
                let new_hash = format!("{new_hash_num:016x}");
                self.followers
                    .rewritten(&resolved.rel, &old_hash, &new_hash);
                let op_id = self.next_id();
                self.remember(Operation {
                    id: op_id,
                    moves: Vec::new(),
                    rewrites: vec![Rewrite {
                        path: resolved.rel,
                        old_hash,
                        new_hash,
                        inverse: invert_edits(std::slice::from_ref(edit)),
                        links: 1,
                    }],
                    folders: Vec::new(),
                });
                Ok(RepairOutcome {
                    status: RepairStatus::Repaired,
                    op_id: Some(op_id),
                    buffer: None,
                })
            }
            Ok(Guarded::Refused(())) => {
                self.own.retract(&resolved.rel, new_hash_num);
                Ok(RepairOutcome::status(RepairStatus::Stale))
            }
            Err(source) => {
                self.own.retract(&resolved.rel, new_hash_num);
                Err(FileOpError::Io {
                    path: path.to_owned(),
                    source,
                })
            }
        }
    }

    // ---- delete and new folders ---------------------------------------------------------------------

    /// Sends files and folders to the recycle bin.
    pub fn delete(&self, paths: &[String]) -> Result<DeleteOutcome, FileOpError> {
        let valid = check_deletes(&self.vault, paths)?;
        execute_deletes(&self.vault, &self.own, self.recycler.as_ref(), &valid)?;
        Ok(DeleteOutcome {
            deleted: valid.into_iter().map(|d| d.rel).collect(),
        })
    }

    /// Makes a folder; the operation can be undone while the folder is empty.
    pub fn new_folder(&self, path: &str) -> Result<CreatedFolder, FileOpError> {
        let created = create_folder(&self.vault, path)?;
        let op_id = self.next_id();
        self.remember(Operation {
            id: op_id,
            moves: Vec::new(),
            rewrites: Vec::new(),
            folders: vec![created.clone()],
        });
        Ok(CreatedFolder {
            op_id,
            path: created,
        })
    }

    // ---- undo ---------------------------------------------------------------------------------------

    fn remember(&self, operation: Operation) {
        let mut state = lock(&self.state);
        state.operations.push(operation);
        let excess = state.operations.len().saturating_sub(KEPT_OPERATIONS);
        state.operations.drain(..excess);
    }

    /// Puts an operation back: the files it moved, the links it rewrote (in the files that are unchanged
    /// since), then the folders it made. What cannot be put back is reported.
    ///
    /// Every move back is checked before anything is written. A link is put back only when everything it
    /// is about goes back (the file it points at and the document it is in): a file that cannot return
    /// because something is in the way of its old place keeps the links that lead to its new one.
    pub fn undo(&self, op_id: u64) -> Result<UndoOutcome, FileOpError> {
        let operation = {
            let mut state = lock(&self.state);
            let at = state
                .operations
                .iter()
                .position(|o| o.id == op_id)
                .ok_or(FileOpError::UnknownOperation)?;
            state.operations.remove(at)
        };
        let mut out = UndoOutcome {
            moved_back: Vec::new(),
            files_restored: 0,
            links_restored: 0,
            folders_removed: 0,
            could_not: Vec::new(),
        };

        // which moves can go back, last first (the order they are made in)
        let mut goes_back = vec![false; operation.moves.len()];
        let mut backs: Vec<(usize, ValidMove)> = Vec::new();
        let mut blocked: Vec<SkippedFile> = Vec::new();
        for (at, m) in operation.moves.iter().enumerate().rev() {
            let back = Move {
                from: m.to.clone(),
                to: m.from.clone(),
            };
            match check_moves(&self.vault, std::slice::from_ref(&back)) {
                Ok(mut valid) => {
                    goes_back[at] = true;
                    backs.extend(valid.pop().map(|v| (at, v)));
                }
                Err(error) => blocked.push(SkippedFile {
                    path: m.to.clone(),
                    reason: why_not_back(&error),
                }),
            }
        }

        for rewrite in operation.rewrites.iter().rev() {
            match self.restore_one(rewrite, &goes_back) {
                Ok(0) => {}
                Ok(links) => {
                    out.files_restored += 1;
                    out.links_restored += links;
                }
                Err(reason) => out.could_not.push(SkippedFile {
                    path: rewrite.path.clone(),
                    reason,
                }),
            }
        }

        out.could_not.extend(blocked);
        for (at, valid) in backs {
            match execute_moves(&self.vault, &self.own, std::slice::from_ref(&valid)) {
                Ok(()) => {
                    self.followers.renamed(&valid.from, &valid.to, valid.is_dir);
                    out.moved_back.push(valid.as_move());
                }
                // it passed the check a moment ago: someone was quicker
                Err(error) => out.could_not.push(SkippedFile {
                    path: operation.moves[at].to.clone(),
                    reason: why_not_back(&error),
                }),
            }
        }

        for folder in &operation.folders {
            if remove_empty_folder(&self.vault, folder) {
                out.folders_removed += 1;
            } else {
                out.could_not.push(SkippedFile {
                    path: folder.clone(),
                    reason: SkipReason::Changed,
                });
            }
        }
        Ok(out)
    }

    /// Writes a document's old links back, if it still is exactly what the operation wrote. Only the edits
    /// whose moves all go back (`goes_back`, by position in the operation) are taken back. Returns how many
    /// links that was.
    fn restore_one(&self, rewrite: &Rewrite, goes_back: &[bool]) -> Result<usize, SkipReason> {
        let wanted: Vec<LinkEdit> = rewrite
            .inverse
            .iter()
            .filter(|e| {
                e.causes
                    .iter()
                    .all(|c| goes_back.get(*c).copied().unwrap_or(false))
            })
            .cloned()
            .collect();
        if wanted.is_empty() {
            return Ok(0);
        }
        let resolved = self
            .vault
            .resolve(&rewrite.path)
            .map_err(|_| SkipReason::Missing)?;
        let bytes = read_with_retry(&resolved.abs).map_err(|_| SkipReason::Missing)?;
        if hash_hex(&bytes) != rewrite.new_hash {
            return Err(SkipReason::Changed);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| SkipReason::NotUtf8)?;
        let restored = apply_edits(text, &wanted).into_bytes();
        // all of them back: the document must be exactly what it was
        if wanted.len() == rewrite.inverse.len() && hash_hex(&restored) != rewrite.old_hash {
            return Err(SkipReason::Changed);
        }
        let hash = hash_bytes(&restored);
        self.own.expect(&resolved.rel, hash);
        let written = write_atomic_guarded(&resolved.abs, &restored, || {
            let on_disk = read_with_retry(&resolved.abs)?;
            Ok((hash_hex(&on_disk) != rewrite.new_hash).then_some(()))
        });
        match written {
            Ok(Guarded::Written) => {
                self.followers
                    .rewritten(&resolved.rel, &rewrite.new_hash, &format!("{hash:016x}"));
                Ok(wanted.len())
            }
            Ok(Guarded::Refused(())) => {
                self.own.retract(&resolved.rel, hash);
                Err(SkipReason::Changed)
            }
            Err(_) => {
                self.own.retract(&resolved.rel, hash);
                Err(SkipReason::WriteFailed)
            }
        }
    }
}

/// Why a move could not be undone.
fn why_not_back(error: &FileOpError) -> SkipReason {
    match error {
        // something is at the old place now
        FileOpError::Exists(_) => SkipReason::Occupied,
        // the file is not where the operation left it
        FileOpError::Vault(_) => SkipReason::Missing,
        _ => SkipReason::WriteFailed,
    }
}

/// The preview the UI shows for a plan.
fn describe(moves: &[Move], links: &LinkPlan) -> MovePreview {
    let mut files = Vec::new();
    let mut listed = 0;
    let mut truncated = false;
    for doc in &links.docs {
        if listed >= MAX_PREVIEW_LINKS {
            truncated = true;
            break;
        }
        let take = doc.edits.len().min(MAX_PREVIEW_LINKS - listed);
        truncated |= take < doc.edits.len();
        listed += take;
        files.push(PreviewFile {
            path: doc.path.clone(),
            moved_to: doc.moved_to.clone(),
            changes: doc.edits[..take]
                .iter()
                .map(|e| PreviewChange {
                    line: e.line,
                    old: e.old.clone(),
                    new: e.new.clone(),
                })
                .collect(),
        });
    }
    MovePreview {
        plan_id: 0,
        moves: moves.to_vec(),
        link_count: links.link_count(),
        file_count: links.docs.len(),
        files,
        truncated,
        skipped: links
            .skipped
            .iter()
            .map(|s| SkippedFile {
                path: s.path.clone(),
                reason: s.reason,
            })
            .collect(),
    }
}

/// Byte-range edits of `text` as the UTF-16 offsets an editor counts in.
fn to_text_edits(text: &str, edits: &[LinkEdit]) -> Vec<TextEdit> {
    let mut out = Vec::with_capacity(edits.len());
    let (mut at_byte, mut at_unit) = (0, 0);
    let mut units = |upto: usize| {
        at_unit += text[at_byte..upto].encode_utf16().count();
        at_byte = upto;
        at_unit
    };
    for e in edits {
        let from = units(e.start);
        let to = units(e.end);
        out.push(TextEdit {
            from,
            to,
            insert: e.new.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests;
