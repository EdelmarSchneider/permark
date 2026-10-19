use permark_core::format::config::fidelity::check_fuzz;
use permark_core::format::config::RowValue;
use permark_core::format::{ConfigEdit, EditRefusal, Format, NewValue};

use super::*;

const GITIGNORE: &str = "# Dependencies\nnode_modules/\n.pnp.js\n\n# Build output\n/dist\n/build/\n*.log\n!important.log\n\n# Env files\n.env\n.env.*\n!.env.example\n\ndebug.txt\n";

fn format() -> ConfigFormat<Ignore> {
    ConfigFormat(Ignore)
}

fn set(path: &str, text: &str) -> ConfigEdit {
    ConfigEdit::Set {
        path: path.to_owned(),
        value: NewValue::Text {
            text: text.to_owned(),
        },
    }
}

#[test]
fn comment_blocks_are_cards_patterns_are_rows() {
    let sheet = format().sheet(GITIGNORE).expect("parses");
    assert_eq!(sheet.kind, SheetKind::Ignore);
    let titles: Vec<&str> = sheet.groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(titles, ["Dependencies", "Build output", "Env files"]);
    let build = &sheet.groups[1];
    assert_eq!(build.line, 5);
    let rows: Vec<(&str, u32, bool)> = build
        .rows
        .iter()
        .map(|r| match &r.value {
            RowValue::Pattern { text, negated, .. } => (text.as_str(), r.line, *negated),
            _ => panic!("pattern"),
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("/dist", 6, false),
            ("/build/", 7, false),
            ("*.log", 8, false),
            ("!important.log", 9, true)
        ]
    );
    assert_eq!(sheet.groups[2].rows.len(), 4);
    let outline: Vec<String> = sheet.outline().into_iter().map(|p| p.label).collect();
    assert_eq!(outline, ["Dependencies", "Build output", "Env files"]);
}

#[test]
fn a_pattern_edit_replaces_the_line_text_only() {
    let crlf = GITIGNORE.replace('\n', "\r\n");
    for text in [GITIGNORE.to_owned(), crlf] {
        let out = format()
            .edit(&text, &set("L8", "*.tmp"))
            .expect("edit")
            .apply(&text);
        assert_eq!(out.replace("*.tmp", "*.log"), text);
    }
    assert_eq!(
        format().edit(GITIGNORE, &set("L8", "a\nb")),
        Err(EditRefusal::Invalid("newline"))
    );
    assert_eq!(
        format().edit(GITIGNORE, &set("L8", "  ")),
        Err(EditRefusal::Invalid("empty"))
    );
    assert_eq!(
        format().edit(GITIGNORE, &set("L99", "x")),
        Err(EditRefusal::NotFound)
    );
    // a comment line is not a pattern
    assert_eq!(
        format().edit(GITIGNORE, &set("L1", "x")),
        Err(EditRefusal::NotFound)
    );
}

#[test]
fn deleting_a_line_takes_its_break_and_nothing_else() {
    let remove = |text: &str, path: &str| {
        format()
            .edit(
                text,
                &ConfigEdit::Remove {
                    path: path.to_owned(),
                },
            )
            .expect("edit")
            .apply(text)
    };
    assert_eq!(remove("a\nb\nc\n", "L2"), "a\nc\n");
    assert_eq!(remove("a\r\nb\r\nc\r\n", "L2"), "a\r\nc\r\n");
    // the last line with no break: the break before it goes, so the file still ends without one
    assert_eq!(remove("a\nb\nc", "L3"), "a\nb");
    assert_eq!(remove("a\r\nb", "L2"), "a");
    assert_eq!(remove("only", "L1"), "");
    let out = remove(GITIGNORE, "L9");
    assert_eq!(out, GITIGNORE.replace("!important.log\n", ""));
}

#[test]
fn adding_a_line_uses_the_files_break_and_keeps_the_end() {
    let insert = |text: &str, after: Option<&str>, new: &str| {
        format()
            .edit(
                text,
                &ConfigEdit::Insert {
                    after: after.map(str::to_owned),
                    text: new.to_owned(),
                },
            )
            .expect("edit")
            .apply(text)
    };
    assert_eq!(insert("a\nb\n", Some("L1"), "x"), "a\nx\nb\n");
    assert_eq!(insert("a\r\nb\r\n", Some("L1"), "x"), "a\r\nx\r\nb\r\n");
    assert_eq!(insert("a\nb\n", None, "x"), "a\nb\nx\n");
    assert_eq!(insert("a\nb", None, "x"), "a\nb\nx");
    assert_eq!(insert("a\nb", Some("L2"), "x"), "a\nb\nx");
    assert_eq!(insert("", None, "x"), "x\n");
    assert_eq!(
        format().edit(
            "a\n",
            &ConfigEdit::Insert {
                after: None,
                text: "x\ny".to_owned()
            }
        ),
        Err(EditRefusal::Invalid("newline"))
    );
    // the new line reads back as a pattern
    let out = insert(GITIGNORE, Some("L7"), "*.tmp");
    let sheet = format().sheet(&out).expect("parses");
    assert_eq!(
        sheet.row_count(),
        format().sheet(GITIGNORE).expect("parses").row_count() + 1
    );
    assert!(matches!(
        sheet.row("L8").map(|r| &r.value),
        Some(RowValue::Pattern { text, .. }) if text == "*.tmp"
    ));
}

#[test]
fn search_reads_patterns() {
    let text = format().searchable(GITIGNORE).expect("patterns");
    assert!(text.contains("node_modules/") && text.contains("!important.log"));
    assert!(!text.contains("Dependencies"));
}

#[test]
fn claims_the_ignore_files() {
    let f = format();
    for name in [
        ".gitignore",
        ".aiignore",
        ".cursorignore",
        ".dockerignore",
        ".prettierignore",
        ".npmignore",
        ".eslintignore",
        ".GITIGNORE",
    ] {
        assert_eq!(f.claims(name, None, b""), Claim::Strong, "{name}");
    }
    for name in ["ignore", "notes.txt", "gitignore", ".ignore.md"] {
        assert_eq!(f.claims(name, None, b""), Claim::No, "{name}");
    }
}

#[test]
fn parse_never_panics() {
    check_fuzz(&format(), &[GITIGNORE, "!\n#\n\\#x\n  \n*\n"]);
}
