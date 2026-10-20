//! What a note's text says about it: the title and the snippet the list shows (the first lines, Markdown marks taken
//! off) and the base of a file name made from a title. Pure functions over text.

/// The longest title kept, in characters. The list cuts it to the width of the window.
const TITLE_MAX_CHARS: usize = 120;
/// The longest snippet kept, in characters.
const SNIPPET_MAX_CHARS: usize = 160;
/// The longest base of a file name made from a title, in characters.
const STEM_MAX_CHARS: usize = 60;
/// How much of a note is read to find its title and snippet.
pub const HEAD_BYTES: usize = 8 * 1024;

/// The start of a file, which is all a title and a snippet need: empty when it cannot be read or is not text.
#[must_use]
pub fn head_of(path: &std::path::Path) -> String {
    use std::io::Read as _;

    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    if file
        .take(HEAD_BYTES as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return String::new();
    }
    if bytes[..bytes.len().min(512)].contains(&0) {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Whether the text has nothing but blank characters (a byte order mark counts as blank).
#[must_use]
pub fn is_blank(text: &str) -> bool {
    text.chars().all(|c| c.is_whitespace() || c == '\u{feff}')
}

/// The title of a note: its first line with the Markdown marks removed. Empty for a note with no text.
#[must_use]
pub fn title_of(text: &str) -> String {
    clean_lines(text)
        .next()
        .map(|line| cut(&line, TITLE_MAX_CHARS))
        .unwrap_or_default()
}

/// The snippet shown under the title: the lines after the first, marks removed and joined by a space.
#[must_use]
pub fn snippet_of(text: &str) -> String {
    let mut snippet = String::new();
    for line in clean_lines(text).skip(1) {
        if !snippet.is_empty() {
            snippet.push(' ');
        }
        snippet.push_str(&line);
        if snippet.chars().count() >= SNIPPET_MAX_CHARS {
            break;
        }
    }
    cut(&snippet, SNIPPET_MAX_CHARS)
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A base for a file name made from a title: what the file systems allow, not too long. `None` when nothing usable is
/// left (an empty title, or one made of marks only).
#[must_use]
pub fn stem_from_title(title: &str) -> Option<String> {
    let kept: String = title
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let squeezed = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = cut(&squeezed, STEM_MAX_CHARS);
    let trimmed = trimmed.trim_matches(|c: char| c == ' ' || c == '.');
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The lines of the note that carry text, each with its Markdown marks removed. Front matter, code fences, rules and
/// the separator row of a table say nothing about the note and are skipped.
fn clean_lines(text: &str) -> impl Iterator<Item = String> + '_ {
    let text = text.trim_start_matches('\u{feff}');
    without_front_matter(text).lines().filter_map(clean_line)
}

/// The text after a leading `---` ... `---` block (the whole text when it does not close).
fn without_front_matter(text: &str) -> &str {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return text;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return &rest[offset..];
        }
    }
    text
}

/// One line without its marks, or `None` when nothing is left.
fn clean_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("```") || line.starts_with("~~~") || is_rule(line) {
        return None;
    }
    let plain = strip_inline_marks(strip_block_marks(line));
    let squeezed = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    (!squeezed.is_empty()).then_some(squeezed)
}

/// A horizontal rule, or the separator row of a table (`---`, `***`, `| --- | :-: |`).
fn is_rule(line: &str) -> bool {
    let mut marks = 0;
    for c in line.chars() {
        match c {
            '-' | '*' | '_' | '=' => marks += 1,
            '|' | ':' | ' ' | '\t' => {}
            _ => return false,
        }
    }
    marks >= 3
}

/// Takes off the marks that start a block: quotes, headings, list items, task boxes.
fn strip_block_marks(line: &str) -> &str {
    let mut rest = line;
    // a quote of a list of a heading: a few rounds are enough
    for _ in 0..6 {
        let before = rest;
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix('>') {
            rest = after;
        } else if rest.starts_with('#') {
            let hashes = rest.chars().take_while(|&c| c == '#').count();
            let after = &rest[hashes..];
            if hashes <= 6
                && (after.is_empty() || after.starts_with(' ') || after.starts_with('\t'))
            {
                rest = after;
            }
        } else if let Some(after) = list_marker(rest) {
            rest = after;
        } else if let Some(after) = task_box(rest) {
            rest = after;
        }
        if rest == before {
            break;
        }
    }
    rest.trim()
}

/// The text after a bullet (`- `, `* `, `+ `) or a number (`1. `, `2) `).
fn list_marker(line: &str) -> Option<&str> {
    let mut chars = line.chars();
    match chars.next()? {
        '-' | '*' | '+' => chars
            .as_str()
            .strip_prefix(' ')
            .or_else(|| chars.as_str().strip_prefix('\t')),
        c if c.is_ascii_digit() => {
            let digits = line.chars().take_while(char::is_ascii_digit).count();
            let after = &line[digits..];
            let after = after
                .strip_prefix('.')
                .or_else(|| after.strip_prefix(')'))?;
            after.strip_prefix(' ')
        }
        _ => None,
    }
}

/// The text after a task box (`[ ]`, `[x]`).
fn task_box(line: &str) -> Option<&str> {
    let inner = line.strip_prefix('[')?;
    let mut chars = inner.chars();
    let mark = chars.next()?;
    if !matches!(mark, ' ' | 'x' | 'X') {
        return None;
    }
    chars.as_str().strip_prefix(']')
}

/// Takes the inline marks off a line: emphasis, code, links (the link text stays), images (the alt text stays), HTML
/// tags and backslash escapes.
fn strip_inline_marks(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if chars.get(i + 1).is_some_and(char::is_ascii_punctuation) => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '!' if chars.get(i + 1) == Some(&'[') => i += 1,
            '[' => {
                // [text](target) or [text][ref]: the text stays, the target goes
                if let Some(close) = bracket_end(&chars, i) {
                    let text: String = chars[i + 1..close].iter().collect();
                    out.push_str(&strip_inline_marks(&text));
                    i = skip_target(&chars, close + 1);
                } else {
                    out.push('[');
                    i += 1;
                }
            }
            '<' => match chars[i + 1..].iter().position(|&n| n == '>') {
                Some(offset) if offset > 0 && !chars[i + 1..i + 1 + offset].contains(&' ') => {
                    let inner: String = chars[i + 1..i + 1 + offset].iter().collect();
                    // an autolink shows its address; a tag shows nothing
                    if inner.contains("://") || inner.contains('@') {
                        out.push_str(&inner);
                    }
                    i += offset + 2;
                }
                _ => {
                    out.push('<');
                    i += 1;
                }
            },
            '`' | '*' | '~' => i += 1,
            '_' => {
                // emphasis marks sit at the edge of a word; the underscore inside `snake_case` is part of it
                let before = i.checked_sub(1).and_then(|p| chars.get(p));
                let after = chars.get(i + 1);
                let edge = !before.is_some_and(|c| c.is_alphanumeric())
                    || !after.is_some_and(|c| c.is_alphanumeric());
                if !edge {
                    out.push('_');
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// The index of the `]` that closes the `[` at `open`, counting nested brackets.
fn bracket_end(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0_i32;
    for (offset, &c) in chars[open..].iter().enumerate() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Past the `(target)` or `[reference]` that follows a link's text, when there is one.
fn skip_target(chars: &[char], at: usize) -> usize {
    let close = match chars.get(at) {
        Some('(') => ')',
        Some('[') => ']',
        _ => return at,
    };
    let mut depth = 0_i32;
    for (offset, &c) in chars[at..].iter().enumerate() {
        if c == chars[at] {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return at + offset + 1;
            }
        }
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_is_the_first_line_without_marks() {
        assert_eq!(title_of("# Server IPs\n\nbody"), "Server IPs");
        assert_eq!(
            title_of("- [x] **Buy** _milk_ and `eggs`"),
            "Buy milk and eggs"
        );
        assert_eq!(
            title_of("> 1. See [the plan](plan.md) now"),
            "See the plan now"
        );
        assert_eq!(title_of("![a picture](x.png) caption"), "a picture caption");
        assert_eq!(
            title_of("snake_case_name is kept"),
            "snake_case_name is kept"
        );
        assert_eq!(title_of(""), "");
        assert_eq!(title_of("  \n\n  "), "");
        assert_eq!(title_of("<https://x.org> go"), "https://x.org go");
        assert_eq!(title_of("a <b>bold</b> word"), "a bold word");
        assert_eq!(title_of(r"escaped \*stars\*"), "escaped *stars*");
    }

    #[test]
    fn front_matter_fences_rules_and_table_separators_say_nothing() {
        assert_eq!(title_of("---\ntitle: x\n---\n# Real title\n"), "Real title");
        assert_eq!(title_of("---\r\nk: v\r\n---\r\nAfter\r\n"), "After");
        // a front matter that never closes is text
        assert_eq!(title_of("---\nnot closed\n"), "not closed");
        assert_eq!(title_of("```\ncode\n```\nafter"), "code");
        assert_eq!(title_of("***\n---\n| --- | :-: |\nfirst"), "first");
        assert_eq!(title_of("\u{feff}# With BOM"), "With BOM");
    }

    #[test]
    fn the_snippet_is_the_following_lines_joined() {
        assert_eq!(snippet_of("# T\none\n\n- two\n"), "one two");
        assert_eq!(snippet_of("only a title"), "");
        let long = format!("T\n{}", "word ".repeat(100));
        assert_eq!(snippet_of(&long).chars().count(), SNIPPET_MAX_CHARS);
    }

    #[test]
    fn titles_are_cut_by_characters_never_in_the_middle_of_one() {
        let long = "é".repeat(300);
        assert_eq!(title_of(&long).chars().count(), TITLE_MAX_CHARS);
        let emoji = "😀".repeat(200);
        assert_eq!(title_of(&emoji).chars().count(), TITLE_MAX_CHARS);
        assert_eq!(title_of("日本語のメモ\n二行目"), "日本語のメモ");
    }

    #[test]
    fn a_file_name_stem_keeps_what_systems_allow() {
        assert_eq!(
            stem_from_title("Server IPs and ports").as_deref(),
            Some("Server IPs and ports")
        );
        assert_eq!(
            stem_from_title("a/b\\c:d*e?f\"g<h>i|j").as_deref(),
            Some("a b c d e f g h i j")
        );
        assert_eq!(stem_from_title("  ..dots..  ").as_deref(), Some("dots"));
        assert_eq!(stem_from_title("").as_deref(), None);
        assert_eq!(stem_from_title("???").as_deref(), None);
        assert_eq!(stem_from_title("tab\there").as_deref(), Some("tab here"));
        let long = stem_from_title(&"y".repeat(200)).unwrap();
        assert_eq!(long.chars().count(), STEM_MAX_CHARS);
    }

    #[test]
    fn blank_text_includes_a_byte_order_mark() {
        assert!(is_blank(""));
        assert!(is_blank(" \n\t\u{feff}"));
        assert!(!is_blank("x"));
    }

    #[test]
    fn the_head_of_a_file_is_empty_for_binary_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("a.md");
        std::fs::write(&text, "# Hello\nworld").unwrap();
        assert_eq!(head_of(&text), "# Hello\nworld");
        let binary = dir.path().join("b.md");
        std::fs::write(&binary, b"ab\0cd").unwrap();
        assert_eq!(head_of(&binary), "");
        assert_eq!(head_of(&dir.path().join("none.md")), "");
        // a big file is read only at its start
        let big = dir.path().join("big.md");
        std::fs::write(&big, "x".repeat(100_000)).unwrap();
        assert_eq!(head_of(&big).len(), HEAD_BYTES);
    }

    #[test]
    fn hostile_text_never_panics() {
        for text in [
            "[[[[[", "]]]]]", "[a](", "[a][b", "<<<<", "\\", "![", "#######", "```", "_ _ _",
            "\0\0\0", "[x]", "1.", "- ",
        ] {
            let _ = title_of(text);
            let _ = snippet_of(text);
            let _ = stem_from_title(text);
        }
    }
}
