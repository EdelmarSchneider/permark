//! Turns "something happened to these paths" hints into real changes by looking at the disk. Events are hints, the
//! disk is the truth: each hinted path is stat'ed and compared with the signature remembered for it. No-op changes
//! (same bytes saved again, antivirus touches, sync clients) are dropped, atomic saves become one `Modified`, a
//! removal plus a creation of the same content becomes a `Renamed`, a folder moved gets one `Renamed` for the folder,
//! a folder that appeared is expanded into its files, a file truncated to nothing gets one more tick to receive its
//! content.

use std::collections::{HashMap, HashSet};
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::noise::{is_git_signal, is_news, team_holder};
use super::sigs::{sig_retried, stat_sig, Sig, SigStore, HASH_LIMIT, NEW_HASH_LIMIT};
use super::types::{Change, ChangeKind};
use crate::fsx::RETRY_DELAYS;
use crate::paths::Vault;
use crate::tree::{below_root, key_of, Ignorer};

/// A created folder is expanded into its files up to this many.
const MAX_EXPANDED: usize = 500;
const MAX_EXPAND_DEPTH: usize = 12;

/// Waits between tries of a file another program holds: one short one, then the hint comes back on the next tick
/// (each wait holds up the whole batch on the watch thread).
const LOCKED_RETRY: &[std::time::Duration] = &[RETRY_DELAYS[0]];

/// What the events said about one path.
#[allow(clippy::struct_excessive_bools)] // independent facts accumulated from several events
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    pub abs: PathBuf,
    pub created: bool,
    pub removed: bool,
    pub modified: bool,
    /// Where it came from, when the event said so (a rename).
    pub rename_from: Option<PathBuf>,
    /// Created in the OS report right after this path was removed (Windows reports a folder moved to another parent
    /// so): maybe a move; only nested projects take it as one ([`project_moves`]).
    pub after_removal: Option<PathBuf>,
    /// Looked at once already (a file truncated to nothing).
    pub deferred: bool,
}

impl Pending {
    pub(crate) fn merge(&mut self, other: &Pending) {
        // the same path under another case of its name (case-insensitive disk): the name it came to is the one now
        if other.created && other.abs != self.abs {
            self.abs.clone_from(&other.abs);
        }
        self.created |= other.created;
        self.removed |= other.removed;
        self.modified |= other.modified;
        if other.rename_from.is_some() {
            self.rename_from.clone_from(&other.rename_from);
        }
        if other.after_removal.is_some() {
            self.after_removal.clone_from(&other.after_removal);
        }
    }
}

pub(crate) struct Env<'a> {
    pub vault: &'a Vault,
    pub sigs: &'a SigStore,
    /// Ignore rules per root: (folded root prefix, rules); the first is the project folder.
    pub ignorers: &'a [(String, Arc<Ignorer>)],
    /// What was at a project path before the watch knew anything of it: `Some(is_folder)` | `None` (unknown).
    pub known: &'a dyn Fn(&str) -> Option<bool>,
    /// Whether this project path is an opened ignored directory (`ignored` and `Dir { unlisted: None }`). Not `known`.
    /// A container and a normal folder are known directories and are not this.
    pub opened_ignored: &'a dyn Fn(&str) -> bool,
    /// Whether the app announced a write of this project path: its content decides whether the change is the app's
    /// own, so a new file there is read at once.
    pub wanted: &'a dyn Fn(&str) -> bool,
}

impl Env<'_> {
    fn ignorer_for(&self, key: &str) -> Option<&Ignorer> {
        self.ignorers
            .iter()
            .skip(1)
            .find(|(prefix, _)| key == prefix || key.starts_with(&format!("{prefix}/")))
            .or_else(|| self.ignorers.first())
            .map(|(_, ignorer)| ignorer.as_ref())
    }

    /// Whether this project path is a team folder | file ([`team_holder`]) of a folder the rules show (the opened
    /// folder, an included project, a shown folder): followed whatever the rules say of the team folder itself. One
    /// that a package ships inside an ignored folder is no team's.
    fn followed_team_path(&self, path: &str) -> bool {
        team_holder(path).is_some_and(|holder| {
            holder.is_empty()
                || self
                    .ignorer_for(&key_of(path))
                    .is_some_and(|ignorer| !ignorer.rel_ignored(holder, true))
        })
    }

    /// Whether a change at this project path is dropped before the disk is looked at.
    fn skip(&self, path: &str, is_dir: bool) -> bool {
        if !is_news(path) {
            return true;
        }
        let rel = below_root(path);
        // git signals are not files of the project tree; team files are followed (the tree decides on its own whether
        // it lists them)
        if rel.starts_with(".git/") || rel.contains("/.git/") || self.followed_team_path(path) {
            return false;
        }
        let ignored = self
            .ignorer_for(&key_of(path))
            .is_some_and(|ignorer| ignorer.rel_ignored(rel, is_dir));
        if !ignored {
            return false;
        }
        // a direct child of an opened ignored folder is news (a new ignored dir stays closed). A container is not
        // one. The project root is not a node. Children of any other dir stay dropped.
        let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        !(self.opened_ignored)(parent)
    }

    /// Whether a hint may be dropped as it arrives, before it counts toward the pending limit (a storm in
    /// `node_modules` would else end in a rescan) and with no look at the disk: its folder is ignored and not opened,
    /// so the pass would drop it too ([`Env::skip`]), and it cannot change which folders are projects. Kept: anything
    /// named `.git` or inside one, a followed team folder | file ([`Env::followed_team_path`]), a rename | a creation
    /// right after a removal (a project folder may be moving), the removal of a folder that held a project, a creation
    /// where a folder moved in may be a repository that shows ([`Ignorer::may_lead_to_project`]). `folders`: what was
    /// found per folder so far (cleared whenever the rules may change).
    pub(crate) fn ignored_early(
        &self,
        hint: &Pending,
        folders: &mut HashMap<PathBuf, bool>,
    ) -> bool {
        if hint.rename_from.is_some() || hint.after_removal.is_some() {
            return false;
        }
        let Some(vp) = self.vault.relativize(&hint.abs) else {
            return false;
        };
        let path = vp.as_str();
        let rel = below_root(path);
        if rel.split('/').any(Ignorer::is_git_name) || self.followed_team_path(path) {
            return false;
        }
        let (Some((parent, _)), Some(dir)) = (path.rsplit_once('/'), hint.abs.parent()) else {
            return false;
        };
        if (self.opened_ignored)(parent) {
            return false;
        }
        let Some(ignorer) = self.ignorer_for(&key_of(path)) else {
            return false;
        };
        let mut closed = |folder: &Path| {
            if let Some(found) = folders.get(folder) {
                return *found;
            }
            let found = ignorer.path_ignored(folder, true);
            folders.insert(folder.to_path_buf(), found);
            found
        };
        if !closed(dir) {
            return false;
        }
        if hint.removed && ignorer.held_project(&ignorer.abs_of(rel)) {
            return false;
        }
        if !hint.created {
            return true;
        }
        // the folder the rules hide first decides whether a repository moved in below it would show
        let mut top = dir;
        while let Some(up) = top.parent() {
            if !closed(up) {
                break;
            }
            top = up;
        }
        !ignorer.may_lead_to_project(top, &hint.abs)
    }

    /// Whether this hint may change which folders are nested repositories (own `.git` came or went, a repository
    /// folder appeared, went or moved; `.permark` changes no decision): then the ignore decisions read so far are
    /// stale. Plain saves never count; a file that appeared costs no look at the disk.
    fn moves_projects(&self, p: &Pending, path: &str, meta: Option<&Metadata>) -> bool {
        if !(p.created || p.removed || p.rename_from.is_some()) {
            return false;
        }
        let Some(ignorer) = self.ignorer_for(&key_of(path)) else {
            return false;
        };
        let rel = below_root(path);
        let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
        if Ignorer::is_git_name(name) {
            return ignorer.project_changed(&ignorer.abs_of(parent));
        }
        if is_git_signal(path) || !is_news(path) {
            return false;
        }
        let here = match meta {
            Some(meta) if meta.is_dir() => ignorer.project_changed(&ignorer.abs_of(rel)),
            Some(_) => false,
            None => ignorer.held_project(&ignorer.abs_of(rel)),
        };
        here || p.rename_from.as_ref().is_some_and(|from| {
            self.vault.relativize(from).is_some_and(|vp| {
                fs::symlink_metadata(from).is_err()
                    && ignorer.held_project(&ignorer.abs_of(below_root(vp.as_str())))
            })
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct Reconciled {
    pub changes: Vec<Change>,
    /// Truncated files to look at again on the next tick.
    pub deferred: Vec<Pending>,
    /// A folder had more files than were listed.
    pub truncated: bool,
    /// Nested projects came, went or moved (decided against what was read before this batch).
    pub projects_moved: bool,
    /// Folders that held a nested project and moved: `(from, to)` (see [`project_moves`]).
    pub moved: Vec<(String, String)>,
}

struct Item {
    pending: Pending,
    path: String,
    meta: Option<Metadata>,
}

/// A file the pass found created.
pub(super) struct Created {
    /// Index in `changes`.
    pub at: usize,
    pub sig: Sig,
    /// Where the event said it came from (a rename), project path.
    pub from: Option<String>,
    /// A new big file not read yet ([`NEW_HASH_LIMIT`]): where to read it when its content may pair it.
    pub unread: Option<PathBuf>,
}

struct Pass<'a> {
    env: &'a Env<'a>,
    out: Reconciled,
    created: Vec<Created>,
    /// (index in `changes`, signature) of removed files.
    removed: Vec<(usize, Sig)>,
    expanded: usize,
}

/// Decides what really changed. `pending` is in arrival order; the result keeps that order.
pub(crate) fn reconcile(env: &Env<'_>, pending: Vec<Pending>) -> Reconciled {
    let mut items: Vec<Item> = Vec::new();
    let mut projects_moved = false;
    // each hinted path looked at once
    let looked: Vec<Item> = pending
        .into_iter()
        .filter_map(|p| {
            let path = env.vault.relativize(&p.abs)?.as_str().to_owned();
            let meta = fs::symlink_metadata(&p.abs).ok();
            (!path.is_empty()).then_some(Item {
                pending: p,
                path,
                meta,
            })
        })
        .collect();
    let moved = project_moves(env, &looked);
    for Item {
        pending: p,
        path,
        meta,
    } in looked
    {
        let is_dir = meta.as_ref().is_some_and(Metadata::is_dir);
        // before any skip: the marks read so far are what decided the items of this batch
        projects_moved = projects_moved || env.moves_projects(&p, &path, meta.as_ref());
        if env.skip(&path, is_dir) {
            continue;
        }
        items.push(Item {
            pending: p,
            path,
            meta,
        });
    }
    let mut pass = Pass {
        env,
        out: Reconciled {
            projects_moved: projects_moved || !moved.is_empty(),
            moved,
            ..Reconciled::default()
        },
        created: Vec::new(),
        removed: Vec::new(),
        expanded: 0,
    };
    let moved_dirs = pass.folder_renames(&items);
    for item in &items {
        let key = key_of(&item.path);
        // children of a folder that moved: the folder's own change tells the story
        if moved_dirs
            .iter()
            .any(|(from, _)| key.starts_with(&format!("{}/", key_of(from))) || key == key_of(from))
        {
            continue;
        }
        pass.item(item);
    }
    let mut read = |abs: &Path, shown: &str| {
        let meta = fs::symlink_metadata(abs).ok().filter(Metadata::is_file)?;
        let sig = sig_retried(abs, &meta, LOCKED_RETRY)?;
        env.sigs.set(key_of(shown), shown, sig);
        Some(sig)
    };
    pair_renames(
        &mut pass.out.changes,
        pass.created,
        &pass.removed,
        &mut read,
    );
    pass.out
}

/// Longest chain of renames in one batch followed back to where it began (`a -> b -> c`).
const MAX_RENAME_CHAIN: usize = 8;

/// Folders that held a nested project (at | below, as read before this batch) and moved, `(from, to)`, decided before
/// any ignore rule (a project moved into an ignored folder keeps its identity too):
/// - a rename the OS reported (one event, or two in a row paired by the watch), followed back through renames of the
///   same batch to where it began;
/// - else a folder created right after a removal in the OS report (Windows: a move to another parent);
/// - else the only folder gone + the only folder come by a rename whose start is unknown (a chain the events cut);
/// - else a folder gone + a folder come in the same batch with the same name, each the only one of that name.
fn project_moves(env: &Env<'_>, looked: &[Item]) -> Vec<(String, String)> {
    let rel = |abs: &Path| {
        env.vault
            .relativize(abs)
            .map(|vp| vp.as_str().to_owned())
            .filter(|p| !p.is_empty())
    };
    let held = |path: &str| {
        env.ignorer_for(&key_of(path))
            .is_some_and(|ignorer| ignorer.held_project(&ignorer.abs_of(below_root(path))))
    };
    let by_key: HashMap<String, &Pending> = looked
        .iter()
        .map(|item| (key_of(&item.path), &item.pending))
        .collect();
    let mut moved = Vec::new();
    let mut gone = Vec::new();
    let mut came = Vec::new();
    let mut renamed_in = Vec::new();
    for Item {
        pending: p,
        path,
        meta,
    } in looked
    {
        let path = path.clone();
        let is_dir = meta.as_ref().is_some_and(Metadata::is_dir);
        if let Some(from) = p.rename_from.as_deref() {
            let mut origin = from.to_path_buf();
            for _ in 0..MAX_RENAME_CHAIN {
                let earlier = rel(&origin)
                    .and_then(|o| by_key.get(&key_of(&o)).copied())
                    .and_then(|o| o.rename_from.clone());
                match earlier {
                    Some(e) if e != p.abs => origin = e,
                    _ => break,
                }
            }
            let Some(origin_path) = rel(&origin) else {
                continue;
            };
            let left =
                key_of(&origin_path) != key_of(&path) && fs::symlink_metadata(&origin).is_err();
            if is_dir && left {
                if held(&origin_path) {
                    moved.push((origin_path, path));
                } else {
                    // a chain whose start the events lost (macOS: each step's two events, undirected)
                    renamed_in.push(path);
                }
            }
            continue;
        }
        if let Some(origin) = p.after_removal.as_deref().filter(|_| is_dir && p.created) {
            if let Some(origin_path) = rel(origin) {
                if fs::symlink_metadata(origin).is_err() && held(&origin_path) {
                    moved.push((origin_path, path));
                    continue;
                }
            }
        }
        if meta.is_none() && p.removed && held(&path) {
            gone.push(path);
        } else if is_dir && p.created {
            came.push(path);
        }
    }
    let name = |path: &str| crate::paths::fold(path.rsplit('/').next().unwrap_or(path));
    gone.retain(|g| !moved.iter().any(|(from, _)| key_of(from) == key_of(g)));
    if let ([from], [to]) = (gone.as_slice(), renamed_in.as_slice()) {
        // one folder holding a project went, one folder came by a rename from a place now gone: the same folder
        moved.push((from.clone(), to.clone()));
        return moved;
    }
    came.extend(renamed_in);
    for from in &gone {
        let same = |list: &[String]| list.iter().filter(|p| name(p) == name(from)).count();
        let to: Vec<&String> = came
            .iter()
            .filter(|c| name(c) == name(from) && !moved.iter().any(|(_, t)| t == *c))
            .collect();
        if let ([to], 1, 1) = (to.as_slice(), same(&gone), same(&came)) {
            moved.push((from.clone(), (*to).clone()));
        }
    }
    moved
}

impl Pass<'_> {
    fn push(&mut self, path: &str, kind: ChangeKind, is_dir: bool) -> usize {
        self.out.changes.push(Change {
            path: path.to_owned(),
            kind,
            is_dir,
            own: false,
        });
        self.out.changes.len() - 1
    }

    /// Folders that were moved (the event named the old place, which is gone): one `Renamed` each, signatures follow.
    fn folder_renames(&mut self, items: &[Item]) -> Vec<(String, String)> {
        let mut moved = Vec::new();
        for item in items {
            let Some(from_abs) = &item.pending.rename_from else {
                continue;
            };
            if !item.meta.as_ref().is_some_and(Metadata::is_dir)
                || fs::symlink_metadata(from_abs).is_ok()
            {
                continue;
            }
            let Some(from) = self.env.vault.relativize(from_abs) else {
                continue;
            };
            let from = from.as_str().to_owned();
            if from.is_empty() || key_of(&from) == key_of(&item.path) {
                continue;
            }
            self.env.sigs.rename_under(&from, &item.path);
            self.push(&item.path, ChangeKind::Renamed { from: from.clone() }, true);
            moved.push((from, item.path.clone()));
        }
        moved
    }

    fn item(&mut self, item: &Item) {
        match &item.meta {
            Some(meta) if meta.is_dir() => self.folder(item),
            Some(meta) => self.file(&item.pending, &item.path, meta),
            None => self.absent(item),
        }
    }

    fn absent(&mut self, item: &Item) {
        let key = key_of(&item.path);
        let under = self.env.sigs.take_under(&key);
        if !under.is_empty() {
            self.push(&item.path, ChangeKind::Removed, true);
            for (shown, sig) in under {
                let at = self.push(&shown, ChangeKind::Removed, false);
                self.removed.push((at, sig));
            }
            return;
        }
        if let Some(sig) = self.env.sigs.remove(&key) {
            let at = self.push(&item.path, ChangeKind::Removed, false);
            self.removed.push((at, sig));
        } else if let Some(is_dir) = (self.env.known)(&item.path) {
            // it was there before the watch knew of it (macOS may also flag it as created: an item keeps the flags it
            // had a moment ago); a folder takes what was below it along
            if is_dir {
                self.push(&item.path, ChangeKind::Removed, true);
            } else {
                let at = self.push(&item.path, ChangeKind::Removed, false);
                let unknown = Sig {
                    len: 0,
                    mtime_ms: 0,
                    hash: 0,
                };
                self.removed.push((at, unknown));
            }
        } else if item.pending.removed && !item.pending.created {
            // existed before the watch remembered signatures: still a real removal
            let at = self.push(&item.path, ChangeKind::Removed, false);
            let unknown = Sig {
                len: 0,
                mtime_ms: 0,
                hash: 0,
            };
            self.removed.push((at, unknown));
        }
        // created and deleted inside one window (a temp file never seen settled): nothing to report
    }

    fn folder(&mut self, item: &Item) {
        // a folder's own time changing is not interesting; its children report themselves
        if !item.pending.created {
            return;
        }
        self.push(&item.path, ChangeKind::Created, true);
        self.expand(&item.pending.abs, 0);
    }

    /// Lists the files of a folder that appeared (created, moved in, checked out): they may report nothing themselves.
    fn expand(&mut self, dir: &Path, depth: usize) {
        if depth > MAX_EXPAND_DEPTH {
            self.out.truncated = true;
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let abs = entry.path();
            let Some(vp) = self.env.vault.relativize(&abs) else {
                continue;
            };
            let path = vp.as_str().to_owned();
            let Ok(meta) = fs::symlink_metadata(&abs) else {
                continue;
            };
            if self.env.skip(&path, meta.is_dir()) {
                continue;
            }
            if meta.is_dir() {
                self.expand(&abs, depth + 1);
            } else {
                if self.expanded >= MAX_EXPANDED {
                    self.out.truncated = true;
                    return;
                }
                self.expanded += 1;
                let child = Pending {
                    abs,
                    created: true,
                    removed: false,
                    modified: false,
                    rename_from: None,
                    after_removal: None,
                    deferred: false,
                };
                self.file(&child, &path, &meta);
            }
        }
    }

    fn file(&mut self, p: &Pending, path: &str, meta: &Metadata) {
        let key = key_of(path);
        let (len, mtime_ms) = stat_sig(meta);
        let prev = self.env.sigs.previous(&key, p.created, (len, mtime_ms));
        if prev.is_some_and(|s| s.len == len && s.mtime_ms == mtime_ms) {
            return; // untouched since last looked at
        }
        if len == 0 && prev.is_some_and(|s| s.len > 0) && !p.deferred {
            // truncate-then-write in progress: one more tick for the writer
            self.out.deferred.push(Pending {
                deferred: true,
                ..p.clone()
            });
            return;
        }
        // a new big file is read only once its content matters (see `NEW_HASH_LIMIT`)
        let unread = (p.created
            && prev.is_none()
            && meta.file_type().is_file()
            && len > NEW_HASH_LIMIT
            && len <= HASH_LIMIT
            && !(self.env.wanted)(path))
        .then(|| p.abs.clone());
        // links and special files are never opened (a named pipe would block the watch)
        let sig = if meta.file_type().is_file() && unread.is_none() {
            sig_retried(&p.abs, meta, LOCKED_RETRY)
        } else {
            Some(Sig {
                len,
                mtime_ms,
                hash: 0,
            })
        };
        if sig.is_none() && !p.deferred {
            // held by another program (or gone): looked at again on the next tick rather than waited for here
            self.out.deferred.push(Pending {
                deferred: true,
                ..p.clone()
            });
            return;
        }
        let Some(sig) = sig else {
            // locked or vanished: what it holds now is unknown, so the old hash stops counting
            let unknown = Sig {
                len,
                mtime_ms,
                hash: 0,
            };
            self.env.sigs.set(key, path, unknown);
            if prev.is_some() || !p.created {
                self.push(path, ChangeKind::Modified, false);
            } else {
                self.push(path, ChangeKind::Created, false);
            }
            return;
        };
        if prev
            .is_some_and(|s| s.hash != 0 && sig.hash != 0 && s.len == sig.len && s.hash == sig.hash)
        {
            self.env.sigs.set(key, path, sig); // same bytes, new time
            return;
        }
        self.env.sigs.set(key, path, sig);
        if prev.is_some() || !p.created {
            self.push(path, ChangeKind::Modified, false);
        } else {
            let from = p
                .rename_from
                .as_ref()
                .and_then(|abs| self.env.vault.relativize(abs))
                .map(|vp| vp.as_str().to_owned());
            let at = self.push(path, ChangeKind::Created, false);
            self.created.push(Created {
                at,
                sig,
                from,
                unread,
            });
        }
    }
}

/// A removal and a creation with the same content in one window is a rename (a move); a creation the event says came
/// from a removed path pairs with it whatever the content. Each removal pairs once, with the first creation that names
/// it | holds its content. `created` and `removed` (with their signatures) index `changes`. `read(abs, shown)` reads a
/// created file not read yet and remembers its signature: done when a removal of its size has a known content (it may
/// pair, or carry its content on after a rename).
pub(super) fn pair_renames(
    changes: &mut Vec<Change>,
    created: Vec<Created>,
    removed: &[(usize, Sig)],
    read: &mut dyn FnMut(&Path, &str) -> Option<Sig>,
) {
    let known = |sig: &Sig| sig.len > 0 && sig.hash != 0;
    let mut by_sig: HashMap<(u64, u64), usize> = HashMap::new();
    let mut by_key: HashMap<String, (usize, Sig)> = HashMap::new();
    let mut sizes: HashSet<u64> = HashSet::new();
    for (at, sig) in removed {
        if known(sig) {
            by_sig.entry((sig.len, sig.hash)).or_insert(*at);
            sizes.insert(sig.len);
        }
        by_key
            .entry(key_of(&changes[*at].path))
            .or_insert((*at, *sig));
    }
    let mut dropped: HashSet<usize> = HashSet::new();
    for Created {
        at,
        mut sig,
        from,
        unread,
    } in created
    {
        let direct = from.and_then(|from| by_key.get(&key_of(&from)).copied());
        if let Some(abs) = unread {
            let wanted = match direct {
                Some((_, removed)) => known(&removed) && removed.len == sig.len,
                None => sizes.contains(&sig.len),
            };
            if let Some(read) = wanted.then(|| read(&abs, &changes[at].path)).flatten() {
                sig = read;
            }
        }
        let direct = direct.map(|(removed_at, _)| removed_at);
        let partner = direct.or_else(|| {
            (sig.len > 0 && sig.hash != 0)
                .then(|| by_sig.get(&(sig.len, sig.hash)).copied())
                .flatten()
        });
        let Some(removed_at) = partner else {
            continue;
        };
        if dropped.contains(&removed_at) {
            continue;
        }
        let from = changes[removed_at].path.clone();
        if key_of(&from) == key_of(&changes[at].path) {
            continue;
        }
        changes[at].kind = ChangeKind::Renamed { from };
        dropped.insert(removed_at);
    }
    if !dropped.is_empty() {
        let mut at = 0;
        changes.retain(|_| {
            let paired = dropped.contains(&at);
            at += 1;
            !paired
        });
    }
}
