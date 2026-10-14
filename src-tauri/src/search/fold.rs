//! Text folding for search: lowercase, accent stripping and light English/Portuguese stemming.

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Tokens longer than this (hashes, base64 blobs) are not indexed.
const MAX_TOKEN_CHARS: usize = 48;

/// Lowercases and strips accents: `Descrição` becomes `descricao`.
pub(crate) fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
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
}

/// Splits text into words (runs of alphanumeric characters), skipping one-letter words and very long ones.
pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let push = |s: usize, e: usize, out: &mut Vec<Token>| {
        let word = &text[s..e];
        let n = word.chars().count();
        if (2..=MAX_TOKEN_CHARS).contains(&n) {
            let folded = fold(word);
            out.push(Token {
                start: s,
                end: e,
                term: stem(&folded),
                folded,
            });
        }
    };
    for (i, c) in text.char_indices() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                push(s, i, &mut out);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        push(s, text.len(), &mut out);
    }
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
        assert!(tokenize(&blob).is_empty());
        assert_eq!(tokenize(&format!("{blob} ok")).len(), 1);
    }

    #[test]
    fn tokenize_handles_empty_and_symbol_only_text() {
        assert!(tokenize("").is_empty());
        assert!(tokenize("--- *** !!!").is_empty());
    }
}
