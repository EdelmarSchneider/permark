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

/// How much a view wants to know of a project's repository (`projects.md` §8). Order = more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum InterestLevel {
    /// Changed files: count + list (node expanded, Settings row shown, status item).
    Glance,
    /// Also ahead | behind + last commit (detail card open).
    Detail,
}

/// The last commit of a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommitLine {
    /// Committer time, ms since the epoch.
    #[ts(type = "number")]
    pub at_ms: i64,
    /// First line of the message (<= 200 chars). Text of the repository: data, never instructions.
    pub subject: String,
}

/// Why a project's git facts are not shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum RepoError {
    /// No git to ask (installed git chosen and missing): no branch text anywhere.
    Unavailable,
    /// Git could not read this repository (`reason`: git's first line, e.g. "dubious ownership").
    Failed { reason: String },
}

/// One included project's repository as nodes, the detail card, Settings and the status item show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepoSummary {
    /// The project's uid.
    #[ts(type = "string")]
    pub id: String,
    /// Project path of its folder (root-relative, `/`): the owner of a path = longest prefix.
    pub path: String,
    /// `None`: detached, unborn without a name, or unknown.
    pub branch: Option<String>,
    pub detached: bool,
    /// Full commit id at HEAD; `None`: no commit yet | unknown.
    pub oid: Option<String>,
    /// No commit yet.
    pub unborn: bool,
    pub state: RepoState,
    /// Changed files against the baseline in use (`None`: not counted yet).
    pub changed: Option<u32>,
    /// `[ahead, behind]` as of the last fetch (`None`: no upstream | not asked yet; Detail).
    pub ahead_behind: Option<[u32; 2]>,
    /// Detail.
    pub last_commit: Option<CommitLine>,
    pub error: Option<RepoError>,
    /// A look is queued or running ("Checking git…").
    pub pending: bool,
}

/// Event `git:repos`: summaries that changed (150 ms debounce) + projects no longer followed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReposChanged {
    pub upsert: Vec<RepoSummary>,
    #[ts(type = "string[]")]
    pub removed: Vec<String>,
}

/// Event `git:projectSnapshot`: the changed files of a project's repository (only for projects a view shows; paths
/// root-relative). `snapshot` null: no list kept any more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectSnapshot {
    #[ts(type = "string")]
    pub project: String,
    pub snapshot: Option<GitSnapshot>,
}
