//! What the info string of a fenced code block says beyond its language, in the spellings of the common tools: a
//! title (`title="x"`, `filename=x`), marked lines (`{1,3-5}`, `hl_lines="1 3-5"`) and line numbers (`showLineNumbers`,
//! `showLineNumbers{5}`, `linenums="5"`, `{.line-numbers}`, `{.numberLines startFrom=5}`). Only these leave as checked
//! `data-*` attributes of `<code>`; nothing else of the info string reaches the page.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{self, Write};

use comrak::adapters::SyntaxHighlighterAdapter;
use comrak::html::{escape, write_opening_tag};

/// Longest title kept (characters).
const MAX_TITLE: usize = 200;
/// Most line ranges kept.
const MAX_RANGES: usize = 100;
/// Highest line number read (a marked line or the first number).
const MAX_LINE: u32 = 1_000_000;

/// The extras of one info string.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Extras {
    title: Option<String>,
    /// Inclusive, 1-based, in the author's order.
    lines: Vec<(u32, u32)>,
    /// The number of the first line, when lines are numbered.
    numbers: Option<u32>,
}

impl Extras {
    /// Reads the part of an info string after the language.
    pub(super) fn parse(meta: &str) -> Self {
        let mut extras = Self::default();
        extras.scan(meta, false);
        extras
    }

    fn scan(&mut self, text: &str, braced: bool) {
        let mut rest = text;
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_whitespace() || (braced && c == ','));
            if rest.is_empty() {
                return;
            }
            if let Some(inner) = rest.strip_prefix('{') {
                let end = inner.find('}').unwrap_or(inner.len());
                self.scan(&inner[..end], true);
                rest = inner.get(end + 1..).unwrap_or_default();
                continue;
            }
            let end = rest
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '=' | '{' | '}') || (braced && c == ',')
                })
                .unwrap_or(rest.len())
                .max(1);
            let word = &rest[..end];
            rest = &rest[end..];
            let value = match rest.strip_prefix('=') {
                Some(after) => {
                    let (value, after) = value_of(after, braced);
                    rest = after;
                    Some(value)
                }
                None => None,
            };
            match (word.to_ascii_lowercase().as_str(), value) {
                ("title" | "filename", Some(value)) => self.title = title_of(value),
                ("hl_lines", Some(value)) => self.add_lines(value),
                ("linenums" | "startfrom", value) => {
                    self.numbers = Some(value.map_or(1, first_number));
                }
                ("showlinenumbers", None) => {
                    let start = match rest.strip_prefix('{') {
                        Some(inner) => {
                            let end = inner.find('}').unwrap_or(inner.len());
                            rest = inner.get(end + 1..).unwrap_or_default();
                            first_number(&inner[..end])
                        }
                        None => 1,
                    };
                    self.numbers = Some(start);
                }
                (".line-numbers" | ".numberlines" | ".number-lines", None) if braced => {
                    self.numbers.get_or_insert(1);
                }
                (word, None) if braced && word.starts_with(|c: char| c.is_ascii_digit()) => {
                    self.add_lines(word);
                }
                _ => {}
            }
        }
    }

    /// Ranges `n` | `a-b` separated by commas | spaces; a range out of order or out of bounds is dropped.
    fn add_lines(&mut self, list: &str) {
        for part in list.split(|c: char| c == ',' || c.is_whitespace()) {
            if self.lines.len() >= MAX_RANGES {
                return;
            }
            let (from, to) = part.split_once('-').unwrap_or((part, part));
            let (Ok(from), Ok(to)) = (from.trim().parse::<u32>(), to.trim().parse::<u32>()) else {
                continue;
            };
            if (1..=to).contains(&from) && to <= MAX_LINE {
                self.lines.push((from, to));
            }
        }
    }

    /// The attributes of `<code>` after its class: `data-title`, `data-lines` (`1,3-5`), `data-numbers` (first number).
    fn write(&self, output: &mut dyn Write) -> fmt::Result {
        if let Some(title) = &self.title {
            output.write_str(" data-title=\"")?;
            escape(output, title)?;
            output.write_str("\"")?;
        }
        if !self.lines.is_empty() {
            output.write_str(" data-lines=\"")?;
            for (at, (from, to)) in self.lines.iter().enumerate() {
                let comma = if at == 0 { "" } else { "," };
                if from == to {
                    write!(output, "{comma}{from}")?;
                } else {
                    write!(output, "{comma}{from}-{to}")?;
                }
            }
            output.write_str("\"")?;
        }
        if let Some(first) = self.numbers {
            write!(output, " data-numbers=\"{first}\"")?;
        }
        Ok(())
    }
}

/// A value: `"..."`, `'...'` or a bare word (to a space, or a comma inside braces); and what follows it.
fn value_of(text: &str, braced: bool) -> (&str, &str) {
    for quote in ['"', '\''] {
        if let Some(inner) = text.strip_prefix(quote) {
            let end = inner.find(quote).unwrap_or(inner.len());
            return (&inner[..end], inner.get(end + 1..).unwrap_or_default());
        }
    }
    let end = text
        .find(|c: char| c.is_whitespace() || c == '}' || (braced && c == ','))
        .unwrap_or(text.len());
    (&text[..end], &text[end..])
}

/// A number of the first line: 0 ..= [`MAX_LINE`], else 1.
fn first_number(text: &str) -> u32 {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|n| *n <= MAX_LINE)
        .unwrap_or(1)
}

fn title_of(value: &str) -> Option<String> {
    let title: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE)
        .collect();
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

/// Draws fenced code without colors (the UI colors it later), its info string's extras as `data-*` attributes. The
/// language word stops at a `{` (`js{1,3}`: the lines are extras).
pub(super) struct Fences;

impl SyntaxHighlighterAdapter for Fences {
    fn write_highlighted(
        &self,
        output: &mut dyn Write,
        _lang: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        escape(output, code)
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        write_opening_tag(output, "pre", attributes)
    }

    fn write_code_tag(
        &self,
        output: &mut dyn Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        let word = attributes
            .get("class")
            .and_then(|class| class.strip_prefix("language-"))
            .unwrap_or_default();
        let meta = attributes.get("data-meta").map_or("", |m| m.as_ref());
        let (language, meta): (&str, Cow<'_, str>) = match word.find('{') {
            Some(at) => (&word[..at], Cow::Owned(format!("{} {meta}", &word[at..]))),
            None => (word, Cow::Borrowed(meta)),
        };
        output.write_str("<code")?;
        if !language.is_empty() {
            output.write_str(" class=\"language-")?;
            escape(output, language)?;
            output.write_str("\"")?;
        }
        Extras::parse(&meta).write(output)?;
        output.write_str(">")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(meta: &str) -> Vec<(u32, u32)> {
        Extras::parse(meta).lines
    }

    #[test]
    fn each_spelling_is_read() {
        assert_eq!(
            Extras::parse("title=\"a b.rs\" {1, 3-5} showLineNumbers"),
            Extras {
                title: Some("a b.rs".into()),
                lines: vec![(1, 1), (3, 5)],
                numbers: Some(1),
            }
        );
        assert_eq!(lines("hl_lines=\"2 4-6\""), vec![(2, 2), (4, 6)]);
        assert_eq!(Extras::parse("linenums=\"12\"").numbers, Some(12));
        assert_eq!(Extras::parse("showLineNumbers{0}").numbers, Some(0));
        assert_eq!(
            Extras::parse("{.python .numberLines startFrom=\"3\"}").numbers,
            Some(3)
        );
        assert_eq!(
            Extras::parse("filename=x.ts").title.as_deref(),
            Some("x.ts")
        );
        assert_eq!(Extras::parse("title='it''s'").title.as_deref(), Some("it"));
    }

    #[test]
    fn odd_input_is_dropped_never_passed_on() {
        assert_eq!(lines("{0,5-2,4294967295,1000001,x-1,2-}"), Vec::new());
        assert_eq!(lines("1,3"), Vec::new(), "bare numbers outside braces");
        assert_eq!(Extras::parse("linenums=\"abc\"").numbers, Some(1));
        assert_eq!(Extras::parse("title=\"\u{7}\" key=v .x #y").title, None);
        let many = format!("{{{}}}", vec!["1"; MAX_RANGES + 5].join(","));
        assert_eq!(lines(&many).len(), MAX_RANGES);
        let long = format!("title=\"{}\"", "t".repeat(MAX_TITLE * 2));
        assert_eq!(
            Extras::parse(&long).title.map(|t| t.chars().count()),
            Some(MAX_TITLE)
        );
        // unclosed quotes and braces end at the end
        assert_eq!(Extras::parse("title=\"open").title.as_deref(), Some("open"));
        assert_eq!(lines("{2,3"), vec![(2, 2), (3, 3)]);
        assert_eq!(Extras::parse("= = {} }{ ==="), Extras::default());
    }
}
