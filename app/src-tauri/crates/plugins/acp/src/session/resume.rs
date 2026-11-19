//! The project's last conversation, kept so the person can continue it (`acp.continue`).
//!
//! - kept in the plugin's profile folder for the project ([`SAVED_FILE`]), never in the project: the file names the
//!   agent the app would start. Written when a conversation opens (a new one takes the place of the one kept),
//!   updated with its title (`session_info_update`), when it last went on and whether the copy held changes the
//!   person had not accepted (each turn's end, each accept | reject | undo).
//! - offered only while its agent is the chosen one and that agent offered a way at its start: `session/load` (it
//!   replays the conversation) else `session/resume` (it remembers it, the window shows none of it).
//! - an agent that refuses to continue it no longer has it: forgotten.
//! - what the agent names (the id, the title) is bounded: a title is cut and loses control | bidi characters (it
//!   labels a button), an id too long to be one is never kept.

use std::time::{SystemTime, UNIX_EPOCH};

use agent_client_protocol_schema::v1::{AgentCapabilities, SessionId};
use permark_core::fsx;
use permark_core::settings::is_unsafe_char;
use permark_core::state::{Loaded, StateData};
use serde::{Deserialize, Serialize};

use super::{Agent, Core};
use crate::agents::Choice;
use crate::dto::{ContinueWay, LastConversation};

/// File of the last conversation in the plugin's profile folder for the project.
pub const SAVED_FILE: &str = "session.json";
/// Longest title kept, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// Longest session id kept, in bytes (a longer one: the conversation cannot be continued).
pub const MAX_SESSION_ID_BYTES: usize = 512;

/// The project's last conversation with an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Saved {
    /// Fingerprint of the person's choice that started it ([`fingerprint`]): offered again only while it is chosen.
    /// Never the choice itself: an own command may carry what the person typed as an argument.
    pub agent: String,
    pub session_id: String,
    /// The agent's title for it.
    #[serde(default)]
    pub title: Option<String>,
    /// The copy held changes the person had not accepted when last told.
    #[serde(default)]
    pub had_changes: bool,
    /// When it last went on (ms since the Unix epoch).
    pub at: u64,
    /// How its agent offered to continue it (`None`: no way).
    #[serde(default)]
    pub way: Option<ContinueWay>,
}

impl Saved {
    /// What the window is offered while `choice` is the chosen agent (`None`: another agent's, or no way).
    #[must_use]
    pub fn offer(&self, choice: &Choice) -> Option<LastConversation> {
        if !self.started_by(choice) {
            return None;
        }
        Some(LastConversation {
            title: self.title.clone(),
            at: self.at,
            supports: self.way?,
        })
    }

    /// Whether `choice` is the one that started it.
    #[must_use]
    pub fn started_by(&self, choice: &Choice) -> bool {
        self.agent == fingerprint(choice)
    }
}

/// What a kept conversation names its agent by: a hash of the choice, never its text.
#[must_use]
pub fn fingerprint(choice: &Choice) -> String {
    fsx::hash(&serde_json::to_vec(choice).unwrap_or_default()).hex()
}

/// A title as kept and shown: control | bidi characters dropped, at most [`MAX_TITLE_CHARS`] (`None`: nothing left).
pub(super) fn clean_title(title: &str) -> Option<String> {
    let kept: String = title
        .chars()
        .filter(|c| !c.is_control() && !is_unsafe_char(*c))
        .take(MAX_TITLE_CHARS)
        .collect();
    let kept = kept.trim();
    (!kept.is_empty()).then(|| kept.to_owned())
}

/// The file's content (`last: None`: forgotten).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Remembered {
    pub last: Option<Saved>,
}

impl StateData for Remembered {
    const VERSION: u32 = 1;
}

/// How an agent offers to continue a conversation: `session/load` first (the window shows it again).
pub(super) fn way_of(caps: &AgentCapabilities) -> Option<ContinueWay> {
    if caps.load_session {
        Some(ContinueWay::Load)
    } else if caps.session_capabilities.resume.is_some() {
        Some(ContinueWay::Resume)
    } else {
        None
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl Core {
    /// The conversation kept (`None`: none, forgotten, unreadable, no profile).
    pub(super) fn saved(&self) -> Option<Saved> {
        match self.host.saved.as_ref()?.load() {
            Loaded::Found(file) => file.last,
            _ => None,
        }
    }

    /// Changes the kept conversation by `change` (`false`: nothing to write); `session`: only while it is that one.
    /// Kept state only: a failed write leaves the earlier one.
    fn change_saved(&self, session: Option<&SessionId>, change: impl Fn(&mut Saved) -> bool) {
        let Some(file) = &self.host.saved else {
            return;
        };
        let _ = file.update(|now| {
            let mut kept = now?.last.clone()?;
            if session.is_some_and(|id| *id.0 != *kept.session_id) || !change(&mut kept) {
                return None;
            }
            Some(Remembered { last: Some(kept) })
        });
    }

    /// The conversation `session` of `agent` just opened: kept in place of any other (an id too long to be one:
    /// none kept). The copy is shared by every conversation of this run: one it already had still holds the changes
    /// left for the person, which go with the new conversation (else lost unsaid if the app quits before its first
    /// turn ends).
    pub(super) fn remember(&self, agent: &Agent, session: &SessionId) {
        let Some(file) = &self.host.saved else {
            return;
        };
        if session.0.len() > MAX_SESSION_ID_BYTES {
            let _ = file.save(&Remembered::default());
            return;
        }
        let had_changes = !agent.fresh && self.left_in_copy() > 0;
        let _ = file.save(&Remembered {
            last: Some(Saved {
                agent: fingerprint(&agent.choice),
                session_id: session.0.to_string(),
                title: None,
                had_changes,
                at: now_ms(),
                way: agent.way(),
            }),
        });
    }

    /// The agent no longer has `session`: no longer offered.
    pub(super) fn forget(&self, session: &SessionId) {
        let Some(file) = &self.host.saved else {
            return;
        };
        let _ = file.update(|now| {
            let kept = now?.last.as_ref()?;
            (*kept.session_id == *session.0).then(Remembered::default)
        });
    }

    /// The agent named `session` (`None`: cleared its title).
    pub(super) fn retitle(&self, session: &SessionId, title: Option<&str>) {
        let title = title.and_then(clean_title);
        self.change_saved(Some(session), |kept| {
            let changed = kept.title != title;
            kept.title.clone_from(&title);
            changed
        });
    }

    /// A turn of `session` ended with `changed` files of the copy waiting for the person.
    pub(super) fn turn_kept(&self, session: &SessionId, changed: u32) {
        self.change_saved(Some(session), |kept| {
            kept.at = now_ms();
            kept.had_changes = changed > 0;
            true
        });
    }

    /// The copy now holds `changed` files waiting for the person (an accept | reject | undo): the kept conversation,
    /// whichever it is, notes it (the copy is shared by every conversation of this run).
    pub(super) fn changes_left(&self, changed: u32) {
        self.change_saved(None, |kept| {
            let had = changed > 0;
            let differs = kept.had_changes != had;
            kept.had_changes = had;
            differs
        });
    }

    /// `session` was continued by `agent`: the way it offered now; a copy made anew holds none of its changes.
    pub(super) fn continued(&self, agent: &Agent, session: &SessionId) {
        self.change_saved(Some(session), |kept| {
            kept.at = now_ms();
            kept.way = agent.way();
            if agent.fresh {
                kept.had_changes = false;
            }
            true
        });
    }
}
