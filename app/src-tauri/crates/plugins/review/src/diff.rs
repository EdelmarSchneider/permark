//! The difference between two versions of a document, block by block. Pure.
//!
//! The blocks are the ones the page is made of (content ids: equal rendered HTML, equal id), so a change of spacing
//! or a blank line is no change. A longest common subsequence finds the blocks that stayed; everything between two of
//! them is one [`Change`]: the old blocks that went and the new blocks that came, a run of consecutive ones. A block
//! that moved is a removal where it was and an addition where it went (two changes, each kept or reverted alone).

use std::ops::Range;

/// Above this many id pairs (the middle between the first and the last block that differ) blocks are not matched:
/// the whole middle is one change.
pub const MAX_CELLS: usize = 4_000_000;

/// What a change is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// New blocks, no old ones.
    Added,
    /// Old blocks, no new ones.
    Removed,
    /// Old blocks replaced by new ones.
    Edited,
}

/// A run of consecutive blocks that differ, between two blocks that are the same in both versions. A range may be
/// empty: its position is where the blocks would be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

impl Change {
    #[must_use]
    pub fn kind(&self) -> Kind {
        match (self.old.is_empty(), self.new.is_empty()) {
            (true, _) => Kind::Added,
            (false, true) => Kind::Removed,
            (false, false) => Kind::Edited,
        }
    }
}

/// The content part of a block id: repeats of the same block are `<id>-<n>`, and where a repeat sits does not make
/// the block different.
#[must_use]
pub fn content_of(id: &str) -> &str {
    id.split('-').next().unwrap_or(id)
}

/// The changes between two sequences of block ids, in document order.
#[must_use]
pub fn changes(old: &[String], new: &[String]) -> Vec<Change> {
    let (a, b): (Vec<&str>, Vec<&str>) = (
        old.iter().map(|id| content_of(id)).collect(),
        new.iter().map(|id| content_of(id)).collect(),
    );
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a_mid, b_mid) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    if a_mid.is_empty() && b_mid.is_empty() {
        return Vec::new();
    }
    let whole = || {
        vec![Change {
            old: prefix..prefix + a_mid.len(),
            new: prefix..prefix + b_mid.len(),
        }]
    };
    if a_mid.is_empty() || b_mid.is_empty() || a_mid.len().saturating_mul(b_mid.len()) > MAX_CELLS {
        return whole();
    }
    let pairs = matched_pairs(a_mid, b_mid);
    let mut out = Vec::new();
    let (mut ai, mut bi) = (0, 0);
    for (ma, mb) in pairs
        .into_iter()
        .chain(std::iter::once((a_mid.len(), b_mid.len())))
    {
        if ai < ma || bi < mb {
            out.push(Change {
                old: prefix + ai..prefix + ma,
                new: prefix + bi..prefix + mb,
            });
        }
        ai = ma + 1;
        bi = mb + 1;
    }
    out
}

/// Indices `(old, new)` of the blocks of a longest common subsequence, in order.
fn matched_pairs(old: &[&str], new: &[&str]) -> Vec<(usize, usize)> {
    let (rows, cols) = (old.len(), new.len());
    // lengths of the common subsequence of the tails old[row..] and new[col..]
    let width = cols + 1;
    let mut table = vec![0_u32; (rows + 1) * width];
    for row in (0..rows).rev() {
        for col in (0..cols).rev() {
            table[row * width + col] = if old[row] == new[col] {
                table[(row + 1) * width + col + 1] + 1
            } else {
                table[(row + 1) * width + col].max(table[row * width + col + 1])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut row, mut col) = (0, 0);
    while row < rows && col < cols {
        if old[row] == new[col] {
            pairs.push((row, col));
            row += 1;
            col += 1;
        } else if table[(row + 1) * width + col] >= table[row * width + col + 1] {
            row += 1;
        } else {
            col += 1;
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    fn diff(old: &str, new: &str) -> Vec<(Kind, Range<usize>, Range<usize>)> {
        changes(&ids(old), &ids(new))
            .into_iter()
            .map(|c| (c.kind(), c.old, c.new))
            .collect()
    }

    #[test]
    fn nothing_differs_nothing_changes() {
        assert_eq!(diff("a b c", "a b c"), []);
        assert_eq!(diff("", ""), []);
    }

    #[test]
    fn additions_removals_and_edits_are_found_with_their_places() {
        assert_eq!(diff("a b", "a x b"), [(Kind::Added, 1..1, 1..2)]);
        assert_eq!(diff("a x b", "a b"), [(Kind::Removed, 1..2, 1..1)]);
        assert_eq!(diff("a x b", "a y b"), [(Kind::Edited, 1..2, 1..2)]);
        assert_eq!(diff("", "a b"), [(Kind::Added, 0..0, 0..2)]);
        assert_eq!(diff("a b", ""), [(Kind::Removed, 0..2, 0..0)]);
        assert_eq!(diff("a b c", "a b c d"), [(Kind::Added, 3..3, 3..4)]);
        assert_eq!(diff("z a b c", "a b c"), [(Kind::Removed, 0..1, 0..0)]);
    }

    #[test]
    fn several_changes_are_separate_and_runs_stay_together() {
        assert_eq!(
            diff("a b c d e", "a X c Y Z e"),
            [(Kind::Edited, 1..2, 1..2), (Kind::Edited, 3..4, 3..5)]
        );
        assert_eq!(
            diff("a b c d", "a c d x y z"),
            [(Kind::Removed, 1..2, 1..1), (Kind::Added, 4..4, 3..6)]
        );
    }

    #[test]
    fn a_moved_block_is_a_removal_and_an_addition() {
        let got = diff("a b c d", "a c d b");
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got.iter().any(|c| c.0 == Kind::Removed));
        assert!(got.iter().any(|c| c.0 == Kind::Added));
    }

    #[test]
    fn repeated_blocks_do_not_turn_into_changes_when_a_repeat_is_added() {
        // `h-1` is the second block with the content of `h`
        let old = vec!["h".to_owned(), "x".to_owned(), "h-1".to_owned()];
        let new = vec![
            "h".to_owned(),
            "x".to_owned(),
            "h-1".to_owned(),
            "h-2".to_owned(),
        ];
        let got = changes(&old, &new);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].kind(), Kind::Added);
        assert_eq!(got[0].new, 3..4);
    }

    #[test]
    fn a_huge_middle_is_one_change_not_a_hang() {
        let old: Vec<String> = (0..3000).map(|n| format!("o{n}")).collect();
        let new: Vec<String> = (0..3000).map(|n| format!("n{n}")).collect();
        let started = std::time::Instant::now();
        let got = changes(&old, &new);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].old.clone(), got[0].new.clone()), (0..3000, 0..3000));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn the_changes_always_rebuild_the_new_sequence_from_the_old_one() {
        // property: applying the changes to `old` gives `new`
        let words = ["a", "b", "c", "d", "e", "f"];
        let mut seed = 12345_u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            usize::try_from(seed >> 33).unwrap()
        };
        for _ in 0..300 {
            let old: Vec<String> = (0..next() % 9)
                .map(|_| words[next() % 6].to_owned())
                .collect();
            let new: Vec<String> = (0..next() % 9)
                .map(|_| words[next() % 6].to_owned())
                .collect();
            let mut rebuilt: Vec<String> = Vec::new();
            let mut at = 0;
            for change in changes(&old, &new) {
                rebuilt.extend_from_slice(&old[at..change.old.start]);
                rebuilt.extend_from_slice(&new[change.new.clone()]);
                at = change.old.end;
            }
            rebuilt.extend_from_slice(&old[at..]);
            assert_eq!(rebuilt, new, "old {old:?}");
        }
    }
}
