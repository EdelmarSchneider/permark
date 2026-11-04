//! Verified downloads: a pinned file (source + path, size, sha256) the person asked for, fetched over an allowlisted
//! `https` source into a store folder the caller names. Pure over the [`Transport`] seam (app crate supplies the real
//! client) and the [`SecretStore`] seam (OS credential store in the app crate). Nothing here touches the network by
//! itself: a plugin calls it after a person's action.

mod error;
mod secret;
mod transport;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use error::DownloadError;
pub use secret::{MemorySecrets, Secret, SecretError, SecretStore};
pub use transport::{
    open, Asked, Opened, Request, Response, Scripted, ScriptedTransport, Transport,
};

/// Where pinned files come from: a fixed `https` base (no free URLs from callers) + host suffixes every hop of a
/// redirect chain must match. Defined in code (pins are covered by the app signature).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    key: &'static str,
    base: &'static str,
    hosts: &'static [&'static str],
}

impl Source {
    /// Hugging Face: `https://huggingface.co`; its CDN redirects stay under `huggingface.co` | `hf.co`.
    pub const HUGGING_FACE: Self = Self::new(
        "huggingface",
        "https://huggingface.co",
        &["huggingface.co", "hf.co"],
    );

    /// A source: `key` names it (credential slot), `base` = `https://host` (no path, no trailing `/`), `hosts` =
    /// allowed host suffixes (a host matches itself | any subdomain). A base outside `hosts` | not `https` is refused
    /// at request time ([`DownloadError::HostNotAllowed`]).
    #[must_use]
    pub const fn new(
        key: &'static str,
        base: &'static str,
        hosts: &'static [&'static str],
    ) -> Self {
        Self { key, base, hosts }
    }

    /// Credential slot name (`SecretStore` key).
    #[must_use]
    pub const fn key(&self) -> &'static str {
        self.key
    }

    /// Address of `path` (validated by [`Asset::new`]: starts with `/`).
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Host of the base, lowercase: the only host a credential is sent to.
    fn first_host(&self) -> String {
        host_of(self.base).unwrap_or_default()
    }

    /// `host` equals an allowed suffix or is a subdomain of one (case-insensitive, one trailing dot ignored).
    fn allows(&self, host: &str) -> bool {
        self.hosts.iter().any(|suffix| {
            let suffix = suffix.to_ascii_lowercase();
            host == suffix
                || host
                    .strip_suffix(suffix.as_str())
                    .is_some_and(|head| head.ends_with('.'))
        })
    }
}

/// Lowercase host (no port, no trailing dot) of an `https://` address; `None` when not `https://`.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .get(..8)
        .filter(|s| s.eq_ignore_ascii_case("https://"))
        .map(|_| &url[8..])?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = crate::remote::host_part(authority);
    let host = host.strip_suffix('.').unwrap_or(host);
    Some(host.to_ascii_lowercase())
}

/// Why an [`Asset`] cannot be built (a bug in the pin, never person input).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidAsset {
    /// Path not `/...`, or holding `..`, `\`, `?`, `#`, whitespace, control characters.
    #[error("invalid asset path")]
    Path,
    /// Name empty, a path, `.`-leading (scratch names are hidden + swept), or holding control characters.
    #[error("invalid asset file name")]
    Name,
}

/// One pinned file: where it comes from, what it must be, what it is called in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    source: Source,
    path: String,
    name: String,
    bytes: u64,
    sha256: [u8; 32],
    revision: Option<String>,
}

impl Asset {
    /// Pin of `source` + `path` (e.g. `/org/repo/resolve/<commit>/model.gguf`) of exactly `bytes` bytes hashing to
    /// `sha256`, stored as `name`.
    ///
    /// # Errors
    /// [`InvalidAsset`]: bad path | name.
    pub fn new(
        source: Source,
        path: impl Into<String>,
        name: impl Into<String>,
        bytes: u64,
        sha256: [u8; 32],
    ) -> Result<Self, InvalidAsset> {
        let (path, name) = (path.into(), name.into());
        let bad_char = |c: char| c.is_whitespace() || c.is_control();
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains(['\\', '?', '#'])
            || path.chars().any(bad_char)
            || path.split('/').any(|segment| segment == "..")
        {
            return Err(InvalidAsset::Path);
        }
        if name.is_empty()
            || name.starts_with('.')
            || name.contains(['/', '\\', ':'])
            || name.chars().any(char::is_control)
        {
            return Err(InvalidAsset::Name);
        }
        Ok(Self {
            source,
            path,
            name,
            bytes,
            sha256,
            revision: None,
        })
    }

    /// Records the pinned revision (e.g. a commit sha) in the part meta; not part of the address.
    #[must_use]
    pub fn with_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
    }

    #[must_use]
    pub const fn source(&self) -> &Source {
        &self.source
    }

    /// File name in the store.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Pinned size.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Pinned sha256.
    #[must_use]
    pub const fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    #[must_use]
    pub fn revision(&self) -> Option<&str> {
        self.revision.as_deref()
    }

    /// First address asked (the source's base + path).
    #[must_use]
    pub fn url(&self) -> String {
        self.source.url(&self.path)
    }
}

/// Step of one download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "DownloadPhase")]
pub enum Phase {
    Downloading,
    /// Rehashing an existing part before resuming | the final check.
    Verifying,
}

/// Progress of one download (plugin UIs show it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "DownloadProgress")]
pub struct Progress {
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number")]
    pub total: u64,
    pub phase: Phase,
}

#[cfg(test)]
mod tests;
