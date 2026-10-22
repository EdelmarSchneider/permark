//! Finding the destination of every link, image and reference definition of a Markdown text, as exact byte ranges.
//!
//! The parser (comrak) says where each inline link, image and HTML node sits (`sourcepos`); the destination inside
//! such a node is located by reading the node's own text. Reference definitions are not in the tree: they are found
//! by their line grammar outside code, HTML blocks and paragraphs. A destination whose text does not agree with what
//! the parser resolved is dropped: a link that is not read with certainty is never edited.

use std::ops::Range;

use comrak::nodes::{NodeValue, Sourcepos};
use comrak::{parse_document, Arena, Options};

const BOM: &str = "\u{feff}";
/// Most escaped pipes of one table row the column shift is searched for.
const SHIFT_LIMIT: usize = 16;

/// How a destination is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `(dest)` | `]: dest`
    Bare,
    /// `(<dest>)`: the range includes the angle brackets.
    Angle,
    /// An HTML attribute value (between the quotes).
    Html,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Inline,
    Image,
    Definition,
    Html,
}

/// One destination: `range` of the token as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dest {
    pub range: Range<usize>,
    pub form: Form,
    pub kind: Kind,
    /// 1-based line of the start.
    pub line: usize,
}

impl Dest {
    /// The token as written (with `<...>` when angled).
    #[must_use]
    pub fn raw<'a>(&self, text: &'a str) -> &'a str {
        &text[self.range.clone()]
    }

    /// The destination without angle brackets.
    #[must_use]
    pub fn inner<'a>(&self, text: &'a str) -> &'a str {
        let raw = self.raw(text);
        match self.form {
            Form::Angle => raw
                .strip_prefix('<')
                .and_then(|r| r.strip_suffix('>'))
                .unwrap_or(raw),
            Form::Bare | Form::Html => raw,
        }
    }
}

/// Byte offsets of line starts (`\n`, `\r\n` and a lone `\r` end a line).
struct Lines {
    starts: Vec<usize>,
}

impl Lines {
    fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut starts = vec![0];
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => starts.push(i + 1),
                b'\r' => {
                    if bytes.get(i + 1) == Some(&b'\n') {
                        i += 1;
                    }
                    starts.push(i + 1);
                }
                _ => {}
            }
            i += 1;
        }
        Self { starts }
    }

    fn start_of(&self, line: usize) -> Option<usize> {
        self.starts.get(line.checked_sub(1)?).copied()
    }

    /// Byte range of a node from its 1-based `sourcepos` (end column inclusive).
    fn range_of(&self, pos: Sourcepos, len: usize) -> Option<Range<usize>> {
        let start = self.start_of(pos.start.line)? + pos.start.column.checked_sub(1)?;
        let end = self.start_of(pos.end.line)? + pos.end.column;
        (start < end && end <= len).then_some(start..end)
    }

    /// 1-based line of a byte offset.
    fn line_of(&self, offset: usize) -> usize {
        self.starts.partition_point(|s| *s <= offset).max(1)
    }

    /// Byte range of line `line` (1-based), without its line ending.
    fn content_of(&self, line: usize, text: &str) -> Option<Range<usize>> {
        let start = self.start_of(line)?;
        let end = self.start_of(line + 1).unwrap_or(text.len());
        let mut stop = end;
        let bytes = text.as_bytes();
        while stop > start && matches!(bytes[stop - 1], b'\n' | b'\r') {
            stop -= 1;
        }
        Some(start..stop)
    }
}

fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.autolink = true;
    options.extension.description_lists = true;
    options.extension.math_dollars = true;
    options.extension.front_matter_delimiter = Some("---".to_owned());
    options
}

/// Every destination of `text`, in text order.
#[must_use]
pub fn parse(text: &str) -> Vec<Dest> {
    let offset = if text.starts_with(BOM) { BOM.len() } else { 0 };
    let body = &text[offset..];
    let lines = Lines::new(body);
    let arena = Arena::new();
    let root = parse_document(&arena, body, &options());
    let mut dests: Vec<Dest> = Vec::new();
    // line ranges where a definition cannot start: code, HTML, front matter, paragraphs
    let mut blocked: Vec<(usize, usize)> = Vec::new();
    for node in root.descendants() {
        let data = node.data();
        let pos = data.sourcepos;
        match &data.value {
            NodeValue::Link(link) | NodeValue::Image(link) => {
                let image = matches!(data.value, NodeValue::Image(_));
                if let Some(range) = lines.range_of(pos, body.len()) {
                    if let Some(dest) = shifted_inline_dest(body, &range, &link.url) {
                        dests.push(with_kind(
                            dest,
                            if image { Kind::Image } else { Kind::Inline },
                        ));
                    }
                }
            }
            NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) => {
                if let Some(range) = lines.range_of(pos, body.len()) {
                    html_dests(&body[range.clone()], range.start, &mut dests);
                }
                if matches!(data.value, NodeValue::HtmlBlock(_)) {
                    blocked.push((pos.start.line, pos.end.line));
                }
            }
            NodeValue::CodeBlock(_) | NodeValue::FrontMatter(_) | NodeValue::Paragraph => {
                blocked.push((pos.start.line, pos.end.line));
            }
            _ => {}
        }
    }
    definitions(body, &lines, &blocked, &mut dests);
    dests.sort_by_key(|d| d.range.start);
    dests.dedup_by(|a, b| a.range == b.range);
    for dest in &mut dests {
        dest.range = dest.range.start + offset..dest.range.end + offset;
        dest.line = lines.line_of(dest.range.start - offset);
    }
    dests
}

fn with_kind(mut dest: Dest, kind: Kind) -> Dest {
    dest.kind = kind;
    dest
}

// ---- inline links ------------------------------------------------------------------------------------------------

/// [`inline_dest`] on the node text; when the parser's columns are off to the left (inside a table cell the parser
/// counts an escaped pipe `\|` as one character), the same text a few bytes to the right.
fn shifted_inline_dest(body: &str, range: &Range<usize>, url: &str) -> Option<Dest> {
    let line_end = body[range.start..]
        .find(['\n', '\r'])
        .map_or(body.len(), |at| range.start + at);
    for shift in 0..=SHIFT_LIMIT {
        let (start, end) = (range.start + shift, range.end + shift);
        if end > line_end || !body.is_char_boundary(start) || !body.is_char_boundary(end) {
            break;
        }
        if let Some(dest) = inline_dest(&body[start..end], url, start) {
            return Some(dest);
        }
    }
    None
}

/// Backslash escapes (`\)`) undone; a text with an entity (`&amp;`) is returned as is, flagged.
fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.peek() {
                if next.is_ascii_punctuation() {
                    out.push(*next);
                    chars.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

/// Finds `(dest "title")` at the end of a link | image node's text. `base`: offset of the node text in the document.
fn inline_dest(node: &str, url: &str, base: usize) -> Option<Dest> {
    if !(node.starts_with('[') || node.starts_with("![")) {
        return None; // reference link, autolink, wikilink: no destination here
    }
    let mut from = 0;
    while let Some(at) = node[from..].find("](") {
        let open = from + at + 2;
        from = from + at + 1;
        if let Some((range, form)) = destination_at(node, open) {
            let raw = &node[range.clone()];
            let inner = match form {
                Form::Angle => &raw[1..raw.len() - 1],
                _ => raw,
            };
            // entities (`&amp;`) are decoded by the parser, not here: those are compared by prefix only
            let agrees = if inner.contains('&') {
                true
            } else {
                unescape(inner) == url
            };
            if agrees && !inner.is_empty() {
                return Some(Dest {
                    range: base + range.start..base + range.end,
                    form,
                    kind: Kind::Inline,
                    line: 0,
                });
            }
        }
    }
    None
}

/// Parses `dest "title")` starting at `open` (just after `](`) and requires it to end the node text.
fn destination_at(node: &str, open: usize) -> Option<(Range<usize>, Form)> {
    let bytes = node.as_bytes();
    let mut i = skip_space(bytes, open);
    let start = i;
    let form = if bytes.get(i) == Some(&b'<') {
        i += 1;
        loop {
            match *bytes.get(i)? {
                b'\\' => i += 2,
                b'>' => {
                    i += 1;
                    break;
                }
                b'<' | b'\n' | b'\r' => return None,
                _ => i += 1,
            }
        }
        Form::Angle
    } else {
        let mut depth = 0_usize;
        loop {
            match bytes.get(i) {
                Some(b'\\') => i += 2,
                Some(b'(') => {
                    depth += 1;
                    i += 1;
                }
                Some(b')') => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    i += 1;
                }
                Some(b) if b.is_ascii_whitespace() || b.is_ascii_control() => break,
                Some(_) => i += 1,
                None => return None,
            }
        }
        Form::Bare
    };
    let end = i.min(node.len());
    let mut j = skip_space(bytes, end);
    // optional title
    if let Some(quote) = bytes
        .get(j)
        .copied()
        .filter(|b| matches!(b, b'"' | b'\'' | b'('))
    {
        if j == end {
            return None; // a title needs white space before it
        }
        let close = if quote == b'(' { b')' } else { quote };
        j += 1;
        loop {
            match *bytes.get(j)? {
                b'\\' => j += 2,
                b if b == close => {
                    j += 1;
                    break;
                }
                _ => j += 1,
            }
        }
        j = skip_space(bytes, j);
    }
    // the node ends with the closing parenthesis
    (bytes.get(j) == Some(&b')') && j + 1 == node.len() && end > start)
        .then_some((start..end, form))
}

fn skip_space(bytes: &[u8], mut i: usize) -> usize {
    while bytes
        .get(i)
        .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
    {
        i += 1;
    }
    i
}

// ---- HTML --------------------------------------------------------------------------------------------------------

const HTML_ATTRIBUTES: [&str; 4] = ["href", "src", "poster", "data"];

/// `href | src | poster | data` attribute values of the tags in `html` (comments skipped).
fn html_dests(html: &str, base: usize, out: &mut Vec<Dest>) {
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // `i` walks one byte at a time, so it can land inside a multibyte character: the
        // comment markers are matched on the bytes and the `&str` is never sliced here.
        if bytes[i..].starts_with(b"<!--") {
            i = bytes[i..]
                .windows(3)
                .position(|w| w == b"-->")
                .map_or(bytes.len(), |e| i + e + 3);
            continue;
        }
        if bytes[i] == b'<' && bytes.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            i = tag_attributes(html, i, base, out);
            continue;
        }
        i += 1;
    }
}

/// Reads one tag from `start`; returns the offset after it.
fn tag_attributes(html: &str, start: usize, base: usize, out: &mut Vec<Dest>) -> usize {
    let bytes = html.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' && bytes[i] != b'/'
    {
        i += 1;
    }
    loop {
        i = skip_space(bytes, i);
        match bytes.get(i) {
            None => return bytes.len(),
            Some(b'>') => return i + 1,
            Some(b'/') => {
                i += 1;
                continue;
            }
            _ => {}
        }
        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'=' | b'>' | b'/')
        {
            i += 1;
        }
        let name = html[name_start..i].to_ascii_lowercase();
        let after_name = skip_space(bytes, i);
        if bytes.get(after_name) != Some(&b'=') {
            i = after_name.max(i + usize::from(i == name_start));
            continue;
        }
        let value_at = skip_space(bytes, after_name + 1);
        let (value, next) = match bytes.get(value_at) {
            Some(&q) if q == b'"' || q == b'\'' => {
                let begin = value_at + 1;
                let Some(len) = html[begin..].find(char::from(q)) else {
                    return bytes.len();
                };
                (begin..begin + len, begin + len + 1)
            }
            Some(_) => {
                let mut end = value_at;
                while end < bytes.len() && !bytes[end].is_ascii_whitespace() && bytes[end] != b'>' {
                    end += 1;
                }
                (value_at..end, end)
            }
            None => return bytes.len(),
        };
        if HTML_ATTRIBUTES.contains(&name.as_str()) && !value.is_empty() {
            out.push(Dest {
                range: base + value.start..base + value.end,
                form: Form::Html,
                kind: Kind::Html,
                line: 0,
            });
        }
        i = next;
    }
}

// ---- reference definitions ---------------------------------------------------------------------------------------

/// Skips the container prefix of a line: up to 3 spaces, `>` markers, one list marker.
fn container_prefix(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut i = 0;
    loop {
        let mut spaces = 0;
        while bytes.get(i + spaces) == Some(&b' ') && spaces < 4 {
            spaces += 1;
        }
        if bytes.get(i + spaces) == Some(&b'>') {
            i += spaces + 1;
            continue;
        }
        i += spaces.min(3);
        break;
    }
    // one list marker: `-`, `*`, `+`, `1.`, `1)`
    let rest = &line[i..];
    let marker = rest
        .chars()
        .next()
        .filter(|c| matches!(c, '-' | '*' | '+'))
        .map(|_| 1)
        .or_else(|| {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            (digits > 0 && matches!(rest.as_bytes().get(digits), Some(b'.' | b')')))
                .then_some(digits + 1)
        });
    if let Some(len) = marker {
        if rest.as_bytes().get(len) == Some(&b' ') {
            let mut j = i + len;
            while bytes.get(j) == Some(&b' ') {
                j += 1;
            }
            return j;
        }
    }
    i
}

/// `[label]: dest "title"` on one line: the destination's range inside the line.
fn definition_in(line: &str) -> Option<(Range<usize>, Form)> {
    let at = container_prefix(line);
    let rest = &line[at..];
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'[') || bytes.get(1) == Some(&b'^') {
        return None;
    }
    let mut i = 1;
    loop {
        match *bytes.get(i)? {
            b'\\' => i += 2,
            b']' => break,
            b'[' => return None,
            _ => i += 1,
        }
    }
    if i == 1 || bytes.get(i + 1) != Some(&b':') {
        return None;
    }
    i += 2;
    let mut j = i;
    while matches!(bytes.get(j), Some(b' ' | b'\t')) {
        j += 1;
    }
    if j == i || j >= bytes.len() {
        return None;
    }
    let start = j;
    let form = if bytes[j] == b'<' {
        j += 1;
        loop {
            match *bytes.get(j)? {
                b'\\' => j += 2,
                b'>' => {
                    j += 1;
                    break;
                }
                b'<' => return None,
                _ => j += 1,
            }
        }
        Form::Angle
    } else {
        while j < bytes.len() && !bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        Form::Bare
    };
    let end = j;
    // optional title, then nothing else
    let mut k = end;
    while matches!(bytes.get(k), Some(b' ' | b'\t')) {
        k += 1;
    }
    if k < bytes.len() {
        if k == end {
            return None;
        }
        let quote = bytes[k];
        let close = match quote {
            b'"' | b'\'' => quote,
            b'(' => b')',
            _ => return None,
        };
        k += 1;
        loop {
            match *bytes.get(k)? {
                b'\\' => k += 2,
                b if b == close => {
                    k += 1;
                    break;
                }
                _ => k += 1,
            }
        }
        while matches!(bytes.get(k), Some(b' ' | b'\t')) {
            k += 1;
        }
        if k < bytes.len() {
            return None;
        }
    }
    (end > start).then_some((at + start..at + end, form))
}

fn definitions(body: &str, lines: &Lines, blocked: &[(usize, usize)], out: &mut Vec<Dest>) {
    for line_no in 1..=lines.starts.len() {
        let Some(content) = lines.content_of(line_no, body) else {
            continue;
        };
        let line = &body[content.clone()];
        if !line.contains("]:") || blocked.iter().any(|(a, b)| (*a..=*b).contains(&line_no)) {
            continue;
        }
        if let Some((range, form)) = definition_in(line) {
            out.push(Dest {
                range: content.start + range.start..content.start + range.end,
                form,
                kind: Kind::Definition,
                line: 0,
            });
        }
    }
}
