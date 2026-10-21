//! Membership (`projects.md` §4): the person's answers (profile `membership.json` v1) + the pure state machine.
//! The state of every candidate is derived from the inputs (record entries, disk presence, found set, declined list,
//! root answer); `step` changes the inputs for one event and names the effects; `changes` compares two inputs and
//! names the members that start | stop. One state per key by construction: record entries are Included | Missing,
//! found folders outside the record are Offered | Declined.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::record::{HiddenRule, RecordOp};
use super::uid::ProjectUid;
use crate::paths::VaultPath;
use crate::state::StateData;

pub const MEMBERSHIP_FILE: &str = "membership.json";
/// Longest `declined` | `migrated` list kept (oldest dropped).
pub const MAX_ANSWERS: usize = 2000;

/// The root's answer to "include the projects inside?".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RootAnswer {
    #[default]
    Unanswered,
    Answered,
    DeclinedAll,
}

/// The marker of a project folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ProjectMarker {
    Git,
    Permark,
    Both,
}

impl ProjectMarker {
    #[must_use]
    pub fn of(git: bool, permark: bool) -> Option<Self> {
        match (git, permark) {
            (true, true) => Some(Self::Both),
            (true, false) => Some(Self::Git),
            (false, true) => Some(Self::Permark),
            (false, false) => None,
        }
    }

    #[must_use]
    pub fn git(self) -> bool {
        matches!(self, Self::Git | Self::Both)
    }
}

/// Why a listed project is not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MissingReason {
    /// No folder | no marker at its path.
    Gone,
    /// Under the person's `ignore` list | `node_modules`.
    Ignored,
}

/// Agent sharing of a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentChoice {
    /// Follows the root's switch.
    Same,
    Off,
}

/// `<profile>/projects/<ProjectId(root)>/membership.json`: the person's answers, never in the folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Membership {
    #[serde(default)]
    pub answer: RootAnswer,
    /// Path keys the person said no to (no uid before an include).
    #[serde(default)]
    pub declined: Vec<String>,
    /// Uids whose 0.6 state was copied (round 4).
    #[serde(default)]
    pub migrated: Vec<String>,
    /// Members the agent does not see (profile only: nothing in a folder widens or narrows it).
    #[serde(default)]
    pub agent_off: Vec<String>,
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

impl StateData for Membership {
    const VERSION: u32 = 1;
}

/// One valid record entry as the machine sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecEntry {
    pub uid: ProjectUid,
    pub path: VaultPath,
    pub hidden: HiddenRule,
}

/// What the disk says at a record entry's path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Here(ProjectMarker),
    Gone,
    Ignored,
}

/// A project folder found by the walk (or offered by a dormant inner record), not the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: VaultPath,
    pub marker: ProjectMarker,
    /// The inner (dormant) record that lists it: its id there is reused at include.
    pub inner_uid: Option<ProjectUid>,
}

/// Everything the states are derived from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inputs {
    pub answer: RootAnswer,
    pub entries: Vec<RecEntry>,
    pub presence: BTreeMap<ProjectUid, Presence>,
    /// Key -> found project.
    pub found: BTreeMap<String, Found>,
    /// Declined path keys, oldest first.
    pub declined: Vec<String>,
    pub agent_off: BTreeSet<ProjectUid>,
}

/// The state of one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Candidate {
    Included {
        uid: ProjectUid,
        marker: ProjectMarker,
    },
    Missing {
        uid: ProjectUid,
        reason: MissingReason,
    },
    Offered,
    Declined,
}

impl Inputs {
    /// The answer in force: a record with entries counts as answered.
    #[must_use]
    pub fn effective_answer(&self) -> RootAnswer {
        if self.answer == RootAnswer::Unanswered && !self.entries.is_empty() {
            RootAnswer::Answered
        } else {
            self.answer
        }
    }

    #[must_use]
    pub fn entry(&self, uid: &ProjectUid) -> Option<&RecEntry> {
        self.entries.iter().find(|e| &e.uid == uid)
    }

    /// The entry at a path key.
    #[must_use]
    pub fn entry_by_key(&self, key: &str) -> Option<&RecEntry> {
        self.entries.iter().find(|e| e.path.key() == key)
    }

    /// Every candidate by key: record entries first (Included | Missing), then found folders outside the record.
    #[must_use]
    pub fn derive(&self) -> BTreeMap<String, Candidate> {
        let mut out = BTreeMap::new();
        for entry in &self.entries {
            let state = match self.presence.get(&entry.uid) {
                Some(Presence::Here(marker)) => Candidate::Included {
                    uid: entry.uid.clone(),
                    marker: *marker,
                },
                Some(Presence::Ignored) => Candidate::Missing {
                    uid: entry.uid.clone(),
                    reason: MissingReason::Ignored,
                },
                Some(Presence::Gone) | None => Candidate::Missing {
                    uid: entry.uid.clone(),
                    reason: MissingReason::Gone,
                },
            };
            out.insert(entry.path.key(), state);
        }
        let declined_all = self.effective_answer() == RootAnswer::DeclinedAll;
        let declined: BTreeSet<&str> = self.declined.iter().map(String::as_str).collect();
        for key in self.found.keys() {
            if out.contains_key(key) {
                continue;
            }
            let state = if declined_all || declined.contains(key.as_str()) {
                Candidate::Declined
            } else {
                Candidate::Offered
            };
            out.insert(key.clone(), state);
        }
        out
    }

    /// Uids of the Included members.
    #[must_use]
    pub fn included(&self) -> BTreeSet<ProjectUid> {
        self.entries
            .iter()
            .filter(|e| matches!(self.presence.get(&e.uid), Some(Presence::Here(_))))
            .map(|e| e.uid.clone())
            .collect()
    }
}

/// What the person (or a move) asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Include these found keys with these uids (the caller resolved the uids, `projects.md` §1.2).
    Include(Vec<(String, ProjectUid)>),
    Decline(Vec<String>),
    /// "No thanks" on the first question.
    DeclineAll,
    LeaveOut(Vec<ProjectUid>),
    SetHidden(ProjectUid, HiddenRule),
    SetAgent(ProjectUid, AgentChoice),
    /// Missing entries taken off the list.
    Remove(Vec<ProjectUid>),
    /// The entry now lives at `path` (move, locate, confirmed move).
    SetPath(ProjectUid, VaultPath),
}

/// What applying an event asks of the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Record(RecordOp),
    SaveMembership,
    /// Ignore frames | agent scope recompute (members set or rules changed).
    Rules,
}

/// Applies one event (pure). Targets that do not fit the event are left alone (stale UI, double click).
#[must_use]
#[allow(clippy::too_many_lines)] // the transition table: one arm per event
pub fn step(inputs: &Inputs, event: &Event) -> (Inputs, Vec<Effect>) {
    let mut next = inputs.clone();
    let mut effects = Vec::new();
    let states = inputs.derive();
    match event {
        Event::Include(picks) => {
            let mut any = false;
            for (key, uid) in picks {
                let open = matches!(
                    states.get(key),
                    Some(Candidate::Offered | Candidate::Declined)
                );
                let fresh = next.entry(uid).is_none() && next.entry_by_key(key).is_none();
                let Some(found) = inputs.found.get(key).filter(|_| open && fresh) else {
                    continue;
                };
                next.entries.push(RecEntry {
                    uid: uid.clone(),
                    path: found.path.clone(),
                    hidden: HiddenRule::Own,
                });
                next.presence
                    .insert(uid.clone(), Presence::Here(found.marker));
                next.declined.retain(|d| d != key);
                effects.push(Effect::Record(RecordOp::Add {
                    uid: uid.clone(),
                    path: found.path.clone(),
                    hidden: HiddenRule::Own,
                }));
                any = true;
            }
            if any {
                next.answer = RootAnswer::Answered;
                effects.push(Effect::SaveMembership);
                effects.push(Effect::Rules);
            }
        }
        Event::Decline(keys) => {
            let mut any = false;
            for key in keys {
                if states.get(key) == Some(&Candidate::Offered) && !next.declined.contains(key) {
                    next.declined.push(key.clone());
                    any = true;
                }
            }
            if any {
                trim(&mut next.declined);
                effects.push(Effect::SaveMembership);
                effects.push(Effect::Rules);
            }
        }
        Event::DeclineAll => {
            if inputs.effective_answer() == RootAnswer::Unanswered {
                next.answer = RootAnswer::DeclinedAll;
                effects.push(Effect::SaveMembership);
                effects.push(Effect::Rules);
            } else {
                let offered: Vec<String> = states
                    .iter()
                    .filter(|(_, s)| **s == Candidate::Offered)
                    .map(|(k, _)| k.clone())
                    .collect();
                return step(inputs, &Event::Decline(offered));
            }
        }
        Event::LeaveOut(uids) => {
            for uid in uids {
                let Some(entry) = inputs.entry(uid) else {
                    continue;
                };
                let key = entry.path.key();
                next.entries.retain(|e| &e.uid != uid);
                next.presence.remove(uid);
                next.agent_off.remove(uid);
                if !next.declined.contains(&key) {
                    next.declined.push(key);
                }
                effects.push(Effect::Record(RecordOp::Remove { uid: uid.clone() }));
            }
            if !effects.is_empty() {
                trim(&mut next.declined);
                effects.push(Effect::SaveMembership);
                effects.push(Effect::Rules);
            }
        }
        Event::SetHidden(uid, rule) => {
            if let Some(entry) = next.entries.iter_mut().find(|e| &e.uid == uid) {
                if entry.hidden != *rule {
                    entry.hidden = *rule;
                    effects.push(Effect::Record(RecordOp::SetHidden {
                        uid: uid.clone(),
                        rule: *rule,
                    }));
                    effects.push(Effect::Rules);
                }
            }
        }
        Event::SetAgent(uid, choice) => {
            if inputs.entry(uid).is_some() {
                let changed = match choice {
                    AgentChoice::Off => next.agent_off.insert(uid.clone()),
                    AgentChoice::Same => next.agent_off.remove(uid),
                };
                if changed {
                    effects.push(Effect::SaveMembership);
                    effects.push(Effect::Rules);
                }
            }
        }
        Event::Remove(uids) => {
            for uid in uids {
                let missing = matches!(
                    inputs.entry(uid).map(|e| states.get(&e.path.key())),
                    Some(Some(Candidate::Missing { .. }))
                );
                if missing {
                    next.entries.retain(|e| &e.uid != uid);
                    next.presence.remove(uid);
                    next.agent_off.remove(uid);
                    effects.push(Effect::Record(RecordOp::Remove { uid: uid.clone() }));
                }
            }
            if !effects.is_empty() {
                effects.push(Effect::SaveMembership);
                effects.push(Effect::Rules);
            }
        }
        Event::SetPath(uid, path) => {
            let key = path.key();
            let taken = inputs
                .entries
                .iter()
                .any(|e| &e.uid != uid && e.path.key() == key);
            if !taken {
                if let Some(entry) = next.entries.iter_mut().find(|e| &e.uid == uid) {
                    if entry.path != *path {
                        entry.path = path.clone();
                        next.declined.retain(|d| *d != key);
                        effects.push(Effect::Record(RecordOp::SetPath {
                            uid: uid.clone(),
                            path: path.clone(),
                        }));
                        effects.push(Effect::Rules);
                    }
                }
            }
        }
    }
    (next, effects)
}

fn trim(list: &mut Vec<String>) {
    if list.len() > MAX_ANSWERS {
        let extra = list.len() - MAX_ANSWERS;
        list.drain(..extra);
    }
}

/// Members that start | stop between two inputs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    pub started: Vec<ProjectUid>,
    pub stopped: Vec<ProjectUid>,
}

#[must_use]
pub fn changes(old: &Inputs, new: &Inputs) -> Changes {
    let (before, after) = (old.included(), new.included());
    Changes {
        started: after.difference(&before).cloned().collect(),
        stopped: before.difference(&after).cloned().collect(),
    }
}

#[cfg(test)]
#[path = "fsm_tests.rs"]
mod tests;
