//! A block-style YAML reader that keeps the bytes of every value: mappings, sequences, plain and quoted scalars.
//! What it cannot edit exactly (anchors, aliases, tags, flow collections, block scalars, multi-line scalars) it
//! shows as read-only. It is not a validator: text it does not understand is skipped, never rewritten.

use std::ops::Range;

use permark_core::format::config::util::{unescape_double, Lines};
use permark_core::format::config::{Quote, ScalarKind};

/// Nesting followed before the rest of a block is shown as it is written.
const MAX_DEPTH: usize = 48;

#[derive(Debug, Clone)]
pub struct Proto {
    pub kind: ScalarKind,
    pub text: String,
    pub span: Range<usize>,
    pub quote: Quote,
    pub read_only: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub enum Val {
    Scalar(Proto),
    Raw {
        text: String,
        reason: &'static str,
        span: Range<usize>,
    },
    Map(Vec<Item>),
    Seq {
        items: Vec<SeqItem>,
        first: usize,
        last: usize,
    },
}

/// A mapping entry. Lines are 0-based.
#[derive(Debug, Clone)]
pub struct Item {
    pub key: String,
    pub line: usize,
    pub end: usize,
    pub comment: Option<String>,
    pub val: Val,
}

#[derive(Debug, Clone)]
pub struct SeqItem {
    pub line: usize,
    pub val: Val,
}

#[derive(Debug, Clone, Copy)]
struct Info {
    start: usize,
    end: usize,
    indent: usize,
    /// Empty, comment only, or a document marker: no content.
    blank: bool,
}

pub struct Parsed {
    pub root: Option<Val>,
    /// The first line of the documents after the first (0-based).
    pub more_documents: Option<usize>,
    pub significant: bool,
}

pub struct Parser<'a> {
    text: &'a str,
    pub lines: Lines<'a>,
    infos: Vec<Info>,
    limit: usize,
}

fn is_seq_start(content: &str) -> bool {
    content == "-" || content.starts_with("- ") || content.starts_with("-\t")
}

fn is_marker(line: &str, marker: &str) -> bool {
    line.starts_with(marker) && matches!(line.as_bytes().get(3), None | Some(b' ' | b'\t'))
}

/// Where a plain scalar ends on its line: before ` #` and trailing spaces.
fn plain_end(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'#' && (i == 0 || matches!(bytes[i - 1], b' ' | b'\t')) {
            end = i;
            break;
        }
    }
    s[..end].trim_end().len()
}

/// Kind of a plain scalar.
pub fn kind_of_plain(s: &str) -> ScalarKind {
    if s.is_empty() || matches!(s, "~" | "null" | "Null" | "NULL") {
        return ScalarKind::Null;
    }
    if matches!(s, "true" | "false" | "True" | "False" | "TRUE" | "FALSE") {
        return ScalarKind::Bool;
    }
    if is_number(s) {
        return ScalarKind::Number;
    }
    if is_date(s) {
        return ScalarKind::Date;
    }
    ScalarKind::String
}

fn is_number(s: &str) -> bool {
    let t = s.strip_prefix(['-', '+']).unwrap_or(s);
    if matches!(t, ".inf" | ".Inf" | ".INF" | ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    if let Some(hex) = t.strip_prefix("0x") {
        return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    if let Some(oct) = t.strip_prefix("0o") {
        return !oct.is_empty() && oct.chars().all(|c| ('0'..='7').contains(&c));
    }
    let mut chars = t.chars().peekable();
    let mut digits = 0;
    while chars.next_if(|c| c.is_ascii_digit() || *c == '_').is_some() {
        digits += 1;
    }
    if chars.next_if(|c| *c == '.').is_some() {
        while chars.next_if(char::is_ascii_digit).is_some() {
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if chars.next_if(|c| matches!(c, 'e' | 'E')).is_some() {
        chars.next_if(|c| matches!(c, '+' | '-'));
        let mut exp = 0;
        while chars.next_if(char::is_ascii_digit).is_some() {
            exp += 1;
        }
        if exp == 0 {
            return false;
        }
    }
    chars.next().is_none()
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    b.len() == 10 || matches!(b[10], b'T' | b't' | b' ')
}

/// The key of a `key: value` line and where its value starts (after the colon).
struct Key {
    text: String,
    /// Byte offset in the content just after the colon.
    after: usize,
}

fn split_key(content: &str) -> Option<Key> {
    let bytes = content.as_bytes();
    let first = *bytes.first()?;
    if matches!(
        first,
        b'[' | b'{' | b'*' | b'&' | b'!' | b'|' | b'>' | b'%' | b'@' | b'`' | b'#'
    ) {
        return None;
    }
    if first == b'?' && matches!(bytes.get(1), None | Some(b' ')) {
        return None;
    }
    if first == b'"' || first == b'\'' {
        let close = quoted_end(content, 0)?;
        let rest = &content[close..];
        let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let at = close + spaces;
        let after = content.get(at..)?;
        if !after.starts_with(':') || !matches!(after.as_bytes().get(1), None | Some(b' ' | b'\t'))
        {
            return None;
        }
        let inner = &content[1..close - 1];
        let text = if first == b'"' {
            unescape_double(inner)
        } else {
            inner.replace("''", "'")
        };
        return Some(Key {
            text,
            after: at + 1,
        });
    }
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b':' if matches!(bytes.get(i + 1), None | Some(b' ' | b'\t')) => {
                let text = content[..i].trim_end().to_owned();
                return Some(Key { text, after: i + 1 });
            }
            b'#' if i > 0 && matches!(bytes[i - 1], b' ' | b'\t') => return None,
            _ => i += 1,
        }
    }
    None
}

/// Index just after the closing quote of the quoted scalar that starts at `from` on this line; none when it does
/// not close here.
fn quoted_end(s: &str, from: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let quote = *bytes.get(from)?;
    let mut i = from + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if quote == b'"' => i += 2,
            b'\'' if quote == b'\'' && bytes.get(i + 1) == Some(&b'\'') => i += 2,
            q if q == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

impl<'a> Parser<'a> {
    pub fn new(text: &'a str) -> Self {
        let lines = Lines::new(text);
        let mut infos = Vec::with_capacity(lines.count());
        for n in 0..lines.count() {
            let (start, end, _) = lines.bounds(n).unwrap_or((0, 0, 0));
            let line = &text[start..end];
            let indent = line.len() - line.trim_start_matches(' ').len();
            let rest = &line[indent..];
            let blank = rest.trim().is_empty() || rest.starts_with('#');
            infos.push(Info {
                start,
                end,
                indent,
                blank,
            });
        }
        Self {
            text,
            lines,
            infos,
            limit: 0,
        }
    }

    fn content(&self, n: usize) -> &'a str {
        let info = self.infos[n];
        &self.text[info.start + info.indent..info.end]
    }

    fn next_sig(&self, from: usize) -> Option<usize> {
        (from..self.limit).find(|&n| !self.infos[n].blank)
    }

    /// Document markers and directives are not content; the first document ends at the second `---` or a `...`.
    fn mark_documents(&mut self) -> Option<usize> {
        let mut seen_content = false;
        let mut opened = false;
        for n in 0..self.infos.len() {
            if self.infos[n].blank {
                continue;
            }
            let line = &self.text[self.infos[n].start..self.infos[n].end];
            if self.infos[n].indent == 0 {
                if is_marker(line, "---") {
                    if seen_content || opened {
                        self.limit = n;
                        return Some(n);
                    }
                    opened = true;
                    self.infos[n].blank = true;
                    continue;
                }
                if is_marker(line, "...") {
                    self.limit = n;
                    return Some(n);
                }
                if line.starts_with('%') && !seen_content {
                    self.infos[n].blank = true;
                    continue;
                }
            }
            seen_content = true;
        }
        None
    }

    pub fn parse(&mut self) -> Parsed {
        self.limit = self.infos.len();
        let more = self.mark_documents();
        let Some(first) = self.next_sig(0) else {
            return Parsed {
                root: None,
                more_documents: more,
                significant: false,
            };
        };
        let indent = self.infos[first].indent;
        let (val, _) = self.block(first, indent, 0);
        Parsed {
            root: Some(val),
            more_documents: more,
            significant: true,
        }
    }

    /// A block (mapping or sequence) whose lines start at `from` with `indent`. Returns it and the next line.
    fn block(&mut self, from: usize, indent: usize, depth: usize) -> (Val, usize) {
        if depth > MAX_DEPTH {
            let last = self.skip_deeper(from, indent.saturating_sub(1));
            return (
                self.region(from, last.saturating_sub(1).max(from), "nested"),
                last,
            );
        }
        if is_seq_start(self.content(from)) {
            let (items, next) = self.seq(from, indent, depth);
            let last = next.saturating_sub(1).max(from);
            (
                Val::Seq {
                    items,
                    first: from,
                    last,
                },
                next,
            )
        } else {
            let (items, next) = self.map(from, indent, depth);
            (Val::Map(items), next)
        }
    }

    /// First line after `from` whose indent is `<= indent` (blank lines go along).
    fn skip_deeper(&self, from: usize, indent: usize) -> usize {
        let mut n = from;
        while n < self.limit {
            let info = self.infos[n];
            if !info.blank && info.indent <= indent && n > from {
                break;
            }
            n += 1;
        }
        n
    }

    fn region(&self, first: usize, last: usize, reason: &'static str) -> Val {
        let start = self.infos[first].start + self.infos[first].indent;
        let end = self.infos[last.min(self.infos.len() - 1)].end.max(start);
        Val::Raw {
            text: self.text[start..end].to_owned(),
            reason,
            span: start..end,
        }
    }

    fn map(&mut self, from: usize, indent: usize, depth: usize) -> (Vec<Item>, usize) {
        let mut items = Vec::new();
        let mut i = from;
        loop {
            let Some(j) = self.next_sig(i) else {
                i = self.limit;
                break;
            };
            let info = self.infos[j];
            if info.indent < indent {
                i = j;
                break;
            }
            if info.indent > indent {
                // deeper lines nobody claimed: left alone
                i = self.skip_deeper(j, indent);
                continue;
            }
            let content = self.content(j);
            if is_seq_start(content) && !items.is_empty() {
                // a dash where a key should be: it ends this mapping (the parent decides)
                i = j;
                break;
            }
            let Some(key) = split_key(content) else {
                i = j + 1;
                continue;
            };
            let (item, next) = self.entry(j, indent, &key, depth);
            items.push(item);
            i = next;
        }
        (items, i)
    }

    /// Reads `anchor` | `tag` prefixes from the start of a value; returns the rest and which prefix was there.
    fn props(rest: &str, offset: usize) -> (&str, usize, Option<&'static str>) {
        let mut rest = rest;
        let mut offset = offset;
        let mut prop = None;
        loop {
            let first = rest.as_bytes().first().copied();
            if !matches!(first, Some(b'&' | b'!')) {
                break;
            }
            let word = rest.find([' ', '\t']).unwrap_or(rest.len());
            prop = Some(if first == Some(b'&') { "anchor" } else { "tag" });
            let tail = &rest[word..];
            let spaces = tail.len() - tail.trim_start_matches([' ', '\t']).len();
            offset += word + spaces;
            rest = &tail[spaces..];
        }
        (rest, offset, prop)
    }

    fn entry(&mut self, j: usize, indent: usize, key: &Key, depth: usize) -> (Item, usize) {
        let info = self.infos[j];
        let line_base = info.start + indent;
        let after_colon = line_base + key.after;
        let tail = &self.text[after_colon..info.end];
        let spaces = tail.len() - tail.trim_start_matches([' ', '\t']).len();
        let (rest, offset, prop) = Self::props(&tail[spaces..], after_colon + spaces);
        let above = permark_core::format::config::util::comment_above(&self.lines, j, &['#']);
        let (val, last, trailing) = if rest.is_empty() || rest.starts_with('#') {
            let comment = trailing(rest);
            match self.next_sig(j + 1) {
                Some(n)
                    if self.infos[n].indent > indent
                        && (is_seq_start(self.content(n))
                            || split_key(self.content(n)).is_some()) =>
                {
                    let (val, next) = self.block(n, self.infos[n].indent, depth + 1);
                    (val, next.saturating_sub(1).max(j), comment)
                }
                Some(n) if self.infos[n].indent == indent && is_seq_start(self.content(n)) => {
                    let (items, next) = self.seq(n, indent, depth + 1);
                    let last = next.saturating_sub(1).max(j);
                    (
                        Val::Seq {
                            items,
                            first: n,
                            last,
                        },
                        last,
                        comment,
                    )
                }
                Some(n) if self.infos[n].indent > indent => {
                    // a scalar written on the lines below its key
                    let end_line = self.skip_deeper(n, indent).saturating_sub(1).max(n);
                    let start = self.infos[n].start + self.infos[n].indent;
                    let end = self.infos[end_line].end;
                    (
                        Val::Scalar(Proto {
                            kind: ScalarKind::String,
                            text: self.text[start..end].to_owned(),
                            span: start..end,
                            quote: Quote::Plain,
                            read_only: Some("multiline"),
                        }),
                        end_line,
                        comment,
                    )
                }
                _ => (
                    Val::Scalar(Proto {
                        kind: ScalarKind::Null,
                        text: String::new(),
                        span: after_colon..after_colon,
                        quote: Quote::Plain,
                        read_only: prop.map(|_| "anchor"),
                    }),
                    j,
                    comment,
                ),
            }
        } else {
            self.value(j, indent, offset, rest, prop)
        };
        (
            Item {
                key: key.text.clone(),
                line: j,
                end: last,
                comment: permark_core::format::config::util::caption(above, trailing),
                val,
            },
            last + 1,
        )
    }

    /// A value that starts on line `j` at byte `at` (`rest` = the line from there). Returns it, the last line it
    /// uses and its trailing comment.
    fn value(
        &mut self,
        j: usize,
        parent_indent: usize,
        at: usize,
        rest: &str,
        prop: Option<&'static str>,
    ) -> (Val, usize, Option<String>) {
        let line_end = self.infos[j].end;
        let first = rest.as_bytes()[0];
        match first {
            b'|' | b'>' => {
                let header = rest.find([' ', '\t', '#']).unwrap_or(rest.len());
                let comment = trailing(&rest[header..]);
                let (val, last) = self.block_scalar(j, parent_indent, line_end);
                (val, last, comment)
            }
            b'[' | b'{' => {
                let (val, last, comment) = self.flow(j, at);
                (val, last, comment)
            }
            b'*' => {
                let end = plain_end(rest);
                let comment = trailing(&rest[end..]);
                (
                    Val::Raw {
                        text: rest[..end].to_owned(),
                        reason: "alias",
                        span: at..at + end,
                    },
                    j,
                    comment,
                )
            }
            b'"' | b'\'' => self.quoted(j, at, rest, prop),
            _ => {
                let end = plain_end(rest);
                let comment = trailing(&rest[end..]);
                let text = &rest[..end];
                // continuation lines make a multi-line scalar
                let mut last = j;
                let mut stop = at + end;
                if comment.is_none() {
                    if let Some(n) = self.next_sig(j + 1) {
                        if self.infos[n].indent > parent_indent {
                            last = self.skip_deeper(n, parent_indent).saturating_sub(1).max(n);
                            stop = self.infos[last].end;
                        }
                    }
                }
                let multi = last != j;
                let whole = if multi { &self.text[at..stop] } else { text };
                let kind = if multi {
                    ScalarKind::String
                } else {
                    kind_of_plain(whole)
                };
                let read_only = if multi { Some("multiline") } else { prop };
                (
                    Val::Scalar(Proto {
                        kind,
                        text: whole.to_owned(),
                        span: at..stop,
                        quote: Quote::Plain,
                        read_only,
                    }),
                    last,
                    comment,
                )
            }
        }
    }

    fn quoted(
        &self,
        j: usize,
        at: usize,
        rest: &str,
        prop: Option<&'static str>,
    ) -> (Val, usize, Option<String>) {
        let quote = if rest.starts_with('"') {
            Quote::Double
        } else {
            Quote::Single
        };
        let unquote = |inner: &str| {
            if quote == Quote::Double {
                unescape_double(inner)
            } else {
                inner.replace("''", "'")
            }
        };
        if let Some(close) = quoted_end(rest, 0) {
            let comment = trailing(&rest[close..]);
            return (
                Val::Scalar(Proto {
                    kind: ScalarKind::String,
                    text: unquote(&rest[1..close - 1]),
                    span: at..at + close,
                    quote,
                    read_only: prop,
                }),
                j,
                comment,
            );
        }
        // closes on a later line
        let mut n = j + 1;
        while n < self.limit && n < j + 2000 {
            let info = self.infos[n];
            let line = &self.text[info.start..info.end];
            let probe = quoted_probe(line, rest.as_bytes()[0]);
            if let Some(close) = probe {
                let end = info.start + close;
                return (
                    Val::Scalar(Proto {
                        kind: ScalarKind::String,
                        text: self.text[at + 1..end - 1].to_owned(),
                        span: at..end,
                        quote,
                        read_only: Some("multiline"),
                    }),
                    n,
                    trailing(&line[close..]),
                );
            }
            n += 1;
        }
        let end = self.infos[self.limit.saturating_sub(1).max(j)].end;
        (
            Val::Raw {
                text: self.text[at..end].to_owned(),
                reason: "multiline",
                span: at..end,
            },
            self.limit.saturating_sub(1).max(j),
            None,
        )
    }

    fn block_scalar(&self, j: usize, parent_indent: usize, header_end: usize) -> (Val, usize) {
        let mut last = j;
        let mut n = j + 1;
        while n < self.limit {
            let info = self.infos[n];
            let line = &self.text[info.start..info.end];
            if line.trim().is_empty() {
                n += 1;
                continue;
            }
            if info.indent <= parent_indent {
                break;
            }
            last = n;
            n += 1;
        }
        if last == j {
            return (
                Val::Raw {
                    text: String::new(),
                    reason: "blockScalar",
                    span: header_end..header_end,
                },
                j,
            );
        }
        let first = j + 1;
        let start = self.infos[first].start;
        let end = self.infos[last].end;
        let strip = (first..=last)
            .filter(|&k| {
                !self.text[self.infos[k].start..self.infos[k].end]
                    .trim()
                    .is_empty()
            })
            .map(|k| self.infos[k].indent)
            .min()
            .unwrap_or(0);
        let text = (first..=last)
            .map(|k| {
                let info = self.infos[k];
                let line = &self.text[info.start..info.end];
                line.get(strip.min(line.len())..).unwrap_or("")
            })
            .collect::<Vec<_>>()
            .join("\n");
        (
            Val::Raw {
                text,
                reason: "blockScalar",
                span: start..end,
            },
            last,
        )
    }

    #[allow(clippy::many_single_char_names)] // a scanner over bytes: line j, position i, byte b
    fn flow(&self, j: usize, at: usize) -> (Val, usize, Option<String>) {
        // find the matching close, across lines when it has to
        let mut depth = 0_i32;
        let mut n = j;
        let mut from = at;
        let mut quote: Option<u8> = None;
        loop {
            let info = self.infos[n];
            let line = &self.text[from..info.end];
            let bytes = line.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let b = bytes[i];
                match quote {
                    Some(q) => {
                        if b == b'\\' && q == b'"' {
                            i += 1;
                        } else if b == q {
                            quote = None;
                        }
                    }
                    None => match b {
                        b'"' | b'\'' => quote = Some(b),
                        b'[' | b'{' => depth += 1,
                        b']' | b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                let end = from + i + 1;
                                return (
                                    Val::Raw {
                                        text: self.text[at..end].to_owned(),
                                        reason: "flow",
                                        span: at..end,
                                    },
                                    n,
                                    trailing(&self.text[end..info.end]),
                                );
                            }
                        }
                        b'#' if i > 0 && matches!(bytes[i - 1], b' ' | b'\t') => break,
                        _ => {}
                    },
                }
                i += 1;
            }
            n += 1;
            if n >= self.limit || n > j + 5000 {
                break;
            }
            from = self.infos[n].start;
        }
        let last = (n - 1).max(j);
        let end = self.infos[last].end;
        (
            Val::Raw {
                text: self.text[at..end].to_owned(),
                reason: "flow",
                span: at..end,
            },
            last,
            None,
        )
    }

    fn seq(&mut self, from: usize, indent: usize, depth: usize) -> (Vec<SeqItem>, usize) {
        let mut items = Vec::new();
        let mut i = from;
        loop {
            let Some(j) = self.next_sig(i) else {
                i = self.limit;
                break;
            };
            let info = self.infos[j];
            if info.indent < indent {
                i = j;
                break;
            }
            if info.indent > indent {
                i = self.skip_deeper(j, indent);
                continue;
            }
            let content = self.content(j);
            if !is_seq_start(content) {
                i = j;
                break;
            }
            let after = &content[1..];
            let spaces = after.len() - after.trim_start_matches([' ', '\t']).len();
            let col = indent + 1 + spaces;
            let at = info.start + col;
            let rest = &self.text[at..info.end];
            let (item, next) = self.seq_item(j, indent, col, at, rest, depth);
            items.push(item);
            i = next;
        }
        (items, i)
    }

    fn seq_item(
        &mut self,
        j: usize,
        indent: usize,
        col: usize,
        at: usize,
        rest: &str,
        depth: usize,
    ) -> (SeqItem, usize) {
        let (rest_after, offset, prop) = Self::props(rest, at);
        if rest_after.is_empty() || rest_after.starts_with('#') {
            if let Some(n) = self.next_sig(j + 1) {
                if self.infos[n].indent > indent {
                    let (val, next) = self.block(n, self.infos[n].indent, depth + 1);
                    let last = next.saturating_sub(1).max(j);
                    return (SeqItem { line: j, val }, last + 1);
                }
            }
            return (
                SeqItem {
                    line: j,
                    val: Val::Scalar(Proto {
                        kind: ScalarKind::Null,
                        text: String::new(),
                        span: at..at,
                        quote: Quote::Plain,
                        read_only: Some("empty"),
                    }),
                },
                j + 1,
            );
        }
        if prop.is_none() && is_seq_start(rest_after) {
            let last = self.skip_deeper(j, indent).saturating_sub(1).max(j);
            let val = self.region(j, last, "nested");
            return (SeqItem { line: j, val }, last + 1);
        }
        if prop.is_none() && split_key(rest_after).is_some() {
            // `- key: value`: a mapping whose first line sits after the dash
            self.infos[j].indent = col;
            let (items, next) = self.map(j, col, depth + 1);
            let last = next.saturating_sub(1).max(j);
            return (
                SeqItem {
                    line: j,
                    val: Val::Map(items),
                },
                last + 1,
            );
        }
        let (val, last, _) = self.value(j, indent, offset, rest_after, prop);
        (SeqItem { line: j, val }, last + 1)
    }
}

/// A comment after a value: its text.
fn trailing(rest: &str) -> Option<String> {
    permark_core::format::config::util::trailing_comment(rest, &['#'])
}

/// Where a quoted scalar that began on an earlier line closes in `line` (byte after the quote).
fn quoted_probe(line: &str, quote: u8) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if quote == b'"' => i += 2,
            b'\'' if quote == b'\'' && bytes.get(i + 1) == Some(&b'\'') => i += 2,
            q if q == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}
