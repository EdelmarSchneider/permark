//! Loose files: the files opened in the Notes window that are not in the notes folder.

use super::*;
use crate::edit::SaveOutcome;
use crate::notes::loose::{self, MAX_LISTED};
use crate::notes::service::files::LooseState;

/// A folder that is not the notes folder, with the files someone sent in it.
struct Elsewhere {
    dir: TempDir,
}

impl Elsewhere {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("tempdir"),
        }
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        fs::write(&path, text).expect("write");
        path
    }
}

/// Opens a file as the window does, and gives the path to show it under.
fn open_file(f: &Fx, file: &Path, now: u64) -> String {
    match f.service.open_file(file, now).expect("open") {
        OpenedFile {
            kind: EntryKind::File,
            path,
        } => path,
        other => panic!("not a loose file: {other:?}"),
    }
}

fn entry(f: &Fx, path: &str) -> NoteEntry {
    f.service
        .list()
        .notes
        .into_iter()
        .find(|note| note.path == path)
        .unwrap_or_else(|| panic!("{path} is not listed"))
}

fn titles(f: &Fx) -> Vec<String> {
    f.service
        .list()
        .notes
        .into_iter()
        .map(|note| note.title)
        .collect()
}

#[test]
fn the_notes_come_first_and_the_files_follow_by_the_time_they_were_opened() {
    let f = fx();
    f.new_note("# A note");
    let there = Elsewhere::new();
    let first = open_file(&f, &there.file("proposal.md", "# Proposal\n"), T0);
    let second = open_file(&f, &there.file("plan.txt", "a plan\n"), T0 + MIN);
    let list = f.service.list();
    let kinds: Vec<EntryKind> = list.notes.iter().map(|n| n.kind).collect();
    assert_eq!(kinds, [EntryKind::Note, EntryKind::File, EntryKind::File]);
    assert_eq!(titles(&f), ["A note", "plan.txt", "proposal.md"]);
    let plan = entry(&f, &second);
    assert_eq!(
        (plan.size_bytes, plan.missing, plan.unsaved),
        (7, false, false)
    );
    assert!(plan.folder.is_some_and(|folder| !folder.is_empty()));
    assert_eq!(entry(&f, &first).title, "proposal.md");
}

#[test]
fn opening_a_file_again_lists_it_once_and_brings_it_to_the_top() {
    let f = fx();
    let there = Elsewhere::new();
    let one = there.file("one.md", "1\n");
    let two = there.file("two.md", "2\n");
    let first = open_file(&f, &one, T0);
    open_file(&f, &two, T0 + MIN);
    assert_eq!(titles(&f), ["two.md", "one.md"]);
    let again = open_file(&f, &one, T0 + 2 * MIN);
    assert_eq!(again, first, "the same file keeps its id");
    assert_eq!(titles(&f), ["one.md", "two.md"]);
}

#[test]
fn a_markdown_file_of_the_notes_folder_opens_as_a_note_and_one_deeper_is_a_loose_file() {
    let f = fx();
    f.write("a note.md", "# A note\n");
    let opened = f
        .service
        .open_file(&f.path("a note.md"), T0)
        .expect("open the note");
    assert_eq!(
        opened,
        OpenedFile {
            kind: EntryKind::Note,
            path: "a note.md".to_owned()
        }
    );
    fs::create_dir(f.path("deeper")).expect("mkdir");
    fs::write(f.path("deeper").join("x.md"), "x\n").expect("write");
    let deeper = f
        .service
        .open_file(&f.path("deeper").join("x.md"), T0)
        .expect("open the deeper one");
    assert_eq!(deeper.kind, EntryKind::File);
}

#[test]
fn what_is_not_a_text_file_is_not_listed() {
    let f = fx();
    let there = Elsewhere::new();
    let binary = there.dir.path().join("picture.bin");
    fs::write(&binary, [0x89, b'P', b'N', b'G', 0, 0, 0, 1]).expect("write");
    let kind_of = |path: &Path| {
        f.service
            .open_file(path, T0)
            .expect_err("not listed")
            .kind()
    };
    assert_eq!(kind_of(&binary), "binaryFile");
    assert_eq!(kind_of(there.dir.path()), "notAFile");
    assert_eq!(kind_of(&there.dir.path().join("missing.md")), "notFound");
    assert_eq!(f.service.list().notes.len(), 0);
    // an empty file is a text file
    assert!(f.service.open_file(&there.file("empty.md", ""), T0).is_ok());
}

#[test]
fn only_the_newest_files_are_listed_but_pinned_ones_and_ones_with_unsaved_text_stay() {
    let f = fx();
    let there = Elsewhere::new();
    let mut now = T0;
    let mut open = |name: &str| {
        now += MIN;
        open_file(&f, &there.file(name, name), now)
    };
    let pinned = open("f00.md");
    f.service.set_pinned(&pinned, true).expect("pin");
    let unsaved = open("f01.md");
    f.service
        .save_pending(&unsaved, "typed", "abc", T0)
        .expect("keep");
    for n in 2..(MAX_LISTED + 2) {
        open(&format!("f{n:02}.md"));
    }
    let count = |f: &Fx| f.service.list().notes.len();
    assert_eq!(count(&f), MAX_LISTED + 2, "the limit applies to the others");
    // one more: the oldest of the others goes
    open(&format!("f{:02}.md", MAX_LISTED + 2));
    let names = titles(&f);
    assert_eq!(names.len(), MAX_LISTED + 2);
    assert!(names.contains(&"f00.md".to_owned()), "the pinned one stays");
    assert!(
        names.contains(&"f01.md".to_owned()),
        "the unsaved one stays"
    );
    assert!(
        !names.contains(&"f02.md".to_owned()),
        "the oldest other goes"
    );
    assert!(names.contains(&"f03.md".to_owned()));
}

#[test]
fn a_loose_file_is_reached_through_its_id_and_the_images_next_to_it_only() {
    let f = fx();
    let there = Elsewhere::new();
    there.file("pic.png", "not really a picture");
    let path = open_file(&f, &there.file("proposal.md", "# P\n"), T0);
    let id = loose::id_of(&path).expect("an id").to_owned();

    let doc = f.service.loose_doc(&path).expect("the file");
    assert_eq!(
        (doc.name.as_str(), doc.asset.as_deref()),
        ("proposal.md", None)
    );
    assert_eq!(doc.ui_path, path);

    let image = f
        .service
        .loose_doc(&format!("@file/{id}/pic.png"))
        .expect("an image next to it");
    assert_eq!(image.asset.as_deref(), Some("pic.png"));
    // anything else of the folder is not reachable, whatever the path says
    for bad in [
        format!("@file/{id}/sub/pic.png"),
        format!("@file/{id}/..\\pic.png"),
        format!("@file/{id}/../pic.png"),
    ] {
        let error = f.service.loose_doc(&bad).err().expect("refused");
        assert_eq!(error.kind(), "pathEscape", "{bad}");
    }
    for unknown in [
        "@file/ffffffff/proposal.md".to_owned(),
        "proposal.md".to_owned(),
        format!("@file/{id}"),
    ] {
        let error = f.service.loose_doc(&unknown).err().expect("refused");
        assert_eq!(error.kind(), "notANote", "{unknown}");
    }
}

#[test]
fn a_file_is_written_when_the_person_saves_and_never_over_a_version_they_have_not_seen() {
    let f = fx();
    let there = Elsewhere::new();
    let file = there.file("proposal.md", "# Proposal\n");
    let path = open_file(&f, &file, T0);
    let saved = |outcome: SaveOutcome| match outcome {
        SaveOutcome::Saved { hash, .. } => hash,
        conflict @ SaveOutcome::Conflict { .. } => panic!("not saved: {conflict:?}"),
    };

    let hash = saved(
        f.service
            .save_file(&path, "# Proposal\nmore\n", &hash_hex(b"# Proposal\n"))
            .expect("save"),
    );
    assert_eq!(
        fs::read_to_string(&file).expect("read"),
        "# Proposal\nmore\n"
    );

    // someone else wrote the file meanwhile: nothing is written
    fs::write(&file, "# Theirs, and a little longer\n").expect("their write");
    let outcome = f.service.save_file(&path, "# Mine\n", &hash).expect("save");
    assert!(matches!(
        outcome,
        SaveOutcome::Conflict { disk_hash: Some(_) }
    ));
    assert_eq!(
        fs::read_to_string(&file).expect("read"),
        "# Theirs, and a little longer\n"
    );

    // the file is gone: nothing is written either
    fs::remove_file(&file).expect("remove");
    assert_eq!(
        f.service.save_file(&path, "# Mine\n", &hash).expect("save"),
        SaveOutcome::Conflict { disk_hash: None }
    );
    assert!(!file.exists());
}

#[test]
fn a_text_file_is_edited_and_saved_like_a_markdown_one_and_another_kind_is_not() {
    let f = fx();
    let there = Elsewhere::new();
    let text = there.file("todo.txt", "buy milk\r\n");
    let data = there.file("data.json", "{}");
    let text_path = open_file(&f, &text, T0);
    let data_path = open_file(&f, &data, T0 + MIN);
    let saved = f
        .service
        .save_file(
            &text_path,
            "buy milk\r\nand eggs\r\n",
            &hash_hex(b"buy milk\r\n"),
        )
        .expect("save");
    assert!(matches!(saved, SaveOutcome::Saved { .. }));
    assert_eq!(
        fs::read(&text).expect("read"),
        b"buy milk\r\nand eggs\r\n",
        "line endings stay as they were typed"
    );
    let error = f
        .service
        .save_file(&data_path, "[]", &hash_hex(b"{}"))
        .expect_err("not an editable kind");
    assert_eq!(error.kind(), "notMarkdown");
    assert_eq!(fs::read(&data).expect("read"), b"{}");
}

#[test]
fn text_that_was_typed_is_kept_apart_from_the_file_and_found_again_after_a_restart() {
    let f = fx();
    let there = Elsewhere::new();
    let file = there.file("proposal.md", "# Proposal\n");
    let path = open_file(&f, &file, T0);
    assert_eq!(f.service.load_pending(&path).expect("ask"), None);
    f.service
        .save_pending(&path, "# Proposal\nhalf a sentence", "base-hash", T0 + MIN)
        .expect("keep");
    assert_eq!(
        fs::read_to_string(&file).expect("read"),
        "# Proposal\n",
        "the file is not touched"
    );
    assert!(entry(&f, &path).unsaved);

    // closing and opening again
    let Fx {
        dir, service, bin, ..
    } = f;
    drop(service);
    let (service, report) = open_in(dir.path(), &bin, &|_| true);
    assert_eq!(report.read_only, None);
    let kept = service
        .load_pending(&path)
        .expect("ask")
        .expect("the text is still kept");
    assert_eq!(kept.text, "# Proposal\nhalf a sentence");
    assert_eq!(kept.base_hash, "base-hash");
    assert_eq!(kept.saved_ms, T0 + MIN);

    // saving the file is what lets it go
    let hash = hash_hex(b"# Proposal\n");
    service
        .save_file(&path, "# Proposal\ndone\n", &hash)
        .expect("save");
    assert_eq!(service.load_pending(&path).expect("ask"), None);
    assert!(!service.list().notes[0].unsaved);
}

#[test]
fn a_damaged_list_loses_the_files_but_not_the_text_typed_into_them() {
    let f = fx();
    let there = Elsewhere::new();
    let file = there.file("proposal.md", "# Proposal\n");
    let path = open_file(&f, &file, T0);
    f.service
        .save_pending(&path, "typed", "base-hash", T0 + MIN)
        .expect("keep");
    let Fx {
        dir, service, bin, ..
    } = f;
    drop(service);
    fs::write(
        dir.path().join(".trailmark").join("notes.json"),
        "{{{ damaged",
    )
    .expect("damage");
    let (service, _) = open_in(dir.path(), &bin, &|_| true);
    let list = service.list();
    assert_eq!(
        list.notes.len(),
        1,
        "the file is listed again from its text"
    );
    assert_eq!(list.notes[0].title, "proposal.md");
    assert!(list.notes[0].unsaved);
    assert_eq!(
        service
            .load_pending(&list.notes[0].path)
            .expect("ask")
            .map(|kept| kept.text),
        Some("typed".to_owned())
    );
}

#[test]
fn the_status_tells_a_change_and_a_loss_and_follows_a_rename_in_the_folder() {
    let f = fx();
    let there = Elsewhere::new();
    let file = there.file("proposal.md", "# Proposal\n");
    let path = open_file(&f, &file, T0);
    f.service.set_pinned(&path, true).expect("pin");
    let id = loose::id_of(&path).expect("an id").to_owned();
    let state = |f: &Fx| {
        let statuses = f.service.loose_status();
        assert_eq!(statuses.len(), 1);
        statuses.into_iter().next().expect("one")
    };

    // nothing was read yet, so nothing can have changed since
    assert_eq!(state(&f).state, LooseState::Same);
    f.service.mark_seen(&id);
    assert_eq!(state(&f).state, LooseState::Same);

    // someone writes it
    fs::write(&file, "# Proposal\nwith an addition\n").expect("write");
    let changed = state(&f);
    assert_eq!(changed.state, LooseState::Changed);
    assert_ne!(changed.stamp, "");
    // reading it again is seeing it
    f.service.mark_seen(&id);
    assert_eq!(state(&f).state, LooseState::Same);

    // renamed in its folder: the same file under another name, with its pin
    let renamed = there.dir.path().join("proposal v2.md");
    fs::rename(&file, &renamed).expect("rename");
    let followed = state(&f);
    assert_eq!(followed.state, LooseState::Same);
    assert_eq!(followed.was.as_deref(), Some(path.as_str()));
    assert_eq!(followed.path, format!("@file/{id}/proposal v2.md"));
    let now = f.service.list();
    assert_eq!(now.notes.len(), 1);
    assert_eq!(now.notes[0].path, followed.path);
    assert!(now.notes[0].pinned, "the pin follows the file");
    assert_eq!(state(&f).was, None, "followed once");

    // gone
    fs::remove_file(&renamed).expect("remove");
    assert_eq!(state(&f).state, LooseState::Missing);
    let row = entry(&f, &followed.path);
    assert!(row.missing && row.pinned, "it stays in the list");
}

#[test]
fn a_file_that_is_missing_can_be_pointed_at_another_one_and_keeps_its_place() {
    let f = fx();
    let there = Elsewhere::new();
    let old = there.file("proposal.md", "# Old\n");
    let path = open_file(&f, &old, T0);
    f.service.set_pinned(&path, true).expect("pin");
    fs::remove_file(&old).expect("remove");
    let elsewhere = there.file("moved.md", "# Moved\n");
    let located = f
        .service
        .locate_file(&path, &elsewhere, T0 + MIN)
        .expect("locate");
    assert_eq!(located.kind, EntryKind::File);
    assert!(located
        .path
        .starts_with(&format!("@file/{}/", loose::id_of(&path).expect("an id"))));
    let row = entry(&f, &located.path);
    assert_eq!(
        (row.title.as_str(), row.pinned, row.missing),
        ("moved.md", true, false)
    );

    // a file that another entry has is not taken twice
    let other = open_file(&f, &there.file("other.md", "x"), T0 + 2 * MIN);
    let error = f
        .service
        .locate_file(&other, &elsewhere, T0 + 3 * MIN)
        .expect_err("already listed");
    assert_eq!(error.kind(), "exists");
}

#[test]
fn removing_a_file_from_the_list_keeps_the_file_and_forgets_what_was_kept_for_it() {
    let f = fx();
    let there = Elsewhere::new();
    let file = there.file("proposal.md", "# Proposal\n");
    let path = open_file(&f, &file, T0);
    f.service.set_pinned(&path, true).expect("pin");
    f.service.opened(&path);
    f.service
        .save_pending(&path, "typed", "base", T0)
        .expect("keep");
    f.service.remove_file(&path).expect("remove");
    assert!(file.is_file(), "the file is not touched");
    let list = f.service.list();
    assert_eq!(list.notes.len(), 0);
    assert_eq!(list.open, None);
    assert_eq!(
        f.service.load_pending(&path).err().map(|e| e.kind()),
        Some("notANote")
    );
    // listed again later, it is a file with nothing kept: not pinned, no text
    let again = open_file(&f, &file, T0 + MIN);
    let row = entry(&f, &again);
    assert!(!row.pinned && !row.unsaved);
}

#[test]
fn the_pins_the_places_and_the_open_file_are_kept_under_the_file_not_its_name() {
    let f = fx();
    let there = Elsewhere::new();
    let note = f.new_note("# A note");
    let one = open_file(&f, &there.file("one.md", "1\n"), T0);
    let two = open_file(&f, &there.file("two.md", "2\n"), T0 + MIN);
    f.service.set_pinned(&two, true).expect("pin");
    f.service.set_pinned(&note, true).expect("pin the note");
    f.service.set_pinned(&one, true).expect("pin");
    assert_eq!(
        f.service
            .list()
            .notes
            .iter()
            .map(|n| n.title.clone())
            .collect::<Vec<_>>(),
        ["two.md", "A note", "one.md"],
        "in the order they were pinned"
    );
    f.service
        .set_pin_order(&[one.clone(), two.clone(), note.clone()])
        .expect("order");
    assert_eq!(titles(&f), ["one.md", "two.md", "A note"]);

    let view = NoteView {
        mode: Some("read".to_owned()),
        caret: None,
        scroll: Some(12.0),
    };
    f.service.set_view(&two, view.clone());
    f.service.opened(&two);
    let list = f.service.list();
    assert_eq!(list.open.as_deref(), Some(two.as_str()));
    assert_eq!(entry(&f, &two).view, Some(view));
    f.service.set_pinned(&two, false).expect("unpin");
    assert!(!entry(&f, &two).pinned);
}

#[test]
fn a_file_that_is_gone_can_be_written_again_where_it_was_and_nothing_is_overwritten() {
    let f = fx();
    let there = Elsewhere::new();
    let file = there.file("proposal.md", "# Old\n");
    let path = open_file(&f, &file, T0);
    f.service
        .save_pending(&path, "# What I typed\n", "base", T0)
        .expect("keep");
    fs::remove_file(&file).expect("remove");
    let created = f
        .service
        .recreate_file(&path, "# What I typed\n")
        .expect("write it again");
    assert_eq!(created.path, path);
    assert_eq!(fs::read_to_string(&file).expect("read"), "# What I typed\n");
    assert_eq!(f.service.load_pending(&path).expect("ask"), None);
    // it is there now: a second time would overwrite it
    let error = f
        .service
        .recreate_file(&path, "# Another\n")
        .expect_err("it exists");
    assert_eq!(error.kind(), "exists");
    assert_eq!(fs::read_to_string(&file).expect("read"), "# What I typed\n");
}

#[test]
fn the_text_of_a_file_that_is_set_aside_becomes_a_note_named_after_it() {
    let f = fx();
    let there = Elsewhere::new();
    let path = open_file(&f, &there.file("proposal.md", "# Proposal\n"), T0);
    f.service
        .save_pending(&path, "my words", "base", T0)
        .expect("keep");
    let note = f
        .service
        .keep_edits_as_note(&path, "my words")
        .expect("a note");
    assert_eq!(note, "proposal (my edits).md");
    assert_eq!(f.read(&note), "my words");
    assert_eq!(f.service.load_pending(&path).expect("ask"), None);
    let second = f
        .service
        .keep_edits_as_note(&path, "more words")
        .expect("another note");
    assert_eq!(second, "proposal (my edits) 2.md", "nothing is overwritten");
}

#[test]
fn a_second_window_reads_the_list_and_changes_nothing() {
    let f = fx();
    let there = Elsewhere::new();
    let file = there.file("proposal.md", "# Proposal\n");
    let path = open_file(&f, &file, T0);
    let (second, report) = open_in(f.dir.path(), &f.bin, &|_| true);
    assert_eq!(report.read_only, Some(ReadOnlyReason::Locked));
    assert_eq!(second.list().notes.len(), 1, "the list is there to read");
    assert_eq!(second.loose_status().len(), 1);
    let kind_of = |error: NotesError| error.kind();
    assert_eq!(
        second.open_file(&file, T0).map_err(kind_of),
        Err("notesLocked")
    );
    assert_eq!(
        second.save_pending(&path, "x", "h", T0).map_err(kind_of),
        Err("notesLocked")
    );
    assert_eq!(
        second.save_file(&path, "x", "h").map_err(kind_of),
        Err("notesLocked")
    );
    assert_eq!(
        second.remove_file(&path).map_err(kind_of),
        Err("notesLocked")
    );
    assert_eq!(fs::read_to_string(&file).expect("read"), "# Proposal\n");
}

#[test]
fn the_folder_of_a_file_is_shown_from_the_home_folder_when_it_is_inside() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = tempfile::tempdir().expect("home");
    fs::create_dir(home.path().join("Downloads")).expect("mkdir");
    let file = home.path().join("Downloads").join("proposal.md");
    fs::write(&file, "# P\n").expect("write");
    let bin = Arc::new(FakeBin::default());
    let recycler: Arc<dyn Recycler> = bin;
    let vault = Arc::new(Vault::open(dir.path()).expect("open"));
    let canonical_home = dunce::canonicalize(home.path()).expect("canonical");
    let (service, _) = NotesService::open(
        vault,
        Arc::new(OwnWrites::new()),
        recycler,
        Some(canonical_home),
        T0,
        &|_| true,
    );
    service.open_file(&file, T0).expect("open");
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(
        service.list().notes[0].folder.as_deref(),
        Some(format!("~{sep}Downloads").as_str())
    );
}
