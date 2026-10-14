//! Turns "something happened to these paths" hints into real changes by looking at the disk.
//!
//! Events are hints and the disk is the truth: each pending path is stat'ed and compared with the
//! signature (size, mtime, xxh3 of the content) we remember. No-op changes (same bytes re-saved,
//! antivirus touches, OneDrive) are dropped, atomic saves become one `modified`, a delete plus a create
//! of identical content becomes a `renamed`, and a file truncated to 0 bytes gets one more tick to
//! receive its content before being reported.

use ignore::WalkBuilder;
use std::collections::HashMap;

use super::aggregate::PendingPath;
use super::filter::{PathFilter, Verdict};
use super::sigs::{hash_file, stat_sig, FileSig, SigStore};
use super::types::{ChangeKind, FsChange};
use crate::vault::{key, Vault};

/// A freshly created or moved-in folder is expanded into its files, up to this many.
const MAX_EXPANDED_FILES: usize = 500;
const MAX_EXPAND_DEPTH: usize = 12;

pub struct Env<'a> {
    pub vault: &'a Vault,
    pub filter: &'a PathFilter,
    pub sigs: &'a SigStore,
}

#[derive(Debug, Default)]
pub struct Reconciled {
    pub changes: Vec<FsChange>,
    /// Files or folders were created, removed or renamed: the tree must be refreshed.
    pub tree_changed: bool,
    /// Truncated files to look at again on the next tick.
    pub deferred: Vec<PendingPath>,
    /// A moved-in folder had more files than were listed: the caller must rescan to find the rest.
    pub truncated: bool,
}

struct Pass<'a> {
    env: &'a Env<'a>,
    now_ms: u64,
    out: Reconciled,
    /// Signature hashes of created/removed entries, for rename pairing: (index in `changes`, sig).
    created: Vec<(usize, FileSig)>,
    removed: Vec<(usize, FileSig)>,
}

/// Decides what really changed. `pending` is in arrival order; the result keeps that order.
pub fn reconcile(env: &Env<'_>, pending: Vec<PendingPath>, now_ms: u64) -> Reconciled {
    let mut pass = Pass {
        env,
        now_ms,
        out: Reconciled::default(),
        created: Vec::new(),
        removed: Vec::new(),
    };
    for p in pending {
        pass.path(&p);
    }
    pass.pair_renames();
    pass.out
}

impl Pass<'_> {
    fn push(&mut self, kind: ChangeKind, rel: &str, from: Option<String>) -> usize {
        self.out.changes.push(FsChange {
            kind,
            path: rel.to_owned(),
            from,
            at_ms: self.now_ms,
        });
        self.out.changes.len() - 1
    }

    fn path(&mut self, p: &PendingPath) {
        match std::fs::symlink_metadata(&p.abs) {
            Ok(meta) if meta.is_dir() => self.folder(p),
            Ok(meta) => self.file(p, &meta),
            Err(_) => self.absent(p),
        }
    }

    fn absent(&mut self, p: &PendingPath) {
        let k = key(&p.rel);
        let under = self.env.sigs.take_under(&k);
        if !under.is_empty() {
            self.out.tree_changed = true;
            for (display, sig) in under {
                let idx = self.push(ChangeKind::Removed, &display, None);
                self.removed.push((idx, sig));
            }
            return;
        }
        if let Some(sig) = self.env.sigs.remove(&k) {
            let idx = self.push(ChangeKind::Removed, &p.rel, None);
            self.removed.push((idx, sig));
            self.out.tree_changed = true;
        } else if p.removed && !p.created {
            // Existed before we started remembering signatures: still a real removal.
            let idx = self.push(ChangeKind::Removed, &p.rel, None);
            // No signature to pair by content, but a rename event can still name this path.
            let unknown = FileSig {
                len: 0,
                mtime_ms: 0,
                hash: 0,
            };
            self.removed.push((idx, unknown));
            self.out.tree_changed = true;
        }
        // Created and deleted inside one window (a temp file we never saw settle): nothing to report.
    }

    fn folder(&mut self, p: &PendingPath) {
        if !(p.created || p.removed) {
            return; // a folder's mtime changing is not interesting; its children report themselves
        }
        self.out.tree_changed = true;
        if !p.created {
            return;
        }
        // A folder that appeared (created, or moved in by a rename/checkout): its files may not
        // produce events of their own, so list them.
        let walker = WalkBuilder::new(&p.abs)
            .follow_links(false)
            .hidden(false)
            .require_git(false)
            .max_depth(Some(MAX_EXPAND_DEPTH))
            .build();
        let mut files = 0;
        for entry in walker.filter_map(Result::ok) {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let Some(loc) = self.env.vault.locate(entry.path()) else {
                continue;
            };
            if self.env.filter.classify(loc.root, &loc.inner, false) != Verdict::Emit {
                continue;
            }
            files += 1;
            if files > MAX_EXPANDED_FILES {
                self.out.truncated = true;
                break;
            }
            let child = PendingPath {
                abs: entry.path().to_path_buf(),
                rel: loc.rel,
                created: true,
                removed: false,
                modified: false,
                is_dir: false,
                rename_from: None,
                deferred: false,
            };
            if let Ok(meta) = std::fs::symlink_metadata(&child.abs) {
                self.file(&child, &meta);
            }
        }
    }

    fn file(&mut self, p: &PendingPath, meta: &std::fs::Metadata) {
        let k = key(&p.rel);
        let prev = self.env.sigs.get(&k);
        let (len, mtime_ms) = stat_sig(meta);

        if prev.is_some_and(|s| s.len == len && s.mtime_ms == mtime_ms) {
            return; // untouched since we last looked
        }
        if len == 0 && prev.is_some_and(|s| s.len > 0) && !p.deferred {
            // Truncate-then-write in progress: give the writer one more tick.
            self.out.deferred.push(p.clone());
            return;
        }
        let Ok(sig) = hash_file(&p.abs, meta) else {
            // Still locked or vanished: report what we know without remembering it.
            if prev.is_some() || !p.created {
                self.push(ChangeKind::Modified, &p.rel, None);
            } else {
                self.push(ChangeKind::Created, &p.rel, None);
                self.out.tree_changed = true;
            }
            return;
        };
        if prev.is_some_and(|s| s.len == sig.len && s.hash == sig.hash) {
            self.env.sigs.set(k, &p.rel, sig); // same bytes, new mtime
            return;
        }
        self.env.sigs.set(k, &p.rel, sig);
        if prev.is_some() || !p.created {
            self.push(ChangeKind::Modified, &p.rel, None);
        } else {
            let idx = self.push(ChangeKind::Created, &p.rel, p.rename_from.clone());
            self.created.push((idx, sig));
            self.out.tree_changed = true;
        }
    }

    /// A removal and a creation with the same content in one window is a rename (a move).
    fn pair_renames(&mut self) {
        let mut by_sig: HashMap<(u64, u64), usize> = HashMap::new();
        for (idx, sig) in &self.removed {
            by_sig.entry((sig.len, sig.hash)).or_insert(*idx);
        }
        let mut drop_idx: Vec<usize> = Vec::new();
        for (cidx, sig) in self.created.clone() {
            let direct = self.out.changes[cidx].from.clone().and_then(|from| {
                self.removed
                    .iter()
                    .find(|(ridx, _)| key(&self.out.changes[*ridx].path) == key(&from))
                    .map(|(ridx, _)| *ridx)
            });
            let partner = direct.or_else(|| {
                (sig.len > 0)
                    .then(|| by_sig.get(&(sig.len, sig.hash)).copied())
                    .flatten()
            });
            if let Some(ridx) = partner {
                if drop_idx.contains(&ridx) {
                    continue;
                }
                let from = self.out.changes[ridx].path.clone();
                if key(&from) == key(&self.out.changes[cidx].path) {
                    continue;
                }
                let c = &mut self.out.changes[cidx];
                c.kind = ChangeKind::Renamed;
                c.from = Some(from);
                drop_idx.push(ridx);
            } else {
                self.out.changes[cidx].from = None;
            }
        }
        drop_idx.sort_unstable();
        for idx in drop_idx.into_iter().rev() {
            self.out.changes.remove(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::aggregate::{Aggregator, Hint, RawEvent, MAX_WAIT, QUIET};
    use std::fs;
    use std::time::Instant;

    struct Fx {
        _dir: tempfile::TempDir,
        vault: Vault,
        filter: PathFilter,
        sigs: SigStore,
    }

    impl Fx {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let vault = Vault::open(dir.path()).expect("open");
            let filter = PathFilter::new(&[(vault.root().to_path_buf(), None)], &[]);
            Self {
                _dir: dir,
                vault,
                filter,
                sigs: SigStore::new(),
            }
        }

        fn path(&self, rel: &str) -> std::path::PathBuf {
            self.vault.root().join(rel)
        }

        fn write(&self, rel: &str, content: &str) {
            let p = self.path(rel);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(p, content).expect("write");
        }

        /// Feeds hints through a real aggregator, then reconciles against the disk.
        fn run(&self, hints: &[(&str, Hint)]) -> Reconciled {
            let mut agg = Aggregator::new(QUIET, MAX_WAIT);
            let now = Instant::now();
            for (rel, hint) in hints {
                agg.push(
                    RawEvent {
                        abs: self.path(rel),
                        hint: *hint,
                        is_dir: None,
                        rename_from: None,
                    },
                    &self.vault,
                    &self.filter,
                    now,
                );
            }
            let env = Env {
                vault: &self.vault,
                filter: &self.filter,
                sigs: &self.sigs,
            };
            reconcile(&env, agg.take().paths, 1234)
        }
    }

    fn kinds(r: &Reconciled) -> Vec<(ChangeKind, &str)> {
        r.changes
            .iter()
            .map(|c| (c.kind, c.path.as_str()))
            .collect()
    }

    #[test]
    fn new_file_is_created_then_unchanged_rewrites_are_dropped() {
        let f = Fx::new();
        f.write("a.md", "one");
        let r = f.run(&[("a.md", Hint::Create), ("a.md", Hint::Modify)]);
        assert_eq!(kinds(&r), [(ChangeKind::Created, "a.md")]);
        assert!(r.tree_changed);
        assert_eq!(r.changes[0].at_ms, 1234);

        // Same bytes written again (new mtime): nothing to report.
        std::thread::sleep(std::time::Duration::from_millis(20));
        f.write("a.md", "one");
        let r = f.run(&[("a.md", Hint::Modify)]);
        assert!(r.changes.is_empty(), "{:?}", r.changes);
        assert!(!r.tree_changed);
    }

    #[test]
    fn real_edits_are_modified() {
        let f = Fx::new();
        f.write("a.md", "one");
        f.run(&[("a.md", Hint::Create)]);
        f.write("a.md", "two!");
        let r = f.run(&[("a.md", Hint::Modify)]);
        assert_eq!(kinds(&r), [(ChangeKind::Modified, "a.md")]);
        assert!(!r.tree_changed);
    }

    #[test]
    fn atomic_save_remove_then_create_is_one_modified() {
        let f = Fx::new();
        f.write("a.md", "one");
        f.run(&[("a.md", Hint::Create)]);
        f.write("a.md", "two!");
        let r = f.run(&[("a.md", Hint::Remove), ("a.md", Hint::Create)]);
        assert_eq!(kinds(&r), [(ChangeKind::Modified, "a.md")]);
    }

    #[test]
    fn removal_of_a_known_file_is_removed_and_forgotten() {
        let f = Fx::new();
        f.write("a.md", "one");
        f.run(&[("a.md", Hint::Create)]);
        fs::remove_file(f.path("a.md")).expect("rm");
        let r = f.run(&[("a.md", Hint::Remove)]);
        assert_eq!(kinds(&r), [(ChangeKind::Removed, "a.md")]);
        assert!(r.tree_changed);
        assert!(f.sigs.is_empty());
    }

    #[test]
    fn removal_of_a_file_we_never_saw_is_still_reported() {
        let f = Fx::new();
        let r = f.run(&[("ghost.md", Hint::Remove)]);
        assert_eq!(kinds(&r), [(ChangeKind::Removed, "ghost.md")]);
    }

    #[test]
    fn created_and_deleted_in_one_window_is_nothing() {
        let f = Fx::new();
        let r = f.run(&[("scratch.md", Hint::Create), ("scratch.md", Hint::Remove)]);
        assert!(r.changes.is_empty());
        assert!(!r.tree_changed);
    }

    #[test]
    fn moving_a_file_is_a_rename() {
        let f = Fx::new();
        f.write("old.md", "same content");
        f.run(&[("old.md", Hint::Create)]);
        fs::create_dir_all(f.path("dir")).expect("mkdir");
        fs::rename(f.path("old.md"), f.path("dir/new.md")).expect("rename");
        let r = f.run(&[("old.md", Hint::Remove), ("dir/new.md", Hint::Create)]);
        assert_eq!(kinds(&r), [(ChangeKind::Renamed, "dir/new.md")]);
        assert_eq!(r.changes[0].from.as_deref(), Some("old.md"));
        assert!(r.tree_changed);
    }

    #[test]
    fn same_content_in_unrelated_files_is_not_paired_when_not_removed() {
        let f = Fx::new();
        f.write("a.md", "dup");
        f.run(&[("a.md", Hint::Create)]);
        f.write("b.md", "dup");
        let r = f.run(&[("b.md", Hint::Create)]);
        assert_eq!(kinds(&r), [(ChangeKind::Created, "b.md")]);
    }

    #[test]
    fn removing_a_folder_removes_every_known_file_below_it() {
        let f = Fx::new();
        f.write("d/a.md", "1");
        f.write("d/s/b.md", "2");
        f.write("keep.md", "3");
        f.run(&[
            ("d/a.md", Hint::Create),
            ("d/s/b.md", Hint::Create),
            ("keep.md", Hint::Create),
        ]);
        fs::remove_dir_all(f.path("d")).expect("rm");
        let r = f.run(&[("d", Hint::Remove)]);
        let mut k: Vec<&str> = r.changes.iter().map(|c| c.path.as_str()).collect();
        k.sort_unstable();
        assert_eq!(k, ["d/a.md", "d/s/b.md"]);
        assert!(r.changes.iter().all(|c| c.kind == ChangeKind::Removed));
        assert!(r.tree_changed);
    }

    #[test]
    fn a_folder_that_appears_is_expanded_into_created_files() {
        let f = Fx::new();
        f.write("moved/a.md", "1");
        f.write("moved/deep/b.md", "2");
        f.write("moved/.a.md.swp", "junk");
        f.write("moved/node_modules/x.js", "junk");
        let r = f.run(&[("moved", Hint::Create)]);
        let mut k: Vec<&str> = r.changes.iter().map(|c| c.path.as_str()).collect();
        k.sort_unstable();
        assert_eq!(k, ["moved/a.md", "moved/deep/b.md"]);
        assert!(r.tree_changed);
    }

    #[test]
    fn a_folder_with_more_files_than_the_expansion_cap_asks_for_a_rescan() {
        let f = Fx::new();
        for i in 0..=MAX_EXPANDED_FILES {
            f.write(&format!("big/n{i}.md"), "x");
        }
        let r = f.run(&[("big", Hint::Create)]);
        assert_eq!(r.changes.len(), MAX_EXPANDED_FILES);
        assert!(r.truncated, "the files past the cap are unknown");

        let small = Fx::new();
        small.write("small/a.md", "x");
        let r = small.run(&[("small", Hint::Create)]);
        assert!(!r.truncated);
    }

    #[test]
    fn folder_mtime_noise_is_ignored() {
        let f = Fx::new();
        f.write("d/a.md", "1");
        let r = f.run(&[("d", Hint::Modify)]);
        assert!(r.changes.is_empty());
        assert!(!r.tree_changed);
    }

    #[test]
    fn truncated_file_gets_one_more_tick() {
        let f = Fx::new();
        f.write("a.md", "content");
        f.run(&[("a.md", Hint::Create)]);
        f.write("a.md", "");
        let first = f.run(&[("a.md", Hint::Modify)]);
        assert!(first.changes.is_empty());
        assert_eq!(first.deferred.len(), 1);

        // Still empty on the second look: report it.
        let env = Env {
            vault: &f.vault,
            filter: &f.filter,
            sigs: &f.sigs,
        };
        let mut again = first.deferred;
        again[0].deferred = true;
        let second = reconcile(&env, again, 5);
        assert_eq!(kinds(&second), [(ChangeKind::Modified, "a.md")]);
    }

    #[test]
    fn truncated_then_rewritten_reports_the_final_content_once() {
        let f = Fx::new();
        f.write("a.md", "content");
        f.run(&[("a.md", Hint::Create)]);
        f.write("a.md", "");
        let first = f.run(&[("a.md", Hint::Modify)]);
        assert_eq!(first.deferred.len(), 1);
        f.write("a.md", "new content");
        let env = Env {
            vault: &f.vault,
            filter: &f.filter,
            sigs: &f.sigs,
        };
        let second = reconcile(&env, first.deferred, 5);
        assert_eq!(kinds(&second), [(ChangeKind::Modified, "a.md")]);
    }

    #[test]
    fn first_sight_of_an_existing_file_without_a_create_hint_is_modified() {
        let f = Fx::new();
        f.write("pre-existing.md", "x");
        let r = f.run(&[("pre-existing.md", Hint::Modify)]);
        assert_eq!(kinds(&r), [(ChangeKind::Modified, "pre-existing.md")]);
        assert!(!r.tree_changed);
    }
}
