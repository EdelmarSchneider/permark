//! Linked worktrees of a project's repository, found by reading files (no git, no backend): they become extra roots of
//! the project before it is built. `<common>/worktrees/<name>/gitdir` names each linked working tree, which names
//! that admin folder back in its `.git` file.

use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use permark_core::fsx::{is_local_path, read_regular, Zone};
use permark_core::paths::canonicalize;

use crate::config_scan::Scan;
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

/// The bytes of a small file of a git folder, at most `limit`; `None`: missing, too large, or not a plain file itself.
/// A link there is never read through (its target may be on a share: even the open connects to the server), a FIFO
/// never waited on.
pub(crate) fn read_git_file(path: &Path, limit: u64) -> Option<Vec<u8>> {
    // the name's own metadata: a read anywhere follows a link at the last name
    if !std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file()) {
        return None;
    }
    read_regular(path, limit, Zone::Anywhere).ok()
}

/// The first line of a small file of a git folder; `None`: missing, empty, too large, not UTF-8, or not a plain file
/// itself ([`read_git_file`]).
pub(crate) fn read_line(path: &Path) -> Option<String> {
    let text = String::from_utf8(read_git_file(path, MAX_LINE_FILE)?).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

/// The path a file of a git folder names (`gitdir`, `commondir`, a `.git` link's target), read against `base` (the
/// folder that holds the file) when relative. `None` when a lookup of it could leave this computer: an absolute path
/// not on a local drive ([`is_local_path`]: a share, a mapped network drive) | a path with a drive of its own (`Z:x`)
/// | a relative path through a link below a local `base` | on Windows a root without a drive or a `?`. A plain
/// relative path stays on the drive | share of `base`, the place the person opened.
pub(crate) fn named_path(base: &Path, named: &str) -> Option<PathBuf> {
    // Win32 takes `\??\…` as a system path (a share through the network redirector) and git passes it on; std reads it
    // as a root without a drive, which git takes as the current drive. No file name holds a `?`.
    #[cfg(windows)]
    if named.contains('?')
        || matches!(
            Path::new(named).components().next(),
            Some(Component::RootDir)
        )
    {
        return None;
    }
    let named = Path::new(named);
    if named.is_absolute() {
        return is_local_path(named).then(|| named.to_path_buf());
    }
    if matches!(named.components().next(), Some(Component::Prefix(_))) {
        return None;
    }
    // name by name, never following one: a link on the way could lead anywhere, a share too. Below a share the person
    // opened its server resolves its links (Windows follows none to another computer by default), and each look is a
    // round trip there.
    if is_local_path(base) {
        let mut at = base.to_path_buf();
        for part in named.components() {
            at.push(part);
            match std::fs::symlink_metadata(&at) {
                Ok(meta) if meta.file_type().is_symlink() => return None,
                Ok(_) => {}
                // nothing below a missing name can be looked up
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) => return None,
            }
        }
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
    /// A file this side cannot read as one `gitdir:` line (too large, not text, another first line). Git cannot open
    /// it as a repository either (it reads the file whole and drops trailing line ends, so padding could hide a path it
    /// would follow), and the walk for a repository ends there as it does for git; for the folders around it, it is
    /// no repository of its own (git's own reading of a folder with a broken `.git` file).
    Unread,
    /// A link (symlink | junction) git would follow somewhere it may not go: a target that may not be looked up
    /// ([`named_path`]), a second link, nothing, or neither folder nor file. A link to a folder | file that may is that
    /// folder | file.
    Link,
}

/// What `<dir>/.git` is; `None`: no `.git`, or something there that is neither folder, file nor link.
fn git_entry(dir: &Path) -> Option<Dot> {
    let entry = dir.join(".git");
    let meta = std::fs::symlink_metadata(&entry).ok()?;
    if !meta.file_type().is_symlink() {
        return dot_at(dir, entry, &meta);
    }
    // one step, read from the link itself: git and libgit2 follow it on every open
    let target = std::fs::read_link(&entry)
        .ok()
        .and_then(|target| named_path(dir, target.to_str()?));
    let Some(target) = target else {
        return Some(Dot::Link);
    };
    match std::fs::symlink_metadata(&target) {
        Ok(meta) if meta.is_dir() || meta.is_file() => dot_at(dir, target, &meta),
        _ => Some(Dot::Link),
    }
}

/// What the `.git` of `dir` at `path` is, a folder | a file (`meta`: its own metadata).
fn dot_at(dir: &Path, path: PathBuf, meta: &std::fs::Metadata) -> Option<Dot> {
    if meta.is_dir() {
        return Some(Dot::Folder(path));
    }
    if meta.is_file() {
        let Some(named) = read_line(&path)
            .as_deref()
            .and_then(|line| line.strip_prefix("gitdir:"))
            .map(str::trim)
            .map(str::to_owned)
        else {
            return Some(Dot::Unread);
        };
        // relative to the folder of `.git` (also when a link led to the file), as git reads it
        return Some(Dot::File(named_path(dir, &named)));
    }
    None
}

/// Names of a git folder that git | libgit2 open on every look at the repository, parents before what they hold: a
/// link (symlink | junction) there would lead them anywhere, a share too.
const READ_ON_OPEN: [&str; 13] = [
    "config",
    "config.worktree",
    "HEAD",
    "info",
    "info/exclude",
    "info/attributes",
    "objects",
    "objects/info",
    "objects/info/alternates",
    "refs",
    "packed-refs",
    "index",
    "shallow",
];

/// Folders of a git folder whose every entry git | libgit2 may open at some look (the git folder itself, a ref folder,
/// an info file, a loose objects folder, a pack): a link (symlink | junction) among them would lead them anywhere, a
/// share too. Each folder is noted: an entry added | removed changes its stamp. With each, the most entries looked at;
/// a fuller folder is refused.
const LISTED: [(&str, usize); 6] = [
    ("", MAX_ENTRIES),
    ("info", MAX_ENTRIES),
    ("refs", MAX_ENTRIES),
    ("objects", MAX_ENTRIES),
    ("objects/info", MAX_ENTRIES),
    ("objects/pack", MAX_PACK_ENTRIES),
];
/// Most entries of a listed folder (git packs loose refs | objects long before).
const MAX_ENTRIES: usize = 4096;
/// Most entries of `objects/pack`: a partial clone keeps a pack, its index and a promisor file per fetch.
const MAX_PACK_ENTRIES: usize = 100_000;
/// Entries of the git folder itself that are never followed, so a link there is no risk: `hooks` (every call of the
/// installed git points `core.hooksPath` elsewhere, the built-in backend runs no hook), `lfs` (Git LFS's own store,
/// which git does not read).
const NOT_READ: [&str; 2] = ["hooks", "lfs"];

/// Why a repository git may not open is refused (a member's reason, a backend call's error).
pub(crate) const NOT_LOCAL: &str =
    "the repository's git folder points outside this computer or through a link";

/// Whether opening the git folder `git_dir` keeps git (the program, libgit2) on this computer: its `commondir`, read
/// on every open, names no path that could leave it ([`named_path`]), and in the git folder and the common folder
/// that names, nothing git opens is a link ([`READ_ON_OPEN`], [`LISTED`]), the config files name only places here
/// and so do the alternates ([`Scan`]). A file there that cannot be read here (a link, a FIFO, too large) is no proof:
/// git reads it whole, wherever it leads.
pub(crate) fn git_dir_stays_local(git_dir: &Path) -> bool {
    git_dir_scan(git_dir, &mut Scan::new())
}

/// [`git_dir_stays_local`], every path looked at kept in `scan` (their stamps keep the answer).
pub(crate) fn git_dir_scan(git_dir: &Path, scan: &mut Scan) -> bool {
    // a git folder swapped for a file | link while the project is open: both backends would follow what it names
    // (git, started in the working tree, finds that `.git` again); a missing one opens nothing
    if scan.note(git_dir).is_ok_and(|meta| !meta.is_dir()) {
        return false;
    }
    let file = git_dir.join("commondir");
    let noted = scan.note(&file);
    let common = match read_line(&file) {
        Some(named) => match scan.resolve(git_dir, &named) {
            Some(common) => Some(common),
            None => return false,
        },
        None if noted.is_err_and(|e| e.kind() == ErrorKind::NotFound) => None,
        None => return false,
    };
    std::iter::once(git_dir.to_path_buf())
        .chain(common)
        .all(|folder| folder_stays_local(&folder, scan))
}

/// [`git_dir_stays_local`] for one folder (the git folder | the common folder).
fn folder_stays_local(folder: &Path, scan: &mut Scan) -> bool {
    let no_link = READ_ON_OPEN.iter().all(|name| {
        let path = name
            .split('/')
            .fold(folder.to_path_buf(), |at, part| at.join(part));
        match scan.note(&path) {
            Ok(meta) => !meta.file_type().is_symlink(),
            Err(e) => matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory),
        }
    });
    no_link
        && LISTED.iter().all(|&(name, cap)| {
            let path = name
                .split('/')
                .filter(|part| !part.is_empty())
                .fold(folder.to_path_buf(), |at, part| at.join(part));
            let skipped: &[&str] = if name.is_empty() { &NOT_READ } else { &[] };
            no_link_in(&path, scan, cap, skipped)
        })
        && ["config", "config.worktree"]
            .iter()
            .all(|name| scan.config_stays_local(folder, &folder.join(name)))
        && scan.alternates_stay_local(&folder.join("objects"))
}

/// Whether no entry of the folder `path` but those named in `skipped` is a link, within its first `cap` entries
/// ([`LISTED`]); a missing folder | a file there holds none. The folder itself as a link is [`READ_ON_OPEN`]'s (the git
/// folder may be one: a `.git` link leads there).
fn no_link_in(path: &Path, scan: &mut Scan, cap: usize, skipped: &[&str]) -> bool {
    match scan.note(path) {
        Ok(meta) if meta.is_file() => return true,
        Ok(_) => {}
        Err(e) => return matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory),
    }
    let Ok(mut entries) = std::fs::read_dir(path) else {
        return false;
    };
    let mut looked = 0;
    entries.all(|entry| {
        looked += 1;
        // the entry's own type: a link (symlink | junction) is never followed
        looked <= cap
            && entry.is_ok_and(|entry| {
                skipped.iter().any(|name| entry.file_name() == *name)
                    || entry.file_type().is_ok_and(|kind| !kind.is_symlink())
            })
    })
}

/// Whether git may open the repository of the folder `root` without leaving this computer: a `.git` file | link | a
/// `commondir` naming a share would make it connect to the server (and offer it the person's sign-in) before anything
/// is known about the repository. git and libgit2 pass over a `.git` that is no repository to them (an empty folder)
/// and open the next one up, so every `.git` walking up must pass, not only the first.
#[must_use]
pub(crate) fn opens_locally(root: &Path) -> bool {
    let root = canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    root.ancestors()
        .take(MAX_LEVELS)
        .filter_map(git_entry)
        .all(|dot| match dot {
            Dot::Folder(git_dir) | Dot::File(Some(git_dir)) => git_dir_stays_local(&git_dir),
            Dot::File(None) | Dot::Unread | Dot::Link => false,
        })
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
        if !matches!(git_entry(&folder), None | Some(Dot::Unread)) {
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
    if !git_dir_stays_local(admin) {
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
        // a `.git` file | link naming a share: a repository git cannot open here
        Dot::File(None) | Dot::Unread | Dot::Link => return Vec::new(),
    };
    if too_broad(&workdir, &root, home) || !git_dir_stays_local(&git_dir) {
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
pub(crate) mod tests {
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

    /// A `.git` file this side cannot read as one `gitdir:` line still stops the walk, and git may not open it: git
    /// reads the file whole and drops trailing line ends, so a padded `gitdir:` naming a share would still lead it
    /// there, while the walk would have gone on to the folders above.
    #[test]
    fn a_git_file_that_cannot_be_read_here_is_never_opened() {
        let dir = tempfile::tempdir().unwrap();
        let padded = format!(
            "gitdir: {}{}",
            on_share("a.git").display(),
            "\n".repeat(8 * 1024)
        );
        let cases: [(&str, Vec<u8>); 3] = [
            ("padded", padded.into_bytes()),
            ("no gitdir line", b"something else\n".to_vec()),
            ("not text", b"gitdir: \xff\xfe\n".to_vec()),
        ];
        for (name, bytes) in cases {
            let project = dir.path().join(name);
            std::fs::create_dir_all(&project).unwrap();
            std::fs::write(project.join(".git"), bytes).unwrap();
            assert!(!opens_locally(&project), "{name}");
            assert!(linked_worktrees(&project, None).is_empty(), "{name}");
        }
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
        make_worktree(&git, "c", &dir.path().join("c"));
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
        // `a` is not offered: git reads its `commondir` on every open of that tree (and would wait on it)
        assert_eq!(from_main, ["c"]);
        assert!(from_a.is_empty(), "{from_a:?}");
        assert_eq!(branch, None);
        assert_eq!(head, None);
        assert_eq!(unborn, Some((true, None)));
    }

    /// A partial clone keeps a pack, its index and a promisor file per fetch: a pack folder with more entries than
    /// any other listed folder may hold still stays local.
    #[test]
    fn a_pack_folder_with_many_entries_stays_local() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        let pack = git.join("objects").join("pack");
        std::fs::create_dir_all(&pack).unwrap();
        for n in 0..=MAX_ENTRIES {
            std::fs::write(pack.join(format!("pack-{n}.promisor")), "").unwrap();
        }
        assert!(git_dir_stays_local(&git));
    }

    /// While the project is open, its git folder may be swapped for a `.git` file naming a share, or for a link: both
    /// backends read the git folder again on their next call (git, started in the working tree, finds that `.git`
    /// again), so the git folder must still be a folder.
    #[test]
    fn a_git_folder_swapped_for_a_file_or_a_link_no_longer_stays_local() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir_all(git.join("objects")).unwrap();
        std::fs::create_dir_all(git.join("refs")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert!(git_dir_stays_local(&git));
        let aside = dir.path().join("aside");
        std::fs::rename(&git, &aside).unwrap();
        std::fs::write(&git, "gitdir: //192.0.2.1/s/r\n").unwrap();
        assert!(!git_dir_stays_local(&git));
        std::fs::remove_file(&git).unwrap();
        if link_folder(&aside, &git) {
            assert!(!git_dir_stays_local(&git));
        }
    }

    /// A folder link at `link` leading to `target` (unix: symlink; Windows: junction, which needs no privilege);
    /// `false`: links cannot be made here.
    pub(crate) fn link_folder(target: &Path, link: &Path) -> bool {
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

    /// A git folder whose config names a share (the common one | a linked tree's own `config.worktree`) is no
    /// repository git may open: its trees are not attached, a member there is refused, and nothing is looked up.
    #[test]
    fn a_git_folder_whose_config_names_a_share_opens_and_attaches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        let git = main.join(".git");
        std::fs::create_dir_all(&git).unwrap();
        for name in ["a", "b"] {
            make_worktree(&git, name, &dir.path().join(name));
        }
        let far = format!(
            "[core]\n\texcludesFile = {}\n",
            on_share("x").display().to_string().replace('\\', "/")
        );
        // the linked tree `b` reads its own config too
        std::fs::write(
            git.join("worktrees").join("b").join("config.worktree"),
            &far,
        )
        .unwrap();
        let other = dir.path().join("other");
        std::fs::create_dir_all(other.join(".git")).unwrap();
        make_worktree(&other.join(".git"), "c", &dir.path().join("c"));
        std::fs::write(other.join(".git").join("config"), &far).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (tree_b, tree_c) = (dir.path().join("b"), dir.path().join("c"));
        std::thread::spawn(move || {
            let _ = tx.send((
                names(linked_worktrees(&main, None)),
                [&main, &tree_b, &other, &tree_c].map(|folder| opens_locally(folder)),
                names(linked_worktrees(&other, None)),
                crate::cheap::member_location(&other).is_err(),
                crate::cheap::member_location(&main).is_ok(),
            ));
        });
        let (from_main, opens, from_other, refused, member) = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("a path on a share was looked up");
        assert_eq!(from_main, ["a"]);
        assert_eq!(opens, [true, false, false, false]);
        assert!(from_other.is_empty(), "{from_other:?}");
        assert!(refused);
        assert!(member);
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
            // Win32 takes `\??\` as a system path (a share through the network redirector); a root without a drive
            // is the current drive for git, not the folder's
            assert_eq!(named_path(base, r"\??\UNC\192.0.2.1\s\x"), None);
            assert_eq!(named_path(base, r"\x"), None);
            assert_eq!(named_path(base, "/x"), None);
            assert_eq!(named_path(base, "a?b"), None);
            assert_eq!(named_path(base, r"x\.git"), Some(base.join(r"x\.git")));
        }
    }

    /// A symlink at `link` naming `target` as written (it may be missing, or on a share); `false`: symlinks cannot be
    /// made here (Windows without the privilege).
    pub(crate) fn symlink(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(target, link).is_ok()
        }
    }

    /// A `.git` that is a link (symlink | junction) is followed one step, only to a real folder | file on this
    /// computer: git and libgit2 follow it on every open. A link to a second link | to nothing is no repository, and
    /// the walk never goes on to the repository around it.
    #[test]
    fn a_git_link_leads_only_to_a_real_folder_or_file_here() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path().join("outer");
        std::fs::create_dir_all(outer.join(".git")).unwrap();
        make_worktree(&outer.join(".git"), "up", &dir.path().join("up"));
        let store = dir.path().join("store").join("near.git");
        std::fs::create_dir_all(&store).unwrap();
        make_worktree(&store, "nw", &dir.path().join("nw"));
        let project = |name: &str| {
            let folder = outer.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            folder
        };
        let near = project("near");
        if !link_folder(&store, &near.join(".git")) {
            eprintln!("links could not be made here; skipped");
            return;
        }
        assert!(opens_locally(&near));
        assert_eq!(names(linked_worktrees(&near, None)), ["nw"]);
        let hop = dir.path().join("hop");
        assert!(link_folder(&store, &hop));
        let chain = project("chain");
        assert!(link_folder(&hop, &chain.join(".git")));
        let gone = dir.path().join("gone");
        std::fs::create_dir_all(&gone).unwrap();
        let dangling = project("dangling");
        assert!(link_folder(&gone, &dangling.join(".git")));
        std::fs::remove_dir(&gone).unwrap();
        for folder in [&chain, &dangling] {
            assert!(!opens_locally(folder), "{}", folder.display());
            assert!(
                linked_worktrees(folder, None).is_empty(),
                "{}",
                folder.display()
            );
            // still a repository of its own for the one around it: no history of its files from that one
            assert!(in_nested_repository(
                &outer,
                &format!("{}/a.md", folder.file_name().unwrap().to_string_lossy())
            ));
        }
    }

    /// A `.git` link naming a share (directly | through the gitfile it leads to) is never looked up: even a lookup
    /// connects to the server and offers it the person's sign-in. A link to a gitfile here reads like that file, a
    /// relative link target against the folder that holds the link.
    #[test]
    fn a_git_link_to_a_share_is_never_looked_up() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path().join("outer");
        std::fs::create_dir_all(outer.join(".git")).unwrap();
        make_worktree(&outer.join(".git"), "up", &dir.path().join("up"));
        let store = dir.path().join("store.git");
        std::fs::create_dir_all(&store).unwrap();
        make_worktree(&store, "nw", &dir.path().join("nw"));
        let project = |name: &str| {
            let folder = outer.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            folder
        };
        let far = project("far");
        if !symlink(&on_share("r.git"), &far.join(".git")) {
            eprintln!("symlinks cannot be made here; skipped");
            return;
        }
        let far_file = dir.path().join("far file");
        std::fs::write(
            &far_file,
            format!("gitdir: {}\n", on_share("r.git").display()),
        )
        .unwrap();
        let by_far_file = project("by far file");
        assert!(symlink(&far_file, &by_far_file.join(".git")));
        let near_file = dir.path().join("near file");
        std::fs::write(&near_file, format!("gitdir: {}\n", store.display())).unwrap();
        let by_near_file = project("by near file");
        assert!(symlink(&near_file, &by_near_file.join(".git")));
        let relative = project("relative");
        assert!(symlink(
            Path::new("../../store.git"),
            &relative.join(".git")
        ));
        let (tx, rx) = std::sync::mpsc::channel();
        let folders = [far, by_far_file, by_near_file, relative];
        std::thread::spawn(move || {
            let _ = tx.send(folders.map(|folder| {
                (
                    opens_locally(&folder),
                    names(linked_worktrees(&folder, None)),
                )
            }));
        });
        let [far, by_far_file, by_near_file, relative] = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("a path on a share was looked up");
        assert_eq!(far, (false, Vec::<String>::new()));
        assert_eq!(by_far_file, (false, Vec::<String>::new()));
        // a gitfile naming a main git folder: opens, attaches nothing (no admin folder names the project back)
        assert_eq!(by_near_file, (true, Vec::<String>::new()));
        assert_eq!(relative, (true, vec!["nw".to_owned()]));
    }

    /// A file of a git folder (`gitdir`, `commondir`, `HEAD`, a ref, `packed-refs`) that is a link is never read
    /// through: its target may be anywhere, a share too. A `commondir` there is no proof that git stays here (git reads
    /// it whole, wherever it leads), so its tree is not attached and does not open.
    #[test]
    fn a_git_file_that_is_a_link_is_not_read_through() {
        const A: &str = "0123456789abcdef0123456789abcdef01234567";
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        let git = main.join(".git");
        std::fs::create_dir_all(git.join("refs").join("heads")).unwrap();
        for name in ["a", "b", "c"] {
            make_worktree(&git, name, &dir.path().join(name));
        }
        let spare = dir.path().join("spare");
        std::fs::create_dir_all(&spare).unwrap();
        let replace = |at: &Path, name: &str, text: &str| {
            std::fs::write(spare.join(name), text).unwrap();
            let _ = std::fs::remove_file(at);
            symlink(&spare.join(name), at)
        };
        if !replace(
            &git.join("worktrees").join("a").join("commondir"),
            "commondir",
            "../..\n",
        ) {
            eprintln!("symlinks cannot be made here; skipped");
            return;
        }
        let tree_b = dir.path().join("b");
        assert!(replace(
            &git.join("worktrees").join("b").join("gitdir"),
            "gitdir",
            &format!("{}\n", tree_b.join(".git").display()),
        ));
        assert!(replace(
            &git.join("refs").join("heads").join("c"),
            "ref",
            &format!("{A}\n"),
        ));
        assert_eq!(names(linked_worktrees(&main, None)), ["c"]);
        assert!(!opens_locally(&dir.path().join("a")));
        assert!(opens_locally(&dir.path().join("c")));
        // git opens `HEAD` and `packed-refs` of the common folder on every look, through a link too: the whole
        // repository is refused then
        assert!(replace(&git.join("HEAD"), "HEAD", "ref: refs/heads/main\n"));
        assert!(replace(
            &git.join("packed-refs"),
            "packed-refs",
            &format!("{A} refs/heads/c\n"),
        ));
        assert!(linked_worktrees(&main, None).is_empty());
        assert!(!opens_locally(&dir.path().join("c")));
        assert_eq!(branch_of(&git), None);
        assert_eq!(crate::cheap::cheap_facts(&git), None);
        let c = crate::cheap::cheap_facts(&git.join("worktrees").join("c")).unwrap();
        assert!(c.head.unborn && c.head.oid.is_none(), "{c:?}");
    }

    /// A relative path a git file names is never looked up through a link below the folder that holds the file: the
    /// link could lead anywhere, a share too.
    #[test]
    fn a_relative_git_path_through_a_folder_link_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        let git = main.join(".git");
        std::fs::create_dir_all(&git).unwrap();
        let tree = dir.path().join("w");
        make_worktree(&git, "w", &tree);
        // the tree names its admin folder through a link of its own
        if !link_folder(&git, &tree.join("lnk")) {
            eprintln!("links could not be made here; skipped");
            return;
        }
        std::fs::write(tree.join(".git"), "gitdir: lnk/worktrees/w\n").unwrap();
        // an admin folder names the common folder through a link inside it
        make_worktree(&git, "v", &dir.path().join("v"));
        let admin_v = git.join("worktrees").join("v");
        assert!(link_folder(&git, &admin_v.join("via")));
        std::fs::write(admin_v.join("commondir"), "via\n").unwrap();
        assert_eq!(named_path(&tree, "lnk/worktrees/w"), None);
        assert_eq!(named_path(&tree, "real/x"), Some(tree.join("real/x")));
        assert!(!opens_locally(&tree));
        assert!(!opens_locally(&dir.path().join("v")));
        assert!(linked_worktrees(&tree, None).is_empty());
        assert!(linked_worktrees(&main, None).is_empty());
        assert!(crate::cheap::member_location(&tree).is_err());
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
