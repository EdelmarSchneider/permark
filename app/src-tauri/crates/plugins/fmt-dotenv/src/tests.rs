use permark_core::format::config::fidelity::{check_corpus, check_masking, masked};
use permark_core::format::config::ScalarKind;
use permark_core::format::{ConfigEdit, EditRefusal, Editability, Format, NewValue};

use super::*;

const EXAMPLE: &str = "# App\nNODE_ENV=development\nPORT=3000\nDEBUG=false\n\n# Database\nDATABASE_URL=postgres://user:pass@localhost:5432/app\nDB_PASSWORD=\"s3cr3t pass\"\nexport API_KEY='sk-live-abc123'\nSECRET_KEY_BASE=\nAPP_NAME=My App # the name\nGREETING=\"hello \\\"world\\\"\"\nBASE_URL=${HOST}/api\nTOKEN_REF=${GITHUB_TOKEN}\n";

const CRLF_MIXED: &str = "A=1\r\nB=two\nC=\"three\"\r\n";

fn format() -> ConfigFormat<Dotenv> {
    ConfigFormat(Dotenv)
}

#[test]
fn corpus_keeps_every_guarantee() {
    check_corpus(
        &format(),
        &[(".env.example", EXAMPLE), (".env", CRLF_MIXED)],
    );
}

#[test]
fn reads_keys_values_kinds_and_captions() {
    let sheet = format().sheet(EXAMPLE).expect("parses");
    let first = &sheet.groups[0].rows[0];
    assert_eq!((first.key.as_str(), first.line), ("NODE_ENV", 2));
    assert_eq!(first.comment.as_deref(), Some("App"));
    assert_eq!(
        sheet.scalar("PORT").map(|s| s.kind),
        Some(ScalarKind::Number)
    );
    assert_eq!(
        sheet.scalar("DEBUG").map(|s| s.kind),
        Some(ScalarKind::Bool)
    );
    assert_eq!(
        sheet.scalar("DB_PASSWORD").map(|s| s.text.as_str()),
        Some("s3cr3t pass")
    );
    assert_eq!(
        sheet.scalar("API_KEY").map(|s| s.text.as_str()),
        Some("sk-live-abc123")
    );
    assert_eq!(
        sheet.scalar("SECRET_KEY_BASE").map(|s| s.kind),
        Some(ScalarKind::Null)
    );
    assert_eq!(
        sheet.scalar("APP_NAME").map(|s| s.text.as_str()),
        Some("My App")
    );
    assert_eq!(
        sheet
            .row("APP_NAME")
            .and_then(|r| r.comment.clone())
            .as_deref(),
        Some("the name")
    );
    assert_eq!(
        sheet.scalar("GREETING").map(|s| s.text.as_str()),
        Some("hello \"world\"")
    );
    // references are shown as written
    assert_eq!(
        sheet.scalar("BASE_URL").map(|s| s.text.as_str()),
        Some("${HOST}/api")
    );
}

#[test]
fn edits_keep_the_quote_kind() {
    let edit = |path: &str, new: &str| {
        format()
            .edit(
                EXAMPLE,
                &ConfigEdit::Set {
                    path: path.to_owned(),
                    value: NewValue::Text {
                        text: new.to_owned(),
                    },
                },
            )
            .expect("edit")
            .apply(EXAMPLE)
    };
    assert!(edit("NODE_ENV", "production").contains("NODE_ENV=production\n"));
    assert!(edit("NODE_ENV", "with space").contains("NODE_ENV=\"with space\"\n"));
    // a shell that sources the file never runs what a value holds
    assert!(edit("API_KEY", "a b;rm x $(id)").contains("export API_KEY='a b;rm x $(id)'\n"));
    assert!(edit("NODE_ENV", "a;b").contains("NODE_ENV=\"a;b\"\n"));
    assert!(edit("NODE_ENV", "$HOME").contains("NODE_ENV='$HOME'\n"));
    assert!(edit("NODE_ENV", "`id`").contains("NODE_ENV='`id`'\n"));
    assert!(format()
        .edit(
            EXAMPLE,
            &ConfigEdit::Set {
                path: "NODE_ENV".to_owned(),
                value: NewValue::Text {
                    text: "it's $(id)".to_owned(),
                },
            },
        )
        .is_err());
    assert!(edit("NODE_ENV", "has # hash").contains("NODE_ENV=\"has # hash\"\n"));
    assert!(edit("API_KEY", "new-key").contains("export API_KEY='new-key'\n"));
    assert!(edit("API_KEY", "it's").contains("export API_KEY=\"it's\"\n"));
    assert!(edit("DB_PASSWORD", "p\"q").contains("DB_PASSWORD=\"p\\\"q\"\n"));
    // an empty value takes text right after the equals sign
    assert!(edit("SECRET_KEY_BASE", "abc").contains("SECRET_KEY_BASE=abc\n"));
    assert!(edit("APP_NAME", "Other").contains("APP_NAME=Other # the name\n"));
}

#[test]
fn multi_line_values_are_read_only() {
    let text = "A=\"one\ntwo\"\nB=2\n";
    let sheet = format().sheet(text).expect("parses");
    assert_eq!(
        sheet.scalar("A").map(|s| s.edit.clone()),
        Some(Editability::ReadOnly {
            reason: "multiline".to_owned()
        })
    );
    assert_eq!(sheet.scalar("B").map(|s| s.text.as_str()), Some("2"));
    assert_eq!(sheet.row("B").map(|r| r.line), Some(3));
    assert_eq!(
        format().edit(
            text,
            &ConfigEdit::Set {
                path: "A".to_owned(),
                value: NewValue::Text {
                    text: "x".to_owned()
                }
            }
        ),
        Err(EditRefusal::ReadOnly("multiline".to_owned()))
    );
}

#[test]
fn repeated_keys_get_their_own_paths() {
    let sheet = format().sheet("A=1\nA=2\n").expect("parses");
    assert_eq!(sheet.scalar("A").map(|s| s.text.as_str()), Some("1"));
    assert_eq!(sheet.scalar("A[1]").map(|s| s.text.as_str()), Some("2"));
}

#[test]
fn secrets_are_masked_references_are_not() {
    check_masking(
        &format(),
        ".env.example",
        EXAMPLE,
        &["s3cr3t pass", "sk-live-abc123", "pass@localhost"],
    );
    let out = masked(&format(), EXAMPLE);
    assert!(out.contains("NODE_ENV=development"));
    assert!(out.contains("DB_PASSWORD=<hidden>"));
    assert!(out.contains("export API_KEY=<hidden>"));
    // a pointer to a secret is not the secret; an empty value hides nothing
    assert!(out.contains("TOKEN_REF=${GITHUB_TOKEN}"));
    assert!(out.contains("SECRET_KEY_BASE=\n"));
    // an address with a password is secret whatever its key says
    assert!(out.contains(
        "DATABASE_URL=<hidden>
"
    ));
}

#[test]
fn claims_env_files() {
    let f = format();
    for name in [
        ".env",
        ".env.local",
        ".env.example",
        ".ENV.production",
        "prod.env",
    ] {
        let ext = permark_core::format::ext_of(name);
        assert_eq!(f.claims(name, ext.as_deref(), b""), Claim::Strong, "{name}");
    }
    assert_eq!(f.claims(".envrc", None, b""), Claim::No);
    assert_eq!(f.claims("env.txt", Some("txt"), b""), Claim::No);
}
