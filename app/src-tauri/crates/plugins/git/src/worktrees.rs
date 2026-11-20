//! Linked worktrees of a project's repository, found by reading files (no git, no backend): they become extra roots of
//! the project before it is built. `<common>/worktrees/<name>/gitdir` names each linked working tree, which names
//! that admin folder back in its `.git` file.

use std::path::{Component, Path, PathBuf};

use permark_core::fsx::{is_local_path, read_regular, Zone};
use permark_core::paths::canonicalize;

use crate::ops::too_broad;

/// Most worktrees offered.
const MAX_WORKTREES: usize = 32;
/// Most admin folders (`<common>/worktrees/*`) looked at: each costs a few reads, and the folder may hold any number.
const MAX_SCANNED: usize = 256;
/// Most folders walked up looking for `.git`.
const MAX_LEVELS: usize = 40;
/// Most bytes of a one-line file of a git folder (`.git` file, `gitdir`, `commondir`, `HEAD`, a loose ref): the
/// folder came with the project, so a bigger one is unreadable rather than taken in whole.
const MAX_LINE_FILE: u64 = 4 * 1024;

/// A working tree of the same repository as the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    pub name: String,
    /// The folder that corresponds to the project folder in that working tree.
    pub folder: PathBuf,
}

/// The first line of a small file of a git folder; `None`: missing, empty, too large, not UTF-8, or not a plain file
/// (the read of a FIFO would wait for a writer forever).
pub(crate) fn read_line(path: &Path) -> Option<String> {
    let bytes = read_regular(path, MAX_LINE_FILE, Zone::Anywhere).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

/// The path a file of a git folder names (`gitdir`, `commondir`), read against `base` (the folder that holds the
/// file) when relative. `None` when a lookup of it could leave this computer: an absolute path not on a local drive
/// ([`is_local_path`]: a share, a mapped network drive) | a path with a drive of its own (`Z:x`). A plain relative
/// path stays on the drive | share of `base`, the place the person opened.
pub(crate) fn named_path(base: &Path, named: &str) -> Option<PathBuf> {
    let named = Path::new(named);
    if named.is_absolute() {
        return is_local_path(named).then(|| named.to_path_buf());
    }
    if matches!(named.components().next(), Some(Component::Prefix(_))) {
        return None;
    }
    Some(base.join(named))
}

/// What `<dir>/.git` is.
enum Dot {
    /// A folder: the git folder of a main working tree.
    Folder(PathBuf),
    /// A file `gitdir: <path>`: a linked working tree | a submodule; `None`: a path that may not be looked up
    /// ([`named_path`]).
    File(Option<PathBuf>),
}

/// What `<dir>/.git` is; `None`: no `.git`, a link, or a file without a `gitdir:` line.
fn git_entry(dir: &Path) -> Option<Dot> {
    let entry = dir.join(".git");
    let meta = std::fs::symlink_metadata(&entry).ok()?;
    if meta.is_dir() {
        return Some(Dot::Folder(entry));
    }
    if meta.is_file() {
        let line = read_line(&entry)?;
        return Some(Dot::File(named_path(
            dir,
            line.strip_prefix("gitdir:")?.trim(),
        )));
    }
    None
}

/// Whether opening the git folder `git_dir` keeps git (the program, libgit2) on this computer: its `commondir`, read
/// on every open, names no path that could leave it ([`named_path`]).
pub(crate) fn commondir_stays_local(git_dir: &Path) -> bool {
    read_line(&git_dir.join("commondir")).is_none_or(|named| named_path(git_dir, &named).is_some())
}

/// Whether git may open the repository of the folder `root` (the first `.git` walking up) without leaving this
/// computer: a `.git` file | a `commondir` naming a share would make it connect to the server (and offer it the
/// person's sign-in) before anything is known about the repository.
#[must_use]
pub(crate) fn opens_locally(root: &Path) -> bool {
    let root = canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    match root.ancestors().take(MAX_LEVELS).find_map(git_entry) {
        None => true,
        Some(Dot::Folder(git_dir) | Dot::File(Some(git_dir))) => commondir_stays_local(&git_dir),
        Some(Dot::File(None)) => false,
    }
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

/// Where the objects and refs live: the git folder itself, or the one its `commondir` names when the git folder is a
/// linked tree's admin folder there (`<common>/worktrees/<name>`, git's own layout). A `commondir` anywhere else came
/// with the folder and names nothing.
pub(crate) fn common_dir(git_dir: &Path) -> PathBuf {
    let own = || canonicalize(git_dir).unwrap_or_else(|_| git_dir.to_path_buf());
    let Some(named) =
        read_line(&git_dir.join("commondir")).and_then(|line| named_path(git_dir, &line))
    else {
        return own();
    };
    match (git_dir.parent(), canonicalize(&named)) {
        (Some(admins), Ok(common)) if same_folder(admins, &common.join("worktrees")) => common,
        _ => own(),
    }
}

/// The working tree the admin folder `admin` (`<common>/worktrees/<name>`) stands for, when that tree names it back.
/// `admin/gitdir` holds `<tree>/.git` (relative: to `admin`, as `git worktree add --relative-paths` writes it), and
/// that `.git` is a file whose `gitdir:` leads to the same admin folder (git's own rule). Both files are content of
/// the folder: a planted `gitdir` | a tree that belongs to another admin folder -> `None`. So is a tree git could not
/// open without leaving this computer (the admin folder's `commondir`).
fn named_back(admin: &Path) -> Option<PathBuf> {
    if !commondir_stays_local(admin) {
        return None;
    }
    let dot_git = named_path(admin, &read_line(&admin.join("gitdir"))?)?;
    let tree = dot_git.parent()?.to_path_buf();
    let Some(Dot::File(Some(back))) = git_entry(&tree) else {
        return None;
    };
    same_folder(&back, admin).then_some(tree)
}

/// `<common>/worktrees` where it really is (canonical), when it is a real folder there: a link in its place would lend
/// another repository's admin folders to this one.
fn admin_folders(common: &Path) -> Option<PathBuf> {
    let admins = common.join("worktrees");
    // the metadata of a link itself (symlink | junction) is never a folder's
    if !std::fs::symlink_metadata(&admins).is_ok_and(|meta| meta.is_dir()) {
        return None;
    }
    canonicalize(&admins).ok()
}

/// Whether `admin` is an admin folder of `admins` (a canonical `<common>/worktrees`) itself: a real folder, not a
/// link, whose real place is `admins/<its name>`.
fn is_admin_in(admins: &Path, admin: &Path) -> bool {
    std::fs::symlink_metadata(admin).is_ok_and(|meta| meta.is_dir())
        && admin
            .file_name()
            .is_some_and(|name| canonicalize(admin).is_ok_and(|real| real == admins.join(name)))
}

/// Whether `a` and `b` are the same existing folder (links resolved, case as on disk).
fn same_folder(a: &Path, b: &Path) -> bool {
    match (canonicalize(a), canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The other working trees of the repository the project folder `root` belongs to.
#[must_use]
pub fn linked_worktrees(root: &Path, home: Option<&Path>) -> Vec<Linked> {
    let root = canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some((workdir, dot)) = root
        .ancestors()
        .take(MAX_LEVELS)
        .find_map(|dir| git_entry(dir).map(|dot| (dir.to_path_buf(), dot)))
    else {
        return Vec::new();
    };
    let (git_dir, linked) = match dot {
        Dot::Folder(git_dir) => (git_dir, false),
        Dot::File(Some(git_dir)) => (git_dir, true),
        // a `.git` file naming a share: a repository git cannot open here
        Dot::File(None) => return Vec::new(),
    };
    if too_broad(&workdir, &root, home) || !commondir_stays_local(&git_dir) {
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
    // a main tree's `.git` folder is the common folder (git writes no `commondir` there); a linked tree's admin
    // folder is a real folder in `<common>/worktrees`, else its `commondir` could name any repository above the
    // project
    let common = if linked {
        let common = common_dir(&git_dir);
        if !admin_folders(&common).is_some_and(|admins| is_admin_in(&admins, &git_dir)) {
            return Vec::new();
        }
        common
    } else {
        canonicalize(&git_dir).unwrap_or(git_dir)
    };
    let mut found: Vec<Linked> = Vec::new();
    let mut add = |name: String, tree: PathBuf| {
        let folder = permark_core::paths::simplified(&tree.join(&prefix)).into_owned();
        // the same rule as the project's own tree (no drive root, no home folder), and never a tree that is the
        // project or holds it: that would offer the project's surroundings as one more root
        let holds_root = |path: &Path| canonicalize(path).is_ok_and(|path| root.starts_with(path));
        if folder.is_dir()
            && !holds_root(&tree)
            && !holds_root(&folder)
            && !too_broad(&tree, &root, home)
            && !too_broad(&folder, &root, home)
            && found.len() < MAX_WORKTREES
        {
            found.push(Linked { name, folder });
        }
    };
    // the main working tree, when the project sits in a linked one: its `.git` is the common folder itself
    if linked && common.file_name().is_some_and(|n| n == ".git") {
        if let Some(main) = common.parent() {
            if matches!(git_entry(main), Some(Dot::Folder(git)) if same_folder(&git, &common)) {
                let name = main
                    .file_name()
                    .map_or_else(|| "main".to_owned(), |n| n.to_string_lossy().into_owned());
                add(name, main.to_path_buf());
            }
        }
    }
    let Some(admins) = admin_folders(&common) else {
        return found;
    };
    let Ok(entries) = std::fs::read_dir(&admins) else {
        return found;
    };
    let mut names: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .take(MAX_SCANNED)
        // the entry's own type: a link among them is never followed
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let admin = entry.path();
            if !is_admin_in(&admins, &admin) {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            Some((name, named_back(&admin)?))
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

    fn names(found: Vec<Linked>) -> Vec<String> {
        found.into_iter().map(|l| l.name).collect()
    }

    /// A linked project's `commondir` is content too: its admin folder must sit in `<common>/worktrees`, so a project
    /// that carries its own admin folder cannot name a parent repository (a dotfiles repository in the home folder)
    /// as its common folder and get that repository's main tree attached.
    #[test]
    fn a_commondir_beside_the_project_attaches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".git")).unwrap();
        make_worktree(&home.join(".git"), "w", &dir.path().join("w"));
        let project = home.join("evil");
        let admin = project.join("x").join("a");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(project.join(".git"), "gitdir: x/a\n").unwrap();
        std::fs::write(admin.join("gitdir"), "../../.git\n").unwrap();
        std::fs::write(admin.join("commondir"), "../../../.git\n").unwrap();
        assert!(linked_worktrees(&project, None).is_empty());
        assert!(linked_worktrees(&project, Some(&home)).is_empty());
    }

    /// git never writes a `commondir` into a main tree's `.git`: one there came with the folder and names nothing.
    #[test]
    fn a_main_tree_never_follows_a_commondir() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("other");
        std::fs::create_dir_all(other.join(".git")).unwrap();
        make_worktree(&other.join(".git"), "w", &dir.path().join("w"));
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".git")).unwrap();
        for named in [
            format!("{}\n", other.join(".git").display()),
            "../../other/.git\n".to_owned(),
        ] {
            std::fs::write(project.join(".git").join("commondir"), &named).unwrap();
            assert!(linked_worktrees(&project, None).is_empty(), "{named}");
        }
    }

    /// Every attached tree passes the same rule as the project's own: never the home folder, never a folder that
    /// holds the project.
    #[test]
    fn a_tree_that_is_the_home_folder_or_holds_the_project_is_not_attached() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".git")).unwrap();
        make_worktree(&home.join(".git"), "a", &dir.path().join("a"));
        make_worktree(&home.join(".git"), "b", &dir.path().join("b"));
        assert_eq!(
            names(linked_worktrees(&dir.path().join("a"), None)),
            ["home", "b"]
        );
        assert_eq!(
            names(linked_worktrees(&dir.path().join("a"), Some(&home))),
            ["b"]
        );
        // a linked tree inside the main one: the main tree holds the project
        let main = dir.path().join("main");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        let nested = main.join("trees").join("feat");
        make_worktree(&main.join(".git"), "feat", &nested);
        make_worktree(&main.join(".git"), "side", &dir.path().join("side"));
        assert_eq!(names(linked_worktrees(&nested, None)), ["side"]);
    }

    /// Files of a git folder come with the folder: one too large for a line is unreadable, not taken in whole.
    #[test]
    fn an_oversized_git_file_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        make_worktree(&main.join(".git"), "w", &dir.path().join("w"));
        assert_eq!(names(linked_worktrees(&main, None)), ["w"]);
        let gitdir = main.join(".git").join("worktrees").join("w").join("gitdir");
        let mut text = std::fs::read_to_string(&gitdir).unwrap();
        text.push_str(&" ".repeat(8 * 1024));
        std::fs::write(&gitdir, text).unwrap();
        assert!(linked_worktrees(&main, None).is_empty());
        let head = main.join(".git").join("HEAD");
        std::fs::write(
            &head,
            format!("ref: refs/heads/main\n{}", " ".repeat(8 * 1024)),
        )
        .unwrap();
        assert_eq!(branch_of(&main.join(".git")), None);
    }

    /// A FIFO where git keeps a one-line file (a sync partner | an archive can place one) is refused at once: a read
    /// would wait for a writer forever while the project opens.
    #[cfg(unix)]
    #[test]
    fn a_fifo_in_a_git_folder_never_holds_the_read() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        let git = main.join(".git");
        std::fs::create_dir_all(&git).unwrap();
        make_worktree(&git, "a", &dir.path().join("a"));
        make_worktree(&git, "b", &dir.path().join("b"));
        let other = dir.path().join("other.git");
        std::fs::create_dir_all(other.join("refs").join("heads")).unwrap();
        std::fs::write(other.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        for fifo in [
            git.join("worktrees").join("b").join("gitdir"),
            git.join("worktrees").join("a").join("commondir"),
            git.join("HEAD"),
            other.join("refs").join("heads").join("main"),
            other.join("packed-refs"),
        ] {
            let _ = std::fs::remove_file(&fifo);
            let made = std::process::Command::new("mkfifo").arg(&fifo).status();
            assert!(made.is_ok_and(|s| s.success()), "{}", fifo.display());
        }
        let tree_a = dir.path().join("a");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((
                names(linked_worktrees(&main, None)),
                names(linked_worktrees(&tree_a, None)),
                branch_of(&git),
                crate::cheap::cheap_facts(&git),
                crate::cheap::cheap_facts(&other).map(|c| (c.head.unborn, c.head.oid)),
            ));
        });
        let (from_main, from_a, branch, head, unborn) = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a read waited on a FIFO");
        assert_eq!(from_main, ["a"]);
        assert!(from_a.is_empty(), "{from_a:?}");
        assert_eq!(branch, None);
        assert_eq!(head, None);
        assert_eq!(unborn, Some((true, None)));
    }

    /// A folder link at `link` leading to `target` (unix: symlink; Windows: junction, which needs no privilege);
    /// `false`: links cannot be made here.
    fn link_folder(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .is_ok_and(|o| o.status.success())
        }
    }

    /// `<common>/worktrees` and each admin folder in it must be real folders of this repository: a link there (an
    /// archive | a sync partner can place one) would lend another repository's working trees to the project.
    #[test]
    fn admin_folders_reached_through_a_link_attach_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("other");
        std::fs::create_dir_all(other.join(".git")).unwrap();
        make_worktree(&other.join(".git"), "w", &dir.path().join("w"));
        let other_admins = other.join(".git").join("worktrees");
        // the whole `worktrees` folder is a link
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".git")).unwrap();
        if !link_folder(&other_admins, &project.join(".git").join("worktrees")) {
            eprintln!("links could not be made here; skipped");
            return;
        }
        assert!(names(linked_worktrees(&project, None)).is_empty());
        // one admin folder is a link: the real one beside it is still found
        let second = dir.path().join("second");
        std::fs::create_dir_all(second.join(".git")).unwrap();
        make_worktree(&second.join(".git"), "real", &dir.path().join("real"));
        assert!(link_folder(
            &other_admins.join("w"),
            &second.join(".git").join("worktrees").join("w")
        ));
        assert_eq!(names(linked_worktrees(&second, None)), ["real"]);
        // a linked project whose own admin folder is a link to a folder outside the common one
        let third = dir.path().join("third");
        std::fs::create_dir_all(third.join(".git")).unwrap();
        make_worktree(&third.join(".git"), "s", &dir.path().join("s"));
        let stash = dir.path().join("stash").join("p");
        std::fs::create_dir_all(&stash).unwrap();
        let tree = dir.path().join("p");
        std::fs::create_dir_all(&tree).unwrap();
        let admin = third.join(".git").join("worktrees").join("p");
        assert!(link_folder(&stash, &admin));
        std::fs::write(
            stash.join("gitdir"),
            format!("{}\n", tree.join(".git").display()),
        )
        .unwrap();
        std::fs::write(
            stash.join("commondir"),
            format!("{}\n", third.join(".git").display()),
        )
        .unwrap();
        std::fs::write(tree.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
        assert!(names(linked_worktrees(&tree, None)).is_empty());
        // the real layouts still list their trees
        assert_eq!(names(linked_worktrees(&other, None)), ["w"]);
        assert_eq!(
            names(linked_worktrees(&dir.path().join("s"), None)),
            ["third"]
        );
    }

    /// A share no host answers (TEST-NET-1, never routed): a lookup there would wait for the network.
    fn on_share(rest: &str) -> PathBuf {
        let share = if cfg!(windows) {
            r"\\192.0.2.1\s"
        } else {
            "//192.0.2.1/s"
        };
        Path::new(share).join(rest)
    }

    /// A path a git file names that is not on a local drive (a share) is never looked up: even a lookup connects to
    /// the server and offers it the person's sign-in. The tree it would lead to is not attached, at once.
    #[test]
    fn a_share_named_by_a_git_file_is_never_looked_up() {
        let dir = tempfile::tempdir().unwrap();
        // the project's own `.git` file
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join(".git"),
            format!("gitdir: {}\n", on_share("a.git").display()),
        )
        .unwrap();
        // an admin folder's `gitdir`, an admin folder's `commondir`
        let main = dir.path().join("main");
        let git = main.join(".git");
        std::fs::create_dir_all(&git).unwrap();
        make_worktree(&git, "w", &dir.path().join("w"));
        let far = git.join("worktrees").join("far");
        std::fs::create_dir_all(&far).unwrap();
        std::fs::write(
            far.join("gitdir"),
            format!("{}\n", on_share("far/.git").display()),
        )
        .unwrap();
        let tree_c = dir.path().join("c");
        make_worktree(&git, "c", &tree_c);
        let admin_c = git.join("worktrees").join("c");
        std::fs::write(
            admin_c.join("commondir"),
            format!("{}\n", on_share("common").display()),
        )
        .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((
                names(linked_worktrees(&project, None)),
                names(linked_worktrees(&main, None)),
                names(linked_worktrees(&tree_c, None)),
                crate::cheap::cheap_facts(&admin_c).map(|c| c.head.unborn),
                crate::cheap::member_location(&project).is_err(),
                [&project, &tree_c, &main].map(|folder| opens_locally(folder)),
            ));
        });
        let (from_project, from_main, from_c, unborn, refused, opens) = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("a path on a share was looked up");
        assert!(from_project.is_empty(), "{from_project:?}");
        assert_eq!(from_main, ["w"]);
        assert!(from_c.is_empty(), "{from_c:?}");
        assert_eq!(unborn, Some(true));
        assert!(refused);
        assert_eq!(opens, [false, false, true]);
    }

    /// A relative path a git file names stays where that file is (also when the project itself sits on a share the
    /// person opened); an absolute one must be on a local drive; one with a drive of its own is never read against
    /// the folder.
    #[test]
    fn a_named_path_is_read_against_its_folder_or_must_be_local() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        assert_eq!(named_path(base, "../x/.git"), Some(base.join("../x/.git")));
        let share = on_share("p");
        assert_eq!(
            named_path(&share, "worktrees/a"),
            Some(share.join("worktrees/a"))
        );
        let local = base.join("y");
        assert_eq!(named_path(base, &local.display().to_string()), Some(local));
        assert_eq!(named_path(base, &on_share("x").display().to_string()), None);
        if cfg!(windows) {
            assert_eq!(named_path(base, "Z:x"), None);
            assert_eq!(named_path(base, r"\\?\UNC\192.0.2.1\s\x"), None);
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
