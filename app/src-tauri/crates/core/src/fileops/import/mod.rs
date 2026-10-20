//! Drop into a project: files and folders from outside the project come in, copied or moved, into the folder the
//! person chose. A plan lists what would happen (nothing is touched); applying it builds every item under a hidden
//! temp name and renames it into place only when whole, so a cancel or a failure never leaves half an item behind.
//! Nothing is overwritten. One undo-journal entry per apply.
//!
//! * [`copy`]: the chunked copy engine (progress, cancel, links skipped, originals deleted only while unchanged);
//! * [`FileOps::import_plan`], [`FileOps::import_apply`], [`FileOps::import_cancel`]: the service.

mod copy;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use unicode_normalization::UnicodeNormalization;

use self::copy::{
    count_tree, crosses_volumes, delete_originals, is_link, place_copy, Run, Stop, TICK,
};
use super::ops::{is_protected, list_files, locate, os_rename, MAX_ANNOUNCED};
use super::undo::{ImportMoved, Op};
use super::{
    check_name, count, lock, FileOpError, FileOps, Skip, SkipReason, UndoKind, UndoOutcome,
};
use crate::error::IpcError;
use crate::paths::{VaultError, VaultPath};

/// Plans kept for `import_apply`.
const KEPT_PLANS: usize = 4;
/// A name is at most this many bytes (the rule of names).
const MAX_NAME: usize = 255;

// ---- DTOs --------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ImportMode {
    Copy,
    Move,
}

/// What happens to an item whose name is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ConflictChoice {
    /// Leave it out.
    Skip,
    /// Give it a free name: `name (2)`.
    KeepBoth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ImportKind {
    File,
    Folder,
}

/// Why a dropped entry cannot come in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ImportRefusal {
    /// Not there (any more), or not readable.
    Missing,
    /// A symbolic link | junction: never followed, never copied.
    Link,
    /// Neither a file nor a folder (a device, a socket).
    Special,
    /// No usable name (a drive root), or a name the project cannot hold.
    BadName,
    /// `.git`, or `.permark` into a root.
    Protected,
    /// A folder dropped into itself or into a folder below it.
    IntoItself,
}

/// A dropped entry that can come in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportItem {
    /// Absolute path as dropped.
    pub source: String,
    pub name: String,
    pub kind: ImportKind,
    /// The source already lies inside a root of this project: it can be copied, not moved here.
    pub in_project: bool,
    /// The name is taken in the folder, or by an earlier item of the same drop.
    pub conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportRefused {
    pub source: String,
    pub name: String,
    pub reason: ImportRefusal,
}

/// What a drop would do. Nothing on disk was touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportPlan {
    #[ts(type = "number")]
    pub plan_id: u64,
    /// Project path of the folder the items come into.
    pub target: String,
    pub items: Vec<ImportItem>,
    pub refused: Vec<ImportRefused>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ImportPhase {
    /// A first quiet walk that totals the files and bytes.
    Counting,
    Copying,
}

/// Progress of a running import (event `import-progress`, to the window that asked).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportProgress {
    #[ts(type = "number")]
    pub plan_id: u64,
    pub phase: ImportPhase,
    pub items_done: u32,
    pub items_total: u32,
    #[ts(type = "number")]
    pub files_done: u64,
    #[ts(type = "number")]
    pub files_total: u64,
    #[ts(type = "number")]
    pub bytes_done: u64,
    #[ts(type = "number")]
    pub bytes_total: u64,
    /// The item being worked on.
    pub item: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportFailure {
    pub name: String,
    pub error: IpcError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportOutcome {
    pub target: String,
    /// Items put in place by copying (the originals untouched).
    pub copied: u32,
    /// Items put in place by moving (the originals gone, wholly or partly).
    pub moved: u32,
    #[ts(type = "number")]
    pub files: u64,
    #[ts(type = "number")]
    pub bytes: u64,
    /// Items left out because their name was taken (`Skip`).
    pub skipped: u32,
    pub failed: Vec<ImportFailure>,
    /// Links and special files inside dropped folders that were not copied.
    pub links: u32,
    /// Originals a move could not delete (locked, changed since, or links that stay).
    pub left_behind: u32,
    pub cancelled: bool,
    /// Project paths of the items put in place, in drop order (the Explorer selects them).
    pub placed: Vec<String>,
    /// The journal entry that takes this import back (`fileops_undo_entry`); `None`: nothing came in, or not journaled.
    #[ts(type = "number | null")]
    pub undo: Option<u64>,
}

// ---- service state -----------------------------------------------------------------------------------------------

struct StoredItem {
    source: PathBuf,
    name: String,
    is_dir: bool,
    in_project: bool,
}

struct StoredImport {
    id: u64,
    target: VaultPath,
    items: Vec<StoredItem>,
}

/// Import plans waiting, and the cancel flags of the imports running.
#[derive(Default)]
pub(super) struct Imports {
    next: u64,
    plans: VecDeque<StoredImport>,
    running: HashMap<u64, Arc<AtomicBool>>,
}

/// An import registered as running; leaves the registry when dropped, however the apply ends.
struct Registered<'a> {
    ops: &'a FileOps,
    id: u64,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        lock(&self.ops.imports).running.remove(&self.id);
    }
}

// ---- names -------------------------------------------------------------------------------------------------------

/// How names are compared for "taken": NFC and without case, on every system (a project must open on all of them).
fn name_key(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase()
}

fn listing(dir: &Path) -> HashSet<String> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .map(|n| name_key(&n))
                .collect()
        })
        .unwrap_or_default()
}

/// `stem (n).ext` | `name (n)`; the stem is cut when the name would pass the limit.
fn numbered(name: &str, is_dir: bool, n: u32) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(at) if !is_dir && at > 0 => (&name[..at], &name[at..]),
        _ => (name, ""),
    };
    let suffix = format!(" ({n}){ext}");
    let mut stem = stem.to_owned();
    while stem.len() + suffix.len() > MAX_NAME {
        stem.pop();
    }
    format!("{stem}{suffix}")
}

/// The name the item takes in `dir`: its own when free, else per the choice. `None`: left out.
fn choose_name(
    name: &str,
    is_dir: bool,
    dir: &Path,
    taken: &mut HashSet<String>,
    choice: ConflictChoice,
) -> Option<String> {
    let free = |candidate: &str, taken: &HashSet<String>| {
        !taken.contains(&name_key(candidate)) && fs::symlink_metadata(dir.join(candidate)).is_err()
    };
    let chosen = if free(name, taken) {
        name.to_owned()
    } else {
        match choice {
            ConflictChoice::Skip => return None,
            ConflictChoice::KeepBoth => (2..10_000)
                .map(|n| numbered(name, is_dir, n))
                .find(|candidate| free(candidate, taken))?,
        }
    };
    taken.insert(name_key(&chosen));
    Some(chosen)
}

/// Path below the item as project path text (`/` separators).
fn joined(base: &str, rel: &Path) -> String {
    let rel: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if rel.is_empty() {
        base.to_owned()
    } else {
        format!("{base}/{}", rel.join("/"))
    }
}

// ---- the service -------------------------------------------------------------------------------------------------

/// A dropped entry looked at: what it is, before it is judged.
struct Seen {
    source: PathBuf,
    name: String,
    is_dir: bool,
    canon: PathBuf,
}

impl FileOps {
    /// Looks at what was dropped and says what would happen. Nothing is touched.
    ///
    /// # Errors
    /// The target is protected | outside the project | not a folder; a source is no absolute path; nothing was
    /// dropped. Entries that cannot come in are listed in the plan (`refused`), not errors.
    pub fn import_plan(&self, target: &str, sources: &[String]) -> Result<ImportPlan, FileOpError> {
        if sources.is_empty() {
            return Err(FileOpError::Nothing);
        }
        let target = Self::parse(target)?;
        if is_protected(&target) {
            return Err(FileOpError::Protected(target.to_string()));
        }
        let target_abs = self.vault.resolve(&target)?;
        if !fs::metadata(&target_abs).is_ok_and(|m| m.is_dir()) {
            return Err(FileOpError::Vault(VaultError::NotFound(target.to_string())));
        }
        let target_canon = dunce::canonicalize(&target_abs).unwrap_or_else(|_| target_abs.clone());
        let mut refused = Vec::new();
        let mut seen: Vec<Seen> = Vec::new();
        for raw in sources {
            let path = PathBuf::from(raw);
            if raw.is_empty() || raw.contains('\0') || !path.is_absolute() {
                return Err(FileOpError::Vault(VaultError::Invalid(raw.clone())));
            }
            match Self::inspect(&path, &target, &target_canon) {
                Ok(found) => seen.push(found),
                Err((name, reason)) => refused.push(ImportRefused {
                    source: path.display().to_string(),
                    name,
                    reason,
                }),
            }
        }
        let seen = collapse(seen);
        let mut taken = listing(&target_abs);
        let mut items = Vec::with_capacity(seen.len());
        let mut stored = Vec::with_capacity(seen.len());
        for found in seen {
            let key = name_key(&found.name);
            let conflict = !taken.insert(key);
            let in_project = self.vault.relativize(&found.canon).is_some();
            items.push(ImportItem {
                source: found.source.display().to_string(),
                name: found.name.clone(),
                kind: if found.is_dir {
                    ImportKind::Folder
                } else {
                    ImportKind::File
                },
                in_project,
                conflict,
            });
            stored.push(StoredItem {
                source: found.source,
                name: found.name,
                is_dir: found.is_dir,
                in_project,
            });
        }
        let mut imports = lock(&self.imports);
        imports.next += 1;
        let id = imports.next;
        imports.plans.push_back(StoredImport {
            id,
            target: target.clone(),
            items: stored,
        });
        while imports.plans.len() > KEPT_PLANS {
            imports.plans.pop_front();
        }
        Ok(ImportPlan {
            plan_id: id,
            target: target.to_string(),
            items,
            refused,
        })
    }

    /// One dropped entry: what it is, or why it cannot come in (`name` for the list, `reason`).
    fn inspect(
        path: &Path,
        target: &VaultPath,
        target_canon: &Path,
    ) -> Result<Seen, (String, ImportRefusal)> {
        let shown = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let refuse = |reason| (shown.clone(), reason);
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return Err(refuse(ImportRefusal::BadName));
        };
        let meta = fs::symlink_metadata(path).map_err(|_| refuse(ImportRefusal::Missing))?;
        if is_link(meta.file_type(), Some(&meta)) {
            return Err(refuse(ImportRefusal::Link));
        }
        if !meta.is_file() && !meta.is_dir() {
            return Err(refuse(ImportRefusal::Special));
        }
        if check_name(name).is_err() {
            return Err(refuse(ImportRefusal::BadName));
        }
        if is_protected(&target.join_name(name)) {
            return Err(refuse(ImportRefusal::Protected));
        }
        let canon = dunce::canonicalize(path).map_err(|_| refuse(ImportRefusal::Missing))?;
        if meta.is_dir() && target_canon.starts_with(&canon) {
            return Err(refuse(ImportRefusal::IntoItself));
        }
        Ok(Seen {
            source: path.to_path_buf(),
            name: name.to_owned(),
            is_dir: meta.is_dir(),
            canon,
        })
    }

    /// Asks a running import to stop. `true`: there was one.
    pub fn import_cancel(&self, plan_id: u64) -> bool {
        lock(&self.imports)
            .running
            .get(&plan_id)
            .is_some_and(|flag| {
                flag.store(true, Ordering::Relaxed);
                true
            })
    }

    /// Does what a plan listed: copies or moves the items into the folder, resolving taken names as the person
    /// chose. Runs on the calling thread (the caller keeps the UI alive); `report` hears the progress.
    ///
    /// # Errors
    /// `UnknownPlan`; a move of items that lie inside the project (`Nothing`); the target gone. Failures of single
    /// items are in the outcome.
    pub fn import_apply(
        &self,
        plan_id: u64,
        mode: ImportMode,
        conflicts: ConflictChoice,
        report: &dyn Fn(&ImportProgress),
    ) -> Result<ImportOutcome, FileOpError> {
        self.import_apply_with(plan_id, mode, conflicts, report, &os_rename, TICK)
    }

    /// [`FileOps::import_apply`] with the rename and the progress tick given, so a test can make a move cross
    /// volumes and hear every step.
    ///
    /// # Errors
    /// As [`FileOps::import_apply`].
    pub(super) fn import_apply_with(
        &self,
        plan_id: u64,
        mode: ImportMode,
        conflicts: ConflictChoice,
        report: &dyn Fn(&ImportProgress),
        rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
        tick: std::time::Duration,
    ) -> Result<ImportOutcome, FileOpError> {
        let cancel = Arc::new(AtomicBool::new(false));
        let stored = {
            let mut imports = lock(&self.imports);
            let at = imports.plans.iter().position(|p| p.id == plan_id);
            let stored = at
                .and_then(|at| imports.plans.remove(at))
                .ok_or(FileOpError::UnknownPlan)?;
            if mode == ImportMode::Move && stored.items.iter().any(|i| i.in_project) {
                return Err(FileOpError::Nothing);
            }
            imports.running.insert(plan_id, Arc::clone(&cancel));
            stored
        };
        let _registered = Registered {
            ops: self,
            id: plan_id,
        };
        let target_abs = self.vault.resolve(&stored.target)?;
        if !fs::metadata(&target_abs).is_ok_and(|m| m.is_dir()) {
            return Err(FileOpError::Vault(VaultError::NotFound(
                stored.target.to_string(),
            )));
        }
        let mut run = Run::new(plan_id, count(stored.items.len()), &cancel, report, tick);
        let mut result = Applied::new(stored.target.to_string());
        run.emit(true);
        // a Copy knows its totals up front; a Move counts an item only when it has to copy it
        if mode == ImportMode::Copy {
            for item in &stored.items {
                let counted = if item.is_dir {
                    count_tree(&mut run, &item.source)
                } else {
                    run.files_total += 1;
                    run.bytes_total += fs::metadata(&item.source).map_or(0, |m| m.len());
                    Ok(())
                };
                if matches!(counted, Err(Stop::Cancelled)) {
                    result.cancelled = true;
                    break;
                }
            }
        }
        run.phase = ImportPhase::Copying;
        run.emit(true);
        let mut taken = listing(&target_abs);
        for item in &stored.items {
            if result.cancelled || run.cancelled() {
                result.cancelled = true;
                break;
            }
            run.item.clone_from(&item.name);
            run.emit(true);
            let Some(name) =
                choose_name(&item.name, item.is_dir, &target_abs, &mut taken, conflicts)
            else {
                result.skipped += 1;
                run.items_done += 1;
                continue;
            };
            match self.import_one(
                &mut run,
                item,
                &stored.target,
                &target_abs,
                &name,
                mode,
                rename,
            ) {
                Ok(placed) => result.add(item, &stored.target, &name, placed),
                Err(Stop::Cancelled) => {
                    result.cancelled = true;
                    break;
                }
                Err(Stop::Failed { path, source }) => {
                    let error = FileOpError::Io {
                        path: path.display().to_string(),
                        source,
                    };
                    result.failed.push(ImportFailure {
                        name: item.name.clone(),
                        error: error.into(),
                    });
                }
            }
            run.items_done += 1;
            run.emit(true);
        }
        run.emit(true);
        if !result.copied.is_empty() || !result.moved.is_empty() {
            result.undo = self.record(Op::Import {
                copied: result.copied.clone(),
                moved: result.moved.clone(),
            });
        }
        Ok(result.into_outcome(&run))
    }

    /// Puts one item in place under `name`.
    #[allow(clippy::too_many_arguments)] // one item of an import: its state, place, name and how
    fn import_one(
        &self,
        run: &mut Run<'_>,
        item: &StoredItem,
        target: &VaultPath,
        target_abs: &Path,
        name: &str,
        mode: ImportMode,
        rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
    ) -> Result<Placed, Stop> {
        let dest = target_abs.join(name);
        let shown = target.join_name(name).to_string();
        let links_before = run.links;
        if mode == ImportMode::Move {
            if fs::symlink_metadata(&dest).is_ok() {
                return Err(Stop::failed(
                    &dest,
                    io::Error::from(io::ErrorKind::AlreadyExists),
                ));
            }
            let mut announced = vec![shown.clone()];
            if item.is_dir {
                let mut files = Vec::new();
                list_files(&item.source, &shown, &mut files);
                announced.extend(files);
            }
            self.own.expect_creations(&announced);
            match crate::fsx::retry(&crate::fsx::RETRY_DELAYS, || rename(&item.source, &dest)) {
                Ok(()) => {
                    self.synced(&shown, item.is_dir);
                    return Ok(Placed::Renamed);
                }
                Err(error) if crosses_volumes(&error) => self.own.retract_moves(&announced),
                Err(error) => {
                    self.own.retract_moves(&announced);
                    return Err(Stop::failed(&item.source, error));
                }
            }
            // another volume: counted now, copied, and the originals deleted after
            if item.is_dir {
                count_tree(run, &item.source)?;
            } else {
                run.files_total += 1;
                run.bytes_total += fs::metadata(&item.source).map_or(0, |m| m.len());
            }
        }
        let mut announced: Vec<String> = Vec::new();
        let copied = place_copy(
            run,
            &item.source,
            item.is_dir,
            &dest,
            rename,
            &mut |copied| {
                announced.push(shown.clone());
                announced.extend(
                    copied
                        .files
                        .iter()
                        .take(MAX_ANNOUNCED)
                        .map(|f| joined(&shown, &f.rel)),
                );
                self.own.expect_creations(&announced);
            },
        );
        let copied = match copied {
            Ok(copied) => copied,
            Err(stop) => {
                if !announced.is_empty() {
                    self.own.retract_moves(&announced);
                }
                return Err(stop);
            }
        };
        self.synced(&shown, item.is_dir);
        if mode == ImportMode::Copy {
            return Ok(Placed::Copied);
        }
        let deleted = delete_originals(run, &item.source, item.is_dir, &copied);
        let links = run.links - links_before;
        Ok(Placed::MovedAcross {
            removed: deleted.removed,
            left_behind: deleted.left + links,
        })
    }

    fn synced(&self, shown: &str, is_dir: bool) {
        if is_dir {
            self.explorer.keep_folder(shown);
        }
        self.explorer.sync_paths(&[shown.to_owned()], &[]);
    }

    // ---- undo ------------------------------------------------------------------------------------------------

    /// Takes an import back: moved items go to where they came from (when that place is free), copied items go to
    /// the recycle bin.
    pub(super) fn undo_import(&self, copied: &[String], moved: &[ImportMoved]) -> UndoOutcome {
        let mut skipped = Vec::new();
        let mut restored = 0_usize;
        let mut touched: Vec<String> = Vec::new();
        for item in moved.iter().rev() {
            touched.push(item.to.clone());
            let skip = |reason| Skip {
                path: item.to.clone(),
                reason,
            };
            let from = PathBuf::from(&item.from);
            let place = VaultPath::parse(&item.to)
                .ok()
                .and_then(|p| locate(&self.vault, &p).ok());
            let Some(here) = place.filter(|p| fs::symlink_metadata(p).is_ok()) else {
                skipped.push(skip(SkipReason::Missing));
                continue;
            };
            if fs::symlink_metadata(&from).is_ok() {
                skipped.push(skip(SkipReason::Occupied));
                continue;
            }
            if !from.parent().is_some_and(Path::is_dir) {
                skipped.push(skip(SkipReason::Missing));
                continue;
            }
            let is_dir = fs::symlink_metadata(&here).is_ok_and(|m| m.is_dir());
            self.own.expect_removals(std::slice::from_ref(&item.to));
            match copy::relocate(&here, is_dir, &from, &os_rename) {
                Ok(true) => restored += 1,
                Ok(false) => skipped.push(skip(SkipReason::Changed)),
                Err(_) => {
                    self.own.retract_moves(std::slice::from_ref(&item.to));
                    skipped.push(skip(SkipReason::Occupied));
                }
            }
        }
        let mut bin: Vec<(String, PathBuf)> = Vec::new();
        for path in copied {
            touched.push(path.clone());
            let abs = VaultPath::parse(path)
                .ok()
                .and_then(|p| locate(&self.vault, &p).ok());
            match abs {
                Some(abs) if fs::symlink_metadata(&abs).is_ok() => bin.push((path.clone(), abs)),
                // already gone: nothing left to take back
                _ => restored += 1,
            }
        }
        if !bin.is_empty() {
            let shown: Vec<String> = bin.iter().map(|(p, _)| p.clone()).collect();
            self.own.expect_removals(&shown);
            if trash::delete_all(bin.iter().map(|(_, abs)| abs)).is_ok() {
                restored += bin.len();
            } else {
                self.own.retract_moves(&shown);
                skipped.extend(bin.iter().map(|(path, _)| Skip {
                    path: path.clone(),
                    reason: SkipReason::Trash,
                }));
            }
        }
        self.explorer.sync_paths(&touched, &[]);
        UndoOutcome {
            kind: UndoKind::Import,
            restored: count(restored),
            skipped,
        }
    }
}

/// How an item was put in place.
#[derive(Clone, Copy)]
enum Placed {
    /// Moved with one rename.
    Renamed,
    /// Copied; the original is untouched.
    Copied,
    /// Moved across volumes: copied, then the originals deleted as far as they were unchanged.
    MovedAcross { removed: u32, left_behind: u32 },
}

/// What an apply did so far.
struct Applied {
    target: String,
    copied: Vec<String>,
    moved: Vec<ImportMoved>,
    placed: Vec<String>,
    undo: Option<u64>,
    skipped: u32,
    failed: Vec<ImportFailure>,
    left_behind: u32,
    cancelled: bool,
}

impl Applied {
    fn new(target: String) -> Self {
        Self {
            target,
            copied: Vec::new(),
            moved: Vec::new(),
            placed: Vec::new(),
            undo: None,
            skipped: 0,
            failed: Vec::new(),
            left_behind: 0,
            cancelled: false,
        }
    }

    fn add(&mut self, item: &StoredItem, target: &VaultPath, name: &str, placed: Placed) {
        let shown = target.join_name(name).to_string();
        let from = item.source.display().to_string();
        self.placed.push(shown.clone());
        match placed {
            Placed::Renamed => self.moved.push(ImportMoved { from, to: shown }),
            Placed::Copied => self.copied.push(shown),
            Placed::MovedAcross {
                removed,
                left_behind,
            } => {
                self.left_behind += left_behind;
                // some original gone: undo must be able to go back; none gone: it is a copy
                if removed > 0 {
                    self.moved.push(ImportMoved { from, to: shown });
                } else {
                    self.copied.push(shown);
                }
            }
        }
    }

    fn into_outcome(self, run: &Run<'_>) -> ImportOutcome {
        ImportOutcome {
            target: self.target,
            copied: count(self.copied.len()),
            moved: count(self.moved.len()),
            files: run.files_done,
            bytes: run.bytes_done,
            skipped: self.skipped,
            failed: self.failed,
            links: run.links,
            left_behind: self.left_behind,
            cancelled: self.cancelled,
            placed: self.placed,
            undo: self.undo,
        }
    }
}

/// Duplicates (the same entry twice) and entries inside another dropped folder go; order kept.
fn collapse(seen: Vec<Seen>) -> Vec<Seen> {
    let covered: Vec<bool> = seen
        .iter()
        .enumerate()
        .map(|(at, found)| {
            seen.iter().enumerate().any(|(other, outer)| {
                other != at
                    && ((found.canon == outer.canon && other < at)
                        || (outer.is_dir
                            && found.canon != outer.canon
                            && found.canon.starts_with(&outer.canon)))
            })
        })
        .collect();
    seen.into_iter()
        .zip(covered)
        .filter_map(|(found, covered)| (!covered).then_some(found))
        .collect()
}

#[cfg(test)]
mod tests;
