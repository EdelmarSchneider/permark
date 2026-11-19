//! Linked worktrees of a project's repository, found by reading files (no git, no backend): they become extra roots of
//! the project before it is built. `<common>/worktrees/<name>/gitdir` names each linked working tree, which names
//! that admin folder back in its `.git` file.

use std::path::{Component, Path, PathBuf};

use crate::ops::too_broad;

/// Most worktrees offered.
const MAX_WORKTREES: usize = 32;
/// Most admin folders (`<common>/worktrees/*`) looked at: each costs a few reads, and the folder may hold any number.
const MAX_SCANNED: usize = 256;
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

/// Whether the file `path` (relative to the working tree `workdir`, `/`-separated) is inside another repository
/// below it: a folder strictly between the two has its own `.git` (a nested repository | a submodule). Read from the
/// folders as they are now; a path that is not made of plain names looks at nothing.
pub(crate) fn in_nested_repository(workdir: &Path, path: &str) -> bool {
    let mut names: Vec<&str> = path.split('/').collect();
    // the last name is the file
    names.pop();
    let mut folder = workdir.to_path_buf();
    for name in names {
        let mut parts = Path::new(name).components();
        if !matches!(
            (parts.next(), parts.next()),
            (Some(Component::Normal(_)), None)
        ) {
            return false;
        }
        folder.push(name);
        if git_entry(&folder).is_some() {
            return true;
        }
    }
    false
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

/// The working tree the admin folder `admin` (`<common>/worktrees/<name>`) stands for, when that tree names it back.
/// `admin/gitdir` holds `<tree>/.git` (relative: to `admin`, as `git worktree add --relative-paths` writes it), and
/// that `.git` is a file whose `gitdir:` leads to the same admin folder (git's own rule). Both files are content of
/// the folder: a planted `gitdir` | a tree that belongs to another admin folder -> `None`.
fn named_back(admin: &Path) -> Option<PathBuf> {
    let line = PathBuf::from(read_line(&admin.join("gitdir"))?);
    let dot_git = if line.is_absolute() {
        line
    } else {
        admin.join(line)
    };
    let tree = dot_git.parent()?.to_path_buf();
    let (back, true) = git_entry(&tree)? else {
        return None;
    };
    same_folder(&back, admin).then_some(tree)
}

/// Whether `a` and `b` are the same existing folder (links resolved, case as on disk).
fn same_folder(a: &Path, b: &Path) -> bool {
    match (
        permark_core::paths::canonicalize(a),
        permark_core::paths::canonicalize(b),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
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
    // the project's own `.git` file is content too: one its admin folder does not name back leads nowhere
    if linked && !named_back(&git_dir).is_some_and(|tree| same_folder(&tree, &workdir)) {
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
    // the main working tree, when the project sits in a linked one: its `.git` is the common folder itself
    if linked && common.file_name().is_some_and(|n| n == ".git") {
        if let Some(main) = common.parent() {
            if git_entry(main).is_some_and(|(git, linked)| !linked && same_folder(&git, &common)) {
                let name = main
                    .file_name()
                    .map_or_else(|| "main".to_owned(), |n| n.to_string_lossy().into_owned());
                add(name, main.to_path_buf());
            }
        }
    }
    let Ok(entries) = std::fs::read_dir(common.join("worktrees")) else {
        return found;
    };
    let mut names: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .take(MAX_SCANNED)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            Some((name, named_back(&entry.path())?))
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

    /// As `git worktree add --relative-paths` writes them: the admin folder's `gitdir` relative to the admin folder,
    /// the tree's `.git` relative to the tree.
    #[test]
    fn relative_links_are_read_against_the_folder_that_holds_them() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        let admin = main.join(".git").join("worktrees").join("rel");
        std::fs::create_dir_all(&admin).unwrap();
        let tree = dir.path().join("wt rel");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(admin.join("gitdir"), "../../../../wt rel/.git\n").unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(tree.join(".git"), "gitdir: ../repo/.git/worktrees/rel\n").unwrap();
        let found = linked_worktrees(&main, None);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].name, "rel");
        assert_eq!(
            permark_core::paths::canonicalize(&found[0].folder).unwrap(),
            permark_core::paths::canonicalize(&tree).unwrap()
        );
        // and from inside the linked tree: the main one
        let names: Vec<String> = linked_worktrees(&tree, None)
            .into_iter()
            .map(|l| l.name)
            .collect();
        assert_eq!(names, ["repo"]);
    }

    /// A `gitdir` file is content of the folder: it attaches a tree only when that tree names the same admin folder
    /// back (git's own rule).
    #[test]
    fn a_tree_that_does_not_name_its_admin_folder_back_is_not_attached() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        make_worktree(&main.join(".git"), "real", &dir.path().join("real"));
        let plant = |name: &str, target: &Path| {
            let admin = main.join(".git").join("worktrees").join(name);
            std::fs::create_dir_all(&admin).unwrap();
            std::fs::write(
                admin.join("gitdir"),
                format!("{}\n", target.join(".git").display()),
            )
            .unwrap();
        };
        // a folder outside with no `.git`, one with its own repository, one that belongs to another admin folder
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        plant("planted", &outside);
        let other_repo = dir.path().join("other repo");
        std::fs::create_dir_all(other_repo.join(".git")).unwrap();
        plant("main tree", &other_repo);
        plant("borrowed", &dir.path().join("real"));
        let names: Vec<String> = linked_worktrees(&main, None)
            .into_iter()
            .map(|l| l.name)
            .collect();
        assert_eq!(names, ["real"]);
    }

    /// The project's own `.git` file is content too: one that names another repository's git folder (no admin folder
    /// naming the project back) attaches nothing of that repository.
    #[test]
    fn a_project_whose_git_file_is_not_named_back_gets_no_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(elsewhere.join(".git")).unwrap();
        make_worktree(&elsewhere.join(".git"), "w", &dir.path().join("w"));
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        for target in [
            elsewhere.join(".git"),
            elsewhere.join(".git").join("worktrees").join("w"),
        ] {
            std::fs::write(
                project.join(".git"),
                format!("gitdir: {}\n", target.display()),
            )
            .unwrap();
            assert!(
                linked_worktrees(&project, None).is_empty(),
                "{}",
                target.display()
            );
        }
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

    #[test]
    fn a_file_below_a_folder_with_its_own_git_is_in_another_repository() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main ü");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        std::fs::create_dir_all(main.join("docs").join("deep")).unwrap();
        // a nested repository, a submodule (its `.git` is a file), a folder with a `.git` that names nothing
        let nested = main.join("docs").join("nested");
        std::fs::create_dir_all(nested.join(".git")).unwrap();
        std::fs::create_dir_all(main.join("module")).unwrap();
        std::fs::write(
            main.join("module").join(".git"),
            "gitdir: ../.git/modules/module\n",
        )
        .unwrap();
        std::fs::create_dir_all(main.join("plain")).unwrap();
        std::fs::write(main.join("plain").join(".git"), "a note\n").unwrap();

        assert!(in_nested_repository(&main, "docs/nested/a.md"));
        assert!(
            in_nested_repository(&main, "docs/nested/gone/deeper/a.md"),
            "folders that are not there any more"
        );
        assert!(in_nested_repository(&main, "module/a.md"));
        // the repository's own files
        assert!(!in_nested_repository(&main, "a.md"));
        assert!(!in_nested_repository(&main, "docs/deep/a.md"));
        assert!(!in_nested_repository(&main, "plain/a.md"));
        assert!(!in_nested_repository(&main, "missing/a.md"));
        // only folders between the two count: not the working tree itself, not the path's last name
        assert!(!in_nested_repository(&main, "docs/nested"));
        assert!(in_nested_repository(&main.join("docs"), "nested/a.md"));
        assert!(!in_nested_repository(&nested, "a.md"));
        // a path that is not made of plain names looks at nothing
        let absolute = format!("{}/a.md", nested.display()).replace('\\', "/");
        for odd in [
            "../main ü/docs/nested/a.md",
            "docs/../docs/nested/a.md",
            "./docs/nested/a.md",
            "/docs/nested/a.md",
            "docs//nested/a.md",
            "docs\\nested/a.md",
            absolute.as_str(),
            "",
        ] {
            assert!(!in_nested_repository(&main, odd), "{odd:?}");
        }
    }
}
