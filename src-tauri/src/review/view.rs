//! What the UI is given to draw a review: the changes of a document, each with its blocks side by side as
//! the reading view renders them.
//!
//! The HTML comes from the reading view's own renderer and sanitizer ([`render_fragments`]), so the UI never
//! draws raw Markdown or anything unsanitized. Offsets are bytes of the text on each side, as in the editor's
//! blocks; `text` is the block's Markdown, for a view of the source.

use serde::Serialize;

use super::diff::{Change, ChangeKind, Comparison, Span};
use crate::edit::render_fragments;

/// Why a document has the changes it has, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewStatus {
    /// It differs from the approved version: `changes` lists how.
    Changes,
    /// It is what was approved (or differs in blank lines only): nothing to review.
    Unchanged,
    /// It was never approved: `changes` is one addition of all of it.
    New,
    /// The file is gone: `changes` is one removal of all of it, if the approved text is kept.
    Deleted,
    /// It differs, but the approved text is not kept (it was too large, or this is the first run after an update).
    NoBaseline,
    /// The file is too large to compare.
    TooLarge,
    /// The file or its approved version is not UTF-8 text.
    NotText,
}

/// A range of bytes. Matches the UI's `ByteRange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ByteRange {
    pub start_byte: usize,
    pub end_byte: usize,
}

impl From<Span> for ByteRange {
    fn from(span: Span) -> Self {
        Self {
            start_byte: span.start,
            end_byte: span.end,
        }
    }
}

/// A range of block numbers. Matches the UI's `BlockRange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRange {
    pub start: usize,
    pub end: usize,
}

/// One block of one version. Matches the UI's `ReviewBlock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockView {
    /// Which block of its version this is.
    pub index: usize,
    #[serde(flatten)]
    pub range: ByteRange,
    /// The block's Markdown.
    pub text: String,
    /// The block as the reading view draws it (sanitized). Empty when it could not be rendered; the UI then
    /// shows `text`.
    pub html: String,
}

/// An old block, a new block, or both when the block was edited. Matches the UI's `ReviewRow`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowView {
    pub old: Option<BlockView>,
    pub new: Option<BlockView>,
}

/// A run of changed blocks. Matches the UI's `ReviewChange`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeView {
    pub kind: ChangeKind,
    /// In the approved text (an empty range, for an addition, is the place the new blocks would go).
    pub old: ByteRange,
    pub new: ByteRange,
    pub old_blocks: BlockRange,
    pub new_blocks: BlockRange,
    pub rows: Vec<RowView>,
}

/// A document under review. Matches the UI's `ReviewView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewView {
    pub path: String,
    pub status: ViewStatus,
    /// The hash of the file the changes were computed from (`None`: there is no file). Hand it back with
    /// every Keep and Revert.
    pub hash: Option<String>,
    /// The hash of the approved version they were computed against (`None`: there is none). Hand it back too.
    pub reviewed_hash: Option<String>,
    /// For a deleted document: whether the approved text is kept, so that it can be put back.
    pub restorable: bool,
    pub changes: Vec<ChangeView>,
}

impl ReviewView {
    /// A view with no changes in it.
    pub fn bare(
        path: &str,
        status: ViewStatus,
        hash: Option<String>,
        reviewed_hash: Option<String>,
    ) -> Self {
        Self {
            path: path.to_owned(),
            status,
            hash,
            reviewed_hash,
            restorable: false,
            changes: Vec::new(),
        }
    }
}

fn block_range(range: &std::ops::Range<usize>) -> BlockRange {
    BlockRange {
        start: range.start,
        end: range.end,
    }
}

/// The changes of a comparison with their blocks rendered.
pub fn changes_view(cmp: &Comparison<'_>) -> Vec<ChangeView> {
    // every block that is shown, in the order the views are built, rendered in one go
    let mut texts: Vec<String> = Vec::new();
    for change in cmp.changes() {
        for row in &change.rows {
            texts.extend(row.old.map(|i| cmp.old_block(i).to_owned()));
            texts.extend(row.new.map(|i| cmp.new_block(i).to_owned()));
        }
    }
    let mut html = render_fragments(&texts).into_iter();
    let mut block = |index: usize, text: &str, range: Span| BlockView {
        index,
        range: range.into(),
        text: text.to_owned(),
        html: html.next().unwrap_or_default(),
    };
    cmp.changes()
        .iter()
        .map(|change| one_change(cmp, change, &mut block))
        .collect()
}

fn one_change(
    cmp: &Comparison<'_>,
    change: &Change,
    block: &mut impl FnMut(usize, &str, Span) -> BlockView,
) -> ChangeView {
    let rows = change
        .rows
        .iter()
        .map(|row| RowView {
            old: row.old.map(|i| block(i, cmp.old_block(i), cmp.old_span(i))),
            new: row.new.map(|i| block(i, cmp.new_block(i), cmp.new_span(i))),
        })
        .collect();
    ChangeView {
        kind: change.kind,
        old: change.old.into(),
        new: change.new.into(),
        old_blocks: block_range(&change.old_blocks),
        new_blocks: block_range(&change.new_blocks),
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modified_block_comes_with_both_versions_rendered() {
        let reviewed = "# T\n\nThe quick brown fox jumps over the lazy dog.\n\nend\n";
        let current = "# T\n\nThe quick brown fox leaps over the lazy dog.\n\nend\n";
        let cmp = Comparison::new(reviewed, current);
        let changes = changes_view(&cmp);
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(change.kind, ChangeKind::Modified);
        assert_eq!(change.old_blocks, BlockRange { start: 1, end: 2 });
        assert_eq!(change.rows.len(), 1);
        let old = change.rows[0].old.as_ref().expect("old");
        let new = change.rows[0].new.as_ref().expect("new");
        assert_eq!(old.index, 1);
        assert_eq!(old.text, "The quick brown fox jumps over the lazy dog.");
        assert!(
            old.html.contains("<p") && old.html.contains("jumps"),
            "{}",
            old.html
        );
        assert!(new.html.contains("leaps"), "{}", new.html);
        assert_eq!(
            &current[new.range.start_byte..new.range.end_byte],
            new.text,
            "the offsets are bytes of the text"
        );
    }

    #[test]
    fn an_addition_has_blocks_on_the_new_side_only() {
        let cmp = Comparison::new("a\n", "a\n\nb\n\n| x | y |\n|---|---|\n| 1 | 2 |\n");
        let changes = changes_view(&cmp);
        assert_eq!(changes[0].kind, ChangeKind::Added);
        assert!(changes[0].rows.iter().all(|r| r.old.is_none()));
        assert!(changes[0].rows[1]
            .new
            .as_ref()
            .expect("table")
            .html
            .contains("<table"));
    }

    #[test]
    fn what_could_be_a_script_is_sanitized_like_the_reading_view_does() {
        let cmp = Comparison::new(
            "a\n",
            "a\n\n<script>alert(1)</script>\n\n<img src=x onerror=alert(2)>\n\nsafe\n",
        );
        let changes = changes_view(&cmp);
        let html: String = changes[0]
            .rows
            .iter()
            .filter_map(|r| r.new.as_ref())
            .map(|b| b.html.as_str())
            .collect();
        assert!(
            !html.contains("<script") && !html.contains("onerror"),
            "{html}"
        );
        assert!(html.contains("safe"));
        let raw = changes[0].rows[0].new.as_ref().expect("block");
        assert_eq!(raw.text, "<script>alert(1)</script>", "the source is text");
    }

    #[test]
    fn a_view_serializes_the_way_the_ui_reads_it() {
        let cmp = Comparison::new("a\n", "a\n\nb\n");
        let mut view = ReviewView::bare(
            "doc.md",
            ViewStatus::Changes,
            Some("00000000000000ff".to_owned()),
            None,
        );
        view.changes = changes_view(&cmp);
        let json = serde_json::to_value(&view).expect("json");
        assert_eq!(json["status"], "changes");
        assert_eq!(json["hash"], "00000000000000ff");
        assert_eq!(json["reviewedHash"], serde_json::Value::Null);
        assert_eq!(json["restorable"], false);
        let change = &json["changes"][0];
        assert_eq!(change["kind"], "added");
        assert_eq!(
            change["newBlocks"],
            serde_json::json!({"start": 1, "end": 2})
        );
        assert_eq!(change["rows"][0]["old"], serde_json::Value::Null);
        assert_eq!(change["rows"][0]["new"]["index"], 1);
        assert_eq!(change["rows"][0]["new"]["startByte"], 3);
        assert_eq!(change["rows"][0]["new"]["text"], "b");
    }
}
