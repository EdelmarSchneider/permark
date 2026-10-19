//! Prompt tags (`<context>` ... `</context>` on lines of their own): turned into one-line comment markers before the
//! parser runs (inner Markdown then parses normally and no line is added), into `<details>` after the HTML is made.

use std::borrow::Cow;

const OPEN_MARK: &str = "<!--pm:open:";
const CLOSE_MARK: &str = "<!--pm:close-->";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mark {
    Open(String),
    Close(String),
}

/// `<name>` | `<name attrs>` alone on a line (indent <= 3) -> the lowercase name; `</name>` likewise.
fn tag_line(line: &str) -> Option<Mark> {
    let body = line.trim_end();
    let indent = body.len() - body.trim_start().len();
    if indent > 3 || body.as_bytes().get(indent) != Some(&b'<') || !body.ends_with('>') {
        return None;
    }
    let inner = &body[indent + 1..body.len() - 1];
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let name_len = inner
        .char_indices()
        .find(|(i, c)| {
            !(c.is_ascii_alphabetic() || (*i > 0 && (c.is_ascii_digit() || *c == '-' || *c == '_')))
        })
        .map_or(inner.len(), |(i, _)| i);
    if name_len == 0 {
        return None;
    }
    let (name, rest) = inner.split_at(name_len);
    if closing {
        return rest
            .trim()
            .is_empty()
            .then(|| Mark::Close(name.to_ascii_lowercase()));
    }
    // attributes (if any) must be separated from the name; a trailing `/` is a self-closing tag: not a block
    let separated = rest.is_empty() || rest.starts_with([' ', '\t']);
    if !separated || rest.trim_end().ends_with('/') {
        return None;
    }
    Some(Mark::Open(name.to_ascii_lowercase()))
}

/// Opening or closing fence of a code block: (character, length).
fn fence_of(line: &str) -> Option<(u8, usize)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let first = *trimmed.as_bytes().first()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let length = trimmed.bytes().take_while(|b| *b == first).count();
    (length >= 3).then_some((first, length))
}

/// Marks the paired prompt tags of `text` (tags outside `tags`, inside fenced code, or unpaired stay as they are).
pub(super) fn mark<'t>(text: &'t str, tags: &[&str]) -> Cow<'t, str> {
    if !text.contains('<') {
        return Cow::Borrowed(text);
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut candidates: Vec<Option<Mark>> = vec![None; lines.len()];
    let mut fence: Option<(u8, usize)> = None;
    for (index, line) in lines.iter().enumerate() {
        let bare = line.trim_end_matches(['\n', '\r']);
        match (fence, fence_of(bare)) {
            (None, Some(open)) => fence = Some(open),
            (Some((c, n)), Some((c2, n2)))
                if c == c2 && n2 >= n && bare.trim().chars().all(|x| x == char::from(c)) =>
            {
                fence = None;
            }
            (Some(_), _) => {}
            (None, None) => {
                candidates[index] = tag_line(bare).filter(|mark| match mark {
                    Mark::Open(name) | Mark::Close(name) => tags.contains(&name.as_str()),
                });
            }
        }
    }
    // pair with a stack: a close pairs with the nearest open of its name; opens above it stay unpaired
    let mut stack: Vec<usize> = Vec::new();
    let mut paired = vec![false; lines.len()];
    for index in 0..lines.len() {
        match &candidates[index] {
            Some(Mark::Open(_)) => stack.push(index),
            Some(Mark::Close(name)) => {
                let at = stack.iter().rposition(
                    |open| matches!(&candidates[*open], Some(Mark::Open(n)) if n == name),
                );
                if let Some(at) = at {
                    paired[stack[at]] = true;
                    paired[index] = true;
                    stack.truncate(at);
                }
            }
            None => {}
        }
    }
    if !paired.contains(&true) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 32);
    for (index, line) in lines.iter().enumerate() {
        if !paired[index] {
            out.push_str(line);
            continue;
        }
        let eol = &line[line.trim_end_matches(['\n', '\r']).len()..];
        match &candidates[index] {
            Some(Mark::Open(name)) => {
                out.push_str(OPEN_MARK);
                out.push_str(name);
                out.push_str("-->");
            }
            _ => out.push_str(CLOSE_MARK),
        }
        out.push_str(eol);
    }
    Cow::Owned(out)
}

/// Turns the markers in rendered (not yet sanitized) HTML into `<details>` blocks.
pub(super) fn to_details(html: &str) -> Cow<'_, str> {
    if !html.contains(OPEN_MARK) && !html.contains(CLOSE_MARK) {
        return Cow::Borrowed(html);
    }
    let mut out = String::with_capacity(html.len() + 64);
    let mut rest = html;
    while let Some(at) = rest.find("<!--pm:") {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(after) = tail.strip_prefix(CLOSE_MARK) {
            out.push_str("</details>");
            rest = after;
        } else if let Some(open) = tail.strip_prefix(OPEN_MARK) {
            match open.split_once("-->") {
                Some((name, after))
                    if name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') =>
                {
                    out.push_str("<details class=\"prompt-block\" data-tag=\"");
                    out.push_str(name);
                    out.push_str("\" open><summary>");
                    out.push_str(name);
                    out.push_str("</summary>");
                    rest = after;
                }
                _ => {
                    out.push_str("<!--pm:");
                    rest = &tail["<!--pm:".len()..];
                }
            }
        } else {
            out.push_str("<!--pm:");
            rest = &tail["<!--pm:".len()..];
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::DEFAULT_PROMPT_TAGS;

    fn marked(text: &str) -> String {
        mark(text, DEFAULT_PROMPT_TAGS).into_owned()
    }

    #[test]
    fn paired_tags_become_same_line_markers() {
        let text = "<context>\nSome **text**\n</context>\n";
        assert_eq!(
            marked(text),
            "<!--pm:open:context-->\nSome **text**\n<!--pm:close-->\n"
        );
        assert_eq!(marked(text).lines().count(), text.lines().count());
    }

    #[test]
    fn attributes_case_indent_and_crlf() {
        let text = "  <Example id=\"1\">\r\nx\r\n  </EXAMPLE>\r\n";
        assert_eq!(
            marked(text),
            "<!--pm:open:example-->\r\nx\r\n<!--pm:close-->\r\n"
        );
    }

    #[test]
    fn unpaired_unknown_inline_and_deep_indent_stay() {
        for text in [
            "<context>\nno close\n",
            "</context>\nonly close\n",
            "<custom>\nx\n</custom>\n",
            "text <context> inline </context>\n",
            "    <context>\ncode\n    </context>\n",
            "<context/>\nx\n</context>\n",
            "<contextx>\nx\n</contextx>\n",
        ] {
            assert_eq!(marked(text), text, "{text:?}");
        }
    }

    #[test]
    fn tags_in_fenced_code_stay() {
        let text = "```\n<context>\nx\n</context>\n```\n~~~xml\n<task>\n</task>\n~~~\n";
        assert_eq!(marked(text), text);
        // after the fence closes, tags work again
        let text = "```\n<context>\n```\n<task>\nx\n</task>\n";
        assert_eq!(
            marked(text),
            "```\n<context>\n```\n<!--pm:open:task-->\nx\n<!--pm:close-->\n"
        );
    }

    #[test]
    fn nested_and_crossed_tags() {
        let nested = "<examples>\n<example>\na\n</example>\n</examples>\n";
        assert_eq!(
            marked(nested),
            "<!--pm:open:examples-->\n<!--pm:open:example-->\na\n<!--pm:close-->\n<!--pm:close-->\n"
        );
        // an open that is never closed inside a pair is left alone
        let crossed = "<task>\n<input>\nx\n</task>\n";
        assert_eq!(
            marked(crossed),
            "<!--pm:open:task-->\n<input>\nx\n<!--pm:close-->\n"
        );
    }

    #[test]
    fn details_from_markers() {
        let html = "<!--pm:open:context-->\n<p>x</p>\n<!--pm:close-->\n";
        assert_eq!(
            to_details(html),
            "<details class=\"prompt-block\" data-tag=\"context\" open><summary>context</summary>\n<p>x</p>\n</details>\n"
        );
        // a marker typed by an author with a hostile name is not turned into markup
        let hostile = "<!--pm:open:a\"onclick=x-->\n";
        assert!(!to_details(hostile).contains("<details"));
        assert_eq!(to_details("<p>plain</p>"), "<p>plain</p>");
    }
}
