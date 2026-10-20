//! In-place edits of a TOON text: each one is a splice of the bytes it changes (a value token, a key token, one
//! line, the digits of a `[N]`), never a new encoding of the document. After the splice the text must still decode
//! by the pinned specification and read back as intended, else nothing changes.

use std::fmt::Write as _;

use permark_core::format::config::util::{join_key, majority_break, Lines};
use permark_core::format::config::{
    ConfigEdit, EditRefusal, NewValue, Quote, RowValue, ScalarKind, Sheet, Splice, Table,
};

use crate::codec::decode;
use crate::scan::{is_json_number, scan, Scan, TableSite};

/// The splice that makes `edit` in `text`.
///
/// # Errors
/// The text is not valid TOON, the part is missing, or the edit cannot be made exactly.
pub fn edit(text: &str, edit: &ConfigEdit) -> Result<Splice, EditRefusal> {
    decode(text).map_err(|_| EditRefusal::Invalid("syntax"))?;
    let scanned = scan(text);
    let lines = Lines::new(text);
    let cx = Cx {
        text,
        scan: &scanned,
        lines: &lines,
    };
    let (splice, expect) = match edit {
        ConfigEdit::Set { path, value } => cx.set(path, value)?,
        ConfigEdit::Rename { path, key } => cx.rename(path, key)?,
        ConfigEdit::RenameField { table, index, name } => cx.rename_field(table, *index, name)?,
        ConfigEdit::AddKey { parent, key, value } => cx.add_key(parent, key, value)?,
        ConfigEdit::Remove { path } => cx.remove(path)?,
        ConfigEdit::AddRow { table, at } => cx.add_row(table, *at)?,
        ConfigEdit::RemoveRow { path } => cx.remove_row(path)?,
        ConfigEdit::DuplicateRow { path } => cx.duplicate_row(path)?,
        ConfigEdit::MoveRow { path, to } => cx.move_row(path, *to)?,
        ConfigEdit::Insert { .. } => return Err(EditRefusal::Unsupported),
    };
    // the safety net: the new text decodes and says what was meant
    let changed = splice.apply(text);
    decode(&changed).map_err(|_| EditRefusal::Invalid("roundTrip"))?;
    if !expect.holds(&scan(&changed)) {
        return Err(EditRefusal::Invalid("roundTrip"));
    }
    Ok(splice)
}

/// What the new text must show.
enum Expect {
    Value {
        path: String,
        text: String,
    },
    Key(String),
    NoKey(String),
    Field {
        table: String,
        index: usize,
        name: String,
    },
    Rows {
        table: String,
        count: usize,
    },
}

impl Expect {
    fn holds(&self, after: &Scan) -> bool {
        match self {
            Self::Value { path, text } => {
                after.sheet.scalar(path).is_some_and(|s| s.text == *text)
                    || after.sheet.row(path).is_some()
            }
            Self::Key(path) => {
                after.sites.keys.contains_key(path) || row_of(&after.sheet, path).is_some()
            }
            Self::NoKey(path) => !after.sites.keys.contains_key(path),
            Self::Field { table, index, name } => after
                .sites
                .tables
                .get(table)
                .and_then(|t| t.fields.get(*index))
                .and_then(|leaf| leaf.segs.last())
                .is_some_and(|last| last == name),
            Self::Rows { table, count } => after
                .sheet
                .table(table)
                .is_some_and(|t| t.rows.len() == *count && t.count as usize == *count),
        }
    }
}

struct Cx<'a> {
    text: &'a str,
    scan: &'a Scan,
    lines: &'a Lines<'a>,
}

type Made = Result<(Splice, Expect), EditRefusal>;

impl Cx<'_> {
    fn set(&self, path: &str, value: &NewValue) -> Made {
        let scalar = self.scan.sheet.scalar(path).ok_or(EditRefusal::NotFound)?;
        let span = scalar.span.clone().ok_or(EditRefusal::NotFound)?;
        let delim = self.scan.sites.delims.get(path).copied();
        let root = self.scan.sites.root_primitive && path.is_empty();
        let token = match (scalar.kind, value) {
            (ScalarKind::String | ScalarKind::Null, NewValue::Text { text }) => {
                string_token(text, scalar.quote, delim, root)
            }
            (ScalarKind::Number | ScalarKind::Bool, NewValue::Literal { text }) => {
                literal_token(scalar.kind, text)?
            }
            (ScalarKind::String | ScalarKind::Null, NewValue::Literal { .. }) => {
                return Err(EditRefusal::Invalid("expectedText"))
            }
            _ => return Err(EditRefusal::Invalid("expectedLiteral")),
        };
        Ok((
            Splice {
                start: span.start,
                end: span.end,
                insert: token,
            },
            Expect::Value {
                path: path.to_owned(),
                text: value.text().to_owned(),
            },
        ))
    }

    fn rename(&self, path: &str, key: &str) -> Made {
        if key.is_empty() {
            return Err(EditRefusal::Invalid("empty"));
        }
        let (span, parent) = if let Some(site) = self.scan.sites.keys.get(path) {
            (site.span.clone(), site.parent.clone())
        } else {
            let (table, i) = row_of(&self.scan.sheet, path).ok_or(EditRefusal::NotFound)?;
            let site = self.table_site(&table.path)?;
            let span = site.rows[i]
                .key
                .as_ref()
                .map(|(_, s)| s.clone())
                .ok_or(EditRefusal::NotFound)?;
            (span, table.path.clone())
        };
        let new_path = join_key(&parent, key);
        if new_path != path
            && (self.scan.sites.keys.contains_key(&new_path)
                || row_of(&self.scan.sheet, &new_path).is_some())
        {
            return Err(EditRefusal::Invalid("duplicate"));
        }
        Ok((
            Splice {
                start: span.start,
                end: span.end,
                insert: key_token(key),
            },
            Expect::Key(new_path),
        ))
    }

    fn rename_field(&self, table: &str, index: u32, name: &str) -> Made {
        if name.is_empty() {
            return Err(EditRefusal::Invalid("empty"));
        }
        let site = self.table_site(table)?;
        let index = index as usize;
        let leaf = site.fields.get(index).ok_or(EditRefusal::NotFound)?;
        let parent = &leaf.segs[..leaf.segs.len().saturating_sub(1)];
        // a name at the same level of the field list (a leaf or a group) cannot repeat
        let taken = site.fields.iter().enumerate().any(|(i, other)| {
            i != index
                && other.segs.len() > parent.len()
                && other.segs[..parent.len()] == *parent
                && other.segs[parent.len()] == name
        });
        if taken {
            return Err(EditRefusal::Invalid("duplicate"));
        }
        Ok((
            Splice {
                start: leaf.span.start,
                end: leaf.span.end,
                insert: key_token(name),
            },
            Expect::Field {
                table: table.to_owned(),
                index,
                name: name.to_owned(),
            },
        ))
    }

    fn add_key(&self, parent: &str, key: &str, value: &NewValue) -> Made {
        if key.is_empty() {
            return Err(EditRefusal::Invalid("empty"));
        }
        let object = self
            .scan
            .sites
            .objects
            .get(parent)
            .ok_or(EditRefusal::NotFound)?;
        let path = join_key(parent, key);
        if self.scan.sites.keys.contains_key(&path) {
            return Err(EditRefusal::Invalid("duplicate"));
        }
        let token = match value {
            NewValue::Text { text } => string_token(text, Quote::Plain, None, false),
            NewValue::Literal { text } => {
                let kind = match text.as_str() {
                    "true" | "false" => ScalarKind::Bool,
                    "null" => ScalarKind::Null,
                    _ => ScalarKind::Number,
                };
                literal_token(kind, text)?
            }
        };
        let line = format!("{}{}: {token}", " ".repeat(object.indent), key_token(key));
        let eol = majority_break(self.text);
        let splice = match object.last {
            Some(last) => self.after_line(last, &line, eol)?,
            None if self.text.is_empty() => Splice {
                start: 0,
                end: 0,
                insert: line,
            },
            None => {
                let end = self.text.len();
                let lead = if self.text.ends_with('\n') { "" } else { eol };
                Splice {
                    start: end,
                    end,
                    insert: format!("{lead}{line}"),
                }
            }
        };
        Ok((
            splice,
            Expect::Value {
                path,
                text: value.text().to_owned(),
            },
        ))
    }

    fn remove(&self, path: &str) -> Made {
        let site = self
            .scan
            .sites
            .keys
            .get(path)
            .ok_or(EditRefusal::NotFound)?;
        if site.hyphen {
            return Err(EditRefusal::ReadOnly("firstField".to_owned()));
        }
        if !site.one_line {
            return Err(EditRefusal::ReadOnly("multiline".to_owned()));
        }
        Ok((self.remove_line(site.line)?, Expect::NoKey(path.to_owned())))
    }

    fn add_row(&self, table: &str, at: u32) -> Made {
        let t = self.scan.sheet.table(table).ok_or(EditRefusal::NotFound)?;
        let site = self.table_site(table)?;
        let at = at as usize;
        if at > site.rows.len() {
            return Err(EditRefusal::NotFound);
        }
        let kinds = site.rows.first().map_or_else(
            || vec![ScalarKind::String; site.fields.len()],
            |r| r.kinds.clone(),
        );
        let cells: Vec<&str> = kinds.iter().map(|k| empty_cell(*k)).collect();
        let key = site.keyed.then(|| unique_key(t, "new"));
        let line = row_line(site, key.as_deref(), &cells.join(&site.delim.to_string()));
        let after = if at == 0 {
            site.header
        } else {
            site.rows[at - 1].line
        };
        let splice = self.with_count(
            site,
            site.rows.len() + 1,
            &self.after_line(after, &line, majority_break(self.text))?,
        )?;
        Ok((
            splice,
            Expect::Rows {
                table: table.to_owned(),
                count: site.rows.len() + 1,
            },
        ))
    }

    fn remove_row(&self, path: &str) -> Made {
        let (table, i) = row_of(&self.scan.sheet, path).ok_or(EditRefusal::NotFound)?;
        let site = self.table_site(&table.path)?;
        let splice = self.with_count(
            site,
            site.rows.len() - 1,
            &self.remove_line(site.rows[i].line)?,
        )?;
        Ok((
            splice,
            Expect::Rows {
                table: table.path.clone(),
                count: site.rows.len() - 1,
            },
        ))
    }

    fn duplicate_row(&self, path: &str) -> Made {
        let (table, i) = row_of(&self.scan.sheet, path).ok_or(EditRefusal::NotFound)?;
        let site = self.table_site(&table.path)?;
        let row = &site.rows[i];
        let (start, end, _) = self.lines.bounds(row.line).ok_or(EditRefusal::NotFound)?;
        let mut copy = self.text[start..end].to_owned();
        if let Some((key, span)) = &row.key {
            // a keyed row needs a key of its own
            let fresh = key_token(&unique_key(table, key));
            copy.replace_range(span.start - start..span.end - start, &fresh);
        }
        let splice = self.with_count(
            site,
            site.rows.len() + 1,
            &self.after_line(row.line, &copy, majority_break(self.text))?,
        )?;
        Ok((
            splice,
            Expect::Rows {
                table: table.path.clone(),
                count: site.rows.len() + 1,
            },
        ))
    }

    fn move_row(&self, path: &str, to: u32) -> Made {
        let (table, from) = row_of(&self.scan.sheet, path).ok_or(EditRefusal::NotFound)?;
        let site = self.table_site(&table.path)?;
        let to = to as usize;
        if to >= site.rows.len() {
            return Err(EditRefusal::NotFound);
        }
        let expect = Expect::Rows {
            table: table.path.clone(),
            count: site.rows.len(),
        };
        if to == from {
            return Ok((
                Splice {
                    start: 0,
                    end: 0,
                    insert: String::new(),
                },
                expect,
            ));
        }
        let (lo, hi) = (from.min(to), from.max(to));
        // the row lines between keep their slots; only their texts move (each slot keeps its own line break)
        let slots: Vec<usize> = site.rows[lo..=hi].iter().map(|r| r.line).collect();
        let mut texts: Vec<&str> = slots.iter().map(|&n| self.lines.line(n)).collect();
        let moved = texts.remove(from - lo);
        texts.insert(to - lo, moved);
        let first = slots[0];
        let last = slots[slots.len() - 1];
        let (start, _, _) = self.lines.bounds(first).ok_or(EditRefusal::NotFound)?;
        let (_, end, _) = self.lines.bounds(last).ok_or(EditRefusal::NotFound)?;
        let mut out = String::with_capacity(end - start);
        let mut slot = 0;
        for n in first..=last {
            let (s, e, next) = self.lines.bounds(n).ok_or(EditRefusal::NotFound)?;
            if slots.get(slot) == Some(&n) {
                out.push_str(texts[slot]);
                slot += 1;
            } else {
                out.push_str(&self.text[s..e]);
            }
            if n < last {
                out.push_str(&self.text[e..next]);
            }
        }
        Ok((
            Splice {
                start,
                end,
                insert: out,
            },
            expect,
        ))
    }

    fn table_site(&self, table: &str) -> Result<&TableSite, EditRefusal> {
        self.scan
            .sites
            .tables
            .get(table)
            .ok_or(EditRefusal::NotFound)
    }

    /// A new line right after line `n` (0-based).
    fn after_line(&self, n: usize, line: &str, eol: &str) -> Result<Splice, EditRefusal> {
        let (_, end, next) = self.lines.bounds(n).ok_or(EditRefusal::NotFound)?;
        Ok(if next == end {
            Splice {
                start: end,
                end,
                insert: format!("{eol}{line}"),
            }
        } else {
            Splice {
                start: next,
                end: next,
                insert: format!("{line}{eol}"),
            }
        })
    }

    /// Line `n` (0-based) goes with its break (the last line without one takes the break before it).
    fn remove_line(&self, n: usize) -> Result<Splice, EditRefusal> {
        let (start, end, next) = self.lines.bounds(n).ok_or(EditRefusal::NotFound)?;
        if next == end && start > 0 {
            let cut = if self.text[..start].ends_with("\r\n") {
                start - 2
            } else {
                start - 1
            };
            return Ok(Splice {
                start: cut,
                end,
                insert: String::new(),
            });
        }
        Ok(Splice {
            start,
            end: next,
            insert: String::new(),
        })
    }

    /// `rows` (a splice below the header) plus the digits of `[N]` set to `count`, as one splice.
    fn with_count(
        &self,
        site: &TableSite,
        count: usize,
        rows: &Splice,
    ) -> Result<Splice, EditRefusal> {
        let digits = site.count.clone();
        if rows.start < digits.end {
            return Err(EditRefusal::Invalid("roundTrip"));
        }
        let mut insert = count.to_string();
        insert.push_str(&self.text[digits.end..rows.start]);
        insert.push_str(&rows.insert);
        Ok(Splice {
            start: digits.start,
            end: rows.end,
            insert,
        })
    }
}

/// The table holding the row at `path`, and the row's index.
fn row_of<'s>(sheet: &'s Sheet, path: &str) -> Option<(&'s Table, usize)> {
    sheet.rows().find_map(|(_, row)| match &row.value {
        RowValue::Table(t) => t.rows.iter().position(|r| r.path == path).map(|i| (t, i)),
        _ => None,
    })
}

/// A key not yet used by the rows of a keyed table: `base`, `base2`, `base3`, ...
fn unique_key(table: &Table, base: &str) -> String {
    let taken = |k: &str| table.rows.iter().any(|r| r.key.as_deref() == Some(k));
    let base = base.trim_end_matches(|c: char| c.is_ascii_digit());
    let base = if base.is_empty() { "new" } else { base };
    // a table has fewer rows than this, so a free key is always found
    (1..=table.rows.len() + 1)
        .map(|n| {
            if n == 1 {
                base.to_owned()
            } else {
                format!("{base}{n}")
            }
        })
        .find(|k| !taken(k))
        .unwrap_or_else(|| base.to_owned())
}

fn row_line(site: &TableSite, key: Option<&str>, cells: &str) -> String {
    let indent = " ".repeat(site.indent);
    match key {
        Some(key) => format!("{indent}{}: {cells}", key_token(key)),
        None => format!("{indent}{cells}"),
    }
}

/// An empty cell of a column's kind.
fn empty_cell(kind: ScalarKind) -> &'static str {
    match kind {
        ScalarKind::Number => "0",
        ScalarKind::Bool => "false",
        ScalarKind::Null => "null",
        ScalarKind::String | ScalarKind::Date => "\"\"",
    }
}

/// A key as TOON writes it: bare when it matches `^[A-Za-z_][A-Za-z0-9_.]*$`, else quoted.
#[must_use]
pub fn key_token(key: &str) -> String {
    let mut chars = key.chars();
    let bare = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if bare {
        key.to_owned()
    } else {
        quoted(key)
    }
}

/// A string as a quoted TOON token.
#[must_use]
pub fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether a string can stand unquoted (§7.2, with the delimiter of its place).
#[must_use]
pub fn plain_safe(text: &str, delim: Option<char>, root: bool) -> bool {
    let numeric_like = {
        let t = text.strip_prefix(['+', '-']).unwrap_or(text);
        let mantissa_end = t.find(['e', 'E']).unwrap_or(t.len());
        let (mantissa, exp) = t.split_at(mantissa_end);
        let mut parts = mantissa.splitn(2, '.');
        let int_ok = parts
            .next()
            .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        let frac_ok = parts
            .next()
            .is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        let exp_ok = exp.is_empty() || {
            let e = exp[1..].strip_prefix(['+', '-']).unwrap_or(&exp[1..]);
            !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit())
        };
        int_ok && frac_ok && exp_ok
    };
    !(text.is_empty()
        || text.starts_with([' ', '\t'])
        || text.ends_with([' ', '\t'])
        || matches!(text, "true" | "false" | "null")
        || numeric_like
        || text.contains([':', '"', '\\', '[', ']', '{', '}'])
        || text.chars().any(|c| u32::from(c) < 0x20)
        || delim.is_some_and(|d| text.contains(d))
        || text.starts_with(['-', '#'])
        || root && text.starts_with('\u{feff}'))
}

fn string_token(text: &str, quote: Quote, delim: Option<char>, root: bool) -> String {
    if quote == Quote::Plain && plain_safe(text, delim, root) {
        text.to_owned()
    } else {
        quoted(text)
    }
}

fn literal_token(kind: ScalarKind, text: &str) -> Result<String, EditRefusal> {
    let ok = match kind {
        ScalarKind::Number => is_json_number(text),
        ScalarKind::Bool => matches!(text, "true" | "false"),
        ScalarKind::Null => text == "null",
        ScalarKind::String | ScalarKind::Date => false,
    };
    if ok {
        Ok(text.to_owned())
    } else {
        Err(EditRefusal::Invalid("literal"))
    }
}
