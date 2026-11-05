//! Engine seam in the plugin: contract + worker protocol + host proxy = crate `permark-engine` (re-exported here;
//! rules in its crate doc). Plugin side: [`model_files`] (installed version -> files), [`files`] (pinned worker
//! program), [`FakeEngine`] (tests of future features).

mod fake;
pub mod files;

pub use permark_engine::*;

use crate::catalog::{self, Catalog};
use crate::installs::AssignedModel;

pub use fake::FakeEngine;

/// Files of the installed version `model` (from [`crate::Installs::model_for`]) + prompt data of the `catalog` entry
/// of the same id (lineage-stable): prefix set (name + prefixes), chat markup, llama.cpp floor.
#[must_use]
pub fn model_files(model: &AssignedModel, catalog: &Catalog) -> ModelFiles {
    let entry = catalog.model(&model.id);
    let prefix_profile = entry
        .and_then(|m| m.embedding.as_ref())
        .map(|e| e.prefix_profile.clone());
    ModelFiles {
        model_id: model.id.clone(),
        catalog_version: model.catalog_version,
        files: model
            .files
            .iter()
            .map(|f| {
                let name = f.path.rsplit('/').next().unwrap_or(&f.path);
                (f.role, model.dir.join(name))
            })
            .collect(),
        prefixes: prefix_profile
            .as_ref()
            .and_then(|name| catalog.prefix_profiles.get(name))
            .cloned(),
        prefix_profile,
        prompt: entry
            .and_then(|m| m.prompt_profile.as_ref())
            .and_then(|name| catalog.prompt_profiles.get(name))
            .cloned(),
        min_engine: entry
            .filter(|m| m.min_engine.engine == catalog::Engine::LlamaCpp)
            .and_then(|m| m.min_engine.version.clone()),
    }
}

#[cfg(test)]
pub(crate) mod tests;
