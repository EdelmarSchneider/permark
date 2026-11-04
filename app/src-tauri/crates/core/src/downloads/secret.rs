//! Per-source credential (e.g. Hugging Face token): kept by the OS credential store (app crate impl), handed to the
//! transport for the source's first host only. NEVER logged, serialized, sent over IPC | to agents: UI sees `is_set`.

use std::collections::HashMap;
use std::sync::Mutex;

use super::Source;
use crate::error::{ErrorKind, IpcError};

/// A credential value. No `Serialize`, `Debug` masked: the value leaves only through [`Secret::expose`].
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw value: ONLY for the transport's `Authorization` header | the credential store.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Credential store failure (no keychain service, access refused). Text never holds the value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    #[error("credential store unavailable: {0}")]
    Unavailable(String),
}

impl From<SecretError> for IpcError {
    fn from(error: SecretError) -> Self {
        match error {
            SecretError::Unavailable(detail) => IpcError::new(ErrorKind::Unavailable)
                .subject("credentials")
                .detail(detail),
        }
    }
}

/// Credentials per [`Source`] (slot = [`Source::key`]).
pub trait SecretStore: Send + Sync {
    /// # Errors
    /// [`SecretError`]: store unreachable.
    fn get(&self, source: &Source) -> Result<Option<Secret>, SecretError>;

    /// # Errors
    /// [`SecretError`]: store unreachable | refused.
    fn set(&self, source: &Source, secret: Secret) -> Result<(), SecretError>;

    /// Removes the credential (absent: no error).
    ///
    /// # Errors
    /// [`SecretError`]: store unreachable | refused.
    fn clear(&self, source: &Source) -> Result<(), SecretError>;

    /// The only fact a UI sees.
    ///
    /// # Errors
    /// [`SecretError`]: store unreachable.
    fn is_set(&self, source: &Source) -> Result<bool, SecretError> {
        Ok(self.get(source)?.is_some())
    }
}

/// In-memory store (tests, builds without an OS store). Lost on exit.
#[derive(Debug, Default)]
pub struct MemorySecrets(Mutex<HashMap<&'static str, Secret>>);

impl MemorySecrets {
    fn with<T>(&self, f: impl FnOnce(&mut HashMap<&'static str, Secret>) -> T) -> T {
        let mut map = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut map)
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, source: &Source) -> Result<Option<Secret>, SecretError> {
        Ok(self.with(|map| map.get(source.key()).cloned()))
    }

    fn set(&self, source: &Source, secret: Secret) -> Result<(), SecretError> {
        self.with(|map| map.insert(source.key(), secret));
        Ok(())
    }

    fn clear(&self, source: &Source) -> Result<(), SecretError> {
        self.with(|map| map.remove(source.key()));
        Ok(())
    }
}
