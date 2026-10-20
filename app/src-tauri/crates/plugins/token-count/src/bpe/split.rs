//! The splitting rules of the two encodings (their regular expressions), written as scanners.
//!
//! Each rule set is a list of alternatives; at a position the FIRST alternative that matches wins (not the longest),
//! and inside an alternative greedy parts give back what the rest needs, exactly like the reference engine. The
//! comment above each alternative is the pattern it follows. Every character matches some alternative, so the
//! scanners always move forward.

use super::unicode::{in_lower_set, in_upper_set, is_letter, is_number};

/// Which encoding's rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rules {
    O200k,
    Cl100k,
}

/// The pieces of `text`, in order; together they are the whole text.
pub fn pieces(text: &str, rules: Rules) -> Pieces<'_> {
    Pieces {
        text,
        pos: 0,
        rules,
    }
}

pub struct Pieces<'a> {
    text: &'a str,
    pos: usize,
    rules: Rules,
}

impl<'a> Iterator for Pieces<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.pos >= self.text.len() {
            return None;
        }
        let end = match self.rules {
            Rules::O200k => o200k_end(self.text, self.pos),
            Rules::Cl100k => cl100k_end(self.text, self.pos),
        };
        // a rule that matched nothing would loop for ever: take one character instead (not reachable by design)
        let end = if end > self.pos {
            end
        } else {
            self.pos
                + self.text[self.pos..]
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8)
        };
        let piece = &self.text[self.pos..end];
        self.pos = end;
        Some(piece)
    }
}

fn at(text: &str, pos: usize) -> Option<char> {
    text.get(pos..)?.chars().next()
}

/// `\s`
fn is_space(c: char) -> bool {
    c.is_whitespace()
}

fn is_newline(c: char) -> bool {
    c == '\r' || c == '\n'
}

/// `[^\r\n\p{L}\p{N}]`
fn is_prefix(c: char) -> bool {
    !is_newline(c) && !is_letter(c) && !is_number(c)
}

/// `[^\s\p{L}\p{N}]`
fn is_other(c: char) -> bool {
    !is_space(c) && !is_letter(c) && !is_number(c)
}

/// The end of the run from `pos` of characters that satisfy `keep`.
fn run_end(text: &str, mut pos: usize, keep: impl Fn(char) -> bool) -> usize {
    while let Some(c) = at(text, pos) {
        if !keep(c) {
            break;
        }
        pos += c.len_utf8();
    }
    pos
}

/// `\p{N}{1,3}` from `pos` (at least one number is there): the end.
fn digits_end(text: &str, mut pos: usize) -> usize {
    for _ in 0..3 {
        match at(text, pos) {
            Some(c) if is_number(c) => pos += c.len_utf8(),
            _ => break,
        }
    }
    pos
}

/// ` ?[^\s\p{L}\p{N}]+[\r\n]*` (with `extra` also allowed after the run, as `/` in `o200k`): the end, if it matches.
fn punctuation_end(text: &str, pos: usize, extra_slash: bool) -> Option<usize> {
    let mut q = pos;
    if at(text, q) == Some(' ') {
        q += 1;
    }
    // a space taken but nothing after it: backtrack to no space; the space is `\s`, so the run cannot start there
    let first = at(text, q)?;
    if !is_other(first) {
        return None;
    }
    let run = run_end(text, q, is_other);
    Some(run_end(text, run, |c| {
        is_newline(c) || (extra_slash && c == '/')
    }))
}

/// The whitespace run from `pos`: its end, and the end of its last `\r` | `\n` (after that character), if it has one.
fn space_run(text: &str, pos: usize) -> (usize, Option<usize>) {
    let mut end = pos;
    let mut last_newline = None;
    while let Some(c) = at(text, end) {
        if !is_space(c) {
            break;
        }
        end += c.len_utf8();
        if is_newline(c) {
            last_newline = Some(end);
        }
    }
    (end, last_newline)
}

/// `\s+(?!\S)`: the run without its last character, when the run is longer than one character or ends the text.
fn space_before_non_space(text: &str, pos: usize, end: usize) -> Option<usize> {
    if end >= text.len() {
        return Some(end);
    }
    let last = text[pos..end].chars().next_back()?;
    let shorter = end - last.len_utf8();
    (shorter > pos).then_some(shorter)
}

// ---- cl100k ---------------------------------------------------------------------------------------------------------

/// `(?i:[sdmt]|ll|ve|re)` after an apostrophe at `pos`: the end.
fn contraction_after_apostrophe(text: &str, pos: usize) -> Option<usize> {
    let rest = text.get(pos + 1..)?;
    let mut chars = rest.chars();
    let first = chars.next()?;
    let second = chars.next();
    let single = matches!(
        first,
        's' | 'S' | '\u{17f}' | 'd' | 'D' | 'm' | 'M' | 't' | 'T'
    );
    if single {
        return Some(pos + 1 + first.len_utf8());
    }
    let pair =
        |a: &[char], b: &[char]| a.contains(&first) && second.is_some_and(|s| b.contains(&s));
    (pair(&['l', 'L'], &['l', 'L'])
        || pair(&['v', 'V'], &['e', 'E'])
        || pair(&['r', 'R'], &['e', 'E']))
    .then(|| pos + 1 + first.len_utf8() + second.map_or(0, char::len_utf8))
}

fn cl100k_end(text: &str, pos: usize) -> usize {
    let Some(c) = at(text, pos) else { return pos };
    // '(?i:[sdmt]|ll|ve|re)
    if c == '\'' {
        if let Some(end) = contraction_after_apostrophe(text, pos) {
            return end;
        }
    }
    // [^\r\n\p{L}\p{N}]?+\p{L}++
    let letters_from = if is_prefix(c) {
        pos + c.len_utf8()
    } else {
        pos
    };
    if at(text, letters_from).is_some_and(is_letter) {
        return run_end(text, letters_from, is_letter);
    }
    // \p{N}{1,3}+
    if is_number(c) {
        return digits_end(text, pos);
    }
    //  ?[^\s\p{L}\p{N}]++[\r\n]*+
    if let Some(end) = punctuation_end(text, pos, false) {
        return end;
    }
    if is_space(c) {
        let (end, last_newline) = space_run(text, pos);
        // \s++$
        if end >= text.len() {
            return end;
        }
        // \s*[\r\n]
        if let Some(newline) = last_newline {
            return newline;
        }
        // \s+(?!\S)
        if let Some(shorter) = space_before_non_space(text, pos, end) {
            return shorter;
        }
        // \s
        return pos + c.len_utf8();
    }
    pos
}

// ---- o200k ----------------------------------------------------------------------------------------------------------

/// `(?i:'s|'t|'re|'ve|'m|'ll|'d)?` at `pos`: the end after it, or `pos` when it is not there.
fn contraction_o200k(text: &str, pos: usize) -> usize {
    if at(text, pos) != Some('\'') {
        return pos;
    }
    contraction_after_apostrophe(text, pos).unwrap_or(pos)
}

/// `[^\r\n\p{L}\p{N}]?` then the body of alternative 1 (`[Lu Lt Lm Lo M]*[Ll Lm Lo M]+`), from `start`: the end of
/// the letters (before the contraction), if the body matches there.
fn word_lower_end(text: &str, start: usize) -> Option<usize> {
    // the upper run is greedy and gives back one character at a time until a lower-set character follows
    let mut i = start;
    let mut last_lower = None;
    while let Some(c) = at(text, i) {
        if in_lower_set(c) {
            last_lower = Some(i);
        }
        if !in_upper_set(c) {
            break;
        }
        i += c.len_utf8();
    }
    let k = last_lower?;
    Some(run_end(text, k, in_lower_set))
}

/// Alternative 2 body from `start`: `[Lu Lt Lm Lo M]+[Ll Lm Lo M]*`.
fn word_upper_end(text: &str, start: usize) -> Option<usize> {
    let upper = run_end(text, start, in_upper_set);
    (upper > start).then(|| run_end(text, upper, in_lower_set))
}

/// Alternatives 1 and 2 of `o200k`, each with its optional prefix character taken first, then not taken.
fn word_end(text: &str, pos: usize, c: char) -> Option<usize> {
    let taken = is_prefix(c).then(|| pos + c.len_utf8());
    let starts = [taken, Some(pos)];
    for body in [word_lower_end, word_upper_end] {
        for start in starts.iter().flatten() {
            if let Some(end) = body(text, *start) {
                return Some(contraction_o200k(text, end));
            }
        }
    }
    None
}

fn o200k_end(text: &str, pos: usize) -> usize {
    let Some(c) = at(text, pos) else { return pos };
    // [^\r\n\p{L}\p{N}]?[Lu Lt Lm Lo M]*[Ll Lm Lo M]+(?i:...)?  |  [^\r\n\p{L}\p{N}]?[Lu Lt Lm Lo M]+[Ll Lm Lo M]*(?i:...)?
    if let Some(end) = word_end(text, pos, c) {
        return end;
    }
    // \p{N}{1,3}
    if is_number(c) {
        return digits_end(text, pos);
    }
    //  ?[^\s\p{L}\p{N}]+[\r\n/]*
    if let Some(end) = punctuation_end(text, pos, true) {
        return end;
    }
    if is_space(c) {
        let (end, last_newline) = space_run(text, pos);
        // \s*[\r\n]+
        if let Some(newline) = last_newline {
            return newline;
        }
        // \s+(?!\S)
        if let Some(shorter) = space_before_non_space(text, pos, end) {
            return shorter;
        }
        // \s+ (reached only by a single space before a non-space)
        return end;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str, rules: Rules) -> Vec<&str> {
        pieces(text, rules).collect()
    }

    #[test]
    fn the_pieces_always_make_the_text() {
        let samples = [
            "",
            "hello world",
            "  \n  x \t\r\n y",
            "a\u{301}b \u{4f60}\u{597d} 123456 !!!\n\n/x",
            "\u{1c5}ellow \u{2b0}abc",
        ];
        for rules in [Rules::O200k, Rules::Cl100k] {
            for text in samples {
                assert_eq!(split(text, rules).concat(), text, "{rules:?} {text:?}");
            }
        }
    }

    #[test]
    fn cl100k_words_numbers_and_punctuation() {
        assert_eq!(
            split("Hello, world! 12345", Rules::Cl100k),
            ["Hello", ",", " world", "!", " ", "123", "45"]
        );
        assert_eq!(split("don't", Rules::Cl100k), ["don", "'t"]);
        assert_eq!(split("I'LL 'ſ", Rules::Cl100k), ["I", "'LL", " '", "ſ"]);
        assert_eq!(split("'ſ", Rules::Cl100k), ["'ſ"]);
        assert_eq!(split("x...\n\ny", Rules::Cl100k), ["x", "...\n\n", "y"]);
    }

    #[test]
    fn cl100k_whitespace_rules() {
        // \s*[\r\n] takes the run through its last newline
        assert_eq!(split("a  \n  b", Rules::Cl100k), ["a", "  \n", " ", " b"]);
        // \s+(?!\S) leaves one space for the next word
        assert_eq!(split("a   b", Rules::Cl100k), ["a", "  ", " b"]);
        // \s++$ takes the whole trailing run
        assert_eq!(split("a \n  ", Rules::Cl100k), ["a", " \n  "]);
        assert_eq!(split("a  ", Rules::Cl100k), ["a", "  "]);
    }

    #[test]
    fn o200k_case_rules() {
        assert_eq!(split("HelloWorld", Rules::O200k), ["Hello", "World"]);
        assert_eq!(split("HTTPServer", Rules::O200k), ["HTTPServer"]);
        assert_eq!(split("HELLO", Rules::O200k), ["HELLO"]);
        assert_eq!(split("don't DON'T", Rules::O200k), ["don't", " DON'T"]);
        assert_eq!(split(" word", Rules::O200k), [" word"]);
        // the upper run gives back characters that are also lower-set (Lo)
        assert_eq!(
            split("\u{4f60}\u{597d}", Rules::O200k),
            ["\u{4f60}\u{597d}"]
        );
    }

    #[test]
    fn o200k_punctuation_keeps_newlines_and_slashes() {
        assert_eq!(split("a //\n/b", Rules::O200k), ["a", " //\n/", "b"]);
        assert_eq!(
            split("end\n/ next", Rules::O200k),
            ["end", "\n", "/", " next"]
        );
    }

    #[test]
    fn o200k_whitespace_at_the_end() {
        assert_eq!(split("a   ", Rules::O200k), ["a", "   "]);
        assert_eq!(split("a \n  ", Rules::O200k), ["a", " \n", "  "]);
    }
}
