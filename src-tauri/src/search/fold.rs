//! Text folding for search: lowercase, accent stripping and light English/Portuguese stemming.

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Tokens longer than this (hashes, base64 blobs) are not indexed.
const MAX_TOKEN_CHARS: usize = 48;

/// Lowercases and strips accents: `Descrição` becomes `descricao`. The Kana voiced marks stay,
/// since they tell `か` from `が`.
pub(crate) fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c) || matches!(*c, '\u{3099}' | '\u{309A}'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn ends_with_vowel_m(w: &str) -> bool {
    let b = w.as_bytes();
    b.len() >= 2
        && b[b.len() - 1] == b'm'
        && matches!(b[b.len() - 2], b'a' | b'e' | b'i' | b'o' | b'u')
}

/// Light stemmer for already folded tokens. It only has to be a consistent function: the same
/// word forms must collapse to the same key on the index side and the query side.
///
/// English: plural `-s/-es/-ies`, `-ing`, `-ed`, final `-e`.
/// Portuguese: `-s`, `-oes/-aes` to `-ao`, and `-m`/`-ns` (`personagem`, `personagens`).
/// Tokens that are not plain ASCII after folding are left alone.
pub(crate) fn stem(token: &str) -> String {
    if token.len() <= 3 || !token.is_ascii() {
        return token.to_owned();
    }
    let mut w = token.to_owned();

    // Plurals.
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

    // Verb endings.
    if w.ends_with("ing") && w.len() >= 6 {
        w.truncate(w.len() - 3);
    } else if w.ends_with("ed") && w.len() >= 5 {
        w.truncate(w.len() - 2);
    }

    // Final -e (describe/described/describes all end up as `describ`).
    if w.ends_with('e') && w.len() >= 5 {
        w.pop();
    }

    // Portuguese singular -m versus plural -ns.
    if ends_with_vowel_m(&w) {
        w.pop();
        w.push('n');
    }
    w
}

/// What a [`Token`] stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// A run of alphanumeric characters that is not CJK.
    Word,
    /// A CJK character that stands alone, or an overlapping pair of CJK characters.
    Cjk,
    /// A single character of a CJK run of two or more. It is indexed, so a one-character query finds
    /// it inside longer text, but queries drop it: the bigrams already carry the match.
    CjkRunUnigram,
}

/// A word found in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    /// Byte range in the original text.
    pub start: usize,
    pub end: usize,
    /// Folded word, not stemmed (used for prefix matching).
    pub folded: String,
    /// Folded and stemmed word (used as index key).
    pub term: String,
    pub kind: TokenKind,
}

/// Han ideographs, Hiragana and Katakana (full and half width). Chinese and Japanese are written
/// without spaces, so [`tokenize`] cannot find words in them and indexes character pairs instead.
/// Hangul is not here: Korean uses spaces.
pub(crate) fn is_cjk(c: char) -> bool {
    matches!(u32::from(c),
        0x3005..=0x3007 // iteration mark, closing mark, ideographic zero
        | 0x3041..=0x3096 | 0x309D..=0x309F // Hiragana
        | 0x30A1..=0x30FA | 0x30FC..=0x30FF // Katakana, with the prolonged sound mark
        | 0x31F0..=0x31FF // Katakana phonetic extensions
        | 0xFF66..=0xFF9F // half-width Katakana
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF // Han: Extension A, Unified
        | 0xF900..=0xFAFF // Han: compatibility
        | 0x20000..=0x2FA1F | 0x30000..=0x323AF // Han: Extensions B and later, supplement
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

/// One token for a word, unless it is a one-letter word or a very long one.
fn push_word(text: &str, start: usize, end: usize, out: &mut Vec<Token>) {
    let n = text[start..end].chars().count();
    if (2..=MAX_TOKEN_CHARS).contains(&n) {
        out.push(make_token(text, start, end, TokenKind::Word));
    }
}

/// Every character and every overlapping pair of a CJK run, in text order. Unlike words, runs have
/// no length limit: a whole paragraph is one run.
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

/// Splits text into words (runs of alphanumeric characters), skipping one-letter words and very long ones.
/// CJK text has no spaces: each run of it yields its characters and their overlapping pairs instead, and
/// a switch between CJK and other characters ends a word (`使用Rust编写` holds `Rust` plus two runs).
pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut run: Option<(usize, Class)> = None;
    let flush = |run: Option<(usize, Class)>, end: usize, out: &mut Vec<Token>| match run {
        Some((s, Class::Word)) => push_word(text, s, end, out),
        Some((s, Class::Cjk)) => push_cjk_run(text, s, end, out),
        None => {}
    };
    for (i, c) in text.char_indices() {
        let class = classify(c);
        if class != run.map(|(_, k)| k) {
            flush(run, i, &mut out);
            run = class.map(|k| (i, k));
        }
    }
    flush(run, text.len(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_lowercases_and_strips_accents() {
        assert_eq!(
            fold("Descrição dos PERSONAGENS"),
            "descricao dos personagens"
        );
        assert_eq!(fold("Crème Brûlée ñ"), "creme brulee n");
    }

    #[test]
    fn english_forms_collapse() {
        let key = |w: &str| stem(&fold(w));
        assert_eq!(key("character"), key("characters"));
        assert_eq!(key("description"), key("descriptions"));
        assert_eq!(key("describe"), key("described"));
        assert_eq!(key("describe"), key("describes"));
        assert_eq!(key("describe"), key("describing"));
        assert_eq!(key("story"), key("stories"));
        assert_eq!(key("class"), key("classes"));
        assert_eq!(key("note"), key("notes"));
        assert_eq!(key("table"), key("tables"));
        assert_eq!(key("box"), key("boxes"));
        assert_ne!(key("class"), key("clash"));
    }

    #[test]
    fn portuguese_forms_collapse() {
        let key = |w: &str| stem(&fold(w));
        assert_eq!(key("descrição"), key("descrições"));
        assert_eq!(key("personagem"), key("personagens"));
        assert_eq!(key("livro"), key("livros"));
        assert_eq!(key("mulher"), key("mulheres"));
        assert_eq!(key("ação"), key("ações"));
        assert_eq!(key("vez"), key("vezes"));
    }

    #[test]
    fn short_and_non_ascii_tokens_are_untouched() {
        assert_eq!(stem("is"), "is");
        assert_eq!(stem("gas"), "gas");
        assert_eq!(stem("你好吗"), "你好吗");
        assert_eq!(stem("status"), "status");
        assert_eq!(stem("analysis"), "analysis");
    }

    #[test]
    fn tokenize_returns_spans_into_the_original_text() {
        let text = "Olá, mundo! A_b c42 x";
        let toks = tokenize(text);
        let words: Vec<&str> = toks.iter().map(|t| &text[t.start..t.end]).collect();
        // One-letter words are skipped; `_` splits words.
        assert_eq!(words, ["Olá", "mundo", "c42"]);
        assert_eq!(toks[0].folded, "ola");
    }

    #[test]
    fn tokenize_skips_giant_tokens() {
        let blob = "a".repeat(100);
        assert_eq!(tokenize(&blob).len(), 0);
        assert_eq!(tokenize(&format!("{blob} ok")).len(), 1);
    }

    #[test]
    fn tokenize_handles_empty_and_symbol_only_text() {
        assert_eq!(tokenize("").len(), 0);
        assert_eq!(tokenize("--- *** !!!").len(), 0);
    }

    fn spans<'a>(text: &'a str, toks: &[Token]) -> Vec<&'a str> {
        toks.iter().map(|t| &text[t.start..t.end]).collect()
    }

    #[test]
    fn chinese_runs_become_unigrams_and_overlapping_bigrams() {
        let text = "搜索测试";
        let toks = tokenize(text);
        assert_eq!(
            spans(text, &toks),
            ["搜", "搜索", "索", "索测", "测", "测试", "试"]
        );
        // The unigrams of a multi-character run are marked so queries can drop them.
        let kinds: Vec<TokenKind> = toks.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            [
                TokenKind::CjkRunUnigram,
                TokenKind::Cjk,
                TokenKind::CjkRunUnigram,
                TokenKind::Cjk,
                TokenKind::CjkRunUnigram,
                TokenKind::Cjk,
                TokenKind::CjkRunUnigram,
            ]
        );
        // Terms are the folded characters themselves: no stemming for CJK.
        assert_eq!(toks[1].term, "搜索");
        assert_eq!(toks[1].folded, "搜索");
    }

    #[test]
    fn a_single_cjk_character_is_one_ordinary_unigram() {
        let text = "买 书 去";
        let toks = tokenize(text);
        assert_eq!(spans(text, &toks), ["买", "书", "去"]);
        assert!(toks.iter().all(|t| t.kind == TokenKind::Cjk));
        // One-letter words of other scripts are still skipped.
        let mixed = "a 我 b";
        assert_eq!(spans(mixed, &tokenize(mixed)), ["我"]);
    }

    #[test]
    fn a_switch_between_cjk_and_other_alphanumerics_ends_a_word() {
        let text = "使用Rust编写 v2";
        let toks = tokenize(text);
        assert_eq!(
            spans(text, &toks),
            ["使", "使用", "用", "Rust", "编", "编写", "写", "v2"]
        );
        let rust = &toks[3];
        assert_eq!(rust.kind, TokenKind::Word);
        assert_eq!(rust.folded, "rust");
        assert_eq!(rust.term, "rust");
    }

    #[test]
    fn japanese_kana_and_kanji_form_one_run() {
        let text = "こんにちは、カタカナ漢字";
        let toks = tokenize(text);
        let words = spans(text, &toks);
        // Five characters (9 tokens), the comma breaks, six characters (11 tokens).
        assert_eq!(toks.len(), 20, "{words:?}");
        assert!(words.contains(&"こん") && words.contains(&"ちは"));
        assert!(!words.contains(&"はカ"), "punctuation must break the run");
        assert!(words.contains(&"カタ") && words.contains(&"ナ漢") && words.contains(&"漢字"));

        // The prolonged sound mark and half-width katakana belong to the run.
        let long = "コーヒー";
        assert_eq!(
            spans(long, &tokenize(long)),
            ["コ", "コー", "ー", "ーヒ", "ヒ", "ヒー", "ー"]
        );
        let half = "ｺﾋﾟｰ";
        assert_eq!(tokenize(half).len(), 7);
    }

    #[test]
    fn kana_voiced_marks_survive_folding() {
        assert_ne!(fold("が"), fold("か"));
        assert_ne!(fold("ぱ"), fold("は"));
        // Precomposed and decomposed spellings still agree.
        assert_eq!(fold("が"), fold("か\u{3099}"));
        // Latin accents are still stripped.
        assert_eq!(fold("é"), "e");
    }

    #[test]
    fn hangul_and_other_scripts_keep_whole_word_tokens() {
        let text = "한국어 검색 привет";
        let toks = tokenize(text);
        assert_eq!(spans(text, &toks), ["한국어", "검색", "привет"]);
        assert!(toks.iter().all(|t| t.kind == TokenKind::Word));
    }

    #[test]
    fn long_cjk_runs_are_not_dropped() {
        let text = "测".repeat(100);
        // 100 unigrams and 99 bigrams.
        assert_eq!(tokenize(&text).len(), 199);
        assert_eq!(tokenize(&format!("{text}，{text}")).len(), 398);
    }

    #[test]
    fn cjk_punctuation_and_symbols_break_runs() {
        let text = "搜索，测试。（读）・写";
        let words = spans(text, &tokenize(text));
        assert!(words.contains(&"搜索") && words.contains(&"测试"));
        assert!(!words.contains(&"索测") && !words.contains(&"试读"));
        assert!(
            !words.contains(&"读写"),
            "the katakana middle dot breaks a run"
        );
    }
}
