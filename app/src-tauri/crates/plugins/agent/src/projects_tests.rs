//! Agent narrowing by the projects inside a folder (`projects.md` §5.2, worked examples §5.4): what an agent with the
//! root shared may read, list and search.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::paths::Root;
use permark_core::plugins::PluginRegistry;
use permark_core::projects::{AgentChoice, ProjectUid};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
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

struct Fx {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    env: Arc<CoreEnv>,
    core: Option<Arc<ProjectCore>>,
}

impl Drop for Fx {
    fn drop(&mut self) {
        self.core.take();
    }
}

impl Fx {
    fn core(&self) -> &Arc<ProjectCore> {
        self.core.as_ref().unwrap()
    }

    fn tools(&self) -> Tools {
        Tools::new(
            self.core().vault().clone(),
            self.core().store(),
            self.core().services(),
            None,
        )
    }
}

/// Root `R` of §5.4: `.gitignore` `hubs/` + `*.env`; config `ignore = ["scratch/"]`; `.aiignore` `/private/` +
/// `drafts/`. Members (listed): `hubs/a` (git; `.aiignore` `notes/`), `hubs/a/sub` (git). Found, not included:
/// `hubs/x` (git), `tools/y` (`.permark` only).
fn fx() -> Fx {
    let dir = tempfile::Builder::new().prefix("pmp").tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap().join("R");
    for (path, text) in [
        (".gitignore", "hubs/\n*.env\n"),
        (".permark/config.toml", "ignore = [\"scratch/\"]\n"),
        (".aiignore", "/private/\ndrafts/\n"),
        (
            ".permark/projects.toml",
            "version = 1\n\n[[project]]\nid = \"aaaaaaaaaa\"\npath = \"hubs/a\"\n\n[[project]]\nid = \"ssssssssss\"\npath = \"hubs/a/sub\"\nhidden = \"own+root\"\n",
        ),
        ("hubs/a/.git/HEAD", "ref: refs/heads/main\n"),
        ("hubs/a/.gitignore", "dist/\n"),
        ("hubs/a/.aiignore", "notes/\n"),
        ("hubs/a/README.md", "# A\n\nlaunch\n"),
        ("hubs/a/notes/plan.md", "# Plan\n\nlaunch\n"),
        ("hubs/a/private/k.md", "# K\n\nlaunch\n"),
        ("hubs/a/drafts/d.md", "# D\n\nlaunch\n"),
        ("hubs/a/sub/.git/HEAD", "ref: refs/heads/main\n"),
        ("hubs/a/sub/x.md", "# X\n\nlaunch\n"),
        ("hubs/x/.git/HEAD", "ref: refs/heads/main\n"),
        ("hubs/x/doc.md", "# X doc\n\nlaunch\n"),
        ("tools/y/.permark/config.toml", ""),
        ("tools/y/doc.md", "# Y doc\n\nlaunch\n"),
        ("top.md", "# Top\n\nlaunch\n"),
    ] {
        write(&root, path, text);
    }
    let dirs = AppDirs::under(&dir.path().join("state"));
    let env = Arc::new(CoreEnv::new(
        dirs,
        Arc::new(PluginRegistry::new(vec![crate::plugin()]).unwrap()),
        Arc::new(permark_core::events::NoEvents),
        Distribution::Direct,
        AppSettings::default(),
    ));
    let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
    let start = Instant::now();
    while !core.projects_view().ready {
        assert!(start.elapsed() < Duration::from_secs(15));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(core.search().wait_ready(Duration::from_secs(15)));
    core.store()
        .profile_file::<ProjectChoice>(PROJECT_FILE)
        .save(&ProjectChoice {
            enabled: true,
            ..ProjectChoice::default()
        })
        .unwrap();
    Fx {
        _dir: dir,
        root,
        env,
        core: Some(core),
    }
}

fn reads(tools: &Tools, path: &str) -> Result<(), ToolError> {
    tools.call("read", json!({ "path": path })).map(|_| ())
}

fn hidden(tools: &Tools, path: &str) -> bool {
    matches!(reads(tools, path), Err(ToolError::NotFound(_)))
}

#[test]
fn worked_examples_of_the_agent_rules() {
    let fx = fx();
    let tools = fx.tools();
    // shared members: their files, minus their own and the root's private files (union along the chain)
    assert!(reads(&tools, "hubs/a/README.md").is_ok());
    assert!(hidden(&tools, "hubs/a/notes/plan.md"), "member .aiignore");
    assert!(
        reads(&tools, "hubs/a/private/k.md").is_ok(),
        "root `/private/` is anchored at the root"
    );
    assert!(
        hidden(&tools, "hubs/a/drafts/d.md"),
        "root `drafts/` at any depth"
    );
    assert!(reads(&tools, "hubs/a/sub/x.md").is_ok());
    // found, not included: hidden, also in visible folders (director Q6)
    assert!(hidden(&tools, "hubs/x/doc.md"));
    assert!(hidden(&tools, "tools/y/doc.md"));
    assert!(reads(&tools, "top.md").is_ok());
    // search never lists hidden paths
    let found = tools
        .call("search", json!({ "query": "launch", "limit": 50 }))
        .unwrap()
        .to_string();
    for no in ["hubs/x/", "tools/y/", "hubs/a/notes/", "hubs/a/drafts/"] {
        assert!(!found.contains(no), "{no} in {found}");
    }
    assert!(found.contains("hubs/a/README.md"), "{found}");
}

#[test]
fn a_member_switched_off_hides_its_chain_and_comes_back() {
    let fx = fx();
    let a = ProjectUid::parse("aaaaaaaaaa").unwrap();
    fx.core()
        .projects()
        .set_agent(&a, AgentChoice::Off)
        .unwrap();
    let tools = fx.tools();
    assert!(hidden(&tools, "hubs/a/README.md"));
    assert!(hidden(&tools, "hubs/a/sub/x.md"), "off on the chain (AND)");
    assert!(reads(&tools, "top.md").is_ok());
    fx.core()
        .projects()
        .set_agent(&a, AgentChoice::Same)
        .unwrap();
    assert!(reads(&fx.tools(), "hubs/a/sub/x.md").is_ok());
    // "No thanks" for a member hides it at once
    fx.core().projects().leave_out(&[a]).unwrap();
    assert!(hidden(&fx.tools(), "hubs/a/README.md"));
}

#[test]
fn a_members_denied_group_omits_rows_and_denies_direct_calls() {
    let fx = fx();
    write(
        &fx.root,
        "hubs/a/.permark/config.toml",
        "[agent]\ndeny = [\"find\"]\n",
    );
    // the member reads its files again when its config changes (watch); wait for it
    let start = Instant::now();
    loop {
        match reads(&fx.tools(), "hubs/a/README.md") {
            Err(ToolError::DeniedBelow { member, .. }) => {
                assert_eq!(member, "hubs/a");
                break;
            }
            other => {
                assert!(start.elapsed() < Duration::from_secs(15), "still {other:?}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    let err = reads(&fx.tools(), "hubs/a/sub/x.md").unwrap_err();
    assert_eq!(
        crate::render::error("read", &err),
        "ERR denied: group find off below hubs/a -> ask person"
    );
    let found = fx
        .tools()
        .call("search", json!({ "query": "launch", "limit": 50 }))
        .unwrap()
        .to_string();
    assert!(!found.contains("hubs/a/"), "{found}");
    assert!(found.contains("top.md"));
}

#[test]
fn opened_alone_a_member_keeps_its_roots_private_files() {
    let fx = fx();
    write(&fx.root, "hubs/a/drafts/again.md", "# again\n");
    let member = fx.root.join("hubs/a");
    let core = ProjectCore::open(Root::open(&member).unwrap(), &fx.env);
    let start = Instant::now();
    while !core.projects_view().ready {
        assert!(start.elapsed() < Duration::from_secs(15));
        std::thread::sleep(Duration::from_millis(20));
    }
    core.store()
        .profile_file::<ProjectChoice>(PROJECT_FILE)
        .save(&ProjectChoice {
            enabled: true,
            ..ProjectChoice::default()
        })
        .unwrap();
    let tools = Tools::new(core.vault().clone(), core.store(), core.services(), None);
    assert!(
        hidden(&tools, "drafts/d.md"),
        "R's `drafts/` holds inside `a` opened alone"
    );
    assert!(
        reads(&tools, "private/k.md").is_ok(),
        "`/private/` matches nothing inside `a`"
    );
    assert!(hidden(&tools, "notes/plan.md"), "its own .aiignore");
    assert!(reads(&tools, "README.md").is_ok());
    drop(core);
}
