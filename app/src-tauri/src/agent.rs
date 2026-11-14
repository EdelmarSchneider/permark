//! `permark mcp` and `permark context`: no window, no Tauri. The project's tools answer only through the app that
//! serves the project; with the app closed every tool is refused but `open`, which starts the app.

use std::process::ExitCode;
use std::sync::Arc;

use permark_core::state::AppDirs;
use permark_plugin_agent::cli::{launch_app, Agent};

fn agent() -> Result<Agent, String> {
    let dirs = AppDirs::system().ok_or("no profile folder on this system")?;
    let cwd =
        std::env::current_dir().map_err(|e| format!("could not read the working folder: {e}"))?;
    Ok(Agent::discover(&dirs, &cwd, Arc::new(launch_app)))
}

/// `permark mcp`: standard output carries protocol messages only; a failure goes to standard error.
#[must_use]
pub fn mcp(structured: bool) -> ExitCode {
    let outcome = agent().and_then(|agent| {
        permark_plugin_agent::mcp::run_stdio(Arc::new(agent), structured).map_err(|e| e.to_string())
    });
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("permark mcp: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `permark context`: one sentence for a prompt hook (a selection fenced below it), or nothing (the reason on
/// standard error); always exit 0.
#[must_use]
pub fn context() -> ExitCode {
    match agent().and_then(|agent| agent.context_line()) {
        Ok(line) => println!("{line}"),
        Err(reason) => eprintln!("permark context: {reason}"),
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::Path;
    use std::time::Duration;

    use permark_core::conditions::Distribution;
    use permark_core::events::NoEvents;
    use permark_core::paths::Root;
    use permark_core::settings::AppSettings;
    use permark_core::state::StateStore;
    use permark_core::workspace::{CoreEnv, ProjectCore};
    use permark_plugin_agent::perms::{ProjectChoice, PROJECT_FILE};
    use permark_plugin_agent::ToolError;
    use serde_json::json;

    use super::*;

    /// Config files reach the agent as the file holds them, through the app that serves the project: `read` gives the
    /// text as written, `outline` the structure, `search` the key names. With the app gone, the same call is refused.
    #[test]
    #[allow(clippy::too_many_lines)] // one project, every format, every way out
    fn config_files_reach_the_agent_as_written() {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dir.path().join("proj");
        std::fs::create_dir_all(&root_dir).unwrap();
        let files = [
            (
                ".env",
                "# app
API_KEY=abc123xyz
PORT=3000
DATABASE_URL=postgres://u:dbpass@h/db
",
            ),
            (
                "deploy.toml",
                "name = \"site\"
[auth]
user = \"bobby\"
pass = \"p4ssw0rd\"
[server]
host = \"example.org\"
api_token = \"tok-777\"
",
            ),
            (
                "stack.yml",
                "services:
  db:
    environment:
      DB_PASSWORD: hunter2
      MODE: prod
    command: |
      run --password=letmein
",
            ),
            (
                "app.properties",
                "db.password=pw-prop
db.user=sa
",
            ),
            (
                ".npmrc",
                "//registry.npmjs.org/:_authToken=npm_tok_999
",
            ),
            (
                "team.toon",
                "# the team
users[2]{name,token}:
  Ada,tok-toon-1
  Bob,tok-toon-2
",
            ),
        ];
        for (name, text) in files {
            std::fs::write(root_dir.join(name), text).unwrap();
        }
        let dirs = AppDirs::under(&dir.path().join("state"));
        let root = Root::open(&root_dir).unwrap();
        let store = StateStore::for_project(&dirs, &root);
        store
            .profile_file::<ProjectChoice>(PROJECT_FILE)
            .save(&ProjectChoice {
                enabled: true,
                groups: [("find".to_owned(), true)].into_iter().collect(),
                resolve_asked: true,
                folder: None,
            })
            .unwrap();
        // the app as it runs: every plugin of the build, the agent switch on, so its server listens
        let env = CoreEnv::new(
            dirs.clone(),
            Arc::new(crate::plugins::registry()),
            Arc::new(NoEvents),
            Distribution::Direct,
            AppSettings::default(),
        );
        env.apply_app_plugins();
        let core = ProjectCore::open(root, &env);
        let agent = Agent::discover(&dirs, &root_dir, Arc::new(|_: &Path| Ok(())));
        let dotenv = agent.call("read", &json!({ "path": ".env" })).unwrap();
        let text = dotenv["text"].as_str().unwrap();
        assert!(
            text.contains("API_KEY=abc123xyz") && text.contains("PORT=3000"),
            "{text}"
        );
        let outline = agent
            .call("outline", &json!({ "path": "deploy.toml" }))
            .unwrap();
        assert_eq!(outline["kind"], "config", "{outline}");
        let at: Vec<&str> = outline["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["at"].as_str().unwrap())
            .collect();
        assert_eq!(at, ["name", "auth", "server"], "{outline}");
        let part = agent
            .call("read", &json!({ "path": "deploy.toml", "at": "server" }))
            .unwrap();
        let text = part["text"].as_str().unwrap();
        assert!(
            text.contains("host = \"example.org\"") && text.contains("api_token = \"tok-777\""),
            "{text}"
        );
        assert_eq!(part["lines"], json!([5, 7]), "{part}");
        let key_path = agent
            .call(
                "read",
                &json!({ "path": "stack.yml", "at": "services.db.environment" }),
            )
            .unwrap();
        let text = key_path["text"].as_str().unwrap();
        assert!(
            text.contains("DB_PASSWORD: hunter2") && text.contains("MODE: prod"),
            "{text}"
        );
        // a TOON file: its tables in the outline, a row read by its path
        let outline = agent
            .call("outline", &json!({ "path": "team.toon" }))
            .unwrap();
        assert_eq!(outline["kind"], "data", "{outline}");
        assert_eq!(outline["spec"], "TOON 4.4", "{outline}");
        assert_eq!(
            outline["items"][0]["label"], "users[2]{name,token}",
            "{outline}"
        );
        let row = agent
            .call("read", &json!({ "path": "team.toon", "at": "users[1]" }))
            .unwrap();
        assert_eq!(row["text"], "  Bob,tok-toon-2\n", "{row}");
        // search finds config keys; first the index must hold the config files, else "no hit" proves nothing
        let found = |query: &str| -> Vec<String> {
            let answer = agent.call("search", &json!({ "query": query })).unwrap();
            assert_eq!(answer["partial"], false, "index ready: {answer}");
            answer["hits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| h["path"].as_str().unwrap().to_owned())
                .collect()
        };
        wait_index_ready(&agent);
        // not polled: once the index says ready, config keys are in it (a later pass would leave a window where an
        // agent's search misses them while `partial` says false)
        assert!(
            found("DB_PASSWORD").contains(&"stack.yml".to_owned()),
            "config keys indexed once the index is ready"
        );
        assert!(found("api_token").contains(&"deploy.toml".to_owned()));
        // the app closes: nothing is answered from the files, and `permark context` gets the reason
        drop(core);
        env.stop_app_plugins();
        assert_eq!(
            agent.call("read", &json!({ "path": ".env" })),
            Err(ToolError::AppClosed)
        );
        assert_eq!(agent.context_line(), Err(ToolError::AppClosed.to_string()));
    }

    /// Waits until a search answers complete (`partial: false`): the first build of the index ended.
    fn wait_index_ready(agent: &Agent) {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let answer = agent.call("search", &json!({ "query": "ready" })).unwrap();
            if answer["partial"] == false {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "search index not ready after 30 s: {answer}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
