//! What the git plugin tells the UI (ts-rs exported): the snapshot of a project's repository.

use permark_core::conditions::RepoState;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One change letter. Plain words in the UI (legend); order = display order of a legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Letter {
    Modified,
    Added,
    Untracked,
    Deleted,
    Renamed,
    Conflict,
}

/// A changed file (project path; `@wt/<name>/...` in a linked worktree).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GitFile {
    pub path: String,
    pub letter: Letter,
    /// In the index (staged).
    pub staged: bool,
    /// Old path of a rename.
    pub from: Option<String>,
}

/// Where HEAD points.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HeadInfo {
    /// `None`: detached or unborn without a name.
    pub branch: Option<String>,
    /// Full commit id; `None`: no commit yet.
    pub oid: Option<String>,
    pub detached: bool,
    /// The branch has no commit yet.
    pub unborn: bool,
}

/// What the letters are measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BaselineKind {
    /// HEAD (since the last commit).
    Head,
    /// The commit HEAD pointed to when the project was last closed (since the person's last visit).
    LastVisit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BaselineInfo {
    pub kind: BaselineKind,
    /// The commit compared with (`None`: HEAD itself, or nothing committed yet).
    pub oid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BackendKind {
    /// The git installed on the computer.
    System,
    /// libgit2, part of PerMark.
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorktreeInfo {
    pub name: String,
    pub branch: Option<String>,
}

/// The repository of a project as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GitSnapshot {
    /// Goes up with every announced change.
    #[ts(type = "number")]
    pub seq: u64,
    pub backend: BackendKind,
    pub head: HeadInfo,
    pub state: RepoState,
    pub baseline: BaselineInfo,
    /// Sorted by path.
    pub files: Vec<GitFile>,
    /// More changed files than are listed.
    pub truncated: bool,
    pub worktrees: Vec<WorktreeInfo>,
}

impl GitSnapshot {
    /// Equal but for `seq` (what decides whether a new snapshot is announced).
    #[must_use]
    pub fn same_content(&self, other: &Self) -> bool {
        self.backend == other.backend
            && self.head == other.head
            && self.state == other.state
            && self.baseline == other.baseline
            && self.files == other.files
            && self.truncated == other.truncated
            && self.worktrees == other.worktrees
    }
}
