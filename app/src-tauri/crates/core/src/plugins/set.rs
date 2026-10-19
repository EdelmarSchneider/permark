//! `PluginSet`: running instances of one scope (the app, or one project). Start deps first; stop dependents first;
//! Drop stops all in reverse start order (teardown symmetry).

use std::collections::BTreeSet;

use super::{PluginCx, PluginError, PluginId, PluginRegistry, PluginScope, Running};

pub struct PluginSet {
    scope: PluginScope,
    /// Start order (dependencies before dependents).
    running: Vec<(PluginId, Box<dyn Running>)>,
}

impl std::fmt::Debug for PluginSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginSet")
            .field("scope", &self.scope)
            .field("running", &self.running_ids())
            .finish()
    }
}

impl PluginSet {
    #[must_use]
    pub fn new(scope: PluginScope) -> Self {
        Self {
            scope,
            running: Vec::new(),
        }
    }

    #[must_use]
    pub fn running_ids(&self) -> Vec<PluginId> {
        self.running.iter().map(|(id, _)| id.clone()).collect()
    }

    #[must_use]
    pub fn is_running(&self, id: &PluginId) -> bool {
        self.running.iter().any(|(r, _)| r == id)
    }

    /// Brings the set to `enabled` (ids of every scope that are on; App ids satisfy Project deps). Instances not
    /// wanted stop (reverse start order); wanted ones start (registry order); unchanged keep running. A start
    /// failure leaves that plugin + its dependents off. Returns the failures.
    pub fn apply(
        &mut self,
        registry: &PluginRegistry,
        enabled: &BTreeSet<PluginId>,
        make_cx: impl Fn(&PluginId) -> PluginCx,
    ) -> Vec<PluginError> {
        // target = enabled plugins of this scope whose deps are all enabled (dependency-closed)
        let mut target: BTreeSet<PluginId> = BTreeSet::new();
        for manifest in registry.ordered() {
            if manifest.scope != self.scope || !enabled.contains(&manifest.id) {
                continue;
            }
            let deps_ok = manifest.depends_on.iter().all(|dep| {
                registry.manifest(dep).is_some_and(|m| {
                    if m.scope == self.scope {
                        target.contains(dep)
                    } else {
                        enabled.contains(dep)
                    }
                })
            });
            if deps_ok {
                target.insert(manifest.id.clone());
            }
        }
        // stop: newest first, so dependents go before what they depend on
        let mut index = self.running.len();
        while index > 0 {
            index -= 1;
            if !target.contains(&self.running[index].0) {
                let stopped = self.running.remove(index);
                drop(stopped);
            }
        }
        // start: registry order, only when same-scope deps actually run
        let mut failures = Vec::new();
        for manifest in registry.ordered() {
            let id = &manifest.id;
            if !target.contains(id) || self.is_running(id) {
                continue;
            }
            let deps_running = manifest.depends_on.iter().all(|dep| {
                registry
                    .manifest(dep)
                    .is_some_and(|m| m.scope != self.scope || self.is_running(dep))
            });
            if !deps_running {
                continue;
            }
            let Some(plugin) = registry.get(id) else {
                continue;
            };
            match plugin.start(&make_cx(id)) {
                Ok(instance) => self.running.push((id.clone(), instance)),
                Err(error) => failures.push(error),
            }
        }
        failures
    }

    /// Stops everything (reverse start order).
    pub fn stop_all(&mut self) {
        while let Some(instance) = self.running.pop() {
            drop(instance);
        }
    }
}

impl Drop for PluginSet {
    fn drop(&mut self) {
        self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conditions::{Context, Distribution};
    use crate::events::Recorder;
    use crate::plugins::fake::{Fake, Log};
    use crate::plugins::{
        CxWorkspace, Plugin,
        PluginScope::{App, Project},
    };
    use std::path::PathBuf;
    use std::sync::Arc;

    fn setup(log: &Log) -> PluginRegistry {
        let plugins: Vec<Arc<dyn Plugin>> = vec![
            Arc::new(Fake::new("updates", App, &[]).with_log(log)),
            Arc::new(Fake::new("git", Project, &["updates"]).with_log(log)),
            Arc::new(Fake::new("unread", Project, &["git"]).with_log(log)),
            Arc::new(Fake::new("review", Project, &["unread"]).with_log(log)),
            Arc::new(Fake::new("tags", Project, &[]).with_log(log)),
        ];
        PluginRegistry::new(plugins).unwrap()
    }

    fn cx(id: &PluginId) -> PluginCx {
        PluginCx::new(
            id.clone(),
            CxWorkspace::App,
            PathBuf::new(),
            None,
            Context::welcome(Distribution::Direct),
            Arc::new(Recorder::default()),
        )
    }

    fn ids(list: &[&'static str]) -> BTreeSet<PluginId> {
        list.iter().map(|s| PluginId::of(s)).collect()
    }

    fn take(log: &Log) -> Vec<String> {
        std::mem::take(&mut *log.lock().unwrap())
    }

    #[test]
    fn starts_deps_first_stops_dependents_first_and_drops_in_reverse() {
        let log = Log::default();
        let registry = setup(&log);
        let mut set = PluginSet::new(Project);
        let all = ids(&["updates", "git", "unread", "review", "tags"]);
        assert_eq!(set.apply(&registry, &all, cx).len(), 0);
        assert_eq!(
            take(&log),
            ["start:git", "start:unread", "start:review", "start:tags"]
        );
        // git off -> its chain stops, dependents first; tags untouched
        set.apply(
            &registry,
            &ids(&["updates", "unread", "review", "tags"]),
            cx,
        );
        assert_eq!(take(&log), ["stop:review", "stop:unread", "stop:git"]);
        // back on: restarts in order
        set.apply(&registry, &all, cx);
        assert_eq!(take(&log), ["start:git", "start:unread", "start:review"]);
        drop(set);
        assert_eq!(
            take(&log),
            ["stop:review", "stop:unread", "stop:git", "stop:tags"]
        );
    }

    #[test]
    fn app_dependency_off_keeps_project_dependents_off() {
        let log = Log::default();
        let registry = setup(&log);
        let mut set = PluginSet::new(Project);
        set.apply(&registry, &ids(&["git", "unread", "tags"]), cx);
        assert_eq!(take(&log), ["start:tags"]);
    }

    #[test]
    fn start_failure_leaves_dependents_off_and_others_running() {
        let log = Log::default();
        let plugins: Vec<Arc<dyn Plugin>> = vec![
            Arc::new(Fake::new("git", Project, &[]).with_log(&log).failing()),
            Arc::new(Fake::new("unread", Project, &["git"]).with_log(&log)),
            Arc::new(Fake::new("tags", Project, &[]).with_log(&log)),
        ];
        let registry = PluginRegistry::new(plugins).unwrap();
        let mut set = PluginSet::new(Project);
        let failures = set.apply(&registry, &ids(&["git", "unread", "tags"]), cx);
        assert_eq!(failures.len(), 1);
        assert_eq!(set.running_ids(), [PluginId::of("tags")]);
    }

    #[test]
    fn toggle_storm_ends_in_the_last_state_with_balanced_starts_and_stops() {
        let log = Log::default();
        let registry = setup(&log);
        let mut set = PluginSet::new(Project);
        let states = [
            ids(&["updates", "git", "unread", "review", "tags"]),
            ids(&["updates", "tags"]),
            ids(&[]),
            ids(&["updates", "git", "review"]),
            ids(&["updates", "git", "unread", "review"]),
        ];
        for round in 0..200 {
            set.apply(&registry, &states[round % states.len()], cx);
        }
        set.apply(&registry, &ids(&["updates", "git", "unread"]), cx);
        let names: Vec<String> = set.running_ids().iter().map(ToString::to_string).collect();
        assert_eq!(names, ["git", "unread"]);
        drop(set);
        let events = take(&log);
        let starts = events.iter().filter(|e| e.starts_with("start:")).count();
        let stops = events.iter().filter(|e| e.starts_with("stop:")).count();
        assert_eq!(starts, stops, "every start has its stop");
    }

    #[test]
    fn app_set_ignores_project_plugins() {
        let log = Log::default();
        let registry = setup(&log);
        let mut set = PluginSet::new(App);
        set.apply(&registry, &ids(&["updates", "git"]), cx);
        assert_eq!(take(&log), ["start:updates"]);
        assert_eq!(set.running_ids(), [PluginId::of("updates")]);
    }
}
