//! Span scanner: where every key, header, row and value of a TOON text is (bytes and lines). It gives the sheet the
//! view and the agents read, and the sites in-place edits splice. It only reads text the decoder accepted; a line it
//! cannot place becomes a read-only row, never a guess.

use std::collections::HashMap;

use permark_core::format::config::util::{comment_above, join_index, join_key, Lines};
use permark_core::format::config::{
    Group, Quote, Row, RowValue, Scalar, ScalarKind, Sheet, SheetKind, Span, Table, TableRow,
};
use permark_core::format::Editability;

use crate::codec::{indent_of, SPEC_LABEL};

/// A key token: where it is, and what removing it would take.
#[derive(Debug, Clone)]
pub struct KeySite {
    pub span: Span,
    /// 0-based line.
    pub line: usize,
    /// The whole field is this one line (a primitive value, an inline array).
    pub one_line: bool,
    /// The first field of a list item (written on the hyphen line).
    pub hyphen: bool,
    /// Path of the object holding the key.
    pub parent: String,
}

/// An object: the indentation of its fields and its last line (0-based; `None`: an empty document).
#[derive(Debug, Clone)]
pub struct ObjectSite {
    pub indent: usize,
    pub last: Option<usize>,
}

/// One leaf field of a table header: its key path below the row, and its name token.
#[derive(Debug, Clone)]
pub struct Leaf {
    pub segs: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RowSite {
    /// 0-based line.
    pub line: usize,
    /// The entry key and its token (keyed tables).
    pub key: Option<(String, Span)>,
    pub kinds: Vec<ScalarKind>,
}

#[derive(Debug, Clone)]
pub struct TableSite {
    /// The digits of `[N]`.
    pub count: Span,
    /// 0-based line of the header.
    pub header: usize,
    /// Spaces before a row.
    pub indent: usize,
    pub delim: char,
    pub keyed: bool,
    pub fields: Vec<Leaf>,
    pub rows: Vec<RowSite>,
}

/// Everything an edit needs to place its splice.
#[derive(Debug, Default)]
pub struct Sites {
    pub keys: HashMap<String, KeySite>,
    pub objects: HashMap<String, ObjectSite>,
    pub tables: HashMap<String, TableSite>,
    /// The delimiter that splits the token at a path (table cells, inline array items).
    pub delims: HashMap<String, char>,
    /// Path of the root primitive (a document that is one value).
    pub root_primitive: bool,
}

#[derive(Debug)]
pub struct Scan {
    pub sheet: Sheet,
    pub sites: Sites,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Blank,
    Comment,
    Content,
}

#[derive(Debug, Clone, Copy)]
struct Info {
    cstart: usize,
    cend: usize,
    indent: usize,
    kind: LineKind,
}

/// The sheet and sites of a text the decoder accepted.
#[must_use]
pub fn scan(text: &str) -> Scan {
    let lines = Lines::new(text);
    let info: Vec<Info> = (0..lines.count())
        .map(|n| line_info(text, &lines, n))
        .collect();
    let content = (0..info.len())
        .filter(|&n| info[n].kind == LineKind::Content)
        .collect();
    let mut scanner = Scanner {
        text,
        lines,
        info,
        content,
        pos: 0,
        isz: indent_of(text).max(1),
        groups: Vec::new(),
        index: HashMap::new(),
        sites: Sites::default(),
    };
    scanner.root();
    // whatever is left (it should not be): shown as written, never edited
    while let Some(li) = scanner.peek() {
        scanner.pos += 1;
        scanner.raw_row(li, "", "nested");
    }
    let mut groups = scanner.groups;
    groups.sort_by_key(|g| g.line);
    let mut sheet = Sheet::new(SheetKind::Data);
    sheet.groups = groups;
    sheet.spec = Some(SPEC_LABEL.to_owned());
    sheet.finalize();
    Scan {
        sheet,
        sites: scanner.sites,
    }
}

fn line_info(text: &str, lines: &Lines<'_>, n: usize) -> Info {
    let (start, end, _) = lines.bounds(n).unwrap_or((0, 0, 0));
    let mut cs = start;
    if n == 0 && text[start..end].starts_with('\u{feff}') {
        cs += '\u{feff}'.len_utf8();
    }
    let line = text[cs..end].trim_end_matches(' ');
    let spaces = line.bytes().take_while(|b| *b == b' ').count();
    let kind = if line.is_empty() {
        LineKind::Blank
    } else if line[spaces..].starts_with('#') {
        LineKind::Comment
    } else {
        LineKind::Content
    };
    Info {
        cstart: cs + spaces,
        cend: cs + line.len(),
        indent: spaces,
        kind,
    }
}

struct Scanner<'a> {
    text: &'a str,
    lines: Lines<'a>,
    info: Vec<Info>,
    /// Indexes of the content lines.
    content: Vec<usize>,
    pos: usize,
    isz: usize,
    groups: Vec<Group>,
    index: HashMap<String, usize>,
    sites: Sites,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<usize> {
        self.content.get(self.pos).copied()
    }

    fn depth(&self, li: usize) -> usize {
        self.info[li].indent / self.isz
    }

    fn line_no(li: usize) -> u32 {
        u32::try_from(li + 1).unwrap_or(u32::MAX)
    }

    fn caption(&self, li: usize) -> Option<String> {
        comment_above(&self.lines, li, &['#'])
    }

    /// The group (card) at `path`, made when first needed.
    fn group(&mut self, path: &str, li: usize) -> usize {
        if let Some(&at) = self.index.get(path) {
            return at;
        }
        let line = Self::line_no(li);
        self.groups.push(Group {
            path: path.to_owned(),
            title: path.to_owned(),
            line,
            end: line,
            comment: None,
            rows: Vec::new(),
        });
        self.index.insert(path.to_owned(), self.groups.len() - 1);
        self.groups.len() - 1
    }

    fn push_row(&mut self, group: &str, row: Row) {
        let at = self.group(group, row.line as usize - 1);
        let g = &mut self.groups[at];
        g.line = g.line.min(row.line);
        g.end = g.end.max(row.end);
        g.rows.push(row);
    }

    fn close_group(&mut self, path: &str, header: usize, last: usize) {
        let caption = self.caption(header);
        let at = self.group(path, header);
        let g = &mut self.groups[at];
        g.line = g.line.min(Self::line_no(header));
        g.end = g.end.max(Self::line_no(last));
        if g.comment.is_none() {
            g.comment = caption;
        }
    }

    /// One line shown as written (a part the scanner does not place).
    fn raw_row(&mut self, li: usize, group: &str, reason: &str) {
        let Info { cstart, cend, .. } = self.info[li];
        let row = Row {
            path: format!("{group}#L{}", li + 1),
            key: String::new(),
            line: Self::line_no(li),
            end: Self::line_no(li),
            value: RowValue::Raw {
                text: self.text[cstart..cend].to_owned(),
                reason: reason.to_owned(),
                secret: false,
                span: Some(cstart..cend),
            },
            comment: None,
        };
        self.push_row(group, row);
    }

    fn root(&mut self) {
        let Some(li) = self.peek() else {
            self.sites.objects.insert(
                String::new(),
                ObjectSite {
                    indent: 0,
                    last: None,
                },
            );
            return;
        };
        let Info { cstart, cend, .. } = self.info[li];
        let c = &self.text[cstart..cend];
        if c.starts_with('[') {
            if let Some(h) = header(self.text, cstart, cend).filter(|h| h.key.is_none()) {
                self.pos += 1;
                self.array(li, &h, "", "", 1);
                return;
            }
        }
        if c == "[]" {
            self.pos += 1;
            self.push_row("", list_row("", "", li, Vec::new(), None));
            return;
        }
        if self.content.len() == 1 && find_unquoted(c, 0, ':').is_none() {
            self.pos += 1;
            let scalar = value_scalar(String::new(), c, cstart..cend);
            self.sites.root_primitive = true;
            self.push_row(
                "",
                Row {
                    path: String::new(),
                    key: String::new(),
                    line: Self::line_no(li),
                    end: Self::line_no(li),
                    value: RowValue::Scalar(scalar),
                    comment: self.caption(li),
                },
            );
            return;
        }
        let last = self.object(0, "");
        self.sites
            .objects
            .insert(String::new(), ObjectSite { indent: 0, last });
    }

    /// The fields of an object at `depth`; returns its last line.
    fn object(&mut self, depth: usize, path: &str) -> Option<usize> {
        let mut last = None;
        while let Some(li) = self.peek() {
            let d = self.depth(li);
            if d < depth {
                break;
            }
            self.pos += 1;
            if d > depth || !self.info[li].indent.is_multiple_of(self.isz) {
                self.raw_row(li, path, "nested");
                last = Some(li);
                continue;
            }
            let Info { cstart, cend, .. } = self.info[li];
            last = Some(self.field(li, cstart, cend, depth, path, false));
        }
        last
    }

    /// One field (`key: value`, `key:`, a header) whose text is `cs..ce` of line `li`, at `depth`, in the object
    /// `parent`; returns the last line of the field (with its scope).
    fn field(
        &mut self,
        li: usize,
        cs: usize,
        ce: usize,
        depth: usize,
        parent: &str,
        hyphen: bool,
    ) -> usize {
        let c = &self.text[cs..ce];
        let colon = find_unquoted(c, 0, ':');
        let bracket = find_unquoted(c, 0, '[');
        match (colon, bracket) {
            (Some(col), Some(b)) if b < col => {
                let Some(h) = header(self.text, cs, ce) else {
                    self.raw_row(li, parent, "nested");
                    return li;
                };
                let Some((key, span)) = h.key.clone() else {
                    self.raw_row(li, parent, "nested");
                    return li;
                };
                let path = join_key(parent, &key);
                let one_line = h.leaves.is_none() && h.rest.0 < h.rest.1;
                self.sites.keys.insert(
                    path.clone(),
                    KeySite {
                        span,
                        line: li,
                        one_line,
                        hyphen,
                        parent: parent.to_owned(),
                    },
                );
                self.array(li, &h, &path, parent, depth + 1)
            }
            (Some(col), _) => {
                let (ks, ke) = trim(c, 0, col);
                let key = parse_key(&c[ks..ke]);
                let path = join_key(parent, &key);
                let (vs, ve) = trim(c, col + 1, c.len());
                let one_line = vs < ve;
                self.sites.keys.insert(
                    path.clone(),
                    KeySite {
                        span: cs + ks..cs + ke,
                        line: li,
                        one_line,
                        hyphen,
                        parent: parent.to_owned(),
                    },
                );
                if !one_line {
                    // a nested object: its fields one level deeper
                    let last = self.object(depth + 1, &path);
                    self.close_group(&path, li, last.unwrap_or(li));
                    self.sites.objects.insert(
                        path,
                        ObjectSite {
                            indent: (depth + 1) * self.isz,
                            last: Some(last.unwrap_or(li)),
                        },
                    );
                    return last.unwrap_or(li);
                }
                let value = &c[vs..ve];
                let row = if value == "[]" {
                    list_row(&path, &key, li, Vec::new(), self.caption(li))
                } else {
                    Row {
                        path: path.clone(),
                        key,
                        line: Self::line_no(li),
                        end: Self::line_no(li),
                        value: RowValue::Scalar(value_scalar(path, value, cs + vs..cs + ve)),
                        comment: self.caption(li),
                    }
                };
                self.push_row(parent, row);
                li
            }
            _ => {
                self.raw_row(li, parent, "nested");
                li
            }
        }
    }

    /// The array (or keyed object) a header opens at line `li`; its rows | items stand at `inner` depth. Returns its
    /// last line.
    fn array(&mut self, li: usize, h: &Header, path: &str, parent: &str, inner: usize) -> usize {
        let key = h.key.as_ref().map(|(k, _)| k.clone()).unwrap_or_default();
        if let Some(leaves) = &h.leaves {
            return self.table(li, h, leaves, path, &key, inner);
        }
        if h.rest.0 < h.rest.1 {
            // inline primitive array
            let items = self.items(path, h.rest, h.delim);
            let caption = self.caption(li);
            self.push_row(parent, list_row(path, &key, li, items, caption));
            return li;
        }
        if h.n == 0 {
            let caption = self.caption(li);
            self.push_row(parent, list_row(path, &key, li, Vec::new(), caption));
            return li;
        }
        let last = self.list(inner, path);
        self.close_group(path, li, last.unwrap_or(li));
        last.unwrap_or(li)
    }

    fn items(&mut self, path: &str, (from, to): (usize, usize), delim: char) -> Vec<Scalar> {
        split(self.text, from, to, delim)
            .into_iter()
            .enumerate()
            .map(|(i, (s, e))| {
                let item = join_index(path, i);
                self.sites.delims.insert(item.clone(), delim);
                value_scalar(item, &self.text[s..e], s..e)
            })
            .collect()
    }

    fn table(
        &mut self,
        li: usize,
        h: &Header,
        leaves: &[Leaf],
        path: &str,
        key: &str,
        row_depth: usize,
    ) -> usize {
        let mut rows = Vec::new();
        let mut sites = Vec::new();
        let mut last = li;
        while let Some(r) = self.peek() {
            if self.depth(r) != row_depth || !self.info[r].indent.is_multiple_of(self.isz) {
                break;
            }
            self.pos += 1;
            last = r;
            let Info { cstart, cend, .. } = self.info[r];
            let c = &self.text[cstart..cend];
            let (row_path, entry, from) = if h.keyed {
                let col = find_unquoted(c, 0, ':').unwrap_or(c.len());
                let (ks, ke) = trim(c, 0, col);
                let entry = parse_key(&c[ks..ke]);
                (
                    join_key(path, &entry),
                    Some((entry, cstart + ks..cstart + ke)),
                    cstart + (col + 1).min(c.len()),
                )
            } else {
                (join_index(path, rows.len()), None, cstart)
            };
            let tokens = split(self.text, from, cend, h.delim);
            let cells: Vec<Scalar> = leaves
                .iter()
                .zip(&tokens)
                .map(|(leaf, &(s, e))| {
                    let cell = leaf
                        .segs
                        .iter()
                        .fold(row_path.clone(), |p, k| join_key(&p, k));
                    self.sites.delims.insert(cell.clone(), h.delim);
                    value_scalar(cell, &self.text[s..e], s..e)
                })
                .collect();
            sites.push(RowSite {
                line: r,
                key: entry.clone(),
                kinds: cells.iter().map(|c| c.kind).collect(),
            });
            rows.push(TableRow {
                path: row_path,
                line: Self::line_no(r),
                key: entry.map(|(k, _)| k),
                cells,
            });
        }
        let table = Table {
            path: path.to_owned(),
            fields: leaves.iter().map(|l| l.segs.join(".")).collect(),
            count: u32::try_from(h.n).unwrap_or(u32::MAX),
            keyed: h.keyed,
            delimiter: h.delim.to_string(),
            rows,
            edit: Editability::Surgical,
        };
        let row = Row {
            path: path.to_owned(),
            key: key.to_owned(),
            line: Self::line_no(li),
            end: Self::line_no(last),
            value: RowValue::Table(table),
            comment: None,
        };
        self.push_row(path, row);
        self.close_group(path, li, last);
        self.sites.tables.insert(
            path.to_owned(),
            TableSite {
                count: h.count.clone(),
                header: li,
                indent: row_depth * self.isz,
                delim: h.delim,
                keyed: h.keyed,
                fields: leaves.to_vec(),
                rows: sites,
            },
        );
        last
    }

    /// The `- ` items of a list-form array at `depth`; returns the last line.
    fn list(&mut self, depth: usize, path: &str) -> Option<usize> {
        let mut last = None;
        let mut i = 0;
        while let Some(li) = self.peek() {
            if self.depth(li) != depth || !self.info[li].indent.is_multiple_of(self.isz) {
                break;
            }
            let Info { cstart, cend, .. } = self.info[li];
            let c = &self.text[cstart..cend];
            if !(c == "-" || c.starts_with("- ")) {
                break;
            }
            self.pos += 1;
            let item = join_index(path, i);
            let key = format!("[{i}]");
            let rs = cstart + 1 + c[1..].bytes().take_while(|b| *b == b' ').count();
            let rest = &self.text[rs..cend];
            let mut end = li;
            if rest.is_empty() {
                // an empty object
                self.close_group(&item, li, li);
                self.sites.objects.insert(
                    item.clone(),
                    ObjectSite {
                        indent: (depth + 1) * self.isz,
                        last: Some(li),
                    },
                );
            } else if rest == "[]" {
                let caption = self.caption(li);
                self.push_row(path, list_row(&item, &key, li, Vec::new(), caption));
            } else if let Some(h) = rest
                .starts_with('[')
                .then(|| header(self.text, rs, cend))
                .flatten()
                .filter(|h| h.key.is_none() && h.leaves.is_none())
            {
                if h.rest.0 < h.rest.1 {
                    let items = self.items(&item, h.rest, h.delim);
                    let caption = self.caption(li);
                    self.push_row(path, list_row(&item, &key, li, items, caption));
                } else if h.n == 0 {
                    let caption = self.caption(li);
                    self.push_row(path, list_row(&item, &key, li, Vec::new(), caption));
                } else {
                    let inner = self.list(depth + 1, &item);
                    end = inner.unwrap_or(li);
                    self.close_group(&item, li, end);
                }
            } else if find_unquoted(rest, 0, ':').is_some() {
                // an object: its first field on the hyphen line, the others one level deeper
                let first = self.field(li, rs, cend, depth + 1, &item, true);
                let others = self.object(depth + 1, &item);
                end = others.unwrap_or(first).max(first);
                self.close_group(&item, li, end);
                self.sites.objects.insert(
                    item.clone(),
                    ObjectSite {
                        indent: (depth + 1) * self.isz,
                        last: Some(end),
                    },
                );
            } else {
                let row = Row {
                    path: item.clone(),
                    key,
                    line: Self::line_no(li),
                    end: Self::line_no(li),
                    value: RowValue::Scalar(value_scalar(item.clone(), rest, rs..cend)),
                    comment: self.caption(li),
                };
                self.push_row(path, row);
            }
            last = Some(end.max(last.unwrap_or(0)));
            i += 1;
        }
        last
    }
}

fn list_row(path: &str, key: &str, li: usize, items: Vec<Scalar>, comment: Option<String>) -> Row {
    let line = u32::try_from(li + 1).unwrap_or(u32::MAX);
    Row {
        path: path.to_owned(),
        key: key.to_owned(),
        line,
        end: line,
        value: RowValue::List { items },
        comment,
    }
}

/// A parsed header line.
#[derive(Debug, Clone)]
pub struct Header {
    pub key: Option<(String, Span)>,
    pub count: Span,
    pub n: u64,
    pub keyed: bool,
    pub delim: char,
    pub leaves: Option<Vec<Leaf>>,
    /// What follows the colon (trimmed), absolute bytes.
    pub rest: (usize, usize),
}

/// The header in `text[cs..ce]` (`key[N<delim>]{fields}:` and its keyless | keyed forms).
#[must_use]
pub fn header(text: &str, cs: usize, ce: usize) -> Option<Header> {
    let c = &text[cs..ce];
    let b = find_unquoted(c, 0, '[')?;
    let key = if b == 0 {
        None
    } else {
        let (ks, ke) = trim(c, 0, b);
        Some((parse_key(&c[ks..ke]), cs + ks..cs + ke))
    };
    let bytes = c.as_bytes();
    let mut i = b + 1;
    let digits = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits {
        return None;
    }
    let n = c[digits..i].parse::<u64>().unwrap_or(u64::MAX);
    let count = cs + digits..cs + i;
    let mut keyed = false;
    if bytes.get(i) == Some(&b':') {
        keyed = true;
        i += 1;
    }
    let mut delim = ',';
    match bytes.get(i) {
        Some(b'\t') => {
            delim = '\t';
            i += 1;
        }
        Some(b'|') => {
            delim = '|';
            i += 1;
        }
        _ => {}
    }
    if bytes.get(i) != Some(&b']') {
        return None;
    }
    i += 1;
    let leaves = if bytes.get(i) == Some(&b'{') {
        let close = matching_brace(c, i)?;
        let mut leaves = Vec::new();
        fields(c, cs, i + 1, close, delim, &[], &mut leaves)?;
        i = close + 1;
        Some(leaves)
    } else {
        None
    };
    if bytes.get(i) != Some(&b':') {
        return None;
    }
    let (rs, re) = trim(c, i + 1, c.len());
    Some(Header {
        key,
        count,
        n,
        keyed,
        delim,
        leaves,
        rest: (cs + rs, cs + re),
    })
}

/// The leaves of a field list `c[from..to]` (inside its braces).
fn fields(
    c: &str,
    cs: usize,
    from: usize,
    to: usize,
    delim: char,
    prefix: &[String],
    out: &mut Vec<Leaf>,
) -> Option<()> {
    let mut at = from;
    loop {
        // the entry runs to the next delimiter outside quotes and braces
        let mut end = to;
        let mut brace = None;
        let mut depth = 0_usize;
        let mut quoted = false;
        let mut escaped = false;
        for (i, ch) in c[at..to].char_indices() {
            let pos = at + i;
            if quoted {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quoted = false;
                }
                continue;
            }
            match ch {
                '"' => quoted = true,
                '{' => {
                    if depth == 0 && brace.is_none() {
                        brace = Some(pos);
                    }
                    depth += 1;
                }
                '}' => depth = depth.saturating_sub(1),
                _ if ch == delim && depth == 0 => {
                    end = pos;
                    break;
                }
                _ => {}
            }
        }
        let name_end = brace.unwrap_or(end);
        let (ns, ne) = trim(c, at, name_end);
        if ns >= ne {
            return None;
        }
        let mut segs = prefix.to_vec();
        segs.push(parse_key(&c[ns..ne]));
        match brace {
            Some(open) => {
                let close = matching_brace(c, open)?;
                fields(c, cs, open + 1, close, delim, &segs, out)?;
            }
            None => out.push(Leaf {
                segs,
                span: cs + ns..cs + ne,
            }),
        }
        if end >= to {
            return Some(());
        }
        at = end + delim.len_utf8();
    }
}

fn matching_brace(c: &str, open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    for (i, ch) in c[open..].char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Byte index of the first `want` outside quoted spans, from `from`.
#[must_use]
pub fn find_unquoted(s: &str, from: usize, want: char) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    for (i, ch) in s.get(from..)?.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        if ch == '"' {
            quoted = true;
        } else if ch == want {
            return Some(from + i);
        }
    }
    None
}

/// `s[from..to]` without spaces on either side, as bounds.
fn trim(s: &str, from: usize, to: usize) -> (usize, usize) {
    let part = &s[from..to];
    let start = from + (part.len() - part.trim_start_matches(' ').len());
    let end = to - (part.len() - part.trim_end_matches(' ').len());
    if start > end {
        (from, from)
    } else {
        (start, end)
    }
}

/// The tokens of `text[from..to]` split on `delim` outside quotes, trimmed of spaces (absolute bounds).
#[must_use]
pub fn split(text: &str, from: usize, to: usize, delim: char) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let s = &text[from..to];
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        if ch == '"' {
            quoted = true;
        } else if ch == delim {
            let (a, b) = trim(s, start, i);
            out.push((from + a, from + b));
            start = i + ch.len_utf8();
        }
    }
    let (a, b) = trim(s, start, s.len());
    out.push((from + a, from + b));
    out
}

/// A key token as the key it names (a quoted one unescaped).
#[must_use]
pub fn parse_key(token: &str) -> String {
    if token.len() >= 2 && token.starts_with('"') && token.ends_with('"') {
        unescape(&token[1..token.len() - 1])
    } else {
        token.to_owned()
    }
}

/// The body of a quoted string, unescaped (`\\ \" \n \r \t \uXXXX`).
#[must_use]
pub fn unescape(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/`: the tokens that decode as numbers.
#[must_use]
pub fn is_json_number(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    match b.get(i) {
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => {
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        _ => return false,
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    i == b.len()
}

/// A value token as a scalar: quoted -> string; `true | false` -> bool; `null`; a JSON number; else a string.
fn value_scalar(path: String, token: &str, span: Span) -> Scalar {
    if token.len() >= 2 && token.starts_with('"') && token.ends_with('"') {
        return Scalar::new(
            path,
            ScalarKind::String,
            unescape(&token[1..token.len() - 1]),
            span,
        )
        .quoted(Quote::Double);
    }
    let kind = match token {
        "true" | "false" => ScalarKind::Bool,
        "null" => ScalarKind::Null,
        _ if is_json_number(token) => ScalarKind::Number,
        _ => ScalarKind::String,
    };
    Scalar::new(path, kind, token.to_owned(), span)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_by_the_json_grammar() {
        for yes in ["0", "-0", "1", "12", "1.5", "-1.5e3", "1E+03", "0e1"] {
            assert!(is_json_number(yes), "{yes}");
        }
        for no in [
            "", "-", "05", "-05", ".5", "1.", "+5", "1e", "0x10", "1_000", "NaN",
        ] {
            assert!(!is_json_number(no), "{no}");
        }
    }

    #[test]
    fn splits_outside_quotes() {
        let text = r#"a, "b,c" ,d"#;
        let parts: Vec<&str> = split(text, 0, text.len(), ',')
            .into_iter()
            .map(|(s, e)| &text[s..e])
            .collect();
        assert_eq!(parts, ["a", "\"b,c\"", "d"]);
    }

    #[test]
    fn headers_of_every_shape() {
        let h = header("users[2]{id,name}:", 0, 18).unwrap();
        assert_eq!(h.key.map(|k| k.0).as_deref(), Some("users"));
        assert_eq!(h.n, 2);
        assert_eq!(h.leaves.map(|l| l.len()), Some(2));
        let h = header("[3|]: a|b|c", 0, 11).unwrap();
        assert!(h.key.is_none());
        assert_eq!(h.delim, '|');
        let h = header("m[2:]{a,c{x,y}}:", 0, 16).unwrap();
        assert!(h.keyed);
        let leaves = h.leaves.unwrap();
        assert_eq!(leaves[2].segs, ["c", "y"]);
        assert!(header("a[x]: 1", 0, 7).is_none());
    }
}
