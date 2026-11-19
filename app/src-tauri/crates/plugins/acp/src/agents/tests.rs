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

/// `x.y.z`, digits only: an exact version (no range, tag or `latest`).
fn exact_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// Variables that change how a runner loads code, finds programs, or where it downloads a pinned package from and how
/// it checks that download: never set by a catalog entry (mirror of `LOADER_NAMES` | `LOADER_PREFIXES` in
/// `agents/refresh.mjs`).
fn loader_variable(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    [
        "NODE_OPTIONS",
        "NODE_PATH",
        "PATH",
        "PATHEXT",
        "COMSPEC",
        "NODE_TLS_REJECT_UNAUTHORIZED",
        "NODE_EXTRA_CA_CERTS",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "REQUESTS_CA_BUNDLE",
        "CURL_CA_BUNDLE",
    ]
    .contains(&name.as_str())
        || name.ends_with("_PROXY")
        || [
            "LD_",
            "DYLD_",
            "PYTHON",
            "UV_",
            "PIP_",
            "NPM_CONFIG_",
            "BUN_",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// What the app may start from the shipped catalog: each package pinned to one exact version (`npx`: `name@x.y.z`,
/// `uvx`: `name==x.y.z` | `name@x.y.z`), and no entry setting a variable that changes how code loads or where
/// programs are found. The refresh script refuses such entries; this guards the file it wrote.
#[test]
fn the_snapshot_pins_every_package_and_sets_no_loader_variable() {
    let pinned = |package: &str, uv: bool| {
        // a scoped npm name starts with `@`: the version follows the last one
        let at = package.rfind('@').filter(|at| *at > 0);
        let version = match (uv, package.split_once("==")) {
            (true, Some((name, version))) => Some((name, version)),
            _ => at.map(|at| (&package[..at], &package[at + 1..])),
        };
        version.is_some_and(|(name, version)| !name.is_empty() && exact_version(version))
    };
    assert!(pinned("@scope/name@1.2.3", false));
    assert!(pinned("name==0.10.1", true));
    for loose in [
        "name",
        "@scope/name",
        "name@latest",
        "name@^1.2.3",
        "name@1.2",
        "name@1.2.3-beta.1",
        "name>=1.0",
    ] {
        assert!(!pinned(loose, loose.contains('=')), "{loose}");
    }
    assert!(loader_variable("node_options") && loader_variable("LD_PRELOAD"));
    assert!(loader_variable("npm_config_registry") && loader_variable("PYTHONPATH"));
    // where a pinned package is downloaded from, and how much its download is checked
    for weakening in [
        "NODE_TLS_REJECT_UNAUTHORIZED",
        "node_extra_ca_certs",
        "HTTP_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "SSL_CERT_FILE",
        "REQUESTS_CA_BUNDLE",
        "UV_INDEX_URL",
        "PIP_INDEX_URL",
        "PIP_EXTRA_INDEX_URL",
        "BUN_CONFIG_REGISTRY",
        "COMSPEC",
        "PATHEXT",
    ] {
        assert!(loader_variable(weakening), "{weakening}");
    }
    assert!(!loader_variable("FAST_AGENT_MODEL"));
    assert!(!loader_variable("VT_ACP_ENABLED"));

    for listed in snapshot() {
        let d = &listed.distribution;
        if let Some(npx) = &d.npx {
            assert!(
                pinned(&npx.package, false),
                "{}: {}",
                listed.id,
                npx.package
            );
        }
        if let Some(uvx) = &d.uvx {
            assert!(pinned(&uvx.package, true), "{}: {}", listed.id, uvx.package);
        }
        let env = d
            .npx
            .iter()
            .chain(d.uvx.iter())
            .flat_map(|p| p.env.keys())
            .chain(d.binary.values().flat_map(|b| b.env.keys()));
        for name in env {
            assert!(!loader_variable(name), "{}: {name}", listed.id);
        }
    }
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
    let resolve = |id: &str| resolve_listed(listed(id), here, &*find);

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
        // the list shows which file starts; a terminal sign-in line names it as a terminal finds it
        assert_eq!(
            claude.shown,
            shown(&plan.program.to_string_lossy(), &build.args)
        );
        assert_eq!(plan.shown, shown(file_name(&build.cmd), &build.args));
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
    assert_eq!(chosen.name.as_deref(), Some("Gemini CLI"));
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
    // no registry name: the window names an own command in its language, never by its program file
    assert_eq!(chosen.name, None);
    assert_eq!(chosen.resolved.how, AgentHow::Custom);
    let resolved_line = format!("{} --acp --verbose", own.display());
    assert_eq!(chosen.resolved.shown, resolved_line);
    let plan = chosen.resolved.plan.unwrap();
    assert_eq!(plan.program, own);
    assert_eq!(plan.args, ["--acp", "--verbose"]);
    assert_eq!(plan.shown, typed);
    let list = agents.list();
    let custom = list.last().unwrap();
    assert_eq!(custom.command, resolved_line);
    assert_eq!(
        (custom.id.as_str(), custom.how, custom.needs, custom.chosen),
        (CUSTOM_ID, AgentHow::Custom, None, true)
    );
    assert_eq!(custom.name, "");
    assert_eq!(list.iter().filter(|entry| entry.chosen).count(), 1);

    agents.choose(&command("no-such-agent-x9 --acp")).unwrap();
    let chosen = agents.chosen().unwrap();
    assert_eq!(chosen.resolved.needs, Some(AgentNeed::Install));
    assert_eq!(chosen.resolved.plan, None);
}

/// Double quotes group a word (paths with spaces); a backslash is a folder separator, never an escape.
#[test]
fn own_command_words_group_in_double_quotes() {
    let words = |typed: &str| command_words(typed);
    assert_eq!(
        words(r#""C:\Program Files\nodejs\node.exe" "C:\a b\agent.mjs" --acp"#).unwrap(),
        [
            r"C:\Program Files\nodejs\node.exe",
            r"C:\a b\agent.mjs",
            "--acp"
        ]
    );
    assert_eq!(
        words("  my-agent\t--acp   --verbose ").unwrap(),
        ["my-agent", "--acp", "--verbose"]
    );
    assert_eq!(
        words(r"C:\tools\my-agent.cmd --dir C:\x\").unwrap(),
        [r"C:\tools\my-agent.cmd", "--dir", r"C:\x\"]
    );
    // empty quotes are an empty argument; quotes join with the letters next to them
    assert_eq!(words(r#"agent "" --x"#).unwrap(), ["agent", "", "--x"]);
    assert_eq!(
        words(r#"agent --name="a b"c"#).unwrap(),
        ["agent", "--name=a bc"]
    );
    assert_eq!(words("   ").unwrap(), Vec::<String>::new());
    assert_eq!(words(r#""C:\Program Files\node.exe --acp"#), None);
    assert_eq!(words(r#"agent "a" "b"#), None);
}

#[test]
fn an_own_command_with_an_open_quote_or_no_program_is_refused() {
    let profile = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let agents = testing::agents(profile.path(), bin.path());
    let refused = |typed: &str| {
        let error = agents.choose(&command(typed)).unwrap_err();
        (error.kind, error.detail)
    };
    assert_eq!(
        refused(r#""C:\Program Files\node.exe --acp"#),
        (ErrorKind::InvalidData, Some("openQuote".to_owned()))
    );
    assert_eq!(
        refused(r#""" --acp"#),
        (ErrorKind::InvalidData, Some("emptyCommand".to_owned()))
    );
    assert!(!profile.path().join(CHOICE_FILE).exists());

    // a stored command with an open quote (the file edited by hand) counts as no choice
    std::fs::write(
        profile.path().join(CHOICE_FILE),
        r#"{ "version": 1, "data": { "command": "\"my agent --acp" } }"#,
    )
    .unwrap();
    assert_eq!(agents.chosen(), None);
    assert!(agents.list().iter().all(|entry| entry.id != CUSTOM_ID));
}

/// A program in a folder with spaces, quoted: that very file starts, with its quoted arguments as single words.
#[test]
fn a_quoted_program_path_with_spaces_starts_that_file() {
    let profile = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let folder = bin.path().join("Program Files").join("nodejs");
    std::fs::create_dir_all(&folder).unwrap();
    let node = testing::program(&folder, "node.exe");
    let agents = testing::agents(profile.path(), bin.path());
    let typed = format!(r#""{}" "C:\a b\agent.mjs" --acp"#, node.display());
    agents.choose(&command(&typed)).unwrap();
    let chosen = agents.chosen().unwrap();
    assert_eq!(chosen.name, None);
    assert_eq!(chosen.resolved.needs, None);
    assert_eq!(chosen.resolved.shown, typed);
    let plan = chosen.resolved.plan.unwrap();
    assert_eq!(plan.program, node);
    assert_eq!(plan.args, [r"C:\a b\agent.mjs", "--acp"]);
    assert_eq!(command_program(&typed), Some(node.display().to_string()));
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

/// An own command found by its bare name shows the full path of the file that starts (a same-named program
/// elsewhere on `PATH` would be a different one).
#[test]
fn an_own_command_shows_the_file_that_starts() {
    let profile = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let own = testing::program(bin.path(), "my-agent");
    let agents = testing::agents(profile.path(), bin.path());
    agents.choose(&command(r#"my-agent --acp "a b""#)).unwrap();
    let custom = agents.list().pop().unwrap();
    let shown_program = custom.command.split(" --acp").next().unwrap().to_owned();
    same_file(Path::new(&shown_program), &own);
    assert!(
        custom.command.ends_with(r#" --acp "a b""#),
        "{}",
        custom.command
    );
}

/// A program named by a relative path would be a file under the folder the agent starts in: refused.
#[test]
fn an_own_command_with_a_relative_program_path_is_refused() {
    let profile = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let agents = testing::agents(profile.path(), bin.path());
    let mut relative = vec!["./agent --acp", "bin/agent", r#""tools/my agent" --acp"#];
    if cfg!(windows) {
        relative.extend([
            r".\agent.cmd",
            r"bin\agent.exe",
            r"\agent.exe",
            "C:agent.exe",
        ]);
    }
    for typed in relative {
        let error = agents.choose(&command(typed)).unwrap_err();
        assert_eq!(
            (error.kind, error.detail.as_deref()),
            (ErrorKind::InvalidData, Some("relativeProgram")),
            "{typed}"
        );
    }
    assert!(!profile.path().join(CHOICE_FILE).exists());
}

/// The preview of an own command answers what a choice of it would: the same refusal, else the file that would start
/// (`None`: not found here). It keeps nothing.
#[test]
fn an_own_command_preview_answers_what_a_choice_would() {
    let profile = tempfile::tempdir().unwrap();
    let chooser = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let own = testing::program(bin.path(), "my-agent");
    let agents = testing::agents(profile.path(), bin.path());
    for typed in [
        "  my-agent --acp \"a b\" ",
        "missing-agent --acp",
        "",
        "   ",
        r#""" --acp"#,
        r#""C:\Program Files\node.exe --acp"#,
        "./agent --acp",
        "bin/agent",
    ] {
        let preview = agents.preview(typed);
        let choice = testing::agents(chooser.path(), bin.path()).choose(&command(typed));
        assert_eq!(
            preview.error,
            choice.err().and_then(|e| e.detail),
            "{typed:?}"
        );
        if preview.error.is_some() {
            assert_eq!(preview.program, None, "{typed:?}");
        }
    }
    let found = agents.preview("  my-agent --acp ");
    same_file(Path::new(&found.program.unwrap()), &own);
    assert_eq!(
        agents.preview("missing-agent --acp"),
        ResolvedCommand {
            program: None,
            error: None
        }
    );
    assert!(!profile.path().join(CHOICE_FILE).exists());

    let snap = testing::agents(profile.path(), bin.path()).in_snap(true);
    assert_eq!(snap.preview("my-agent").error.as_deref(), Some("snap"));
    let unkept = Agents::with(None, testing::find_in(bin.path()), platforms());
    assert_eq!(
        unkept.preview("my-agent").error.as_deref(),
        Some("noProfile")
    );
}

/// Inside a strict Snap no program of the computer can start: every agent needs `snap`, nothing can be chosen, and
/// a choice made before starts nothing.
#[test]
fn inside_a_snap_no_agent_can_start() {
    let profile = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    testing::program(bin.path(), "npx");
    testing::program(bin.path(), "my-agent");
    let free = testing::agents(profile.path(), bin.path());
    assert!(free.blocked().is_none());
    free.choose(&command("my-agent --acp")).unwrap();
    let snap = testing::agents(profile.path(), bin.path()).in_snap(true);
    assert_eq!(snap.blocked(), Some(AgentNeed::Snap));
    let list = snap.list();
    assert!(list.len() > 40);
    for entry in &list {
        let how = if entry.id == CUSTOM_ID {
            AgentHow::Custom
        } else {
            AgentHow::Missing
        };
        assert_eq!(
            (entry.how, entry.needs),
            (how, Some(AgentNeed::Snap)),
            "{}",
            entry.id
        );
    }
    let claude = list.iter().find(|entry| entry.id == "claude-acp").unwrap();
    assert_eq!(
        claude.command,
        "npx -y @agentclientprotocol/claude-agent-acp@0.108.0"
    );
    let chosen = snap.chosen().unwrap();
    assert_eq!(chosen.resolved.needs, Some(AgentNeed::Snap));
    assert_eq!(chosen.resolved.plan, None);
    let error = snap.choose(&agent("gemini")).unwrap_err();
    assert_eq!(
        (error.kind, error.detail.as_deref()),
        (ErrorKind::Unavailable, Some("snap"))
    );
}
