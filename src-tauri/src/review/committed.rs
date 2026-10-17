//! The committed version of a document: where the review looks for the text of a version it has only the hash of
//! (the first run after an update, which knows what was read but kept no copy).
//!
//! Local, read-only use of the repository, like the rest of [`crate::git`].

use std::path::Path;
use std::sync::Mutex;

use git2::Repository;

use super::store::MAX_COPY_BYTES;
use crate::git::GitContext;
use crate::vault::WORKTREE_PREFIX;

/// Where committed texts come from.
pub trait Committed: Send + Sync {
    /// The document at `path` (a vault path) as the last commit has it.
    fn blob(&self, path: &str) -> Option<Vec<u8>>;
}

/// The last commit of the repository around a project.
pub struct GitBlobs {
    context: GitContext,
    /// Opened when first needed, and kept: a repository is opened once for all the documents of a run.
    repo: Mutex<Option<Repository>>,
}

impl GitBlobs {
    pub fn new(context: GitContext) -> Self {
        Self {
            context,
            repo: Mutex::new(None),
        }
    }

    /// The path of a vault path in the repository.
    fn repo_path(&self, path: &str) -> String {
        if self.context.vault_prefix.is_empty() {
            path.to_owned()
        } else {
            format!("{}/{path}", self.context.vault_prefix)
        }
    }
}

impl Committed for GitBlobs {
    fn blob(&self, path: &str) -> Option<Vec<u8>> {
        // a linked worktree has a HEAD of its own, which this repository handle does not follow
        if path.starts_with(WORKTREE_PREFIX) {
            return None;
        }
        let mut guard = self.repo.lock().ok()?;
        if guard.is_none() {
            *guard = Some(self.context.open().ok()?);
        }
        let repo = guard.as_ref()?;
        let tree = repo.head().ok()?.peel_to_tree().ok()?;
        let entry = tree.get_path(Path::new(&self.repo_path(path))).ok()?;
        let object = entry.to_object(repo).ok()?;
        let blob = object.as_blob()?;
        (blob.size() <= MAX_COPY_BYTES).then(|| blob.content().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::{commit_all, init_repo, write};
    use crate::git::{discover, Discovery};

    fn blobs(vault_root: &Path) -> GitBlobs {
        let root = dunce::canonicalize(vault_root).expect("canonical");
        let Discovery::Repo(context) = discover(&root, None) else {
            panic!("a repository");
        };
        GitBlobs::new(context)
    }

    #[test]
    fn the_committed_text_is_found_whatever_the_file_holds_now() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path());
        write(dir.path(), "docs/a.md", "# Committed\n");
        commit_all(&repo, "first");
        write(dir.path(), "docs/a.md", "# Edited since\n");
        let git = blobs(dir.path());
        assert_eq!(git.blob("docs/a.md"), Some(b"# Committed\n".to_vec()));
        assert_eq!(git.blob("docs/none.md"), None);
        assert_eq!(git.blob("docs"), None, "a folder is no document");
    }

    #[test]
    fn a_file_that_was_never_committed_has_no_committed_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path());
        write(dir.path(), "a.md", "a\n");
        commit_all(&repo, "first");
        write(dir.path(), "new.md", "new\n");
        assert_eq!(blobs(dir.path()).blob("new.md"), None);
    }

    #[test]
    fn a_repository_without_commits_has_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        init_repo(dir.path());
        write(dir.path(), "a.md", "a\n");
        assert_eq!(blobs(dir.path()).blob("a.md"), None);
    }

    #[test]
    fn a_project_inside_a_repository_is_found_under_its_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path());
        write(dir.path(), "project/notes/a.md", "# In the project\n");
        write(dir.path(), "other.md", "elsewhere\n");
        commit_all(&repo, "first");
        let git = blobs(&dir.path().join("project"));
        assert_eq!(git.blob("notes/a.md"), Some(b"# In the project\n".to_vec()));
        assert_eq!(git.blob("other.md"), None);
    }

    #[test]
    fn a_linked_worktree_is_not_looked_up_in_this_repository() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path());
        write(dir.path(), "a.md", "a\n");
        commit_all(&repo, "first");
        assert_eq!(blobs(dir.path()).blob("@wt/feature/a.md"), None);
    }

    #[test]
    fn a_text_over_the_size_a_copy_may_have_is_left_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path());
        write(dir.path(), "big.md", &"x".repeat(MAX_COPY_BYTES + 1));
        write(dir.path(), "ok.md", &"x".repeat(MAX_COPY_BYTES));
        commit_all(&repo, "first");
        let git = blobs(dir.path());
        assert_eq!(git.blob("big.md"), None);
        assert!(git.blob("ok.md").is_some());
    }
}
