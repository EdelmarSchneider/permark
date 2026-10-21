//! Verifier adversarial tests (branch `verify/projects-round2`, not for merge): concurrent record edits.

use std::cell::Cell;

use super::record::{self, HiddenRule, RecordError, RecordOp, RecordRead};
use super::uid::ProjectUid;
use crate::paths::VaultPath;

const PERSON: &str =
    "# person's comment\nversion = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"a\"\n";

fn add() -> RecordOp {
    RecordOp::Add {
        uid: ProjectUid::parse("bbbbbbbbbb").unwrap(),
        path: VaultPath::parse("b").unwrap(),
        hidden: HiddenRule::Own,
    }
}

/// Editor writes once between PerMark's read and rename -> PerMark re-applies on the person's bytes.
#[test]
fn record_edit_between_read_and_rename_is_reapplied() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".permark")).unwrap();
    let path = record::record_path(dir.path());
    std::fs::write(&path, "version = 1\n").unwrap();
    let calls = Cell::new(0);
    let done = record::write(dir.path(), &[add()], false, &|_| {
        if calls.get() == 0 {
            std::fs::write(&path, PERSON).unwrap();
        }
        calls.set(calls.get() + 1);
    })
    .unwrap();
    let text = String::from_utf8(done.bytes).unwrap();
    assert!(text.starts_with(PERSON), "person's bytes kept: {text}");
    assert!(text.contains("path = \"b\""), "{text}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

/// Editor writes twice -> Conflict, nothing written, the person's last bytes on disk.
#[test]
fn record_edited_twice_is_a_conflict_and_person_bytes_stay() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".permark")).unwrap();
    let path = record::record_path(dir.path());
    std::fs::write(&path, "version = 1\n").unwrap();
    let calls = Cell::new(0);
    let r = record::write(dir.path(), &[add()], false, &|_| {
        let n = calls.get();
        std::fs::write(&path, format!("{PERSON}# edit {n}\n")).unwrap();
        calls.set(n + 1);
    });
    assert!(matches!(r, Err(RecordError::Conflict)), "{r:?}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        format!("{PERSON}# edit 1\n")
    );
    assert!(matches!(record::read(dir.path()), RecordRead::Read(_)));
}
