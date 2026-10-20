//! Plugin `fmt-ini`: INI-style files (`.ini`, `.cfg`, `.editorconfig`, `.gitconfig`, ...) as a sheet; a value edit
//! replaces one value token.

use std::collections::HashMap;
use std::sync::Arc;

use permark_core::format::config::util::{
    comment_above, escape_double, is_number, join_index, join_key, trailing_comment,
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
        "fmt-ini",
        PluginKind::Format,
        Contributions::default().formats(vec![Arc::new(ConfigFormat(Ini))]),
    )
}

/// Names that are INI files without an extension that says so.
const NAMES: [&str; 7] = [
    ".editorconfig",
    ".gitconfig",
    ".gitmodules",
    ".npmrc",
    ".pypirc",
    ".flake8",
    ".pylintrc",
];

const BOOLS: [&str; 6] = ["true", "false", "yes", "no", "on", "off"];

/// The INI syntax.
#[derive(Debug, Clone, Copy)]
pub struct Ini;

/// Whether the start of a `.conf` file reads like INI (a `[section]` line or a `key = value` line).
fn looks_like_ini(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head);
    text.lines().take(60).any(|line| {
        let t = line.trim();
        (t.starts_with('[') && t.ends_with(']'))
            || t.split_once('=').is_some_and(|(k, _)| {
                !k.trim().is_empty() && !k.contains(char::is_whitespace) || k.trim().contains('.')
            })
    })
}

impl ConfigSyntax for Ini {
    fn id(&self) -> FormatId {
        FormatId::INI
    }

    fn claims(&self, name: &str, ext: Option<&str>, head: &[u8]) -> Claim {
        let lower = name.to_ascii_lowercase();
        if NAMES.contains(&lower.as_str()) || matches!(ext, Some("ini" | "cfg")) {
            Claim::Strong
        } else if ext == Some("conf") && looks_like_ini(head) {
            Claim::Weak
        } else {
            Claim::No
        }
    }

    fn language(&self) -> &'static str {
        "ini"
    }

    fn parse(&self, text: &str) -> Sheet {
        let lines = Lines::new(text);
        let mut groups: Vec<Group> = vec![Group {
            path: String::new(),
            title: String::new(),
            line: 1,
            end: 1,
            comment: None,
            rows: Vec::new(),
        }];
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut n = 0;
        while n < lines.count() {
            let info = lines.bounds(n).unwrap_or((0, 0, 0));
            let line = &text[info.0..info.1];
            let t = line.trim();
            let line_no = u32::try_from(n + 1).unwrap_or(u32::MAX);
            if t.is_empty() || t.starts_with([';', '#']) {
                n += 1;
                continue;
            }
            if let Some(inner) = t.strip_prefix('[') {
                let name = inner.split(']').next().unwrap_or(inner).trim();
                // a section written twice continues its keys' numbering (its rows never take the first one's paths)
                let path = section_path(name);
                groups.push(Group {
                    path,
                    title: name.to_owned(),
                    line: line_no,
                    end: line_no,
                    comment: comment_above(&lines, n, &[';', '#']),
                    rows: Vec::new(),
                });
                n += 1;
                continue;
            }
            // `key = value`
            let offset = info.0 + (line.len() - line.trim_start().len());
            let (row, last) = entry(text, &lines, n, offset, info.1, &mut seen, &groups);
            if let Some(group) = groups.last_mut() {
                group.end = group.end.max(row.end);
                group.rows.push(row);
            }
            n = last + 1;
        }
        groups.retain(|g| !g.rows.is_empty() || !g.path.is_empty());
        Sheet {
            kind: SheetKind::Config,
            groups,
            spec: None,
            failed: false,
        }
    }

    fn token(&self, old: &Scalar, new: &NewValue) -> Result<String, EditRefusal> {
        match (old.kind, new) {
            (ScalarKind::String | ScalarKind::Null, NewValue::Text { text }) => {
                string_token(old.quote, text)
            }
            (ScalarKind::Bool, NewValue::Literal { text }) if BOOLS.contains(&text.as_str()) => {
                Ok(text.clone())
            }
            (ScalarKind::Number, NewValue::Literal { text }) if is_number(text) => Ok(text.clone()),
            _ => Err(EditRefusal::Invalid("kind")),
        }
    }
}

/// `remote "origin"` -> `remote.origin`; otherwise the name as one key.
fn section_path(name: &str) -> String {
    if let Some((first, rest)) = name.split_once(char::is_whitespace) {
        let rest = rest.trim();
        if let Some(sub) = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
            return join_key(&join_key("", first), &unescape_double(sub));
        }
    }
    join_key("", name)
}

/// A string as the file writes it, or why it cannot be written that way.
fn string_token(quote: Quote, text: &str) -> Result<String, EditRefusal> {
    if text.contains(['\n', '\r']) {
        return Err(EditRefusal::Invalid("newline"));
    }
    if quote == Quote::Double {
        return Ok(format!("\"{}\"", escape_double(text)));
    }
    let plain_ok = text.trim() == text
        && !text.contains(" #")
        && !text.contains(" ;")
        && !text.starts_with(['"', '\'', ';', '#'])
        && !text.contains('\\');
    if plain_ok {
        Ok(text.to_owned())
    } else {
        Err(EditRefusal::Invalid("notRepresentable"))
    }
}

/// The value part after `key =`: its token and a trailing comment. Quoted values keep their quotes in the token.
fn entry(
    text: &str,
    lines: &Lines<'_>,
    n: usize,
    start: usize,
    line_end: usize,
    seen: &mut HashMap<String, usize>,
    groups: &[Group],
) -> (Row, usize) {
    let line_no = u32::try_from(n + 1).unwrap_or(u32::MAX);
    let content = &text[start..line_end];
    let sep = content.find('=').or_else(|| {
        // `key: value` only when the key is a single word
        content
            .find(':')
            .filter(|i| !content[..*i].trim().contains(char::is_whitespace))
    });
    let group_path = groups.last().map_or("", |g| g.path.as_str());
    let Some(sep) = sep else {
        // a key with no value (git: true) or a line that is not a pair at all
        let key = content.trim();
        let path = unique(seen, group_path, key);
        let scalar = Scalar::new(
            path.clone(),
            ScalarKind::Null,
            String::new(),
            line_end..line_end,
        )
        .read_only("noValue");
        return (
            Row {
                path,
                key: key.to_owned(),
                line: line_no,
                end: line_no,
                value: RowValue::Scalar(scalar),
                comment: comment_above(lines, n, &[';', '#']),
            },
            n,
        );
    };
    let key = content[..sep].trim();
    let path = unique(seen, group_path, key);
    let after = &content[sep + 1..];
    let skipped = after.len() - after.trim_start().len();
    let value_start = start + sep + 1 + skipped;
    let rest = &text[value_start..line_end];
    let mut last = n;
    let (token_end, quote, trailing, continued) = split_value(rest);
    let mut span = value_start..value_start + token_end;
    let mut read_only = None;
    if continued {
        // a trailing backslash continues the value on the next line
        while last + 1 < lines.count() {
            last += 1;
            let (_, e, _) = lines.bounds(last).unwrap_or((0, 0, 0));
            span.end = e;
            if !text[..e].ends_with('\\') {
                break;
            }
        }
        read_only = Some("continuation");
    }
    let raw = &text[span.clone()];
    let (kind, shown) = if quote == Quote::Double {
        (
            ScalarKind::String,
            unescape_double(
                raw.strip_prefix('"')
                    .and_then(|r| r.strip_suffix('"'))
                    .unwrap_or(raw),
            ),
        )
    } else if raw.is_empty() {
        (ScalarKind::Null, String::new())
    } else if BOOLS.contains(&raw) {
        (ScalarKind::Bool, raw.to_owned())
    } else if is_number(raw) {
        (ScalarKind::Number, raw.to_owned())
    } else {
        (ScalarKind::String, raw.to_owned())
    };
    let mut scalar = Scalar::new(path.clone(), kind, shown, span).quoted(quote);
    if let Some(reason) = read_only {
        scalar = scalar.read_only(reason);
    }
    let above = comment_above(lines, n, &[';', '#']);
    let comment = match (above, trailing) {
        (Some(a), Some(t)) => Some(format!("{a} {t}")),
        (a, t) => a.or(t),
    };
    (
        Row {
            path,
            key: key.to_owned(),
            line: line_no,
            end: u32::try_from(last + 1).unwrap_or(u32::MAX),
            value: RowValue::Scalar(scalar),
            comment,
        },
        last,
    )
}

/// The value token at the start of `rest`: where it ends, how it is quoted, its trailing comment, and whether a
/// backslash continues it on the next line.
fn split_value(rest: &str) -> (usize, Quote, Option<String>, bool) {
    if rest.starts_with('"') {
        // quoted: up to the closing quote
        return match closing_quote(rest) {
            Some(close) => (
                close,
                Quote::Double,
                trailing_comment(&rest[close..], &[';', '#']),
                false,
            ),
            None => (rest.len(), Quote::Plain, None, false),
        };
    }
    let end = unquoted_end(rest);
    let comment = trailing_comment(&rest[end..], &[';', '#']);
    (end, Quote::Plain, comment, rest[..end].ends_with('\\'))
}

/// The path of a key in the current section; a repeated key is `key[1]`, `key[2]`, ...
fn unique(seen: &mut HashMap<String, usize>, group_path: &str, key: &str) -> String {
    let base = join_key(group_path, key);
    let count = seen.entry(base.clone()).or_insert(0);
    let path = if *count == 0 {
        base
    } else {
        join_index(&base, *count)
    };
    *count += 1;
    path
}

/// End of the closing quote of a value that starts with `"` (escapes honored); none when it does not close.
fn closing_quote(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// End of an unquoted value: before ` ;` | ` #` and trailing spaces.
fn unquoted_end(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    let mut end = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if matches!(b, b';' | b'#') && i > 0 && matches!(bytes[i - 1], b' ' | b'\t') {
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
        assert_eq!(manifest.id.as_str(), "fmt-ini");
        assert_eq!(manifest.kind, PluginKind::Format);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }
}
