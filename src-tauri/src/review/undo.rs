//! What a person can take back in a review: the Keeps and Reverts of each document, last first. In memory only, and
//! let go when the review of the document is closed ([`super::ReviewService::release`]).
//!
//! A step holds what it takes to put things back and to know that nothing else happened meanwhile (see
//! [`super::ReviewService::undo`]): where the document stood in the book for a Keep, and for a Revert the text the
//! file had and the hash it was left with. Texts are what costs memory, so a document's steps are capped.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use super::book::Standing;
use crate::render::markdown::is_markdown_path;
use crate::vault::key;

/// The most steps kept for a document; the oldest go first.
const MAX_STEPS: usize = 50;

/// The most text kept for the steps of a document, in bytes; the oldest steps go first.
const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// One Keep or Revert, with what it takes to take it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Something was kept: the document stood at `before`, and stands at `after`.
    Kept { before: Standing, after: Standing },
    /// Something was reverted: the file held `text`, and was left with the hash `after_hash`.
    Reverted { text: String, after_hash: u64 },
    /// A new document was sent to the recycle bin: the file held `text`.
    Discarded { text: String },
}

impl Step {
    fn text_bytes(&self) -> usize {
        match self {
            Self::Kept { .. } => 0,
            Self::Reverted { text, .. } | Self::Discarded { text } => text.len(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Stacks {
    /// By case-folded path, the last step last.
    steps: BTreeMap<String, Vec<Step>>,
}

impl Stacks {
    /// `step` is the newest of the document's.
    pub fn push(&mut self, path: &str, step: Step) {
        let steps = self.steps.entry(key(path)).or_default();
        steps.push(step);
        let too_many = steps.len().saturating_sub(MAX_STEPS);
        steps.drain(..too_many);
        // and not more text than fits, but the newest step stays whatever it weighs
        let mut bytes: usize = steps.iter().map(Step::text_bytes).sum();
        let mut oldest = 0;
        while bytes > MAX_TEXT_BYTES && oldest + 1 < steps.len() {
            bytes -= steps[oldest].text_bytes();
            oldest += 1;
        }
        steps.drain(..oldest);
    }

    /// Takes the newest step of the document off.
    pub fn pop(&mut self, path: &str) -> Option<Step> {
        let k = key(path);
        let steps = self.steps.get_mut(&k)?;
        let step = steps.pop();
        if steps.is_empty() {
            self.steps.remove(&k);
        }
        step
    }

    /// Lets go of all the steps of the document. Returns whether there were any.
    pub fn clear(&mut self, path: &str) -> bool {
        self.steps.remove(&key(path)).is_some()
    }

    /// How many steps the document has.
    #[cfg(test)]
    pub fn depth(&self, path: &str) -> usize {
        self.steps.get(&key(path)).map_or(0, Vec::len)
    }

    /// The document moved to `to`, and its steps with it. `to` is no document (empty, or not a Markdown name) when it
    /// was deleted or renamed away, and then the steps are let go: what was done to a file that is gone cannot be taken
    /// back. Except that a new document was sent away, which is what a step is for.
    pub fn moved(&mut self, from: &str, to: &str) {
        let k = key(from);
        let Some(mut steps) = self.steps.remove(&k) else {
            return;
        };
        if is_markdown_path(Path::new(to)) {
            self.steps.insert(key(to), steps);
            return;
        }
        steps.retain(|step| matches!(step, Step::Discarded { .. }));
        if !steps.is_empty() {
            self.steps.insert(k, steps);
        }
    }

    /// The hashes of the versions the steps need to put things back: their copies are kept.
    pub fn hashes(&self) -> HashSet<u64> {
        self.steps
            .values()
            .flatten()
            .filter_map(|step| match step {
                Step::Kept { before, after } => Some(before.hashes().chain(after.hashes())),
                Step::Reverted { .. } | Step::Discarded { .. } => None,
            })
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(from: u64, to: u64) -> Step {
        Step::Kept {
            before: Standing {
                reviewed: Some(from),
                held: None,
            },
            after: Standing {
                reviewed: Some(to),
                held: None,
            },
        }
    }

    fn reverted(text: &str) -> Step {
        Step::Reverted {
            text: text.to_owned(),
            after_hash: 9,
        }
    }

    #[test]
    fn the_last_step_comes_off_first_and_each_document_has_its_own() {
        let mut stacks = Stacks::default();
        stacks.push("a.md", kept(1, 2));
        stacks.push("a.md", reverted("text"));
        stacks.push("b.md", kept(5, 6));
        assert_eq!(stacks.depth("a.md"), 2);
        assert_eq!(stacks.pop("a.md"), Some(reverted("text")));
        assert_eq!(stacks.pop("a.md"), Some(kept(1, 2)));
        assert_eq!(stacks.pop("a.md"), None);
        assert_eq!(stacks.depth("b.md"), 1);
    }

    #[test]
    fn clearing_lets_go_of_one_document_only() {
        let mut stacks = Stacks::default();
        stacks.push("a.md", kept(1, 2));
        stacks.push("b.md", kept(5, 6));
        assert!(stacks.clear("a.md"));
        assert!(!stacks.clear("a.md"));
        assert_eq!(stacks.depth("b.md"), 1);
    }

    #[test]
    fn paths_are_compared_the_way_the_file_system_compares_them() {
        let mut stacks = Stacks::default();
        stacks.push("Docs/A.md", kept(1, 2));
        if crate::vault::CASE_INSENSITIVE {
            assert_eq!(stacks.depth("docs/a.md"), 1);
        } else {
            assert_eq!(stacks.depth("docs/a.md"), 0);
        }
    }

    #[test]
    fn a_document_that_moves_takes_its_steps_along_and_one_that_is_deleted_loses_them() {
        let mut stacks = Stacks::default();
        stacks.push("a.md", kept(1, 2));
        stacks.moved("a.md", "docs/b.md");
        assert_eq!(stacks.depth("a.md"), 0);
        assert_eq!(stacks.depth("docs/b.md"), 1);
        stacks.moved("docs/b.md", "");
        assert_eq!(stacks.depth("docs/b.md"), 0);
        // a new document that was sent away stays to be brought back
        stacks.push("new.md", kept(1, 2));
        stacks.push(
            "new.md",
            Step::Discarded {
                text: "# New
"
                .to_owned(),
            },
        );
        stacks.moved("new.md", "");
        assert_eq!(stacks.depth("new.md"), 1);
        assert!(matches!(stacks.pop("new.md"), Some(Step::Discarded { .. })));
        stacks.push("c.md", kept(1, 2));
        stacks.moved("c.md", "c.txt");
        assert_eq!(stacks.depth("c.txt"), 0, "no longer a document");
        stacks.moved("never-had-steps.md", "x.md");
        assert_eq!(stacks.depth("x.md"), 0);
    }

    #[test]
    fn the_oldest_steps_go_when_there_are_too_many() {
        let mut stacks = Stacks::default();
        for i in 0..60 {
            stacks.push("a.md", kept(i, i + 1));
        }
        assert_eq!(stacks.depth("a.md"), MAX_STEPS);
        assert_eq!(stacks.pop("a.md"), Some(kept(59, 60)));
        let mut oldest = None;
        while let Some(step) = stacks.pop("a.md") {
            oldest = Some(step);
        }
        assert_eq!(oldest, Some(kept(10, 11)));
    }

    #[test]
    fn the_oldest_steps_go_when_their_texts_are_too_big_but_the_newest_always_stays() {
        let mut stacks = Stacks::default();
        let big = "x".repeat(MAX_TEXT_BYTES / 2 + 1);
        stacks.push("a.md", reverted(&big));
        stacks.push("a.md", reverted(&big));
        assert_eq!(stacks.depth("a.md"), 1, "two do not fit");
        stacks.push("a.md", reverted("small"));
        assert_eq!(stacks.depth("a.md"), 2);
        let huge = "x".repeat(MAX_TEXT_BYTES + 5);
        stacks.push("a.md", reverted(&huge));
        assert_eq!(stacks.depth("a.md"), 1, "even alone, the newest is kept");
    }

    #[test]
    fn the_versions_the_steps_point_to_are_listed() {
        let mut stacks = Stacks::default();
        stacks.push("a.md", kept(1, 2));
        stacks.push("a.md", reverted("text"));
        stacks.push("b.md", kept(5, 6));
        assert_eq!(stacks.hashes(), HashSet::from([1, 2, 5, 6]));
    }
}
