//! What an agent may see of the project: one predicate for every tool, every list and every snippet. Hidden paths are
//! answered exactly as missing ones.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use permark_core::organizing::{SilenceApi, SilenceLookup};
use permark_core::paths::{fold, Root, Vault, VaultPath, CASE_INSENSITIVE, WORKTREE_PREFIX};

use crate::ToolError;

/// The ignore file agents honor (project root, gitignore syntax).
pub const AI_IGNORE: &str = ".aiignore";
/// Largest `.aiignore` read (a bigger one is read up to this size).
const MAX_IGNORE_BYTES: u64 = 1024 * 1024;
/// Folders of the project's own machinery, never shared.
const PRIVATE_FOLDERS: [&str; 2] = [".permark", ".git"];

/// The rules of one call (read fresh: `.aiignore` and `.permark/config.toml` can change while an agent runs).
pub struct Guard {
    /// The project folder only: linked worktrees live outside it.
    vault: Vault,
    rules: Option<Gitignore>,
    silence: Option<SilenceApi>,
}

impl Guard {
    #[must_use]
    pub fn new(root: &Root, deny_paths: &[String], silence: Option<SilenceApi>) -> Self {
        let mut builder = GitignoreBuilder::new(root.path());
        builder.case_insensitive(CASE_INSENSITIVE).ok();
        let mut any = false;
        let file = root.path().join(AI_IGNORE);
        if let Ok(bytes) = permark_core::fsx::read_bytes(&file, MAX_IGNORE_BYTES) {
            for line in String::from_utf8_lossy(&bytes).lines() {
                any |= builder.add_line(None, line).is_ok();
            }
        }
        for pattern in deny_paths {
            any |= builder.add_line(None, pattern).is_ok();
        }
        let rules = if any { builder.build().ok() } else { None };
        Self {
            vault: Vault::new(root.clone()),
            rules,
            silence,
        }
    }

    /// Whether a project path (as the app stores it: `/` separators) is kept from agents.
    #[must_use]
    pub fn hidden(&self, path: &str, is_dir: bool) -> bool {
        if path.starts_with(WORKTREE_PREFIX) {
            return true;
        }
        let first = path.split('/').next().unwrap_or_default();
        if PRIVATE_FOLDERS.iter().any(|p| fold(first) == fold(p)) {
            return true;
        }
        if self.rules.as_ref().is_some_and(|rules| {
            rules
                .matched_path_or_any_parents(Path::new(path), is_dir)
                .is_ignore()
        }) {
            return true;
        }
        self.silence
            .as_ref()
            .is_some_and(|silence| silence.0.is_silenced(path))
    }

    /// An existing, shared file of the project from an agent's word: `(project path, absolute path)`. Anything else
    /// (outside, through a link, hidden, a folder, missing) is `not found`.
    ///
    /// # Errors
    /// `NotFound` with the path as the agent gave it.
    pub fn file(&self, raw: &str) -> Result<(String, PathBuf), ToolError> {
        let missing = || ToolError::NotFound(raw.trim().to_owned());
        let path = VaultPath::parse(raw.trim()).map_err(|_| missing())?;
        if path.is_root() || self.hidden(path.as_str(), false) {
            return Err(missing());
        }
        let abs = self.vault.resolve(&path).map_err(|_| missing())?;
        if !abs.is_file() {
            return Err(missing());
        }
        // `abs` is the real place (links followed): a link inside the project may lead to a hidden part of it
        let shown = self.vault.relativize(&abs).ok_or_else(missing)?;
        if self.hidden(shown.as_str(), false) {
            return Err(missing());
        }
        Ok((path.as_str().to_owned(), abs))
    }

    /// The guard as a search filter: hidden paths do not answer.
    #[must_use]
    pub fn as_lookup(self: &Arc<Self>) -> Arc<dyn SilenceLookup> {
        Arc::new(Lookup(Arc::clone(self)))
    }
}

struct Lookup(Arc<Guard>);

impl SilenceLookup for Lookup {
    fn is_silenced(&self, path: &str) -> bool {
        self.0.hidden(path, false)
    }
}
