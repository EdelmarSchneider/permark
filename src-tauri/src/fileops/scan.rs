//! Finding the link destinations of a Markdown source, as exact byte ranges of the text itself.
//!
//! Rewriting a link after a file moved must change the destination and nothing else, so the planner needs
//! to know where every destination starts and ends. comrak's inline source positions are not reliable for
//! that (they drift after reference definitions and in nested containers), so comrak is only asked for the
//! *block* structure (which lines are code, front matter, HTML, paragraphs, headings, tables) and the
//! inline syntax is scanned here, following the `CommonMark` rules for what makes a link: backslash escapes,
//! code spans, autolinks and raw HTML are skipped, `[text](dest "title")` and `![alt](dest)` give an inline
//! destination, `[label]: dest "title"` a definition, and `href` / `src` attributes of raw HTML a third kind.
//! Reference links (`[text][label]`) have no destination of their own: the definition they point at is
//! rewritten instead.
//!
//! Anything the scanner is not sure about is left alone: a missed link is a link that stays as it was,
//! never a document that got damaged.
//!
//! The same scan tells [`crate::refs`] which images the documents use.

use std::borrow::Cow;
use std::collections::HashSet;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena};

use crate::edit::blocks::{split_lines, Line, BOM};
use crate::render::markdown::{
    is_deeper_than, options, too_many_container_markers, MAX_TREE_DEPTH,
};
use crate::render::{normalize_newlines, MAX_RENDER_BYTES};
use crate::util::on_parser_stack;

/// How a destination is written in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    /// `dest` as it stands.
    Bare,
    /// `<dest>`: the range includes the angle brackets.
    Angle,
    /// The value of an HTML attribute: the range is the text between the quotes (or the whole unquoted value).
    Html,
}

/// Where a destination comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `[text](dest)` or `![alt](dest)`.
    Inline,
    /// `[label]: dest`.
    Definition,
    /// `href` or `src` of a raw HTML tag.
    Html,
}

/// The destination of one link, image or definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dest {
    /// First byte of the destination token (the `<` of an angle-bracketed one).
    pub start: usize,
    /// One past its last byte (past the `>`).
    pub end: usize,
    pub form: Form,
    pub kind: Kind,
}

impl Dest {
    /// The token as written, brackets included.
    pub(crate) fn raw<'a>(&self, text: &'a str) -> &'a str {
        &text[self.start..self.end]
    }

    /// The destination without its angle brackets.
    pub(crate) fn inner<'a>(&self, text: &'a str) -> &'a str {
        let raw = self.raw(text);
        if self.form == Form::Angle {
            raw.strip_prefix('<')
                .and_then(|r| r.strip_suffix('>'))
                .unwrap_or(raw)
        } else {
            raw
        }
    }
}

/// A destination with backslash escapes of ASCII punctuation removed: the text a reader's link resolver
/// would see. `docs\guide.md` has no escape in it (`\g` is a literal backslash), `docs\_x.md` does.
pub(crate) fn unescape(raw: &str) -> Cow<'_, str> {
    if !raw.contains('\\') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some_and(char::is_ascii_punctuation) {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// What a line of the source is, as far as links are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    /// Not claimed by any block that was looked at.
    Free,
    /// Code, front matter: never scanned.
    Skip,
    Paragraph,
    Heading,
    Table,
    Html,
}

/// A run of lines scanned together (or, for tables, cell by cell).
#[derive(Debug, Clone, Copy)]
struct Region {
    kind: LineKind,
    /// 0-based, inclusive.
    first: usize,
    last: usize,
}

/// A reference definition found at the start of a line.
struct Definition {
    label: String,
    dest: Dest,
}

/// Splits a destination at the first `?` or `#`: the path and the rest (kept as written).
pub(crate) fn split_suffix(raw: &str) -> (&str, &str) {
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if b.get(i + 1).is_some_and(u8::is_ascii_punctuation) => i += 2,
            b'?' | b'#' => return raw.split_at(i),
            _ => i += 1,
        }
    }
    (raw, "")
}

/// The destinations of `text`, in the order they appear. Empty for a text too large or too deeply nested
/// to be parsed safely.
pub(crate) fn find_destinations(text: &str) -> Vec<Dest> {
    if text.len() as u64 > MAX_RENDER_BYTES {
        return Vec::new();
    }
    on_parser_stack(|| scan_document(text))
}

/// [`find_destinations`] on the calling thread, for a caller that already runs on a parser thread (the
/// search index worker does) and scans one document after another.
pub(crate) fn find_destinations_here(text: &str) -> Vec<Dest> {
    if text.len() as u64 > MAX_RENDER_BYTES {
        return Vec::new();
    }
    scan_document(text)
}

fn scan_document(text: &str) -> Vec<Dest> {
    let body_start = if text.starts_with(BOM) {
        BOM.len_utf8()
    } else {
        0
    };
    let (lines, _) = split_lines(text, body_start);
    let normalized = normalize_newlines(&text[body_start..]);
    if too_many_container_markers(&normalized) {
        return Vec::new();
    }
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, &normalized, &opts);
    if is_deeper_than(root, MAX_TREE_DEPTH) {
        return Vec::new();
    }
    let regions = regions_of(root, &lines, text);

    // Definitions first: whether `[x]` is a link depends on a definition that may come later.
    let mut defs: Vec<Definition> = Vec::new();
    let mut inline_spans: Vec<(usize, usize)> = Vec::new();
    let mut html_spans: Vec<(usize, usize)> = Vec::new();
    let mut tables: Vec<(usize, usize)> = Vec::new();
    for region in &regions {
        let (lo, hi) = (lines[region.first].start, lines[region.last].end);
        match region.kind {
            LineKind::Paragraph => {
                // Leading definitions are consumed by the parser, line by line.
                let mut line = region.first;
                while line <= region.last {
                    let l = lines[line];
                    match parse_definition(text, l.start, l.end, line == region.first) {
                        Some(def) => {
                            defs.push(def);
                            line += 1;
                        }
                        None => break,
                    }
                }
                if line <= region.last {
                    inline_spans.push((lines[line].start, hi));
                }
            }
            LineKind::Free => {
                // A line outside every block: a definition (its paragraph vanished), or loose text.
                match parse_definition(text, lo, hi, true) {
                    Some(def) => defs.push(def),
                    None => inline_spans.push((lo, hi)),
                }
            }
            LineKind::Heading => inline_spans.push((lo, hi)),
            LineKind::Table => {
                for line in &lines[region.first..=region.last] {
                    tables.push((line.start, line.end));
                }
            }
            LineKind::Html => html_spans.push((lo, hi)),
            LineKind::Skip => {}
        }
    }
    let defined: HashSet<String> = defs.iter().map(|d| d.label.clone()).collect();

    let mut out: Vec<Dest> = defs.into_iter().map(|d| d.dest).collect();
    for (lo, hi) in inline_spans {
        scan_inline(text, lo, hi, &defined, &mut out);
    }
    for (lo, hi) in tables {
        for (cell_lo, cell_hi) in table_cells(text, lo, hi) {
            scan_inline(text, cell_lo, cell_hi, &defined, &mut out);
        }
    }
    for (lo, hi) in html_spans {
        scan_html_block(text, lo, hi, &mut out);
    }
    out.sort_by_key(|d| d.start);
    out.dedup_by_key(|d| d.start);
    out
}

/// The lines of the document grouped into regions, in order. Lines of code and front matter are never part of one.
fn regions_of<'a>(root: &'a AstNode<'a>, lines: &[Line], text: &str) -> Vec<Region> {
    let mut kinds = vec![LineKind::Free; lines.len()];
    let mut found: Vec<Region> = Vec::new();
    // Code and front matter first, so that nothing else claims their lines.
    for node in root.descendants() {
        let data = node.data.borrow();
        if matches!(
            data.value,
            NodeValue::CodeBlock(_) | NodeValue::FrontMatter(_)
        ) {
            mark(&mut kinds, data.sourcepos, LineKind::Skip, true);
        }
    }
    for node in root.descendants() {
        let data = node.data.borrow();
        let kind = match data.value {
            NodeValue::Paragraph => LineKind::Paragraph,
            NodeValue::Heading(_) => LineKind::Heading,
            NodeValue::Table(_) => LineKind::Table,
            NodeValue::HtmlBlock(_) => LineKind::Html,
            _ => continue,
        };
        if let Some(region) = claim(&mut kinds, data.sourcepos, kind) {
            found.push(region);
        }
    }
    // Lines no block claimed that hold something: loose text and definitions.
    for (i, kind) in kinds.iter().enumerate() {
        if *kind == LineKind::Free && !text[lines[i].start..lines[i].end].trim().is_empty() {
            found.push(Region {
                kind: LineKind::Free,
                first: i,
                last: i,
            });
        }
    }
    found.sort_by_key(|r| r.first);
    found
}

fn mark(kinds: &mut [LineKind], pos: comrak::nodes::Sourcepos, kind: LineKind, force: bool) {
    let first = pos.start.line.saturating_sub(1);
    let last = pos
        .end
        .line
        .saturating_sub(1)
        .min(kinds.len().saturating_sub(1));
    for slot in kinds.iter_mut().take(last + 1).skip(first) {
        if force || *slot == LineKind::Free {
            *slot = kind;
        }
    }
}

/// Claims the lines of a block that nobody has claimed yet; the region covers the claimed run.
fn claim(kinds: &mut [LineKind], pos: comrak::nodes::Sourcepos, kind: LineKind) -> Option<Region> {
    if pos.start.line == 0 || kinds.is_empty() {
        return None;
    }
    let first = pos.start.line - 1;
    let last = (pos.end.line.max(pos.start.line) - 1).min(kinds.len() - 1);
    if first > last || kinds[first..=last].iter().any(|k| *k != LineKind::Free) {
        return None;
    }
    for slot in &mut kinds[first..=last] {
        *slot = kind;
    }
    Some(Region { kind, first, last })
}

/// The ranges of the cells of one table row. Cells are separated by `|` that is not escaped.
fn table_cells(text: &str, lo: usize, hi: usize) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut cells = Vec::new();
    let (mut start, mut i) = (lo, lo);
    while i < hi {
        match b[i] {
            b'\\' => i += 2,
            b'|' => {
                cells.push((start, i));
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    cells.push((start, hi.max(start)));
    cells
}

// ---- reference definitions ------------------------------------------------------------------------

/// Skips what a container puts before the content of a line: indentation, `>` markers and (on the first
/// line of a block) a list marker. Returns the offset of the content.
fn skip_container_prefix(b: &[u8], mut i: usize, hi: usize, first_line: bool) -> usize {
    loop {
        while i < hi && matches!(b[i], b' ' | b'\t') {
            i += 1;
        }
        if i < hi && b[i] == b'>' {
            i += 1;
            continue;
        }
        if first_line {
            let marker = list_marker_len(b, i, hi);
            if marker > 0 {
                i += marker;
                continue;
            }
        }
        return i;
    }
}

/// The length of a list marker (`-`, `+`, `*`, `1.`, `1)`) and the space after it, or 0.
fn list_marker_len(b: &[u8], i: usize, hi: usize) -> usize {
    let blank = |at: usize| at < hi && matches!(b[at], b' ' | b'\t');
    if i < hi && matches!(b[i], b'-' | b'+' | b'*') && blank(i + 1) {
        return 2;
    }
    let digits = b[i..hi].iter().take_while(|c| c.is_ascii_digit()).count();
    if (1..=9).contains(&digits)
        && i + digits < hi
        && matches!(b[i + digits], b'.' | b')')
        && blank(i + digits + 1)
    {
        return digits + 2;
    }
    0
}

/// `[label]: dest "title"` on one line. Footnote definitions (`[^1]: ...`) are not link definitions.
fn parse_definition(text: &str, lo: usize, hi: usize, first_line: bool) -> Option<Definition> {
    let b = text.as_bytes();
    let mut i = skip_container_prefix(b, lo, hi, first_line);
    if i >= hi || b[i] != b'[' {
        return None;
    }
    let label_start = i + 1;
    i = label_start;
    while i < hi && b[i] != b']' {
        match b[i] {
            b'\\' => i += 2,
            b'[' => return None,
            _ => i += 1,
        }
    }
    if i >= hi || b.get(i + 1) != Some(&b':') {
        return None;
    }
    let label = text.get(label_start..i)?;
    if label.trim().is_empty() || label.starts_with('^') {
        return None;
    }
    i += 2;
    while i < hi && matches!(b[i], b' ' | b'\t') {
        i += 1;
    }
    let (dest, mut j) = parse_destination(text, i, hi, Kind::Definition)?;
    let dest = dest?;
    while j < hi && matches!(b[j], b' ' | b'\t') {
        j += 1;
    }
    if j < hi {
        // an optional title may follow, and nothing else
        if j == dest.end {
            return None;
        }
        let after = skip_title(b, j, hi)?;
        if !text.get(after..hi)?.trim().is_empty() {
            return None;
        }
    }
    Some(Definition {
        label: normalize_label(label),
        dest,
    })
}

/// Reference labels match without regard to case or runs of whitespace.
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// ---- destinations and titles ------------------------------------------------------------------------

/// Parses a link destination at `i`: `<...>` or a bare run without spaces and with balanced parentheses.
/// Returns the destination (`None` for an empty bare one, as in `[a]()`) and the offset after it.
fn parse_destination(text: &str, i: usize, hi: usize, kind: Kind) -> Option<(Option<Dest>, usize)> {
    let b = text.as_bytes();
    if i < hi && b[i] == b'<' {
        let mut j = i + 1;
        while j < hi {
            match b[j] {
                b'\\' => j += 2,
                b'\n' | b'\r' | b'<' => return None,
                b'>' => {
                    let dest = Dest {
                        start: i,
                        end: j + 1,
                        form: Form::Angle,
                        kind,
                    };
                    return Some((Some(dest), j + 1));
                }
                _ => j += 1,
            }
        }
        return None;
    }
    let mut j = i;
    let mut depth = 0_usize;
    while j < hi {
        match b[j] {
            b'\\' if b.get(j + 1).is_some_and(u8::is_ascii_punctuation) => j += 2,
            c if c == b' ' || c.is_ascii_control() => break,
            b'(' => {
                depth += 1;
                j += 1;
            }
            b')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                j += 1;
            }
            _ => j += 1,
        }
    }
    let j = j.min(hi);
    if depth != 0 {
        return None;
    }
    if j == i {
        return Some((None, i));
    }
    let dest = Dest {
        start: i,
        end: j,
        form: Form::Bare,
        kind,
    };
    Some((Some(dest), j))
}

/// The offset after a link title at `i` (`"..."`, `'...'` or `(...)`, with backslash escapes).
fn skip_title(b: &[u8], i: usize, hi: usize) -> Option<usize> {
    let close = match b.get(i)? {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let mut j = i + 1;
    while j < hi {
        match b[j] {
            b'\\' => j += 2,
            c if c == close => return Some(j + 1),
            b'(' if close == b')' => return None,
            _ => j += 1,
        }
    }
    None
}

/// The tail of an inline link: `(` dest title `)`, with `open` at the `(`. Returns the destination, if the
/// link has one, and the offset after the `)`.
fn parse_inline_tail(text: &str, open: usize, hi: usize) -> Option<(Option<Dest>, usize)> {
    let b = text.as_bytes();
    let mut j = skip_space(b, open + 1, hi, true);
    let (dest, after) = parse_destination(text, j, hi, Kind::Inline)?;
    j = after;
    let spaced = skip_space(b, j, hi, true);
    let had_space = spaced > j;
    j = spaced;
    if had_space && j < hi && matches!(b[j], b'"' | b'\'' | b'(') {
        j = skip_title(b, j, hi)?;
        j = skip_space(b, j, hi, true);
    }
    if j < hi && b[j] == b')' {
        Some((dest, j + 1))
    } else {
        None
    }
}

/// Skips spaces and tabs, and one line ending when `one_newline` is set.
fn skip_space(b: &[u8], mut i: usize, hi: usize, one_newline: bool) -> usize {
    let mut newline = false;
    while i < hi {
        match b[i] {
            b' ' | b'\t' => i += 1,
            b'\n' | b'\r' if one_newline && !newline => {
                newline = true;
                i += if b[i] == b'\r' && b.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
            }
            _ => break,
        }
    }
    i
}

// ---- inline scanning --------------------------------------------------------------------------------

struct Opener {
    /// Offset of the `[`.
    pos: usize,
    image: bool,
    active: bool,
}

/// Scans `text[lo..hi]` as inline Markdown and appends the destinations of its links and images.
fn scan_inline(text: &str, lo: usize, hi: usize, defined: &HashSet<String>, out: &mut Vec<Dest>) {
    let b = text.as_bytes();
    let mut stack: Vec<Opener> = Vec::new();
    let mut i = lo;
    while i < hi {
        match b[i] {
            b'\\' => {
                i += if b.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
                    2
                } else {
                    1
                }
            }
            b'`' => i = skip_code_span(b, i, hi),
            b'$' => i = skip_math(b, i, hi),
            b'<' => i = skip_angle(text, i, hi, out),
            b'!' if i + 1 < hi && b[i + 1] == b'[' => {
                stack.push(Opener {
                    pos: i + 1,
                    image: true,
                    active: true,
                });
                i += 2;
            }
            b'[' => {
                stack.push(Opener {
                    pos: i,
                    image: false,
                    active: true,
                });
                i += 1;
            }
            b']' => i = close_bracket(text, i, hi, defined, &mut stack, out),
            _ => i += 1,
        }
    }
}

/// The offset after the code span that opens at `i`, or after the backtick run if it never closes.
fn skip_code_span(b: &[u8], i: usize, hi: usize) -> usize {
    let run = |at: usize| b[at..hi].iter().take_while(|c| **c == b'`').count();
    let width = run(i);
    let mut j = i + width;
    while j < hi {
        if b[j] == b'`' {
            let found = run(j);
            if found == width {
                return j + found;
            }
            j += found;
        } else {
            j += 1;
        }
    }
    i + width
}

/// The offset after the math that opens at `i` (`$x$` or `$$x$$`), or after the run of dollar signs when
/// nothing opens there. `[x](m.md)` inside a formula is not a link. The rules are the ones comrak reads
/// `math_dollars` by (from pandoc): a single `$` must not be followed or closed by a space, a closing one
/// is not followed by a digit (so prices are text), `\$` does not close, and more than two do not open.
fn skip_math(b: &[u8], i: usize, hi: usize) -> usize {
    let space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let open = b[i..hi].iter().take_while(|c| **c == b'$').count();
    let after = i + open;
    if open > 2 || (open == 1 && after < hi && space(b[after])) {
        return after;
    }
    let mut j = after;
    while j < hi {
        if b[j] != b'$' {
            j += 1;
            continue;
        }
        if open == 1 {
            if space(b[j - 1]) {
                // a space before the closing sign: not math
                return after;
            }
            if b[j - 1] == b'\\' {
                // an escaped sign is part of the formula
                j += 1;
                continue;
            }
        }
        let mut end = j;
        while end < hi && end - j < open && b[end] == b'$' {
            end += 1;
        }
        if open == 1 && end < hi && b[end].is_ascii_digit() {
            return after;
        }
        if end - j == open {
            return end;
        }
        j = end;
    }
    after
}

/// Handles a `]`: closes the innermost bracket as an inline link, a reference link, or plain text.
fn close_bracket(
    text: &str,
    i: usize,
    hi: usize,
    defined: &HashSet<String>,
    stack: &mut Vec<Opener>,
    out: &mut Vec<Dest>,
) -> usize {
    let b = text.as_bytes();
    let Some(opener) = stack.pop() else {
        return i + 1;
    };
    if !opener.active {
        return i + 1;
    }
    let formed = |stack: &mut Vec<Opener>, end: usize| {
        // links do not nest: a link inside deactivates the openers of the links around it
        if !opener.image {
            for o in stack.iter_mut().filter(|o| !o.image) {
                o.active = false;
            }
        }
        end
    };
    if i + 1 < hi && b[i + 1] == b'(' {
        if let Some((dest, end)) = parse_inline_tail(text, i + 1, hi) {
            if let Some(dest) = dest {
                out.push(dest);
            }
            return formed(stack, end);
        }
    }
    // reference forms: [text][label], [text][] and [text]
    let inner_start = opener.pos + 1;
    let inner = text.get(inner_start..i).unwrap_or_default();
    let known = |label: &str| defined.contains(&normalize_label(label));
    if i + 1 < hi && b[i + 1] == b'[' {
        if let Some(close) = text.get(i + 2..hi).and_then(|rest| rest.find(']')) {
            let label = &text[i + 2..i + 2 + close];
            if !label.contains('[') {
                let label = if label.trim().is_empty() {
                    inner
                } else {
                    label
                };
                if known(label) {
                    return formed(stack, i + 2 + close + 1);
                }
                // an unknown full reference is plain text; so is the label that follows
                return i + 1;
            }
        }
    }
    if known(inner) {
        return formed(stack, i + 1);
    }
    i + 1
}

// ---- raw HTML ---------------------------------------------------------------------------------------

/// Handles a `<` in inline text: skips an autolink, a comment or a tag (collecting the `href` / `src`
/// values of a tag), and returns the offset to go on from.
fn skip_angle(text: &str, i: usize, hi: usize, out: &mut Vec<Dest>) -> usize {
    let b = text.as_bytes();
    if text[i..hi].starts_with("<!--") {
        return text[i + 4..hi]
            .find("-->")
            .map_or(i + 1, |at| i + 4 + at + 3);
    }
    if let Some(end) = autolink_end(b, i, hi) {
        return end;
    }
    match parse_tag(text, i, hi) {
        Some((end, dests)) => {
            out.extend(dests);
            end
        }
        None => i + 1,
    }
}

/// The offset after an autolink (`<https://x>`, `<me@host>`) that opens at `i`.
fn autolink_end(b: &[u8], i: usize, hi: usize) -> Option<usize> {
    let mut j = i + 1;
    let first = j;
    while j < hi && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'+' | b'.' | b'-')) {
        j += 1;
    }
    let scheme_len = j - first;
    let scheme =
        b.get(first).is_some_and(u8::is_ascii_alphabetic) && (2..=32).contains(&scheme_len);
    if scheme && b.get(j) == Some(&b':') {
        j += 1;
        while j < hi && !matches!(b[j], b'>' | b'<' | b' ' | b'\n' | b'\r' | b'\t') {
            j += 1;
        }
        return (b.get(j) == Some(&b'>')).then_some(j + 1);
    }
    // an e-mail address
    let mut j = i + 1;
    while j < hi && !matches!(b[j], b'>' | b'<' | b' ' | b'\n' | b'\r' | b'\t') {
        j += 1;
    }
    let mail = &b[i + 1..j.min(hi)];
    (b.get(j) == Some(&b'>') && mail.contains(&b'@') && !mail.is_empty()).then_some(j + 1)
}

/// Parses an HTML tag at `i`; returns the offset after it and the destinations of its `href` / `src`.
fn parse_tag(text: &str, i: usize, hi: usize) -> Option<(usize, Vec<Dest>)> {
    let b = text.as_bytes();
    let mut j = i + 1;
    let closing = b.get(j) == Some(&b'/');
    if closing {
        j += 1;
    }
    let name_start = j;
    while j < hi && (b[j].is_ascii_alphanumeric() || b[j] == b'-') {
        j += 1;
    }
    if j == name_start || !b[name_start].is_ascii_alphabetic() {
        return None;
    }
    let name = text[name_start..j].to_ascii_lowercase();
    let mut dests = Vec::new();
    loop {
        let before = j;
        while j < hi && b[j].is_ascii_whitespace() {
            j += 1;
        }
        match b.get(j) {
            Some(b'>') if j < hi => return Some((j + 1, dests)),
            Some(b'/') if j + 1 < hi && b[j + 1] == b'>' => return Some((j + 2, dests)),
            _ => {}
        }
        if closing || j == before || j >= hi {
            // attributes need white space before them; a closing tag has none
            return None;
        }
        let attr_start = j;
        if !(b[j].is_ascii_alphabetic() || matches!(b[j], b'_' | b':')) {
            return None;
        }
        while j < hi && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b':' | b'.' | b'-'))
        {
            j += 1;
        }
        let attr = text[attr_start..j].to_ascii_lowercase();
        let mut cursor = j;
        while cursor < hi && b[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor < hi && b[cursor] == b'=' {
            cursor += 1;
            while cursor < hi && b[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            let (value_lo, value_hi, after) = match b.get(cursor) {
                Some(quote @ (b'"' | b'\'')) if cursor < hi => {
                    let close = text[cursor + 1..hi].find(char::from(*quote))?;
                    (cursor + 1, cursor + 1 + close, cursor + 1 + close + 1)
                }
                Some(_) if cursor < hi => {
                    let mut stop = cursor;
                    while stop < hi
                        && !b[stop].is_ascii_whitespace()
                        && !matches!(b[stop], b'"' | b'\'' | b'=' | b'<' | b'>' | b'`')
                    {
                        stop += 1;
                    }
                    if stop == cursor {
                        return None;
                    }
                    (cursor, stop, stop)
                }
                _ => return None,
            };
            if is_link_attribute(&name, &attr) && value_hi > value_lo {
                dests.push(Dest {
                    start: value_lo,
                    end: value_hi,
                    form: Form::Html,
                    kind: Kind::Html,
                });
            }
            j = after;
        }
    }
}

/// `href` of an anchor or `link`, `src` of media tags.
fn is_link_attribute(tag: &str, attr: &str) -> bool {
    match attr {
        "href" => matches!(tag, "a" | "link"),
        "src" => matches!(
            tag,
            "img" | "source" | "video" | "audio" | "track" | "embed"
        ),
        _ => false,
    }
}

/// Finds the tags of an HTML block.
fn scan_html_block(text: &str, lo: usize, hi: usize, out: &mut Vec<Dest>) {
    let b = text.as_bytes();
    let mut i = lo;
    while i < hi {
        if b[i] == b'<' {
            i = skip_angle(text, i, hi, out);
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests;
