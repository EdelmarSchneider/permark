//! Which folders and files a person has silenced.
//!
//! Everything speaks until they say otherwise. A mark on a path covers that path and whatever is
//! inside it, including files that appear later. A closer mark wins, so `src` can be quiet while
//! `src/docs` speaks. The choice is personal: it is kept next to `read.json`, and an agent's search
//! and unread list are not filtered by it.

use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::session::write_local_state;

/// Whether a path asks for the person's attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Interest {
    Quiet,
    Speaking,
}

/// The closest mark on `path` or a folder above it. No mark means speaking.
pub fn is_quiet(path: &str, marks: &BTreeMap<String, Interest>) -> bool {
    let mut current = Some(path);
    while let Some(at) = current {
        if let Some(mark) = marks.get(at) {
            return *mark == Interest::Quiet;
        }
        current = parent(at);
    }
    false
}

/// Documents that were quiet and now speak: they are marked read so old changes do not flood in.
pub fn waking_paths<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    before: &BTreeMap<String, Interest>,
    after: &BTreeMap<String, Interest>,
) -> Vec<String> {
    paths
        .into_iter()
        .filter(|path| is_quiet(path, before) && !is_quiet(path, after))
        .map(str::to_owned)
        .collect()
}

/// The folder that contains `path`, or `None` at a root (the project, or a worktree).
fn parent(path: &str) -> Option<&str> {
    if path.is_empty() {
        return None;
    }
    if let Some(rest) = path.strip_prefix("@wt/") {
        if !rest.contains('/') {
            return None;
        }
    }
    match path.rfind('/') {
        Some(at) => Some(&path[..at]),
        None => Some(""),
    }
}

/// Rewrites marks when a file or folder moves. A mark on `from` and on anything inside it follows.
fn rewrite(marks: &mut BTreeMap<String, Interest>, from: &str, to: &str) -> bool {
    if from.is_empty() || from == to {
        return false;
    }
    let prefix = format!("{from}/");
    let moving: Vec<(String, Interest)> = marks
        .iter()
        .filter_map(|(path, interest)| {
            if path == from {
                Some((to.to_owned(), *interest))
            } else {
                path.strip_prefix(&prefix)
                    .map(|rest| (format!("{to}/{rest}"), *interest))
            }
        })
        .collect();
    if moving.is_empty() {
        return false;
    }
    marks.retain(|path, _| path != from && !path.starts_with(&prefix));
    for (path, interest) in moving {
        marks.insert(path, interest);
    }
    true
}

/// The marks of one open folder, loaded from `quiet.json`.
pub struct QuietBook {
    path: Mutex<PathBuf>,
    marks: Mutex<BTreeMap<String, Interest>>,
}

impl QuietBook {
    pub fn open(path: PathBuf) -> Self {
        let marks = std::fs::read(&path)
            .ok()
            .and_then(|bytes| decode(&bytes))
            .unwrap_or_default();
        Self {
            path: Mutex::new(path),
            marks: Mutex::new(marks),
        }
    }

    /// The project keeps its state somewhere else from now on.
    pub fn relocate(&self, path: PathBuf) {
        let marks = self.rules();
        if let Ok(mut slot) = self.path.lock() {
            *slot = path;
        }
        self.save(&marks);
    }

    pub fn rules(&self) -> BTreeMap<String, Interest> {
        self.marks.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Sets the same mark on each path. Paths are stored as given (the caller resolved them).
    pub fn set(&self, paths: &[String], interest: Interest) -> BTreeMap<String, Interest> {
        let mut marks = self.rules();
        for path in paths {
            marks.insert(path.clone(), interest);
        }
        self.store(marks)
    }

    /// A file or folder was renamed or moved: its mark, and the marks inside it, follow.
    pub fn renamed(&self, from: &str, to: &str) {
        let mut marks = self.rules();
        if rewrite(&mut marks, from, to) {
            self.store(marks);
        }
    }

    fn store(&self, marks: BTreeMap<String, Interest>) -> BTreeMap<String, Interest> {
        self.save(&marks);
        if let Ok(mut slot) = self.marks.lock() {
            *slot = marks.clone();
        }
        marks
    }

    fn save(&self, marks: &BTreeMap<String, Interest>) {
        let Ok(path) = self.path.lock() else {
            return;
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(marks) {
            let _ = write_local_state(&path, &bytes);
        }
    }
}

fn decode(bytes: &[u8]) -> Option<BTreeMap<String, Interest>> {
    let raw: BTreeMap<String, String> = serde_json::from_slice(bytes).ok()?;
    Some(
        raw.into_iter()
            .filter_map(|(path, value)| match value.as_str() {
                "quiet" => Some((path, Interest::Quiet)),
                "speaking" => Some((path, Interest::Speaking)),
                _ => None,
            })
            .collect(),
    )
}

/// Where `quiet.json` is kept: next to `read.json`.
#[cfg(test)]
fn quiet_path(dir: &Path) -> PathBuf {
    dir.join("quiet.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(pairs: &[(&str, Interest)]) -> BTreeMap<String, Interest> {
        pairs
            .iter()
            .map(|(path, interest)| ((*path).to_owned(), *interest))
            .collect()
    }

    #[test]
    fn nothing_is_quiet_until_a_mark_says_so() {
        let marks = BTreeMap::new();
        assert!(!is_quiet("src/main.rs", &marks));
        assert!(!is_quiet("", &marks));
    }

    #[test]
    fn a_folder_mark_covers_what_is_inside_and_a_closer_mark_wins() {
        let marks = marks(&[
            ("src", Interest::Quiet),
            ("src/docs", Interest::Speaking),
            ("src/docs/drafts", Interest::Quiet),
        ]);
        assert!(is_quiet("src", &marks));
        assert!(is_quiet("src/main.rs", &marks));
        assert!(!is_quiet("src/docs", &marks));
        assert!(!is_quiet("src/docs/guide.md", &marks));
        assert!(is_quiet("src/docs/drafts/old.md", &marks));
        assert!(!is_quiet("README.md", &marks));
    }

    #[test]
    fn silencing_the_project_does_not_silence_a_worktree() {
        let marks = marks(&[("", Interest::Quiet)]);
        assert!(is_quiet("src/main.rs", &marks));
        assert!(!is_quiet("@wt/feature/src/main.rs", &marks));
    }

    #[test]
    fn a_worktree_folder_can_be_quiet_on_its_own() {
        let marks = marks(&[("@wt/feature/src", Interest::Quiet)]);
        assert!(is_quiet("@wt/feature/src/main.rs", &marks));
        assert!(!is_quiet("@wt/feature/README.md", &marks));
        assert!(!is_quiet("src/main.rs", &marks));
    }

    #[test]
    fn waking_is_only_what_was_quiet_and_now_speaks() {
        let before = marks(&[("src", Interest::Quiet), ("src/docs", Interest::Speaking)]);
        let after = marks(&[
            ("src", Interest::Speaking),
            ("src/docs", Interest::Speaking),
        ]);
        let woke = waking_paths(
            ["src/main.rs", "src/docs/guide.md", "README.md"],
            &before,
            &after,
        );
        assert_eq!(woke, ["src/main.rs"]);
    }

    #[test]
    fn a_rename_carries_the_mark_and_the_marks_inside() {
        let mut book = marks(&[
            ("src", Interest::Quiet),
            ("src/docs", Interest::Speaking),
            ("other", Interest::Quiet),
        ]);
        assert!(rewrite(&mut book, "src", "lib"));
        assert_eq!(book.get("lib"), Some(&Interest::Quiet));
        assert_eq!(book.get("lib/docs"), Some(&Interest::Speaking));
        assert!(!book.contains_key("src"));
        assert_eq!(book.get("other"), Some(&Interest::Quiet));
        assert!(!rewrite(&mut book, "missing", "gone"));
    }

    #[test]
    fn a_file_is_round_tripped_and_unknown_marks_are_dropped() {
        let dir = std::env::temp_dir().join(format!("permark-quiet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = quiet_path(&dir);
        let book = QuietBook::open(path.clone());
        assert!(book.rules().is_empty());
        book.set(&["src".to_owned()], Interest::Quiet);
        book.set(&["src/docs".to_owned()], Interest::Speaking);
        let again = QuietBook::open(path);
        assert_eq!(again.rules().get("src"), Some(&Interest::Quiet));
        assert_eq!(again.rules().get("src/docs"), Some(&Interest::Speaking));
        std::fs::write(quiet_path(&dir), br#"{"src":"nope","docs":"quiet"}"#).expect("write");
        let dropped = QuietBook::open(quiet_path(&dir));
        assert!(!dropped.rules().contains_key("src"));
        assert_eq!(dropped.rules().get("docs"), Some(&Interest::Quiet));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
