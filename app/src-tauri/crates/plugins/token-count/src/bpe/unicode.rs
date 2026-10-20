//! General categories the splitting rules ask about, from the generated table.

use super::unicode_tables::RANGES;

/// The categories as the table codes them.
pub(super) const NONE: u8 = 0;
pub(super) const LU: u8 = 1;
pub(super) const LL: u8 = 2;
pub(super) const LT: u8 = 3;
pub(super) const LM: u8 = 4;
pub(super) const LO: u8 = 5;
pub(super) const MARK: u8 = 6;
pub(super) const NUMBER: u8 = 7;

/// Bit sets over the codes: letters, and the two classes of the `o200k` word rules.
const LETTER: u8 = 0b0011_1110;
const UPPER_SET: u8 = (1 << LU) | (1 << LT) | (1 << LM) | (1 << LO) | (1 << MARK);
const LOWER_SET: u8 = (1 << LL) | (1 << LM) | (1 << LO) | (1 << MARK);

const ASCII: [u8; 128] = {
    let mut table = [NONE; 128];
    let mut c = 0;
    while c < 128 {
        table[c] = match c {
            0x41..=0x5A => LU,
            0x61..=0x7A => LL,
            0x30..=0x39 => NUMBER,
            _ => NONE,
        };
        c += 1;
    }
    table
};

/// The category code of `c` (0 = none of the ones the rules name).
pub(super) fn category(c: char) -> u8 {
    let cp = u32::from(c);
    if cp < 128 {
        return ASCII[cp as usize];
    }
    let at = RANGES.partition_point(|&(start, _, _)| start <= cp);
    match at.checked_sub(1).map(|i| RANGES[i]) {
        Some((_, end, code)) if cp <= end => code,
        _ => NONE,
    }
}

pub(super) fn is_letter(c: char) -> bool {
    LETTER & (1 << category(c)) != 0
}

pub(super) fn is_number(c: char) -> bool {
    category(c) == NUMBER
}

/// `[\p{Lu}\p{Lt}\p{Lm}\p{Lo}\p{M}]`
pub(super) fn in_upper_set(c: char) -> bool {
    UPPER_SET & (1 << category(c)) != 0
}

/// `[\p{Ll}\p{Lm}\p{Lo}\p{M}]`
pub(super) fn in_lower_set(c: char) -> bool {
    LOWER_SET & (1 << category(c)) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_of_known_characters() {
        assert_eq!(category('A'), LU);
        assert_eq!(category('z'), LL);
        assert_eq!(category('7'), NUMBER);
        assert_eq!(category(' '), NONE);
        assert_eq!(category('\u{e9}'), LL);
        assert_eq!(category('\u{c9}'), LU);
        assert_eq!(category('\u{1c5}'), LT);
        assert_eq!(category('\u{2b0}'), LM);
        assert_eq!(category('\u{4f60}'), LO);
        assert_eq!(category('\u{301}'), MARK);
        assert_eq!(category('\u{0967}'), NUMBER);
        assert_eq!(category('\u{2167}'), NUMBER);
        assert_eq!(category('\u{bd}'), NUMBER);
        assert_eq!(category('\u{1f600}'), NONE);
        assert_eq!(category('\u{10ffff}'), NONE);
        assert_eq!(category('\u{e000}'), NONE);
    }

    #[test]
    fn sets_overlap_where_the_rules_do() {
        assert!(in_upper_set('\u{4f60}') && in_lower_set('\u{4f60}'));
        assert!(in_upper_set('\u{301}') && in_lower_set('\u{301}'));
        assert!(in_upper_set('A') && !in_lower_set('A'));
        assert!(in_lower_set('a') && !in_upper_set('a'));
        assert!(is_letter('\u{1c5}') && in_upper_set('\u{1c5}') && !in_lower_set('\u{1c5}'));
        assert!(!is_letter('\u{301}') && !is_letter('7'));
    }

    #[test]
    fn every_range_is_sorted_and_apart() {
        for pair in RANGES.windows(2) {
            assert!(pair[0].1 < pair[1].0, "{:X?} {:X?}", pair[0], pair[1]);
        }
        assert!(RANGES.iter().all(|r| r.0 <= r.1 && (1..=7).contains(&r.2)));
    }
}
