//! The curated model catalog: bundled `catalog.json` (`include_str!`), schema-versioned, pinned to commits.
//!
//! Rules (checked by [`validate`], tested on the bundled file):
//! - publishers ONLY [`PUBLISHERS`]; `repo` = `<publisher>/<name>`; `commit` = 40-hex sha (never a branch).
//! - every file pinned: `bytes` > 0 + 64-hex `sha256` (LFS oid). Pins built by `tools/build_pins.mjs`; the app never
//!   asks the HF API.
//! - address = `https://huggingface.co/<repo>/resolve/<commit>/<path>` via `downloads::Source::HUGGING_FACE`.
//! - `needs` (bytes) = rough estimate v1, refined in C4 by the engine: `ram_min` = all files x 1.2 (rounded up to
//!   MiB); `ram_rec` = `ram_min` + context overhead (1 GiB chat models, 256 MiB embed | speech); `vram_rec` =
//!   `ram_rec`.
//! - `languages` BCP-47; `mul` = many languages (100+, per model card).
//! - gated model: `gated: true` only (token from the person, `downloads::SecretStore`).

use std::collections::HashSet;
use std::sync::OnceLock;

use permark_core::downloads::{Asset, Source, Unit};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Schema of `catalog.json` this build reads.
pub const SCHEMA_VERSION: u32 = 1;

/// Repo owners the catalog may point at.
pub const PUBLISHERS: [&str; 4] = ["google", "LiquidAI", "ggml-org", "ggerganov"];

const BUNDLED: &str = include_str!("catalog.json");

/// What a model can do + part a file plays: engine contract types, catalog JSON names (camelCase).
pub use permark_engine::{Capability, FileRole};

/// One pinned file of a model (also kept in the plugin state for an installed version).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelFile {
    /// Path in the repo at `commit`.
    pub path: String,
    pub role: FileRole,
    /// Capabilities that need this file.
    pub required_for: Vec<Capability>,
    pub bytes: u64,
    /// LFS sha256, 64 hex.
    pub sha256: String,
}

/// Engine family a model runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Engine {
    #[serde(rename = "llama.cpp")]
    LlamaCpp,
    #[serde(rename = "whisper.cpp")]
    WhisperCpp,
}

/// Oldest engine that reads the files.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MinEngine {
    pub engine: Engine,
    /// Release tag (`b11960`, `v1.7.0`); `None`: not documented by the publisher (C4 checks at load).
    pub version: Option<String>,
}

/// Memory a model wants, bytes (rule: module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, rename = "ModelNeeds")]
pub struct Needs {
    #[ts(type = "number")]
    pub ram_min: u64,
    #[ts(type = "number")]
    pub ram_rec: u64,
    #[ts(type = "number")]
    pub vram_rec: u64,
}

/// Terms a model comes with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, rename = "ModelLicense")]
pub struct License {
    pub name: String,
    pub spdx: Option<String>,
    pub url: String,
    /// Plain note on who may use it (shown before Get).
    pub restriction: Option<String>,
    /// Person accepts before the first byte.
    pub needs_acceptance: bool,
    /// Version of the terms text (e.g. `v1.0`); required when `needs_acceptance`: a new revision asks again.
    #[serde(default)]
    pub revision: Option<String>,
}

/// How an embedding model is used.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Embedding {
    pub dims: u32,
    /// Shorter vectors the model supports (Matryoshka truncation).
    pub mrl_dims: Vec<u32>,
    pub pooling: String,
    /// Task prefixes (query | document) set name.
    pub prefix_profile: String,
}

/// One catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Model {
    /// Unique per entry.
    pub id: String,
    /// Stable across versions of one model (install folder, assignments follow it).
    pub lineage: String,
    /// Monotonic per lineage.
    pub catalog_version: u32,
    pub name: String,
    pub family: String,
    pub publisher: String,
    pub repo: String,
    pub commit: String,
    pub files: Vec<ModelFile>,
    pub quant: String,
    /// Parameter count (total, per model card).
    pub params: u64,
    pub context_len: u32,
    pub min_engine: MinEngine,
    pub needs: Needs,
    pub license: License,
    pub gated: bool,
    pub capabilities: Vec<Capability>,
    pub languages: Vec<String>,
    pub embedding: Option<Embedding>,
    pub prompt_profile: Option<String>,
    pub withdrawn: bool,
}

/// The catalog file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub models: Vec<Model>,
}

/// Why a catalog is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog does not parse: {0}")]
    Parse(String),
    #[error("catalog schema {0}, this build reads {SCHEMA_VERSION}")]
    Schema(u32),
    #[error("model {0}: duplicate id")]
    DuplicateId(String),
    #[error("model {0}: publisher not allowed")]
    Publisher(String),
    #[error("model {0}: repo is not <publisher>/<name>")]
    Repo(String),
    #[error("model {0}: commit is not a 40-hex sha")]
    Commit(String),
    #[error("model {0}: file {1} not pinned (bytes | sha256)")]
    Pin(String, String),
    #[error("model {0}: file {1} has no valid address")]
    Address(String, String),
    #[error("model {0}: capabilities and files disagree")]
    Capabilities(String),
    #[error("model {0}: no files")]
    NoFiles(String),
    #[error("model {0}: id | lineage is not a plain folder name")]
    Folder(String),
    #[error("model {0}: terms to accept need a revision")]
    License(String),
}

impl Catalog {
    /// Parses + validates `text`.
    ///
    /// # Errors
    /// [`CatalogError`]: bad JSON | any rule of [`validate`].
    pub fn parse(text: &str) -> Result<Self, CatalogError> {
        let catalog: Self =
            serde_json::from_str(text).map_err(|e| CatalogError::Parse(e.to_string()))?;
        validate(&catalog)?;
        Ok(catalog)
    }

    #[must_use]
    pub fn model(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }
}

/// The bundled catalog, parsed once.
///
/// # Errors
/// [`CatalogError`] when the bundled file is bad (a build bug: the tests read the same file).
pub fn bundled() -> Result<&'static Catalog, &'static CatalogError> {
    static CATALOG: OnceLock<Result<Catalog, CatalogError>> = OnceLock::new();
    CATALOG.get_or_init(|| Catalog::parse(BUNDLED)).as_ref()
}

/// Checks the rules of the module doc.
///
/// # Errors
/// First broken rule.
pub fn validate(catalog: &Catalog) -> Result<(), CatalogError> {
    if catalog.schema_version != SCHEMA_VERSION {
        return Err(CatalogError::Schema(catalog.schema_version));
    }
    let mut ids = HashSet::new();
    for model in &catalog.models {
        let id = || model.id.clone();
        if !ids.insert(model.id.as_str()) {
            return Err(CatalogError::DuplicateId(id()));
        }
        if !PUBLISHERS.contains(&model.publisher.as_str()) {
            return Err(CatalogError::Publisher(id()));
        }
        let name = model.repo.strip_prefix(&format!("{}/", model.publisher));
        if name.is_none_or(|n| n.is_empty() || n.contains('/')) {
            return Err(CatalogError::Repo(id()));
        }
        if !is_hex(&model.commit, 40) {
            return Err(CatalogError::Commit(id()));
        }
        if model.files.is_empty() {
            return Err(CatalogError::NoFiles(id()));
        }
        let license = &model.license;
        if license.needs_acceptance
            && license
                .revision
                .as_deref()
                .is_none_or(|r| r.trim().is_empty())
        {
            return Err(CatalogError::License(id()));
        }
        for file in &model.files {
            if file.bytes == 0 || !is_hex(&file.sha256, 64) {
                return Err(CatalogError::Pin(id(), file.path.clone()));
            }
        }
        model.unit()?;
        let covered = |cap: &Capability| model.files.iter().any(|f| f.required_for.contains(cap));
        let declared = |cap: &Capability| model.capabilities.contains(cap);
        if !model.capabilities.iter().all(covered)
            || !model
                .files
                .iter()
                .all(|f| f.required_for.iter().all(declared))
        {
            return Err(CatalogError::Capabilities(id()));
        }
    }
    Ok(())
}

impl Model {
    /// Total download size, bytes.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }

    /// The download pins of every file (stored under its file name; revision = commit).
    ///
    /// # Errors
    /// [`CatalogError::Pin`] | [`CatalogError::Address`].
    pub fn assets(&self) -> Result<Vec<Asset>, CatalogError> {
        assets_of(&self.id, &self.repo, &self.commit, &self.files)
    }

    /// Install unit of this version: id = model id, folder `<lineage>/<catalog_version>` (D-2).
    ///
    /// # Errors
    /// [`CatalogError::Pin`] | [`CatalogError::Address`] | [`CatalogError::Folder`].
    pub fn unit(&self) -> Result<Unit, CatalogError> {
        unit_of(
            &self.id,
            &install_folder(&self.lineage, self.catalog_version),
            &self.repo,
            &self.commit,
            &self.files,
        )
    }
}

/// Store folder of one installed version, relative (`/`-separated).
#[must_use]
pub fn install_folder(lineage: &str, catalog_version: u32) -> String {
    format!("{lineage}/{catalog_version}")
}

/// Install unit `id` of `files` pinned at `repo` + `commit`, in store folder `folder`.
///
/// # Errors
/// [`CatalogError::Pin`] | [`CatalogError::Address`] | [`CatalogError::Folder`].
pub fn unit_of(
    id: &str,
    folder: &str,
    repo: &str,
    commit: &str,
    files: &[ModelFile],
) -> Result<Unit, CatalogError> {
    Unit::new(id, folder, assets_of(id, repo, commit, files)?)
        .map_err(|_| CatalogError::Folder(id.to_owned()))
}

/// Download pins of `files` (stored under the file name; revision = commit).
fn assets_of(
    id: &str,
    repo: &str,
    commit: &str,
    files: &[ModelFile],
) -> Result<Vec<Asset>, CatalogError> {
    files
        .iter()
        .map(|file| {
            let sha256 = sha256_of(&file.sha256)
                .ok_or_else(|| CatalogError::Pin(id.to_owned(), file.path.clone()))?;
            let name = file.path.rsplit('/').next().unwrap_or(&file.path);
            Asset::new(
                Source::HUGGING_FACE,
                format!("/{repo}/resolve/{commit}/{}", file.path),
                name,
                file.bytes,
                sha256,
            )
            .map(|asset| asset.with_revision(commit.to_owned()))
            .map_err(|_| CatalogError::Address(id.to_owned(), file.path.clone()))
        })
        .collect()
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 64 lowercase hex -> 32 bytes.
fn sha256_of(text: &str) -> Option<[u8; 32]> {
    if !is_hex(text, 64) {
        return None;
    }
    let mut out = [0_u8; 32];
    for (byte, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// One model as the windows see it (RPC `local-ai.catalog`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "CatalogModel")]
pub struct ModelInfo {
    pub id: String,
    pub lineage: String,
    pub catalog_version: u32,
    pub name: String,
    pub family: String,
    pub publisher: String,
    pub repo: String,
    /// Pinned revision (Details).
    pub commit: String,
    pub quant: String,
    #[ts(type = "number")]
    pub params: u64,
    pub context_len: u32,
    /// Total download size.
    #[ts(type = "number")]
    pub bytes: u64,
    pub needs: Needs,
    pub license: License,
    /// Needs a Hugging Face token.
    pub gated: bool,
    pub capabilities: Vec<Capability>,
    /// BCP-47; `mul` = many.
    pub languages: Vec<String>,
    pub withdrawn: bool,
}

/// The catalog as the windows see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "LocalAiCatalog")]
pub struct CatalogInfo {
    pub schema_version: u32,
    pub models: Vec<ModelInfo>,
}

impl From<&Model> for ModelInfo {
    fn from(m: &Model) -> Self {
        Self {
            id: m.id.clone(),
            lineage: m.lineage.clone(),
            catalog_version: m.catalog_version,
            name: m.name.clone(),
            family: m.family.clone(),
            publisher: m.publisher.clone(),
            repo: m.repo.clone(),
            commit: m.commit.clone(),
            quant: m.quant.clone(),
            params: m.params,
            context_len: m.context_len,
            bytes: m.bytes(),
            needs: m.needs,
            license: m.license.clone(),
            gated: m.gated,
            capabilities: m.capabilities.clone(),
            languages: m.languages.clone(),
            withdrawn: m.withdrawn,
        }
    }
}

impl From<&Catalog> for CatalogInfo {
    fn from(c: &Catalog) -> Self {
        Self {
            schema_version: c.schema_version,
            models: c.models.iter().map(ModelInfo::from).collect(),
        }
    }
}
