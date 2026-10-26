use permark_core::format::config::fidelity::check_corpus;
use permark_core::format::config::{RowValue, ScalarKind};
use permark_core::format::{ConfigEdit, EditRefusal, Editability, Format, NewValue};

use super::*;

const EDITORCONFIG: &str = "# EditorConfig is awesome: https://EditorConfig.org\nroot = true\n\n[*]\nindent_style = space\nindent_size = 2\nend_of_line = lf\ncharset = utf-8\ntrim_trailing_whitespace = true ; keep\ninsert_final_newline = true\n\n# Tabs for Makefiles\n[Makefile]\nindent_style = tab\n\n[*.{js,py}]\nmax_line_length = 100\n";

const GITCONFIG: &str = "[user]\n\tname = Edelmar S\n\temail = e@example.com\n[core]\n\tautocrlf = true\n\teditor = \"code --wait\"\n[remote \"origin\"]\n\turl = https://github.com/x/y.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n\tfetch = +refs/tags/*:refs/tags/*\n[credential]\n\thelper = store\n";

const SETUP_CFG: &str = "[metadata]\nname = demo\nversion = 1.2.0\nlong_description = file: README.md\nauthor = Ana\n\n[options]\npackages = find:\npython_requires = >=3.9\ninstall_requires =\n    requests\n\n[tool:pytest]\naddopts = -q\n";

const PHP_INI: &str = "; php settings\n[PHP]\nmemory_limit = 128M\ndisplay_errors = Off\nerror_reporting = E_ALL & ~E_DEPRECATED\nsession.save_path = \"/tmp\"\nextension=curl\nlong = one \\\n  two\n";

const NPMRC: &str = "registry=https://registry.npmjs.org/\n//registry.npmjs.org/:_authToken=npm_abcdef123456\nalways-auth=true\nsave-exact=true\n";

const PYPIRC: &str = "[distutils]\nindex-servers = pypi\n\n[pypi]\nusername = __token__\npassword = pypi-AgEIcHlwaS5vcmcCJGsecret\n";

fn format() -> ConfigFormat<Ini> {
    ConfigFormat(Ini)
}

#[test]
fn corpus_keeps_every_guarantee() {
    check_corpus(
        &format(),
        &[
            (".editorconfig", EDITORCONFIG),
            (".gitconfig", GITCONFIG),
            ("setup.cfg", SETUP_CFG),
            ("php.ini", PHP_INI),
            (".npmrc", NPMRC),
            (".pypirc", PYPIRC),
        ],
    );
}

#[test]
fn an_editorconfig_reads_as_cards_with_captions() {
    let sheet = format().sheet(EDITORCONFIG).expect("parses");
    let titles: Vec<&str> = sheet.groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(titles, ["", "*", "Makefile", "*.{js,py}"]);
    assert_eq!(sheet.groups[0].rows[0].path, "root");
    assert_eq!(
        sheet.groups[0].rows[0].comment.as_deref(),
        Some("EditorConfig is awesome: https://EditorConfig.org")
    );
    assert_eq!(
        sheet.groups[2].comment.as_deref(),
        Some("Tabs for Makefiles")
    );
    assert_eq!(sheet.scalar("root").map(|s| s.kind), Some(ScalarKind::Bool));
    assert_eq!(
        sheet.scalar("*.indent_size").map(|s| s.kind),
        Some(ScalarKind::Number)
    );
    let trim = sheet.row("*.trim_trailing_whitespace").expect("row");
    assert_eq!(trim.comment.as_deref(), Some("keep"));
    assert_eq!(
        sheet
            .scalar("*.trim_trailing_whitespace")
            .map(|s| s.text.as_str()),
        Some("true")
    );
}

#[test]
fn a_section_written_twice_edits_the_row_that_was_chosen() {
    let text = "[remote \"o\"]
url = A
[core]
x = 1
[remote \"o\"]
url = B
";
    let sheet = format().sheet(text).expect("parses");
    assert_eq!(
        sheet.scalar("remote.o.url").map(|s| s.text.as_str()),
        Some("A")
    );
    assert_eq!(
        sheet.scalar("remote.o.url[1]").map(|s| s.text.as_str()),
        Some("B")
    );
    let set = |path: &str| {
        format()
            .edit(
                text,
                &ConfigEdit::Set {
                    path: path.to_owned(),
                    value: NewValue::Text {
                        text: "Z".to_owned(),
                    },
                },
            )
            .expect("edit")
            .apply(text)
    };
    assert_eq!(
        set("remote.o.url[1]"),
        "[remote \"o\"]
url = A
[core]
x = 1
[remote \"o\"]
url = Z
"
    );
    assert_eq!(
        set("remote.o.url"),
        "[remote \"o\"]
url = Z
[core]
x = 1
[remote \"o\"]
url = B
"
    );
}

#[test]
fn git_subsections_and_repeated_keys_have_their_own_paths() {
    let sheet = format().sheet(GITCONFIG).expect("parses");
    assert_eq!(
        sheet.scalar("remote.origin.url").map(|s| s.text.as_str()),
        Some("https://github.com/x/y.git")
    );
    assert_eq!(
        sheet
            .scalar("remote.origin.fetch[1]")
            .map(|s| s.text.as_str()),
        Some("+refs/tags/*:refs/tags/*")
    );
    // quoted values lose their quotes in the sheet and keep them in the file
    assert_eq!(
        sheet.scalar("core.editor").map(|s| s.text.as_str()),
        Some("code --wait")
    );
    let out = format()
        .edit(
            GITCONFIG,
            &ConfigEdit::Set {
                path: "core.editor".to_owned(),
                value: NewValue::Text {
                    text: "vim".to_owned(),
                },
            },
        )
        .expect("edit")
        .apply(GITCONFIG);
    assert!(out.contains("\teditor = \"vim\"\n"));
}

#[test]
fn continuations_and_valueless_keys_are_read_only() {
    let sheet = format().sheet(PHP_INI).expect("parses");
    let long = sheet.scalar("PHP.long").expect("long");
    assert_eq!(
        long.edit,
        Editability::ReadOnly {
            reason: "continuation".to_owned()
        }
    );
    assert!(long.text.contains("two"));
    let sheet = format().sheet("[a]\nflag\nname = x\n").expect("parses");
    assert_eq!(
        sheet.scalar("a.flag").map(|s| s.edit.clone()),
        Some(Editability::ReadOnly {
            reason: "noValue".to_owned()
        })
    );
    assert_eq!(
        format().edit(
            "[a]\nflag\n",
            &ConfigEdit::Set {
                path: "a.flag".to_owned(),
                value: NewValue::Text {
                    text: "x".to_owned()
                }
            }
        ),
        Err(EditRefusal::ReadOnly("noValue".to_owned()))
    );
}

#[test]
fn values_that_cannot_be_written_are_declined() {
    let set = |text: &str| {
        format().edit(
            SETUP_CFG,
            &ConfigEdit::Set {
                path: "metadata.name".to_owned(),
                value: NewValue::Text {
                    text: text.to_owned(),
                },
            },
        )
    };
    assert!(set("fine value").is_ok());
    assert_eq!(set("a\nb"), Err(EditRefusal::Invalid("newline")));
    assert_eq!(set(" lead"), Err(EditRefusal::Invalid("notRepresentable")));
    assert_eq!(set("x # y"), Err(EditRefusal::Invalid("notRepresentable")));
}

#[test]
fn claims_by_name_extension_and_sniff() {
    let f = format();
    assert_eq!(f.claims(".editorconfig", None, b""), Claim::Strong);
    assert_eq!(f.claims("setup.cfg", Some("cfg"), b""), Claim::Strong);
    assert_eq!(f.claims("a.ini", Some("ini"), b""), Claim::Strong);
    assert_eq!(
        f.claims("app.conf", Some("conf"), b"[main]\nx = 1\n"),
        Claim::Weak
    );
    assert_eq!(
        f.claims("nginx.conf", Some("conf"), b"server {\n  listen 80;\n}\n"),
        Claim::No
    );
    assert_eq!(f.claims("a.toml", Some("toml"), b""), Claim::No);
}

#[test]
fn rows_hold_lines() {
    let sheet = format().sheet(SETUP_CFG).expect("parses");
    let rows: Vec<(&str, u32)> = sheet
        .rows()
        .map(|(_, r)| (r.key.as_str(), r.line))
        .collect();
    assert!(rows.contains(&("version", 3)));
    assert!(matches!(
        sheet.row("options.install_requires").map(|r| &r.value),
        Some(RowValue::Scalar(s)) if s.kind == ScalarKind::Null
    ));
    let outline: Vec<String> = sheet.outline().into_iter().map(|p| p.path).collect();
    assert_eq!(outline, ["metadata", "options", "tool:pytest"]);
}
