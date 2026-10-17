//! The tags and descriptions an agent narrows a large project down with: which tags the documents carry and how many
//! documents have each, the documents that have some tags, and the tags and description to show beside a search hit.
//!
//! Plain data in, answers out. The window builds the [`DocIndex`] from the metadata it keeps live; `permark mcp`
//! builds it from the project's files when the window is closed (see `Project::docs`), so an agent reads the same
//! shapes either way.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::Serialize;

use crate::meta::normalize::{normalize_tag, normalize_tags};
use crate::meta::store::Problem;
use crate::search::indexer::is_indexable;
use crate::vault::WORKTREE_PREFIX;
use crate::watch::{PathFilter, Verdict};

use super::protocol::{
    Hit, DEFAULT_TAGGED_LISTED, DEFAULT_TAGS_LISTED, MAX_TAGGED_LISTED, MAX_TAGS_LISTED,
};

/// How many tags a list gives: what the client asked for, within bounds.
pub(crate) fn tags_limit(asked: Option<usize>) -> usize {
    asked
        .unwrap_or(DEFAULT_TAGS_LISTED)
        .clamp(1, MAX_TAGS_LISTED)
}

/// How many documents a list of tagged documents gives: what the client asked for, within bounds.
pub(crate) fn docs_limit(asked: Option<usize>) -> usize {
    asked
        .unwrap_or(DEFAULT_TAGGED_LISTED)
        .clamp(1, MAX_TAGGED_LISTED)
}

// serde's `skip_serializing_if` gives the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Whether an agent may be told about the file `rel` of a project: a Markdown document in the project's own folder that
/// the project's ignore rules leave in. These are the documents the search index holds, so a count of documents here
/// is a count of what `search` can find.
pub(crate) fn is_document(filter: &PathFilter, rel: &str) -> bool {
    is_indexable(rel)
        && !rel.starts_with(WORKTREE_PREFIX)
        && filter.classify(0, rel, false) == Verdict::Emit
}

/// What an agent is told beside a list when the tags added in PerMark could not be read (see `Entries::read_file`).
pub(crate) fn unreadable_meta(problem: &Problem) -> String {
    format!(
        "{problem}. Only the tags written in the documents' own front matter are listed; the tags added in PerMark are not."
    )
}

/// Whether the tags an agent gave say anything once cleaned up (`" "` and `""` do not).
pub(crate) fn names_a_tag(raw: &[String]) -> bool {
    !normalize_tags(raw).is_empty()
}

/// One Markdown document of the project and what is known about its tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Doc {
    pub path: String,
    /// Written in the document's front matter.
    pub file_tags: Vec<String>,
    /// Added in PerMark (kept in `meta.toml`).
    pub app_tags: Vec<String>,
    pub description: String,
}

impl Doc {
    /// Every tag of the document, once, alphabetical.
    fn tags(&self) -> BTreeSet<&str> {
        self.file_tags
            .iter()
            .chain(&self.app_tags)
            .map(String::as_str)
            .collect()
    }

    fn has_all(&self, wanted: &[String]) -> bool {
        wanted
            .iter()
            .all(|tag| self.file_tags.contains(tag) || self.app_tags.contains(tag))
    }

    fn is_untagged(&self) -> bool {
        self.file_tags.is_empty() && self.app_tags.is_empty()
    }
}

/// Where a tag is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TagOrigin {
    /// In the documents' own front matter.
    FrontMatter,
    /// Added in PerMark.
    Permark,
    /// Both: on some documents (or the same ones) in each place.
    Both,
}

/// A tag with the number of documents that have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TagCount {
    pub name: String,
    pub count: usize,
    pub from: TagOrigin,
}

/// What `list_tags` answers without a tag: the distribution of the tags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TagOverview {
    /// How many Markdown documents the project has.
    pub documents: usize,
    /// How many of them have no tag at all.
    pub untagged: usize,
    /// Most documents first, then by name: the first [`DEFAULT_TAGS_LISTED`] unless the client asked for another number.
    pub tags: Vec<TagCount>,
    /// How many more tags there are than `tags` lists. Left out when none.
    #[serde(skip_serializing_if = "is_zero")]
    pub more: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// A document found by its tags, as an agent reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaggedDoc {
    pub path: String,
    /// The one-line description. Left out when the document has none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// All of its tags, alphabetical.
    pub tags: Vec<String>,
}

/// The documents that have some tags: what `list_tags` answers for a tag, and `search` for tags without a query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaggedDocs {
    /// How many documents have all the tags.
    pub total: usize,
    /// By path: the first [`DEFAULT_TAGGED_LISTED`] unless the client asked for another number.
    pub docs: Vec<TaggedDoc>,
    /// How many more there are than `docs` lists. Left out when none.
    #[serde(skip_serializing_if = "is_zero")]
    pub more: usize,
    /// Said when no document has the tags.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// What `list_tags` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(crate) enum TagsAnswer {
    Overview(TagOverview),
    Docs(TaggedDocs),
}

/// The Markdown documents of a project with their tags and descriptions, sorted by path.
#[derive(Debug, Clone, Default)]
pub(crate) struct DocIndex {
    docs: Vec<Doc>,
    /// Said beside the answers when some tags could not be read, since the lists are then shorter than they should be.
    warning: Option<String>,
}

impl DocIndex {
    pub fn new(mut docs: Vec<Doc>, warning: Option<String>) -> Self {
        docs.sort_by(|a, b| a.path.cmp(&b.path));
        Self { docs, warning }
    }

    /// Keeps the documents `keep` says yes to.
    pub fn retain(&mut self, keep: impl FnMut(&Doc) -> bool) {
        self.docs.retain(keep);
    }

    /// The tags in use, with how many documents have each: the `limit` most used (see [`tags_limit`]).
    pub fn overview(&self, limit: Option<usize>) -> TagOverview {
        // (documents, in a front matter, added in PerMark)
        let mut counts: BTreeMap<&str, (usize, bool, bool)> = BTreeMap::new();
        for doc in &self.docs {
            for tag in doc.tags() {
                let slot = counts.entry(tag).or_default();
                slot.0 += 1;
                slot.1 |= doc.file_tags.iter().any(|t| t == tag);
                slot.2 |= doc.app_tags.iter().any(|t| t == tag);
            }
        }
        let mut tags: Vec<TagCount> = counts
            .into_iter()
            .map(|(name, (count, file, app))| TagCount {
                name: name.to_owned(),
                count,
                from: match (file, app) {
                    (true, true) => TagOrigin::Both,
                    (true, false) => TagOrigin::FrontMatter,
                    (false, _) => TagOrigin::Permark,
                },
            })
            .collect();
        tags.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        let more = tags.len().saturating_sub(tags_limit(limit));
        tags.truncate(tags_limit(limit));
        TagOverview {
            documents: self.docs.len(),
            untagged: self.docs.iter().filter(|doc| doc.is_untagged()).count(),
            tags,
            more,
            warning: self.warning.clone(),
        }
    }

    /// The documents that have all of `raw` (tags are compared the way PerMark writes them: `Needs Review` is
    /// `needs-review`), the first `limit` of them (see [`docs_limit`]). No tag selects nothing.
    pub fn tagged(&self, raw: &[String], limit: Option<usize>) -> TaggedDocs {
        let wanted = normalize_tags(raw);
        let found: Vec<&Doc> = self
            .docs
            .iter()
            .filter(|doc| !wanted.is_empty() && doc.has_all(&wanted))
            .collect();
        let total = found.len();
        let docs: Vec<TaggedDoc> = found
            .into_iter()
            .take(docs_limit(limit))
            .map(|doc| TaggedDoc {
                path: doc.path.clone(),
                description: doc.description.clone(),
                tags: doc.tags().into_iter().map(str::to_owned).collect(),
            })
            .collect();
        let message = (total == 0).then(|| {
            let (which, names) = if let [only] = wanted.as_slice() {
                ("the tag", only.clone())
            } else {
                ("all of these tags", wanted.join(", "))
            };
            format!(
                "No document has {which}: {names}. Call list_tags to see the tags in use, and how many documents have each."
            )
        });
        TaggedDocs {
            total,
            more: total - docs.len(),
            docs,
            message,
            warning: self.warning.clone(),
        }
    }

    /// What `list_tags` answers: the tags in use, or the documents that have `tag`, at most `limit` of either. A blank
    /// tag is no tag.
    pub fn list_tags(&self, tag: Option<&str>, limit: Option<usize>) -> TagsAnswer {
        match tag.and_then(normalize_tag) {
            Some(tag) => TagsAnswer::Docs(self.tagged(&[tag], limit)),
            None => TagsAnswer::Overview(self.overview(limit)),
        }
    }

    /// Whether a document (by its path) has all of `raw`: always, when no tag is named.
    pub fn allowing<'a>(&'a self, raw: &[String]) -> Box<dyn Fn(&str) -> bool + 'a> {
        let wanted = normalize_tags(raw);
        if wanted.is_empty() {
            return Box::new(|_| true);
        }
        let allowed: HashSet<&str> = self
            .docs
            .iter()
            .filter(|doc| doc.has_all(&wanted))
            .map(|doc| doc.path.as_str())
            .collect();
        Box::new(move |path| allowed.contains(path))
    }

    /// The hits with the tags and the description of the document each is in.
    pub fn describe(&self, hits: Vec<Hit>) -> Vec<Hit> {
        hits.into_iter()
            .map(|mut hit| {
                let found = self
                    .docs
                    .binary_search_by(|doc| doc.path.as_str().cmp(&hit.path))
                    .ok()
                    .map(|at| &self.docs[at]);
                if let Some(doc) = found {
                    hit.tags = doc.tags().into_iter().map(str::to_owned).collect();
                    hit.description.clone_from(&doc.description);
                }
                hit
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_owned()).collect()
    }

    fn doc(path: &str, file: &[&str], app: &[&str], description: &str) -> Doc {
        Doc {
            path: path.to_owned(),
            file_tags: s(file),
            app_tags: s(app),
            description: description.to_owned(),
        }
    }

    fn sample() -> DocIndex {
        DocIndex::new(
            vec![
                doc("plan.md", &[], &[], ""),
                doc(
                    "docs/live-feed.md",
                    &["feed"],
                    &["spec", "needs-review"],
                    "The feed.",
                ),
                doc("docs/watcher.md", &["feed", "watcher"], &["spec"], ""),
                doc("docs/notes.md", &[], &[], "Just a note."),
                doc("docs/overlap.md", &["feed"], &["feed"], "Both places."),
            ],
            None,
        )
    }

    fn hit(path: &str) -> Hit {
        Hit {
            path: path.to_owned(),
            heading_path: Vec::new(),
            line: 1,
            snippet: String::new(),
            tags: Vec::new(),
            description: String::new(),
        }
    }

    #[test]
    fn the_overview_counts_documents_per_tag_most_first_and_says_where_each_is_written() {
        let overview = sample().overview(None);
        assert_eq!(overview.documents, 5);
        assert_eq!(overview.untagged, 2);
        assert_eq!(overview.more, 0);
        let got: Vec<(&str, usize, TagOrigin)> = overview
            .tags
            .iter()
            .map(|t| (t.name.as_str(), t.count, t.from))
            .collect();
        assert_eq!(
            got,
            [
                ("feed", 3, TagOrigin::Both),
                ("spec", 2, TagOrigin::Permark),
                ("needs-review", 1, TagOrigin::Permark),
                ("watcher", 1, TagOrigin::FrontMatter),
            ]
        );
    }

    #[test]
    fn a_tag_written_in_both_places_of_one_document_counts_the_document_once() {
        let index = DocIndex::new(vec![doc("a.md", &["x"], &["x"], "")], None);
        let overview = index.overview(None);
        assert_eq!(overview.tags.len(), 1);
        assert_eq!(
            (overview.tags[0].count, overview.tags[0].from),
            (1, TagOrigin::Both)
        );
    }

    #[test]
    fn the_overview_of_a_project_with_no_tags_says_so_with_numbers() {
        let index = DocIndex::new(
            vec![doc("a.md", &[], &[], ""), doc("b.md", &[], &[], "")],
            None,
        );
        let overview = index.overview(None);
        assert_eq!(
            (overview.documents, overview.untagged, overview.tags.len()),
            (2, 2, 0)
        );
        assert_eq!(overview.tags, Vec::<TagCount>::new());
        let empty = DocIndex::default().overview(None);
        assert_eq!((empty.documents, empty.untagged), (0, 0));
    }

    #[test]
    fn a_long_tag_list_is_cut_to_a_few_and_says_how_many_more_there_are() {
        let many: Vec<Doc> = (0..MAX_TAGS_LISTED + 5)
            .map(|n| doc(&format!("d{n}.md"), &[], &[&format!("t{n:03}")], ""))
            .collect();
        let index = DocIndex::new(many, None);
        let overview = index.overview(None);
        assert_eq!(overview.tags.len(), DEFAULT_TAGS_LISTED);
        assert_eq!(overview.more, MAX_TAGS_LISTED + 5 - DEFAULT_TAGS_LISTED);
        assert_eq!(overview.documents, MAX_TAGS_LISTED + 5);
        // the client can ask for more or for fewer, within bounds
        assert_eq!(index.overview(Some(5)).tags.len(), 5);
        assert_eq!(index.overview(Some(5)).more, MAX_TAGS_LISTED);
        assert_eq!(index.overview(Some(10_000)).tags.len(), MAX_TAGS_LISTED);
        assert_eq!(index.overview(Some(10_000)).more, 5);
        assert_eq!(index.overview(Some(0)).tags.len(), 1);
        // and says nothing of more when it listed them all
        let json = serde_json::to_value(index.overview(Some(MAX_TAGS_LISTED))).expect("json");
        assert_eq!(json["more"], 5);
        let all = DocIndex::new(vec![doc("a.md", &["x"], &[], "")], None);
        assert!(serde_json::to_value(all.overview(None))
            .expect("json")
            .get("more")
            .is_none());
    }

    #[test]
    fn several_tags_must_all_match_and_the_input_is_written_the_way_tags_are() {
        let index = sample();
        let paths = |tags: &[&str]| -> Vec<String> {
            index
                .tagged(&s(tags), None)
                .docs
                .into_iter()
                .map(|d| d.path)
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
    fn a_found_document_carries_its_description_and_all_its_tags() {
        let found = sample().tagged(&s(&["needs-review"]), None);
        assert_eq!((found.total, found.more), (1, 0));
        assert_eq!(found.message, None);
        assert_eq!(
            found.docs,
            [TaggedDoc {
                path: "docs/live-feed.md".to_owned(),
                description: "The feed.".to_owned(),
                tags: s(&["feed", "needs-review", "spec"]),
            }]
        );
    }

    #[test]
    fn a_long_list_of_documents_is_cut_to_a_few_and_says_how_many_more_there_are() {
        let many: Vec<Doc> = (0..MAX_TAGGED_LISTED + 7)
            .map(|n| doc(&format!("d{n:03}.md"), &[], &["adr"], ""))
            .collect();
        let index = DocIndex::new(many, None);
        let found = index.tagged(&s(&["adr"]), None);
        assert_eq!(found.docs.len(), DEFAULT_TAGGED_LISTED);
        assert_eq!(found.total, MAX_TAGGED_LISTED + 7);
        assert_eq!(found.more, MAX_TAGGED_LISTED + 7 - DEFAULT_TAGGED_LISTED);
        assert_eq!(found.docs[0].path, "d000.md");
        // more can be asked for, up to a bound
        let more = index.tagged(&s(&["adr"]), Some(1_000));
        assert_eq!(more.docs.len(), MAX_TAGGED_LISTED);
        assert_eq!(more.more, 7);
        assert_eq!(index.tagged(&s(&["adr"]), Some(3)).docs.len(), 3);
        // a list that is whole says nothing of more
        let whole = serde_json::to_value(sample().tagged(&s(&["spec"]), None)).expect("json");
        assert!(whole.get("more").is_none());
    }

    #[test]
    fn no_document_with_the_tags_is_said_in_words_the_agent_can_act_on() {
        let one = sample().tagged(&s(&["Missing Tag"]), None);
        assert_eq!(one.total, 0);
        let message = one.message.expect("a message");
        assert!(message.contains("the tag: missing-tag"), "{message}");
        assert!(message.contains("list_tags"), "{message}");
        let two = sample().tagged(&s(&["spec", "nope"]), None);
        assert!(two
            .message
            .expect("a message")
            .contains("all of these tags: spec, nope"));
    }

    #[test]
    fn list_tags_answers_the_distribution_or_the_documents_of_one_tag() {
        let index = sample();
        assert!(matches!(
            index.list_tags(None, None),
            TagsAnswer::Overview(_)
        ));
        // a blank tag is no tag
        assert!(matches!(
            index.list_tags(Some("  "), None),
            TagsAnswer::Overview(_)
        ));
        match index.list_tags(Some("Spec"), None) {
            TagsAnswer::Docs(found) => assert_eq!(found.total, 2),
            TagsAnswer::Overview(_) => panic!("a tag lists its documents"),
        }
        let overview = serde_json::to_value(index.list_tags(None, None)).expect("json");
        assert_eq!(overview["documents"], 5);
        assert_eq!(
            overview["tags"][0],
            json!({"name": "feed", "count": 3, "from": "both"})
        );
        assert!(overview.get("warning").is_none());
        let docs = serde_json::to_value(index.list_tags(Some("watcher"), None)).expect("json");
        assert_eq!(
            docs["docs"][0],
            json!({"path": "docs/watcher.md", "tags": ["feed", "spec", "watcher"]}),
            "an empty description is left out"
        );
        assert_eq!(docs["total"], 1);
        assert!(docs.get("more").is_none() && docs.get("message").is_none());
    }

    #[test]
    fn a_warning_is_said_beside_every_list_it_could_shorten() {
        let index = DocIndex::new(
            vec![doc("a.md", &["x"], &[], "")],
            Some("trouble".to_owned()),
        );
        let overview = serde_json::to_value(index.list_tags(None, None)).expect("json");
        let docs = serde_json::to_value(index.list_tags(Some("x"), None)).expect("json");
        assert_eq!(overview["warning"], "trouble");
        assert_eq!(docs["warning"], "trouble");
    }

    #[test]
    fn hits_are_narrowed_to_the_documents_that_have_all_the_tags() {
        let index = sample();
        let allow = index.allowing(&s(&["spec", "feed"]));
        assert!(allow("docs/live-feed.md") && allow("docs/watcher.md"));
        assert!(!allow("docs/notes.md") && !allow("elsewhere.md"));
        let everything = index.allowing(&[]);
        assert!(everything("anything.md"));
        assert!(index.allowing(&s(&[" "]))("anything.md"));
    }

    #[test]
    fn hits_carry_the_tags_and_description_of_their_document() {
        let described = sample().describe(vec![
            hit("docs/live-feed.md"),
            hit("not-in-the-index.md"),
            hit("plan.md"),
        ]);
        assert_eq!(described[0].tags, ["feed", "needs-review", "spec"]);
        assert_eq!(described[0].description, "The feed.");
        assert_eq!(described[1].tags, Vec::<String>::new());
        assert_eq!(described[1].description, "");
        assert_eq!(described[2].tags, Vec::<String>::new());
    }

    #[test]
    fn documents_can_be_taken_out_of_the_index() {
        let mut index = sample();
        index.retain(|doc| !doc.path.starts_with("docs/"));
        assert_eq!(index.overview(None).documents, 1);
        assert_eq!(index.overview(None).tags, Vec::<TagCount>::new());
    }

    #[test]
    fn only_markdown_the_project_keeps_is_a_document() {
        let filter = PathFilter::new(&[(PathBuf::from("/project"), None)], &s(&["drafts/"]));
        for yes in ["README.md", "docs/plan.markdown", "a/b/C.MD"] {
            assert!(is_document(&filter, yes), "{yes}");
        }
        for no in [
            "notes.txt",
            "data.json",
            "drafts/idea.md",
            "node_modules/pkg/README.md",
            "@wt/feature/docs/plan.md",
            "plan.md.bak",
        ] {
            assert!(!is_document(&filter, no), "{no}");
        }
    }

    #[test]
    fn tags_that_say_nothing_are_told_apart() {
        assert!(names_a_tag(&s(&["adr"])));
        assert!(names_a_tag(&s(&["", " Needs Review "])));
        assert!(!names_a_tag(&s(&["", "  "])));
        assert!(!names_a_tag(&[]));
    }
}
