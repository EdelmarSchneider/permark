//! A small YAML subset for the properties card: flat `key: value` pairs.
//!
//! Supported: plain and quoted scalars, trailing comments, inline lists (`[a, b]`), block lists (`- a`), block
//! scalars (`|` and `>`), nested maps (flattened to `parent.child`). Anything fancier (anchors, tags, flow maps,
//! complex keys) is kept as its raw text rather than failing.

use permark_core::format::{Field, FieldValue};

use crate::clip;

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// Removes a trailing ` # comment` that is not inside quotes.
fn strip_comment(text: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for (at, c) in text.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' && previous.is_whitespace() => return text[..at].trim_end(),
            _ => {}
        }
        previous = c;
    }
    text.trim_end()
}

/// `'it''s'` | `"say \"hi\""` | plain -> the text.
fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            return inner
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
                .replace("\\n", "\n");
        }
        if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            return inner.replace("''", "'");
        }
    }
    value.to_owned()
}

/// Splits `key: value`; the colon must be followed by a space or end the line. The key may be quoted.
fn split_key(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    if let Some(quote) = trimmed.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let close = trimmed[1..].find(quote)? + 1;
        let rest = trimmed[close + 1..].strip_prefix(':')?;
        if !rest.is_empty() && !rest.starts_with(' ') {
            return None;
        }
        return Some((trimmed[1..close].to_owned(), rest.trim()));
    }
    let mut from = 0;
    while let Some(at) = trimmed[from..].find(':') {
        let colon = from + at;
        let after = &trimmed[colon + 1..];
        if after.is_empty() || after.starts_with(' ') {
            let key = trimmed[..colon].trim();
            return (!key.is_empty()).then(|| (key.to_owned(), after.trim()));
        }
        from = colon + 1;
    }
    None
}

/// Items of an inline list, split at commas outside quotes. A bracketed item inside stays as written.
fn inline_items(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in inner.chars() {
        match quote {
            Some(q) if c == q => {
                quote = None;
                current.push(c);
            }
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                current.push(c);
            }
            None if c == ',' => items.push(unquote(&std::mem::take(&mut current))),
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        items.push(unquote(&current));
    }
    items.into_iter().filter(|i| !i.is_empty()).collect()
}

fn text(value: &str) -> FieldValue {
    FieldValue::Text { text: clip(value) }
}

fn list(items: &[String]) -> FieldValue {
    FieldValue::List {
        items: items.iter().map(|i| clip(i)).collect(),
    }
}

/// What follows a `key:` with nothing after the colon.
enum Block {
    /// A block list; parsing continues at `next`.
    List { items: Vec<String>, next: usize },
    /// A nested map: its keys are flattened under this one.
    Nested,
    /// Just an empty value.
    Empty,
}

fn block_after(lines: &[&str], at: usize, indent: usize) -> Block {
    let mut items = Vec::new();
    let mut next = at;
    while next < lines.len() {
        let candidate = lines[next];
        if candidate.trim().is_empty() {
            next += 1;
            continue;
        }
        let trimmed = candidate.trim_start();
        let item = trimmed
            .strip_prefix("- ")
            .or_else(|| (trimmed == "-").then_some(""));
        match item {
            // a list item at the key's own indent is still its list (`key:` then `- a`)
            Some(item) if indent_of(candidate) >= indent => {
                items.push(unquote(strip_comment(item)));
                next += 1;
            }
            _ => break,
        }
    }
    if !items.is_empty() {
        return Block::List { items, next };
    }
    let nested = lines[at..]
        .iter()
        .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .is_some_and(|l| indent_of(l) > indent && split_key(l).is_some());
    if nested {
        Block::Nested
    } else {
        Block::Empty
    }
}

/// The lines of a block scalar (`|` keeps line breaks, `>` folds them) and where parsing continues.
fn block_scalar(lines: &[&str], at: usize, indent: usize, literal: bool) -> (String, usize) {
    let mut next = at;
    let mut body: Vec<&str> = Vec::new();
    while next < lines.len() && (lines[next].trim().is_empty() || indent_of(lines[next]) > indent) {
        body.push(lines[next].trim());
        next += 1;
    }
    while body.last().is_some_and(|l| l.is_empty()) {
        body.pop();
    }
    let joined = if literal {
        body.join("\n")
    } else {
        body.iter()
            .map(|l| if l.is_empty() { "\n" } else { l })
            .collect::<Vec<_>>()
            .join(" ")
    };
    (joined, next)
}

fn is_block_scalar_header(value: &str) -> bool {
    value.starts_with(['|', '>'])
        && value
            .chars()
            .skip(1)
            .all(|c| matches!(c, '+' | '-' | '0'..='9'))
}

/// Fields of the YAML subset; empty input gives no fields.
pub(crate) fn parse(raw: &str) -> Vec<Field> {
    let lines: Vec<&str> = raw.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut fields = Vec::new();
    let mut parents: Vec<(usize, String)> = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        at += 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = indent_of(line);
        let Some((key, rest)) = split_key(line) else {
            continue;
        };
        while parents.last().is_some_and(|(i, _)| *i >= indent) {
            parents.pop();
        }
        let full = parents
            .iter()
            .map(|(_, k)| k.as_str())
            .chain(std::iter::once(key.as_str()))
            .collect::<Vec<_>>()
            .join(".");
        let value = strip_comment(rest);
        let field = if value.is_empty() {
            match block_after(&lines, at, indent) {
                Block::List { items, next } => {
                    at = next;
                    list(&items)
                }
                Block::Nested => {
                    parents.push((indent, key));
                    continue;
                }
                Block::Empty => text(""),
            }
        } else if is_block_scalar_header(value) {
            let (body, next) = block_scalar(&lines, at, indent, value.starts_with('|'));
            at = next;
            text(&body)
        } else if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            list(&inline_items(inner))
        } else {
            // flow maps, anchors, aliases, tags: the raw text
            text(&unquote(value))
        };
        fields.push(Field {
            key: full,
            value: field,
        });
    }
    fields
}

#[cfg(test)]
#[allow(clippy::assert_is_empty)] // `assert!(x.is_empty())` reads best for "nothing found"
mod tests {
    use std::fmt::Write;

    use super::*;

    fn t(text: &str) -> FieldValue {
        FieldValue::Text {
            text: text.to_owned(),
        }
    }

    fn l(items: &[&str]) -> FieldValue {
        FieldValue::List {
            items: items.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn shape(raw: &str) -> Vec<(String, FieldValue)> {
        parse(raw).into_iter().map(|f| (f.key, f.value)).collect()
    }

    #[test]
    fn scalars_lists_and_quotes() {
        let got = shape(
            "title: A plan\nstatus: \"in: progress\"\nowner: 'it''s me'\ntags: [a, \"b, c\", 'd']\nflag: true\ndate: 2026-10-19\nurl: https://example.com/a:b\nempty:\n",
        );
        assert_eq!(
            got,
            [
                ("title".into(), t("A plan")),
                ("status".into(), t("in: progress")),
                ("owner".into(), t("it's me")),
                ("tags".into(), l(&["a", "b, c", "d"])),
                ("flag".into(), t("true")),
                ("date".into(), t("2026-10-19")),
                ("url".into(), t("https://example.com/a:b")),
                ("empty".into(), t("")),
            ]
        );
    }

    #[test]
    fn block_lists_nested_maps_and_block_scalars() {
        let got = shape(
            "tags:\n  - one\n  - \"two\"  # note\nauthor:\n  name: Ana\n  team:\n    id: 7\nafter: x\nsummary: |\n  line one\n  line two\nfolded: >\n  a\n  b\n\n  c\nlast: end\n",
        );
        assert_eq!(
            got,
            [
                ("tags".into(), l(&["one", "two"])),
                ("author.name".into(), t("Ana")),
                ("author.team.id".into(), t("7")),
                ("after".into(), t("x")),
                ("summary".into(), t("line one\nline two")),
                ("folded".into(), t("a b \n c")),
                ("last".into(), t("end")),
            ]
        );
    }

    #[test]
    fn comments_blank_lines_and_odd_input() {
        assert_eq!(
            shape("# header\n\na: 1 # trailing\nb: \"x # not a comment\"\n"),
            [("a".into(), t("1")), ("b".into(), t("x # not a comment"))]
        );
        assert!(shape("").is_empty());
        assert!(shape("just text\n- only a list\n").is_empty());
        // CRLF
        assert_eq!(
            shape("a: 1\r\nb: [x, y]\r\n"),
            [("a".into(), t("1")), ("b".into(), l(&["x", "y"]))]
        );
        // fancy things stay as raw text instead of failing
        let fancy = shape("a: &anchor value\nb: *anchor\nc: {x: 1, y: 2}\nd: !!str 5\n");
        assert_eq!(fancy.len(), 4);
        assert_eq!(fancy[2].1, t("{x: 1, y: 2}"));
    }

    #[test]
    fn hostile_input_never_panics_or_blows_up() {
        let nested = format!("a: [{}]\n", "[".repeat(100_000));
        assert_eq!(parse(&nested).len(), 1);
        let mut deep = String::new();
        for i in 0..2000 {
            let _ = writeln!(deep, "{}k{i}:", " ".repeat(i % 50));
        }
        let _ = parse(&deep);
        let _ = parse(&"- ".repeat(50_000));
        let _ = parse("\u{0}: \u{202e}\n'\n\"\n: x\n:\n");
        let _ = parse(&"a: |\n".repeat(10_000));
    }
}
