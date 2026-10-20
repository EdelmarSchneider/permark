//! "What does this pattern catch?" for ignore files: how many files under the ignore file's folder each pattern
//! matches (gitignore rules, each pattern on its own). The walk does not apply any ignore rules, so `node_modules/`
//! counts what is in it. Bounded: a long walk is cut and the counts are lower bounds.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use ignore::gitignore::GitignoreBuilder;
use ignore::WalkBuilder;
use serde::Serialize;
use ts_rs::TS;

/// Files looked at before the walk stops.
pub const MAX_FILES: usize = 50_000;
/// Patterns counted.
pub const MAX_PATTERNS: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PatternMatches {
    /// Line of the pattern (1-based).
    pub line: u32,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IgnoreMatches {
    pub counts: Vec<PatternMatches>,
    /// The walk or the pattern list was cut: counts are lower bounds.
    pub capped: bool,
}

struct Tree {
    files: Vec<String>,
    /// Directory (relative, `/` separators) -> files below it.
    dirs: BTreeMap<String, u32>,
    capped: bool,
}

fn walk(base: &Path) -> Tree {
    let mut files = Vec::new();
    let mut capped = false;
    let walker = WalkBuilder::new(base)
        .standard_filters(false)
        .hidden(false)
        .follow_links(false)
        .sort_by_file_name(std::cmp::Ord::cmp)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if files.len() >= MAX_FILES {
            capped = true;
            break;
        }
        if let Ok(rel) = entry.path().strip_prefix(base) {
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    let mut dirs: BTreeMap<String, u32> = BTreeMap::new();
    for file in &files {
        let mut end = 0;
        while let Some(i) = file[end..].find('/') {
            end += i;
            *dirs.entry(file[..end].to_owned()).or_insert(0) += 1;
            end += 1;
        }
    }
    Tree {
        files,
        dirs,
        capped,
    }
}

/// Whether `path` lies under a directory of `hit`.
fn under(hit: &HashSet<&str>, path: &str) -> bool {
    let mut end = 0;
    while let Some(i) = path[end..].find('/') {
        end += i;
        if hit.contains(&path[..end]) {
            return true;
        }
        end += 1;
    }
    false
}

fn count(tree: &Tree, pattern: &str, base: &Path) -> u32 {
    let mut builder = GitignoreBuilder::new(base);
    if builder.add_line(None, pattern).is_err() {
        return 0;
    }
    let Ok(gitignore) = builder.build() else {
        return 0;
    };
    let mut hit: HashSet<&str> = HashSet::new();
    let mut total = 0_u32;
    for (dir, files) in &tree.dirs {
        if under(&hit, dir) {
            continue;
        }
        if !gitignore.matched(dir, true).is_none() {
            hit.insert(dir);
            total = total.saturating_add(*files);
        }
    }
    for file in &tree.files {
        if under(&hit, file) {
            continue;
        }
        if !gitignore.matched(file, false).is_none() {
            total = total.saturating_add(1);
        }
    }
    total
}

/// Counts for `patterns` (`(line, text)`) against the files under `base`.
#[must_use]
pub fn ignore_matches(base: &Path, patterns: &[(u32, String)]) -> IgnoreMatches {
    let tree = walk(base);
    let capped = tree.capped || patterns.len() > MAX_PATTERNS;
    let wanted: Vec<&(u32, String)> = patterns.iter().take(MAX_PATTERNS).collect();
    let threads = std::thread::available_parallelism()
        .map_or(2, usize::from)
        .min(4);
    let chunk = wanted.len().div_ceil(threads).max(1);
    let mut counts: Vec<PatternMatches> = std::thread::scope(|scope| {
        let handles: Vec<_> = wanted
            .chunks(chunk)
            .map(|part| {
                let tree = &tree;
                scope.spawn(move || {
                    part.iter()
                        .map(|(line, text)| PatternMatches {
                            line: *line,
                            count: count(tree, text, base),
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    counts.sort_by_key(|c| c.line);
    IgnoreMatches { counts, capped }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in [
            "a.log",
            "b.txt",
            "keep.log",
            "src/main.rs",
            "src/deep/x.tmp",
            "src/y.tmp",
            "node_modules/p/index.js",
            "node_modules/p/lib/a.js",
            "node_modules/q.js",
            "dist/out.js",
            ".git/config",
            "docs/notes.md",
        ] {
            let abs = dir.path().join(file);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(abs, "x").unwrap();
        }
        dir
    }

    fn counts(patterns: &[&str]) -> Vec<u32> {
        let dir = project();
        let list: Vec<(u32, String)> = patterns
            .iter()
            .enumerate()
            .map(|(i, p)| (u32::try_from(i + 1).unwrap(), (*p).to_owned()))
            .collect();
        let result = ignore_matches(dir.path(), &list);
        assert!(!result.capped);
        result.counts.into_iter().map(|c| c.count).collect()
    }

    #[test]
    fn each_pattern_counts_what_it_catches() {
        let got = counts(&[
            "*.log",
            "node_modules/",
            "/dist",
            "src/**/*.tmp",
            "*.tmp",
            "!keep.log",
            "nothing-here",
            "docs/",
            "/b.txt",
            "*",
        ]);
        // .git is never counted; a directory pattern counts the files below it once
        assert_eq!(got, [2, 3, 1, 2, 2, 1, 0, 1, 1, 11]);
    }

    #[test]
    fn bad_and_odd_patterns_count_nothing_and_never_panic() {
        assert_eq!(counts(&["[", "\\", "**/", "a/**", "#c", " "]).len(), 6);
    }

    #[test]
    fn the_pattern_list_is_bounded() {
        let dir = project();
        let many: Vec<(u32, String)> = (1..=500).map(|n| (n, "*.log".to_owned())).collect();
        let result = ignore_matches(dir.path(), &many);
        assert!(result.capped);
        assert_eq!(result.counts.len(), MAX_PATTERNS);
    }
}
