//! What a note's text says about it: the title and the snippet shown in the list (the first lines, with the Markdown
//! marks taken off) and the base of the file names made from them. Pure functions over text.

/// The longest title kept, in characters. The list cuts it to the width of the window.
const TITLE_MAX_CHARS: usize = 120;
/// The longest snippet kept, in characters.
const SNIPPET_MAX_CHARS: usize = 160;
/// The longest base of a file name made from a title, in characters.
const STEM_MAX_CHARS: usize = 60;
/// How much of a note is read to find its title and snippet.
pub(crate) const HEAD_BYTES: usize = 8 * 1024;

/// The start of a file, which is all a title and a snippet need: empty when it cannot be read or is not text.
pub(crate) fn head_of(path: &std::path::Path) -> String {
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
pub(crate) fn is_blank(text: &str) -> bool {
    text.chars().all(|c| c.is_whitespace() || c == '\u{feff}')
}

/// The title of a note: its first line with the Markdown marks removed. Empty for a note with no text.
pub(crate) fn title_of(text: &str) -> String {
    clean_lines(text)
        .next()
        .map(|line| cut(&line, TITLE_MAX_CHARS))
        .unwrap_or_default()
}

/// The snippet shown under the title: the lines after the first, marks removed and joined by a space.
pub(crate) fn snippet_of(text: &str) -> String {
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

/// The first `max` characters of `text`.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A base for a file name made from a note's title: what the file systems allow, not too long. `None` when nothing
/// usable is left (an empty title, or one made of marks only).
pub(crate) fn stem_from_title(title: &str) -> Option<String> {
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
    let body = without_front_matter(text);
    body.lines().filter_map(clean_line)
}

/// The text after a leading `---` ... `---` block (none when it does not close).
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
    if line.is_empty() || line.starts_with("```") || line.starts_with("~~~") {
        return None;
    }
    if is_rule(line) {
        return None;
    }
    let body = strip_block_marks(line);
    let plain = strip_inline_marks(body);
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

/// Takes the inline marks off a line: emphasis, code, links (the link text stays), images (the alt text stays), HTML tags
/// and backslash escapes.
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
    let mut depth = 0;
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
    let mut depth = 0;
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
    fn the_title_is_the_first_line_without_its_marks() {
        assert_eq!(
            title_of("# Server IPs and ports\n\napi 10.0.0.4"),
            "Server IPs and ports"
        );
        assert_eq!(
            title_of("## **Bold** and _lean_ `code`"),
            "Bold and lean code"
        );
        assert_eq!(title_of("> a quote\n"), "a quote");
        assert_eq!(title_of("- [ ] buy milk"), "buy milk");
        assert_eq!(title_of("1. first\n2. second"), "first");
        assert_eq!(
            title_of("[the docs](https://example.com/x) are here"),
            "the docs are here"
        );
        assert_eq!(
            title_of("![a diagram](img/a.png) caption"),
            "a diagram caption"
        );
        assert_eq!(title_of("snake_case_name stays"), "snake_case_name stays");
        assert_eq!(
            title_of("<https://example.com> is up"),
            "https://example.com is up"
        );
        assert_eq!(title_of("a <b>bold</b> word"), "a bold word");
        assert_eq!(title_of(r"price \* 2"), "price * 2");
        assert_eq!(title_of("a < b and b > c"), "a < b and b > c");
    }

    #[test]
    fn a_note_without_text_has_no_title() {
        assert_eq!(title_of(""), "");
        assert_eq!(title_of("  \n\t\n"), "");
        assert_eq!(title_of("\u{feff}\n"), "");
        assert_eq!(title_of("---\n***\n"), "");
        assert!(is_blank(" \r\n\t\u{feff}"));
        assert!(!is_blank("  x "));
    }

    #[test]
    fn blank_lines_fences_rules_and_front_matter_do_not_make_the_title() {
        assert_eq!(title_of("\n\n   \n# After blanks"), "After blanks");
        assert_eq!(title_of("---\ntitle: x\n---\n\n# Real title"), "Real title");
        assert_eq!(title_of("---\nnever closed\n"), "never closed");
        assert_eq!(title_of("\u{feff}# With a BOM"), "With a BOM");
        // the separator row of a table is skipped
        let table = "| a | b |\n| --- | --- |\n| 1 | 2 |";
        assert_eq!(title_of(table), "| a | b |");
        assert_eq!(snippet_of(table), "| 1 | 2 |");
    }

    #[test]
    fn the_title_is_cut_to_a_sane_length() {
        let long = "x".repeat(500);
        assert_eq!(title_of(&long).chars().count(), TITLE_MAX_CHARS);
        let accents = "á".repeat(500);
        assert_eq!(title_of(&accents).chars().count(), TITLE_MAX_CHARS);
    }

    #[test]
    fn the_snippet_is_what_follows_the_title() {
        let text = "# Weekly checklist\n\n- [ ] backups\n- [x] **patches**\n\nnotes here";
        assert_eq!(snippet_of(text), "backups patches notes here");
        assert_eq!(snippet_of("only a title"), "");
        assert_eq!(snippet_of(""), "");
        let long = format!("title\n{}", "word ".repeat(100));
        assert_eq!(snippet_of(&long).chars().count(), SNIPPET_MAX_CHARS);
    }

    #[test]
    fn a_file_name_base_keeps_what_the_systems_allow() {
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
        let long = stem_from_title(&"y".repeat(200)).expect("a stem");
        assert_eq!(long.chars().count(), STEM_MAX_CHARS);
    }
}
