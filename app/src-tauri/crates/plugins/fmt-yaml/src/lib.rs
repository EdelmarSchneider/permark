//! Plugin `fmt-yaml`: YAML files (block style) as a sheet; a value edit replaces one scalar token.

mod parse;

use std::collections::HashSet;
use std::sync::Arc;

use permark_core::format::config::util::{
    caption, comment_above, escape_double, join_index, join_key, Lines,
};
use permark_core::format::config::{
    Group, Quote, Row, RowValue, Scalar, ScalarKind, Sheet, SheetKind,
};
use permark_core::format::{
    Claim, ConfigFormat, ConfigSyntax, Contributions, EditRefusal, FormatId, NewValue,
};
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

use parse::{kind_of_plain, Item, Parser, Proto, SeqItem, Val};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "fmt-yaml",
        PluginKind::Format,
        Contributions::default().formats(vec![Arc::new(ConfigFormat(Yaml))]),
    )
}

/// The YAML syntax.
#[derive(Debug, Clone, Copy)]
pub struct Yaml;

impl ConfigSyntax for Yaml {
    fn id(&self) -> FormatId {
        FormatId::YAML
    }

    fn claims(&self, _name: &str, ext: Option<&str>, _head: &[u8]) -> Claim {
        if matches!(ext, Some("yaml" | "yml")) {
            Claim::Strong
        } else {
            Claim::No
        }
    }

    fn language(&self) -> &'static str {
        "yaml"
    }

    fn parse(&self, text: &str) -> Sheet {
        let mut parser = Parser::new(text);
        let parsed = parser.parse();
        let mut emit = Emit {
            text,
            lines: &parser.lines,
            groups: Vec::new(),
            seen: HashSet::new(),
        };
        let mut root_rows: Vec<Row> = Vec::new();
        let mut root_end = 1;
        match &parsed.root {
            Some(Val::Map(items)) => emit.map(items, "", "", 0, None),
            Some(Val::Seq { items, first, last }) => {
                emit.seq(items, *first, *last, "", "", &mut root_rows);
            }
            Some(other) => {
                // a lone scalar at the top of the file
                let _ = other;
            }
            None => {}
        }
        let mut groups = std::mem::take(&mut emit.groups);
        if !root_rows.is_empty() {
            root_end = root_rows.iter().map(|r| r.end).max().unwrap_or(1);
            groups.push(Group {
                path: String::new(),
                title: String::new(),
                line: 1,
                end: root_end,
                comment: None,
                rows: root_rows,
            });
        }
        if let Some(more) = parsed.more_documents {
            if let Some((start, _, _)) = parser.lines.bounds(more) {
                let span = start..text.len();
                let raw = Row {
                    path: "documents".to_owned(),
                    key: "---".to_owned(),
                    line: u32::try_from(more + 1).unwrap_or(u32::MAX),
                    end: u32::try_from(parser.lines.count()).unwrap_or(u32::MAX),
                    value: RowValue::Raw {
                        text: text[span.clone()].trim_end().to_owned(),
                        reason: "multiDoc".to_owned(),
                        secret: false,
                        span: Some(span),
                    },
                    comment: None,
                };
                match groups.iter_mut().find(|g| g.path.is_empty()) {
                    Some(root) => root.rows.push(raw),
                    None => groups.push(Group {
                        path: String::new(),
                        title: String::new(),
                        line: 1,
                        end: root_end,
                        comment: None,
                        rows: vec![raw],
                    }),
                }
            }
        }
        groups.retain(|g| !g.rows.is_empty());
        groups.sort_by_key(|g| (g.line, g.path.len()));
        if groups.is_empty() && parsed.significant {
            return Sheet::failed(SheetKind::Config);
        }
        Sheet {
            kind: SheetKind::Config,
            groups,
            failed: false,
        }
    }

    fn token(&self, old: &Scalar, new: &NewValue) -> Result<String, EditRefusal> {
        let token = match (old.kind, new) {
            (ScalarKind::String, NewValue::Text { text }) => string_token(old.quote, text),
            // an empty value takes a string
            (ScalarKind::Null, NewValue::Text { text }) => string_token(Quote::Plain, text),
            (ScalarKind::String | ScalarKind::Null, NewValue::Literal { .. }) => {
                return Err(EditRefusal::Invalid("expectedText"));
            }
            (kind, NewValue::Literal { text }) => literal_token(kind, text)?,
            (_, NewValue::Text { .. }) => return Err(EditRefusal::Invalid("expectedLiteral")),
        };
        // `key:` with nothing after the colon: the value needs a space of its own
        if old.text.is_empty() && old.kind == ScalarKind::Null {
            Ok(format!(" {token}"))
        } else {
            Ok(token)
        }
    }
}

/// A number | date | bool as typed, when YAML reads it as that kind.
fn literal_token(kind: ScalarKind, text: &str) -> Result<String, EditRefusal> {
    let ok = !text.is_empty()
        && text.trim() == text
        && !text.contains(['#', '\n', '\r', ' ', ':'])
        && kind_of_plain(text) == kind;
    if ok {
        Ok(text.to_owned())
    } else {
        Err(EditRefusal::Invalid("literal"))
    }
}

/// Whether `text` can be written without quotes and still read as the same string.
fn plain_ok(text: &str) -> bool {
    let first = text.chars().next();
    let Some(first) = first else {
        return false;
    };
    !(text.trim() != text
        || "-?:,[]{}#&*!|>'\"%@`".contains(first)
        || text.contains(": ")
        || text.contains(" #")
        || text.ends_with(':')
        || text.chars().any(char::is_control)
        || kind_of_plain(text) != ScalarKind::String
        || matches!(
            text.to_ascii_lowercase().as_str(),
            "yes" | "no" | "on" | "off" | "y" | "n"
        ))
}

/// A string as the file writes it: the quote kind it had when the text fits, else a safer one.
fn string_token(quote: Quote, text: &str) -> String {
    let single_ok = !text.chars().any(char::is_control);
    match quote {
        Quote::Plain if plain_ok(text) => text.to_owned(),
        Quote::Single if single_ok => format!("'{}'", text.replace('\'', "''")),
        _ => format!("\"{}\"", escape_double(text)),
    }
}

struct Emit<'a> {
    text: &'a str,
    lines: &'a Lines<'a>,
    groups: Vec<Group>,
    seen: HashSet<String>,
}

fn line_no(line: usize) -> u32 {
    u32::try_from(line + 1).unwrap_or(u32::MAX)
}

impl Emit<'_> {
    fn scalar(&mut self, proto: &Proto, path: String) -> Scalar {
        let duplicate = !self.seen.insert(path.clone());
        let mut scalar = Scalar::new(path, proto.kind, proto.text.clone(), proto.span.clone())
            .quoted(proto.quote);
        if let Some(reason) = proto.read_only {
            scalar = scalar.read_only(reason);
        } else if duplicate {
            scalar = scalar.read_only("duplicate");
        }
        scalar
    }

    /// A mapping: one group of its own rows; mappings below it become groups of their own.
    fn map(
        &mut self,
        items: &[Item],
        path: &str,
        title: &str,
        line: usize,
        comment: Option<String>,
    ) {
        let mut rows: Vec<Row> = Vec::new();
        let mut subs: Vec<(String, &Item)> = Vec::new();
        for item in items {
            let row_path = join_key(path, &item.key);
            match &item.val {
                Val::Scalar(proto) => {
                    let scalar = self.scalar(proto, row_path.clone());
                    rows.push(Row {
                        path: row_path,
                        key: item.key.clone(),
                        line: line_no(item.line),
                        end: line_no(item.end),
                        value: RowValue::Scalar(scalar),
                        comment: item.comment.clone(),
                    });
                }
                Val::Raw { text, reason, span } => rows.push(Row {
                    path: row_path,
                    key: item.key.clone(),
                    line: line_no(item.line),
                    end: line_no(item.end),
                    value: RowValue::Raw {
                        text: text.clone(),
                        reason: (*reason).to_owned(),
                        secret: false,
                        span: Some(span.clone()),
                    },
                    comment: item.comment.clone(),
                }),
                Val::Seq {
                    items: seq,
                    first,
                    last,
                } => {
                    let before = rows.len();
                    self.seq(seq, *first, *last, &row_path, &item.key, &mut rows);
                    if let Some(row) = rows.get_mut(before) {
                        row.line = line_no(item.line);
                        row.comment.clone_from(&item.comment);
                    }
                }
                Val::Map(_) => subs.push((row_path, item)),
            }
        }
        let end = rows
            .iter()
            .map(|r| r.end)
            .max()
            .unwrap_or(line_no(line))
            .max(line_no(line));
        self.groups.push(Group {
            path: path.to_owned(),
            title: title.to_owned(),
            line: line_no(line),
            end,
            comment,
            rows,
        });
        for (sub_path, item) in subs {
            if let Val::Map(children) = &item.val {
                self.map(
                    children,
                    &sub_path,
                    &sub_path,
                    item.line,
                    item.comment.clone(),
                );
            }
        }
    }

    /// A sequence under `path`: a list row when every item is a scalar, a group per item when every item is a
    /// mapping, else shown as written.
    fn seq(
        &mut self,
        items: &[SeqItem],
        first: usize,
        last: usize,
        path: &str,
        key: &str,
        rows: &mut Vec<Row>,
    ) {
        let all_scalars = items.iter().all(|i| matches!(i.val, Val::Scalar(_)));
        let all_maps = !items.is_empty() && items.iter().all(|i| matches!(i.val, Val::Map(_)));
        if all_scalars && !items.is_empty() {
            let scalars: Vec<Scalar> = items
                .iter()
                .enumerate()
                .filter_map(|(n, item)| match &item.val {
                    Val::Scalar(proto) => Some(self.scalar(proto, join_index(path, n))),
                    _ => None,
                })
                .collect();
            rows.push(Row {
                path: path.to_owned(),
                key: key.to_owned(),
                line: line_no(first),
                end: line_no(last),
                value: RowValue::List { items: scalars },
                comment: None,
            });
        } else if all_maps {
            for (n, item) in items.iter().enumerate() {
                if let Val::Map(children) = &item.val {
                    let item_path = join_index(path, n);
                    let above = comment_above(self.lines, item.line, &['#']);
                    self.map(children, &item_path, &item_path, item.line, above);
                }
            }
        } else if let Some(span) = self.region(first, last) {
            rows.push(Row {
                path: path.to_owned(),
                key: key.to_owned(),
                line: line_no(first),
                end: line_no(last),
                value: RowValue::Raw {
                    text: self.text[span.clone()].to_owned(),
                    reason: "mixed".to_owned(),
                    secret: false,
                    span: Some(span),
                },
                comment: caption(None, None),
            });
        }
    }

    fn region(&self, first: usize, last: usize) -> Option<std::ops::Range<usize>> {
        let (start, _, _) = self.lines.bounds(first)?;
        let (_, end, _) = self.lines.bounds(last)?;
        (start <= end).then_some(start..end)
    }
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
        assert_eq!(manifest.id.as_str(), "fmt-yaml");
        assert_eq!(manifest.kind, PluginKind::Format);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }
}
