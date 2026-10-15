use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use tempfile::TempDir;

use super::*;
use crate::notes::history::MIN_GAP_MS;

/// 2026-10-15 14:32:00 UTC.
const T0: u64 = 1_792_074_720_000;
const MIN: u64 = 60 * 1000;
const MOMENT: Moment = Moment {
    year: 2026,
    month: 10,
    day: 5,
    hour: 9,
    minute: 7,
};

/// A recycle bin that deletes and remembers what it was given.
#[derive(Default)]
struct FakeBin {
    given: Mutex<Vec<PathBuf>>,
    calls: AtomicUsize,
}

impl Recycler for FakeBin {
    fn recycle(&self, paths: &[PathBuf]) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        for path in paths {
            fs::remove_file(path).map_err(|e| e.to_string())?;
            lock(&self.given).push(path.clone());
        }
        Ok(())
    }
}

struct Fx {
    dir: TempDir,
    service: NotesService,
    report: OpenReport,
    bin: Arc<FakeBin>,
}

fn open_in(
    dir: &Path,
    bin: &Arc<FakeBin>,
    is_running: &dyn Fn(u32) -> bool,
) -> (NotesService, OpenReport) {
    let vault = Arc::new(Vault::open(dir).expect("open the folder"));
    let recycler: Arc<dyn Recycler> = bin.clone();
    NotesService::open(vault, Arc::new(OwnWrites::new()), recycler, T0, is_running)
}

fn fx() -> Fx {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = Arc::new(FakeBin::default());
    let (service, report) = open_in(dir.path(), &bin, &|_| true);
    Fx {
        dir,
        service,
        report,
        bin,
    }
}

impl Fx {
    fn path(&self, name: &str) -> PathBuf {
        self.service.root().join(name)
    }

    fn write(&self, name: &str, text: &str) {
        fs::write(self.path(name), text).expect("write");
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.path(name)).expect("read")
    }

    fn set_modified(&self, name: &str, ms: u64) {
        let file = fs::OpenOptions::new()
            .write(true)
            .open(self.path(name))
            .expect("open");
        file.set_modified(UNIX_EPOCH + Duration::from_millis(ms))
            .expect("set the time");
    }

    fn hash(&self, name: &str) -> String {
        hash_hex(&fs::read(self.path(name)).expect("read"))
    }

    fn save(&self, name: &str, text: &str, expected: &str, now: u64) -> NoteSaved {
        self.service
            .save(name, text, expected, now, MOMENT)
            .expect("save")
    }

    /// Makes a note the way the window does: a new name, then a first save.
    fn new_note(&self, text: &str) -> String {
        let name = self.service.reserve_new(MOMENT).expect("reserve");
        match self.save(&name, text, "", T0) {
            NoteSaved::Saved { path, .. } => path,
            other => panic!("not saved: {other:?}"),
        }
    }

    fn paths(&self) -> Vec<String> {
        self.service
            .list()
            .notes
            .into_iter()
            .map(|n| n.path)
            .collect()
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.service.root())
            .expect("ls")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .filter(|name| name != ".trailmark")
            .collect();
        names.sort();
        names
    }
}

// ---- names and new notes ---------------------------------------------------------------------------------

#[test]
fn a_new_note_gets_a_name_but_no_file_until_it_has_text() {
    let f = fx();
    let name = f.service.reserve_new(MOMENT).expect("reserve");
    assert_eq!(name, "2026-10-05 0907.md");
    assert!(f.service.is_unborn(&name));
    assert_eq!(f.files(), Vec::<String>::new(), "no file yet");

    // blank characters make no file
    for blank in ["", "   ", "\n\n\t \n", "\u{feff}"] {
        assert_eq!(f.save(&name, blank, "", T0), NoteSaved::Blank);
    }
    assert_eq!(f.files(), Vec::<String>::new());

    // the first character that is not blank does
    let NoteSaved::Saved {
        path,
        hash,
        created,
        ..
    } = f.save(&name, "\n  # Hello", "", T0)
    else {
        panic!("saved");
    };
    assert_eq!((path.as_str(), created), (name.as_str(), true));
    assert_eq!(f.read(&name), "\n  # Hello");
    assert_eq!(hash, f.hash(&name));
    assert!(!f.service.is_unborn(&name), "it has a file now");
    // and from then on it is an ordinary note
    let NoteSaved::Saved { created, .. } = f.save(&name, "# Hello, again", &hash, T0 + 3000) else {
        panic!("saved");
    };
    assert!(!created);
}

#[test]
fn two_notes_in_the_same_minute_do_not_share_a_name() {
    let f = fx();
    let first = f.service.reserve_new(MOMENT).expect("reserve");
    let second = f.service.reserve_new(MOMENT).expect("reserve");
    assert_eq!(
        (first.as_str(), second.as_str()),
        ("2026-10-05 0907.md", "2026-10-05 0907-2.md")
    );
    assert!(matches!(
        f.save(&second, "second", "", T0),
        NoteSaved::Saved { .. }
    ));
    // the first is still reserved, so a third does not take its name
    assert_eq!(
        f.service.reserve_new(MOMENT).expect("reserve"),
        "2026-10-05 0907-3.md"
    );
    // a note that is forgotten gives its name back
    f.service.release_new(&first);
    assert_eq!(f.service.reserve_new(MOMENT).expect("reserve"), first);
}

#[test]
fn a_name_taken_between_the_reservation_and_the_save_gives_the_next_free_one() {
    let f = fx();
    let name = f.service.reserve_new(MOMENT).expect("reserve");
    // another program made a note with that name meanwhile
    f.write(&name, "theirs");
    let NoteSaved::Saved { path, created, .. } = f.save(&name, "mine", "", T0) else {
        panic!("saved");
    };
    assert_eq!(path, "2026-10-05 0907-2.md");
    assert!(created);
    assert_eq!(f.read(&name), "theirs", "nothing of theirs was touched");
    assert_eq!(f.read(&path), "mine");
}

#[test]
fn only_notes_directly_in_the_folder_are_notes() {
    let f = fx();
    fs::create_dir(f.path("sub")).expect("mkdir");
    f.write("sub/x.md", "x");
    f.write("a.txt", "x");
    f.write(".hidden.md", "x");
    for bad in [
        "../x.md",
        "sub/x.md",
        "a.txt",
        ".hidden.md",
        "..",
        "/abs.md",
    ] {
        let saved = f.service.save(bad, "text", "", T0, MOMENT);
        assert!(saved.is_err(), "{bad}");
        assert!(f.service.delete(&[bad.to_owned()]).is_err(), "{bad}");
        assert!(f.service.rename(bad, "x.md").is_err(), "{bad}");
    }
    assert!(f.service.versions("sub/x.md").is_err());
    assert_eq!(
        f.service.list().notes.len(),
        0,
        "the list has no notes in it"
    );
}

// ---- the list --------------------------------------------------------------------------------------------

#[test]
fn the_list_has_the_pinned_notes_first_in_the_persons_order_then_the_rest_by_last_change() {
    let f = fx();
    for (name, text, at) in [
        ("a.md", "# Alpha\n\nfirst note", T0 + MIN),
        ("b.md", "# Bravo", T0 + 5 * MIN),
        ("c.md", "# Charlie", T0 + 3 * MIN),
        ("d.md", "# Delta", T0 + 4 * MIN),
    ] {
        f.write(name, text);
        f.set_modified(name, at);
    }
    assert_eq!(
        f.paths(),
        ["b.md", "d.md", "c.md", "a.md"],
        "by last change"
    );
    f.service.set_pinned("c.md", true).expect("pin");
    f.service.set_pinned("a.md", true).expect("pin");
    assert_eq!(
        f.paths(),
        ["c.md", "a.md", "b.md", "d.md"],
        "pinned first, in the order they were pinned"
    );
    f.service
        .set_pin_order(&["a.md".to_owned(), "c.md".to_owned(), "b.md".to_owned()])
        .expect("order");
    assert_eq!(
        f.paths(),
        ["a.md", "c.md", "b.md", "d.md"],
        "b.md is not pinned, so the order ignores it"
    );
    let list = f.service.list();
    assert_eq!(list.notes[0].title, "Alpha");
    assert_eq!(list.notes[0].snippet, "first note");
    assert!(list.notes[0].pinned && !list.notes[2].pinned);
    f.service.set_pinned("a.md", false).expect("unpin");
    assert_eq!(f.paths(), ["c.md", "b.md", "d.md", "a.md"]);
}

#[test]
fn the_list_follows_the_files_and_what_is_kept_comes_back_with_them() {
    let f = fx();
    f.write("a.md", "# One");
    f.service.set_pinned("a.md", true).expect("pin");
    f.service.opened("a.md");
    f.service.set_view(
        "a.md",
        NoteView {
            mode: Some("source".to_owned()),
            caret: Some(3),
            scroll: Some(10.0),
        },
    );
    assert_eq!(f.service.list().notes[0].title, "One");
    // changed from outside: the cached title is not used
    f.write("a.md", "# Two, a longer title");
    assert_eq!(f.service.list().notes[0].title, "Two, a longer title");
    // a new window on the same folder remembers the pin, the open note and the view
    let Fx {
        dir, service, bin, ..
    } = f;
    drop(service);
    let (again, _) = open_in(dir.path(), &bin, &|_| true);
    let list = again.list();
    assert_eq!(list.open.as_deref(), Some("a.md"));
    assert!(list.notes[0].pinned);
    assert_eq!(
        list.notes[0].view,
        Some(NoteView {
            mode: Some("source".to_owned()),
            caret: Some(3),
            scroll: Some(10.0)
        })
    );
}

#[test]
fn a_missing_or_broken_notes_file_is_rebuilt_from_the_files() {
    let f = fx();
    f.write("a.md", "# One");
    f.write("b.md", "# Two");
    f.service.set_pinned("a.md", true).expect("pin");
    let Fx {
        dir, service, bin, ..
    } = f;
    drop(service);
    let data = NotesData::path(dir.path());
    assert!(data.is_file());
    for damage in ["{{{ broken", "", "[]"] {
        fs::write(&data, damage).expect("damage");
        let (again, _) = open_in(dir.path(), &bin, &|_| true);
        let list = again.list();
        assert_eq!(list.notes.len(), 2, "{damage}");
        assert!(
            list.notes.iter().all(|n| !n.pinned),
            "{damage}: the pins are lost, the notes are not"
        );
        assert_eq!(list.open, None);
        drop(again);
    }
    fs::remove_file(&data).expect("remove");
    let (again, _) = open_in(dir.path(), &bin, &|_| true);
    assert_eq!(again.list().notes.len(), 2);
}

#[test]
fn what_is_kept_about_a_note_that_is_gone_is_dropped_when_the_folder_opens() {
    let f = fx();
    f.write("a.md", "# One");
    f.write("b.md", "# Two");
    f.service.set_pinned("a.md", true).expect("pin");
    f.service.set_pinned("b.md", true).expect("pin");
    let Fx {
        dir, service, bin, ..
    } = f;
    drop(service);
    fs::remove_file(dir.path().join("a.md")).expect("remove");
    let (again, _) = open_in(dir.path(), &bin, &|_| true);
    assert_eq!(NotesData::load(dir.path()).pins, ["b.md"]);
    assert_eq!(again.list().notes.len(), 1);
}

#[test]
fn the_summary_for_the_welcome_screen_is_the_pinned_notes_then_the_three_latest() {
    let f = fx();
    for (n, name) in ["a.md", "b.md", "c.md", "d.md", "e.md"]
        .into_iter()
        .enumerate()
    {
        f.write(name, &format!("# Note {name}"));
        f.set_modified(name, T0 + n as u64 * MIN);
    }
    f.service.set_pinned("a.md", true).expect("pin");
    let summary = summary(f.service.root());
    assert_eq!(
        summary
            .pinned
            .iter()
            .map(|n| n.path.as_str())
            .collect::<Vec<_>>(),
        ["a.md"]
    );
    assert_eq!(
        summary
            .latest
            .iter()
            .map(|n| n.path.as_str())
            .collect::<Vec<_>>(),
        ["e.md", "d.md", "c.md"]
    );
    // a folder that is not there has nothing, and is not created by asking
    let missing = f.dir.path().join("nowhere");
    assert_eq!(super::summary(&missing).latest.len(), 0);
    assert!(!missing.exists());
}

// ---- saving without losing text -----------------------------------------------------------------------------

#[test]
fn a_note_that_changed_on_disk_is_not_overwritten_and_the_text_is_kept_as_a_copy() {
    let f = fx();
    let name = f.new_note("# Server IPs\n\napi 10.0.0.4");
    let started_from = f.hash(&name);
    // another computer, through a synced folder, saved it meanwhile
    f.write(&name, "# Server IPs\n\napi 10.0.0.9");
    let saved = f.save(
        &name,
        "# Server IPs\n\napi 10.0.0.4\n\ndb 10.0.0.5",
        &started_from,
        T0 + MIN,
    );
    let NoteSaved::Conflict { copy } = saved else {
        panic!("a conflict, not {saved:?}");
    };
    assert_eq!(copy, "Server IPs (conflict 2026-10-05 0907).md");
    assert_eq!(
        f.read(&name),
        "# Server IPs\n\napi 10.0.0.9",
        "their version is untouched"
    );
    assert_eq!(
        f.read(&copy),
        "# Server IPs\n\napi 10.0.0.4\n\ndb 10.0.0.5",
        "what the person has is kept"
    );
    // the same minute twice makes another copy, never overwrites the first
    let NoteSaved::Conflict { copy: second } =
        f.save(&name, "# Server IPs\n\nsecond", &started_from, T0 + MIN)
    else {
        panic!("a conflict");
    };
    assert_eq!(second, "Server IPs (conflict 2026-10-05 0907) 2.md");
    assert_eq!(f.read(&copy), "# Server IPs\n\napi 10.0.0.4\n\ndb 10.0.0.5");
}

#[test]
fn a_writer_that_gets_in_during_the_save_is_noticed_too() {
    let f = fx();
    let name = f.new_note("# Plan\n\none");
    let hash = f.hash(&name);
    let saved = f
        .service
        .save_with(
            &name,
            "# Plan\n\none\n\ntwo",
            &hash,
            T0 + MIN,
            MOMENT,
            || {
                // between the look at the file and the write
                fs::write(f.path(&name), "# Plan\n\nTHEIRS").expect("their write");
            },
        )
        .expect("save");
    let NoteSaved::Conflict { copy } = saved else {
        panic!("a conflict, not {saved:?}");
    };
    assert_eq!(f.read(&name), "# Plan\n\nTHEIRS");
    assert_eq!(f.read(&copy), "# Plan\n\none\n\ntwo");
    assert!(
        !f.files().iter().any(|n| n.contains(".part")),
        "no temporary file is left"
    );
}

#[test]
fn a_note_deleted_behind_the_editors_back_comes_back_with_its_text() {
    let f = fx();
    let name = f.new_note("# Keep me");
    let hash = f.hash(&name);
    fs::remove_file(f.path(&name)).expect("their delete");
    let NoteSaved::Saved { path, created, .. } =
        f.save(&name, "# Keep me, edited", &hash, T0 + MIN)
    else {
        panic!("saved");
    };
    assert_eq!((path.as_str(), created), (name.as_str(), true));
    assert_eq!(f.read(&name), "# Keep me, edited");
}

#[test]
fn saving_what_is_already_there_writes_nothing() {
    let f = fx();
    let name = f.new_note("# Same");
    let hash = f.hash(&name);
    let before = fs::metadata(f.path(&name))
        .expect("meta")
        .modified()
        .expect("time");
    let NoteSaved::Saved { hash: again, .. } = f.save(&name, "# Same", &hash, T0 + MIN) else {
        panic!("saved");
    };
    assert_eq!(again, hash);
    assert_eq!(
        fs::metadata(f.path(&name))
            .expect("meta")
            .modified()
            .expect("time"),
        before
    );
    assert!(
        f.service.versions(&name).expect("versions").is_empty(),
        "no change, no version"
    );
}

#[test]
fn a_second_program_with_the_folder_open_leaves_this_one_read_only() {
    let f = fx();
    assert_eq!(f.report.read_only, None);
    f.write("a.md", "# One");
    let bin = Arc::new(FakeBin::default());
    let (second, report) = open_in(f.service.root(), &bin, &|_| true);
    assert_eq!(report.read_only, Some(ReadOnlyReason::Locked));
    // it can read...
    assert_eq!(second.list().notes.len(), 1);
    // ...and nothing else
    let hash = f.hash("a.md");
    assert!(matches!(
        second.save("a.md", "# Changed", &hash, T0, MOMENT),
        Err(NotesError::Locked)
    ));
    assert!(matches!(
        second.reserve_new(MOMENT),
        Err(NotesError::Locked)
    ));
    assert!(matches!(
        second.delete(&["a.md".to_owned()]),
        Err(NotesError::Locked)
    ));
    assert!(matches!(
        second.rename("a.md", "b.md"),
        Err(NotesError::Locked)
    ));
    assert!(matches!(
        second.set_pinned("a.md", true),
        Err(NotesError::Locked)
    ));
    assert_eq!(f.read("a.md"), "# One");
    assert_eq!(bin.calls.load(Ordering::SeqCst), 0);
    // the first one is not in the way of itself
    assert!(f.service.set_pinned("a.md", true).is_ok());
}

// ---- after a crash ---------------------------------------------------------------------------------------

#[test]
fn text_a_crash_left_in_a_temporary_file_is_kept_as_a_note_and_the_file_stays() {
    let f = fx();
    let name = f.new_note("# Weekly checklist\n\n- backups");
    f.set_modified(&name, T0 - 10 * MIN);
    let temp = format!(".{name}.4242.7.part");
    f.write(
        &temp,
        "# Weekly checklist\n\n- backups\n- patches (typed just before the crash)",
    );
    let Fx {
        dir, service, bin, ..
    } = f;
    drop(service);

    // the process that was writing it is still running: not ours to touch
    let (running, report) = open_in(dir.path(), &bin, &|pid| pid == 4242);
    assert_eq!(report.recovered, Vec::<String>::new());
    drop(running);

    // it is gone
    let (again, report) = open_in(dir.path(), &bin, &|_| false);
    assert_eq!(report.recovered, ["Weekly checklist (recovered).md"]);
    let recovered =
        fs::read_to_string(dir.path().join("Weekly checklist (recovered).md")).expect("read");
    assert!(recovered.ends_with("(typed just before the crash)"));
    assert_eq!(
        fs::read_to_string(dir.path().join(&name)).expect("read"),
        "# Weekly checklist\n\n- backups",
        "the note itself is not changed"
    );
    assert!(
        !dir.path().join(&temp).exists(),
        "the temporary file is renamed"
    );
    assert!(
        dir.path()
            .join(format!(".{name}.4242.7.recovered"))
            .is_file(),
        "and never deleted"
    );
    drop(again);

    // opened again, it is not recovered twice
    let (_third, report) = open_in(dir.path(), &bin, &|_| false);
    assert_eq!(report.recovered, Vec::<String>::new());
}

#[test]
fn a_temporary_file_older_than_its_note_or_with_nothing_new_is_left_alone() {
    let f = fx();
    let name = f.new_note("# Plan\n\nnewest text");
    let temp_old = format!(".{name}.1.1.part");
    f.write(&temp_old, "# Plan\n\nolder text");
    f.set_modified(&temp_old, T0 - 60 * MIN);
    let temp_same = format!(".{name}.2.1.part");
    f.write(&temp_same, "# Plan\n\nnewest text");
    f.set_modified(&temp_same, T0 + 60 * MIN);
    let temp_blank = format!(".{name}.3.1.part");
    f.write(&temp_blank, "   ");
    f.set_modified(&temp_blank, T0 + 60 * MIN);
    let Fx {
        dir, service, bin, ..
    } = f;
    drop(service);
    let (_again, report) = open_in(dir.path(), &bin, &|_| false);
    assert_eq!(report.recovered, Vec::<String>::new());
    for temp in [temp_old, temp_same, temp_blank] {
        assert!(dir.path().join(temp).is_file(), "never deleted");
    }
}

// ---- earlier versions --------------------------------------------------------------------------------------

#[test]
fn a_version_is_kept_at_the_first_save_after_opening_and_then_every_ten_minutes() {
    let f = fx();
    let name = f.new_note("one");
    f.service.opened(&name);
    let mut hash = f.hash(&name);
    let mut save = |text: &str, now: u64| {
        let NoteSaved::Saved { hash: next, .. } = f.save(&name, text, &hash, now) else {
            panic!("saved");
        };
        hash = next;
    };
    f.set_modified(&name, T0);
    save("two", T0 + 1000);
    assert_eq!(
        f.service.versions(&name).expect("versions").len(),
        1,
        "the first save after opening"
    );
    save("three", T0 + 4000);
    save("four", T0 + 7000);
    assert_eq!(
        f.service.versions(&name).expect("versions").len(),
        1,
        "typing on does not make more"
    );
    save("five", T0 + 1000 + MIN_GAP_MS - 1);
    assert_eq!(f.service.versions(&name).expect("versions").len(), 1);
    save("six", T0 + 1000 + MIN_GAP_MS);
    let versions = f.service.versions(&name).expect("versions");
    assert_eq!(versions.len(), 2, "ten minutes after the last one");
    // a version is the text a save replaced
    assert_eq!(
        f.service
            .version_text(&name, &versions[1].id)
            .expect("text"),
        "one"
    );
    assert_eq!(
        f.service
            .version_text(&name, &versions[0].id)
            .expect("text"),
        "five"
    );
    // opening the note again starts over
    f.service.opened(&name);
    save("seven", T0 + 1000 + MIN_GAP_MS + 1000);
    assert_eq!(f.service.versions(&name).expect("versions").len(), 3);
}

#[test]
fn restoring_a_version_keeps_the_current_text_as_a_version_first() {
    let f = fx();
    let name = f.new_note("original");
    f.service.opened(&name);
    let hash = f.hash(&name);
    f.set_modified(&name, T0);
    f.save(&name, "rewritten", &hash, T0 + 1000);
    let original = f.service.versions(&name).expect("versions").remove(0);
    assert_eq!(
        f.service.version_text(&name, &original.id).expect("text"),
        "original"
    );

    let restored = f
        .service
        .restore_version(&name, &original.id, T0 + 5 * MIN)
        .expect("restore");
    assert_eq!(f.read(&name), "original");
    assert_eq!(restored.hash, f.hash(&name));
    // the rewritten text is a version now, so the restore can be undone
    let versions = f.service.versions(&name).expect("versions");
    let texts: Vec<String> = versions
        .iter()
        .map(|v| f.service.version_text(&name, &v.id).expect("text"))
        .collect();
    assert!(texts.contains(&"rewritten".to_owned()), "{texts:?}");
    let undo = versions
        .iter()
        .find(|v| {
            f.service
                .version_text(&name, &v.id)
                .is_ok_and(|t| t == "rewritten")
        })
        .expect("the version");
    f.service
        .restore_version(&name, &undo.id, T0 + 6 * MIN)
        .expect("undo the restore");
    assert_eq!(f.read(&name), "rewritten");
    assert!(f
        .service
        .restore_version(&name, "20200101-000000-000", T0)
        .is_err());
    assert!(f.service.restore_version(&name, "../../x", T0).is_err());
}

// ---- deleting, renaming and the rest ---------------------------------------------------------------------------

#[test]
fn a_deleted_note_goes_to_the_bin_and_comes_back_with_its_text_and_its_time() {
    let f = fx();
    f.write("a.md", "# Keep\n\nthis text");
    f.set_modified("a.md", T0 - 3 * 60 * MIN);
    f.write("b.md", "# Other");
    let deleted = f
        .service
        .delete(&["a.md".to_owned(), "a.md".to_owned()])
        .expect("delete");
    assert_eq!(deleted.deleted, ["a.md"], "once");
    assert!(!f.path("a.md").exists());
    assert_eq!(lock(&f.bin.given).len(), 1, "it went to the bin");
    let undo = deleted.undo_id.expect("it can be undone");

    let back = f.service.undo_delete(undo).expect("undo");
    assert_eq!(back, ["a.md"]);
    assert_eq!(f.read("a.md"), "# Keep\n\nthis text");
    let modified = modified_of(&f.path("a.md")).expect("time");
    assert!(modified.abs_diff(T0 - 3 * 60 * MIN) < 2000, "{modified}");
    assert!(
        matches!(f.service.undo_delete(undo), Err(NotesError::UnknownUndo)),
        "once"
    );
    assert!(f.service.delete(&["missing.md".to_owned()]).is_err());
}

#[test]
fn a_note_whose_name_was_taken_meanwhile_comes_back_under_another_name() {
    let f = fx();
    f.write("a.md", "# Mine");
    let undo = f
        .service
        .delete(&["a.md".to_owned()])
        .expect("delete")
        .undo_id
        .expect("undo");
    f.write("a.md", "# Someone else's");
    let back = f.service.undo_delete(undo).expect("undo");
    assert_eq!(back, ["a (restored).md"]);
    assert_eq!(f.read("a.md"), "# Someone else's");
    assert_eq!(f.read("a (restored).md"), "# Mine");
}

#[test]
fn the_copied_mark_goes_when_the_note_is_edited_and_delete_copied_keeps_the_pinned() {
    let f = fx();
    for name in ["a.md", "b.md", "c.md"] {
        f.write(name, &format!("# {name}"));
        f.service.mark_copied(name).expect("copied");
    }
    f.service.set_pinned("b.md", true).expect("pin");
    assert!(f.service.list().notes.iter().all(|n| n.copied));
    // c.md is edited: it is no longer a copy of what was copied
    let hash = f.hash("c.md");
    f.save("c.md", "# c.md, edited", &hash, T0);
    let list = f.service.list();
    assert_eq!(
        list.notes
            .iter()
            .filter(|n| n.copied)
            .map(|n| n.path.as_str())
            .collect::<Vec<_>>(),
        ["b.md", "a.md"]
    );
    let deleted = f.service.delete_copied().expect("delete copied");
    assert_eq!(
        deleted.deleted,
        ["a.md"],
        "not the pinned one, not the edited one"
    );
    assert!(f.path("b.md").is_file() && f.path("c.md").is_file());
    assert_eq!(
        f.service.delete_copied().expect("nothing left").undo_id,
        None
    );
}

#[test]
fn a_note_gets_a_real_name_and_keeps_what_was_kept_for_it() {
    let f = fx();
    let name = f.new_note("# Plan");
    f.service.opened(&name);
    f.service.set_pinned(&name, true).expect("pin");
    let hash = f.hash(&name);
    f.save(&name, "# Plan, v2", &hash, T0 + 1000);
    assert_eq!(f.service.versions(&name).expect("versions").len(), 1);

    let renamed = f.service.rename(&name, "Launch plan").expect("rename");
    assert_eq!(renamed, "Launch plan.md");
    assert!(!f.path(&name).exists());
    assert_eq!(f.read("Launch plan.md"), "# Plan, v2");
    let list = f.service.list();
    assert!(list.notes[0].pinned && list.notes[0].path == "Launch plan.md");
    assert_eq!(list.open.as_deref(), Some("Launch plan.md"));
    assert_eq!(
        f.service
            .versions("Launch plan.md")
            .expect("versions")
            .len(),
        1
    );
    assert_eq!(
        f.service.versions(&name).expect("versions"),
        Vec::<NoteVersion>::new()
    );
}

#[test]
fn a_rename_never_overwrites_and_refuses_names_the_systems_refuse() {
    let f = fx();
    f.write("a.md", "# A");
    f.write("b.md", "# B");
    assert!(matches!(
        f.service.rename("a.md", "b.md"),
        Err(NotesError::Exists(_))
    ));
    assert!(matches!(
        f.service.rename("a.md", "b"),
        Err(NotesError::Exists(_))
    ));
    for bad in [
        "",
        "  ",
        "a/b",
        "a\\b",
        "x:y",
        "con",
        ".hidden",
        "trailing.",
        "..",
        "../x",
    ] {
        assert!(f.service.rename("a.md", bad).is_err(), "{bad:?}");
    }
    assert_eq!(
        (f.read("a.md").as_str(), f.read("b.md").as_str()),
        ("# A", "# B")
    );
    // the same name again is not an error and not a change
    assert_eq!(f.service.rename("a.md", "a.md").expect("same"), "a.md");
    // only the case changes
    assert_eq!(f.service.rename("a.md", "A.md").expect("case"), "A.md");
    assert!(f.files().contains(&"A.md".to_owned()));
}

#[test]
fn a_note_that_was_never_written_in_is_removed_unless_it_is_pinned_or_has_text() {
    let f = fx();
    f.write("blank.md", "  \n\t\n");
    f.write("text.md", "# Words");
    f.write("pinned.md", "   ");
    f.service.set_pinned("pinned.md", true).expect("pin");
    assert!(f.service.discard_blank("blank.md").expect("discard"));
    assert!(!f.path("blank.md").exists());
    assert!(!f.service.discard_blank("text.md").expect("discard"));
    assert!(!f.service.discard_blank("pinned.md").expect("discard"));
    assert!(!f.service.discard_blank("gone.md").expect("discard"));
    assert_eq!(f.files(), ["pinned.md", "text.md"]);
}

#[test]
fn a_note_that_is_too_big_is_refused_with_its_name() {
    let f = fx();
    let name = f.service.reserve_new(MOMENT).expect("reserve");
    let huge = "x".repeat(usize::try_from(MAX_RENDER_BYTES).expect("fits") + 1);
    let error = f
        .service
        .save(&name, &huge, "", T0, MOMENT)
        .expect_err("too big");
    assert_eq!(error.kind(), "tooLarge");
    assert_eq!(f.files(), Vec::<String>::new());
}
