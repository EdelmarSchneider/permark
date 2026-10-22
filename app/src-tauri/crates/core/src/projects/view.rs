//! What the UI gets (ts-rs): the view of Settings > Projects + the strip, the picks of the commands, the feed events.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::vocabulary::TagStatus;

use super::card::{CardDto, CardPlace};
use super::fsm::{AgentChoice, MissingReason, ProjectMarker, RootAnswer};
use super::record::{HiddenRule, SkippedEntry};
use super::uid::ProjectUid;

/// The state of one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MemberState {
    Included,
    Offered,
    Declined,
    Missing,
}

/// One project row (Settings > Projects, the strip names, the Explorer node look).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectRow {
    /// Identity key of the path (case-folded where the system ignores case).
    pub key: String,
    /// Record entries only (included | missing).
    pub uid: Option<ProjectUid>,
    /// Project path (disk spelling when present, else as the record writes it).
    pub path: String,
    pub state: MemberState,
    /// Why a missing project is missing.
    pub missing: Option<MissingReason>,
    /// `None` for a missing project.
    pub marker: Option<ProjectMarker>,
    /// Card name (included), else the folder name.
    pub name: String,
    pub short: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub hidden: HiddenRule,
    /// The record's `hidden` value is unknown to PerMark: read as `own+root` (stricter), shown as a warning.
    pub hidden_unknown: bool,
    pub agent: AgentChoice,
    /// Key of the nearest row containing it (nesting by indent).
    pub parent: Option<String>,
    /// An included project with a record of its own: how many projects that record lists.
    pub inner: Option<u32>,
    /// Where its card is saved (included only).
    pub card_place: Option<CardPlace>,
    /// Missing only: the folder Locate…'s picker opens at (absolute): the old parent folder when it still exists,
    /// else the opened folder.
    pub locate_from: Option<String>,
    /// Its card's category against the official lists (`projects.md` §7.2); `None`: no category, or no official
    /// list (marks dormant).
    pub category_status: Option<TagStatus>,
    /// Included only: tags | categories its own `.permark/vocabulary.toml` lists that are not official (count as
    /// local; Settings Tags row).
    pub local_tags: u32,
    pub local_categories: u32,
}

/// The opened folder's vocabulary file as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum VocabularyStatus {
    Absent,
    Ok,
    Unreadable { line: u32, message: String },
    Newer,
}

/// One official name with its look.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct VocabularyEntry {
    pub name: String,
    pub color: Option<String>,
    pub icon: Option<String>,
}

/// Official tags + categories of the opened folder (`projects.md` §7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VocabularyView {
    pub file: VocabularyStatus,
    /// Marks are shown (the opened folder lists >= 1 name); `false`: dormant, nothing new on screen.
    pub active: bool,
    pub tags: Vec<VocabularyEntry>,
    pub categories: Vec<VocabularyEntry>,
}

/// The record as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum RecordStatus {
    Absent,
    Ok,
    Unreadable { line: u32, message: String },
    Newer,
}

/// "{name} includes {n} projects of its own." for the strip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct InnerLine {
    pub name: String,
    pub count: u32,
}

/// What the strip says about the offered projects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct OfferSummary {
    pub count: u32,
    /// First 3 names (strip text "holds 2 projects: a and b").
    pub names: Vec<String>,
    pub inner: Vec<InnerLine>,
}

/// The move question (`projects.md` §4.4 step 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct MoveAsk {
    pub uid: ProjectUid,
    pub name: String,
    /// Where it was (record path).
    pub was: String,
    /// The found project that may be it (project path).
    pub candidate: String,
}

/// Everything Settings > Projects + the strip need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectsView {
    #[ts(type = "number")]
    pub seq: u64,
    /// Settings > Projects exists (a record, or >= 1 project found). `false`: a plain folder, nothing new.
    pub present: bool,
    /// The walk ended (else "Looking for projects…").
    pub ready: bool,
    pub answer: RootAnswer,
    pub rows: Vec<ProjectRow>,
    pub offered: OfferSummary,
    pub record: RecordStatus,
    pub skipped: Vec<SkippedEntry>,
    pub ask: Option<MoveAsk>,
    /// The opened folder's own card (Settings > Project).
    pub root_card: CardDto,
    /// The opened folder's card category against its own official list (`None`: no category | dormant).
    pub root_category_status: Option<TagStatus>,
    pub vocabulary: VocabularyView,
}

/// Which projects a command acts on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum ProjectPick {
    /// Every offered project (Include all; No thanks on the first question).
    All,
    /// These project paths.
    Paths { paths: Vec<String> },
}

/// What an include did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct IncludeOutcome {
    pub included: u32,
    /// Token for `projects_undo` (valid until the next change of the list | the tab closes).
    #[ts(type = "number | null")]
    pub undo: Option<u64>,
}

/// What an undo did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectsUndone {
    pub undone: bool,
    /// `.permark/` made by the include stays: it changed since ("The .permark folder stays: it has changed.").
    pub permark_kept: bool,
}

/// Why a folder chosen with Locate… cannot be the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LocateRefusal {
    Outside,
    NotProject,
    Taken,
    Link,
}

/// Whose card a command edits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum CardTarget {
    /// The opened folder itself.
    Root,
    Member {
        uid: ProjectUid,
    },
}

/// A change of the project list the Changes panel shows (`FeedItem::Project`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum ProjectEvent {
    /// Found while the folder is open ("Project {name} appeared in {folder}").
    Appeared { name: String, path: String },
    /// Moved within the folder ("Project {name} moved to {path}").
    Moved { name: String, path: String },
    /// "Project {name} is no longer here" (once per session).
    Gone { name: String, path: String },
    /// "Project {name} found again at {path}".
    Back { name: String, path: String },
    /// The record was changed outside PerMark.
    ListChanged,
    /// The record (changed outside PerMark) now lists a project the person said no to: it stays left out
    /// (`projects.md` §2.1, D28); Settings > Projects can include it.
    KeptOut { name: String, path: String },
}

/// `projects-changed`: the window showing `root` reads `projects_view` again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ProjectsChanged {
    pub root: String,
    #[ts(type = "number")]
    pub seq: u64,
}
