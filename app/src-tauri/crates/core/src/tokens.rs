//! Tokens: the estimator every part of the program shares, and the seam through which the `token-count` plugin hands
//! its counter to core consumers (the agent connection).
//!
//! The estimator is a weighted sum over the characters of a text, by class (Latin letters, digits, CJK, punctuation,
//! ...) and by word, with integer weights in milli-tokens: the result never depends on floating point, so the Rust and
//! the TypeScript copies (`plugins/token-count/estimate.ts`) agree on every text. Classes come from code point ranges
//! only, never from Unicode property tables.
//!
//! [`estimate`] uses the generic weights: good enough to tell how big a part of a document | an answer for a model is,
//! never presented as the count of a model. A tab and ASCII punctuation right after ASCII punctuation are features of
//! their own: tab tables and JSON are made of them, and a weight shared with spaces | lone punctuation misjudged both
//! (JSON came out ~80 % bigger than a tab table of the same data, real tokenizers say ~43 %).

use std::sync::Arc;

use crate::settings::ProjectConfig;

/// What kind of character: decides which weight it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Space,
    Tab,
    Newline,
    Lower,
    Upper,
    Digit,
    LatinExt,
    Cjk,
    GreekCyr,
    LetterOther,
    Emoji,
    Symbol,
    Punct,
}

/// The class of a code point (first match of the ranges below).
#[must_use]
pub fn class_of(c: char) -> Class {
    let cp = u32::from(c);
    match cp {
        0x09 => Class::Tab,
        0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000 => Class::Space,
        0x0A..=0x0D | 0x85 | 0x2028 | 0x2029 => Class::Newline,
        0x30..=0x39 => Class::Digit,
        0x41..=0x5A => Class::Upper,
        0x61..=0x7A => Class::Lower,
        0x00..=0x7F => Class::Punct,
        0x80..=0xBF | 0xD7 | 0xF7 => Class::Symbol,
        0xC0..=0x36F | 0x1E00..=0x1EFF => Class::LatinExt,
        0x2E80..=0x2FDF
        | 0x3040..=0x30FF
        | 0x3100..=0x312F
        | 0x3190..=0x31FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF
        | 0x20000..=0x3FFFF => Class::Cjk,
        0x370..=0x52F | 0x1F00..=0x1FFF => Class::GreekCyr,
        0x530..=0x1EFF | 0x2C00..=0x2DFF | 0xA000..=0xABFF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF => {
            Class::LetterOther
        }
        0x1F000..=0x1FAFF
        | 0x2600..=0x27BF
        | 0x2B00..=0x2BFF
        | 0xFE00..=0xFE0F
        | 0x200D
        | 0xE0000..=0xE007F => Class::Emoji,
        _ => Class::Symbol,
    }
}

/// A word has this many characters before the rest count as `excess`.
const EXCESS_FROM: u64 = 6;

/// What a text is made of: characters by class, words, and the characters of words beyond their sixth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Features {
    pub space: u64,
    pub tab: u64,
    pub newline: u64,
    pub lower: u64,
    pub upper: u64,
    pub digit: u64,
    pub lat_ext: u64,
    pub cjk: u64,
    pub greek_cyr: u64,
    pub letter_other: u64,
    pub emoji: u64,
    pub symbol: u64,
    /// ASCII punctuation not right after ASCII punctuation.
    pub punct: u64,
    /// ASCII punctuation right after ASCII punctuation (`":"`, `},{`): mostly merges into the token before it.
    pub punct_join: u64,
    /// Maximal runs of digits | letters of the Latin classes.
    pub word: u64,
    pub excess: u64,
}

impl Features {
    #[must_use]
    pub fn of(text: &str) -> Self {
        let mut f = Self::default();
        let mut run = 0_u64;
        let mut previous = None;
        for c in text.chars() {
            let class = class_of(c);
            match class {
                Class::Space => f.space += 1,
                Class::Tab => f.tab += 1,
                Class::Newline => f.newline += 1,
                Class::Lower => f.lower += 1,
                Class::Upper => f.upper += 1,
                Class::Digit => f.digit += 1,
                Class::LatinExt => f.lat_ext += 1,
                Class::Cjk => f.cjk += 1,
                Class::GreekCyr => f.greek_cyr += 1,
                Class::LetterOther => f.letter_other += 1,
                Class::Emoji => f.emoji += 1,
                Class::Symbol => f.symbol += 1,
                Class::Punct if previous == Some(Class::Punct) => f.punct_join += 1,
                Class::Punct => f.punct += 1,
            }
            previous = Some(class);
            if matches!(
                class,
                Class::Lower | Class::Upper | Class::Digit | Class::LatinExt
            ) {
                if run == 0 {
                    f.word += 1;
                }
                run += 1;
                if run > EXCESS_FROM {
                    f.excess += 1;
                }
            } else {
                run = 0;
            }
        }
        f
    }
}

/// Milli-tokens per unit of each feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weights {
    pub space: u64,
    pub tab: u64,
    pub newline: u64,
    pub lower: u64,
    pub upper: u64,
    pub digit: u64,
    pub lat_ext: u64,
    pub cjk: u64,
    pub greek_cyr: u64,
    pub letter_other: u64,
    pub emoji: u64,
    pub symbol: u64,
    pub punct: u64,
    pub punct_join: u64,
    pub word: u64,
    pub excess: u64,
}

impl Weights {
    /// The estimate: sum of weight x count in milli-tokens, rounded up to whole tokens.
    #[must_use]
    pub fn tokens(&self, f: &Features) -> u64 {
        let milli = self.space * f.space
            + self.tab * f.tab
            + self.newline * f.newline
            + self.lower * f.lower
            + self.upper * f.upper
            + self.digit * f.digit
            + self.lat_ext * f.lat_ext
            + self.cjk * f.cjk
            + self.greek_cyr * f.greek_cyr
            + self.letter_other * f.letter_other
            + self.emoji * f.emoji
            + self.symbol * f.symbol
            + self.punct * f.punct
            + self.punct_join * f.punct_join
            + self.word * f.word
            + self.excess * f.excess;
        milli.div_ceil(1000)
    }
}

/// The generic weights: fitted against `o200k` on a mixed corpus (prose in many scripts, code, config, data) plus
/// answers for models in tab-table, JSON and TOON form (`token-count` crate, `tools/calibrate.py`).
pub const GENERIC: Weights = Weights {
    space: 121,
    tab: 593,
    newline: 712,
    lower: 0,
    upper: 12,
    digit: 497,
    lat_ext: 625,
    cjk: 888,
    greek_cyr: 317,
    letter_other: 396,
    emoji: 3050,
    symbol: 1026,
    punct: 639,
    punct_join: 182,
    word: 837,
    excess: 568,
};

/// Approximate tokens of a text with the generic weights: a size hint, not the count of any model.
#[must_use]
pub fn estimate(text: &str) -> u64 {
    GENERIC.tokens(&Features::of(text))
}

/// A count, the tokenizer it belongs to (its id) and whether it is only an estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counted {
    pub value: u64,
    pub tokenizer: &'static str,
    pub approx: bool,
}

/// What the agent connection asks of the token count plugin.
pub trait TokenCounter: Send + Sync {
    /// Tokens of `text` by the primary tokenizer of the project whose settings are `project` (its own choice over the
    /// app's). `None`: no tokenizer is chosen, or its vocabulary is not on this computer.
    fn primary(&self, project: &ProjectConfig, text: &str) -> Option<Counted>;
}

/// The counter, offered through [`crate::plugins::Services`] while the plugin runs.
#[derive(Clone)]
pub struct TokenCounterApi(pub Arc<dyn TokenCounter>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_by_range() {
        let cases = [
            (' ', Class::Space),
            ('\t', Class::Tab),
            ('\u{a0}', Class::Space),
            ('\u{3000}', Class::Space),
            ('\n', Class::Newline),
            ('\r', Class::Newline),
            ('\u{2028}', Class::Newline),
            ('7', Class::Digit),
            ('Q', Class::Upper),
            ('q', Class::Lower),
            ('!', Class::Punct),
            ('\u{0}', Class::Punct),
            ('\u{7f}', Class::Punct),
            ('\u{a9}', Class::Symbol),
            ('\u{d7}', Class::Symbol),
            ('\u{e9}', Class::LatinExt),
            ('\u{301}', Class::LatinExt),
            ('\u{1ea0}', Class::LatinExt),
            ('\u{4f60}', Class::Cjk),
            ('\u{30ab}', Class::Cjk),
            ('\u{d55c}', Class::Cjk),
            ('\u{20000}', Class::Cjk),
            ('\u{416}', Class::GreekCyr),
            ('\u{3b3}', Class::GreekCyr),
            ('\u{5d0}', Class::LetterOther),
            ('\u{627}', Class::LetterOther),
            ('\u{e01}', Class::LetterOther),
            ('\u{1f600}', Class::Emoji),
            ('\u{2705}', Class::Emoji),
            ('\u{200d}', Class::Emoji),
            ('\u{2192}', Class::Symbol),
            ('\u{ff21}', Class::Symbol),
            ('\u{e000}', Class::Symbol),
        ];
        for (c, class) in cases {
            assert_eq!(class_of(c), class, "U+{:04X}", u32::from(c));
        }
    }

    #[test]
    fn features_count_words_and_excess() {
        let f = Features::of("abc abcdefghij 12ab, x");
        assert_eq!(f.word, 4);
        // "abcdefghij" has 4 characters past the sixth
        assert_eq!(f.excess, 4);
        assert_eq!(f.space, 3);
        assert_eq!(f.punct, 1);
    }

    #[test]
    fn the_estimate_is_zero_for_nothing_and_grows_with_text() {
        assert_eq!(estimate(""), 0);
        let one = estimate("hello");
        let two = estimate("hello world");
        assert!(one >= 1 && two > one);
    }
}
