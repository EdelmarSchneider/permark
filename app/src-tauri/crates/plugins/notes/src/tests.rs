//! Tests of the notes service and the plugin: spec behavior over a temp notes folder and a temp profile, and the
//! failures that matter (hostile paths, damaged side data, races, other programs writing to the folder).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use permark_core::edit::{Editor, LineEdit, SaveOutcome};
use permark_core::error::ErrorKind;
use permark_core::format::FormatRegistry;
use permark_core::notes::{loose_key, NotesScope};
use permark_core::paths::Root;
use permark_core::state::{AppDirs, StateStore};

use crate::folder::{FolderStatus, Places};
use crate::journal::Entry as JournalEntry;
use crate::names::Moment;
use crate::state::{FolderState, LooseEntry, Stamp};
use crate::types::{EntryKind, LooseState};
use crate::NotesService;

const MOMENT: Moment = Moment {
    year: 2026,
    month: 10,
    day: 19,
    hour: 9,
    minute: 7,
};

struct Fx {
    tmp: tempfile::TempDir,
    notes: PathBuf,
    profile: PathBuf,
    service: Arc<NotesService>,
    scope: Arc<NotesScope>,
    store: StateStore,
    chosen: Arc<Mutex<String>>,
}

fn places(tmp: &Path) -> Places {
    Places {
        documents: Some(tmp.join("Documents")),
        home: Some(tmp.to_path_buf()),
        profile: tmp.join("profile"),
        debug: false,
    }
}

fn fx() -> Fx {
    fx_with(|_| {})
}

/// A service over a notes folder that `setup` filled before the service opened it.
fn fx_with(setup: impl FnOnce(&Path)) -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let notes = tmp.path().join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    setup(&notes);
    let dirs = AppDirs::under(tmp.path());
    let store = StateStore::app(&dirs);
    let scope = NotesScope::new();
    let chosen = Arc::new(Mutex::new(notes.display().to_string()));
    let read = Arc::clone(&chosen);
    let service = NotesService::open(
        places(tmp.path()),
        store.clone(),
        Arc::clone(&scope),
        Box::new(move || read.lock().unwrap().clone()),
    );
    Fx {
        profile: dirs.profile.clone(),
        notes,
        tmp,
        service,
        scope,
        store,
        chosen,
    }
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

/// Writes a note with a modification time `age_s` seconds ago.
fn note(f: &Fx, name: &str, text: &str, age_s: u64) {
    let path = f.notes.join(name);
    write(&path, text);
    let at = std::time::SystemTime::now() - std::time::Duration::from_secs(age_s);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

fn names(f: &Fx) -> Vec<String> {
    f.service
        .list(1)
        .entries
        .iter()
        .map(|e| e.path.clone())
        .collect()
}

fn loose_file(dir: &Path, name: &str, text: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    write(&path, text);
    path
}

// ---- creating and listing ---------------------------------------------------------------------------------------------

#[test]
fn new_notes_are_files_named_after_their_moment_with_numbers_when_taken() {
    let f = fx();
    assert_eq!(f.service.create(MOMENT).unwrap(), "2026-10-19 0907.md");
    assert_eq!(f.service.create(MOMENT).unwrap(), "2026-10-19 0907-2.md");
    assert_eq!(f.service.create(MOMENT).unwrap(), "2026-10-19 0907-3.md");
    assert_eq!(
        std::fs::read(f.notes.join("2026-10-19 0907.md")).unwrap(),
        b""
    );
    let list = f.service.list(1);
    assert_eq!(list.entries.len(), 3);
    assert!(list.entries.iter().all(|e| e.title.is_empty()));
}

#[test]
fn the_list_shows_notes_newest_first_and_only_notes() {
    let f = fx();
    note(&f, "old.md", "# Old\nbody", 5000);
    note(&f, "new.md", "# New\nfresh words", 10);
    note(&f, "mid.markdown", "Middle", 500);
    write(&f.notes.join(".hidden.md"), "no");
    write(&f.notes.join("readme.txt"), "no");
    std::fs::create_dir_all(f.notes.join("assets")).unwrap();
    write(&f.notes.join("assets").join("deep.md"), "no");
    std::fs::create_dir_all(f.notes.join(".permark")).unwrap();
    write(&f.notes.join(".permark").join("x.md"), "no");
    let list = f.service.list(1);
    let paths: Vec<&str> = list.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["new.md", "mid.markdown", "old.md"]);
    assert_eq!(list.entries[0].title, "New");
    assert_eq!(list.entries[0].snippet, "fresh words");
    assert_eq!(list.folder.status, FolderStatus::Ready);
    assert!(!list.folder.is_default);
}

#[test]
fn titles_follow_the_file_and_are_not_read_again_while_it_looks_the_same() {
    let f = fx();
    note(&f, "a.md", "# First", 100);
    assert_eq!(f.service.list(1).entries[0].title, "First");
    note(&f, "a.md", "# Second title", 50);
    assert_eq!(f.service.list(1).entries[0].title, "Second title");
}

#[test]
fn pinned_notes_come_first_in_the_persons_order() {
    let f = fx();
    note(&f, "a.md", "A", 300);
    note(&f, "b.md", "B", 200);
    note(&f, "c.md", "C", 100);
    f.service.set_pinned("a.md", true).unwrap();
    f.service.set_pinned("c.md", true).unwrap();
    assert_eq!(names(&f), ["a.md", "c.md", "b.md"]);
    f.service
        .set_pin_order(&["c.md".into(), "a.md".into(), "b.md".into()]);
    assert_eq!(names(&f), ["c.md", "a.md", "b.md"]);
    let list = f.service.list(1);
    assert!(list.entries[0].pinned && list.entries[1].pinned && !list.entries[2].pinned);
    f.service.set_pinned("c.md", false).unwrap();
    assert_eq!(names(&f), ["a.md", "c.md", "b.md"]);
    // a pin of a note that is not there is refused; one whose note went away is forgotten
    assert!(f.service.set_pinned("zz.md", true).is_err());
    std::fs::remove_file(f.notes.join("a.md")).unwrap();
    assert_eq!(names(&f), ["c.md", "b.md"]);
    let state = f.service.ctx().read(Clone::clone);
    assert!(state.pins.is_empty());
}

#[test]
fn the_pin_order_ignores_notes_that_are_not_pinned_and_keeps_those_it_leaves_out() {
    let f = fx();
    for n in ["a.md", "b.md", "c.md"] {
        note(&f, n, n, 10);
        f.service.set_pinned(n, true).unwrap();
    }
    f.service
        .set_pin_order(&["zz.md".into(), "c.md".into(), "a.md".into()]);
    let state = f.service.ctx().read(Clone::clone);
    assert_eq!(state.pins, ["c.md", "a.md", "b.md"]);
}

#[test]
fn a_folder_that_is_missing_is_reported_and_never_made_by_itself() {
    let f = fx();
    *f.chosen.lock().unwrap() = f.tmp.path().join("gone").display().to_string();
    f.service.settings_changed();
    let list = f.service.list(1);
    assert_eq!(list.folder.status, FolderStatus::Missing);
    assert!(list.entries.is_empty());
    assert_eq!(
        f.service.create(MOMENT).unwrap_err().kind,
        ErrorKind::NotFound
    );
    assert!(!f.tmp.path().join("gone").exists());
    let info = f.service.create_folder().unwrap();
    assert_eq!(info.status, FolderStatus::Ready);
    assert!(f.service.create(MOMENT).is_ok());
}

#[test]
fn the_default_folder_is_made_quietly_when_the_first_note_is_made() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = AppDirs::under(tmp.path());
    let service = NotesService::open(
        places(tmp.path()),
        StateStore::app(&dirs),
        NotesScope::new(),
        Box::new(String::new),
    );
    let default = tmp.path().join("Documents").join("PerMark Notes");
    assert!(!default.exists(), "asking creates nothing");
    let list = service.list(1);
    assert!(list.folder.is_default && list.entries.is_empty());
    assert!(!default.exists());
    service.create(MOMENT).unwrap();
    assert!(default.join("2026-10-19 0907.md").is_file());
}

#[test]
fn a_changed_setting_moves_the_notes_to_the_other_folder_and_the_scope_follows() {
    let f = fx();
    note(&f, "here.md", "here", 10);
    assert_eq!(names(&f), ["here.md"]);
    assert!(f.scope.resolve("here.md").is_ok());
    let other = f.tmp.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    write(&other.join("there.md"), "there");
    *f.chosen.lock().unwrap() = other.display().to_string();
    f.service.settings_changed();
    assert_eq!(names(&f), ["there.md"]);
    assert!(f.scope.file_exists("there.md"));
    assert!(!f.scope.file_exists("here.md"));
}

// ---- deleting ------------------------------------------------------------------------------------------------------------

#[test]
fn undo_puts_notes_back_with_their_text_time_and_marks() {
    let f = fx();
    note(&f, "keep.md", "# Keep", 900);
    note(&f, "gone.md", "# Gone\nline\r\nCRLF", 800);
    f.service.set_pinned("gone.md", true).unwrap();
    f.service.set_shared("gone.md", true).unwrap();
    let before = std::fs::metadata(f.notes.join("gone.md"))
        .unwrap()
        .modified()
        .unwrap();
    let deleted = f.service.delete(&["gone.md".into()]).unwrap();
    assert_eq!(deleted.names, ["gone.md"]);
    assert!(!f.notes.join("gone.md").exists());
    assert_eq!(names(&f), ["keep.md"]);
    let state = f.service.ctx().read(Clone::clone);
    assert!(state.pins.is_empty() && state.shared.is_empty());
    let back = f.service.undo_delete(deleted.undo_id.unwrap()).unwrap();
    assert_eq!(back, ["gone.md"]);
    assert_eq!(
        std::fs::read_to_string(f.notes.join("gone.md")).unwrap(),
        "# Gone\nline\r\nCRLF"
    );
    let after = std::fs::metadata(f.notes.join("gone.md"))
        .unwrap()
        .modified()
        .unwrap();
    let drift = after
        .duration_since(before)
        .or_else(|_| before.duration_since(after))
        .unwrap();
    assert!(drift.as_millis() < 5, "{drift:?}");
    let state = f.service.ctx().read(Clone::clone);
    assert_eq!(state.pins, ["gone.md"]);
    assert_eq!(state.shared, ["gone.md"]);
    // the undo is used up
    assert!(f.service.undo_delete(deleted.undo_id.unwrap()).is_err());
}

#[test]
fn a_name_taken_meanwhile_comes_back_as_restored() {
    let f = fx();
    note(&f, "plan.md", "# Plan v1", 100);
    let deleted = f.service.delete(&["plan.md".into()]).unwrap();
    note(&f, "plan.md", "# Another plan", 5);
    let back = f.service.undo_delete(deleted.undo_id.unwrap()).unwrap();
    assert_eq!(back, ["Plan v1 (restored).md"]);
    assert_eq!(
        std::fs::read_to_string(f.notes.join("plan.md")).unwrap(),
        "# Another plan"
    );
    assert_eq!(
        std::fs::read_to_string(f.notes.join("Plan v1 (restored).md")).unwrap(),
        "# Plan v1"
    );
}

#[test]
fn a_batch_deletes_several_and_one_bad_path_deletes_nothing() {
    let f = fx();
    note(&f, "a.md", "a", 10);
    note(&f, "b.md", "b", 10);
    assert!(f
        .service
        .delete(&["a.md".into(), "missing.md".into()])
        .is_err());
    assert!(f.notes.join("a.md").exists());
    let deleted = f.service.delete(&["a.md".into(), "b.md".into()]).unwrap();
    assert_eq!(deleted.names.len(), 2);
    assert!(names(&f).is_empty());
    assert_eq!(
        f.service
            .undo_delete(deleted.undo_id.unwrap())
            .unwrap()
            .len(),
        2
    );
    assert_eq!(names(&f).len(), 2);
}

#[test]
fn only_notes_of_the_folder_can_be_deleted_whatever_the_path_says() {
    let f = fx();
    let outside = f.tmp.path().join("outside.md");
    write(&outside, "keep me");
    write(&f.notes.join("a.md"), "a");
    for bad in [
        "../outside.md",
        "..\\outside.md",
        "a/../../outside.md",
        "sub/a.md",
        "",
        "..",
        "a.txt",
        "a.md\0",
        "@file/zz/a.md",
        "@file/0a1b2c3d/outside.md",
    ] {
        assert!(f.service.delete(&[bad.to_owned()]).is_err(), "{bad:?}");
    }
    assert!(outside.exists() && f.notes.join("a.md").exists());
}

// ---- renaming --------------------------------------------------------------------------------------------------------------

#[test]
fn a_rename_keeps_everything_the_notes_know_about_the_note() {
    let f = fx();
    note(&f, "a.md", "# A", 100);
    f.service.set_pinned("a.md", true).unwrap();
    f.service.set_shared("a.md", true).unwrap();
    f.service.mark_copied("a.md").unwrap();
    f.service.buffer("a.md", "0123", Some("typed"), 5).unwrap();
    f.service.before_replace("a.md", b"# A", 100);
    let new = f.service.rename("a.md", "plans for may").unwrap();
    assert_eq!(new, "plans for may.md");
    assert!(f.notes.join("plans for may.md").is_file() && !f.notes.join("a.md").exists());
    let state = f.service.ctx().read(Clone::clone);
    assert_eq!(state.pins, ["plans for may.md"]);
    assert_eq!(state.shared, ["plans for may.md"]);
    assert!(state.copied.contains_key("plans for may.md"));
    assert_eq!(f.service.versions("plans for may.md").unwrap().len(), 1);
    assert!(f.service.versions("a.md").unwrap().is_empty());
    assert_eq!(
        f.service.recovered_text("plans for may.md").as_deref(),
        Some("typed")
    );
    assert!(f.service.recovered_text("a.md").is_none());
}

#[test]
fn a_rename_refuses_names_that_are_taken_or_not_names() {
    let f = fx();
    note(&f, "a.md", "a", 10);
    note(&f, "b.md", "b", 10);
    assert_eq!(
        f.service.rename("a.md", "b.md").unwrap_err().kind,
        ErrorKind::Exists
    );
    assert_eq!(
        f.service.rename("a.md", "b").unwrap_err().kind,
        ErrorKind::Exists
    );
    for (typed, why) in [
        ("", "empty"),
        ("a/b", "chars"),
        ("a:b", "chars"),
        (".hidden", "dot"),
        ("con", "reserved"),
        ("x.txt", "notMarkdown"),
    ] {
        let error = f.service.rename("a.md", typed).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidName, "{typed}");
        assert_eq!(error.detail.as_deref(), Some(why), "{typed}");
    }
    assert!(f.notes.join("a.md").exists());
    // the same name, and the same name in another case, are renames, not clashes
    assert_eq!(f.service.rename("a.md", "a.md").unwrap(), "a.md");
    assert_eq!(f.service.rename("a.md", "A.md").unwrap(), "A.md");
    assert_eq!(
        f.service.rename("zz.md", "q").unwrap_err().kind,
        ErrorKind::NotFound
    );
}

// ---- share, copy ------------------------------------------------------------------------------------------------------------

#[test]
fn an_agent_sees_only_the_notes_the_person_shared_one_by_one() {
    let f = fx();
    note(&f, "public.md", "# Shared one\nbody", 10);
    note(&f, "private.md", "# Secret", 10);
    f.service.set_shared("public.md", true).unwrap();
    let shared = f.service.shared_notes();
    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0].id, "public.md");
    assert_eq!(shared[0].title, "Shared one");
    assert_eq!(
        f.service.read_shared("public.md").unwrap(),
        "# Shared one\nbody"
    );
    for id in ["private.md", "../outside.md", "", "@file/0a1b2c3d/x.md"] {
        assert!(f.service.read_shared(id).is_err(), "{id}");
    }
    assert!(f
        .service
        .list(1)
        .entries
        .iter()
        .any(|e| e.shared && e.path == "public.md"));
    // sharing stops at once
    f.service.set_shared("public.md", false).unwrap();
    assert!(f.service.shared_notes().is_empty());
    assert!(f.service.read_shared("public.md").is_err());
}

#[test]
fn a_loose_file_can_never_be_shared() {
    let f = fx();
    let file = loose_file(&f.tmp.path().join("elsewhere"), "x.md", "x");
    let path = f
        .service
        .open_file(&file.display().to_string(), 10)
        .unwrap();
    assert_eq!(
        f.service.set_shared(&path, true).unwrap_err().kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn a_copied_mark_lasts_until_the_note_is_edited_and_the_bulk_delete_spares_pinned_notes() {
    let f = fx();
    note(&f, "a.md", "aaa", 100);
    note(&f, "b.md", "bbb", 100);
    note(&f, "c.md", "ccc", 100);
    for n in ["a.md", "b.md", "c.md"] {
        f.service.mark_copied(n).unwrap();
    }
    f.service.set_pinned("b.md", true).unwrap();
    assert!(f.service.list(1).entries.iter().all(|e| e.copied));
    // c.md is edited after it was copied
    note(&f, "c.md", "ccc edited", 1);
    let list = f.service.list(1);
    assert!(list
        .entries
        .iter()
        .find(|e| e.path == "c.md")
        .is_some_and(|e| !e.copied));
    let deleted = f.service.delete_copied().unwrap();
    assert_eq!(deleted.names, ["a.md"]);
    assert_eq!(names(&f).len(), 2);
    assert!(f.service.delete_copied().unwrap().names.is_empty());
}

// ---- versions --------------------------------------------------------------------------------------------------------------

#[test]
fn a_version_is_taken_at_the_first_save_after_opening_then_every_ten_minutes() {
    let f = fx();
    note(&f, "a.md", "one", 10);
    f.service.opened(Some("a.md"), 1);
    f.service.before_replace("a.md", b"one", 1_000);
    f.service.before_replace("a.md", b"two", 1_000 + 60_000);
    assert_eq!(f.service.versions("a.md").unwrap().len(), 1);
    f.service
        .before_replace("a.md", b"three", 1_000 + 11 * 60_000);
    assert_eq!(f.service.versions("a.md").unwrap().len(), 2);
    // opening it again makes the next save take one
    f.service.opened(Some("a.md"), 2);
    f.service
        .before_replace("a.md", b"four", 1_000 + 11 * 60_000 + 5);
    assert_eq!(f.service.versions("a.md").unwrap().len(), 3);
    // loose files and hostile paths take none and do no harm
    f.service.before_replace("@file/0a1b2c3d/x.md", b"x", 5);
    f.service.before_replace("../a.md", b"x", 5);
}

#[test]
fn restoring_keeps_the_current_text_first_and_writes_the_old_one() {
    let f = fx();
    note(&f, "a.md", "# Old text", 10);
    f.service.before_replace("a.md", b"# Old text", 5_000);
    note(&f, "a.md", "# New text", 1);
    let version = f.service.versions("a.md").unwrap().remove(0);
    assert_eq!(
        f.service.version_text("a.md", &version.id).unwrap(),
        "# Old text"
    );
    f.service
        .restore_version("a.md", &version.id, 9_000)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(f.notes.join("a.md")).unwrap(),
        "# Old text"
    );
    let texts: Vec<String> = f
        .service
        .versions("a.md")
        .unwrap()
        .iter()
        .map(|v| f.service.version_text("a.md", &v.id).unwrap())
        .collect();
    assert_eq!(
        texts,
        ["# New text", "# Old text"],
        "the restore can be undone"
    );
    assert!(f.service.restore_version("a.md", "../x", 1).is_err());
    assert!(f.service.restore_version("a.md", "424242", 1).is_err());
    assert_eq!(
        f.service.versions("@file/0a1b2c3d/x.md").unwrap_err().kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn the_editors_save_takes_the_version_through_the_observer_and_only_when_bytes_change() {
    let f = fx();
    note(&f, "a.md", "# Title\n\nbody\n", 50);
    let lease = f
        .scope
        .install_observer(Arc::new(crate::Observer(Arc::clone(&f.service))));
    let scope = permark_core::document::DocScope::Notes(Arc::clone(&f.scope));
    let formats = FormatRegistry::core();
    let observer = f.scope.observer();
    let editor = Editor {
        scope: &scope,
        formats: &formats,
        own: None,
        observer: observer.as_deref(),
    };
    let source = editor.source("a.md").unwrap();
    // nothing changes: nothing is written, nothing is kept
    let same = editor.save("a.md", &source.hash, &[]).unwrap();
    assert!(matches!(same, SaveOutcome::Saved { .. }));
    assert!(f.service.versions("a.md").unwrap().is_empty());
    // a real edit: the old bytes become a version
    let edit = LineEdit {
        at: 2,
        remove: 1,
        insert: vec!["new body".into()],
    };
    let saved = editor.save("a.md", &source.hash, &[edit]).unwrap();
    assert!(matches!(saved, SaveOutcome::Saved { .. }));
    assert_eq!(
        std::fs::read_to_string(f.notes.join("a.md")).unwrap(),
        "# Title\n\nnew body\n"
    );
    let versions = f.service.versions("a.md").unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(
        f.service.version_text("a.md", &versions[0].id).unwrap(),
        "# Title\n\nbody\n"
    );
    drop(lease);
    assert!(f.scope.observer().is_none());
}

// ---- the journal ---------------------------------------------------------------------------------------------------------------

#[test]
fn text_typed_and_never_saved_marks_the_note_recovered_and_is_kept_as_a_version() {
    let f = fx();
    note(&f, "a.md", "line one\r\nline two\r\n", 100);
    // the journal holds the file's own text: nothing to recover (line endings do not count)
    f.service
        .buffer("a.md", "h", Some("line one\nline two\n"), 5)
        .unwrap();
    assert!(!f.service.list(1).entries[0].recovered);
    // typed beyond what was saved
    f.service
        .buffer("a.md", "h", Some("line one\nline two\nline three\n"), 6)
        .unwrap();
    let entry = f.service.list(1).entries.remove(0);
    assert!(entry.recovered);
    // the text is safe whatever the person decides
    let versions = f.service.versions("a.md").unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(
        f.service.version_text("a.md", &versions[0].id).unwrap(),
        "line one\nline two\nline three\n"
    );
    // asking again does not add versions
    let _ = f.service.list(2);
    assert_eq!(f.service.versions("a.md").unwrap().len(), 1);
    assert_eq!(
        f.service.recovered_text("a.md").as_deref(),
        Some("line one\nline two\nline three\n")
    );
}

#[test]
fn keeping_the_recovered_text_writes_it_in_the_files_own_form_and_keeps_the_old_text() {
    let f = fx();
    note(&f, "a.md", "one\r\ntwo\r\n", 100);
    f.service
        .buffer("a.md", "h", Some("one\ntwo\nthree\n"), 5)
        .unwrap();
    f.service.keep_recovered_text("a.md", 50).unwrap();
    assert_eq!(
        std::fs::read(f.notes.join("a.md")).unwrap(),
        b"one\r\ntwo\r\nthree\r\n"
    );
    assert!(f.service.recovered_text("a.md").is_none());
    assert!(!f.service.list(1).entries[0].recovered);
    let old: Vec<String> = f
        .service
        .versions("a.md")
        .unwrap()
        .iter()
        .map(|v| f.service.version_text("a.md", &v.id).unwrap())
        .collect();
    assert!(old.contains(&"one\r\ntwo\r\n".to_owned()), "{old:?}");
    assert!(
        f.service.keep_recovered_text("a.md", 60).is_err(),
        "nothing left to keep"
    );
}

#[test]
fn discarding_the_recovered_text_removes_the_entry_and_the_mark() {
    let f = fx();
    note(&f, "a.md", "saved", 100);
    f.service.buffer("a.md", "h", Some("typed"), 5).unwrap();
    assert!(f.service.list(1).entries[0].recovered);
    f.service.discard_recovered("a.md");
    assert!(!f.service.list(2).entries[0].recovered);
    assert!(f.service.recovered_text("a.md").is_none());
}

#[test]
fn a_clean_editor_clears_its_entry() {
    let f = fx();
    note(&f, "a.md", "saved", 100);
    f.service.buffer("a.md", "h", Some("typed"), 5).unwrap();
    f.service.buffer("a.md", "h", None, 6).unwrap();
    assert!(f.service.recovered_text("a.md").is_none());
    assert!(f.service.buffer("../x.md", "h", Some("t"), 1).is_err());
}

#[test]
fn text_journaled_for_a_note_that_is_gone_becomes_a_note_of_its_own() {
    let f = fx();
    f.service
        .buffer("lost.md", "h", Some("# Lost idea\nprecious"), 5)
        .unwrap();
    let list = f.service.list(10);
    assert_eq!(list.entries.len(), 1);
    assert_eq!(list.entries[0].path, "Lost idea (recovered).md");
    assert_eq!(
        std::fs::read_to_string(f.notes.join("Lost idea (recovered).md")).unwrap(),
        "# Lost idea\nprecious"
    );
    assert!(f.service.recovered_text("lost.md").is_none());
    // blank text is not worth a note
    f.service.buffer("blank.md", "h", Some("  \n"), 5).unwrap();
    assert_eq!(f.service.list(11).entries.len(), 1);
}

#[test]
fn damaged_journal_files_are_ignored() {
    let f = fx();
    note(&f, "a.md", "a", 10);
    f.service.buffer("a.md", "h", Some("typed"), 5).unwrap();
    let dir = f.profile.join("plugins").join("notes").join("journal");
    let folder = std::fs::read_dir(&dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    write(&folder.join("junk.json"), "{ not json");
    write(&folder.join("empty.json"), "");
    assert_eq!(f.service.list(1).entries.len(), 1);
}

// ---- loose files ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_loose_file_is_listed_under_files_with_its_state() {
    let f = fx();
    note(&f, "a.md", "note", 10);
    let dir = f.tmp.path().join("downloads");
    let file = loose_file(&dir, "sent to me.md", "# Sent\nhello");
    let path = f
        .service
        .open_file(&file.display().to_string(), 1_000)
        .unwrap();
    assert!(
        path.starts_with("@file/") && path.ends_with("/sent to me.md"),
        "{path}"
    );
    // opening it again is the same entry
    assert_eq!(
        f.service
            .open_file(&file.display().to_string(), 2_000)
            .unwrap(),
        path
    );
    let list = f.service.list(1);
    assert_eq!(list.entries.len(), 2);
    let row = &list.entries[1];
    assert_eq!(row.kind, EntryKind::File);
    assert_eq!(row.state, Some(LooseState::Clean));
    assert_eq!(row.title, "sent to me.md");
    assert!(row.folder.as_deref().unwrap().ends_with("downloads"));
    // its document resolves through the scope
    assert!(f.scope.resolve(&path).is_ok());
}

#[test]
fn a_loose_file_changed_on_disk_or_with_unsaved_text_or_gone_says_so() {
    let f = fx();
    let file = loose_file(&f.tmp.path().join("d"), "x.md", "one");
    let path = f.service.open_file(&file.display().to_string(), 1).unwrap();
    let state = |f: &Fx| f.service.list(1).entries[0].state.unwrap();
    assert_eq!(state(&f), LooseState::Clean);
    // another program wrote it
    write(&file, "one two");
    assert_eq!(state(&f), LooseState::ChangedOnDisk);
    // opening it again takes the new look as the one to measure against
    f.service.opened(Some(&path), 5);
    assert_eq!(state(&f), LooseState::Clean);
    // text typed and not saved
    f.service
        .buffer(&path, "h", Some("one two three"), 6)
        .unwrap();
    assert_eq!(state(&f), LooseState::Unsaved);
    assert!(f.service.list(1).entries[0].pending);
    // the disk text equal to the journal is not unsaved
    f.service.buffer(&path, "h", Some("one two"), 7).unwrap();
    assert_eq!(state(&f), LooseState::Clean);
    f.service
        .buffer(&path, "h", Some("kept after delete"), 8)
        .unwrap();
    std::fs::remove_file(&file).unwrap();
    assert_eq!(state(&f), LooseState::Gone);
    assert!(f.service.list(1).entries[0].pending);
}

#[test]
fn a_loose_file_renamed_in_its_folder_is_found_again() {
    let f = fx();
    let dir = f.tmp.path().join("d");
    let file = loose_file(&dir, "old name.md", "some text here");
    let path = f.service.open_file(&file.display().to_string(), 1).unwrap();
    std::fs::rename(&file, dir.join("new name.md")).unwrap();
    let row = f.service.list(1).entries.remove(0);
    assert_eq!(row.state, Some(LooseState::Clean));
    assert!(row.path.ends_with("/new name.md"), "{}", row.path);
    // the id stayed: what is kept under it follows
    assert_eq!(
        permark_core::notes::loose_id_of(&row.path),
        permark_core::notes::loose_id_of(&path)
    );
    // two files with the same look are not guessed between
    let a = loose_file(&dir, "twin-a.md", "same");
    let _b = loose_file(&dir, "twin-b.md", "same");
    let at = std::time::SystemTime::now();
    for p in [&a, &dir.join("twin-b.md")] {
        std::fs::OpenOptions::new()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }
    let twin = f.service.open_file(&a.display().to_string(), 2).unwrap();
    std::fs::rename(&a, dir.join("twin-c.md")).unwrap();
    let rows = f.service.list(1).entries;
    let row = rows.iter().find(|r| r.path == twin).unwrap();
    assert_eq!(row.state, Some(LooseState::Gone));
}

#[test]
fn renaming_a_loose_file_works_and_keeps_its_id_and_pin() {
    let f = fx();
    let dir = f.tmp.path().join("d");
    let file = loose_file(&dir, "draft.md", "text");
    let path = f.service.open_file(&file.display().to_string(), 1).unwrap();
    f.service.set_pinned(&path, true).unwrap();
    f.service.buffer(&path, "h", Some("typed more"), 2).unwrap();
    let new = f.service.rename(&path, "final").unwrap();
    assert!(new.ends_with("/final.md"), "{new}");
    assert!(dir.join("final.md").is_file() && !file.exists());
    let rows = f.service.list(1).entries;
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].pinned && rows[0].pending,
        "pin and typed text follow"
    );
    assert!(f.scope.resolve(&new).is_ok());
    // a typed extension is respected; none keeps the file's own
    let txt = loose_file(&dir, "notes.txt", "t");
    let txt_path = f.service.open_file(&txt.display().to_string(), 3).unwrap();
    let renamed = f.service.rename(&txt_path, "todo").unwrap();
    assert!(renamed.ends_with("/todo.txt"), "{renamed}");
    // clashes and bad names
    assert_eq!(
        f.service.rename(&new, "todo.txt").unwrap_err().kind,
        ErrorKind::Exists
    );
    assert_eq!(
        f.service.rename(&new, "a/b").unwrap_err().kind,
        ErrorKind::InvalidName
    );
    // the file is gone: nothing to rename
    std::fs::remove_file(dir.join("final.md")).unwrap();
    assert_eq!(
        f.service.rename(&new, "other").unwrap_err().kind,
        ErrorKind::NotFound
    );
}

#[test]
fn loose_file_actions_reload_keep_save_as_note_locate_and_remove() {
    let f = fx();
    let dir = f.tmp.path().join("d");
    let file = loose_file(&dir, "x.md", "disk text\r\nline two\r\n");
    let path = f.service.open_file(&file.display().to_string(), 1).unwrap();
    // Keep mine: the typed text goes over the file in the file's own form
    f.service
        .buffer(&path, "h", Some("my text\nline two\n"), 2)
        .unwrap();
    f.service.keep_mine(&path).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"my text\r\nline two\r\n");
    assert_eq!(f.service.list(1).entries[0].state, Some(LooseState::Clean));
    assert!(
        f.service.keep_mine(&path).is_err(),
        "nothing typed is kept now"
    );
    // Reload: the typed text is let go
    f.service.buffer(&path, "h", Some("discard me"), 3).unwrap();
    f.service.reload_file(&path).unwrap();
    assert_eq!(f.service.list(1).entries[0].state, Some(LooseState::Clean));
    // Save as note: the typed text becomes a note
    f.service
        .buffer(&path, "h", Some("# Typed idea\nbody"), 4)
        .unwrap();
    let made = f.service.save_as_note(&path).unwrap();
    assert_eq!(made, "Typed idea (my edits).md");
    assert_eq!(
        std::fs::read_to_string(f.notes.join(&made)).unwrap(),
        "# Typed idea\nbody"
    );
    assert!(f.service.save_as_note(&path).is_err());
    // Locate
    let moved = loose_file(&f.tmp.path().join("elsewhere"), "found.md", "found");
    let located = f
        .service
        .locate_file(&path, &moved.display().to_string(), 9)
        .unwrap();
    assert!(located.ends_with("/found.md"), "{located}");
    assert!(f.service.list(1).entries.iter().any(|e| e.path == located));
    // Remove from the list never touches the file
    f.service.remove_file(&located).unwrap();
    assert!(moved.exists());
    assert!(f
        .service
        .list(1)
        .entries
        .iter()
        .all(|e| e.kind == EntryKind::Note));
    assert!(f.scope.resolve(&located).is_err());
    assert!(f.service.remove_file(&located).is_err());
}

#[test]
fn keep_mine_makes_a_file_that_is_gone_again() {
    let f = fx();
    let dir = f.tmp.path().join("d");
    let file = loose_file(&dir, "x.md", "text");
    let path = f.service.open_file(&file.display().to_string(), 1).unwrap();
    f.service.buffer(&path, "h", Some("typed"), 2).unwrap();
    std::fs::remove_file(&file).unwrap();
    f.service.keep_mine(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "typed");
}

#[test]
fn a_file_of_the_notes_folder_is_opened_as_a_note_and_bad_files_are_refused() {
    let f = fx();
    note(&f, "inside.md", "x", 10);
    let inside = dunce::canonicalize(f.notes.join("inside.md")).unwrap();
    assert_eq!(
        f.service
            .open_file(&inside.display().to_string(), 1)
            .unwrap(),
        "inside.md"
    );
    assert!(f
        .service
        .list(1)
        .entries
        .iter()
        .all(|e| e.kind == EntryKind::Note));
    assert!(f
        .service
        .open_file(&f.tmp.path().join("nope.md").display().to_string(), 1)
        .is_err());
    assert!(f
        .service
        .open_file(&f.tmp.path().display().to_string(), 1)
        .is_err());
}

#[test]
fn old_loose_files_beyond_fifty_drop_but_pinned_and_unsaved_ones_stay() {
    let f = fx();
    let dir = f.tmp.path().join("many");
    let mut paths = Vec::new();
    for n in 0..60_u64 {
        let file = loose_file(&dir, &format!("f{n:02}.md"), &format!("text {n}"));
        paths.push(
            f.service
                .open_file(&file.display().to_string(), 1_000 + n)
                .unwrap(),
        );
    }
    let loose = f.service.list(1).entries.len();
    assert!(loose <= 60);
    // the oldest are gone, the newest stay
    assert!(f
        .service
        .list(1)
        .entries
        .iter()
        .any(|e| e.path == paths[59]));
    assert!(!f.service.list(1).entries.iter().any(|e| e.path == paths[0]));
    assert_eq!(f.service.list(1).entries.len(), 50);
    // a pinned one survives the next prune even when it is the oldest
    f.service.set_pinned(&paths[10], true).unwrap();
    let more = loose_file(&dir, "extra.md", "extra");
    f.service
        .open_file(&more.display().to_string(), 5_000)
        .unwrap();
    assert!(f
        .service
        .list(1)
        .entries
        .iter()
        .any(|e| e.path == paths[10]));
}

#[test]
fn new_ids_are_eight_hex_digits_and_never_a_taken_one() {
    let taken = ["00000000".to_owned()];
    for n in 0..200_u64 {
        let id = crate::new_id("C:\\a\\b.md", n, |c| taken.iter().any(|t| t == c));
        assert!(permark_core::notes::is_loose_id(&id), "{id}");
        assert_ne!(id, "00000000");
    }
    assert_eq!(
        crate::new_id("x", 1, |_| false),
        crate::new_id("x", 1, |_| false),
        "the same seed gives the same id"
    );
}

// ---- moving to a project ----------------------------------------------------------------------------------------------------

#[test]
fn a_name_is_suggested_from_the_title_and_numbered_when_taken() {
    let f = fx();
    note(&f, "2026-10-19 0907.md", "# Plan: Q4?\nbody", 10);
    let project = f.tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    assert_eq!(
        f.service
            .suggest_name("2026-10-19 0907.md", Some(&project.display().to_string()))
            .unwrap(),
        "Plan Q4.md"
    );
    write(&project.join("Plan Q4.md"), "x");
    assert_eq!(
        f.service
            .suggest_name("2026-10-19 0907.md", Some(&project.display().to_string()))
            .unwrap(),
        "Plan Q4 2.md"
    );
    note(&f, "blank.md", "", 5);
    assert_eq!(
        f.service.suggest_name("blank.md", None).unwrap(),
        "blank.md"
    );
}

#[test]
fn moving_copies_the_exact_bytes_removes_the_note_and_can_be_undone() {
    let f = fx();
    let project = f.tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    note(&f, "a.md", "# A\r\nline\r\n", 10);
    f.service.set_pinned("a.md", true).unwrap();
    let moved = f
        .service
        .move_to_project("a.md", &project.display().to_string(), "plan")
        .unwrap();
    assert!(!moved.kept);
    assert_eq!(
        std::fs::read(project.join("plan.md")).unwrap(),
        b"# A\r\nline\r\n"
    );
    assert!(!f.notes.join("a.md").exists());
    let undone = f.service.undo_move(moved.undo_id.unwrap()).unwrap();
    assert_eq!(undone.note, "a.md");
    assert!(undone.removed);
    assert!(!project.join("plan.md").exists());
    assert_eq!(
        std::fs::read(f.notes.join("a.md")).unwrap(),
        b"# A\r\nline\r\n"
    );
    assert!(f.service.undo_move(moved.undo_id.unwrap()).is_err());
}

#[test]
fn a_move_never_overwrites_and_never_goes_into_the_notes_folder() {
    let f = fx();
    let project = f.tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    write(&project.join("plan.md"), "theirs");
    note(&f, "a.md", "mine", 10);
    let folder = project.display().to_string();
    assert_eq!(
        f.service
            .move_to_project("a.md", &folder, "plan.md")
            .unwrap_err()
            .kind,
        ErrorKind::Exists
    );
    assert_eq!(
        std::fs::read_to_string(project.join("plan.md")).unwrap(),
        "theirs"
    );
    assert!(f.notes.join("a.md").exists());
    let inside = f.notes.display().to_string();
    assert_eq!(
        f.service
            .move_to_project("a.md", &inside, "b.md")
            .unwrap_err()
            .kind,
        ErrorKind::IntoItself
    );
    for (dest, name, kind) in [
        ("relative", "b.md", ErrorKind::InvalidPath),
        (folder.as_str(), "a/b.md", ErrorKind::InvalidName),
        (folder.as_str(), "b.txt", ErrorKind::InvalidName),
        (folder.as_str(), "", ErrorKind::InvalidName),
    ] {
        assert_eq!(
            f.service
                .move_to_project("a.md", dest, name)
                .unwrap_err()
                .kind,
            kind,
            "{dest} {name}"
        );
    }
    let missing = f.tmp.path().join("nope").display().to_string();
    assert!(f.service.move_to_project("a.md", &missing, "b.md").is_err());
    assert!(
        f.notes.join("a.md").exists(),
        "the note stays whenever the move fails"
    );
}

#[test]
fn a_copy_changed_after_the_move_stays_when_the_move_is_undone() {
    let f = fx();
    let project = f.tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    note(&f, "a.md", "original", 10);
    let moved = f
        .service
        .move_to_project("a.md", &project.display().to_string(), "x.md")
        .unwrap();
    write(&project.join("x.md"), "edited in the project");
    let undone = f.service.undo_move(moved.undo_id.unwrap()).unwrap();
    assert!(!undone.removed);
    assert_eq!(
        std::fs::read_to_string(project.join("x.md")).unwrap(),
        "edited in the project"
    );
    assert_eq!(
        std::fs::read_to_string(f.notes.join("a.md")).unwrap(),
        "original"
    );
}

// ---- the side data of 0.5 --------------------------------------------------------------------------------------------------

#[test]
fn the_folder_of_0_5_opens_as_it_is_and_its_side_data_is_carried_over_once() {
    let loose_dir = tempfile::tempdir().unwrap();
    let loose = loose_file(loose_dir.path(), "trailer.md", "# Trailer");
    let loose_text = loose.display().to_string().replace('\\', "\\\\");
    let json = format!(
        r#"{{ "version": 1, "pins": ["2026-10-16 1031.md", "@file/d7d5f937"], "open": "2026-10-16 0140.md",
              "copied": {{ "2026-10-16 1032.md": {{ "size": 3, "modifiedMs": 5 }} }},
              "loose": [ {{ "id": "d7d5f937", "path": "{loose_text}", "openedMs": 9, "stamp": {{ "size": 9, "modifiedMs": 1 }} }} ],
              "shared": ["2026-10-16 0140.md"] }}"#
    );
    let mut legacy_bytes = Vec::new();
    let f = fx_with(|notes| {
        write(&notes.join("2026-10-16 0140.md"), "Beneficiary\nAddress");
        write(&notes.join("2026-10-16 1031.md"), "# Second");
        write(&notes.join("2026-10-16 1032.md"), "Third");
        std::fs::create_dir_all(notes.join("assets")).unwrap();
        write(&notes.join("assets").join("img-0020.jpg"), "jpg");
        std::fs::create_dir_all(notes.join(".permark").join("history")).unwrap();
        write(&notes.join(".permark").join("notes.json"), &json);
        write(&notes.join(".permark").join("notes.lock"), "");
        legacy_bytes = std::fs::read(notes.join(".permark").join("notes.json")).unwrap();
    });
    let list = f.service.list(1);
    // the notes are the files, with the names 0.5 gave
    let notes: Vec<&str> = list
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Note)
        .map(|e| e.path.as_str())
        .collect();
    assert_eq!(notes.len(), 3);
    // pins in 0.5's order, the loose file pinned too
    assert_eq!(list.entries[0].path, "2026-10-16 1031.md");
    assert!(list.entries[0].pinned);
    assert!(list.entries[1].kind == EntryKind::File && list.entries[1].pinned);
    let shared = list
        .entries
        .iter()
        .find(|e| e.path == "2026-10-16 0140.md")
        .unwrap();
    assert!(shared.shared && shared.title == "Beneficiary");
    assert_eq!(list.open.as_deref(), Some("2026-10-16 0140.md"));
    assert_eq!(f.service.shared_notes().len(), 1);
    // the old file is untouched
    assert_eq!(
        std::fs::read(f.notes.join(".permark").join("notes.json")).unwrap(),
        legacy_bytes
    );
    // changes now live in the profile; the old file is not read again
    f.service.set_pinned("2026-10-16 1032.md", true).unwrap();
    f.service.settings_changed();
    let again = NotesService::open(
        places(f.tmp.path()),
        f.store.clone(),
        NotesScope::new(),
        Box::new({
            let notes = f.notes.display().to_string();
            move || notes.clone()
        }),
    );
    let state = again.ctx().read(Clone::clone);
    assert!(state.pins.contains(&"2026-10-16 1032.md".to_owned()));
    assert!(state.imported);
}

#[test]
fn a_damaged_state_file_starts_clean_without_losing_a_note() {
    let f = fx();
    note(&f, "a.md", "a", 10);
    f.service.set_pinned("a.md", true).unwrap();
    let dir = f.profile.join("plugins").join("notes");
    let file = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .find(|e| e.file_name().to_string_lossy().starts_with("folder-"))
        .unwrap()
        .path();
    write(&file, "{ broken");
    let again = NotesService::open(
        places(f.tmp.path()),
        f.store.clone(),
        NotesScope::new(),
        Box::new({
            let notes = f.notes.display().to_string();
            move || notes.clone()
        }),
    );
    let list = again.list(1);
    assert_eq!(list.entries.len(), 1);
    assert!(!list.entries[0].pinned);
    assert!(a_broken_copy_exists(&dir));
}

fn a_broken_copy_exists(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().contains(".broken-"))
}

// ---- adversarial -----------------------------------------------------------------------------------------------------------------

#[test]
fn unicode_and_space_names_work_end_to_end() {
    let f = fx();
    note(&f, "日本語 のメモ.md", "# 日本語のメモ\n二行目", 10);
    note(&f, "café  ☕ notes.md", "# Café", 20);
    let list = f.service.list(1);
    assert_eq!(list.entries[0].title, "日本語のメモ");
    f.service.set_pinned("café  ☕ notes.md", true).unwrap();
    let renamed = f.service.rename("日本語 のメモ.md", "メモ 2").unwrap();
    assert_eq!(renamed, "メモ 2.md");
    let deleted = f.service.delete(std::slice::from_ref(&renamed)).unwrap();
    assert_eq!(
        f.service.undo_delete(deleted.undo_id.unwrap()).unwrap(),
        [renamed]
    );
}

#[test]
fn a_big_folder_lists_in_one_pass() {
    let f = fx();
    for n in 0..2_000 {
        write(
            &f.notes.join(format!("n{n:05}.md")),
            &format!("# Note {n}\nbody {n}"),
        );
    }
    let started = std::time::Instant::now();
    assert_eq!(f.service.list(1).entries.len(), 2_000);
    let second = std::time::Instant::now();
    assert_eq!(f.service.list(2).entries.len(), 2_000);
    // the second listing reads no note again
    assert!(second.elapsed() <= started.elapsed() + std::time::Duration::from_millis(50));
}

#[test]
fn notes_made_at_the_same_moment_by_many_threads_get_different_names() {
    let f = fx();
    let service = Arc::clone(&f.service);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let service = Arc::clone(&service);
            std::thread::spawn(move || {
                (0..10)
                    .map(|_| service.create(MOMENT).unwrap())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut all: Vec<String> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    assert_eq!(all.len(), 80);
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 80, "no name twice");
    assert_eq!(f.service.list(1).entries.len(), 80);
}

#[test]
fn rapid_renames_and_listing_from_other_threads_never_lose_the_file() {
    let f = fx();
    note(&f, "start.md", "# Start", 10);
    let lister = {
        let service = Arc::clone(&f.service);
        std::thread::spawn(move || {
            for n in 0..50 {
                let _ = service.list(n);
            }
        })
    };
    let mut current = "start.md".to_owned();
    for n in 0..40 {
        current = f.service.rename(&current, &format!("name {n}")).unwrap();
    }
    lister.join().unwrap();
    assert_eq!(current, "name 39.md");
    assert_eq!(names(&f), ["name 39.md"]);
    assert_eq!(
        std::fs::read_to_string(f.notes.join("name 39.md")).unwrap(),
        "# Start"
    );
}

#[test]
fn a_note_with_a_byte_order_mark_and_crlf_has_a_clean_title() {
    let f = fx();
    std::fs::write(
        f.notes.join("bom.md"),
        b"\xef\xbb\xbf# Heading\r\nsecond\r\n",
    )
    .unwrap();
    let entry = f.service.list(1).entries.remove(0);
    assert_eq!(entry.title, "Heading");
    assert_eq!(entry.snippet, "second");
}

#[test]
fn a_binary_file_named_like_a_note_has_no_title_and_does_not_break_the_list() {
    let f = fx();
    std::fs::write(f.notes.join("odd.md"), b"\0\x01\x02binary").unwrap();
    let entry = f.service.list(1).entries.remove(0);
    assert_eq!(entry.title, "");
}

#[test]
fn state_with_hostile_keys_is_forgotten_not_followed() {
    let f = fx();
    note(&f, "a.md", "a", 10);
    f.service.ctx().update(|s| {
        s.pins = vec!["../../etc/passwd".into(), "a.md".into(), "@file/zz".into()];
        s.shared = vec!["../outside.md".into()];
        s.loose.push(LooseEntry {
            id: "0a1b2c3d".into(),
            path: "/definitely/not/there.md".into(),
            opened_ms: 1,
            stamp: Stamp::default(),
        });
    });
    let list = f.service.list(1);
    assert!(list.entries.iter().any(|e| e.path == "a.md" && e.pinned));
    assert!(f.service.shared_notes().is_empty());
    let row = list
        .entries
        .iter()
        .find(|e| e.kind == EntryKind::File)
        .unwrap();
    assert_eq!(row.state, Some(LooseState::Gone));
    let state = f.service.ctx().read(Clone::clone);
    assert_eq!(state.pins, ["a.md"], "pins of nothing are dropped");
    let _ = loose_key("0a1b2c3d");
}

// ---- the plugin ---------------------------------------------------------------------------------------------------------------

mod plugin {
    use super::*;
    use permark_core::conditions::{Context, Distribution};
    use permark_core::events::{CoreEvent, EventSink};
    use permark_core::plugins::{
        AppServices, CxWorkspace, PluginCx, PluginId, RpcCall, RpcTable, Services,
    };
    use permark_core::settings::AppSettings;
    use serde_json::json;
    use std::sync::RwLock;

    struct Quiet;
    impl EventSink for Quiet {
        fn emit(&self, _event: CoreEvent) {}
    }

    fn call(
        table: &RpcTable,
        method: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, permark_core::IpcError> {
        table.call(
            "notes",
            method,
            &RpcCall {
                window: "main".into(),
                args,
            },
        )
    }

    #[test]
    fn the_plugin_serves_its_methods_while_it_runs_and_leaves_nothing_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("a.md"), "# Hello\nworld").unwrap();
        let dirs = AppDirs::under(tmp.path());
        let store = StateStore::app(&dirs);
        let rpc = RpcTable::new();
        let services = Services::new();
        let scope = NotesScope::new();
        let _scope_lease = services.provide(Arc::clone(&scope));
        let settings = Arc::new(RwLock::new(AppSettings {
            notes_folder: notes.display().to_string(),
            ..AppSettings::default()
        }));
        let cx = PluginCx::new(
            PluginId::of("notes"),
            CxWorkspace::App,
            store.plugin_dir(&PluginId::of("notes")),
            None,
            Context::welcome(Distribution::Direct),
            Arc::new(Quiet),
        )
        .with_store(store)
        .with_app(AppServices::new(
            Arc::clone(&rpc),
            Arc::clone(&services),
            Arc::clone(&settings),
        ));
        let plugin = crate::plugin();
        assert_eq!(plugin.manifest().id.as_str(), "notes");
        let running = plugin.start(&cx).unwrap();

        let list = call(&rpc, "list", json!(null)).unwrap();
        assert_eq!(list["entries"][0]["title"], "Hello");
        assert_eq!(list["folder"]["status"]["status"], "ready");
        let made = call(&rpc, "create", json!({})).unwrap();
        assert!(made.as_str().unwrap().to_lowercase().ends_with(".md"));
        assert_eq!(call(&rpc, "summary", json!({})).unwrap()["count"], 2);
        // bad arguments are a typed error, not a panic
        assert_eq!(
            call(&rpc, "rename", json!({ "path": 3 })).unwrap_err().kind,
            ErrorKind::InvalidData
        );
        assert_eq!(
            call(&rpc, "delete", json!({ "paths": ["../x.md"] }))
                .unwrap_err()
                .kind,
            ErrorKind::NotFound
        );
        // agents may ask the shared notes
        call(&rpc, "setShared", json!({ "path": "a.md", "shared": true })).unwrap();
        let api = services
            .get::<permark_core::notes::SharedNotesApi>()
            .unwrap();
        assert_eq!(api.0.list().len(), 1);
        assert_eq!(api.0.read("a.md").unwrap(), "# Hello\nworld");
        assert!(scope.observer().is_some());

        // a changed setting reaches the running plugin
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("b.md"), "# Other").unwrap();
        settings.write().unwrap().notes_folder = other.display().to_string();
        running.settings_changed(&settings.read().unwrap());
        let list = call(&rpc, "list", json!({})).unwrap();
        assert_eq!(list["entries"][0]["title"], "Other");
        assert_eq!(
            scope.folder().map(|r| r.path().to_path_buf()),
            Root::open(&other).ok().map(|r| r.path().to_path_buf())
        );

        drop(running);
        assert_eq!(
            call(&rpc, "list", json!({})).unwrap_err().kind,
            ErrorKind::Unavailable
        );
        assert!(services
            .get::<permark_core::notes::SharedNotesApi>()
            .is_none());
        assert!(scope.observer().is_none());
        assert!(rpc.served().is_empty());
    }

    #[test]
    fn without_the_app_services_the_plugin_does_not_start() {
        let tmp = tempfile::tempdir().unwrap();
        let cx = PluginCx::new(
            PluginId::of("notes"),
            CxWorkspace::App,
            tmp.path().to_path_buf(),
            None,
            Context::welcome(Distribution::Direct),
            Arc::new(Quiet),
        );
        assert!(crate::plugin().start(&cx).is_err());
    }
}

#[test]
fn journal_entries_are_per_folder_so_another_folder_does_not_see_them() {
    let f = fx();
    note(&f, "a.md", "a", 10);
    f.service.buffer("a.md", "h", Some("typed"), 5).unwrap();
    let other = f.tmp.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    write(&other.join("a.md"), "a");
    *f.chosen.lock().unwrap() = other.display().to_string();
    f.service.settings_changed();
    assert!(!f.service.list(1).entries[0].recovered);
    *f.chosen.lock().unwrap() = f.notes.display().to_string();
    f.service.settings_changed();
    assert!(f.service.list(2).entries[0].recovered);
    let _ = JournalEntry::default();
    let _ = FolderState::default();
}

#[test]
fn a_note_that_changes_while_it_is_copied_is_kept_with_its_copy() {
    let f = fx();
    let project = f.tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    note(&f, "a.md", "before", 10);
    let moved = f
        .service
        .move_after("a.md", &project.display().to_string(), "x.md", || {
            write(&f.notes.join("a.md"), "typed meanwhile");
        })
        .unwrap();
    assert!(moved.kept && moved.undo_id.is_none());
    assert_eq!(
        std::fs::read_to_string(f.notes.join("a.md")).unwrap(),
        "typed meanwhile"
    );
    assert_eq!(
        std::fs::read_to_string(project.join("x.md")).unwrap(),
        "before"
    );
}

#[test]
fn what_an_editor_of_this_run_is_typing_is_not_text_a_crash_left() {
    let f = fx();
    note(&f, "a.md", "saved", 100);
    let file = loose_file(&f.tmp.path().join("d"), "x.md", "disk");
    let path = f.service.open_file(&file.display().to_string(), 1).unwrap();
    let live = f.service.started_ms + 10_000;
    f.service
        .buffer("a.md", "h", Some("typing now"), live)
        .unwrap();
    f.service
        .buffer(&path, "h", Some("typing here"), live)
        .unwrap();
    let list = f.service.list(1);
    assert!(list.entries.iter().all(|e| !e.recovered));
    let row = list
        .entries
        .iter()
        .find(|e| e.kind == EntryKind::File)
        .unwrap();
    assert_eq!(row.state, Some(LooseState::Clean));
    assert!(row.pending, "the typed text is still known");
    // the same entries from before this run are what a crash left
    f.service
        .buffer("a.md", "h", Some("typing now"), 5)
        .unwrap();
    f.service
        .buffer(&path, "h", Some("typing here"), 5)
        .unwrap();
    let list = f.service.list(2);
    assert!(list.entries.iter().any(|e| e.recovered));
    assert_eq!(
        list.entries
            .iter()
            .find(|e| e.kind == EntryKind::File)
            .unwrap()
            .state,
        Some(LooseState::Unsaved)
    );
}
