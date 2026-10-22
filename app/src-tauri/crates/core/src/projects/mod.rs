//! Projects inside an opened folder (`projects.md`): marker discovery, the root record, membership, owner
//! resolution, rule combination along the chain, identity cards. Tauri-free.

pub mod card;
pub mod fsm;
#[cfg(test)]
mod git_seam_tests;
pub mod map;
pub mod record;
#[cfg(test)]
mod record_concurrency_tests;
mod service;
pub mod uid;
pub mod view;

pub use card::{CardChange, CardDto, CardField, CardPlace, CardWarning};
pub use fsm::{AgentChoice, MissingReason, ProjectMarker, RootAnswer};
pub use map::{AgentRules, AncestorRules, MemberInfo, Owner, ProjectMap};
pub use record::{HiddenRule, RecordOp, RecordRead, RecordSkip, SkippedEntry};
pub use service::{Projects, ProjectsSubscription, RootCommitAsk, UndoPlan};
pub use uid::ProjectUid;
pub use view::{
    CardTarget, IncludeOutcome, LocateRefusal, MemberState, ProjectEvent, ProjectPick, ProjectRow,
    ProjectsChanged, ProjectsUndone, ProjectsView, RecordStatus,
};
