//! The project's files with their tags: counting tags, finding the files that have some, and the
//! combined tag list of one file. Pure data in, answers out; the scan that builds it is in the service.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::normalize::normalize_tags;

/// One tag of a file. A tag in both places is listed once, as a file tag (`file`), and `app` says
/// whether the app has it too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagRef {
    pub name: String,
    /// Written in the file's front matter: the app cannot change it.
    pub file: bool,
    /// Added in the app (stored in `meta.toml`).
    pub app: bool,
}

/// The tags of one file: the front matter's first, then the ones only the app has.
pub fn combine_tags(file_tags: &[String], app_tags: &[String]) -> Vec<TagRef> {
    let mut out: Vec<TagRef> = file_tags
        .iter()
        .map(|name| TagRef {
            name: name.clone(),
            file: true,
            app: app_tags.contains(name),
        })
        .collect();
    out.extend(
        app_tags
            .iter()
            .filter(|name| !file_tags.contains(name))
            .map(|name| TagRef {
                name: name.clone(),
                file: false,
                app: true,
            }),
    );
    out
}

/// A file of the project with everything known about its tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    pub app_tags: Vec<String>,
    pub file_tags: Vec<String>,
    pub description: String,
    pub modified_ms: u64,
}

impl Row {
    /// Every tag of the file, once, in alphabetical order.
    fn tags(&self) -> BTreeSet<&str> {
        self.file_tags
            .iter()
            .chain(&self.app_tags)
            .map(String::as_str)
            .collect()
    }

    fn has_tag(&self, tag: &str) -> bool {
        self.file_tags
            .iter()
            .chain(&self.app_tags)
            .any(|t| t == tag)
    }

    fn is_untagged(&self) -> bool {
        self.file_tags.is_empty() && self.app_tags.is_empty()
    }
}

/// A tag with the number of files that have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub name: String,
    pub count: usize,
    /// No file has it from the app: it exists only in front matter.
    pub only_in_files: bool,
}

/// A file found by its tags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaggedFile {
    pub path: String,
    pub description: String,
    pub modified_ms: u64,
}

/// What the UI needs to show tags and descriptions next to a file (only files that have some).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub path: String,
    pub tags: Vec<String>,
    pub description: String,
    pub modified_ms: u64,
}

/// All the files of the project (sorted by path).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    rows: Vec<Row>,
}

impl Catalog {
    pub fn new(mut rows: Vec<Row>) -> Self {
        rows.sort_by(|a, b| a.path.cmp(&b.path));
        Self { rows }
    }

    /// Tags by number of files, most files first, then by name.
    pub fn tag_counts(&self) -> Vec<TagCount> {
        let mut counts: BTreeMap<&str, (usize, bool)> = BTreeMap::new();
        for row in &self.rows {
            for tag in row.tags() {
                let slot = counts.entry(tag).or_insert((0, true));
                slot.0 += 1;
                slot.1 &= !row.app_tags.iter().any(|t| t == tag);
            }
        }
        let mut out: Vec<TagCount> = counts
            .into_iter()
            .map(|(name, (count, only_in_files))| TagCount {
                name: name.to_owned(),
                count,
                only_in_files,
            })
            .collect();
        out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        out
    }

    /// Files without any tag.
    pub fn untagged_count(&self) -> usize {
        self.rows.iter().filter(|r| r.is_untagged()).count()
    }

    /// The files that have every one of `tags`; or, with `untagged`, the files that have none.
    /// No tags and not `untagged` selects nothing.
    pub fn files_with(&self, tags: &[String], untagged: bool) -> Vec<TaggedFile> {
        let wanted = normalize_tags(tags);
        if !untagged && wanted.is_empty() {
            return Vec::new();
        }
        self.rows
            .iter()
            .filter(|r| {
                if untagged {
                    r.is_untagged()
                } else {
                    wanted.iter().all(|t| r.has_tag(t))
                }
            })
            .map(|r| TaggedFile {
                path: r.path.clone(),
                description: r.description.clone(),
                modified_ms: r.modified_ms,
            })
            .collect()
    }

    /// The files that have a tag or a description.
    pub fn index(&self) -> Vec<IndexEntry> {
        self.rows
            .iter()
            .filter(|r| !r.is_untagged() || !r.description.is_empty())
            .map(|r| IndexEntry {
                path: r.path.clone(),
                tags: r.tags().into_iter().map(str::to_owned).collect(),
                description: r.description.clone(),
                modified_ms: r.modified_ms,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_owned()).collect()
    }

    fn row(path: &str, app: &[&str], file: &[&str], description: &str) -> Row {
        Row {
            path: path.to_owned(),
            app_tags: s(app),
            file_tags: s(file),
            description: description.to_owned(),
            modified_ms: 7,
        }
    }

    fn sample() -> Catalog {
        Catalog::new(vec![
            row("plan.md", &[], &[], ""),
            row(
                "docs/live-feed.md",
                &["spec", "needs-review"],
                &["feed"],
                "The feed.",
            ),
            row("docs/watcher.md", &["spec"], &["feed", "watcher"], ""),
            row("docs/notes.md", &[], &[], "Just a note."),
            row("docs/overlap.md", &["feed"], &["feed"], ""),
        ])
    }

    #[test]
    fn a_tag_in_both_places_is_listed_once_as_a_file_tag() {
        let tags = combine_tags(&s(&["feed", "watcher"]), &s(&["spec", "feed"]));
        let got: Vec<(&str, bool, bool)> = tags
            .iter()
            .map(|t| (t.name.as_str(), t.file, t.app))
            .collect();
        assert_eq!(
            got,
            [
                ("feed", true, true),
                ("watcher", true, false),
                ("spec", false, true)
            ]
        );
        assert!(combine_tags(&[], &[]).is_empty());
    }

    #[test]
    fn counts_files_per_tag_most_first_and_flags_file_only_tags() {
        let counts = sample().tag_counts();
        let got: Vec<(&str, usize, bool)> = counts
            .iter()
            .map(|t| (t.name.as_str(), t.count, t.only_in_files))
            .collect();
        assert_eq!(
            got,
            [
                ("feed", 3, false), // the app also has it on one file
                ("spec", 2, false),
                ("needs-review", 1, false),
                ("watcher", 1, true),
            ]
        );
    }

    #[test]
    fn a_tag_present_in_both_places_on_one_file_counts_that_file_once() {
        let c = Catalog::new(vec![row("a.md", &["x"], &["x"], "")]);
        assert_eq!(c.tag_counts().len(), 1);
        assert_eq!(c.tag_counts()[0].count, 1);
    }

    #[test]
    fn untagged_files_are_counted_and_listed() {
        let c = sample();
        assert_eq!(c.untagged_count(), 2);
        let paths: Vec<String> = c
            .files_with(&[], true)
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(paths, ["docs/notes.md", "plan.md"]);
    }

    #[test]
    fn several_tags_must_all_match_and_the_input_is_normalized() {
        let c = sample();
        let paths = |tags: &[&str]| -> Vec<String> {
            c.files_with(&s(tags), false)
                .into_iter()
                .map(|f| f.path)
                .collect()
        };
        assert_eq!(paths(&["spec"]), ["docs/live-feed.md", "docs/watcher.md"]);
        assert_eq!(
            paths(&["SPEC", "feed"]),
            ["docs/live-feed.md", "docs/watcher.md"]
        );
        assert_eq!(paths(&["spec", "needs review"]), ["docs/live-feed.md"]);
        assert_eq!(paths(&["spec", "nope"]), Vec::<String>::new());
        assert_eq!(paths(&[]), Vec::<String>::new());
        assert_eq!(paths(&["  "]), Vec::<String>::new());
    }

    #[test]
    fn found_files_carry_their_description_and_time() {
        let found = sample().files_with(&s(&["needs-review"]), false);
        assert_eq!(
            found,
            [TaggedFile {
                path: "docs/live-feed.md".to_owned(),
                description: "The feed.".to_owned(),
                modified_ms: 7,
            }]
        );
    }

    #[test]
    fn the_index_lists_files_with_a_tag_or_a_description() {
        let index = sample().index();
        let paths: Vec<&str> = index.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "docs/live-feed.md",
                "docs/notes.md",
                "docs/overlap.md",
                "docs/watcher.md"
            ]
        );
        let live = &index[0];
        assert_eq!(live.tags, ["feed", "needs-review", "spec"]);
        assert_eq!(live.description, "The feed.");
        assert_eq!(live.modified_ms, 7);
        assert_eq!(index[1].tags, Vec::<String>::new());
    }

    #[test]
    fn serializes_for_the_ui() {
        let json = serde_json::to_value(TagCount {
            name: "x".to_owned(),
            count: 2,
            only_in_files: true,
        })
        .expect("json");
        assert_eq!(
            json,
            serde_json::json!({"name": "x", "count": 2, "onlyInFiles": true})
        );
    }
}
