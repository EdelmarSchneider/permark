//! The project on disk, as the planner sees it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::path::Path;

use super::plan::{EntryKind, World};
use crate::render::markdown::is_markdown_path;
use crate::util::read_with_retry;
use crate::vault::{NodeKind, Vault};

/// A [`World`] over a [`Vault`]. What a path is (file, folder, nothing) is asked of the disk once.
pub(crate) struct VaultWorld<'a> {
    vault: &'a Vault,
    kinds: RefCell<HashMap<String, Option<EntryKind>>>,
}

impl<'a> VaultWorld<'a> {
    pub(crate) fn new(vault: &'a Vault) -> Self {
        Self {
            vault,
            kinds: RefCell::new(HashMap::new()),
        }
    }
}

impl World for VaultWorld<'_> {
    fn documents(&self) -> Vec<String> {
        self.vault
            .walk()
            .into_iter()
            .filter(|item| item.kind == NodeKind::File && is_markdown_path(Path::new(&item.rel)))
            .map(|item| item.rel)
            .collect()
    }

    fn read(&self, rel: &str) -> io::Result<Vec<u8>> {
        let resolved = self
            .vault
            .resolve(rel)
            .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?;
        read_with_retry(&resolved.abs)
    }

    fn kind(&self, rel: &str) -> Option<EntryKind> {
        if let Some(known) = self.kinds.borrow().get(rel) {
            return *known;
        }
        let kind = self.vault.resolve(rel).ok().and_then(|r| {
            let meta = std::fs::metadata(r.abs).ok()?;
            Some(if meta.is_dir() {
                EntryKind::Dir
            } else {
                EntryKind::File
            })
        });
        self.kinds.borrow_mut().insert(rel.to_owned(), kind);
        kind
    }
}
