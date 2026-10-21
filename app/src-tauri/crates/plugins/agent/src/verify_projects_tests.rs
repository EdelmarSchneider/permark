//! Verifier adversarial tests (branch `verify/projects-round2`, not for merge): agent narrowing edge cases.

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

/// Concept §2b: private-file rules combine along the chain, also when a governed project is opened alone.
/// `R` lists `hubs/a` and `hubs/a/sub`; `hubs/a/.aiignore` = `notes/`. In R's tab `hubs/a/sub/notes/n.md` is hidden
/// (a on the chain). Opened alone, `sub` should keep that rule.
#[test]
fn v1_opened_alone_keeps_middle_chain_members_rules() {
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

/// A subfolder of an included member opened alone: R's private-file rule (`drafts/`) no longer applies (exact key
/// match only).
#[test]
fn v1b_subfolder_of_member_opened_alone_loses_root_rules() {
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

/// A member switched off must look absent: a `[agent] deny` inside it must not answer `denied below <member>`.
#[test]
fn v2_off_member_with_deny_leaks_its_path() {
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

/// Before the first walk ends the map does not know the found set: a not-included project must not be readable.
#[test]
fn v3_not_included_project_readable_before_walk_ends() {
    let (dir, root) = tmp();
    write(&root, "hubs/x/.git/HEAD", "ref: refs/heads/main\n");
    write(&root, "hubs/x/secret.md", "# secret\n");
    // a big tree so the walk takes a while
    for i in 0..300 {
        for j in 0..100 {
            let p = root.join(format!("bulk/d{i}/f{j}.md"));
            if j == 0 {
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            }
            std::fs::write(p, "x").unwrap();
        }
    }
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
        early_ready || matches!(early, Err(ToolError::NotFound(_))),
        "before the walk ended the agent read a not-included project: {early:?}"
    );
}

/// Person said "No thanks" (declined all); a record arriving by pull | agent edit lists the project -> the agent sees
/// it (spec §2.1 "valid as written"; vision: nothing inside a project switches sharing on). Documents the behavior.
#[test]
fn v4_record_written_after_decline_opens_project_to_agent() {
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

/// Files mode (`permark mcp`, app closed): the first call races the walk the same way.
#[test]
fn v5_files_mode_first_call_reads_not_included_project() {
    let (dir, root) = tmp();
    write(&root, "hubs/x/.git/HEAD", "ref: refs/heads/main\n");
    write(&root, "hubs/x/secret.md", "# secret\n");
    for i in 0..300 {
        for j in 0..100 {
            let p = root.join(format!("bulk/d{i}/f{j}.md"));
            if j == 0 {
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            }
            std::fs::write(p, "x").unwrap();
        }
    }
    let env = env(dir.path());
    share(&env, &root);
    let core = ProjectCore::from_files(Root::open(&root).unwrap(), &env, &[crate::ID.clone()]);
    let early_ready = core.projects_view().ready;
    let early = read(&tools(&core), "hubs/x/secret.md");
    drop(core);
    assert!(
        early_ready || matches!(early, Err(ToolError::NotFound(_))),
        "files mode, first call read a not-included project: {early:?}"
    );
}
