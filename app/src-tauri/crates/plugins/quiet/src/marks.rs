//! Which folders and files the person silenced (pure).
//!
//! Everything speaks until a mark says otherwise. A mark on a path covers that path and whatever is inside it,
//! including files that appear later. A closer mark wins, so `src` can be quiet while `src/docs` speaks. A linked
//! worktree is a root of its own (`@wt/<name>`): a mark above it does not reach into it. The choice is personal: it is
//! kept in `quiet.json` (a flat map of path to `"quiet"` | `"speaking"`, the layout of 0.5), and an agent is not
//! filtered by it.

use std::collections::{BTreeMap, HashMap};

use permark_core::paths::{key_of, WORKTREE_PREFIX};
use serde::Serialize;
use ts_rs::TS;

/// Whether a path asks for the person's attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Interest {
    Quiet,
    Speaking,
}

impl Interest {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "quiet" => Some(Self::Quiet),
            "speaking" => Some(Self::Speaking),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quiet => "quiet",
            Self::Speaking => "speaking",
        }
    }
}

/// The folder that contains `path`, or `None` at a root (the project, or a worktree).
fn parent(path: &str) -> Option<&str> {
    if path.is_empty() {
        return None;
    }
    if let Some(rest) = path.strip_prefix(WORKTREE_PREFIX) {
        if !rest.contains('/') {
            return None;
        }
    }
    match path.rfind('/') {
        Some(at) => Some(&path[..at]),
        None => Some(""),
    }
}

/// The marks of one project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    /// Path as written to interest.
    marks: BTreeMap<String, Interest>,
    /// Identity key to interest (case folded where the file system is).
    by_key: HashMap<String, Interest>,
}

impl Marks {
    fn index(marks: BTreeMap<String, Interest>) -> Self {
        let by_key = marks.iter().map(|(p, i)| (key_of(p), *i)).collect();
        Self { marks, by_key }
    }

    /// Reads the `quiet.json` of 0.5: `{ "path": "quiet" | "speaking" }`. Anything else in it is skipped; text that is
    /// not such a map is no marks.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let raw: BTreeMap<String, serde_json::Value> = serde_json::from_slice(bytes).ok()?;
        Some(Self::index(
            raw.into_iter()
                .filter_map(|(path, value)| Some((path, Interest::parse(value.as_str()?)?)))
                .collect(),
        ))
    }

    /// Marks from a map (path as written -> interest).
    #[must_use]
    pub fn from_map(marks: BTreeMap<String, Interest>) -> Self {
        Self::index(marks)
    }

    /// The text of `quiet.json`.
    #[must_use]
    pub fn to_json(&self) -> Vec<u8> {
        let map: BTreeMap<&str, &str> = self
            .marks
            .iter()
            .map(|(path, interest)| (path.as_str(), interest.as_str()))
            .collect();
        serde_json::to_vec_pretty(&map).unwrap_or_default()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.marks.len()
    }

    /// Every mark, by path.
    pub fn iter(&self) -> impl Iterator<Item = (&str, Interest)> {
        self.marks.iter().map(|(p, i)| (p.as_str(), *i))
    }

    /// The marks as the UI gets them.
    #[must_use]
    pub fn to_map(&self) -> BTreeMap<String, Interest> {
        self.marks.clone()
    }

    /// The closest mark on `path` or a folder above it. No mark means speaking.
    #[must_use]
    pub fn is_quiet(&self, path: &str) -> bool {
        if self.by_key.is_empty() {
            return false;
        }
        let mut current = Some(path);
        while let Some(at) = current {
            if let Some(mark) = self.by_key.get(&key_of(at)) {
                return *mark == Interest::Quiet;
            }
            current = parent(at);
        }
        false
    }

    /// Sets the same mark on each path. A path that already has that mark through a parent only (no mark of its own)
    /// still gets its own: the person said it.
    pub fn set(&mut self, paths: &[String], interest: Interest) {
        for path in paths {
            if let Some(old) = self.spelled(path) {
                self.marks.remove(&old);
            }
            self.marks.insert(path.clone(), interest);
        }
        *self = Self::index(std::mem::take(&mut self.marks));
    }

    /// Takes a mark of a path off (it follows its parent again).
    pub fn clear(&mut self, paths: &[String]) {
        for path in paths {
            if let Some(old) = self.spelled(path) {
                self.marks.remove(&old);
            }
        }
        *self = Self::index(std::mem::take(&mut self.marks));
    }

    /// The spelling in which `path` has a mark of its own.
    fn spelled(&self, path: &str) -> Option<String> {
        if self.marks.contains_key(path) {
            return Some(path.to_owned());
        }
        let key = key_of(path);
        self.marks.keys().find(|k| key_of(k) == key).cloned()
    }

    /// A file or folder moved: its mark, and the marks inside it, follow. `true` when anything changed.
    pub fn renamed(&mut self, from: &str, to: &str) -> bool {
        if from.is_empty() || key_of(from) == key_of(to) && from == to {
            return false;
        }
        let from_key = key_of(from);
        let prefix = format!("{from_key}/");
        let moving: Vec<(String, Interest)> = self
            .marks
            .iter()
            .filter(|(p, _)| {
                let k = key_of(p);
                k == from_key || k.starts_with(&prefix)
            })
            .map(|(p, i)| (p.clone(), *i))
            .collect();
        if moving.is_empty() {
            return false;
        }
        for (path, interest) in moving {
            self.marks.remove(&path);
            let rest = path.get(from.len()..).unwrap_or_default();
            self.marks.insert(format!("{to}{rest}"), interest);
        }
        *self = Self::index(std::mem::take(&mut self.marks));
        true
    }
}

/// The paths among `paths` that were quiet and now speak (changes while quiet must not arrive all at once).
pub fn waking<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    before: &Marks,
    after: &Marks,
) -> Vec<String> {
    paths
        .into_iter()
        .filter(|p| before.is_quiet(p) && !after.is_quiet(p))
        .map(str::to_owned)
        .collect()
}
