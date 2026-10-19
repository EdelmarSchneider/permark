//! Code references (`src/main.rs:42` in a document): which project files a written path can mean.
//! The UI reads references out of the page; this finds the files.

use std::collections::{HashMap, HashSet};

use crate::links::join_relative;
use crate::paths::WORKTREE_PREFIX;
use crate::tree::{Explorer, NodeKind};

/// A name that matches more files than this is too common to guess from (`index.ts`, `mod.rs`): it stays plain text.
pub const MAX_CANDIDATES: usize = 12;

/// `@wt/<name>/` of a project path, if it lies in a linked worktree.
fn worktree_prefix(path: &str) -> Option<&str> {
    let rest = path.strip_prefix(WORKTREE_PREFIX)?;
    let end = rest.find('/')?;
    Some(&path[..=WORKTREE_PREFIX.len() + end])
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// How many leading folders two paths share.
fn shared_folders(a: &str, b: &str) -> usize {
    let folders = |p: &'_ str| -> Vec<String> {
        let mut parts: Vec<String> = p.split('/').map(str::to_owned).collect();
        parts.pop();
        parts
    };
    folders(a)
        .iter()
        .zip(folders(b).iter())
        .take_while(|(x, y)| x == y)
        .count()
}

/// The written path without the `./` and `../` that only mean something from a folder.
fn tail_of(clean: &str) -> String {
    let segments: Vec<&str> = clean.split('/').collect();
    let last = segments.len().saturating_sub(1);
    segments
        .iter()
        .enumerate()
        .filter(|(i, s)| !((**s == "." || **s == "..") && *i < last))
        .map(|(_, s)| *s)
        .collect::<Vec<_>>()
        .join("/")
}

/// For each written path: the project files it can mean. One entry: found (relative to the project root, else to the
/// document's folder). Several: the path matches the end of more than one file (closest to the document first).
/// None: not a file of the project. A document in a linked worktree looks in that worktree only, and one in the main
/// folder never looks into the worktrees (which hold a copy of every file).
#[must_use]
pub fn resolve_code_refs(explorer: &Explorer, doc: &str, written: &[String]) -> Vec<Vec<String>> {
    let prefix = worktree_prefix(doc).unwrap_or("");
    let cleaned: Vec<String> = written
        .iter()
        .map(|w| w.replace('\\', "/").trim_start_matches('/').to_owned())
        .collect();
    // names to look for, with one pass over the tree for all of them
    let names: HashSet<String> = cleaned
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| basename(&tail_of(c)).to_owned())
        .collect();
    let mut by_name: Option<HashMap<String, Vec<String>>> = None;
    written
        .iter()
        .zip(&cleaned)
        .map(|(raw, clean)| {
            if clean.is_empty() {
                return Vec::new();
            }
            let from_root = join_relative(&format!("{prefix}_"), &format!("/{clean}"));
            let from_doc = join_relative(doc, &raw.replace('\\', "/"));
            for candidate in [from_root, from_doc].into_iter().flatten() {
                if matches!(explorer.listed(&candidate), Some(NodeKind::File { .. })) {
                    return vec![candidate];
                }
            }
            let tail = tail_of(clean);
            let index = by_name.get_or_insert_with(|| explorer.files_named(&names));
            let mut matches: Vec<String> = index
                .get(basename(&tail))
                .map(|files| {
                    files
                        .iter()
                        .filter(|file| {
                            worktree_prefix(file).unwrap_or("") == prefix
                                && (**file == format!("{prefix}{tail}")
                                    || file.ends_with(&format!("/{tail}")))
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            if matches.is_empty() || matches.len() > MAX_CANDIDATES {
                return Vec::new();
            }
            matches.sort_by(|a, b| {
                shared_folders(b, doc)
                    .cmp(&shared_folders(a, doc))
                    .then_with(|| a.cmp(b))
            });
            matches
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(worktree_prefix("@wt/feat/src/a.rs"), Some("@wt/feat/"));
        assert_eq!(worktree_prefix("src/a.rs"), None);
        assert_eq!(worktree_prefix("@wt/feat"), None);
        assert_eq!(tail_of("../../src/main.rs"), "src/main.rs");
        assert_eq!(tail_of("./a.rs"), "a.rs");
        assert_eq!(tail_of("a/./b/c.rs"), "a/b/c.rs");
        assert_eq!(shared_folders("docs/api/a.md", "docs/api/b.rs"), 2);
        assert_eq!(shared_folders("docs/a.md", "src/b.rs"), 0);
    }
}
