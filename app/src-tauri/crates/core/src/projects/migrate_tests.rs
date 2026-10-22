//! Round 4 saved-data tests (`projects.md` §9.3, §14 D3): the one-time copy of 0.6 state into included projects,
//! on byte copies of the files 0.6.3 writes (`fixtures/root-0.6.3/`: a root with `.permark/`, `sub-a` with its own
//! `.permark/`, `sub-b` without), and the owners the plugins read after it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::migrate::copy_state;
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
const H3: &str = "0000000000000003";
const H4: &str = "0000000000000004";
const H5: &str = "0000000000000005";

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

#[test]
fn fixture_0_6_3_copies_into_both_members_and_the_root_stays_byte_identical() {
    let fx = Fx::new();
    let before = files_of(&Path::new(FIXTURE).join(".permark"));
    let (root, a, b) = (fx.store(""), fx.store("sub-a"), fx.store("sub-b"));
    assert_eq!(root.location(), StateLocation::Project);
    assert_eq!(a.location(), StateLocation::Project);
    assert_eq!(b.location(), StateLocation::Profile);

    let copied_a = copy_state(&root, &a, "sub-a").unwrap();
    let copied_b = copy_state(&root, &b, "sub-b").unwrap();
    assert_eq!(
        copied_a.written,
        [
            "read.json",
            "visit.json",
            "comments.json",
            "quiet.json",
            "meta.toml"
        ]
    );
    assert_eq!(copied_b.written, copied_a.written);

    // sub-a: in its own .permark/
    let local = fx.root.join("sub-a/.permark/local");
    assert_eq!(
        text_at(&local.join("plugins/unread/read.json")),
        "{\n  \"data\": {\n    \"docs\": {\n      \"notes/n.md\": \"0000000000000004\",\n      \"plan.md\": \"0000000000000003\"\n    }\n  },\n  \"version\": 1\n}"
    );
    let visit = json_at(&local.join("plugins/unread/visit.json"));
    assert_eq!(visit["data"]["at_ms"], json!(1_760_000_000_000_u64));
    assert_eq!(
        visit["data"]["docs"],
        json!({ "notes/n.md": H4, "plan.md": H3, "unread.md": "0000000000000008" })
    );
    let comments = json_at(&local.join("comments.json"));
    assert_eq!(comments["version"], json!(1));
    let list = comments["comments"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], json!("c1f9a0000002"));
    assert_eq!(list[0]["path"], json!("plan.md"));
    assert_eq!(list[0]["futureKey"], json!("kept"), "unknown keys survive");
    assert_eq!(list[0]["done"]["reply"], json!("Fixed."));
    assert_eq!(list[0]["replies"][0]["text"], json!("Please fix."));
    assert_eq!(
        text_at(&local.join("quiet.json")),
        "{\n  \"\": \"speaking\",\n  \"drafts\": \"quiet\"\n}",
        "marks at + below the member; the root's \"\" mark (above) stays there"
    );
    assert_eq!(
        text_at(&fx.root.join("sub-a/.permark/meta.toml")),
        "# PerMark metadata: tags and descriptions added in the app.\n# Safe to commit and share. PerMark rewrites this file when you edit tags.\n\n[files.\"notes/n.md\"]\ndescription = \"Loose notes.\"\n\n[files.\"plan.md\"]\ntags = [\"spec\", \"needs-review\"]\ndescription = \"The plan of sub-a.\"\n\n[tag_styles.spec]\ncolor = \"blue\"\nicon = \"book-open\"\n"
    );

    // sub-b: no .permark/ -> the profile folder of its ProjectId; siblings `sub-bc`, `sub-b ü` not taken
    let profile = b.dir().to_path_buf();
    assert!(profile.starts_with(&fx.dirs.profile));
    assert_eq!(
        json_at(&profile.join("plugins/unread/read.json"))["data"]["docs"],
        json!({ "x.md": H5 })
    );
    assert_eq!(
        json_at(&profile.join("plugins/unread/visit.json"))["data"]["docs"],
        json!({ "x.md": H5, "new.md": "0000000000000009" })
    );
    let list = json_at(&profile.join("comments.json"))["comments"].clone();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["path"], json!("x.md"));
    assert_eq!(
        json_at(&profile.join("quiet.json")),
        json!({ "old": "quiet" })
    );
    let meta = text_at(&profile.join("meta.toml"));
    assert!(
        meta.contains("[files.\"x.md\"]\ntags = [\"draft\"]\n"),
        "{meta}"
    );
    assert!(
        meta.contains("[tag_styles.draft]\ncolor = \"gray\"\n"),
        "{meta}"
    );
    assert!(
        !meta.contains("spec"),
        "only the styles of copied tags: {meta}"
    );

    // the root's files: byte-identical (CRLF meta.toml included)
    assert_eq!(fx.root_files(), before);

    // a second run writes nothing
    let snapshot_a = files_of(&fx.root.join("sub-a"));
    let snapshot_b = files_of(&profile);
    assert_eq!(copy_state(&root, &a, "sub-a").unwrap().written.len(), 0);
    assert_eq!(copy_state(&root, &b, "sub-b").unwrap().written.len(), 0);
    assert_eq!(files_of(&fx.root.join("sub-a")), snapshot_a);
    assert_eq!(files_of(&profile), snapshot_b);
    assert_eq!(fx.root_files(), before);
}

#[test]
fn the_projects_own_entries_win_and_crlf_comments_unknown_keys_are_kept() {
    let fx = Fx::new();
    let (root, a) = (fx.store(""), fx.store("sub-a"));
    let local = fx.root.join("sub-a/.permark/local");
    fs::create_dir_all(local.join("plugins/unread")).unwrap();
    fs::write(
        local.join("plugins/unread/read.json"),
        r#"{"version":1,"data":{"docs":{"PLAN.md":"00000000000000ff"},"future":1}}"#,
    )
    .unwrap();
    fs::write(
        local.join("comments.json"),
        r#"{"version":1,"comments":[{"id":"c1f9a0000002","path":"plan.md","text":"mine","anchor":{"quote":"q","line":1,"endLine":1},"createdMs":1}]}"#,
    )
    .unwrap();
    fs::write(local.join("quiet.json"), r#"{"drafts":"speaking"}"#).unwrap();
    let own_meta = "# my notes\r\n[files.\"plan.md\"]\r\ntags = [\"mine\"] # keep\r\n\r\n[tag_styles.spec]\r\ncolor = \"red\"\r\n";
    fs::write(fx.root.join("sub-a/.permark/meta.toml"), own_meta).unwrap();

    copy_state(&root, &a, "sub-a").unwrap();

    let read = json_at(&local.join("plugins/unread/read.json"));
    assert_eq!(read["data"]["future"], json!(1), "unknown data keys kept");
    let docs = read["data"]["docs"].as_object().unwrap();
    if crate::paths::CASE_INSENSITIVE {
        assert_eq!(docs.len(), 2, "PLAN.md is plan.md here: {docs:?}");
        assert!(!docs.contains_key("plan.md"));
    } else {
        assert_eq!(docs["plan.md"], json!(H3));
    }
    assert_eq!(docs["PLAN.md"], json!("00000000000000ff"), "target wins");
    let comments = json_at(&local.join("comments.json"));
    assert_eq!(comments["comments"].as_array().unwrap().len(), 1);
    assert_eq!(
        comments["comments"][0]["text"],
        json!("mine"),
        "same id: target wins"
    );
    assert_eq!(
        json_at(&local.join("quiet.json")),
        json!({ "": "speaking", "drafts": "speaking" })
    );
    let meta = text_at(&fx.root.join("sub-a/.permark/meta.toml"));
    assert!(
        meta.starts_with("# my notes\r\n[files.\"plan.md\"]\r\n"),
        "{meta:?}"
    );
    assert!(
        meta.contains("tags = [\"mine\", \"spec\", \"needs-review\"] # keep\r\n"),
        "tags united, trailing comment kept: {meta:?}"
    );
    assert!(
        meta.contains("description = \"The plan of sub-a.\"\r\n"),
        "{meta:?}"
    );
    assert!(
        meta.contains("color = \"red\"\r\n") && !meta.contains("book-open"),
        "style: target wins: {meta:?}"
    );
    assert!(
        !meta.replace("\r\n", "").contains('\n'),
        "CRLF only: {meta:?}"
    );
}

#[test]
fn an_interrupted_copy_converges_and_a_broken_target_is_never_written_over() {
    let fx = Fx::new();
    let (root, a) = (fx.store(""), fx.store("sub-a"));
    // the last step fails: a folder stands where meta.toml goes
    let meta = fx.root.join("sub-a/.permark/meta.toml");
    fs::create_dir_all(&meta).unwrap();
    assert!(copy_state(&root, &a, "sub-a").is_err());
    fs::remove_dir(&meta).unwrap();
    copy_state(&root, &a, "sub-a").unwrap();
    // the same result as one clean run
    let clean = Fx::new();
    copy_state(&clean.store(""), &clean.store("sub-a"), "sub-a").unwrap();
    assert_eq!(
        files_of(&fx.root.join("sub-a")),
        files_of(&clean.root.join("sub-a"))
    );

    // a target that is not comments: the copy fails, the bytes stay
    let broken = Fx::new();
    let local = broken.root.join("sub-a/.permark/local");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join("comments.json"), b"<<<<<<< HEAD\nnot json").unwrap();
    assert!(copy_state(&broken.store(""), &broken.store("sub-a"), "sub-a").is_err());
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
    assert!(copy_state(&broken.store(""), &broken.store("sub-a"), "sub-a").is_err());
}

#[test]
fn a_member_whose_permark_is_not_a_real_folder_gets_its_state_in_the_profile() {
    let fx = Fx::new();
    // `local` is a file: PerMark never writes inside that .permark/
    fs::write(fx.root.join("sub-a/.permark/local"), b"a file").unwrap();
    let a = fx.store("sub-a");
    assert_eq!(a.location(), StateLocation::Profile);
    let inside = files_of(&fx.root.join("sub-a"));
    copy_state(&fx.store(""), &a, "sub-a").unwrap();
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
    let first = copy_state(&root, &a, "sub-a");
    #[allow(clippy::permissions_set_readonly_false)] // the test's own file, back to writable
    perms.set_readonly(false);
    fs::set_permissions(local.join("quiet.json"), perms).unwrap();
    if first.is_ok() {
        // some systems let an atomic rename replace a read-only file: then the copy simply worked
        return;
    }
    assert_eq!(fs::read(local.join("quiet.json")).unwrap(), b"{}");
    copy_state(&root, &a, "sub-a").unwrap();
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
    fs::rename(fx.root.join("sub-b"), fx.root.join("sub-b-moved")).unwrap();
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
