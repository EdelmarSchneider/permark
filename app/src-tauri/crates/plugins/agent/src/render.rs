//! Each tool's answer as the agent reads it: one text block per `ai-format` (header, table, zones, cut markers),
//! drawn from the same data `--structured` hands out as JSON. `ERR` lines for errors. Pure: the app and `permark mcp`
//! draw the same text from the same data.

use std::fmt::Write as _;

use serde_json::Value;

use crate::format::{self, ceiling, rows_only, sub, table, zone, Header};
use crate::ToolError;

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn u(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

fn b(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    list(v, key)
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn n_of(items: &[Value]) -> usize {
    items.len()
}

/// `a-b` of a `[a, b]` pair.
fn range(v: Option<&Value>) -> Option<String> {
    let pair = v?.as_array()?;
    match (pair.first()?.as_u64(), pair.get(1)?.as_u64()) {
        (Some(a), Some(b)) => Some(format!("{a}-{b}")),
        _ => None,
    }
}

fn size(v: &Value, key: &str) -> String {
    u(v, key).map(format::tokens).unwrap_or_default()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn age_of(iso: &str, now: u64) -> String {
    format::ms_of(iso).map_or_else(String::new, |ms| format::age(now, ms))
}

/// The text of a tool's answer (cut at the ceiling).
#[must_use]
pub fn render(tool: &str, data: &Value) -> String {
    ceiling(render_at(tool, data, now_ms()))
}

/// [`render`] with the time ages are measured from.
#[must_use]
pub fn render_at(tool: &str, data: &Value, now: u64) -> String {
    match tool {
        "get_context" => get_context(data),
        "list_unread" => list_unread(data, now),
        "search" => search(data),
        "list_tags" => list_tags(data),
        "outline" => outline(data),
        "read" => read(data),
        "list_comments" => list_comments(data),
        "resolve_comment" => {
            let mut out = Header::new(tool)
                .kv("id", s(data, "resolved"))
                .flag("done", true)
                .line();
            out.push('\n');
            out
        }
        "list_notes" => list_notes(data, now),
        "read_note" => read_note(data),
        "open" => open(data),
        "map" => map(data),
        "activity" => activity(data),
        other => format!("{other}\n"),
    }
}

/// The one line of an error of `tool`: `ERR <kind>: <reason> -> <remedy>`.
#[must_use]
pub fn error(tool: &str, error: &ToolError) -> String {
    let one = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    match error {
        ToolError::Off => "ERR off: agent connection off for this project -> ask person to turn it on in PerMark Settings > Agent".to_owned(),
        ToolError::Denied { group, .. } => format!(
            "ERR denied: group {} off in this project -> ask person; NEVER retry around it",
            group.id()
        ),
        ToolError::DeniedBelow { group, member } => format!(
            "ERR denied: group {} off below {} -> ask person",
            group.id(),
            format::value(&one(member))
        ),
        ToolError::NotFound(what) => format!(
            "ERR notFound: {} -> {}",
            format::value(&one(what)),
            match tool {
                "resolve_comment" => "check ids with list_comments",
                "read_note" => "check names with list_notes",
                "read" => "check parts with outline, paths with map",
                _ => "check with map | outline | search",
            }
        ),
        ToolError::Invalid(why) => format!("ERR invalid: {} -> fix the arguments", one(why)),
        ToolError::NotOpen => {
            "ERR notOpen: PerMark not open on this project -> ask person to open it".to_owned()
        }
        ToolError::Failed(why) => format!("ERR error: {} -> retry later", one(why)),
    }
}

fn get_context(data: &Value) -> String {
    let doc = s(data, "doc");
    let section = data.get("section");
    let heading = section.map(|x| strings(x, "headingPath").join(" > "));
    let selection = data.get("selection").filter(|v| v.is_object());
    let mut out = Header::new("get_context")
        .flag("nodoc", doc.is_empty())
        .opt("doc", (!doc.is_empty()).then_some(doc))
        .opt("mode", (!doc.is_empty()).then(|| s(data, "mode")))
        .opt("section", heading.filter(|h| !h.is_empty()))
        .opt("lines", range(section.and_then(|x| x.get("lines"))))
        .opt("selection", selection.and_then(|x| range(x.get("lines"))))
        .flag("selection_cut", selection.is_some_and(|x| b(x, "cut")))
        .flag("unsaved", b(data, "unsaved"))
        .opt("open_comments", u(data, "openComments").filter(|n| *n > 0))
        .line();
    out.push('\n');
    if let Some(selection) = selection {
        zone(&mut out, s(selection, "text"));
    }
    out
}

fn list_unread(data: &Value, now: u64) -> String {
    let items = list(data, "unread");
    let mut out = Header::new("list_unread")
        .kv("n", n_of(items))
        .opt("more", u(data, "more").filter(|m| *m > 0))
        .flag("partial", !b(data, "ready"))
        .line();
    out.push('\n');
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|i| vec![s(i, "path").to_owned(), age_of(s(i, "modified"), now)])
        .collect();
    if !rows.is_empty() {
        table(&mut out, &["path", "age"], &rows);
    }
    out
}

fn file_rows(items: &[Value]) -> Vec<Vec<String>> {
    items
        .iter()
        .map(|f| {
            vec![
                s(f, "path").to_owned(),
                strings(f, "tags").join(","),
                s(f, "description").to_owned(),
            ]
        })
        .collect()
}

fn search(data: &Value) -> String {
    if let Some(documents) = data.get("documents").and_then(Value::as_array) {
        let mut out = Header::new("search")
            .kv("tags", strings(data, "tags").join(","))
            .kv("n", documents.len())
            .opt("total", u(data, "total"))
            .opt("more", u(data, "more").filter(|m| *m > 0))
            .line();
        out.push('\n');
        if !documents.is_empty() {
            table(
                &mut out,
                &["path", "tags", "description"],
                &file_rows(documents),
            );
        }
        return out;
    }
    let hits = list(data, "hits");
    let tags = strings(data, "tags");
    let mut out = Header::new("search")
        .kv("q", s(data, "query"))
        .opt("tags", (!tags.is_empty()).then(|| tags.join(",")))
        .kv("n", n_of(hits))
        .flag("partial", b(data, "partial"))
        .flag("trimmed", b(data, "trimmed"))
        .opt("dropped", u(data, "dropped").filter(|d| *d > 0))
        .line();
    out.push('\n');
    let rows: Vec<Vec<String>> = hits
        .iter()
        .map(|h| {
            vec![
                s(h, "path").to_owned(),
                u(h, "line").map(|l| l.to_string()).unwrap_or_default(),
                size(h, "size_tok"),
                s(h, "snippet").to_owned(),
                strings(h, "headingPath").join(" > "),
            ]
        })
        .collect();
    if !rows.is_empty() {
        table(
            &mut out,
            &["path", "line", "size_tok", "snippet", "heading"],
            &rows,
        );
    }
    if let Some(budget) = u(data, "budget").filter(|_| b(data, "trimmed")) {
        let _ = writeln!(out, "…trimmed budget={budget}");
    }
    out
}

fn list_tags(data: &Value) -> String {
    if let Some(files) = data.get("files").and_then(Value::as_array) {
        let mut out = Header::new("list_tags")
            .kv("tag", s(data, "tag"))
            .kv("n", files.len())
            .opt("total", u(data, "total"))
            .opt("more", u(data, "more").filter(|m| *m > 0))
            .line();
        out.push('\n');
        if !files.is_empty() {
            table(
                &mut out,
                &["path", "tags", "description"],
                &file_rows(files),
            );
        }
        return out;
    }
    let tags = list(data, "tags");
    let mut out = Header::new("list_tags")
        .kv("n", n_of(tags))
        .opt("documents", u(data, "documents"))
        .opt("untagged", u(data, "untagged"))
        .opt("more", u(data, "more").filter(|m| *m > 0))
        .line();
    out.push('\n');
    let rows: Vec<Vec<String>> = tags
        .iter()
        .map(|t| {
            vec![
                s(t, "tag").to_owned(),
                u(t, "count").map(|c| c.to_string()).unwrap_or_default(),
            ]
        })
        .collect();
    if !rows.is_empty() {
        table(&mut out, &["tag", "count"], &rows);
    }
    out
}

#[allow(clippy::too_many_lines)] // three tables of one answer: parts, links in, links out
fn outline(data: &Value) -> String {
    let kind = s(data, "kind");
    let approx = b(data, "tokens_approx");
    let tokenizer = s(data, "tokenizer");
    let items = list(data, "items");
    let mut out = Header::new("outline")
        .kv("path", s(data, "path"))
        .kv("kind", kind)
        .opt("lines", u(data, "lines"))
        .opt("spec", data.get("spec").and_then(Value::as_str))
        .flag(
            "invalid",
            data.get("valid").and_then(Value::as_bool) == Some(false),
        )
        .opt("tokenizer", (!tokenizer.is_empty()).then_some(tokenizer))
        .line();
    out.push('\n');
    let tokens = |i: &Value| {
        u(i, "tokens")
            .map(|t| {
                if approx {
                    format!("~{t}")
                } else {
                    t.to_string()
                }
            })
            .unwrap_or_default()
    };
    let num = |i: &Value, k: &str| u(i, k).map(|n| n.to_string()).unwrap_or_default();
    // the person's tokenizer column only when one counted
    let counted = !tokenizer.is_empty();
    let cols = |base: &[&'static str]| -> Vec<&'static str> {
        let mut cols = base.to_vec();
        if counted {
            cols.push("tokens");
        }
        cols
    };
    if !items.is_empty() {
        if kind == "json" {
            let rows: Vec<Vec<String>> = items
                .iter()
                .map(|i| {
                    vec![
                        s(i, "at").to_owned(),
                        s(i, "type").to_owned(),
                        num(i, "line"),
                        num(i, "lines"),
                        size(i, "size_tok"),
                        tokens(i),
                    ]
                })
                .collect();
            table(
                &mut out,
                &cols(&["at", "type", "line", "lines", "size_tok"]),
                &rows,
            );
        } else {
            let rows: Vec<Vec<String>> = items
                .iter()
                .map(|i| {
                    vec![
                        s(i, "at").to_owned(),
                        num(i, "line"),
                        num(i, "lines"),
                        size(i, "size_tok"),
                        tokens(i),
                    ]
                })
                .collect();
            table(&mut out, &cols(&["at", "line", "lines", "size_tok"]), &rows);
        }
    }
    if let Some(links) = data.get("links_in") {
        let paths = strings(links, "paths");
        sub(
            &mut out,
            Header::new("links_in")
                .kv("n", u(links, "total").unwrap_or(0))
                .flag("cut", u(links, "total").unwrap_or(0) > paths.len() as u64),
        );
        if !paths.is_empty() {
            let rows: Vec<Vec<String>> = paths.into_iter().map(|p| vec![p]).collect();
            table(&mut out, &["path"], &rows);
        }
    }
    if let Some(links) = data.get("links_out") {
        let targets = list(links, "targets");
        sub(
            &mut out,
            Header::new("links_out")
                .kv("n", u(links, "total").unwrap_or(0))
                .flag("cut", u(links, "total").unwrap_or(0) > targets.len() as u64),
        );
        if !targets.is_empty() {
            let rows: Vec<Vec<String>> = targets
                .iter()
                .map(|t| {
                    vec![
                        s(t, "path").to_owned(),
                        if b(t, "missing") { "missing" } else { "" }.to_owned(),
                    ]
                })
                .collect();
            table(&mut out, &["path", "state"], &rows);
        }
    }
    out
}

fn read(data: &Value) -> String {
    let at = s(data, "at");
    let mut out = Header::new("read")
        .kv("path", s(data, "path"))
        .opt("at", (!at.is_empty()).then_some(at))
        .opt("lines", range(data.get("lines")))
        .line();
    out.push('\n');
    zone(&mut out, s(data, "text"));
    if let Some(next) = data.get("cut").and_then(Value::as_str) {
        let _ = writeln!(out, "…cut next={next}");
    }
    out
}

fn list_comments(data: &Value) -> String {
    let comments = list(data, "comments");
    let which = s(data, "which");
    let mut out = Header::new("list_comments")
        .opt("which", (!which.is_empty()).then_some(which))
        .kv("n", n_of(comments))
        .line();
    out.push('\n');
    let rows: Vec<Vec<String>> = comments
        .iter()
        .map(|c| {
            let state = s(c, "state");
            vec![
                s(c, "id").to_owned(),
                format!(
                    "{}:{}",
                    s(c, "path"),
                    u(c, "line").map(|l| l.to_string()).unwrap_or_default()
                ),
                if b(c, "done") {
                    format!("{state}+done")
                } else {
                    state.to_owned()
                },
                s(c, "quote").to_owned(),
                s(c, "text").to_owned(),
                strings(c, "headingPath").join(" > "),
                s(c, "reply").to_owned(),
            ]
        })
        .collect();
    if !rows.is_empty() {
        table(
            &mut out,
            &["id", "place", "state", "quote", "text", "heading", "reply"],
            &rows,
        );
    }
    out
}

fn list_notes(data: &Value, now: u64) -> String {
    let notes = list(data, "notes");
    let mut out = Header::new("list_notes").kv("n", n_of(notes)).line();
    out.push('\n');
    let rows: Vec<Vec<String>> = notes
        .iter()
        .map(|n| {
            vec![
                s(n, "note").to_owned(),
                age_of(s(n, "updated"), now),
                s(n, "title").to_owned(),
            ]
        })
        .collect();
    if !rows.is_empty() {
        table(&mut out, &["note", "age", "title"], &rows);
    }
    out
}

fn read_note(data: &Value) -> String {
    let mut out = Header::new("read_note")
        .kv("note", s(data, "note"))
        .kv("title", s(data, "title"))
        .line();
    out.push('\n');
    zone(&mut out, s(data, "text"));
    out
}

fn open(data: &Value) -> String {
    let mut out = Header::new("open")
        .kv("path", s(data, "path"))
        .flag(s(data, "status"), !s(data, "status").is_empty())
        .flag("heading_not_found", b(data, "headingNotFound"))
        .flag("started", b(data, "started"))
        .line();
    out.push('\n');
    out
}

fn map(data: &Value) -> String {
    let files = list(data, "files");
    let cut = list(data, "dirs_cut");
    let mut out = Header::new("map")
        .kv("path", s(data, "path"))
        .opt("depth", u(data, "depth"))
        .kv("n", n_of(files))
        .opt("total", u(data, "total"))
        .opt("more", u(data, "more").filter(|m| *m > 0))
        .flag("partial", b(data, "partial"))
        .line();
    out.push('\n');
    let rows: Vec<Vec<String>> = files
        .iter()
        .map(|f| {
            let flags: Vec<&str> = ["unread", "changed", "binary", "large"]
                .into_iter()
                .filter(|k| b(f, k))
                .collect();
            vec![
                s(f, "path").to_owned(),
                size(f, "size_tok"),
                flags.join(","),
                strings(f, "tags").join(","),
                s(f, "description").to_owned(),
            ]
        })
        .collect();
    if !rows.is_empty() {
        table(
            &mut out,
            &["path", "size_tok", "flags", "tags", "description"],
            &rows,
        );
    }
    if !cut.is_empty() {
        sub(&mut out, Header::new("dirs_cut").kv("n", cut.len()));
        let rows: Vec<Vec<String>> = cut
            .iter()
            .map(|d| {
                vec![
                    s(d, "path").to_owned(),
                    u(d, "files").map(|n| n.to_string()).unwrap_or_default(),
                    size(d, "size_tok"),
                ]
            })
            .collect();
        table(&mut out, &["path", "files", "size_tok"], &rows);
    }
    out
}

/// One activity row: `seq time by kind path detail flags`.
fn activity_row(r: &Value) -> Vec<String> {
    vec![
        u(r, "seq").map(|n| n.to_string()).unwrap_or_default(),
        s(r, "time").to_owned(),
        s(r, "by").to_owned(),
        s(r, "kind").to_owned(),
        s(r, "path").to_owned(),
        s(r, "detail").to_owned(),
        strings(r, "flags").join(","),
    ]
}

const ACTIVITY_COLS: [&str; 7] = ["seq", "time", "by", "kind", "path", "detail", "flags"];

fn activity(data: &Value) -> String {
    // a hand-off package is frozen text: exactly what was handed off
    if let Some(text) = data.get("frozen").and_then(Value::as_str) {
        return text.to_owned();
    }
    activity_text(data)
}

/// The text of an activity answer | a hand-off package from its data.
#[must_use]
pub fn activity_text(data: &Value) -> String {
    let sessions = list(data, "sessions");
    let outside = list(data, "outside");
    let folders = list(data, "outside_folders");
    let rows: usize = sessions
        .iter()
        .map(|x| list(x, "rows").len())
        .sum::<usize>()
        + list(data, "rows").len()
        + outside.len();
    let mut out = Header::new("activity")
        .kv("since", s(data, "since"))
        .opt("package", data.get("package").and_then(Value::as_str))
        .kv("tz", s(data, "tz"))
        .kv("n", rows)
        .opt("sessions", (!sessions.is_empty()).then_some(sessions.len()))
        .opt("hidden", u(data, "hidden").filter(|h| *h > 0))
        .flag("no_handoff", b(data, "no_handoff"))
        .flag("older_entries_not_kept", b(data, "trimmed"))
        .flag("format_upgraded", b(data, "upgraded"))
        .flag("older_unchecked", b(data, "older_unchecked"))
        .flag("outside_only", b(data, "outside_only"))
        .line();
    out.push('\n');
    let plain = list(data, "rows");
    if rows == 0 && folders.is_empty() {
        return out;
    }
    let _ = writeln!(out, "#cols {}", ACTIVITY_COLS.join(" "));
    let body = |items: &[Value]| -> Vec<Vec<String>> { items.iter().map(activity_row).collect() };
    if !plain.is_empty() {
        rows_only(&mut out, &body(plain));
    }
    for session in sessions {
        sub(
            &mut out,
            Header::new("session")
                .kv("id", u(session, "id").unwrap_or(0))
                .kv("from", s(session, "from"))
                .kv("to", s(session, "to"))
                .kv("rows", list(session, "rows").len()),
        );
        rows_only(&mut out, &body(list(session, "rows")));
    }
    let git = s(data, "git");
    if !outside.is_empty() {
        sub(
            &mut out,
            Header::new("outside")
                .kv("n", outside.len())
                .opt("git", (!git.is_empty()).then_some(git)),
        );
        rows_only(&mut out, &body(outside));
    }
    if !folders.is_empty() {
        sub(
            &mut out,
            Header::new("outside_folders").kv("n", folders.len()).opt(
                "git",
                (outside.is_empty() && !git.is_empty()).then_some(git),
            ),
        );
        let rows: Vec<Vec<String>> = folders
            .iter()
            .map(|f| {
                ["folder", "created", "modified", "deleted"]
                    .iter()
                    .map(|k| match f.get(*k) {
                        Some(Value::String(t)) => t.clone(),
                        Some(Value::Number(n)) => n.to_string(),
                        _ => String::new(),
                    })
                    .collect()
            })
            .collect();
        table(
            &mut out,
            &["folder", "created", "modified", "deleted"],
            &rows,
        );
    }
    if let Some(next) = u(data, "next") {
        let _ = writeln!(out, "…cut next={next}");
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::perms::Group;

    #[test]
    fn errors_are_one_line_with_a_remedy() {
        let lines = [
            error("read", &ToolError::Off),
            error(
                "resolve_comment",
                &ToolError::Denied {
                    group: Group::CommentsResolve,
                    first_ask: false,
                },
            ),
            error(
                "outline",
                &ToolError::NotFound("docs/my plan.md".to_owned()),
            ),
            error("search", &ToolError::Invalid("limit\nis bad".to_owned())),
            error("get_context", &ToolError::NotOpen),
            error("map", &ToolError::Failed("x".to_owned())),
            error("resolve_comment", &ToolError::NotFound("c9".to_owned())),
        ];
        for line in &lines {
            assert!(line.starts_with("ERR "), "{line}");
            assert!(line.contains(" -> "), "{line}");
            assert!(!line.contains('\n'), "{line}");
        }
        assert_eq!(
            lines[1],
            "ERR denied: group comments.resolve off in this project -> ask person; NEVER retry around it"
        );
        assert!(lines[2].contains("\"docs/my plan.md\""));
        assert_eq!(lines[6], "ERR notFound: c9 -> check ids with list_comments");
    }

    #[test]
    fn the_context_puts_the_selection_in_a_zone() {
        let text = render_at(
            "get_context",
            &json!({
                "doc": "docs/plan.md", "mode": "view",
                "section": { "headingPath": ["Plan", "Rollout"], "lines": [15, 25] },
                "selection": { "text": ">>> run()\nnext", "lines": [17, 18], "cut": false },
                "unsaved": false, "openComments": 2
            }),
            0,
        );
        assert_eq!(
            text,
            "get_context doc=docs/plan.md mode=view section=\"Plan > Rollout\" lines=15-25 selection=17-18 open_comments=2\n<<<<\n>>> run()\nnext\n>>>>\n"
        );
        assert_eq!(
            render_at("get_context", &json!({ "openComments": 0 }), 0),
            "get_context nodoc\n"
        );
    }

    #[test]
    fn read_cuts_say_where_to_go_on() {
        let text = render_at(
            "read",
            &json!({ "path": "a.md", "at": "A", "lines": [1, 3], "text": "# A\nx\n", "cut": "L4" }),
            0,
        );
        assert_eq!(
            text,
            "read path=a.md at=A lines=1-3\n<<<\n# A\nx\n>>>\n…cut next=L4\n"
        );
    }
}
