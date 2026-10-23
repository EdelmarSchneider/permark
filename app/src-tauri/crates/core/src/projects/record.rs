//! Root record `<root>/.permark/projects.toml` v1 (`projects.md` §2): pure parse + surgical text ops + the checked
//! atomic write. Ops splice the text by the parser's spans: every byte outside the touched entry stays (comments,
//! order, spacing, unknown keys | tables, CRLF | LF, BOM, trailing newline).

use std::ops::Range;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{Document, Item, Table};
use ts_rs::TS;

use super::uid::ProjectUid;
use crate::paths::VaultPath;

pub const RECORD_FILE: &str = "projects.toml";
/// Largest record read.
pub const MAX_RECORD_BYTES: u64 = 1024 * 1024;
/// Entries used; the rest are skipped `tooMany`.
pub const MAX_ENTRIES: usize = 2000;
/// Longest path text of an entry (characters).
const MAX_PATH_CHARS: usize = 1024;
/// Format written.
pub const VERSION: i64 = 1;
/// Header of a record PerMark creates.
pub const HEADER: &str =
    "# Projects this folder includes. PerMark changes only the lines it needs.";

/// Which ignore rules hide files inside a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum HiddenRule {
    /// Its own `.gitignore` | `.ignore` files (+ its exclude, global excludes).
    #[serde(rename = "own")]
    Own,
    /// Its own, layered over the root's files from the root down to its parent.
    #[serde(rename = "own+root")]
    OwnRoot,
}

/// Why an entry of the record is not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RecordSkip {
    BadPath,
    #[serde(rename = "self")]
    SelfPath,
    Absolute,
    DotDot,
    Duplicate,
    BadId,
    TooMany,
    /// A segment on disk is a link | junction | reparse point (checked at map build).
    Link,
    /// The resolved path leaves the root (checked at map build).
    Outside,
}

/// An entry that is not used, with its line (1-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SkippedEntry {
    pub line: u32,
    pub reason: RecordSkip,
}

/// One valid entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub uid: ProjectUid,
    /// No `id` written: `uid` derived from the path (written with the entry's next PerMark write).
    pub derived: bool,
    pub path: VaultPath,
    pub hidden: HiddenRule,
    /// `hidden` had a value PerMark does not know: read as `own+root` (stricter), shown as a warning.
    pub hidden_unknown: bool,
    pub first_commit: Option<String>,
    pub line: u32,
    /// Index among the `[[project]]` tables (ops find their text by it).
    table: usize,
}

/// The record as read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    pub version: i64,
    pub entries: Vec<Entry>,
    pub skipped: Vec<SkippedEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordRead {
    Absent,
    Read(Record),
    /// Written by a newer version: known keys read, writes refused.
    Newer(Record),
    Unreadable {
        line: u32,
        message: String,
    },
}

impl RecordRead {
    /// The entries in force (none when unreadable | absent).
    #[must_use]
    pub fn record(&self) -> Option<&Record> {
        match self {
            Self::Read(r) | Self::Newer(r) => Some(r),
            Self::Absent | Self::Unreadable { .. } => None,
        }
    }
}

/// One change of the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordOp {
    Add {
        uid: ProjectUid,
        path: VaultPath,
        hidden: HiddenRule,
    },
    Remove {
        uid: ProjectUid,
    },
    SetPath {
        uid: ProjectUid,
        path: VaultPath,
    },
    SetHidden {
        uid: ProjectUid,
        rule: HiddenRule,
    },
    SetFirstCommit {
        uid: ProjectUid,
        oid: String,
    },
}

// ---- layout (spans) ----------------------------------------------------------------------------------------------

/// Byte offsets of line starts.
pub(super) struct Lines(Vec<usize>);

impl Lines {
    pub(super) fn of(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self(starts)
    }

    /// 1-based line of a byte offset.
    pub(super) fn line(&self, at: usize) -> u32 {
        u32::try_from(self.0.partition_point(|&s| s <= at)).unwrap_or(u32::MAX)
    }

    pub(super) fn start_of(&self, at: usize) -> usize {
        let i = self.0.partition_point(|&s| s <= at);
        self.0[i.saturating_sub(1)]
    }

    /// End of the line holding `at`, after its newline (or the text end).
    pub(super) fn end_of(&self, at: usize, len: usize) -> usize {
        let i = self.0.partition_point(|&s| s <= at);
        self.0.get(i).copied().unwrap_or(len)
    }
}

/// Where one key of a table is: its whole line(s) + its value token.
#[derive(Debug, Clone)]
pub(super) struct KeyAt {
    pub(super) lines: Range<usize>,
    pub(super) value: Range<usize>,
}

/// One `[[project]]` table as text.
#[derive(Debug, Clone)]
struct TableAt {
    /// Header line start .. end of its last line (after the newline).
    region: Range<usize>,
    /// End of the header line.
    header_end: usize,
    path: Option<KeyAt>,
    hidden: Option<KeyAt>,
    first_commit: Option<KeyAt>,
}

pub(super) fn key_at(table: &Table, key: &str, lines: &Lines, len: usize) -> Option<KeyAt> {
    let (k, item) = table.get_key_value(key)?;
    let value = item.span()?;
    let start = k.span().map_or(value.start, |s| s.start);
    Some(KeyAt {
        lines: lines.start_of(start)..lines.end_of(value.end.saturating_sub(1), len),
        value,
    })
}

/// Last byte any part of `table` (keys, values, sub-tables) covers.
fn table_end(table: &Table) -> usize {
    let mut end = table.span().map_or(0, |s| s.end);
    for (_, item) in table {
        let reach = match item {
            Item::Value(v) => v.span().map_or(0, |s| s.end),
            Item::Table(t) => table_end(t),
            Item::ArrayOfTables(a) => a.iter().map(table_end).max().unwrap_or(0),
            Item::None => 0,
        };
        end = end.max(reach);
    }
    end
}

struct Scanned<'a> {
    doc: Document<&'a str>,
    lines: Lines,
}

/// Unreadable at a byte offset.
fn unreadable(lines: &Lines, at: usize, message: &str) -> RecordRead {
    RecordRead::Unreadable {
        line: lines.line(at),
        message: message.to_owned(),
    }
}

pub(super) fn strip_bom(text: &str) -> (&str, &str) {
    text.strip_prefix('\u{feff}')
        .map_or(("", text), |rest| ("\u{feff}", rest))
}

fn scan(text: &str) -> Result<Scanned<'_>, RecordRead> {
    let lines = Lines::of(text);
    match Document::parse(text) {
        Ok(doc) => Ok(Scanned { doc, lines }),
        Err(error) => {
            let at = error.span().map_or(0, |s| s.start);
            Err(unreadable(&lines, at, error.message()))
        }
    }
}

fn tables_of(scanned: &Scanned<'_>, len: usize) -> Vec<TableAt> {
    let Some(Item::ArrayOfTables(aot)) = scanned.doc.get("project") else {
        return Vec::new();
    };
    let lines = &scanned.lines;
    aot.iter()
        .map(|table| {
            let header = table.span().unwrap_or(0..0);
            let end = table_end(table).max(header.end);
            TableAt {
                region: lines.start_of(header.start)..lines.end_of(end.saturating_sub(1), len),
                header_end: lines.end_of(header.start, len),
                path: key_at(table, "path", lines, len),
                hidden: key_at(table, "hidden", lines, len),
                first_commit: key_at(table, "first_commit", lines, len),
            }
        })
        .collect()
}

// ---- parse -------------------------------------------------------------------------------------------------------

/// Path text of an entry -> its project path, or why not (pure; disk checks come later).
fn check_path(raw: &str) -> Result<VaultPath, RecordSkip> {
    if raw.chars().count() > MAX_PATH_CHARS {
        return Err(RecordSkip::BadPath);
    }
    if raw.is_empty() || raw == "." {
        return Err(RecordSkip::SelfPath);
    }
    let bytes = raw.as_bytes();
    let drive = bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic();
    if raw.starts_with('/') || raw.starts_with('\\') || drive {
        return Err(RecordSkip::Absolute);
    }
    if raw.split(['/', '\\']).any(|s| s == "..") {
        return Err(RecordSkip::DotDot);
    }
    if raw.contains('\\') || raw.contains('\0') || raw.split('/').any(|s| s.is_empty() || s == ".")
    {
        return Err(RecordSkip::BadPath);
    }
    match VaultPath::parse(raw) {
        Ok(path) if path.is_root() => Err(RecordSkip::SelfPath),
        Ok(path) => Ok(path),
        Err(_) => Err(RecordSkip::BadPath),
    }
}

fn hex_commit(text: &str) -> bool {
    (4..=64).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Reads the record text (pure).
#[must_use]
#[allow(clippy::too_many_lines)] // one pass over the file: every reason of `projects.md` §2.2 in order
pub fn parse(bytes: &[u8]) -> RecordRead {
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return RecordRead::Unreadable {
            line: 1,
            message: "the file is larger than 1 MiB".to_owned(),
        };
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let line = std::str::from_utf8(&bytes[..error.valid_up_to()])
                .map_or(0, |valid| valid.matches('\n').count())
                + 1;
            return RecordRead::Unreadable {
                line: u32::try_from(line).unwrap_or(u32::MAX),
                message: "the file is not UTF-8 text".to_owned(),
            };
        }
    };
    let (_, text) = strip_bom(text);
    let scanned = match scan(text) {
        Ok(s) => s,
        Err(read) => return read,
    };
    let lines = &scanned.lines;
    let version = match scanned.doc.get("version") {
        None => VERSION,
        Some(item) => match item.as_integer() {
            Some(v) if v >= 1 => v,
            _ => {
                let at = item.span().map_or(0, |s| s.start);
                return unreadable(lines, at, "version is not a number PerMark knows");
            }
        },
    };
    let aot = match scanned.doc.get("project") {
        None => None,
        Some(Item::ArrayOfTables(aot)) => Some(aot),
        Some(item) => {
            let at = item.span().map_or(0, |s| s.start);
            return unreadable(lines, at, "project is not a list of [[project]] tables");
        }
    };
    let mut record = Record {
        version,
        ..Record::default()
    };
    let mut keys = std::collections::HashSet::new();
    let mut ids = std::collections::HashSet::new();
    for (table_index, table) in aot.into_iter().flat_map(|a| a.iter()).enumerate() {
        let line = lines.line(table.span().map_or(0, |s| s.start));
        let skip = |reason| SkippedEntry { line, reason };
        if record.entries.len() >= MAX_ENTRIES {
            record.skipped.push(skip(RecordSkip::TooMany));
            continue;
        }
        let path = match table.get("path").and_then(Item::as_str) {
            None => Err(RecordSkip::BadPath),
            Some(raw) => check_path(raw),
        };
        let path = match path {
            Ok(path) => path,
            Err(reason) => {
                record.skipped.push(skip(reason));
                continue;
            }
        };
        if !keys.insert(path.key()) {
            record.skipped.push(skip(RecordSkip::Duplicate));
            continue;
        }
        let (uid, derived) = match table.get("id") {
            None => (ProjectUid::derived(&path.key()), true),
            Some(item) => {
                let Some(uid) = item.as_str().and_then(ProjectUid::parse) else {
                    record.skipped.push(skip(RecordSkip::BadId));
                    continue;
                };
                (uid, false)
            }
        };
        if !ids.insert(uid.clone()) {
            record.skipped.push(skip(RecordSkip::BadId));
            continue;
        }
        let (hidden, hidden_unknown) = match table.get("hidden") {
            None => (HiddenRule::Own, false),
            Some(item) => match item.as_str() {
                Some("own") => (HiddenRule::Own, false),
                Some("own+root") => (HiddenRule::OwnRoot, false),
                _ => (HiddenRule::OwnRoot, true),
            },
        };
        let first_commit = table
            .get("first_commit")
            .and_then(Item::as_str)
            .filter(|t| hex_commit(t))
            .map(str::to_owned);
        record.entries.push(Entry {
            uid,
            derived,
            path,
            hidden,
            hidden_unknown,
            first_commit,
            line,
            table: table_index,
        });
    }
    if version > VERSION {
        RecordRead::Newer(record)
    } else {
        RecordRead::Read(record)
    }
}

// ---- ops (text) --------------------------------------------------------------------------------------------------

/// The text of a TOML basic string (quotes, escapes).
pub(super) fn quoted(text: &str) -> String {
    toml_edit::Value::from(text).to_string().trim().to_owned()
}

pub(super) fn eol_of(text: &str) -> &'static str {
    match text.find('\n') {
        Some(i) if i > 0 && text.as_bytes()[i - 1] == b'\r' => "\r\n",
        _ => "\n",
    }
}

/// A new record's text (header + version), in `eol`.
#[must_use]
pub fn new_text(eol: &str) -> String {
    format!("{HEADER}{eol}version = {VERSION}{eol}")
}

/// Applies `ops` to the record text (fresh from disk). Ops naming an entry the text does not hold change nothing.
/// The caller refuses records that are not `Read`.
#[must_use]
pub fn apply(text: &str, ops: &[RecordOp]) -> String {
    let (bom, body) = strip_bom(text);
    let ended_bare = !body.is_empty() && !body.ends_with('\n');
    let eol = eol_of(body);
    let mut out = body.to_owned();
    for op in ops {
        out = apply_one(&out, op, eol);
    }
    if ended_bare {
        // a file without a final newline keeps none (only what PerMark added ends with one)
        if let Some(stripped) = out.strip_suffix(eol) {
            out = stripped.to_owned();
        }
    }
    format!("{bom}{out}")
}

fn entry_of<'r>(record: &'r Record, uid: &ProjectUid) -> Option<&'r Entry> {
    record.entries.iter().find(|e| &e.uid == uid)
}

pub(super) fn splice(text: &str, range: Range<usize>, with: &str) -> String {
    let mut out = String::with_capacity(text.len() + with.len());
    out.push_str(&text[..range.start]);
    out.push_str(with);
    out.push_str(&text[range.end..]);
    out
}

fn apply_one(text: &str, op: &RecordOp, eol: &str) -> String {
    let RecordRead::Read(record) = parse(text.as_bytes()) else {
        return text.to_owned();
    };
    let Ok(scanned) = scan(text) else {
        return text.to_owned();
    };
    let tables = tables_of(&scanned, text.len());
    if let RecordOp::Add { uid, path, hidden } = op {
        return add(text, &tables, uid, path, *hidden, eol);
    }
    let uid = match op {
        RecordOp::Add { uid, .. }
        | RecordOp::Remove { uid }
        | RecordOp::SetPath { uid, .. }
        | RecordOp::SetHidden { uid, .. }
        | RecordOp::SetFirstCommit { uid, .. } => uid,
    };
    let Some(entry) = entry_of(&record, uid) else {
        return text.to_owned();
    };
    let Some(at) = tables.get(entry.table) else {
        return text.to_owned();
    };
    if let RecordOp::Remove { .. } = op {
        return remove(text, at, eol);
    }
    if entry.derived {
        // the id goes in first; the op runs on the new text
        let line = format!("id = {}{eol}", quoted(uid.as_str()));
        let with_id = splice(text, at.header_end..at.header_end, &line);
        return apply_one(&with_id, op, eol);
    }
    match op {
        RecordOp::SetPath { path, .. } => at.path.as_ref().map_or_else(
            || text.to_owned(),
            |k| splice(text, k.value.clone(), &quoted(path.as_str())),
        ),
        RecordOp::SetHidden { rule, .. } => match (rule, &at.hidden) {
            (HiddenRule::Own, Some(k)) => splice(text, k.lines.clone(), ""),
            (HiddenRule::Own, None) => text.to_owned(),
            (HiddenRule::OwnRoot, Some(k)) => splice(text, k.value.clone(), &quoted("own+root")),
            (HiddenRule::OwnRoot, None) => {
                let after = at.path.as_ref().map_or(at.header_end, |k| k.lines.end);
                let line = format!("hidden = {}{eol}", quoted("own+root"));
                splice(text, after..after, &line)
            }
        },
        RecordOp::SetFirstCommit { oid, .. } => {
            if let Some(k) = &at.first_commit {
                splice(text, k.value.clone(), &quoted(oid))
            } else {
                let after = at
                    .hidden
                    .as_ref()
                    .or(at.path.as_ref())
                    .map_or(at.header_end, |k| k.lines.end);
                let line = format!("first_commit = {}{eol}", quoted(oid));
                splice(text, after..after, &line)
            }
        }
        RecordOp::Add { .. } | RecordOp::Remove { .. } => text.to_owned(),
    }
}

fn block(uid: &ProjectUid, path: &VaultPath, hidden: HiddenRule, eol: &str) -> String {
    let mut out = format!(
        "[[project]]{eol}id = {}{eol}path = {}{eol}",
        quoted(uid.as_str()),
        quoted(path.as_str())
    );
    if hidden == HiddenRule::OwnRoot {
        out.push_str("hidden = ");
        out.push_str(&quoted("own+root"));
        out.push_str(eol);
    }
    out
}

fn add(
    text: &str,
    tables: &[TableAt],
    uid: &ProjectUid,
    path: &VaultPath,
    hidden: HiddenRule,
    eol: &str,
) -> String {
    let body = block(uid, path, hidden, eol);
    let at = tables.last().map_or(text.len(), |t| t.region.end);
    let before = &text[..at];
    let mut insert = String::new();
    if !before.is_empty() && !before.ends_with('\n') {
        insert.push_str(eol);
    }
    // one blank line before the new entry
    if !before.is_empty() && !before.ends_with(&format!("{eol}{eol}")) {
        insert.push_str(eol);
    }
    insert.push_str(&body);
    if at < text.len() && !text[at..].starts_with(eol) {
        // something follows (an unknown table): keep a blank line between
        insert.push_str(eol);
    }
    splice(text, at..at, &insert)
}

fn remove(text: &str, at: &TableAt, eol: &str) -> String {
    let blank = format!("{eol}{eol}");
    let mut out = splice(text, at.region.clone(), "");
    let p = at.region.start;
    let blank_before = out[..p].ends_with(&blank) || p == 0;
    if blank_before {
        if out[p..].starts_with(eol) {
            out = splice(&out, p..p + eol.len(), "");
        } else if p == out.len() && p >= eol.len() && out[..p].ends_with(&blank) {
            out = splice(&out, p - eol.len()..p, "");
        }
    }
    out
}

// ---- disk --------------------------------------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    /// Record unreadable | newer, `.permark/` | the record a link or a file.
    #[error("the project list cannot be changed now")]
    Unavailable,
    /// The file changed twice while PerMark wrote it.
    #[error("the project list changed while it was saved")]
    Conflict,
    #[error("cannot write {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

impl From<RecordError> for crate::IpcError {
    fn from(error: RecordError) -> Self {
        use crate::{ErrorKind, IpcError};
        match error {
            RecordError::Unavailable => IpcError::new(ErrorKind::Unavailable).subject("projects"),
            RecordError::Conflict => IpcError::new(ErrorKind::Conflict).subject("projects"),
            RecordError::Io { path, source } => IpcError::io(path, &source),
        }
    }
}

/// `<root>/.permark/projects.toml`.
#[must_use]
pub fn record_path(root: &Path) -> PathBuf {
    root.join(crate::state::PERMARK_DIR).join(RECORD_FILE)
}

pub(super) fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| crate::tree::is_link_meta(&m))
}

/// Reads the record of `root` from disk.
#[must_use]
pub fn read(root: &Path) -> RecordRead {
    let path = record_path(root);
    let permark = root.join(crate::state::PERMARK_DIR);
    if !std::fs::symlink_metadata(&permark)
        .is_ok_and(|m| m.is_dir() && !crate::tree::is_link_meta(&m))
    {
        return RecordRead::Absent;
    }
    if is_link(&path) {
        return RecordRead::Unreadable {
            line: 1,
            message: "the project list is a link".to_owned(),
        };
    }
    match crate::fsx::read_bytes(&path, MAX_RECORD_BYTES) {
        Ok(bytes) => parse(&bytes),
        Err(crate::fsx::FsxError::TooLarge { .. }) => RecordRead::Unreadable {
            line: 1,
            message: "the file is larger than 1 MiB".to_owned(),
        },
        Err(crate::fsx::FsxError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            RecordRead::Absent
        }
        Err(_) => RecordRead::Unreadable {
            line: 1,
            message: "the file cannot be read".to_owned(),
        },
    }
}

/// What a write did: the bytes now on disk (their hash announced as the app's own write) + whether it made the file.
#[derive(Debug, Clone)]
pub struct Written {
    pub bytes: Vec<u8>,
    pub created: bool,
}

/// Applies `ops` to the record of `root` on disk: read fresh, spliced, written atomically when the file is still the
/// one read (else read + applied once more; a second change -> `Conflict`, nothing written). `create`: a missing
/// record is made (header + version). Needs a real `.permark/`.
///
/// # Errors
/// `Unavailable` (unreadable | newer record, `.permark/` missing | a link | a file, record a link), `Conflict`, `Io`.
pub fn write(
    root: &Path,
    ops: &[RecordOp],
    create: bool,
    announce: &dyn Fn(&[u8]),
) -> Result<Written, RecordError> {
    let permark = root.join(crate::state::PERMARK_DIR);
    let real = std::fs::symlink_metadata(&permark)
        .is_ok_and(|m| m.is_dir() && !crate::tree::is_link_meta(&m));
    let path = record_path(root);
    if !real || is_link(&path) {
        return Err(RecordError::Unavailable);
    }
    let io = |source| RecordError::Io {
        path: path.display().to_string(),
        source,
    };
    for _attempt in 0..2 {
        let (before, created) = match std::fs::read(&path) {
            Ok(bytes) => (Some(bytes), false),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => (None, true),
            Err(e) => return Err(io(e)),
        };
        let text = match &before {
            Some(bytes) => {
                if !matches!(parse(bytes), RecordRead::Read(_)) {
                    return Err(RecordError::Unavailable);
                }
                String::from_utf8_lossy(bytes).into_owned()
            }
            None => new_text("\n"),
        };
        let after = apply(&text, ops).into_bytes();
        let seen = before.as_deref().map(crate::fsx::hash);
        announce(&after);
        let done = crate::fsx::write_atomic_checked(&path, &after, || {
            Ok(match std::fs::read(&path) {
                Ok(now) => Some(crate::fsx::hash(&now)) == seen,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => seen.is_none(),
                Err(e) => return Err(e),
            })
        })
        .map_err(io)?;
        if done {
            return Ok(Written {
                bytes: after,
                created,
            });
        }
    }
    Err(RecordError::Conflict)
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
