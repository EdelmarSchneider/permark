//! The bookkeeping behind the unread markers: for every Markdown document, what it holds on disk now and
//! what it held when it was last read. No files, threads or events in here, so every rule is tested
//! directly.
//!
//! A document is unread when the two differ, or when it was never read. Paths are the vault paths the UI
//! uses (`@wt/<name>/...` in a linked worktree) and are compared the way the file system compares them
//! (case-folded on Windows and macOS).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::render::markdown::is_markdown_path;
use crate::vault::key;

/// Whether the file name says Markdown: the only documents that are tracked.
pub fn is_tracked(path: &str) -> bool {
    is_markdown_path(Path::new(path))
}

#[derive(Debug, Clone)]
struct Doc {
    /// The path as the UI spells it.
    path: String,
    /// The hash of the content on disk now (`None`: not looked at yet).
    current: Option<u64>,
    /// The hash of the content when the document was last read (`None`: never read).
    read: Option<u64>,
}

/// The documents of a project and their read state.
#[derive(Debug, Default)]
pub struct Tracker {
    /// By case-folded path.
    docs: BTreeMap<String, Doc>,
    /// The project had no read state when it was opened: the first look at the disk ([`Tracker::scanned`])
    /// becomes the baseline, where everything that exists counts as read.
    baseline_pending: bool,
    /// Documents marked unread before the baseline was taken: they stay unread through it.
    forced: BTreeSet<String>,
}

/// The folder part of a key, ready to be a prefix of the keys below it.
fn tree_prefix(folder: &str) -> String {
    format!("{}/", key(folder).trim_end_matches('/'))
}

impl Tracker {
    /// `saved` is what `read.json` held, or `None` when the project has no read state yet.
    pub fn new(saved: Option<BTreeMap<String, u64>>) -> Self {
        let baseline_pending = saved.is_none();
        let docs = saved
            .unwrap_or_default()
            .into_iter()
            .map(|(path, hash)| {
                (
                    key(&path),
                    Doc {
                        path,
                        current: None,
                        read: Some(hash),
                    },
                )
            })
            .collect();
        Self {
            docs,
            baseline_pending,
            forced: BTreeSet::new(),
        }
    }

    /// Whether the first look at the disk, which decides what counts as read, has not happened yet.
    pub fn baseline_pending(&self) -> bool {
        self.baseline_pending
    }

    /// What `read.json` holds: the hash of every document that was read.
    pub fn saved(&self) -> BTreeMap<String, u64> {
        self.docs
            .values()
            .filter_map(|doc| doc.read.map(|hash| (doc.path.clone(), hash)))
            .collect()
    }

    /// The documents that were never read or changed since: nothing is unread until the disk has been looked at.
    pub fn unread(&self) -> Vec<String> {
        self.docs
            .values()
            .filter(|doc| doc.current.is_some() && doc.read != doc.current)
            .map(|doc| doc.path.clone())
            .collect()
    }

    fn keys_under(&self, folder: &str) -> Vec<String> {
        let prefix = tree_prefix(folder);
        self.docs
            .range(prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&prefix))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Every Markdown document of the project, as the disk shows them now (`files` are paths with hashes).
    /// A document that is not in the list is gone and is forgotten. With no read state yet, everything that
    /// is there counts as read. Returns whether `read.json` has to be written.
    pub fn scanned(&mut self, files: Vec<(String, u64)>) -> bool {
        let before = self.saved();
        let baseline = std::mem::take(&mut self.baseline_pending);
        let forced = std::mem::take(&mut self.forced);
        let mut docs = BTreeMap::new();
        for (path, hash) in files {
            let k = key(&path);
            let mut doc = self.docs.remove(&k).unwrap_or(Doc {
                path: String::new(),
                current: None,
                read: None,
            });
            doc.path = path;
            doc.current = Some(hash);
            if baseline && doc.read.is_none() && !forced.contains(&k) {
                doc.read = Some(hash);
            }
            docs.insert(k, doc);
        }
        self.docs = docs;
        // The baseline is written even when it is empty: the file is what tells "nothing was read yet" from
        // "nothing has been looked at yet".
        baseline || self.saved() != before
    }

    /// A document was created or its content changed: this is what it holds now.
    pub fn changed(&mut self, path: &str, hash: u64) {
        if self.baseline_pending || !is_tracked(path) {
            return;
        }
        let doc = self.docs.entry(key(path)).or_insert_with(|| Doc {
            path: String::new(),
            current: None,
            read: None,
        });
        path.clone_into(&mut doc.path);
        doc.current = Some(hash);
    }

    /// A document, or a folder with documents in it, is gone. Returns whether `read.json` has to be written.
    pub fn removed(&mut self, path: &str) -> bool {
        let mut gone = self.keys_under(path);
        gone.push(key(path));
        let mut wrote = false;
        for k in gone {
            wrote |= self.docs.remove(&k).is_some_and(|doc| doc.read.is_some());
        }
        wrote
    }

    /// A document, or a folder with documents in it, moved: what was read stays read. Returns whether
    /// `read.json` has to be written.
    pub fn renamed(&mut self, from: &str, to: &str) -> bool {
        if self.baseline_pending {
            return false;
        }
        let mut moving: Vec<(String, Doc)> = Vec::new();
        if let Some(doc) = self.docs.remove(&key(from)) {
            moving.push((to.to_owned(), doc));
        }
        // Below a folder: the part after the folder's name stays, whatever the case it was spelled in.
        let depth = from.trim_end_matches('/').split('/').count();
        for k in self.keys_under(from) {
            if let Some(doc) = self.docs.remove(&k) {
                let rest = doc
                    .path
                    .split('/')
                    .skip(depth)
                    .collect::<Vec<_>>()
                    .join("/");
                moving.push((format!("{}/{rest}", to.trim_end_matches('/')), doc));
            }
        }
        let mut wrote = false;
        for (path, mut doc) in moving {
            wrote |= doc.read.is_some();
            if is_tracked(&path) {
                doc.path.clone_from(&path);
                self.docs.insert(key(&path), doc);
            }
        }
        wrote
    }

    /// The person read the document, which holds `hash`. Returns whether `read.json` has to be written.
    pub fn mark_read(&mut self, path: &str, hash: u64) -> bool {
        if self.baseline_pending || !is_tracked(path) {
            return false;
        }
        let doc = self.docs.entry(key(path)).or_insert_with(|| Doc {
            path: String::new(),
            current: None,
            read: None,
        });
        path.clone_into(&mut doc.path);
        doc.current = Some(hash);
        doc.read.replace(hash) != Some(hash)
    }

    /// The person wants the document to count as unread again. Returns whether `read.json` has to be written.
    pub fn mark_unread(&mut self, path: &str) -> bool {
        let k = key(path);
        if self.baseline_pending {
            if is_tracked(path) {
                self.forced.insert(k);
            }
            return false;
        }
        self.docs
            .get_mut(&k)
            .is_some_and(|doc| doc.read.take().is_some())
    }

    fn read_all_of(&mut self, keys: &[String]) -> bool {
        let mut wrote = false;
        for k in keys {
            if let Some(doc) = self.docs.get_mut(k) {
                if doc.current.is_some() && doc.read != doc.current {
                    doc.read = doc.current;
                    wrote = true;
                }
            }
        }
        wrote
    }

    /// Everything that is there counts as read. Returns whether `read.json` has to be written.
    pub fn mark_all_read(&mut self) -> bool {
        if self.baseline_pending {
            return false;
        }
        let keys: Vec<String> = self.docs.keys().cloned().collect();
        self.read_all_of(&keys)
    }

    /// Everything in a folder counts as read. Returns whether `read.json` has to be written.
    pub fn mark_folder_read(&mut self, folder: &str) -> bool {
        if self.baseline_pending {
            return false;
        }
        let keys = self.keys_under(folder);
        self.read_all_of(&keys)
    }

    /// The app rewrote a document (the links in it) and it went from `old` to `new`: a document that had been
    /// read stays read, because the change is the app's own doing. Returns whether `read.json` has to be written.
    pub fn rewritten(&mut self, path: &str, old: u64, new: u64) -> bool {
        let Some(doc) = self.docs.get_mut(&key(path)) else {
            return false;
        };
        if doc.read == Some(old) {
            doc.read = Some(new);
            doc.current = Some(new);
            true
        } else {
            if doc.current == Some(old) {
                doc.current = Some(new);
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTHING: [&str; 0] = [];

    fn scanned(files: &[(&str, u64)]) -> Tracker {
        let mut t = Tracker::new(None);
        t.scanned(files.iter().map(|(p, h)| ((*p).to_owned(), *h)).collect());
        t
    }

    fn saved(entries: &[(&str, u64)]) -> BTreeMap<String, u64> {
        entries.iter().map(|(p, h)| ((*p).to_owned(), *h)).collect()
    }

    fn files(entries: &[(&str, u64)]) -> Vec<(String, u64)> {
        entries.iter().map(|(p, h)| ((*p).to_owned(), *h)).collect()
    }

    #[test]
    fn a_project_without_read_state_counts_everything_that_exists_as_read() {
        let mut t = Tracker::new(None);
        assert!(t.baseline_pending());
        assert_eq!(t.unread(), NOTHING);
        assert!(t.scanned(files(&[("a.md", 1), ("docs/b.md", 2)])));
        assert!(!t.baseline_pending());
        assert_eq!(t.unread(), NOTHING);
        assert_eq!(t.saved(), saved(&[("a.md", 1), ("docs/b.md", 2)]));
    }

    #[test]
    fn a_baseline_of_nothing_is_still_written() {
        let mut t = Tracker::new(None);
        assert!(t.scanned(Vec::new()), "the file tells a baseline from none");
        assert!(!t.scanned(Vec::new()), "but only the first time");
    }

    #[test]
    fn files_created_after_the_baseline_start_unread() {
        let mut t = scanned(&[("a.md", 1)]);
        t.changed("new.md", 7);
        assert_eq!(t.unread(), ["new.md"]);
        assert_eq!(t.saved(), saved(&[("a.md", 1)]), "never read, so not saved");
    }

    #[test]
    fn with_a_read_state_files_that_changed_or_appeared_while_closed_are_unread() {
        let mut t = Tracker::new(Some(saved(&[("a.md", 1), ("b.md", 2), ("gone.md", 3)])));
        assert!(
            t.unread().is_empty(),
            "nothing is known before the disk is looked at"
        );
        // a.md is as read, b.md changed, c.md is new, gone.md is gone
        let wrote = t.scanned(files(&[("a.md", 1), ("b.md", 22), ("c.md", 4)]));
        assert!(wrote, "the forgotten document changes the state");
        assert_eq!(t.unread(), ["b.md", "c.md"]);
        assert_eq!(t.saved(), saved(&[("a.md", 1), ("b.md", 2)]));
    }

    #[test]
    fn a_scan_that_changes_nothing_does_not_ask_for_a_write() {
        let mut t = Tracker::new(Some(saved(&[("a.md", 1)])));
        assert!(!t.scanned(files(&[("a.md", 1), ("new.md", 5)])));
    }

    #[test]
    fn a_file_changed_and_then_reverted_to_the_content_last_read_is_read_again() {
        let mut t = scanned(&[("a.md", 1)]);
        t.changed("a.md", 2);
        assert_eq!(t.unread(), ["a.md"]);
        t.changed("a.md", 1);
        assert_eq!(t.unread(), NOTHING);
    }

    #[test]
    fn only_markdown_documents_are_tracked() {
        let mut t = scanned(&[("a.md", 1)]);
        t.changed("logo.png", 5);
        t.changed("notes.txt", 6);
        assert_eq!(t.unread(), NOTHING);
        assert!(!t.mark_read("logo.png", 5));
        t.changed("b.markdown", 7);
        t.changed("c.MD", 8);
        assert_eq!(t.unread(), ["b.markdown", "c.MD"]);
    }

    #[test]
    fn marking_read_stores_the_content_that_was_read() {
        let mut t = scanned(&[("a.md", 1)]);
        t.changed("a.md", 2);
        assert!(t.mark_read("a.md", 2));
        assert_eq!(t.unread(), NOTHING);
        assert_eq!(t.saved(), saved(&[("a.md", 2)]));
        assert!(!t.mark_read("a.md", 2), "nothing new to write");
        // the document changes again after it was read
        t.changed("a.md", 3);
        assert_eq!(t.unread(), ["a.md"]);
    }

    #[test]
    fn marking_read_a_document_that_was_not_known_yet_tracks_it() {
        let mut t = scanned(&[]);
        assert!(t.mark_read("made-here.md", 9));
        assert_eq!(t.unread(), NOTHING);
        assert_eq!(t.saved(), saved(&[("made-here.md", 9)]));
    }

    #[test]
    fn marking_unread_forgets_what_was_read() {
        let mut t = scanned(&[("a.md", 1), ("b.md", 2)]);
        assert!(t.mark_unread("a.md"));
        assert_eq!(t.unread(), ["a.md"]);
        assert_eq!(t.saved(), saved(&[("b.md", 2)]));
        assert!(!t.mark_unread("a.md"), "already unread");
        assert!(!t.mark_unread("nothing.md"));
        // reading it again brings it back
        t.mark_read("a.md", 1);
        assert_eq!(t.unread(), NOTHING);
    }

    #[test]
    fn marking_unread_before_the_baseline_is_taken_survives_it() {
        let mut t = Tracker::new(None);
        assert!(!t.mark_unread("a.md"));
        t.scanned(files(&[("a.md", 1), ("b.md", 2)]));
        assert_eq!(t.unread(), ["a.md"]);
        assert_eq!(t.saved(), saved(&[("b.md", 2)]));
    }

    #[test]
    fn nothing_but_the_scan_changes_anything_before_the_baseline() {
        let mut t = Tracker::new(None);
        t.changed("a.md", 1);
        assert!(!t.mark_read("a.md", 1));
        assert!(!t.mark_all_read());
        assert!(!t.mark_folder_read("docs"));
        assert!(!t.renamed("a.md", "b.md"));
        assert_eq!(t.saved(), saved(&[]));
        assert_eq!(t.unread(), NOTHING);
    }

    #[test]
    fn mark_all_read_reads_whatever_is_unread() {
        let mut t = scanned(&[("a.md", 1)]);
        t.changed("a.md", 2);
        t.changed("b.md", 3);
        assert!(t.mark_all_read());
        assert_eq!(t.unread(), NOTHING);
        assert_eq!(t.saved(), saved(&[("a.md", 2), ("b.md", 3)]));
        assert!(!t.mark_all_read(), "nothing left to read");
    }

    #[test]
    fn mark_folder_read_reads_the_folder_and_what_is_below_it_only() {
        let mut t = scanned(&[]);
        for path in [
            "docs/a.md",
            "docs/deep/b.md",
            "docs.md",
            "documents/c.md",
            "other/d.md",
        ] {
            t.changed(path, 1);
        }
        assert!(t.mark_folder_read("docs"));
        assert_eq!(t.unread(), ["docs.md", "documents/c.md", "other/d.md"]);
        assert!(!t.mark_folder_read("docs"));
    }

    #[test]
    fn a_removed_document_is_dropped_and_comes_back_as_new() {
        let mut t = scanned(&[("a.md", 1), ("b.md", 2)]);
        assert!(t.removed("a.md"));
        assert_eq!(t.saved(), saved(&[("b.md", 2)]));
        t.changed("a.md", 1);
        assert_eq!(t.unread(), ["a.md"], "the read state went with the file");
    }

    #[test]
    fn a_removed_folder_drops_every_document_below_it() {
        let mut t = scanned(&[("docs/a.md", 1), ("docs/sub/b.md", 2), ("keep.md", 3)]);
        assert!(t.removed("docs"));
        assert_eq!(t.saved(), saved(&[("keep.md", 3)]));
        assert!(!t.removed("docs"));
        assert!(!t.removed("never-there.md"));
    }

    #[test]
    fn a_renamed_document_keeps_its_read_state() {
        let mut t = scanned(&[("a.md", 1), ("b.md", 2)]);
        t.changed("b.md", 22);
        assert!(t.renamed("a.md", "docs/a2.md"));
        assert!(t.renamed("b.md", "b2.md"));
        // read stays read, unread stays unread
        assert_eq!(t.unread(), ["b2.md"]);
        assert_eq!(t.saved(), saved(&[("docs/a2.md", 1), ("b2.md", 2)]));
    }

    #[test]
    fn a_renamed_folder_takes_its_documents_along() {
        let mut t = scanned(&[("old/a.md", 1), ("old/sub/b.md", 2), ("older/c.md", 3)]);
        t.changed("old/sub/b.md", 22);
        assert!(t.renamed("old", "new/place"));
        assert_eq!(t.unread(), ["new/place/sub/b.md"]);
        assert_eq!(
            t.saved(),
            saved(&[
                ("new/place/a.md", 1),
                ("new/place/sub/b.md", 2),
                ("older/c.md", 3)
            ])
        );
    }

    #[test]
    fn a_document_renamed_to_another_kind_of_file_is_no_longer_tracked() {
        let mut t = scanned(&[("a.md", 1)]);
        assert!(t.renamed("a.md", "a.txt"));
        assert_eq!(t.saved(), saved(&[]));
        assert_eq!(t.unread(), NOTHING);
    }

    #[test]
    fn renaming_something_that_is_not_known_does_nothing() {
        let mut t = scanned(&[("a.md", 1)]);
        assert!(!t.renamed("x.md", "y.md"));
        assert_eq!(t.saved(), saved(&[("a.md", 1)]));
    }

    #[test]
    fn a_rewrite_by_the_app_keeps_a_read_document_read() {
        let mut t = scanned(&[("a.md", 1), ("b.md", 2)]);
        t.changed("b.md", 22);
        // links rewritten: a.md was read and stays read; b.md was not and stays unread
        assert!(t.rewritten("a.md", 1, 10));
        assert!(!t.rewritten("b.md", 22, 23));
        assert_eq!(t.unread(), ["b.md"]);
        assert_eq!(t.saved(), saved(&[("a.md", 10), ("b.md", 2)]));
        // putting the links back (an undo) is the same in reverse
        assert!(t.rewritten("a.md", 10, 1));
        assert_eq!(t.saved(), saved(&[("a.md", 1), ("b.md", 2)]));
        assert!(t.unread().contains(&"b.md".to_owned()));
    }

    #[test]
    fn a_rewrite_of_a_document_that_was_not_read_in_the_version_it_had_stays_unread() {
        let mut t = scanned(&[("a.md", 1)]);
        t.changed("a.md", 5);
        // the app rewrote version 5, which nobody read
        assert!(!t.rewritten("a.md", 5, 10));
        assert_eq!(t.unread(), ["a.md"]);
        assert_eq!(t.saved(), saved(&[("a.md", 1)]));
        // it is read as it is now, not as it was
        t.mark_read("a.md", 10);
        assert_eq!(t.unread(), NOTHING);
        assert!(!t.rewritten("unknown.md", 1, 2));
    }

    // Windows and macOS compare paths without regard to case; Linux does not.
    #[test]
    fn paths_are_compared_the_way_the_file_system_compares_them() {
        let mut t = scanned(&[("Docs/A.md", 1)]);
        t.changed("docs/a.md", 2);
        assert_eq!(t.unread(), ["docs/a.md"]);
        // Where case does not matter that is the same document, now spelled as the disk spells it.
        let spelling = if crate::vault::CASE_INSENSITIVE {
            "docs/a.md"
        } else {
            "Docs/A.md"
        };
        assert_eq!(t.saved(), saved(&[(spelling, 1)]));
    }

    #[test]
    fn a_scan_replaces_the_spelling_of_a_path_with_the_disks() {
        let mut t = Tracker::new(Some(saved(&[("docs/a.md", 1)])));
        t.scanned(files(&[("docs/a.md", 1)]));
        assert_eq!(t.saved(), saved(&[("docs/a.md", 1)]));
    }

    #[test]
    fn unread_is_listed_in_a_stable_order() {
        let mut t = scanned(&[]);
        for path in ["b/z.md", "a.md", "b/a.md"] {
            t.changed(path, 1);
        }
        assert_eq!(t.unread(), ["a.md", "b/a.md", "b/z.md"]);
    }
}
