//! The difference between two versions of a Markdown document, block by block, and the splices that keep
//! or undo one part of it.
//!
//! The blocks are the ones the editor works with ([`parse_blocks`]): paragraphs, headings, lists, tables, code
//! and so on, as byte ranges of the text. Two blocks are the same when their text is the same, line endings
//! aside; what lies between blocks (blank lines, a byte order mark, the final newline) is never compared, so
//! a file that only gained a blank line has nothing to review. A longest common subsequence of the blocks
//! finds the ones that stayed; everything between two of them is one [`Change`], a run of consecutive blocks
//! that were removed, added or edited. Inside a change, an edited block is paired with its old version when
//! they share enough words (the pairing the reading view does on live reload), which the change keeps as
//! [`Row`]s so that a view can draw the two next to each other.
//!
//! A block that moved is a removal where it was and an addition where it went: they are two changes, each
//! kept or reverted on its own.
//!
//! [`Comparison::kept`] and [`Comparison::reverted`] splice one change into the other text. They replace only
//! the blocks and the separator that goes with them, so the rest of the file, byte for byte, is left as it is.

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

use serde::Serialize;

use crate::edit::blocks::{parse_blocks, BlockInfo, Eol};
use crate::search::fold::tokenize;

/// Above this many cells (old blocks x new blocks, between the first and the last block that differ) the
/// blocks are not matched: the whole middle is one change.
const MAX_CELLS: usize = 4_000_000;

/// Two blocks are one block edited when at least this share of their words is the same, in thousandths.
const SAME_BLOCK: usize = 400;

/// A run of changed blocks bigger than this many old x new blocks is not paired: it is a rewrite, not an edit.
const MAX_COMPARISONS: usize = 40_000;

/// A range of bytes of a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    /// Blocks that were not in the reviewed version.
    Added,
    /// Blocks of the reviewed version that are gone.
    Removed,
    /// Blocks that were replaced by others, or edited.
    Modified,
}

/// One line of a change: an old block, a new block, or both when the block was edited. The numbers are
/// block indices of the reviewed and the current text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub old: Option<usize>,
    pub new: Option<usize>,
}

/// A run of consecutive blocks that differ, between two blocks that are the same in both versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub kind: ChangeKind,
    /// The blocks of the reviewed text this change covers (empty for an addition).
    pub old_blocks: Range<usize>,
    /// The blocks of the current text this change covers (empty for a removal).
    pub new_blocks: Range<usize>,
    /// From the first byte of the first old block to the last byte of the last; for an addition, the empty
    /// place in the reviewed text where the new blocks would go.
    pub old: Span,
    /// The same for the current text.
    pub new: Span,
    /// The old and new blocks side by side, in document order: edited blocks paired, the rest alone.
    pub rows: Vec<Row>,
}

/// One text with its blocks.
struct Side<'a> {
    text: &'a str,
    blocks: Vec<BlockInfo>,
    eol: Eol,
}

impl<'a> Side<'a> {
    fn new(text: &'a str) -> Self {
        let parsed = parse_blocks(text);
        Self {
            text,
            blocks: parsed.blocks,
            eol: parsed.eol,
        }
    }

    fn block_text(&self, index: usize) -> &'a str {
        let block = &self.blocks[index];
        &self.text[block.start_byte..block.end_byte]
    }

    /// What two blocks are compared by: their text with the same line ending everywhere.
    fn keys(&self) -> Vec<Cow<'a, str>> {
        (0..self.blocks.len())
            .map(|i| {
                let text = self.block_text(i);
                if text.contains('\r') {
                    Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
                } else {
                    Cow::Borrowed(text)
                }
            })
            .collect()
    }

    /// The bytes of blocks `range` (non-empty), from the start of the first to the end of the last.
    fn span(&self, range: &Range<usize>) -> Span {
        Span {
            start: self.blocks[range.start].start_byte,
            end: self.blocks[range.end - 1].end_byte,
        }
    }

    /// Where blocks would go that were to be put in front of block `index`: the empty place at the start
    /// of that block, or at the end of the last one when there is none after.
    fn place(&self, index: usize) -> Span {
        let at = match (self.blocks.get(index), index.checked_sub(1)) {
            (Some(block), _) => block.start_byte,
            (None, Some(before)) => self.blocks[before].end_byte,
            (None, None) => 0,
        };
        Span { start: at, end: at }
    }

    fn range_span(&self, range: &Range<usize>) -> Span {
        if range.is_empty() {
            self.place(range.start)
        } else {
            self.span(range)
        }
    }

    /// The separator between block `index - 1` and block `index`.
    fn gap_before(&self, index: usize) -> &'a str {
        &self.text[self.blocks[index - 1].end_byte..self.blocks[index].start_byte]
    }

    /// The separator written when the other text has none to offer: a blank line.
    fn default_gap(&self) -> &'static str {
        if self.eol == Eol::Crlf {
            "\r\n\r\n"
        } else {
            "\n\n"
        }
    }

    /// `text` from another file, with this file's line ending when that is the same in all of it.
    fn like_mine<'t>(&self, text: &'t str) -> Cow<'t, str> {
        match self.eol {
            Eol::Mixed => Cow::Borrowed(text),
            eol => {
                let plain = text.replace("\r\n", "\n").replace('\r', "\n");
                if eol == Eol::Crlf {
                    Cow::Owned(plain.replace('\n', "\r\n"))
                } else if plain == text {
                    Cow::Borrowed(text)
                } else {
                    Cow::Owned(plain)
                }
            }
        }
    }
}

/// The reviewed text and the current one, with what differs between them.
pub struct Comparison<'a> {
    reviewed: Side<'a>,
    current: Side<'a>,
    changes: Vec<Change>,
}

impl<'a> Comparison<'a> {
    /// The differences between the reviewed text and the current one, in document order. A text over what the
    /// reader and the editor take ([`crate::render::MAX_RENDER_BYTES`]) has no blocks, so callers keep to less.
    pub fn new(reviewed: &'a str, current: &'a str) -> Self {
        let reviewed = Side::new(reviewed);
        let current = Side::new(current);
        let old_keys = reviewed.keys();
        let new_keys = current.keys();
        let matches = lcs_pairs(&old_keys, &new_keys);

        let mut changes = Vec::new();
        let (mut old_from, mut new_from) = (0, 0);
        let end = (reviewed.blocks.len(), current.blocks.len());
        for (old_at, new_at) in matches.into_iter().chain(std::iter::once(end)) {
            if old_at > old_from || new_at > new_from {
                changes.push(make_change(
                    &reviewed,
                    &current,
                    old_from..old_at,
                    new_from..new_at,
                ));
            }
            (old_from, new_from) = (old_at + 1, new_at + 1);
        }
        Self {
            reviewed,
            current,
            changes,
        }
    }

    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// The text of block `index` of the reviewed text.
    pub fn old_block(&self, index: usize) -> &'a str {
        self.reviewed.block_text(index)
    }

    /// The text of block `index` of the current text.
    pub fn new_block(&self, index: usize) -> &'a str {
        self.current.block_text(index)
    }

    /// The bytes of block `index` of the reviewed text.
    pub fn old_span(&self, index: usize) -> Span {
        self.reviewed.span(&(index..index + 1))
    }

    /// The bytes of block `index` of the current text.
    pub fn new_span(&self, index: usize) -> Span {
        self.current.span(&(index..index + 1))
    }

    /// The reviewed text with change `index` accepted: its new blocks take the place of the old ones.
    pub fn kept(&self, index: usize) -> String {
        let change = &self.changes[index];
        splice(
            &self.reviewed,
            &change.old_blocks,
            &self.current,
            &change.new_blocks,
        )
    }

    /// The current text with change `index` undone: its old blocks take the place of the new ones.
    pub fn reverted(&self, index: usize) -> String {
        let change = &self.changes[index];
        splice(
            &self.current,
            &change.new_blocks,
            &self.reviewed,
            &change.old_blocks,
        )
    }
}

fn make_change(
    reviewed: &Side<'_>,
    current: &Side<'_>,
    old_blocks: Range<usize>,
    new_blocks: Range<usize>,
) -> Change {
    let kind = match (old_blocks.is_empty(), new_blocks.is_empty()) {
        (true, _) => ChangeKind::Added,
        (_, true) => ChangeKind::Removed,
        _ => ChangeKind::Modified,
    };
    let old_words: Vec<Words> = old_blocks
        .clone()
        .map(|i| Words::of(reviewed.block_text(i)))
        .collect();
    let new_words: Vec<Words> = new_blocks
        .clone()
        .map(|i| Words::of(current.block_text(i)))
        .collect();
    let pairs = align(&old_words, &new_words);

    let mut rows = Vec::with_capacity(old_words.len() + new_words.len());
    let (mut i, mut j) = (0, 0);
    let end = (old_words.len(), new_words.len());
    for (pair_old, pair_new) in pairs.into_iter().chain(std::iter::once(end)) {
        while i < pair_old {
            rows.push(Row {
                old: Some(old_blocks.start + i),
                new: None,
            });
            i += 1;
        }
        while j < pair_new {
            rows.push(Row {
                old: None,
                new: Some(new_blocks.start + j),
            });
            j += 1;
        }
        if pair_old < old_words.len() {
            rows.push(Row {
                old: Some(old_blocks.start + pair_old),
                new: Some(new_blocks.start + pair_new),
            });
            (i, j) = (pair_old + 1, pair_new + 1);
        }
    }
    Change {
        kind,
        old: reviewed.range_span(&old_blocks),
        new: current.range_span(&new_blocks),
        old_blocks,
        new_blocks,
        rows,
    }
}

/// The text of `target` with its blocks `replace` swapped for the blocks `with` of `source`.
///
/// The blocks go with the separators inside them. What is outside stays as `target` has it, except that
/// taking blocks out takes one separator with them (the one after, or before the last block) and putting blocks
/// in brings one (the one that followed them in `source`, or preceded them at the end), so the blank lines
/// around stay as they were. When the swap covers all the blocks of both texts, the result is `source` itself.
fn splice(
    target: &Side<'_>,
    replace: &Range<usize>,
    source: &Side<'_>,
    with: &Range<usize>,
) -> String {
    let whole =
        |range: &Range<usize>, side: &Side<'_>| range.start == 0 && range.end == side.blocks.len();
    if whole(replace, target) && whole(with, source) {
        return source.text.to_owned();
    }
    let text = target.text;
    let blocks = &target.blocks;
    let (cut, put): (Range<usize>, String) = match (replace.is_empty(), with.is_empty()) {
        (true, true) => return text.to_owned(),
        // both have blocks: the blocks change, the separators around them stay
        (false, false) => {
            let place = target.span(replace);
            let new = source.span(with);
            (
                place.start..place.end,
                target
                    .like_mine(&source.text[new.start..new.end])
                    .into_owned(),
            )
        }
        // blocks go
        (false, true) => {
            let first = blocks[replace.start].start_byte;
            if let Some(next) = blocks.get(replace.end) {
                (first..next.start_byte, String::new())
            } else if let Some(before) = replace.start.checked_sub(1) {
                (
                    blocks[before].end_byte..blocks[replace.end - 1].end_byte,
                    String::new(),
                )
            } else {
                return source.text.to_owned();
            }
        }
        // blocks come in
        (true, false) => {
            let new = source.span(with);
            let fragment = target.like_mine(&source.text[new.start..new.end]);
            if let Some(next) = blocks.get(replace.start) {
                let gap = if with.end < source.blocks.len() {
                    target.like_mine(source.gap_before(with.end)).into_owned()
                } else {
                    target.default_gap().to_owned()
                };
                (next.start_byte..next.start_byte, format!("{fragment}{gap}"))
            } else if let Some(before) = replace.start.checked_sub(1) {
                let gap = if with.start > 0 {
                    target.like_mine(source.gap_before(with.start)).into_owned()
                } else {
                    target.default_gap().to_owned()
                };
                let at = blocks[before].end_byte;
                (at..at, format!("{gap}{fragment}"))
            } else {
                return source.text.to_owned();
            }
        }
    };
    let mut out = String::with_capacity(text.len() + put.len());
    out.push_str(&text[..cut.start]);
    out.push_str(&put);
    out.push_str(&text[cut.end..]);
    out
}

// ---- finding the blocks that stayed ---------------------------------------------------------------------------

/// Longest common subsequence of two key lists, as `(old index, new index)` pairs in order.
///
/// What both versions start with and end with is paired first, so that a small edit in a big document costs
/// next to nothing. The middle is matched when it is small enough (see [`MAX_CELLS`]).
fn lcs_pairs(old: &[Cow<'_, str>], new: &[Cow<'_, str>]) -> Vec<(usize, usize)> {
    let mut start = 0;
    while start < old.len() && start < new.len() && old[start] == new[start] {
        start += 1;
    }
    let (mut old_end, mut new_end) = (old.len(), new.len());
    while old_end > start && new_end > start && old[old_end - 1] == new[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }

    let mut pairs: Vec<(usize, usize)> = (0..start).map(|at| (at, at)).collect();
    let (rows, cols) = (old_end - start, new_end - start);
    if rows > 0 && cols > 0 && rows.saturating_mul(cols) <= MAX_CELLS {
        // The longest match is at most min(rows, cols), which is under 2000 when rows * cols fits in MAX_CELLS.
        let width = cols + 1;
        let mut table = vec![0_u16; (rows + 1) * width];
        for row in (0..rows).rev() {
            for col in (0..cols).rev() {
                table[row * width + col] = if old[start + row] == new[start + col] {
                    table[(row + 1) * width + col + 1] + 1
                } else {
                    table[(row + 1) * width + col].max(table[row * width + col + 1])
                };
            }
        }
        let (mut row, mut col) = (0, 0);
        while row < rows && col < cols {
            if old[start + row] == new[start + col] {
                pairs.push((start + row, start + col));
                row += 1;
                col += 1;
            } else if table[(row + 1) * width + col] >= table[row * width + col + 1] {
                row += 1;
            } else {
                col += 1;
            }
        }
    }
    pairs.extend((0..old.len() - old_end).map(|k| (old_end + k, new_end + k)));
    pairs
}

// ---- recognising edited blocks --------------------------------------------------------------------------------

/// The words of a block with how many times each appears. Words are folded the way the search folds them
/// (case, accents, endings), and Chinese and Japanese text counts its characters and pairs of them.
struct Words {
    counts: HashMap<String, usize>,
    total: usize,
}

impl Words {
    fn of(text: &str) -> Self {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut total = 0;
        for token in tokenize(text) {
            *counts.entry(token.term).or_default() += 1;
            total += 1;
        }
        Self { counts, total }
    }

    /// The share of words two blocks have in common (repeats counted), in thousandths. Blocks without
    /// words share none.
    fn similarity(&self, other: &Self) -> usize {
        if self.total == 0 || other.total == 0 {
            return 0;
        }
        let (small, large) = if self.counts.len() <= other.counts.len() {
            (self, other)
        } else {
            (other, self)
        };
        let common: usize = small
            .counts
            .iter()
            .map(|(word, count)| *count.min(large.counts.get(word).unwrap_or(&0)))
            .sum();
        2000 * common / (self.total + other.total)
    }
}

/// Pairs the old and new blocks of one change, in order, so that the total similarity of the pairs is the
/// highest and no pair is less similar than [`SAME_BLOCK`]. Returns `(old, new)` pairs of indices into the
/// two lists.
fn align(old: &[Words], new: &[Words]) -> Vec<(usize, usize)> {
    let (rows, cols) = (old.len(), new.len());
    if rows == 0 || cols == 0 || rows * cols > MAX_COMPARISONS {
        return Vec::new();
    }
    let width = cols + 1;
    // best[i * width + j]: the best total for old blocks i.. and new blocks j..
    let mut best = vec![0_usize; (rows + 1) * width];
    let mut same = vec![0_usize; rows * cols];
    for i in (0..rows).rev() {
        for j in (0..cols).rev() {
            let s = old[i].similarity(&new[j]);
            same[i * cols + j] = s;
            let mut value = best[(i + 1) * width + j].max(best[i * width + j + 1]);
            if s >= SAME_BLOCK {
                value = value.max(best[(i + 1) * width + j + 1] + s);
            }
            best[i * width + j] = value;
        }
    }

    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < rows && j < cols {
        let s = same[i * cols + j];
        if s >= SAME_BLOCK && best[i * width + j] == best[(i + 1) * width + j + 1] + s {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if best[(i + 1) * width + j] >= best[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}
