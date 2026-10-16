use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::watch::ChangeKind;

const NOTHING: [&str; 0] = [];

/// Waits are not part of these tests: the thread answers `flush` only after everything sent before it, so
/// a flush is how a test waits, and a write that waits an hour is one that cannot have happened by itself.
const NEVER: Timing = Timing {
    quiet: Duration::from_secs(3600),
    max_wait: Duration::from_secs(7200),
    retry: Duration::from_secs(3600),
};

struct Fx {
    project: tempfile::TempDir,
    state: tempfile::TempDir,
    heard: Arc<Mutex<Vec<Vec<String>>>>,
}

fn change(kind: ChangeKind, path: &str) -> FsChange {
    FsChange {
        kind,
        path: path.to_owned(),
        from: None,
        at_ms: 0,
    }
}

fn renamed(from: &str, to: &str) -> FsChange {
    FsChange {
        kind: ChangeKind::Renamed,
        path: to.to_owned(),
        from: Some(from.to_owned()),
        at_ms: 0,
    }
}

impl Fx {
    fn new(files: &[(&str, &str)]) -> Self {
        let fx = Self {
            project: tempfile::tempdir().expect("tempdir"),
            state: tempfile::tempdir().expect("tempdir"),
            heard: Arc::default(),
        };
        for (path, text) in files {
            fx.write(path, text);
        }
        fx
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.project.path().join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    fn remove(&self, rel: &str) {
        fs::remove_file(self.project.path().join(rel)).expect("remove");
    }

    fn read_path(&self) -> PathBuf {
        self.state.path().join("read.json")
    }

    fn open(&self) -> UnreadService {
        self.open_timed(NEVER)
    }

    fn open_timed(&self, timing: Timing) -> UnreadService {
        let vault = Arc::new(Vault::open(self.project.path()).expect("vault"));
        self.service(vault, self.read_path(), timing)
    }

    fn service(&self, vault: Arc<Vault>, read_path: PathBuf, timing: Timing) -> UnreadService {
        let filter = Arc::new(PathFilter::new(&[(vault.root().to_path_buf(), None)], &[]));
        let heard = Arc::clone(&self.heard);
        let service = UnreadService::open(
            vault,
            filter,
            read_path,
            timing,
            Arc::new(move |unread: &[String]| lock(&heard).push(unread.to_vec())),
            None,
        );
        // the first look at the disk is done once the thread answers
        service.flush();
        service
    }

    /// What `read.json` holds, with the hashes as written.
    fn saved(&self) -> Option<BTreeMap<String, String>> {
        let bytes = fs::read(self.read_path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn saved_paths(&self) -> Vec<String> {
        self.saved()
            .map(|m| m.into_keys().collect())
            .unwrap_or_default()
    }

    fn last_heard(&self) -> Option<Vec<String>> {
        lock(&self.heard).last().cloned()
    }
}

fn hex_of(text: &str) -> String {
    hash_to_hex(hash_bytes(text.as_bytes()))
}

#[test]
fn the_first_visit_counts_every_markdown_document_as_read_and_writes_the_baseline() {
    let fx = Fx::new(&[
        ("a.md", "# A\n"),
        ("docs/b.md", "# B\n"),
        ("notes.txt", "text"),
        ("logo.png", "png"),
    ]);
    let service = fx.open();
    assert_eq!(service.unread(), NOTHING);
    assert_eq!(
        *lock(&fx.heard),
        Vec::<Vec<String>>::new(),
        "nothing changed for the UI"
    );
    assert_eq!(fx.saved_paths(), ["a.md", "docs/b.md"]);
    let saved = fx.saved().expect("saved");
    assert_eq!(saved["a.md"], hex_of("# A\n"), "16 hex digits of xxh3");
    service.stop();
}

#[test]
fn files_created_later_start_unread_and_the_listener_hears_it() {
    let fx = Fx::new(&[("a.md", "# A\n")]);
    let service = fx.open();
    fx.write("new.md", "# New\n");
    service.apply(&[change(ChangeKind::Created, "new.md")]);
    service.flush();
    assert_eq!(service.unread(), ["new.md"]);
    assert_eq!(fx.last_heard(), Some(vec!["new.md".to_owned()]));
    assert_eq!(
        fx.saved_paths(),
        ["a.md"],
        "an unread file is not in the file"
    );
    service.stop();
}

#[test]
fn a_change_the_disk_does_not_show_is_not_a_change() {
    let fx = Fx::new(&[("a.md", "# A\n")]);
    let service = fx.open();
    // the watcher said modified, but the content is what was read
    service.apply(&[change(ChangeKind::Modified, "a.md")]);
    service.flush();
    assert_eq!(service.unread(), NOTHING);
    assert_eq!(*lock(&fx.heard), Vec::<Vec<String>>::new());
    service.stop();
}

#[test]
fn a_file_changed_and_then_put_back_is_read_again() {
    let fx = Fx::new(&[("a.md", "one\n")]);
    let service = fx.open();
    fx.write("a.md", "two\n");
    service.apply(&[change(ChangeKind::Modified, "a.md")]);
    service.flush();
    assert_eq!(service.unread(), ["a.md"]);
    fx.write("a.md", "one\n");
    service.apply(&[change(ChangeKind::Modified, "a.md")]);
    service.flush();
    assert_eq!(service.unread(), NOTHING);
    assert_eq!(
        *lock(&fx.heard),
        [vec!["a.md".to_owned()], Vec::<String>::new()]
    );
    service.stop();
}

#[test]
fn what_changed_while_the_app_was_closed_is_unread_on_the_next_visit() {
    let fx = Fx::new(&[("a.md", "one\n"), ("b.md", "two\n"), ("keep.md", "same\n")]);
    fx.open().stop();
    assert_eq!(fx.saved_paths(), ["a.md", "b.md", "keep.md"]);

    fx.write("a.md", "one, edited\n");
    fx.remove("b.md");
    fx.write("c.md", "three\n");
    let service = fx.open();
    assert_eq!(service.unread(), ["a.md", "c.md"]);
    assert_eq!(fx.saved_paths(), ["a.md", "keep.md"].map(String::from));
    service.stop();
}

#[test]
fn a_removed_document_is_dropped_and_a_removed_folder_takes_its_documents() {
    let fx = Fx::new(&[("a.md", "a"), ("docs/b.md", "b"), ("docs/deep/c.md", "c")]);
    let service = fx.open();
    fx.remove("a.md");
    service.apply(&[change(ChangeKind::Removed, "a.md")]);
    fs::remove_dir_all(fx.project.path().join("docs")).expect("rmdir");
    service.apply(&[change(ChangeKind::Removed, "docs")]);
    service.flush();
    assert_eq!(fx.saved_paths(), NOTHING);
    assert_eq!(service.unread(), NOTHING);
    service.stop();
}

#[test]
fn a_file_removed_and_written_again_in_one_batch_keeps_what_it_was() {
    let fx = Fx::new(&[("a.md", "one\n")]);
    let service = fx.open();
    // an agent saves by writing a temp file and moving it over: the batch can say gone, then new
    service.apply(&[
        change(ChangeKind::Removed, "a.md"),
        change(ChangeKind::Created, "a.md"),
    ]);
    service.flush();
    assert!(service.unread().is_empty(), "the content is what was read");
    service.stop();
}

#[test]
fn a_rescan_catches_up_with_the_disk_without_being_told_what_happened() {
    let fx = Fx::new(&[("a.md", "one\n"), ("b.md", "two\n")]);
    let service = fx.open();
    fx.write("a.md", "one, edited\n");
    fx.remove("b.md");
    fx.write("new.md", "new\n");
    service.rescan();
    service.flush();
    assert_eq!(service.unread(), ["a.md", "new.md"]);
    assert_eq!(fx.saved_paths(), ["a.md"]);
    service.stop();
}

#[test]
fn marking_read_stores_what_the_document_holds_now() {
    let fx = Fx::new(&[("a.md", "one\n")]);
    let service = fx.open();
    fx.write("a.md", "two\n");
    service.apply(&[change(ChangeKind::Modified, "a.md")]);
    service.flush();
    assert_eq!(service.unread(), ["a.md"]);

    service.mark_read("a.md").expect("mark");
    assert_eq!(service.unread(), NOTHING);
    assert_eq!(fx.last_heard(), Some(Vec::new()));
    service.flush();
    assert_eq!(fx.saved().expect("saved")["a.md"], hex_of("two\n"));
    service.stop();
}

#[test]
fn marking_read_sees_the_file_as_it_is_even_before_the_watcher_reports_it() {
    let fx = Fx::new(&[("a.md", "one\n")]);
    let service = fx.open();
    fx.write("a.md", "two\n");
    // no watcher event yet: the person read the new version
    service.mark_read("a.md").expect("mark");
    assert_eq!(service.unread(), NOTHING);
    service.apply(&[change(ChangeKind::Modified, "a.md")]);
    service.flush();
    assert!(
        service.unread().is_empty(),
        "the late report changes nothing"
    );
    service.stop();
}

#[test]
fn marking_unread_forgets_what_was_read() {
    let fx = Fx::new(&[("a.md", "one\n"), ("b.md", "two\n")]);
    let service = fx.open();
    service.mark_unread("a.md").expect("mark");
    assert_eq!(service.unread(), ["a.md"]);
    service.flush();
    assert_eq!(fx.saved_paths(), ["b.md"]);
    service.mark_read("a.md").expect("mark");
    assert_eq!(service.unread(), NOTHING);
    service.stop();
}

#[test]
fn mark_all_read_and_mark_folder_read() {
    let fx = Fx::new(&[("a.md", "a")]);
    let service = fx.open();
    for path in ["docs/b.md", "docs/deep/c.md", "other/d.md", "e.md"] {
        fx.write(path, "x");
        service.apply(&[change(ChangeKind::Created, path)]);
    }
    service.flush();
    assert_eq!(service.unread().len(), 4);

    service.mark_folder_read("docs").expect("folder");
    assert_eq!(service.unread(), ["e.md", "other/d.md"]);
    service.mark_all_read();
    assert_eq!(service.unread(), NOTHING);
    service.flush();
    assert_eq!(fx.saved_paths().len(), 5);
    service.stop();
}

#[test]
fn the_project_root_as_a_folder_is_everything() {
    let fx = Fx::new(&[("a.md", "a")]);
    let service = fx.open();
    fx.write("b.md", "b");
    service.apply(&[change(ChangeKind::Created, "b.md")]);
    service.flush();
    service.mark_folder_read("").expect("root");
    assert_eq!(service.unread(), NOTHING);
    service.stop();
}

#[test]
fn many_marks_are_written_once_things_settle_not_one_by_one() {
    let fx = Fx::new(&[("a.md", "a")]);
    let service = fx.open();
    for i in 0..30 {
        let path = format!("n{i}.md");
        fx.write(&path, "x");
        service.apply(&[change(ChangeKind::Created, &path)]);
    }
    service.flush();
    let baseline = fx.saved().expect("baseline");
    for i in 0..30 {
        service.mark_read(&format!("n{i}.md")).expect("mark");
    }
    // the settling time is an hour in this test: nothing has been written yet...
    assert_eq!(service.unread(), NOTHING);
    assert_eq!(fx.saved().expect("still"), baseline);
    // ...until something asks for it, and then all of it is in one file
    service.flush();
    assert_eq!(fx.saved_paths().len(), 31);
    service.stop();
}

#[test]
fn what_is_pending_is_written_when_the_service_stops() {
    let fx = Fx::new(&[("a.md", "a")]);
    let service = fx.open();
    service.mark_unread("a.md").expect("mark");
    assert_eq!(fx.saved_paths(), ["a.md"], "not written yet");
    service.stop();
    assert_eq!(fx.saved_paths(), NOTHING);
    service.stop();
}

#[test]
fn a_move_made_by_the_app_keeps_the_read_state() {
    let fx = Fx::new(&[("read.md", "r"), ("unread.md", "u"), ("dir/in.md", "i")]);
    let service = fx.open();
    service.mark_unread("unread.md").expect("mark");

    // the app moves the files and tells the service; the watcher reports it a moment later
    fs::create_dir_all(fx.project.path().join("moved")).expect("mkdir");
    fs::rename(
        fx.project.path().join("read.md"),
        fx.project.path().join("moved/read.md"),
    )
    .expect("move");
    fs::rename(
        fx.project.path().join("unread.md"),
        fx.project.path().join("moved/unread.md"),
    )
    .expect("move");
    fs::rename(
        fx.project.path().join("dir"),
        fx.project.path().join("moved/dir"),
    )
    .expect("move");
    service.renamed("read.md", "moved/read.md");
    service.renamed("unread.md", "moved/unread.md");
    service.renamed("dir", "moved/dir");
    service.apply(&[
        renamed("read.md", "moved/read.md"),
        renamed("unread.md", "moved/unread.md"),
        renamed("dir/in.md", "moved/dir/in.md"),
    ]);
    service.flush();
    assert_eq!(service.unread(), ["moved/unread.md"]);
    assert_eq!(
        fx.saved_paths(),
        ["moved/dir/in.md", "moved/read.md"].map(String::from)
    );
    service.stop();
}

#[test]
fn a_rename_the_watcher_reports_alone_keeps_the_read_state_too() {
    let fx = Fx::new(&[("a.md", "a")]);
    let service = fx.open();
    fs::rename(
        fx.project.path().join("a.md"),
        fx.project.path().join("b.md"),
    )
    .expect("rename");
    service.apply(&[renamed("a.md", "b.md")]);
    service.flush();
    assert_eq!(service.unread(), NOTHING);
    assert_eq!(fx.saved_paths(), ["b.md"]);
    service.stop();
}

#[test]
fn links_rewritten_by_the_app_do_not_make_a_read_document_unread() {
    let fx = Fx::new(&[("a.md", "see [x](old.md)\n"), ("b.md", "see [x](old.md)\n")]);
    let service = fx.open();
    service.mark_unread("b.md").expect("mark");

    fx.write("a.md", "see [x](new.md)\n");
    fx.write("b.md", "see [x](new.md)\n");
    for path in ["a.md", "b.md"] {
        service.rewritten(
            path,
            &hex_of("see [x](old.md)\n"),
            &hex_of("see [x](new.md)\n"),
        );
    }
    service.apply(&[
        change(ChangeKind::Modified, "a.md"),
        change(ChangeKind::Modified, "b.md"),
    ]);
    service.flush();
    assert_eq!(service.unread(), ["b.md"], "b.md was unread before");
    assert_eq!(
        fx.saved().expect("saved")["a.md"],
        hex_of("see [x](new.md)\n")
    );
    service.stop();
}

#[test]
fn a_hash_that_is_not_one_leaves_the_rewrite_alone() {
    let fx = Fx::new(&[("a.md", "a")]);
    let service = fx.open();
    service.rewritten("a.md", "nonsense", &hex_of("b"));
    service.flush();
    assert_eq!(fx.saved().expect("saved")["a.md"], hex_of("a"));
    service.stop();
}

#[test]
fn what_the_editor_wrote_is_read() {
    let fx = Fx::new(&[("a.md", "one\n")]);
    let service = fx.open();
    fx.write("a.md", "an agent\n");
    service.apply(&[change(ChangeKind::Modified, "a.md")]);
    service.flush();
    assert_eq!(service.unread(), ["a.md"]);

    // the person edits and saves: the editor reports the hash it wrote
    fx.write("a.md", "merged\n");
    service.mark_written("a.md", &hex_of("merged\n"));
    assert_eq!(service.unread(), NOTHING);
    // a new file made in the app is read as well
    fx.write("fresh.md", "x");
    service.mark_written("fresh.md", &hex_of("x"));
    service.apply(&[
        change(ChangeKind::Modified, "a.md"),
        change(ChangeKind::Created, "fresh.md"),
    ]);
    service.flush();
    assert_eq!(service.unread(), NOTHING);
    // a hash that is not one is ignored
    service.mark_written("a.md", "xyz");
    service.stop();
}

#[test]
fn a_write_by_someone_else_right_after_the_editors_is_news() {
    let fx = Fx::new(&[("a.md", "one\n")]);
    let service = fx.open();
    fx.write("a.md", "mine\n");
    service.mark_written("a.md", &hex_of("mine\n"));
    fx.write("a.md", "an agent, right after\n");
    service.apply(&[change(ChangeKind::Modified, "a.md")]);
    service.flush();
    assert_eq!(service.unread(), ["a.md"]);
    service.stop();
}

#[test]
fn paths_are_checked_against_the_project() {
    let fx = Fx::new(&[("a.md", "a")]);
    let service = fx.open();
    for bad in ["../outside.md", "/etc/passwd", "a/../../b.md"] {
        assert!(service.mark_read(bad).is_err(), "{bad}");
        assert!(service.mark_unread(bad).is_err(), "{bad}");
        assert!(service.mark_folder_read(bad).is_err(), "{bad}");
    }
    // not a document, or not there: nothing to mark
    fx.write("logo.png", "png");
    service.mark_read("logo.png").expect("ignored");
    service.mark_read("missing.md").expect("ignored");
    assert_eq!(service.unread(), NOTHING);
    service.flush();
    assert_eq!(fx.saved_paths(), ["a.md"]);
    service.stop();
}

#[test]
fn a_damaged_read_file_is_a_project_without_read_state() {
    let fx = Fx::new(&[("a.md", "a"), ("b.md", "b")]);
    fs::write(fx.read_path(), "{ not json").expect("write");
    let service = fx.open();
    assert!(service.unread().is_empty(), "everything counts as read");
    assert_eq!(fx.saved_paths(), ["a.md", "b.md"], "and the file is mended");
    service.stop();
}

#[test]
fn entries_that_are_not_hashes_leave_their_document_unread() {
    let fx = Fx::new(&[("a.md", "a"), ("b.md", "b"), ("c.md", "c")]);
    let good = hex_of("b");
    fs::write(
        fx.read_path(),
        format!(r#"{{"a.md": "not a hash", "b.md": "{good}", "c.md": 7}}"#),
    )
    .expect("write");
    let service = fx.open();
    assert_eq!(service.unread(), ["a.md", "c.md"]);
    service.stop();
}

#[test]
fn a_project_with_no_markdown_has_a_baseline_too() {
    let fx = Fx::new(&[("notes.txt", "x")]);
    let service = fx.open();
    assert_eq!(
        fx.saved(),
        Some(BTreeMap::new()),
        "an empty state is a state"
    );
    fx.write("late.md", "x");
    service.apply(&[change(ChangeKind::Created, "late.md")]);
    service.flush();
    assert_eq!(service.unread(), ["late.md"]);
    service.stop();
}

#[test]
fn state_moves_with_the_project_folder() {
    let fx = Fx::new(&[("a.md", "a")]);
    let service = fx.open();
    let elsewhere = fx.state.path().join("local").join("read.json");
    service.relocate(elsewhere.clone());
    service.flush();
    let bytes = fs::read(&elsewhere).expect("written at the new place");
    assert!(decode(&bytes).expect("state").contains_key("a.md"));
    // from then on that is where it goes
    service.mark_unread("a.md").expect("mark");
    service.flush();
    assert!(decode(&fs::read(&elsewhere).expect("read"))
        .expect("state")
        .is_empty());
    service.stop();
}

#[test]
fn a_single_file_has_no_markers() {
    let fx = Fx::new(&[("a.md", "a")]);
    let vault = Arc::new(Vault::open(&fx.project.path().join("a.md")).expect("vault"));
    let service = fx.service(vault, fx.read_path(), NEVER);
    service.mark_read("a.md").expect("mark");
    service.mark_unread("a.md").expect("mark");
    service.mark_all_read();
    service.mark_folder_read("").expect("mark");
    service.renamed("a.md", "b.md");
    service.apply(&[change(ChangeKind::Created, "b.md")]);
    service.flush();
    assert_eq!(service.unread(), NOTHING);
    assert_eq!(*lock(&fx.heard), Vec::<Vec<String>>::new());
    service.stop();
    assert!(!fx.read_path().exists(), "no state is kept for it");
}

#[test]
fn a_state_that_cannot_be_written_does_not_stop_anything() {
    let fx = Fx::new(&[("a.md", "a")]);
    // the folder that should hold the file is a file
    let blocker = fx.state.path().join("blocker");
    fs::write(&blocker, "x").expect("write");
    let service = fx.service(
        Arc::new(Vault::open(fx.project.path()).expect("vault")),
        blocker.join("read.json"),
        NEVER,
    );
    service.mark_unread("a.md").expect("mark");
    assert_eq!(service.unread(), ["a.md"], "the session still works");
    service.flush();
    service.stop();
}

fn on_disk(fx: &Fx) -> Vec<String> {
    let vault = Vault::open(fx.project.path()).expect("vault");
    let filter = PathFilter::new(&[(vault.root().to_path_buf(), None)], &[]);
    unread_on_disk(&vault, &filter, &fx.read_path())
}

#[test]
fn with_no_window_the_unread_documents_are_read_from_the_saved_state_and_the_disk() {
    let fx = Fx::new(&[("a.md", "one"), ("docs/b.md", "two"), ("notes.txt", "text")]);
    // Never looked at: everything counts as read, as in the app.
    assert_eq!(on_disk(&fx), NOTHING);
    // Read state as the app saved it; then a document changes, one is new, one is gone.
    let service = fx.open();
    service.stop();
    assert_eq!(on_disk(&fx), NOTHING);
    fx.write("a.md", "changed");
    fx.write("c.md", "new");
    fx.write("notes.txt", "also changed, but not Markdown");
    fx.remove("docs/b.md");
    assert_eq!(on_disk(&fx), ["a.md", "c.md"]);
    // The state file is only read: asking never marks anything as read.
    assert_eq!(on_disk(&fx), ["a.md", "c.md"]);
    assert_eq!(fx.saved_paths(), ["a.md", "docs/b.md"]);
}

#[test]
fn a_damaged_state_file_leaves_the_documents_unread_like_the_service_does() {
    let fx = Fx::new(&[("a.md", "one")]);
    fs::write(fx.read_path(), "{ not json").expect("write");
    assert_eq!(on_disk(&fx), NOTHING, "no usable state: the baseline");
    fs::write(fx.read_path(), r#"{"a.md": "not a hash"}"#).expect("write");
    assert_eq!(on_disk(&fx), ["a.md"]);
}

#[test]
fn the_hex_form_is_sixteen_lowercase_digits() {
    assert_eq!(hash_to_hex(0xAB), "00000000000000ab");
    assert_eq!(hash_from_hex("00000000000000ab"), Some(0xAB));
    assert_eq!(hash_from_hex(&hash_to_hex(u64::MAX)), Some(u64::MAX));
    for bad in [
        "",
        "ab",
        "+000000000000ab1",
        "00000000000000ag",
        "0000000000000000f",
    ] {
        assert_eq!(hash_from_hex(bad), None, "{bad:?}");
    }
}

// ---- a look at the disk that is under way while the app works ----

/// The pieces of a service without its thread, so that a test decides what happens while the disk is walked.
/// The queue is returned to keep the service's sends from failing.
fn parts(fx: &Fx, saved: Option<BTreeMap<String, u64>>) -> (UnreadService, Worker, Receiver<Msg>) {
    let vault = Arc::new(Vault::open(fx.project.path()).expect("vault"));
    let filter = Arc::new(PathFilter::new(&[(vault.root().to_path_buf(), None)], &[]));
    let heard = Arc::clone(&fx.heard);
    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner {
            tracker: Tracker::new(saved),
            path: fx.read_path(),
            announced: Vec::new(),
            dirty: false,
        }),
        notify: Arc::new(move |unread: &[String]| lock(&heard).push(unread.to_vec())),
        observer: None,
    });
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let service = UnreadService {
        vault: Arc::clone(&vault),
        shared: Arc::clone(&shared),
        tx: Some(tx),
        stop: Arc::clone(&stop),
        handle: Mutex::new(None),
    };
    let worker = Worker {
        vault,
        filter,
        shared,
        stop,
        stamps: HashMap::new(),
    };
    (service, worker, rx)
}

fn read_state(entries: &[(&str, &str)]) -> BTreeMap<String, u64> {
    entries
        .iter()
        .map(|(path, text)| ((*path).to_owned(), hash_bytes(text.as_bytes())))
        .collect()
}

fn saved_paths_of(service: &UnreadService) -> Vec<String> {
    lock(&service.shared.inner)
        .tracker
        .saved()
        .into_keys()
        .collect()
}

#[test]
fn a_move_in_the_explorer_while_the_disk_is_walked_does_not_unread_the_document() {
    let fx = Fx::new(&[("old.md", "# Old\n"), ("keep.md", "# Keep\n")]);
    let (service, mut worker, _queue) = parts(
        &fx,
        Some(read_state(&[
            ("old.md", "# Old\n"),
            ("keep.md", "# Keep\n"),
        ])),
    );
    // the walk passes and sees old.md; then the person moves it
    let found = worker.walk().expect("walked");
    fs::rename(
        fx.project.path().join("old.md"),
        fx.project.path().join("new.md"),
    )
    .expect("rename");
    service.renamed("old.md", "new.md");
    worker.settle(found);
    assert_eq!(saved_paths_of(&service), ["keep.md", "new.md"]);

    // the watcher reports the move afterwards
    worker.apply(&[renamed("old.md", "new.md")]);
    assert_eq!(
        service.unread(),
        NOTHING,
        "it was read, and it is the same text"
    );
    assert_eq!(saved_paths_of(&service), ["keep.md", "new.md"]);
}

#[test]
fn a_document_made_in_the_app_while_the_disk_is_walked_is_not_unread() {
    let fx = Fx::new(&[("a.md", "# A\n")]);
    let (service, mut worker, _queue) = parts(&fx, Some(read_state(&[("a.md", "# A\n")])));
    // the walk passes before the document exists; the editor makes it and marks it read
    let found = worker.walk().expect("walked");
    fx.write("made.md", "# Made\n");
    service.mark_written("made.md", &hex_of("# Made\n"));
    worker.settle(found);
    assert_eq!(saved_paths_of(&service), ["a.md", "made.md"]);

    // the watcher reports the new file afterwards, and it is no news
    worker.apply(&[change(ChangeKind::Created, "made.md")]);
    assert_eq!(service.unread(), NOTHING);
    assert_eq!(*lock(&fx.heard), Vec::<Vec<String>>::new());
}

#[test]
fn a_mark_made_while_the_disk_is_walked_stands() {
    let fx = Fx::new(&[("a.md", "# A\n"), ("b.md", "# B\n")]);
    let (service, mut worker, _queue) = parts(
        &fx,
        Some(read_state(&[("a.md", "# A\n"), ("b.md", "# Old B\n")])),
    );
    let found = worker.walk().expect("walked");
    service.mark_read("b.md").expect("mark");
    service.mark_unread("a.md").expect("mark");
    worker.settle(found);
    assert_eq!(service.unread(), ["a.md"]);
    assert_eq!(saved_paths_of(&service), ["b.md"]);
}

#[test]
fn a_walk_that_was_stopped_tells_the_tracker_nothing() {
    let fx = Fx::new(&[("a.md", "# A\n")]);
    let (service, mut worker, _queue) = parts(&fx, Some(read_state(&[("a.md", "# Old\n")])));
    worker.stop.store(true, Ordering::Relaxed);
    assert!(worker.walk().is_none());
    assert_eq!(service.unread(), NOTHING, "nothing was looked at");
}

#[test]
fn a_batch_of_changes_is_cut_short_once_the_service_is_stopping() {
    let fx = Fx::new(&[("a.md", "# A\n")]);
    let (service, worker, _queue) = parts(&fx, Some(read_state(&[("a.md", "# A\n")])));
    fx.write("a.md", "# Changed\n");
    worker.stop.store(true, Ordering::Relaxed);
    worker.apply(&[change(ChangeKind::Modified, "a.md")]);
    assert_eq!(service.unread(), NOTHING, "the file was not even read");
}

// ---- closing does not wait for a look at the disk ----

#[test]
fn a_flush_gives_up_on_a_thread_that_is_busy_and_the_thread_still_writes_later() {
    let fx = Fx::new(&[("a.md", "# A\n")]);
    // a state in which a.md changed: the first look at the disk makes it unread and tells the listener
    fs::write(
        fx.read_path(),
        format!(r#"{{ "a.md": "{}" }}"#, hex_of("# Old\n")),
    )
    .expect("state");
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Mutex::new(release_rx);
    let vault = Arc::new(Vault::open(fx.project.path()).expect("vault"));
    let filter = Arc::new(PathFilter::new(&[(vault.root().to_path_buf(), None)], &[]));
    // The listener is called by the thread. The first call does not return until the test lets it go, which makes
    // a thread that is busy for exactly as long as the test says.
    let first_call = AtomicBool::new(true);
    let service = UnreadService::open(
        vault,
        filter,
        fx.read_path(),
        NEVER,
        Arc::new(move |_: &[String]| {
            if first_call.swap(false, Ordering::SeqCst) {
                let _ = entered_tx.send(());
                let _ = lock(&release_rx).recv();
            }
        }),
        None,
    );
    entered_rx.recv().expect("the thread is busy");

    assert!(
        !service.flush_within(Duration::from_millis(20)),
        "the answer does not come while the thread is busy, and the caller is not held up"
    );

    release_tx.send(()).expect("release");
    assert!(
        service.flush_within(Duration::MAX),
        "once it is free it catches up"
    );
    service.mark_read("a.md").expect("mark");
    service.flush();
    assert_eq!(fx.saved().expect("saved")["a.md"], hex_of("# A\n"));
    service.stop();
}

#[test]
fn a_flush_with_no_thread_has_nothing_to_wait_for() {
    let fx = Fx::new(&[("a.md", "a")]);
    let vault = Arc::new(Vault::open(&fx.project.path().join("a.md")).expect("vault"));
    let service = fx.service(vault, fx.read_path(), NEVER);
    assert!(service.flush_within(Duration::ZERO));
    service.stop();
}

// ---- following the read state (the review of changes) ----

/// What a follower was told, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Told {
    Reading(String, u64, Vec<u8>),
    Changed {
        moves: Vec<(String, String)>,
        looked: bool,
        docs: Vec<(String, Option<u64>, Option<u64>)>,
    },
}

#[derive(Default)]
struct Follower(Mutex<Vec<Told>>);

impl Follower {
    fn told(&self) -> Vec<Told> {
        lock(&self.0).clone()
    }

    fn last_docs(&self) -> Vec<(String, Option<u64>, Option<u64>)> {
        match self.told().pop() {
            Some(Told::Changed { docs, .. }) => docs,
            other => panic!("expected the documents, got {other:?}"),
        }
    }
}

impl ReadObserver for Follower {
    fn reading(&self, path: &str, hash: u64, bytes: &[u8]) {
        lock(&self.0).push(Told::Reading(path.to_owned(), hash, bytes.to_vec()));
    }

    fn changed(&self, moves: Vec<(String, String)>, looked: bool, docs: Vec<DocState>) {
        lock(&self.0).push(Told::Changed {
            moves,
            looked,
            docs: docs
                .into_iter()
                .map(|d| (d.path, d.current, d.read))
                .collect(),
        });
    }
}

fn open_followed(fx: &Fx) -> (UnreadService, Arc<Follower>) {
    let follower = Arc::new(Follower::default());
    let vault = Arc::new(Vault::open(fx.project.path()).expect("vault"));
    let filter = Arc::new(PathFilter::new(&[(vault.root().to_path_buf(), None)], &[]));
    let service = UnreadService::open(
        vault,
        filter,
        fx.read_path(),
        NEVER,
        Arc::new(|_: &[String]| {}),
        Some(Arc::clone(&follower) as Arc<dyn ReadObserver>),
    );
    service.flush();
    (service, follower)
}

fn hash_of(text: &str) -> u64 {
    hash_bytes(text.as_bytes())
}

#[test]
fn a_follower_hears_the_documents_once_the_disk_has_been_looked_at() {
    let fx = Fx::new(&[("a.md", "one\n"), ("b.md", "two\n")]);
    let (service, follower) = open_followed(&fx);
    let told = follower.told();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(
        told[0],
        Told::Changed {
            moves: Vec::new(),
            looked: true,
            docs: vec![
                (
                    "a.md".to_owned(),
                    Some(hash_of("one\n")),
                    Some(hash_of("one\n"))
                ),
                (
                    "b.md".to_owned(),
                    Some(hash_of("two\n")),
                    Some(hash_of("two\n"))
                ),
            ],
        }
    );
    service.stop();
}

#[test]
fn a_follower_hears_what_an_agent_writes_as_the_documents_change() {
    let fx = Fx::new(&[("a.md", "one\n")]);
    let (service, follower) = open_followed(&fx);
    fx.write("a.md", "one, then two\n");
    fx.write("new.md", "new\n");
    service.apply(&[
        change(ChangeKind::Modified, "a.md"),
        change(ChangeKind::Created, "new.md"),
    ]);
    service.flush();
    assert_eq!(
        follower.last_docs(),
        [
            (
                "a.md".to_owned(),
                Some(hash_of("one, then two\n")),
                Some(hash_of("one\n"))
            ),
            ("new.md".to_owned(), Some(hash_of("new\n")), None),
        ]
    );
    fx.remove("new.md");
    service.apply(&[change(ChangeKind::Removed, "new.md")]);
    service.flush();
    assert_eq!(
        follower.last_docs().len(),
        1,
        "a deleted document is not listed"
    );
    service.stop();
}

#[test]
fn the_person_reading_a_document_is_told_with_its_bytes_before_it_counts_as_read() {
    let fx = Fx::new(&[("a.md", "one\n")]);
    let (service, follower) = open_followed(&fx);
    fx.write("a.md", "two\n");
    service.apply(&[change(ChangeKind::Modified, "a.md")]);
    service.flush();
    let before = follower.told().len();

    service.mark_read("a.md").expect("read");
    let told = follower.told();
    assert_eq!(
        told[before],
        Told::Reading("a.md".to_owned(), hash_of("two\n"), b"two\n".to_vec())
    );
    assert!(
        matches!(&told[before + 1], Told::Changed { docs, .. }
            if docs == &[("a.md".to_owned(), Some(hash_of("two\n")), Some(hash_of("two\n")))]),
        "{told:?}"
    );
    service.stop();
}

#[test]
fn moves_by_the_app_and_by_the_watcher_are_told_to_a_follower() {
    let fx = Fx::new(&[("a.md", "a\n"), ("docs/b.md", "b\n")]);
    let (service, follower) = open_followed(&fx);
    service.renamed("a.md", "z.md");
    fs::rename(
        fx.project.path().join("docs/b.md"),
        fx.project.path().join("docs/c.md"),
    )
    .expect("rename");
    service.apply(&[renamed("docs/b.md", "docs/c.md")]);
    service.flush();
    let moved: Vec<(String, String)> = follower
        .told()
        .into_iter()
        .filter_map(|t| match t {
            Told::Changed { moves, .. } => Some(moves),
            Told::Reading(..) => None,
        })
        .flatten()
        .collect();
    assert_eq!(
        moved,
        [
            ("a.md".to_owned(), "z.md".to_owned()),
            ("docs/b.md".to_owned(), "docs/c.md".to_owned())
        ]
    );
    service.stop();
}

#[test]
fn a_follower_is_told_only_when_something_changed() {
    let fx = Fx::new(&[("a.md", "one\n"), ("b.md", "two\n")]);
    let (service, follower) = open_followed(&fx);
    let before = follower.told().len();

    // reading a document that is read already, marking everything read, a change that changes nothing
    service.mark_read("a.md").expect("read");
    service.mark_all_read();
    service.apply(&[change(ChangeKind::Modified, "a.md")]);
    service.flush();
    let told = follower.told();
    assert_eq!(
        told[before..],
        [Told::Reading(
            "a.md".to_owned(),
            hash_of("one\n"),
            b"one\n".to_vec()
        )],
        "the reading is told, for its bytes; the documents have not changed"
    );

    // and when one does change, it is told
    fx.write("a.md", "one, again\n");
    service.apply(&[change(ChangeKind::Modified, "a.md")]);
    service.flush();
    assert_eq!(follower.told().len(), before + 2);
    service.stop();
}
