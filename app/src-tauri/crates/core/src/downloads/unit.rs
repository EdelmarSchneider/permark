//! Install unit: files installed together (model + tokenizer, engine parts) in one store folder. Installed =
//! manifest `<id>.installed.json` present, listing exactly the unit's pins, every file of its pinned size. Manifest
//! written LAST, after every file verified.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::part::{remove_if_there, sha_hex, Files};
use super::{plain_name, Asset, InvalidAsset};
use crate::fsx;

/// Manifest size read back (a few pins).
const MANIFEST_LIMIT: u64 = 1 << 20;

/// Files installed together under `<store>/<folder>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    id: String,
    folder: Vec<String>,
    assets: Vec<Asset>,
}

impl Unit {
    /// Unit `id` of `assets` in store folder `folder` (`""` = store root, `/`-separated plain names).
    ///
    /// # Errors
    /// [`InvalidAsset::Unit`]: id | folder segment not a plain name, no assets, two assets of one name
    /// (case-insensitive), an asset name ending `.part` | `.part.meta`.
    pub fn new(
        id: impl Into<String>,
        folder: &str,
        assets: Vec<Asset>,
    ) -> Result<Self, InvalidAsset> {
        let id = id.into();
        let folder: Vec<String> = folder
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let mut names: Vec<String> = assets.iter().map(|a| a.name().to_lowercase()).collect();
        names.sort_unstable();
        let twins = names.windows(2).any(|w| w[0] == w[1]);
        let temp_named = names
            .iter()
            .any(|n| [".part", ".part.meta"].iter().any(|temp| n.ends_with(temp)));
        if !plain_name(&id)
            || !folder.iter().all(|s| plain_name(s))
            || assets.is_empty()
            || twins
            || temp_named
        {
            return Err(InvalidAsset::Unit);
        }
        Ok(Self { id, folder, assets })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn assets(&self) -> &[Asset] {
        &self.assets
    }

    /// Folder of the unit's files in `store`.
    pub(super) fn dir_in(&self, store: &Path) -> PathBuf {
        self.folder.iter().fold(store.to_path_buf(), |p, s| p.join(s))
    }

    fn manifest_path(&self, store: &Path) -> PathBuf {
        self.dir_in(store).join(format!("{}.installed.json", self.id))
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            unit: self.id.clone(),
            files: self
                .assets
                .iter()
                .map(|a| Pinned {
                    name: a.name().to_owned(),
                    bytes: a.bytes(),
                    sha256: sha_hex(a.sha256()),
                    revision: a.revision().map(str::to_owned),
                })
                .collect(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    unit: String,
    files: Vec<Pinned>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Pinned {
    name: String,
    bytes: u64,
    sha256: String,
    revision: Option<String>,
}

/// Writes the manifest (atomic): call ONLY after every file verified.
pub(super) fn write_manifest(store: &Path, unit: &Unit) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(&unit.manifest()).map_err(io::Error::other)?;
    fsx::write_atomic(&unit.manifest_path(store), &bytes)
}

/// Manifest lists exactly `unit`'s pins and every file has its pinned size.
pub(super) fn is_installed(store: &Path, unit: &Unit) -> bool {
    let dir = unit.dir_in(store);
    let listed = fsx::read_bytes(&unit.manifest_path(store), MANIFEST_LIMIT)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).ok())
        .is_some_and(|m| m == unit.manifest());
    listed
        && unit.assets.iter().all(|a| {
            std::fs::metadata(dir.join(a.name())).is_ok_and(|m| m.is_file() && m.len() == a.bytes())
        })
}

/// Manifest first (no longer installed), then files + parts.
pub(super) fn remove(store: &Path, unit: &Unit) -> io::Result<()> {
    remove_if_there(&unit.manifest_path(store))?;
    let dir = unit.dir_in(store);
    for asset in &unit.assets {
        let files = Files::of(&dir, asset);
        for path in [&files.target, &files.part, &files.meta] {
            remove_if_there(path)?;
        }
    }
    Ok(())
}
