//! Finding the destination of every link, image and reference definition of a Markdown text, as exact byte ranges.
//!
//! The parser (comrak) says where each inline link, image and HTML node sits (`sourcepos`); the destination inside
//! such a node is located by reading the node's own text. Reference definitions are not in the tree: they are found
//! by their grammar outside code, HTML blocks, tables and paragraph | heading text (definitions at the start of a
//! paragraph are taken off it by the parser, which then reports the positions of the text left too high: see
//! `AfterDefinitions`). A definition read over several lines is kept only when the parser defines its label the same
//! way (see `confirmed`). A destination whose text does not agree with what the parser resolved is dropped: a link
//! that is not read with certainty is never edited.
//!
//! With the prompt features on, the parser reads the text reading parses (prompt tags marked, front matter cut), so a
//! link inside a prompt block is a link even with no blank line after its tag. Only whole lines differ there: each
//! destination is placed back on its line and column, and one whose bytes do not match the document's is dropped.

use std::borrow::Cow;
use std::ops::Range;

use comrak::nodes::{AstNode, LineColumn, NodeHeading, NodeValue, Sourcepos};
use comrak::{parse_document, Arena, Options};

use crate::format::{reading_text, MarkdownFeatures};

const BOM: &str = "\u{feff}";
/// Most escaped pipes of one table row the column shift is searched for.
const SHIFT_LIMIT: usize = 16;
/// Longest link label, in bytes, and deepest parentheses of a bare destination the parser accepts.
const MAX_LABEL: usize = 1000;
const MAX_PARENS: usize = 32;

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
pub(crate) struct Lines {
    starts: Vec<usize>,
}

impl Lines {
    pub(crate) fn new(text: &str) -> Self {
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

    /// Byte range in `text` of a node from its 1-based `sourcepos` (end column inclusive); `None` when it does not
    /// fall on characters of `text`.
    pub(crate) fn range_of(&self, pos: Sourcepos, text: &str) -> Option<Range<usize>> {
        let start = self.start_of(pos.start.line)? + pos.start.column.checked_sub(1)?;
        let end = self.start_of(pos.end.line)? + pos.end.column;
        within(text, start, end)
    }

    /// 1-based line of a byte offset.
    pub(crate) fn line_of(&self, offset: usize) -> usize {
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

/// `start..end` when it is a non-empty range of whole characters of `text`.
fn within(text: &str, start: usize, end: usize) -> Option<Range<usize>> {
    (start < end && text.is_char_boundary(start) && text.is_char_boundary(end))
        .then_some(start..end)
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

/// Every destination of `text`, in text order, as reading sees the links with `features` on.
#[must_use]
pub fn parse(text: &str, features: &MarkdownFeatures) -> Vec<Dest> {
    let offset = if text.starts_with(BOM) { BOM.len() } else { 0 };
    let body = &text[offset..];
    let lines = Lines::new(body);
    let source = match features.prompt_blocks {
        Some(_) => reading_text(body, features),
        None => Cow::Borrowed(body),
    };
    let mut dests = match source {
        Cow::Owned(source) => {
            placed_back(&source, body, &lines).unwrap_or_else(|| scan(body, &lines))
        }
        Cow::Borrowed(_) => scan(body, &lines),
    };
    for dest in &mut dests {
        dest.line = lines.line_of(dest.range.start);
        dest.range = dest.range.start + offset..dest.range.end + offset;
    }
    dests
}

/// Whether [`parse`] finds the same destinations in every text with `a` as with `b`: the prompt features change the
/// text parsed, and whether a front matter card is on changes how the front matter is cut from it.
#[must_use]
pub fn same_links(a: &MarkdownFeatures, b: &MarkdownFeatures) -> bool {
    a.prompt_blocks == b.prompt_blocks
        && a.front_matter_card.is_some() == b.front_matter_card.is_some()
}

/// The destinations of `source` (`body` with whole lines replaced) at their places in `body`: the same line, the same
/// column. `None` when a replaced line lost a line break (the lines no longer pair up).
fn placed_back(source: &str, body: &str, lines: &Lines) -> Option<Vec<Dest>> {
    let own = Lines::new(source);
    if own.starts.len() != lines.starts.len() {
        return None;
    }
    let place = |at: usize| -> Option<usize> {
        let line = own.line_of(at);
        Some(at.checked_sub(own.start_of(line)?)? + lines.start_of(line)?)
    };
    let dests = scan(source, &own)
        .into_iter()
        .filter_map(|mut dest| {
            let start = place(dest.range.start)?;
            let end = place(dest.range.end.checked_sub(1)?)? + 1;
            (body.get(start..end)? == source.get(dest.range.clone())?).then(|| {
                dest.range = start..end;
                dest
            })
        })
        .collect();
    Some(dests)
}

/// Every destination of `body` (no byte order mark; `lines` are its lines), in text order; lines are left unset.
fn scan(body: &str, lines: &Lines) -> Vec<Dest> {
    let arena = Arena::new();
    let root = parse_document(&arena, body, &options());
    let mut dests: Vec<Dest> = Vec::new();
    // line ranges where a definition cannot start: code, HTML, front matter, paragraph text
    let mut blocked: Vec<(usize, usize)> = Vec::new();
    // the last paragraph seen, when it began with definitions: its inline nodes follow it in tree order
    let mut raised: Option<AfterDefinitions> = None;
    for node in root.descendants() {
        let data = node.data();
        let pos = data.sourcepos;
        let moved = raised.as_ref().filter(|p| p.holds(pos.start.line));
        let range = match moved {
            Some(paragraph) => paragraph.range_of(pos, lines, body),
            None => lines.range_of(pos, body),
        };
        let moved = moved.is_some();
        match &data.value {
            NodeValue::Link(link) | NodeValue::Image(link) => {
                let image = matches!(data.value, NodeValue::Image(_));
                if let Some(range) = range {
                    if let Some(dest) = shifted_inline_dest(body, &range, &link.url) {
                        dests.push(with_kind(
                            dest,
                            if image { Kind::Image } else { Kind::Inline },
                        ));
                    }
                }
            }
            NodeValue::HtmlInline(html) => {
                // placed again: kept only where the bytes are the parser's
                if let Some(range) =
                    range.filter(|r| !moved || body.get(r.clone()) == Some(html.as_str()))
                {
                    html_dests(&body[range.clone()], range.start, &mut dests);
                }
            }
            NodeValue::HtmlBlock(_) => {
                if let Some(range) = range {
                    html_dests(&body[range.clone()], range.start, &mut dests);
                }
                blocked.push((pos.start.line, pos.end.line));
            }
            NodeValue::CodeBlock(_) | NodeValue::FrontMatter(_) | NodeValue::Table(_) => {
                blocked.push((pos.start.line, pos.end.line));
            }
            NodeValue::Paragraph | NodeValue::Heading(NodeHeading { setext: true, .. }) => {
                // the last line of a setext heading is its underline
                let text_end = match data.value {
                    NodeValue::Paragraph => pos.end.line,
                    _ => pos.end.line.saturating_sub(1),
                };
                raised = AfterDefinitions::of(node, pos.start, text_end, body, lines);
                let taken = raised.as_ref().map_or(0, |p| p.taken);
                blocked.push((pos.start.line + taken, pos.end.line));
            }
            _ => {}
        }
    }
    definitions(body, lines, &blocked, &mut dests);
    dests.sort_by_key(|d| d.range.start);
    dests.dedup_by(|a, b| a.range == b.range);
    dests
}

/// A paragraph | setext heading that begins with reference definitions. The parser takes them off but keeps the first
/// line as the start of the text left, so it reports each inline position `taken` lines too high, with its column
/// counted from where the text of that higher line starts. The definitions take whole lines, so the text left starts
/// on line `first + taken`.
struct AfterDefinitions {
    /// First line of the paragraph: the first definition.
    first: usize,
    /// Lines the definitions take.
    taken: usize,
    /// Byte column where the paragraph text starts, for each of its lines.
    starts: Vec<usize>,
}

impl AfterDefinitions {
    /// `None` when the paragraph | setext heading `node` (from `start`, its text ending on line `text_end`) kept all
    /// of its lines.
    fn of<'a>(
        node: &'a AstNode<'a>,
        start: LineColumn,
        text_end: usize,
        body: &str,
        lines: &Lines,
    ) -> Option<Self> {
        let first = start.line;
        let column = start.column.checked_sub(1)?;
        if body.as_bytes().get(lines.start_of(first)? + column) != Some(&b'[') {
            return None; // a definition starts with its label
        }
        // the text left ends on the last line of text: the lines it reports less are the definitions'
        let last = node
            .descendants()
            .skip(1)
            .map(|n| n.data().sourcepos.end.line)
            .max()?;
        let taken = text_end.checked_sub(last)?;
        if taken == 0 || taken > text_end.saturating_sub(first) {
            return None;
        }
        // the lines the parser took off must read here as definitions, back to back: else what they hold is not known
        // for sure, and nothing of the paragraph is placed or taken as a definition
        let mut line = first;
        while line < first + taken {
            line = definition_at(body, lines, line, first + taken - 1)?.last + 1;
        }
        // before the text of the other lines: indentation, and quote marks inside a container
        let top = node
            .parent()
            .is_some_and(|p| matches!(p.data().value, NodeValue::Document));
        let marks: &[u8] = if top { b" \t" } else { b" \t>" };
        let starts = (first + 1..=text_end)
            .map(|line| {
                let content = lines.content_of(line, body)?;
                Some(
                    body.as_bytes()[content]
                        .iter()
                        .take_while(|b| marks.contains(b))
                        .count(),
                )
            })
            .collect::<Option<Vec<usize>>>()?;
        Some(Self {
            first,
            taken,
            starts: std::iter::once(column).chain(starts).collect(),
        })
    }

    /// Whether a position the parser reports on `line` is in this paragraph's text.
    fn holds(&self, line: usize) -> bool {
        (self.first..self.first + self.starts.len() - self.taken).contains(&line)
    }

    /// Byte offset of a reported position: 1-based `line`, `column` bytes from the line start.
    fn place(&self, line: usize, column: usize, lines: &Lines) -> Option<usize> {
        let row = line.checked_sub(self.first)?;
        let at = column.checked_sub(*self.starts.get(row)?)?;
        let real = row + self.taken;
        Some(lines.start_of(self.first + real)? + self.starts.get(real)? + at)
    }

    /// [`Lines::range_of`] for a node of this paragraph's text. The parser counts a tab as spaces, so a place found
    /// from a line that holds one can fall inside a character: such a node is left out.
    fn range_of(&self, pos: Sourcepos, lines: &Lines, text: &str) -> Option<Range<usize>> {
        let start = self.place(pos.start.line, pos.start.column.checked_sub(1)?, lines)?;
        let end = self.place(pos.end.line, pos.end.column, lines)?;
        within(text, start, end)
    }
}

fn with_kind(mut dest: Dest, kind: Kind) -> Dest {
    dest.kind = kind;
    dest
}

// ---- inline links ------------------------------------------------------------------------------------------------

/// [`inline_dest`] on the node text; when the parser's columns are off to the left (inside a table cell the parser
/// counts an escaped pipe `\|` as one character), the same text a few bytes to the right.
fn shifted_inline_dest(body: &str, range: &Range<usize>, url: &str) -> Option<Dest> {
    let line_end = body
        .get(range.start..)?
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

/// Whether a destination as written (`inner`: without angle brackets) is the one the parser resolved, `url`. Entities
/// (`&amp;`) are decoded by the parser, not here: a destination with one is taken as it is written.
fn agrees(inner: &str, url: &str) -> bool {
    inner.contains('&') || unescape(inner) == url
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
            if agrees(inner, url) && !inner.is_empty() {
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

/// A reference definition: its destination, its label (inside the brackets) and its last line.
struct Definition {
    dest: Dest,
    label: Range<usize>,
    last: usize,
    /// It is one only through what follows its first line (destination on the next line, a title from the
    /// destination's line closing on a later one): the parser's own definitions confirm it (see [`confirmed`]).
    spans: bool,
}

/// A place in the text of a definition: byte `pos` of line `line`, whose text ends at byte `end`. The definition may
/// go on over the lines after it up to line `limit`.
#[derive(Clone, Copy)]
struct Cursor<'t> {
    body: &'t str,
    lines: &'t Lines,
    line: usize,
    pos: usize,
    end: usize,
    limit: usize,
}

impl Cursor<'_> {
    fn byte(&self, ahead: usize) -> Option<u8> {
        let at = self.pos + ahead;
        (at < self.end).then(|| self.body.as_bytes()[at])
    }

    fn peek(&self) -> Option<u8> {
        self.byte(0)
    }

    fn escaped(&self) -> bool {
        self.peek() == Some(b'\\') && self.byte(1).is_some_and(|b| b.is_ascii_punctuation())
    }

    fn at_end(&self) -> bool {
        self.pos >= self.end
    }

    fn spaces(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    /// Goes to the text of the next line (after its quote marks and indentation) when the definition can go on
    /// there: up to `limit`, and not over a blank line (it ends the paragraph).
    fn next_line(&mut self) -> bool {
        if self.line >= self.limit {
            return false;
        }
        let Some(content) = self.lines.content_of(self.line + 1, self.body) else {
            return false;
        };
        let bytes = self.body.as_bytes();
        let mut pos = content.start;
        while pos < content.end && matches!(bytes[pos], b' ' | b'\t' | b'>') {
            pos += 1;
        }
        if pos == content.end {
            return false;
        }
        (self.line, self.pos, self.end) = (self.line + 1, pos, content.end);
        true
    }

    /// White space with at most one line break in it.
    fn space_and_break(&mut self) {
        self.spaces();
        if self.at_end() && self.next_line() {
            self.spaces();
        }
    }

    /// A link destination, `<...>` (on one line) | bare (parentheses balanced); the cursor goes past it.
    fn destination(&mut self) -> Option<Form> {
        let start = self.pos;
        if self.peek() == Some(b'<') {
            self.pos += 1;
            loop {
                match self.peek()? {
                    b'>' => {
                        self.pos += 1;
                        return Some(Form::Angle);
                    }
                    b'\\' => self.pos += 2,
                    b'<' => return None,
                    _ => self.pos += 1,
                }
            }
        }
        let mut depth = 0_usize;
        while let Some(b) = self.peek() {
            match b {
                b'\\' if self.escaped() => self.pos += 2,
                b'(' => {
                    depth += 1;
                    if depth > MAX_PARENS {
                        return None;
                    }
                    self.pos += 1;
                }
                b')' if depth == 0 => break,
                b')' => {
                    depth -= 1;
                    self.pos += 1;
                }
                b if b.is_ascii_whitespace() || (b.is_ascii_control() && b != 0) => break,
                _ => self.pos += 1,
            }
        }
        (self.pos > start && depth == 0).then_some(Form::Bare)
    }

    /// A link title, `"..."` | `'...'` | `(...)`, over lines too; the cursor goes past it.
    fn title(&mut self) -> bool {
        let close = match self.peek() {
            Some(b'"') => b'"',
            Some(b'\'') => b'\'',
            Some(b'(') => b')',
            _ => return false,
        };
        self.pos += 1;
        loop {
            match self.peek() {
                None if self.next_line() => {}
                None => return false,
                Some(b'\\') if self.escaped() => self.pos += 2,
                Some(b) if b == close => {
                    self.pos += 1;
                    return true;
                }
                Some(b'(') if close == b')' => return false,
                Some(_) => self.pos += 1,
            }
        }
    }
}

/// The reference definition starting on line `first`, read as the parser reads one (CommonMark): `[label]:`, the
/// destination on that line or the next, an optional title after white space on the destination's line or the next
/// one, then nothing else on its line. Lines after `first` are read up to line `limit`.
fn definition_at(body: &str, lines: &Lines, first: usize, limit: usize) -> Option<Definition> {
    let content = lines.content_of(first, body)?;
    let mut at = Cursor {
        body,
        lines,
        line: first,
        pos: content.start + container_prefix(&body[content.clone()]),
        end: content.end,
        limit,
    };
    // the label: on its line, no bracket in it unless escaped, not blank, not a footnote's
    if at.peek() != Some(b'[') || at.byte(1) == Some(b'^') {
        return None;
    }
    at.pos += 1;
    let label_start = at.pos;
    loop {
        match at.peek()? {
            b']' => break,
            b'[' => return None,
            b'\\' if at.escaped() => at.pos += 2,
            _ => at.pos += 1,
        }
        if at.pos - label_start > MAX_LABEL {
            return None;
        }
    }
    let label = label_start..at.pos;
    if body[label.clone()].trim_ascii().is_empty() || at.byte(1) != Some(b':') {
        return None;
    }
    at.pos += 2;
    at.space_and_break();
    let dest_line = at.line;
    let start = at.pos;
    let form = at.destination()?;
    let dest = Dest {
        range: start..at.pos,
        form,
        kind: Kind::Definition,
        line: 0,
    };
    // a title needs white space before it and nothing after it on its line; else the destination ends its line
    let mut titled = at;
    titled.space_and_break();
    let title_line = titled.line;
    let spaced = (titled.line, titled.pos) != (at.line, at.pos);
    let end = if spaced && titled.title() && {
        titled.spaces();
        titled.at_end()
    } {
        titled
    } else {
        at.spaces();
        if !at.at_end() {
            return None;
        }
        at
    };
    Some(Definition {
        dest,
        label,
        last: end.line,
        spans: dest_line > first || (title_line == dest_line && end.line > dest_line),
    })
}

fn definitions(body: &str, lines: &Lines, blocked: &[(usize, usize)], out: &mut Vec<Dest>) {
    let count = lines.starts.len();
    let mut spanning = Vec::new();
    let mut line = 1;
    while line <= count {
        let free = !blocked.iter().any(|(a, b)| (*a..=*b).contains(&line));
        let found = lines
            .content_of(line, body)
            .filter(|content| free && body[content.clone()].contains("]:"))
            .and_then(|_| {
                // a definition goes on up to the next line where none can be
                let limit = blocked
                    .iter()
                    .map(|(a, _)| *a)
                    .filter(|a| *a > line)
                    .min()
                    .map_or(count, |a| a - 1);
                definition_at(body, lines, line, limit)
            });
        match found {
            Some(definition) => {
                line = definition.last + 1;
                if definition.spans {
                    spanning.push(definition);
                } else {
                    out.push(definition.dest);
                }
            }
            None => line += 1,
        }
    }
    if !spanning.is_empty() {
        let confirmed = confirmed(body, &spanning);
        out.extend(
            spanning
                .into_iter()
                .zip(confirmed)
                .filter_map(|(definition, ok)| ok.then_some(definition.dest)),
        );
    }
}

/// Whether the parser defines the label of each of `asked` with that destination. Lines that read as a definition
/// over several lines can be something else to it (a setext heading, a table, a paragraph a block cuts short): a
/// reference to each label, added after the text, is resolved by the parser through its own definitions.
fn confirmed(body: &str, asked: &[Definition]) -> Vec<bool> {
    let mut probe = String::with_capacity(body.len() + 2 + asked.len() * 8);
    probe.push_str(body);
    probe.push_str("\n\n");
    // one reference per line from here
    let first = Lines::new(&probe).starts.len();
    for definition in asked {
        probe.push_str("[r][");
        probe.push_str(&body[definition.label.clone()]);
        probe.push_str("]\n");
    }
    let arena = Arena::new();
    let root = parse_document(&arena, &probe, &options());
    let mut found = vec![false; asked.len()];
    for node in root.descendants() {
        let data = node.data();
        let NodeValue::Link(link) = &data.value else {
            continue;
        };
        let Some(at) = data.sourcepos.start.line.checked_sub(first) else {
            continue;
        };
        if let (Some(definition), Some(ok)) = (asked.get(at), found.get_mut(at)) {
            *ok = agrees(definition.dest.inner(body), &link.url);
        }
    }
    found
}
