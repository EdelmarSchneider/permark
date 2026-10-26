use permark_core::format::config::fidelity::check_corpus;
use permark_core::format::config::ScalarKind;
use permark_core::format::{ConfigEdit, EditRefusal, Editability, Format, NewValue};

use super::*;

const SPRING: &str = "# Server\nserver.port=8080\nserver.servlet.context-path = /api\n\n# Database\nspring.datasource.url=jdbc:postgresql://localhost:5432/app\nspring.datasource.username: app\nspring.datasource.password=s3cr3t\nspring.jpa.show-sql true\nspring.jpa.open-in-view=false\n! legacy comment\napp.name=Demo App\napp.greeting=Hello, \\u00e9\\u00e8!\nmessage.long=first part \\\n    second part\nempty.value=\ntop-level=1\n";

fn format() -> ConfigFormat<Properties> {
    ConfigFormat(Properties)
}

#[test]
fn corpus_keeps_every_guarantee() {
    check_corpus(&format(), &[("application.properties", SPRING)]);
}

#[test]
fn dotted_keys_group_by_their_first_part() {
    let sheet = format().sheet(SPRING).expect("parses");
    let titles: Vec<&str> = sheet.groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(titles, ["server", "spring", "app", "message", "empty", ""]);
    let server = &sheet.groups[0];
    assert_eq!(server.comment, None);
    assert_eq!(server.rows[0].key, "port");
    assert_eq!(server.rows[0].comment.as_deref(), Some("Server"));
    assert_eq!(server.rows[1].key, "servlet.context-path");
    assert_eq!(
        sheet
            .scalar("server.servlet.context-path")
            .map(|s| s.text.as_str()),
        Some("/api")
    );
    assert_eq!(
        sheet.scalar("server.port").map(|s| s.kind),
        Some(ScalarKind::Number)
    );
    // separators: `=`, `:` and plain whitespace
    assert_eq!(
        sheet
            .scalar("spring.datasource.username")
            .map(|s| s.text.as_str()),
        Some("app")
    );
    assert_eq!(
        sheet.scalar("spring.jpa.show-sql").map(|s| s.text.as_str()),
        Some("true")
    );
    assert_eq!(
        sheet.scalar("spring.jpa.show-sql").map(|s| s.kind),
        Some(ScalarKind::Bool)
    );
    assert_eq!(
        sheet.scalar("top-level").map(|s| s.kind),
        Some(ScalarKind::Number)
    );
    // the escapes of the file are shown as written
    assert_eq!(
        sheet.scalar("app.greeting").map(|s| s.text.as_str()),
        Some("Hello, \\u00e9\\u00e8!")
    );
    // `!` comments count as captions
    assert_eq!(
        sheet
            .row("app.name")
            .and_then(|r| r.comment.clone())
            .as_deref(),
        Some("legacy comment")
    );
    let outline: Vec<String> = sheet.outline().into_iter().map(|p| p.path).collect();
    assert_eq!(
        outline,
        ["server", "spring", "app", "message", "empty", "top-level"]
    );
}

#[test]
fn continuation_lines_are_one_read_only_value() {
    let sheet = format().sheet(SPRING).expect("parses");
    let long = sheet.scalar("message.long").expect("row");
    assert_eq!(
        long.edit,
        Editability::ReadOnly {
            reason: "continuation".to_owned()
        }
    );
    assert!(long.text.contains("second part"));
    assert_eq!(
        sheet.row("message.long").map(|r| (r.line, r.end)),
        Some((14, 15))
    );
    assert_eq!(sheet.row("empty.value").map(|r| r.line), Some(16));
    assert_eq!(
        format().edit(
            SPRING,
            &ConfigEdit::Set {
                path: "message.long".to_owned(),
                value: NewValue::Text {
                    text: "x".to_owned()
                }
            }
        ),
        Err(EditRefusal::ReadOnly("continuation".to_owned()))
    );
}

#[test]
fn edits_replace_only_the_value_text() {
    let edit = |path: &str, new: &str| {
        format()
            .edit(
                SPRING,
                &ConfigEdit::Set {
                    path: path.to_owned(),
                    value: NewValue::Text {
                        text: new.to_owned(),
                    },
                },
            )
            .map(|s| s.apply(SPRING))
    };
    assert!(edit("app.name", "Other # not a comment")
        .expect("ok")
        .contains("app.name=Other # not a comment\n"));
    assert!(edit("empty.value", "now")
        .expect("ok")
        .contains("empty.value=now\n"));
    assert_eq!(
        edit("app.name", "a\nb"),
        Err(EditRefusal::Invalid("newline"))
    );
    assert_eq!(
        edit("app.name", "ends with \\"),
        Err(EditRefusal::Invalid("continuation"))
    );
    // leading spaces would be lost when the file is read again
    assert_eq!(
        edit("app.name", " lead"),
        Err(EditRefusal::Invalid("roundTrip"))
    );
}

#[test]
fn keys_with_escapes_and_repeats() {
    let sheet = format()
        .sheet("a\\ b=1\na\\=b: 2\nk=1\nk=2\n")
        .expect("parses");
    assert_eq!(sheet.scalar("\"a b\"").map(|s| s.text.as_str()), Some("1"));
    assert_eq!(sheet.scalar("k[1]").map(|s| s.text.as_str()), Some("2"));
    assert_eq!(sheet.row_count(), 4);
}
