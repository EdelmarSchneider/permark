//! Keeping links working when files move: which links of which documents have to change, and to what.
//!
//! The planner is plain Rust over a [`World`] (the files of the project, before the move), so it is tested
//! without a disk. Given the moves (`from` to `to`, files or folders) it finds
//!
//! * the links and images of every Markdown document that point at something that moves, and
//! * the relative links and images of the documents that move themselves (their folder changes, so the
//!   way to their targets does),
//!
//! and produces edits: the exact byte range of each link destination with its replacement. The text of the
//! link, its title, the `#anchor` and `?query`, angle brackets, percent-encoding, `\` or `/` separators, a
//! leading `./` and a missing `.md` are kept as the author wrote them, and every other byte stays as it is.
//!
//! Links that do not resolve to something that exists are left alone (they were broken already), and so are
//! external links, anchors, and links the scanner could not read with certainty.

use std::fmt::Write as _;
use std::io;

use serde::{Deserialize, Serialize};

use super::scan::{find_destinations, unescape, Dest, Form};
use super::SkipReason;
use crate::edit::blocks::{split_lines, BOM};
use crate::links::{join_relative, percent_decode, scheme_of};
use crate::render::MAX_RENDER_BYTES;
use crate::util::hash_hex;
use crate::vault::{key, WORKTREE_PREFIX};

/// A file or folder that moves (or is renamed): vault paths in the UI form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    pub from: String,
    pub to: String,
}

/// What a path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    File,
    Dir,
}

/// The project as the planner needs to see it, before anything has moved.
pub(crate) trait World {
    /// The Markdown documents of the project (vault paths).
    fn documents(&self) -> Vec<String>;
    /// The bytes of a document.
    fn read(&self, rel: &str) -> io::Result<Vec<u8>>;
    /// Whether a path is a file, a folder or nothing.
    fn kind(&self, rel: &str) -> Option<EntryKind>;
}

/// One link destination to rewrite: `start..end` of the text the plan was made from.
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
    /// The moves (by position in the list given to the planner) this edit is about: the one that moves
    /// the document, and the one that moves what the link points at. An undo can take the edit back only
    /// when all of them go back.
    pub causes: Vec<usize>,
}

/// The edits of one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocPlan {
    /// Where the document is now.
    pub path: String,
    /// Where it is after the move, when it moves itself.
    pub moved_to: Option<String>,
    /// Hash of the bytes the edits are based on.
    pub hash: String,
    pub edits: Vec<LinkEdit>,
}

/// A document that could not be looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkippedDoc {
    pub path: String,
    pub reason: SkipReason,
}

/// Everything the move changes in documents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LinkPlan {
    pub docs: Vec<DocPlan>,
    pub skipped: Vec<SkippedDoc>,
}

impl LinkPlan {
    /// How many links change.
    pub(crate) fn link_count(&self) -> usize {
        self.docs.iter().map(|d| d.edits.len()).sum()
    }
}

// ---- paths ------------------------------------------------------------------------------------------

/// `@wt/<name>/` and the rest, for a worktree path; `""` and the whole path otherwise.
fn split_root(rel: &str) -> (&str, &str) {
    let Some(rest) = rel.strip_prefix(WORKTREE_PREFIX) else {
        return ("", rel);
    };
    match rest.find('/') {
        Some(at) => rel.split_at(WORKTREE_PREFIX.len() + at + 1),
        None => (rel, ""),
    }
}

fn segments(inner: &str) -> Vec<&str> {
    inner.split('/').filter(|s| !s.is_empty()).collect()
}

/// The moves, answering "where does this path end up?".
struct Mover {
    /// `from` segments (case-folded where the system folds case), `to`, their count, and the position of
    /// the move in the list it came from; longest `from` first.
    entries: Vec<(Vec<String>, String, usize, usize)>,
}

impl Mover {
    fn new(moves: &[Move]) -> Self {
        let mut entries: Vec<(Vec<String>, String, usize, usize)> = moves
            .iter()
            .enumerate()
            .map(|(at, m)| {
                let segs: Vec<String> = m.from.split('/').map(key).collect();
                (segs.clone(), m.to.clone(), segs.len(), at)
            })
            .collect();
        entries.sort_by_key(|(_, _, len, _)| std::cmp::Reverse(*len));
        Self { entries }
    }

    /// The move that takes `path` along, and the new place it gives it.
    fn find(&self, path: &str) -> Option<(usize, String)> {
        let parts: Vec<&str> = path.split('/').collect();
        for (from, to, len, at) in &self.entries {
            if parts.len() >= *len && parts.iter().zip(from).all(|(p, f)| key(p) == *f) {
                let mut out = to.clone();
                for rest in &parts[*len..] {
                    out.push('/');
                    out.push_str(rest);
                }
                return Some((*at, out));
            }
        }
        None
    }

    /// The new place of `path`: the path itself when it is moved, or one inside a moved folder.
    fn map(&self, path: &str) -> Option<String> {
        self.find(path).map(|(_, new)| new)
    }

    /// Which move takes `path` along.
    fn move_of(&self, path: &str) -> Option<usize> {
        self.find(path).map(|(at, _)| at)
    }

    fn map_or_same(&self, path: &str) -> String {
        self.map(path).unwrap_or_else(|| path.to_owned())
    }
}

/// The way from the folder `from_dir` to `target`, both as segments below the same root.
fn relative_path(from_dir: &[&str], target: &[&str]) -> String {
    let mut common = 0;
    while common < from_dir.len()
        && common < target.len()
        && key(from_dir[common]) == key(target[common])
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

// ---- how a destination is written ----------------------------------------------------------------------

/// Splits a destination at the first `?` or `#`: the path and the rest (kept as written).
fn split_suffix(raw: &str) -> (&str, &str) {
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
    /// The old path had `%XX` escapes.
    percent: bool,
    /// ... and some of them were for non-ASCII characters.
    percent_non_ascii: bool,
    /// The old path used `\` between folders and no `/`.
    backslash: bool,
    /// The old path began with `./`.
    dot_slash: bool,
    /// The old path ended with a `/`.
    trailing_slash: bool,
}

/// Whether the text starts like the rest of an HTML entity (`copy;`, `#169;`).
fn looks_like_entity(rest: &str) -> bool {
    let name_len = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '#')
        .count();
    name_len > 0 && rest[name_len..].starts_with(';')
}

/// Whether `c` has to be percent-encoded in a path segment written in `style`. `rest` is what follows it.
fn needs_encoding(c: char, rest: &str, style: Style) -> bool {
    let angle = style.form == Form::Angle;
    match c {
        // these end or split the destination in every form
        '#' | '?' | '%' | '<' | '>' => true,
        '"' | '`' | '|' | '(' | ')' => !angle,
        // the attribute may be quoted with either: `<a href='it%27s.md'>`
        '\'' => style.form == Form::Html,
        ' ' => !angle || style.percent,
        '&' => !angle && looks_like_entity(rest),
        c if c.is_control() => true,
        c if !c.is_ascii() => style.percent_non_ascii,
        _ => false,
    }
}

fn encode_segment(seg: &str, style: Style) -> String {
    let mut out = String::with_capacity(seg.len());
    for (at, c) in seg.char_indices() {
        if needs_encoding(c, &seg[at + c.len_utf8()..], style) {
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

/// 1-based line numbers for byte offsets.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let from = if text.starts_with(BOM) {
            BOM.len_utf8()
        } else {
            0
        };
        let (lines, _) = split_lines(text, from);
        let mut starts: Vec<usize> = lines.iter().map(|l| l.start).collect();
        if starts.is_empty() {
            starts.push(0);
        }
        Self { starts }
    }

    fn line_of(&self, offset: usize) -> usize {
        self.starts.partition_point(|s| *s <= offset).max(1)
    }
}

// ---- the planner ------------------------------------------------------------------------------------------

/// Plans the rewrite of links for a set of moves.
pub(crate) struct Planner<'a> {
    world: &'a dyn World,
    moves: &'a [Move],
    mover: Mover,
}

impl<'a> Planner<'a> {
    pub(crate) fn new(world: &'a dyn World, moves: &'a [Move]) -> Self {
        Self {
            world,
            moves,
            mover: Mover::new(moves),
        }
    }

    /// The plan for every document of the project.
    pub(crate) fn plan(&self) -> LinkPlan {
        let mut out = LinkPlan::default();
        let Some(first) = self.moves.first() else {
            return out;
        };
        let (root, _) = split_root(&first.from);
        let needles = self.needles();
        for doc in self.world.documents() {
            if split_root(&doc).0 != root {
                continue;
            }
            let moved_to = self.mover.map(&doc);
            let bytes = match self.world.read(&doc) {
                Ok(bytes) if bytes.len() as u64 > MAX_RENDER_BYTES => {
                    out.skipped.push(SkippedDoc {
                        path: doc,
                        reason: SkipReason::TooLarge,
                    });
                    continue;
                }
                Ok(bytes) => bytes,
                Err(_) => {
                    out.skipped.push(SkippedDoc {
                        path: doc,
                        reason: SkipReason::Unreadable,
                    });
                    continue;
                }
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                out.skipped.push(SkippedDoc {
                    path: doc,
                    reason: SkipReason::NotUtf8,
                });
                continue;
            };
            // A document that stays put only matters when it mentions something that moves.
            if moved_to.is_none() && !mentions(text, &needles) {
                continue;
            }
            let edits = self.edits_for(&doc, text);
            if !edits.is_empty() {
                out.docs.push(DocPlan {
                    path: doc,
                    moved_to,
                    hash: hash_hex(&bytes),
                    edits,
                });
            }
        }
        out
    }

    /// The names (lowercase) a link to a moved path has to contain, in the forms a link can spell them.
    fn needles(&self) -> Vec<String> {
        let mut out = Vec::new();
        for m in self.moves {
            let name = m.from.rsplit('/').next().unwrap_or(&m.from);
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

    /// Where a path ends up after the moves.
    pub(crate) fn new_path(&self, path: &str) -> String {
        self.mover.map_or_same(path)
    }

    /// The edits that `text`, the content of the document `doc`, needs.
    pub(crate) fn edits_for(&self, doc: &str, text: &str) -> Vec<LinkEdit> {
        let doc_new = self.mover.map_or_same(doc);
        let lines = LineIndex::new(text);
        let mut edits = Vec::new();
        for dest in find_destinations(text) {
            if let Some((new, target)) = self.rewrite(doc, &doc_new, text, &dest) {
                let old = dest.raw(text).to_owned();
                if new != old {
                    let mut causes: Vec<usize> =
                        [self.mover.move_of(doc), self.mover.move_of(&target)]
                            .into_iter()
                            .flatten()
                            .collect();
                    causes.dedup();
                    edits.push(LinkEdit {
                        start: dest.start,
                        end: dest.end,
                        old,
                        new,
                        line: lines.line_of(dest.start),
                        causes,
                    });
                }
            }
        }
        edits
    }

    /// The new token for one destination and the path it points at (before the move), or `None` when it
    /// stays as it is.
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
        let path = unescape(path_raw);
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
        // Cheap test first: a link in a document that stays put matters only if its target moves.
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
            target_new.clone()
        };
        // Resolved from where the document will be, does the link as written still reach the target?
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
        let (percent, percent_non_ascii) = percent_style(&path);
        let style = Style {
            form: dest.form,
            percent,
            percent_non_ascii,
            backslash: path.contains('\\') && !path.contains('/'),
            dot_slash: path.starts_with("./") || path.starts_with(".\\"),
            trailing_slash: decoded.ends_with('/') && decoded.len() > 1,
        };
        Some((render_path(&rel, suffix, style), target))
    }

    /// What a link path points at (before the move): the path itself, or the `.md` file it leaves out.
    fn target_of(&self, joined: &str) -> Option<(String, bool)> {
        let (_, inner) = split_root(joined);
        if inner.is_empty() {
            return None;
        }
        if self.world.kind(joined).is_some() {
            return Some((joined.to_owned(), false));
        }
        if !is_md(joined) {
            let with_ext = format!("{joined}.md");
            if self.world.kind(&with_ext) == Some(EntryKind::File) {
                return Some((with_ext, true));
            }
        }
        None
    }
}

fn is_md(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".md")
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

/// Whether `text` contains any of the lowercase `needles`, ignoring case.
fn mentions(text: &str, needles: &[String]) -> bool {
    let lower = text.to_lowercase();
    needles.iter().any(|n| lower.contains(n.as_str()))
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
    // how many bytes the earlier edits added and removed so far
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

#[cfg(test)]
mod tests;
