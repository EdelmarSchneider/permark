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

use crate::edit::blocks::{parse_blocks, split_lines, BlockInfo, BlockKind, Eol, Line, BOM};
use crate::render::MAX_RENDER_BYTES;
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
    /// What two blocks are compared by: their text with the same line ending everywhere.
    keys: Vec<Cow<'a, str>>,
    eol: Eol,
    /// Where each line starts, numbered the way the reading view numbers them (`data-sourcepos`): a line break is
    /// `\n`, `\r\n` or a lone `\r`, and a byte order mark is not part of the first line.
    line_starts: Vec<usize>,
    /// The link reference definitions of the text, one run after the other: what a block needs to draw its
    /// reference-style links when it is drawn on its own.
    definitions: String,
}

/// The runs of non-blank lines that no block covers: text the parser consumes without making a node of it, which
/// is link reference definitions (`[d]: https://...`). Each run is a block of kind `Other`.
fn uncovered_runs(text: &str, lines: &[Line], blocks: &[BlockInfo]) -> Vec<BlockInfo> {
    let run = |(start_byte, end_byte)| BlockInfo {
        start_byte,
        end_byte,
        kind: BlockKind::Other,
    };
    let mut runs = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    let mut next = 0;
    for line in lines {
        while next < blocks.len() && blocks[next].end_byte < line.start {
            next += 1;
        }
        let covered = blocks
            .get(next)
            .is_some_and(|b| b.start_byte <= line.start && line.end <= b.end_byte);
        if covered || text[line.start..line.end].trim().is_empty() {
            runs.extend(open.take().map(run));
        } else {
            open = Some((open.map_or(line.start, |(start, _)| start), line.end));
        }
    }
    runs.extend(open.map(run));
    runs
}

impl<'a> Side<'a> {
    fn new(text: &'a str) -> Self {
        let parsed = parse_blocks(text);
        let body_start = if text.starts_with(BOM) {
            BOM.len_utf8()
        } else {
            0
        };
        let (lines, _) = split_lines(text, body_start);
        let mut blocks = parsed.blocks;
        // A text over the size the parser takes has no blocks, and is not one block made of everything.
        let mut definitions = String::new();
        if text.len() as u64 <= MAX_RENDER_BYTES {
            let runs = uncovered_runs(text, &lines, &blocks);
            for run in &runs {
                definitions.push_str(&text[run.start_byte..run.end_byte]);
                definitions.push('\n');
            }
            blocks.extend(runs);
            blocks.sort_by_key(|b| b.start_byte);
        }
        let keys = blocks
            .iter()
            .map(|block| {
                let block_text = &text[block.start_byte..block.end_byte];
                if block_text.contains('\r') {
                    Cow::Owned(block_text.replace("\r\n", "\n").replace('\r', "\n"))
                } else {
                    Cow::Borrowed(block_text)
                }
            })
            .collect();
        Self {
            text,
            blocks,
            keys,
            eol: parsed.eol,
            line_starts: lines.iter().map(|line| line.start).collect(),
            definitions,
        }
    }

    fn block_text(&self, index: usize) -> &'a str {
        let block = &self.blocks[index];
        &self.text[block.start_byte..block.end_byte]
    }

    /// The first and the last line of block `index`, counted from 1.
    fn lines_of(&self, index: usize) -> (usize, usize) {
        let block = &self.blocks[index];
        let line_at = |byte: usize| self.line_starts.partition_point(|&start| start <= byte);
        (line_at(block.start_byte), line_at(block.end_byte))
    }

    /// The blocks `range` as they are compared: kind and text.
    fn shape(&self, range: Range<usize>) -> impl Iterator<Item = (BlockKind, &str)> {
        range.map(|i| (self.blocks[i].kind, self.keys[i].as_ref()))
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

    /// The bytes that belong to blocks `range` and the separators that join them to their neighbours: from the end of
    /// the block before to the start of the block after. Where there is no block before (or after), it starts at the
    /// first block (ends at the last), and for no blocks at all it is the empty place where they would be.
    fn region(&self, range: &Range<usize>) -> Span {
        let before = range.start.checked_sub(1).map(|i| self.blocks[i].end_byte);
        let after = self.blocks.get(range.end).map(|b| b.start_byte);
        let (first, last) = if range.is_empty() {
            (None, None)
        } else {
            (
                Some(self.blocks[range.start].start_byte),
                Some(self.blocks[range.end - 1].end_byte),
            )
        };
        match (before, after) {
            (Some(start), Some(end)) => Span { start, end },
            (None, Some(end)) => Span {
                start: first.unwrap_or(end),
                end,
            },
            (Some(start), None) => Span {
                start,
                end: last.unwrap_or(start),
            },
            (None, None) => Span {
                start: first.unwrap_or(0),
                end: last.unwrap_or(0),
            },
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
        let matches = lcs_pairs(&reviewed.keys, &current.keys);

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

    /// The link reference definitions of the reviewed text: what its blocks need to be drawn on their own.
    pub fn old_definitions(&self) -> &str {
        &self.reviewed.definitions
    }

    /// The link reference definitions of the current text (see [`Comparison::old_definitions`]).
    pub fn new_definitions(&self) -> &str {
        &self.current.definitions
    }

    /// The first and the last line of block `index` of the reviewed text, counted from 1 as the reading view counts
    /// them (`data-sourcepos`).
    pub fn old_lines(&self, index: usize) -> (usize, usize) {
        self.reviewed.lines_of(index)
    }

    /// The first and the last line of block `index` of the current text (see [`Comparison::old_lines`]).
    pub fn new_lines(&self, index: usize) -> (usize, usize) {
        self.current.lines_of(index)
    }

    /// The line of the current text where `change` is: the first line of its new blocks; for a removal, the line of
    /// the block it sits in front of; `None` when it is at the end.
    pub fn new_line(&self, change: &Change) -> Option<usize> {
        (change.new_blocks.start < self.current.blocks.len())
            .then(|| self.current.lines_of(change.new_blocks.start).0)
    }

    /// The reviewed text with change `index` accepted: its new blocks take the place of the old ones.
    ///
    /// The result has the blocks of the reviewed text with that change swapped for the new ones, and no other: when a
    /// splice would join or split blocks (a table that takes in the paragraph next to it, a line of dashes that turns
    /// the top of the file into front matter), it is tried again with blank lines around the swapped blocks, and when
    /// that does not help either, the change cannot be applied on its own ([`CannotSplice`]).
    pub fn kept(&self, index: usize) -> Result<String, CannotSplice> {
        let change = &self.changes[index];
        splice_checked(
            &self.reviewed,
            &change.old_blocks,
            &self.current,
            &change.new_blocks,
        )
    }

    /// The current text with change `index` undone: its old blocks take the place of the new ones. The result is
    /// checked as in [`Comparison::kept`].
    pub fn reverted(&self, index: usize) -> Result<String, CannotSplice> {
        let change = &self.changes[index];
        splice_checked(
            &self.current,
            &change.new_blocks,
            &self.reviewed,
            &change.old_blocks,
        )
    }
}

/// A change cannot be kept or reverted on its own: whatever the splice does, the document ends up with other blocks
/// than the ones the review showed. Keeping or reverting everything is still possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CannotSplice;

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

/// Whether a separator holds a blank line, which keeps the blocks on both sides of it apart whatever they are.
fn has_blank_line(gap: &str) -> bool {
    let breaks = gap.replace("\r\n", "\n").replace('\r', "\n");
    breaks.matches('\n').count() >= 2
}

/// Whether a swap covers every block of both texts, which makes the result `source` itself.
fn swaps_everything(
    target: &Side<'_>,
    replace: &Range<usize>,
    source: &Side<'_>,
    with: &Range<usize>,
) -> bool {
    let whole =
        |range: &Range<usize>, side: &Side<'_>| range.start == 0 && range.end == side.blocks.len();
    whole(replace, target) && whole(with, source)
}

/// [`splice`], and the result is parsed again: it must have exactly the blocks of `target` with `replace` swapped for
/// `with`, which is what the review showed. If not, the swap is made again with a blank line between the swapped
/// blocks and their neighbours ([`splice_spaced`]); if that fails too, there is no result.
fn splice_checked(
    target: &Side<'_>,
    replace: &Range<usize>,
    source: &Side<'_>,
    with: &Range<usize>,
) -> Result<String, CannotSplice> {
    if swaps_everything(target, replace, source, with) || (replace.is_empty() && with.is_empty()) {
        return Ok(splice(target, replace, source, with));
    }
    let wanted: Vec<(BlockKind, &str)> = target
        .shape(0..replace.start)
        .chain(source.shape(with.clone()))
        .chain(target.shape(replace.end..target.blocks.len()))
        .collect();
    [splice, splice_spaced]
        .into_iter()
        .map(|attempt| attempt(target, replace, source, with))
        .find(|result| {
            let made = Side::new(result);
            made.shape(0..made.blocks.len()).eq(wanted.iter().copied())
        })
        .ok_or(CannotSplice)
}

/// [`splice`] with a blank line between the swapped blocks and the blocks next to them, and the text before the first
/// block and after the last as `target` has it. The separators between the blocks that go in are those of `source`.
fn splice_spaced(
    target: &Side<'_>,
    replace: &Range<usize>,
    source: &Side<'_>,
    with: &Range<usize>,
) -> String {
    let blank = if target.eol == Eol::Crlf {
        "\r\n\r\n"
    } else {
        "\n\n"
    };
    let first = target
        .blocks
        .first()
        .map_or(target.text.len(), |b| b.start_byte);
    // what is kept in front: up to the end of the block before, or the text before the first block
    let (head, has_before) = match replace.start.checked_sub(1) {
        Some(before) => (&target.text[..target.blocks[before].end_byte], true),
        None => (&target.text[..first], false),
    };
    // and behind: from the start of the block after, or the text after the last block
    let (tail, has_after) = match target.blocks.get(replace.end) {
        Some(after) => (&target.text[after.start_byte..], true),
        None => (
            target
                .blocks
                .last()
                .map_or("", |last| &target.text[last.end_byte..]),
            false,
        ),
    };
    let middle = if with.is_empty() {
        Cow::Borrowed("")
    } else {
        let span = source.span(with);
        target.like_mine(&source.text[span.start..span.end])
    };
    let mut out = String::with_capacity(head.len() + middle.len() + tail.len() + 2 * blank.len());
    out.push_str(head);
    if has_before && (!middle.is_empty() || has_after) {
        out.push_str(blank);
    }
    out.push_str(&middle);
    if has_after && !middle.is_empty() {
        out.push_str(blank);
    }
    out.push_str(tail);
    out
}

/// The text of `target` with its blocks `replace` swapped for the blocks `with` of `source`.
///
/// Blocks that are edited in place are swapped for each other, and the separators around them stay as `target`
/// has them. Blocks that go or come in are swapped together with the separators between their neighbours, taken
/// from `source`: the text from the block before to the block after, so that what was a blank line stays one, and
/// two blocks that were tight against each other are not left glued into a paragraph. The same is done for edited
/// blocks when a separator next to them is not a blank line (a list that follows a paragraph straight on).
/// The rest of `target`, byte for byte, is left as it is. When the swap covers all the blocks of both texts, the
/// result is `source` itself.
fn splice(
    target: &Side<'_>,
    replace: &Range<usize>,
    source: &Side<'_>,
    with: &Range<usize>,
) -> String {
    if swaps_everything(target, replace, source, with) || (replace.is_empty() && with.is_empty()) {
        return if replace.is_empty() && with.is_empty() {
            target.text.to_owned()
        } else {
            source.text.to_owned()
        };
    }
    let tight = |side: &Side<'_>, range: &Range<usize>| {
        (range.start > 0 && !has_blank_line(side.gap_before(range.start)))
            || (range.end < side.blocks.len() && !has_blank_line(side.gap_before(range.end)))
    };
    let edited_in_place = !replace.is_empty() && !with.is_empty();
    let blocks_only = edited_in_place && !tight(target, replace);
    let (cut, put) = if blocks_only {
        (target.span(replace), source.span(with))
    } else {
        (target.region(replace), source.region(with))
    };
    let put = target.like_mine(&source.text[put.start..put.end]);
    let mut out = String::with_capacity(target.text.len() + put.len());
    out.push_str(&target.text[..cut.start]);
    out.push_str(&put);
    out.push_str(&target.text[cut.end..]);
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
