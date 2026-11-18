//! The catalog snapshot, how each agent would start with a `PATH` of the test's own, and the person's choice.

use std::path::Path;

use permark_core::paths::Root;
use permark_core::state::{AppDirs, StateLocation, StateStore};
use serde_json::{json, Value};

use super::*;
use crate::testing;

fn listed(id: &str) -> &'static Listed {
    snapshot()
        .iter()
        .find(|listed| listed.id == id)
        .unwrap_or_else(|| panic!("{id} not in the snapshot"))
}

/// Windows names files without regard to case (`PATHEXT` gives `.CMD`).
fn same_file(a: &Path, b: &Path) {
    assert_eq!(
        a.to_string_lossy().to_lowercase(),
        b.to_string_lossy().to_lowercase()
    );
}

fn agent(id: &str) -> ChooseArgs {
    ChooseArgs {
        agent: Some(id.to_owned()),
        command: None,
    }
}

fn command(typed: &str) -> ChooseArgs {
    ChooseArgs {
        agent: None,
        command: Some(typed.to_owned()),
    }
}

/// Every file name under `dir`.
fn names_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(names_under(&path));
        } else {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out
}

#[test]
fn the_snapshot_parses_and_lists_the_main_agents() {
    let all = snapshot();
    assert!(all.len() >= 40, "{}", all.len());
    for id in ["claude-acp", "codex-acp", "gemini"] {
        assert!(all.iter().any(|listed| listed.id == id), "{id}");
    }
    assert_eq!(listed("claude-acp").name, "Claude Agent");
    let raw: Value = serde_json::from_str(SNAPSHOT).unwrap();
    assert_eq!(raw["license"], "Apache-2.0");
    assert!(raw["notice"]
        .as_str()
        .unwrap()
        .contains("Apache License 2.0"));
    assert!(raw["source"]
        .as_str()
        .unwrap()
        .starts_with("https://cdn.agentclientprotocol.com/registry/"));
    assert!(raw["fetched"].is_string());
}

#[test]
fn agents_without_a_way_to_run_here_are_left_out() {
    let ids = |platforms: &[&str]| -> Vec<String> {
        catalog(platforms).map(|listed| listed.id.clone()).collect()
    };
    // Kiro ships no Windows build and no package
    let windows = ids(&["windows-x86_64"]);
    assert!(!windows.contains(&"kiro".to_owned()));
    // an own build | a package (any platform)
    assert!(windows.contains(&"goose".to_owned()));
    assert!(windows.contains(&"gemini".to_owned()));
    assert!(ids(&["linux-x86_64"]).contains(&"kiro".to_owned()));
    let unknown = ids(&[]);
    assert!(!unknown.contains(&"goose".to_owned()));
    assert!(unknown.contains(&"fast-agent".to_owned()));
}

/// Own program on `PATH` first, then `npx`, then `uvx`; else what to install. Only `PATH` is looked at.
#[test]
fn each_agent_starts_the_best_way_this_computer_has() {
    let bin = tempfile::tempdir().unwrap();
    let find = testing::find_in(bin.path());
    let here = platforms();
    let resolve = |id: &str| resolve_listed(listed(id), here, &find);

    let claude = resolve("claude-acp");
    assert_eq!(
        (claude.how, claude.needs),
        (AgentHow::Missing, Some(AgentNeed::Node))
    );
    assert_eq!(
        claude.shown,
        "npx -y @agentclientprotocol/claude-agent-acp@0.108.0"
    );
    assert_eq!(claude.plan, None);
    let fast = resolve("fast-agent");
    assert_eq!(
        (fast.how, fast.needs),
        (AgentHow::Missing, Some(AgentNeed::Uv))
    );
    let goose = resolve("goose");
    assert_eq!(
        (goose.how, goose.needs),
        (AgentHow::Missing, Some(AgentNeed::Install))
    );

    let npx = testing::program(bin.path(), "npx");
    let claude = resolve("claude-acp");
    assert_eq!((claude.how, claude.needs), (AgentHow::Npx, None));
    let plan = claude.plan.unwrap();
    same_file(&plan.program, &npx);
    assert_eq!(
        plan.args,
        ["-y", "@agentclientprotocol/claude-agent-acp@0.108.0"]
    );
    assert_eq!(plan.shown, claude.shown);
    assert_eq!(plan.env, Vec::<(String, String)>::new());
    let gemini = resolve("gemini").plan.unwrap();
    assert_eq!(gemini.args, ["-y", "@google/gemini-cli@0.68.0", "--acp"]);
    assert_eq!(gemini.shown, "npx -y @google/gemini-cli@0.68.0 --acp");

    // the registry's variables ride along
    let uvx = testing::program(bin.path(), "uvx");
    let fast = resolve("fast-agent");
    assert_eq!(fast.how, AgentHow::Uvx);
    let plan = fast.plan.unwrap();
    same_file(&plan.program, &uvx);
    assert_eq!(plan.args, ["fast-agent-acp==0.10.1", "-x"]);
    assert_eq!(
        plan.env,
        [("FAST_AGENT_MODEL".to_owned(), "codexplan".to_owned())]
    );

    // the agent's own program, by the file name of its build, comes before npx
    if let Some(build) = listed("claude-acp").build(here) {
        let own = testing::program(bin.path(), file_name(&build.cmd));
        let claude = resolve("claude-acp");
        assert_eq!((claude.how, claude.needs), (AgentHow::Native, None));
        let plan = claude.plan.unwrap();
        same_file(&plan.program, &own);
        assert_eq!(claude.shown, file_name(&build.cmd));
    }
    if let Some(build) = listed("goose").build(here) {
        testing::program(bin.path(), file_name(&build.cmd));
        let goose = resolve("goose");
        assert_eq!(goose.how, AgentHow::Native);
        assert_eq!(goose.plan.unwrap().args, ["acp"]);
    }
}

#[test]
fn registry_paths_name_the_program_file() {
    assert_eq!(file_name("./goose-package\\goose.exe"), "goose.exe");
    assert_eq!(file_name("./kirocli/bin/kiro-cli"), "kiro-cli");
    assert_eq!(file_name("vtcode.exe"), "vtcode.exe");
}

/// The choice goes to the profile folder itself, also when the project has its own state folder inside it.
#[test]
fn the_choice_lives_in_the_profile_folder_never_in_the_project() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".permark")).unwrap();
    std::fs::write(project.path().join("notes.md"), "n\n").unwrap();
    let profile = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let dirs = AppDirs {
        profile: profile.path().to_path_buf(),
        cache: profile.path().join("cache"),
    };
    let store = StateStore::for_project(&dirs, &Root::open(project.path()).unwrap());
    assert_eq!(store.location(), StateLocation::Project);
    let agents = Agents::with(
        Some(store.app_file(CHOICE_FILE)),
        testing::find_in(bin.path()),
        platforms(),
    );
    assert_eq!(agents.chosen(), None);
    agents.choose(&agent("gemini")).unwrap();
    let saved: Value =
        serde_json::from_slice(&std::fs::read(profile.path().join(CHOICE_FILE)).unwrap()).unwrap();
    assert_eq!(saved["data"], json!({ "agent": "gemini" }));
    assert!(!names_under(project.path()).contains(&CHOICE_FILE.to_owned()));
    let chosen = agents.chosen().unwrap();
    assert_eq!(chosen.name, "Gemini CLI");
    assert_eq!(chosen.resolved.needs, Some(AgentNeed::Node));
}

#[test]
fn a_choice_is_one_listed_agent_or_an_own_command() {
    let profile = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let agents = testing::agents(profile.path(), bin.path());
    let refused = |args: ChooseArgs| agents.choose(&args).unwrap_err().detail;
    assert_eq!(
        refused(agent("no-such-agent")).as_deref(),
        Some("unknownAgent")
    );
    assert_eq!(refused(command("   ")).as_deref(), Some("emptyCommand"));
    assert_eq!(refused(ChooseArgs::default()).as_deref(), Some("oneChoice"));
    assert_eq!(
        refused(ChooseArgs {
            agent: Some("gemini".to_owned()),
            command: Some("x".to_owned()),
        })
        .as_deref(),
        Some("oneChoice")
    );
    assert!(!profile.path().join(CHOICE_FILE).exists());
    assert!(agents.list().iter().all(|entry| !entry.chosen));

    agents.choose(&agent("gemini")).unwrap();
    let list = agents.list();
    let chosen: Vec<&AgentEntry> = list.iter().filter(|entry| entry.chosen).collect();
    assert_eq!(chosen.len(), 1);
    let gemini = chosen[0];
    assert_eq!(gemini.id, "gemini");
    assert_eq!(
        (gemini.how, gemini.needs),
        (AgentHow::Missing, Some(AgentNeed::Node))
    );
    assert_eq!(gemini.command, "npx -y @google/gemini-cli@0.68.0 --acp");
    assert!(gemini.website.is_some());
    assert!(list.iter().all(|entry| entry.id != CUSTOM_ID));

    // an own command whose program is named with its folder: that very file
    let own = testing::program(bin.path(), "my-agent");
    let typed = format!("{} --acp  --verbose", own.display());
    agents.choose(&command(&format!("  {typed} "))).unwrap();
    let chosen = agents.chosen().unwrap();
    let own_name = own.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(chosen.name, own_name);
    assert_eq!(chosen.resolved.how, AgentHow::Custom);
    assert_eq!(chosen.resolved.shown, typed);
    let plan = chosen.resolved.plan.unwrap();
    assert_eq!(plan.program, own);
    assert_eq!(plan.args, ["--acp", "--verbose"]);
    let list = agents.list();
    let custom = list.last().unwrap();
    assert_eq!(
        (custom.id.as_str(), custom.how, custom.needs, custom.chosen),
        (CUSTOM_ID, AgentHow::Custom, None, true)
    );
    assert_eq!(list.iter().filter(|entry| entry.chosen).count(), 1);

    agents.choose(&command("no-such-agent-x9 --acp")).unwrap();
    let chosen = agents.chosen().unwrap();
    assert_eq!(chosen.resolved.needs, Some(AgentNeed::Install));
    assert_eq!(chosen.resolved.plan, None);
}

/// A stored agent this computer's list does not have (gone from a newer snapshot) counts as no choice.
#[test]
fn a_stored_agent_the_list_lacks_is_no_choice() {
    let profile = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    std::fs::write(
        profile.path().join(CHOICE_FILE),
        r#"{ "version": 1, "data": { "agent": "gone-agent" } }"#,
    )
    .unwrap();
    let agents = testing::agents(profile.path(), bin.path());
    assert_eq!(agents.chosen(), None);
    assert!(agents.list().iter().all(|entry| !entry.chosen));
}
