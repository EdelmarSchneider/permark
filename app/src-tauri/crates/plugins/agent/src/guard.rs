//! What an agent may see of the project: one predicate for every tool, every list and every snippet. Hidden paths are
//! answered exactly as missing ones. Silenced paths are not hidden: silence quiets the person's attention only.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use permark_core::organizing::SilenceLookup;
use permark_core::paths::{fold, Root, Vault, VaultPath, CASE_INSENSITIVE, WORKTREE_PREFIX};

use crate::ToolError;

/// The ignore file agents honor (project root, gitignore syntax).
pub const AI_IGNORE: &str = ".aiignore";
/// Largest `.aiignore` read (a bigger one is read up to this size).
const MAX_IGNORE_BYTES: u64 = 1024 * 1024;
/// Folders of the project's own machinery (at any depth: a nested repository has its own), never shared.
const PRIVATE_FOLDERS: [&str; 2] = [".permark", ".git"];
/// Files that hold keys whole (no `key = value` for masking to find), never shared: exact names (folded).
const KEY_FILES: [&str; 4] = [".netrc", "_netrc", ".pgpass", ".git-credentials"];
/// Extensions of key files (folded).
const KEY_EXTENSIONS: [&str; 5] = ["pem", "key", "p12", "pfx", "keystore"];
/// Name starts of SSH private keys (folded; `id_rsa.pub` is public and stays visible).
const SSH_KEYS: [&str; 4] = ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"];

/// A file name that holds a private key or credentials whole.
fn is_key_file(name: &str) -> bool {
    let name = name.to_lowercase();
    if KEY_FILES.contains(&name.as_str()) {
        return true;
    }
    let public = Path::new(&name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"));
    if SSH_KEYS.iter().any(|k| name.starts_with(k)) && !public {
        return true;
    }
    name.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && KEY_EXTENSIONS.contains(&ext))
}

/// The rules of one call (read fresh: `.aiignore` and `.permark/config.toml` can change while an agent runs).
pub struct Guard {
    /// The project folder only: linked worktrees live outside it.
    vault: Vault,
    rules: Option<Gitignore>,
}

impl Guard {
    #[must_use]
    pub fn new(root: &Root, deny_paths: &[String]) -> Self {
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
        }
    }

    /// Whether a project path (as the app stores it: `/` separators) is kept from agents.
    #[must_use]
    pub fn hidden(&self, path: &str, is_dir: bool) -> bool {
        if path.starts_with(WORKTREE_PREFIX) {
            return true;
        }
        if path
            .split('/')
            .any(|segment| PRIVATE_FOLDERS.iter().any(|p| fold(segment) == fold(p)))
        {
            return true;
        }
        if !is_dir && is_key_file(path.rsplit('/').next().unwrap_or_default()) {
            return true;
        }
        self.rules.as_ref().is_some_and(|rules| {
            rules
                .matched_path_or_any_parents(Path::new(path), is_dir)
                .is_ignore()
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machinery_folders_at_any_depth_and_key_files_are_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::open(dir.path()).unwrap();
        let guard = Guard::new(&root, &[]);
        for path in [
            ".git/config",
            "sub/.git/config",
            "sub/.GIT/FETCH_HEAD",
            "a/b/.permark/local/comments.json",
            "sub/.git",
            "deploy/id_ed25519",
            "id_rsa",
            "certs/server.key",
            "certs/chain.PEM",
            "store.p12",
            "home/.netrc",
            ".git-credentials",
        ] {
            assert!(guard.hidden(path, false), "{path}");
        }
        assert!(guard.hidden("sub/.git", true));
        for path in [
            "docs/plan.md",
            "deploy/id_ed25519.pub",
            "keys/readme.md",
            "a.gitignore",
            ".github/workflows/ci.yml",
            ".pem",
            "monkey.md",
        ] {
            assert!(!guard.hidden(path, false), "{path}");
        }
        assert!(
            !guard.hidden("certs.key", true),
            "a folder is not a key file"
        );
    }
}
