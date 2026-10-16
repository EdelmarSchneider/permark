//! The tags and descriptions added in the app, and their `meta.toml` form.
//!
//! ```toml
//! [files."docs/specs/live-feed.md"]
//! tags = ["spec", "needs-review"]
//! description = "How the live feed groups watcher events."
//!
//! [tag_styles.spec]
//! color = "blue"
//! icon = "book-open"
//! ```
//!
//! Files are keyed by their path relative to the project, `/`-separated. A tag's look (an optional color
//! and icon, see [`super::style`]) is kept per project under its name, whichever files have the tag; the
//! style of a tag that no file has any more is harmless and stays. Keys and tables the app does
//! not know (a newer version may add some, in the file or at the top level) are kept when the file is
//! written again. Two paths that differ only in case are two entries, even on a case-insensitive
//! system: the file may have been written on one that tells them apart.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::normalize::{normalize_description, normalize_tag, normalize_tags};
use super::style::TagStyle;
use crate::vault::key;

/// Written at the top of every `meta.toml`. A file this app wrote is never shorter than this.
const HEADER: &str = "\
# TrailMark metadata: tags and descriptions added in the app.
# Safe to commit and share. TrailMark rewrites this file when you edit tags.

";

/// How many bytes the header takes: a `meta.toml` the app wrote is at least this long, so a shorter
/// one was cut off while someone was writing it.
pub const HEADER_LEN: usize = HEADER.len();

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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    files: BTreeMap<String, Entry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    tag_styles: BTreeMap<String, TagStyle>,
    /// Top-level keys and tables written by other versions or tools.
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
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
    /// (folded path, path as written) to the entry.
    map: BTreeMap<(String, String), Entry>,
    /// The look of tags, by (normalized) tag name.
    styles: BTreeMap<String, TagStyle>,
    /// Top-level keys and tables the app does not know.
    extra: BTreeMap<String, toml::Value>,
}

fn slot(path: &str) -> (String, String) {
    (key(path), path.to_owned())
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
        let mut entries = Self {
            map: BTreeMap::new(),
            styles: BTreeMap::new(),
            extra: disk.extra,
        };
        for (tag, style) in disk.tag_styles {
            if let Some(tag) = normalize_tag(&tag).filter(|_| !style.is_blank()) {
                entries.styles.entry(tag).or_insert(style);
            }
        }
        for (path, entry) in disk.files {
            let entry = entry.normalized();
            if !path.is_empty() && !entry.is_blank() {
                entries.map.insert(slot(&path), entry);
            }
        }
        Ok(entries)
    }

    /// The text of the `meta.toml`: sorted by path, so the file diffs cleanly.
    pub fn to_toml(&self) -> String {
        let files: BTreeMap<String, Entry> = self
            .map
            .iter()
            .map(|((_, path), entry)| (path.clone(), entry.clone()))
            .collect();
        let body = if files.is_empty() && self.styles.is_empty() && self.extra.is_empty() {
            String::new()
        } else {
            let disk = Disk {
                files,
                tag_styles: self.styles.clone(),
                extra: self.extra.clone(),
            };
            toml::to_string(&disk).unwrap_or_default()
        };
        format!("{HEADER}{body}")
    }

    /// The entry key a path refers to: the entry spelled exactly like it, or else one that differs
    /// only in case (the same file on a case-insensitive system).
    fn locate(&self, path: &str) -> Option<(String, String)> {
        let exact = slot(path);
        if self.map.contains_key(&exact) {
            return Some(exact);
        }
        let folded = exact.0;
        self.map
            .range((folded.clone(), String::new())..)
            .next()
            .filter(|((k, _), _)| *k == folded)
            .map(|(found, _)| found.clone())
    }

    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.locate(path).and_then(|s| self.map.get(&s))
    }

    /// Nothing is kept: no entry, no tag style and no unknown key.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty() && self.styles.is_empty() && self.extra.is_empty()
    }

    /// Every path with its entry, in folded-path order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Entry)> {
        self.map
            .iter()
            .map(|((_, path), entry)| (path.as_str(), entry))
    }

    /// Replaces the entry of a path; a blank entry removes it.
    pub(super) fn put(&mut self, path: &str, entry: Entry) {
        if path.is_empty() {
            return;
        }
        let at = self.locate(path).unwrap_or_else(|| slot(path));
        if entry.is_blank() {
            self.map.remove(&at);
        } else {
            self.map.insert(at, entry);
        }
    }

    /// The look kept for a tag (the name is normalized first).
    pub fn style(&self, tag: &str) -> Option<&TagStyle> {
        normalize_tag(tag).and_then(|tag| self.styles.get(&tag))
    }

    /// Every styled tag with its style, by name.
    pub fn styles(&self) -> impl Iterator<Item = (&str, &TagStyle)> {
        self.styles.iter().map(|(tag, style)| (tag.as_str(), style))
    }

    /// Gives a tag a color and an icon, `None` for none. A name that is not in the palette or the set
    /// means none, and a tag left with neither loses its style.
    pub fn set_style(&mut self, tag: &str, color: Option<&str>, icon: Option<&str>) {
        let Some(tag) = normalize_tag(tag) else {
            return;
        };
        let style = self
            .styles
            .get(&tag)
            .cloned()
            .unwrap_or_default()
            .with(color, icon);
        self.put_style(&tag, Some(style));
    }

    /// Replaces the style of a tag (`None`, or a blank one, removes it).
    pub(super) fn put_style(&mut self, tag: &str, style: Option<TagStyle>) {
        match style.filter(|s| !s.is_blank()) {
            Some(style) => {
                self.styles.insert(tag.to_owned(), style);
            }
            None => {
                self.styles.remove(tag);
            }
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
        self.locate(path)
            .is_some_and(|at| self.map.remove(&at).is_some())
    }

    /// Moves an entry to a new path (a renamed file). If the new path already has one, the two are
    /// merged. Returns whether anything moved.
    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        if key(from) == key(to) {
            return false;
        }
        let Some(moved) = self.locate(from).and_then(|at| self.map.remove(&at)) else {
            return false;
        };
        self.merge_entry(to, moved);
        true
    }

    /// Adds everything from `other`: tags are united, and `other`'s description is used only where this
    /// side has none. Tag styles and unknown top-level keys of `other` are added where this side has none.
    pub fn merge(&mut self, other: Entries) {
        for ((_, path), entry) in other.map {
            self.merge_entry(&path, entry);
        }
        for (tag, style) in other.styles {
            self.styles.entry(tag).or_insert(style);
        }
        for (k, v) in other.extra {
            self.extra.entry(k).or_insert(v);
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
        assert_ne!(problem.message, "");
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
    fn unknown_top_level_keys_and_tables_survive_a_rewrite() {
        let source = "\
version = 2
[tags.spec]
color = \"red\"
[files.\"a.md\"]
tags = [\"x\"]
[files.\"a.md\".review]
by = \"ana\"
";
        let mut e = Entries::parse(source).expect("parse");
        // an ordinary edit, as the app would make it
        e.set_tags("b.md", &tags(&["new"]));
        e.set_description("a.md", "changed");
        let text = e.to_toml();
        assert!(text.contains("version = 2"), "{text}");
        assert!(
            text.contains("[tags.spec]") && text.contains("color = \"red\""),
            "{text}"
        );
        assert!(text.contains("by = \"ana\""), "{text}");
        assert!(text.contains("description = \"changed\""), "{text}");
        // and nothing else changed on the way: reading it back gives the same store
        assert_eq!(Entries::parse(&text), Ok(e));
    }

    #[test]
    fn unknown_top_level_keys_are_kept_without_any_file_entry() {
        let mut e = Entries::parse("version = 3\n[tags.a]\nx = 1\n").expect("parse");
        assert!(!e.is_empty(), "the unknown keys count as content");
        e.set_tags("a.md", &tags(&["x"]));
        e.set_tags("a.md", &[]);
        let text = e.to_toml();
        assert!(
            text.contains("version = 3") && text.contains("[tags.a]"),
            "{text}"
        );
        assert!(!text.contains("files"), "{text}");
    }

    #[test]
    fn merging_brings_unknown_top_level_keys_along_without_overriding_the_receivers() {
        let mut base = Entries::parse("version = 2\n").expect("parse");
        let other = Entries::parse("version = 1\nextra = true\n").expect("parse");
        base.merge(other);
        let text = base.to_toml();
        assert!(
            text.contains("version = 2") && text.contains("extra = true"),
            "{text}"
        );
    }

    #[test]
    fn paths_that_differ_only_in_case_stay_two_entries() {
        let source =
            "[files.\"Notes.md\"]\ntags = [\"upper\"]\n[files.\"notes.md\"]\ntags = [\"lower\"]\n";
        let mut e = Entries::parse(source).expect("parse");
        assert_eq!(e.iter().count(), 2, "not collapsed on read");
        // an edit to an unrelated file writes both spellings back
        e.set_tags("other.md", &tags(&["x"]));
        let text = e.to_toml();
        assert!(
            text.contains("\"Notes.md\"") && text.contains("\"notes.md\""),
            "{text}"
        );
        assert!(
            text.contains("\"upper\"") && text.contains("\"lower\""),
            "{text}"
        );
        // the exact spelling is found first, whatever the system
        assert_eq!(
            e.get("Notes.md").map(|x| x.tags.clone()),
            Some(tags(&["upper"]))
        );
        assert_eq!(
            e.get("notes.md").map(|x| x.tags.clone()),
            Some(tags(&["lower"]))
        );
        // changing one leaves the other alone
        e.set_tags("notes.md", &tags(&["changed"]));
        assert_eq!(
            e.get("Notes.md").map(|x| x.tags.clone()),
            Some(tags(&["upper"]))
        );
        assert_eq!(
            e.get("notes.md").map(|x| x.tags.clone()),
            Some(tags(&["changed"]))
        );
        assert!(e.remove("Notes.md"));
        assert!(e.get("notes.md").is_some());
    }

    #[test]
    fn a_file_without_tag_styles_reads_with_none() {
        let e = Entries::parse("[files.\"a.md\"]\ntags = [\"x\"]\n").expect("parse");
        assert_eq!(e.styles().count(), 0);
        assert!(e.style("x").is_none());
        assert!(!e.to_toml().contains("tag_styles"));
    }

    #[test]
    fn tag_styles_are_written_in_the_documented_format_and_round_trip() {
        let mut e = Entries::default();
        e.set_tags("a.md", &tags(&["spec"]));
        e.set_style("spec", Some("blue"), Some("book-open"));
        e.set_style("Area/API", Some("teal"), None);
        e.set_style("idea", None, Some("lightbulb"));
        let text = e.to_toml();
        assert!(
            text.contains("[tag_styles.spec]\ncolor = \"blue\"\nicon = \"book-open\"\n"),
            "{text}"
        );
        assert!(
            text.contains("[tag_styles.\"area/api\"]\ncolor = \"teal\"\n"),
            "{text}"
        );
        assert!(
            text.contains("[tag_styles.idea]\nicon = \"lightbulb\"\n"),
            "{text}"
        );
        assert_eq!(Entries::parse(&text), Ok(e));
    }

    #[test]
    fn a_tag_left_with_neither_color_nor_icon_loses_its_style() {
        let mut e = Entries::default();
        e.set_style("spec", Some("red"), Some("flag"));
        e.set_style("spec", None, Some("flag"));
        assert_eq!(
            e.style("spec").and_then(TagStyle::view).map(|v| v.color),
            Some(None)
        );
        e.set_style("spec", None, None);
        assert!(e.is_empty(), "no style, nothing else: the store is empty");
        // names outside the palette and the set mean none
        e.set_style("spec", Some("chartreuse"), Some("rocket"));
        assert!(e.is_empty());
        e.set_style("   ", Some("red"), None);
        assert!(e.is_empty(), "a blank tag name is nobody's");
    }

    #[test]
    fn style_keys_are_read_as_tag_names_and_blank_styles_are_dropped() {
        let e = Entries::parse(
            "[tag_styles.\"Needs Review\"]\ncolor = \"Orange\"\n[tag_styles.empty]\n[tag_styles.\"  \"]\ncolor = \"red\"\n",
        )
        .expect("parse");
        assert_eq!(e.styles().count(), 1);
        let view = e
            .style("needs-review")
            .and_then(TagStyle::view)
            .expect("view");
        assert_eq!(view.color, Some("orange"));
        assert_eq!(e.style("Needs Review"), e.style("needs-review"));
    }

    #[test]
    fn unknown_colors_icons_and_keys_of_a_style_survive_a_rewrite_but_are_not_shown() {
        let source = "[tag_styles.spec]\ncolor = \"chartreuse\"\nicon = \"star\"\nweight = 3\n";
        let mut e = Entries::parse(source).expect("parse");
        let view = e.style("spec").and_then(TagStyle::view).expect("view");
        assert_eq!((view.color, view.icon), (None, Some("star")));
        // an ordinary edit elsewhere writes the whole style back
        e.set_tags("a.md", &tags(&["x"]));
        let text = e.to_toml();
        assert!(
            text.contains("chartreuse") && text.contains("weight = 3"),
            "{text}"
        );
        // choosing a look for that tag replaces its colour and icon, and leaves the other keys alone
        e.set_style("spec", Some("green"), None);
        let text = e.to_toml();
        assert!(
            text.contains("color = \"green\"") && text.contains("weight = 3"),
            "{text}"
        );
        assert!(
            !text.contains("chartreuse") && !text.contains("star"),
            "{text}"
        );
    }

    #[test]
    fn a_style_for_a_tag_no_file_has_is_harmless() {
        let mut e = Entries::default();
        e.set_style("ghost", Some("purple"), None);
        e.set_tags("a.md", &tags(&["x"]));
        e.set_tags("a.md", &[]);
        assert!(!e.is_empty(), "the style still counts as content");
        e.remove("a.md");
        assert!(e.style("ghost").is_some());
        assert_eq!(Entries::parse(&e.to_toml()), Ok(e));
    }

    #[test]
    fn wrong_types_in_a_style_are_reported_like_any_other_bad_edit() {
        let problem =
            Entries::parse("[tag_styles.spec]\ncolor = 5\n").expect_err("a number is no color");
        assert_eq!(problem.line, Some(2));
        assert!(Entries::parse("tag_styles = 3").is_err());
    }

    #[test]
    fn merging_brings_styles_along_without_overriding_the_receivers() {
        let mut base = Entries::default();
        base.set_style("spec", Some("blue"), None);
        let mut other = Entries::default();
        other.set_style("spec", Some("red"), Some("flag"));
        other.set_style("idea", Some("yellow"), None);
        base.merge(other);
        assert_eq!(
            base.style("spec").and_then(TagStyle::view).map(|v| v.color),
            Some(Some("blue"))
        );
        assert_eq!(
            base.style("spec").and_then(TagStyle::view).map(|v| v.icon),
            Some(None)
        );
        assert!(base.style("idea").is_some());
    }

    #[test]
    fn lookups_follow_the_file_systems_case_rules() {
        let mut e = Entries::default();
        e.set_tags("Docs/A.md", &tags(&["x"]));
        assert_eq!(e.get("Docs/A.md").map(|x| x.tags.len()), Some(1));
        assert_eq!(e.get("docs/a.md").is_some(), crate::vault::CASE_INSENSITIVE);
        assert!(e.remove("Docs/A.md"));
        assert!(e.is_empty());
    }
}
