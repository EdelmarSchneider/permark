//! Plugin `fmt-toml`: TOML files as a sheet; a value edit replaces one value token (spans from `toml_edit`).

use std::sync::Arc;

use permark_core::format::config::util::{
    caption, comment_above, escape_double, join_index, join_key, trailing_comment, Lines,
};
use permark_core::format::config::{
    Group, Quote, Row, RowValue, Scalar, ScalarKind, Sheet, SheetKind,
};
use permark_core::format::{
    Claim, ConfigFormat, ConfigSyntax, Contributions, EditRefusal, FormatId, NewValue,
};
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};
use toml_edit::{Document, Item, Table, Value};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "fmt-toml",
        PluginKind::Format,
        Contributions::default().formats(vec![Arc::new(ConfigFormat(Toml))]),
    )
}

/// The TOML syntax.
#[derive(Debug, Clone, Copy)]
pub struct Toml;

impl ConfigSyntax for Toml {
    fn id(&self) -> FormatId {
        FormatId::TOML
    }

    fn claims(&self, _name: &str, ext: Option<&str>, _head: &[u8]) -> Claim {
        if ext == Some("toml") {
            Claim::Strong
        } else {
            Claim::No
        }
    }

    fn language(&self) -> &'static str {
        "toml"
    }

    fn parse(&self, text: &str) -> Sheet {
        let Ok(doc) = Document::parse(text) else {
            return Sheet::failed(SheetKind::Config);
        };
        let lines = Lines::new(text);
        let mut walk = Walk {
            text,
            lines,
            groups: Vec::new(),
        };
        walk.table(doc.as_table(), "", "", 0);
        let mut groups = walk.groups;
        groups.sort_by_key(|g| g.line);
        Sheet {
            kind: SheetKind::Config,
            groups,
            spec: None,
            failed: false,
        }
    }

    fn token(&self, old: &Scalar, new: &NewValue) -> Result<String, EditRefusal> {
        match (old.kind, new) {
            (ScalarKind::String, NewValue::Text { text }) => Ok(string_token(old.quote, text)),
            (ScalarKind::String, NewValue::Literal { .. }) => {
                Err(EditRefusal::Invalid("expectedText"))
            }
            (kind, NewValue::Literal { text }) => literal_token(kind, text),
            (_, NewValue::Text { .. }) => Err(EditRefusal::Invalid("expectedLiteral")),
        }
    }
}

/// A string as the file writes it: the quote kind it had when the text fits, else a basic string.
fn string_token(quote: Quote, text: &str) -> String {
    let literal_ok = !text.contains('\'') && !text.chars().any(|c| (c as u32) < 0x20 && c != '\t');
    if quote == Quote::Single && literal_ok {
        format!("'{text}'")
    } else {
        format!("\"{}\"", escape_double(text))
    }
}

/// A number | date | bool as typed, when TOML reads it as that kind.
fn literal_token(kind: ScalarKind, text: &str) -> Result<String, EditRefusal> {
    if text.is_empty() || text.trim() != text || text.contains(['#', '\n', '\r', ' ']) {
        return Err(EditRefusal::Invalid("literal"));
    }
    let doc =
        Document::parse(format!("v = {text}")).map_err(|_| EditRefusal::Invalid("literal"))?;
    let ok = matches!(
        (kind, doc.as_table().get("v")),
        (
            ScalarKind::Number,
            Some(Item::Value(Value::Integer(_) | Value::Float(_)))
        ) | (ScalarKind::Bool, Some(Item::Value(Value::Boolean(_))))
            | (ScalarKind::Date, Some(Item::Value(Value::Datetime(_))))
    );
    if ok {
        Ok(text.to_owned())
    } else {
        Err(EditRefusal::Invalid("literal"))
    }
}

struct Walk<'a> {
    text: &'a str,
    lines: Lines<'a>,
    groups: Vec<Group>,
}

impl Walk<'_> {
    fn line(&self, at: usize) -> u32 {
        self.lines.line_of(at)
    }

    /// The last line a span touches.
    fn end_line(&self, end: usize) -> u32 {
        self.line(end.saturating_sub(1))
    }

    fn header_caption(&self, line: u32) -> Option<String> {
        comment_above(&self.lines, (line as usize).saturating_sub(1), &['#'])
    }

    /// A table's rows go to its own group; dotted sub-tables put their rows in the same group.
    fn table(&mut self, table: &Table, path: &str, title: &str, header_at: usize) {
        let line = if path.is_empty() {
            1
        } else {
            self.line(table.span().map_or(header_at, |s| s.start))
        };
        let mut group = Group {
            path: path.to_owned(),
            title: title.to_owned(),
            line,
            end: line,
            comment: if path.is_empty() {
                None
            } else {
                self.header_caption(line)
            },
            rows: Vec::new(),
        };
        let mut subs: Vec<(String, String, &Table, usize)> = Vec::new();
        self.rows_of(table, path, &[], &mut group, &mut subs);
        group.end = group
            .rows
            .iter()
            .map(|r| r.end)
            .max()
            .unwrap_or(line)
            .max(line);
        // a table that only exists because a deeper one is named has nothing to show
        if !group.rows.is_empty() || (!path.is_empty() && !table.is_implicit()) {
            self.groups.push(group);
        }
        for (sub_path, sub_title, sub, at) in subs {
            self.table(sub, &sub_path, &sub_title, at);
        }
    }

    fn rows_of<'t>(
        &self,
        table: &'t Table,
        path: &str,
        prefix: &[String],
        group: &mut Group,
        subs: &mut Vec<(String, String, &'t Table, usize)>,
    ) {
        for (key, item) in table {
            let key_at = table
                .key(key)
                .and_then(toml_edit::Key::span)
                .map_or(0, |s| s.start);
            let mut label = prefix.to_vec();
            label.push(key.to_owned());
            let row_path = label.iter().fold(path.to_owned(), |p, k| join_key(&p, k));
            match item {
                Item::Value(value) => {
                    if let Some(row) = self.row(&label.join("."), &row_path, key_at, value) {
                        group.rows.push(row);
                    }
                }
                Item::Table(sub) if sub.is_dotted() => {
                    self.rows_of(sub, path, &label, group, subs);
                }
                Item::Table(sub) => {
                    let title = row_path.clone();
                    subs.push((row_path, title, sub, key_at));
                }
                Item::ArrayOfTables(array) => {
                    for (i, sub) in array.iter().enumerate() {
                        let item_path = join_index(&row_path, i);
                        subs.push((item_path.clone(), item_path, sub, key_at));
                    }
                }
                Item::None => {}
            }
        }
    }

    fn row(&self, key: &str, path: &str, key_at: usize, value: &Value) -> Option<Row> {
        let span = value.span()?;
        let line = self.line(key_at);
        let end = self.end_line(span.end).max(line);
        let above = comment_above(&self.lines, (line as usize).saturating_sub(1), &['#']);
        let rest_end = self
            .lines
            .bounds(end as usize - 1)
            .map_or(self.text.len(), |(_, e, _)| e);
        let trailing = self
            .text
            .get(span.end..rest_end)
            .and_then(|rest| trailing_comment(rest, &['#']));
        let value = match value {
            Value::Array(array) => {
                let mut items = Vec::new();
                let mut plain = true;
                for (i, v) in array.iter().enumerate() {
                    match self.scalar(&join_index(path, i), v) {
                        Some(s) => items.push(s),
                        None => plain = false,
                    }
                }
                if plain {
                    RowValue::List { items }
                } else {
                    self.raw(span, "nested")
                }
            }
            Value::InlineTable(_) => self.raw(span, "inlineTable"),
            other => RowValue::Scalar(self.scalar(path, other)?),
        };
        Some(Row {
            path: path.to_owned(),
            key: key.to_owned(),
            line,
            end,
            value,
            comment: caption(above, trailing),
        })
    }

    fn raw(&self, span: std::ops::Range<usize>, reason: &str) -> RowValue {
        RowValue::Raw {
            text: self.text.get(span.clone()).unwrap_or_default().to_owned(),
            reason: reason.to_owned(),
            span: Some(span),
        }
    }

    /// A scalar value (None for arrays and inline tables).
    fn scalar(&self, path: &str, value: &Value) -> Option<Scalar> {
        let span = value.span()?;
        let raw = self.text.get(span.clone())?;
        let make = |kind: ScalarKind, text: &str| {
            Scalar::new(path.to_owned(), kind, text.to_owned(), span.clone())
        };
        Some(match value {
            Value::String(s) => {
                let multi = raw.starts_with("\"\"\"") || raw.starts_with("'''");
                let quote = if raw.starts_with('\'') {
                    Quote::Single
                } else {
                    Quote::Double
                };
                let scalar = make(ScalarKind::String, s.value()).quoted(quote);
                if multi {
                    scalar.read_only("multiline")
                } else {
                    scalar
                }
            }
            Value::Integer(_) | Value::Float(_) => make(ScalarKind::Number, raw),
            Value::Boolean(_) => make(ScalarKind::Bool, raw),
            Value::Datetime(_) => make(ScalarKind::Date, raw),
            Value::Array(_) | Value::InlineTable(_) => return None,
        })
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
        assert_eq!(manifest.id.as_str(), "fmt-toml");
        assert_eq!(manifest.kind, PluginKind::Format);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }
}
