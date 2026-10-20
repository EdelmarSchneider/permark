//! The data of one project's index: the file list (quick open), the sections (BM25), the link index. Plain data
//! behind one `RwLock`; the worker is the only writer.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Bound;
use std::sync::Arc;

use super::bm25::Bm25;
use super::fuzzy::folded_if_different;
use super::prepare::{Prepared, Sig};
use crate::tree::Category;

/// One file of the list.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub category: Category,
    /// 0 = unknown (set for documents the index read and for files seen in a watch batch).
    pub mtime_ms: u64,
    /// The folded path when folding changes it (the common lowercase ASCII path needs no copy).
    pub folded: Option<Box<str>>,
}

#[derive(Debug)]
struct DocEntry {
    sections: Vec<u32>,
    sig: Sig,
    links: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Index {
    /// Every file that is not ignored, by project path.
    pub files: BTreeMap<String, FileEntry>,
    /// Folder path -> files below it (any depth).
    pub folders: BTreeMap<String, u32>,
    pub bm25: Bm25,
    docs: HashMap<Arc<str>, DocEntry>,
    /// Link target key -> documents linking to it.
    link_counts: HashMap<String, u32>,
    /// The set of link targets changed since `take_linked`.
    linked_dirty: bool,
}

fn parents(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(move |(at, _)| &path[..at])
}

impl Index {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of documents whose content is indexed.
    #[must_use]
    pub fn doc_count(&self) -> usize {
        self.docs.len()
    }

    /// The signature the index read a document with.
    #[must_use]
    pub fn sig_of(&self, path: &str) -> Option<Sig> {
        self.docs.get(path).map(|d| d.sig)
    }

    pub fn add_file(&mut self, path: &str, category: Category) {
        if self.files.contains_key(path) {
            return;
        }
        for folder in parents(path) {
            *self.folders.entry(folder.to_owned()).or_default() += 1;
        }
        self.files.insert(
            path.to_owned(),
            FileEntry {
                category,
                mtime_ms: 0,
                folded: folded_if_different(path),
            },
        );
    }

    pub fn set_mtime(&mut self, path: &str, mtime_ms: u64) {
        if let Some(entry) = self.files.get_mut(path) {
            entry.mtime_ms = mtime_ms;
        }
    }

    /// Inserts (or replaces) a document read from disk.
    pub fn upsert_doc(&mut self, prepared: Prepared, category: Category) {
        let old = self.docs.remove(&*prepared.path);
        self.add_file(&prepared.path, category);
        self.set_mtime(&prepared.path, prepared.sig.mtime_ms);
        // the new links count before the old ones go: links that stay never flicker out of the set
        for key in &prepared.links {
            let count = self.link_counts.entry(key.clone()).or_default();
            *count += 1;
            if *count == 1 {
                self.linked_dirty = true;
            }
        }
        if let Some(old) = old {
            self.bm25.remove(&old.sections);
            self.release_links(old.links);
        }
        let sections = self.bm25.add(&prepared.path, prepared.sections);
        self.docs.insert(
            prepared.path,
            DocEntry {
                sections,
                sig: prepared.sig,
                links: prepared.links,
            },
        );
    }

    /// A file that cannot be read as text (any more): it stays in the list, its content goes.
    pub fn drop_content(&mut self, path: &str) {
        self.drop_doc(path);
    }

    fn drop_doc(&mut self, path: &str) {
        if let Some(doc) = self.docs.remove(path) {
            self.bm25.remove(&doc.sections);
            self.release_links(doc.links);
        }
    }

    fn release_links(&mut self, keys: Vec<String>) {
        for key in keys {
            if let Some(count) = self.link_counts.get_mut(&key) {
                *count -= 1;
                if *count == 0 {
                    self.link_counts.remove(&key);
                    self.linked_dirty = true;
                }
            }
        }
    }

    /// Forgets a file and its content.
    pub fn remove_file(&mut self, path: &str) {
        self.drop_doc(path);
        if self.files.remove(path).is_some() {
            for folder in parents(path) {
                if let Some(count) = self.folders.get_mut(folder) {
                    *count -= 1;
                    if *count == 0 {
                        self.folders.remove(folder);
                    }
                }
            }
        }
    }

    /// Forgets every file below the folder `dir` (`docs` goes with `docs/a.md`, not `docs2/b.md`).
    pub fn remove_under(&mut self, dir: &str) -> usize {
        let from = format!("{dir}/");
        let until = format!("{dir}0"); // '0' follows '/'
        let gone: Vec<String> = self
            .files
            .range::<str, _>((
                Bound::Included(from.as_str()),
                Bound::Excluded(until.as_str()),
            ))
            .map(|(path, _)| path.clone())
            .collect();
        let mut section_ids: Vec<u32> = Vec::new();
        let mut links: Vec<String> = Vec::new();
        for path in &gone {
            if let Some(doc) = self.docs.remove(path.as_str()) {
                section_ids.extend(doc.sections);
                links.extend(doc.links);
            }
            self.files.remove(path);
        }
        self.bm25.remove(&section_ids);
        self.release_links(links);
        // folder counts: the folder itself and everything below it go; the ones above lose the files
        let below: Vec<String> = self
            .folders
            .range::<str, _>((
                Bound::Included(from.as_str()),
                Bound::Excluded(until.as_str()),
            ))
            .map(|(folder, _)| folder.clone())
            .collect();
        for folder in below {
            self.folders.remove(&folder);
        }
        let removed = u32::try_from(gone.len()).unwrap_or(u32::MAX);
        for folder in std::iter::once(dir).chain(parents(dir)) {
            if let Some(count) = self.folders.get_mut(folder) {
                *count = count.saturating_sub(removed);
                if *count == 0 {
                    self.folders.remove(folder);
                }
            }
        }
        gone.len()
    }

    /// Documents whose links name the target `key`.
    #[must_use]
    pub fn linking_to(&self, key: &str) -> Vec<String> {
        self.docs
            .iter()
            .filter(|(_, doc)| doc.links.iter().any(|k| k == key))
            .map(|(path, _)| path.to_string())
            .collect()
    }

    /// The set of link targets when it changed since the last call.
    pub fn take_linked(&mut self) -> Option<Arc<HashSet<String>>> {
        if !self.linked_dirty {
            return None;
        }
        self.linked_dirty = false;
        Some(Arc::new(self.link_counts.keys().cloned().collect()))
    }

    /// Link targets right now (tests).
    #[must_use]
    pub fn linked_now(&self) -> HashSet<String> {
        self.link_counts.keys().cloned().collect()
    }
}
