//! The agent's own settings the person may change from the panel: its model, reasoning level and model settings
//! (session config options of category `model` | `thought_level` | `model_config`).
//!
//! - kept from every answer | news that carries them (`session/new` | `load` | `resume`, `session/set_config_option`,
//!   `config_option_update`), told to the window whenever what it shows changes. Never a mode option (one can switch
//!   off the person's questions, e.g. `bypassPermissions`), never one of no | an unknown category: the agent may put
//!   anything there.
//! - a change goes to the agent aside (the RPC thread never waits on it); taken -> kept as the person's pick for that
//!   agent ([`OPTIONS_FILE`] in the profile, never in the project: by agent, then option id); refused -> said
//!   (`optionRefused`) and the settings told again as they are.
//! - picks apply at every start | continue, after the asking mode, in the agent's order, each only while the agent
//!   still offers that option and value (else skipped without a word: a model may be gone since).
//! - news and answers carrying options may cross on their way in: the last one kept wins (both tell the agent's state
//!   at that time).

use std::collections::BTreeMap;

use agent_client_protocol_schema::v1::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionConfigOptionValue,
    SessionConfigSelectOption, SessionConfigSelectOptions, SessionId,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, AGENT_METHOD_NAMES,
};
use permark_core::state::{Loaded, StateData};
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::resume::{clean_title, fingerprint};
use super::{said, Agent, Core, Inner, Live, SETUP_WAIT};
use crate::agents::Choice;
use crate::client::{lock, CallError};
use crate::dto::{
    AcpConfigCategory, AcpConfigChoice, AcpConfigKind, AcpConfigOption, AcpConfigValue, AcpError,
    AcpErrorCode, AcpUpdate,
};

/// File of the person's picks in the profile folder itself (one for every project: the agent is the person's).
pub const OPTIONS_FILE: &str = "acp-options.json";
/// Longest option id | value kept as a pick, in bytes (a longer one is used, never kept).
const MAX_PICK_BYTES: usize = 512;

/// The person's picks: agent ([`agent_key`]) -> option id -> value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Picks(pub BTreeMap<String, BTreeMap<String, AcpConfigValue>>);

impl StateData for Picks {
    const VERSION: u32 = 1;
}

/// What picks name an agent by: its registry id; the person's own command by its fingerprint, never its text (it may
/// carry what the person typed as an argument).
pub(crate) fn agent_key(choice: &Choice) -> String {
    match choice {
        Choice::Agent(id) => id.clone(),
        Choice::Command(_) => fingerprint(choice),
    }
}

/// The values of a choice option, its groups flattened in the agent's order.
pub(super) fn values(options: &SessionConfigSelectOptions) -> Vec<&SessionConfigSelectOption> {
    match options {
        SessionConfigSelectOptions::Ungrouped(values) => values.iter().collect(),
        SessionConfigSelectOptions::Grouped(groups) => {
            groups.iter().flat_map(|g| &g.options).collect()
        }
        _ => Vec::new(),
    }
}

/// The window's category of `option` (`None`: not one the person may change here).
fn category(option: &SessionConfigOption) -> Option<AcpConfigCategory> {
    match option.category.as_ref()? {
        SessionConfigOptionCategory::Model => Some(AcpConfigCategory::Model),
        SessionConfigOptionCategory::ModelConfig => Some(AcpConfigCategory::ModelConfig),
        SessionConfigOptionCategory::ThoughtLevel => Some(AcpConfigCategory::ThoughtLevel),
        _ => None,
    }
}

/// A label from the agent as shown (control | bidi characters dropped, cut); `fallback` when nothing is left.
fn label(name: &str, fallback: &str) -> String {
    clean_title(name).unwrap_or_else(|| fallback.to_owned())
}

/// `option` as the window shows it (`None`: not one it may change, or a choice with no values).
fn shown(option: &SessionConfigOption) -> Option<AcpConfigOption> {
    let category = category(option)?;
    let kind = match &option.kind {
        SessionConfigKind::Select(select) => {
            let choices: Vec<AcpConfigChoice> = values(&select.options)
                .into_iter()
                .map(|v| AcpConfigChoice {
                    value: v.value.0.to_string(),
                    name: label(&v.name, &v.value.0),
                    description: v.description.as_deref().and_then(clean_title),
                })
                .collect();
            if choices.is_empty() {
                return None;
            }
            AcpConfigKind::Select {
                current: select.current_value.0.to_string(),
                choices,
            }
        }
        SessionConfigKind::Boolean(toggle) => AcpConfigKind::Toggle {
            on: toggle.current_value,
        },
        _ => return None,
    };
    Some(AcpConfigOption {
        id: option.id.0.to_string(),
        name: label(&option.name, &option.id.0),
        description: option.description.as_deref().and_then(clean_title),
        category,
        kind,
    })
}

/// The options of `options` the window may show and change, in the agent's order.
pub(super) fn exposed(options: &[SessionConfigOption]) -> Vec<AcpConfigOption> {
    options.iter().filter_map(shown).collect()
}

/// `wanted` as the value of `option` (`None`: not offered | of the other kind).
fn offered(
    option: &SessionConfigOption,
    wanted: &AcpConfigValue,
) -> Option<SessionConfigOptionValue> {
    match (&option.kind, wanted) {
        (SessionConfigKind::Select(select), AcpConfigValue::Value(value)) => {
            values(&select.options)
                .into_iter()
                .find(|v| *v.value.0 == **value)
                .map(|v| SessionConfigOptionValue::value_id(v.value.clone()))
        }
        (SessionConfigKind::Boolean(_), AcpConfigValue::On(on)) => {
            Some(SessionConfigOptionValue::boolean(*on))
        }
        _ => None,
    }
}

/// `option` holds `wanted` now.
fn holds(option: &SessionConfigOption, wanted: &AcpConfigValue) -> bool {
    match (&option.kind, wanted) {
        (SessionConfigKind::Select(select), AcpConfigValue::Value(value)) => {
            *select.current_value.0 == **value
        }
        (SessionConfigKind::Boolean(toggle), AcpConfigValue::On(on)) => toggle.current_value == *on,
        _ => false,
    }
}

/// The option `id` the window may change, found in `options`.
fn changeable<'a>(options: &'a [SessionConfigOption], id: &str) -> Option<&'a SessionConfigOption> {
    options
        .iter()
        .find(|o| *o.id.0 == *id && category(o).is_some())
}

/// The value the agent takes for `wanted` of the option `id`.
///
/// # Errors
/// `NotFound` (`unknownSetting`): no option `id` the window may change; `InvalidData` (`notOffered`): a value the
/// option does not offer | of the other kind.
pub(super) fn value_for(
    options: &[SessionConfigOption],
    id: &str,
    wanted: &AcpConfigValue,
) -> Result<SessionConfigOptionValue, IpcError> {
    let option = changeable(options, id).ok_or_else(|| {
        IpcError::new(ErrorKind::NotFound)
            .subject(id)
            .detail("unknownSetting")
    })?;
    offered(option, wanted).ok_or_else(|| {
        IpcError::new(ErrorKind::InvalidData)
            .subject(id)
            .detail("notOffered")
    })
}

/// The whole list a `session/set_config_option` answer carries (`None`: none readable).
pub(super) fn answered(raw: Value) -> Option<Vec<SessionConfigOption>> {
    serde_json::from_value::<SetSessionConfigOptionResponse>(raw)
        .ok()
        .map(|answer| answer.config_options)
}

impl Inner {
    /// Keeps the session's options as the agent now tells them; answers the news for the window when what it shows
    /// changed (`force`: also when not).
    pub(super) fn take_options(
        &mut self,
        options: Vec<SessionConfigOption>,
        force: bool,
    ) -> Option<AcpUpdate> {
        let shown = exposed(&options);
        self.options = options;
        if shown == self.shown && !force {
            return None;
        }
        self.shown.clone_from(&shown);
        Some(AcpUpdate::Options { options: shown })
    }

    /// Forgets the options of the agent that ran; answers whether the window was showing some.
    pub(super) fn drop_options(&mut self) -> bool {
        self.options.clear();
        !std::mem::take(&mut self.shown).is_empty()
    }
}

impl Core {
    /// Keeps `options` for the agent of `generation` (`None`: the ones kept), told under the lock when what the window
    /// shows changed (or `force`): news of two threads never reach the window in another order than they were kept.
    pub(super) fn keep_options(
        &self,
        generation: u64,
        options: Option<Vec<SessionConfigOption>>,
        force: bool,
    ) {
        let mut inner = lock(&self.inner);
        if inner.generation != generation {
            return;
        }
        let options = options.unwrap_or_else(|| inner.options.clone());
        if let Some(update) = inner.take_options(options, force) {
            self.emit(update);
        }
    }

    /// The person's picks for `choice` (empty: none, unreadable, no profile).
    fn picks_of(&self, choice: &Choice) -> BTreeMap<String, AcpConfigValue> {
        let Some(file) = &self.host.picks else {
            return BTreeMap::new();
        };
        match file.load() {
            Loaded::Found(mut picks) => picks.0.remove(&agent_key(choice)).unwrap_or_default(),
            _ => BTreeMap::new(),
        }
    }

    /// Keeps `value` of option `id` as the person's pick for `choice` (kept state only: a failed write leaves the one
    /// before; an id | a value too long to be one is never kept).
    fn keep_pick(&self, choice: &Choice, id: &str, value: &AcpConfigValue) {
        let Some(file) = &self.host.picks else {
            return;
        };
        let too_long = |s: &str| s.len() > MAX_PICK_BYTES;
        if too_long(id) || matches!(value, AcpConfigValue::Value(v) if too_long(v)) {
            return;
        }
        let key = agent_key(choice);
        let _ = file.update(|now| {
            let mut next = now.cloned().unwrap_or_default();
            let mine = next.0.entry(key.clone()).or_default();
            if mine.get(id) == Some(value) {
                return None;
            }
            mine.insert(id.to_owned(), value.clone());
            Some(next)
        });
    }

    /// Applies the person's picks for `agent` to the session just opened | continued, in the agent's order: each one
    /// still offered and not on already; a refusal skips it. Answers the options as they are afterwards.
    pub(super) fn apply_picks(
        &self,
        generation: u64,
        agent: &Agent,
        session: &SessionId,
        mut options: Vec<SessionConfigOption>,
    ) -> Vec<SessionConfigOption> {
        let mut picks = self.picks_of(&agent.choice);
        if picks.is_empty() {
            return options;
        }
        let order: Vec<String> = options
            .iter()
            .filter(|o| category(o).is_some())
            .map(|o| o.id.0.to_string())
            .collect();
        for id in order {
            let Some(wanted) = picks.remove(&id) else {
                continue;
            };
            // a change before may have changed what this one offers (a model's own reasoning levels)
            let Some(option) = changeable(&options, &id) else {
                continue;
            };
            let Some(value) = offered(option, &wanted).filter(|_| !holds(option, &wanted)) else {
                continue;
            };
            if !self.is_current(generation) {
                break;
            }
            let set: Result<Value, CallError> = agent.connection.call(
                AGENT_METHOD_NAMES.session_set_config_option,
                SetSessionConfigOptionRequest::new(session.clone(), option.id.clone(), value),
                Some(SETUP_WAIT),
            );
            match set {
                Ok(raw) => {
                    if let Some(now) = answered(raw) {
                        options = now;
                    }
                }
                // gone: the start | the turn after it says so
                Err(CallError::Closed) => break,
                Err(_) => {}
            }
        }
        options
    }

    /// The person's change of option `id` to `value` (checked: [`value_for`]) in `live`: taken -> the options as the
    /// agent answers them, the pick kept for its agent; refused -> said with the agent's words, the options told
    /// again as they are.
    pub(super) fn set_option(
        &self,
        generation: u64,
        live: &Live,
        id: &str,
        value: SessionConfigOptionValue,
        picked: &AcpConfigValue,
    ) {
        let connection = &live.agent.connection;
        let set: Result<Value, CallError> = connection.call(
            AGENT_METHOD_NAMES.session_set_config_option,
            SetSessionConfigOptionRequest::new(live.session.clone(), id.to_owned(), value),
            Some(SETUP_WAIT),
        );
        let refusal = match set {
            Ok(raw) => {
                self.keep_pick(&live.agent.choice, id, picked);
                // no list in the answer: the window hears the one kept (a news of the agent may follow)
                return self.keep_options(generation, answered(raw), true);
            }
            // the agent left: whatever runs reports it
            Err(CallError::Closed) => return,
            Err(CallError::Refused { message, .. }) => said(&message),
            Err(CallError::TimedOut | CallError::Unreadable) => None,
        };
        // the control the person changed shows the value that holds again
        self.keep_options(generation, None, true);
        let status = self.with_current(generation, |inner| AcpUpdate::Status {
            state: inner.state.name(),
            error: Some(AcpError {
                message: refusal,
                ..AcpError::new(AcpErrorCode::OptionRefused)
            }),
            methods: inner.state.methods(),
        });
        if let Some(status) = status {
            self.emit(status);
        }
    }
}
