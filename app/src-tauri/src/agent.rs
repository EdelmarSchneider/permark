//! `permark mcp` and `permark context`: no window, no Tauri. The project's tools answer through the app when it
//! serves the project, else from the files through the same composition as the app (`ProjectCore::from_files`).

use std::process::ExitCode;
use std::sync::Arc;

use permark_core::events::NoEvents;
use permark_core::paths::Root;
use permark_core::plugins::{PluginId, PluginRegistry};
use permark_core::settings::SettingsFile;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use permark_plugin_agent::cli::{launch_app, Agent, Compose, Composed};
use permark_plugin_agent::tools::Tools;

/// The plugins a reader with no window starts: what the tools ask of (tags, silence, comments, unread, notes, the token
/// count when the person turned it on) and the config formats, so secret values are hidden by the structure of the
/// file here too.
const FILES_PLUGINS: [&str; 13] = [
    "tags",
    "quiet",
    "comments",
    "unread",
    "notes",
    "token-count",
    "fmt-toml",
    "fmt-yaml",
    "fmt-ini",
    "fmt-dotenv",
    "fmt-properties",
    "fmt-ignore",
    "fmt-toon",
];

/// Keeps the headless project alive for the tools; stops it in order (project first, then the app's plugins).
struct Keep {
    core: Option<Arc<ProjectCore>>,
    env: Arc<CoreEnv>,
}

impl Drop for Keep {
    fn drop(&mut self) {
        self.core.take();
        self.env.stop_app_plugins();
    }
}

fn compose(dirs: AppDirs) -> Compose {
    Arc::new(move |root: &Root| {
        let only: Vec<PluginId> = FILES_PLUGINS.iter().map(|id| PluginId::of(id)).collect();
        let plugins = crate::plugins::all()
            .into_iter()
            .filter(|p| only.contains(&p.manifest().id))
            .collect();
        let registry = PluginRegistry::new(plugins).map_err(|e| e.to_string())?;
        let distribution = crate::shell::distribution();
        let env = Arc::new(CoreEnv::new(
            dirs.clone(),
            Arc::new(registry),
            Arc::new(NoEvents),
            distribution,
            SettingsFile::app(&dirs).load(),
        ));
        env.apply_app_plugins();
        let core = ProjectCore::from_files(root.clone(), &env, &only);
        let tools = Tools::new(
            core.vault().clone(),
            core.store().clone(),
            core.services(),
            None,
        );
        Ok(Composed {
            tools,
            keep: Box::new(Keep {
                core: Some(core),
                env,
            }),
        })
    })
}

fn agent() -> Result<Agent, String> {
    let dirs = AppDirs::system().ok_or("no profile folder on this system")?;
    let cwd =
        std::env::current_dir().map_err(|e| format!("could not read the working folder: {e}"))?;
    Ok(Agent::discover(
        &dirs,
        &cwd,
        compose(dirs.clone()),
        Arc::new(launch_app),
    ))
}

/// `permark mcp`: standard output carries protocol messages only; a failure goes to standard error.
#[must_use]
pub fn mcp() -> ExitCode {
    let outcome = agent().and_then(|agent| {
        permark_plugin_agent::mcp::run_stdio(Arc::new(agent)).map_err(|e| e.to_string())
    });
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("permark mcp: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `permark context`: one line for a prompt hook, or nothing (the reason on standard error); always exit 0.
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

    use std::time::Duration;

    use permark_core::state::StateStore;
    use permark_plugin_agent::perms::{ProjectChoice, PROJECT_FILE};
    use serde_json::{json, Value};

    use super::*;

    /// The real composition with the app closed: tags from front matter, comments from the person's state, unread
    /// from the files, and nothing written but the one change asked for.
    #[test]
    fn the_files_composition_answers_and_writes_nothing_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dir.path().join("proj");
        std::fs::create_dir_all(root_dir.join("docs")).unwrap();
        std::fs::write(
            root_dir.join("README.md"),
            "---\ntags: [plan]\n---\n# Readme\n\nShip the launch.\n",
        )
        .unwrap();
        std::fs::write(root_dir.join("docs/a.md"), "# A\n\nlaunch notes\n").unwrap();
        let dirs = AppDirs::under(&dir.path().join("state"));
        let root = Root::open(&root_dir).unwrap();
        let store = StateStore::for_project(&dirs, &root);
        store
            .profile_file::<ProjectChoice>(PROJECT_FILE)
            .save(&ProjectChoice {
                enabled: true,
                groups: [("comments.resolve".to_owned(), true)]
                    .into_iter()
                    .collect(),
                resolve_asked: true,
            })
            .unwrap();
        let comments = store.raw_file("comments.json");
        comments
            .write(
                json!({ "version": 1, "comments": [{
                    "id": "c1", "path": "README.md", "text": "simplify",
                    "anchor": { "quote": "Ship the launch.", "before": "", "after": "", "headingPath": ["Readme"], "line": 6, "endLine": 6 },
                    "createdMs": 1
                }] })
                .to_string()
                .as_bytes(),
            )
            .unwrap();
        let composed = compose(dirs.clone())(&root).unwrap();
        let tools = &composed.tools;
        let tags = tools.call("list_tags", Value::Null).unwrap();
        assert_eq!(
            tags["tags"],
            json!([{ "tag": "plan", "count": 1 }]),
            "{tags}"
        );
        let search = tools.call("search", json!({ "query": "launch" })).unwrap();
        assert_eq!(search["hits"].as_array().unwrap().len(), 2, "{search}");
        let list = tools.call("list_comments", Value::Null).unwrap();
        assert_eq!(list["comments"][0]["id"], "c1", "{list}");
        assert_eq!(list["comments"][0]["state"], "anchored", "{list}");
        tools
            .call(
                "resolve_comment",
                json!({ "id": "c1", "reply": "Done in 3f2a" }),
            )
            .unwrap();
        let unread = tools.call("list_unread", Value::Null).unwrap();
        assert_eq!(unread["ready"], true, "{unread}");
        assert_eq!(
            tools.call("get_context", Value::Null).unwrap_err().code(),
            "notOpen"
        );
        assert_eq!(
            tools.call("list_notes", Value::Null).unwrap(),
            json!({ "notes": [] })
        );
        drop(composed);
        std::thread::sleep(Duration::from_millis(100));
        let written = String::from_utf8(comments.read().unwrap().unwrap()).unwrap();
        assert!(
            written.contains("\"by\": \"agent\"") || written.contains("\"by\":\"agent\""),
            "{written}"
        );
        assert!(written.contains("Done in 3f2a"), "{written}");
        // unread keeps the person's read state and the picture of the last visit untouched
        let unread_dir = store.plugin_dir(&PluginId::of("unread"));
        assert!(!unread_dir.join("read.json").exists());
        assert!(!unread_dir.join("visit.json").exists());
        // nothing was written into the project
        let mut names: Vec<String> = std::fs::read_dir(&root_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["README.md", "docs"]);
    }

    /// Config files are hidden by their structure in files mode: no secret value in `read` | `outline` | `search`.
    #[test]
    #[allow(clippy::too_many_lines)] // one project, every format, every way out
    fn config_secrets_never_reach_the_agent() {
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
            })
            .unwrap();
        let composed = compose(dirs.clone())(&root).unwrap();
        let tools = &composed.tools;
        let secrets = [
            "abc123xyz",
            "dbpass",
            "bobby",
            "p4ssw0rd",
            "tok-777",
            "hunter2",
            "letmein",
            "pw-prop",
            "npm_tok_999",
            "tok-toon-1",
            "tok-toon-2",
        ];
        for (name, _) in files {
            let whole = tools.call("read", json!({ "path": name })).unwrap();
            let outline = tools.call("outline", json!({ "path": name })).unwrap();
            let both = format!("{whole}{outline}");
            for secret in secrets {
                assert!(!both.contains(secret), "{name}: {secret} leaked: {both}");
            }
        }
        // what stays: keys, plain values, the structure
        let env = tools.call("read", json!({ "path": ".env" })).unwrap();
        let text = env["text"].as_str().unwrap();
        assert!(
            text.contains("API_KEY=<hidden>") && text.contains("PORT=3000"),
            "{text}"
        );
        let outline = tools
            .call("outline", json!({ "path": "deploy.toml" }))
            .unwrap();
        assert_eq!(outline["kind"], "config", "{outline}");
        let at: Vec<&str> = outline["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["at"].as_str().unwrap())
            .collect();
        assert_eq!(at, ["name", "auth", "server"], "{outline}");
        let part = tools
            .call("read", json!({ "path": "deploy.toml", "at": "server" }))
            .unwrap();
        let text = part["text"].as_str().unwrap();
        assert!(
            text.contains("host = \"example.org\"") && text.contains("api_token = <hidden>"),
            "{text}"
        );
        assert_eq!(part["lines"], json!([5, 7]), "{part}");
        let key_path = tools
            .call(
                "read",
                json!({ "path": "stack.yml", "at": "services.db.environment" }),
            )
            .unwrap();
        let text = key_path["text"].as_str().unwrap();
        assert!(
            text.contains("DB_PASSWORD: <hidden>") && text.contains("MODE: prod"),
            "{text}"
        );
        // a TOON file: its tables in the outline, a row read by its path, the secret column hidden
        let outline = tools.call("outline", json!({ "path": "team.toon" })).unwrap();
        assert_eq!(outline["kind"], "data", "{outline}");
        assert_eq!(outline["spec"], "TOON 4.4", "{outline}");
        assert_eq!(outline["items"][0]["label"], "users[2]{name,token}", "{outline}");
        let row = tools
            .call("read", json!({ "path": "team.toon", "at": "users[1]" }))
            .unwrap();
        assert_eq!(row["text"], "  Bob,<hidden>
", "{row}");
        let hits = tools.call("search", json!({ "query": "tok-toon-1" })).unwrap();
        assert!(hits["hits"].as_array().unwrap().is_empty(), "{hits}");
        // search finds keys, never values
        let hits = tools.call("search", json!({ "query": "hunter2" })).unwrap();
        assert!(hits["hits"].as_array().unwrap().is_empty(), "{hits}");
        let hits = tools
            .call("search", json!({ "query": "DB_PASSWORD" }))
            .unwrap();
        assert!(!hits["hits"].as_array().unwrap().is_empty(), "{hits}");
        assert!(!hits.to_string().contains("hunter2"), "{hits}");
    }
}
