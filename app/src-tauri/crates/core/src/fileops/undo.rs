//! The undo journal: what the Explorer did, newest last, enough to take it back. Persisted in the project's state
//! (`fileops/undo.json`), at most [`MAX_ENTRIES`] operations.

use serde::{Deserialize, Serialize};

use crate::state::StateData;

pub(crate) const JOURNAL_FILE: &str = "fileops/undo.json";
pub(crate) const MAX_ENTRIES: usize = 20;

/// One link edit, as it takes the link back: bytes `start..end` of the text the operation wrote hold the new
/// destination; `old` is what was there before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UndoEdit {
    pub start: usize,
    pub end: usize,
    pub old: String,
}

/// A document whose links the operation rewrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DocUndo {
    /// Where the document is after the operation.
    pub path: String,
    /// Where it was before (differs when the document moved itself).
    pub original: String,
    /// Hash (hex) of the bytes the operation wrote: the document must still be these to be put back.
    pub hash_after: String,
    pub edits: Vec<UndoEdit>,
    /// Moves (by position) whose undo this document's edits depend on.
    pub causes: Vec<usize>,
    /// The move that took the document itself along, if any.
    pub moved_by: Option<usize>,
}

/// An item an import moved in: where it came from (absolute) and where it is now (project path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ImportMoved {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub(crate) enum Op {
    Move {
        /// `(from, to)` project paths, in the order they were made.
        moves: Vec<(String, String)>,
        docs: Vec<DocUndo>,
    },
    NewFile {
        path: String,
    },
    NewFolder {
        path: String,
    },
    Delete {
        paths: Vec<String>,
    },
    /// Items dropped in from outside: copies (project paths) and moves.
    Import {
        copied: Vec<String>,
        moved: Vec<ImportMoved>,
    },
}

impl Op {
    /// Project paths the operation made, moved, removed or changed (a later operation on them, or inside them, makes
    /// an earlier one unsafe to take back out of order).
    pub(crate) fn paths(&self) -> Vec<&str> {
        match self {
            Self::Move { moves, docs } => moves
                .iter()
                .flat_map(|(from, to)| [from.as_str(), to.as_str()])
                .chain(docs.iter().map(|d| d.path.as_str()))
                .collect(),
            Self::NewFile { path } | Self::NewFolder { path } => vec![path.as_str()],
            Self::Delete { paths } => paths.iter().map(String::as_str).collect(),
            Self::Import { copied, moved } => copied
                .iter()
                .map(String::as_str)
                .chain(moved.iter().map(|m| m.to.as_str()))
                .collect(),
        }
    }
}

/// One path is the other, or inside it (project paths, `/`-separated; compared without case: Windows | macOS).
pub(crate) fn overlap(a: &str, b: &str) -> bool {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    /// Names the operation for an undo of exactly it (a notice's Undo). 0 = written before ids existed.
    #[serde(default)]
    pub id: u64,
    pub at_ms: u64,
    pub op: Op,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Journal {
    pub entries: Vec<Entry>,
    /// The next entry's id: never given twice, also after an undo (a stale notice never takes back another
    /// operation).
    #[serde(default)]
    pub next: u64,
}

impl StateData for Journal {
    const VERSION: u32 = 1;
}

impl Journal {
    /// Adds the newest operation; returns its id.
    pub(crate) fn push(&mut self, at_ms: u64, op: Op) -> u64 {
        let newest = self.entries.iter().map(|e| e.id).max().unwrap_or(0);
        let id = self.next.max(newest + 1).max(1);
        self.next = id + 1;
        self.entries.push(Entry { id, at_ms, op });
        let extra = self.entries.len().saturating_sub(MAX_ENTRIES);
        self.entries.drain(..extra);
        id
    }

    /// Takes out the entry `id` when it can still be taken back: present, and no newer operation touched its paths.
    pub(crate) fn take(&mut self, id: u64) -> Option<Entry> {
        let at = self.entries.iter().position(|e| e.id == id && id != 0)?;
        let own = self.entries[at].op.paths();
        let touched = self.entries[at + 1..].iter().any(|later| {
            later
                .op
                .paths()
                .iter()
                .any(|p| own.iter().any(|o| overlap(p, o)))
        });
        if touched {
            return None;
        }
        Some(self.entries.remove(at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_twenty() {
        let mut journal = Journal::default();
        for n in 0..25 {
            journal.push(
                n,
                Op::NewFolder {
                    path: format!("f{n}"),
                },
            );
        }
        assert_eq!(journal.entries.len(), MAX_ENTRIES);
        assert_eq!(journal.entries.first().map(|e| e.at_ms), Some(5));
        assert_eq!(journal.entries.last().map(|e| e.at_ms), Some(24));
    }

    #[test]
    fn round_trips_through_json() {
        let journal = Journal {
            next: 2,
            entries: vec![Entry {
                id: 1,
                at_ms: 1,
                op: Op::Move {
                    moves: vec![("a.md".to_owned(), "b/a.md".to_owned())],
                    docs: vec![DocUndo {
                        path: "x.md".to_owned(),
                        original: "x.md".to_owned(),
                        hash_after: "0123456789abcdef".to_owned(),
                        edits: vec![UndoEdit {
                            start: 3,
                            end: 9,
                            old: "a.md".to_owned(),
                        }],
                        causes: vec![0],
                        moved_by: None,
                    }],
                },
            }],
        };
        let text = serde_json::to_string(&journal).unwrap();
        assert!(text.contains("\"op\":\"move\""));
        assert_eq!(serde_json::from_str::<Journal>(&text).unwrap(), journal);
    }

    fn import(paths: &[&str]) -> Op {
        Op::Import {
            copied: paths.iter().map(|p| (*p).to_owned()).collect(),
            moved: Vec::new(),
        }
    }

    #[test]
    fn ids_are_never_given_twice_also_after_an_undo() {
        let mut journal = Journal::default();
        let a = journal.push(1, import(&["a.md"]));
        let b = journal.push(2, import(&["b.md"]));
        assert_eq!((a, b), (1, 2));
        assert!(journal.take(b).is_some());
        let c = journal.push(3, import(&["c.md"]));
        assert_eq!(c, 3, "b's id stays used");
        assert!(journal.take(b).is_none(), "an old notice finds nothing");
    }

    #[test]
    fn an_entry_is_taken_back_out_of_order_only_when_nothing_newer_touched_its_paths() {
        let mut journal = Journal::default();
        let first = journal.push(1, import(&["docs/a.md", "docs/pics"]));
        let other = journal.push(2, import(&["notes/x.md"]));
        let _ = journal.push(
            3,
            Op::Move {
                moves: vec![("docs/PICS/1.png".to_owned(), "img/1.png".to_owned())],
                docs: Vec::new(),
            },
        );
        assert!(
            journal.take(first).is_none(),
            "a file inside a copied folder moved since"
        );
        assert_eq!(journal.entries.len(), 3, "nothing taken out");
        assert_eq!(journal.take(other).map(|e| e.id), Some(other));
        assert_eq!(journal.entries.len(), 2);
    }

    #[test]
    fn a_journal_from_before_ids_reads_and_goes_on() {
        let old = r#"{"entries":[{"at_ms":5,"op":{"op":"newFolder","path":"x"}}]}"#;
        let mut journal: Journal = serde_json::from_str(old).unwrap();
        assert_eq!(journal.entries[0].id, 0);
        assert!(
            journal.take(0).is_none(),
            "an entry without id is never named"
        );
        assert_eq!(journal.push(6, import(&["y"])), 1);
    }

    #[test]
    fn paths_overlap_without_case_and_only_on_whole_names() {
        assert!(overlap("docs/a.md", "docs/a.md"));
        assert!(overlap("Docs", "docs/a.md"));
        assert!(overlap("docs/a.md", "docs"));
        assert!(!overlap("docs/a.md", "docs/a.md2"));
        assert!(!overlap("doc", "docs/a.md"));
    }
}
