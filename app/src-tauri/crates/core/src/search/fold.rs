//! Text folding for search: NFKD without accents, lowercase, light English + Portuguese stemming, and tokens
//! (words, CJK characters and pairs). Pure.

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Words longer than this (hashes, base64 blobs) are not indexed.
const MAX_TOKEN_CHARS: usize = 48;

/// Lowercase, accents gone (`Descrição` -> `descricao`), compatibility forms unified (`ﬁ` -> `fi`, full width ->
/// plain). The kana voicing marks stay: they tell `か` from `が`.
#[must_use]
pub fn fold(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    s.nfkd()
        .filter(|c| !is_combining_mark(*c) || matches!(*c, '\u{3099}' | '\u{309A}'))
        .flat_map(char::to_lowercase)
        .map(base_letter)
        .collect()
}

/// The base letter of a lowercase letter that has no decomposition to strip: Vietnamese `đ`, Turkish dotless `ı`
/// (`İ` needs no entry: it decomposes to `I` + a combining dot).
#[must_use]
pub(super) fn base_letter(c: char) -> char {
    match c {
        'đ' => 'd',
        'ı' => 'i',
        _ => c,
    }
}

fn ends_with_vowel_m(w: &str) -> bool {
    let b = w.as_bytes();
    b.len() >= 2
        && b[b.len() - 1] == b'm'
        && matches!(b[b.len() - 2], b'a' | b'e' | b'i' | b'o' | b'u')
}

/// Light stemmer for folded tokens: a consistent function (same forms collapse to one key on both sides), not a
/// linguist. English: plural `-s -es -ies`, `-ing`, `-ed`, final `-e`. Portuguese: `-s`, `-oes|-aes -> -ao`,
/// `-m|-ns`. Tokens that are not plain ASCII are left alone.
#[must_use]
pub fn stem(token: &str) -> String {
    if token.len() <= 3 || !token.is_ascii() {
        return token.to_owned();
    }
    let mut w = token.to_owned();
    if w.ends_with("oes") || w.ends_with("aes") {
        w.truncate(w.len() - 3);
        w.push_str("ao");
    } else if w.ends_with("ies") && w.len() > 4 {
        w.truncate(w.len() - 3);
        w.push('y');
    } else if ["ses", "xes", "zes", "ches", "shes"]
        .iter()
        .any(|s| w.ends_with(s))
    {
        w.truncate(w.len() - 2);
    } else if w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") && !w.ends_with("is") {
        w.pop();
    }
    if w.ends_with("ing") && w.len() >= 6 {
        w.truncate(w.len() - 3);
    } else if w.ends_with("ed") && w.len() >= 5 {
        w.truncate(w.len() - 2);
    }
    if w.ends_with('e') && w.len() >= 5 {
        w.pop();
    }
    if ends_with_vowel_m(&w) {
        w.pop();
        w.push('n');
    }
    w
}

/// The start every folded word with stem `term` has: [`stem`] keeps a word's beginning and rewrites at most its last
/// two letters (`-oes` -> `-ao`, `-ies` -> `-y`, `-m` -> `-n`); a term that is not plain ASCII is the word itself.
#[must_use]
pub(super) fn stem_lead(term: &str) -> &str {
    if term.is_ascii() && term.len() >= 3 {
        &term[..term.len() - 2]
    } else {
        term
    }
}

/// What a [`Token`] stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A run of alphanumerics that is not CJK.
    Word,
    /// A CJK character that stands alone, or an overlapping pair of CJK characters.
    Cjk,
    /// One character of a CJK run of two or more: indexed (a one-character query finds it inside longer text), but
    /// queries drop it, the pairs already carry the match.
    CjkRunUnigram,
}

/// A word found in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Byte range in the original text.
    pub start: usize,
    pub end: usize,
    /// Folded, not stemmed (prefix matching).
    pub folded: String,
    /// Folded and stemmed (index key).
    pub term: String,
    pub kind: TokenKind,
}

/// Han ideographs, Hiragana and Katakana (full and half width): written without spaces, so they are indexed as
/// characters and character pairs. Hangul is not here: Korean uses spaces.
#[must_use]
pub fn is_cjk(c: char) -> bool {
    matches!(u32::from(c),
        0x3005..=0x3007
        | 0x3041..=0x3096 | 0x309D..=0x309F
        | 0x30A1..=0x30FA | 0x30FC..=0x30FF
        | 0x31F0..=0x31FF
        | 0xFF66..=0xFF9F
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0x20000..=0x2FA1F | 0x30000..=0x323AF
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Cjk,
}

fn classify(c: char) -> Option<Class> {
    if !c.is_alphanumeric() {
        None
    } else if is_cjk(c) {
        Some(Class::Cjk)
    } else {
        Some(Class::Word)
    }
}

fn make_token(text: &str, start: usize, end: usize, kind: TokenKind) -> Token {
    let folded = fold(&text[start..end]);
    Token {
        start,
        end,
        term: stem(&folded),
        folded,
        kind,
    }
}

fn push_word(text: &str, start: usize, end: usize, out: &mut Vec<Token>) {
    let n = text[start..end].chars().count();
    if (2..=MAX_TOKEN_CHARS).contains(&n) {
        out.push(make_token(text, start, end, TokenKind::Word));
    }
}

/// Every character and every overlapping pair of a CJK run, in text order (a run has no length limit).
fn push_cjk_run(text: &str, start: usize, end: usize, out: &mut Vec<Token>) {
    let bounds: Vec<usize> = text[start..end]
        .char_indices()
        .map(|(i, _)| start + i)
        .chain(std::iter::once(end))
        .collect();
    let chars = bounds.len() - 1;
    let unigram = if chars == 1 {
        TokenKind::Cjk
    } else {
        TokenKind::CjkRunUnigram
    };
    for i in 0..chars {
        out.push(make_token(text, bounds[i], bounds[i + 1], unigram));
        if i + 1 < chars {
            out.push(make_token(text, bounds[i], bounds[i + 2], TokenKind::Cjk));
        }
    }
}

/// Whether `c` belongs to a word of a spaced script (a run [`tokenize`] makes one token of).
#[must_use]
pub(super) fn is_word_char(c: char) -> bool {
    classify(c) == Some(Class::Word)
}

/// The runs tokens come from, in text order: `(start, end, cjk)` byte ranges of the longest stretches of word
/// characters | CJK characters. [`tokenize`] of one run gives exactly the tokens the whole text has inside it.
pub(super) fn runs(text: &str) -> impl Iterator<Item = (usize, usize, bool)> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        let (start, class) = loop {
            let (at, c) = chars.next()?;
            if let Some(class) = classify(c) {
                break (at, class);
            }
        };
        let mut end = text.len();
        while let Some(&(at, c)) = chars.peek() {
            if classify(c) != Some(class) {
                end = at;
                break;
            }
            chars.next();
        }
        Some((start, end, class == Class::Cjk))
    })
}

/// Splits text into words (runs of alphanumerics; one-letter and very long words skipped). A CJK run yields its
/// characters and their overlapping pairs; a switch between CJK and other characters ends a word
/// (`使用Rust编写` holds `Rust` and two runs).
#[must_use]
pub fn tokenize(text: &str) -> Vec<Token> {
    #[cfg(test)]
    TOKENIZED.with(|n| n.set(n.get() + text.len()));
    let mut out = Vec::new();
    for (start, end, cjk) in runs(text) {
        if cjk {
            push_cjk_run(text, start, end, &mut out);
        } else {
            push_word(text, start, end, &mut out);
        }
    }
    out
}

#[cfg(test)]
thread_local! {
    /// Bytes this thread handed to [`tokenize`].
    static TOKENIZED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Bytes the calling thread tokenized so far (queries run on the caller's thread).
#[cfg(test)]
pub(super) fn tokenized_bytes() -> usize {
    TOKENIZED.with(std::cell::Cell::get)
}
