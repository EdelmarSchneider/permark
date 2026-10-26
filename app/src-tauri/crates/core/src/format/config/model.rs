//! The structure of a small configuration file: groups (cards) of rows, each row a key and a value, with the lines it
//! sits on and the bytes of every value token. Parsers fill it; the sheet view, the outline, search keys and surgical
//! edits read it.

use std::ops::Range;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::util::split_path;
use crate::format::Editability;

/// Bytes of a text.
pub type Span = Range<usize>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ScalarKind {
    String,
    Number,
    Bool,
    Date,
    Null,
}

/// How a string is written (kept when it is edited).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quote {
    #[default]
    Plain,
    Single,
    Double,
}

/// One value: a row's own, or one item of a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Scalar {
    pub path: String,
    pub kind: ScalarKind,
    /// A string without its quotes; any other kind as written.
    pub text: String,
    pub edit: Editability,
    /// The value token in the text (quotes included).
    #[serde(skip)]
    #[ts(skip)]
    pub span: Option<Span>,
    #[serde(skip)]
    #[ts(skip)]
    pub quote: Quote,
}

impl Scalar {
    #[must_use]
    pub fn new(path: String, kind: ScalarKind, text: String, span: Span) -> Self {
        Self {
            path,
            kind,
            text,
            edit: Editability::Surgical,
            span: Some(span),
            quote: Quote::Plain,
        }
    }

    #[must_use]
    pub fn quoted(mut self, quote: Quote) -> Self {
        self.quote = quote;
        self
    }

    /// Cannot be edited, with the reason (a word the UI knows).
    #[must_use]
    pub fn read_only(mut self, reason: &str) -> Self {
        self.edit = Editability::ReadOnly {
            reason: reason.to_owned(),
        };
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum RowValue {
    Scalar(Scalar),
    List {
        items: Vec<Scalar>,
    },
    /// A value shown as written and not editable here (a map in flow style, an inline table, a block scalar).
    Raw {
        text: String,
        reason: String,
        #[serde(skip)]
        #[ts(skip)]
        span: Option<Span>,
    },
    /// A line of an ignore file (`text` as written, with its `!`).
    Pattern {
        path: String,
        text: String,
        negated: bool,
    },
    /// An array of uniform objects (TOON tabular | keyed form): a header of fields and one row per element.
    Table(Table),
}

/// Rows of uniform objects: the fields once, then one line per element (TOON `key[N]{a,b}:` | keyed `key[N:]{a,b}:`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Table {
    pub path: String,
    /// Leaf fields in header order (a nested group's leaves as `group.leaf`).
    pub fields: Vec<String>,
    /// The declared count (`[N]`).
    pub count: u32,
    /// Rows carry their own key (an object of uniform objects).
    pub keyed: bool,
    /// The active delimiter (`,` | tab | `|`).
    pub delimiter: String,
    pub rows: Vec<TableRow>,
    /// Whether rows can be added, removed, duplicated and moved.
    pub edit: Editability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TableRow {
    /// `table[i]` | `table.key` (keyed).
    pub path: String,
    pub line: u32,
    /// The entry key of a keyed row.
    pub key: Option<String>,
    pub cells: Vec<Scalar>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Row {
    pub path: String,
    /// The key as the person reads it (`name`, `a.b`).
    pub key: String,
    pub line: u32,
    /// Last line of the row (a value may span lines).
    pub end: u32,
    pub value: RowValue,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Group {
    /// Path of the table | mapping (`""` for the rows before any header).
    pub path: String,
    pub title: String,
    pub line: u32,
    pub end: u32,
    pub comment: Option<String>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SheetKind {
    Config,
    Ignore,
    /// Structured data (TOON): keys can be renamed, added and removed; tables take rows.
    Data,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Sheet {
    pub kind: SheetKind,
    pub groups: Vec<Group>,
    /// The version of the format's specification the sheet was read with (`TOON 4.4`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub spec: Option<String>,
    /// The text is not valid for its syntax (the sheet is empty; Source shows the text).
    #[serde(skip)]
    #[ts(skip)]
    pub failed: bool,
}

/// A top-level part of a file (agents read it by `path`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub path: String,
    pub label: String,
    pub line: u32,
    pub end: u32,
}

impl Sheet {
    #[must_use]
    pub fn new(kind: SheetKind) -> Self {
        Self {
            kind,
            groups: Vec::new(),
            spec: None,
            failed: false,
        }
    }

    #[must_use]
    pub fn failed(kind: SheetKind) -> Self {
        Self {
            failed: true,
            ..Self::new(kind)
        }
    }

    /// Rows, a table counting one for itself, one per row and one per cell.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows()
            .map(|(_, r)| match &r.value {
                RowValue::Table(t) => {
                    1 + t.rows.iter().map(|row| 1 + row.cells.len()).sum::<usize>()
                }
                _ => 1,
            })
            .sum()
    }

    pub fn rows(&self) -> impl Iterator<Item = (&Group, &Row)> {
        self.groups
            .iter()
            .flat_map(|g| g.rows.iter().map(move |r| (g, r)))
    }

    /// The scalar at `path` (a row's own value or a list item).
    #[must_use]
    pub fn scalar(&self, path: &str) -> Option<&Scalar> {
        self.rows().find_map(|(_, row)| match &row.value {
            RowValue::Scalar(s) if s.path == path => Some(s),
            RowValue::List { items } => items.iter().find(|s| s.path == path),
            RowValue::Table(t) if path.starts_with(t.path.as_str()) => t
                .rows
                .iter()
                .flat_map(|r| r.cells.iter())
                .find(|s| s.path == path),
            _ => None,
        })
    }

    /// The table at `path`.
    #[must_use]
    pub fn table(&self, path: &str) -> Option<&Table> {
        self.rows().find_map(|(_, row)| match &row.value {
            RowValue::Table(t) if t.path == path => Some(t),
            _ => None,
        })
    }

    /// The row at `path`.
    #[must_use]
    pub fn row(&self, path: &str) -> Option<&Row> {
        self.rows().map(|(_, r)| r).find(|r| r.path == path)
    }

    /// Top-level parts: the first path segment of every group and row, in file order, with their line range.
    #[must_use]
    pub fn outline(&self) -> Vec<Part> {
        if self.kind == SheetKind::Ignore {
            // the commented blocks of an ignore file
            return self
                .groups
                .iter()
                .filter(|g| !g.title.is_empty())
                .map(|g| Part {
                    path: g.path.clone(),
                    label: g.title.clone(),
                    line: g.line,
                    end: g.end,
                })
                .collect();
        }
        let mut parts: Vec<Part> = Vec::new();
        let mut add = |path: &str, line: u32, end: u32, table: Option<&Table>| {
            let Some(first) = split_path(path).into_iter().next() else {
                return;
            };
            let key = super::util::quote_key(&first);
            match parts.iter_mut().find(|p| p.path == key) {
                Some(part) => {
                    part.line = part.line.min(line);
                    part.end = part.end.max(end);
                }
                None => parts.push(Part {
                    // a top-level table says its shape: `users[3]{id,name}`
                    label: match table {
                        Some(t) if t.path == key => {
                            format!("{first}[{}]{{{}}}", t.rows.len(), t.fields.join(","))
                        }
                        _ => first,
                    },
                    path: key,
                    line,
                    end,
                }),
            }
        };
        for group in &self.groups {
            if !group.path.is_empty() {
                let table = group.rows.iter().find_map(|r| match &r.value {
                    RowValue::Table(t) if t.path == group.path => Some(t),
                    _ => None,
                });
                add(&group.path, group.line, group.end, table);
            }
            for row in &group.rows {
                add(&row.path, row.line, row.end, None);
            }
        }
        parts
    }

    /// The lines of a group (with the groups below it) or of a row: `(first, last)`.
    #[must_use]
    pub fn range_of(&self, path: &str) -> Option<(u32, u32)> {
        let mut range: Option<(u32, u32)> = None;
        let mut grow = |line: u32, end: u32| {
            range = Some(range.map_or((line, end), |(a, b)| (a.min(line), b.max(end))));
        };
        let below = |candidate: &str| {
            candidate == path
                || candidate
                    .strip_prefix(path)
                    .is_some_and(|rest| rest.starts_with(['.', '[']))
        };
        for group in &self.groups {
            if !group.path.is_empty() && below(&group.path) {
                grow(group.line, group.end);
            }
            for row in &group.rows {
                if below(&row.path) {
                    grow(row.line, row.end);
                } else if let RowValue::Table(table) = &row.value {
                    for r in table.rows.iter().filter(|r| below(&r.path)) {
                        grow(r.line, r.line);
                    }
                }
            }
        }
        range
    }

    /// What search may read: the keys (group titles and row keys, once each), never a value or a comment.
    #[must_use]
    pub fn keys_text(&self, limit: usize) -> String {
        let mut seen = std::collections::HashSet::new();
        let mut out = String::new();
        let mut push = |word: &str| {
            if out.len() < limit && !word.is_empty() && seen.insert(word.to_owned()) {
                out.push_str(word);
                out.push('\n');
            }
        };
        for group in &self.groups {
            if let Some(last) = split_path(&group.path).last() {
                push(last);
            }
            for row in &group.rows {
                if self.kind != SheetKind::Ignore {
                    push(&row.key);
                }
                if let RowValue::Table(table) = &row.value {
                    for field in &table.fields {
                        push(field);
                    }
                }
            }
        }
        out
    }

    /// Patterns of an ignore sheet, one per line.
    #[must_use]
    pub fn patterns_text(&self, limit: usize) -> String {
        let mut out = String::new();
        for (_, row) in self.rows() {
            if let RowValue::Pattern { text, .. } = &row.value {
                if out.len() + text.len() < limit {
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
        out
    }
}

/// What a new value is.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum NewValue {
    /// A string (quoted as the file needs).
    Text { text: String },
    /// A number, date or bool as typed.
    Literal { text: String },
}

impl NewValue {
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Text { text } | Self::Literal { text } => text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase")]
#[ts(export)]
pub enum ConfigEdit {
    Set {
        path: String,
        value: NewValue,
    },
    /// Ignore files: delete the line.
    Remove {
        path: String,
    },
    /// Ignore files: a new line after the row `after` (`None`: at the end).
    Insert {
        after: Option<String>,
        text: String,
    },
    /// Data sheets: the key at `path` takes a new name (an object key, an array's key, an entry key).
    Rename {
        path: String,
        key: String,
    },
    /// Data sheets: one leaf field name of a table's header.
    RenameField {
        table: String,
        index: u32,
        name: String,
    },
    /// Data sheets: `key: value` at the end of the object at `parent` (`""`: the root).
    AddKey {
        parent: String,
        key: String,
        value: NewValue,
    },
    /// Tables: a new row at `at` (0 = first; the row count = after the last), its cells empty and typed per column.
    AddRow {
        table: String,
        at: u32,
    },
    /// Tables: the row at `path` goes.
    RemoveRow {
        path: String,
    },
    /// Tables: a copy of the row at `path` right below it.
    DuplicateRow {
        path: String,
    },
    /// Tables: the row at `path` moves to index `to` (among the rows as they are before the move).
    MoveRow {
        path: String,
        to: u32,
    },
}

/// Replace `start..end` of the text by `insert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    pub start: usize,
    pub end: usize,
    pub insert: String,
}

impl Splice {
    /// `text` with the splice applied.
    #[must_use]
    pub fn apply(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + self.insert.len());
        out.push_str(&text[..self.start]);
        out.push_str(&self.insert);
        out.push_str(&text[self.end..]);
        out
    }
}

/// Why an edit was not made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditRefusal {
    #[error("no such value")]
    NotFound,
    #[error("read only: {0}")]
    ReadOnly(String),
    #[error("invalid: {0}")]
    Invalid(&'static str),
    #[error("not supported by this format")]
    Unsupported,
}

impl EditRefusal {
    /// The word the UI shows.
    #[must_use]
    pub fn word(&self) -> String {
        match self {
            Self::NotFound => "notFound".to_owned(),
            Self::ReadOnly(reason) => reason.clone(),
            Self::Invalid(why) => (*why).to_owned(),
            Self::Unsupported => "unsupported".to_owned(),
        }
    }
}
