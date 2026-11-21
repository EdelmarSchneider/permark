//! Cheap facts of a project's repository (`projects.md` §8 cheap tier): read from files in its git folder, no git
//! process, no libgit2: where it is (`.git` folder | gitfile), HEAD (branch | detached commit | no commit yet) and the
//! repository state from marker files. A few small reads per repository.

use std::path::Path;

use permark_core::conditions::RepoState;
use permark_core::plugins::CommitId;

use crate::ops::{repo_state, GitError, RepoLocation};
use crate::types::HeadInfo;
use crate::worktrees::{
    common_dir, git_dir_stays_local, named_path, read_git_file, read_line, NOT_LOCAL,
};

/// Most bytes of `packed-refs` read looking for one ref (a bigger file is not read: the ref counts as missing).
const MAX_PACKED: u64 = 8 * 1024 * 1024;

/// A member's repository that may not be used, and why.
fn refused(why: &str) -> GitError {
    GitError::Failed(why.to_owned())
}

#[cfg(test)]
thread_local! {
    /// Member folders read on this thread (tests: opening a project reads none).
    pub(crate) static LOCATIONS_READ: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// What files say about a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cheap {
    pub head: HeadInfo,
    pub state: RepoState,
}

/// The repository whose working tree is `folder` (an included project with a `.git` marker): its `.git` folder, or
/// the folder a gitfile names (`gitdir: <path>`, relative to `folder`; may point outside the opened folder: read
/// only). `Err`: why it cannot be used (a failure whose words name the cause).
///
/// # Errors
/// No `.git` | a gitfile without a `gitdir:` line | its target missing | not on this computer (also a path its git
/// folder's files name, or a link there: [`git_dir_stays_local`]).
pub fn member_location(folder: &Path) -> Result<RepoLocation, GitError> {
    #[cfg(test)]
    LOCATIONS_READ.with(|c| c.set(c.get() + 1));
    let entry = folder.join(".git");
    let meta = std::fs::symlink_metadata(&entry).map_err(|_| refused("no .git"))?;
    let git_dir = if meta.is_dir() {
        entry
    } else if meta.is_file() {
        let line = read_line(&entry).ok_or_else(|| refused("unreadable .git file"))?;
        let target = line
            .strip_prefix("gitdir:")
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| refused("a .git file without gitdir"))?;
        // a share | a mapped network drive named here would be looked up below, and by git on every open
        let target = named_path(folder, target)
            .ok_or_else(|| refused("the repository named by .git is not on this computer"))?;
        if !target.is_dir() {
            return Err(refused("the repository named by .git is missing"));
        }
        permark_core::paths::canonicalize(&target).unwrap_or(target)
    } else {
        // a link (symlink | junction) too: a member is read from files only, never through a link
        return Err(refused("no .git"));
    };
    if !git_dir_stays_local(&git_dir) {
        return Err(refused(NOT_LOCAL));
    }
    Ok(RepoLocation {
        workdir: folder.to_path_buf(),
        git_dir,
        prefix: String::new(),
    })
}

/// Whether HEAD may name `name`: a ref below `refs/` made of plain segments only. Anything else (`..`, an absolute
/// path, a drive `C:`, a `\`) would read outside the git folder: HEAD is invalid then.
fn valid_ref(name: &str) -> bool {
    name.starts_with("refs/")
        && !name.contains(['\\', ':'])
        && name
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
        && Path::new(name)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// The commit a ref names: its loose file, else its line in `packed-refs`; `None`: no such ref (unborn branch).
fn resolve_ref(common: &Path, git_dir: &Path, name: &str) -> Option<String> {
    // a ref name never climbs out of the git folder
    if !valid_ref(name) {
        return None;
    }
    let loose = [git_dir, common]
        .into_iter()
        .find_map(|dir| CommitId::parse(&read_line(&dir.join(name))?));
    // a plain file only, never through a link (a FIFO would hold the read forever)
    let id = loose.or_else(|| {
        let packed = read_git_file(&common.join("packed-refs"), MAX_PACKED)?;
        String::from_utf8_lossy(&packed)
            .lines()
            .filter_map(|line| line.split_once(' '))
            .filter(|(_, refname)| refname.trim() == name)
            .find_map(|(oid, _)| CommitId::parse(oid))
    })?;
    Some(id.as_str().to_owned())
}

/// HEAD + state of the repository at `git_dir`; `None`: HEAD unreadable (not a repository any more).
#[must_use]
pub fn cheap_facts(git_dir: &Path) -> Option<Cheap> {
    let line = read_line(&git_dir.join("HEAD"))?;
    let head = if let Some(target) = line.strip_prefix("ref:").map(str::trim) {
        if !valid_ref(target) {
            // HEAD names something outside the git folder: not a repository PerMark reads
            return None;
        }
        let common = common_dir(git_dir);
        let oid = resolve_ref(&common, git_dir, target);
        HeadInfo {
            branch: Some(
                target
                    .strip_prefix("refs/heads/")
                    .unwrap_or(target)
                    .to_owned(),
            ),
            unborn: oid.is_none(),
            oid,
            detached: false,
        }
    } else {
        HeadInfo {
            branch: None,
            oid: Some(CommitId::parse(&line)?.as_str().to_owned()),
            detached: true,
            unborn: false,
        }
    };
    Some(Cheap {
        head,
        state: repo_state(git_dir),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0123456789abcdef0123456789abcdef01234567";
    const B: &str = "89abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn head_from_files() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path();
        assert_eq!(cheap_facts(git), None, "no HEAD");
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let unborn = cheap_facts(git).unwrap();
        assert_eq!(unborn.head.branch.as_deref(), Some("main"));
        assert!(unborn.head.unborn && unborn.head.oid.is_none() && !unborn.head.detached);
        std::fs::write(
            git.join("packed-refs"),
            format!("# pack-refs\n{B} refs/heads/main\n"),
        )
        .unwrap();
        assert_eq!(
            cheap_facts(git).unwrap().head.oid.as_deref(),
            Some(B),
            "packed"
        );
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::write(git.join("refs/heads/main"), format!("{A}\n")).unwrap();
        let born = cheap_facts(git).unwrap();
        assert_eq!(born.head.oid.as_deref(), Some(A), "loose beats packed");
        assert!(!born.head.unborn);
        std::fs::write(git.join("HEAD"), format!("{}\n", A.to_uppercase())).unwrap();
        let detached = cheap_facts(git).unwrap();
        assert!(detached.head.detached && detached.head.branch.is_none());
        assert_eq!(detached.head.oid.as_deref(), Some(A));
        // only a full commit id names a commit: SHA-1 | SHA-256, never a short one
        let sha256 = B.repeat(2)[..64].to_owned();
        std::fs::write(git.join("HEAD"), format!("{sha256}\n")).unwrap();
        assert_eq!(cheap_facts(git).unwrap().head.oid, Some(sha256));
        std::fs::write(git.join("HEAD"), "0123abcd\n").unwrap();
        assert_eq!(cheap_facts(git), None);
        std::fs::write(git.join("refs/heads/short"), "0123abcd\n").unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/short\n").unwrap();
        assert!(cheap_facts(git).unwrap().head.unborn, "a short id in a ref");
        std::fs::write(git.join("HEAD"), "garbage\n").unwrap();
        assert_eq!(cheap_facts(git), None);
        // a HEAD naming anything outside the git folder is invalid: nothing read there
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, format!("{A}\n")).unwrap();
        let drive = outside.display().to_string().replace('\\', "/");
        for bad in [
            "refs/heads/../../../etc".to_owned(),
            drive.clone(),
            format!("refs/{drive}"),
            "/etc/passwd".to_owned(),
            "C:/x/y".to_owned(),
            "refs\\heads\\main".to_owned(),
            "refs/heads//main".to_owned(),
            "../outside.txt".to_owned(),
            "HEAD".to_owned(),
        ] {
            std::fs::write(git.join("HEAD"), format!("ref: {bad}\n")).unwrap();
            assert_eq!(cheap_facts(git), None, "{bad}");
        }
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir(git.join("rebase-merge")).unwrap();
        assert_eq!(cheap_facts(git).unwrap().state, RepoState::Rebasing);
    }

    #[test]
    fn locations_of_folders_and_gitfiles() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain ü");
        std::fs::create_dir_all(plain.join(".git")).unwrap();
        let loc = member_location(&plain).unwrap();
        assert_eq!(loc.git_dir, plain.join(".git"));
        assert_eq!(loc.prefix, "");
        // gitfile, relative target (a submodule's `../.git/modules/x`)
        let sub = dir.path().join("sub");
        let modules = dir.path().join("modules").join("x");
        std::fs::create_dir_all(&modules).unwrap();
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(".git"), "gitdir: ../modules/x\n").unwrap();
        let loc = member_location(&sub).unwrap();
        assert_eq!(loc.git_dir, dunce::canonicalize(&modules).unwrap());
        // gitfile to a missing folder | without the line | no marker
        std::fs::write(sub.join(".git"), "gitdir: ../nowhere\n").unwrap();
        assert!(member_location(&sub).is_err());
        std::fs::write(sub.join(".git"), "nothing\n").unwrap();
        assert!(member_location(&sub).is_err());
        assert!(member_location(&dir.path().join("none")).is_err());
    }

    /// A member's `.git` that is a link is refused wherever it leads: a member is read from files only, never through
    /// a link.
    #[test]
    fn a_member_whose_git_is_a_link_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir_all(real.join(".git")).unwrap();
        let member = dir.path().join("member");
        std::fs::create_dir_all(&member).unwrap();
        if !crate::worktrees::tests::link_folder(&real.join(".git"), &member.join(".git")) {
            eprintln!("links could not be made here; skipped");
            return;
        }
        assert!(member_location(&real).is_ok());
        assert!(member_location(&member).is_err());
    }

    #[test]
    fn a_linked_worktree_reads_refs_from_its_common_folder() {
        let dir = tempfile::tempdir().unwrap();
        let common = dir.path().join("main").join(".git");
        let admin = common.join("worktrees").join("w");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::create_dir_all(common.join("refs/heads")).unwrap();
        std::fs::write(common.join("refs/heads/feat"), format!("{A}\n")).unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(admin.join("HEAD"), "ref: refs/heads/feat\n").unwrap();
        let facts = cheap_facts(&admin).unwrap();
        assert_eq!(facts.head.branch.as_deref(), Some("feat"));
        assert_eq!(facts.head.oid.as_deref(), Some(A));
    }
}
