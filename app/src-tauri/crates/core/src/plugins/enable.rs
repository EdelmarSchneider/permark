//! Which plugins are on: pure resolution of settings + project overrides + dependency closure.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use ts_rs::TS;

use super::{PluginId, PluginRegistry, PluginScope};

/// Why a plugin is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "reason", content = "plugin", rename_all = "camelCase")]
#[ts(export)]
pub enum OffReason {
    /// App setting (or manifest default) says off.
    Setting,
    /// This project turned it off (`.permark/config.toml` | the person's per-project choice).
    Project,
    /// A plugin it depends on is off.
    Dependency(PluginId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", content = "why", rename_all = "camelCase")]
#[ts(export)]
pub enum Enablement {
    On,
    Off(OffReason),
}

/// Inputs of one resolution. App-wide resolution: project parts empty.
#[derive(Debug, Default, Clone)]
pub struct EnableInputs {
    /// `[plugins.<id>] enabled` from app settings.
    pub app: BTreeMap<PluginId, bool>,
    /// `.permark/config.toml` `[plugins.<id>] enabled = false` (may only disable).
    pub project_config_off: BTreeSet<PluginId>,
    /// Person's per-project choices (profile): may enable | disable.
    pub project_choice: BTreeMap<PluginId, bool>,
}

/// Every plugin -> `On | Off(reason)`. Project overrides apply to Project-scope plugins only (App plugins are one
/// per process). Dependency closure in registry order: a dependent of an off plugin is off.
#[must_use]
pub fn resolve_enabled(
    registry: &PluginRegistry,
    inputs: &EnableInputs,
) -> BTreeMap<PluginId, Enablement> {
    let mut result: BTreeMap<PluginId, Enablement> = BTreeMap::new();
    for manifest in registry.ordered() {
        let id = &manifest.id;
        let mut on = inputs
            .app
            .get(id)
            .copied()
            .unwrap_or(manifest.default_enabled);
        let mut reason = OffReason::Setting;
        if manifest.scope == PluginScope::Project {
            if let Some(&choice) = inputs.project_choice.get(id) {
                on = choice;
                reason = OffReason::Project;
            }
            if inputs.project_config_off.contains(id) {
                on = false;
                reason = OffReason::Project;
            }
        }
        let state = if !on {
            Enablement::Off(reason)
        } else if let Some(dep) = manifest
            .depends_on
            .iter()
            .find(|dep| result.get(*dep) != Some(&Enablement::On))
        {
            Enablement::Off(OffReason::Dependency(dep.clone()))
        } else {
            Enablement::On
        };
        result.insert(id.clone(), state);
    }
    result
}

/// Ids that are on.
#[must_use]
pub fn on_ids(resolved: &BTreeMap<PluginId, Enablement>) -> BTreeSet<PluginId> {
    resolved
        .iter()
        .filter(|(_, state)| **state == Enablement::On)
        .map(|(id, _)| id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::fake::{fake, Fake};
    use crate::plugins::PluginScope::{App, Project};
    use std::sync::Arc;

    fn registry() -> PluginRegistry {
        PluginRegistry::new(vec![
            fake("updates", App, &[]),
            fake("git", Project, &[]),
            fake("unread", Project, &["git"]),
            fake("review", Project, &["unread"]),
            Arc::new(Fake::new("agent", Project, &[]).default_off()),
        ])
        .unwrap()
    }

    fn id(text: &'static str) -> PluginId {
        PluginId::of(text)
    }

    #[test]
    fn defaults_come_from_manifests() {
        let resolved = resolve_enabled(&registry(), &EnableInputs::default());
        assert_eq!(resolved[&id("git")], Enablement::On);
        assert_eq!(resolved[&id("agent")], Enablement::Off(OffReason::Setting));
    }

    #[test]
    fn dependency_off_turns_the_whole_chain_off_with_the_direct_reason() {
        let inputs = EnableInputs {
            app: BTreeMap::from([(id("git"), false)]),
            ..EnableInputs::default()
        };
        let resolved = resolve_enabled(&registry(), &inputs);
        assert_eq!(resolved[&id("git")], Enablement::Off(OffReason::Setting));
        assert_eq!(
            resolved[&id("unread")],
            Enablement::Off(OffReason::Dependency(id("git")))
        );
        assert_eq!(
            resolved[&id("review")],
            Enablement::Off(OffReason::Dependency(id("unread")))
        );
        assert_eq!(resolved[&id("updates")], Enablement::On);
    }

    #[test]
    fn project_config_only_disables_and_wins_over_the_persons_choice() {
        let inputs = EnableInputs {
            project_config_off: BTreeSet::from([id("unread"), id("updates")]),
            project_choice: BTreeMap::from([(id("unread"), true), (id("agent"), true)]),
            ..EnableInputs::default()
        };
        let resolved = resolve_enabled(&registry(), &inputs);
        assert_eq!(resolved[&id("unread")], Enablement::Off(OffReason::Project));
        assert_eq!(
            resolved[&id("agent")],
            Enablement::On,
            "per-project choice may enable"
        );
        assert_eq!(
            resolved[&id("updates")],
            Enablement::On,
            "project cannot touch app plugins"
        );
    }

    #[test]
    fn persons_project_choice_overrides_the_app_setting() {
        let inputs = EnableInputs {
            app: BTreeMap::from([(id("git"), true)]),
            project_choice: BTreeMap::from([(id("git"), false)]),
            ..EnableInputs::default()
        };
        let resolved = resolve_enabled(&registry(), &inputs);
        assert_eq!(resolved[&id("git")], Enablement::Off(OffReason::Project));
        assert_eq!(on_ids(&resolved), BTreeSet::from([id("updates")]));
    }

    #[test]
    fn unknown_ids_in_inputs_are_ignored() {
        let inputs = EnableInputs {
            app: BTreeMap::from([(id("ghost"), true)]),
            ..EnableInputs::default()
        };
        assert!(!resolve_enabled(&registry(), &inputs).contains_key(&id("ghost")));
    }
}
