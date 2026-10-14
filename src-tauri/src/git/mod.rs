//! Git awareness: which files changed against a baseline, HEAD/branch info, repo state, worktrees.
//!
//! Local, read-only use of libgit2 (no network features). All repository access happens on one worker
//! thread ([`GitHandle`]); everything else is plain data. These are the failure
//! modes this module is built around: `.git` outside the vault, owner validation on non-system drives,
//! unrelated parent repos, unborn/detached HEAD, `index.lock`, bursts of triggers, and echo loops.

mod dirwatch;
mod discover;
mod snapshot;
#[cfg(test)]
pub(crate) mod testutil;
mod worker;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use discover::{discover, list_worktrees, read_head_oid, Discovery, GitContext};
pub use worker::{GitHandle, GitSettings, GitTimings};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git: {0}")]
    Git(String),
    #[error("the git worker is not available")]
    WorkerGone,
}

impl From<git2::Error> for GitError {
    fn from(e: git2::Error) -> Self {
        Self::Git(e.message().to_owned())
    }
}

/// What a file's change is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BaselineKind {
    /// The HEAD commit stored when the project was last closed (agents commit often, so HEAD would reset).
    #[default]
    #[serde(alias = "last_visit")]
    LastVisit,
    Head,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GitState {
    Modified,
    Added,
    Untracked,
    Deleted,
    Renamed,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFile {
    pub state: GitState,
    pub staged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadInfo {
    pub branch: Option<String>,
    pub oid: Option<String>,
    pub detached: bool,
    pub unborn: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RepoState {
    #[default]
    Clean,
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineInfo {
    pub kind: BaselineKind,
    pub oid: Option<String>,
}

/// Matches the UI's `GitSnapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSnapshot {
    /// Increases whenever the content changes; the UI drops snapshots older than the one it shows.
    pub generation: u64,
    pub is_repo: bool,
    /// Why git could not be used (never a silent "not a repo").
    pub git_error: Option<String>,
    pub head: HeadInfo,
    pub repo_state: RepoState,
    pub baseline: BaselineInfo,
    /// Keyed by vault-relative path.
    pub files: BTreeMap<String, GitFile>,
    pub truncated: bool,
}

impl GitSnapshot {
    /// A snapshot for a folder without usable git.
    pub fn without_repo(kind: BaselineKind, git_error: Option<String>) -> Self {
        Self {
            generation: 0,
            is_repo: false,
            git_error,
            head: HeadInfo::default(),
            repo_state: RepoState::Clean,
            baseline: BaselineInfo { kind, oid: None },
            files: BTreeMap::new(),
            truncated: false,
        }
    }

    /// Equality ignoring `generation`: used to avoid announcing identical snapshots.
    pub fn same_content(&self, other: &Self) -> bool {
        let mut a = self.clone();
        a.generation = other.generation;
        a == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_serializes_to_the_ui_contract() {
        let mut files = BTreeMap::new();
        files.insert(
            "docs/a.md".to_owned(),
            GitFile {
                state: GitState::Renamed,
                staged: true,
                from: Some("docs/old.md".to_owned()),
            },
        );
        files.insert(
            "b.md".to_owned(),
            GitFile {
                state: GitState::Untracked,
                staged: false,
                from: None,
            },
        );
        let snap = GitSnapshot {
            generation: 3,
            is_repo: true,
            git_error: None,
            head: HeadInfo {
                branch: Some("main".to_owned()),
                oid: Some("abc".to_owned()),
                detached: false,
                unborn: false,
            },
            repo_state: RepoState::CherryPick,
            baseline: BaselineInfo {
                kind: BaselineKind::LastVisit,
                oid: None,
            },
            files,
            truncated: false,
        };
        assert_eq!(
            serde_json::to_value(&snap).expect("json"),
            serde_json::json!({
                "generation": 3,
                "isRepo": true,
                "gitError": null,
                "head": {"branch": "main", "oid": "abc", "detached": false, "unborn": false},
                "repoState": "cherryPick",
                "baseline": {"kind": "lastVisit", "oid": null},
                "files": {
                    "b.md": {"state": "untracked", "staged": false},
                    "docs/a.md": {"state": "renamed", "staged": true, "from": "docs/old.md"},
                },
                "truncated": false,
            })
        );
    }

    #[test]
    fn same_content_ignores_the_generation_only() {
        let a = GitSnapshot::without_repo(BaselineKind::Head, None);
        let mut b = a.clone();
        b.generation = 9;
        assert!(a.same_content(&b));
        b.truncated = true;
        assert!(!a.same_content(&b));
    }

    #[test]
    fn baseline_kind_deserializes_from_the_ui_strings() {
        let k: BaselineKind = serde_json::from_str("\"lastVisit\"").expect("lastVisit");
        assert_eq!(k, BaselineKind::LastVisit);
        let k: BaselineKind = serde_json::from_str("\"head\"").expect("head");
        assert_eq!(k, BaselineKind::Head);
        assert!(serde_json::from_str::<BaselineKind>("\"other\"").is_err());
    }
}
