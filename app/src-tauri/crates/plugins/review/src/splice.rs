//! Line edits that keep or revert one change. Pure, on the text with every line break as `\n` (the numbering is the
//! one of the file itself: `n` breaks, `n + 1` lines).
//!
//! A change is made in the text that has to look like the other one ("target" from "source"), replacing only the lines
//! of the blocks and the one separator that goes with them; every other line of the target is left as it is, so the
//! file is saved through the editor's surgical line save.

use std::ops::Range;

use permark_core::edit::LineEdit;

/// Lines of a block, 1-based, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

/// A text as the splices see it: its lines and the lines its blocks cover.
#[derive(Debug)]
pub struct Text<'a> {
    pub lines: Vec<&'a str>,
    pub blocks: &'a [Span],
}

impl<'a> Text<'a> {
    #[must_use]
    pub fn new(text: &'a str, blocks: &'a [Span]) -> Self {
        Self {
            lines: text.split('\n').collect(),
            blocks,
        }
    }

    fn count(&self) -> u32 {
        u32::try_from(self.lines.len()).unwrap_or(u32::MAX)
    }

    /// The lines from the first block of `range` to the end of its last (1-based, inclusive).
    fn span(&self, range: &Range<usize>) -> Result<Span, CannotSplice> {
        let first = self.blocks.get(range.start).ok_or(CannotSplice)?;
        let last = self
            .blocks
            .get(range.end.wrapping_sub(1))
            .ok_or(CannotSplice)?;
        if first.start == 0 || first.start > last.end || last.end > self.count() {
            return Err(CannotSplice);
        }
        Ok(Span {
            start: first.start,
            end: last.end,
        })
    }

    /// Lines `first..=last` (1-based) as owned strings.
    fn take(&self, span: Span) -> Vec<String> {
        self.lines[(span.start - 1) as usize..span.end as usize]
            .iter()
            .map(|line| (*line).to_owned())
            .collect()
    }
}

/// The blocks of the text do not line up with its lines, so the change cannot be made on its own (keeping or
/// reverting everything still can).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CannotSplice;

/// The edit that makes `target` look like `source` for one change: the blocks `t` of the target become the blocks `s`
/// of the source.
///
/// # Errors
/// `CannotSplice`: a block range or line range does not fit its text.
pub fn edit_for(
    target: &Text<'_>,
    t: &Range<usize>,
    source: &Text<'_>,
    s: &Range<usize>,
) -> Result<LineEdit, CannotSplice> {
    match (t.is_empty(), s.is_empty()) {
        (true, true) => Err(CannotSplice),
        // the blocks are replaced by the other version's
        (false, false) => {
            let there = target.span(t)?;
            let from = source.span(s)?;
            Ok(LineEdit {
                at: there.start - 1,
                remove: there.end - there.start + 1,
                insert: source.take(from),
            })
        }
        // the blocks only the source has come in, with one blank line to set them apart
        (true, false) => {
            let lines = source.take(source.span(s)?);
            Ok(
                match (
                    t.start.checked_sub(1).and_then(|i| target.blocks.get(i)),
                    target.blocks.get(t.start),
                ) {
                    (Some(prev), _) => LineEdit {
                        at: prev.end,
                        remove: 0,
                        insert: std::iter::once(String::new()).chain(lines).collect(),
                    },
                    (None, Some(next)) => LineEdit {
                        at: next.start - 1,
                        remove: 0,
                        insert: lines
                            .into_iter()
                            .chain(std::iter::once(String::new()))
                            .collect(),
                    },
                    // nothing but blank lines: the blocks come first
                    (None, None) => LineEdit {
                        at: 0,
                        remove: 0,
                        insert: lines,
                    },
                },
            )
        }
        // the blocks only the target has go, with the blank lines that set them apart
        (false, true) => {
            let there = target.span(t)?;
            let prev = t.start.checked_sub(1).and_then(|i| target.blocks.get(i));
            Ok(match (target.blocks.get(t.end), prev) {
                (Some(next), _) => LineEdit {
                    at: there.start - 1,
                    remove: next.start - there.start,
                    insert: Vec::new(),
                },
                (None, Some(prev)) => LineEdit {
                    at: prev.end,
                    remove: there.end - prev.end,
                    insert: Vec::new(),
                },
                (None, None) => LineEdit {
                    at: there.start - 1,
                    remove: there.end - there.start + 1,
                    insert: Vec::new(),
                },
            })
        }
    }
}

/// The edits that undo `edits` (given in the coordinates of `before`): positions are those of the text the edits
/// produce.
#[must_use]
pub fn invert(edits: &[LineEdit], before: &[&str]) -> Vec<LineEdit> {
    let mut shift: i64 = 0;
    let mut out = Vec::with_capacity(edits.len());
    for edit in edits {
        let start = (edit.at as usize).min(before.len());
        let end = (start + edit.remove as usize).min(before.len());
        out.push(LineEdit {
            at: u32::try_from(i64::from(edit.at) + shift).unwrap_or(0),
            remove: u32::try_from(edit.insert.len()).unwrap_or(u32::MAX),
            insert: before[start..end].iter().map(|l| (*l).to_owned()).collect(),
        });
        shift += i64::try_from(edit.insert.len()).unwrap_or(0) - i64::from(edit.remove);
    }
    out
}

#[cfg(test)]
mod tests {
    use permark_core::edit::lines::apply;

    use super::*;

    /// Blocks of a text where paragraphs are separated by blank lines (one block per run of non-blank lines).
    fn blocks_of(text: &str) -> Vec<Span> {
        let mut out = Vec::new();
        let mut start: Option<u32> = None;
        for (i, line) in text.split('\n').enumerate() {
            let n = u32::try_from(i + 1).unwrap();
            match (line.is_empty(), start) {
                (false, None) => start = Some(n),
                (true, Some(s)) => {
                    out.push(Span {
                        start: s,
                        end: n - 1,
                    });
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            out.push(Span {
                start: s,
                end: u32::try_from(text.split('\n').count()).unwrap(),
            });
        }
        out
    }

    fn run(target: &str, t: Range<usize>, source: &str, s: Range<usize>) -> String {
        let (tb, sb) = (blocks_of(target), blocks_of(source));
        let edit = edit_for(&Text::new(target, &tb), &t, &Text::new(source, &sb), &s).unwrap();
        apply(target, &[edit]).unwrap()
    }

    #[test]
    fn an_edited_block_gets_the_other_versions_lines() {
        let cur = "# T\n\nnew text\nsecond line\n\nend\n";
        let base = "# T\n\nold text\n\nend\n";
        assert_eq!(run(cur, 1..2, base, 1..2), base);
        assert_eq!(run(base, 1..2, cur, 1..2), cur);
    }

    #[test]
    fn an_added_block_goes_with_one_blank_line_in_either_direction() {
        let cur = "a\n\nB\n\nc\n";
        let base = "a\n\nc\n";
        // revert the addition: B and the blank run after it go
        assert_eq!(run(cur, 1..2, base, 1..1), base);
        // keep it: B comes into the base after a
        assert_eq!(run(base, 1..1, cur, 1..2), cur);
    }

    #[test]
    fn a_block_at_the_top_or_the_end_is_added_and_removed_cleanly() {
        let with_top = "TOP\n\na\n\nb\n";
        let plain = "a\n\nb\n";
        assert_eq!(run(with_top, 0..1, plain, 0..0), plain);
        assert_eq!(run(plain, 0..0, with_top, 0..1), with_top);
        let with_end = "a\n\nb\n\nEND\n";
        assert_eq!(run(with_end, 2..3, plain, 2..2), "a\n\nb\n");
        assert_eq!(run(plain, 2..2, with_end, 2..3), "a\n\nb\n\nEND\n");
        // an empty document gets the block
        assert_eq!(run("", 0..0, "x\n", 0..1), "x\n");
    }

    #[test]
    fn a_run_of_blocks_is_replaced_whole() {
        let cur = "a\n\nX\n\nY\n\nz\n";
        let base = "a\n\nq\n\nz\n";
        assert_eq!(run(cur, 1..3, base, 1..2), base);
        assert_eq!(run(base, 1..2, cur, 1..3), cur);
    }

    #[test]
    fn crlf_files_keep_their_endings_through_the_editors_apply() {
        // the file has CRLF; the edit numbers are those of the lines
        let file = "a\r\n\r\nNEW\r\n\r\nz\r\n";
        let normalized = "a\n\nNEW\n\nz\n";
        let base = "a\n\nz\n";
        let (tb, sb) = (blocks_of(normalized), blocks_of(base));
        let edit = edit_for(
            &Text::new(normalized, &tb),
            &(1..2),
            &Text::new(base, &sb),
            &(1..1),
        )
        .unwrap();
        assert_eq!(apply(file, &[edit]).unwrap(), "a\r\n\r\nz\r\n");
        // a block put back takes the file's line ending
        let (tb, sb) = (blocks_of(base), blocks_of(normalized));
        let edit = edit_for(
            &Text::new(base, &tb),
            &(1..1),
            &Text::new(normalized, &sb),
            &(1..2),
        )
        .unwrap();
        assert_eq!(apply("a\r\n\r\nz\r\n", &[edit]).unwrap(), file);
    }

    #[test]
    fn several_changes_apply_together_and_invert_back() {
        let cur = "a\n\nB\n\nc\n\nd2\n\ne\n";
        let base = "a\n\nc\n\nd\n\ne\n\nf\n";
        let (cb, bb) = (blocks_of(cur), blocks_of(base));
        let (target, source) = (Text::new(cur, &cb), Text::new(base, &bb));
        // B added; d2 edited; f removed (at the end)
        let edits = vec![
            edit_for(&target, &(1..2), &source, &(1..1)).unwrap(),
            edit_for(&target, &(3..4), &source, &(2..3)).unwrap(),
            edit_for(&target, &(5..5), &source, &(4..5)).unwrap(),
        ];
        let reverted = apply(cur, &edits).unwrap();
        assert_eq!(reverted, base);
        let back = invert(&edits, &target.lines);
        assert_eq!(apply(&reverted, &back).unwrap(), cur);
    }

    #[test]
    fn ranges_that_do_not_fit_are_refused_not_applied() {
        let text = "a\n\nb\n";
        let blocks = blocks_of(text);
        let t = Text::new(text, &blocks);
        assert_eq!(edit_for(&t, &(0..0), &t, &(0..0)), Err(CannotSplice));
        assert_eq!(edit_for(&t, &(0..5), &t, &(0..1)), Err(CannotSplice));
        assert_eq!(edit_for(&t, &(3..4), &t, &(0..1)), Err(CannotSplice));
        let lying = [Span { start: 1, end: 99 }];
        let bad = Text::new(text, &lying);
        assert_eq!(edit_for(&bad, &(0..1), &t, &(0..1)), Err(CannotSplice));
        let zero = [Span { start: 0, end: 1 }];
        assert_eq!(
            edit_for(&Text::new(text, &zero), &(0..1), &t, &(0..1)),
            Err(CannotSplice)
        );
    }
}
