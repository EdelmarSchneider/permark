//! The file system side of the Explorer's operations: checking that a rename, move, delete or new folder
//! is allowed, and doing it. Every path comes from the UI, so every one goes through [`Vault::resolve`]
//! first, and nothing is overwritten: a destination that exists is refused.
//!
//! Paths are resolved through their *parent folder* and the name is joined on afterwards. Resolving the
//! whole path would follow a link to its target (renaming the target instead of the link) and, for a
//! case-only rename on Windows, hand back the old spelling of the name.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use super::plan::Move;
use super::FileOpError;
use crate::edit::validate_file_name;
use crate::util::retry_io;
use crate::vault::{key, strip_root, Vault, VaultError, WORKTREE_PREFIX};
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
    /// The root (the project or a worktree) both ends are in.
    pub root: usize,
    /// Only the case of the name changes: the destination "exists" because it is the source itself.
    pub case_only: bool,
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

/// Whether Windows would read `name` as something other than the one entry it spells: `D:x` is a file on the
/// current folder of drive D (and `a:b` a data stream of `a`), and a trailing dot or space is dropped, so
/// `.git.` is `.git`. Not a rule of other systems, but a function of its own so every system tests it.
fn windows_would_misread(name: &str) -> bool {
    name.contains(':') || name.ends_with('.') || name.ends_with(' ')
}

/// Whether `name` is one plain path component: no separators, no `.` or `..`, no drive prefix, and nothing
/// the system would read as another place. Joining such a name onto a folder can only go one level down.
fn plain_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    let one_component = matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(only)), None) if only == OsStr::new(name)
    );
    one_component && !(cfg!(windows) && windows_would_misread(name))
}

/// Whether `abs` is a place strictly inside the root `root` is the index of: below it by plain components
/// only. The last look before the disk is touched, on top of everything [`locate`] did.
fn confine(vault: &Vault, root: usize, abs: &Path) -> Result<(), FileOpError> {
    let inside = vault
        .root_path(root)
        .and_then(|base| strip_root(abs, base))
        .is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .iter()
                    .all(|part| *part != OsStr::new("..") && plain_name(&part.to_string_lossy()))
        });
    if inside {
        Ok(())
    } else {
        Err(FileOpError::Vault(VaultError::Escapes(
            abs.display().to_string(),
        )))
    }
}

/// Places a path of the UI inside the vault. Roots and protected folders are refused here.
///
/// The parent folder goes through [`Vault::resolve`]; the name is joined on afterwards, so it must be one
/// plain component (see [`plain_name`]): `PathBuf::join` would let a name such as `D:x` replace the folder.
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
    if !plain_name(name) {
        return Err(FileOpError::Vault(VaultError::Invalid(path.to_owned())));
    }
    let parent = vault.resolve(parent_of(norm))?;
    let rel = if parent.rel.is_empty() {
        name.to_owned()
    } else {
        format!("{}/{name}", parent.rel)
    };
    // the folders as the disk spells them: a spelling that only resolves to `.git` is caught here
    if is_root_path(&rel) || is_protected(&rel) {
        return Err(FileOpError::Protected(path.to_owned()));
    }
    let abs = parent.abs.join(name);
    confine(vault, parent.root, &abs)?;
    Ok(Located {
        abs,
        rel,
        root: parent.root,
        name: name.to_owned(),
    })
}

/// The linked worktree inside `folder`, if there is one: moving or deleting the folder would take it along.
fn worktree_inside<'a>(vault: &'a Vault, folder: &Path) -> Option<&'a str> {
    vault
        .worktrees()
        .iter()
        .find(|wt| strip_root(&wt.path, folder).is_some())
        .map(|wt| wt.name.as_str())
}

fn check_name(name: &str) -> Result<(), FileOpError> {
    validate_file_name(name).map_err(|e| FileOpError::InvalidName(e.to_string()))
}

// ---- move and rename ---------------------------------------------------------------------------------

/// Checks a set of moves. Every one of these is an error: a source that does not exist, a root or a
/// protected folder, a folder that holds a linked worktree, a destination outside the root of the source
/// (the project and each worktree are separate roots), a bad name, a destination that exists, a folder moved
/// into itself, two moves that overlap or land on the same name.
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
        if meta.is_dir() {
            if let Some(worktree) = worktree_inside(vault, &from.abs) {
                return Err(FileOpError::HoldsWorktree {
                    path: from.rel,
                    worktree: worktree.to_owned(),
                });
            }
        }
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
            root: from.root,
            case_only,
        });
    }
    check_together(&valid)?;
    Ok(valid)
}

/// The moves of one operation must not step on each other.
///
/// Destinations are compared without regard to case on every system: on a case-insensitive volume (Windows,
/// the default on macOS) `Plan.md` and `plan.md` are one file, and which kind of volume a folder is on is not
/// known from here. On a case-sensitive one this refuses a harmless pair, which can be moved in two steps.
fn check_together(moves: &[ValidMove]) -> Result<(), FileOpError> {
    let mut destinations: HashSet<String> = HashSet::new();
    for (i, a) in moves.iter().enumerate() {
        if !destinations.insert(a.to.to_lowercase()) {
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

/// Renames a file or folder, retrying the transient errors Windows gives while something else has it open.
fn os_rename(from: &Path, to: &Path) -> io::Result<()> {
    retry_io(&RENAME_RETRY, || std::fs::rename(from, to))
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

/// What the watcher will report for a move: every file, old and new path.
fn announced_pairs(m: &ValidMove) -> Vec<(String, String)> {
    if m.is_dir {
        list_files(&m.abs_from, &m.from)
            .into_iter()
            .map(|old| {
                let new = format!("{}{}", m.to, &old[m.from.len()..]);
                (old, new)
            })
            .collect()
    } else {
        vec![(m.from.clone(), m.to.clone())]
    }
}

/// Moves the files one after the other. If one fails, the ones already moved are put back and the
/// error is returned, so a failed operation leaves the folder as it was. What cannot be put back is
/// named in the error ([`FileOpError::PartlyMoved`]).
pub(crate) fn execute_moves(
    vault: &Vault,
    own: &OwnWrites,
    moves: &[ValidMove],
) -> Result<(), FileOpError> {
    run_moves(vault, own, moves, &os_rename)
}

/// [`execute_moves`] with the rename that does the work given, so a test can make one fail.
fn run_moves(
    vault: &Vault,
    own: &OwnWrites,
    moves: &[ValidMove],
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(), FileOpError> {
    let pairs: Vec<Vec<(String, String)>> = moves.iter().map(announced_pairs).collect();
    own.expect_moves(&pairs.concat());
    let mut done: Vec<usize> = Vec::new();
    for (at, m) in moves.iter().enumerate() {
        match move_one(vault, m, rename) {
            Ok(()) => done.push(at),
            Err(e) => {
                let stuck = put_back(vault, moves, &done, rename);
                // what stayed where it was, or went back, is not a move the feed should hide
                let settled: Vec<(String, String)> = pairs
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !stuck.contains(i))
                    .flat_map(|(_, p)| p.iter().cloned())
                    .collect();
                own.retract_moves(&settled);
                return Err(if stuck.is_empty() {
                    e
                } else {
                    FileOpError::PartlyMoved {
                        reason: e.to_string(),
                        left: stuck.iter().map(|&i| moves[i].as_move()).collect(),
                    }
                });
            }
        }
    }
    Ok(())
}

/// One rename, with the last checks before the disk is touched: both ends are inside the root, and the
/// destination is still free.
///
/// The free check and the rename are two steps: the system's rename replaces a destination that appears
/// in between (there is no portable rename that refuses to, and this crate has no `unsafe` to call the
/// ones that exist). The window is a few microseconds wide, and what it can cost is a file that another
/// program created in that instant.
fn move_one(
    vault: &Vault,
    m: &ValidMove,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(), FileOpError> {
    confine(vault, m.root, &m.abs_from)?;
    confine(vault, m.root, &m.abs_to)?;
    if !m.case_only && std::fs::symlink_metadata(&m.abs_to).is_ok() {
        return Err(FileOpError::Exists(m.to.clone()));
    }
    rename(&m.abs_from, &m.abs_to).map_err(|e| io_error(&m.from, e))
}

/// Puts the moves `done` back, last first. Returns those that could not go back (something is in the way
/// of the old place, or the rename failed).
fn put_back(
    vault: &Vault,
    moves: &[ValidMove],
    done: &[usize],
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> Vec<usize> {
    let mut stuck = Vec::new();
    for &at in done.iter().rev() {
        let m = &moves[at];
        let back = ValidMove {
            from: m.to.clone(),
            to: m.from.clone(),
            abs_from: m.abs_to.clone(),
            abs_to: m.abs_from.clone(),
            ..m.clone()
        };
        if move_one(vault, &back, rename).is_err() {
            stuck.push(at);
        }
    }
    stuck
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
    /// The root (the project or a worktree) it is in.
    pub root: usize,
}

/// Checks the paths to delete: they exist, none is a root or protected, none is a folder that holds a
/// linked worktree, and a path inside another one that is deleted too is dropped (it goes with its folder).
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
        if meta.is_dir() {
            if let Some(worktree) = worktree_inside(vault, &target.abs) {
                return Err(FileOpError::HoldsWorktree {
                    path: target.rel,
                    worktree: worktree.to_owned(),
                });
            }
        }
        if !all.iter().any(|d| key(&d.rel) == key(&target.rel)) {
            all.push(ValidDelete {
                rel: target.rel,
                abs: target.abs,
                is_dir: meta.is_dir(),
                root: target.root,
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

/// The vault paths the watcher will report as removed when `d` is deleted.
fn announced_removals(d: &ValidDelete) -> Vec<String> {
    if d.is_dir {
        list_files(&d.abs, &d.rel)
    } else {
        vec![d.rel.clone()]
    }
}

/// Deletes to the recycle bin and announces the removals to the watcher. If the bin fails, the
/// announcements for what is still there are taken back, so a removal by someone else is news again.
pub(crate) fn execute_deletes(
    vault: &Vault,
    own: &OwnWrites,
    recycler: &dyn Recycler,
    deletes: &[ValidDelete],
) -> Result<(), FileOpError> {
    for d in deletes {
        confine(vault, d.root, &d.abs)?;
    }
    let announced: Vec<Vec<String>> = deletes.iter().map(announced_removals).collect();
    own.expect_removals(&announced.concat());
    let paths: Vec<PathBuf> = deletes.iter().map(|d| d.abs.clone()).collect();
    recycler.recycle(&paths).map_err(|message| {
        // the bin may have taken some of them before it failed: only what is still there is retracted
        for (d, names) in deletes.iter().zip(&announced) {
            if std::fs::symlink_metadata(&d.abs).is_ok() {
                own.retract_removals(names);
            }
        }
        FileOpError::Trash(message)
    })
}

#[cfg(test)]
mod tests;
