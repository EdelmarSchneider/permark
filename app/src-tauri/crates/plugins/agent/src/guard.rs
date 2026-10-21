//! What an agent may see of the project: one predicate for every tool, every list and every snippet. Hidden paths are
//! answered exactly as missing ones. Silenced paths are not hidden: silence quiets the person's attention only.
//! Projects inside the folder narrow it (`projects.md` §5.2): found projects not included, missing ones, members the
//! person switched off, each member's own `.aiignore` + `[agent] deny_paths`, a member's `[agent] deny` for the
//! call's group, and (opened alone) the rules of an ancestor whose record lists this folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use permark_core::organizing::SilenceLookup;
use permark_core::paths::{
    fold, key_of, Root, Vault, VaultPath, CASE_INSENSITIVE, WORKTREE_PREFIX,
};
use permark_core::projects::{map::within, AgentRules, ProjectMap};

use crate::perms::Group;
use crate::ToolError;

/// The ignore file agents honor (project root, gitignore syntax).
pub const AI_IGNORE: &str = ".aiignore";
/// Largest `.aiignore` read (a bigger one is read up to this size).
const MAX_IGNORE_BYTES: u64 = 1024 * 1024;
/// Folders of the project's own machinery (at any depth: a nested repository has its own), never shared.
const PRIVATE_FOLDERS: [&str; 2] = [".permark", ".git"];
/// Files that hold keys whole (no `key = value` for masking to find), never shared: exact names (folded).
const KEY_FILES: [&str; 4] = [".netrc", "_netrc", ".pgpass", ".git-credentials"];
/// Extensions of key files (folded).
const KEY_EXTENSIONS: [&str; 5] = ["pem", "key", "p12", "pfx", "keystore"];
/// Name starts of SSH private keys (folded; `id_rsa.pub` is public and stays visible).
const SSH_KEYS: [&str; 4] = ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"];

/// A file name that holds a private key or credentials whole.
fn is_key_file(name: &str) -> bool {
    let name = name.to_lowercase();
    if KEY_FILES.contains(&name.as_str()) {
        return true;
    }
    let public = Path::new(&name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"));
    if SSH_KEYS.iter().any(|k| name.starts_with(k)) && !public {
        return true;
    }
    name.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && KEY_EXTENSIONS.contains(&ext))
}

/// The rules of one call (read fresh: `.aiignore` and `.permark/config.toml` can change while an agent runs).
pub struct Guard {
    /// The project folder only: linked worktrees live outside it.
    vault: Vault,
    rules: Option<Gitignore>,
    /// Projects inside the folder (none: a plain folder, nothing more to check).
    scope: Option<Scope>,
    /// The group of the call (a member's `[agent] deny` may take it away below it).
    group: Option<Group>,
}

/// The narrowing the projects of the folder add to one call.
struct Scope {
    map: Arc<ProjectMap>,
    /// Member key, segment count, its `.aiignore` + `deny_paths` (matched member-relative).
    members: Vec<(String, usize, Gitignore)>,
    /// Members whose `[agent] deny` holds the call's group: key + path (rows below them omitted, direct calls denied).
    denied: Vec<(String, String)>,
    /// Opened alone: an ancestor's rules (the folder's path below it, its rules).
    ancestors: Vec<(String, Gitignore)>,
    /// An ancestor listing this folder denies the call's group.
    ancestor_denied: bool,
}

/// `.aiignore` lines + `deny_paths` patterns as one matcher (`None`: nothing in them).
fn matcher(base: &Path, rules: &AgentRules) -> Option<Gitignore> {
    let mut builder = GitignoreBuilder::new(base);
    builder.case_insensitive(CASE_INSENSITIVE).ok();
    let mut any = false;
    for line in rules.aiignore.iter().chain(&rules.deny_paths) {
        any |= builder.add_line(None, line).is_ok();
    }
    any.then(|| builder.build().ok())
        .flatten()
        .filter(|g| !g.is_empty())
}

impl Scope {
    fn new(root: &Path, map: Arc<ProjectMap>, group: Option<Group>) -> Self {
        let denies = |rules: &AgentRules| group.is_some_and(|g| rules.deny.contains(g.id()));
        let members = map
            .members()
            .iter()
            .filter_map(|m| {
                let mut base = root.to_path_buf();
                base.extend(m.path.segments());
                matcher(&base, &m.agent).map(|g| (m.key.clone(), m.path.segments().count(), g))
            })
            .collect();
        let denying = map
            .members()
            .iter()
            .filter(|m| denies(&m.agent))
            .map(|m| (m.key.clone(), m.path.as_str().to_owned()))
            .collect();
        let ancestors = map
            .ancestors
            .iter()
            .filter_map(|a| matcher(&a.base, &a.rules).map(|g| (a.rel.clone(), g)))
            .collect();
        let ancestor_denied = map.ancestors.iter().any(|a| denies(&a.rules));
        Self {
            map,
            members,
            denied: denying,
            ancestors,
            ancestor_denied,
        }
    }

    fn hides(&self, path: &str, is_dir: bool) -> bool {
        if self.ancestor_denied || self.map.closed_to_agent(path) {
            return true;
        }
        let key = key_of(path);
        let member_rule = self.members.iter().any(|(member, depth, rules)| {
            within(&key, member) && {
                let rel: Vec<&str> = path.split('/').skip(*depth).collect();
                !rel.is_empty()
                    && rules
                        .matched_path_or_any_parents(Path::new(&rel.join("/")), is_dir)
                        .is_ignore()
            }
        });
        member_rule
            || self.denied.iter().any(|(member, _)| within(&key, member))
            || self.ancestors.iter().any(|(rel, rules)| {
                rules
                    .matched_path_or_any_parents(Path::new(&format!("{rel}/{path}")), is_dir)
                    .is_ignore()
            })
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
    #[must_use]
    pub fn new(root: &Root, deny_paths: &[String]) -> Self {
        let mut builder = GitignoreBuilder::new(root.path());
        builder.case_insensitive(CASE_INSENSITIVE).ok();
        let mut any = false;
        let file = root.path().join(AI_IGNORE);
        if let Ok(bytes) = permark_core::fsx::read_bytes(&file, MAX_IGNORE_BYTES) {
            for line in String::from_utf8_lossy(&bytes).lines() {
                any |= builder.add_line(None, line).is_ok();
            }
        }
        for pattern in deny_paths {
            any |= builder.add_line(None, pattern).is_ok();
        }
        let rules = if any { builder.build().ok() } else { None };
        Self {
            vault: Vault::new(root.clone()),
            rules,
            scope: None,
            group: None,
        }
    }

    /// The narrowing of the folder's projects for a call of `group` (`None`: the published context, no tool group).
    /// A plain folder's map adds nothing.
    #[must_use]
    pub fn with_projects(mut self, map: Arc<ProjectMap>, group: Option<Group>) -> Self {
        if !map.is_plain() {
            self.scope = Some(Scope::new(self.vault.main().path(), map, group));
        }
        self.group = group;
        self
    }

    /// Whether a project path (as the app stores it: `/` separators) is kept from agents.
    #[must_use]
    pub fn hidden(&self, path: &str, is_dir: bool) -> bool {
        if path.starts_with(WORKTREE_PREFIX) {
            return true;
        }
        if path
            .split('/')
            .any(|segment| PRIVATE_FOLDERS.iter().any(|p| fold(segment) == fold(p)))
        {
            return true;
        }
        if !is_dir && is_key_file(path.rsplit('/').next().unwrap_or_default()) {
            return true;
        }
        if self.scope.as_ref().is_some_and(|s| s.hides(path, is_dir)) {
            return true;
        }
        self.rules.as_ref().is_some_and(|rules| {
            rules
                .matched_path_or_any_parents(Path::new(path), is_dir)
                .is_ignore()
        })
    }

    /// An existing, shared file of the project from an agent's word: `(project path, absolute path)`. Anything else
    /// (outside, through a link, hidden, a folder, missing) is `not found`.
    ///
    /// # Errors
    /// `NotFound` with the path as the agent gave it.
    pub fn file(&self, raw: &str) -> Result<(String, PathBuf), ToolError> {
        let missing = || ToolError::NotFound(raw.trim().to_owned());
        let path = VaultPath::parse(raw.trim()).map_err(|_| missing())?;
        if let Some((scope, group)) = self.scope.as_ref().zip(self.group) {
            if let Some(member) = scope.denied_at(path.as_str()) {
                return Err(ToolError::DeniedBelow {
                    group,
                    member: member.to_owned(),
                });
            }
        }
        if path.is_root() || self.hidden(path.as_str(), false) {
            return Err(missing());
        }
        let abs = self.vault.resolve(&path).map_err(|_| missing())?;
        if !abs.is_file() {
            return Err(missing());
        }
        // `abs` is the real place (links followed): a link inside the project may lead to a hidden part of it
        let shown = self.vault.relativize(&abs).ok_or_else(missing)?;
        if self.hidden(shown.as_str(), false) {
            return Err(missing());
        }
        Ok((path.as_str().to_owned(), abs))
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
mod tests {
    use super::*;

    #[test]
    fn machinery_folders_at_any_depth_and_key_files_are_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::open(dir.path()).unwrap();
        let guard = Guard::new(&root, &[]);
        for path in [
            ".git/config",
            "sub/.git/config",
            "a/b/.permark/local/comments.json",
            "sub/.git",
            "deploy/id_ed25519",
            "id_rsa",
            "certs/server.key",
            "certs/chain.PEM",
            "store.p12",
            "home/.netrc",
            ".git-credentials",
        ] {
            assert!(guard.hidden(path, false), "{path}");
        }
        assert!(guard.hidden("sub/.git", true));
        // the file system's own case rule: `.GIT` is git's folder only where case is ignored
        assert_eq!(guard.hidden("sub/.GIT/FETCH_HEAD", false), CASE_INSENSITIVE);
        for path in [
            "docs/plan.md",
            "deploy/id_ed25519.pub",
            "keys/readme.md",
            "a.gitignore",
            ".github/workflows/ci.yml",
            ".pem",
            "monkey.md",
        ] {
            assert!(!guard.hidden(path, false), "{path}");
        }
        assert!(
            !guard.hidden("certs.key", true),
            "a folder is not a key file"
        );
    }
}
