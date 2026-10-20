//! The rules of the unread markers, with no files, threads or events in here (so every rule is tested directly).
//!
//! For every tracked document (Markdown): the hash of what it holds on disk now (`current`) and of what it held when
//! it was last read (`read`). Unread = they differ, or it was never read. Paths are project paths (`@wt/<name>/...`
//! in a linked worktree), compared the way the file system compares them (folded).

use std::collections::{BTreeMap, HashSet};

use permark_core::paths::VaultPath;

/// The folded key of a path.
#[must_use]
pub fn key_of(path: &str) -> String {
    VaultPath::parse(path).map_or_else(|_| path.to_lowercase(), |p| p.key())
}

/// Only Markdown documents are tracked.
#[must_use]
pub fn is_tracked(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && matches!(
                ext.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd" | "mdx" | "mdc"
            )
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    /// The path as shown.
    pub path: String,
    /// What the disk holds now (`None`: not looked at yet).
    pub current: Option<u64>,
    /// What was held when last read (`None`: never read).
    pub read: Option<u64>,
}

/// What changed while the disk was being walked: the walk saw these as they were when it passed, so it must not
/// undo them.
#[derive(Debug, Default)]
struct ScanLog {
    touched: HashSet<String>,
    gone: HashSet<String>,
}

#[derive(Debug)]
pub struct Tracker {
    docs: BTreeMap<String, Doc>,
    /// The project had no read state: the first look at the disk becomes the baseline (everything there is read).
    baseline_pending: bool,
    looked: bool,
    scan: Option<ScanLog>,
    revision: u64,
}

fn under(folder_key: &str, key: &str) -> bool {
    folder_key.is_empty()
        || key == folder_key
        || key
            .strip_prefix(folder_key)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The linked worktree a key is in (`@wt/<name>`), if any.
fn worktree_of(key: &str) -> Option<&str> {
    let rest = key.strip_prefix("@wt/")?;
    let end = rest.find('/').unwrap_or(rest.len());
    Some(&key[..4 + end])
}

impl Tracker {
    /// `saved`: what `read.json` held (`None`: the project has no read state yet).
    #[must_use]
    pub fn new(saved: Option<BTreeMap<String, u64>>) -> Self {
        let mut tracker = Self {
            docs: BTreeMap::new(),
            baseline_pending: saved.is_none(),
            looked: false,
            scan: None,
            revision: 0,
        };
        for (path, hash) in saved.into_iter().flatten() {
            if is_tracked(&path) {
                tracker.docs.insert(
                    key_of(&path),
                    Doc {
                        path,
                        current: None,
                        read: Some(hash),
                    },
                );
            }
        }
        tracker
    }

    /// The first look at the disk has not happened yet.
    #[must_use]
    pub fn baseline_pending(&self) -> bool {
        self.baseline_pending
    }

    /// The disk has been looked at: from now on the documents' content is known.
    #[must_use]
    pub fn looked(&self) -> bool {
        self.looked
    }

    /// Goes up whenever something about a document changed.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn touch(&mut self, key: &str) {
        if let Some(log) = &mut self.scan {
            log.touched.insert(key.to_owned());
            log.gone.remove(key);
        }
    }

    fn forget(&mut self, key: &str) {
        if let Some(log) = &mut self.scan {
            log.gone.insert(key.to_owned());
            log.touched.remove(key);
        }
    }

    /// The disk is about to be walked.
    pub fn begin_scan(&mut self) {
        self.scan = Some(ScanLog::default());
    }

    /// The walk was stopped: it tells nothing.
    pub fn abort_scan(&mut self) {
        self.scan = None;
    }

    /// The documents of the disk with their hashes (the whole walk). Returns whether `read.json` must be written.
    /// With no read state yet everything found counts as read, and so do the documents of a linked worktree the
    /// project did not have before (agents make worktrees all the time: it would be a flood of dots). A document not
    /// in the list is gone, unless the app touched it since the walk began.
    pub fn scanned(&mut self, files: Vec<(String, u64)>) -> bool {
        let log = self.scan.take().unwrap_or_default();
        let known_worktrees: HashSet<String> = self
            .docs
            .keys()
            .filter_map(|k| worktree_of(k).map(str::to_owned))
            .collect();
        let had_state = !self.baseline_pending;
        let mut seen: HashSet<String> = HashSet::with_capacity(files.len());
        let mut wrote = self.baseline_pending;
        for (path, hash) in files {
            let key = key_of(&path);
            seen.insert(key.clone());
            if log.touched.contains(&key) || log.gone.contains(&key) || !is_tracked(&path) {
                continue;
            }
            let new_worktree = had_state
                && worktree_of(&key).is_some_and(|w| !known_worktrees.contains(w))
                && !self.docs.contains_key(&key);
            let doc = self.docs.entry(key).or_insert_with(|| Doc {
                path: path.clone(),
                current: None,
                read: None,
            });
            path.clone_into(&mut doc.path);
            doc.current = Some(hash);
            if self.baseline_pending || new_worktree {
                doc.read = Some(hash);
                wrote = true;
            }
        }
        // documents the walk did not find are gone (except what the app touched meanwhile)
        let before = self.docs.len();
        self.docs
            .retain(|key, _| seen.contains(key) || log.touched.contains(key));
        wrote |= self.docs.len() != before;
        self.baseline_pending = false;
        self.looked = true;
        self.revision += 1;
        wrote
    }

    /// A document was created or its content changed: this is what it holds now. `own`: the app's doing (a save, a
    /// link update, a new document). Returns whether `read.json` must be written.
    pub fn changed(&mut self, path: &str, hash: u64, own: bool) -> bool {
        if self.baseline_pending || !is_tracked(path) {
            return false;
        }
        let key = key_of(path);
        self.touch(&key);
        let doc = self.docs.entry(key).or_insert_with(|| Doc {
            path: path.to_owned(),
            current: None,
            read: None,
        });
        let fresh = doc.current.is_none() && doc.read.is_none();
        let up_to_date = doc.current.is_some() && doc.read == doc.current;
        let same = doc.current == Some(hash) && doc.path == path;
        path.clone_into(&mut doc.path);
        doc.current = Some(hash);
        let mut wrote = false;
        // the person's own new document, or their save of one they had read, stays read
        if own && (fresh || up_to_date) && doc.read != Some(hash) {
            doc.read = Some(hash);
            wrote = true;
        }
        if !same {
            self.revision += 1;
        }
        wrote
    }

    /// A document, or a folder with documents in it, is gone. Returns whether `read.json` must be written.
    pub fn removed(&mut self, path: &str) -> bool {
        self.removed_docs(path).1
    }

    /// Like [`Tracker::removed`], and hands back the documents that went (a creation with the same content in the
    /// same batch is a move the watch did not pair).
    pub fn removed_docs(&mut self, path: &str) -> (Vec<Doc>, bool) {
        if self.baseline_pending {
            return (Vec::new(), false);
        }
        let key = key_of(path);
        let keys: Vec<String> = self
            .docs
            .keys()
            .filter(|k| under(&key, k))
            .cloned()
            .collect();
        let mut gone = Vec::new();
        let mut wrote = false;
        for k in &keys {
            if let Some(doc) = self.docs.remove(k) {
                wrote |= doc.read.is_some();
                gone.push(doc);
            }
            self.forget(k);
        }
        if !keys.is_empty() {
            self.revision += 1;
        }
        (gone, wrote)
    }

    /// A document appeared at `path` that is one that went elsewhere (same content): what was read of it carries
    /// over. Returns whether `read.json` must be written.
    pub fn adopt(&mut self, path: &str, hash: u64, read: Option<u64>) -> bool {
        if self.baseline_pending || !is_tracked(path) {
            return false;
        }
        let key = key_of(path);
        self.touch(&key);
        self.docs.insert(
            key,
            Doc {
                path: path.to_owned(),
                current: Some(hash),
                read,
            },
        );
        self.revision += 1;
        read.is_some()
    }

    /// A document, or a folder with documents in it, moved: what was read stays read. Returns the moves `(from, to)`
    /// of the documents and whether `read.json` must be written.
    pub fn renamed(&mut self, from: &str, to: &str) -> (Vec<(String, String)>, bool) {
        if self.baseline_pending {
            return (Vec::new(), false);
        }
        let from_key = key_of(from);
        let keys: Vec<String> = self
            .docs
            .keys()
            .filter(|k| under(&from_key, k))
            .cloned()
            .collect();
        let mut moves = Vec::new();
        let mut wrote = false;
        for k in keys {
            let Some(mut doc) = self.docs.remove(&k) else {
                continue;
            };
            self.forget(&k);
            // the part of the path below the moved item (starts with `/`)
            let tail = if doc.path.len() >= from.len() && doc.path.is_char_boundary(from.len()) {
                doc.path[from.len()..].to_owned()
            } else {
                k[from_key.len().min(k.len())..].to_owned()
            };
            let new_path = if k == from_key {
                to.to_owned()
            } else {
                format!("{}{}", to.trim_end_matches('/'), tail)
            };
            if !is_tracked(&new_path) {
                // renamed to something that is not a document any more: it is gone as a document
                wrote |= doc.read.is_some();
                continue;
            }
            moves.push((doc.path.clone(), new_path.clone()));
            doc.path.clone_from(&new_path);
            let new_key = key_of(&new_path);
            self.touch(&new_key);
            wrote |= doc.read.is_some();
            self.docs.insert(new_key, doc);
        }
        if !moves.is_empty() {
            self.revision += 1;
        }
        (moves, wrote)
    }

    /// The person read the document as it was when it held `hash`. Returns `(before, wrote)`: what was read before.
    pub fn mark_read(&mut self, path: &str, hash: u64) -> (Option<u64>, bool) {
        if self.baseline_pending || !is_tracked(path) {
            return (None, false);
        }
        let key = key_of(path);
        self.touch(&key);
        let doc = self.docs.entry(key).or_insert_with(|| Doc {
            path: path.to_owned(),
            current: None,
            read: None,
        });
        let before = doc.read;
        path.clone_into(&mut doc.path);
        let wrote = doc.read.replace(hash) != Some(hash);
        if wrote {
            self.revision += 1;
        }
        (before, wrote)
    }

    /// The person wants the document to count as unread again. Returns whether `read.json` must be written.
    pub fn mark_unread(&mut self, path: &str) -> bool {
        if self.baseline_pending || !is_tracked(path) {
            return false;
        }
        let key = key_of(path);
        self.touch(&key);
        let Some(doc) = self.docs.get_mut(&key) else {
            return false;
        };
        let wrote = doc.read.take().is_some();
        if wrote {
            self.revision += 1;
        }
        wrote
    }

    /// Everything in a folder (`""`: everything) counts as read as it stands. Returns the paths that changed and
    /// whether `read.json` must be written.
    pub fn mark_folder_read(&mut self, folder: &str) -> (Vec<String>, bool) {
        if self.baseline_pending {
            return (Vec::new(), false);
        }
        let folder_key = key_of(folder.trim_end_matches('/'));
        let folder_key = if folder.is_empty() {
            String::new()
        } else {
            folder_key
        };
        let mut paths = Vec::new();
        let keys: Vec<String> = self
            .docs
            .keys()
            .filter(|k| under(&folder_key, k))
            .cloned()
            .collect();
        for key in keys {
            self.touch(&key);
            if let Some(doc) = self.docs.get_mut(&key) {
                if let Some(current) = doc.current {
                    if doc.read != Some(current) {
                        doc.read = Some(current);
                        paths.push(doc.path.clone());
                    }
                }
            }
        }
        if !paths.is_empty() {
            self.revision += 1;
        }
        let wrote = !paths.is_empty();
        (paths, wrote)
    }

    /// The documents that were never read or changed since: nothing is unread until the disk has been looked at.
    #[must_use]
    pub fn unread(&self) -> Vec<String> {
        if self.baseline_pending || !self.looked {
            return Vec::new();
        }
        self.docs
            .values()
            .filter(|d| d.current.is_some() && d.read != d.current)
            .map(|d| d.path.clone())
            .collect()
    }

    /// What `read.json` holds: the hash of every document that was read.
    #[must_use]
    pub fn saved(&self) -> BTreeMap<String, u64> {
        self.docs
            .values()
            .filter_map(|d| Some((d.path.clone(), d.read?)))
            .collect()
    }

    /// Every document with the hash of what it holds now (the picture `visit.json` keeps).
    #[must_use]
    pub fn current(&self) -> BTreeMap<String, u64> {
        self.docs
            .values()
            .filter_map(|d| Some((d.path.clone(), d.current?)))
            .collect()
    }

    /// The hash of what was read of a document.
    #[must_use]
    pub fn read_of(&self, path: &str) -> Option<u64> {
        self.docs.get(&key_of(path)).and_then(|d| d.read)
    }

    /// The hash a document holds now.
    #[must_use]
    pub fn current_of(&self, path: &str) -> Option<u64> {
        self.docs.get(&key_of(path)).and_then(|d| d.current)
    }

    /// Every hash that stands for a version somebody read.
    #[must_use]
    pub fn read_hashes(&self) -> HashSet<u64> {
        self.docs.values().filter_map(|d| d.read).collect()
    }

    /// Paths of the documents tracked.
    #[must_use]
    pub fn paths(&self) -> Vec<String> {
        self.docs.values().map(|d| d.path.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::unnecessary_wraps)] // the shape `Tracker::new` takes: Some(state)
    fn saved(entries: &[(&str, u64)]) -> Option<BTreeMap<String, u64>> {
        Some(entries.iter().map(|(p, h)| ((*p).to_owned(), *h)).collect())
    }

    fn scanned(t: &mut Tracker, files: &[(&str, u64)]) -> bool {
        t.begin_scan();
        t.scanned(files.iter().map(|(p, h)| ((*p).to_owned(), *h)).collect())
    }

    #[test]
    fn what_is_tracked_is_markdown() {
        for yes in ["a.md", "d/e.MD", "x.markdown", "r.mdc", "@wt/w/a.mdx"] {
            assert!(is_tracked(yes), "{yes}");
        }
        for no in ["a.txt", "md", ".md", "a.md.bak", "dir/", "a.json"] {
            assert!(!is_tracked(no), "{no}");
        }
    }

    #[test]
    fn the_first_look_counts_everything_as_read_and_nothing_is_unread_before_it() {
        let mut t = Tracker::new(None);
        assert!(t.baseline_pending());
        assert!(t.unread().is_empty());
        // changes before the baseline are not news either
        assert!(!t.changed("a.md", 1, false));
        assert!(scanned(
            &mut t,
            &[("a.md", 1), ("b/c.md", 2), ("notes.txt", 3)]
        ));
        assert!(!t.baseline_pending() && t.looked());
        assert!(t.unread().is_empty());
        assert_eq!(t.saved().len(), 2, "text files are not tracked");
        // later documents are unread, changed ones too
        t.changed("new.md", 9, false);
        t.changed("a.md", 5, false);
        assert_eq!(t.unread(), ["a.md", "new.md"]);
    }

    #[test]
    fn a_known_state_marks_what_changed_while_the_app_was_closed() {
        let mut t = Tracker::new(saved(&[("a.md", 1), ("b.md", 2), ("gone.md", 3)]));
        assert!(!t.baseline_pending());
        assert!(
            t.unread().is_empty(),
            "nothing is known before the disk is looked at"
        );
        assert!(scanned(&mut t, &[("a.md", 1), ("b.md", 20), ("c.md", 7)]));
        assert_eq!(t.unread(), ["b.md", "c.md"]);
        assert_eq!(
            t.saved().keys().map(String::as_str).collect::<Vec<_>>(),
            ["a.md", "b.md"]
        );
    }

    #[test]
    fn a_change_put_back_is_read_again_and_an_unchanged_one_is_no_news() {
        let mut t = Tracker::new(saved(&[("a.md", 1)]));
        scanned(&mut t, &[("a.md", 1)]);
        let before = t.revision();
        t.changed("a.md", 1, false);
        assert_eq!(t.revision(), before, "same content, nothing to tell");
        t.changed("a.md", 2, false);
        assert_eq!(t.unread(), ["a.md"]);
        t.changed("a.md", 1, false);
        assert!(t.unread().is_empty());
    }

    #[test]
    fn own_actions_the_persons_new_document_and_the_save_of_a_read_one_stay_read() {
        let mut t = Tracker::new(saved(&[("read.md", 1), ("unread.md", 2)]));
        scanned(&mut t, &[("read.md", 1), ("unread.md", 20)]);
        assert!(
            t.changed("made.md", 5, true),
            "a new document of the person's"
        );
        assert!(
            t.changed("read.md", 6, true),
            "the person's save of a read document"
        );
        assert!(
            !t.changed("unread.md", 21, true),
            "an unread one stays unread"
        );
        assert_eq!(t.unread(), ["unread.md"]);
        // someone else's change of a read document is news
        t.changed("made.md", 8, false);
        assert_eq!(t.unread(), ["made.md", "unread.md"]);
    }

    #[test]
    fn renames_keep_the_read_state_for_documents_and_folders() {
        let mut t = Tracker::new(saved(&[
            ("docs/a.md", 1),
            ("docs/sub/b.md", 2),
            ("docs2/c.md", 3),
        ]));
        scanned(
            &mut t,
            &[("docs/a.md", 1), ("docs/sub/b.md", 20), ("docs2/c.md", 3)],
        );
        let (moves, wrote) = t.renamed("docs", "papers");
        assert!(wrote);
        assert_eq!(
            moves,
            [
                ("docs/a.md".to_owned(), "papers/a.md".to_owned()),
                ("docs/sub/b.md".to_owned(), "papers/sub/b.md".to_owned())
            ]
        );
        assert_eq!(
            t.unread(),
            ["papers/sub/b.md"],
            "read stays read, unread stays unread"
        );
        let (moves, _) = t.renamed("papers/a.md", "papers/renamed.md");
        assert_eq!(moves.len(), 1);
        assert_eq!(t.read_of("papers/renamed.md"), Some(1));
        // renamed to something that is not a document: gone as one
        t.renamed("papers/renamed.md", "papers/renamed.txt");
        assert_eq!(t.read_of("papers/renamed.md"), None);
        // a sibling folder that merely starts with the same letters is left alone
        assert_eq!(t.read_of("docs2/c.md"), Some(3));
    }

    #[test]
    fn a_document_that_went_and_came_back_elsewhere_keeps_what_was_read() {
        let mut t = Tracker::new(saved(&[("old/a.md", 1), ("old/b.md", 2)]));
        scanned(&mut t, &[("old/a.md", 1), ("old/b.md", 20)]);
        let (gone, wrote) = t.removed_docs("old");
        assert!(wrote);
        let a = gone.iter().find(|d| d.path == "old/a.md").unwrap();
        assert_eq!((a.current, a.read), (Some(1), Some(1)));
        assert!(t.paths().is_empty());
        assert!(t.adopt("new/a.md", 1, a.read));
        t.changed("new/b.md", 20, false);
        assert_eq!(t.unread(), ["new/b.md"]);
        assert_eq!(t.read_of("new/a.md"), Some(1));
    }

    #[test]
    fn removals_forget_documents_and_whole_folders() {
        let mut t = Tracker::new(saved(&[("d/a.md", 1), ("d/b.md", 2), ("dd/c.md", 3)]));
        scanned(&mut t, &[("d/a.md", 1), ("d/b.md", 2), ("dd/c.md", 3)]);
        assert!(t.removed("d"));
        assert_eq!(t.paths(), ["dd/c.md"]);
        assert!(!t.removed("nothing"));
        assert!(t.removed("dd/c.md"));
        assert!(t.paths().is_empty());
    }

    #[test]
    fn marks_read_unread_folder_and_all() {
        let mut t = Tracker::new(saved(&[("a/x.md", 1), ("a/y.md", 1), ("b/z.md", 1)]));
        scanned(&mut t, &[("a/x.md", 2), ("a/y.md", 2), ("b/z.md", 2)]);
        assert_eq!(t.unread().len(), 3);
        let (before, wrote) = t.mark_read("a/x.md", 2);
        assert_eq!((before, wrote), (Some(1), true));
        assert_eq!(
            t.mark_read("a/x.md", 2),
            (Some(2), false),
            "reading twice changes nothing"
        );
        // reading a version that is no longer on disk leaves it unread
        t.mark_read("a/y.md", 99);
        assert!(t.unread().contains(&"a/y.md".to_owned()));
        let (paths, wrote) = t.mark_folder_read("a");
        assert!(wrote);
        assert_eq!(paths, ["a/y.md"]);
        assert_eq!(t.unread(), ["b/z.md"]);
        assert!(t.mark_unread("a/x.md"));
        assert_eq!(t.unread(), ["a/x.md", "b/z.md"]);
        let (all, _) = t.mark_folder_read("");
        assert_eq!(all.len(), 2);
        assert!(t.unread().is_empty());
        assert!(!t.mark_unread("not there.md"));
        assert!(!t.mark_unread("a.txt"));
    }

    #[test]
    #[cfg(any(windows, target_os = "macos"))]
    fn case_folded_paths_are_one_document() {
        let mut t = Tracker::new(saved(&[("Docs/Plan.md", 1)]));
        scanned(&mut t, &[("docs/plan.md", 1)]);
        assert!(t.unread().is_empty());
        assert_eq!(t.paths(), ["docs/plan.md"]);
    }

    #[test]
    fn what_the_app_does_while_the_disk_is_walked_is_not_undone_by_the_walk() {
        let mut t = Tracker::new(saved(&[("old.md", 1), ("moved.md", 2), ("marked.md", 3)]));
        scanned(&mut t, &[("old.md", 1), ("moved.md", 2), ("marked.md", 30)]);
        t.begin_scan();
        // during the walk: a document is made, one is deleted, one is read
        t.changed("made.md", 5, true);
        t.removed("old.md");
        t.mark_read("marked.md", 30);
        // the walk passed before all of that
        let wrote = t.scanned(vec![
            ("old.md".to_owned(), 1),
            ("moved.md".to_owned(), 2),
            ("marked.md".to_owned(), 30),
        ]);
        let _ = wrote;
        assert_eq!(
            t.paths(),
            ["made.md", "marked.md", "moved.md"],
            "made stays, deleted stays gone"
        );
        assert_eq!(t.read_of("marked.md"), Some(30));
        // a walk that is stopped tells nothing
        t.begin_scan();
        t.abort_scan();
        assert!(t.unread().is_empty());
    }

    #[test]
    fn a_new_linked_worktree_does_not_flood_with_dots() {
        let mut t = Tracker::new(saved(&[("a.md", 1), ("@wt/old/x.md", 1)]));
        scanned(&mut t, &[("a.md", 1), ("@wt/old/x.md", 1)]);
        // an agent made a worktree: its documents count as read when first seen ...
        scanned(
            &mut t,
            &[
                ("a.md", 1),
                ("@wt/old/x.md", 1),
                ("@wt/new/a.md", 1),
                ("@wt/new/b.md", 2),
            ],
        );
        assert!(t.unread().is_empty());
        // ... and what changes there later is news
        t.changed("@wt/new/a.md", 9, false);
        assert_eq!(t.unread(), ["@wt/new/a.md"]);
        // a new document in a worktree the project knows is unread
        scanned(
            &mut t,
            &[
                ("a.md", 1),
                ("@wt/old/x.md", 1),
                ("@wt/old/fresh.md", 4),
                ("@wt/new/a.md", 9),
            ],
        );
        assert_eq!(t.unread(), ["@wt/new/a.md", "@wt/old/fresh.md"]);
    }

    #[test]
    fn hostile_input_does_not_panic() {
        let mut t = Tracker::new(saved(&[
            ("", 1),
            ("..", 2),
            ("a\0b.md", 3),
            ("日本/ü.md", 4),
        ]));
        scanned(&mut t, &[("日本/ü.md", 4)]);
        t.renamed("", "x");
        t.renamed("日本", "");
        t.removed("");
        t.mark_folder_read("///");
        t.changed("//.md", 1, false);
        let _ = t.unread();
    }
}
