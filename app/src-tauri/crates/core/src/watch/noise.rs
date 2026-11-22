//! Which paths are noise: scratch files, the app's private state folders, git internals nobody reads.

use crate::fsx::is_scratch_name;
use crate::state::PERMARK_DIR;
use crate::tree::{below_root, named};

/// Files of a team folder (`.permark`) followed while the folder is open: shared settings, the project card, the
/// project list.
const TEAM_FILES: [&str; 3] = ["config.toml", "meta.toml", "projects.toml"];

/// `.git` entries the git plugin takes as a signal that the repository moved: HEAD, the index, packed refs, refs; and
/// the repository's ignore file (what the tree and the index list follows it).
fn git_signal(inside_git: &str) -> bool {
    matches!(
        inside_git,
        "HEAD" | "index" | "packed-refs" | "info/exclude"
    ) || inside_git.starts_with("refs/")
}

/// The part of `rel` inside a `.git` folder (any level), if it is inside one.
fn inside_git(rel: &str) -> Option<&str> {
    if let Some(rest) = rel.strip_prefix(".git/") {
        return Some(rest);
    }
    rel.find("/.git/").map(|at| &rel[at + "/.git/".len()..])
}

/// Whether a change of this project path is worth looking at (before the ignore rules).
#[must_use]
pub fn is_news(path: &str) -> bool {
    let rel = below_root(path);
    // `.git` at any level: the project's own or a nested repository's
    if let Some(inside) = inside_git(rel) {
        return git_signal(inside);
    }
    if rel == ".git" || rel.ends_with("/.git") {
        return false;
    }
    // the app's private state folders, the project's own and a nested project's
    let mut above = "";
    for part in rel.split('/') {
        if is_scratch_name(part) || (above == ".permark" && matches!(part, "local" | "cache")) {
            return false;
        }
        above = part;
    }
    true
}

/// When this project path is a team folder (`.permark`, of the opened folder or of a folder below) or one of its
/// [`TEAM_FILES`]: the folder holding that `.permark`, below its root (`""`: the root). Their followers hear of a change
/// whatever the ignore rules say of the team folder (git sharing set to private ignores everything inside it; a
/// `.gitignore` may name it), as long as the holder itself is shown.
#[must_use]
pub(crate) fn team_holder(path: &str) -> Option<&str> {
    let rel = below_root(path);
    let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    let team_folder = if named(name, PERMARK_DIR) {
        rel
    } else if TEAM_FILES.iter().any(|file| named(name, file))
        && named(parent.rsplit('/').next().unwrap_or(""), PERMARK_DIR)
    {
        parent
    } else {
        return None;
    };
    Some(
        team_folder
            .rsplit_once('/')
            .map_or("", |(holder, _)| holder),
    )
}

/// `.git/...` signal path (the tree ignores these; they wake the git plugin).
#[must_use]
pub fn is_git_signal(path: &str) -> bool {
    inside_git(below_root(path)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_files_are_noise_anywhere() {
        for p in [
            "a/b.tmp",
            "a/b.md~",
            ".#b.md",
            "a/.b.md.123.4.part",
            "a/.b.swp",
            "4913",
            "x/tmp~/y.md",
        ] {
            assert!(!is_news(p), "{p}");
        }
        for p in ["a/b.md", "a/b.tmpx.md", "a/tmp/b.md", "notes~.md"] {
            assert!(is_news(p), "{p}");
        }
    }

    #[test]
    fn permark_private_folders_are_noise_but_its_config_is_news() {
        assert!(!is_news(".permark/local/session.json"));
        assert!(!is_news(".permark/cache/x"));
        assert!(!is_news("@wt/w/.permark/local/x"));
        assert!(!is_news("hubs/a/.permark/local/read.json"));
        assert!(!is_news("hubs/a/.permark/cache"));
        assert!(is_news("hubs/a/.permark/config.toml"));
        assert!(is_news("local/cache.md"));
        assert!(is_news(".permark/config.toml"));
        assert!(is_news(".permark/meta.toml"));
    }

    #[test]
    fn team_paths_are_the_folder_and_its_followed_files_at_any_level() {
        for (p, holder) in [
            (".permark", ""),
            ("hubs/a/.permark", "hubs/a"),
            (".permark/config.toml", ""),
            (".permark/meta.toml", ""),
            ("hubs/a/.permark/projects.toml", "hubs/a"),
            ("@wt/w/.permark/config.toml", ""),
            ("@wt/w/sub/.permark", "sub"),
        ] {
            assert_eq!(team_holder(p), Some(holder), "{p}");
        }
        for p in [
            ".permark/tags.toml",
            ".permark/.gitignore",
            ".permark/local/config.toml",
            "config.toml",
            "docs/config.toml",
            ".permarks/config.toml",
            "@wt/w",
        ] {
            assert_eq!(team_holder(p), None, "{p}");
        }
    }

    #[test]
    fn git_internals_pass_only_as_signals() {
        for p in [
            ".git/HEAD",
            ".git/index",
            ".git/packed-refs",
            ".git/refs/heads/main",
            ".git/info/exclude",
            "hubs/a/.git/info/exclude",
        ] {
            assert!(is_news(p), "{p}");
            assert!(is_git_signal(p));
        }
        for p in [
            ".git",
            ".git/objects/ab/cdef",
            ".git/index.lock",
            ".git/config",
            ".git/logs/HEAD",
            ".git/info/attributes",
        ] {
            assert!(!is_news(p), "{p}");
        }
        assert!(!is_news("@wt/w/.git/objects/x"));
        assert!(!is_news("hubs/a/.git/objects/ab/cd"));
        assert!(is_news("hubs/a/.git/HEAD"));
        assert!(is_news("@wt/w/.git/HEAD"));
        assert!(is_news(".github/workflows/ci.yml"));
    }
}
