//! The project's files with their tags, as pure functions over the app's entries and the front matter tags found in
//! the documents: counting, finding, combining.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::meta::Entries;
use crate::normalize::normalize_tag;

/// What was read of a document's front matter, and the stamp it was read at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocTags {
    pub tags: Vec<String>,
    pub len: u64,
    pub mtime_ms: u64,
}

/// The front matter tags of the Markdown documents of the main root, by project path.
pub type Docs = HashMap<String, DocTags>;

/// One tag of a file. A tag in both places is listed once, as a file tag, and `app` says the app has it too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRef {
    pub name: String,
    pub file: bool,
    pub app: bool,
}

/// The tags of one file: the front matter's first, then the ones only the app has.
#[must_use]
pub fn combine(file_tags: &[String], app_tags: &[String]) -> Vec<TagRef> {
    let mut out: Vec<TagRef> = file_tags
        .iter()
        .map(|name| TagRef {
            name: name.clone(),
            file: true,
            app: app_tags.contains(name),
        })
        .collect();
    for name in app_tags {
        if !out.iter().any(|t| &t.name == name) {
            out.push(TagRef {
                name: name.clone(),
                file: false,
                app: true,
            });
        }
    }
    out
}

fn file_tags<'a>(docs: &'a Docs, path: &str) -> &'a [String] {
    docs.get(path).map_or(&[], |d| d.tags.as_slice())
}

fn app_tags<'a>(entries: &'a Entries, path: &str) -> &'a [String] {
    entries.get(path).map_or(&[], |e| e.tags.as_slice())
}

/// The names of every tag of a file (front matter first).
#[must_use]
pub fn tags_of(entries: &Entries, docs: &Docs, path: &str) -> Vec<String> {
    combine(file_tags(docs, path), app_tags(entries, path))
        .into_iter()
        .map(|t| t.name)
        .collect()
}

/// A tag with the number of files that have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    pub name: String,
    pub count: usize,
    /// No file has it from the app: it exists only in front matter.
    pub only_in_files: bool,
}

/// Every path that has an entry or is a document.
fn universe<'a>(entries: &'a Entries, docs: &'a Docs) -> BTreeSet<&'a str> {
    entries
        .iter()
        .map(|(p, _)| p)
        .chain(docs.keys().map(String::as_str))
        .collect()
}

/// Tags by number of files, most files first, then by name.
#[must_use]
pub fn tag_counts(entries: &Entries, docs: &Docs) -> Vec<TagCount> {
    let mut counts: BTreeMap<String, (usize, bool)> = BTreeMap::new();
    for path in universe(entries, docs) {
        for tag in combine(file_tags(docs, path), app_tags(entries, path)) {
            let slot = counts.entry(tag.name).or_insert((0, true));
            slot.0 += 1;
            slot.1 &= !tag.app;
        }
    }
    let mut out: Vec<TagCount> = counts
        .into_iter()
        .map(|(name, (count, only_in_files))| TagCount {
            name,
            count,
            only_in_files,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Markdown documents without any tag, by path.
#[must_use]
pub fn untagged(entries: &Entries, docs: &Docs) -> Vec<String> {
    let mut out: Vec<String> = docs
        .keys()
        .filter(|path| file_tags(docs, path).is_empty() && app_tags(entries, path).is_empty())
        .cloned()
        .collect();
    out.sort();
    out
}

/// The files that have every one of `tags` (names are normalized first; none given = nothing), by path.
#[must_use]
pub fn files_with(entries: &Entries, docs: &Docs, tags: &[String]) -> Vec<String> {
    let wanted: Vec<String> = tags.iter().filter_map(|t| normalize_tag(t)).collect();
    if wanted.is_empty() {
        return Vec::new();
    }
    universe(entries, docs)
        .into_iter()
        .filter(|path| {
            let have = tags_of(entries, docs, path);
            wanted.iter().all(|w| have.contains(w))
        })
        .map(str::to_owned)
        .collect()
}

/// [`files_with`] as a set (search).
#[must_use]
pub fn files_with_set(entries: &Entries, docs: &Docs, tags: &[String]) -> HashSet<String> {
    files_with(entries, docs, tags).into_iter().collect()
}

/// A file with something to show: tags (both places) or a description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRow {
    pub path: String,
    pub tags: Vec<String>,
    pub description: String,
}

/// The files that have a tag or a description, by path.
#[must_use]
pub fn index(entries: &Entries, docs: &Docs) -> Vec<IndexRow> {
    universe(entries, docs)
        .into_iter()
        .filter_map(|path| {
            let tags = tags_of(entries, docs, path);
            let description = entries
                .get(path)
                .map(|e| e.description.clone())
                .unwrap_or_default();
            (!tags.is_empty() || !description.is_empty()).then(|| IndexRow {
                path: path.to_owned(),
                tags,
                description,
            })
        })
        .collect()
}
