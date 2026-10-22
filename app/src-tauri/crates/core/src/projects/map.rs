//! `ProjectMap`: the immutable snapshot readers get (`projects.md` §1.3): included members, owner resolution, what an
//! agent may not see (§5.2). Replaced whole on every change; pure lookups.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use super::fsm::ProjectMarker;
use super::record::HiddenRule;
use super::uid::ProjectUid;
use crate::paths::{VaultPath, WORKTREE_PREFIX};

/// The agent rules a folder's own files set (`.aiignore` lines, `[agent] deny_paths`, `[agent] deny` groups).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentRules {
    pub aiignore: Vec<String>,
    pub deny_paths: Vec<String>,
    pub deny: BTreeSet<String>,
}

impl AgentRules {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.aiignore.is_empty() && self.deny_paths.is_empty() && self.deny.is_empty()
    }
}

/// One included project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberInfo {
    pub uid: ProjectUid,
    pub path: VaultPath,
    pub key: String,
    pub marker: ProjectMarker,
    pub hidden: HiddenRule,
    pub name: String,
    pub short: String,
    /// The person switched the agent off for it (profile).
    pub agent_off: bool,
    pub agent: AgentRules,
    /// The record's identity hint (root commit of its repository), when written.
    pub first_commit: Option<String>,
}

/// An ancestor folder whose record lists the opened folder (opened alone, `projects.md` §5.2): its agent rules
/// narrow, matched against paths relative to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AncestorRules {
    /// The ancestor folder on disk.
    pub base: PathBuf,
    /// The opened folder below it (`/` separators).
    pub rel: String,
    pub rules: AgentRules,
}

/// Who owns a path: the members containing it, outer -> inner; `owner` = the last (`None`: the root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub chain: Vec<ProjectUid>,
    pub owner: Option<ProjectUid>,
    /// The path relative to its owner.
    pub rel: String,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectMap {
    pub seq: u64,
    /// The walk ended and the found set is known.
    pub ready: bool,
    members: Vec<MemberInfo>,
    index: HashMap<String, usize>,
    /// Keys of project folders the agent never sees inside: found, not included (offered | declined); missing ones.
    closed: Vec<String>,
    pub ancestors: Vec<AncestorRules>,
}

/// Whether `key` is `base` or inside it (whole segments).
#[must_use]
pub fn within(key: &str, base: &str) -> bool {
    key == base
        || key
            .strip_prefix(base)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl ProjectMap {
    #[must_use]
    pub fn new(
        seq: u64,
        ready: bool,
        mut members: Vec<MemberInfo>,
        closed: Vec<String>,
        ancestors: Vec<AncestorRules>,
    ) -> Self {
        members.sort_by(|a, b| a.key.cmp(&b.key));
        let index = members
            .iter()
            .enumerate()
            .map(|(i, m)| (m.key.clone(), i))
            .collect();
        Self {
            seq,
            ready,
            members,
            index,
            closed,
            ancestors,
        }
    }

    /// Known (the walk ended) and no member, nothing closed, no ancestor rules: a plain folder. A map that is not
    /// ready is never plain (the found set is unknown).
    #[must_use]
    pub fn is_plain(&self) -> bool {
        self.ready && self.members.is_empty() && self.closed.is_empty() && self.ancestors.is_empty()
    }

    #[must_use]
    pub fn members(&self) -> &[MemberInfo] {
        &self.members
    }

    #[must_use]
    pub fn member(&self, uid: &ProjectUid) -> Option<&MemberInfo> {
        self.members.iter().find(|m| &m.uid == uid)
    }

    /// Members containing `path` (a project path), outer -> inner. Worktree paths belong to the root.
    #[must_use]
    pub fn chain(&self, path: &str) -> Vec<&MemberInfo> {
        if self.members.is_empty() || path.starts_with(WORKTREE_PREFIX) {
            return Vec::new();
        }
        let key = crate::paths::key_of(path);
        let mut out = Vec::new();
        let mut end = 0;
        while end < key.len() {
            end = key[end..].find('/').map_or(key.len(), |i| end + i);
            if let Some(&i) = self.index.get(&key[..end]) {
                out.push(&self.members[i]);
            }
            end += 1;
        }
        out
    }

    /// The owner of a project path (`projects.md` §1.3).
    #[must_use]
    pub fn owner_of(&self, path: &str) -> Owner {
        let chain = self.chain(path);
        let owner = chain.last().map(|m| m.uid.clone());
        let rel = chain.last().map_or_else(
            || path.to_owned(),
            |m| {
                // whole segments: the member's own spelling is cut by its segment count
                let depth = m.path.segments().count();
                path.split('/').skip(depth).collect::<Vec<_>>().join("/")
            },
        );
        Owner {
            chain: chain.iter().map(|m| m.uid.clone()).collect(),
            owner,
            rel,
        }
    }

    /// Whether the agent may not see `path`: inside a found project that is not included, a missing one, or a member
    /// switched off (any on its chain). Before the walk ended every path is closed (fail closed: the found set is
    /// unknown).
    #[must_use]
    pub fn closed_to_agent(&self, path: &str) -> bool {
        if !self.ready {
            return true;
        }
        if self.is_plain() {
            return false;
        }
        let key = crate::paths::key_of(path);
        self.closed.iter().any(|c| within(&key, c)) || self.chain(path).iter().any(|m| m.agent_off)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(uid: &str, path: &str) -> MemberInfo {
        let path = VaultPath::parse(path).unwrap();
        MemberInfo {
            uid: ProjectUid::parse(uid).unwrap(),
            key: path.key(),
            path,
            marker: ProjectMarker::Git,
            hidden: HiddenRule::Own,
            name: String::new(),
            short: String::new(),
            agent_off: false,
            agent: AgentRules::default(),
            first_commit: None,
        }
    }

    fn map() -> ProjectMap {
        ProjectMap::new(
            1,
            true,
            vec![
                member("aaaaaaaaaa", "hubs/a"),
                member("bbbbbbbbbb", "hubs/a/sub"),
                member("cccccccccc", "Ünï/e\u{301}"),
            ],
            vec![crate::paths::key_of("hubs/x")],
            Vec::new(),
        )
    }

    #[test]
    fn owner_resolution_table() {
        let m = map();
        let uid = |s| ProjectUid::parse(s).unwrap();
        let cases: &[(&str, &[&str], &str)] = &[
            ("hubs/a/README.md", &["aaaaaaaaaa"], "README.md"),
            ("hubs/a", &["aaaaaaaaaa"], ""),
            ("hubs/a/sub/x.md", &["aaaaaaaaaa", "bbbbbbbbbb"], "x.md"),
            ("hubs/a/subway/x.md", &["aaaaaaaaaa"], "subway/x.md"),
            ("hubs/ab/x.md", &[], "hubs/ab/x.md"),
            ("hubs/x.md", &[], "hubs/x.md"),
            ("@wt/feat/hubs/a/x.md", &[], "@wt/feat/hubs/a/x.md"),
            ("Ünï/e\u{301}/doc.md", &["cccccccccc"], "doc.md"),
        ];
        for (path, chain, rel) in cases {
            let owner = m.owner_of(path);
            let want: Vec<ProjectUid> = chain.iter().map(|s| uid(s)).collect();
            assert_eq!(owner.chain, want, "{path}");
            assert_eq!(owner.owner, want.last().cloned(), "{path}");
            assert_eq!(owner.rel, *rel, "{path}");
        }
        if crate::paths::CASE_INSENSITIVE {
            assert_eq!(m.owner_of("HUBS/A/x.md").owner, Some(uid("aaaaaaaaaa")));
        } else {
            assert_eq!(m.owner_of("HUBS/A/x.md").owner, None);
        }
    }

    #[test]
    fn agent_closed_paths() {
        let mut m = map();
        assert!(m.closed_to_agent("hubs/x/doc.md"));
        assert!(m.closed_to_agent("hubs/x"));
        assert!(!m.closed_to_agent("hubs/xy/doc.md"));
        assert!(!m.closed_to_agent("hubs/a/sub/x.md"));
        m.members[0].agent_off = true;
        assert!(m.closed_to_agent("hubs/a/sub/x.md"), "off on the chain");
        assert!(m.closed_to_agent("hubs/a/README.md"));
        assert!(!m.closed_to_agent("README.md"));
        assert!(
            ProjectMap::default().closed_to_agent("x"),
            "not ready: closed"
        );
        assert!(!ProjectMap::default().is_plain(), "not ready: not plain");
        let plain = ProjectMap::new(1, true, Vec::new(), Vec::new(), Vec::new());
        assert!(plain.is_plain() && !plain.closed_to_agent("x"));
    }
}
