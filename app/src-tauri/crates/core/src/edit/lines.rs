//! Lines of a file with their own endings, and line edits applied to them. Pure.
//!
//! The editor works on the text with every line break as `\n`; the file keeps what it had. A line the person did not
//! change keeps its own ending (`\r\n`, `\n`, lone `\r`); a new line gets the ending most lines of the file use.

use serde::Deserialize;
use ts_rs::TS;

use crate::fsx::Encoding;

/// One change of the edit text: lines `at .. at + remove` of the base replaced by `insert` (0-based, base lines).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LineEdit {
    pub at: u32,
    pub remove: u32,
    pub insert: Vec<String>,
}

/// A line of the file: its text and the break after it (`""` for the last line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line<'a> {
    pub text: &'a str,
    pub ending: &'a str,
}

/// Splits at `\r\n`, `\n` and a lone `\r`. Always at least one line; the last one has no ending (empty when the
/// text ends with a break), so `n` breaks give `n + 1` lines.
#[must_use]
pub fn split(text: &str) -> Vec<Line<'_>> {
    let bytes = text.as_bytes();
    let mut lines = Vec::with_capacity(bytes.len() / 32 + 1);
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\n' => {
                lines.push(Line {
                    text: &text[start..at],
                    ending: &text[at..=at],
                });
                at += 1;
                start = at;
            }
            b'\r' => {
                let end = if bytes.get(at + 1) == Some(&b'\n') {
                    at + 2
                } else {
                    at + 1
                };
                lines.push(Line {
                    text: &text[start..at],
                    ending: &text[at..end],
                });
                at = end;
                start = at;
            }
            _ => at += 1,
        }
    }
    lines.push(Line {
        text: &text[start..],
        ending: "",
    });
    lines
}

/// The text with every line break as `\n` (what the editor works on).
#[must_use]
pub fn normalized(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_owned();
    }
    let lines = split(text);
    let mut out = String::with_capacity(text.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.text);
    }
    out
}

/// The ending most lines use (`\n` on a tie or when there is none).
#[must_use]
pub fn majority<'a>(lines: &[Line<'a>]) -> &'a str {
    let (mut lf, mut crlf, mut cr) = (0_usize, 0_usize, 0_usize);
    for line in lines {
        match line.ending {
            "\n" => lf += 1,
            "\r\n" => crlf += 1,
            "\r" => cr += 1,
            _ => {}
        }
    }
    if crlf > lf && crlf >= cr {
        "\r\n"
    } else if cr > lf && cr > crlf {
        "\r"
    } else {
        "\n"
    }
}

/// Why edits cannot apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditProblem {
    /// A range past the end of the base.
    OutOfRange,
    /// Not sorted, or overlapping.
    Order,
    /// An inserted line holds a line break.
    LineBreak,
}

/// Applies `edits` to `base`: kept lines keep their bytes and endings; inserted lines (and a kept last line that now
/// has lines after it) get the majority ending; the last line of the result has none.
///
/// # Errors
/// [`EditProblem`].
pub fn apply(base: &str, edits: &[LineEdit]) -> Result<String, EditProblem> {
    let lines = split(base);
    let count = lines.len();
    let mut cursor = 0_usize;
    for edit in edits {
        let at = edit.at as usize;
        let end = at
            .checked_add(edit.remove as usize)
            .ok_or(EditProblem::OutOfRange)?;
        if end > count {
            return Err(EditProblem::OutOfRange);
        }
        if at < cursor {
            return Err(EditProblem::Order);
        }
        if edit
            .insert
            .iter()
            .any(|l| l.contains('\n') || l.contains('\r'))
        {
            return Err(EditProblem::LineBreak);
        }
        cursor = end;
    }
    if edits.is_empty() {
        return Ok(base.to_owned());
    }
    let new_ending = majority(&lines);
    // (text, ending when it is followed by another line)
    let mut out: Vec<(&str, &str)> = Vec::with_capacity(count + 8);
    let mut next = 0_usize;
    for edit in edits {
        let at = edit.at as usize;
        push_kept(&mut out, &lines, next..at, new_ending);
        for line in &edit.insert {
            out.push((line.as_str(), new_ending));
        }
        next = at + edit.remove as usize;
    }
    push_kept(&mut out, &lines, next..count, new_ending);
    let mut text = String::with_capacity(base.len() + 64);
    let last = out.len().saturating_sub(1);
    for (i, (line, ending)) in out.iter().enumerate() {
        text.push_str(line);
        if i < last {
            text.push_str(ending);
        }
    }
    Ok(text)
}

/// Kept lines keep their ending; the base's last line gets `new_ending` when something follows it.
fn push_kept<'a>(
    out: &mut Vec<(&'a str, &'a str)>,
    lines: &[Line<'a>],
    range: std::ops::Range<usize>,
    new_ending: &'a str,
) {
    let count = lines.len();
    for i in range {
        let line = lines[i];
        let ending = if i + 1 == count {
            new_ending
        } else {
            line.ending
        };
        out.push((line.text, ending));
    }
}

/// The bytes of `text` in `encoding` (BOM written for the encodings that carry one).
#[must_use]
pub fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Utf8Bom => {
            let mut out = Vec::with_capacity(text.len() + 3);
            out.extend_from_slice(b"\xEF\xBB\xBF");
            out.extend_from_slice(text.as_bytes());
            out
        }
        Encoding::Utf16Le => {
            let mut out = vec![0xFF, 0xFE];
            out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            out
        }
        Encoding::Utf16Be => {
            let mut out = vec![0xFE, 0xFF];
            out.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(at: u32, remove: u32, insert: &[&str]) -> LineEdit {
        LineEdit {
            at,
            remove,
            insert: insert.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn split_knows_three_breaks_and_keeps_the_last_line() {
        let lines = split("a\r\nb\nc\rd");
        let texts: Vec<_> = lines.iter().map(|l| (l.text, l.ending)).collect();
        assert_eq!(texts, [("a", "\r\n"), ("b", "\n"), ("c", "\r"), ("d", "")]);
        assert_eq!(split("").len(), 1);
        assert_eq!(split("x\n").len(), 2);
        assert_eq!(normalized("a\r\nb\rc\n"), "a\nb\nc\n");
    }

    #[test]
    fn no_edits_give_the_base_back() {
        for base in ["", "a", "a\r\nb\r\n", "x\ry\nz"] {
            assert_eq!(apply(base, &[]).unwrap(), base);
        }
    }

    #[test]
    fn changed_lines_take_the_majority_and_kept_lines_their_own() {
        let base = "one\r\ntwo\nthree\r\nfour\r\n";
        let out = apply(base, &[edit(1, 1, &["TWO", "extra"])]).unwrap();
        assert_eq!(out, "one\r\nTWO\r\nextra\r\nthree\r\nfour\r\n");
    }

    #[test]
    fn trailing_newline_follows_the_edit() {
        // remove the final empty line: no newline at the end any more
        assert_eq!(apply("a\nb\n", &[edit(2, 1, &[])]).unwrap(), "a\nb");
        // add after a last line without break: it gains the majority ending
        assert_eq!(
            apply("a\r\nb", &[edit(2, 0, &["c"])]).unwrap(),
            "a\r\nb\r\nc"
        );
        // a file without any break gets LF
        assert_eq!(apply("a", &[edit(1, 0, &["b"])]).unwrap(), "a\nb");
    }

    #[test]
    fn bad_edits_are_refused() {
        assert_eq!(
            apply("a\nb", &[edit(1, 5, &[])]),
            Err(EditProblem::OutOfRange)
        );
        assert_eq!(
            apply("a\nb\nc", &[edit(1, 1, &[]), edit(0, 1, &[])]),
            Err(EditProblem::Order)
        );
        assert_eq!(
            apply("a\nb", &[edit(1, 1, &["x\ny"])]),
            Err(EditProblem::LineBreak)
        );
        assert_eq!(
            apply("a", &[edit(u32::MAX, u32::MAX, &[])]),
            Err(EditProblem::OutOfRange)
        );
    }

    #[test]
    fn encodings_round_trip() {
        let text = "ação 文 😀\r\n";
        for encoding in [
            Encoding::Utf8,
            Encoding::Utf8Bom,
            Encoding::Utf16Le,
            Encoding::Utf16Be,
        ] {
            let bytes = encode(text, encoding);
            let decoded = crate::fsx::decode(&bytes).unwrap();
            assert_eq!(decoded.text, text);
            assert_eq!(decoded.encoding, encoding);
        }
    }

    #[test]
    fn majority_prefers_lf_on_ties() {
        assert_eq!(majority(&split("a\r\nb\n")), "\n");
        assert_eq!(majority(&split("a\r\nb\r\nc\n")), "\r\n");
        assert_eq!(majority(&split("a\rb\rc\n")), "\r");
        assert_eq!(majority(&split("abc")), "\n");
    }
}
