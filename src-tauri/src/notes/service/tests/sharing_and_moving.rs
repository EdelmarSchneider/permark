//! Sharing a note with agents, and moving a note into a project.

use super::*;
use crate::notes::share::{list_shared, read_shared};

/// A folder for a project: the notes move into it.
fn project() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn entry(f: &Fx, path: &str) -> NoteEntry {
    f.service
        .list()
        .notes
        .into_iter()
        .find(|note| note.path == path)
        .unwrap_or_else(|| panic!("{path} is not listed"))
}

fn shared_names(f: &Fx) -> Vec<String> {
    list_shared(f.service.root())
        .into_iter()
        .map(|note| note.note)
        .collect()
}

// ---- sharing ---------------------------------------------------------------------------------------------------

#[test]
fn a_note_is_shared_and_stops_being_shared_at_once() {
    let f = fx();
    let one = f.new_note("# One\n");
    let two = f.new_note("# Two\n");
    assert!(!entry(&f, &one).shared && !entry(&f, &two).shared);
    assert_eq!(shared_names(&f), Vec::<String>::new());

    f.service.set_shared(&one, true).expect("share");
    assert!(entry(&f, &one).shared && !entry(&f, &two).shared);
    // an agent's program reads the same file the notes keep: nothing else is needed for it to see the note
    assert_eq!(shared_names(&f), std::slice::from_ref(&one));
    assert_eq!(
        read_shared(f.service.root(), &one).expect("read").text,
        "# One\n"
    );
    assert!(read_shared(f.service.root(), &two).is_err());

    f.service.set_shared(&one, false).expect("stop sharing");
    assert!(!entry(&f, &one).shared);
    assert_eq!(shared_names(&f), Vec::<String>::new());
    assert!(read_shared(f.service.root(), &one).is_err());
}

#[test]
fn sharing_is_for_notes_only_and_a_note_that_is_not_there_cannot_be_shared() {
    let f = fx();
    let there = tempfile::tempdir().expect("tempdir");
    let file = there.path().join("proposal.md");
    fs::write(&file, "# Proposal\n").expect("write");
    let path = match f.service.open_file(&file, T0).expect("open") {
        OpenedFile {
            kind: EntryKind::File,
            path,
        } => path,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        f.service.set_shared(&path, true).err().map(|e| e.kind()),
        Some("notANote"),
        "a loose file never reaches an agent"
    );
    assert_eq!(
        f.service
            .set_shared("missing.md", true)
            .err()
            .map(|e| e.kind()),
        Some("notFound")
    );
    // stopping what is not shared is nothing
    assert!(f.service.set_shared("missing.md", false).is_ok());
}

#[test]
fn a_shared_note_stays_shared_when_it_is_renamed_and_the_name_it_had_is_not_shared() {
    let f = fx();
    let note = f.new_note("# Old\n");
    f.service.set_shared(&note, true).expect("share");
    let to = f.service.rename(&note, "Server IPs").expect("rename");
    assert_eq!(shared_names(&f), std::slice::from_ref(&to));
    // a note made later under the old name is not shared by that
    f.write(&note, "# Another\n");
    assert!(read_shared(f.service.root(), &note).is_err());
    assert!(read_shared(f.service.root(), &to).is_ok());
}

#[test]
fn deleting_a_shared_note_unshares_it_and_undo_shares_it_again() {
    let f = fx();
    let note = f.new_note("# Shared\n");
    f.service.set_shared(&note, true).expect("share");
    let deleted = f
        .service
        .delete(std::slice::from_ref(&note))
        .expect("delete");
    assert_eq!(shared_names(&f), Vec::<String>::new());
    // a note made under that name meanwhile is not shared
    f.write(&note, "# Made later\n");
    assert!(read_shared(f.service.root(), &note).is_err());
    fs::remove_file(f.path(&note)).expect("remove");

    f.service
        .undo_delete(deleted.undo_id.expect("an undo"))
        .expect("undo");
    assert_eq!(shared_names(&f), [note]);
}

#[test]
fn what_is_shared_survives_a_restart_and_is_forgotten_when_the_note_is_gone() {
    let f = fx();
    let kept = f.new_note("# Kept\n");
    let gone = f.new_note("# Gone\n");
    f.service.set_shared(&kept, true).expect("share");
    f.service.set_shared(&gone, true).expect("share");
    let Fx {
        dir, service, bin, ..
    } = f;
    drop(service);
    fs::remove_file(dir.path().join(&gone)).expect("another program deletes it");
    let (service, _) = open_in(dir.path(), &bin, &|_| true);
    assert!(service
        .list()
        .notes
        .iter()
        .any(|n| n.path == kept && n.shared));
    let listed: Vec<String> = list_shared(dir.path())
        .into_iter()
        .map(|n| n.note)
        .collect();
    assert_eq!(listed, [kept]);
    // the name is not shared any more, so a note made under it later is not
    fs::write(dir.path().join(&gone), "# Back\n").expect("write");
    assert!(read_shared(dir.path(), &gone).is_err());
}

#[test]
fn stopping_to_share_reaches_agents_even_when_notes_json_is_from_a_newer_version() {
    let f = fx();
    let kept = f.new_note("# Kept\n");
    let secret = f.new_note("# Secret\n");
    f.service.set_shared(&secret, true).expect("share");
    let Fx {
        dir, service, bin, ..
    } = f;
    drop(service);
    // a newer version of TrailMark wrote the file since, with more in it than this one knows
    let file = NotesData::path(dir.path());
    fs::write(
        &file,
        format!(
            r#"{{"version": 99, "shared": ["{kept}", "{secret}", {{"name": "x.md"}}], "folders": {{"work": ["{secret}"]}}}}"#
        ),
    )
    .expect("write");
    let (service, _) = open_in(dir.path(), &bin, &|_| true);
    assert!(read_shared(dir.path(), &secret).is_ok());

    // nothing is shared through a file that must not be written over
    assert_eq!(
        service.set_shared(&kept, true).err().map(|e| e.kind()),
        Some("notesNewer")
    );
    // but stopping to share is what the person asked for, and an agent must stop seeing the note at once
    service.set_shared(&secret, false).expect("stop sharing");
    assert!(read_shared(dir.path(), &secret).is_err());
    assert!(read_shared(dir.path(), &kept).is_ok());
    assert!(!service
        .list()
        .notes
        .iter()
        .any(|n| n.shared && n.path == secret));
    // the rest of the newer file is as it was
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(&file).expect("read")).expect("json");
    assert_eq!(after["version"], 99);
    assert_eq!(after["folders"]["work"][0], secret.as_str());
    assert_eq!(after["shared"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        after["shared"][1]["name"], "x.md",
        "what this version does not read is kept"
    );
}

#[test]
fn a_change_of_sharing_that_could_not_be_written_is_an_error_and_does_not_happen() {
    let f = fx();
    let note = f.new_note("# One\n");
    // the file cannot be replaced: there is a folder where it should be
    let file = NotesData::path(f.service.root());
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    let _ = fs::remove_file(&file);
    fs::create_dir(&file).expect("a folder in its place");
    assert!(f.service.set_shared(&note, true).is_err());
    assert!(
        !entry(&f, &note).shared,
        "the row does not say it is shared"
    );
    assert_eq!(shared_names(&f), Vec::<String>::new());
}

#[test]
fn a_shared_name_does_not_outlive_its_note_when_the_note_is_deleted_from_outside() {
    let f = fx();
    let shared = f.new_note("# Shared\n");
    f.service.set_shared(&shared, true).expect("share");
    // Explorer, a sync client or an agent deletes the file while the window is open
    fs::remove_file(f.path(&shared)).expect("deleted from outside");
    // the next list of the window forgets the name, in the file too (what an agent reads)
    assert!(!f.service.list().notes.iter().any(|n| n.path == shared));
    assert_eq!(
        NotesData::read(f.service.root()).shared,
        Vec::<String>::new()
    );
    // so a note that is made or taken under that name later is not shared by it
    f.write(&shared, "# Someone else's\n");
    assert!(read_shared(f.service.root(), &shared).is_err());
    assert!(!entry(&f, &shared).shared);
}

#[test]
fn a_note_that_takes_a_name_that_was_shared_does_not_take_the_sharing_with_it() {
    let f = fx();
    let public = f.new_note("# Public\n");
    let private = f.new_note("# Private\n");
    f.service.set_shared(&public, true).expect("share");
    // the shared note is deleted from outside and, before the window has looked at the folder again, another note is
    // renamed to its name: it is a different note, and the person never shared it
    fs::remove_file(f.path(&public)).expect("deleted from outside");
    let to = f
        .service
        .rename(&private, public.trim_end_matches(".md"))
        .expect("rename");
    assert_eq!(to, public, "it took the name that was shared");
    assert!(
        read_shared(f.service.root(), &to).is_err(),
        "a private note must not be read by an agent because it took a shared name"
    );
    assert_eq!(shared_names(&f), Vec::<String>::new());

    // the same when the name is taken by a note that is created
    f.service.set_shared(&to, true).expect("share");
    fs::remove_file(f.path(&to)).expect("deleted from outside");
    let NoteSaved::Saved { path, .. } = f.save(&to, "# Brand new\n", "", T0) else {
        panic!("it is saved")
    };
    assert_eq!(path, to);
    assert!(read_shared(f.service.root(), &to).is_err());
    assert!(!entry(&f, &to).shared);
}

#[test]
fn a_renamed_note_that_was_shared_stays_shared_once_under_its_new_name() {
    let f = fx();
    let note = f.new_note("# Old\n");
    f.service.set_shared(&note, true).expect("share");
    let first = f.service.rename(&note, "one").expect("rename");
    let second = f.service.rename(&first, "two").expect("rename");
    assert_eq!(shared_names(&f), std::slice::from_ref(&second));
    assert_eq!(NotesData::read(f.service.root()).shared, [second]);
}

#[test]
fn a_second_window_cannot_share() {
    let f = fx();
    let note = f.new_note("# One\n");
    let (second, _) = open_in(f.dir.path(), &f.bin, &|_| true);
    assert_eq!(
        second.set_shared(&note, true).err().map(|e| e.kind()),
        Some("notesLocked")
    );
}

// ---- moving a note into a project ------------------------------------------------------------------------------------

#[test]
fn a_note_moves_into_a_project_byte_for_byte_and_its_row_becomes_a_file() {
    let f = fx();
    let text = "\u{feff}# Ask the agent\r\n\r\nto add tests.\r\n";
    let note = f.new_note(text);
    f.service.set_pinned(&note, true).expect("pin");
    let project = project();
    let target = project.path().join("Ask the agent.md");

    let moved = f.service.move_to(&note, &target, T0).expect("move");
    assert_eq!(fs::read(&target).expect("read"), text.as_bytes());
    assert_eq!(
        moved.destination,
        dunce::canonicalize(&target)
            .expect("canonical")
            .display()
            .to_string()
    );
    assert!(moved.undo_id.is_some());
    // the note is gone from the notes (to the recycle bin), and the copy is a file of the list, pinned like the note was
    assert!(!f.path(&note).exists());
    assert_eq!(f.bin.calls.load(Ordering::SeqCst), 1);
    let list = f.service.list();
    assert_eq!(list.notes.len(), 1);
    assert_eq!(list.notes[0].kind, EntryKind::File);
    assert_eq!(list.notes[0].path, moved.file);
    assert_eq!(list.notes[0].title, "Ask the agent.md");
    assert!(list.notes[0].pinned);
}

#[test]
fn a_name_without_an_extension_gets_md_and_the_name_a_file_cannot_have_is_refused() {
    let f = fx();
    let note = f.new_note("# One\n");
    let project = project();
    let moved = f
        .service
        .move_to(&note, &project.path().join("plan"), T0)
        .expect("move");
    assert!(project.path().join("plan.md").is_file());
    assert!(moved.destination.ends_with("plan.md"));

    let other = f.new_note("# Two\n");
    for (bad, kind) in [
        ("what?.md", "invalidName"),
        ("notes.txt", "notMarkdown"),
        ("con.md", "invalidName"),
    ] {
        let error = f
            .service
            .move_to(&other, &project.path().join(bad), T0)
            .expect_err("refused");
        assert_eq!(error.kind(), kind, "{bad}");
    }
    assert!(
        f.path(&other).is_file(),
        "the note stays when the move is refused"
    );
}

#[test]
fn a_move_never_overwrites_and_never_goes_into_the_notes_folder_or_to_a_place_that_is_not_there() {
    let f = fx();
    let note = f.new_note("# One\n");
    let project = project();
    fs::write(project.path().join("taken.md"), "someone's file\n").expect("write");
    let error = f
        .service
        .move_to(&note, &project.path().join("taken.md"), T0)
        .expect_err("it exists");
    assert_eq!(error.kind(), "exists");
    assert_eq!(
        fs::read_to_string(project.path().join("taken.md")).expect("read"),
        "someone's file\n"
    );

    for (place, kind) in [
        (f.path("copy.md"), "moveInside"),
        (f.path("sub").join("copy.md"), "notFound"),
        (project.path().join("not-here").join("copy.md"), "notFound"),
        (PathBuf::from("relative.md"), "invalidPath"),
    ] {
        let error = f.service.move_to(&note, &place, T0).expect_err("refused");
        assert_eq!(error.kind(), kind, "{}", place.display());
    }
    assert!(
        f.path(&note).is_file(),
        "the note stays when the move is refused"
    );
    assert_eq!(f.bin.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn the_note_stays_and_the_copy_goes_when_the_note_cannot_be_recycled() {
    struct Refusing;
    impl Recycler for Refusing {
        fn recycle(&self, _paths: &[PathBuf]) -> Result<(), String> {
            Err("the bin is full".to_owned())
        }
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let vault = Arc::new(Vault::open(dir.path()).expect("open"));
    let (service, _) = NotesService::open(
        vault,
        Arc::new(OwnWrites::new()),
        Arc::new(Refusing),
        None,
        T0,
        &|_| true,
    );
    let created = service.create_new(MOMENT, T0).expect("create");
    let name = created.path;
    service
        .save(&name, "# One\n", &created.hash, T0, MOMENT)
        .expect("save");
    let project = project();
    let error = service
        .move_to(&name, &project.path().join("one.md"), T0)
        .expect_err("not recycled");
    assert_eq!(error.kind(), "io");
    assert!(dir.path().join(&name).is_file(), "the note stays");
    assert!(
        !project.path().join("one.md").exists(),
        "and no copy is left behind"
    );
}

#[test]
fn undo_brings_the_note_back_and_takes_the_copy_away_when_nobody_changed_it() {
    let f = fx();
    let note = f.new_note("# One\n");
    f.service.set_shared(&note, true).expect("share");
    let project = project();
    let target = project.path().join("one.md");
    let moved = f.service.move_to(&note, &target, T0).expect("move");
    assert_eq!(
        shared_names(&f),
        Vec::<String>::new(),
        "a note in the bin is not shared"
    );

    let undone = f
        .service
        .undo_move(moved.undo_id.expect("an undo"))
        .expect("undo");
    assert_eq!(
        undone,
        MoveUndone {
            note: note.clone(),
            removed: true
        }
    );
    assert_eq!(f.read(&note), "# One\n");
    assert!(!target.exists(), "the copy is taken away");
    let list = f.service.list();
    assert_eq!(list.notes.len(), 1);
    assert_eq!(list.notes[0].kind, EntryKind::Note);
    assert_eq!(shared_names(&f), [note], "and it is shared again");
    // once
    assert_eq!(
        f.service
            .undo_move(moved.undo_id.expect("an undo"))
            .err()
            .map(|e| e.kind()),
        Some("unknownUndo")
    );
}

#[test]
fn undo_leaves_a_copy_with_text_typed_into_it_and_keeps_that_text() {
    let f = fx();
    let note = f.new_note("# One\n");
    let project = project();
    let target = project.path().join("one.md");
    let moved = f.service.move_to(&note, &target, T0).expect("move");
    // the copy is open in the Notes window and has text typed into it, which is kept apart from the file until it is saved
    let copy = moved.file.clone();
    f.service
        .save_pending(
            &copy,
            "# One\n\nTYPED IN THE COPY",
            &hash_hex(&fs::read(&target).expect("read")),
            T0,
        )
        .expect("kept");

    let undone = f
        .service
        .undo_move(moved.undo_id.expect("an undo"))
        .expect("undo");
    assert!(!undone.removed, "a copy with typed text is not taken away");
    assert!(target.is_file());
    assert_eq!(f.read(&note), "# One\n", "the note is back");
    // the text is where it was, and the copy is still listed with its mark
    assert_eq!(
        f.service
            .load_pending(&copy)
            .expect("listed")
            .map(|kept| kept.text),
        Some("# One\n\nTYPED IN THE COPY".to_owned())
    );
    let row = f
        .service
        .list()
        .notes
        .into_iter()
        .find(|n| n.path == copy)
        .expect("still listed");
    assert!(row.unsaved);
}

#[test]
fn an_undo_that_cannot_put_the_note_back_keeps_the_record_of_the_move() {
    let f = fx();
    let note = f.new_note("# One\n");
    let project = project();
    let moved = f
        .service
        .move_to(&note, &project.path().join("one.md"), T0)
        .expect("move");
    let id = moved.undo_id.expect("an undo");
    // what the bin held for the note is gone (it was pushed out), so the note cannot be put back
    lock(&f.service.state).undo.clear();
    assert!(f.service.undo_move(id).is_err());
    assert!(
        lock(&f.service.state)
            .moves
            .iter()
            .any(|record| record.undo_id == id),
        "the toast's Undo is not answered as if there never was a move"
    );
}

#[test]
fn undo_leaves_a_copy_that_was_changed_since_and_keeps_it_listed() {
    let f = fx();
    let note = f.new_note("# One\n");
    let project = project();
    let target = project.path().join("one.md");
    let moved = f.service.move_to(&note, &target, T0).expect("move");
    fs::write(&target, "# One, and what I wrote in the project\n").expect("edit");

    let undone = f
        .service
        .undo_move(moved.undo_id.expect("an undo"))
        .expect("undo");
    assert!(!undone.removed);
    assert!(target.is_file(), "what was written in the project stays");
    assert_eq!(f.read(&note), "# One\n", "and the note is back");
    let kinds: Vec<EntryKind> = f.service.list().notes.iter().map(|n| n.kind).collect();
    assert_eq!(kinds, [EntryKind::Note, EntryKind::File]);
}

#[test]
fn a_name_for_the_save_dialog_is_made_from_the_title_and_is_free_in_the_folder() {
    let f = fx();
    let titled = f.new_note("# Ask the agent: add tests?\n\nbody\n");
    let blank_title = f.new_note("---\n\n");
    let project = project();
    assert_eq!(
        f.service
            .suggest_name(&titled, Some(project.path()))
            .expect("name"),
        "Ask the agent add tests.md"
    );
    fs::write(project.path().join("Ask the agent add tests.md"), "x").expect("write");
    assert_eq!(
        f.service
            .suggest_name(&titled, Some(project.path()))
            .expect("name"),
        "Ask the agent add tests 2.md"
    );
    // without a folder there is nothing to be taken
    assert_eq!(
        f.service.suggest_name(&titled, None).expect("name"),
        "Ask the agent add tests.md"
    );
    // a note with no title is named after its own file
    assert!(f
        .service
        .suggest_name(&blank_title, None)
        .expect("name")
        .starts_with("2026-10-05 0907"));
}

#[test]
fn a_second_window_cannot_move() {
    let f = fx();
    let note = f.new_note("# One\n");
    let (second, _) = open_in(f.dir.path(), &f.bin, &|_| true);
    let project = project();
    let error = second
        .move_to(&note, &project.path().join("one.md"), T0)
        .expect_err("locked");
    assert_eq!(error.kind(), "notesLocked");
    assert!(!project.path().join("one.md").exists());
}

#[test]
fn an_empty_note_can_be_shared_and_moved_like_any_other() {
    let f = fx();
    let note = f.new_note("");
    f.service.set_shared(&note, true).expect("share");
    assert!(entry(&f, &note).shared);
    assert_eq!(shared_names(&f), std::slice::from_ref(&note));
    let shared = read_shared(f.service.root(), &note).expect("an empty note reads as empty");
    assert_eq!((shared.text.as_str(), shared.title.as_str()), ("", ""));

    // it is moved as it is: an empty file, named after its own file because it has no title
    let project = project();
    let name = f.service.suggest_name(&note, None).expect("name");
    assert_eq!(name, "2026-10-05 0907.md");
    let target = project.path().join(&name);
    let moved = f.service.move_to(&note, &target, T0).expect("move");
    assert_eq!(fs::read(&target).expect("read"), b"");
    assert!(!f.path(&note).exists());
    assert_eq!(
        shared_names(&f),
        Vec::<String>::new(),
        "sharing went with it"
    );
    let undone = f
        .service
        .undo_move(moved.undo_id.expect("undo"))
        .expect("undo");
    assert!(undone.removed);
    assert_eq!(f.read(&undone.note), "");
}
