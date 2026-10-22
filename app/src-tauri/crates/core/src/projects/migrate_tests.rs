//! Round 4 saved-data tests (`projects.md` §9.3, §14 D3, D49, D50): the copy of 0.6 state into included projects, on
//! the files 0.6.3 itself wrote (`fixtures/root-0.6.3/`: made by the v0.6.3 plugins through their RPCs, see §13 round
//! 4 fix; a root with `.permark/`, `sub-a` with its own `.permark/`, `sub-b` without), and the owners the plugins read
//! after it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::migrate::{copy_state, Ledger};
use super::*;
use crate::conditions::Distribution;
use crate::events::Recorder;
use crate::paths::Root;
use crate::plugins::PluginRegistry;
use crate::settings::AppSettings;
use crate::state::{AppDirs, StateLocation, StateStore};
use crate::workspace::{CoreEnv, ProjectCore};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/projects/fixtures/root-0.6.3"
);

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Every file below `dir` with its bytes (relative paths, `/`).
fn files_of(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

struct Fx {
    _dir: tempfile::TempDir,
    root: PathBuf,
    dirs: AppDirs,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(dir.path()).unwrap();
        let root = base.join("root ü");
        copy_tree(Path::new(FIXTURE), &root);
        let dirs = AppDirs::under(&base.join("app"));
        Self {
            _dir: dir,
            root,
            dirs,
        }
    }

    fn store(&self, rel: &str) -> StateStore {
        let path = if rel.is_empty() {
            self.root.clone()
        } else {
            self.root.join(rel)
        };
        fs::create_dir_all(&path).unwrap();
        StateStore::for_project(&self.dirs, &Root::open(&path).unwrap())
    }

    /// The root's files only (members excluded): what the copy must never change.
    fn root_files(&self) -> Vec<(String, Vec<u8>)> {
        files_of(&self.root.join(".permark"))
            .into_iter()
            .filter(|(p, _)| !p.ends_with(".gitignore"))
            .collect()
    }
}

fn json_at(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn text_at(path: &Path) -> String {
    String::from_utf8(fs::read(path).unwrap()).unwrap()
}

/// The id of the opened folder's comment on `path` (the fixture's ids are made by 0.6.3).
fn root_comment_id(fx: &Fx, path: &str) -> String {
    json_at(&fx.root.join(".permark/local/comments.json"))["comments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == json!(path))
        .and_then(|c| c["id"].as_str())
        .unwrap()
        .to_owned()
}

/// The entries of a 0.6 map file (`data.docs`) under `member`, relative to it.
fn docs_under(file: &Path, member: &str) -> Value {
    let prefix = format!("{member}/");
    Value::Object(
        json_at(file)["data"]["docs"]
            .as_object()
            .unwrap()
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix(&prefix)?.to_owned(), v.clone())))
            .collect(),
    )
}

#[test]
fn fixture_0_6_3_copies_into_both_members_and_the_root_stays_byte_identical() {
    let fx = Fx::new();
    let before = files_of(&Path::new(FIXTURE).join(".permark"));
    let (root, a, b) = (fx.store(""), fx.store("sub-a"), fx.store("sub-b"));
    assert_eq!(root.location(), StateLocation::Project);
    assert_eq!(a.location(), StateLocation::Project);
    assert_eq!(b.location(), StateLocation::Profile);
    let root_local = fx.root.join(".permark/local");
    let read_src = root_local.join("plugins/unread/read.json");
    let visit_src = root_local.join("plugins/unread/visit.json");

    let (mut ledger_a, mut ledger_b) = (Ledger::default(), Ledger::default());
    let copied_a = copy_state(&root, &a, "sub-a", &mut ledger_a).unwrap();
    let copied_b = copy_state(&root, &b, "sub-b", &mut ledger_b).unwrap();
    assert_eq!(
        copied_a.written,
        ["read.json", "visit.json", "comments.json", "quiet.json"]
    );
    assert_eq!(copied_b.written, copied_a.written);

    // sub-a: in its own .permark/
    let local = fx.root.join("sub-a/.permark/local");
    let read = json_at(&local.join("plugins/unread/read.json"));
    assert_eq!(read["version"], json!(1));
    assert_eq!(read["data"]["docs"], docs_under(&read_src, "sub-a"));
    assert_eq!(read["data"]["docs"].as_object().unwrap().len(), 2);
    let visit = json_at(&local.join("plugins/unread/visit.json"));
    assert_eq!(visit["data"]["at_ms"], json_at(&visit_src)["data"]["at_ms"]);
    assert_eq!(visit["data"]["docs"], docs_under(&visit_src, "sub-a"));
    assert!(
        visit["data"]["docs"].get("unread.md").is_some(),
        "unread there: in the picture, not read"
    );
    let comments = json_at(&local.join("comments.json"));
    assert_eq!(comments["version"], json!(1));
    let list = comments["comments"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], json!(root_comment_id(&fx, "sub-a/plan.md")));
    assert_eq!(list[0]["path"], json!("plan.md"));
    assert_eq!(list[0]["done"]["by"], json!("user"));
    assert_eq!(list[0]["replies"][0]["text"], json!("Please fix."));
    assert_eq!(
        text_at(&local.join("quiet.json")),
        "{\n  \"\": \"speaking\",\n  \"drafts\": \"quiet\"\n}",
        "marks at + below the member; the root's \"\" mark (above) stays there"
    );
    assert!(
        !fx.root.join("sub-a/.permark/meta.toml").exists(),
        "tags stay in the opened folder's meta.toml (D49)"
    );

    // sub-b: no .permark/ -> the profile folder of its ProjectId; siblings `sub-bc`, `sub-b ü` not taken
    let profile = b.dir().to_path_buf();
    assert!(profile.starts_with(&fx.dirs.profile));
    let docs = json_at(&profile.join("plugins/unread/read.json"))["data"]["docs"].clone();
    assert_eq!(docs, docs_under(&read_src, "sub-b"));
    assert_eq!(
        docs.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["x.md"]
    );
    let docs = json_at(&profile.join("plugins/unread/visit.json"))["data"]["docs"].clone();
    assert_eq!(docs, docs_under(&visit_src, "sub-b"));
    assert_eq!(docs.as_object().unwrap().len(), 2, "x.md + new.md");
    let list = json_at(&profile.join("comments.json"))["comments"].clone();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["path"], json!("x.md"));
    assert_eq!(
        json_at(&profile.join("quiet.json")),
        json!({ "old": "quiet" })
    );
    assert!(!profile.join("meta.toml").exists());

    // the root's files: byte-identical (CRLF meta.toml included)
    assert_eq!(fx.root_files(), before);

    // a second run writes nothing, with the ledger and without it (the project's entries win)
    let snapshot_a = files_of(&fx.root.join("sub-a"));
    let snapshot_b = files_of(&profile);
    assert_eq!(
        copy_state(&root, &a, "sub-a", &mut ledger_a)
            .unwrap()
            .written,
        Vec::<&str>::new()
    );
    assert_eq!(
        copy_state(&root, &b, "sub-b", &mut ledger_b)
            .unwrap()
            .written,
        Vec::<&str>::new()
    );
    assert_eq!(
        copy_state(&root, &a, "sub-a", &mut Ledger::default())
            .unwrap()
            .written,
        Vec::<&str>::new()
    );
    assert_eq!(files_of(&fx.root.join("sub-a")), snapshot_a);
    assert_eq!(files_of(&profile), snapshot_b);
    assert_eq!(fx.root_files(), before);
}

#[test]
fn the_projects_own_entries_win_and_unknown_keys_are_kept() {
    let fx = Fx::new();
    let (root, a) = (fx.store(""), fx.store("sub-a"));
    let local = fx.root.join("sub-a/.permark/local");
    fs::create_dir_all(local.join("plugins/unread")).unwrap();
    fs::write(
        local.join("plugins/unread/read.json"),
        r#"{"version":1,"data":{"docs":{"PLAN.md":"00000000000000ff"},"future":1}}"#,
    )
    .unwrap();
    let plan_id = root_comment_id(&fx, "sub-a/plan.md");
    fs::write(
        local.join("comments.json"),
        format!(r#"{{"version":1,"comments":[{{"id":"{plan_id}","path":"plan.md","text":"mine","anchor":{{"quote":"q","line":1,"endLine":1}},"createdMs":1,"futureKey":"kept"}}]}}"#),
    )
    .unwrap();
    fs::write(local.join("quiet.json"), r#"{"drafts":"speaking"}"#).unwrap();
    let own_meta = "# my notes\r\n[files.\"plan.md\"]\r\ntags = [\"mine\"] # keep\r\n";
    fs::write(fx.root.join("sub-a/.permark/meta.toml"), own_meta).unwrap();

    copy_state(&root, &a, "sub-a", &mut Ledger::default()).unwrap();

    let read = json_at(&local.join("plugins/unread/read.json"));
    assert_eq!(read["data"]["future"], json!(1), "unknown data keys kept");
    let docs = read["data"]["docs"].as_object().unwrap();
    if crate::paths::CASE_INSENSITIVE {
        assert!(
            !docs.contains_key("plan.md"),
            "PLAN.md is plan.md here: {docs:?}"
        );
    } else {
        assert!(docs.contains_key("plan.md"));
    }
    assert_eq!(docs["PLAN.md"], json!("00000000000000ff"), "target wins");
    let comments = json_at(&local.join("comments.json"));
    let list = comments["comments"].as_array().unwrap();
    let mine = list.iter().find(|c| c["id"] == json!(plan_id)).unwrap();
    assert_eq!(mine["text"], json!("mine"), "same id: target wins");
    assert_eq!(mine["futureKey"], json!("kept"), "unknown keys survive");
    assert_eq!(
        json_at(&local.join("quiet.json")),
        json!({ "": "speaking", "drafts": "speaking" })
    );
    // tags + descriptions: never copied into the project's shared meta.toml (D49)
    assert_eq!(text_at(&fx.root.join("sub-a/.permark/meta.toml")), own_meta);
}

/// S1 (D50): copied by entry, once. A comment | mark the opened folder got while the project was left out is copied at
/// the next include; what was copied before and removed in the project since never comes back; a value the project
/// still holds as copied follows the opened folder's newer one.
#[test]
fn a_second_copy_takes_only_what_is_new_and_never_brings_back_what_was_removed() {
    let fx = Fx::new();
    let (root, a) = (fx.store(""), fx.store("sub-a"));
    let mut ledger = Ledger::default();
    copy_state(&root, &a, "sub-a", &mut ledger).unwrap();
    let local = fx.root.join("sub-a/.permark/local");
    let plan_id = root_comment_id(&fx, "sub-a/plan.md");

    // in the project: the copied comment and the copied drafts mark removed
    let mut book = json_at(&local.join("comments.json"));
    book["comments"]
        .as_array_mut()
        .unwrap()
        .retain(|c| c["id"] != json!(plan_id));
    fs::write(
        local.join("comments.json"),
        serde_json::to_vec(&book).unwrap(),
    )
    .unwrap();
    let mut marks = json_at(&local.join("quiet.json"));
    marks.as_object_mut().unwrap().remove("drafts");
    fs::write(
        local.join("quiet.json"),
        serde_json::to_vec(&marks).unwrap(),
    )
    .unwrap();

    // in the opened folder while the project was left out: a new comment, a new mark, a newer read of plan.md
    let root_local = fx.root.join(".permark/local");
    let mut root_book = json_at(&root_local.join("comments.json"));
    let mut fresh = root_book["comments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == json!(plan_id))
        .unwrap()
        .clone();
    fresh["id"] = json!("c-while-left-out");
    fresh["text"] = json!("while left out");
    root_book["comments"].as_array_mut().unwrap().push(fresh);
    fs::write(
        root_local.join("comments.json"),
        serde_json::to_vec(&root_book).unwrap(),
    )
    .unwrap();
    let mut root_marks = json_at(&root_local.join("quiet.json"));
    root_marks["sub-a/later"] = json!("quiet");
    fs::write(
        root_local.join("quiet.json"),
        serde_json::to_vec(&root_marks).unwrap(),
    )
    .unwrap();
    let read_path = root_local.join("plugins/unread/read.json");
    let mut root_read = json_at(&read_path);
    root_read["data"]["docs"]["sub-a/plan.md"] = json!("00000000000000aa");
    fs::write(&read_path, serde_json::to_vec(&root_read).unwrap()).unwrap();

    let copied = copy_state(&root, &a, "sub-a", &mut ledger).unwrap();
    assert_eq!(copied.written, ["read.json", "comments.json", "quiet.json"]);
    let ids: Vec<Value> = json_at(&local.join("comments.json"))["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].clone())
        .collect();
    assert!(ids.contains(&json!("c-while-left-out")), "{ids:?}");
    assert!(
        !ids.contains(&json!(plan_id)),
        "removed in the project: stays removed {ids:?}"
    );
    let marks = json_at(&local.join("quiet.json"));
    assert_eq!(marks["later"], json!("quiet"));
    assert!(
        marks.get("drafts").is_none(),
        "removed in the project: {marks}"
    );
    assert_eq!(
        json_at(&local.join("plugins/unread/read.json"))["data"]["docs"]["plan.md"],
        json!("00000000000000aa"),
        "the project still held the copied value: the newer one follows"
    );

    // the project's own newer value wins over a later change in the opened folder
    let own_read = local.join("plugins/unread/read.json");
    let mut own = json_at(&own_read);
    own["data"]["docs"]["plan.md"] = json!("00000000000000bb");
    fs::write(&own_read, serde_json::to_vec(&own).unwrap()).unwrap();
    root_read["data"]["docs"]["sub-a/plan.md"] = json!("00000000000000cc");
    fs::write(&read_path, serde_json::to_vec(&root_read).unwrap()).unwrap();
    copy_state(&root, &a, "sub-a", &mut ledger).unwrap();
    assert_eq!(
        json_at(&own_read)["data"]["docs"]["plan.md"],
        json!("00000000000000bb")
    );
    // nothing new: nothing written
    assert_eq!(
        copy_state(&root, &a, "sub-a", &mut ledger).unwrap().written,
        Vec::<&str>::new()
    );
}

/// B1 (D51): the copy and a tab with the project open alone write the same file at once: both keep their entries.
#[test]
fn a_copy_racing_a_tab_on_the_same_file_loses_nothing() {
    for round in 0..20 {
        let fx = Fx::new();
        let (root, a) = (fx.store(""), fx.store("sub-a"));
        let quiet = a.raw_file("quiet.json");
        let writer = {
            let quiet = quiet.clone();
            std::thread::spawn(move || {
                for i in 0..20 {
                    quiet
                        .update(|now| {
                            let mut map: serde_json::Map<String, Value> = now
                                .and_then(|b| serde_json::from_slice(b).ok())
                                .unwrap_or_default();
                            map.insert(format!("tab{i}"), json!("quiet"));
                            serde_json::to_vec(&map).ok()
                        })
                        .unwrap();
                }
            })
        };
        copy_state(&root, &a, "sub-a", &mut Ledger::default()).unwrap();
        writer.join().unwrap();
        let marks = json_at(quiet.path());
        let tabs = (0..20)
            .filter(|i| marks.get(format!("tab{i}")).is_some())
            .count();
        assert_eq!(tabs, 20, "round {round}: {marks}");
        assert_eq!(marks["drafts"], json!("quiet"), "round {round}: {marks}");
    }
}

#[test]
fn an_interrupted_copy_converges_and_a_broken_target_is_never_written_over() {
    let fx = Fx::new();
    let (root, a) = (fx.store(""), fx.store("sub-a"));
    // the last step fails: a folder stands where quiet.json goes
    let quiet = fx.root.join("sub-a/.permark/local/quiet.json");
    fs::create_dir_all(&quiet).unwrap();
    assert!(copy_state(&root, &a, "sub-a", &mut Ledger::default()).is_err());
    fs::remove_dir(&quiet).unwrap();
    copy_state(&root, &a, "sub-a", &mut Ledger::default()).unwrap();
    // the same result as one clean run
    let clean = Fx::new();
    copy_state(
        &clean.store(""),
        &clean.store("sub-a"),
        "sub-a",
        &mut Ledger::default(),
    )
    .unwrap();
    assert_eq!(
        files_of(&fx.root.join("sub-a")),
        files_of(&clean.root.join("sub-a"))
    );

    // a target that is not comments: the copy fails, the bytes stay
    let broken = Fx::new();
    let local = broken.root.join("sub-a/.permark/local");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join("comments.json"), b"<<<<<<< HEAD\nnot json").unwrap();
    assert!(copy_state(
        &broken.store(""),
        &broken.store("sub-a"),
        "sub-a",
        &mut Ledger::default()
    )
    .is_err());
    assert_eq!(
        fs::read(local.join("comments.json")).unwrap(),
        b"<<<<<<< HEAD\nnot json"
    );
    // a newer comments layout is the project's too
    fs::write(
        local.join("comments.json"),
        br#"{"version":2,"comments":[]}"#,
    )
    .unwrap();
    assert!(copy_state(
        &broken.store(""),
        &broken.store("sub-a"),
        "sub-a",
        &mut Ledger::default()
    )
    .is_err());
}

#[test]
fn a_member_whose_permark_is_not_a_real_folder_gets_its_state_in_the_profile() {
    let fx = Fx::new();
    // `local` is a file: PerMark never writes inside that .permark/
    fs::write(fx.root.join("sub-a/.permark/local"), b"a file").unwrap();
    let a = fx.store("sub-a");
    assert_eq!(a.location(), StateLocation::Profile);
    let inside = files_of(&fx.root.join("sub-a"));
    copy_state(&fx.store(""), &a, "sub-a", &mut Ledger::default()).unwrap();
    assert_eq!(
        files_of(&fx.root.join("sub-a")),
        inside,
        "nothing written in the project"
    );
    assert!(a.dir().join("comments.json").is_file());
}

#[test]
fn read_only_target_fails_and_is_retried() {
    let fx = Fx::new();
    let (root, a) = (fx.store(""), fx.store("sub-a"));
    let local = fx.root.join("sub-a/.permark/local");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join("quiet.json"), b"{}").unwrap();
    let mut perms = fs::metadata(local.join("quiet.json"))
        .unwrap()
        .permissions();
    perms.set_readonly(true);
    fs::set_permissions(local.join("quiet.json"), perms.clone()).unwrap();
    let first = copy_state(&root, &a, "sub-a", &mut Ledger::default());
    #[allow(clippy::permissions_set_readonly_false)] // the test's own file, back to writable
    perms.set_readonly(false);
    fs::set_permissions(local.join("quiet.json"), perms).unwrap();
    if first.is_ok() {
        // some systems let an atomic rename replace a read-only file: then the copy simply worked
        return;
    }
    assert_eq!(fs::read(local.join("quiet.json")).unwrap(), b"{}");
    copy_state(&root, &a, "sub-a", &mut Ledger::default()).unwrap();
    assert_eq!(
        json_at(&local.join("quiet.json")),
        json!({ "": "speaking", "drafts": "quiet" })
    );
}

// ---- through the core: open, include, owners -------------------------------------------------------------------

fn env(base: &Path) -> CoreEnv {
    CoreEnv::new(
        AppDirs::under(base),
        Arc::new(PluginRegistry::new(Vec::new()).unwrap()),
        Arc::new(Recorder::default()),
        Distribution::Direct,
        AppSettings::default(),
    )
}

fn wait(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

const UID_A: &str = "aaaaaaaaaa";
const UID_B: &str = "bbbbbbbbbb";

fn record_both(root: &Path) {
    fs::create_dir_all(root.join("sub-b/.git")).unwrap();
    fs::write(
        root.join(".permark/projects.toml"),
        format!("version = 1\n\n[[project]]\nid = \"{UID_A}\"\npath = \"sub-a\"\n\n[[project]]\nid = \"{UID_B}\"\npath = \"sub-b\"\n"),
    )
    .unwrap();
}

#[test]
fn open_copies_members_included_before_round_4_once_then_owners_hold_them() {
    let fx = Fx::new();
    record_both(&fx.root);
    let root_before = fx.root_files();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&fx.root).unwrap(), &env);
    let projects = core.projects();
    let a = ProjectUid::parse(UID_A).unwrap();
    let b = ProjectUid::parse(UID_B).unwrap();
    // copied by the projects worker after it read the members (never on the open path)
    wait("owners", || projects.owners().slots().len() == 3);
    assert!(projects.is_migrated(&a) && projects.is_migrated(&b));
    let owners = projects.owners();
    assert_eq!(owners.slots().len(), 3);
    let ia = owners.index_of(&a).unwrap();
    assert_eq!(owners.owner_of("sub-a/plan.md"), ia);
    assert_eq!(owners.rel(ia, "sub-a/plan.md"), "plan.md");
    assert!(fx.root.join("sub-a/.permark/local/comments.json").is_file());
    let b_store = owners.slots()[owners.index_of(&b).unwrap()].store.clone();
    assert!(b_store.dir().join("comments.json").is_file());
    assert!(b_store.dir().starts_with(&env.dirs.profile));
    // the root's files are as 0.6.3 left them, except the record the test wrote
    let now: Vec<_> = fx
        .root_files()
        .into_iter()
        .filter(|(p, _)| p != "projects.toml")
        .collect();
    let was: Vec<_> = root_before
        .into_iter()
        .filter(|(p, _)| p != "projects.toml")
        .collect();
    assert_eq!(now, was);
    drop(core);

    // second open: nothing copied again (a pre-filled target stays as the person left it)
    fs::write(fx.root.join("sub-a/.permark/local/quiet.json"), b"{}").unwrap();
    let core = ProjectCore::open(Root::open(&fx.root).unwrap(), &env);
    wait("owners", || core.projects().owners().slots().len() == 3);
    assert!(core.projects().is_migrated(&a));
    assert_eq!(
        fs::read(fx.root.join("sub-a/.permark/local/quiet.json")).unwrap(),
        b"{}"
    );
}

#[test]
fn include_pauses_followers_copies_then_resumes_with_the_new_owner() {
    let fx = Fx::new();
    fs::create_dir_all(fx.root.join("sub-b/.git")).unwrap();
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&fx.root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    assert!(core.projects().owners().is_single());
    let heard: Arc<Mutex<Vec<String>>> = Arc::default();
    let _sub = core.projects().follow_owners({
        let heard = Arc::clone(&heard);
        let comments = fx.root.join("sub-a/.permark/local/comments.json");
        move |change| {
            let line = match change {
                OwnersChange::Pause => format!("pause copied={}", comments.is_file()),
                OwnersChange::Resume(o) => {
                    format!("resume {} copied={}", o.slots().len(), comments.is_file())
                }
            };
            heard.lock().unwrap().push(line);
        }
    });
    let outcome = core
        .projects_include(
            &env,
            &ProjectPick::Paths {
                paths: vec!["sub-a".to_owned()],
            },
        )
        .unwrap();
    assert_eq!(outcome.0.included, 1);
    assert_eq!(
        *heard.lock().unwrap(),
        ["pause copied=false", "resume 2 copied=true"]
    );
    let uid = core.projects().snapshot().members()[0].uid.clone();
    assert!(core.projects().is_migrated(&uid));

    // left out: the owners go back to the root alone; its dormant entries are in use again
    heard.lock().unwrap().clear();
    core.projects()
        .leave_out(std::slice::from_ref(&uid))
        .unwrap();
    assert_eq!(
        *heard.lock().unwrap(),
        ["pause copied=true", "resume 1 copied=true"]
    );
    // included again: no second copy (a change made in the project meanwhile stays)
    fs::write(fx.root.join("sub-a/.permark/local/quiet.json"), b"{}").unwrap();
    core.projects_include(
        &env,
        &ProjectPick::Paths {
            paths: vec!["sub-a".to_owned()],
        },
    )
    .unwrap();
    assert_eq!(
        fs::read(fx.root.join("sub-a/.permark/local/quiet.json")).unwrap(),
        b"{}"
    );
    wait("owners back", || {
        core.projects().owners().slots().len() == 2
    });
}

#[test]
fn a_member_moved_while_open_takes_its_profile_state_along_before_followers_resume() {
    let fx = Fx::new();
    record_both(&fx.root);
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&fx.root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    let b = ProjectUid::parse(UID_B).unwrap();
    let old_dir = {
        let o = core.projects().owners();
        o.slots()[o.index_of(&b).unwrap()].store.dir().to_path_buf()
    };
    assert!(old_dir.join("comments.json").is_file());
    let seen: Arc<Mutex<Vec<bool>>> = Arc::default();
    let _sub = core.projects().follow_owners({
        let seen = Arc::clone(&seen);
        let old = old_dir.clone();
        move |change| {
            // a move the system reports in two steps may first show the project gone (no slot), then back
            if let OwnersChange::Resume(o) = change {
                if let Some(i) = o.index_of(&ProjectUid::parse(UID_B).unwrap()) {
                    // at resume the state is in the new place already
                    seen.lock().unwrap().push(
                        o.slots()[i].store.dir().join("comments.json").is_file() && !old.exists(),
                    );
                }
            }
        }
    });
    // Windows refuses to rename a folder while a file inside is open (the first index pass reads them): retried
    let start = Instant::now();
    while let Err(e) = fs::rename(fx.root.join("sub-b"), fx.root.join("sub-b-moved")) {
        assert!(start.elapsed() < Duration::from_secs(5), "{e}");
        std::thread::sleep(Duration::from_millis(50));
    }
    wait("member moved", || {
        core.projects()
            .snapshot()
            .member(&b)
            .is_some_and(|m| m.path.as_str() == "sub-b-moved")
    });
    wait("owners resumed", || !seen.lock().unwrap().is_empty());
    assert!(
        seen.lock().unwrap().iter().all(|ok| *ok),
        "{:?}",
        seen.lock().unwrap()
    );
}

/// Round 4 journal (`projects.md` §9.2): a fact about a project's file also lands in that project's own journal (the
/// one it uses opened alone), its path relative to it; the opened folder's journal keeps every fact (hand-off and the
/// agent read it as before). Outside changes too; a move in from outside reads as new there.
#[test]
fn the_journal_of_a_project_gets_the_facts_about_its_files() {
    use crate::activity::{Fact, FileOp, Journal, OutsideChange};
    use crate::state::Loaded;
    let fx = Fx::new();
    record_both(&fx.root);
    let base = tempfile::tempdir().unwrap();
    let env = env(base.path());
    let core = ProjectCore::open(Root::open(&fx.root).unwrap(), &env);
    assert!(core.projects().wait_ready(Duration::from_secs(10)));
    let journal = core.activity();
    journal.person(Fact::Tag {
        added: true,
        path: "sub-a/plan.md".to_owned(),
        tag: "spec".to_owned(),
    });
    journal.person(Fact::File {
        op: FileOp::Move,
        path: "sub-a/moved.md".to_owned(),
        from: Some("docs/a.md".to_owned()),
        undo: false,
    });
    journal.person(Fact::Tag {
        added: true,
        path: "docs/a.md".to_owned(),
        tag: "root".to_owned(),
    });
    fs::write(fx.root.join("sub-a/outside.md"), "# o\n").unwrap();
    let member_file = fx.root.join("sub-a/.permark/local/activity.json");
    let load = || -> Journal {
        match crate::state::StateStore::for_project(
            &env.dirs,
            &Root::open(&fx.root.join("sub-a")).unwrap(),
        )
        .file::<Journal>(crate::activity::JOURNAL_FILE)
        .load()
        {
            Loaded::Found(j) => j,
            _ => Journal::default(),
        }
    };
    wait("member journal", || {
        journal.flush();
        load()
            .entries
            .iter()
            .any(|e| matches!(&e.fact, Fact::Outside { path, .. } if path == "outside.md"))
    });
    let entries = load().entries;
    assert!(member_file.is_file());
    let facts: Vec<&Fact> = entries.iter().map(|e| &e.fact).collect();
    assert!(facts.contains(&&Fact::Tag {
        added: true,
        path: "plan.md".to_owned(),
        tag: "spec".to_owned()
    }));
    assert!(
        facts.contains(&&Fact::File {
            op: FileOp::New,
            path: "moved.md".to_owned(),
            from: None,
            undo: false
        }),
        "moved in from the root: new here"
    );
    assert!(facts
        .iter()
        .any(|f| matches!(f, Fact::Outside { change: OutsideChange::Created, path, .. } if path == "outside.md")));
    assert!(!facts
        .iter()
        .any(|f| f.path() == Some("docs/a.md") || f.path() == Some("a.md")));
    // the root's journal: every fact, root-relative
    let root_facts = journal.snapshot().entries;
    assert!(root_facts
        .iter()
        .any(|e| e.fact.path() == Some("sub-a/plan.md")));
    assert!(root_facts
        .iter()
        .any(|e| e.fact.path() == Some("docs/a.md")));
}

/// V7 (verifier, local only): the copy on REAL 0.6.x bytes (the director's `F:/PerMarkRoot/.permark`, copied to a temp
/// folder; never touched in place). Every member with entries: copy OK, root bytes identical, entry counts == source
/// entries under the member, second run writes nothing.
#[test]
#[ignore = "verifier: needs F:/PerMarkRoot"]
fn verify_real_profile_copy() {
    let real = Path::new("F:/PerMarkRoot/.permark");
    let dir = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(dir.path()).unwrap();
    let root = base.join("root");
    for f in [
        "config.toml",
        "meta.toml",
        "local/comments.json",
        "local/quiet.json",
        "local/plugins/unread/read.json",
        "local/plugins/unread/visit.json",
    ] {
        let from = real.join(f);
        if from.is_file() {
            let to = root.join(".permark").join(f);
            fs::create_dir_all(to.parent().unwrap()).unwrap();
            fs::copy(&from, &to).unwrap();
        }
    }
    let dirs = AppDirs::under(&base.join("app"));
    let store = |rel: &str| {
        let p = if rel.is_empty() {
            root.clone()
        } else {
            root.join(rel)
        };
        fs::create_dir_all(&p).unwrap();
        StateStore::for_project(&dirs, &Root::open(&p).unwrap())
    };
    let before = files_of(&root.join(".permark"));
    let read_json = json_at(&root.join(".permark/local/plugins/unread/read.json"));
    let docs = read_json["data"]["docs"].as_object().unwrap();
    let comments = json_at(&root.join(".permark/local/comments.json"));
    let list = comments["comments"].as_array().unwrap();
    println!("real: read {} docs, {} comments", docs.len(), list.len());
    for member in [
        "hubs/hub-b-software",
        "hubs/hub-a-vision",
        "orchestrator",
        "shared",
    ] {
        let target = store(member);
        let t = Instant::now();
        let mut ledger = Ledger::default();
        let copied = copy_state(&store(""), &target, member, &mut ledger).unwrap();
        let took = t.elapsed();
        let want_docs = docs
            .keys()
            .filter(|k| k.starts_with(&format!("{member}/")))
            .count();
        let want_comments = list
            .iter()
            .filter(|c| {
                c["path"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with(&format!("{member}/"))
            })
            .count();
        let got_docs = if want_docs > 0 {
            json_at(
                &target
                    .plugin_dir(&crate::plugins::PluginId::of("unread"))
                    .join("read.json"),
            )["data"]["docs"]
                .as_object()
                .unwrap()
                .len()
        } else {
            0
        };
        let got_comments = if want_comments > 0 {
            json_at(&target.dir().join("comments.json"))["comments"]
                .as_array()
                .unwrap()
                .len()
        } else {
            0
        };
        println!("{member}: wrote {:?} in {took:?}; docs {got_docs}/{want_docs}; comments {got_comments}/{want_comments}", copied.written);
        assert_eq!(got_docs, want_docs, "{member}");
        assert_eq!(got_comments, want_comments, "{member}");
        assert!(
            copy_state(&store(""), &target, member, &mut ledger)
                .unwrap()
                .written
                .is_empty(),
            "{member}: second run wrote"
        );
    }
    assert_eq!(
        files_of(&root.join(".permark")),
        before,
        "root bytes changed"
    );
}
