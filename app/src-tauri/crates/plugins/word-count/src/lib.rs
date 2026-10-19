//! Plugin `word-count`: words, characters and reading time of a document or a selection. The frontend mirror
//! (`src/plugins/word-count/count.ts`) runs the same steps on the same fixtures (`fixtures.json`).
//!
//! Markdown is measured without its syntax: front matter, fenced code, link reference definitions, HTML tags,
//! inline code, link and image addresses, footnote references, quote | heading | list markers and task boxes are
//! left out; link text and image alt text count. A word is a run of letters | digits (Unicode alphabetic | numeric,
//! combining accents U+0300..U+036F); `'` or `’` between two word characters joins; every CJK ideograph, kana and
//! hangul syllable is a word of its own.

use std::sync::Arc;

use permark_core::format::{Analysis, Contributions, Measure, MeasureUnit, TextKind};
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

/// Words a minute (reading); CJK characters a minute.
const WORDS_PER_MINUTE: u64 = 230;
const CJK_PER_MINUTE: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub words: u64,
    pub cjk: u64,
    pub characters: u64,
}

impl Counts {
    /// Minutes to read, rounded up (0 for no words).
    #[must_use]
    pub fn minutes(&self) -> u64 {
        let latin = self.words - self.cjk;
        let scaled = latin * CJK_PER_MINUTE + self.cjk * WORDS_PER_MINUTE;
        scaled.div_ceil(WORDS_PER_MINUTE * CJK_PER_MINUTE)
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || ('\u{300}'..='\u{36F}').contains(&c)
}

/// Words and characters of plain text.
#[must_use]
pub fn count(text: &str) -> Counts {
    let mut counts = Counts::default();
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\n' && c != '\r' {
            counts.characters += 1;
        }
        if is_cjk(c) {
            counts.words += 1;
            counts.cjk += 1;
            in_word = false;
        } else if is_word_char(c) {
            if !in_word {
                counts.words += 1;
                in_word = true;
            }
        } else if (c == '\'' || c == '\u{2019}')
            && in_word
            && chars
                .peek()
                .is_some_and(|n| is_word_char(*n) && !is_cjk(*n))
        {
            // apostrophe inside a word
        } else {
            in_word = false;
        }
    }
    counts
}

/// A fence line (up to 3 spaces, 3+ backticks | tildes): (char, length).
fn fence_of(line: &str) -> Option<(char, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let c = rest.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = rest.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

fn closes_fence(line: &str, fence: (char, usize)) -> bool {
    fence_of(line).is_some_and(|(c, n)| {
        c == fence.0 && n >= fence.1 && {
            let trimmed = line.trim_start_matches(' ');
            trimmed[n * c.len_utf8()..].trim().is_empty()
        }
    })
}

/// `[label]: target` (up to 3 spaces).
fn is_definition(line: &str) -> bool {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return false;
    }
    let rest = &line[indent..];
    rest.starts_with('[') && rest.find("]:").is_some_and(|at| at > 1)
}

/// Line start without quote, heading and list markers and a task box.
fn strip_line_start(line: &str) -> &str {
    let mut s = line.trim_start_matches([' ', '\t']);
    loop {
        if let Some(rest) = s.strip_prefix('>') {
            s = rest.trim_start_matches([' ', '\t']);
            continue;
        }
        let hashes = s.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) {
            let rest = &s[hashes..];
            if rest.is_empty() || rest.starts_with([' ', '\t']) {
                s = rest.trim_start_matches([' ', '\t']);
                continue;
            }
        }
        if let Some(rest) = s.strip_prefix(['-', '*', '+']) {
            if rest.starts_with([' ', '\t']) {
                s = rest.trim_start_matches([' ', '\t']);
                continue;
            }
        }
        let digits = s.chars().take_while(char::is_ascii_digit).count();
        if (1..=9).contains(&digits) {
            let rest = &s[digits..];
            if let Some(after) = rest.strip_prefix(['.', ')']) {
                if after.starts_with([' ', '\t']) {
                    s = after.trim_start_matches([' ', '\t']);
                    continue;
                }
            }
        }
        for box_ in ["[ ]", "[x]", "[X]"] {
            if let Some(rest) = s.strip_prefix(box_) {
                if rest.starts_with([' ', '\t']) {
                    return rest.trim_start_matches([' ', '\t']);
                }
            }
        }
        return s;
    }
}

/// Inline syntax out: code spans, tags, link | image addresses, footnote references.
fn strip_inline(line: &str, out: &mut String) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '`' => {
                let n = chars[i..].iter().take_while(|x| **x == '`').count();
                let close = (i + n..chars.len()).find(|&j| {
                    chars[j] == '`'
                        && chars[j..].iter().take_while(|x| **x == '`').count() == n
                        && (j == 0 || chars[j - 1] != '`')
                });
                if let Some(j) = close {
                    i = j + n;
                } else {
                    out.extend(&chars[i..i + n]);
                    i += n;
                }
            }
            '<' => {
                let close = chars[i + 1..].iter().position(|x| *x == '>');
                let tag_like = chars
                    .get(i + 1)
                    .is_some_and(|n| n.is_ascii_alphabetic() || *n == '/' || *n == '!');
                match close {
                    Some(k) if tag_like => i += k + 2,
                    _ => {
                        out.push(c);
                        i += 1;
                    }
                }
            }
            '[' => {
                let Some(k) = chars[i + 1..].iter().position(|x| *x == ']') else {
                    out.push(c);
                    i += 1;
                    continue;
                };
                let end = i + 1 + k;
                let inner: String = chars[i + 1..end].iter().collect();
                if inner.starts_with('^') {
                    i = end + 1;
                    continue;
                }
                match chars.get(end + 1) {
                    Some('(') => {
                        if let Some(p) = chars[end + 2..].iter().position(|x| *x == ')') {
                            strip_inline(&inner, out);
                            i = end + 2 + p + 1;
                        } else {
                            out.push(c);
                            i += 1;
                        }
                    }
                    Some('[') => {
                        if let Some(p) = chars[end + 2..].iter().position(|x| *x == ']') {
                            strip_inline(&inner, out);
                            i = end + 2 + p + 1;
                        } else {
                            out.push(c);
                            i += 1;
                        }
                    }
                    _ => {
                        out.push(c);
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
}

/// The words of a Markdown text, syntax left out (see the module header).
#[must_use]
pub fn strip_markdown(text: &str) -> String {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut start = 0;
    if let Some(first) = lines.first() {
        let delimiter = first.trim_end();
        if delimiter == "---" || delimiter == "+++" {
            if let Some(close) = lines.iter().skip(1).position(|l| {
                let l = l.trim_end();
                l == delimiter || (delimiter == "---" && l == "...")
            }) {
                start = close + 2;
            }
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<(char, usize)> = None;
    for line in lines.iter().skip(start) {
        if let Some(open) = fence {
            if closes_fence(line, open) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = fence_of(line) {
            fence = Some(open);
            continue;
        }
        if is_definition(line) {
            continue;
        }
        strip_inline(strip_line_start(line), &mut out);
        out.push('\n');
    }
    out
}

/// Counts of `text` as `kind`.
#[must_use]
pub fn measure(text: &str, kind: TextKind) -> Counts {
    match kind {
        TextKind::Markdown => count(&strip_markdown(text)),
        TextKind::Plain => count(text),
    }
}

struct WordCount;

impl Analysis for WordCount {
    fn id(&self) -> &'static str {
        "word-count"
    }

    fn measure(&self, text: &str, kind: TextKind) -> Vec<Measure> {
        let counts = measure(text, kind);
        vec![
            Measure {
                id: "word-count.words".to_owned(),
                value: counts.words,
                unit: MeasureUnit::Words,
            },
            Measure {
                id: "word-count.characters".to_owned(),
                value: counts.characters,
                unit: MeasureUnit::Characters,
            },
            Measure {
                id: "word-count.reading".to_owned(),
                value: counts.minutes(),
                unit: MeasureUnit::Minutes,
            },
        ]
    }
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "word-count",
        PluginKind::Analysis,
        Contributions::default().analyses(vec![Arc::new(WordCount)]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        kind: String,
        text: String,
        words: u64,
        characters: u64,
        minutes: u64,
    }

    #[test]
    fn manifest_is_a_valid_app_plugin() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "word-count");
        assert_eq!(manifest.kind, PluginKind::Analysis);
        assert_eq!(plugin.contributions().analyses.len(), 1);
    }

    #[test]
    fn the_shared_fixtures_count_the_same_here_and_in_the_frontend() {
        let cases: Vec<Case> = serde_json::from_str(include_str!("../fixtures.json")).unwrap();
        assert!(cases.len() >= 15);
        for case in cases {
            let kind = if case.kind == "markdown" {
                TextKind::Markdown
            } else {
                TextKind::Plain
            };
            let counts = measure(&case.text, kind);
            assert_eq!(counts.words, case.words, "{}: words", case.name);
            assert_eq!(
                counts.characters, case.characters,
                "{}: characters",
                case.name
            );
            assert_eq!(counts.minutes(), case.minutes, "{}: minutes", case.name);
        }
    }

    #[test]
    fn a_megabyte_is_counted_quickly() {
        let text = "Some words, a [link](x.md) and `code`.\n".repeat(30_000);
        let start = std::time::Instant::now();
        let counts = measure(&text, TextKind::Markdown);
        assert_eq!(counts.words, 30_000 * 5);
        assert!(start.elapsed().as_millis() < 2_000);
    }
}
