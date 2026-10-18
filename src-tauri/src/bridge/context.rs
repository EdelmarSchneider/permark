//! What the person is looking at, as an agent sees it: the open document, the mode, the section on screen, the
//! selection, and whether there are unsaved edits.
//!
//! The window publishes this (debounced); the program keeps the latest and answers `get_context` from it, and writes
//! it to `.permark/local/agent.json` for tools that do not speak MCP. Everything here is plain data with no
//! window: [`AgentContext::sanitize`] is the one place that decides what leaves the program, so a document outside
//! the project or an oversized selection never reaches an agent, whatever the window sent.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::vault::{Vault, WORKTREE_PREFIX};

/// A selection longer than this (in characters) is cut, and says so. One constant to change.
pub const MAX_SELECTION_CHARS: usize = 4_000;
/// A heading longer than this (in characters) is cut.
const MAX_HEADING_CHARS: usize = 200;
/// A heading path deeper than this keeps its last entries (a document has six levels; this is generous).
const MAX_HEADING_DEPTH: usize = 12;
/// How much of a selection the one-paragraph summary quotes.
const SUMMARY_QUOTE_CHARS: usize = 240;

/// How the document is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewMode {
    /// The reading view.
    Reading,
    /// The Visual editor.
    Visual,
    /// The Source editor.
    Source,
}

/// The section on screen: its heading path and the lines it spans (1-based, both included). The lines are missing
/// when the editor cannot tell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub heading_path: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
}

/// The text the person selected, with where it is. The lines are missing when the editor cannot tell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub text: String,
    /// The text was longer than [`MAX_SELECTION_CHARS`] and was cut.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    #[serde(default)]
    pub heading_path: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
}

/// What the window shows, in the form the window publishes and an agent receives.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentContext {
    /// The open document, relative to the project with `/` separators; `None` when none is open.
    #[serde(default)]
    pub document: Option<String>,
    #[serde(default)]
    pub mode: Option<ViewMode>,
    #[serde(default)]
    pub section: Option<Section>,
    #[serde(default)]
    pub selection: Option<Selection>,
    /// The editor holds changes that are not saved to the file yet.
    #[serde(default)]
    pub unsaved: bool,
}

/// A context with the time it was published (`updatedAtMs`, milliseconds since the Unix epoch) and how many
/// comments are open (`openComments`, counted by the program when this is answered or written, not by the
/// window): the answer of `get_context` and the content of `agent.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Published {
    #[serde(flatten)]
    pub context: AgentContext,
    pub updated_at_ms: u64,
    /// How many of the person's comments are open, awaiting an agent. Old files and older windows say 0.
    #[serde(default)]
    pub open_comments: u32,
}

impl Published {
    /// The context as one short paragraph (see [`AgentContext::summary`]), with how many comments are open
    /// joined to the same sentence when there are any, so a hook points the agent at `list_comments`.
    #[must_use]
    pub fn summary_with_comments(&self) -> String {
        let mut out = self.context.summary();
        if self.open_comments > 0 {
            // the summary is one sentence that ends with a period; the count joins it before the period
            out.truncate(out.len() - 1);
            let noun = if self.open_comments == 1 {
                "comment"
            } else {
                "comments"
            };
            let _ = write!(out, ", {} {} open.", self.open_comments, noun);
        }
        out
    }
}

/// Cuts `text` to `max` characters (not bytes), at a character boundary. Returns whether it was cut.
fn cut(text: &mut String, max: usize) -> bool {
    match text.char_indices().nth(max) {
        Some((at, _)) => {
            text.truncate(at);
            true
        }
        None => false,
    }
}

/// The headings above a passage, each on one line (a heading that wraps, or text taken from the page with its line
/// breaks, is one line of words: the paragraph a hook prints must stay one line), cut, without blanks, and no deeper
/// than [`MAX_HEADING_DEPTH`].
fn clean_path(path: Vec<String>) -> Vec<String> {
    let mut path: Vec<String> = path
        .into_iter()
        .map(|heading| {
            let mut heading = heading.split_whitespace().collect::<Vec<_>>().join(" ");
            cut(&mut heading, MAX_HEADING_CHARS);
            heading.trim().to_owned()
        })
        .filter(|heading| !heading.is_empty())
        .collect();
    if path.len() > MAX_HEADING_DEPTH {
        path.drain(..path.len() - MAX_HEADING_DEPTH);
    }
    path
}

/// A line number from a window: 1-based, so zero is nothing.
fn line(value: Option<u32>) -> Option<u32> {
    value.filter(|n| *n >= 1)
}

/// A range whose ends are in order; a range that is not is dropped.
fn range(start: Option<u32>, end: Option<u32>) -> (Option<u32>, Option<u32>) {
    match (line(start), line(end)) {
        (Some(s), Some(e)) if s <= e => (Some(s), Some(e)),
        (Some(s), None) => (Some(s), None),
        _ => (None, None),
    }
}

impl AgentContext {
    /// What may be shared: the document must be an existing file inside the project (a linked worktree lives
    /// outside it and is left out); text is cut to its limits; lines are in order. The window's word is never
    /// trusted for a path.
    #[must_use]
    pub fn sanitize(mut self, vault: &Vault) -> Self {
        let document = self.document.take().and_then(|path| {
            if path.starts_with(WORKTREE_PREFIX) {
                return None;
            }
            let resolved = vault.resolve(&path).ok()?;
            (resolved.root == 0 && resolved.abs.is_file()).then_some(resolved.rel)
        });
        let Some(document) = document else {
            return Self::default();
        };
        self.document = Some(document);
        self.section = self.section.take().and_then(|section| {
            let heading_path = clean_path(section.heading_path);
            let (start_line, end_line) = range(section.start_line, section.end_line);
            // A section that says neither where it is nor what it is called says nothing.
            (!heading_path.is_empty() || start_line.is_some()).then_some(Section {
                heading_path,
                start_line,
                end_line,
            })
        });
        self.selection = self.selection.take().and_then(|mut selection| {
            if selection.text.trim().is_empty() {
                return None;
            }
            selection.truncated = cut(&mut selection.text, MAX_SELECTION_CHARS);
            selection.heading_path = clean_path(selection.heading_path);
            let (start, end) = range(selection.start_line, selection.end_line);
            selection.start_line = start;
            selection.end_line = end;
            Some(selection)
        });
        self
    }

    /// The context as one short paragraph, for a hook or a prompt:
    /// `Viewing docs/plan.md › Rollout (lines 40-62), selected: '…'.`
    #[must_use]
    pub fn summary(&self) -> String {
        let Some(document) = &self.document else {
            return "PerMark is open on this project, with no document open.".to_owned();
        };
        let verb = match self.mode {
            Some(ViewMode::Visual) => "Editing (visual)",
            Some(ViewMode::Source) => "Editing (source)",
            Some(ViewMode::Reading) | None => "Viewing",
        };
        let mut out = format!("{verb} {document}");
        if let Some(section) = &self.section {
            for heading in &section.heading_path {
                out.push_str(" › ");
                out.push_str(heading);
            }
            if let Some(start) = section.start_line {
                let _ = write!(out, " (lines {})", lines(start, section.end_line));
            }
        }
        if self.unsaved {
            out.push_str(", with unsaved edits");
        }
        if let Some(selection) = &self.selection {
            let mut quote = selection
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let shortened = cut(&mut quote, SUMMARY_QUOTE_CHARS);
            out.push_str(", selected");
            if let Some(start) = selection.start_line {
                let _ = write!(out, " (lines {})", lines(start, selection.end_line));
            }
            out.push_str(": '");
            out.push_str(&quote);
            if shortened || selection.truncated {
                out.push('…');
            }
            out.push('\'');
        }
        out.push('.');
        out
    }
}

/// `40-62`, or `40` for a single line.
fn lines(start: u32, end: Option<u32>) -> String {
    match end {
        Some(end) if end != start => format!("{start}-{end}"),
        _ => start.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("docs")).expect("mkdir");
        std::fs::write(dir.path().join("docs/plan.md"), "# Plan\n").expect("write");
        std::fs::write(dir.path().join("README.md"), "# Hi\n").expect("write");
        let vault = Vault::open(dir.path()).expect("vault");
        (dir, vault)
    }

    fn full() -> AgentContext {
        AgentContext {
            document: Some("docs/plan.md".to_owned()),
            mode: Some(ViewMode::Reading),
            section: Some(Section {
                heading_path: vec!["Plan".to_owned(), "Rollout".to_owned()],
                start_line: Some(40),
                end_line: Some(62),
            }),
            selection: Some(Selection {
                text: "ship it on Friday".to_owned(),
                truncated: false,
                heading_path: vec!["Plan".to_owned(), "Rollout".to_owned()],
                start_line: Some(44),
                end_line: Some(46),
            }),
            unsaved: false,
        }
    }

    #[test]
    fn a_context_round_trips_in_the_shape_the_window_sends() {
        let json = serde_json::json!({
            "document": "docs/plan.md",
            "mode": "source",
            "section": { "headingPath": ["Plan"], "startLine": 1, "endLine": 9 },
            "selection": { "text": "x", "headingPath": ["Plan"], "startLine": 2, "endLine": 3 },
            "unsaved": true
        });
        let context: AgentContext = serde_json::from_value(json.clone()).expect("parse");
        assert_eq!(context.mode, Some(ViewMode::Source));
        assert!(context.unsaved);
        assert_eq!(serde_json::to_value(&context).expect("json"), json);
        // Everything is optional: an empty message is "nothing is open".
        let empty: AgentContext = serde_json::from_str("{}").expect("parse");
        assert_eq!(empty, AgentContext::default());
    }

    #[test]
    fn a_published_context_carries_its_time_beside_the_fields() {
        let published = Published {
            context: full(),
            updated_at_ms: 1_760_000_000_000,
            open_comments: 2,
        };
        let json = serde_json::to_value(&published).expect("json");
        assert_eq!(json["document"], "docs/plan.md");
        assert_eq!(json["updatedAtMs"], 1_760_000_000_000_u64);
        assert_eq!(json["openComments"], 2);
        let back: Published = serde_json::from_value(json).expect("parse");
        assert_eq!(back, published);
        // An older window, or an `agent.json` from before the count, still reads: it says 0.
        let old = serde_json::json!({ "document": "docs/plan.md", "updatedAtMs": 1 });
        let old: Published = serde_json::from_value(old).expect("parse");
        assert_eq!(old.open_comments, 0);
    }

    #[test]
    fn the_published_summary_says_how_many_comments_are_open() {
        let published = |open: u32| Published {
            context: full(),
            updated_at_ms: 1,
            open_comments: open,
        };
        assert_eq!(
            published(1).summary_with_comments(),
            "Viewing docs/plan.md › Plan › Rollout (lines 40-62), selected (lines 44-46): \
'ship it on Friday', 1 comment open."
        );
        assert_eq!(
            published(3).summary_with_comments(),
            "Viewing docs/plan.md › Plan › Rollout (lines 40-62), selected (lines 44-46): \
'ship it on Friday', 3 comments open."
        );
        // None open: the paragraph says nothing of comments.
        assert_eq!(published(0).summary_with_comments(), full().summary());
    }

    #[test]
    fn a_document_of_the_project_is_shared_with_its_real_spelling() {
        let (_dir, vault) = project();
        let shared = full().sanitize(&vault);
        assert_eq!(shared.document.as_deref(), Some("docs/plan.md"));
        assert_eq!(shared.section, full().section);
        assert_eq!(shared.selection, full().selection);
        // A backslash path from a Windows window means the same file.
        let mut windows = full();
        windows.document = Some("docs\\plan.md".to_owned());
        assert_eq!(
            windows.sanitize(&vault).document.as_deref(),
            Some("docs/plan.md")
        );
    }

    #[test]
    fn nothing_outside_the_project_is_shared() {
        let (_dir, vault) = project();
        for path in [
            "../secret.md",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "docs/../../x.md",
            "@wt/feature/docs/plan.md",
            "docs/missing.md",
            "docs",
            "",
        ] {
            let mut context = full();
            context.document = Some(path.to_owned());
            assert_eq!(
                context.sanitize(&vault),
                AgentContext::default(),
                "{path:?} must not be shared"
            );
        }
        // Without a document there is no section or selection either.
        let mut orphan = full();
        orphan.document = None;
        assert_eq!(orphan.sanitize(&vault), AgentContext::default());
    }

    #[test]
    fn a_long_selection_is_cut_at_a_character_and_says_so() {
        let (_dir, vault) = project();
        let mut context = full();
        let mut selection = context.selection.take().expect("selection");
        selection.text = "é".repeat(MAX_SELECTION_CHARS + 50);
        context.selection = Some(selection);
        let shared = context.sanitize(&vault).selection.expect("selection");
        assert_eq!(shared.text.chars().count(), MAX_SELECTION_CHARS);
        assert!(shared.truncated);
        // A selection that fits is left alone.
        let short = full().sanitize(&vault).selection.expect("selection");
        assert!(!short.truncated);
        assert_eq!(short.text, "ship it on Friday");
    }

    #[test]
    fn a_blank_selection_is_no_selection() {
        let (_dir, vault) = project();
        let mut context = full();
        context.selection.as_mut().expect("selection").text = "  \n\t ".to_owned();
        assert_eq!(context.sanitize(&vault).selection, None);
    }

    #[test]
    fn lines_must_be_in_order_and_start_at_one() {
        let (_dir, vault) = project();
        let mut context = full();
        context.section = Some(Section {
            heading_path: vec![],
            start_line: Some(0),
            end_line: Some(5),
        });
        let selection = context.selection.as_mut().expect("selection");
        selection.start_line = Some(9);
        selection.end_line = Some(3);
        let shared = context.sanitize(&vault);
        assert_eq!(
            shared.section, None,
            "a section starting at line 0 is dropped"
        );
        let selection = shared.selection.expect("the text is still shared");
        assert_eq!((selection.start_line, selection.end_line), (None, None));
        // A start without an end is a single line.
        let mut single = full();
        let selection = single.selection.as_mut().expect("selection");
        selection.start_line = Some(7);
        selection.end_line = None;
        let selection = single.sanitize(&vault).selection.expect("selection");
        assert_eq!((selection.start_line, selection.end_line), (Some(7), None));
    }

    #[test]
    fn a_section_may_have_a_name_without_lines_or_lines_without_a_name() {
        let (_dir, vault) = project();
        let mut named = full();
        named.section = Some(Section {
            heading_path: vec!["Plan".to_owned()],
            start_line: None,
            end_line: None,
        });
        let shared = named.clone().sanitize(&vault);
        assert_eq!(shared.section, named.section);
        assert_eq!(
            shared.summary(),
            "Viewing docs/plan.md › Plan, selected (lines 44-46): 'ship it on Friday'."
        );
        let json = serde_json::to_value(&shared.section).expect("json");
        assert_eq!(json, serde_json::json!({ "headingPath": ["Plan"] }));
        let mut numbered = full();
        numbered.section = Some(Section {
            heading_path: vec![],
            start_line: Some(3),
            end_line: None,
        });
        assert!(numbered.sanitize(&vault).section.is_some());
    }

    #[test]
    fn a_heading_that_wraps_is_one_line_in_the_summary_a_hook_prints() {
        let (_dir, vault) = project();
        let mut context = full();
        context.section.as_mut().expect("section").heading_path = vec![
            "Plan\nfor  the\r\nyear".to_owned(),
            "Roll\tout\u{a0}now".to_owned(),
        ];
        let shared = context.sanitize(&vault);
        let section = shared.section.as_ref().expect("section");
        assert_eq!(section.heading_path, ["Plan for the year", "Roll out now"]);
        assert!(!shared.summary().contains(['\n', '\r', '\t']), "one line");
    }

    #[test]
    fn headings_are_trimmed_cut_and_limited_in_depth() {
        let (_dir, vault) = project();
        let mut context = full();
        let long = "h".repeat(MAX_HEADING_CHARS + 10);
        let mut path = vec!["  ".to_owned(), long];
        path.extend((0..MAX_HEADING_DEPTH + 3).map(|n| format!("level {n}")));
        context.section.as_mut().expect("section").heading_path = path;
        let shared = context.sanitize(&vault).section.expect("section");
        assert_eq!(shared.heading_path.len(), MAX_HEADING_DEPTH);
        assert_eq!(
            shared.heading_path.last().map(String::as_str),
            Some(format!("level {}", MAX_HEADING_DEPTH + 2).as_str())
        );
        // The blank entry is gone and the long one is cut where it still shows.
        assert!(shared.heading_path.iter().all(|h| !h.trim().is_empty()));
        assert!(shared
            .heading_path
            .iter()
            .all(|h| h.chars().count() <= MAX_HEADING_CHARS));
    }

    #[test]
    fn the_summary_reads_like_a_sentence() {
        assert_eq!(
            full().summary(),
            "Viewing docs/plan.md › Plan › Rollout (lines 40-62), selected (lines 44-46): 'ship it on Friday'."
        );
        let mut editing = full();
        editing.mode = Some(ViewMode::Source);
        editing.unsaved = true;
        editing.selection = None;
        assert_eq!(
            editing.summary(),
            "Editing (source) docs/plan.md › Plan › Rollout (lines 40-62), with unsaved edits."
        );
        let mut bare = AgentContext {
            document: Some("README.md".to_owned()),
            ..AgentContext::default()
        };
        assert_eq!(bare.summary(), "Viewing README.md.");
        bare.mode = Some(ViewMode::Visual);
        assert_eq!(bare.summary(), "Editing (visual) README.md.");
        assert_eq!(
            AgentContext::default().summary(),
            "PerMark is open on this project, with no document open."
        );
    }

    #[test]
    fn the_summary_quotes_a_short_collapsed_selection() {
        let mut context = full();
        let selection = context.selection.as_mut().expect("selection");
        selection.text = format!("first line\n\n   second   line {}", "w ".repeat(400));
        selection.start_line = Some(5);
        selection.end_line = Some(5);
        let summary = context.summary();
        assert!(
            summary.contains("selected (lines 5): 'first line second line w w"),
            "{summary}"
        );
        assert!(summary.ends_with("…'."), "{summary}");
        assert!(!summary.contains('\n'), "one paragraph");
        assert!(summary.chars().count() < 600, "short enough for a prompt");
    }

    #[test]
    fn a_selection_cut_by_the_limit_is_quoted_with_an_ellipsis() {
        let mut context = full();
        let selection = context.selection.as_mut().expect("selection");
        selection.text = "short".to_owned();
        selection.truncated = true;
        assert!(context.summary().ends_with("'short…'."));
    }
}
