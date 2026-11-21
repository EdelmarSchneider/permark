//! What an agent may see of the project: one predicate for every tool, every list, every snippet and every copy made
//! for an agent. Hidden paths are answered exactly as missing ones. Silenced paths are not hidden: silence quiets the
//! person's attention only. Projects inside the folder narrow it (`projects.md` §5.2): found projects not included,
//! missing ones, members the person switched off, each member's own `.aiignore` + `[agent] deny_paths`, a member's
//! `[agent] deny` for the call's group, and (opened alone) the rules of an ancestor whose record lists this folder.
//!
//! Rule files are read checked: one that is there but cannot be read (too large, not text, a folder, a pattern that
//! does not build, a team file that is not TOML) never counts as "no rules". It stands on its last good rules, kept
//! in the person's profile; with none, nothing it governs is shared.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::map::{within, AgentRules, ProjectMap};
use crate::fsx::{self, FolderIdentity, FsxError, Zone};
use crate::organizing::SilenceLookup;
use crate::paths::{fold, key_of, Root, Vault, VaultPath, CASE_INSENSITIVE, WORKTREE_PREFIX};
use crate::settings::{ConfigRead, ConfigUnreadable, ProjectConfig, SettingsError, Unreadable};
use crate::state::{Loaded, StateData, StateFile, StateStore};

/// The ignore file agents honor (project root, gitignore syntax).
pub const AI_IGNORE: &str = ".aiignore";
/// Largest `.aiignore` read: a larger one is unreadable, never cut (the rules past a cut would be lost).
pub(crate) const MAX_IGNORE_BYTES: u64 = 1024 * 1024;
/// Folders of the project's own machinery (at any depth: a nested repository has its own), never shared.
pub const PRIVATE_FOLDERS: [&str; 2] = [".permark", ".git"];
/// Rule files (at any depth: a member project has its own), never shared: their lines name what agents may not see.
pub const PRIVATE_FILES: [&str; 1] = [AI_IGNORE];
/// A folder's team settings file, as a rule file is named in a [`RulesProblem`].
pub const CONFIG_FILE: &str = ".permark/config.toml";
/// The profile file keeping the last good rules of a project's rule files ([`LastGood`]).
pub const LAST_GOOD_FILE: &str = "agent-rules.json";
/// Most rule files whose last good rules a project keeps (the oldest written make room): far more than a project
/// holds, so members never push each other out (each would rewrite the store at every load).
const MAX_KEPT_FILES: usize = 256;
/// What the state file's envelope adds around the kept rules (its fields, one more indent on each of their lines).
const ENVELOPE_ROOM: u64 = 4096;

/// Why a rule file could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RulesWhy {
    /// Larger than a rule file is read.
    TooLarge,
    /// Not text it can read whole (UTF-8, or UTF-16 with its BOM).
    NotText,
    /// Not TOML (a merge conflict left in it).
    NotToml,
    /// A team file's `[agent]` part holds a value of the wrong kind (a text where a list belongs): the lenient read
    /// would drop it.
    BadValue,
    /// A pattern that does not build: what it names would stay shared.
    BadPattern,
    /// A folder, link or device where the file belongs.
    NotAFile,
    /// Held by another program.
    Locked,
    /// No permission to read it.
    Denied,
    /// Any other read failure.
    Failed,
}

/// What stands in for a rule file that could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RulesFallback {
    /// Its rules as last read well.
    LastGood,
    /// None: nothing it governs is shared.
    Closed,
}

/// A rule file that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RulesProblem {
    /// The file as a project path (`.aiignore`, `sub/.permark/config.toml`; above the folder `../.aiignore`).
    pub file: String,
    pub why: RulesWhy,
    pub fallback: RulesFallback,
}

/// One rule file read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleRead {
    Absent,
    /// Its text (BOM dropped, UTF-16 decoded).
    Read(String),
    Unreadable(RulesWhy),
}

/// Reads a rule file (`.aiignore`): a plain file of at most [`MAX_IGNORE_BYTES`] (a FIFO never makes the read wait),
/// UTF-8 or UTF-16 with its BOM (Windows editors and PowerShell write both). What is there but cannot be read whole
/// is unreadable, never "no rules".
#[must_use]
pub fn read_rules(path: &Path) -> RuleRead {
    match fsx::read_regular(path, MAX_IGNORE_BYTES, Zone::Anywhere) {
        Ok(bytes) => match fsx::decode(&bytes) {
            // a replaced byte would turn a rule into one that matches nothing
            Some(decoded) if !decoded.lossy => RuleRead::Read(decoded.text),
            _ => RuleRead::Unreadable(RulesWhy::NotText),
        },
        Err(FsxError::TooLarge { .. }) => RuleRead::Unreadable(RulesWhy::TooLarge),
        Err(FsxError::Binary(_)) => RuleRead::Unreadable(RulesWhy::NotText),
        Err(FsxError::Io { source, .. }) => match io_why(&source) {
            Some(why) => RuleRead::Unreadable(why),
            // a link leading nowhere is there all the same: never "no rules", so its last good ones stay kept
            None if std::fs::symlink_metadata(path).is_ok() => {
                RuleRead::Unreadable(RulesWhy::NotAFile)
            }
            None => RuleRead::Absent,
        },
    }
}

/// Why an IO failure left a rule file unread; `None`: nothing is there.
fn io_why(error: &io::Error) -> Option<RulesWhy> {
    Some(match error.kind() {
        io::ErrorKind::NotFound => return None,
        _ if fsx::is_locked(error) => RulesWhy::Locked,
        io::ErrorKind::PermissionDenied => RulesWhy::Denied,
        // `read_regular`'s answer for a folder, FIFO or device (a live link is followed to what it leads to)
        io::ErrorKind::InvalidData => RulesWhy::NotAFile,
        _ => RulesWhy::Failed,
    })
}

fn config_why(error: &ConfigUnreadable) -> RulesWhy {
    match error {
        ConfigUnreadable::NotToml => RulesWhy::NotToml,
        ConfigUnreadable::LinkedFolder => RulesWhy::NotAFile,
        ConfigUnreadable::Read(SettingsError::Unreadable { why, .. }) => match why {
            Unreadable::TooLarge => RulesWhy::TooLarge,
            Unreadable::NotText => RulesWhy::NotText,
            Unreadable::Locked => RulesWhy::Locked,
        },
        ConfigUnreadable::Read(SettingsError::Io { source, .. }) => {
            io_why(source).unwrap_or(RulesWhy::Failed)
        }
        ConfigUnreadable::Read(_) => RulesWhy::Failed,
    }
}

/// One rule file's patterns as one matcher (`None`: no pattern in them).
///
/// # Errors
/// `BadPattern`: a pattern that does not build.
fn matcher<'a>(
    base: &Path,
    lines: impl IntoIterator<Item = &'a str>,
) -> Result<Option<Gitignore>, RulesWhy> {
    let mut builder = GitignoreBuilder::new(base);
    builder.case_insensitive(CASE_INSENSITIVE).ok();
    for line in lines {
        builder
            .add_line(None, line)
            .map_err(|_| RulesWhy::BadPattern)?;
    }
    let rules = builder.build().map_err(|_| RulesWhy::BadPattern)?;
    Ok((!rules.is_empty()).then_some(rules))
}

/// Each rule file of a folder as its own matcher: one file never un-hides (`!pattern`) what another hides. `None`: a
/// pattern that does not build ([`folder_rules`] checks them first: only a list a caller passed itself).
fn matchers(base: &Path, rules: &AgentRules) -> Option<Vec<Gitignore>> {
    [&rules.aiignore, &rules.deny_paths]
        .into_iter()
        .filter_map(|lines| matcher(base, lines.iter().map(String::as_str)).transpose())
        .collect::<Result<_, _>>()
        .ok()
}

/// The agent rules a folder's own files set: its `.aiignore` and the `[agent]` part of `config` (its
/// `.permark/config.toml` read checked). `at`: the folder as its files are named in a problem (`""`: the project
/// folder; `sub`; above it `..`). A file read well becomes its last good one in `kept`; one that cannot be read stands
/// on its last good rules, or, with none, carries a problem that closes what the folder governs.
pub(crate) fn folder_rules(
    folder: &Path,
    at: &str,
    config: ConfigRead,
    kept: Option<&LastGood>,
) -> AgentRules {
    let named = |file: &str| {
        if at.is_empty() {
            file.to_owned()
        } else {
            format!("{at}/{file}")
        }
    };
    let lines_of = |text: &str| -> Result<Vec<String>, RulesWhy> {
        matcher(folder, text.lines())?;
        Ok(text.lines().map(str::to_owned).collect())
    };
    let checked = |config: ProjectConfig| -> Result<ProjectConfig, RulesWhy> {
        if config.agent_bad_value {
            return Err(RulesWhy::BadValue);
        }
        matcher(folder, config.agent_deny_paths.iter().map(String::as_str))?;
        Ok(config)
    };
    let mut rules = AgentRules::default();
    // the lines of an edit that build, beside its bad pattern: added to the last good rules, so a line added with a
    // typo elsewhere still hides; never a `!` line (it could un-hide what the last good rules hide)
    let mut built = Vec::new();
    let read = match read_rules(&folder.join(AI_IGNORE)) {
        RuleRead::Absent => Ok(None),
        RuleRead::Read(text) => match lines_of(&text) {
            Ok(lines) => Ok(Some((text, lines))),
            Err(why) => {
                built = text
                    .lines()
                    .filter(|line| !line.starts_with('!') && matcher(folder, [*line]).is_ok())
                    .map(str::to_owned)
                    .collect();
                Err(why)
            }
        },
        RuleRead::Unreadable(why) => Err(why),
    };
    // no last good rules: closed, what builds of the edit adds nothing
    rules.aiignore = settle(&mut rules.problems, named(AI_IGNORE), read, kept, |text| {
        lines_of(text).ok()
    })
    .map(|mut lines| {
        lines.append(&mut built);
        lines
    })
    .unwrap_or_default();
    let read = match config {
        ConfigRead::Absent => Ok(None),
        ConfigRead::Read(text, config) => checked(config).map(|config| Some((text, config))),
        ConfigRead::Unreadable(error) => Err(config_why(&error)),
    };
    let config = settle(
        &mut rules.problems,
        named(CONFIG_FILE),
        read,
        kept,
        |text| ProjectConfig::from_text(text).and_then(|config| checked(config).ok()),
    );
    if let Some(config) = config {
        rules.deny_paths = config.agent_deny_paths;
        rules.deny = config.agent_deny;
    }
    rules
}

/// What stands for one rule file: what it says now (its text kept as the last good one), else what its last good text
/// says (a problem tells), else nothing (a problem closes what it governs).
fn settle<T>(
    problems: &mut Vec<RulesProblem>,
    file: String,
    read: Result<Option<(String, T)>, RulesWhy>,
    kept: Option<&LastGood>,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Option<T> {
    match read {
        Ok(found) => {
            if let Some(kept) = kept {
                kept.keep(&file, found.as_ref().map(|(text, _)| text.as_str()));
            }
            found.map(|(_, rules)| rules)
        }
        Err(why) => {
            let last = kept
                .and_then(|kept| kept.text_of(&file))
                .and_then(|text| parse(&text));
            let fallback = if last.is_some() {
                RulesFallback::LastGood
            } else {
                RulesFallback::Closed
            };
            problems.push(RulesProblem {
                file,
                why,
                fallback,
            });
            last
        }
    }
}

/// The last good rules of a project's rule files: the text of each as last read with rules that built. Kept in the
/// profile only ([`LAST_GOOD_FILE`] is profile-only): a file arriving with the folder must never set what stands in
/// for its own rules. Bound to the project folder on disk: the profile file is found by the folder's path, and
/// another folder made at that path (deleted and made again, cloned anew) never stands on the rules of the old one.
pub(crate) struct LastGood {
    file: StateFile<Kept>,
    /// The project folder now (`None`: the system cannot tell; nothing is kept then).
    folder: Option<FolderIdentity>,
}

/// The kept texts, oldest written first.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Kept {
    /// The folder they were read in (`None`: written before it was kept, or unknown: none of this folder's).
    #[serde(default)]
    folder: Option<FolderIdentity>,
    files: Vec<KeptText>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct KeptText {
    file: String,
    text: String,
}

impl StateData for Kept {
    const VERSION: u32 = 1;
    const MAX_BYTES: u64 = 4 * 1024 * 1024;
}

impl Kept {
    fn text(&self, file: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|kept| kept.file == file)
            .map(|kept| kept.text.as_str())
    }

    /// Drops the oldest written files until at most [`MAX_KEPT_FILES`] are kept and the file stays one its reader
    /// takes (a file larger than [`StateData::MAX_BYTES`] would be left unread, its rules lost).
    fn fit(&mut self) {
        let excess = self.files.len().saturating_sub(MAX_KEPT_FILES);
        self.files.drain(..excess);
        while !self.files.is_empty() && !self.fits() {
            self.files.remove(0);
        }
    }

    fn fits(&self) -> bool {
        serde_json::to_vec_pretty(self)
            .is_ok_and(|bytes| bytes.len() as u64 + ENVELOPE_ROOM <= Self::MAX_BYTES)
    }
}

impl LastGood {
    /// The last good rules `store` keeps for the project folder `root`.
    #[must_use]
    pub(crate) fn of(store: &StateStore, root: &Path) -> Self {
        Self {
            file: store.profile_file(LAST_GOOD_FILE),
            folder: FolderIdentity::of(root),
        }
    }

    /// Whether `kept` was read in this folder.
    fn ours(&self, kept: &Kept) -> bool {
        self.folder
            .zip(kept.folder)
            .is_some_and(|(now, then)| now.same(then))
    }

    /// What is kept for this folder (another folder's rules: nothing).
    fn held(&self) -> Kept {
        match self.file.load() {
            Loaded::Found(kept) if self.ours(&kept) => kept,
            Loaded::Found(_) | Loaded::Missing | Loaded::Recovered { .. } => Kept::default(),
        }
    }

    fn text_of(&self, file: &str) -> Option<String> {
        self.held().text(file).map(str::to_owned)
    }

    /// Keeps `text` as `file`'s last good one (`None`: the file is gone, so nothing stands in for it if it comes back
    /// broken). Written only when it changed; best effort: a store that cannot be written keeps what it holds.
    fn keep(&self, file: &str, text: Option<&str>) {
        if self.folder.is_none() || self.held().text(file) == text {
            return;
        }
        let _ = self.file.update(|now| {
            // another folder's rules are dropped: this folder's start anew
            let mut next = now
                .filter(|kept| self.ours(kept))
                .cloned()
                .unwrap_or_default();
            next.folder = self.folder;
            next.files.retain(|kept| kept.file != file);
            if let Some(text) = text {
                next.files.push(KeptText {
                    file: file.to_owned(),
                    text: text.to_owned(),
                });
            }
            next.fit();
            (now != Some(&next)).then_some(next)
        });
    }
}

/// Whether one of `rules` hides `path`, as written or in its composed (NFC) spelling: a name made on macOS can reach
/// the folder decomposed while the person types `.aiignore` composed.
fn ignores(rules: &[Gitignore], path: &str, is_dir: bool) -> bool {
    let hit = |p: &str| {
        rules.iter().any(|rules| {
            rules
                .matched_path_or_any_parents(Path::new(p), is_dir)
                .is_ignore()
        })
    };
    // `key_of` is the NFC spelling, folded only where the matcher ignores case too
    hit(path) || (!path.is_ascii() && hit(&key_of(path)))
}

/// Why [`Guard::file`] gives no file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Outside, through a link, hidden, a folder, missing: all answered alike, so nothing hidden is named.
    Missing,
    /// A visible member's `[agent] deny` takes the call's group away below it (`member`: its project path).
    DeniedBelow { member: String },
}

/// The rules of one call (read fresh: `.aiignore` and `.permark/config.toml` can change while an agent runs).
pub struct Guard {
    /// The project folder only: linked worktrees live outside it.
    vault: Vault,
    /// The folder's own rule files, each its own matcher.
    rules: Vec<Gitignore>,
    /// Projects inside the folder (none: a plain folder, nothing more to check).
    scope: Option<Scope>,
    /// The tool group of the call (a member's `[agent] deny` may take it away below it).
    deny_group: Option<String>,
    /// The folder's own `[agent] deny` groups, as read checked (the last good ones while the file is unreadable).
    denied_groups: BTreeSet<String>,
    /// Rule files that could not be read: the folder's own, then those above it, then its members'.
    problems: Vec<RulesProblem>,
    /// A rule file governing the whole folder could not be read and has no last good rules: nothing is shared.
    closed: bool,
}

/// The narrowing the projects of the folder add to one call.
struct Scope {
    map: Arc<ProjectMap>,
    members: Vec<MemberRules>,
    /// Members whose `[agent] deny` holds the call's group: key + path (rows below them omitted, direct calls denied).
    denied: Vec<(String, String)>,
    /// Opened alone: an ancestor's rules (the folder's path below it, its rules).
    ancestors: Vec<(String, Vec<Gitignore>)>,
    /// An ancestor listing this folder denies the call's group.
    ancestor_denied: bool,
    /// An ancestor's rule file could not be read and has no last good rules (it governs the whole folder).
    closed: bool,
}

/// A member's own `.aiignore` + `deny_paths`, matched member-relative.
struct MemberRules {
    key: String,
    /// Segments of its path.
    depth: usize,
    rules: Vec<Gitignore>,
    /// One of its rule files could not be read and has no last good rules: nothing inside it is shared.
    closed: bool,
}

impl Scope {
    fn new(root: &Path, map: Arc<ProjectMap>, group: Option<&str>) -> Self {
        let denies = |rules: &AgentRules| group.is_some_and(|g| rules.deny.contains(g));
        let members = map
            .members()
            .iter()
            .map(|m| {
                let mut base = root.to_path_buf();
                base.extend(m.path.segments());
                let rules = matchers(&base, &m.agent);
                MemberRules {
                    key: m.key.clone(),
                    depth: m.path.segments().count(),
                    closed: m.agent.closed() || rules.is_none(),
                    rules: rules.unwrap_or_default(),
                }
            })
            .filter(|m| m.closed || !m.rules.is_empty())
            .collect();
        // a member the agent may not see (off, or inside a closed one) never names itself in a denial
        let denying = map
            .members()
            .iter()
            .filter(|m| denies(&m.agent) && !map.closed_to_agent(m.path.as_str()))
            .map(|m| (m.key.clone(), m.path.as_str().to_owned()))
            .collect();
        let mut closed = false;
        let ancestors = map
            .ancestors
            .iter()
            .filter_map(|a| {
                let rules = matchers(&a.base, &a.rules);
                closed |= a.rules.closed() || rules.is_none();
                rules
                    .filter(|rules| !rules.is_empty())
                    .map(|rules| (a.rel.clone(), rules))
            })
            .collect();
        let ancestor_denied = map.ancestors.iter().any(|a| denies(&a.rules));
        Self {
            map,
            members,
            denied: denying,
            ancestors,
            ancestor_denied,
            closed,
        }
    }

    /// Hidden by the projects of the folder, a member's group denial included.
    fn hides(&self, path: &str, is_dir: bool) -> bool {
        self.hides_absent(path, is_dir)
            || self
                .denied
                .iter()
                .any(|(member, _)| within(&key_of(path), member))
    }

    /// Hidden as if absent (answered `not found`): closed projects, rules along the chain, an ancestor's denial; not a
    /// visible member's group denial (that one answers `denied below <member>`).
    fn hides_absent(&self, path: &str, is_dir: bool) -> bool {
        if self.ancestor_denied || self.map.closed_to_agent(path) {
            return true;
        }
        let key = key_of(path);
        let member_rule = self.members.iter().any(|member| {
            within(&key, &member.key)
                && (member.closed || {
                    let rel: Vec<&str> = path.split('/').skip(member.depth).collect();
                    !rel.is_empty() && ignores(&member.rules, &rel.join("/"), is_dir)
                })
        });
        member_rule
            || self
                .ancestors
                .iter()
                .any(|(rel, rules)| ignores(rules, &format!("{rel}/{path}"), is_dir))
    }

    /// The member whose `[agent] deny` takes the call's group away at `path`.
    fn denied_at(&self, path: &str) -> Option<&str> {
        let key = key_of(path);
        self.denied
            .iter()
            .find(|(member, _)| within(&key, member))
            .map(|(_, shown)| shown.as_str())
    }
}

impl Guard {
    /// The rules of a call from the folder's `.aiignore` (read checked; unreadable: closed, as no last good rules are
    /// at hand) and `deny_paths` as given: a seam for tests, which set the team rules themselves. Calls for an agent
    /// read the team file too: [`Guard::checked`], [`Guard::checked_alone`].
    #[must_use]
    pub fn new(root: &Root, deny_paths: &[String]) -> Self {
        let mut rules = folder_rules(root.path(), "", ConfigRead::Absent, None);
        match matcher(root.path(), deny_paths.iter().map(String::as_str)) {
            Ok(_) => rules.deny_paths = deny_paths.to_vec(),
            Err(why) => rules.problems.push(RulesProblem {
                file: CONFIG_FILE.to_owned(),
                why,
                fallback: RulesFallback::Closed,
            }),
        }
        Self::of_rules(root, rules)
    }

    /// The rules of a call read now: the folder's `.aiignore` and the `[agent]` part of its `.permark/config.toml`,
    /// each read checked. A file that cannot be read stands on its last good rules (kept in `store`'s profile); with
    /// none the guard is closed ([`Guard::closed`]).
    #[must_use]
    pub fn checked(root: &Root, store: &StateStore) -> Self {
        Self::read(root, Some(&LastGood::of(store, root.path())))
    }

    /// [`Guard::checked`] with no profile at hand: no last good rules stand in, so a rule file that cannot be read
    /// closes the guard (never read leniently as "no rules").
    #[must_use]
    pub fn checked_alone(root: &Root) -> Self {
        Self::read(root, None)
    }

    fn read(root: &Root, kept: Option<&LastGood>) -> Self {
        let rules = folder_rules(root.path(), "", ProjectConfig::load_checked(root), kept);
        Self::of_rules(root, rules)
    }

    fn of_rules(root: &Root, rules: AgentRules) -> Self {
        let matchers = matchers(root.path(), &rules);
        Self {
            vault: Vault::new(root.clone()),
            closed: rules.closed() || matchers.is_none(),
            rules: matchers.unwrap_or_default(),
            scope: None,
            deny_group: None,
            denied_groups: rules.deny,
            problems: rules.problems,
        }
    }

    /// The narrowing of the folder's projects for a call of the tool group `deny_group` (`None`: no tool group, e.g.
    /// the published context or a copy for an agent). A plain folder's map adds nothing.
    #[must_use]
    pub fn with_projects(mut self, map: Arc<ProjectMap>, deny_group: Option<&str>) -> Self {
        if !map.is_plain() {
            // those above before the members': one that closes the whole folder comes before one closing a member
            let ancestors = map.ancestors.iter().map(|a| &a.rules);
            let members = map.members().iter().map(|m| &m.agent);
            self.problems.extend(
                ancestors
                    .chain(members)
                    .flat_map(|rules| rules.problems.iter().cloned()),
            );
            let scope = Scope::new(self.vault.main().path(), map, deny_group);
            self.closed |= scope.closed;
            self.scope = Some(scope);
        }
        self.deny_group = deny_group.map(str::to_owned);
        self
    }

    /// The tool group this call was narrowed for (named by a [`Refusal::DeniedBelow`]).
    #[must_use]
    pub fn deny_group(&self) -> Option<&str> {
        self.deny_group.as_deref()
    }

    /// The folder's own `[agent] deny` groups as [`Guard::checked`] read them (none for [`Guard::new`]).
    #[must_use]
    pub fn denied_groups(&self) -> &BTreeSet<String> {
        &self.denied_groups
    }

    /// Rule files that could not be read: the folder's own first, then those above it, then its members'.
    #[must_use]
    pub fn rules_problems(&self) -> &[RulesProblem] {
        &self.problems
    }

    /// The rule file a person | an agent is told about first: one that keeps agents out of what it governs, else the
    /// first one standing on its last good rules. With the guard [`Guard::closed`], the one that closed it.
    #[must_use]
    pub fn rules_problem(&self) -> Option<&RulesProblem> {
        self.problems
            .iter()
            .find(|p| p.fallback == RulesFallback::Closed)
            .or_else(|| self.problems.first())
    }

    /// A rule file governing the whole folder (its own, or one above it) could not be read and has no last good
    /// rules: every path is hidden, every file refused ([`Guard::rules_problems`] tells which and why).
    #[must_use]
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Whether a project path (as the app stores it: `/` separators) is kept from agents (rule files and machinery
    /// folders always, whatever the rules say).
    #[must_use]
    pub fn hidden(&self, path: &str, is_dir: bool) -> bool {
        self.hidden_as(path, is_dir, true)
    }

    /// [`Guard::hidden`]; `denied`: a visible member's group denial counts too (lists), else only what answers as
    /// absent (a direct call then names the denial).
    fn hidden_as(&self, path: &str, is_dir: bool, denied: bool) -> bool {
        if self.closed || path.starts_with(WORKTREE_PREFIX) {
            return true;
        }
        // in any case on every system (only ever narrower): a `.AIIGNORE` names what agents may not see all the same
        let private = |segment: &str, names: &[&str]| {
            names
                .iter()
                .any(|p| segment.to_lowercase() == *p || fold(segment) == fold(p))
        };
        if path
            .split('/')
            .any(|segment| private(segment, &PRIVATE_FOLDERS))
            || path
                .rsplit('/')
                .next()
                .is_some_and(|name| private(name, &PRIVATE_FILES))
        {
            return true;
        }
        if self.scope.as_ref().is_some_and(|s| {
            if denied {
                s.hides(path, is_dir)
            } else {
                s.hides_absent(path, is_dir)
            }
        }) {
            return true;
        }
        ignores(&self.rules, path, is_dir)
    }

    /// An existing, shared file of the project from an agent's word: `(project path, absolute path)`.
    ///
    /// # Errors
    /// [`Refusal::Missing`] for anything else (outside, through a link, hidden, a folder, missing, a closed guard);
    /// [`Refusal::DeniedBelow`] when a visible member denies the call's group there.
    pub fn file(&self, raw: &str) -> Result<(String, PathBuf), Refusal> {
        let path = VaultPath::parse(raw.trim()).map_err(|_| Refusal::Missing)?;
        // what the agent may not see answers as missing first: a denial would name a hidden project
        if path.is_root() || self.hidden_as(path.as_str(), false, false) {
            return Err(Refusal::Missing);
        }
        if let Some(member) = self
            .scope
            .as_ref()
            .and_then(|scope| scope.denied_at(path.as_str()))
        {
            return Err(Refusal::DeniedBelow {
                member: member.to_owned(),
            });
        }
        let abs = self.vault.resolve(&path).map_err(|_| Refusal::Missing)?;
        if !abs.is_file() {
            return Err(Refusal::Missing);
        }
        // `abs` is the real place (links followed): a link inside the project may lead to a hidden part of it
        let shown = self.vault.relativize(&abs).ok_or(Refusal::Missing)?;
        if self.hidden(shown.as_str(), false) {
            return Err(Refusal::Missing);
        }
        Ok((path.as_str().to_owned(), abs))
    }

    /// Whether the file a read opened is a shared file of the folder (`None`: the system cannot tell, refused). A read
    /// checks the place it really opened: a link swapped in after [`Guard::file`] and before the open never leads out
    /// of the folder or to a hidden part of it.
    #[must_use]
    pub fn opened_shared(&self, opened: Option<&Path>) -> bool {
        opened
            .and_then(|o| self.vault.relativize(o))
            .is_some_and(|shown| !shown.is_root() && !self.hidden(shown.as_str(), false))
    }

    /// The guard as a search filter: hidden paths do not answer.
    #[must_use]
    pub fn as_lookup(self: &Arc<Self>) -> Arc<dyn SilenceLookup> {
        Arc::new(Lookup(Arc::clone(self)))
    }
}

struct Lookup(Arc<Guard>);

impl SilenceLookup for Lookup {
    fn is_silenced(&self, path: &str) -> bool {
        self.0.hidden(path, false)
    }
}

#[cfg(test)]
#[path = "agent_view_tests.rs"]
mod rules_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// The file a read opened is checked where it really is: a link swapped in between the check of the path and the
    /// open can lead nowhere outside the folder or hidden in it.
    #[test]
    fn a_read_is_kept_only_when_the_opened_file_is_shared() {
        let dir = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(dir.path()).unwrap();
        let proj = base.join("proj");
        for (path, text) in [
            ("docs/a.md", "# A\n"),
            ("private/s.md", "# S\n"),
            (".aiignore", "private/\n"),
        ] {
            let file = proj.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        std::fs::write(base.join("outside.md"), "# Out\n").unwrap();
        let root = Root::open(&proj).unwrap();
        let guard = Guard::new(&root, &[]);
        assert_eq!(
            guard.file("docs/a.md").unwrap(),
            ("docs/a.md".to_owned(), proj.join("docs").join("a.md"))
        );
        assert_eq!(guard.file("private/s.md"), Err(Refusal::Missing));
        assert_eq!(guard.file("../outside.md"), Err(Refusal::Missing));
        assert!(guard.opened_shared(Some(&proj.join("docs/a.md"))));
        #[cfg(windows)]
        assert!(guard.opened_shared(Some(Path::new(&format!(
            r"\\?\{}",
            proj.join("docs/a.md").display()
        )))));
        assert!(!guard.opened_shared(Some(&base.join("outside.md"))));
        assert!(!guard.opened_shared(Some(&proj.join("private/s.md"))));
        assert!(!guard.opened_shared(None), "unknown: refused");
    }

    #[test]
    fn machinery_folders_at_any_depth_are_hidden_and_key_files_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::open(dir.path()).unwrap();
        let guard = Guard::new(&root, &[]);
        for path in [
            ".git/config",
            "sub/.git/config",
            "a/b/.permark/local/comments.json",
            "sub/.git",
        ] {
            assert!(guard.hidden(path, false), "{path}");
        }
        assert!(guard.hidden("sub/.git", true));
        // in any case on every system: only ever narrower
        assert!(guard.hidden("sub/.GIT/FETCH_HEAD", false));
        // key files are not hidden by name: `.aiignore` keeps them away
        for path in [
            "docs/plan.md",
            "deploy/id_ed25519",
            "deploy/id_ed25519.pub",
            "id_rsa",
            "certs/server.key",
            "certs/chain.PEM",
            "store.p12",
            "home/.netrc",
            ".git-credentials",
            "a.gitignore",
            ".github/workflows/ci.yml",
        ] {
            assert!(!guard.hidden(path, false), "{path}");
        }
    }

    /// A rule file names what agents may not see: the folder's, a member's, any `.aiignore` is itself never shared.
    #[test]
    fn rule_files_at_any_depth_are_hidden() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".aiignore"), "private/\n").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join(".aiignore"), "notes/\n").unwrap();
        let root = Root::open(dir.path()).unwrap();
        let guard = Guard::new(&root, &[]);
        for path in [".aiignore", "sub/.aiignore", "a/b/.aiignore"] {
            assert!(guard.hidden(path, false), "{path}");
        }
        assert!(guard.hidden("sub/.AIIGNORE", false));
        assert_eq!(guard.file(".aiignore"), Err(Refusal::Missing));
        assert_eq!(guard.file("sub/.aiignore"), Err(Refusal::Missing));
        for path in ["aiignore.md", "docs/.aiignore.md", "notes.aiignore"] {
            assert!(!guard.hidden(path, false), "{path}");
        }
    }
}
