//! Cheap facts of a project's repository (`projects.md` §8 cheap tier): read from files in its git folder, no git
//! process, no libgit2: where it is (`.git` folder | gitfile), HEAD (branch | detached commit | no commit yet) and the
//! repository state from marker files. A few small reads per repository.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use permark_core::conditions::RepoState;

use crate::ops::{repo_state, RepoLocation};
use crate::types::HeadInfo;
use crate::worktrees::{common_dir, read_line};

/// Most bytes of `packed-refs` read looking for one ref (a huge file is not read to its end).
const MAX_PACKED: u64 = 8 * 1024 * 1024;

/// What files say about a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cheap {
    pub head: HeadInfo,
    pub state: RepoState,
}

/// The repository whose working tree is `folder` (an included project with a `.git` marker): its `.git` folder, or
/// the folder a gitfile names (`gitdir: <path>`, relative to `folder`; may point outside the opened folder: read
/// only). `Err`: why it cannot be used (the sentence names the cause).
///
/// # Errors
/// No `.git` | a gitfile without a `gitdir:` line | its target missing.
pub fn member_location(folder: &Path) -> Result<RepoLocation, String> {
    let entry = folder.join(".git");
    let meta = std::fs::symlink_metadata(&entry).map_err(|_| "no .git".to_owned())?;
    let git_dir = if meta.is_dir() {
        entry
    } else if meta.is_file() {
        let line = read_line(&entry).ok_or_else(|| "unreadable .git file".to_owned())?;
        let target = line
            .strip_prefix("gitdir:")
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| "a .git file without gitdir".to_owned())?;
        let target = PathBuf::from(target);
        let target = if target.is_absolute() {
            target
        } else {
            folder.join(target)
        };
        if !target.is_dir() {
            return Err("the repository named by .git is missing".to_owned());
        }
        dunce::canonicalize(&target).unwrap_or(target)
    } else {
        return Err("no .git".to_owned());
    };
    Ok(RepoLocation {
        workdir: folder.to_path_buf(),
        git_dir,
        prefix: String::new(),
    })
}

fn is_hex_oid(text: &str) -> bool {
    (4..=64).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The commit a ref names: its loose file, else its line in `packed-refs`; `None`: no such ref (unborn branch).
fn resolve_ref(common: &Path, git_dir: &Path, name: &str) -> Option<String> {
    // a ref name never climbs out of the git folder
    if name
        .split('/')
        .any(|s| s.is_empty() || s == "." || s == "..")
        || name.contains('\\')
    {
        return None;
    }
    for dir in [git_dir, common] {
        if let Some(line) = read_line(&dir.join(name)) {
            if is_hex_oid(&line) {
                return Some(line.to_ascii_lowercase());
            }
        }
    }
    let file = std::fs::File::open(common.join("packed-refs")).ok()?;
    let reader = BufReader::new(std::io::Read::take(file, MAX_PACKED));
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if let Some((oid, refname)) = line.split_once(' ') {
            if refname.trim() == name && is_hex_oid(oid) {
                return Some(oid.to_ascii_lowercase());
            }
        }
    }
    None
}

/// HEAD + state of the repository at `git_dir`; `None`: HEAD unreadable (not a repository any more).
#[must_use]
pub fn cheap_facts(git_dir: &Path) -> Option<Cheap> {
    let line = read_line(&git_dir.join("HEAD"))?;
    let head = if let Some(target) = line.strip_prefix("ref:").map(str::trim) {
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
    } else if is_hex_oid(&line) {
        HeadInfo {
            branch: None,
            oid: Some(line.to_ascii_lowercase()),
            detached: true,
            unborn: false,
        }
    } else {
        return None;
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
        std::fs::write(git.join("HEAD"), "garbage\n").unwrap();
        assert_eq!(cheap_facts(git), None);
        std::fs::write(git.join("HEAD"), "ref: refs/heads/../../../etc\n").unwrap();
        assert!(
            cheap_facts(git).unwrap().head.unborn,
            "a ref never climbs out"
        );
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
