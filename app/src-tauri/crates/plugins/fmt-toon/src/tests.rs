// Test code: fixture numbers are small; edits read best by value.
#![allow(clippy::cast_possible_truncation, clippy::needless_pass_by_value)]

use std::collections::HashSet;
use std::path::Path;

use permark_core::format::config::fidelity;
use permark_core::format::config::{ConfigEdit, NewValue, Scalar};
use serde_json::Value;
use toon_format::types::{DecodeOptions, Delimiter, EncodeOptions, Indent};

use super::*;

fn spec_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("spec")
}

/// JSON-model equality (§2): numbers by value, objects by key order.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((k1, v1), (k2, v2))| k1 == k2 && same(v1, v2))
        }
        _ => a == b,
    }
}

fn fixtures(kind: &str) -> Vec<(String, Value)> {
    let mut files: Vec<_> = std::fs::read_dir(spec_dir().join(kind))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        for (i, case) in doc["tests"].as_array().unwrap().iter().enumerate() {
            out.push((format!("{name} #{i}"), case.clone()));
        }
    }
    out
}

/// Leaves of a decoded value (primitives), the way the sheet shows them (one scalar each).
fn leaves(value: &Value) -> usize {
    match value {
        Value::Array(items) => items.iter().map(leaves).sum(),
        Value::Object(map) => map.values().map(leaves).sum(),
        _ => 1,
    }
}

fn scalars(sheet: &Sheet) -> Vec<&Scalar> {
    sheet
        .rows()
        .flat_map(|(_, row)| match &row.value {
            RowValue::Scalar(s) => vec![s],
            RowValue::List { items } => items.iter().collect(),
            RowValue::Table(t) => t.rows.iter().flat_map(|r| r.cells.iter()).collect(),
            RowValue::Raw { .. } | RowValue::Pattern { .. } => Vec::new(),
        })
        .collect()
}

#[test]
fn the_pinned_conformance_suite_passes() {
    let mut run = 0;
    for (name, case) in fixtures("decode") {
        let options = &case["options"];
        let indent = options["indentSize"].as_u64().unwrap_or(2) as usize;
        let opts = DecodeOptions::new()
            .with_strict(options["strict"].as_bool().unwrap_or(true))
            .with_indent(Indent::Spaces(indent));
        let got: Result<Value, _> = toon_format::decode(case["input"].as_str().unwrap(), &opts);
        if case["shouldError"].as_bool().unwrap_or(false) {
            assert!(got.is_err(), "{name}: must fail");
        } else {
            let got = got.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                same(&got, &case["expected"]),
                "{name}: {got} != {}",
                case["expected"]
            );
        }
        run += 1;
    }
    for (name, case) in fixtures("encode") {
        let options = &case["options"];
        let mut opts =
            EncodeOptions::new().with_spaces(options["indentSize"].as_u64().unwrap_or(2) as usize);
        if let Some(d) = options["delimiter"].as_str() {
            opts = opts.with_delimiter(match d {
                "\t" => Delimiter::Tab,
                "|" => Delimiter::Pipe,
                _ => Delimiter::Comma,
            });
        }
        let got = toon_format::encode(&case["input"], &opts);
        if case["shouldError"].as_bool().unwrap_or(false) {
            assert!(got.is_err(), "{name}: must fail");
        } else {
            assert_eq!(got.ok().as_deref(), case["expected"].as_str(), "{name}");
        }
        run += 1;
    }
    assert!(run > 500, "the suite is there ({run} cases)");
}

#[test]
fn every_valid_fixture_scans_to_the_values_it_decodes_to() {
    let mut scanned = 0;
    for (name, case) in fixtures("decode") {
        let text = case["input"].as_str().unwrap();
        let Ok(value) = decode(text) else { continue };
        let sheet = ToonFormat.sheet(text).unwrap();
        let raw = sheet
            .rows()
            .filter(|(_, r)| matches!(r.value, RowValue::Raw { .. }))
            .count();
        assert_eq!(raw, 0, "{name}: every line placed\n{text}\n{sheet:#?}");
        let found = scalars(&sheet);
        assert_eq!(
            found.len(),
            leaves(&value),
            "{name}: one scalar per value\n{text}"
        );
        for s in found {
            let span = s.span.clone().unwrap();
            assert!(text.get(span).is_some(), "{name}: {} on boundaries", s.path);
        }
        scanned += 1;
    }
    assert!(scanned > 150, "{scanned}");
}

const USERS: &str = "# people of the project\nusers[3]{id,name,role,active}:\n  1,Ada,admin,true\n  2,Bob,\"user, guest\",false\n  3,Cy,user,true\nproject:\n  name: Demo\n  # where it runs\n  url: \"https://example.com\"\n  tags[3]: a,b,c\naccess[2:]{token,owner}:\n  prod: sk-live-1234,ops\n  test: sk-test-5678,dev\n";

#[test]
fn a_file_reads_as_cards_tables_and_lists() {
    let sheet = ToonFormat.sheet(USERS).unwrap();
    assert_eq!(sheet.kind, permark_core::format::config::SheetKind::Data);
    assert_eq!(sheet.spec.as_deref(), Some("TOON 4.4"));
    let users = sheet.table("users").unwrap();
    assert_eq!(users.fields, ["id", "name", "role", "active"]);
    assert_eq!(users.count, 3);
    assert_eq!(users.rows[1].path, "users[1]");
    assert_eq!(users.rows[1].cells[2].text, "user, guest");
    assert_eq!(users.rows[1].cells[2].path, "users[1].role");
    assert_eq!(users.rows[0].line, 3);
    let keys = sheet.table("access").unwrap();
    assert!(keys.keyed);
    assert_eq!(keys.rows[0].key.as_deref(), Some("prod"));
    assert_eq!(keys.rows[0].cells[0].path, "access.prod.token");
    assert_eq!(
        sheet.scalar("project.url").unwrap().text,
        "https://example.com"
    );
    assert_eq!(
        sheet
            .groups
            .iter()
            .map(|g| g.path.as_str())
            .collect::<Vec<_>>(),
        ["users", "project", "access"]
    );
    let project = sheet.groups.iter().find(|g| g.path == "project").unwrap();
    assert_eq!(project.rows[1].comment.as_deref(), Some("where it runs"));
    assert_eq!(
        sheet.groups[0].comment.as_deref(),
        Some("people of the project")
    );
    let outline: Vec<String> = sheet.outline().into_iter().map(|p| p.label).collect();
    assert_eq!(
        outline,
        [
            "users[3]{id,name,role,active}",
            "project",
            "access[2]{token,owner}"
        ]
    );
    assert_eq!(sheet.range_of("users[1]"), Some((4, 4)));
    assert_eq!(sheet.range_of("project"), Some((6, 10)));
}

#[test]
fn every_value_is_searched_as_data() {
    let search = ToonFormat.searchable(USERS).unwrap();
    assert!(search.contains("Ada") && search.contains("users") && search.contains("token"));
    assert!(search.contains("sk-live-1234"));
}

#[test]
fn a_file_that_is_not_valid_shows_as_text_with_the_line() {
    let cx_known = HashSet::new();
    let env = NoFiles;
    let features = permark_core::format::MarkdownFeatures::default();
    let cx = RenderCx {
        path: "a.toon",
        env: &env,
        features: &features,
        known: &cx_known,
    };
    let rendered = ToonFormat.render("a: 1\nitems[3]: x,y\n", &cx);
    match rendered.body {
        Body::Text {
            problem: Some(p),
            language,
            ..
        } => {
            assert_eq!(p.line, 2);
            assert_eq!(language.as_deref(), Some("toon"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(rendered.notes, ["parseError"]);
    // a big file is text at once
    let big = "k: v\n".repeat(SHEET_MAX_BYTES / 5 + 10);
    let start = std::time::Instant::now();
    let rendered = ToonFormat.render(&big, &cx);
    assert!(matches!(rendered.body, Body::Text { problem: None, .. }));
    assert!(start.elapsed().as_millis() < 500);
}

struct NoFiles;
impl permark_core::format::DocEnv for NoFiles {
    fn file_exists(&self, _: &str) -> bool {
        false
    }
}

#[test]
fn nesting_bombs_are_refused_not_followed() {
    let mut text = String::new();
    for depth in 0..400 {
        text.push_str(&"  ".repeat(depth));
        text.push_str("a:\n");
    }
    assert!(decode(&text).is_err_and(|p| p.detail.contains("nesting")));
    assert!(ToonFormat.sheet(&text).is_none());
    let braces = format!("t[1]{{{}x{}}}:\n  1", "a{".repeat(200), "}".repeat(200));
    assert!(decode(&braces).is_err());
}

// ---- edits ------------------------------------------------------------------------------------------------------

fn apply(text: &str, edit: ConfigEdit) -> String {
    ToonFormat
        .edit(text, &edit)
        .unwrap_or_else(|e| panic!("{edit:?}: {e}"))
        .apply(text)
}

fn text(t: &str) -> NewValue {
    NewValue::Text { text: t.to_owned() }
}

fn literal(t: &str) -> NewValue {
    NewValue::Literal { text: t.to_owned() }
}

/// The lines that differ (old, new), in order; equal lengths only.
fn changed(old: &str, new: &str) -> Vec<(String, String)> {
    old.split('\n')
        .zip(new.split('\n'))
        .filter(|(a, b)| a != b)
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect()
}

#[test]
fn a_cell_edit_changes_only_its_token() {
    let new = apply(
        USERS,
        ConfigEdit::Set {
            path: "users[0].name".into(),
            value: text("Ada Lovelace"),
        },
    );
    assert_eq!(
        changed(USERS, &new),
        [(
            "  1,Ada,admin,true".into(),
            "  1,Ada Lovelace,admin,true".into()
        )]
    );
    // the delimiter of the table is quoted; a number stays a number
    let new = apply(
        USERS,
        ConfigEdit::Set {
            path: "users[2].role".into(),
            value: text("a,b"),
        },
    );
    assert!(new.contains("  3,Cy,\"a,b\",true\n"));
    let new = apply(
        USERS,
        ConfigEdit::Set {
            path: "users[2].id".into(),
            value: literal("30"),
        },
    );
    assert!(new.contains("  30,Cy,user,true\n"));
    let refused = ToonFormat.edit(
        USERS,
        &ConfigEdit::Set {
            path: "users[2].id".into(),
            value: literal("thirty"),
        },
    );
    assert_eq!(refused, Err(EditRefusal::Invalid("literal")));
    // a quoted string stays quoted
    let new = apply(
        USERS,
        ConfigEdit::Set {
            path: "project.url".into(),
            value: text("https://permark.app"),
        },
    );
    assert!(new.contains("  url: \"https://permark.app\"\n"));
    // a pipe table quotes pipes, not commas
    let pipes = "t[2|]{a|b}:\n  x|y\n  z|w";
    let new = apply(
        pipes,
        ConfigEdit::Set {
            path: "t[0].b".into(),
            value: text("1,2|3"),
        },
    );
    assert_eq!(new, "t[2|]{a|b}:\n  x|\"1,2|3\"\n  z|w");
}

#[test]
fn keys_are_renamed_in_place() {
    let new = apply(
        USERS,
        ConfigEdit::Rename {
            path: "project.name".into(),
            key: "title".into(),
        },
    );
    assert_eq!(
        changed(USERS, &new),
        [("  name: Demo".into(), "  title: Demo".into())]
    );
    let new = apply(
        USERS,
        ConfigEdit::Rename {
            path: "users".into(),
            key: "team members".into(),
        },
    );
    assert!(new.contains("\"team members\"[3]{id,name,role,active}:\n"));
    let new = apply(
        USERS,
        ConfigEdit::Rename {
            path: "access.prod".into(),
            key: "live".into(),
        },
    );
    assert!(new.contains("  live: sk-live-1234,ops\n"));
    let new = apply(
        USERS,
        ConfigEdit::RenameField {
            table: "users".into(),
            index: 1,
            name: "full_name".into(),
        },
    );
    assert!(new.contains("users[3]{id,full_name,role,active}:\n"));
    for refused in [
        ConfigEdit::Rename {
            path: "project.name".into(),
            key: "url".into(),
        },
        ConfigEdit::Rename {
            path: "access.prod".into(),
            key: "test".into(),
        },
        ConfigEdit::RenameField {
            table: "users".into(),
            index: 1,
            name: "role".into(),
        },
    ] {
        assert_eq!(
            ToonFormat.edit(USERS, &refused),
            Err(EditRefusal::Invalid("duplicate")),
            "{refused:?}"
        );
    }
}

#[test]
fn keys_are_added_and_removed_as_one_line() {
    let new = apply(
        USERS,
        ConfigEdit::AddKey {
            parent: "project".into(),
            key: "year".into(),
            value: literal("2026"),
        },
    );
    assert!(new.contains("  tags[3]: a,b,c\n  year: 2026\naccess[2:]"));
    let new = apply(
        USERS,
        ConfigEdit::AddKey {
            parent: String::new(),
            key: "owner".into(),
            value: text("Ana: lead"),
        },
    );
    assert!(new.ends_with("  test: sk-test-5678,dev\nowner: \"Ana: lead\"\n"));
    let new = apply(
        USERS,
        ConfigEdit::Remove {
            path: "project.tags".into(),
        },
    );
    assert_eq!(new, USERS.replace("  tags[3]: a,b,c\n", ""));
    assert_eq!(
        ToonFormat.edit(
            USERS,
            &ConfigEdit::Remove {
                path: "project".into()
            }
        ),
        Err(EditRefusal::ReadOnly("multiline".into()))
    );
    let items = "items[1]:\n  - id: 1\n    name: x";
    assert_eq!(
        ToonFormat.edit(
            items,
            &ConfigEdit::Remove {
                path: "items[0].id".into()
            }
        ),
        Err(EditRefusal::ReadOnly("firstField".into()))
    );
    let new = apply(
        items,
        ConfigEdit::Remove {
            path: "items[0].name".into(),
        },
    );
    assert_eq!(new, "items[1]:\n  - id: 1");
    let new = apply(
        items,
        ConfigEdit::AddKey {
            parent: "items[0]".into(),
            key: "ok".into(),
            value: literal("true"),
        },
    );
    assert_eq!(new, "items[1]:\n  - id: 1\n    name: x\n    ok: true");
    // an empty document takes its first key
    assert_eq!(
        apply(
            "",
            ConfigEdit::AddKey {
                parent: String::new(),
                key: "a".into(),
                value: literal("1"),
            }
        ),
        "a: 1"
    );
}

#[test]
fn rows_are_added_removed_duplicated_and_moved_with_the_count() {
    let new = apply(
        USERS,
        ConfigEdit::AddRow {
            table: "users".into(),
            at: 3,
        },
    );
    assert!(new.contains("users[4]{id,name,role,active}:\n"));
    assert!(new.contains("  3,Cy,user,true\n  0,\"\",\"\",false\nproject:"));
    let new = apply(
        USERS,
        ConfigEdit::AddRow {
            table: "users".into(),
            at: 0,
        },
    );
    assert!(new.contains("active}:\n  0,\"\",\"\",false\n  1,Ada"));
    let new = apply(
        USERS,
        ConfigEdit::AddRow {
            table: "access".into(),
            at: 2,
        },
    );
    assert!(new.contains("access[3:]{token,owner}:\n"));
    assert!(new.contains("  test: sk-test-5678,dev\n  new: \"\",\"\"\n"));
    let new = apply(
        USERS,
        ConfigEdit::RemoveRow {
            path: "users[1]".into(),
        },
    );
    assert_eq!(
        new,
        USERS
            .replace("users[3]", "users[2]")
            .replace("  2,Bob,\"user, guest\",false\n", "")
    );
    let new = apply(
        USERS,
        ConfigEdit::DuplicateRow {
            path: "access.test".into(),
        },
    );
    assert!(new.contains("  test: sk-test-5678,dev\n  test2: sk-test-5678,dev\n"));
    let new = apply(
        USERS,
        ConfigEdit::MoveRow {
            path: "users[2]".into(),
            to: 0,
        },
    );
    assert!(new.contains("active}:\n  3,Cy,user,true\n  1,Ada,admin,true\n  2,Bob"));
    assert_eq!(new.len(), USERS.len());
}

#[test]
fn row_edits_keep_every_other_byte_on_crlf() {
    let crlf = USERS.replace('\n', "\r\n");
    let new = apply(
        &crlf,
        ConfigEdit::AddRow {
            table: "users".into(),
            at: 1,
        },
    );
    assert!(new.contains("  1,Ada,admin,true\r\n  0,\"\",\"\",false\r\n  2,Bob"));
    assert_eq!(new.matches('\n').count(), new.matches("\r\n").count());
    let new = apply(
        &crlf,
        ConfigEdit::MoveRow {
            path: "users[0]".into(),
            to: 2,
        },
    );
    assert!(new.contains(
        "  2,Bob,\"user, guest\",false\r\n  3,Cy,user,true\r\n  1,Ada,admin,true\r\nproject:"
    ));
    // a comment between rows keeps its place
    let commented = "t[3]{a}:\n  1\n  # two\n  2\n  3";
    assert_eq!(
        apply(
            commented,
            ConfigEdit::MoveRow {
                path: "t[0]".into(),
                to: 2
            }
        ),
        "t[3]{a}:\n  2\n  # two\n  3\n  1"
    );
    // the last row without a final break
    assert_eq!(
        apply(
            commented,
            ConfigEdit::RemoveRow {
                path: "t[2]".into()
            }
        ),
        "t[2]{a}:\n  1\n  # two\n  2"
    );
}

#[test]
fn edits_on_text_that_is_not_valid_are_refused() {
    assert_eq!(
        ToonFormat.edit(
            "a[3]: x,y",
            &ConfigEdit::Set {
                path: "a[0]".into(),
                value: text("z")
            }
        ),
        Err(EditRefusal::Invalid("syntax"))
    );
    assert_eq!(
        ToonFormat.edit(
            "a: 1",
            &ConfigEdit::Insert {
                after: None,
                text: "b: 2".into()
            }
        ),
        Err(EditRefusal::Unsupported)
    );
}

const CORPUS: &[(&str, &str)] = &[
    ("users", USERS),
    (
        "nested",
        "server:\n  host: localhost\n  port: 8080\n  tls:\n    enabled: true\n    cert: \"/etc/ssl/cert.pem\"\nlimits[3]: 10,20,30\nempty: []\n",
    ),
    (
        "list items",
        "items[3]:\n  - id: 1\n    name: First\n  - 42\n  - [2]: x,y\nmixed[2]:\n  - title: A\n    rows[2]{a,b}:\n      1,2\n      3,4\n  - plain text\n",
    ),
    (
        "nested fields",
        "orders[2]{id,customer{name,country},total}:\n  1,Ada,UK,9.5\n  2,\"Bob, Jr\",US,12\n",
    ),
    ("tabs", "t[2\t]{a\tb}:\n  x\ty\n  \"p q\"\tz\n"),
    ("root table", "[2]{a,b}:\n  1,x\n  2,y\n"),
    ("root keyed", "[2:]{a,b}:\n  one: 1,x\n  two: 2,y\n"),
    ("unicode", "名前: 太郎\n\"key with space\": \"value: quoted\"\nemoji: 😀 ok\n"),
    ("four spaces", "a:\n    b: 1\n    c:\n        d: text\n"),
];

#[test]
fn the_corpus_keeps_every_untouched_byte() {
    for (name, text) in CORPUS {
        assert!(decode(text).is_ok(), "{name} decodes");
    }
    fidelity::check_corpus(&ToonFormat, CORPUS);
}

#[test]
fn bom_and_root_forms_read() {
    let sheet = ToonFormat.sheet("\u{feff}a: 1\nb: x").unwrap();
    assert_eq!(sheet.scalar("a").unwrap().text, "1");
    let new = apply(
        "\u{feff}a: 1\nb: x",
        ConfigEdit::Set {
            path: "a".into(),
            value: literal("2"),
        },
    );
    assert_eq!(new, "\u{feff}a: 2\nb: x");
    let one = ToonFormat.sheet("hello").unwrap();
    assert_eq!(one.scalar("").unwrap().text, "hello");
    assert_eq!(
        apply(
            "hello",
            ConfigEdit::Set {
                path: String::new(),
                value: text("true")
            }
        ),
        "\"true\""
    );
    let list = ToonFormat.sheet("[3]: a,b,c").unwrap();
    assert_eq!(list.scalar("[2]").unwrap().text, "c");
}

#[test]
fn the_plugin_serves_json_and_claims_toon_files() {
    assert_eq!(
        ToonFormat.claims("data.toon", Some("toon"), b""),
        Claim::Strong
    );
    assert_eq!(ToonFormat.claims("data.json", Some("json"), b""), Claim::No);
    let p = plugin();
    assert_eq!(p.manifest().id.as_str(), "fmt-toon");
    assert!(p.manifest().default_enabled);
    assert_eq!(p.manifest().kind, PluginKind::Format);
}

#[test]
fn a_new_row_takes_plain_values_where_they_need_no_quotes() {
    let added = apply(
        USERS,
        ConfigEdit::AddRow {
            table: "users".into(),
            at: 3,
        },
    );
    let filled = apply(
        &added,
        ConfigEdit::Set {
            path: "users[3].role".into(),
            value: text("math"),
        },
    );
    assert!(filled.contains("  0,\"\",math,false\n"), "{filled}");
    let quoted = apply(
        &filled,
        ConfigEdit::Set {
            path: "users[3].name".into(),
            value: text("Johnson, K"),
        },
    );
    assert!(
        quoted.contains("  0,\"Johnson, K\",math,false\n"),
        "{quoted}"
    );
}
