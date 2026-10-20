//! Finding a comment's passage again after the document changed. Pure: it takes the text units of a document (leaf
//! blocks with their lines and the headings above them) and returns places.
//!
//! A comment keeps the text that was selected, the few words around it, the headings above it and its lines. To find the
//! passage in a document as it is now:
//!
//! 1. **The quote.** The units are folded to letters and digits, lowercased ([`fold`]); the quote is folded the same
//!    way, so a quote taken from the reading view matches the source behind it (`**bold** claim`, a link's text, smart
//!    quotes and line breaks all compare equal). Where the quote occurs more than once, the words around it decide, then
//!    the nearest line.
//! 2. **The heading.** If the quote is gone, the section it was in still points to the right neighbourhood.
//! 3. **The line.** If that is gone too, the line it was on (kept inside the document).
//!
//! Only the first step is a [`Resolution::Found`]; the others say the passage changed, and the comment stays.

use std::cmp::Reverse;

use permark_core::format::TextUnit;

use crate::model::Anchor;

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

impl Analysis {
    /// Reads the units of a document (`permark_core::format::units`).
    #[must_use]
    pub fn of(units: &[TextUnit], last_line: u32) -> Self {
        let mut sections = vec![Section {
            path: Vec::new(),
            line: 1,
        }];
        let mut out = Vec::with_capacity(units.len());
        let mut text = String::new();
        let mut ends = Vec::with_capacity(units.len());
        for unit in units {
            if unit.heading {
                sections.push(Section {
                    path: unit.heading_path.clone(),
                    line: unit.start_line,
                });
            }
            text.push_str(&fold(&unit.text));
            ends.push(text.len());
            out.push(Unit {
                start_line: unit.start_line,
                end_line: unit.end_line.max(unit.start_line),
                section: sections.len() - 1,
            });
        }
        Self {
            units: out,
            sections,
            text,
            ends,
            last_line: last_line.max(1),
        }
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

    /// Where `quote` is, if it is. With more than one place, the words around it decide, then the nearest line to the
    /// probe's, then the first.
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
