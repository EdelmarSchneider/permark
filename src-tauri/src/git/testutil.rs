//! Helpers for tests that need real repositories.

use std::path::Path;

use git2::{Oid, Repository, Signature};

/// `git init` with `main` as the initial branch.
pub(crate) fn init_repo(dir: &Path) -> Repository {
    let mut opts = git2::RepositoryInitOptions::new();
    opts.initial_head("main");
    Repository::init_opts(dir, &opts).expect("init")
}

pub(crate) fn write(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
    std::fs::write(p, content).expect("write");
}

/// `git add -A && git commit`.
pub(crate) fn commit_all(repo: &Repository, message: &str) -> Oid {
    let mut index = repo.index().expect("index");
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .expect("add");
    index.update_all(["*"], None).expect("update");
    index.write().expect("write index");
    let tree_id = index.write_tree().expect("write tree");
    let tree = repo.find_tree(tree_id).expect("tree");
    let sig = Signature::now("Test", "test@example.com").expect("sig");
    let parents: Vec<git2::Commit<'_>> = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter()
        .collect();
    let refs: Vec<&git2::Commit<'_>> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &refs)
        .expect("commit")
}

/// `git add <path>`.
pub(crate) fn stage(repo: &Repository, rel: &str) {
    let mut index = repo.index().expect("index");
    index.add_path(Path::new(rel)).expect("add");
    index.write().expect("write index");
}
