//! The estimators: the shared model of `permark_core::tokens` with the weights of each family.
//!
//! Weights come from `tools/calibrate.py` (non-negative least squares on relative error over the committed corpus,
//! rounded to milli-tokens). `Interim` sets stand in for an exact count while it is pending and are never offered as a
//! choice. `gemini` is fitted against the tokenizer Google's SDK uses to count Gemini tokens locally. `claude` has no
//! public tokenizer to measure: it is the `o200k` set times 1.07, a factor chosen so the English prose sample of the
//! corpus comes out at 3.5 characters per token (the published rule of thumb). Its accuracy is unknown.

use permark_core::tokens::{Features, Weights};

use crate::tokenizer::{Encoding, Tokenizer};

pub const O200K_INTERIM: Weights = permark_core::tokens::GENERIC;

pub const CL100K_INTERIM: Weights = Weights {
    space: 124,
    newline: 421,
    lower: 0,
    upper: 47,
    digit: 521,
    lat_ext: 1833,
    cjk: 1338,
    greek_cyr: 639,
    letter_other: 1021,
    emoji: 4357,
    symbol: 595,
    punct: 455,
    word: 900,
    excess: 640,
};

pub const GEMINI: Weights = Weights {
    space: 123,
    newline: 916,
    lower: 30,
    upper: 161,
    digit: 1043,
    lat_ext: 532,
    cjk: 757,
    greek_cyr: 300,
    letter_other: 416,
    emoji: 2706,
    symbol: 632,
    punct: 675,
    word: 714,
    excess: 436,
};

pub const CLAUDE: Weights = Weights {
    space: 126,
    newline: 414,
    lower: 0,
    upper: 87,
    digit: 551,
    lat_ext: 781,
    cjk: 936,
    greek_cyr: 347,
    letter_other: 431,
    emoji: 2799,
    symbol: 1310,
    punct: 550,
    word: 921,
    excess: 623,
};

/// Tokens of `text` by the estimator of `tokenizer`. For an exact tokenizer this is its interim estimate.
#[must_use]
pub fn estimate(tokenizer: Tokenizer, text: &str) -> u64 {
    let weights = match tokenizer {
        Tokenizer::Exact(Encoding::O200k) => O200K_INTERIM,
        Tokenizer::Exact(Encoding::Cl100k) => CL100K_INTERIM,
        Tokenizer::Claude => CLAUDE,
        Tokenizer::Gemini => GEMINI,
        Tokenizer::Chars4 => return chars4(text),
    };
    weights.tokens(&Features::of(text))
}

/// Characters divided by four, rounded up: the rule of thumb, model-free.
#[must_use]
pub fn chars4(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

#[cfg(test)]
mod tests;
