//! Plugin `fmt-properties`: Java-style `.properties` files as a sheet; a value edit replaces the value text of a
//! single-line entry. Dotted keys group by their first part.

use std::collections::HashMap;
use std::sync::Arc;

use permark_core::format::config::util::{comment_above, is_number, join_index, join_key, Lines};
use permark_core::format::config::{Group, Row, RowValue, Scalar, ScalarKind, Sheet, SheetKind};
use permark_core::format::{
    Claim, ConfigFormat, ConfigSyntax, Contributions, EditRefusal, FormatId, NewValue,
};
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "fmt-properties",
        PluginKind::Format,
        Contributions::default().formats(vec![Arc::new(ConfigFormat(Properties))]),
    )
}

/// The `.properties` syntax.
#[derive(Debug, Clone, Copy)]
pub struct Properties;

impl ConfigSyntax for Properties {
    fn id(&self) -> FormatId {
        FormatId::PROPERTIES
    }

    fn claims(&self, _name: &str, ext: Option<&str>, _head: &[u8]) -> Claim {
        if ext == Some("properties") {
            Claim::Strong
        } else {
            Claim::No
        }
    }

    fn language(&self) -> &'static str {
        "properties"
    }

    fn parse(&self, text: &str) -> Sheet {
        let lines = Lines::new(text);
        let mut groups: Vec<Group> = Vec::new();
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut n = 0;
        while n < lines.count() {
            let (start, end, _) = lines.bounds(n).unwrap_or((0, 0, 0));
            let line = &text[start..end];
            let lead = line.len() - line.trim_start_matches([' ', '\t', '\u{c}']).len();
            let content = &line[lead..];
            if content.is_empty() || content.starts_with(['#', '!']) {
                n += 1;
                continue;
            }
            let (row, last, group) = entry(text, &lines, n, start + lead, end, &mut seen);
            match groups.iter_mut().find(|g| g.path == group.0) {
                Some(found) => {
                    found.end = found.end.max(row.end);
                    found.rows.push(row);
                }
                None => groups.push(Group {
                    path: group.0,
                    title: group.1,
                    line: row.line,
                    end: row.end,
                    comment: None,
                    rows: vec![row],
                }),
            }
            n = last + 1;
        }
        groups.sort_by_key(|g| g.line);
        Sheet {
            kind: SheetKind::Config,
            groups,
            failed: false,
        }
    }

    fn token(&self, old: &Scalar, new: &NewValue) -> Result<String, EditRefusal> {
        match (old.kind, new) {
            (ScalarKind::String | ScalarKind::Null, NewValue::Text { text }) => {
                if text.contains(['\n', '\r']) {
                    Err(EditRefusal::Invalid("newline"))
                } else if text.ends_with('\\') {
                    Err(EditRefusal::Invalid("continuation"))
                } else {
                    Ok(text.clone())
                }
            }
            (ScalarKind::Bool, NewValue::Literal { text }) if text == "true" || text == "false" => {
                Ok(text.clone())
            }
            (ScalarKind::Number, NewValue::Literal { text }) if is_number(text) => Ok(text.clone()),
            _ => Err(EditRefusal::Invalid("kind")),
        }
    }
}

/// Splits a key at its dots (an escaped dot stays in the part).
fn key_parts(key: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = key.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let (Some(next), Some(part)) = (chars.next(), parts.last_mut()) {
                    part.push(next);
                }
            }
            '.' => parts.push(String::new()),
            _ => {
                if let Some(part) = parts.last_mut() {
                    part.push(c);
                }
            }
        }
    }
    parts.retain(|p| !p.is_empty());
    parts
}

/// One entry starting at `at` on line `n`. Returns the row, its last line and its group `(path, title)`.
fn entry(
    text: &str,
    lines: &Lines<'_>,
    n: usize,
    at: usize,
    line_end: usize,
    seen: &mut HashMap<String, usize>,
) -> (Row, usize, (String, String)) {
    let content = &text[at..line_end];
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'=' | b':' | b' ' | b'\t' | 0x0c => break,
            _ => i += 1,
        }
    }
    let i = i.min(content.len());
    let key = &content[..i];
    let mut j = i;
    while content[j..].starts_with([' ', '\t', '\u{c}']) {
        j += 1;
    }
    if content[j..].starts_with(['=', ':']) {
        j += 1;
        while content[j..].starts_with([' ', '\t', '\u{c}']) {
            j += 1;
        }
    }
    let value_start = at + j;
    let mut span = value_start..line_end;
    let mut last = n;
    let mut read_only = None;
    // an odd number of trailing backslashes continues the value on the next line
    let continues = |s: &str| (s.len() - s.trim_end_matches('\\').len()) & 1 == 1;
    if continues(&text[span.clone()]) {
        read_only = Some("continuation");
        while last + 1 < lines.count() {
            last += 1;
            let (_, e, _) = lines.bounds(last).unwrap_or((0, 0, 0));
            span.end = e;
            if !continues(&text[..e]) {
                break;
            }
        }
    }
    let raw = &text[span.clone()];
    let kind = if raw.is_empty() {
        ScalarKind::Null
    } else if raw == "true" || raw == "false" {
        ScalarKind::Bool
    } else if is_number(raw) {
        ScalarKind::Number
    } else {
        ScalarKind::String
    };
    let parts = key_parts(key);
    let base = parts.iter().fold(String::new(), |p, k| join_key(&p, k));
    let count = seen.entry(base.clone()).or_insert(0);
    let path = if *count == 0 {
        base
    } else {
        join_index(&base, *count)
    };
    *count += 1;
    let (group, label) = if parts.len() > 1 {
        let first = parts[0].clone();
        (
            join_key("", &first),
            key[key.find('.').map_or(0, |d| d + 1)..].to_owned(),
        )
    } else {
        (String::new(), key.to_owned())
    };
    let title = if group.is_empty() {
        String::new()
    } else {
        parts[0].clone()
    };
    let mut scalar = Scalar::new(path.clone(), kind, raw.to_owned(), span);
    if let Some(reason) = read_only {
        scalar = scalar.read_only(reason);
    }
    (
        Row {
            path,
            key: label,
            line: u32::try_from(n + 1).unwrap_or(u32::MAX),
            end: u32::try_from(last + 1).unwrap_or(u32::MAX),
            value: RowValue::Scalar(scalar),
            comment: comment_above(lines, n, &['#', '!']),
        },
        last,
        (group, title),
    )
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
        assert_eq!(manifest.id.as_str(), "fmt-properties");
        assert_eq!(manifest.kind, PluginKind::Format);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }
}
