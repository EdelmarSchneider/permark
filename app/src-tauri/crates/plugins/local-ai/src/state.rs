//! What the plugin keeps in the profile (D-6): `<profile>/plugins/local-ai/state.json` (`state::StateFile`). Written
//! ONLY after a person's action (accept terms, assign, a download that finished, remove); never at start.

use std::collections::BTreeMap;
use std::path::PathBuf;

use permark_core::downloads::Unit;
use permark_core::state::StateData;
use serde::{Deserialize, Serialize};

use crate::catalog::{install_folder, unit_of, CatalogError, License, Model, ModelFile};
use crate::tasks::Task;

/// File name in the plugin's namespace.
pub const FILE: &str = "state.json";

/// Terms the person accepted before a download (AC-4): one record per license address + terms revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Acceptance {
    /// Model the person was getting.
    pub model: String,
    pub license_url: String,
    /// Catalog's terms revision accepted (`None`: a record made before revisions; asked again).
    #[serde(default)]
    pub revision: Option<String>,
    /// Unix time, ms.
    pub accepted_at: u64,
}

/// One installed version of a lineage: the pins it was installed with (its unit is rebuilt from them even after the
/// catalog moved on to a newer version).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub model: String,
    pub catalog_version: u32,
    pub repo: String,
    pub commit: String,
    pub files: Vec<ModelFile>,
}

impl Installed {
    #[must_use]
    pub fn of(model: &Model) -> Self {
        Self {
            model: model.id.clone(),
            catalog_version: model.catalog_version,
            repo: model.repo.clone(),
            commit: model.commit.clone(),
            files: model.files.clone(),
        }
    }

    /// Install unit of this version of `lineage`.
    ///
    /// # Errors
    /// [`CatalogError`]: a record that no longer makes valid pins.
    pub fn unit(&self, lineage: &str) -> Result<Unit, CatalogError> {
        unit_of(
            &self.model,
            &install_folder(lineage, self.catalog_version),
            &self.repo,
            &self.commit,
            &self.files,
        )
    }
}

/// An old version whose removal failed (file held open | store busy): removed again by the next action that writes
/// to the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stale {
    pub lineage: String,
    pub installed: Installed,
    /// Store holding it (`None`: a record made before stores were named = the store in use).
    #[serde(default)]
    pub store: Option<PathBuf>,
}

/// The plugin state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginState {
    #[serde(default)]
    pub acceptances: Vec<Acceptance>,
    /// Task -> lineage (follows updates).
    #[serde(default)]
    pub assignments: BTreeMap<Task, String>,
    /// Chosen store folder; `None` = default `<profile>/plugins/local-ai/models/`.
    #[serde(default)]
    pub store: Option<PathBuf>,
    /// Lineage -> installed version.
    #[serde(default)]
    pub installed: BTreeMap<String, Installed>,
    #[serde(default)]
    pub stale: Vec<Stale>,
}

impl StateData for PluginState {
    const VERSION: u32 = 1;
}

impl PluginState {
    /// Terms `license` already accepted (for any model) at its catalog revision.
    #[must_use]
    pub fn accepted(&self, license: &License) -> bool {
        self.acceptances
            .iter()
            .any(|a| a.license_url == license.url && a.revision == license.revision)
    }

    /// Drops the tasks assigned to `lineage`.
    pub fn unassign(&mut self, lineage: &str) {
        self.assignments.retain(|_, assigned| assigned != lineage);
    }
}
