//! Keeping links working when files move: which links of which documents have to change, and to what.
//!
//! Given the moves (`from` -> `to`, files or folders) the planner finds the links and images of every Markdown
//! document that point at something that moves, and the relative links of the documents that move themselves (their
//! folder changes, so the way to their targets does). It produces edits: the exact byte range of each destination
//! with its replacement. The text of the link, its title, the `#anchor` and `?query`, angle brackets,
//! percent-encoding, `\` or `/`, a leading `./` and a missing `.md` are kept as the author wrote them; every other
//! byte stays as it is. Links that do not resolve to something that exists are left alone (they were broken
//! already), and so are external links, anchors, and links not read with certainty.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;

use crate::links::{join_relative, parse, percent_decode, scheme_of, Dest, Form};
use crate::paths::{Vault, VaultPath, WORKTREE_PREFIX};
use crate::tree::key_of;

/// One edit: replace `range` of the text the plan was made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkEdit {
    pub start: usize,
    pub end: usize,
    /// The destination as written.
    pub old: String,
    /// What it becomes.
    pub new: String,
    /// 1-based line of the link.
    pub line: usize,
    /// The moves (by position) this edit is about: the one that moves the document, the one that moves what the
    /// link points at. An undo takes the edit back only when all of them go back.
    pub causes: Vec<usize>,
}

/// The edits of one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocPlan {
    /// Where the document is now.
    pub path: String,
    /// Where it is after the moves, when it moves (itself or inside a moved folder).
    pub moved_to: Option<String>,
    /// Hash of the bytes the edits are based on.
    pub hash: crate::fsx::ContentHash,
    pub edits: Vec<LinkEdit>,
}

// ---- paths -------------------------------------------------------------------------------------------------------

/// `@wt/<name>/` and the rest, for a worktree path; `""` and the whole path otherwise.
pub(crate) fn split_root(path: &str) -> (&str, &str) {
    let Some(rest) = path.strip_prefix(WORKTREE_PREFIX) else {
        return ("", path);
    };
    match rest.find('/') {
        Some(at) => path.split_at(WORKTREE_PREFIX.len() + at + 1),
        None => (path, ""),
    }
}

fn segments(inner: &str) -> Vec<&str> {
    inner.split('/').filter(|s| !s.is_empty()).collect()
}

/// The moves, answering "where does this path end up?".
pub(crate) struct Mover {
    /// `from` segments (folded), `to`, and the position of the move; longest `from` first.
    entries: Vec<(Vec<String>, String, usize)>,
}

impl Mover {
    pub(crate) fn new(moves: &[(String, String)]) -> Self {
        let mut entries: Vec<(Vec<String>, String, usize)> = moves
            .iter()
            .enumerate()
            .map(|(at, (from, to))| (from.split('/').map(key_of).collect(), to.clone(), at))
            .collect();
        entries.sort_by_key(|(from, _, _)| std::cmp::Reverse(from.len()));
        Self { entries }
    }

    /// The move that takes `path` along and the place it gives it.
    fn find(&self, path: &str) -> Option<(usize, String)> {
        let parts: Vec<&str> = path.split('/').collect();
        for (from, to, at) in &self.entries {
            if parts.len() >= from.len() && parts.iter().zip(from).all(|(p, f)| key_of(p) == *f) {
                let mut out = to.clone();
                for rest in &parts[from.len()..] {
                    out.push('/');
                    out.push_str(rest);
                }
                return Some((*at, out));
            }
        }
        None
    }

    pub(crate) fn map(&self, path: &str) -> Option<String> {
        self.find(path).map(|(_, new)| new)
    }

    fn move_of(&self, path: &str) -> Option<usize> {
        self.find(path).map(|(at, _)| at)
    }

    /// Position of the move that takes `path` along.
    pub(crate) fn move_index_of(&self, path: &str) -> Option<usize> {
        self.move_of(path)
    }

    pub(crate) fn map_or_same(&self, path: &str) -> String {
        self.map(path).unwrap_or_else(|| path.to_owned())
    }
}

/// The way from the folder `from_dir` to `target`, both as segments below the same root.
fn relative_path(from_dir: &[&str], target: &[&str]) -> String {
    let mut common = 0;
    while common < from_dir.len()
        && common < target.len()
        && key_of(from_dir[common]) == key_of(target[common])
    {
        common += 1;
    }
    let mut parts: Vec<&str> = vec![".."; from_dir.len() - common];
    parts.extend_from_slice(&target[common..]);
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

// ---- how a destination is written --------------------------------------------------------------------------------

/// Whether `s` has a `%XX` escape, and whether one of them is a byte of a non-ASCII character.
fn percent_style(s: &str) -> (bool, bool) {
    let b = s.as_bytes();
    let hex = |at: usize| b.get(at).and_then(|c| char::from(*c).to_digit(16));
    let (mut any, mut non_ascii) = (false, false);
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let (Some(hi), Some(lo)) = (hex(i + 1), hex(i + 2)) {
                any = true;
                non_ascii |= hi * 16 + lo >= 0x80;
                i += 3;
                continue;
            }
        }
        i += 1;
    }
    (any, non_ascii)
}

/// How a path was written, so the new one can be written the same way.
#[allow(clippy::struct_excessive_bools)] // independent facts about one link, not a state machine
#[derive(Debug, Clone, Copy)]
struct Style {
    form: Form,
    percent: bool,
    percent_non_ascii: bool,
    /// The old path used `\` between folders and no `/`.
    backslash: bool,
    dot_slash: bool,
    trailing_slash: bool,
}

fn style_of(form: Form, path: &str, decoded: &str) -> Style {
    let (percent, percent_non_ascii) = percent_style(path);
    Style {
        form,
        percent,
        percent_non_ascii,
        backslash: path.contains('\\') && !path.contains('/'),
        dot_slash: path.starts_with("./") || path.starts_with(".\\"),
        trailing_slash: decoded.ends_with('/') && decoded.len() > 1,
    }
}

/// Whether the text starts like the rest of an HTML entity (`copy;`, `#169;`).
fn looks_like_entity(rest: &str) -> bool {
    let name_len = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '#')
        .count();
    name_len > 0 && rest[name_len..].starts_with(';')
}

/// Whether `c` has to be percent-encoded in a path segment written in `style`. `rest` follows it.
fn needs_encoding(c: char, rest: &str, style: Style) -> bool {
    let angle = style.form == Form::Angle;
    match c {
        '#' | '?' | '%' | '<' | '>' => true,
        '"' | '`' | '|' | '(' | ')' => !angle,
        '\'' => style.form == Form::Html,
        ' ' => !angle || style.percent,
        '&' => !angle && looks_like_entity(rest),
        c if c.is_control() => true,
        c if !c.is_ascii() => style.percent_non_ascii,
        _ => false,
    }
}

fn encode_segment(segment: &str, style: Style) -> String {
    let mut out = String::with_capacity(segment.len());
    for (at, c) in segment.char_indices() {
        if needs_encoding(c, &segment[at + c.len_utf8()..], style) {
            let mut buf = [0_u8; 4];
            for byte in c.encode_utf8(&mut buf).bytes() {
                // writing into a String cannot fail
                let _ = write!(out, "%{byte:02X}");
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The new destination token for the path `rel` (segments joined with `/`), written like the old one.
fn render_path(rel: &str, suffix: &str, style: Style) -> String {
    let segs: Vec<String> = rel
        .split('/')
        .map(|s| {
            if s == ".." || s == "." {
                s.to_owned()
            } else {
                encode_segment(s, style)
            }
        })
        .collect();
    // `\` only where it cannot be read as the escape of the character after it
    let separator = if style.backslash
        && segs
            .iter()
            .all(|s| !s.chars().next().is_some_and(|c| c.is_ascii_punctuation()))
    {
        "\\"
    } else {
        "/"
    };
    let mut path = segs.join(separator);
    if style.dot_slash && !rel.starts_with("../") && !rel.starts_with('/') && rel != "." {
        path = format!(".{separator}{path}");
    }
    if style.trailing_slash && !path.ends_with(separator) && path != "." {
        path.push_str(separator);
    }
    let token = format!("{path}{suffix}");
    match style.form {
        Form::Bare => token,
        Form::Angle => format!("<{token}>"),
        Form::Html => token.replace('&', "&amp;"),
    }
}

fn strip_md(path: &str) -> &str {
    path.strip_suffix(".md")
        .or_else(|| path.strip_suffix(".MD"))
        .unwrap_or(path)
}

fn is_md(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".md")
}

/// Path part and `#anchor` | `?query` suffix of a destination.
fn split_suffix(raw: &str) -> (&str, &str) {
    raw.find(['#', '?'])
        .map_or((raw, ""), |at| raw.split_at(at))
}

/// Backslash escapes of Markdown (`\)`) undone.
fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.peek().filter(|n| n.is_ascii_punctuation()) {
                out.push(*next);
                chars.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// A name as a link writes it with percent-encoding.
fn encode_name(name: &str) -> String {
    let style = Style {
        form: Form::Bare,
        percent: true,
        percent_non_ascii: true,
        backslash: false,
        dot_slash: false,
        trailing_slash: false,
    };
    encode_segment(name, style)
}

/// The names (lowercase) a link to a moved path has to contain, in the forms a link can spell them.
fn needles(moves: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (from, _) in moves {
        let name = from.rsplit('/').next().unwrap_or(from);
        let stem = strip_md(name);
        for spelled in [stem.to_owned(), encode_name(stem)] {
            let spelled = spelled.to_lowercase();
            if !spelled.is_empty() && !out.contains(&spelled) {
                out.push(spelled);
            }
        }
    }
    out
}

/// Whether `text` contains any of the lowercase `needles`, ignoring case.
pub(crate) fn mentions(text: &str, needles: &[String]) -> bool {
    let lower = text.to_lowercase();
    needles.iter().any(|n| lower.contains(n.as_str()))
}

// ---- the planner -------------------------------------------------------------------------------------------------

/// What a path of the project is on disk (before the moves).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    File,
    Dir,
}

pub(crate) struct Planner<'a> {
    vault: &'a Vault,
    mover: Mover,
    needles: Vec<String>,
    seen: RefCell<HashMap<String, Option<Entry>>>,
}

impl<'a> Planner<'a> {
    pub(crate) fn new(vault: &'a Vault, moves: &[(String, String)]) -> Self {
        Self {
            vault,
            mover: Mover::new(moves),
            needles: needles(moves),
            seen: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) fn needles(&self) -> &[String] {
        &self.needles
    }

    pub(crate) fn mover(&self) -> &Mover {
        &self.mover
    }

    fn kind(&self, path: &str) -> Option<Entry> {
        if let Some(found) = self.seen.borrow().get(path) {
            return *found;
        }
        let found = VaultPath::parse(path)
            .ok()
            .and_then(|vp| self.vault.resolve(&vp).ok())
            .and_then(|abs| std::fs::metadata(abs).ok())
            .map(|m| if m.is_dir() { Entry::Dir } else { Entry::File });
        self.seen.borrow_mut().insert(path.to_owned(), found);
        found
    }

    /// The edits `text`, the content of the document `doc`, needs.
    pub(crate) fn edits_for(&self, doc: &str, text: &str) -> Vec<LinkEdit> {
        let doc_new = self.mover.map_or_same(doc);
        let mut edits = Vec::new();
        for dest in parse(text) {
            let Some((new, target)) = self.rewrite(doc, &doc_new, text, &dest) else {
                continue;
            };
            let old = dest.raw(text).to_owned();
            if new == old {
                continue;
            }
            let mut causes: Vec<usize> = [self.mover.move_of(doc), self.mover.move_of(&target)]
                .into_iter()
                .flatten()
                .collect();
            causes.dedup();
            edits.push(LinkEdit {
                start: dest.range.start,
                end: dest.range.end,
                old,
                new,
                line: dest.line,
                causes,
            });
        }
        edits
    }

    /// The new token for one destination and the path it points at (before the move), or `None` when it stays.
    fn rewrite(
        &self,
        doc: &str,
        doc_new: &str,
        text: &str,
        dest: &Dest,
    ) -> Option<(String, String)> {
        let raw = dest.inner(text);
        if dest.form == Form::Html && (raw.contains('&') || raw.trim() != raw) {
            return None;
        }
        let (path_raw, suffix) = split_suffix(raw);
        let path = if dest.form == Form::Html {
            path_raw.to_owned()
        } else {
            unescape(path_raw)
        };
        if path.is_empty()
            || path.starts_with("//")
            || path.starts_with("\\\\")
            || scheme_of(&path).is_some()
            || path.as_bytes().get(1) == Some(&b':')
        {
            return None;
        }
        let decoded = percent_decode(&path).replace('\\', "/");
        let root_relative = decoded.starts_with('/');
        let joined = join_relative(doc, &decoded)?;
        // cheap test first: a link in a document that stays put matters only if its target moves
        let touches = self.mover.map(&joined).is_some()
            || (!is_md(&joined) && self.mover.map(&format!("{joined}.md")).is_some());
        if !touches && doc_new == doc {
            return None;
        }
        let (target, extless) = self.target_of(&joined)?;
        let target_new = self.mover.map_or_same(&target);
        let written_new = if extless && is_md(&target_new) {
            strip_md(&target_new).to_owned()
        } else {
            target_new
        };
        // resolved from where the document will be, does the link as written still reach the target?
        if join_relative(doc_new, &decoded).as_deref() == Some(written_new.as_str()) {
            return None;
        }
        let (doc_root, doc_inner) = split_root(doc_new);
        let (target_root, target_inner) = split_root(&written_new);
        if doc_root != target_root {
            return None;
        }
        let rel = if root_relative {
            format!("/{target_inner}")
        } else {
            let mut dir = segments(doc_inner);
            dir.pop(); // the document's own name
            relative_path(&dir, &segments(target_inner))
        };
        let style = style_of(dest.form, &path, &decoded);
        Some((render_path(&rel, suffix, style), target))
    }

    /// What a link path points at (before the move): the path itself, or the `.md` file it leaves out.
    fn target_of(&self, joined: &str) -> Option<(String, bool)> {
        let (_, inner) = split_root(joined);
        if inner.is_empty() {
            return None;
        }
        if self.kind(joined).is_some() {
            return Some((joined.to_owned(), false));
        }
        if !is_md(joined) {
            let with_ext = format!("{joined}.md");
            if self.kind(&with_ext) == Some(Entry::File) {
                return Some((with_ext, true));
            }
        }
        None
    }
}

/// Applies edits (sorted, not overlapping) to the text they were made from.
pub(crate) fn apply_edits(text: &str, edits: &[LinkEdit]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for edit in edits {
        out.push_str(&text[at..edit.start]);
        out.push_str(&edit.new);
        at = edit.end;
    }
    out.push_str(&text[at..]);
    out
}

/// The edits that undo `edits` in the text they produced: the same places, seen from the new text.
pub(crate) fn invert_edits(edits: &[LinkEdit]) -> Vec<LinkEdit> {
    let (mut grown, mut shrunk) = (0_usize, 0_usize);
    edits
        .iter()
        .map(|e| {
            let start = (e.start + grown).saturating_sub(shrunk);
            let end = start + e.new.len();
            grown += e.new.len().saturating_sub(e.old.len());
            shrunk += e.old.len().saturating_sub(e.new.len());
            LinkEdit {
                start,
                end,
                old: e.new.clone(),
                new: e.old.clone(),
                line: e.line,
                causes: e.causes.clone(),
            }
        })
        .collect()
}
