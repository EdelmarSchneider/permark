//! The file system side of the Explorer's operations: checking that a rename, move, delete or new folder is allowed,
//! and doing it. Every path comes from the UI, so every one goes through [`Vault::resolve`] first and nothing is
//! overwritten.
//!
//! Paths are resolved through their *parent folder* and the name is joined on afterwards. Resolving the whole path
//! would follow a link to its target (moving the target instead of the link) and, for a case-only rename on Windows,
//! hand back the old spelling of the name.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{check_name, FileOpError};
use crate::fsx::{retry, RETRY_DELAYS};
use crate::paths::{fold, strip_root, Vault, VaultPath, WORKTREE_PREFIX};
use crate::tree::below_root;

/// At most this many files of a moved folder are announced to the watch; the rest would show as news.
pub(super) const MAX_ANNOUNCED: usize = 500;

/// A move that passed the checks: both ends inside the same root, the destination is free.
#[derive(Debug, Clone)]
pub(crate) struct ValidMove {
    pub from: VaultPath,
    pub to: VaultPath,
    pub abs_from: PathBuf,
    pub abs_to: PathBuf,
    pub is_dir: bool,
    /// Only the case of the name changes: the destination "exists" because it is the source itself.
    pub case_only: bool,
}

pub(crate) fn io_error(path: &VaultPath, source: io::Error) -> FileOpError {
    FileOpError::Io {
        path: path.to_string(),
        source,
    }
}

/// Which root a path belongs to: 0 = the project folder, n = worktree n-1.
fn root_index(vault: &Vault, path: &VaultPath) -> Option<usize> {
    if !path.as_str().starts_with(WORKTREE_PREFIX) {
        return Some(0);
    }
    let name = below_prefix_name(path.as_str())?;
    vault
        .worktrees()
        .iter()
        .position(|w| fold(&w.name) == fold(name))
        .map(|at| at + 1)
}

fn below_prefix_name(path: &str) -> Option<&str> {
    path.strip_prefix(WORKTREE_PREFIX)?.split('/').next()
}

/// The project folder itself or a worktree's folder.
pub(crate) fn is_root_path(path: &VaultPath) -> bool {
    below_root(path.as_str()).is_empty()
}

/// Folders the app keeps for itself or that make a folder a repository: never renamed, moved or deleted here.
pub(crate) fn is_protected(path: &VaultPath) -> bool {
    let rel = below_root(path.as_str());
    let mut parts = rel.split('/');
    parts.next().is_some_and(|first| fold(first) == ".permark")
        || rel.split('/').any(|part| fold(part) == ".git")
}

/// Absolute place of a project path: its parent folder is resolved (links followed, escape refused), the name joined.
pub(crate) fn locate(vault: &Vault, path: &VaultPath) -> Result<PathBuf, FileOpError> {
    let (Some(parent), Some(name)) = (path.parent(), path.name()) else {
        return Err(FileOpError::Protected(path.to_string()));
    };
    Ok(vault.resolve(&parent)?.join(name))
}

/// Last look before the disk is touched: `abs` is a place strictly inside the root `root` is the index of.
fn confine(vault: &Vault, root: usize, abs: &Path) -> Result<(), FileOpError> {
    let base = if root == 0 {
        vault.main().path()
    } else {
        vault.worktrees()[root - 1].root.path()
    };
    match strip_root(abs, base) {
        Some(rest) if !rest.is_empty() => Ok(()),
        _ => Err(FileOpError::Vault(crate::paths::VaultError::Escapes(
            abs.display().to_string(),
        ))),
    }
}

/// The linked worktree inside `folder`, if there is one: moving or deleting the folder would take it along.
fn worktree_inside(vault: &Vault, folder: &Path) -> Option<String> {
    vault
        .worktrees()
        .iter()
        .find(|w| strip_root(w.root.path(), folder).is_some())
        .map(|w| w.name.clone())
}

/// Checks a set of moves. Every one of these is an error: a source that does not exist, a root or a protected
/// folder, a folder that holds a linked worktree, a destination outside the root of the source, a bad name, a
/// destination that exists, a folder moved into itself, two moves that overlap or land on the same name.
pub(crate) fn check_moves(
    vault: &Vault,
    moves: &[(VaultPath, VaultPath)],
) -> Result<Vec<ValidMove>, FileOpError> {
    if moves.is_empty() {
        return Err(FileOpError::Nothing);
    }
    let mut valid = Vec::with_capacity(moves.len());
    for (from, to) in moves {
        for path in [from, to] {
            if is_root_path(path) || is_protected(path) {
                return Err(FileOpError::Protected(path.to_string()));
            }
        }
        let (Some(from_root), Some(to_root)) = (root_index(vault, from), root_index(vault, to))
        else {
            return Err(FileOpError::Vault(crate::paths::VaultError::NotFound(
                to.to_string(),
            )));
        };
        if from_root != to_root {
            return Err(FileOpError::OtherRoot(to.to_string()));
        }
        let name = to.name().unwrap_or("");
        check_name(name).map_err(FileOpError::InvalidName)?;
        let abs_from = locate(vault, from)?;
        let abs_to = locate(vault, to)?;
        confine(vault, from_root, &abs_from)?;
        confine(vault, to_root, &abs_to)?;
        let meta = fs::symlink_metadata(&abs_from).map_err(|e| io_error(from, e))?;
        let is_dir = meta.is_dir();
        // the same entry under another case is a rename; the same name is nothing to do
        let same_parent = from.parent().map(|p| p.key()) == to.parent().map(|p| p.key());
        let case_only = same_parent && from.key() == to.key() && from.name() != to.name();
        if from.key() == to.key() && !case_only {
            return Err(FileOpError::Nothing);
        }
        if is_dir {
            if to.is_within(from) {
                return Err(FileOpError::IntoItself(from.to_string()));
            }
            if let Some(worktree) = worktree_inside(vault, &abs_from) {
                return Err(FileOpError::HoldsWorktree {
                    path: from.to_string(),
                    worktree,
                });
            }
        }
        let parent_ok = to
            .parent()
            .and_then(|p| vault.resolve(&p).ok())
            .is_some_and(|p| fs::metadata(p).is_ok_and(|m| m.is_dir()));
        if !parent_ok {
            return Err(FileOpError::Vault(crate::paths::VaultError::NotFound(
                to.parent().map_or_else(String::new, |p| p.to_string()),
            )));
        }
        // any other existing destination is refused
        if !case_only && fs::symlink_metadata(&abs_to).is_ok() {
            return Err(FileOpError::Exists(to.to_string()));
        }
        valid.push(ValidMove {
            from: from.clone(),
            to: to.clone(),
            abs_from,
            abs_to,
            is_dir,
            case_only,
        });
    }
    check_together(&valid)?;
    Ok(valid)
}

/// The moves of one operation must not step on each other. Destinations are compared without regard to case on
/// every system: on a case-insensitive volume `Plan.md` and `plan.md` are one file, and which kind of volume a
/// folder is on is not known from here.
fn check_together(moves: &[ValidMove]) -> Result<(), FileOpError> {
    for (at, a) in moves.iter().enumerate() {
        for b in &moves[at + 1..] {
            if a.from.is_within(&b.from) || b.from.is_within(&a.from) {
                return Err(FileOpError::Overlap(a.from.to_string()));
            }
            let (ta, tb) = (a.to.key().to_lowercase(), b.to.key().to_lowercase());
            if ta == tb || ta.starts_with(&format!("{tb}/")) || tb.starts_with(&format!("{ta}/")) {
                return Err(FileOpError::Overlap(a.to.to_string()));
            }
        }
    }
    Ok(())
}

/// Renames a file or folder, retrying the transient errors Windows gives while something else has it open.
pub(super) fn os_rename(from: &Path, to: &Path) -> io::Result<()> {
    retry(&RETRY_DELAYS, || fs::rename(from, to))
}

/// Moves the files one after the other. If one fails, the ones already moved are put back and the error is
/// returned, so a failed operation leaves the folder as it was. What cannot be put back is named in the error.
pub(crate) fn execute_moves(moves: &[ValidMove]) -> Result<(), FileOpError> {
    run_moves(moves, os_rename)
}

/// [`execute_moves`] with the rename given, so a test can make one fail.
pub(crate) fn run_moves(
    moves: &[ValidMove],
    rename: impl Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(), FileOpError> {
    for (done, m) in moves.iter().enumerate() {
        if let Err(error) = move_one(m, &rename) {
            let left = put_back(&moves[..done], &rename);
            return Err(if left.is_empty() {
                error
            } else {
                FileOpError::PartlyMoved {
                    reason: Box::new(error),
                    left,
                }
            });
        }
    }
    Ok(())
}

/// One rename with the last check before the disk is touched: the destination is still free. (A rename that
/// replaces a destination appearing in between cannot be refused portably without `unsafe`; the window is a few
/// microseconds wide.)
fn move_one(
    m: &ValidMove,
    rename: &impl Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(), FileOpError> {
    if !m.case_only && fs::symlink_metadata(&m.abs_to).is_ok() {
        return Err(FileOpError::Exists(m.to.to_string()));
    }
    rename(&m.abs_from, &m.abs_to).map_err(|e| io_error(&m.from, e))
}

/// Puts the moves `done` back, last first. Returns those that could not go back.
fn put_back(
    done: &[ValidMove],
    rename: &impl Fn(&Path, &Path) -> io::Result<()>,
) -> Vec<(String, String)> {
    let mut left = Vec::new();
    for m in done.iter().rev() {
        let free = m.case_only || fs::symlink_metadata(&m.abs_from).is_err();
        if !free || rename(&m.abs_to, &m.abs_from).is_err() {
            left.push((m.from.to_string(), m.to.to_string()));
        }
    }
    left
}

/// The project paths of the files inside a folder, any depth (up to a limit).
pub(super) fn list_files(abs_dir: &Path, rel_dir: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(abs_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_ANNOUNCED {
            return;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let rel = format!("{rel_dir}/{name}");
        match entry.file_type() {
            Ok(t) if t.is_dir() => list_files(&entry.path(), &rel, out),
            Ok(_) => out.push(rel),
            Err(_) => {}
        }
    }
}

/// What the watch will report for a move: the item itself and, for a folder, every file in it (old and new path).
pub(crate) fn announced_pairs(m: &ValidMove) -> Vec<(String, String)> {
    let mut pairs = vec![(m.from.to_string(), m.to.to_string())];
    if m.is_dir {
        let mut files = Vec::new();
        list_files(&m.abs_from, m.from.as_str(), &mut files);
        for file in files {
            let rest = file.get(m.from.as_str().len()..).unwrap_or("");
            pairs.push((file.clone(), format!("{}{rest}", m.to)));
        }
    }
    pairs
}

/// Paths to delete, checked: they exist, stay inside their root, are not a root, protected or holding a worktree;
/// nested paths collapse into the outer one. Returns each path with its absolute place.
pub(crate) fn check_deletes(
    vault: &Vault,
    paths: &[VaultPath],
) -> Result<Vec<(VaultPath, PathBuf)>, FileOpError> {
    if paths.is_empty() {
        return Err(FileOpError::Nothing);
    }
    let mut checked: Vec<(VaultPath, PathBuf)> = Vec::new();
    for path in paths {
        if is_root_path(path) || is_protected(path) {
            return Err(FileOpError::Protected(path.to_string()));
        }
        let Some(root) = root_index(vault, path) else {
            return Err(FileOpError::Vault(crate::paths::VaultError::NotFound(
                path.to_string(),
            )));
        };
        let abs = locate(vault, path)?;
        confine(vault, root, &abs)?;
        let meta = fs::symlink_metadata(&abs).map_err(|e| io_error(path, e))?;
        if meta.is_dir() {
            if let Some(worktree) = worktree_inside(vault, &abs) {
                return Err(FileOpError::HoldsWorktree {
                    path: path.to_string(),
                    worktree,
                });
            }
        }
        checked.push((path.clone(), abs));
    }
    // a path inside another listed path goes with it; duplicates collapse
    let keys: Vec<String> = checked.iter().map(|(p, _)| p.key()).collect();
    let mut kept: Vec<(VaultPath, PathBuf)> = Vec::new();
    for (at, (path, abs)) in checked.iter().enumerate() {
        let covered = keys.iter().enumerate().any(|(other, key)| {
            other != at
                && (path.key().starts_with(&format!("{key}/"))
                    || (path.key() == *key && other < at))
        });
        if !covered {
            kept.push((path.clone(), abs.clone()));
        }
    }
    Ok(kept)
}
