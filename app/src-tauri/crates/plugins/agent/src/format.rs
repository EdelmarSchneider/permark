//! The pieces of every text PerMark writes for a model: a header line `<tool> k=v`, tables (`#cols` once, rows split
//! by tabs, folder groups `@dir/`, sub-tables `-- name`), verbatim zones fenced so their text can never close them,
//! `ERR` lines, sizes and times, and the ceiling no answer passes. Pure.

use std::borrow::Cow;
use std::fmt::Write as _;

/// No answer is longer (characters): under the size clients save to a file instead of reading.
pub const CEILING_CHARS: usize = 48_000;

/// A header value as it is, or as a JSON string when it holds whitespace, `"`, `=` or nothing.
#[must_use]
pub fn value(text: &str) -> Cow<'_, str> {
    if text.is_empty()
        || text
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '=' || c.is_control())
    {
        Cow::Owned(serde_json::Value::String(text.to_owned()).to_string())
    } else {
        Cow::Borrowed(text)
    }
}

/// The first line of an answer: the tool and the facts of the call.
#[derive(Debug, Clone)]
pub struct Header(String);

impl Header {
    #[must_use]
    pub fn new(tool: &str) -> Self {
        Self(tool.to_owned())
    }

    /// `k=v` (the value quoted when it needs it).
    #[must_use]
    pub fn kv(mut self, key: &str, v: impl std::fmt::Display) -> Self {
        let v = v.to_string();
        let _ = write!(self.0, " {key}={}", value(&v));
        self
    }

    /// `k=v` when there is a value.
    #[must_use]
    pub fn opt(self, key: &str, v: Option<impl std::fmt::Display>) -> Self {
        match v {
            Some(v) => self.kv(key, v),
            None => self,
        }
    }

    /// A flag: the word when it holds, nothing otherwise.
    #[must_use]
    pub fn flag(mut self, word: &str, on: bool) -> Self {
        if on {
            self.0.push(' ');
            self.0.push_str(word);
        }
        self
    }

    #[must_use]
    pub fn line(self) -> String {
        self.0
    }
}

/// A table cell: tabs, line breaks and backslashes escaped (`\t`, `\n`, `\r`, `\\`).
#[must_use]
pub fn cell(text: &str) -> Cow<'_, str> {
    if !text.contains(['\t', '\n', '\r', '\\']) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

fn parent(path: &str) -> &str {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    trimmed.rfind('/').map_or("", |at| &trimmed[..at])
}

/// Rows of a table under its `#cols` line. Cells are escaped here; empty trailing cells dropped. `path_col`: the
/// column that holds a path: runs of >= 3 rows in the same folder are grouped under `@<folder>/`.
pub fn table(out: &mut String, cols: &[&str], rows: &[Vec<String>], path_col: Option<usize>) {
    let _ = writeln!(out, "#cols {}", cols.join(" "));
    rows_only(out, rows, path_col);
}

/// The rows of a table whose `#cols` line is already written (a sub-table with the same columns).
pub fn rows_only(out: &mut String, rows: &[Vec<String>], path_col: Option<usize>) {
    let folders: Vec<&str> = rows
        .iter()
        .map(|r| path_col.and_then(|c| r.get(c)).map_or("", |p| parent(p)))
        .collect();
    let mut group: Option<&str> = None;
    let mut i = 0;
    while i < rows.len() {
        let folder = folders[i];
        let run = folders[i..].iter().take_while(|f| **f == folder).count();
        let grouped = path_col.is_some() && !folder.is_empty() && run >= 3;
        if grouped {
            let _ = writeln!(out, "@{}/", cell(folder));
            group = Some(folder);
        } else if group.is_some() {
            out.push_str("@/\n");
            group = None;
        }
        let take = if grouped { run } else { 1 };
        for row in &rows[i..i + take] {
            let mut cells: Vec<Cow<'_, str>> = row
                .iter()
                .enumerate()
                .map(|(c, text)| match (grouped, path_col) {
                    (true, Some(p)) if p == c => cell(
                        text.strip_prefix(folder)
                            .map_or(text.as_str(), |r| r.strip_prefix('/').unwrap_or(r)),
                    ),
                    _ => cell(text),
                })
                .collect();
            while cells.last().is_some_and(|c| c.is_empty()) {
                cells.pop();
            }
            out.push_str(&cells.join("\t"));
            out.push('\n');
        }
        i += take;
    }
}

/// A sub-table line `-- name k=v` (the header pieces after the name).
pub fn sub(out: &mut String, header: Header) {
    let _ = writeln!(out, "-- {}", header.line());
}

/// Fence length for a zone: 3, or one more than the longest run of `<` or `>` that starts a line of the text
/// (spaces before it ignored), so no line of the text can be read as the fence.
#[must_use]
pub fn fence_len(text: &str) -> usize {
    let longest = text
        .lines()
        .map(|line| {
            let line = line.trim_start();
            let lt = line.chars().take_while(|c| *c == '<').count();
            let gt = line.chars().take_while(|c| *c == '>').count();
            lt.max(gt)
        })
        .max()
        .unwrap_or(0);
    (longest + 1).max(3)
}

/// A verbatim zone: the text exactly as it is between `<<<` and `>>>` lines (longer fences when the text needs them).
pub fn zone(out: &mut String, text: &str) {
    let n = fence_len(text);
    out.push_str(&"<".repeat(n));
    out.push('\n');
    out.push_str(text);
    if !text.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&">".repeat(n));
    out.push('\n');
}

/// Approximate tokens: `~340`, `~1.2k`, `~12k`.
#[must_use]
pub fn tokens(n: u64) -> String {
    match n {
        0..=999 => format!("~{n}"),
        1_000..=9_999 => {
            let tenths = (n + 50) / 100;
            if tenths.is_multiple_of(10) {
                format!("~{}k", tenths / 10)
            } else {
                format!("~{}.{}k", tenths / 10, tenths % 10)
            }
        }
        _ => format!("~{}k", (n + 500) / 1000),
    }
}

/// How long ago: `12m`, `5h`, `3d` (under a minute: `0m`).
#[must_use]
pub fn age(now_ms: u64, at_ms: u64) -> String {
    let minutes = now_ms.saturating_sub(at_ms) / 60_000;
    match minutes {
        0..=59 => format!("{minutes}m"),
        60..=2_879 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1_440),
    }
}

/// Local time, minute precision: `2026-10-19T14:05`.
#[must_use]
pub fn minute(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%dT%H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The local offset, said once in a header: `+02:00`.
#[must_use]
pub fn tz() -> String {
    chrono::Local::now().format("%:z").to_string()
}

/// Milliseconds of an RFC 3339 time (`None`: unreadable).
#[must_use]
pub fn ms_of(iso: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .and_then(|t| u64::try_from(t.timestamp_millis()).ok())
}

/// The answer cut at a line under [`CEILING_CHARS`], with the marker as its last line.
#[must_use]
pub fn ceiling(text: String) -> String {
    if text.chars().count() <= CEILING_CHARS {
        return text;
    }
    let marker = format!("…cut ceiling={CEILING_CHARS}\n");
    // room for the marker and for closing a zone the cut falls in
    let room = CEILING_CHARS.saturating_sub(marker.chars().count() + 64);
    let mut out = String::with_capacity(CEILING_CHARS * 2);
    let mut used = 0;
    let mut open: Option<usize> = None;
    for line in text.split_inclusive('\n') {
        let n = line.chars().count();
        if used + n > room {
            break;
        }
        let bare = line.trim_end_matches(['\n', '\r']);
        let fence = bare.len();
        match open {
            None if fence >= 3 && bare.bytes().all(|b| b == b'<') => open = Some(fence),
            Some(len) if fence == len && bare.bytes().all(|b| b == b'>') => open = None,
            _ => {}
        }
        out.push_str(line);
        used += n;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if let Some(len) = open {
        out.push_str(&">".repeat(len));
        out.push('\n');
    }
    out.push_str(&marker);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_values_are_quoted_only_when_they_need_it() {
        let line = Header::new("read")
            .kv("path", "docs/plan.md")
            .kv("at", "Plan > Rollout")
            .kv("q", "say \"hi\"")
            .kv("empty", "")
            .kv("eq", "a=b")
            .flag("cut", true)
            .flag("partial", false)
            .line();
        assert_eq!(
            line,
            r#"read path=docs/plan.md at="Plan > Rollout" q="say \"hi\"" empty="" eq="a=b" cut"#
        );
    }

    #[test]
    fn cells_escape_tabs_breaks_and_backslashes() {
        assert_eq!(cell("a\tb\nc\\d\re"), "a\\tb\\nc\\\\d\\re");
        assert!(matches!(cell("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn tables_group_three_rows_of_a_folder_and_drop_trailing_empties() {
        let rows: Vec<Vec<String>> = [
            ["README.md", "~200", ""],
            ["docs/a.md", "~10", "x"],
            ["docs/b.md", "~20", ""],
            ["docs/c.md", "~30", ""],
            ["src/d.rs", "~1", ""],
            ["src/e.rs", "~2", ""],
        ]
        .iter()
        .map(|r| r.iter().map(|s| (*s).to_owned()).collect())
        .collect();
        let mut out = String::new();
        table(&mut out, &["path", "size_tok", "tags"], &rows, Some(0));
        assert_eq!(
            out,
            "#cols path size_tok tags\nREADME.md\t~200\n@docs/\na.md\t~10\tx\nb.md\t~20\nc.md\t~30\n@/\nsrc/d.rs\t~1\nsrc/e.rs\t~2\n"
        );
    }

    #[test]
    fn a_middle_empty_cell_stays_empty_between_tabs() {
        let mut out = String::new();
        rows_only(
            &mut out,
            &[vec!["a".to_owned(), String::new(), "c".to_owned()]],
            None,
        );
        assert_eq!(out, "a\t\tc\n");
    }

    #[test]
    fn zones_take_a_longer_fence_than_any_line_of_their_text() {
        for (text, n) in [
            ("plain\n", 3),
            (">>> python prompt\n", 4),
            ("<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> branch\n", 8),
            ("  >>>> indented\n", 5),
            ("> quote\n>> nested\n", 3),
            ("", 3),
        ] {
            assert_eq!(fence_len(text), n, "{text:?}");
            let mut out = String::new();
            zone(&mut out, text);
            let fence_open = "<".repeat(n);
            let fence_close = ">".repeat(n);
            let lines: Vec<&str> = out.lines().collect();
            assert_eq!(lines[0], fence_open);
            assert_eq!(*lines.last().unwrap(), fence_close);
            // the text itself never holds a line equal to the closing fence
            assert!(!lines[1..lines.len() - 1].contains(&fence_close.as_str()));
            assert!(out.contains(text), "verbatim");
        }
    }

    #[test]
    fn sizes_and_ages_read_short() {
        assert_eq!(tokens(0), "~0");
        assert_eq!(tokens(340), "~340");
        assert_eq!(tokens(1_000), "~1k");
        assert_eq!(tokens(1_249), "~1.2k");
        assert_eq!(tokens(9_960), "~10k");
        assert_eq!(tokens(12_400), "~12k");
        assert_eq!(age(10 * 60_000, 0), "10m");
        assert_eq!(age(3 * 3_600_000, 0), "3h");
        assert_eq!(age(5 * 86_400_000, 0), "5d");
        assert_eq!(age(0, 5), "0m", "a time in the future is now");
    }

    #[test]
    fn the_ceiling_cuts_at_a_line_with_a_marker() {
        let text = (0..10_000).fold(String::new(), |mut t, i| {
            let _ = writeln!(t, "row {i}");
            t
        });
        let cut = ceiling(text);
        assert!(cut.chars().count() <= CEILING_CHARS);
        assert!(cut.ends_with("…cut ceiling=48000\n"));
        let before = cut.lines().rev().nth(1).unwrap();
        assert!(before.starts_with("row "), "{before}");
        assert_eq!(ceiling("short\n".to_owned()), "short\n");
        // one line longer than the ceiling: only the marker
        let one = ceiling("x".repeat(CEILING_CHARS + 1));
        assert_eq!(one, "…cut ceiling=48000\n");
        // a cut inside a zone closes the zone first
        let mut zoned = String::from("read path=a.md\n");
        zone(&mut zoned, &"line of text\n".repeat(5_000));
        let cut = ceiling(zoned);
        assert!(cut.ends_with("line of text\n>>>\n…cut ceiling=48000\n"));
        assert!(cut.chars().count() <= CEILING_CHARS);
    }
}
