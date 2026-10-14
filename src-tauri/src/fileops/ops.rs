//! The file system side of the Explorer's operations: checking that a rename, move, delete or new folder
//! is allowed, and doing it. Every path comes from the UI, so every one goes through [`Vault::resolve`]
//! first, and nothing is overwritten: a destination that exists is refused.
//!
//! Paths are resolved through their *parent folder* and the name is joined on afterwards. Resolving the
//! whole path would follow a link to its target (renaming the target instead of the link) and, for a
//! case-only rename on Windows, hand back the old spelling of the name.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::plan::Move;
use super::FileOpError;
use crate::edit::validate_file_name;
use crate::util::retry_io;
use crate::vault::{key, Vault, VaultError, WORKTREE_PREFIX};
use crate::watch::OwnWrites;

/// Folders the app keeps for itself or that make a folder a repository: never renamed, moved or deleted from the Explorer.
const PROTECTED: [&str; 2] = [".git", ".trailmark"];

const RENAME_RETRY: [Duration; 3] = [
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(200),
];

/// At most this many files of a folder are announced to the watcher; the rest would show in the feed.
const MAX_ANNOUNCED: usize = 20_000;

/// A move that passed the checks: both ends are inside the same root, the destination is free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidMove {
    /// Vault paths in the UI form.
    pub from: String,
    pub to: String,
    pub abs_from: PathBuf,
    pub abs_to: PathBuf,
    pub is_dir: bool,
}

impl ValidMove {
    pub(crate) fn as_move(&self) -> Move {
        Move {
            from: self.from.clone(),
            to: self.to.clone(),
        }
    }
}

/// A path of the UI placed inside the vault: its parent folder is resolved, the name is as given.
#[derive(Debug, Clone)]
struct Located {
    rel: String,
    abs: PathBuf,
    root: usize,
    name: String,
}

/// Whether a path is a root: the project's own, or the folder of a worktree.
fn is_root_path(rel: &str) -> bool {
    rel.is_empty()
        || rel
            .strip_prefix(WORKTREE_PREFIX)
            .is_some_and(|rest| !rest.contains('/'))
}

fn is_protected(rel: &str) -> bool {
    rel.split('/')
        .any(|part| PROTECTED.iter().any(|p| part.eq_ignore_ascii_case(p)))
}

fn io_error(path: &str, source: io::Error) -> FileOpError {
    if source.kind() == io::ErrorKind::NotFound {
        FileOpError::Vault(VaultError::NotFound(path.to_owned()))
    } else {
        FileOpError::Io {
            path: path.to_owned(),
            source,
        }
    }
}

fn parent_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// Whether `path` is `folder` or inside it.
fn inside(path: &str, folder: &str) -> bool {
    let (p, f) = (key(path), key(folder));
    p == f || p.starts_with(&format!("{f}/"))
}

/// Places a path of the UI inside the vault. Roots and protected folders are refused here.
fn locate(vault: &Vault, path: &str) -> Result<Located, FileOpError> {
    if path.contains('\0') {
        return Err(FileOpError::Vault(VaultError::Invalid(
            "contains NUL".to_owned(),
        )));
    }
    let norm = path.replace('\\', "/");
    let norm = norm.trim_end_matches('/');
    // the parent is what gets resolved, so an absolute path has to be refused before it is cut up
    if norm.starts_with('/') {
        return Err(FileOpError::Vault(VaultError::Invalid(format!(
            "absolute path: {path}"
        ))));
    }
    if is_root_path(norm) || is_protected(norm) {
        return Err(FileOpError::Protected(path.to_owned()));
    }
    let name = norm.rsplit('/').next().unwrap_or(norm);
    if matches!(name, "" | "." | "..") {
        return Err(FileOpError::Vault(VaultError::Invalid(path.to_owned())));
    }
    let parent = vault.resolve(parent_of(norm))?;
    let rel = if parent.rel.is_empty() {
        name.to_owned()
    } else {
        format!("{}/{name}", parent.rel)
    };
    Ok(Located {
        abs: parent.abs.join(name),
        rel,
        root: parent.root,
        name: name.to_owned(),
    })
}

fn check_name(name: &str) -> Result<(), FileOpError> {
    validate_file_name(name).map_err(|e| FileOpError::InvalidName(e.to_string()))
}

// ---- move and rename ---------------------------------------------------------------------------------

/// Checks a set of moves. Every one of these is an error: a source that does not exist, a root or a
/// protected folder, a destination outside the root of the source (the project and each worktree are
/// separate roots), a bad name, a destination that exists, a folder moved into itself, two moves that
/// overlap or land on the same name.
pub(crate) fn check_moves(vault: &Vault, moves: &[Move]) -> Result<Vec<ValidMove>, FileOpError> {
    if moves.is_empty() {
        return Err(FileOpError::Nothing);
    }
    let mut valid: Vec<ValidMove> = Vec::with_capacity(moves.len());
    for m in moves {
        let from = locate(vault, &m.from)?;
        let to = locate(vault, &m.to)?;
        if from.root != to.root {
            return Err(FileOpError::OtherRoot(m.to.clone()));
        }
        let meta = std::fs::symlink_metadata(&from.abs).map_err(|e| io_error(&from.rel, e))?;
        if to.name != from.name {
            check_name(&to.name)?;
        }
        let case_only = key(&from.rel) == key(&to.rel);
        if case_only && from.rel == to.rel {
            return Err(FileOpError::Nothing);
        }
        if !case_only {
            if meta.is_dir() && inside(&to.rel, &from.rel) {
                return Err(FileOpError::IntoItself(from.rel));
            }
            if std::fs::symlink_metadata(&to.abs).is_ok() {
                return Err(FileOpError::Exists(to.rel));
            }
        }
        if !to.abs.parent().is_some_and(Path::is_dir) {
            return Err(FileOpError::Vault(VaultError::NotFound(
                parent_of(&to.rel).to_owned(),
            )));
        }
        valid.push(ValidMove {
            from: from.rel,
            to: to.rel,
            abs_from: from.abs,
            abs_to: to.abs,
            is_dir: meta.is_dir(),
        });
    }
    check_together(&valid)?;
    Ok(valid)
}

/// The moves of one operation must not step on each other.
fn check_together(moves: &[ValidMove]) -> Result<(), FileOpError> {
    let mut destinations: HashSet<String> = HashSet::new();
    for (i, a) in moves.iter().enumerate() {
        if !destinations.insert(key(&a.to)) {
            return Err(FileOpError::Exists(a.to.clone()));
        }
        for (j, b) in moves.iter().enumerate() {
            // one source inside another, or a destination inside a folder that is itself moving
            if i != j && (inside(&a.from, &b.from) || inside(parent_of(&a.to), &b.from)) {
                return Err(FileOpError::Overlap(a.from.clone()));
            }
        }
    }
    Ok(())
}

fn rename_path(from: &Path, to: &Path, rel: &str) -> Result<(), FileOpError> {
    retry_io(&RENAME_RETRY, || std::fs::rename(from, to)).map_err(|e| io_error(rel, e))
}

/// The vault paths of the files inside a folder, any depth (up to a limit).
fn list_files(abs_dir: &Path, rel_dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(abs_dir.to_path_buf(), rel_dir.to_owned())];
    while let Some((dir, rel)) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if out.len() >= MAX_ANNOUNCED {
                return out;
            }
            let child = format!("{rel}/{}", entry.file_name().to_string_lossy());
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push((entry.path(), child));
            } else {
                out.push(child);
            }
        }
    }
    out
}

/// What the watcher will report for a set of moves: every file, old and new path.
fn announced_pairs(moves: &[ValidMove]) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for m in moves {
        if m.is_dir {
            for old in list_files(&m.abs_from, &m.from) {
                let new = format!("{}{}", m.to, &old[m.from.len()..]);
                pairs.push((old, new));
            }
        } else {
            pairs.push((m.from.clone(), m.to.clone()));
        }
    }
    pairs
}

/// Moves the files one after the other. If one fails, the ones already moved are put back and the
/// error is returned, so a failed operation leaves the folder as it was.
pub(crate) fn execute_moves(own: &OwnWrites, moves: &[ValidMove]) -> Result<(), FileOpError> {
    let pairs = announced_pairs(moves);
    own.expect_moves(&pairs);
    let mut done: Vec<&ValidMove> = Vec::new();
    for m in moves {
        if let Err(e) = rename_path(&m.abs_from, &m.abs_to, &m.from) {
            for undo in done.iter().rev() {
                // best effort: the folder should be as it was
                let _ = std::fs::rename(&undo.abs_to, &undo.abs_from);
            }
            own.retract_moves(&pairs);
            return Err(e);
        }
        done.push(m);
    }
    Ok(())
}

// ---- new folder ------------------------------------------------------------------------------------

/// Creates a folder (not its parents). Returns its vault path.
pub(crate) fn create_folder(vault: &Vault, path: &str) -> Result<String, FileOpError> {
    let target = locate(vault, path)?;
    check_name(&target.name)?;
    if std::fs::symlink_metadata(&target.abs).is_ok() {
        return Err(FileOpError::Exists(target.rel));
    }
    std::fs::create_dir(&target.abs).map_err(|e| io_error(&target.rel, e))?;
    Ok(target.rel)
}

/// Removes a folder if it is empty (the undo of "New folder"). Returns whether it was removed.
pub(crate) fn remove_empty_folder(vault: &Vault, path: &str) -> bool {
    locate(vault, path).is_ok_and(|target| std::fs::remove_dir(target.abs).is_ok())
}

// ---- delete ----------------------------------------------------------------------------------------

/// Sends files and folders to the system's recycle bin (or trash).
pub trait Recycler: Send + Sync {
    fn recycle(&self, paths: &[PathBuf]) -> Result<(), String>;
}

/// The operating system's recycle bin.
pub struct OsRecycler;

impl Recycler for OsRecycler {
    fn recycle(&self, paths: &[PathBuf]) -> Result<(), String> {
        trash::delete_all(paths).map_err(|e| e.to_string())
    }
}

/// A file or folder that will be deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidDelete {
    pub rel: String,
    pub abs: PathBuf,
    pub is_dir: bool,
}

/// Checks the paths to delete: they exist, none is a root or protected, and a path inside another one
/// that is deleted too is dropped (it goes with its folder).
pub(crate) fn check_deletes(
    vault: &Vault,
    paths: &[String],
) -> Result<Vec<ValidDelete>, FileOpError> {
    if paths.is_empty() {
        return Err(FileOpError::Nothing);
    }
    let mut all: Vec<ValidDelete> = Vec::new();
    for path in paths {
        let target = locate(vault, path)?;
        let meta = std::fs::symlink_metadata(&target.abs).map_err(|e| io_error(&target.rel, e))?;
        if !all.iter().any(|d| key(&d.rel) == key(&target.rel)) {
            all.push(ValidDelete {
                rel: target.rel,
                abs: target.abs,
                is_dir: meta.is_dir(),
            });
        }
    }
    let rels: Vec<String> = all.iter().map(|d| d.rel.clone()).collect();
    all.retain(|d| {
        !rels
            .iter()
            .any(|other| key(other) != key(&d.rel) && inside(&d.rel, other))
    });
    Ok(all)
}

/// Deletes to the recycle bin and announces the removals to the watcher.
pub(crate) fn execute_deletes(
    own: &OwnWrites,
    recycler: &dyn Recycler,
    deletes: &[ValidDelete],
) -> Result<(), FileOpError> {
    let mut announced: Vec<String> = Vec::new();
    for d in deletes {
        if d.is_dir {
            announced.extend(list_files(&d.abs, &d.rel));
        } else {
            announced.push(d.rel.clone());
        }
    }
    own.expect_removals(&announced);
    let paths: Vec<PathBuf> = deletes.iter().map(|d| d.abs.clone()).collect();
    recycler.recycle(&paths).map_err(FileOpError::Trash)
}

#[cfg(test)]
mod tests;
