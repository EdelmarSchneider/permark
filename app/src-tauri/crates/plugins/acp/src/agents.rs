//! The agents the person can choose and how each would start on this computer.
//!
//! - catalog: a trimmed snapshot of the Agent Client Protocol registry, built into the app (`agents/registry.json`,
//!   refreshed by hand with `agents/refresh.mjs`): the app never fetches the registry. Agents without a way to run on
//!   this platform are left out.
//! - start: the agent's own program when it is on `PATH`, else its package through `npx`, else through `uvx`, else
//!   missing (what to install). Programs are found by looking at `PATH` only, never by running anything.
//! - choice: `acp-agent.json` in the profile folder itself (a registry id | the person's own command), read at every
//!   agent start. Never in a project's state folder: that can be `.permark/local` inside the project, and a folder
//!   someone shares must never choose the program the app starts.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use permark_core::process;
use permark_core::state::{Loaded, StateData, StateFile};
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};

use crate::client::AgentPipes;
use crate::dto::{AgentEntry, AgentHow, AgentNeed, ChooseArgs};

/// The registry snapshot.
const SNAPSHOT: &str = include_str!("../agents/registry.json");
/// File of the person's choice, in the profile folder.
pub const CHOICE_FILE: &str = "acp-agent.json";
/// Id of the entry standing for the person's own command.
pub const CUSTOM_ID: &str = "custom";
/// Wait for the agent's own exit after its stdin closed, before it is killed.
pub const STOP_GRACE: Duration = Duration::from_secs(3);

#[derive(Deserialize)]
struct Snapshot {
    agents: Vec<Listed>,
}

/// One agent of the registry.
#[derive(Debug, Clone, Deserialize)]
pub struct Listed {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    distribution: Distribution,
}

/// Ways the registry knows to start an agent.
#[derive(Debug, Clone, Default, Deserialize)]
struct Distribution {
    #[serde(default)]
    npx: Option<Published>,
    #[serde(default)]
    uvx: Option<Published>,
    /// By registry platform name (`windows-x86_64`, `linux-aarch64-musl`, ...).
    #[serde(default)]
    binary: BTreeMap<String, Build>,
}

/// A package a runner (`npx` | `uvx`) fetches and starts.
#[derive(Debug, Clone, Deserialize)]
struct Published {
    package: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// The agent's own program for one platform (`cmd`: its path inside the registry's archive).
#[derive(Debug, Clone, Deserialize)]
struct Build {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// Every agent of the snapshot (empty if it cannot be read: a test guards it).
fn snapshot() -> &'static [Listed] {
    static ALL: OnceLock<Vec<Listed>> = OnceLock::new();
    ALL.get_or_init(|| {
        serde_json::from_str::<Snapshot>(SNAPSHOT)
            .map(|snapshot| snapshot.agents)
            .unwrap_or_default()
    })
}

/// Registry platform names of this computer, best first (empty: a platform the registry builds nothing for).
#[must_use]
pub fn platforms() -> &'static [&'static str] {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => &["windows-x86_64"],
        ("windows", "aarch64") => &["windows-aarch64"],
        ("macos", "x86_64") => &["darwin-x86_64"],
        ("macos", "aarch64") => &["darwin-aarch64"],
        ("linux", "x86_64") => &["linux-x86_64", "linux-x86_64-musl"],
        ("linux", "aarch64") => &["linux-aarch64", "linux-aarch64-musl"],
        _ => &[],
    }
}

impl Listed {
    /// Its own program for the first of `platforms` it has one for.
    fn build(&self, platforms: &[&str]) -> Option<&Build> {
        platforms
            .iter()
            .find_map(|platform| self.distribution.binary.get(*platform))
    }

    /// A runner package (any platform) | its own program for `platforms`.
    fn runs_on(&self, platforms: &[&str]) -> bool {
        self.distribution.npx.is_some()
            || self.distribution.uvx.is_some()
            || self.build(platforms).is_some()
    }
}

/// The agents of the snapshot that can run on `platforms`.
pub fn catalog<'a>(platforms: &'a [&'a str]) -> impl Iterator<Item = &'static Listed> + 'a {
    snapshot()
        .iter()
        .filter(move |listed| listed.runs_on(platforms))
}

/// What starts an agent: the program file, its arguments and variables, and the command as the person reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Set on top of the app's own environment.
    pub env: Vec<(String, String)>,
    /// The program by its name, then the arguments.
    pub shown: String,
}

/// How one choice would start now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub how: AgentHow,
    pub needs: Option<AgentNeed>,
    /// The command as it would run (as it would, once installed, for a missing one).
    pub shown: String,
    /// `None`: nothing to start it with (`needs` says what).
    pub plan: Option<Plan>,
}

/// Finds a program by name the way the app starts one (`process::resolve_program`; tests: a folder list of theirs).
pub type Find = Arc<dyn Fn(&OsStr) -> Option<PathBuf> + Send + Sync>;

/// Starts an agent's plan with its working folder.
pub type Launch = Arc<dyn Fn(&Plan, &Path) -> io::Result<AgentPipes> + Send + Sync>;

fn shown(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The file name of a registry `cmd` (a path inside the agent's archive, `/` or `\` separated).
fn file_name(cmd: &str) -> &str {
    cmd.rsplit(['/', '\\']).next().unwrap_or(cmd)
}

/// A runner way to start: the runner's name, its arguments, the variables.
struct Runner<'a> {
    how: AgentHow,
    program: &'static str,
    args: Vec<String>,
    env: &'a BTreeMap<String, String>,
}

fn pairs(env: &BTreeMap<String, String>) -> Vec<(String, String)> {
    env.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// How `listed` would start with the programs `find` sees: own program, else `npx`, else `uvx`, else missing.
fn resolve_listed(listed: &Listed, platforms: &[&str], find: &Find) -> Resolved {
    let build = listed.build(platforms);
    if let Some(build) = build {
        let name = file_name(&build.cmd);
        if let Some(program) = Some(name)
            .filter(|n| !n.is_empty())
            .and_then(|n| find(OsStr::new(n)))
        {
            let line = shown(name, &build.args);
            return Resolved {
                how: AgentHow::Native,
                needs: None,
                shown: line.clone(),
                plan: Some(Plan {
                    program,
                    args: build.args.clone(),
                    env: pairs(&build.env),
                    shown: line,
                }),
            };
        }
    }
    let npx = listed.distribution.npx.as_ref().map(|p| Runner {
        how: AgentHow::Npx,
        program: "npx",
        // `-y`: no question on the first download (nobody could answer it on a pipe)
        args: ["-y", &p.package]
            .into_iter()
            .map(str::to_owned)
            .chain(p.args.iter().cloned())
            .collect(),
        env: &p.env,
    });
    let uvx = listed.distribution.uvx.as_ref().map(|p| Runner {
        how: AgentHow::Uvx,
        program: "uvx",
        args: std::iter::once(p.package.clone())
            .chain(p.args.iter().cloned())
            .collect(),
        env: &p.env,
    });
    for runner in npx.iter().chain(uvx.iter()) {
        if let Some(program) = find(OsStr::new(runner.program)) {
            let line = shown(runner.program, &runner.args);
            return Resolved {
                how: runner.how,
                needs: None,
                shown: line.clone(),
                plan: Some(Plan {
                    program,
                    args: runner.args.clone(),
                    env: pairs(runner.env),
                    shown: line,
                }),
            };
        }
    }
    let needs = match (&npx, &uvx) {
        (Some(_), _) => AgentNeed::Node,
        (None, Some(_)) => AgentNeed::Uv,
        (None, None) => AgentNeed::Install,
    };
    let line = npx
        .iter()
        .chain(uvx.iter())
        .map(|runner| shown(runner.program, &runner.args))
        .next()
        .or_else(|| build.map(|b| shown(file_name(&b.cmd), &b.args)))
        .unwrap_or_default();
    Resolved {
        how: AgentHow::Missing,
        needs: Some(needs),
        shown: line,
        plan: None,
    }
}

/// Words of the person's own command: double quotes group (a path with spaces), whitespace splits outside them, `""`
/// is an empty word, quotes join the letters next to them (`--a="b c"` -> `--a=b c`). No backslash escapes: on
/// Windows a backslash separates folders (`C:\x\` ends with one). `None`: a quote is left open.
pub(crate) fn command_words(typed: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    // `Some`: a word has begun (a quote alone begins one: `""` is a word)
    let mut word: Option<String> = None;
    let mut quoted = false;
    for c in typed.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                word.get_or_insert_with(String::new);
            }
            c if c.is_whitespace() && !quoted => words.extend(word.take()),
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    if quoted {
        return None;
    }
    words.extend(word);
    Some(words)
}

/// The program of an own command as typed (`None`: no program, or a quote left open).
pub(crate) fn command_program(typed: &str) -> Option<String> {
    command_words(typed)?
        .into_iter()
        .next()
        .filter(|program| !program.is_empty())
}

/// The person's own command: its words (double quotes group), its program found like any other.
fn resolve_command(typed: &str, find: &Find) -> Resolved {
    let mut words = command_words(typed).unwrap_or_default().into_iter();
    let program = words.next().unwrap_or_default();
    let args: Vec<String> = words.collect();
    let found = Some(program.as_str())
        .filter(|p| !p.is_empty())
        .and_then(|p| find(OsStr::new(p)));
    Resolved {
        how: AgentHow::Custom,
        needs: found.is_none().then_some(AgentNeed::Install),
        shown: typed.to_owned(),
        plan: found.map(|program| Plan {
            program,
            args,
            env: Vec::new(),
            shown: typed.to_owned(),
        }),
    }
}

/// The person's choice (`acp-agent.json`): `{ "agent": <registry id> }` | `{ "command": <own command> }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Choice {
    Agent(String),
    Command(String),
}

impl StateData for Choice {
    const VERSION: u32 = 1;
}

/// The chosen agent as it would start now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    /// Registry name; `None`: the person's own command (the window names it in its language).
    pub name: Option<String>,
    pub resolved: Resolved,
}

/// The catalog of this computer and the person's choice.
pub struct Agents {
    /// `None`: no profile, so nothing can be chosen (and nothing starts).
    file: Option<StateFile<Choice>>,
    find: Find,
    platforms: &'static [&'static str],
}

impl Agents {
    /// Programs found on this process's `PATH`, builds of this platform.
    #[must_use]
    pub fn new(file: Option<StateFile<Choice>>) -> Self {
        Self::with(file, Arc::new(process::resolve_program), platforms())
    }

    #[must_use]
    pub fn with(
        file: Option<StateFile<Choice>>,
        find: Find,
        platforms: &'static [&'static str],
    ) -> Self {
        Self {
            file,
            find,
            platforms,
        }
    }

    fn listed(&self, id: &str) -> Option<&'static Listed> {
        catalog(self.platforms).find(|listed| listed.id == id)
    }

    /// The choice on disk now (`None`: none, unreadable, or an agent this computer's list does not have).
    fn choice(&self) -> Option<Choice> {
        let Loaded::Found(choice) = self.file.as_ref()?.load() else {
            return None;
        };
        match &choice {
            Choice::Agent(id) => self.listed(id).map(|_| choice),
            // a command without a program | with an open quote (the file edited by hand) cannot start
            Choice::Command(typed) => command_program(typed).map(|_| choice),
        }
    }

    /// The chosen agent as it would start now (`None`: nothing chosen).
    #[must_use]
    pub fn chosen(&self) -> Option<Chosen> {
        match self.choice()? {
            Choice::Agent(id) => self.listed(&id).map(|listed| Chosen {
                name: Some(listed.name.clone()),
                resolved: resolve_listed(listed, self.platforms, &self.find),
            }),
            Choice::Command(typed) => Some(Chosen {
                name: None,
                resolved: resolve_command(&typed, &self.find),
            }),
        }
    }

    /// Every agent this computer can run, as each would start now; the person's own command last when chosen.
    #[must_use]
    pub fn list(&self) -> Vec<AgentEntry> {
        let choice = self.choice();
        let mut list: Vec<AgentEntry> = catalog(self.platforms)
            .map(|listed| {
                let resolved = resolve_listed(listed, self.platforms, &self.find);
                AgentEntry {
                    id: listed.id.clone(),
                    name: listed.name.clone(),
                    description: listed.description.clone(),
                    version: listed.version.clone(),
                    how: resolved.how,
                    needs: resolved.needs,
                    command: resolved.shown,
                    website: listed.website.clone().or_else(|| listed.repository.clone()),
                    chosen: choice.as_ref() == Some(&Choice::Agent(listed.id.clone())),
                }
            })
            .collect();
        if let Some(Choice::Command(typed)) = choice {
            let resolved = resolve_command(&typed, &self.find);
            list.push(AgentEntry {
                id: CUSTOM_ID.to_owned(),
                name: String::new(),
                description: String::new(),
                version: String::new(),
                how: resolved.how,
                needs: resolved.needs,
                command: resolved.shown,
                website: None,
                chosen: true,
            });
        }
        list
    }

    /// Keeps the person's choice for the next agent start.
    ///
    /// # Errors
    /// `InvalidData`: `unknownAgent` (not in this computer's list) | `emptyCommand` (no program) | `openQuote` (a
    /// quote of the command left open) | `oneChoice` (none or both given); `Unavailable` (`noProfile`); IO.
    pub fn choose(&self, args: &ChooseArgs) -> Result<(), IpcError> {
        let refused = |detail: &str| IpcError::new(ErrorKind::InvalidData).detail(detail);
        let choice = match (
            args.agent.as_deref(),
            args.command.as_deref().map(str::trim),
        ) {
            (Some(id), None) => {
                if self.listed(id).is_none() {
                    return Err(refused("unknownAgent").subject(id));
                }
                Choice::Agent(id.to_owned())
            }
            (None, Some(typed)) => match command_words(typed) {
                None => return Err(refused("openQuote")),
                Some(words) if words.first().is_none_or(String::is_empty) => {
                    return Err(refused("emptyCommand"));
                }
                Some(_) => Choice::Command(typed.to_owned()),
            },
            _ => return Err(refused("oneChoice")),
        };
        let file = self
            .file
            .as_ref()
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).detail("noProfile"))?;
        file.update(|_| Some(choice.clone()))?;
        Ok(())
    }
}

/// Starts a plan: its program with its arguments, its variables on top of the app's, no shell, the copy as folder.
#[must_use]
pub fn person_launch() -> Launch {
    Arc::new(|plan: &Plan, cwd: &Path| {
        let args: Vec<&OsStr> = plan.args.iter().map(OsStr::new).collect();
        let env: Vec<(&OsStr, &OsStr)> = plan
            .env
            .iter()
            .map(|(name, value)| (OsStr::new(name), OsStr::new(value)))
            .collect();
        let process::Piped {
            stdin,
            stdout,
            stderr,
            mut child,
        } = process::spawn_person_command(&plan.program, &args, cwd, &env)?;
        Ok(AgentPipes {
            to_agent: Box::new(stdin),
            from_agent: Box::new(stdout),
            stderr: Box::new(stderr),
            stop: Box::new(move || {
                let _ = child.stop(STOP_GRACE);
            }),
        })
    })
}

#[cfg(test)]
mod tests;
