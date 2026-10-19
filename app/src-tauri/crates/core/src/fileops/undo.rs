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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub at_ms: u64,
    pub op: Op,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Journal {
    pub entries: Vec<Entry>,
}

impl StateData for Journal {
    const VERSION: u32 = 1;
}

impl Journal {
    pub(crate) fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
        let extra = self.entries.len().saturating_sub(MAX_ENTRIES);
        self.entries.drain(..extra);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_twenty() {
        let mut journal = Journal::default();
        for n in 0..25 {
            journal.push(Entry {
                at_ms: n,
                op: Op::NewFolder {
                    path: format!("f{n}"),
                },
            });
        }
        assert_eq!(journal.entries.len(), MAX_ENTRIES);
        assert_eq!(journal.entries.first().map(|e| e.at_ms), Some(5));
        assert_eq!(journal.entries.last().map(|e| e.at_ms), Some(24));
    }

    #[test]
    fn round_trips_through_json() {
        let journal = Journal {
            entries: vec![Entry {
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
}
