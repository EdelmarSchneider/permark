//! Projects inside an opened folder (`projects.md`): marker discovery, the root record, membership, owner
//! resolution, rule combination along the chain, identity cards. Tauri-free.

pub mod card;
pub mod fsm;
pub mod map;
pub mod record;
mod service;
pub mod uid;
#[cfg(test)]
mod verify_tests;
pub mod view;

pub use card::{CardChange, CardDto, CardField, CardPlace, CardWarning};
pub use fsm::{AgentChoice, MissingReason, ProjectMarker, RootAnswer};
pub use map::{AgentRules, AncestorRules, MemberInfo, Owner, ProjectMap};
pub use record::{HiddenRule, RecordOp, RecordRead, RecordSkip, SkippedEntry};
pub use service::{Projects, ProjectsSubscription, UndoPlan};
pub use uid::ProjectUid;
pub use view::{
    CardTarget, IncludeOutcome, LocateRefusal, MemberState, ProjectEvent, ProjectPick, ProjectRow,
    ProjectsChanged, ProjectsUndone, ProjectsView, RecordStatus,
};
