//! Tags written in a file's front matter (`tags`). They count as the file's tags and are read-only for the app.
//!
//! Read: YAML (`tags: [a, b]`, a block list, `tags: a, b`, `tags: a b`) and TOML (`tags = ["a", "b"]`, one line or
//! several). Nothing else of the front matter is understood here; anything unreadable has no tags.

use crate::normalize::normalize_tags;

/// How much of the start of a file is read to find its front matter.
pub const HEAD_BYTES: usize = 64 * 1024;

/// The block at the very top of `text`: its kind (`true` = TOML) and the lines between the delimiters.
fn block(text: &str) -> Option<(bool, Vec<&str>)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    let first = lines.next()?.trim_end();
    let (toml, close): (bool, &[&str]) = match first {
        "---" => (false, &["---", "..."]),
        "+++" => (true, &["+++"]),
        _ => return None,
    };
    let mut body = Vec::new();
    for line in lines {
        if close.contains(&line.trim_end()) {
            return Some((toml, body));
        }
        body.push(line);
    }
    None
}

fn unquote(text: &str) -> String {
    let t = text.trim();
    for q in ['"', '\''] {
        if let Some(inner) = t.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner.to_owned();
        }
    }
    t.to_owned()
}

/// Cuts a trailing ` # comment` that is not inside quotes.
fn without_comment(text: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut prev_space = true;
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '#') if prev_space => return text[..at].trim_end(),
            _ => {}
        }
        prev_space = c.is_whitespace();
    }
    text
}

/// Items of `a, "b c", d` (a flow list's inside): split at commas outside quotes.
fn split_items(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in inner.chars() {
        match (quote, c) {
            (None, '"' | '\'') => {
                quote = Some(c);
                current.push(c);
            }
            (Some(q), c) if c == q => {
                quote = None;
                current.push(c);
            }
            (None, ',') => items.push(unquote(&std::mem::take(&mut current))),
            _ => current.push(c),
        }
    }
    items.push(unquote(&current));
    items
}

/// A scalar: one tag per comma-separated part, or per word when there is no comma.
fn scalar_tags(text: &str) -> Vec<String> {
    let text = unquote(without_comment(text));
    if text.contains(',') {
        text.split(',').map(str::to_owned).collect()
    } else {
        text.split_whitespace().map(str::to_owned).collect()
    }
}

fn flow_or_scalar(value: &str) -> Vec<String> {
    let value = without_comment(value).trim();
    match value.strip_prefix('[') {
        Some(rest) => split_items(rest.strip_suffix(']').unwrap_or(rest)),
        None => scalar_tags(value),
    }
}

fn yaml_tags(body: &[&str]) -> Vec<String> {
    for (at, line) in body.iter().enumerate() {
        let Some(rest) = line
            .strip_prefix("tags:")
            .or_else(|| line.strip_prefix("\"tags\":"))
            .or_else(|| line.strip_prefix("'tags':"))
        else {
            continue;
        };
        let rest = without_comment(rest).trim();
        if !rest.is_empty() {
            return flow_or_scalar(rest);
        }
        // a block list: the dash lines that follow
        let mut items = Vec::new();
        for next in &body[at + 1..] {
            let t = next.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            match t.strip_prefix('-') {
                Some(item) if item.is_empty() || item.starts_with(char::is_whitespace) => {
                    items.push(unquote(without_comment(item)));
                }
                _ => break,
            }
        }
        return items;
    }
    Vec::new()
}

fn toml_tags(body: &[&str]) -> Vec<String> {
    for (at, line) in body.iter().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("tags") else {
            continue;
        };
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let mut value = without_comment(value).trim().to_owned();
        if value.starts_with('[') && !value.contains(']') {
            // an array over several lines
            for next in &body[at + 1..] {
                value.push(' ');
                value.push_str(without_comment(next).trim());
                if next.contains(']') {
                    break;
                }
            }
        }
        return flow_or_scalar(&value);
    }
    Vec::new()
}

/// The normalized tags of a document's front matter.
#[must_use]
pub fn from_text(doc: &str) -> Vec<String> {
    let Some((toml, body)) = block(doc) else {
        return Vec::new();
    };
    let raw = if toml {
        toml_tags(&body)
    } else {
        yaml_tags(&body)
    };
    normalize_tags(&raw)
}
