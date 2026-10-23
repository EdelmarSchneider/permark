//! Linked worktrees of a project's repository, found by reading files (no git, no backend): they become extra roots of
//! the project before it is built. `<common>/worktrees/<name>/gitdir` names each linked working tree.

use std::path::{Path, PathBuf};

use crate::ops::too_broad;

/// Most worktrees offered.
const MAX_WORKTREES: usize = 32;
/// Most folders walked up looking for `.git`.
const MAX_LEVELS: usize = 40;

/// A working tree of the same repository as the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    pub name: String,
    /// The folder that corresponds to the project folder in that working tree.
    pub folder: PathBuf,
}

pub(crate) fn read_line(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

/// `<dir>/.git` as `(git folder, linked)`: a folder is a main working tree; a file `gitdir: <path>` a linked one.
fn git_entry(dir: &Path) -> Option<(PathBuf, bool)> {
    let entry = dir.join(".git");
    let meta = std::fs::symlink_metadata(&entry).ok()?;
    if meta.is_dir() {
        return Some((entry, false));
    }
    if meta.is_file() {
        let line = read_line(&entry)?;
        let target = PathBuf::from(line.strip_prefix("gitdir:")?.trim());
        let git_dir = if target.is_absolute() {
            target
        } else {
            dir.join(target)
        };
        return Some((git_dir, true));
    }
    None
}

/// Where the objects and refs live: the git folder itself, or the one named by `commondir` (linked worktree).
pub(crate) fn common_dir(git_dir: &Path) -> PathBuf {
    let found = match read_line(&git_dir.join("commondir")) {
        Some(relative) => {
            let relative = PathBuf::from(relative);
            if relative.is_absolute() {
                relative
            } else {
                git_dir.join(relative)
            }
        }
        None => git_dir.to_path_buf(),
    };
    permark_core::paths::canonicalize(&found).unwrap_or(found)
}

/// The other working trees of the repository the project folder `root` belongs to.
#[must_use]
pub fn linked_worktrees(root: &Path, home: Option<&Path>) -> Vec<Linked> {
    let root = permark_core::paths::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some((workdir, git_dir, linked)) = root
        .ancestors()
        .take(MAX_LEVELS)
        .find_map(|dir| git_entry(dir).map(|(git, linked)| (dir.to_path_buf(), git, linked)))
    else {
        return Vec::new();
    };
    if too_broad(&workdir, &root, home) {
        return Vec::new();
    }
    let prefix = root
        .strip_prefix(&workdir)
        .unwrap_or(Path::new(""))
        .to_path_buf();
    let common = common_dir(&git_dir);
    let mut found: Vec<Linked> = Vec::new();
    let mut add = |name: String, tree: PathBuf| {
        let folder = permark_core::paths::simplified(&tree.join(&prefix)).into_owned();
        if folder.is_dir()
            && permark_core::paths::canonicalize(&folder).ok() != Some(root.clone())
            && found.len() < MAX_WORKTREES
        {
            found.push(Linked { name, folder });
        }
    };
    // the main working tree, when the project sits in a linked one
    if linked && common.file_name().is_some_and(|n| n == ".git") {
        if let Some(main) = common.parent() {
            let name = main
                .file_name()
                .map_or_else(|| "main".to_owned(), |n| n.to_string_lossy().into_owned());
            add(name, main.to_path_buf());
        }
    }
    let Ok(entries) = std::fs::read_dir(common.join("worktrees")) else {
        return found;
    };
    let mut names: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let line = read_line(&entry.path().join("gitdir"))?;
            // the file names the `.git` file inside the working tree
            let tree = PathBuf::from(line).parent()?.to_path_buf();
            Some((name, tree))
        })
        .collect();
    names.sort();
    for (name, tree) in names {
        add(name, tree);
    }
    found
}

/// The branch a git folder's HEAD names (`None`: detached or unreadable).
#[must_use]
pub fn branch_of(git_dir: &Path) -> Option<String> {
    let line = read_line(&git_dir.join("HEAD"))?;
    let target = line.strip_prefix("ref:")?.trim();
    Some(
        target
            .strip_prefix("refs/heads/")
            .unwrap_or(target)
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_worktree(common: &Path, name: &str, tree: &Path) {
        let admin = common.join("worktrees").join(name);
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::create_dir_all(tree).unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", tree.join(".git").display()),
        )
        .unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(admin.join("HEAD"), format!("ref: refs/heads/{name}\n")).unwrap();
        std::fs::write(tree.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
    }

    #[test]
    fn no_repository_no_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        assert!(linked_worktrees(dir.path(), None).is_empty());
    }

    #[test]
    fn the_main_tree_lists_its_linked_ones_and_skips_missing_folders() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main repo");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        make_worktree(&main.join(".git"), "feat-ü", &dir.path().join("wt one"));
        make_worktree(&main.join(".git"), "gone", &dir.path().join("wt gone"));
        std::fs::remove_dir_all(dir.path().join("wt gone")).unwrap();
        let found = linked_worktrees(&main, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "feat-ü");
        assert!(found[0].folder.ends_with("wt one"));
    }

    #[test]
    fn a_project_in_a_subfolder_gets_the_same_subfolder_of_each_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        std::fs::create_dir_all(main.join("docs")).unwrap();
        make_worktree(&main.join(".git"), "w", &dir.path().join("w"));
        // the worktree has no docs folder yet: nothing to offer
        assert!(linked_worktrees(&main.join("docs"), None).is_empty());
        std::fs::create_dir_all(dir.path().join("w").join("docs")).unwrap();
        let found = linked_worktrees(&main.join("docs"), None);
        assert_eq!(found.len(), 1);
        assert!(found[0].folder.ends_with("docs"));
        assert!(found[0].folder.starts_with(dir.path().join("w")));
    }

    #[test]
    fn a_project_that_is_a_linked_tree_lists_the_main_one_and_its_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        let wt_a = dir.path().join("a");
        let wt_b = dir.path().join("b");
        make_worktree(&main.join(".git"), "a", &wt_a);
        make_worktree(&main.join(".git"), "b", &wt_b);
        let found = linked_worktrees(&wt_a, None);
        let names: Vec<&str> = found.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["main", "b"], "{found:?}");
    }

    #[test]
    fn a_broad_repository_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".git")).unwrap();
        make_worktree(&home.join(".git"), "w", &dir.path().join("w"));
        std::fs::create_dir_all(home.join("proj")).unwrap();
        assert!(linked_worktrees(&home.join("proj"), Some(&home)).is_empty());
    }

    #[test]
    fn garbage_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("m");
        std::fs::create_dir_all(main.join(".git").join("worktrees").join("x")).unwrap();
        std::fs::write(
            main.join(".git").join("worktrees").join("x").join("gitdir"),
            "\n\n",
        )
        .unwrap();
        std::fs::create_dir_all(main.join(".git").join("worktrees").join("y")).unwrap();
        assert!(linked_worktrees(&main, None).is_empty());
        std::fs::write(dir.path().join("bad.git"), "not a gitdir line").unwrap();
    }

    #[test]
    fn the_branch_of_a_worktree_comes_from_its_head_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(branch_of(dir.path()), None);
        std::fs::write(dir.path().join("HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        assert_eq!(branch_of(dir.path()).as_deref(), Some("feature/x"));
        std::fs::write(
            dir.path().join("HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .unwrap();
        assert_eq!(branch_of(dir.path()), None);
    }
}
