//! Plugin `fmt-dotenv`: `.env` files as a sheet; a value edit replaces one value token and keeps its quotes.

use std::collections::HashMap;
use std::sync::Arc;

use permark_core::format::config::util::{
    caption, comment_above, escape_double, is_number, join_index, join_key, trailing_comment,
    unescape_double, Lines,
};
use permark_core::format::config::{
    Group, Quote, Row, RowValue, Scalar, ScalarKind, Sheet, SheetKind,
};
use permark_core::format::{
    Claim, ConfigFormat, ConfigSyntax, Contributions, EditRefusal, FormatId, NewValue,
};
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "fmt-dotenv",
        PluginKind::Format,
        Contributions::default().formats(vec![Arc::new(ConfigFormat(Dotenv))]),
    )
}

/// The dotenv syntax.
#[derive(Debug, Clone, Copy)]
pub struct Dotenv;

impl ConfigSyntax for Dotenv {
    fn id(&self) -> FormatId {
        FormatId::DOTENV
    }

    fn claims(&self, name: &str, ext: Option<&str>, _head: &[u8]) -> Claim {
        let lower = name.to_ascii_lowercase();
        if lower == ".env" || lower.starts_with(".env.") || ext == Some("env") {
            Claim::Strong
        } else {
            Claim::No
        }
    }

    fn language(&self) -> &'static str {
        "dotenv"
    }

    fn parse(&self, text: &str) -> Sheet {
        let lines = Lines::new(text);
        let mut rows: Vec<Row> = Vec::new();
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut n = 0;
        while n < lines.count() {
            let (start, end, _) = lines.bounds(n).unwrap_or((0, 0, 0));
            let line = &text[start..end];
            let lead = line.len() - line.trim_start().len();
            let content = &line[lead..];
            n += 1;
            if content.is_empty() || content.starts_with('#') {
                continue;
            }
            let content_start = start + lead;
            let (content, content_start) = match content.strip_prefix("export") {
                Some(rest) if rest.starts_with([' ', '\t']) => {
                    let skipped = rest.len() - rest.trim_start().len();
                    (rest.trim_start(), content_start + "export".len() + skipped)
                }
                _ => (content, content_start),
            };
            let key_len = content
                .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-')))
                .unwrap_or(content.len());
            if key_len == 0 {
                continue;
            }
            let key = &content[..key_len];
            let after_key = &content[key_len..];
            let spaces = after_key.len() - after_key.trim_start_matches([' ', '\t']).len();
            let Some(after_eq) = after_key[spaces..].strip_prefix('=') else {
                continue;
            };
            let gap = after_eq.len() - after_eq.trim_start_matches([' ', '\t']).len();
            let value_start = content_start + key_len + spaces + 1 + gap;
            let (row, last) = value(text, &lines, n - 1, key, value_start, &mut seen);
            n = last + 1;
            rows.push(row);
        }
        let end = rows.iter().map(|r| r.end).max().unwrap_or(1);
        Sheet {
            kind: SheetKind::Config,
            groups: vec![Group {
                path: String::new(),
                title: String::new(),
                line: 1,
                end,
                comment: None,
                rows,
            }],
            spec: None,
            failed: false,
        }
    }

    fn token(&self, old: &Scalar, new: &NewValue) -> Result<String, EditRefusal> {
        match (old.kind, new) {
            (ScalarKind::String | ScalarKind::Null, NewValue::Text { text }) => {
                string_token(old.quote, text)
            }
            (ScalarKind::Bool, NewValue::Literal { text }) if text == "true" || text == "false" => {
                Ok(text.clone())
            }
            (ScalarKind::Number, NewValue::Literal { text }) if is_number(text) => Ok(text.clone()),
            _ => Err(EditRefusal::Invalid("kind")),
        }
    }
}

/// A string as the file writes it: the quote kind it had when the text fits, else double quotes. A file of this kind
/// is often read by a shell (`source .env`, `export KEY=value`): a plain value never holds spaces or shell signs, and
/// a value with `$` or a backquote goes in single quotes (no expansion there), or is refused when it cannot.
fn string_token(quote: Quote, text: &str) -> Result<String, EditRefusal> {
    let control = text.chars().any(|c| c.is_control() && c != '\t');
    let expands = text.contains(['$', '`']);
    let plain_ok = !control
        && !text.contains(char::is_whitespace)
        && !text.contains([
            '"', '\'', '\\', '`', '$', ';', '|', '&', '(', ')', '<', '>', '#', '*', '?', '[', ']',
            '{', '}', '!', '~',
        ]);
    let single_ok = !control && !text.contains('\'');
    match quote {
        Quote::Plain if plain_ok => Ok(text.to_owned()),
        Quote::Single if single_ok => Ok(format!("'{text}'")),
        _ if expands && single_ok => Ok(format!("'{text}'")),
        _ if expands => Err(EditRefusal::Invalid("notRepresentable")),
        _ => Ok(format!("\"{}\"", escape_double(text))),
    }
}

/// One `KEY=value` whose value starts at byte `at` of line `n`. Returns the row and its last line.
fn value(
    text: &str,
    lines: &Lines<'_>,
    n: usize,
    key: &str,
    at: usize,
    seen: &mut HashMap<String, usize>,
) -> (Row, usize) {
    let (_, line_end, _) = lines.bounds(n).unwrap_or((0, 0, 0));
    let rest = &text[at..line_end];
    let base = join_key("", key);
    let count = seen.entry(base.clone()).or_insert(0);
    let path = if *count == 0 {
        base
    } else {
        join_index(&base, *count)
    };
    *count += 1;
    let mut last = n;
    let (kind, shown, span, quote, trailing, read_only);
    if let Some(open) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        // quoted: to the matching quote, maybe on a later line
        if let Some((close, close_line)) = closing(text, lines, n, at, open) {
            last = close_line;
            let inner = &text[at + 1..close - 1];
            let (_, e, _) = lines.bounds(last).unwrap_or((0, 0, 0));
            trailing = trailing_comment(&text[close..e], &['#']);
            kind = ScalarKind::String;
            shown = if open == '"' {
                unescape_double(inner)
            } else {
                inner.to_owned()
            };
            span = at..close;
            quote = if open == '"' {
                Quote::Double
            } else {
                Quote::Single
            };
            read_only = (last != n).then_some("multiline");
        } else {
            // never closes: the rest of the line, as it is
            kind = ScalarKind::String;
            shown = rest.to_owned();
            span = at..line_end;
            quote = Quote::Plain;
            trailing = None;
            read_only = Some("unterminated");
        }
    } else {
        let end = plain_end(rest);
        let token = &rest[..end];
        trailing = trailing_comment(&rest[end..], &['#']);
        kind = if token.is_empty() {
            ScalarKind::Null
        } else if token == "true" || token == "false" {
            ScalarKind::Bool
        } else if is_number(token) {
            ScalarKind::Number
        } else {
            ScalarKind::String
        };
        shown = token.to_owned();
        span = at..at + end;
        quote = Quote::Plain;
        read_only = None;
    }
    let mut scalar = Scalar::new(path.clone(), kind, shown, span).quoted(quote);
    if let Some(reason) = read_only {
        scalar = scalar.read_only(reason);
    }
    let above = comment_above(lines, n, &['#']);
    (
        Row {
            path,
            key: key.to_owned(),
            line: u32::try_from(n + 1).unwrap_or(u32::MAX),
            end: u32::try_from(last + 1).unwrap_or(u32::MAX),
            value: RowValue::Scalar(scalar),
            comment: caption(above, trailing),
        },
        last,
    )
}

/// Where the quote opened at byte `at` of line `n` closes: the byte after the closing quote and its line. Double
/// quotes honor backslash escapes; the search stops 500 lines on.
fn closing(
    text: &str,
    lines: &Lines<'_>,
    n: usize,
    at: usize,
    open: char,
) -> Option<(usize, usize)> {
    let mut from = at + 1;
    for line in n..lines.count().min(n + 500) {
        let (start, end, _) = lines.bounds(line)?;
        if line > n {
            from = start;
        }
        let bytes = &text.as_bytes()[from..end];
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' && open == '"' {
                i += 2;
                continue;
            }
            if bytes[i] == open as u8 {
                return Some((from + i + 1, line));
            }
            i += 1;
        }
    }
    None
}

/// End of an unquoted value: before ` #` and trailing spaces.
fn plain_end(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    let mut end = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'#' && i > 0 && matches!(bytes[i - 1], b' ' | b'\t') {
            end = i;
            break;
        }
    }
    rest[..end].trim_end().len()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod manifest_tests {
    use super::*;

    #[test]
    fn manifest_is_a_valid_app_plugin() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "fmt-dotenv");
        assert_eq!(manifest.kind, PluginKind::Format);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }
}
