//! What the person is looking at, as the window publishes it and an agent receives it. [`WindowContext::sanitize`] is
//! the one place that decides what of it leaves the app: the window's word is never trusted for a path or a size.

use std::fmt::Write as _;

use permark_core::secrets::mask_text;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::guard::Guard;

/// Longest selection shared (characters).
pub const MAX_SELECTION_CHARS: usize = 2000;
/// Longest heading kept (characters).
const MAX_HEADING_CHARS: usize = 200;
/// Deepest heading path kept (its last entries).
const MAX_HEADING_DEPTH: usize = 8;
/// How much of a selection the one-line summary quotes.
const SUMMARY_QUOTE_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AgentMode")]
pub enum Mode {
    #[default]
    View,
    Edit,
    Source,
}

/// The section on screen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AgentSection")]
pub struct Section {
    pub heading_path: Vec<String>,
    /// 1-based, both included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<(u32, u32)>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde hands the field by reference
fn is_false(value: &bool) -> bool {
    !*value
}

/// The text the person selected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AgentSelection")]
pub struct Selection {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<(u32, u32)>,
    /// The text was longer than the limit and was cut.
    #[serde(default, skip_serializing_if = "is_false")]
    pub cut: bool,
}

/// What a window shows (published by the window, debounced).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AgentWindowContext")]
pub struct WindowContext {
    /// Project path of the open document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unsaved: bool,
}

fn cut_chars(text: &str, max: usize) -> (String, bool) {
    match text.char_indices().nth(max) {
        Some((at, _)) => (text[..at].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ordered(lines: Option<(u32, u32)>) -> Option<(u32, u32)> {
    lines.filter(|(a, b)| *a >= 1 && a <= b)
}

impl WindowContext {
    /// What may be shared: the document must be a shared file of the project (else the whole context is nothing);
    /// headings and the selection are cut to their limits, one line each, secrets masked; lines in order.
    #[must_use]
    pub fn sanitize(self, guard: &Guard) -> Self {
        let Some(doc) = self
            .doc
            .as_deref()
            .and_then(|d| guard.file(d).ok())
            .map(|(p, _)| p)
        else {
            return Self::default();
        };
        let section = self.section.map(|s| {
            let skip = s.heading_path.len().saturating_sub(MAX_HEADING_DEPTH);
            Section {
                heading_path: s
                    .heading_path
                    .iter()
                    .skip(skip)
                    // a heading is the person's text: its secret values stay hidden
                    .map(|h| mask_text(&cut_chars(&one_line(h), MAX_HEADING_CHARS).0).into_owned())
                    .filter(|h| !h.is_empty())
                    .collect(),
                lines: ordered(s.lines),
            }
        });
        let selection = self
            .selection
            .filter(|s| !s.text.trim().is_empty())
            .map(|s| {
                let (text, cut) = cut_chars(&s.text, MAX_SELECTION_CHARS);
                Selection {
                    text: mask_text(&text).into_owned(),
                    lines: ordered(s.lines),
                    cut: cut || s.cut,
                }
            });
        Self {
            doc: Some(doc),
            mode: self.mode,
            section,
            selection,
            unsaved: self.unsaved,
        }
    }

    /// One line for a prompt hook: `Viewing docs/plan.md › Rollout (lines 40-62), selected (lines 44-46): "...";
    /// 2 open comments.` (the selection as a JSON string). `None` when no document is open.
    #[must_use]
    pub fn summary(&self, open_comments: usize) -> Option<String> {
        let doc = self.doc.as_deref()?;
        let verb = match self.mode {
            Mode::View => "Viewing",
            Mode::Edit | Mode::Source => "Editing",
        };
        let mut line = format!("{verb} {doc}");
        if let Some(section) = &self.section {
            if !section.heading_path.is_empty() {
                line.push_str(" › ");
                line.push_str(&section.heading_path.join(" › "));
            }
            if let Some((a, b)) = section.lines {
                let _ = write!(line, " (lines {a}-{b})");
            }
        }
        if let Some(selection) = &self.selection {
            line.push_str(", selected");
            if let Some((a, b)) = selection.lines {
                let _ = write!(line, " (lines {a}-{b})");
            }
            let (quote, cut) = cut_chars(&one_line(&selection.text), SUMMARY_QUOTE_CHARS);
            let quoted =
                serde_json::Value::String(format!("{quote}{}", if cut { "…" } else { "" }));
            let _ = write!(line, ": {quoted}");
        }
        if self.unsaved {
            line.push_str(", unsaved");
        }
        if open_comments > 0 {
            let _ = write!(
                line,
                "; {open_comments} open comment{}",
                if open_comments == 1 { "" } else { "s" }
            );
        }
        line.push('.');
        Some(line)
    }
}
