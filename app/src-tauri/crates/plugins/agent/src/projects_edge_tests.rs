//! Agent narrowing edge cases found by the verifier (round 2 findings B1, S1, S2, S3), kept as spec tests: each asserts
//! the fixed behavior.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::paths::Root;
use permark_core::plugins::PluginRegistry;
use permark_core::projects::{AgentChoice, ProjectPick, ProjectUid};
use permark_core::settings::AppSettings;
use permark_core::state::{AppDirs, StateStore};
use permark_core::workspace::{CoreEnv, ProjectCore};
use serde_json::json;

use crate::perms::{ProjectChoice, PROJECT_FILE};
use crate::tools::Tools;
use crate::ToolError;

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

fn env(dir: &Path) -> Arc<CoreEnv> {
    Arc::new(CoreEnv::new(
        AppDirs::under(&dir.join("state")),
        Arc::new(PluginRegistry::new(vec![crate::plugin()]).unwrap()),
        Arc::new(permark_core::events::NoEvents),
        Distribution::Direct,
        AppSettings::default(),
    ))
}

fn share(env: &CoreEnv, root: &Path) {
    StateStore::for_project(&env.dirs, &Root::open(root).unwrap())
        .profile_file::<ProjectChoice>(PROJECT_FILE)
        .save(&ProjectChoice {
            enabled: true,
            ..ProjectChoice::default()
        })
        .unwrap();
}

fn open_ready(env: &CoreEnv, root: &Path) -> Arc<ProjectCore> {
    let core = ProjectCore::open(Root::open(root).unwrap(), env);
    let start = Instant::now();
    while !core.projects_view().ready {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(20));
    }
    core
}

fn tools(core: &Arc<ProjectCore>) -> Tools {
    Tools::new(core.vault().clone(), core.store(), core.services(), None)
}

fn read(tools: &Tools, path: &str) -> Result<(), ToolError> {
    tools.call("read", json!({ "path": path })).map(|_| ())
}

fn tmp() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new().prefix("pmv").tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("R");
    (dir, root)
}

/// Runs a test body on its own thread: a hang fails the test after `limit` instead of holding the whole run (CI
/// 39698724615 hung 40 min on Windows).
fn bounded(limit: Duration, body: impl FnOnce() + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::channel();
    let runner = std::thread::spawn(move || {
        body();
        let _ = tx.send(());
    });
    match rx.recv_timeout(limit) {
        Ok(()) => runner.join().unwrap(),
        // the body panicked: its message is the test's
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(runner.join().unwrap_err())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("hung for over {limit:?}"),
    }
}

/// A big tree (30 000 documents) so the walk + the index take a while.
fn big_tree(root: &Path) {
    for i in 0..300 {
        for j in 0..100 {
            let p = root.join(format!("bulk/d{i}/f{j}.md"));
            if j == 0 {
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            }
            std::fs::write(p, "x").unwrap();
        }
    }
}

/// A test with a [`big_tree`]: written in ~30 s on a Windows CI runner; the rest takes seconds.
const BIG_TREE_LIMIT: Duration = Duration::from_secs(150);

/// Concept §2b: private-file rules combine along the chain, also when a governed project is opened alone.
/// `R` lists `hubs/a` and `hubs/a/sub`; `hubs/a/.aiignore` = `notes/`. In R's tab `hubs/a/sub/notes/n.md` is hidden
/// (a on the chain). Opened alone, `sub` should keep that rule.
#[test]
fn opened_alone_keeps_middle_chain_members_rules() {
    let (dir, root) = tmp();
    for (p, t) in [
        (
            ".permark/projects.toml",
            "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"hubs/a\"\n\n[[project]]\nid = \"ssssssssss\"\npath = \"hubs/a/sub\"\n",
        ),
        ("hubs/a/.git/HEAD", "ref: refs/heads/main\n"),
        ("hubs/a/.aiignore", "notes/\n"),
        ("hubs/a/sub/.git/HEAD", "ref: refs/heads/main\n"),
        ("hubs/a/sub/notes/n.md", "# n\n"),
        ("hubs/a/sub/ok.md", "# ok\n"),
    ] {
        write(&root, p, t);
    }
    let env = env(dir.path());
    share(&env, &root);
    let rcore = open_ready(&env, &root);
    let in_root = read(&tools(&rcore), "hubs/a/sub/notes/n.md");
    assert!(
        matches!(in_root, Err(ToolError::NotFound(_))),
        "R tab: {in_root:?}"
    );
    drop(rcore);
    let sub = root.join("hubs/a/sub");
    share(&env, &sub);
    let score = open_ready(&env, &sub);
    let t = tools(&score);
    assert!(read(&t, "ok.md").is_ok());
    let alone = read(&t, "notes/n.md");
    drop(score);
    assert!(
        matches!(alone, Err(ToolError::NotFound(_))),
        "opened alone: a's .aiignore lost -> {alone:?}"
    );
}

/// A subfolder of an included member opened alone keeps R's private-file rule (`drafts/`): the record lists a folder
/// containing it.
#[test]
fn subfolder_of_member_opened_alone_keeps_root_rules() {
    let (dir, root) = tmp();
    for (p, t) in [
        (
            ".permark/projects.toml",
            "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"hubs/a\"\n",
        ),
        (".aiignore", "drafts/\n"),
        ("hubs/a/.git/HEAD", "ref: refs/heads/main\n"),
        ("hubs/a/docs/drafts/d.md", "# d\n"),
    ] {
        write(&root, p, t);
    }
    let env = env(dir.path());
    let docs = root.join("hubs/a/docs");
    share(&env, &docs);
    let core = open_ready(&env, &docs);
    let r = read(&tools(&core), "drafts/d.md");
    drop(core);
    assert!(
        matches!(r, Err(ToolError::NotFound(_))),
        "subfolder alone: {r:?}"
    );
}

/// A member switched off looks absent: its `[agent] deny` never answers `denied below <member>` (no path leak).
#[test]
fn off_member_with_deny_answers_not_found() {
    let (dir, root) = tmp();
    for (p, t) in [
        (
            ".permark/projects.toml",
            "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"hubs/a\"\n",
        ),
        ("hubs/a/.git/HEAD", "ref: refs/heads/main\n"),
        (
            "hubs/a/.permark/config.toml",
            "[agent]\ndeny = [\"find\"]\n",
        ),
        ("hubs/a/README.md", "# a\n"),
    ] {
        write(&root, p, t);
    }
    let env = env(dir.path());
    share(&env, &root);
    let core = open_ready(&env, &root);
    core.projects()
        .set_agent(&ProjectUid::parse("aaaaaaaaaa").unwrap(), AgentChoice::Off)
        .unwrap();
    let r = read(&tools(&core), "hubs/a/README.md");
    let r2 = read(&tools(&core), "hubs/a/no-such-file.md");
    drop(core);
    assert!(
        matches!(r, Err(ToolError::NotFound(_))),
        "off member: {r:?}"
    );
    assert!(
        matches!(r2, Err(ToolError::NotFound(_))),
        "off member, missing file: {r2:?}"
    );
}

/// Before the first walk ends the found set is unknown: the agent waits, then answers `busy`; never reads a
/// not-included project.
#[test]
fn not_included_project_never_readable_before_walk_ends() {
    bounded(
        BIG_TREE_LIMIT,
        not_included_project_never_readable_before_walk_ends_body,
    );
}

fn not_included_project_never_readable_before_walk_ends_body() {
    let (dir, root) = tmp();
    write(&root, "hubs/x/.git/HEAD", "ref: refs/heads/main\n");
    write(&root, "hubs/x/secret.md", "# secret\n");
    big_tree(&root);
    let env = env(dir.path());
    share(&env, &root);
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    let t = tools(&core);
    let early_ready = core.projects_view().ready;
    let early = read(&t, "hubs/x/secret.md");
    let start = Instant::now();
    while !core.projects_view().ready {
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(20));
    }
    let late = read(&tools(&core), "hubs/x/secret.md");
    drop(core);
    assert!(
        matches!(late, Err(ToolError::NotFound(_))),
        "after walk: {late:?}"
    );
    assert!(
        early_ready || matches!(early, Err(ToolError::NotFound(_) | ToolError::Busy)),
        "before the walk ended the agent read a not-included project: {early:?}"
    );
}

/// Person said "No thanks" (declined all); a record arriving by pull | agent edit lists the project -> it stays out
/// and hidden from the agent (spec §2.1 + D28: the person's answer beats a later entry).
#[test]
fn record_written_after_decline_keeps_project_out() {
    let (dir, root) = tmp();
    write(&root, "hubs/x/.git/HEAD", "ref: refs/heads/main\n");
    write(&root, "hubs/x/doc.md", "# x\n");
    write(&root, ".permark/config.toml", "");
    let env = env(dir.path());
    share(&env, &root);
    let core = open_ready(&env, &root);
    core.projects().decline(&ProjectPick::All).unwrap();
    assert!(matches!(
        read(&tools(&core), "hubs/x/doc.md"),
        Err(ToolError::NotFound(_))
    ));
    write(
        &root,
        ".permark/projects.toml",
        "version = 1\n\n[[project]]\npath = \"hubs/x\"\n",
    );
    let start = Instant::now();
    let mut opened = false;
    while start.elapsed() < Duration::from_secs(10) {
        if read(&tools(&core), "hubs/x/doc.md").is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(core);
    assert!(
        !opened,
        "a file write overrode the person's No thanks for the agent"
    );
}

/// Files mode (`permark mcp`, app closed): the first call waits for the walk the same way.
#[test]
fn files_mode_first_call_never_reads_not_included_project() {
    bounded(BIG_TREE_LIMIT, files_mode_first_call_body);
}

fn files_mode_first_call_body() {
    let (dir, root) = tmp();
    write(&root, "hubs/x/.git/HEAD", "ref: refs/heads/main\n");
    write(&root, "hubs/x/secret.md", "# secret\n");
    big_tree(&root);
    let env = env(dir.path());
    share(&env, &root);
    let core = ProjectCore::from_files(
        Root::open(&root).unwrap(),
        &env,
        std::slice::from_ref(&crate::ID),
    );
    let early_ready = core.projects_view().ready;
    let early = read(&tools(&core), "hubs/x/secret.md");
    drop(core);
    assert!(
        early_ready || matches!(early, Err(ToolError::NotFound(_) | ToolError::Busy)),
        "files mode, first call read a not-included project: {early:?}"
    );
}

/// Fail closed (B1): a map that is not ready hides every path, lists included; the published context waits for nothing.
#[test]
fn a_map_not_ready_hides_everything() {
    let (_dir, root) = tmp();
    write(
        &root, "a.md", "# a
",
    );
    let guard = crate::guard::Guard::new(&Root::open(&root).unwrap(), &[]).with_projects(
        Arc::new(permark_core::projects::ProjectMap::default()),
        None,
    );
    assert!(guard.hidden("a.md", false));
    assert!(matches!(guard.file("a.md"), Err(ToolError::NotFound(_))));
    assert_eq!(
        crate::render::error("read", &ToolError::Busy),
        "ERR busy: PerMark still reading this folder -> retry in a few seconds"
    );
}
