//! JSON Lines record boundaries. One physical line is one record. This does not parse JSON.

/// One non-blank line of a JSON Lines file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonLineRecord<'a> {
    /// 1-based index over non-blank lines.
    pub index: u32,
    /// 1-based physical line (`\n` breaks; the empty piece after a final `\n` is not a line).
    pub line: u32,
    /// The line with one trailing `\r` removed. Not parsed.
    pub text: &'a str,
}

/// Non-blank lines of a JSON Lines text, in order.
///
/// A break is `\n`. One trailing `\r` is not part of the line. Blank = empty after that and a trim of spaces and
/// tabs; a blank line has no index. The index is 1-based. A last line with no break counts when it is not blank.
/// Lines are not joined. A leading U+FEFF stays (decode already stripped one).
#[must_use]
pub fn records(text: &str) -> Vec<JsonLineRecord<'_>> {
    let mut out = Vec::new();
    let mut index = 0_u32;
    let mut number = 0_u32;
    let mut rest = text;
    // A final empty piece after `\n` leaves `rest` empty, so it is not a line.
    while !rest.is_empty() {
        let Some(line) = number.checked_add(1) else {
            break;
        };
        number = line;
        let (raw, next) = match rest.split_once('\n') {
            Some((raw, next)) => (raw, Some(next)),
            None => (rest, None),
        };
        let value = raw.strip_suffix('\r').unwrap_or(raw);
        if !value.trim_matches([' ', '\t']).is_empty() {
            let Some(next_index) = index.checked_add(1) else {
                break;
            };
            index = next_index;
            out.push(JsonLineRecord {
                index,
                line,
                text: value,
            });
        }
        match next {
            Some(next) => rest = next,
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(index: u32, line: u32, text: &str) -> JsonLineRecord<'_> {
        JsonLineRecord { index, line, text }
    }

    #[test]
    fn blanks_are_skipped_crlf_is_one_break_and_the_index_starts_at_the_first_value() {
        // physical 1 and 2 are blank; index 1 is the first non-blank line; the last line has no break
        let text = "\n\t \r\n{\"a\":1}\r\n\n  {\"b\": 2}";
        assert_eq!(
            records(text),
            [rec(1, 3, "{\"a\":1}"), rec(2, 5, "  {\"b\": 2}")]
        );
        assert_eq!(records("{\"a\":1}"), [rec(1, 1, "{\"a\":1}")]);
    }

    #[test]
    fn empty_and_blank_files_have_no_records() {
        assert_eq!(records(""), []);
        assert_eq!(records("\n"), []);
        assert_eq!(records(" \t\r\n\r\n"), []);
        assert_eq!(records("\r"), []);
        // a space that is not U+0020 or a tab is still a record
        assert_eq!(records("\u{00a0}"), [rec(1, 1, "\u{00a0}")]);
    }

    #[test]
    fn lines_are_not_joined_and_json_is_not_parsed() {
        // a value split over two lines stays two records; two values on one line stay one; digits are kept as written
        let text = "{\"a\":\n1}{\"b\":2}\n123456789012345678901234567890\n";
        assert_eq!(
            records(text),
            [
                rec(1, 1, "{\"a\":"),
                rec(2, 2, "1}{\"b\":2}"),
                rec(3, 3, "123456789012345678901234567890"),
            ]
        );
    }

    #[test]
    fn a_leading_bom_stays_and_only_one_trailing_cr_is_removed() {
        assert_eq!(records("\u{feff}{\"a\":1}\n")[0].text, "\u{feff}{\"a\":1}");
        assert_eq!(records("{\"a\":1}\r\r\n")[0].text, "{\"a\":1}\r");
    }
}
