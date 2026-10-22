//! The projects service inside a real `ProjectCore` (real walk, real watch, temp folders).

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::conditions::Distribution;
use crate::events::Recorder;
use crate::plugins::PluginRegistry;
use crate::settings::AppSettings;
use crate::tree::TreeFilter;
use crate::workspace::{CoreEnv, ProjectCore};

fn env(base: &Path) -> (CoreEnv, Arc<Recorder>) {
    let sink = Arc::new(Recorder::default());
    let env = CoreEnv::new(
        AppDirs::under(base),
        Arc::new(PluginRegistry::new(Vec::new()).unwrap()),
        sink.clone(),
        Distribution::Direct,
        AppSettings::default(),
    );
    (env, sink)
}

fn wait(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn ready(core: &ProjectCore) -> ProjectsView {
    wait("projects ready", || core.projects_view().ready);
    core.projects_view()
}

fn mkdirs(root: &Path, rel: &str) {
    fs::create_dir_all(root.join(rel)).unwrap();
}

fn git(root: &Path, rel: &str) {
    mkdirs(root, &format!("{rel}/.git"));
}

fn permark(root: &Path, rel: &str) {
    if rel.is_empty() {
        fs::create_dir_all(root.join(".permark")).unwrap();
    } else {
        mkdirs(root, &format!("{rel}/.permark"));
    }
}

fn row<'v>(view: &'v ProjectsView, path: &str) -> Option<&'v ProjectRow> {
    view.rows.iter().find(|r| r.path == path)
}

fn state(view: &ProjectsView, path: &str) -> Option<MemberState> {
    row(view, path).map(|r| r.state)
}

fn listed(core: &ProjectCore) -> Vec<String> {
    core.explorer()
        .open_view(
            "w",
            Some(TreeFilter {
                show_hidden: true,
                ..TreeFilter::default()
            }),
        )
        .nodes
        .into_iter()
        .filter(|n| !n.ignored)
        .map(|n| n.path)
        .collect()
}

fn paths(list: &[&str]) -> ProjectPick {
    ProjectPick::Paths {
        paths: list.iter().map(|s| (*s).to_owned()).collect(),
    }
}

#[test]
fn a_plain_folder_gets_nothing_new_and_no_file_written() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    mkdirs(dir.path(), "docs");
    fs::write(dir.path().join("docs/a.md"), "# a").unwrap();
    let (env, sink) = env(base.path());
    let core = ProjectCore::open(Root::open(dir.path()).unwrap(), &env);
    let view = ready(&core);
    assert!(!view.present, "no Settings section");
    assert!(view.rows.is_empty() && view.offered.count == 0);
    assert_eq!(view.record, RecordStatus::Absent);
    assert!(core.projects().snapshot().is_plain());
    assert!(
        !dir.path().join(".permark").exists(),
        "nothing written in the folder"
    );
    assert!(
        !base.path().join("profile/projects").exists()
            || !fs::read_dir(base.path().join("profile/projects"))
                .unwrap()
                .flatten()
                .any(|p| p.path().join(MEMBERSHIP_FILE).exists()),
        "no membership file"
    );
    let _ = sink.take();
}

/// Root holding: `hubs/` gitignored with repo `hubs/a` (shown, round 1) and `.permark`-only `hubs/p` (not found),
/// visible repo `tools/b`, visible `.permark`-only `notes`.
fn sample_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join(".gitignore"), "hubs/\n*.env\n").unwrap();
    git(root, "hubs/a");
    fs::write(root.join("hubs/a/README.md"), "# a").unwrap();
    fs::write(root.join("hubs/a/.env"), "K=v").unwrap();
    permark(root, "hubs/p");
    fs::write(root.join("hubs/p/plan.md"), "# p").unwrap();
    git(root, "tools/b");
    fs::write(root.join("tools/b/x.md"), "# x").unwrap();
    permark(root, "notes");
    fs::write(root.join("notes/n.md"), "# n").unwrap();
    fs::write(root.join("top.md"), "# top").unwrap();
    dir
}

#[test]
fn found_projects_are_offered_then_included_all_and_undone() {
    let base = tempfile::tempdir().unwrap();
    let dir = sample_root();
    let root = dir.path();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    let view = ready(&core);
    assert!(view.present);
    assert_eq!(view.answer, RootAnswer::Unanswered);
    assert_eq!(state(&view, "hubs/a"), Some(MemberState::Offered));
    assert_eq!(state(&view, "tools/b"), Some(MemberState::Offered));
    assert_eq!(state(&view, "notes"), Some(MemberState::Offered));
    assert_eq!(
        state(&view, "hubs/p"),
        None,
        ".permark-only below a hidden folder: not found"
    );
    assert_eq!(view.offered.count, 3);
    // before an answer the agent sees no project, even in visible folders
    let map = core.projects().snapshot();
    assert!(map.closed_to_agent("tools/b/x.md"));
    assert!(map.closed_to_agent("notes/n.md"));
    assert!(!map.closed_to_agent("top.md"));

    let (outcome, initialized) = core.projects_include(&env, &ProjectPick::All).unwrap();
    assert!(initialized, "no .permark/: PerMark initialized first");
    assert_eq!(outcome.included, 3);
    assert!(root.join(".permark/config.toml").is_file());
    let text = fs::read_to_string(root.join(".permark/projects.toml")).unwrap();
    assert_eq!(text.matches("[[project]]").count(), 3);
    let view = core.projects_view();
    assert_eq!(view.answer, RootAnswer::Answered);
    assert!(view.rows.iter().all(|r| r.state == MemberState::Included));
    assert_eq!(core.projects().held_members(), 3);
    assert!(!core.projects().snapshot().closed_to_agent("tools/b/x.md"));

    let (undone, moved) = core.projects_undo(&env, outcome.undo.unwrap()).unwrap();
    assert!(undone.undone && !undone.permark_kept && moved);
    assert!(
        !root.join(".permark").exists(),
        "untouched .permark/ removed"
    );
    let view = core.projects_view();
    assert_eq!(view.answer, RootAnswer::Unanswered);
    assert_eq!(view.offered.count, 3);
    assert_eq!(core.projects().held_members(), 0);
    // the token is used up
    let (again, _) = core.projects_undo(&env, outcome.undo.unwrap()).unwrap();
    assert!(!again.undone);
}

#[test]
fn undo_keeps_a_permark_folder_that_changed_and_a_record_that_existed() {
    let base = tempfile::tempdir().unwrap();
    let dir = sample_root();
    let root = dir.path();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    ready(&core);
    let (outcome, _) = core.projects_include(&env, &paths(&["notes"])).unwrap();
    fs::write(root.join(".permark/meta.toml"), "x = 1\n").unwrap();
    let (undone, moved) = core.projects_undo(&env, outcome.undo.unwrap()).unwrap();
    assert!(undone.undone && undone.permark_kept && !moved);
    assert!(root.join(".permark/meta.toml").is_file());
    assert!(
        !root.join(".permark/projects.toml").exists(),
        "record made by the include goes"
    );
    // a record that existed before stays (entries out only)
    fs::write(root.join(".permark/projects.toml"), "# mine\nversion = 1\n").unwrap();
    wait("record read", || {
        core.projects_view().record == RecordStatus::Ok
    });
    let (outcome, _) = core.projects_include(&env, &paths(&["tools/b"])).unwrap();
    core.projects_undo(&env, outcome.undo.unwrap()).unwrap();
    assert_eq!(
        fs::read_to_string(root.join(".permark/projects.toml")).unwrap(),
        "# mine\nversion = 1\n"
    );
}

#[test]
fn decline_all_writes_only_the_profile_and_later_projects_stay_quiet() {
    let base = tempfile::tempdir().unwrap();
    let dir = sample_root();
    let root = dir.path();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    ready(&core);
    core.projects().decline(&ProjectPick::All).unwrap();
    let view = core.projects_view();
    assert_eq!(view.answer, RootAnswer::DeclinedAll);
    assert_eq!(view.offered.count, 0);
    assert!(view.rows.iter().all(|r| r.state == MemberState::Declined));
    assert!(!root.join(".permark").exists());
    drop(core);
    // reopened: the answer holds; a new project is declined silently
    git(root, "tools/c");
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    let view = ready(&core);
    assert_eq!(view.answer, RootAnswer::DeclinedAll);
    assert_eq!(state(&view, "tools/c"), Some(MemberState::Declined));
}

#[test]
fn leave_out_and_include_storm_keeps_one_record_state_and_no_leaks() {
    let base = tempfile::tempdir().unwrap();
    let dir = sample_root();
    let root = dir.path();
    permark(root, "");
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    ready(&core);
    core.projects_include(&env, &paths(&["tools/b", "notes"]))
        .unwrap();
    for i in 0..50 {
        let view = core.projects_view();
        let b = row(&view, "tools/b").unwrap();
        if b.state == MemberState::Included {
            core.projects()
                .leave_out(&[b.uid.clone().unwrap()])
                .unwrap();
        } else {
            core.projects_include(&env, &paths(&["tools/b"])).unwrap();
        }
        let view = core.projects_view();
        let want = if i % 2 == 0 {
            MemberState::Declined
        } else {
            MemberState::Included
        };
        assert_eq!(state(&view, "tools/b"), Some(want), "toggle {i}");
    }
    let text = fs::read_to_string(root.join(".permark/projects.toml")).unwrap();
    let record = match record::parse(text.as_bytes()) {
        RecordRead::Read(r) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(record.entries.len(), 2, "{text}");
    assert_eq!(core.projects().held_members(), 2);
    let projects = Arc::clone(core.projects());
    drop(core);
    assert_eq!(projects.held_members(), 0, "every member released at drop");
    projects.stop(); // twice: harmless
}

#[test]
fn included_projects_shape_the_tree_with_their_rules() {
    let base = tempfile::tempdir().unwrap();
    let dir = sample_root();
    let root = dir.path();
    permark(root, "");
    // the record lists the `.permark`-only project below the hidden folder (not found by the walk)
    fs::write(
        root.join(".permark/projects.toml"),
        "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"hubs/a\"\n\n[[project]]\nid = \"pppppppppp\"\npath = \"hubs/p\"\n",
    )
    .unwrap();
    fs::write(root.join("hubs/a/.permark.keep"), "").unwrap();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    let view = ready(&core);
    assert_eq!(
        view.answer,
        RootAnswer::Answered,
        "a record with entries counts as an answer"
    );
    assert_eq!(state(&view, "hubs/p"), Some(MemberState::Included));
    let shown = listed(&core);
    assert!(shown.contains(&"hubs/p/plan.md".to_owned()), "{shown:?}");
    assert!(shown.contains(&"hubs/a/README.md".to_owned()));
    // own rules: the root's `*.env` does not apply inside `hubs/a`
    assert!(shown.contains(&"hubs/a/.env".to_owned()));
    let uid = ProjectUid::parse("aaaaaaaaaa").unwrap();
    core.projects()
        .set_hidden(&uid, HiddenRule::OwnRoot)
        .unwrap();
    wait("own+root hides .env", || {
        !listed(&core).contains(&"hubs/a/.env".to_owned())
    });
    assert!(fs::read_to_string(root.join(".permark/projects.toml"))
        .unwrap()
        .contains("hidden = \"own+root\""));
    // walk verdict == watch verdict for every listed path
    let ignorers = core.explorer().ignorers();
    let main = &ignorers[0].1;
    for node in core
        .explorer()
        .open_view(
            "v",
            Some(TreeFilter {
                show_hidden: true,
                show_ignored: true,
                ..TreeFilter::default()
            }),
        )
        .nodes
    {
        let is_dir = matches!(node.kind, crate::tree::NodeKind::Dir { .. });
        assert_eq!(
            main.rel_ignored(&node.path, is_dir),
            node.ignored,
            "{}",
            node.path
        );
    }
}

#[test]
fn the_persons_ignore_list_beats_a_listed_project() {
    let base = tempfile::tempdir().unwrap();
    let dir = sample_root();
    let root = dir.path();
    permark(root, "");
    fs::write(root.join(".permark/config.toml"), "ignore = [\"tools/\"]\n").unwrap();
    fs::write(
        root.join(".permark/projects.toml"),
        "[[project]]\nid = \"bbbbbbbbbb\"\npath = \"tools/b\"\n",
    )
    .unwrap();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    let view = ready(&core);
    let b = row(&view, "tools/b").unwrap();
    assert_eq!(b.state, MemberState::Missing);
    assert_eq!(b.missing, Some(MissingReason::Ignored));
    assert!(!listed(&core).iter().any(|p| p.starts_with("tools/")));
    assert!(core.projects().snapshot().closed_to_agent("tools/b/x.md"));
}

#[test]
fn record_edits_outside_apply_at_once_and_gone_projects_come_back() {
    let base = tempfile::tempdir().unwrap();
    let dir = sample_root();
    let root = dir.path();
    permark(root, "");
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    ready(&core);
    let news: Arc<Mutex<Vec<ProjectEvent>>> = Arc::default();
    let _sub = core.projects().subscribe({
        let news = Arc::clone(&news);
        move |e| news.lock().unwrap().push(e.clone())
    });
    std::thread::sleep(Duration::from_millis(200));
    // a pull adds an entry: valid as written
    fs::write(
        root.join(".permark/projects.toml"),
        "[[project]]\npath = \"tools/b\"\n",
    )
    .unwrap();
    wait("entry included", || {
        state(&core.projects_view(), "tools/b") == Some(MemberState::Included)
    });
    wait("list changed row", || {
        news.lock().unwrap().contains(&ProjectEvent::ListChanged)
    });
    // the folder goes: missing (record untouched), one Changes row
    let before = fs::read(root.join(".permark/projects.toml")).unwrap();
    fs::rename(root.join("tools/b"), base.path().join("b-away")).unwrap();
    wait("missing", || {
        state(&core.projects_view(), "tools/b") == Some(MemberState::Missing)
    });
    wait("gone row", || {
        news.lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, ProjectEvent::Gone { .. }))
    });
    assert_eq!(
        fs::read(root.join(".permark/projects.toml")).unwrap(),
        before
    );
    assert!(core.projects().snapshot().closed_to_agent("tools/b/x.md"));
    // back at the same place
    fs::rename(base.path().join("b-away"), root.join("tools/b")).unwrap();
    wait("back", || {
        state(&core.projects_view(), "tools/b") == Some(MemberState::Included)
    });
    wait("back row", || {
        news.lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, ProjectEvent::Back { .. }))
    });
}

#[test]
fn a_moved_project_is_asked_by_its_card_id_or_its_name() {
    let base = tempfile::tempdir().unwrap();
    let dir = sample_root();
    let root = dir.path();
    permark(root, "");
    permark(root, "tools/b");
    fs::write(
        root.join("tools/b/.permark/project.toml"),
        "id = \"bbbbbbbbbb\"\n",
    )
    .unwrap();
    git(root, "x/notes-app");
    fs::write(
        root.join(".permark/projects.toml"),
        "[[project]]\nid = \"bbbbbbbbbb\"\npath = \"tools/b\"\n\n[[project]]\nid = \"nnnnnnnnnn\"\npath = \"old/notes-app\"\n",
    )
    .unwrap();
    // moved while closed: `tools/b` now lives at `moved/b`
    mkdirs(root, "moved");
    fs::rename(root.join("tools/b"), root.join("moved/b")).unwrap();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    let view = ready(&core);
    // found at open by its card id: asked (D31: clones carry the same card), nothing written before the answer
    let ask = view.ask.clone().expect("card id question");
    assert_eq!(
        (ask.was.as_str(), ask.candidate.as_str()),
        ("tools/b", "moved/b")
    );
    assert_eq!(state(&view, "moved/b"), Some(MemberState::Offered));
    assert!(fs::read_to_string(root.join(".permark/projects.toml"))
        .unwrap()
        .contains("path = \"tools/b\""));
    core.projects()
        .answer_move(&ask.uid, &ask.candidate, true)
        .unwrap();
    let view = core.projects_view();
    assert_eq!(
        state(&view, "moved/b"),
        Some(MemberState::Included),
        "{view:#?}"
    );
    assert!(fs::read_to_string(root.join(".permark/projects.toml"))
        .unwrap()
        .contains("path = \"moved/b\""));
    // `old/notes-app` is missing and `x/notes-app` has its name: the person is asked next
    let ask = view.ask.clone().expect("move question");
    assert_eq!(ask.was, "old/notes-app");
    assert_eq!(ask.candidate, "x/notes-app");
    core.projects()
        .answer_move(&ask.uid, &ask.candidate, false)
        .unwrap();
    assert!(core.projects_view().ask.is_none(), "not asked again");
    assert_eq!(
        state(&core.projects_view(), "x/notes-app"),
        Some(MemberState::Offered)
    );
}

#[test]
fn an_inner_root_offers_its_projects_and_its_record_stays_byte_identical() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, "hub");
    permark(root, "hub");
    fs::write(root.join("hub/.gitignore"), "deep/\n").unwrap();
    permark(root, "hub/deep/spec");
    let inner_record = "# inner\r\nversion = 1\r\n\r\n[[project]]\r\nid = \"ssssssssss\"\r\npath = \"deep/spec\"\r\n";
    fs::write(root.join("hub/.permark/projects.toml"), inner_record).unwrap();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    let view = ready(&core);
    assert_eq!(state(&view, "hub"), Some(MemberState::Offered));
    assert_eq!(
        state(&view, "hub/deep/spec"),
        Some(MemberState::Offered),
        "{view:#?}"
    );
    assert_eq!(view.offered.inner.len(), 1);
    assert_eq!(view.offered.inner[0].count, 1);
    core.projects_include(&env, &ProjectPick::All).unwrap();
    let view = core.projects_view();
    let spec = row(&view, "hub/deep/spec").unwrap();
    assert_eq!(
        spec.uid.as_ref().map(ProjectUid::as_str),
        Some("ssssssssss"),
        "inner id reused"
    );
    assert_eq!(
        spec.parent.as_deref(),
        Some(crate::paths::key_of("hub").as_str())
    );
    core.projects()
        .leave_out(&[spec.uid.clone().unwrap()])
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.join("hub/.permark/projects.toml")).unwrap(),
        inner_record,
        "the dormant record is never written"
    );
}

#[test]
fn cards_locate_and_agent_choice() {
    let base = tempfile::tempdir().unwrap();
    let dir = sample_root();
    let root = dir.path();
    permark(root, "");
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    ready(&core);
    core.projects_include(&env, &paths(&["tools/b", "notes"]))
        .unwrap();
    let view = core.projects_view();
    let notes = row(&view, "notes").unwrap().uid.clone().unwrap();
    let b = row(&view, "tools/b").unwrap().uid.clone().unwrap();
    // notes has .permark/: its card file; tools/b has none: the profile
    let dto = core
        .projects_card_set(
            &CardTarget::Member { uid: notes.clone() },
            &CardChange {
                field: card::CardField::Name,
                value: "Notes".into(),
            },
        )
        .unwrap();
    assert_eq!(dto.name, "Notes");
    assert_eq!(dto.place, CardPlace::Project);
    let written = fs::read_to_string(root.join("notes/.permark/project.toml")).unwrap();
    assert!(written.contains(&format!("id = \"{notes}\"")));
    let dto = core
        .projects_card_set(
            &CardTarget::Member { uid: b.clone() },
            &CardChange {
                field: card::CardField::Short,
                value: "b".into(),
            },
        )
        .unwrap();
    assert_eq!(dto.place, CardPlace::Profile);
    assert!(!root.join("tools/b/.permark").exists());
    assert_eq!(row(&core.projects_view(), "notes").unwrap().name, "Notes");
    let root_card = core
        .projects_card_set(
            &CardTarget::Root,
            &CardChange {
                field: card::CardField::Description,
                value: "The  root\nfolder".into(),
            },
        )
        .unwrap();
    assert_eq!(root_card.description.as_deref(), Some("The root folder"));
    // agent off for one member (profile only)
    core.projects().set_agent(&b, AgentChoice::Off).unwrap();
    assert!(core.projects().snapshot().closed_to_agent("tools/b/x.md"));
    assert_eq!(
        row(&core.projects_view(), "tools/b").unwrap().agent,
        AgentChoice::Off
    );
    // locate refusals
    let outside = base.path().to_path_buf();
    assert_eq!(
        core.projects_locate(&b, &outside).unwrap(),
        Some(LocateRefusal::Outside)
    );
    assert_eq!(
        core.projects_locate(&b, &root.join("hubs")).unwrap(),
        Some(LocateRefusal::NotProject)
    );
    assert_eq!(
        core.projects_locate(&b, &root.join("notes")).unwrap(),
        Some(LocateRefusal::Taken)
    );
}

#[test]
fn unreadable_and_newer_records_refuse_writes_and_keep_their_bytes() {
    let base = tempfile::tempdir().unwrap();
    let dir = sample_root();
    let root = dir.path();
    permark(root, "");
    for text in [
        "<<<<<<< HEAD\n[[project]]\n",
        "version = 7\n[[project]]\npath = \"tools/b\"\n",
    ] {
        fs::write(root.join(".permark/projects.toml"), text).unwrap();
        let (env, _sink) = env(base.path());
        let core = ProjectCore::open(Root::open(root).unwrap(), &env);
        let view = ready(&core);
        assert!(matches!(
            view.record,
            RecordStatus::Unreadable { line: 1, .. } | RecordStatus::Newer
        ));
        let err = core.projects_include(&env, &paths(&["notes"])).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Unavailable);
        assert_eq!(
            fs::read_to_string(root.join(".permark/projects.toml")).unwrap(),
            text
        );
    }
}

#[test]
fn rapid_project_churn_converges_without_writing_the_record() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    permark(root, "");
    fs::write(root.join(".permark/projects.toml"), "version = 1\n").unwrap();
    let before = fs::read(root.join(".permark/projects.toml")).unwrap();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    ready(&core);
    for i in 0..100 {
        git(root, &format!("churn/p{i}"));
    }
    for i in 0..100 {
        if i % 2 == 0 {
            fs::remove_dir_all(root.join(format!("churn/p{i}"))).unwrap();
        }
    }
    wait("50 offered", || core.projects_view().offered.count == 50);
    assert_eq!(
        fs::read(root.join(".permark/projects.toml")).unwrap(),
        before
    );
}

#[test]
fn unicode_and_space_paths_round_trip() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for p in ["Ünï cödé/项目 a", "space dir/b c"] {
        git(root, p);
    }
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    ready(&core);
    core.projects_include(&env, &ProjectPick::All).unwrap();
    let view = core.projects_view();
    assert_eq!(view.rows.len(), 2);
    assert!(
        view.rows.iter().all(|r| r.state == MemberState::Included),
        "{view:#?}"
    );
    let text = fs::read_to_string(root.join(".permark/projects.toml")).unwrap();
    assert!(text.contains("项目 a"));
    let map = core.projects().snapshot();
    let owner = map.owner_of("Ünï cödé/项目 a/doc.md");
    assert_eq!(owner.rel, "doc.md");
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) -> bool {
    std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(windows)]
#[test]
fn junctions_are_never_projects_and_a_listed_path_through_one_is_skipped() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(outside.path(), "repo");
    fs::write(outside.path().join("repo/secret.md"), "# s").unwrap();
    mkdirs(root, "links");
    if !junction(&root.join("links/repo"), &outside.path().join("repo")) {
        eprintln!("junction could not be made here; skipped");
        return;
    }
    permark(root, "");
    fs::write(
        root.join(".permark/projects.toml"),
        "[[project]]\nid = \"jjjjjjjjjj\"\npath = \"links/repo\"\n",
    )
    .unwrap();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    let view = ready(&core);
    assert!(view.rows.is_empty(), "never found, never listed: {view:#?}");
    assert_eq!(
        view.skipped,
        vec![SkippedEntry {
            line: 1,
            reason: RecordSkip::Link
        }]
    );
    assert_eq!(core.projects().snapshot().members().len(), 0);
}

#[test]
fn case_twins_and_a_long_root_path() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    // a root deeper than 260 characters (Windows long paths)
    let mut root = dir.path().to_path_buf();
    while root.as_os_str().len() < 280 {
        root.push("a-rather-long-folder-name");
    }
    fs::create_dir_all(&root).unwrap();
    git(&root, "Hub");
    if !crate::paths::CASE_INSENSITIVE {
        git(&root, "hub");
    }
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    let view = ready(&core);
    let want = if crate::paths::CASE_INSENSITIVE { 1 } else { 2 };
    assert_eq!(view.rows.len(), want, "{view:#?}");
    core.projects_include(&env, &ProjectPick::All).unwrap();
    let view = core.projects_view();
    assert!(view.rows.iter().all(|r| r.state == MemberState::Included));
    assert_eq!(core.projects().held_members(), want);
}

/// Spec §13 perf (counting test): a plain folder costs the projects service nothing after open. Open = one record probe
/// and one stat per ancestor level (opened-alone rules, <= 32); discovery after the walk, rescans and folder batches
/// touch the disk 0 times (the walk finds markers by name, no extra syscall).
#[test]
fn a_plain_folder_costs_no_disk_touch_after_open() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for i in 0..20 {
        mkdirs(dir.path(), &format!("docs/d{i}"));
        fs::write(dir.path().join(format!("docs/d{i}/a.md")), "# a").unwrap();
    }
    let root = Root::open(dir.path()).unwrap();
    let levels = root.path().ancestors().count() as u64;
    let config = ProjectConfig::load(&root);
    let sink: Arc<dyn EventSink> = Arc::new(crate::events::NoEvents);
    let explorer = Arc::new(Explorer::new(
        &crate::paths::Vault::new(root.clone()),
        &config,
        Arc::clone(&sink),
        None,
    ));
    let before = probe::count();
    let projects = Projects::open(
        &root,
        &AppDirs::under(base.path()),
        Arc::clone(&explorer),
        Arc::new(OwnWrites::new()),
        WatchHub::new(),
        sink,
        false,
        &config.ignore,
    );
    let at_open = probe::count() - before;
    assert!(
        at_open <= 1 + levels.min(MAX_ANCESTORS as u64),
        "open: {at_open} touches for {levels} levels"
    );
    let markers = explorer.markers();
    let before = probe::count();
    projects.discover(&Hint::default(), true, &markers);
    projects.on_hint(
        &Hint {
            rescan: true,
            folders: true,
            ..Hint::default()
        },
        &markers,
    );
    assert_eq!(probe::count() - before, 0, "no disk touch after open");
    let map = projects.snapshot();
    assert!(map.ready && map.is_plain());
}

/// Concept §2b + spec §5.2 (S1): opened alone, every chain level's rules apply and a change of any of them (record,
/// `.aiignore`, config) shows at the next read once the 5 s cache is old.
#[test]
fn opened_alone_rules_follow_every_chain_level() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let r = dunce::canonicalize(dir.path()).unwrap();
    let r = r.as_path();
    permark(r, "");
    fs::write(
        r.join(".permark/projects.toml"),
        "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"hubs/a\"\n",
    )
    .unwrap();
    git(r, "hubs/a");
    mkdirs(r, "hubs/a/sub/docs");
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(&r.join("hubs/a/sub/docs")).unwrap(), &env);
    ready(&core);
    let projects = core.projects();
    let bases = |p: &Projects| -> Vec<(String, String)> {
        let map = p.agent_map(Duration::from_secs(5)).unwrap();
        let mut out: Vec<(String, String)> = map
            .ancestors
            .iter()
            .map(|a| {
                (
                    a.base
                        .strip_prefix(r)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    a.rel.clone(),
                )
            })
            .collect();
        out.sort();
        out
    };
    // a folder below a listed project: the root + the listed project on the chain
    assert_eq!(
        bases(projects),
        vec![
            (String::new(), "hubs/a/sub/docs".to_owned()),
            ("hubs/a".to_owned(), "sub/docs".to_owned())
        ]
    );
    let stale = |p: &Projects| {
        lock(&p.inner).ancestors.1 =
            Instant::now().checked_sub(ANCESTORS_TTL + Duration::from_secs(1));
    };
    let rules_of = |p: &Projects, rel: &str| {
        let want = if rel.is_empty() {
            r.to_path_buf()
        } else {
            r.join(rel)
        };
        p.snapshot()
            .ancestors
            .iter()
            .find(|a| a.base == want)
            .map(|a| a.rules.clone())
            .unwrap()
    };
    // a middle level's `.aiignore` changes: seen once the cache is old, not before
    fs::write(r.join("hubs/a/.aiignore"), "notes/\n").unwrap();
    let _ = projects.agent_map(Duration::ZERO);
    assert!(rules_of(projects, "hubs/a").aiignore.is_empty(), "cached");
    stale(projects);
    let _ = projects.agent_map(Duration::ZERO);
    assert_eq!(
        rules_of(projects, "hubs/a").aiignore,
        vec!["notes/".to_owned()]
    );
    // the root's config changes
    fs::write(
        r.join(".permark/config.toml"),
        "[agent]\ndeny_paths = [\"drafts/\"]\n",
    )
    .unwrap();
    stale(projects);
    let _ = projects.agent_map(Duration::ZERO);
    assert_eq!(
        rules_of(projects, "").deny_paths,
        vec!["drafts/".to_owned()]
    );
    // the record drops the project: no rules any more
    fs::write(r.join(".permark/projects.toml"), "version = 1\n").unwrap();
    stale(projects);
    assert_eq!(bases(projects), Vec::<(String, String)>::new());
}

/// D28 (S3): the person's "No thanks" beats an entry written later; the Changes panel says so; including it in
/// Settings uses the entry as written; undo puts the refusal back and keeps the entry.
#[test]
fn a_refused_project_listed_later_stays_out_until_included() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, "hubs/x");
    permark(root, "");
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    ready(&core);
    let news = Arc::new(Mutex::new(Vec::new()));
    let _sub = core.projects().subscribe({
        let news = Arc::clone(&news);
        move |e| news.lock().unwrap().push(e.clone())
    });
    core.projects().decline(&ProjectPick::All).unwrap();
    let record = "version = 1\n\n[[project]]\nid = \"xxxxxxxxxx\"\npath = \"hubs/x\"\n";
    fs::write(root.join(".permark/projects.toml"), record).unwrap();
    wait("kept out news", || {
        news.lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, ProjectEvent::KeptOut { path, .. } if path == "hubs/x"))
    });
    let view = core.projects_view();
    let found = row(&view, "hubs/x").unwrap();
    assert_eq!(found.state, MemberState::Declined);
    assert_eq!(
        found.uid.as_ref().map(ProjectUid::as_str),
        Some("xxxxxxxxxx")
    );
    assert!(core.projects().snapshot().closed_to_agent("hubs/x/doc.md"));
    assert_eq!(core.projects().held_members(), 0);
    // included in Settings: the entry as written, no record write
    let (outcome, _) = core.projects_include(&env, &paths(&["hubs/x"])).unwrap();
    assert_eq!(outcome.included, 1);
    assert_eq!(
        fs::read_to_string(root.join(".permark/projects.toml")).unwrap(),
        record
    );
    assert_eq!(
        state(&core.projects_view(), "hubs/x"),
        Some(MemberState::Included)
    );
    assert!(!core.projects().snapshot().closed_to_agent("hubs/x/doc.md"));
    // undo: refused again, the entry stays
    core.projects_undo(&env, outcome.undo.unwrap()).unwrap();
    assert_eq!(
        state(&core.projects_view(), "hubs/x"),
        Some(MemberState::Declined)
    );
    assert_eq!(
        fs::read_to_string(root.join(".permark/projects.toml")).unwrap(),
        record
    );
}

/// N2: a record path whose segment becomes a junction after the record was read counts as gone at use (nothing read
/// from beyond it).
#[cfg(windows)]
#[test]
fn a_segment_turned_into_a_junction_later_makes_the_member_gone() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, "hubs/a");
    permark(root, "");
    fs::write(
        root.join(".permark/projects.toml"),
        "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"hubs/a\"\n",
    )
    .unwrap();
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(root).unwrap(), &env);
    let view = ready(&core);
    assert_eq!(state(&view, "hubs/a"), Some(MemberState::Included));
    // the real folder leaves the opened folder (a rename inside it is a move the member follows: same folder on disk);
    // held for the grace window (the clock stopped), then a junction takes its place before the window ends
    core.projects().freeze_clock();
    let real = base.path().join("real-hubs");
    fs::rename(root.join("hubs"), &real).unwrap();
    wait("held", || core.projects().vanished_count() == 1);
    if !junction(&root.join("hubs"), &real) {
        eprintln!("junction could not be made here; skipped");
        return;
    }
    core.projects().advance(grace::GRACE);
    wait("member gone through the junction", || {
        state(&core.projects_view(), "hubs/a") == Some(MemberState::Missing)
    });
    assert_eq!(core.projects().held_members(), 0);
}

/// N3: closing the tab while the first walk runs stops the worker: it holds nothing of the service while it waits.
#[test]
fn closing_during_the_walk_lets_the_service_go_at_once() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for i in 0..200 {
        mkdirs(dir.path(), &format!("bulk/d{i}"));
        for j in 0..50 {
            fs::write(dir.path().join(format!("bulk/d{i}/f{j}.md")), "x").unwrap();
        }
    }
    let (env, _sink) = env(base.path());
    let core = ProjectCore::open(Root::open(dir.path()).unwrap(), &env);
    let explorer = Arc::clone(core.explorer());
    let weak = Arc::downgrade(core.projects());
    let started = Instant::now();
    drop(core);
    let walking = explorer.walking();
    assert!(
        weak.upgrade().is_none(),
        "service released (walk running: {walking})"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "close never waits for the walk"
    );
}

/// Live moves (real fs renames under the real watch).
#[path = "rename_tests.rs"]
mod rename;
