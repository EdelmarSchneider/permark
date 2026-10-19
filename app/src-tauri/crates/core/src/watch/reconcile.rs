//! Turns "something happened to these paths" hints into real changes by looking at the disk. Events are hints, the
//! disk is the truth: each hinted path is stat'ed and compared with the signature remembered for it. No-op changes
//! (same bytes saved again, antivirus touches, sync clients) are dropped, atomic saves become one `Modified`, a
//! removal plus a creation of the same content becomes a `Renamed`, a folder moved gets one `Renamed` for the folder,
//! a folder that appeared is expanded into its files, a file truncated to nothing gets one more tick to receive its
//! content.

use std::collections::HashMap;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::noise::is_news;
use super::sigs::{sig_of, stat_sig, Sig, SigStore};
use super::types::{Change, ChangeKind};
use crate::paths::Vault;
use crate::tree::{key_of, Ignorer};

/// A created folder is expanded into its files up to this many.
const MAX_EXPANDED: usize = 500;
const MAX_EXPAND_DEPTH: usize = 12;

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
    /// Looked at once already (a file truncated to nothing).
    pub deferred: bool,
}

impl Pending {
    pub(crate) fn merge(&mut self, other: &Pending) {
        self.created |= other.created;
        self.removed |= other.removed;
        self.modified |= other.modified;
        if other.rename_from.is_some() {
            self.rename_from.clone_from(&other.rename_from);
        }
    }
}

pub(crate) struct Env<'a> {
    pub vault: &'a Vault,
    pub sigs: &'a SigStore,
    /// Ignore rules per root: (folded root prefix, rules); the first is the project folder.
    pub ignorers: &'a [(String, Arc<Ignorer>)],
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

    /// Whether a change at this project path is dropped before the disk is looked at.
    fn skip(&self, path: &str, is_dir: bool) -> bool {
        if !is_news(path) {
            return true;
        }
        let rel = crate::tree::below_root(path);
        // git signals are not files of the project tree
        if rel.starts_with(".git/") || rel.contains("/.git/") {
            return false;
        }
        self.ignorer_for(&key_of(path))
            .is_some_and(|ignorer| ignorer.rel_ignored(rel, is_dir))
    }
}

#[derive(Debug, Default)]
pub(crate) struct Reconciled {
    pub changes: Vec<Change>,
    /// Truncated files to look at again on the next tick.
    pub deferred: Vec<Pending>,
    /// A folder had more files than were listed.
    pub truncated: bool,
}

struct Item {
    pending: Pending,
    path: String,
    meta: Option<Metadata>,
}

struct Pass<'a> {
    env: &'a Env<'a>,
    out: Reconciled,
    /// (index in `changes`, signature, rename hint) of created files.
    created: Vec<(usize, Sig, Option<String>)>,
    /// (index in `changes`, signature) of removed files.
    removed: Vec<(usize, Sig)>,
    expanded: usize,
}

/// Decides what really changed. `pending` is in arrival order; the result keeps that order.
pub(crate) fn reconcile(env: &Env<'_>, pending: Vec<Pending>) -> Reconciled {
    let mut items: Vec<Item> = Vec::new();
    for p in pending {
        let Some(vp) = env.vault.relativize(&p.abs) else {
            continue;
        };
        let path = vp.as_str().to_owned();
        if path.is_empty() {
            continue;
        }
        let meta = fs::symlink_metadata(&p.abs).ok();
        let is_dir = meta.as_ref().is_some_and(Metadata::is_dir);
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
        out: Reconciled::default(),
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
    pass.pair_renames();
    pass.out
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
                    deferred: false,
                };
                self.file(&child, &path, &meta);
            }
        }
    }

    fn file(&mut self, p: &Pending, path: &str, meta: &Metadata) {
        let key = key_of(path);
        let prev = self.env.sigs.get(&key);
        let (len, mtime_ms) = stat_sig(meta);
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
        // links and special files are never opened (a named pipe would block the watch)
        let sig = if meta.file_type().is_file() {
            sig_of(&p.abs, meta)
        } else {
            Some(Sig {
                len,
                mtime_ms,
                hash: 0,
            })
        };
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
            self.created.push((at, sig, from));
        }
    }

    /// A removal and a creation with the same content in one window is a rename (a move).
    fn pair_renames(&mut self) {
        let mut by_sig: HashMap<(u64, u64), usize> = HashMap::new();
        for (at, sig) in &self.removed {
            if sig.len > 0 && sig.hash != 0 {
                by_sig.entry((sig.len, sig.hash)).or_insert(*at);
            }
        }
        let mut dropped: Vec<usize> = Vec::new();
        for (created_at, sig, hinted_from) in std::mem::take(&mut self.created) {
            let direct = hinted_from.and_then(|from| {
                self.removed
                    .iter()
                    .find(|(at, _)| key_of(&self.out.changes[*at].path) == key_of(&from))
                    .map(|(at, _)| *at)
            });
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
            let from = self.out.changes[removed_at].path.clone();
            if key_of(&from) == key_of(&self.out.changes[created_at].path) {
                continue;
            }
            let change = &mut self.out.changes[created_at];
            change.kind = ChangeKind::Renamed { from };
            dropped.push(removed_at);
        }
        dropped.sort_unstable();
        for at in dropped.into_iter().rev() {
            self.out.changes.remove(at);
        }
    }
}
