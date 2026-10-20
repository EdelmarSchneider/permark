use permark_core::format::config::fidelity::{check_corpus, check_masking};
use permark_core::format::config::{RowValue, ScalarKind};
use permark_core::format::Editability;
use permark_core::format::{ConfigEdit, EditRefusal, Format, NewValue};

use super::*;

const CARGO: &str = r#"# The app crate.
[package]
name = "permark" # the crate
version = "0.5.8"
edition = "2021"
authors = ["Edelmar Schneider"]
description = 'A local-first app'
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
tokio = "1.40"
toml_edit = "0.25.17"

# Build profiles
[profile.release]
opt-level = 3
lto = true
codegen-units = 1

[[bin]]
name = "permark"
path = "src/main.rs"

[[bin]]
name = "tool"
path = "src/tool.rs"

[workspace.dependencies]
tempfile = "3.27.0"
"#;

const PYPROJECT: &str = r#"[build-system]
requires = ["setuptools>=61", "wheel"]
build-backend = "setuptools.build_meta"

[project]
name = "demo-app"
version = "1.2.3"
description = """
A long description
over two lines."""
requires-python = ">=3.10"
dependencies = [
  "requests>=2",
  "rich",
]

[project.optional-dependencies]
dev = ["pytest", "ruff"]

[tool.ruff]
line-length = 100
select = ["E", "F"]

[tool.ruff.lint.per-file-ignores]
"tests/*" = ["S101"]

[tool.demo]
api_key = "sk-live-123456"
password = 'hunter2'
retries = 3
released = 2026-10-19
ratio = 0.75
"#;

const SECRETS: &str = r#"# credentials
token = "ghp_abcdef123456"
name = "service"

[auth]
user = "bob"
pass = "p@ss"
enabled = true

[server]
host = "localhost"
db.password = "s3cret"
keys = ["k-one", "k-two"]
authors = ["Ana"]
max_tokens = 4096
"#;

struct NoEnv;

impl permark_core::format::DocEnv for NoEnv {
    fn file_exists(&self, _: &str) -> bool {
        false
    }
}

fn format() -> ConfigFormat<Toml> {
    ConfigFormat(Toml)
}

#[test]
fn corpus_keeps_every_guarantee() {
    check_corpus(
        &format(),
        &[
            ("Cargo.toml", CARGO),
            ("pyproject.toml", PYPROJECT),
            ("secrets.toml", SECRETS),
        ],
    );
}

#[test]
fn structure_of_a_cargo_manifest() {
    let sheet = format().sheet(CARGO).expect("parses");
    let titles: Vec<&str> = sheet.groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "package",
            "dependencies",
            "profile.release",
            "bin[0]",
            "bin[1]",
            "workspace.dependencies"
        ]
    );
    let package = &sheet.groups[0];
    assert_eq!(package.line, 2);
    assert_eq!(package.comment.as_deref(), Some("The app crate."));
    let name = &package.rows[0];
    assert_eq!(name.path, "package.name");
    assert_eq!(name.comment.as_deref(), Some("the crate"));
    let RowValue::Scalar(scalar) = &name.value else {
        panic!("scalar");
    };
    assert_eq!(scalar.text, "permark");
    assert_eq!(scalar.kind, ScalarKind::String);
    assert!(matches!(package.rows[3].value, RowValue::List { .. }));
    let dependencies = &sheet.groups[1];
    assert!(matches!(
        dependencies.rows[0].value,
        RowValue::Raw { ref reason, .. } if reason == "inlineTable"
    ));
    assert_eq!(sheet.groups[2].comment.as_deref(), Some("Build profiles"));
    let outline: Vec<String> = sheet.outline().into_iter().map(|p| p.path).collect();
    assert_eq!(
        outline,
        ["package", "dependencies", "profile", "bin", "workspace"]
    );
}

#[test]
fn multi_line_strings_and_inline_tables_are_read_only() {
    let sheet = format().sheet(PYPROJECT).expect("parses");
    let description = sheet.scalar("project.description").expect("row");
    assert_eq!(
        description.edit,
        Editability::ReadOnly {
            reason: "multiline".to_owned()
        }
    );
    let refused = format().edit(
        PYPROJECT,
        &ConfigEdit::Set {
            path: "project.description".to_owned(),
            value: NewValue::Text {
                text: "x".to_owned(),
            },
        },
    );
    assert_eq!(refused, Err(EditRefusal::ReadOnly("multiline".to_owned())));
}

#[test]
fn a_string_edit_keeps_its_quote_kind() {
    let edit = |path: &str, text: &str| {
        let splice = format()
            .edit(
                PYPROJECT,
                &ConfigEdit::Set {
                    path: path.to_owned(),
                    value: NewValue::Text {
                        text: text.to_owned(),
                    },
                },
            )
            .expect("edit");
        splice.apply(PYPROJECT)
    };
    // literal string stays literal when it can
    assert!(edit("tool.demo.password", "plain").contains("password = 'plain'"));
    // and turns into a basic string when it cannot
    assert!(edit("tool.demo.password", "it's").contains("password = \"it's\""));
    assert!(edit("project.name", "a\"b").contains("name = \"a\\\"b\""));
}

#[test]
fn numbers_dates_and_bools_are_validated_literals() {
    let set = |path: &str, text: &str| {
        format().edit(
            PYPROJECT,
            &ConfigEdit::Set {
                path: path.to_owned(),
                value: NewValue::Literal {
                    text: text.to_owned(),
                },
            },
        )
    };
    assert!(set("tool.demo.retries", "5").is_ok());
    assert!(set("tool.demo.retries", "five").is_err());
    assert!(set("tool.demo.retries", "5 # x").is_err());
    assert!(set("tool.demo.ratio", "1e3").is_ok());
    assert!(set("tool.demo.released", "2027-01-01").is_ok());
    assert!(set("tool.demo.released", "tomorrow").is_err());
    assert!(set("tool.ruff.line-length", "true").is_err());
}

#[test]
fn secrets_are_hidden_by_key_and_by_table() {
    check_masking(
        &format(),
        "secrets",
        SECRETS,
        &[
            "ghp_abcdef123456",
            "bob",
            "p@ss",
            "s3cret",
            "k-one",
            "k-two",
        ],
    );
    let out = permark_core::format::config::fidelity::masked(&format(), SECRETS);
    // keys, bools, harmless words and plain values stay
    assert!(out.contains("token = <hidden>"));
    assert!(out.contains("enabled = true"));
    assert!(out.contains("authors = [\"Ana\"]"));
    assert!(out.contains("max_tokens = 4096"));
    assert!(out.contains("host = \"localhost\""));
    check_masking(
        &format(),
        "pyproject",
        PYPROJECT,
        &["sk-live-123456", "hunter2"],
    );
}

#[test]
fn invalid_toml_is_text_with_a_note() {
    let rendered = {
        let cx_text = "name = \n[";
        let known = std::collections::HashSet::new();
        let features = permark_core::format::MarkdownFeatures::default();
        format().render(
            cx_text,
            &permark_core::format::RenderCx {
                path: "x.toml",
                env: &NoEnv,
                features: &features,
                known: &known,
            },
        )
    };
    assert_eq!(rendered.notes, ["parseError"]);
    assert!(format().sheet("name = \n[").is_none());
    assert_eq!(
        format().edit(
            "name = \n[",
            &ConfigEdit::Remove {
                path: "x".to_owned()
            }
        ),
        Err(EditRefusal::Invalid("syntax"))
    );
}

#[test]
fn search_reads_keys_never_values() {
    let keys = format().searchable(SECRETS).expect("keys");
    assert!(keys.contains("token") && keys.contains("password") && keys.contains("host"));
    assert!(!keys.contains("ghp_") && !keys.contains("s3cret") && !keys.contains("localhost"));
}

#[test]
fn claims_only_toml() {
    let f = format();
    assert_eq!(f.claims("Cargo.toml", Some("toml"), b""), Claim::Strong);
    assert_eq!(f.claims("a.yaml", Some("yaml"), b""), Claim::No);
}
