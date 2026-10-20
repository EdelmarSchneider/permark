//! Helpers the syntaxes share: lines of a text, key paths, comments, string escapes.

use std::fmt::Write as _;

/// Where the lines of a text begin. Lines end at `\n`; a `\r` before it belongs to the line break.
#[derive(Debug, Clone)]
pub struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i + 1),
        );
        // a text that ends with a break has no extra empty line
        if starts.len() > 1 && starts.last() == Some(&text.len()) {
            starts.pop();
        }
        Self { text, starts }
    }

    #[must_use]
    pub fn text(&self) -> &'a str {
        self.text
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.starts.len()
    }

    /// 1-based line holding byte `at`.
    #[must_use]
    pub fn line_of(&self, at: usize) -> u32 {
        let n = match self.starts.binary_search(&at) {
            Ok(i) => i + 1,
            Err(i) => i,
        };
        u32::try_from(n.max(1)).unwrap_or(u32::MAX)
    }

    /// Bytes of line `n` (0-based) without its break, and where the next line starts.
    #[must_use]
    pub fn bounds(&self, n: usize) -> Option<(usize, usize, usize)> {
        let start = *self.starts.get(n)?;
        let next = self.starts.get(n + 1).copied().unwrap_or(self.text.len());
        let mut end = next;
        let bytes = self.text.as_bytes();
        if end > start && bytes[end - 1] == b'\n' {
            end -= 1;
        }
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        Some((start, end, next))
    }

    /// The text of line `n` (0-based) without its break.
    #[must_use]
    pub fn line(&self, n: usize) -> &'a str {
        self.bounds(n).map_or("", |(s, e, _)| &self.text[s..e])
    }

    /// Lines as `(start, text without break)`.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &'a str)> + '_ {
        (0..self.count()).filter_map(|n| self.bounds(n).map(|(s, e, _)| (s, &self.text[s..e])))
    }
}

/// `a.b` / `a."b.c"` / `a[0]` -> `["a", "b"]` | `["a", "b.c"]` | `["a"]`: the key segments of a path (list indexes left out).
#[must_use]
pub fn split_path(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = path.chars();
    let mut open = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                open = true;
                while let Some(d) = chars.next() {
                    match d {
                        '\\' => cur.extend(chars.next()),
                        '"' => break,
                        _ => cur.push(d),
                    }
                }
            }
            '.' => {
                if open || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                open = false;
            }
            '[' => {
                if open || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                open = false;
                // skip the index
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                }
            }
            _ => cur.push(c),
        }
    }
    if open || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A key as it is written in a path: quoted when it holds `. [ ] "` or space (or is empty).
#[must_use]
pub fn quote_key(key: &str) -> String {
    if !key.is_empty()
        && !key
            .chars()
            .any(|c| matches!(c, '.' | '[' | ']' | '"' | ' ' | '\\'))
    {
        return key.to_owned();
    }
    let mut out = String::with_capacity(key.len() + 2);
    out.push('"');
    for c in key.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// `parent` + `key` as a path.
#[must_use]
pub fn join_key(parent: &str, key: &str) -> String {
    if parent.is_empty() {
        quote_key(key)
    } else {
        format!("{parent}.{}", quote_key(key))
    }
}

/// `parent` + a list index as a path.
#[must_use]
pub fn join_index(parent: &str, index: usize) -> String {
    format!("{parent}[{index}]")
}

/// The caption of a line: the comment lines directly above it (no blank line between), marker removed.
/// `markers`: characters that start a comment. `before` = 0-based index of the line.
#[must_use]
pub fn comment_above(lines: &Lines<'_>, before: usize, markers: &[char]) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let mut n = before;
    while n > 0 {
        n -= 1;
        let line = lines.line(n).trim();
        match line.chars().next() {
            Some(c) if markers.contains(&c) => {
                parts.push(
                    line.trim_start_matches(|c: char| markers.contains(&c))
                        .trim(),
                );
            }
            _ => break,
        }
    }
    parts.reverse();
    let joined = parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!joined.is_empty()).then_some(joined)
}

/// The caption: comment above, then a trailing comment (`rest` = the line after the value).
#[must_use]
pub fn caption(above: Option<String>, trailing: Option<String>) -> Option<String> {
    match (above, trailing) {
        (Some(a), Some(t)) => Some(format!("{a} {t}")),
        (a, t) => a.or(t),
    }
}

/// `# text` after optional spaces: the text.
#[must_use]
pub fn trailing_comment(rest: &str, markers: &[char]) -> Option<String> {
    let rest = rest.trim();
    let c = rest.chars().next()?;
    markers.contains(&c).then(|| {
        rest.trim_start_matches(|c: char| markers.contains(&c))
            .trim()
            .to_owned()
    })
}

/// A double-quoted string body: backslash escapes of TOML basic strings and YAML double-quoted scalars.
#[must_use]
pub fn escape_double(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Inverse of [`escape_double`] for the escapes it writes plus the common ones; unknown escapes stay as written.
#[must_use]
pub fn unescape_double(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            // a lone backslash at the end stays
            Some('\\') | None => out.push('\\'),
            Some('/') => out.push('/'),
            Some('0') => out.push('\0'),
            Some('u') => {
                let hex: String = chars.clone().take(4).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(ch) if hex.len() == 4 => {
                        out.push(ch);
                        for _ in 0..4 {
                            chars.next();
                        }
                    }
                    _ => out.push_str("\\u"),
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// A decimal number: optional sign, digits, optional fraction, optional exponent.
#[must_use]
pub fn is_number(s: &str) -> bool {
    let t = s.strip_prefix(['-', '+']).unwrap_or(s);
    let (int, rest) = match t.find(|c: char| !c.is_ascii_digit()) {
        Some(i) => t.split_at(i),
        None => (t, ""),
    };
    if int.is_empty() {
        return false;
    }
    let rest = match rest.strip_prefix('.') {
        Some(frac) => {
            let digits = frac
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(frac.len());
            if digits == 0 {
                return false;
            }
            &frac[digits..]
        }
        None => rest,
    };
    match rest.strip_prefix(['e', 'E']) {
        Some(exp) => {
            let exp = exp.strip_prefix(['-', '+']).unwrap_or(exp);
            !exp.is_empty() && exp.chars().all(|c| c.is_ascii_digit())
        }
        None => rest.is_empty(),
    }
}

/// The line break of a text: `\r\n` when it has more of those than bare `\n`, else `\n`.
#[must_use]
pub fn majority_break(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    if crlf > lf {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_know_their_bytes() {
        let text = "a\r\nbb\n\nlast";
        let lines = Lines::new(text);
        assert_eq!(lines.count(), 4);
        assert_eq!(lines.line(0), "a");
        assert_eq!(lines.line(1), "bb");
        assert_eq!(lines.line(2), "");
        assert_eq!(lines.line(3), "last");
        assert_eq!(lines.line_of(0), 1);
        assert_eq!(lines.line_of(4), 2);
        assert_eq!(lines.line_of(text.len()), 4);
        assert_eq!(Lines::new("a\n").count(), 1);
        assert_eq!(Lines::new("").count(), 1);
    }

    #[test]
    fn paths_split_and_quote() {
        assert_eq!(split_path("a.b.c"), ["a", "b", "c"]);
        assert_eq!(split_path("a.\"b.c\".d"), ["a", "b.c", "d"]);
        assert_eq!(split_path("steps[2].run"), ["steps", "run"]);
        assert_eq!(split_path(""), Vec::<String>::new());
        assert_eq!(join_key("", "x y"), "\"x y\"");
        assert_eq!(join_key("a", "b.c"), "a.\"b.c\"");
        assert_eq!(join_index("jobs.steps", 3), "jobs.steps[3]");
        assert_eq!(split_path(&join_key("a", "b.c")), ["a", "b.c"]);
    }

    #[test]
    fn captions_stop_at_a_blank_line() {
        let lines = Lines::new("# far\n\n# near\n; also\nkey = 1\n");
        assert_eq!(
            comment_above(&lines, 4, &['#', ';']),
            Some("near also".to_owned())
        );
        assert_eq!(comment_above(&lines, 2, &['#']), None);
        assert_eq!(
            trailing_comment("  # note ", &['#']),
            Some("note".to_owned())
        );
        assert_eq!(trailing_comment("x", &['#']), None);
    }

    #[test]
    fn escapes_round_trip() {
        for text in [
            "plain",
            "a \"q\" b",
            "back\\slash",
            "tab\tnew\nline",
            "\u{1}ctl",
            "ünï",
        ] {
            assert_eq!(unescape_double(&escape_double(text)), text, "{text}");
        }
        assert_eq!(unescape_double("\\u00e9\\q"), "é\\q");
    }

    #[test]
    fn numbers() {
        for yes in ["0", "42", "-7", "+3", "3.5", "1e3", "2.5E-4"] {
            assert!(is_number(yes), "{yes}");
        }
        for no in ["", "-", "1.", ".5", "1e", "0x1", "12a", "1 2", "--1"] {
            assert!(!is_number(no), "{no}");
        }
    }

    #[test]
    fn break_majority() {
        assert_eq!(majority_break("a\r\nb\r\nc\n"), "\r\n");
        assert_eq!(majority_break("a\nb\r\n"), "\n");
        assert_eq!(majority_break("abc"), "\n");
    }
}
