//! The difference between two versions of a text, line by line. Pure.
//!
//! Lines are compared as they are (indentation and trailing blanks count: in some languages they are the program).
//! Equal lines are interned to small numbers first; the common start and end are cut; Myers' greedy algorithm
//! (O(N·D), D = the number of differing lines) finds the shortest edit script of the middle. When the texts differ
//! in more than [`MAX_D`] lines, lines that occur exactly once on each side (patience diff) split the middle, and each
//! piece is tried again; a piece that still has too many differences is one hunk. A line that moved is a removal
//! where it was and an addition where it went: two hunks, each kept or reverted alone.

use std::collections::HashMap;
use std::ops::Range;

use permark_core::edit::LineEdit;

/// Differences past which Myers' trace (a quarter of D² numbers) is not kept: 2 000 take about 16 MiB.
pub const MAX_D: usize = 2000;

/// A run of consecutive lines that differ, between lines that are the same in both versions. A range may be empty:
/// its position is where the lines would be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

impl Hunk {
    #[must_use]
    pub fn kind(&self) -> HunkKind {
        match (self.old.is_empty(), self.new.is_empty()) {
            (true, _) => HunkKind::Added,
            (false, true) => HunkKind::Removed,
            (false, false) => HunkKind::Edited,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkKind {
    Added,
    Removed,
    Edited,
}

/// The hunks that turn `old` into `new`, in order. Never adjacent: a line both versions have lies between two.
#[must_use]
pub fn hunks<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Hunk> {
    let mut ids: HashMap<&'a str, u32> = HashMap::with_capacity(old.len().min(1 << 16));
    let mut number = |line: &'a str| -> u32 {
        let next = u32::try_from(ids.len()).unwrap_or(u32::MAX);
        *ids.entry(line).or_insert(next)
    };
    let a: Vec<u32> = old.iter().map(|line| number(line)).collect();
    let b: Vec<u32> = new.iter().map(|line| number(line)).collect();
    let mut out = Vec::new();
    align(&a, &b, 0, 0, true, &mut out);
    out
}

/// The edit that replaces the lines `target` of the text being edited by the lines `range` of `source`.
#[must_use]
pub fn replace(target: &Range<usize>, source: &[&str], range: &Range<usize>) -> LineEdit {
    LineEdit {
        at: u32::try_from(target.start).unwrap_or(u32::MAX),
        remove: u32::try_from(target.len()).unwrap_or(u32::MAX),
        insert: source[range.clone()]
            .iter()
            .map(|line| (*line).to_owned())
            .collect(),
    }
}

/// Whether the two sides differ in whitespace only (indentation, trailing blanks, blank lines).
#[must_use]
pub fn whitespace_only(old: &[&str], new: &[&str]) -> bool {
    let strip = |lines: &[&str]| -> String {
        lines
            .iter()
            .flat_map(|l| l.chars())
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    strip(old) == strip(new)
}

/// Appends the hunks of `a` against `b` (numbers of lines); `a0`, `b0`: where the pieces start in the whole.
fn align(a: &[u32], b: &[u32], a0: usize, b0: usize, patience: bool, out: &mut Vec<Hunk>) {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let (a0, b0) = (a0 + prefix, b0 + prefix);
    let whole = || Hunk {
        old: a0..a0 + am.len(),
        new: b0..b0 + bm.len(),
    };
    if am.is_empty() && bm.is_empty() {
        return;
    }
    if am.is_empty() || bm.is_empty() {
        out.push(whole());
        return;
    }
    if let Some(pairs) = myers(am, bm) {
        hunks_between(&pairs, am.len(), bm.len(), a0, b0, out);
        return;
    }
    if !patience {
        out.push(whole());
        return;
    }
    let anchors = anchors(am, bm);
    if anchors.is_empty() {
        out.push(whole());
        return;
    }
    let (mut ai, mut bi) = (0, 0);
    for (pa, pb) in anchors {
        align(&am[ai..pa], &bm[bi..pb], a0 + ai, b0 + bi, false, out);
        ai = pa + 1;
        bi = pb + 1;
    }
    align(&am[ai..], &bm[bi..], a0 + ai, b0 + bi, false, out);
}

/// The hunks in the gaps between matched pairs (`(index in a, index in b)`, in order).
fn hunks_between(
    pairs: &[(usize, usize)],
    n: usize,
    m: usize,
    a0: usize,
    b0: usize,
    out: &mut Vec<Hunk>,
) {
    let (mut ai, mut bi) = (0, 0);
    for &(pa, pb) in pairs.iter().chain(std::iter::once(&(n, m))) {
        if ai < pa || bi < pb {
            out.push(Hunk {
                old: a0 + ai..a0 + pa,
                new: b0 + bi..b0 + pb,
            });
        }
        ai = pa + 1;
        bi = pb + 1;
    }
}

/// Myers' greedy diff: the matched pairs of a shortest edit script, in order; `None` past [`MAX_D`] differences.
fn myers(old: &[u32], new: &[u32]) -> Option<Vec<(usize, usize)>> {
    let (rows, cols) = (old.len(), new.len());
    let limit = (rows + cols).min(MAX_D);
    // furthest x on diagonal `diag = x - y`, kept at `diag + offset`
    let offset = limit + 1;
    let mut furthest = vec![0_i32; 2 * limit + 3];
    // the numbers before each round, only the part the walk back reads (diagonals -round - 1 ..= round + 1)
    let mut trace: Vec<Vec<i32>> = Vec::new();
    for round in 0..=limit {
        trace.push(furthest[offset - round - 1..=offset + round + 1].to_vec());
        for step in 0..=round {
            // the diagonal is 2 * step - round
            let slot = offset - round + 2 * step;
            let (left, right) = (furthest[slot - 1], furthest[slot + 1]);
            let from = if step == 0 || (step != round && left < right) {
                right
            } else {
                left + 1
            };
            let mut x = usize::try_from(from).ok()?;
            let mut y = (x + round).checked_sub(2 * step)?;
            while x < rows && y < cols && old[x] == new[y] {
                x += 1;
                y += 1;
            }
            furthest[slot] = i32::try_from(x).ok()?;
            if x >= rows && y >= cols {
                return Some(walk_back(&trace, round, x, y));
            }
        }
    }
    None
}

/// The matched pairs of the path that ended at `(x, y)` after `found` rounds.
fn walk_back(trace: &[Vec<i32>], found: usize, mut x: usize, mut y: usize) -> Vec<(usize, usize)> {
    let signed = |value: usize| isize::try_from(value).unwrap_or(0);
    let mut pairs = Vec::new();
    for round in (1..=found).rev() {
        let Some(snapshot) = trace.get(round) else {
            break;
        };
        let reach = signed(round);
        // the snapshot starts at diagonal -round - 1
        let read = |diag: isize| -> usize {
            usize::try_from(diag + reach + 1)
                .ok()
                .and_then(|i| snapshot.get(i))
                .and_then(|value| usize::try_from(*value).ok())
                .unwrap_or(0)
        };
        let diag = signed(x) - signed(y);
        let down = diag == -reach || (diag != reach && read(diag - 1) < read(diag + 1));
        let prev_diag = if down { diag + 1 } else { diag - 1 };
        let prev_x = read(prev_diag);
        let prev_y = usize::try_from(signed(prev_x) - prev_diag).unwrap_or(0);
        let (step_x, step_y) = if down {
            (prev_x, prev_y + 1)
        } else {
            (prev_x + 1, prev_y)
        };
        // the diagonal from the step to the end of the round: lines that are the same
        for at in (0..x.saturating_sub(step_x)).rev() {
            pairs.push((step_x + at, step_y + at));
        }
        x = prev_x;
        y = prev_y;
    }
    // round 0: the diagonal from the start
    for at in (0..x.min(y)).rev() {
        pairs.push((at, at));
    }
    pairs.reverse();
    pairs
}

/// Lines that occur once in `a` and once in `b`, as the longest run that keeps their order: pairs `(index in a,
/// index in b)` in order.
fn anchors(a: &[u32], b: &[u32]) -> Vec<(usize, usize)> {
    // number -> (times seen, index)
    let tally = |lines: &[u32]| -> HashMap<u32, (u32, usize)> {
        let mut map: HashMap<u32, (u32, usize)> = HashMap::new();
        for (i, line) in lines.iter().enumerate() {
            let entry = map.entry(*line).or_insert((0, i));
            entry.0 += 1;
        }
        map
    };
    let (in_a, in_b) = (tally(a), tally(b));
    // (index in b) of each line unique on both sides, in the order of a
    let mut candidates: Vec<(usize, usize)> = a
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let (times_a, _) = in_a.get(line)?;
            let (times_b, at_b) = in_b.get(line)?;
            (*times_a == 1 && *times_b == 1).then_some((i, *at_b))
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    // longest increasing run of the indexes in b (patience sorting)
    let mut tails: Vec<usize> = Vec::new(); // candidate index ending a run of this length
    let mut prev: Vec<Option<usize>> = vec![None; candidates.len()];
    for (c, &(_, bi)) in candidates.iter().enumerate() {
        let place = tails.partition_point(|&t| candidates[t].1 < bi);
        prev[c] = place.checked_sub(1).map(|p| tails[p]);
        if place == tails.len() {
            tails.push(c);
        } else {
            tails[place] = c;
        }
    }
    let mut chain = Vec::new();
    let mut at = tails.last().copied();
    while let Some(c) = at {
        chain.push(candidates[c]);
        at = prev[c];
    }
    chain.reverse();
    candidates.clear();
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<&str> {
        text.split('\n').collect()
    }

    /// The hunks of two texts as `(old, new)` ranges.
    fn diff(old: &str, new: &str) -> Vec<(Range<usize>, Range<usize>)> {
        hunks(&lines(old), &lines(new))
            .into_iter()
            .map(|h| (h.old, h.new))
            .collect()
    }

    /// Applies the hunks to `old`: must give `new`.
    fn rebuilt(old: &str, new: &str) -> String {
        let (a, b) = (lines(old), lines(new));
        let mut out: Vec<&str> = Vec::new();
        let mut at = 0;
        for h in hunks(&a, &b) {
            out.extend(&a[at..h.old.start]);
            out.extend(&b[h.new.clone()]);
            at = h.old.end;
        }
        out.extend(&a[at..]);
        out.join("\n")
    }

    #[test]
    fn equal_texts_have_no_hunks() {
        assert_eq!(diff("a\nb\nc", "a\nb\nc"), []);
        assert_eq!(diff("", ""), []);
        assert_eq!(diff("a\n", "a\n"), []);
    }

    #[test]
    fn insert_delete_and_replace_have_their_places() {
        // inserted
        assert_eq!(diff("a\nc", "a\nb\nc"), [(1..1, 1..2)]);
        // deleted
        assert_eq!(diff("a\nb\nc", "a\nc"), [(1..2, 1..1)]);
        // replaced
        assert_eq!(diff("a\nb\nc", "a\nX\nc"), [(1..2, 1..2)]);
        // at the start and at the end
        assert_eq!(diff("b\nc", "a\nb\nc"), [(0..0, 0..1)]);
        assert_eq!(diff("a\nb", "a\nb\nc"), [(2..2, 2..3)]);
        // two apart
        assert_eq!(
            diff("a\nb\nc\nd\ne", "a\nB\nc\nd\nE"),
            [(1..2, 1..2), (4..5, 4..5)]
        );
        // everything
        assert_eq!(diff("a\nb", "x\ny\nz"), [(0..2, 0..3)]);
    }

    #[test]
    fn a_moved_block_is_a_removal_and_an_addition() {
        let old = "one\ntwo\nthree\nfour\nfive\nsix";
        let new = "three\nfour\none\ntwo\nfive\nsix";
        let found = diff(old, new);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(rebuilt(old, new), new);
        // one of the two is a pure addition, the other a pure removal
        let kinds: Vec<_> = hunks(&lines(old), &lines(new))
            .iter()
            .map(Hunk::kind)
            .collect();
        assert!(kinds.contains(&HunkKind::Added) && kinds.contains(&HunkKind::Removed));
    }

    #[test]
    fn whitespace_changes_are_changes_and_told_apart() {
        let h = diff("fn a() {\n    x\n}", "fn a() {\n\tx\n}");
        assert_eq!(h, [(1..2, 1..2)]);
        assert!(whitespace_only(&["    x"], &["\tx"]));
        assert!(whitespace_only(&["x "], &["x"]));
        assert!(whitespace_only(&[], &["", "  "]));
        assert!(!whitespace_only(&["x"], &["y"]));
        // trailing blank removed
        assert_eq!(diff("a \nb", "a\nb"), [(0..1, 0..1)]);
        // an extra blank line
        assert_eq!(diff("a\nb", "a\n\nb"), [(1..1, 1..2)]);
    }

    #[test]
    fn the_end_of_the_text_is_a_line() {
        // no trailing newline against one: the last (empty) line differs
        assert_eq!(diff("a\nb", "a\nb\n"), [(2..2, 2..3)]);
        assert_eq!(diff("a\nb\n", "a\nb"), [(2..3, 2..2)]);
        // a line appended to a text that ended with a newline
        assert_eq!(diff("a\nb\n", "a\nb\nc\n"), [(2..2, 2..3)]);
        assert_eq!(rebuilt("a\nb", "a\nb\n"), "a\nb\n");
    }

    #[test]
    fn repeated_lines_do_not_confuse_the_script() {
        let old = "}\n\nfn a() {\n}\n\nfn b() {\n}\n";
        let new = "}\n\nfn a() {\n}\n\nfn c() {\n}\n\nfn b() {\n}\n";
        assert_eq!(rebuilt(old, new), new);
        let found = diff(old, new);
        assert_eq!(found.len(), 1, "{found:?}");
        // one block of three lines came in
        assert_eq!(found[0].1.len() - found[0].0.len(), 3);
    }

    #[test]
    fn odd_input_is_diffed_without_a_panic() {
        assert_eq!(diff("", "a"), [(0..1, 0..1)]);
        assert_eq!(diff("a", ""), [(0..1, 0..1)]);
        // one empty line less: which of the equal ones goes is up to the script, that it is one hunk is not
        let found = diff("\n\n\n", "\n\n");
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].0.len(), found[0].1.len()), (1, 0));
        assert_eq!(diff("ü\n日本語\n😀", "ü\n日本\n😀"), [(1..2, 1..2)]);
        let h = hunks(&[], &[]);
        assert!(h.is_empty());
    }

    /// A small deterministic generator for the property test.
    fn next(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    #[test]
    fn applying_the_hunks_always_gives_the_new_text() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        for round in 0..400 {
            let alphabet = 1 + next(&mut seed) % 6;
            let len_a = next(&mut seed) % 40;
            let len_b = next(&mut seed) % 40;
            let make = |len: u64, seed: &mut u64| -> String {
                (0..len)
                    .map(|_| format!("l{}", next(seed) % alphabet))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let (old, new) = (make(len_a, &mut seed), make(len_b, &mut seed));
            assert_eq!(
                rebuilt(&old, &new),
                new,
                "round {round}: {old:?} -> {new:?}"
            );
            // never adjacent, in order, no empty pair
            let found = diff(&old, &new);
            for pair in found.windows(2) {
                assert!(
                    pair[0].0.end < pair[1].0.start || pair[0].1.end < pair[1].1.start,
                    "adjacent hunks {found:?}"
                );
                assert!(pair[0].0.end <= pair[1].0.start && pair[0].1.end <= pair[1].1.start);
            }
            assert!(found.iter().all(|(o, n)| !o.is_empty() || !n.is_empty()));
        }
    }

    #[test]
    fn the_script_is_the_shortest_one() {
        // a classic: one insert, one delete
        let old = "a\nb\nc\na\nb\nb\na";
        let new = "c\nb\na\nb\na\nc";
        let (a, b) = (lines(old), lines(new));
        let removed: usize = hunks(&a, &b).iter().map(|h| h.old.len()).sum();
        let added: usize = hunks(&a, &b).iter().map(|h| h.new.len()).sum();
        // the longest common subsequence of the two is 4 lines long
        assert_eq!(a.len() - removed, 4);
        assert_eq!(b.len() - added, 4);
    }

    #[test]
    fn ten_thousand_lines_with_a_few_edits_are_quick() {
        let old: Vec<String> = (0..10_000).map(|i| format!("line {i}")).collect();
        let mut new = old.clone();
        new[10] = "changed".to_owned();
        new.insert(5_000, "added".to_owned());
        new.remove(9_000);
        for i in (2_000..3_000).step_by(37) {
            new[i] = format!("edited {i}");
        }
        let (a, b): (Vec<&str>, Vec<&str>) = (
            old.iter().map(String::as_str).collect(),
            new.iter().map(String::as_str).collect(),
        );
        let started = std::time::Instant::now();
        let found = hunks(&a, &b);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(found.len(), 3 + (2_000..3_000).step_by(37).count());
    }

    #[test]
    fn a_rewrite_of_everything_falls_back_to_anchors_then_to_one_hunk() {
        // 6 000 lines, every third one changed: far past the limit, but lines are unique, so anchors split it
        let old: Vec<String> = (0..6_000).map(|i| format!("unique {i}")).collect();
        let new: Vec<String> = old
            .iter()
            .enumerate()
            .map(|(i, l)| {
                if i % 3 == 0 {
                    format!("rewritten {i}")
                } else {
                    l.clone()
                }
            })
            .collect();
        let (a, b): (Vec<&str>, Vec<&str>) = (
            old.iter().map(String::as_str).collect(),
            new.iter().map(String::as_str).collect(),
        );
        let started = std::time::Instant::now();
        let found = hunks(&a, &b);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(found.len(), 2_000, "each rewritten line is its own hunk");
        // nothing in common at all: one hunk
        let other: Vec<String> = (0..6_000).map(|i| format!("other {i}")).collect();
        let c: Vec<&str> = other.iter().map(String::as_str).collect();
        let all = hunks(&a, &c);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].old, 0..6_000);
        assert_eq!(all[0].new, 0..6_000);
    }
}
