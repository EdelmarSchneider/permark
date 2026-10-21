#![allow(clippy::assert_is_empty)]

use std::fmt::Write as _;

use super::*;

fn uid(text: &str) -> ProjectUid {
    ProjectUid::parse(text).unwrap()
}

fn vp(text: &str) -> VaultPath {
    VaultPath::parse(text).unwrap()
}

fn read(text: &str) -> Record {
    match parse(text.as_bytes()) {
        RecordRead::Read(r) => r,
        other => panic!("not read: {other:?}"),
    }
}

fn reasons(text: &str) -> Vec<(u32, RecordSkip)> {
    read(text)
        .skipped
        .iter()
        .map(|s| (s.line, s.reason))
        .collect()
}

#[test]
fn reads_entries_with_defaults_and_hints() {
    let text = "# head\nversion = 1\n\n[[project]]\nid = \"k7m2q7xd2a\"\npath = \"hubs/hub-b\"\nhidden = \"own+root\"\nfirst_commit = \"89412c0d\"\nnote = \"kept\"\n\n[[project]]\npath = \"tools/notes\"\n";
    let r = read(text);
    assert_eq!(r.version, 1);
    assert_eq!(r.entries.len(), 2);
    let a = &r.entries[0];
    assert_eq!(a.uid, uid("k7m2q7xd2a"));
    assert!(!a.derived);
    assert_eq!(a.path.as_str(), "hubs/hub-b");
    assert_eq!(a.hidden, HiddenRule::OwnRoot);
    assert_eq!(a.first_commit.as_deref(), Some("89412c0d"));
    assert_eq!(a.line, 4);
    let b = &r.entries[1];
    assert!(b.derived);
    assert_eq!(b.uid, ProjectUid::derived(&vp("tools/notes").key()));
    assert_eq!(b.hidden, HiddenRule::Own);
    assert_eq!(b.line, 11);
}

#[test]
fn every_skip_reason_is_reported_with_its_line() {
    let cases: &[(&str, RecordSkip)] = &[
        ("path = 3", RecordSkip::BadPath),
        ("note = \"no path\"", RecordSkip::BadPath),
        ("path = \"\"", RecordSkip::SelfPath),
        ("path = \".\"", RecordSkip::SelfPath),
        ("path = \"/etc\"", RecordSkip::Absolute),
        (r"path = '\x'", RecordSkip::Absolute),
        ("path = \"C:/x\"", RecordSkip::Absolute),
        (r"path = '\\server\share'", RecordSkip::Absolute),
        (r"path = '\\?\C:\x'", RecordSkip::Absolute),
        (r"path = '\\.\pipe\x'", RecordSkip::Absolute),
        ("path = \"a/../b\"", RecordSkip::DotDot),
        ("path = \"..\"", RecordSkip::DotDot),
        (r"path = 'a\b'", RecordSkip::BadPath),
        ("path = \"a//b\"", RecordSkip::BadPath),
        ("path = \"a/\"", RecordSkip::BadPath),
        ("path = \"./a\"", RecordSkip::BadPath),
        (r#"path = "a\u0000b""#, RecordSkip::BadPath),
        ("path = \"ok\"\nid = \"BAD\"", RecordSkip::BadId),
    ];
    let mut text = String::from("version = 1\n");
    let mut want = Vec::new();
    let mut line = 2u32;
    for (body, reason) in cases {
        text.push_str("[[project]]\n");
        text.push_str(body);
        text.push('\n');
        want.push((line, *reason));
        line += 2 + u32::try_from(body.matches('\n').count()).unwrap();
    }
    assert_eq!(reasons(&text), want);
    assert!(read(&text).entries.is_empty());
    // a duplicate path + a duplicate id
    let dup = "[[project]]\npath = \"a\"\nid = \"aaaaaaaaaa\"\n[[project]]\npath = \"a\"\n[[project]]\npath = \"b\"\nid = \"aaaaaaaaaa\"\n";
    assert_eq!(
        reasons(dup),
        vec![(4, RecordSkip::Duplicate), (6, RecordSkip::BadId)]
    );
    // case twins: one folder where case is ignored, two on Linux
    let twins = "[[project]]\npath = \"Hub\"\n[[project]]\npath = \"hub\"\n";
    let twin_reasons = reasons(twins);
    if crate::paths::CASE_INSENSITIVE {
        assert_eq!(twin_reasons, vec![(3, RecordSkip::Duplicate)]);
    } else {
        assert!(twin_reasons.is_empty(), "Linux: two folders");
    }
}

#[test]
fn long_paths_and_too_many_entries() {
    let long = "a/".repeat(600) + "b";
    let text = format!("[[project]]\npath = \"{long}\"\n");
    assert_eq!(reasons(&text), vec![(1, RecordSkip::BadPath)]);
    let mut many = String::new();
    for i in 0..=MAX_ENTRIES {
        let _ = writeln!(many, "[[project]]\npath = \"p{i}\"");
    }
    let r = read(&many);
    assert_eq!(r.entries.len(), MAX_ENTRIES);
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].reason, RecordSkip::TooMany);
}

#[test]
fn versions_and_unreadable_files() {
    assert_eq!(read("").entries.len(), 0, "0 bytes = empty record");
    assert_eq!(read("[[project]]\npath = \"a\"\n").version, 1, "absent = 1");
    match parse(b"version = 2\n[[project]]\npath = \"a\"\nfuture = 1\n") {
        RecordRead::Newer(r) => assert_eq!(r.entries.len(), 1),
        other => panic!("{other:?}"),
    }
    for (text, line) in [
        ("version = 0\n", 1),
        ("\n\nversion = \"1\"\n", 3),
        ("version = 1\nproject = [1, 2]\n", 2),
        ("version = 1\n[project]\npath = \"a\"\n", 2),
        ("<<<<<<< HEAD\n[[project]]\n", 1),
        ("version = 1\n[[project]]\npath = \"a\n", 3),
    ] {
        match parse(text.as_bytes()) {
            RecordRead::Unreadable { line: got, .. } => assert_eq!(got, line, "{text:?}"),
            other => panic!("{text:?}: {other:?}"),
        }
    }
    assert!(matches!(
        parse(b"version = 1\n\xff\xfe"),
        RecordRead::Unreadable { line: 2, .. }
    ));
    let big = vec![b'#'; usize::try_from(MAX_RECORD_BYTES).unwrap() + 1];
    assert!(matches!(
        parse(&big),
        RecordRead::Unreadable { line: 1, .. }
    ));
    // a BOM is fine
    assert_eq!(read("\u{feff}[[project]]\npath = \"a\"\n").entries.len(), 1);
}

#[test]
fn unknown_hidden_value_is_stricter_and_bad_commit_ignored() {
    let r = read("[[project]]\npath = \"a\"\nhidden = \"none\"\nfirst_commit = \"xyz\"\n");
    assert_eq!(r.entries[0].hidden, HiddenRule::OwnRoot);
    assert!(r.entries[0].hidden_unknown);
    assert_eq!(r.entries[0].first_commit, None);
}

#[test]
fn unicode_space_and_cjk_paths_are_kept_as_written() {
    for p in ["Ünïcode dir/a b", "项目/子项目", "e\u{301}cole"] {
        let r = read(&format!("[[project]]\npath = \"{p}\"\n"));
        assert_eq!(r.entries[0].path.as_str(), p);
    }
}

// ---- ops -----------------------------------------------------------------------------------------------------------

const BASE: &str = "# Projects this folder includes.\n# keep me\nversion = 1\nextra = \"unknown\"  # kept\n\n# about a\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"hubs/a\"   # where a lives\nmine = true\n\n# about b\n[[project]]\nid = \"bbbbbbbbbb\"\npath = \"hubs/b\"\nhidden = \"own+root\"\n\n[other]\nx = 1\n";

#[test]
fn add_appends_after_the_last_entry_with_one_blank_line() {
    let out = apply(
        BASE,
        &[RecordOp::Add {
            uid: uid("cccccccccc"),
            path: vp("tools/c d"),
            hidden: HiddenRule::Own,
        }],
    );
    let want = BASE.replace(
        "hidden = \"own+root\"\n\n[other]",
        "hidden = \"own+root\"\n\n[[project]]\nid = \"cccccccccc\"\npath = \"tools/c d\"\n\n[other]",
    );
    assert_eq!(out, want);
    assert_eq!(read(&out).entries.len(), 3);
}

#[test]
fn add_to_a_new_and_to_an_empty_record() {
    let fresh = apply(
        &new_text("\n"),
        &[RecordOp::Add {
            uid: uid("cccccccccc"),
            path: vp("a"),
            hidden: HiddenRule::OwnRoot,
        }],
    );
    assert_eq!(
        fresh,
        format!("{HEADER}\nversion = 1\n\n[[project]]\nid = \"cccccccccc\"\npath = \"a\"\nhidden = \"own+root\"\n")
    );
    let empty = apply(
        "",
        &[RecordOp::Add {
            uid: uid("cccccccccc"),
            path: vp("a"),
            hidden: HiddenRule::Own,
        }],
    );
    assert_eq!(empty, "[[project]]\nid = \"cccccccccc\"\npath = \"a\"\n");
}

#[test]
fn remove_drops_the_table_and_comments_above_move_to_the_next() {
    let out = apply(
        BASE,
        &[RecordOp::Remove {
            uid: uid("aaaaaaaaaa"),
        }],
    );
    // the comment above `a` now sits above the next entry
    let want = BASE.replace(
        "[[project]]\nid = \"aaaaaaaaaa\"\npath = \"hubs/a\"   # where a lives\nmine = true\n",
        "",
    );
    assert_eq!(out, want);
    let out = apply(
        BASE,
        &[RecordOp::Remove {
            uid: uid("bbbbbbbbbb"),
        }],
    );
    assert_eq!(
        out,
        BASE.replace(
            "[[project]]\nid = \"bbbbbbbbbb\"\npath = \"hubs/b\"\nhidden = \"own+root\"\n",
            ""
        )
    );
    let r = read(&out);
    assert_eq!(r.entries.len(), 1);
    assert!(out.contains("# about b"), "comment stays");
    assert!(out.contains("[other]\nx = 1"));
}

#[test]
fn set_path_hidden_and_commit_touch_only_their_tokens() {
    let out = apply(
        BASE,
        &[
            RecordOp::SetPath {
                uid: uid("aaaaaaaaaa"),
                path: vp("hubs/a2"),
            },
            RecordOp::SetHidden {
                uid: uid("aaaaaaaaaa"),
                rule: HiddenRule::OwnRoot,
            },
            RecordOp::SetHidden {
                uid: uid("bbbbbbbbbb"),
                rule: HiddenRule::Own,
            },
            RecordOp::SetFirstCommit {
                uid: uid("bbbbbbbbbb"),
                oid: "89412c0d".to_owned(),
            },
        ],
    );
    let want = BASE
        .replace(
            "path = \"hubs/a\"   # where a lives\n",
            "path = \"hubs/a2\"   # where a lives\nhidden = \"own+root\"\n",
        )
        .replace(
            "path = \"hubs/b\"\nhidden = \"own+root\"\n",
            "path = \"hubs/b\"\nfirst_commit = \"89412c0d\"\n",
        );
    assert_eq!(out, want);
}

#[test]
fn a_derived_entry_gets_its_id_line_first() {
    let text = "[[project]]\npath = \"tools/x\"\n";
    let derived = read(text).entries[0].uid.clone();
    let out = apply(
        text,
        &[RecordOp::SetHidden {
            uid: derived.clone(),
            rule: HiddenRule::OwnRoot,
        }],
    );
    assert_eq!(
        out,
        format!("[[project]]\nid = \"{derived}\"\npath = \"tools/x\"\nhidden = \"own+root\"\n")
    );
    assert_eq!(read(&out).entries[0].uid, derived);
}

#[test]
fn crlf_bom_and_missing_final_newline_are_kept() {
    let crlf = format!("\u{feff}{}", BASE.replace('\n', "\r\n"));
    let out = apply(
        &crlf,
        &[
            RecordOp::Add {
                uid: uid("cccccccccc"),
                path: vp("c"),
                hidden: HiddenRule::OwnRoot,
            },
            RecordOp::Remove {
                uid: uid("aaaaaaaaaa"),
            },
        ],
    );
    assert!(out.starts_with('\u{feff}'));
    assert!(
        !out.replace("\r\n", "").contains('\n'),
        "only CRLF: {out:?}"
    );
    assert_eq!(read(&out).entries.len(), 2);
    let bare = "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"a\"";
    let out = apply(
        bare,
        &[RecordOp::Add {
            uid: uid("cccccccccc"),
            path: vp("c"),
            hidden: HiddenRule::Own,
        }],
    );
    assert_eq!(
        out,
        "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"a\"\n\n[[project]]\nid = \"cccccccccc\"\npath = \"c\""
    );
}

#[test]
fn quoting_is_toml_escaped() {
    let out = apply(
        "",
        &[RecordOp::Add {
            uid: uid("cccccccccc"),
            path: vp("it's 'name' #1"),
            hidden: HiddenRule::Own,
        }],
    );
    assert_eq!(read(&out).entries[0].path.as_str(), "it's 'name' #1");
}

#[test]
fn ops_on_unknown_ids_change_nothing() {
    let out = apply(
        BASE,
        &[RecordOp::Remove {
            uid: uid("zzzzzzzzzz"),
        }],
    );
    assert_eq!(out, BASE);
}

/// xorshift: deterministic random sequences without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % (n as u64)).unwrap()
    }
}

/// In-memory model of the record: (uid, path, hidden).
type Model = Vec<(ProjectUid, String, HiddenRule)>;

fn model_of(text: &str) -> Model {
    read(text)
        .entries
        .iter()
        .map(|e| (e.uid.clone(), e.path.as_str().to_owned(), e.hidden))
        .collect()
}

#[test]
fn memory_model_equals_text_edit_property() {
    let seeds = [
        BASE.to_owned(),
        BASE.replace('\n', "\r\n"),
        new_text("\n"),
        String::new(),
    ];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for run in 0..500 {
        let mut text = seeds[run % seeds.len()].clone();
        let unknown_before = text.contains("extra = \"unknown\"");
        let mut model = model_of(&text);
        for step in 0..12 {
            let op = if model.is_empty() || rng.below(3) == 0 {
                let id = ProjectUid::draw(&format!("{run}-{step}"), &|u| {
                    model.iter().any(|(m, _, _)| m == u)
                });
                let path = format!("p{run}/s {step}/ü");
                let hidden = if rng.below(2) == 0 {
                    HiddenRule::Own
                } else {
                    HiddenRule::OwnRoot
                };
                model.push((id.clone(), path.clone(), hidden));
                RecordOp::Add {
                    uid: id,
                    path: vp(&path),
                    hidden,
                }
            } else {
                let i = rng.below(model.len());
                let id = model[i].0.clone();
                match rng.below(3) {
                    0 => {
                        model.remove(i);
                        RecordOp::Remove { uid: id }
                    }
                    1 => {
                        let path = format!("moved/{run}/{step}");
                        model[i].1.clone_from(&path);
                        RecordOp::SetPath {
                            uid: id,
                            path: vp(&path),
                        }
                    }
                    _ => {
                        let rule = if model[i].2 == HiddenRule::Own {
                            HiddenRule::OwnRoot
                        } else {
                            HiddenRule::Own
                        };
                        model[i].2 = rule;
                        RecordOp::SetHidden { uid: id, rule }
                    }
                }
            };
            text = apply(&text, &[op]);
            assert_eq!(model_of(&text), model, "run {run} step {step}:\n{text}");
        }
        if unknown_before {
            assert!(
                text.contains("extra = \"unknown\"  # kept"),
                "unknown key kept"
            );
            assert!(text.contains("[other]\nx = 1") || text.contains("[other]\r\nx = 1"));
        }
        if seeds[run % seeds.len()].contains("\r\n") {
            assert!(!text.replace("\r\n", "").contains('\n'), "CRLF kept");
        }
    }
}

// ---- disk ----------------------------------------------------------------------------------------------------------

fn root_with_permark() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".permark")).unwrap();
    dir
}

#[test]
fn write_creates_reads_back_and_refuses_unusable_records() {
    let dir = root_with_permark();
    let done = super::write(
        dir.path(),
        &[RecordOp::Add {
            uid: uid("cccccccccc"),
            path: vp("a"),
            hidden: HiddenRule::Own,
        }],
        true,
        &|_| {},
    )
    .unwrap();
    assert!(done.created);
    let RecordRead::Read(r) = super::read(dir.path()) else {
        panic!()
    };
    assert_eq!(r.entries.len(), 1);
    // newer | unreadable: writes refused, bytes untouched
    for text in ["version = 9\n", "not toml ["] {
        std::fs::write(record_path(dir.path()), text).unwrap();
        assert!(matches!(
            super::write(
                dir.path(),
                &[RecordOp::Remove {
                    uid: uid("cccccccccc")
                }],
                true,
                &|_| {}
            ),
            Err(RecordError::Unavailable)
        ));
        assert_eq!(
            std::fs::read_to_string(record_path(dir.path())).unwrap(),
            text
        );
    }
    // no .permark
    let bare = tempfile::tempdir().unwrap();
    assert!(matches!(super::read(bare.path()), RecordRead::Absent));
    assert!(matches!(
        super::write(bare.path(), &[], true, &|_| {}),
        Err(RecordError::Unavailable)
    ));
    // .permark is a file
    std::fs::write(bare.path().join(".permark"), "x").unwrap();
    assert!(matches!(super::read(bare.path()), RecordRead::Absent));
    assert!(matches!(
        super::write(bare.path(), &[], true, &|_| {}),
        Err(RecordError::Unavailable)
    ));
}

#[test]
fn a_record_that_is_too_large_is_unreadable() {
    let dir = root_with_permark();
    std::fs::write(
        record_path(dir.path()),
        vec![b'#'; usize::try_from(MAX_RECORD_BYTES).unwrap() + 1],
    )
    .unwrap();
    assert!(matches!(
        super::read(dir.path()),
        RecordRead::Unreadable { line: 1, .. }
    ));
}
