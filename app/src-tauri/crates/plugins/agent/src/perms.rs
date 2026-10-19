//! The switch and the permission groups: the person's choices live in the profile only (`agent.json` per project,
//! `agent-defaults.json` for new projects); a project's `.permark/config.toml` may only take groups away.

use std::collections::{BTreeMap, BTreeSet};

use permark_core::state::{Loaded, StateData, StateStore};
use serde::{Deserialize, Serialize};

/// Per-project file (profile only, `PROFILE_ONLY`).
pub const PROJECT_FILE: &str = "agent.json";
/// App-wide defaults a project starts from when its switch first goes on.
pub const DEFAULTS_FILE: &str = "agent-defaults.json";

/// What an agent may do, by kind of tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Context,
    Changes,
    Find,
    CommentsRead,
    CommentsResolve,
    Notes,
    Show,
}

impl Group {
    pub const ALL: [Self; 7] = [
        Self::Context,
        Self::Changes,
        Self::Find,
        Self::CommentsRead,
        Self::CommentsResolve,
        Self::Notes,
        Self::Show,
    ];

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::Changes => "changes",
            Self::Find => "find",
            Self::CommentsRead => "comments.read",
            Self::CommentsResolve => "comments.resolve",
            Self::Notes => "notes",
            Self::Show => "show",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.id() == text)
    }

    /// On for a project that has not chosen: everything but resolving comments (offered once, when first asked).
    #[must_use]
    pub fn default_on(self) -> bool {
        !matches!(self, Self::CommentsResolve)
    }
}

/// `agent.json` of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChoice {
    pub enabled: bool,
    /// Group id -> on; a group not listed takes its default.
    #[serde(default)]
    pub groups: BTreeMap<String, bool>,
    /// The person was offered `comments.resolve` once already.
    #[serde(default)]
    pub resolve_asked: bool,
}

impl StateData for ProjectChoice {
    const VERSION: u32 = 1;
}

/// `agent-defaults.json`: the groups a project starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default)]
    pub groups: BTreeMap<String, bool>,
}

impl StateData for Defaults {
    const VERSION: u32 = 1;
}

/// Whether a group is open for this project now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    On,
    /// The person turned it off (or never turned it on).
    Off,
    /// The project's `.permark/config.toml` denies it, whatever the person chose.
    DeniedByProject,
}

/// The groups as they apply: profile choice (else default), minus what the project denies. Pure.
#[must_use]
pub fn effective(
    choice: &BTreeMap<String, bool>,
    deny: &BTreeSet<String>,
) -> BTreeMap<Group, Access> {
    Group::ALL
        .into_iter()
        .map(|group| {
            let access = if deny.contains(group.id()) {
                Access::DeniedByProject
            } else if choice
                .get(group.id())
                .copied()
                .unwrap_or_else(|| group.default_on())
            {
                Access::On
            } else {
                Access::Off
            };
            (group, access)
        })
        .collect()
}

/// The person's choices for one project, read fresh (the switch can change while an agent runs).
#[must_use]
pub fn load(store: &StateStore) -> ProjectChoice {
    match store.profile_file::<ProjectChoice>(PROJECT_FILE).load() {
        Loaded::Found(choice) => choice,
        Loaded::Missing | Loaded::Recovered { .. } => ProjectChoice::default(),
    }
}

/// Whether the profile holds a choice for the project of `store` (a project PerMark has been used on).
#[must_use]
pub fn known(store: &StateStore) -> bool {
    store
        .profile_file::<ProjectChoice>(PROJECT_FILE)
        .path()
        .is_file()
}

/// The app-wide defaults (missing -> the built-in ones).
#[must_use]
pub fn defaults(store: &StateStore) -> BTreeMap<String, bool> {
    match store.app_file::<Defaults>(DEFAULTS_FILE).load() {
        Loaded::Found(defaults) => defaults.groups,
        Loaded::Missing | Loaded::Recovered { .. } => BTreeMap::new(),
    }
}

/// Only known group ids survive (a hand-edited file cannot add others).
#[must_use]
pub fn clean(groups: &BTreeMap<String, bool>) -> BTreeMap<String, bool> {
    groups
        .iter()
        .filter(|(id, _)| Group::parse(id).is_some())
        .map(|(id, on)| (id.clone(), *on))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn defaults_are_all_but_resolving_comments() {
        let groups = effective(&BTreeMap::new(), &BTreeSet::new());
        for group in Group::ALL {
            let expected = if group == Group::CommentsResolve {
                Access::Off
            } else {
                Access::On
            };
            assert_eq!(groups[&group], expected, "{}", group.id());
        }
    }

    #[test]
    fn the_project_can_only_narrow() {
        let mut choice = BTreeMap::new();
        choice.insert("comments.resolve".to_owned(), true);
        choice.insert("find".to_owned(), false);
        let groups = effective(&choice, &set(&["comments.resolve", "notes", "unknown"]));
        assert_eq!(groups[&Group::CommentsResolve], Access::DeniedByProject);
        assert_eq!(groups[&Group::Notes], Access::DeniedByProject);
        assert_eq!(
            groups[&Group::Find],
            Access::Off,
            "deny never turns a group on"
        );
        assert_eq!(groups[&Group::Context], Access::On);
    }

    #[test]
    fn group_ids_round_trip_and_unknown_ones_are_dropped() {
        for group in Group::ALL {
            assert_eq!(Group::parse(group.id()), Some(group));
        }
        assert_eq!(Group::parse("Find"), None);
        let mut raw = BTreeMap::new();
        raw.insert("find".to_owned(), true);
        raw.insert("write".to_owned(), true);
        assert_eq!(clean(&raw).len(), 1);
    }
}
