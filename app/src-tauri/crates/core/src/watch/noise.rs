//! Which paths are noise: scratch files, the app's private state folders, git internals nobody reads.

use crate::fsx::is_scratch_name;
use crate::tree::below_root;

/// `.git` entries the git plugin takes as a signal that the repository moved: HEAD, the index, packed refs, refs.
fn git_signal(inside_git: &str) -> bool {
    matches!(inside_git, "HEAD" | "index" | "packed-refs") || inside_git.starts_with("refs/")
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
    for private in [".permark/local", ".permark/cache"] {
        if rel == private
            || rel
                .strip_prefix(private)
                .is_some_and(|r| r.starts_with('/'))
        {
            return false;
        }
    }
    if rel.split('/').any(is_scratch_name) {
        return false;
    }
    true
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
        assert!(is_news(".permark/config.toml"));
        assert!(is_news(".permark/meta.toml"));
    }

    #[test]
    fn git_internals_pass_only_as_signals() {
        for p in [
            ".git/HEAD",
            ".git/index",
            ".git/packed-refs",
            ".git/refs/heads/main",
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
