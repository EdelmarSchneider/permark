//! The hand-off: the person marks a point, PerMark copies one line for the prompt, and the agent asks `activity` what
//! the person did since then instead of rereading the project. The preview shows the sessions (S1..S7), the changes
//! made outside and the size; confirming freezes the package (the agent reads exactly what the person saw) and sets
//! the checkpoint. `activity` with `since=` reads net rows straight from the journal.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::time::UNIX_EPOCH;

use permark_core::activity::handoff::{entries_of, scope, session_of, Case, Scope, Session};
use permark_core::activity::net::{net, net_outside, verify, Disk, Flag, Kind, NetOptions, Row};
use permark_core::activity::{
    By, CommentOp, Entry, Journal, OutsideChange, ReviewOp, OUTSIDE_DETAIL_FILES,
};
use permark_core::format::sections;
use permark_core::organizing::{CommentFilter, CommentsApi};
use permark_core::paths::{Vault, VaultPath};
use permark_core::plugins::RepoHandle;
use permark_core::tokens::estimate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::format::{self, CEILING_CHARS};
use crate::guard::Guard;
use crate::render::activity_text;
use crate::tools::{read_text, Tools};
use crate::ToolError;

/// Rows `activity` lists by default, and at most.
pub const LIMIT: (usize, usize) = (50, 200);

/// The disk and the state now, for the check of each fact.
struct ProjectDisk<'a> {
    vault: &'a Vault,
    guard: &'a Guard,
    comments: Option<Vec<(String, bool)>>,
}

impl ProjectDisk<'_> {
    fn abs(&self, path: &str) -> Option<std::path::PathBuf> {
        if self.guard.hidden(path, false) {
            return None;
        }
        let parsed = VaultPath::parse(path).ok()?;
        self.vault.resolve(&parsed).ok()
    }
}

impl Disk for ProjectDisk<'_> {
    fn exists(&self, path: &str) -> bool {
        self.abs(path).is_some_and(|abs| abs.exists())
    }

    fn headings(&self, path: &str) -> Option<Vec<String>> {
        let text = read_text(&self.abs(path)?).ok()?;
        Some(
            sections(&text)
                .into_iter()
                .filter(|s| !s.heading_path.is_empty())
                .map(|s| s.heading_path.join(" > "))
                .collect(),
        )
    }

    fn modified(&self, path: &str) -> Option<u64> {
        let meta = std::fs::metadata(self.abs(path)?).ok()?;
        let at = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        u64::try_from(at.as_millis()).ok()
    }

    fn comment_done(&self, id: &str) -> Option<bool> {
        self.comments
            .as_ref()?
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, done)| *done)
    }
}

fn disk<'a>(tools: &'a Tools, guard: &'a Guard) -> ProjectDisk<'a> {
    let comments = tools
        .services()
        .services
        .get::<CommentsApi>()
        .and_then(|api| api.0.list(CommentFilter::All).ok())
        .map(|list| list.into_iter().map(|c| (c.id, c.done)).collect());
    ProjectDisk {
        vault: tools.vault(),
        guard,
        comments,
    }
}

fn parent_dir(path: &str) -> String {
    path.rfind('/')
        .map_or_else(|| "./".to_owned(), |at| format!("{}/", &path[..at]))
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `kind` + `detail` of a row as the table shows them.
fn words(kind: &Kind) -> (&'static str, String) {
    match kind {
        Kind::New => ("new", String::new()),
        Kind::Imported => ("imported", String::new()),
        Kind::Deleted => ("deleted", String::new()),
        Kind::Renamed { from } => ("renamed", format!("from={}", format::value(from))),
        Kind::Moved { from } => ("moved", format!("from={}", format::value(from))),
        Kind::Edited { sections, blocks } => {
            let mut detail = sections
                .iter()
                .map(|s| one_line(s))
                .collect::<Vec<_>>()
                .join("; ");
            if !detail.is_empty() {
                detail.push(' ');
            }
            let _ = write!(detail, "blocks={blocks}");
            ("edited", detail)
        }
        Kind::Converted { to, saved } => (
            "converted",
            match saved {
                Some(saved) => format!("to={to} saved={}", format::value(saved)),
                None => format!("to={to} copied"),
            },
        ),
        Kind::Comment { op, id, heading } => {
            let op = match op {
                CommentOp::Add => "added",
                CommentOp::Reply => "replied",
                CommentOp::Done => "done",
                CommentOp::Reopen => "reopened",
                CommentOp::Remove => "removed",
            };
            let mut detail = format!("{op} id={id}");
            if let Some(heading) = heading {
                let _ = write!(detail, " at={}", format::value(&one_line(heading)));
            }
            ("comment", detail)
        }
        Kind::Tag { added, tag } => ("tag", format!("{}{tag}", if *added { "+" } else { "-" })),
        Kind::Review { op, section } => {
            let op = match op {
                ReviewOp::Keep => "kept",
                ReviewOp::Revert => "reverted",
            };
            (
                "review",
                match section {
                    Some(section) => format!("{op} {}", one_line(section)),
                    None => format!("{op} all"),
                },
            )
        }
        Kind::Outside { change, from } => {
            let word = match change {
                OutsideChange::Created => "created",
                OutsideChange::Modified => "modified",
                OutsideChange::Deleted => "deleted",
                OutsideChange::Renamed => "renamed",
            };
            (
                word,
                from.as_deref()
                    .map(|f| format!("from={}", format::value(f)))
                    .unwrap_or_default(),
            )
        }
    }
}

fn flag_words(flags: &[Flag]) -> Vec<String> {
    flags
        .iter()
        .map(|f| match f {
            Flag::Gone => "gone".to_owned(),
            Flag::MovedTo(p) => format!("moved_to={p}"),
            Flag::SectionGone(h) => format!("section_gone={}", one_line(h)),
            Flag::ChangedSince => "changed_since".to_owned(),
        })
        .collect()
}

/// Paths a row names (its own, where it came from, where it went): a hidden one hides the row.
fn named(row: &Row) -> Vec<&str> {
    let mut paths = vec![row.path.as_str()];
    match &row.kind {
        Kind::Renamed { from: other }
        | Kind::Moved { from: other }
        | Kind::Converted {
            saved: Some(other), ..
        }
        | Kind::Outside {
            from: Some(other), ..
        } => paths.push(other),
        _ => {}
    }
    for flag in &row.flags {
        if let Flag::MovedTo(p) = flag {
            paths.push(p);
        }
    }
    paths
}

fn row_value(row: &Row) -> Value {
    let (kind, detail) = words(&row.kind);
    let mut value = json!({
        "seq": row.seq,
        "time": format::minute(row.at),
        "by": match row.by { By::Person => "person", By::Outside => "outside" },
        "kind": kind,
        "path": row.path,
    });
    if !detail.is_empty() {
        value["detail"] = json!(detail);
    }
    let flags = flag_words(&row.flags);
    if !flags.is_empty() {
        value["flags"] = json!(flags);
    }
    value
}

/// Rows the agent may see (hidden paths out) and how many were hidden.
fn shown(rows: Vec<Row>, guard: &Guard) -> (Vec<Row>, usize) {
    let before = rows.len();
    let kept: Vec<Row> = rows
        .into_iter()
        .filter(|r| !named(r).iter().any(|p| guard.hidden(p, false)))
        .collect();
    let hidden = before - kept.len();
    (kept, hidden)
}

/// A frozen package as it is served now: rows naming a path hidden since it was frozen are left out and counted in
/// `hidden=` (the counts of the header and of each sub-table follow). Byte for byte the frozen text when none is.
pub(crate) fn frozen_now(text: &str, hidden: &dyn Fn(&str, bool) -> bool) -> String {
    let mut lines = text.split_inclusive('\n');
    let Some(head) = lines.next() else {
        return String::new();
    };
    let mut out: Vec<String> = Vec::new();
    // the open sub-table: its line in `out`, whether it counts folders, rows kept | left out
    let mut open: Option<(usize, bool, usize, usize)> = None;
    let (mut left_rows, mut left_folders, mut gone_sessions, mut kept_any) = (0, 0, 0, false);
    let mut close = |out: &mut Vec<String>, open: Option<(usize, bool, usize, usize)>| {
        let Some((at, _, kept, left)) = open else {
            return;
        };
        if left == 0 {
            return;
        }
        if kept == 0 {
            if out[at].starts_with("-- session ") {
                gone_sessions += 1;
            }
            out.remove(at);
        } else {
            let key = if out[at].starts_with("-- session ") {
                "rows"
            } else {
                "n"
            };
            out[at] = with_count(&out[at], key, Some(kept), &[]);
        }
    };
    for line in lines {
        if line.starts_with("-- ") {
            close(&mut out, open.take());
            open = Some((out.len(), line.starts_with("-- outside_folders"), 0, 0));
            out.push(line.to_owned());
            continue;
        }
        if !line.contains('\t') {
            out.push(line.to_owned());
            continue;
        }
        let folders = open.is_some_and(|o| o.1);
        let gone = if folders {
            let folder = line.split('\t').next().unwrap_or_default();
            let folder = uncell(folder.trim_end_matches('/'));
            folder != "." && hidden(&folder, true)
        } else {
            row_paths(line).iter().any(|p| hidden(p, false))
        };
        if let Some(o) = open.as_mut() {
            if gone {
                o.3 += 1;
            } else {
                o.2 += 1;
            }
        }
        if gone {
            if folders {
                left_folders += 1;
            } else {
                left_rows += 1;
            }
        } else {
            kept_any = true;
            out.push(line.to_owned());
        }
    }
    close(&mut out, open.take());
    if left_rows + left_folders == 0 {
        return text.to_owned();
    }
    if !kept_any {
        out.retain(|l| !l.starts_with("#cols "));
    }
    let mut first = head.to_owned();
    let count = |line: &str, key: &str| -> usize {
        line.trim_end()
            .split(' ')
            .find_map(|t| t.strip_prefix(key)?.strip_prefix('=')?.parse().ok())
            .unwrap_or(0)
    };
    let n = count(&first, "n").saturating_sub(left_rows);
    let sessions = count(&first, "sessions").saturating_sub(gone_sessions);
    let was_hidden = count(&first, "hidden");
    first = with_count(&first, "n", Some(n), &[]);
    first = with_count(
        &first,
        "sessions",
        (sessions > 0).then_some(sessions),
        &["n"],
    );
    first = with_count(
        &first,
        "hidden",
        Some(was_hidden + left_rows + left_folders),
        &["n", "sessions"],
    );
    first + &out.concat()
}

/// A header line with `key=count` set (`None`: removed); a new one goes after the last of `after` it holds.
fn with_count(line: &str, key: &str, count: Option<usize>, after: &[&str]) -> String {
    let (body, end) = line.split_at(line.trim_end_matches(['\n', '\r']).len());
    let is = |t: &str, k: &str| {
        t.strip_prefix(k)
            .and_then(|r| r.strip_prefix('='))
            .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
    };
    let mut tokens: Vec<String> = body.split(' ').map(str::to_owned).collect();
    let set = count.map(|c| format!("{key}={c}"));
    if let Some(at) = tokens.iter().position(|t| is(t, key)) {
        match set {
            Some(t) => tokens[at] = t,
            None => {
                tokens.remove(at);
            }
        }
    } else if let Some(t) = set {
        let at = tokens
            .iter()
            .rposition(|t| after.iter().any(|k| is(t, k)))
            .map_or(tokens.len(), |at| at + 1);
        tokens.insert(at, t);
    }
    tokens.join(" ") + end
}

/// The paths an activity row line names: its path, where it came from (`from=` | `saved=`), where it went.
fn row_paths(line: &str) -> Vec<String> {
    let fields: Vec<&str> = line.trim_end_matches(['\n', '\r']).split('\t').collect();
    let mut paths = Vec::new();
    if let Some(path) = fields.get(4) {
        paths.push(uncell(path));
    }
    let detail = uncell(fields.get(5).copied().unwrap_or_default());
    let other = match fields.get(3).copied() {
        Some("renamed" | "moved") => detail.strip_prefix("from="),
        Some("converted") => detail.find("saved=").map(|at| &detail[at + 6..]),
        _ => None,
    };
    if let Some(rest) = other {
        paths.push(header_value(rest));
    }
    let flags = uncell(fields.get(6).copied().unwrap_or_default());
    let mut rest = flags.as_str();
    while let Some(at) = rest.find("moved_to=") {
        if at > 0 && !rest[..at].ends_with(',') {
            rest = &rest[at + 9..];
            continue;
        }
        let tail = &rest[at + 9..];
        let end = [",gone", ",section_gone=", ",changed_since", ",moved_to="]
            .iter()
            .filter_map(|f| {
                tail.match_indices(f).map(|(i, _)| i).find(|i| {
                    tail[i + f.len()..].is_empty()
                        || f.ends_with('=')
                        || tail[i + f.len()..].starts_with(',')
                })
            })
            .min()
            .unwrap_or(tail.len());
        paths.push(tail[..end].to_owned());
        rest = &tail[end..];
    }
    paths
}

/// A header value read back: a JSON string, or the text up to the first space.
fn header_value(text: &str) -> String {
    if text.starts_with('"') {
        if let Some(Ok(value)) = serde_json::Deserializer::from_str(text)
            .into_iter::<String>()
            .next()
        {
            return value;
        }
    }
    text.split(' ').next().unwrap_or_default().to_owned()
}

/// A table cell read back (`\t`, `\n`, `\r`, `\\` unescaped).
fn uncell(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}
/// The changes outside: one row per file when few (or the person touched it), else counted per folder.
struct Outside {
    rows: Vec<Row>,
    folders: Vec<Value>,
    files: usize,
}

fn outside_of(
    entries: &[Entry],
    touched: &HashSet<String>,
    guard: &Guard,
    by_folder: bool,
) -> (Outside, usize) {
    let (rows, hidden) = shown(net_outside(entries), guard);
    let files = rows.len();
    if !by_folder && files <= OUTSIDE_DETAIL_FILES {
        return (
            Outside {
                rows,
                folders: Vec::new(),
                files,
            },
            hidden,
        );
    }
    let mut detailed = Vec::new();
    let mut counts: BTreeMap<String, [u64; 3]> = BTreeMap::new();
    for row in rows {
        if touched.contains(&row.path) && !by_folder {
            detailed.push(row);
            continue;
        }
        let slot = match &row.kind {
            Kind::Outside {
                change: OutsideChange::Created,
                ..
            } => 0,
            Kind::Outside {
                change: OutsideChange::Deleted,
                ..
            } => 2,
            _ => 1,
        };
        counts.entry(parent_dir(&row.path)).or_default()[slot] += 1;
    }
    let folders = counts
        .into_iter()
        .map(|(folder, [c, m, d])| json!({ "folder": folder, "created": c, "modified": m, "deleted": d }))
        .collect();
    (
        Outside {
            rows: detailed,
            folders,
            files,
        },
        hidden,
    )
}

/// `HEAD old..new (N commits)` when the repository moved since the last hand-off.
fn git_fact(tools: &Tools, journal: &Journal) -> Option<String> {
    let reader = tools.services().services.get::<RepoHandle>()?;
    let now = reader.0.head()?;
    let old = journal.checkpoint.as_ref()?.head.clone()?;
    if old == now {
        return None;
    }
    let short = |oid: &str| oid.chars().take(7).collect::<String>();
    Some(match reader.0.commits_between(&old, &now) {
        Some(1) => format!("HEAD {}..{} (1 commit)", short(&old), short(&now)),
        Some(n) => format!("HEAD {}..{} ({n} commits)", short(&old), short(&now)),
        None => format!("HEAD {}..{}", short(&old), short(&now)),
    })
}

/// What one session of a package holds.
struct Packed {
    id: u32,
    rows: Vec<Row>,
}

/// A package built for a choice of sessions.
struct Built {
    data: Value,
    text: String,
    sessions: Vec<Packed>,
    outside_files: usize,
    by_folder: bool,
    /// Sessions left out to fit the ceiling (oldest first).
    dropped: Vec<u32>,
    rows: usize,
}

fn session_value(s: &Session, rows: &[Row]) -> Value {
    json!({
        "id": s.id,
        "from": format::minute(s.start),
        "to": format::minute(s.end),
        "rows": rows.iter().map(row_value).collect::<Vec<_>>(),
    })
}

/// Builds the package of the chosen sessions: netted, checked against the disk, hidden paths out, within the ceiling
/// (changes outside counted per folder first, then the oldest sessions dropped).
fn build(tools: &Tools, guard: &Guard, journal: &Journal, scope: &Scope, chosen: &[u32]) -> Built {
    let disk = disk(tools, guard);
    let git = git_fact(tools, journal);
    let mut chosen: Vec<u32> = chosen.to_vec();
    chosen.sort_unstable();
    let mut dropped = Vec::new();
    let mut by_folder = false;
    loop {
        let person = entries_of(scope, &chosen);
        let rows = verify(
            net(&person, NetOptions::default()),
            &disk,
            &journal.entries,
            NetOptions::default(),
        );
        let (rows, hidden_person) = shown(rows, guard);
        let touched: HashSet<String> = rows.iter().map(|r| r.path.clone()).collect();
        let (outside, hidden_outside) = outside_of(&scope.outside, &touched, guard, by_folder);
        let mut packed: Vec<Packed> = Vec::new();
        for row in rows {
            let id = session_of(&scope.sessions, row.seq).unwrap_or(0);
            match packed.iter_mut().find(|p| p.id == id) {
                Some(p) => p.rows.push(row),
                None => packed.push(Packed {
                    id,
                    rows: vec![row],
                }),
            }
        }
        packed.sort_by_key(|p| p.id);
        let sessions: Vec<Value> = packed
            .iter()
            .filter_map(|p| {
                scope
                    .sessions
                    .iter()
                    .find(|s| s.id == p.id)
                    .map(|s| session_value(s, &p.rows))
            })
            .collect();
        let rows_count = packed.iter().map(|p| p.rows.len()).sum::<usize>() + outside.rows.len();
        let mut data = json!({
            "since": "handoff",
            "tz": format::tz(),
            "sessions": sessions,
            "outside": outside.rows.iter().map(row_value).collect::<Vec<_>>(),
        });
        if !outside.folders.is_empty() {
            data["outside_folders"] = json!(outside.folders);
        }
        if let Some(git) = &git {
            data["git"] = json!(git);
        }
        let hidden = hidden_person + hidden_outside;
        if hidden > 0 {
            data["hidden"] = json!(hidden);
        }
        for (key, on) in [
            ("trimmed", scope.trimmed),
            ("upgraded", scope.upgraded),
            (
                "older_unchecked",
                scope.all_stale || scope.sessions.iter().any(|s| !chosen.contains(&s.id)),
            ),
            ("outside_only", scope.case == Case::OutsideOnly),
        ] {
            if on {
                data[key] = json!(true);
            }
        }
        let text = activity_text(&data);
        let fits = text.chars().count() <= CEILING_CHARS;
        if fits || (by_folder && chosen.len() <= 1) {
            return Built {
                text: format::ceiling(text),
                data,
                sessions: packed,
                outside_files: outside.files,
                by_folder,
                dropped,
                rows: rows_count,
            };
        }
        if !by_folder && outside.files > 0 {
            by_folder = true;
        } else if chosen.len() > 1 {
            dropped.push(chosen.remove(0));
        } else {
            by_folder = true;
        }
    }
}

/// One session as the dialog lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "HandOffSessionView")]
pub struct SessionView {
    pub id: u32,
    #[ts(type = "number")]
    pub start: u64,
    #[ts(type = "number")]
    pub end: u64,
    /// The person's actions in it (before netting).
    pub actions: u32,
    /// Rows it adds to the package (after netting), when checked.
    pub rows: u32,
    /// Approximate tokens of its rows.
    #[ts(type = "number")]
    pub tokens: u64,
    pub stale: bool,
    pub checked: bool,
    /// Left out so the package fits.
    pub dropped: bool,
}

/// The changes outside as the dialog says them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "HandOffOutside")]
pub struct OutsideView {
    pub files: u32,
    /// Counted per folder (too many to list one by one).
    pub by_folder: bool,
    pub git: Option<String>,
}

/// What the hand-off dialog shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum HandOffPreview {
    /// S5: nothing since the last hand-off (no dialog).
    Nothing,
    #[serde(rename_all = "camelCase")]
    Ready {
        case: Case,
        first: bool,
        sessions: Vec<SessionView>,
        outside: OutsideView,
        /// Every session is stale: the latest is checked, older work is not.
        all_stale: bool,
        trimmed: bool,
        upgraded: bool,
        rows: u32,
        #[ts(type = "number")]
        tokens: u64,
        /// What a confirm hands off (the journal seq the preview was built from).
        #[ts(type = "number")]
        upto: u64,
    },
}

/// What a confirmed hand-off gives the window to copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HandOffDone {
    /// The line for the prompt.
    pub line: String,
    /// The whole package (for agents without the connection).
    pub full: String,
    pub rows: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewArgs {
    /// The sessions the person checked; none: the default.
    #[serde(default)]
    pub checked: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmArgs {
    pub upto: u64,
    pub checked: Vec<u32>,
}

/// The scope as the preview saw it: the journal up to `upto`.
fn scope_upto(journal: &Journal, upto: u64, now: u64) -> Scope {
    let mut seen = journal.clone();
    seen.entries.retain(|e| e.seq <= upto);
    scope(&seen, now)
}

/// The hand-off dialog's content (`checked`: the person's choice of sessions, else the default).
#[must_use]
pub fn preview(tools: &Tools, guard: &Guard, checked: Option<&[u32]>) -> HandOffPreview {
    let activity = &tools.services().activity;
    let journal = activity.snapshot();
    let scope = scope(&journal, activity.now());
    if scope.case == Case::Nothing {
        return HandOffPreview::Nothing;
    }
    let chosen: Vec<u32> = checked.map_or_else(
        || {
            scope
                .sessions
                .iter()
                .filter(|s| s.checked)
                .map(|s| s.id)
                .collect()
        },
        <[u32]>::to_vec,
    );
    let built = build(tools, guard, &journal, &scope, &chosen);
    let sessions = scope
        .sessions
        .iter()
        .map(|s| {
            let rows = built
                .sessions
                .iter()
                .find(|p| p.id == s.id)
                .map_or(&[][..], |p| p.rows.as_slice());
            let tokens = estimate(&activity_text(
                &json!({ "since": "handoff", "tz": "", "sessions": [session_value(s, rows)] }),
            ));
            SessionView {
                id: s.id,
                start: s.start,
                end: s.end,
                actions: s.actions,
                rows: u32::try_from(rows.len()).unwrap_or(u32::MAX),
                tokens,
                stale: s.stale,
                checked: chosen.contains(&s.id),
                dropped: built.dropped.contains(&s.id),
            }
        })
        .collect();
    HandOffPreview::Ready {
        case: scope.case,
        first: journal.checkpoint.is_none(),
        sessions,
        outside: OutsideView {
            files: u32::try_from(built.outside_files).unwrap_or(u32::MAX),
            by_folder: built.by_folder,
            git: built.data["git"].as_str().map(str::to_owned),
        },
        all_stale: scope.all_stale,
        trimmed: scope.trimmed,
        upgraded: scope.upgraded,
        rows: u32::try_from(built.rows).unwrap_or(u32::MAX),
        tokens: estimate(&built.text),
        upto: scope.upto,
    }
}

/// The line the person pastes in the prompt.
#[must_use]
pub fn prompt_line(rows: u32, outside_only: bool) -> String {
    if outside_only {
        format!("PerMark: {rows} changes outside PerMark since last hand-off -> call activity since=handoff")
    } else {
        format!("PerMark: {rows} actions since last hand-off -> call activity since=handoff")
    }
}

/// Confirms a hand-off: the package of what the preview showed is frozen, the checkpoint set.
///
/// # Errors
/// `Invalid` when nothing is left to hand off (S5) | the journal cannot be written (a reader with no window).
pub fn confirm(
    tools: &Tools,
    guard: &Guard,
    upto: u64,
    checked: &[u32],
) -> Result<HandOffDone, ToolError> {
    let activity = &tools.services().activity;
    let journal = activity.snapshot();
    let scope = scope_upto(&journal, upto, activity.now());
    if scope.case == Case::Nothing {
        return Err(ToolError::Invalid(
            "nothing new since the last hand-off".to_owned(),
        ));
    }
    let built = build(tools, guard, &journal, &scope, checked);
    // the package says when it was handed off
    let mut data = built.data.clone();
    data["package"] = json!(format::minute(activity.now()));
    let text = format::ceiling(activity_text(&data));
    let rows = u32::try_from(built.rows).unwrap_or(u32::MAX);
    let head = tools
        .services()
        .services
        .get::<RepoHandle>()
        .and_then(|r| r.0.head());
    activity
        .hand_off(scope.upto, head, text.clone(), rows)
        .ok_or_else(|| ToolError::Failed("the activity journal is read-only here".to_owned()))?;
    Ok(HandOffDone {
        line: prompt_line(rows, scope.case == Case::OutsideOnly),
        full: text,
        rows,
    })
}

/// The `activity` tool: the latest package (default, `since=handoff`), or net rows since a time | seq.
///
/// # Errors
/// `Invalid` for an unreadable `since` | `by`.
pub fn activity_tool(
    tools: &Tools,
    guard: &Guard,
    since: Option<&str>,
    limit: Option<usize>,
    by: Option<&str>,
) -> Result<Value, ToolError> {
    let by = match by.map(str::trim) {
        None | Some("") => None,
        Some("person") => Some(By::Person),
        Some("outside") => Some(By::Outside),
        Some(other) => {
            return Err(ToolError::Invalid(format!(
                "by is person or outside, not {other:?}"
            )))
        }
    };
    let journal = tools.services().activity.snapshot();
    let since = since
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("handoff");
    if since == "handoff" && by.is_none() && limit.is_none() {
        return Ok(match &journal.package {
            Some(package) => json!({
                "frozen": frozen_now(&package.text, &|path, is_dir| guard.hidden(path, is_dir))
            }),
            None => json!({ "since": "handoff", "tz": format::tz(), "no_handoff": true }),
        });
    }
    // what the query starts after, and whether retention dropped entries it would have held
    let (after, trimmed): (After, bool) = if since == "handoff" {
        let seq = journal.checkpoint.as_ref().map_or(0, |c| c.seq);
        (
            Box::new(move |e| e.seq > seq),
            journal.trimmed_through > seq,
        )
    } else if let Ok(seq) = since.parse::<u64>() {
        (
            Box::new(move |e| e.seq > seq),
            journal.trimmed_through > seq,
        )
    } else if let Some(ms) = format::ms_of(since).or_else(|| local_minute(since)) {
        let oldest = journal.entries.first().map_or(u64::MAX, |e| e.at);
        (
            Box::new(move |e| e.at >= ms),
            journal.trimmed_through > 0 && oldest > ms,
        )
    } else {
        return Err(ToolError::Invalid(format!(
            "since is handoff, a seq or an ISO time, not {since:?}"
        )));
    };
    let entries: Vec<Entry> = journal
        .entries
        .iter()
        .filter(|e| after(e))
        .cloned()
        .collect();
    let disk = disk(tools, guard);
    let mut rows = Vec::new();
    if by != Some(By::Outside) {
        rows.extend(verify(
            net(&entries, NetOptions::default()),
            &disk,
            &journal.entries,
            NetOptions::default(),
        ));
    }
    if by != Some(By::Person) {
        rows.extend(net_outside(&entries));
    }
    rows.sort_by_key(|r| r.seq);
    let (rows, hidden) = shown(rows, guard);
    let max = limit.unwrap_or(LIMIT.0).clamp(1, LIMIT.1);
    let more = rows.len() > max;
    let listed: Vec<Value> = rows.iter().take(max).map(row_value).collect();
    let mut data = json!({
        "since": since,
        "tz": format::tz(),
        "rows": listed,
    });
    if hidden > 0 {
        data["hidden"] = json!(hidden);
    }
    if trimmed {
        data["trimmed"] = json!(true);
    }
    if more {
        data["next"] = json!(rows[max - 1].seq);
    }
    Ok(data)
}

/// Which entries a query keeps.
type After = Box<dyn Fn(&Entry) -> bool>;

/// `2026-10-19T14:05` (local time, no offset).
fn local_minute(text: &str) -> Option<u64> {
    let naive = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| {
            chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default())
        })
        .ok()?;
    let local = naive.and_local_timezone(chrono::Local).earliest()?;
    u64::try_from(local.timestamp_millis()).ok()
}
