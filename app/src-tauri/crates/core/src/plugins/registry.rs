//! `PluginRegistry`: the plugins compiled in, validated once at construction; keeps a dependency order.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{is_valid_id, Manifest, Plugin, PluginId, PluginScope};
use crate::format::Contributions;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("invalid plugin id {0:?}")]
    InvalidId(String),
    #[error("plugin id {0} registered twice")]
    Duplicate(PluginId),
    #[error("plugin {plugin} depends on unknown plugin {dep}")]
    MissingDependency { plugin: PluginId, dep: PluginId },
    #[error("plugin dependency cycle: {}", .0.iter().map(PluginId::as_str).collect::<Vec<_>>().join(" -> "))]
    Cycle(Vec<PluginId>),
    #[error("app plugin {plugin} cannot depend on project plugin {dep}")]
    ScopeMismatch { plugin: PluginId, dep: PluginId },
}

pub struct PluginRegistry {
    plugins: Vec<Arc<dyn Plugin>>,
    /// Indexes into `plugins`: every plugin after its dependencies; peers in declaration order.
    order: Vec<usize>,
    by_id: BTreeMap<PluginId, usize>,
}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.ordered().map(|m| m.id.as_str()))
            .finish()
    }
}

impl PluginRegistry {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            plugins: Vec::new(),
            order: Vec::new(),
            by_id: BTreeMap::new(),
        }
    }

    /// Validates: id syntax, unique ids, dependencies exist, no cycles, App plugins never depend on Project ones.
    ///
    /// # Errors
    /// First violation found.
    pub fn new(plugins: Vec<Arc<dyn Plugin>>) -> Result<Self, RegistryError> {
        let mut by_id = BTreeMap::new();
        for (index, plugin) in plugins.iter().enumerate() {
            let id = &plugin.manifest().id;
            if !is_valid_id(id.as_str()) {
                return Err(RegistryError::InvalidId(id.as_str().to_owned()));
            }
            if by_id.insert(id.clone(), index).is_some() {
                return Err(RegistryError::Duplicate(id.clone()));
            }
        }
        for plugin in &plugins {
            let manifest = plugin.manifest();
            for dep in manifest.depends_on.iter() {
                let Some(&dep_index) = by_id.get(dep) else {
                    return Err(RegistryError::MissingDependency {
                        plugin: manifest.id.clone(),
                        dep: dep.clone(),
                    });
                };
                let dep_scope = plugins[dep_index].manifest().scope;
                if manifest.scope == PluginScope::App && dep_scope == PluginScope::Project {
                    return Err(RegistryError::ScopeMismatch {
                        plugin: manifest.id.clone(),
                        dep: dep.clone(),
                    });
                }
            }
        }
        let order = topo_order(&plugins, &by_id)?;
        Ok(Self {
            plugins,
            order,
            by_id,
        })
    }

    /// Manifests, dependencies first.
    pub fn ordered(&self) -> impl Iterator<Item = &Manifest> {
        self.order.iter().map(|&i| self.plugins[i].manifest())
    }

    #[must_use]
    pub fn get(&self, id: &PluginId) -> Option<&Arc<dyn Plugin>> {
        self.by_id.get(id).map(|&i| &self.plugins[i])
    }

    #[must_use]
    pub fn manifest(&self, id: &PluginId) -> Option<&Manifest> {
        self.get(id).map(|p| p.manifest())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// The extra roots the Project plugins in `on` offer for the project at `root` (plugin order).
    #[must_use]
    pub fn roots(
        &self,
        on: &BTreeSet<PluginId>,
        root: &crate::paths::Root,
    ) -> Vec<(String, std::path::PathBuf)> {
        let mut found = Vec::new();
        for &index in &self.order {
            let plugin = &self.plugins[index];
            let manifest = plugin.manifest();
            if manifest.scope == PluginScope::Project && on.contains(&manifest.id) {
                found.extend(plugin.roots(root));
            }
        }
        found
    }

    /// What the plugins in `on` contribute, in plugin order (dependencies first).
    #[must_use]
    pub fn contributions(&self, on: &BTreeSet<PluginId>) -> Contributions {
        let mut all = Contributions::default();
        for &index in &self.order {
            let plugin = &self.plugins[index];
            if on.contains(&plugin.manifest().id) {
                all.extend(plugin.contributions());
            }
        }
        all
    }
}

/// Depth-first topological order; reports the cycle path.
fn topo_order(
    plugins: &[Arc<dyn Plugin>],
    by_id: &BTreeMap<PluginId, usize>,
) -> Result<Vec<usize>, RegistryError> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Visiting,
        Done,
    }
    fn visit(
        index: usize,
        plugins: &[Arc<dyn Plugin>],
        by_id: &BTreeMap<PluginId, usize>,
        marks: &mut [Mark],
        path: &mut Vec<usize>,
        order: &mut Vec<usize>,
    ) -> Result<(), RegistryError> {
        match marks[index] {
            Mark::Done => return Ok(()),
            Mark::Visiting => {
                let start = path.iter().position(|&i| i == index).unwrap_or(0);
                let mut cycle: Vec<PluginId> = path[start..]
                    .iter()
                    .map(|&i| plugins[i].manifest().id.clone())
                    .collect();
                cycle.push(plugins[index].manifest().id.clone());
                return Err(RegistryError::Cycle(cycle));
            }
            Mark::New => {}
        }
        marks[index] = Mark::Visiting;
        path.push(index);
        for dep in plugins[index].manifest().depends_on.iter() {
            if let Some(&dep_index) = by_id.get(dep) {
                visit(dep_index, plugins, by_id, marks, path, order)?;
            }
        }
        path.pop();
        marks[index] = Mark::Done;
        order.push(index);
        Ok(())
    }
    let mut marks = vec![Mark::New; plugins.len()];
    let mut order = Vec::with_capacity(plugins.len());
    let mut path = Vec::new();
    for index in 0..plugins.len() {
        visit(index, plugins, by_id, &mut marks, &mut path, &mut order)?;
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::fake::{fake, Fake};
    use crate::plugins::PluginScope::{App, Project};

    fn ids(registry: &PluginRegistry) -> Vec<&str> {
        registry.ordered().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn orders_dependencies_first_stable_among_peers() {
        let registry = PluginRegistry::new(vec![
            fake("review", Project, &["unread", "git"]),
            fake("git", Project, &[]),
            fake("unread", Project, &["git"]),
            fake("word-count", Project, &[]),
            fake("updates", App, &[]),
        ])
        .unwrap();
        assert_eq!(
            ids(&registry),
            ["git", "unread", "review", "word-count", "updates"]
        );
    }

    #[test]
    fn rejects_duplicates_bad_ids_and_missing_deps() {
        assert_eq!(
            PluginRegistry::new(vec![fake("git", Project, &[]), fake("git", App, &[])])
                .unwrap_err(),
            RegistryError::Duplicate(PluginId::of("git"))
        );
        assert!(matches!(
            PluginRegistry::new(vec![fake("Bad_Id", App, &[])]),
            Err(RegistryError::InvalidId(_))
        ));
        assert!(matches!(
            PluginRegistry::new(vec![fake("review", Project, &["unread"])]),
            Err(RegistryError::MissingDependency { .. })
        ));
    }

    #[test]
    fn rejects_cycles_with_their_path() {
        let err = PluginRegistry::new(vec![
            fake("a", Project, &["b"]),
            fake("b", Project, &["c"]),
            fake("c", Project, &["a"]),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            RegistryError::Cycle(["a", "b", "c", "a"].map(PluginId::of).to_vec())
        );
        let self_loop = PluginRegistry::new(vec![fake("a", App, &["a"])]).unwrap_err();
        assert!(matches!(self_loop, RegistryError::Cycle(c) if c.len() == 2));
    }

    #[test]
    fn app_plugins_cannot_depend_on_project_plugins() {
        assert!(matches!(
            PluginRegistry::new(vec![
                fake("git", Project, &[]),
                fake("updates", App, &["git"])
            ]),
            Err(RegistryError::ScopeMismatch { .. })
        ));
        // the other way round is fine
        assert!(PluginRegistry::new(vec![
            fake("updates", App, &[]),
            fake("git", Project, &["updates"])
        ])
        .is_ok());
    }

    #[test]
    fn long_chains_and_wide_registries_order_correctly() {
        let names: Vec<&'static str> = (0..200)
            .map(|i| &*Box::leak(format!("p{i}").into_boxed_str()))
            .collect();
        let plugins: Vec<Arc<dyn Plugin>> = names
            .iter()
            .enumerate()
            .rev()
            .map(|(i, name)| -> Arc<dyn Plugin> {
                let deps: &'static [&'static str] = if i == 0 {
                    &[]
                } else {
                    Box::leak(Box::new([names[i - 1]]))
                };
                Arc::new(Fake::new(name, Project, deps))
            })
            .collect();
        let registry = PluginRegistry::new(plugins).unwrap();
        assert_eq!(ids(&registry), names);
    }
}
