//! The tags and descriptions added in the app, and their `meta.toml` form.
//!
//! ```toml
//! [files."docs/specs/live-feed.md"]
//! tags = ["spec", "needs-review"]
//! description = "How the live feed groups watcher events."
//! ```
//!
//! Files are keyed by their path relative to the project, `/`-separated. Keys the app does not know
//! (a newer version may add some) are kept when the file is written again.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::normalize::{normalize_description, normalize_tags};
use crate::vault::key;

/// Written at the top of every `meta.toml`.
const HEADER: &str = "\
# TrailMark metadata: tags and descriptions added in the app.
# Safe to commit and share. TrailMark rewrites this file when you edit tags.

";

/// What the app knows about one file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Keys written by other versions.
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

impl Entry {
    fn is_blank(&self) -> bool {
        self.tags.is_empty() && self.description.is_empty() && self.extra.is_empty()
    }

    fn normalized(mut self) -> Self {
        self.tags = normalize_tags(&self.tags);
        self.description = normalize_description(&self.description);
        self
    }
}

/// The `meta.toml` document as it is on disk.
#[derive(Serialize, Deserialize, Default)]
struct Disk {
    #[serde(default)]
    files: BTreeMap<String, Entry>,
}

/// Why a `meta.toml` could not be read (a merge conflict, a hand-edit gone wrong).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub message: String,
    /// The line of the first error, counting from 1, when it is known.
    pub line: Option<usize>,
}

/// Every entry, looked up by path (case-insensitively on Windows, like the file system).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entries {
    /// Folded path to the path as written and its entry.
    map: BTreeMap<String, (String, Entry)>,
}

impl Entries {
    /// Reads the text of a `meta.toml`. Tags and descriptions are normalized; blank entries are dropped.
    pub fn parse(text: &str) -> Result<Self, Problem> {
        let disk: Disk = toml::from_str(text).map_err(|e| Problem {
            message: e.message().trim().to_owned(),
            line: e
                .span()
                .map(|s| 1 + text[..s.start.min(text.len())].matches('\n').count()),
        })?;
        let mut entries = Self::default();
        for (path, entry) in disk.files {
            entries.put(&path, entry.normalized());
        }
        Ok(entries)
    }

    /// The text of the `meta.toml`: sorted by path, so the file diffs cleanly.
    pub fn to_toml(&self) -> String {
        let mut files: BTreeMap<String, Entry> = BTreeMap::new();
        for (path, entry) in self.map.values() {
            files.insert(path.clone(), entry.clone());
        }
        let body = if files.is_empty() {
            String::new()
        } else {
            toml::to_string(&Disk { files }).unwrap_or_default()
        };
        format!("{HEADER}{body}")
    }

    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.map.get(&key(path)).map(|(_, e)| e)
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Every path with its entry, in folded-path order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Entry)> {
        self.map.values().map(|(p, e)| (p.as_str(), e))
    }

    /// Replaces the entry of a path; a blank entry removes it.
    pub(super) fn put(&mut self, path: &str, entry: Entry) {
        let k = key(path);
        if path.is_empty() || entry.is_blank() {
            self.map.remove(&k);
        } else {
            self.map.insert(k, (path.to_owned(), entry));
        }
    }

    /// Sets the app tags of a file (normalized).
    pub fn set_tags(&mut self, path: &str, tags: &[String]) {
        let mut entry = self.get(path).cloned().unwrap_or_default();
        entry.tags = normalize_tags(tags);
        self.put(path, entry);
    }

    /// Sets the description of a file (normalized).
    pub fn set_description(&mut self, path: &str, description: &str) {
        let mut entry = self.get(path).cloned().unwrap_or_default();
        entry.description = normalize_description(description);
        self.put(path, entry);
    }

    /// Removes a path. Returns whether it had an entry.
    pub fn remove(&mut self, path: &str) -> bool {
        self.map.remove(&key(path)).is_some()
    }

    /// Moves an entry to a new path (a renamed file). If the new path already has one, the two are
    /// merged. Returns whether anything moved.
    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        if key(from) == key(to) {
            return false;
        }
        let Some((_, moved)) = self.map.remove(&key(from)) else {
            return false;
        };
        self.merge_entry(to, moved);
        true
    }

    /// Adds everything from `other`: tags are united, and `other`'s description is used only where this
    /// side has none.
    pub fn merge(&mut self, other: Entries) {
        for (_, (path, entry)) in other.map {
            self.merge_entry(&path, entry);
        }
    }

    fn merge_entry(&mut self, path: &str, incoming: Entry) {
        let Some(mut entry) = self.get(path).cloned() else {
            self.put(path, incoming);
            return;
        };
        for tag in incoming.tags {
            if !entry.tags.contains(&tag) {
                entry.tags.push(tag);
            }
        }
        if entry.description.is_empty() {
            entry.description = incoming.description;
        }
        for (k, v) in incoming.extra {
            entry.extra.entry(k).or_insert(v);
        }
        self.put(path, entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn writes_the_documented_format() {
        let mut e = Entries::default();
        e.set_tags("docs/specs/live-feed.md", &tags(&["spec", "Needs Review"]));
        e.set_description("docs/specs/live-feed.md", "How the feed\ngroups events.");
        e.set_tags("a.md", &tags(&["x"]));
        assert_eq!(
            e.to_toml(),
            "\
# TrailMark metadata: tags and descriptions added in the app.
# Safe to commit and share. TrailMark rewrites this file when you edit tags.

[files.\"a.md\"]
tags = [\"x\"]

[files.\"docs/specs/live-feed.md\"]
tags = [\"spec\", \"needs-review\"]
description = \"How the feed groups events.\"
"
        );
    }

    #[test]
    fn round_trips() {
        let mut e = Entries::default();
        e.set_tags("docs/a b.md", &tags(&["one", "two"]));
        e.set_description("docs/a b.md", "A \"quoted\" description, with ünïcode.");
        e.set_description("only-description.md", "Just text");
        assert_eq!(Entries::parse(&e.to_toml()), Ok(e));
    }

    #[test]
    fn an_empty_store_is_just_the_header_and_reads_back_empty() {
        let text = Entries::default().to_toml();
        assert!(text.starts_with('#') && !text.contains("files"));
        assert_eq!(Entries::parse(&text), Ok(Entries::default()));
        assert_eq!(Entries::parse(""), Ok(Entries::default()));
    }

    #[test]
    fn hand_edited_entries_are_normalized_and_blank_ones_dropped() {
        let e = Entries::parse(
            "[files.\"a.md\"]\ntags = [\"Spec\", \"spec\", \"  \", \"New Idea\"]\ndescription = \"  two\\nlines \"\n\n[files.\"b.md\"]\ntags = []\n",
        )
        .expect("parse");
        let a = e.get("a.md").expect("a");
        assert_eq!(a.tags, ["spec", "new-idea"]);
        assert_eq!(a.description, "two lines");
        assert!(e.get("b.md").is_none());
    }

    #[test]
    fn keys_from_other_versions_survive_a_rewrite() {
        let source = "[files.\"a.md\"]\ntags = [\"x\"]\ncolor = \"blue\"\n";
        let mut e = Entries::parse(source).expect("parse");
        e.set_description("a.md", "d");
        let text = e.to_toml();
        assert!(text.contains("color = \"blue\""), "{text}");
        assert_eq!(Entries::parse(&text), Ok(e));
    }

    #[test]
    fn blank_entries_are_removed() {
        let mut e = Entries::default();
        e.set_tags("a.md", &tags(&["x"]));
        e.set_tags("a.md", &[]);
        assert!(e.is_empty());
        e.set_description("a.md", " \n ");
        assert!(e.is_empty());
    }

    #[test]
    fn a_conflicted_file_is_reported_with_its_line() {
        let text = "[files.\"a.md\"]\ntags = [\"x\"]\n<<<<<<< HEAD\ntags = [\"y\"]\n=======\n";
        let problem = Entries::parse(text).expect_err("conflict markers");
        assert_eq!(problem.line, Some(3));
        assert!(!problem.message.is_empty());
        let wrong_type = Entries::parse("[files.\"a.md\"]\ntags = 5\n").expect_err("type");
        assert_eq!(wrong_type.line, Some(2));
        assert!(Entries::parse("files = 3").is_err());
    }

    #[test]
    fn rename_moves_the_entry_and_merges_into_an_existing_one() {
        let mut e = Entries::default();
        e.set_tags("old.md", &tags(&["a", "b"]));
        e.set_description("old.md", "from old");
        assert!(e.rename("old.md", "new.md"));
        assert!(e.get("old.md").is_none());
        assert_eq!(
            e.get("new.md").map(|x| x.tags.clone()),
            Some(tags(&["a", "b"]))
        );

        e.set_tags("target.md", &tags(&["b", "c"]));
        e.set_description("target.md", "keep me");
        assert!(e.rename("new.md", "target.md"));
        let t = e.get("target.md").expect("target");
        assert_eq!(t.tags, ["b", "c", "a"]);
        assert_eq!(t.description, "keep me");

        assert!(!e.rename("missing.md", "x.md"));
        assert!(!e.rename("target.md", "target.md"));
    }

    #[test]
    fn merge_unites_tags_and_prefers_the_receivers_description() {
        let mut base = Entries::default();
        base.set_tags("a.md", &tags(&["x"]));
        base.set_description("a.md", "project");
        let mut other = Entries::default();
        other.set_tags("a.md", &tags(&["y", "x"]));
        other.set_description("a.md", "mine");
        other.set_description("b.md", "only mine");
        base.merge(other);
        let a = base.get("a.md").expect("a");
        assert_eq!(
            (a.tags.clone(), a.description.as_str()),
            (tags(&["x", "y"]), "project")
        );
        assert_eq!(
            base.get("b.md").map(|e| e.description.as_str()),
            Some("only mine")
        );
    }

    #[test]
    fn lookups_follow_the_file_systems_case_rules() {
        let mut e = Entries::default();
        e.set_tags("Docs/A.md", &tags(&["x"]));
        assert_eq!(e.get("Docs/A.md").map(|x| x.tags.len()), Some(1));
        assert_eq!(e.get("docs/a.md").is_some(), cfg!(windows));
        assert!(e.remove("Docs/A.md"));
        assert!(e.is_empty());
    }
}
