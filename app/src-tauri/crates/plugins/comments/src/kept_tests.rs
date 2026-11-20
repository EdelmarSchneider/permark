//! Comments an outside write removes (a sync partner, a copy, git, another program) are set aside and can be
//! restored; the kept copy and the asides are this install's own sealed files, so what comes with a folder is never
//! restored. The merge of this core's changes with the file stays as it was.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::events::{CoreEvent, EventSink};
use permark_core::paths::Root;
use permark_core::plugins::{PluginRegistry, RpcCall};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use permark_core::ErrorKind;
use serde_json::{json, Value};

use super::*;
use crate::kept::{ASIDES_CAP, KEPT_CAP, MAX_KEPT_TEXT};
use crate::model::{Anchor, Book};
use crate::service::{merge_book, now_ms};

// ---- the merge, as it is ---------------------------------------------------------------------------------------

fn comment(id: &str, text: &str) -> Comment {
    Comment {
        id: id.to_owned(),
        path: "a.md".to_owned(),
        text: text.to_owned(),
        edited_ms: None,
        anchor: Anchor {
            quote: "q".to_owned(),
            line: 1,
            end_line: 1,
            ..Anchor::default()
        },
        created_ms: 1,
        done: None,
        replies: Vec::new(),
        extra: BTreeMap::default(),
    }
}

fn book(comments: Vec<Comment>) -> Book {
    Book {
        comments,
        ..Book::default()
    }
}

fn pairs(book: &Book) -> Vec<(&str, &str)> {
    book.comments
        .iter()
        .map(|c| (c.id.as_str(), c.text.as_str()))
        .collect()
}

#[test]
fn a_comment_removed_here_stays_removed_while_the_file_changed_elsewhere() {
    let base = [comment("a", "one"), comment("b", "two")];
    let mine = [comment("b", "two")];
    let fresh = book(vec![
        comment("a", "one, changed elsewhere"),
        comment("b", "two"),
        comment("c", "added elsewhere"),
    ]);
    assert_eq!(
        pairs(&merge_book(&base, &mine, fresh)),
        [("b", "two"), ("c", "added elsewhere")]
    );
}

#[test]
fn a_comment_removed_elsewhere_stays_removed_even_when_changed_here() {
    let base = [comment("a", "one"), comment("b", "two")];
    let mine = [
        comment("a", "one, changed here"),
        comment("b", "two"),
        comment("d", "added here"),
    ];
    let fresh = book(vec![comment("b", "two, changed elsewhere")]);
    assert_eq!(
        pairs(&merge_book(&base, &mine, fresh)),
        [("b", "two, changed elsewhere"), ("d", "added here")]
    );
}

// ---- a project ---------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Sink(Mutex<Vec<String>>);

impl EventSink for Sink {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, .. } = event {
            self.0.lock().unwrap().push(name);
        }
    }
}

struct Fx {
    _base: tempfile::TempDir,
    _dir: tempfile::TempDir,
    root: PathBuf,
    sink: Arc<Sink>,
    env: CoreEnv,
}

impl Fx {
    /// A folder whose state lives in the profile.
    fn new() -> Self {
        let base = tempfile::tempdir().unwrap();
        Self::under(AppDirs::under(base.path()), base, false)
    }

    /// A folder with its own `.permark/local`.
    fn initialized() -> Self {
        let base = tempfile::tempdir().unwrap();
        Self::under(AppDirs::under(base.path()), base, true)
    }

    /// An install folder that cannot be made (a file in the way): no install key.
    fn without_key() -> Self {
        let base = tempfile::tempdir().unwrap();
        let mut dirs = AppDirs::under(base.path());
        std::fs::write(base.path().join("blocked"), "x").unwrap();
        dirs.install = base.path().join("blocked").join("install");
        Self::under(dirs, base, false)
    }

    fn under(dirs: AppDirs, base: tempfile::TempDir, initialized: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap().join("notes");
        std::fs::create_dir_all(&root).unwrap();
        if initialized {
            std::fs::create_dir_all(root.join(".permark/local")).unwrap();
        }
        std::fs::write(root.join("doc.md"), "alpha beta\n").unwrap();
        let sink = Arc::new(Sink::default());
        let env = CoreEnv::new(
            dirs,
            Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
            sink.clone(),
            Distribution::Direct,
            AppSettings::default(),
        );
        Self {
            _base: base,
            _dir: dir,
            root,
            sink,
            env,
        }
    }

    fn open(&self) -> Arc<ProjectCore> {
        self.open_at(&self.root)
    }

    fn open_at(&self, root: &Path) -> Arc<ProjectCore> {
        let core = ProjectCore::open(Root::open(root).unwrap(), &self.env);
        assert!(core.projects().wait_owners(Duration::from_secs(10)));
        core
    }

    fn told(&self, event: &str) -> usize {
        self.sink
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|n| *n == event)
            .count()
    }
}

fn call(core: &ProjectCore, method: &str, args: Value) -> Result<Value, permark_core::IpcError> {
    core.rpc().call(
        "comments",
        method,
        &RpcCall {
            window: "w".to_owned(),
            args,
        },
    )
}

fn add(core: &ProjectCore, text: &str) -> String {
    call(
        core,
        "add",
        json!({ "path": "doc.md", "quote": "alpha", "text": text }),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn texts(core: &ProjectCore) -> Vec<String> {
    let mut texts: Vec<String> = call(core, "list", json!({ "scope": "all" }))
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["text"].as_str().unwrap().to_owned())
        .collect();
    texts.sort();
    texts
}

fn asides(core: &ProjectCore) -> Vec<Value> {
    call(core, "asides", Value::Null)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

fn file_of(core: &ProjectCore) -> PathBuf {
    core.store().dir().join(FILE)
}

/// Another program rewrites the comments file: `change` gets its comments.
fn outside(file: &Path, change: impl FnOnce(&mut Vec<Value>)) {
    let mut book: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    let mut comments = book["comments"].as_array().unwrap().clone();
    change(&mut comments);
    book["comments"] = Value::Array(comments);
    std::fs::write(file, serde_json::to_vec_pretty(&book).unwrap()).unwrap();
}

fn without(texts: &'static [&'static str]) -> impl FnOnce(&mut Vec<Value>) {
    move |comments| comments.retain(|c| !texts.contains(&c["text"].as_str().unwrap()))
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn comments_removed_outside_are_set_aside_and_restored_without_touching_others() {
    let fx = Fx::new();
    let core = fx.open();
    for n in 1..=8 {
        add(&core, &format!("c{n}"));
    }
    let file = file_of(&core);
    // one comment removed and another one's text changed by another program
    outside(&file, |comments| {
        comments.retain(|c| c["text"] != "c1");
        for c in comments.iter_mut().filter(|c| c["text"] == "c2") {
            c["text"] = json!("c2, changed elsewhere");
        }
    });
    let found = asides(&core);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0]["count"].clone(), found[0]["big"].clone()),
        (json!(1), json!(false))
    );
    assert!(found[0]["at"].as_u64().unwrap() > 0);
    let id = found[0]["id"].as_str().unwrap().to_owned();
    assert!(id.bytes().all(|b| b.is_ascii_digit()), "{id}");
    assert!(fx.told("comments:aside") >= 1);
    assert_eq!(texts(&core).len(), 7, "the list shows the file as it is");

    // restored: only the missing comment comes back, the outside change stays
    assert_eq!(
        call(&core, "restore", json!({ "id": id })).unwrap(),
        json!(1)
    );
    let mut expected = strings(&[
        "c1",
        "c2, changed elsewhere",
        "c3",
        "c4",
        "c5",
        "c6",
        "c7",
        "c8",
    ]);
    expected.sort();
    assert_eq!(texts(&core), expected);
    let on_disk: Book = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(on_disk.comments.len(), 8);
    assert!(asides(&core).is_empty(), "restored: the aside is gone");
    assert_eq!(
        call(&core, "restore", json!({ "id": id })).unwrap(),
        json!(0),
        "a second restore does nothing"
    );
    assert_eq!(texts(&core), expected);

    // five of eight removed at once: a big loss
    outside(&file, without(&["c1", "c3", "c4", "c5", "c6"]));
    let found = asides(&core);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0]["count"].clone(), found[0]["big"].clone()),
        (json!(5), json!(true))
    );
    // dismissed: the aside goes, nothing comes back
    let told = fx.told("comments:aside");
    call(&core, "dismissAside", json!({ "id": found[0]["id"] })).unwrap();
    assert!(asides(&core).is_empty());
    assert!(fx.told("comments:aside") > told);
    assert_eq!(texts(&core).len(), 3);
}

#[test]
fn additions_changes_and_the_same_bytes_from_outside_set_nothing_aside() {
    let fx = Fx::new();
    let core = fx.open();
    add(&core, "mine");
    add(&core, "second");
    let file = file_of(&core);
    outside(&file, |comments| {
        let mut new = comments[0].clone();
        new["id"] = json!("from-elsewhere");
        new["text"] = json!("added elsewhere");
        comments.push(new);
        comments[0]["text"] = json!("mine, reworded elsewhere");
        comments[1]["done"] = json!({ "atMs": 5, "by": "agent" });
    });
    assert!(asides(&core).is_empty());
    assert_eq!(texts(&core).len(), 3);
    let bytes = std::fs::read(&file).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    assert!(asides(&core).is_empty());
    assert_eq!(fx.told("comments:aside"), 0);
}

#[test]
fn a_file_gone_or_unreadable_sets_its_comments_aside_and_the_unreadable_file_is_kept() {
    let fx = Fx::new();
    let core = fx.open();
    for n in 1..=3 {
        add(&core, &format!("c{n}"));
    }
    let file = file_of(&core);
    std::fs::write(&file, "{ not json").unwrap();
    let found = asides(&core);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0]["count"].clone(), found[0]["big"].clone()),
        (json!(3), json!(true))
    );
    assert!(texts(&core).is_empty());
    // restored through the normal save: the unreadable file is put aside as before, never overwritten
    assert_eq!(
        call(&core, "restore", json!({ "id": found[0]["id"] })).unwrap(),
        json!(3)
    );
    assert_eq!(texts(&core), strings(&["c1", "c2", "c3"]));
    let kept: Vec<PathBuf> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("comments.unreadable-")
        })
        .collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), "{ not json");
    assert!(asides(&core).is_empty());
    // the file deleted by another program
    std::fs::remove_file(&file).unwrap();
    let found = asides(&core);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0]["count"], json!(3));
}

#[test]
fn comments_removed_while_the_folder_was_closed_are_set_aside_at_the_next_open() {
    let fx = Fx::new();
    let core = fx.open();
    add(&core, "kept");
    add(&core, "removed while closed");
    let file = file_of(&core);
    drop(core);
    outside(&file, without(&["removed while closed"]));
    let core = fx.open();
    let found = asides(&core);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0]["count"], json!(1));
    call(&core, "restore", json!({ "id": found[0]["id"] })).unwrap();
    assert_eq!(texts(&core), strings(&["kept", "removed while closed"]));
}

/// The kept copy's file in the folder's state (the plugin's folder of this install's own folder in it).
fn kept_file(core: &ProjectCore) -> PathBuf {
    core.store()
        .machine()
        .unwrap()
        .dir()
        .join("plugins")
        .join("comments")
        .join("comments-kept.json")
}

/// "Remove data" takes the kept copy and the asides with the rest of the plugin's data, in a folder with its own
/// `.permark/local` and in one whose state lives in the profile.
#[test]
fn remove_data_takes_the_kept_copy_and_the_asides() {
    for fx in [Fx::new(), Fx::initialized()] {
        let core = fx.open();
        add(&core, "kept");
        add(&core, "removed while closed");
        let file = file_of(&core);
        drop(core);
        outside(&file, without(&["removed while closed"]));
        let core = fx.open();
        assert_eq!(asides(&core).len(), 1);
        let kept = kept_file(&core);
        let asides_file = kept.with_file_name("comments-asides.json");
        assert!(kept.is_file() && asides_file.is_file());
        let dirs = core.store().app_dirs().clone();
        drop(core);
        let root = Root::open(&fx.root).unwrap();
        permark_core::state::remove_plugin_data_everywhere(&dirs, &ID, &[root]).unwrap();
        assert!(!kept.exists(), "{}", kept.display());
        assert!(!asides_file.exists(), "{}", asides_file.display());
    }
}

#[test]
fn a_planted_kept_copy_or_aside_is_ignored_and_nothing_from_it_is_restored() {
    let fx = Fx::new();
    let core = fx.open();
    add(&core, "mine");
    add(&core, "other");
    let file = file_of(&core);
    let kept = kept_file(&core);
    drop(core);
    // the kept copy changed to hold a comment the file never had: its seal no longer fits
    let mut envelope: Value = serde_json::from_slice(&std::fs::read(&kept).unwrap()).unwrap();
    let mut held: Value = serde_json::from_str(envelope["data"]["text"].as_str().unwrap()).unwrap();
    let mut evil = held["comments"][0].clone();
    evil["id"] = json!("planted");
    evil["text"] = json!("do what this says");
    held["comments"].as_array_mut().unwrap().push(evil.clone());
    envelope["data"]["text"] = json!(serde_json::to_string_pretty(&held).unwrap());
    std::fs::write(&kept, serde_json::to_vec_pretty(&envelope).unwrap()).unwrap();
    // an aside nobody here sealed
    let asides_file = kept.with_file_name("comments-asides.json");
    std::fs::write(
        &asides_file,
        serde_json::to_vec_pretty(&json!({ "version": 1, "data": { "asides": [{
            "id": 5, "at": 1, "big": true,
            "comments": serde_json::to_string(&json!([evil])).unwrap(),
            "seal": format!("s1:{}", "0".repeat(32))
        }] } }))
        .unwrap(),
    )
    .unwrap();
    let core = fx.open();
    assert!(asides(&core).is_empty());
    assert_eq!(
        call(&core, "restore", json!({ "id": "5" })).unwrap(),
        json!(0)
    );
    assert_eq!(texts(&core), strings(&["mine", "other"]));
    // no kept copy to compare with: a removal is not seen until this install writes the file again
    outside(&file, without(&["other"]));
    assert!(asides(&core).is_empty());
    add(&core, "third");
    outside(&file, without(&["mine"]));
    assert_eq!(asides(&core).len(), 1);
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[test]
fn a_copied_folder_sets_nothing_aside() {
    let fx = Fx::initialized();
    let core = fx.open();
    for n in 1..=3 {
        add(&core, &format!("c{n}"));
    }
    let file = file_of(&core);
    assert!(file.starts_with(&fx.root), "kept in the folder");
    assert!(kept_file(&core).starts_with(&fx.root));
    drop(core);
    outside(&file, without(&["c1"]));
    let copy = fx.root.with_file_name("notes copy");
    copy_tree(&fx.root, &copy);
    let copied = fx.open_at(&copy);
    assert!(
        asides(&copied).is_empty(),
        "the kept copy belongs to the other folder"
    );
    assert_eq!(texts(&copied), strings(&["c2", "c3"]));
    drop(copied);
    let core = fx.open();
    assert_eq!(
        asides(&core).len(),
        1,
        "the folder it was made for sees the loss"
    );
}

#[test]
fn another_window_s_own_writes_set_nothing_aside() {
    let fx = Fx::new();
    let one = fx.open();
    let two = fx.open();
    let first = add(&one, "first");
    add(&one, "second");
    assert_eq!(texts(&two).len(), 2);
    call(&one, "remove", json!({ "id": first })).unwrap();
    add(&two, "third");
    assert!(asides(&two).is_empty());
    assert!(asides(&one).is_empty());
    assert_eq!(texts(&one), strings(&["second", "third"]));
    assert_eq!(fx.told("comments:aside"), 0);
}

/// Makes every write of the comments file fail (`true`) or work again: a read-only file is never replaced on
/// Windows; a read-only folder takes no new file on unix.
fn hold_writes(file: &Path, held: bool) {
    #[cfg(windows)]
    {
        let mut permissions = std::fs::metadata(file).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(held);
        std::fs::set_permissions(file, permissions).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if held { 0o555 } else { 0o755 };
        std::fs::set_permissions(
            file.parent().unwrap(),
            std::fs::Permissions::from_mode(mode),
        )
        .unwrap();
    }
}

#[test]
fn a_failed_write_sets_the_unsaved_comment_aside_and_restore_waits_for_the_file() {
    let fx = Fx::new();
    let core = fx.open();
    add(&core, "saved");
    let file = file_of(&core);
    hold_writes(&file, true);
    let failed = call(
        &core,
        "add",
        json!({ "path": "doc.md", "quote": "alpha", "text": "unsaved" }),
    );
    assert!(failed.is_err(), "{failed:?}");
    assert_eq!(texts(&core), strings(&["saved"]));
    // the kept copy was written first: the comment that never reached the file can be brought back
    let found = asides(&core);
    assert_eq!(found.len(), 1, "{found:?}");
    let id = found[0]["id"].clone();
    let refused = call(&core, "restore", json!({ "id": id })).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Unavailable);
    assert_eq!(
        asides(&core),
        found,
        "kept, and no second aside of the same comment"
    );
    hold_writes(&file, false);
    assert_eq!(
        call(&core, "restore", json!({ "id": id })).unwrap(),
        json!(1)
    );
    assert_eq!(texts(&core), strings(&["saved", "unsaved"]));
    assert!(asides(&core).is_empty());
}

#[test]
fn without_an_install_key_comments_work_and_nothing_is_kept() {
    let fx = Fx::without_key();
    let core = fx.open();
    assert!(core.store().machine().is_none());
    let first = add(&core, "first");
    add(&core, "second");
    add(&core, "third");
    call(&core, "remove", json!({ "id": first })).unwrap();
    let file = file_of(&core);
    outside(&file, without(&["second"]));
    assert!(asides(&core).is_empty());
    assert_eq!(texts(&core), strings(&["third"]));
    assert_eq!(
        call(&core, "restore", json!({ "id": "1" })).unwrap(),
        json!(0)
    );
    assert!(!core.store().dir().join("machine").exists());
    assert_eq!(fx.told("comments:aside"), 0);
}

#[test]
fn at_most_three_asides_are_kept_the_oldest_going_first() {
    let fx = Fx::new();
    let core = fx.open();
    for n in 1..=6 {
        add(&core, &format!("c{n}"));
    }
    let file = file_of(&core);
    let mut ids = Vec::new();
    for gone in ["c1", "c2", "c3", "c4"] {
        outside(&file, move |comments| {
            comments.retain(|c| c["text"] != gone);
        });
        let found = asides(&core);
        ids.push(found.last().unwrap()["id"].as_str().unwrap().to_owned());
    }
    let left: Vec<String> = asides(&core)
        .iter()
        .map(|a| a["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(left, ids[1..]);
    call(&core, "restore", json!({ "id": ids[3] })).unwrap();
    assert_eq!(texts(&core), strings(&["c4", "c5", "c6"]));
}

#[test]
fn an_aside_id_is_digits_or_a_project_id_and_digits_never_a_path() {
    let fx = Fx::new();
    let core = fx.open();
    add(&core, "one");
    for bad in [
        "",
        "../comments.json",
        "1/2",
        "-1",
        "+1",
        "1.5",
        "abc",
        ":1",
        "x:1",
        "1:",
    ] {
        for method in ["restore", "dismissAside"] {
            assert_eq!(
                call(&core, method, json!({ "id": bad })).unwrap_err().kind,
                ErrorKind::InvalidData,
                "{method} {bad:?}"
            );
        }
    }
    // well formed, unknown: nothing to do
    assert_eq!(
        call(&core, "restore", json!({ "id": "123" })).unwrap(),
        json!(0)
    );
    call(&core, "dismissAside", json!({ "id": "abcdefgh:1" })).unwrap();
    assert_eq!(texts(&core), strings(&["one"]));
}

#[test]
fn an_aside_older_than_thirty_days_is_dropped() {
    let fx = Fx::new();
    let core = fx.open();
    add(&core, "kept");
    let keeper = crate::kept::Keeper::of(&core.store(), Arc::new(|| {})).unwrap();
    let day = 24 * 60 * 60 * 1000;
    let now = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    assert!(keeper
        .set_aside(now - 31 * day, &[comment("old", "gone long ago")], false)
        .unwrap());
    assert!(keeper
        .set_aside(now - 29 * day, &[comment("recent", "gone lately")], false)
        .unwrap());
    let found = asides(&core);
    assert_eq!(found.len(), 1, "{found:?}");
    call(&core, "restore", json!({ "id": found[0]["id"] })).unwrap();
    assert_eq!(texts(&core), strings(&["gone lately", "kept"]));
    let stored =
        std::fs::read_to_string(kept_file(&core).with_file_name("comments-asides.json")).unwrap();
    assert!(!stored.contains("gone long ago"), "{stored}");
}

// ---- sizes and the writers' lock -------------------------------------------------------------------------------

#[test]
fn a_planted_kept_copy_or_asides_file_over_its_cap_is_refused_by_size_and_left_in_place() {
    let fx = Fx::new();
    let core = fx.open();
    add(&core, "mine");
    let kept = kept_file(&core);
    let asides_file = kept.with_file_name("comments-asides.json");
    drop(core);
    // not JSON: a file that was read would be found corrupt and moved aside; one refused by its size stays as it is
    let planted = [(&kept, KEPT_CAP + 1), (&asides_file, ASIDES_CAP + 1)];
    for (path, len) in planted {
        std::fs::write(path, vec![b'x'; usize::try_from(len).unwrap()]).unwrap();
    }
    let core = fx.open();
    assert!(asides(&core).is_empty());
    assert_eq!(texts(&core), strings(&["mine"]));
    add(&core, "second");
    assert_eq!(texts(&core), strings(&["mine", "second"]));
    for (path, len) in planted {
        assert_eq!(
            std::fs::metadata(path).unwrap().len(),
            len,
            "{}",
            path.display()
        );
    }
    const { assert!(KEPT_CAP < 9 * 1024 * 1024 && ASIDES_CAP < 25 * 1024 * 1024) };
}

/// Another program pads the comments file to exactly `len` bytes (a key the comments keep beside them).
fn pad_to(file: &Path, len: u64) {
    let mut book: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    book["pad"] = json!("");
    let bare = serde_json::to_vec_pretty(&book).unwrap().len();
    book["pad"] = json!("x".repeat(usize::try_from(len).unwrap() - bare));
    let bytes = serde_json::to_vec_pretty(&book).unwrap();
    assert_eq!(bytes.len() as u64, len);
    std::fs::write(file, bytes).unwrap();
}

/// The text of the kept copy; `None`: no kept copy.
fn kept_text(core: &ProjectCore) -> Option<String> {
    let bytes = std::fs::read(kept_file(core)).ok()?;
    let envelope: Value = serde_json::from_slice(&bytes).unwrap();
    Some(envelope["data"]["text"].as_str().unwrap().to_owned())
}

#[test]
fn a_comments_file_over_four_mib_gets_no_kept_copy_and_its_comments_still_work() {
    let fx = Fx::new();
    let core = fx.open();
    add(&core, "first");
    add(&core, "second");
    let file = file_of(&core);
    // at the limit: kept as before
    pad_to(&file, MAX_KEPT_TEXT);
    assert_eq!(texts(&core), strings(&["first", "second"]));
    assert_eq!(
        kept_text(&core).map(String::into_bytes),
        Some(std::fs::read(&file).unwrap())
    );
    // one byte more: the kept copy goes, the comments work as before
    pad_to(&file, MAX_KEPT_TEXT + 1);
    assert_eq!(texts(&core), strings(&["first", "second"]));
    assert_eq!(kept_text(&core), None);
    add(&core, "third");
    assert!(std::fs::metadata(&file).unwrap().len() > MAX_KEPT_TEXT);
    assert_eq!(kept_text(&core), None);
    // removals are not seen while the file is that large
    outside(&file, without(&["first"]));
    assert!(asides(&core).is_empty());
    assert_eq!(texts(&core), strings(&["second", "third"]));
    // smaller again: kept from this install's next write on
    let mut book: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    book.as_object_mut().unwrap().remove("pad");
    std::fs::write(&file, serde_json::to_vec_pretty(&book).unwrap()).unwrap();
    add(&core, "fourth");
    assert!(kept_text(&core).is_some());
    outside(&file, without(&["second"]));
    assert_eq!(asides(&core).len(), 1);
}

#[test]
fn an_aside_that_would_take_the_asides_file_over_its_cap_is_not_made_and_older_ones_stay() {
    let fx = Fx::new();
    let core = fx.open();
    add(&core, "kept");
    let keeper = crate::kept::Keeper::of(&core.store(), Arc::new(|| {})).unwrap();
    let now = now_ms();
    // a quote is escaped twice (in the list of comments, then in the asides file): four bytes each
    let big = |id: &str| comment(id, &"\"".repeat(usize::try_from(ASIDES_CAP / 7).unwrap()));
    assert!(keeper.set_aside(now, &[big("one")], false).unwrap());
    assert!(
        !keeper.set_aside(now + 1, &[big("two")], false).unwrap(),
        "over the cap: not made"
    );
    assert!(keeper
        .set_aside(now + 2, &[comment("small", "small")], false)
        .unwrap());
    let ids: Vec<u64> = keeper.asides(now + 2).iter().map(|a| a.id).collect();
    assert_eq!(ids, [now, now + 2]);
    let stored = kept_file(&core).with_file_name("comments-asides.json");
    assert!(std::fs::metadata(stored).unwrap().len() <= ASIDES_CAP);
}

#[test]
fn a_look_while_another_writer_holds_the_lock_waits_briefly_and_the_next_look_compares() {
    let fx = Fx::new();
    let core = fx.open();
    for n in 1..=3 {
        add(&core, &format!("c{n}"));
    }
    let file = file_of(&core);
    let held = permark_core::state::shared_write::lock_for(&file)
        .unwrap()
        .expect("lock");
    outside(&file, without(&["c1"]));
    let started = Instant::now();
    assert!(
        asides(&core).is_empty(),
        "not compared while another writer holds the lock"
    );
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(texts(&core), strings(&["c2", "c3"]));
    drop(held);
    let found = asides(&core);
    assert_eq!(found.len(), 1, "compared at the next look: {found:?}");
    assert_eq!(found[0]["count"], json!(1));
}
