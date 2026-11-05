//! Engine seam in the plugin: contract + worker protocol + host proxy = crate `permark-engine` (re-exported here;
//! rules in its crate doc). Plugin side: [`model_files`] (installed version -> files) + [`FakeEngine`] (tests of
//! future features).

mod fake;

pub use permark_engine::*;

use crate::catalog::Catalog;
use crate::installs::AssignedModel;

pub use fake::FakeEngine;

/// Files of the installed version `model` (from [`crate::Installs::model_for`]); prefix profile from `catalog`
/// entry of the same id (lineage-stable).
#[must_use]
pub fn model_files(model: &AssignedModel, catalog: &Catalog) -> ModelFiles {
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
        prefix_profile: catalog
            .model(&model.id)
            .and_then(|m| m.embedding.as_ref())
            .map(|e| e.prefix_profile.clone()),
    }
}

#[cfg(test)]
mod tests;
