//! Edits of official names (`projects.md` §7.2, D46): rename | remove | look change only the entry; every other byte
//! of `vocabulary.toml` (comments, order, spacing, CRLF | LF, trailing newline, other spellings of the file) stays.
//! Then the service: refusals, BOM, an outside edit between read and write (op re-applied, never written over),
//! newer | unreadable files never written, a folder without `.permark/`.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::vocabulary::{
    parse, remove_text, rename_text, set_style_text, EditRefusal, VocabKind, VocabRead, VocabStyle,
};
use crate::conditions::Distribution;
use crate::error::ErrorKind;
use crate::events::{CoreEvent, Recorder};
use crate::paths::Root;
use crate::plugins::PluginRegistry;
use crate::settings::AppSettings;
use crate::state::AppDirs;
use crate::workspace::{CoreEnv, ProjectCore};

/// A hand-written list with everything the format allows around the entries (CRLF).
const HAND: &str = "\
# Official tags of the team. Ask Ana before adding.\r\n\
version = 1\r\n\
\r\n\
# --- tags ---\r\n\
[tags.spec] # specs + plans\r\n\
color = \"blue\"   # kept\r\n\
icon = \"book-open\"\r\n\
owner = \"ana\"\r\n\
\r\n\
[tags.\"needs review\"]\r\n\
\r\n\
# about drafts\r\n\
[tags.draft]\r\n\
color = \"gray\"\r\n\
\r\n\
[categories.software]\r\n\
color = \"teal\"\r\n";

fn style(color: Option<&str>, icon: Option<&str>) -> VocabStyle {
    VocabStyle {
        color: color.map(str::to_owned),
        icon: icon.map(str::to_owned),
    }
}

fn tags_of(text: &str) -> Vec<String> {
    match parse(text.as_bytes()) {
        VocabRead::Read(v) => v.tags.keys().cloned().collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn rename_changes_only_the_key_text() {
    let out = rename_text(HAND, VocabKind::Tag, "spec", "specs")
        .unwrap()
        .unwrap();
    assert_eq!(
        out.as_bytes(),
        HAND.replace("[tags.spec] #", "[tags.specs] #").as_bytes()
    );
    // a quoted key: the new name written the way TOML needs it
    let out = rename_text(HAND, VocabKind::Tag, "needs-review", "ready")
        .unwrap()
        .unwrap();
    assert_eq!(out, HAND.replace("[tags.\"needs review\"]", "[tags.ready]"));
    let out = rename_text(HAND, VocabKind::Tag, "draft", "été 2026")
        .unwrap()
        .unwrap();
    assert_eq!(out, HAND.replace("[tags.draft]", "[tags.\"été 2026\"]"));
    let out = rename_text(HAND, VocabKind::Category, "software", "code")
        .unwrap()
        .unwrap();
    assert_eq!(
        out,
        HAND.replace("[categories.software]", "[categories.code]")
    );
    assert_eq!(tags_of(&out), ["draft", "needs-review", "spec"]);
}

#[test]
fn rename_refuses_a_taken_or_unknown_name_and_same_name_is_nothing() {
    assert_eq!(
        rename_text(HAND, VocabKind::Tag, "spec", "draft"),
        Err(EditRefusal::Taken)
    );
    assert_eq!(
        rename_text(HAND, VocabKind::Tag, "spec", "needs-review"),
        Err(EditRefusal::Taken),
        "taken by another spelling"
    );
    assert_eq!(
        rename_text(HAND, VocabKind::Tag, "nope", "x"),
        Err(EditRefusal::NotListed)
    );
    assert_eq!(
        rename_text(HAND, VocabKind::Tag, "software", "x"),
        Err(EditRefusal::NotListed),
        "a category is not a tag"
    );
    assert_eq!(rename_text(HAND, VocabKind::Tag, "spec", "spec"), Ok(None));
    assert_eq!(
        rename_text("version = 1\n", VocabKind::Tag, "a", "b"),
        Err(EditRefusal::NotListed)
    );
    assert_eq!(
        rename_text("", VocabKind::Tag, "a", "b"),
        Err(EditRefusal::NotListed)
    );
    assert!(matches!(
        rename_text("tags = 3\n", VocabKind::Tag, "a", "b"),
        Err(EditRefusal::Unreadable(_))
    ));
    assert!(matches!(
        rename_text("[tags.a]\n<<<<<<< HEAD\n", VocabKind::Tag, "a", "b"),
        Err(EditRefusal::Unreadable(_))
    ));
}

#[test]
fn rename_keeps_the_order_of_key_value_and_inline_lists() {
    let kv = "[tags]\nalpha = {}\nbeta = { color = \"red\" } # b\ngamma = true\n";
    let out = rename_text(kv, VocabKind::Tag, "beta", "bravo")
        .unwrap()
        .unwrap();
    assert_eq!(out, kv.replace("beta =", "bravo ="));
    let inline = "tags = { alpha = {}, beta = {}, gamma = {} }\n";
    let out = rename_text(inline, VocabKind::Tag, "alpha", "a")
        .unwrap()
        .unwrap();
    assert_eq!(out, "tags = { a = {}, beta = {}, gamma = {} }\n");
    let dotted = "tags.alpha.color = \"red\"\ntags.beta.icon = \"star\"\n";
    let out = rename_text(dotted, VocabKind::Tag, "alpha", "a")
        .unwrap()
        .unwrap();
    assert_eq!(out, "tags.a.color = \"red\"\ntags.beta.icon = \"star\"\n");
}

#[test]
fn several_spellings_of_one_name() {
    // `Spec` is the one read; `spec` was ignored: rename keeps the read one, drops the other
    let text = "[tags.Spec]\ncolor = \"blue\"\n\n[tags.x]\n\n[tags.spec]\ncolor = \"red\"\n";
    let out = rename_text(text, VocabKind::Tag, "spec", "plan")
        .unwrap()
        .unwrap();
    assert_eq!(out, "[tags.plan]\ncolor = \"blue\"\n\n[tags.x]\n");
    let out = remove_text(text, VocabKind::Tag, "spec").unwrap();
    assert_eq!(out, "\n[tags.x]\n");
    assert_eq!(tags_of(&out), ["x"]);
}

#[test]
fn remove_takes_only_the_entry_and_keeps_comments() {
    let out = remove_text(HAND, VocabKind::Tag, "needs-review").unwrap();
    assert_eq!(out, HAND.replace("[tags.\"needs review\"]\r\n\r\n", ""));
    // the comments in front of a removed table stay in front of the next one
    let out = remove_text(HAND, VocabKind::Tag, "spec").unwrap();
    assert_eq!(
        out,
        HAND.replace(
            "[tags.spec] # specs + plans\r\ncolor = \"blue\"   # kept\r\nicon = \"book-open\"\r\nowner = \"ana\"\r\n\r\n",
            ""
        )
    );
    let out = remove_text(HAND, VocabKind::Tag, "draft").unwrap();
    assert_eq!(
        out,
        HAND.replace(
            "\r\n# about drafts\r\n[tags.draft]\r\ncolor = \"gray\"\r\n",
            "\r\n# about drafts\r\n"
        )
        .replace(
            "# about drafts\r\n\r\n[categories",
            "# about drafts\r\n[categories"
        )
    );
    // the last table: its comments go to the end of the file
    let last = "version = 1\n\n# keep me\n[tags.a]\n";
    assert_eq!(
        remove_text(last, VocabKind::Tag, "a").unwrap(),
        "version = 1\n\n# keep me\n"
    );
    // every entry gone: the file stays (version + comments), the list is empty
    let mut text = HAND.to_owned();
    for name in ["spec", "needs-review", "draft"] {
        text = remove_text(&text, VocabKind::Tag, name).unwrap();
    }
    assert_eq!(tags_of(&text), Vec::<String>::new());
    assert!(text.contains("# Official tags of the team") && text.contains("# about drafts"));
    assert!(text.contains("[categories.software]\r\ncolor = \"teal\"\r\n"));
    assert!(
        !text.contains('\n') || !text.replace("\r\n", "").contains('\n'),
        "still all CRLF"
    );
    assert_eq!(
        remove_text(HAND, VocabKind::Tag, "nope"),
        Err(EditRefusal::NotListed)
    );
}

#[test]
fn remove_in_key_value_inline_and_no_trailing_newline() {
    let kv = "[tags]\nalpha = {}\nbeta = {} # b\ngamma = true";
    assert_eq!(
        remove_text(kv, VocabKind::Tag, "beta").unwrap(),
        "[tags]\nalpha = {}\ngamma = true"
    );
    assert_eq!(
        remove_text("tags = { a = {}, b = {} }\n", VocabKind::Tag, "a").unwrap(),
        "tags = { b = {} }\n"
    );
    assert_eq!(
        remove_text("[tags.a]\ncolor = \"red\"", VocabKind::Tag, "a").unwrap(),
        ""
    );
}

#[test]
fn set_style_writes_only_color_and_icon() {
    // a new key goes after the entry's keys; the rest stays
    let out = set_style_text(
        HAND,
        VocabKind::Tag,
        "needs-review",
        &style(Some("red"), None),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        out,
        HAND.replace(
            "[tags.\"needs review\"]\r\n",
            "[tags.\"needs review\"]\r\ncolor = \"red\"\r\n"
        )
    );
    // a changed value keeps its trailing comment and spacing; a cleared one goes; unknown keys stay
    let out = set_style_text(HAND, VocabKind::Tag, "spec", &style(Some("green"), None))
        .unwrap()
        .unwrap();
    assert_eq!(
        out,
        HAND.replace("color = \"blue\"   # kept", "color = \"green\"   # kept")
            .replace("icon = \"book-open\"\r\n", "")
    );
    let out = set_style_text(HAND, VocabKind::Tag, "spec", &style(None, None))
        .unwrap()
        .unwrap();
    assert!(
        out.contains("[tags.spec] # specs + plans\r\nowner = \"ana\"\r\n"),
        "{out}"
    );
    // same look -> nothing
    assert_eq!(
        set_style_text(
            HAND,
            VocabKind::Tag,
            "spec",
            &style(Some("blue"), Some("book-open"))
        ),
        Ok(None)
    );
    assert_eq!(
        set_style_text(HAND, VocabKind::Category, "nope", &style(None, None)),
        Err(EditRefusal::NotListed)
    );
}

#[test]
fn set_style_in_key_value_and_inline_entries_stays_on_its_line() {
    let kv = "[tags]\nalpha = true # a\nbeta = {}\n";
    assert_eq!(
        set_style_text(kv, VocabKind::Tag, "alpha", &style(Some("red"), None))
            .unwrap()
            .unwrap(),
        "[tags]\nalpha = { color = \"red\" } # a\nbeta = {}\n"
    );
    let inline = "tags = { a = { icon = \"star\" } }\n";
    let out = set_style_text(
        inline,
        VocabKind::Tag,
        "a",
        &style(Some("red"), Some("star")),
    )
    .unwrap()
    .unwrap();
    assert_eq!(out, "tags = { a = { icon = \"star\", color = \"red\" } }\n");
    // an implicit entry (only a sub-table of it written): gets its own header
    let implicit = "[tags.a.extra]\nx = 1\n";
    let out = set_style_text(implicit, VocabKind::Tag, "a", &style(Some("red"), None))
        .unwrap()
        .unwrap();
    assert!(
        matches!(parse(out.as_bytes()), VocabRead::Read(v) if v.tags["a"].color.as_deref() == Some("red")),
        "{out}"
    );
    assert!(out.contains("[tags.a.extra]\nx = 1\n"), "{out}");
}

// ---- the service -------------------------------------------------------------------------------------------------

fn env(base: &Path) -> (CoreEnv, Arc<Recorder>) {
    let sink = Arc::new(Recorder::default());
    let env = CoreEnv::new(
        AppDirs::under(base),
        Arc::new(PluginRegistry::new(Vec::new()).unwrap()),
        sink.clone(),
        Distribution::Direct,
        AppSettings::default(),
    );
    (env, sink)
}

fn changed(sink: &Recorder) -> bool {
    sink.take()
        .iter()
        .any(|e| matches!(e, CoreEvent::ProjectsChanged(_)))
}

#[test]
#[allow(clippy::too_many_lines)] // one story: every op, refusal and no-op on one file
fn service_ops_write_the_file_tell_the_window_and_refuse_clearly() {
    let dir = tempfile::tempdir().unwrap();
    let base = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("team ü");
    fs::create_dir_all(root.join(".permark")).unwrap();
    let file = root.join(".permark/vocabulary.toml");
    let bom = [b"\xEF\xBB\xBF".as_slice(), HAND.as_bytes()].concat();
    fs::write(&file, &bom).unwrap();
    fs::write(root.join("doc.md"), "---\ntags: [spec]\n---\n# doc\n").unwrap();
    let (env, sink) = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    let projects = core.projects();
    let _ = sink.take();

    projects
        .vocabulary_rename(VocabKind::Tag, " Spec ", "Plans")
        .unwrap();
    let want = [
        b"\xEF\xBB\xBF".as_slice(),
        HAND.replace("[tags.spec] #", "[tags.plans] #").as_bytes(),
    ]
    .concat();
    assert_eq!(fs::read(&file).unwrap(), want, "BOM kept, only the key");
    assert!(changed(&sink));
    assert!(projects
        .vocabularies()
        .official_style(VocabKind::Tag, "plans")
        .is_some());
    assert_eq!(
        fs::read_to_string(root.join("doc.md")).unwrap(),
        "---\ntags: [spec]\n---\n# doc\n",
        "documents never edited"
    );

    let kind = |r: Result<(), crate::error::IpcError>| {
        let e = r.unwrap_err();
        (e.kind, e.subject.clone())
    };
    assert_eq!(
        kind(projects.vocabulary_rename(VocabKind::Tag, "plans", "draft")),
        (ErrorKind::Exists, Some("newName".to_owned()))
    );
    assert_eq!(
        kind(projects.vocabulary_rename(VocabKind::Tag, "nope", "x")),
        (ErrorKind::NotFound, Some("name".to_owned()))
    );
    assert_eq!(
        kind(projects.vocabulary_rename(VocabKind::Tag, "plans", "  ")),
        (ErrorKind::InvalidData, Some("newName".to_owned()))
    );
    assert_eq!(
        kind(projects.vocabulary_remove(VocabKind::Category, "")),
        (ErrorKind::InvalidData, Some("name".to_owned()))
    );
    assert_eq!(
        kind(projects.vocabulary_set_style(VocabKind::Tag, "plans", Some("ultraviolet"), None)),
        (ErrorKind::InvalidData, Some("color".to_owned()))
    );
    assert_eq!(
        kind(projects.vocabulary_set_style(VocabKind::Tag, "plans", None, Some("<svg>"))),
        (ErrorKind::InvalidData, Some("icon".to_owned()))
    );
    assert_eq!(fs::read(&file).unwrap(), want, "refusals write nothing");
    assert!(!changed(&sink));

    // same look | same name: nothing written, nobody told
    projects
        .vocabulary_set_style(VocabKind::Tag, "plans", Some("BLUE"), Some("book-open"))
        .unwrap();
    projects
        .vocabulary_rename(VocabKind::Tag, "plans", "PLANS")
        .unwrap();
    assert_eq!(fs::read(&file).unwrap(), want);
    assert!(!changed(&sink));

    projects
        .vocabulary_set_style(VocabKind::Category, "software", None, Some("code"))
        .unwrap();
    projects.vocabulary_remove(VocabKind::Tag, "draft").unwrap();
    let text = String::from_utf8(fs::read(&file).unwrap()).unwrap();
    assert!(
        text.contains("[categories.software]\r\nicon = \"code\"\r\n"),
        "{text}"
    );
    assert!(!text.contains("[tags.draft]") && text.contains("# about drafts"));
    let vocab = projects.vocabularies();
    let official = vocab.official.vocab().unwrap();
    assert_eq!(
        official.tags.keys().collect::<Vec<_>>(),
        ["needs-review", "plans"]
    );
    assert!(changed(&sink));

    // newer | unreadable: never written over
    for bad in [
        "version = 2\n[tags.plans]\n",
        "[tags.plans]\n<<<<<<< HEAD\n",
    ] {
        fs::write(&file, bad).unwrap();
        for r in [
            projects.vocabulary_remove(VocabKind::Tag, "plans"),
            projects.vocabulary_rename(VocabKind::Tag, "plans", "x"),
            projects.vocabulary_set_style(VocabKind::Tag, "plans", None, None),
            projects.promote(VocabKind::Tag, "y"),
        ] {
            assert_eq!(
                kind(r),
                (ErrorKind::Unavailable, Some("vocabulary".to_owned()))
            );
        }
        assert_eq!(fs::read_to_string(&file).unwrap(), bad);
    }
    // no file: nothing listed
    fs::remove_file(&file).unwrap();
    assert_eq!(
        kind(projects.vocabulary_remove(VocabKind::Tag, "plans")),
        (ErrorKind::NotFound, Some("name".to_owned()))
    );
    assert!(!file.exists());
}

#[test]
fn a_folder_without_permark_refuses_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    let e = core
        .projects()
        .vocabulary_remove(VocabKind::Tag, "a")
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unavailable);
    assert!(!root.join(".permark").exists());
}

/// Two writers (the root tab and a second app instance) edit the list at once: every edit lands, none written over.
#[test]
fn concurrent_writers_lose_no_edit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join(".permark")).unwrap();
    let file = root.join(".permark/vocabulary.toml");
    let mut text = String::from("# shared\nversion = 1\n");
    for i in 0..20 {
        text.push_str("\n[tags.t");
        text.push_str(&i.to_string());
        text.push_str("]\n");
    }
    fs::write(&file, &text).unwrap();
    let (base_a, base_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (env_a, _) = env(base_a.path());
    let (env_b, _) = env(base_b.path());
    let a = ProjectCore::open(Root::open(&root).unwrap(), &env_a);
    let b = ProjectCore::open(Root::open(&root).unwrap(), &env_b);
    let ta = std::thread::spawn({
        let a = Arc::clone(&a);
        move || {
            for i in 0..10 {
                a.projects()
                    .vocabulary_rename(VocabKind::Tag, &format!("t{i}"), &format!("a{i}"))
                    .unwrap();
            }
        }
    });
    let tb = std::thread::spawn({
        let b = Arc::clone(&b);
        move || {
            for i in 10..20 {
                b.projects()
                    .vocabulary_set_style(VocabKind::Tag, &format!("t{i}"), Some("red"), None)
                    .unwrap();
            }
            for i in 0..5 {
                b.projects()
                    .promote(VocabKind::Category, &format!("c{i}"))
                    .unwrap();
            }
        }
    });
    ta.join().unwrap();
    tb.join().unwrap();
    let out = fs::read_to_string(&file).unwrap();
    let VocabRead::Read(v) = parse(out.as_bytes()) else {
        panic!("{out}")
    };
    for i in 0..10 {
        assert!(v.tags.contains_key(&format!("a{i}")), "rename {i}: {out}");
    }
    for i in 10..20 {
        assert_eq!(
            v.tags[&format!("t{i}")].color.as_deref(),
            Some("red"),
            "{out}"
        );
    }
    assert_eq!(v.tags.len(), 20);
    assert_eq!(v.categories.len(), 5);
    assert!(out.starts_with("# shared\nversion = 1\n"));
}

/// The person's editor writes the file between PerMark's read and its write: the op lands on the person's bytes.
#[test]
fn an_outside_edit_between_read_and_write_is_kept_and_the_op_reapplied() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vocabulary.toml");
    fs::write(&path, "version = 1\n\n[tags.a]\n").unwrap();
    let own = crate::watch::OwnWrites::new();
    let mut calls = 0;
    let written = super::vocabulary::write_file(&path, &own, |text| {
        if calls == 0 {
            fs::write(&path, "# person\nversion = 1\n\n[tags.a]\n\n[tags.b]\n").unwrap();
        }
        calls += 1;
        rename_text(text, VocabKind::Tag, "a", "z")
            .map_err(|_| crate::IpcError::new(ErrorKind::Internal))
    })
    .unwrap()
    .unwrap();
    assert_eq!(calls, 2);
    let want = "# person\nversion = 1\n\n[tags.z]\n\n[tags.b]\n";
    assert_eq!(written, want.as_bytes());
    assert_eq!(fs::read_to_string(&path).unwrap(), want);
    // the person renames the entry meanwhile: the op is asked again on the new text and refuses (never written over)
    let mut calls = 0;
    let e = super::vocabulary::write_file(&path, &own, |text| {
        if calls == 0 {
            fs::write(&path, "[tags.y]\n").unwrap();
        }
        calls += 1;
        remove_text(text, VocabKind::Tag, "z")
            .map(Some)
            .map_err(|_| crate::IpcError::new(ErrorKind::NotFound).subject("name"))
    })
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotFound);
    assert_eq!(fs::read_to_string(&path).unwrap(), "[tags.y]\n");
}

/// An editor that writes on every try: `Conflict` after the attempts, the person's last bytes on disk.
#[test]
fn a_file_that_keeps_changing_is_a_conflict_and_the_persons_bytes_stay() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vocabulary.toml");
    fs::write(&path, "[tags.a]\n").unwrap();
    let own = crate::watch::OwnWrites::new();
    let mut n = 0;
    let e = super::vocabulary::write_file(&path, &own, |text| {
        n += 1;
        fs::write(&path, format!("[tags.a]\n# edit {n}\n")).unwrap();
        set_style_text(text, VocabKind::Tag, "a", &style(Some("red"), None))
            .map_err(|_| crate::IpcError::new(ErrorKind::Internal))
    })
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Conflict);
    assert_eq!(n, super::vocabulary::WRITE_ATTEMPTS);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        format!("[tags.a]\n# edit {n}\n")
    );
}
