use permark_core::format::config::fidelity::{check_corpus, check_masking, masked};
use permark_core::format::config::{RowValue, ScalarKind};
use permark_core::format::{ConfigEdit, EditRefusal, Editability, Format, NewValue};

use std::fmt::Write as _;

use super::*;

const COMPOSE: &str = r#"# Local stack
version: "3.9"
name: demo

services:
  web:
    image: nginx:1.27   # pinned
    ports:
      - "80:80"
      - "443:443"
    environment:
      API_KEY: abc123secret
      MODE: production
      DEBUG: false
    depends_on: [db, cache]
    command: ["nginx", "-g", "daemon off;"]
  db:
    image: 'postgres:16'
    restart: unless-stopped
    environment:
      POSTGRES_PASSWORD: "hunter2"
    healthcheck:
      test: ["CMD", "pg_isready"]
      interval: 10s
      retries: 5
volumes:
  data: {}
"#;

const WORKFLOW: &str = r#"name: CI
on:
  push:
    branches: [main]
  pull_request:
env:
  RUST_BACKTRACE: 1
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install
        run: |
          npm ci
          npm run build
      - name: Test
        run: npm test
        env:
          NPM_TOKEN: ${{ secrets.NPM_TOKEN }}
          CI: true
      # last
      - name: Done
        if: always()
        run: echo "done" # bye
"#;

const ANCHORS: &str = r#"defaults: &defaults
  adapter: postgres
  host: localhost
dev:
  <<: *defaults
  database: dev_db
  timeout: !!int 5000
  note: &n hello
title: >-
  folded text
  continues here
empty:
nulls: ~
quoted: "line one
  line two"
list:
  - a
  - b: 1
    c: 2
  - - nested
"#;

const MULTI: &str = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: cfg\ndata:\n  password: s3cr3t-value\n---\napiVersion: v1\nkind: Secret\nstringData:\n  token: tok-123\n";

const K8S: &str = "# deploy\napiVersion: apps/v1\nkind: Deployment\nspec:\n  replicas: 3\n  template:\n    spec:\n      containers:\n      - name: app\n        image: app:1.0\n        env:\n        - name: DB_PASSWORD\n          value: pw-1\n        - name: LEVEL\n          value: debug\n";

fn format() -> ConfigFormat<Yaml> {
    ConfigFormat(Yaml)
}

#[test]
fn corpus_keeps_every_guarantee() {
    check_corpus(
        &format(),
        &[
            ("docker-compose.yml", COMPOSE),
            ("ci.yml", WORKFLOW),
            ("anchors.yml", ANCHORS),
            ("k8s.yaml", K8S),
            ("multi.yaml", MULTI),
        ],
    );
}

#[test]
fn compose_reads_as_cards() {
    let sheet = format().sheet(COMPOSE).expect("parses");
    let titles: Vec<&str> = sheet.groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "",
            "services.web",
            "services.web.environment",
            "services.db",
            "services.db.environment",
            "services.db.healthcheck",
            "volumes"
        ]
    );
    let root = &sheet.groups[0];
    assert_eq!(root.rows[0].path, "version");
    assert_eq!(root.comment, None);
    let image = sheet.scalar("services.web.image").expect("image");
    assert_eq!(image.text, "nginx:1.27");
    assert_eq!(
        sheet
            .row("services.web.image")
            .and_then(|r| r.comment.clone())
            .as_deref(),
        Some("pinned")
    );
    assert!(matches!(
        sheet.row("services.web.ports").map(|r| &r.value),
        Some(RowValue::List { items }) if items.len() == 2 && items[0].text == "80:80"
    ));
    assert!(matches!(
        sheet.row("services.web.depends_on").map(|r| &r.value),
        Some(RowValue::Raw { reason, .. }) if reason == "flow"
    ));
    assert_eq!(
        sheet
            .scalar("services.web.environment.DEBUG")
            .map(|s| s.kind),
        Some(ScalarKind::Bool)
    );
    assert_eq!(
        sheet.scalar("services.db.image").map(|s| s.text.as_str()),
        Some("postgres:16")
    );
    let outline: Vec<String> = sheet.outline().into_iter().map(|p| p.path).collect();
    assert_eq!(outline, ["version", "name", "services", "volumes"]);
}

#[test]
fn a_workflow_has_a_card_per_step() {
    let sheet = format().sheet(WORKFLOW).expect("parses");
    let titles: Vec<&str> = sheet.groups.iter().map(|g| g.title.as_str()).collect();
    assert!(titles.contains(&"jobs.test.steps[0]"));
    assert!(titles.contains(&"jobs.test.steps[1]"));
    assert!(titles.contains(&"jobs.test.steps[2].env"));
    assert_eq!(
        sheet
            .scalar("jobs.test.steps[0].uses")
            .map(|s| s.text.as_str()),
        Some("actions/checkout@v4")
    );
    // a block scalar is shown whole and read only
    assert!(matches!(
        sheet.row("jobs.test.steps[1].run").map(|r| &r.value),
        Some(RowValue::Raw { text, reason, .. }) if reason == "blockScalar" && text == "npm ci\nnpm run build"
    ));
    // the comment line above a step is its card's caption
    let last = sheet
        .groups
        .iter()
        .find(|g| g.title == "jobs.test.steps[3]")
        .expect("step");
    assert_eq!(last.comment.as_deref(), Some("last"));
    assert_eq!(
        sheet
            .row("jobs.test.steps[3].run")
            .and_then(|r| r.comment.clone())
            .as_deref(),
        Some("bye")
    );
}

#[test]
fn anchors_tags_aliases_and_multiline_stay_read_only() {
    let sheet = format().sheet(ANCHORS).expect("parses");
    let reason = |path: &str| match sheet.scalar(path).map(|s| &s.edit) {
        Some(Editability::ReadOnly { reason }) => Some(reason.clone()),
        _ => None,
    };
    assert_eq!(reason("dev.timeout").as_deref(), Some("tag"));
    assert_eq!(reason("dev.note").as_deref(), Some("anchor"));
    assert_eq!(reason("quoted").as_deref(), Some("multiline"));
    assert!(matches!(
        sheet.row("dev.<<").map(|r| &r.value),
        Some(RowValue::Raw { reason, .. }) if reason == "alias"
    ));
    assert!(matches!(
        sheet.row("title").map(|r| &r.value),
        Some(RowValue::Raw { reason, .. }) if reason == "blockScalar"
    ));
    assert!(matches!(
        sheet.row("list").map(|r| &r.value),
        Some(RowValue::Raw { reason, .. }) if reason == "mixed"
    ));
    // plain values of an anchored mapping are still editable
    assert_eq!(reason("defaults.host"), None);
    assert_eq!(reason("dev.database"), None);
    let refused = format().edit(
        ANCHORS,
        &ConfigEdit::Set {
            path: "dev.note".to_owned(),
            value: NewValue::Text {
                text: "x".to_owned(),
            },
        },
    );
    assert_eq!(refused, Err(EditRefusal::ReadOnly("anchor".to_owned())));
}

#[test]
fn documents_after_the_first_are_one_read_only_row() {
    let sheet = format().sheet(MULTI).expect("parses");
    assert_eq!(
        sheet.scalar("data.password").map(|s| s.text.as_str()),
        Some("s3cr3t-value")
    );
    let Some(RowValue::Raw {
        text,
        reason,
        secret,
        ..
    }) = sheet.row("documents").map(|r| r.value.clone())
    else {
        panic!("documents row");
    };
    assert_eq!(reason, "multiDoc");
    assert!(text.contains("kind: Secret"));
    // the first document's masking does not leave the second one open: its tokens are masked by `mask_text`
    assert!(
        secret,
        "a secret key anywhere in the rest hides the whole rest"
    );
    check_masking(&format(), "multi", MULTI, &["s3cr3t-value", "tok-123"]);
}

#[test]
fn edits_keep_quote_kind_and_never_make_a_string_something_else() {
    let edit = |text: &str, path: &str, new: &str| {
        format()
            .edit(
                text,
                &ConfigEdit::Set {
                    path: path.to_owned(),
                    value: NewValue::Text {
                        text: new.to_owned(),
                    },
                },
            )
            .expect("edit")
            .apply(text)
    };
    assert!(edit(COMPOSE, "services.db.image", "mysql:8").contains("image: 'mysql:8'"));
    assert!(edit(COMPOSE, "services.db.image", "it's").contains("image: 'it''s'"));
    assert!(edit(COMPOSE, "version", "4").contains("version: \"4\""));
    assert!(edit(COMPOSE, "name", "other").contains("name: other\n"));
    // plain text that would read as a number | bool | null is quoted
    assert!(edit(COMPOSE, "name", "123").contains("name: \"123\""));
    assert!(edit(COMPOSE, "name", "true").contains("name: \"true\""));
    assert!(edit(COMPOSE, "name", "a: b").contains("name: \"a: b\""));
    assert!(edit(COMPOSE, "services.web.image", "x").contains("image: x   # pinned"));
    assert!(edit(COMPOSE, "services.web.ports[1]", "8443:443").contains("- \"8443:443\""));
}

#[test]
fn an_empty_value_takes_text_after_the_colon() {
    let text = "a: 1\nempty:\nb: 2\n";
    let sheet = format().sheet(text).expect("parses");
    assert_eq!(
        sheet.scalar("empty").map(|s| s.kind),
        Some(ScalarKind::Null)
    );
    let out = format()
        .edit(
            text,
            &ConfigEdit::Set {
                path: "empty".to_owned(),
                value: NewValue::Text {
                    text: "now".to_owned(),
                },
            },
        )
        .expect("edit")
        .apply(text);
    assert_eq!(out, "a: 1\nempty: now\nb: 2\n");
}

#[test]
fn literals_are_checked_against_their_kind() {
    let set = |path: &str, text: &str| {
        format().edit(
            COMPOSE,
            &ConfigEdit::Set {
                path: path.to_owned(),
                value: NewValue::Literal {
                    text: text.to_owned(),
                },
            },
        )
    };
    assert!(set("services.db.healthcheck.retries", "7").is_ok());
    assert!(set("services.db.healthcheck.retries", "seven").is_err());
    assert!(set("services.web.environment.DEBUG", "true").is_ok());
    assert!(set("services.web.environment.DEBUG", "maybe").is_err());
}

#[test]
fn secrets_are_masked_structurally() {
    check_masking(&format(), "compose", COMPOSE, &["abc123secret", "hunter2"]);
    check_masking(&format(), "workflow", WORKFLOW, &[]);
    check_masking(&format(), "k8s", K8S, &["pw-1"]);
    let out = masked(&format(), K8S);
    assert!(out.contains("value: <hidden>"));
    assert!(out.contains("value: debug"));
}

#[test]
fn crlf_text_gives_the_same_sheet() {
    let lf = format().sheet(COMPOSE).expect("lf");
    let crlf = format()
        .sheet(&COMPOSE.replace('\n', "\r\n"))
        .expect("crlf");
    assert_eq!(lf.row_count(), crlf.row_count());
    for ((_, a), (_, b)) in lf.rows().zip(crlf.rows()) {
        assert_eq!((&a.path, a.line, &a.comment), (&b.path, b.line, &b.comment));
    }
}

#[test]
fn text_that_is_not_a_mapping_or_list_is_text() {
    assert!(format().sheet("just a sentence\nand another\n").is_none());
    let empty = format().sheet("# only a comment\n").expect("empty");
    assert_eq!(empty.row_count(), 0);
    assert_eq!(format().sheet("").expect("empty").row_count(), 0);
}

struct NoEnv;

impl permark_core::format::DocEnv for NoEnv {
    fn file_exists(&self, _: &str) -> bool {
        false
    }
}

fn rendered_notes(text: &str) -> Vec<&'static str> {
    let known = std::collections::HashSet::new();
    let features = permark_core::format::MarkdownFeatures::default();
    format()
        .render(
            text,
            &permark_core::format::RenderCx {
                path: "big.yml",
                env: &NoEnv,
                features: &features,
                known: &known,
            },
        )
        .notes
}

#[test]
fn a_big_file_parses_quickly_and_one_over_the_limit_is_shown_as_text() {
    let mut text = String::from("services:\n");
    let mut n = 0;
    while text.len() < 200_000 {
        let _ = writeln!(
            text,
            "  svc{n}:\n    image: app:{n}\n    env:\n      KEY_{n}: value-{n}"
        );
        n += 1;
    }
    let start = std::time::Instant::now();
    let sheet = format().sheet(&text).expect("parses");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(3),
        "{:?}",
        start.elapsed()
    );
    assert!(sheet.row_count() > 1500);
    // a sheet this long is shown as text; so is a file over the size limit
    assert_eq!(rendered_notes(&text), ["bigConfig"]);
    assert_eq!(rendered_notes(&text.repeat(2)), ["bigConfig"]);
    assert_eq!(rendered_notes("a: 1\n"), Vec::<&str>::new());
}
