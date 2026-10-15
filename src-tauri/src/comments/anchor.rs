//! Finding a comment's passage again after the document changed. Pure: it takes Markdown text and returns places.
//!
//! A comment keeps the text that was selected, the few words around it, the headings above it and its lines. To find the
//! passage in a document as it is now:
//!
//! 1. **The quote.** The document is turned into the text a reader sees (formatting, link addresses and list markers
//!    are gone) and then into letters and digits only, lowercased ([`fold`]). Folding makes the match the same for a
//!    quote taken from the reading view and for the source behind it: `**bold** claim`, a link's text, smart quotes
//!    and line breaks all compare equal. Where the quote occurs more than once, the words around it decide, then
//!    the nearest line.
//! 2. **The heading.** If the quote is gone, the section it was in still points to the right neighbourhood.
//! 3. **The line.** If that is gone too, the line it was on (kept inside the document).
//!
//! Only the first step is a [`Resolution::Found`]; the others say the passage changed, and the comment stays.

use std::cmp::Reverse;

use comrak::nodes::NodeValue;
use comrak::{parse_document, Arena};

use crate::render::markdown::{
    is_deeper_than, options, plain_text, too_many_container_markers, MAX_TREE_DEPTH,
};

use super::Anchor;

/// How many letters and digits of the text around a quote are kept to tell two places apart.
pub const CONTEXT_CHARS: usize = 32;
/// A quote that occurs in more places than this is looked at in the first of them only (the word "the").
const MAX_CANDIDATES: usize = 500;

/// The letters and digits of `text`, lowercased: what two spellings of the same words have in common.
#[must_use]
pub fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A run of text in the document that comes from one block: a paragraph, a heading, a table cell, a code block.
#[derive(Debug, Clone, Copy)]
struct Unit {
    start_line: u32,
    end_line: u32,
    /// The section it belongs to (an index into `sections`).
    section: usize,
}

/// The part of a document under one heading (the first section is the text above the first heading).
#[derive(Debug, Clone)]
struct Section {
    path: Vec<String>,
    line: u32,
}

/// A document ready to be searched for passages.
#[derive(Debug)]
pub struct Analysis {
    units: Vec<Unit>,
    sections: Vec<Section>,
    /// The folded text of every unit, one after the other.
    text: String,
    /// Where each unit's text ends in `text` (so unit `n` is `ends[n - 1]..ends[n]`).
    ends: Vec<usize>,
    last_line: u32,
}

/// Where a passage is in a document now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub start_line: u32,
    pub end_line: u32,
    pub heading_path: Vec<String>,
    /// The folded text just before and after the quote (see [`Anchor`]).
    pub before: String,
    pub after: String,
}

/// What a comment's anchor comes to in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The quote was found.
    Found(Placement),
    /// It was not: the line to take the person to (the section's heading, else where it was).
    Changed { line: u32 },
}

/// How well a place fits: the words around it that agree, then the nearer line, then the earlier place. Bigger is better.
type Rank = (usize, Reverse<u32>, Reverse<usize>);

/// What is known about where a quote should be, to choose between places with the same words.
#[derive(Debug, Clone, Copy, Default)]
pub struct Probe<'a> {
    /// The line it was on (0: unknown).
    pub line: u32,
    pub before: &'a str,
    pub after: &'a str,
}

impl<'a> From<&'a Anchor> for Probe<'a> {
    fn from(anchor: &'a Anchor) -> Self {
        Self {
            line: anchor.line,
            before: &anchor.before,
            after: &anchor.after,
        }
    }
}

fn to_line(line: usize) -> u32 {
    u32::try_from(line).unwrap_or(u32::MAX)
}

/// Collects units while a document is walked.
struct Builder {
    units: Vec<Unit>,
    sections: Vec<Section>,
    text: String,
    ends: Vec<usize>,
}

impl Builder {
    fn new() -> Self {
        Self {
            units: Vec::new(),
            sections: vec![Section {
                path: Vec::new(),
                line: 1,
            }],
            text: String::new(),
            ends: Vec::new(),
        }
    }

    fn push(&mut self, start_line: u32, end_line: u32, content: &str) {
        self.text.push_str(&fold(content));
        self.ends.push(self.text.len());
        self.units.push(Unit {
            start_line,
            end_line: end_line.max(start_line),
            section: self.sections.len() - 1,
        });
    }

    fn finish(self, last_line: u32) -> Analysis {
        Analysis {
            units: self.units,
            sections: self.sections,
            text: self.text,
            ends: self.ends,
            last_line,
        }
    }
}

/// The text of a raw HTML block without its tags.
fn without_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

impl Analysis {
    /// Reads a document (already decoded, `\n` line endings).
    ///
    /// The parser may need a deep stack for absurdly nested input; callers on a worker thread with a normal stack
    /// run this through `util::on_parser_stack`.
    #[must_use]
    pub fn of(markdown: &str) -> Self {
        let last_line = to_line(markdown.lines().count().max(1));
        if too_many_container_markers(markdown) {
            return Self::by_lines(markdown, last_line);
        }
        let arena = Arena::new();
        let root = parse_document(&arena, markdown, &options());
        if is_deeper_than(root, MAX_TREE_DEPTH) {
            return Self::by_lines(markdown, last_line);
        }
        let mut builder = Builder::new();
        // (level, text) of the headings that are open
        let mut open: Vec<(u8, String)> = Vec::new();
        for node in root.descendants() {
            let data = node.data.borrow();
            let (start, end) = (
                to_line(data.sourcepos.start.line),
                to_line(data.sourcepos.end.line),
            );
            match &data.value {
                NodeValue::Heading(heading) => {
                    let title = plain_text(node);
                    while open
                        .last()
                        .is_some_and(|(level, _)| *level >= heading.level)
                    {
                        open.pop();
                    }
                    open.push((heading.level, title.clone()));
                    builder.sections.push(Section {
                        path: open.iter().map(|(_, text)| text.clone()).collect(),
                        line: start,
                    });
                    builder.push(start, end, &title);
                }
                NodeValue::Paragraph | NodeValue::TableCell => {
                    builder.push(start, end, &plain_text(node));
                }
                NodeValue::CodeBlock(code) => builder.push(start, end, &code.literal),
                NodeValue::HtmlBlock(html) => {
                    builder.push(start, end, &without_tags(&html.literal));
                }
                _ => {}
            }
        }
        builder.finish(last_line)
    }

    /// The fallback for text too deeply nested to parse: every line is a unit (list markers and link addresses are
    /// read as text, which only matters for a quote that spans them).
    fn by_lines(markdown: &str, last_line: u32) -> Self {
        let mut builder = Builder::new();
        for (index, line) in markdown.lines().enumerate() {
            let number = to_line(index + 1);
            builder.push(number, number, line);
        }
        builder.finish(last_line)
    }

    /// The last line of the document (at least 1).
    #[cfg(test)]
    #[must_use]
    pub fn last_line(&self) -> u32 {
        self.last_line
    }

    /// The unit that holds the byte `offset` of the folded text.
    fn unit_at(&self, offset: usize) -> usize {
        self.ends
            .partition_point(|end| *end <= offset)
            .min(self.units.len().saturating_sub(1))
    }

    fn placement(&self, at: usize, len: usize) -> Placement {
        let first = &self.units[self.unit_at(at)];
        let last = &self.units[self.unit_at(at + len - 1)];
        let end = at + len;
        let before: Vec<char> = self.text[..at].chars().rev().take(CONTEXT_CHARS).collect();
        Placement {
            start_line: first.start_line,
            end_line: last.end_line.max(first.start_line),
            heading_path: self.sections[first.section].path.clone(),
            before: before.into_iter().rev().collect(),
            after: self.text[end..].chars().take(CONTEXT_CHARS).collect(),
        }
    }

    /// How alike the words around the match at `at` are to what was around the quote: the number of letters and
    /// digits that agree, before and after.
    fn context_agreement(&self, at: usize, len: usize, probe: &Probe<'_>) -> usize {
        let before = self.text[..at]
            .chars()
            .rev()
            .zip(probe.before.chars().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let after = self.text[at + len..]
            .chars()
            .zip(probe.after.chars())
            .take_while(|(a, b)| a == b)
            .count();
        before + after
    }

    /// Where `quote` is, if it is. With more than one place, the words around it decide, then the nearest line to
    /// the probe's, then the first.
    #[must_use]
    pub fn locate(&self, quote: &str, probe: &Probe<'_>) -> Option<Placement> {
        let needle = fold(quote);
        if needle.is_empty() || self.units.is_empty() {
            return None;
        }
        let mut best: Option<(Rank, usize)> = None;
        let mut from = 0;
        for _ in 0..MAX_CANDIDATES {
            let Some(found) = self.text[from..].find(&needle) else {
                break;
            };
            let at = from + found;
            let line = self.units[self.unit_at(at)].start_line;
            let distance = if probe.line == 0 {
                0
            } else {
                line.abs_diff(probe.line)
            };
            let score = (
                self.context_agreement(at, needle.len(), probe),
                Reverse(distance),
                Reverse(at),
            );
            if best.as_ref().is_none_or(|(top, _)| score > *top) {
                best = Some((score, at));
            }
            // matches may overlap ("aa" in "aaa"): go on from the next character
            from = at + self.text[at..].chars().next().map_or(1, char::len_utf8);
        }
        best.map(|(_, at)| self.placement(at, needle.len()))
    }

    /// The line of the section called `path` (else the section with the same last heading), if there is one.
    fn section_line(&self, path: &[String]) -> Option<u32> {
        if path.is_empty() {
            return None;
        }
        let same = |a: &str, b: &str| fold(a) == fold(b);
        let exact = self.sections.iter().skip(1).find(|section| {
            section.path.len() == path.len()
                && section.path.iter().zip(path).all(|(a, b)| same(a, b))
        });
        let last = || {
            let wanted = path.last()?;
            self.sections
                .iter()
                .skip(1)
                .find(|section| section.path.last().is_some_and(|own| same(own, wanted)))
        };
        exact.or_else(last).map(|section| section.line)
    }

    /// The anchor against the document: found by its quote, or the best place to take the person.
    #[must_use]
    pub fn resolve(&self, anchor: &Anchor) -> Resolution {
        if let Some(placement) = self.locate(&anchor.quote, &Probe::from(anchor)) {
            return Resolution::Found(placement);
        }
        let line = self
            .section_line(&anchor.heading_path)
            .unwrap_or(anchor.line)
            .clamp(1, self.last_line);
        Resolution::Changed { line }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(quote: &str, line: u32) -> Anchor {
        Anchor {
            quote: quote.to_owned(),
            line,
            end_line: line,
            ..Anchor::default()
        }
    }

    fn found(doc: &str, quote: &str) -> Placement {
        match Analysis::of(doc).resolve(&anchor(quote, 0)) {
            Resolution::Found(placement) => placement,
            other @ Resolution::Changed { .. } => panic!("{quote:?} was not found: {other:?}"),
        }
    }

    const PLAN: &str = "\
Preface text.

# Plan

Intro paragraph.

## Rollout

Ship it on **Friday**, after the [review](https://example.com/a-long/address?x=1).
Then watch the dashboards.

- first step
- second step with `code`

### Week 1

| Day | Task |
| --- | ---- |
| Mon | Start small |

## Risks

> Quoted warning.

```rust
let answer = 42;
```
";

    #[test]
    fn folding_keeps_letters_and_digits_in_lower_case() {
        assert_eq!(fold("Ship it, on **Friday**!"), "shipitonfriday");
        assert_eq!(fold("Árvíztűrő  tükörfúrógép"), "árvíztűrőtükörfúrógép");
        assert_eq!(fold("日本語 テキスト"), "日本語テキスト");
        assert_eq!(fold("\u{201c}smart\u{201d} -- quotes..."), "smartquotes");
        assert_eq!(fold("---  ***"), "");
    }

    #[test]
    fn a_quote_is_found_through_formatting_and_link_addresses() {
        let placement = found(PLAN, "Ship it on Friday, after the review");
        assert_eq!((placement.start_line, placement.end_line), (9, 10));
        assert_eq!(placement.heading_path, ["Plan", "Rollout"]);
        // a quote that starts and ends mid-word, across the line break of the paragraph
        let placement = found(PLAN, "ay, after the review.\nThen watch");
        assert_eq!(placement.start_line, 9);
    }

    #[test]
    fn a_quote_across_blocks_runs_from_the_first_to_the_last() {
        let placement = found(PLAN, "Intro paragraph.\n\nRollout\n\nShip it");
        assert_eq!((placement.start_line, placement.end_line), (5, 10));
        assert_eq!(placement.heading_path, ["Plan"]);
    }

    #[test]
    fn lists_tables_quotes_and_code_are_found_with_their_own_lines() {
        let list = found(PLAN, "second step with code");
        assert_eq!((list.start_line, list.end_line), (13, 13));
        let table = found(PLAN, "Mon Start small");
        assert_eq!(table.start_line, 19);
        assert_eq!(table.heading_path, ["Plan", "Rollout", "Week 1"]);
        let quote = found(PLAN, "Quoted warning.");
        assert_eq!(quote.start_line, 23);
        assert_eq!(quote.heading_path, ["Plan", "Risks"]);
        let code = found(PLAN, "let answer = 42;");
        assert_eq!((code.start_line, code.end_line), (25, 27));
    }

    #[test]
    fn text_above_the_first_heading_has_no_heading_path() {
        let placement = found(PLAN, "Preface text");
        assert_eq!(placement.start_line, 1);
        assert!(placement.heading_path.is_empty(), "no heading above it");
    }

    #[test]
    fn a_heading_can_be_quoted_and_is_in_its_own_section() {
        let placement = found(PLAN, "Rollout");
        // "rollout" is also a word of nothing else above the heading: the heading is the first place
        assert_eq!(placement.start_line, 7);
        assert_eq!(placement.heading_path, ["Plan", "Rollout"]);
    }

    #[test]
    fn nothing_that_is_not_in_the_document_is_found() {
        let doc = Analysis::of(PLAN);
        assert_eq!(doc.locate("", &Probe::default()), None);
        assert_eq!(doc.locate("   ", &Probe::default()), None);
        assert_eq!(doc.locate("---", &Probe::default()), None);
        assert_eq!(doc.locate("Ship it on Thursday", &Probe::default()), None);
        assert_eq!(Analysis::of("").locate("anything", &Probe::default()), None);
    }

    const TWICE: &str = "\
# A

See the plan for details.

# B

Ship it now.

See the plan for details.

# C

Ship it now.
";

    #[test]
    fn a_quote_in_two_places_is_told_apart_by_its_surroundings() {
        let doc = Analysis::of(TWICE);
        // each remembers what stood around it
        let first = doc.locate("See the plan for details", &Probe::default());
        let first = first.expect("found");
        assert_eq!(first.start_line, 3, "without hints, the first");
        let second = doc
            .locate(
                "See the plan for details",
                &Probe {
                    line: 0,
                    before: &fold("Ship it now."),
                    after: "",
                },
            )
            .expect("found");
        assert_eq!(second.start_line, 9);
        assert_eq!(second.heading_path, ["B"]);
    }

    #[test]
    fn a_quote_in_two_places_goes_to_the_nearest_line_when_nothing_else_tells() {
        let doc = Analysis::of(TWICE);
        for (line, expected) in [(1, 7), (7, 7), (12, 13), (11, 13)] {
            let placement = doc
                .locate(
                    "Ship it now.",
                    &Probe {
                        line,
                        ..Probe::default()
                    },
                )
                .expect("found");
            assert_eq!(placement.start_line, expected, "near line {line}");
        }
    }

    #[test]
    fn overlapping_matches_are_all_considered() {
        let doc = Analysis::of("aaa bbb\n");
        assert!(doc.locate("aa", &Probe::default()).is_some());
        // "aabb" sits at the second a, which a search that skips past the first match would miss
        assert!(doc.locate("aabb", &Probe::default()).is_some());
    }

    #[test]
    fn a_passage_that_moved_is_found_at_its_new_lines() {
        let before = "# T\n\nOne.\n\nThe passage.\n";
        let after = "# T\n\nNew text.\n\nMore text.\n\nOne.\n\nThe passage.\n";
        let old = found(before, "The passage.");
        assert_eq!(old.start_line, 5);
        let comment = Anchor {
            quote: "The passage.".to_owned(),
            before: old.before,
            after: old.after,
            heading_path: old.heading_path,
            line: 5,
            end_line: 5,
        };
        match Analysis::of(after).resolve(&comment) {
            Resolution::Found(now) => assert_eq!((now.start_line, now.end_line), (9, 9)),
            other @ Resolution::Changed { .. } => panic!("lost: {other:?}"),
        }
    }

    #[test]
    fn a_passage_that_was_rewritten_falls_back_to_its_heading() {
        let comment = Anchor {
            quote: "Ship it on Thursday".to_owned(),
            heading_path: vec!["Plan".to_owned(), "Rollout".to_owned()],
            line: 3,
            end_line: 3,
            ..Anchor::default()
        };
        assert_eq!(
            Analysis::of(PLAN).resolve(&comment),
            Resolution::Changed { line: 7 }
        );
        // the heading matches without regard to case and punctuation
        let loose = Anchor {
            heading_path: vec!["plan".to_owned(), "ROLLOUT!".to_owned()],
            ..comment.clone()
        };
        assert_eq!(
            Analysis::of(PLAN).resolve(&loose),
            Resolution::Changed { line: 7 }
        );
        // a section that moved under another heading is still found by its own title
        let moved = Anchor {
            heading_path: vec!["Old parent".to_owned(), "Rollout".to_owned()],
            ..comment
        };
        assert_eq!(
            Analysis::of(PLAN).resolve(&moved),
            Resolution::Changed { line: 7 }
        );
    }

    #[test]
    fn a_passage_and_heading_that_are_gone_fall_back_to_the_line_inside_the_document() {
        let comment = Anchor {
            quote: "something that is gone".to_owned(),
            heading_path: vec!["Deleted section".to_owned()],
            line: 40,
            end_line: 40,
            ..Anchor::default()
        };
        let doc = Analysis::of("# Short\n\nOnly this.\n");
        assert_eq!(doc.resolve(&comment), Resolution::Changed { line: 3 });
        assert_eq!(
            Analysis::of("").resolve(&Anchor {
                line: 0,
                ..comment.clone()
            }),
            Resolution::Changed { line: 1 }
        );
        let early = Anchor { line: 2, ..comment };
        assert_eq!(doc.resolve(&early), Resolution::Changed { line: 2 });
    }

    #[test]
    fn a_comment_above_the_first_heading_keeps_its_line() {
        let comment = Anchor {
            quote: "gone".to_owned(),
            line: 1,
            end_line: 1,
            ..Anchor::default()
        };
        assert_eq!(
            Analysis::of(PLAN).resolve(&comment),
            Resolution::Changed { line: 1 }
        );
    }

    #[test]
    fn html_entities_and_escapes_read_as_the_reader_sees_them() {
        let doc = "Fish &amp; chips, caf&eacute; \\*not bold\\* and \u{201c}smart\u{201d}.\n";
        let placement = found(doc, "Fish & chips, café *not bold* and \"smart\".");
        assert_eq!(placement.start_line, 1);
    }

    #[test]
    fn raw_html_blocks_count_without_their_tags() {
        let doc = "# T\n\n<details>\n<summary>More</summary>\nHidden words here.\n</details>\n";
        let placement = found(doc, "More Hidden words here.");
        assert_eq!(placement.start_line, 3);
    }

    #[test]
    fn crlf_documents_have_the_same_lines() {
        let doc = "# T\r\n\r\nOne.\r\n\r\nTwo.\r\n";
        let placement = found(&doc.replace("\r\n", "\n"), "Two.");
        assert_eq!(placement.start_line, 5);
        let crlf = found(doc, "Two.");
        assert_eq!(crlf.start_line, 5);
    }

    #[test]
    fn absurdly_nested_text_is_read_line_by_line() {
        let mut doc = String::from("# Top\n\n");
        doc.push_str(&"> ".repeat(150));
        doc.push_str("deep quote\n\nafter\n");
        let analysis = Analysis::of(&doc);
        let placement = analysis
            .locate("deep quote", &Probe::default())
            .expect("found in the fallback");
        assert_eq!(placement.start_line, 3);
        assert_eq!(analysis.last_line(), 5);
    }

    #[test]
    fn the_context_kept_is_a_few_letters_either_side() {
        let placement = found(PLAN, "dashboards");
        assert!(placement.after.starts_with("firststep"));
        assert!(placement.before.ends_with("thenwatchthe"));
        assert!(placement.before.chars().count() <= CONTEXT_CHARS);
        let middle = found(PLAN, "after the review");
        assert!(middle.after.starts_with("thenwatch"));
    }

    #[test]
    fn a_word_that_is_everywhere_is_looked_at_in_a_bounded_number_of_places() {
        // 200,000 occurrences of one word: the search stops after MAX_CANDIDATES places and still answers
        let doc = "word ".repeat(200_000);
        let placement = Analysis::of(&doc).locate("word", &Probe::default());
        assert!(placement.is_some());
    }
}
