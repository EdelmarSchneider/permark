//! The undo journal: what the Explorer did, newest last, enough to take it back, at most [`MAX_ENTRIES`] operations.
//! Kept in this install's own folder of the project's state (`machine/<iid>/fileops/journal.json`). Every entry is
//! sealed with this install's key and bound to the project folder, and the install folder's `marks.json` names the
//! entries still to be taken back: a journal that came with the folder (a zip, a clone, a sync partner), one of
//! another install, or an older copy of this one is never acted on.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ops::undoable;
use crate::paths::VaultPath;
use crate::state::{Machine, Seal, StateData, StateError, StateFile, StateStore};

pub(crate) const JOURNAL_FILE: &str = "fileops/journal.json";
pub(crate) const MAX_ENTRIES: usize = 20;
/// In the install folder: per project folder, the entries still to be taken back.
pub(crate) const MARKS_FILE: &str = "marks.json";
/// Where versions before the sealed journal kept it, in the state folder: renamed once, never read.
const LEGACY_FILE: &str = "fileops/undo.json";
const LEGACY_KEPT: &str = "undo.json.pre-1.0";
/// Project folders `marks.json` remembers; the one used longest ago goes first.
const MAX_MARK_ROWS: usize = 1000;
const SEAL_KIND: &str = "fileops/journal";

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

    /// Every project path it names may be changed by an undo ([`undoable`]): one bad path drops the whole entry.
    fn paths_usable(&self) -> bool {
        let originals = match self {
            Self::Move { docs, .. } => docs.iter().map(|d| d.original.as_str()).collect(),
            _ => Vec::new(),
        };
        self.paths()
            .into_iter()
            .chain(originals)
            .all(|path| VaultPath::parse(path).is_ok_and(|path| undoable(&path)))
    }

    /// Its kind, then every field in one fixed order per kind; a list = its length, then its elements.
    fn seal_into(&self, seal: &mut Seal) {
        let texts = |seal: &mut Seal, list: &[String]| {
            seal.num(list.len() as u64);
            for text in list {
                seal.field(text.as_bytes());
            }
        };
        match self {
            Self::Move { moves, docs } => {
                seal.field(b"move").num(moves.len() as u64);
                for (from, to) in moves {
                    seal.field(from.as_bytes()).field(to.as_bytes());
                }
                seal.num(docs.len() as u64);
                for doc in docs {
                    seal.field(doc.path.as_bytes())
                        .field(doc.original.as_bytes())
                        .field(doc.hash_after.as_bytes())
                        .num(doc.edits.len() as u64);
                    for edit in &doc.edits {
                        seal.num(edit.start as u64)
                            .num(edit.end as u64)
                            .field(edit.old.as_bytes());
                    }
                    seal.num(doc.causes.len() as u64);
                    for cause in &doc.causes {
                        seal.num(*cause as u64);
                    }
                    let moved_by = doc.moved_by.map(|m| (m as u64).to_be_bytes());
                    seal.opt(moved_by.as_ref().map(<[u8; 8]>::as_slice));
                }
            }
            Self::NewFile { path } => {
                seal.field(b"newFile").field(path.as_bytes());
            }
            Self::NewFolder { path } => {
                seal.field(b"newFolder").field(path.as_bytes());
            }
            Self::Delete { paths } => texts(seal.field(b"delete"), paths),
            Self::Import { copied, moved } => {
                texts(seal.field(b"import"), copied);
                seal.num(moved.len() as u64);
                for item in moved {
                    seal.field(item.from.as_bytes()).field(item.to.as_bytes());
                }
            }
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
    /// Names the operation for an undo of exactly it (a notice's Undo); never 0 for an entry this version wrote.
    #[serde(default)]
    pub id: u64,
    pub at_ms: u64,
    pub op: Op,
    /// This install's seal over the project folder, `id` and every field of `op` (`at_ms` is only shown).
    #[serde(default)]
    pub seal: String,
}

impl Entry {
    /// The seal of this entry under `machine` (its key and project folder), not signed yet.
    fn sealed(&self, machine: &Machine) -> Seal {
        let mut seal = machine.seal(SEAL_KIND);
        seal.num(self.id);
        self.op.seal_into(&mut seal);
        seal
    }

    /// The seal this entry carries when `machine` wrote it.
    pub(crate) fn seal_with(&self, machine: &Machine) -> String {
        self.sealed(machine).sign()
    }
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
    /// 2: sealed entries in this install's folder (1: `fileops/undo.json` of earlier versions, never read).
    const VERSION: u32 = 2;
    /// No writer limit: a move keeps one entry per document whose links it rewrote.
    const MAX_BYTES: u64 = 32 * 1024 * 1024;
}

impl Journal {
    /// Adds the newest operation; returns its id.
    pub(crate) fn push(&mut self, at_ms: u64, op: Op) -> u64 {
        let newest = self.entries.iter().map(|e| e.id).max().unwrap_or(0);
        let id = self.next.max(newest + 1).max(1);
        self.next = id + 1;
        self.entries.push(Entry {
            id,
            at_ms,
            op,
            seal: String::new(),
        });
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

/// Per project folder (the one used last at the end): the next entry id and the entries still to be taken back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Marks {
    rows: Vec<MarkRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MarkRow {
    /// The project folder's id under this install's key ([`Machine::rid`]).
    rid: String,
    /// Never given again for this folder, also when its journal is lost.
    next: u64,
    /// Ids of the entries an undo may still take back (at most [`MAX_ENTRIES`]).
    live: Vec<u64>,
}

impl StateData for Marks {
    const VERSION: u32 = 1;
}

impl Marks {
    fn row(&self, rid: &str) -> Option<&MarkRow> {
        self.rows.iter().find(|row| row.rid == rid)
    }

    /// The row of `rid`, made if missing and moved last; the rows used longest ago beyond [`MAX_MARK_ROWS`] go.
    fn touch(&mut self, rid: &str) -> &mut MarkRow {
        let row = match self.rows.iter().position(|row| row.rid == rid) {
            Some(at) => self.rows.remove(at),
            None => MarkRow {
                rid: rid.to_owned(),
                next: 0,
                live: Vec::new(),
            },
        };
        let extra = (self.rows.len() + 1).saturating_sub(MAX_MARK_ROWS);
        self.rows.drain(..extra);
        self.rows.push(row);
        let last = self.rows.len() - 1;
        &mut self.rows[last]
    }
}

/// The journal of one project with its seals and marks.
#[derive(Debug)]
pub(crate) struct History {
    file: StateFile<Journal>,
    marks: StateFile<Marks>,
    machine: Machine,
}

impl History {
    /// The journal of `store`'s project. `None`: no usable install key: nothing is journaled (no undo rather than
    /// an undo of something this install cannot vouch for).
    pub(crate) fn of(store: &StateStore) -> Option<Self> {
        retire_legacy(store.dir());
        Some(Self {
            file: store.machine_file(JOURNAL_FILE)?,
            marks: store.install_file(MARKS_FILE)?,
            machine: store.machine()?.clone(),
        })
    }

    /// The entries this install recorded for this folder that an undo may still take back: sealed by it for this
    /// folder, marked live, every path usable. The rest is left out (and gone from the file at its next save).
    pub(crate) fn load(&self) -> Journal {
        let mut journal = self.file.load().into_value();
        let marks = self.marks.load().into_value();
        let row = marks.row(self.machine.rid());
        let live = row.map_or(&[][..], |row| row.live.as_slice());
        journal.entries.retain(|entry| {
            entry.id != 0
                && live.contains(&entry.id)
                && entry.op.paths_usable()
                && entry.sealed(&self.machine).verify(&entry.seal)
        });
        journal.next = journal.next.max(row.map_or(0, |row| row.next));
        journal
    }

    /// Journals `op`: marked live first, then written, so a failure in between leaves a mark of nothing, never an
    /// entry to replay. Its id; `None`: not journaled (the marks or the journal could not be written).
    pub(crate) fn record(&self, at_ms: u64, op: Op) -> Option<u64> {
        let mut journal = self.load();
        // the entries the push keeps beside the new one: the marks of the others go
        let kept: Vec<u64> = journal
            .entries
            .iter()
            .rev()
            .take(MAX_ENTRIES - 1)
            .map(|entry| entry.id)
            .collect();
        let newest = journal.entries.iter().map(|e| e.id).max().unwrap_or(0);
        let lowest = journal.next.max(newest + 1).max(1);
        let mut id = 0;
        self.marks
            .update(|marks| {
                let mut marks = marks.cloned().unwrap_or_default();
                let row = marks.touch(self.machine.rid());
                id = row.next.max(lowest);
                row.next = id + 1;
                row.live.retain(|seq| kept.contains(seq));
                row.live.push(id);
                Some(marks)
            })
            .ok()?;
        journal.next = id;
        let pushed = journal.push(at_ms, op);
        if let Some(entry) = journal.entries.last_mut() {
            entry.seal = entry.seal_with(&self.machine);
        }
        self.file.save(&journal).ok().map(|()| pushed)
    }

    /// Takes the mark of entry `id` away, before its undo runs: no copy of the journal brings it back. `Ok(false)`:
    /// not marked (taken back meanwhile elsewhere): nothing to take back.
    ///
    /// # Errors
    /// The marks could not be read or written.
    pub(crate) fn unmark(&self, id: u64) -> Result<bool, StateError> {
        let mut had = false;
        self.marks.update(|marks| {
            had = false;
            let mut marks = marks?.clone();
            let row = marks
                .rows
                .iter_mut()
                .find(|row| row.rid == self.machine.rid())?;
            had = row.live.contains(&id);
            row.live.retain(|seq| *seq != id);
            had.then_some(marks)
        })?;
        Ok(had)
    }

    /// # Errors
    /// The journal could not be written.
    pub(crate) fn save(&self, journal: &Journal) -> Result<(), StateError> {
        self.file.save(journal)
    }
}

/// The journal of a version before the sealed one is renamed to `undo.json.pre-1.0`, once: kept for whoever wants
/// it, never read again. Best effort: a name already taken leaves both as they are, and only in a real folder (a
/// link in place of `fileops` is never followed).
fn retire_legacy(state_dir: &Path) {
    let old = state_dir.join(LEGACY_FILE);
    let Some(folder) = old.parent() else {
        return;
    };
    let kept = folder.join(LEGACY_KEPT);
    let real_folder = fs::symlink_metadata(folder).is_ok_and(|m| m.file_type().is_dir());
    if real_folder && fs::symlink_metadata(&old).is_ok() && fs::symlink_metadata(&kept).is_err() {
        let _ = fs::rename(&old, &kept);
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
                seal: "s1:0123456789abcdef0123456789abcdef".to_owned(),
            }],
        };
        let text = serde_json::to_string(&journal).unwrap();
        assert!(text.contains("\"op\":\"move\""));
        assert!(text.contains("\"seal\":\"s1:"));
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
    fn an_entry_without_id_or_seal_still_reads_and_is_never_named() {
        let old = r#"{"entries":[{"at_ms":5,"op":{"op":"newFolder","path":"x"}}]}"#;
        let mut journal: Journal = serde_json::from_str(old).unwrap();
        assert_eq!(journal.entries[0].id, 0);
        assert_eq!(journal.entries[0].seal, "");
        assert!(
            journal.take(0).is_none(),
            "an entry without id is never named"
        );
        assert_eq!(journal.push(6, import(&["y"])), 1);
    }

    /// A store of a project folder: its install key, folder id and journal folder are real.
    fn project_store(base: &Path) -> (tempfile::TempDir, StateStore) {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::Root::open(dir.path()).unwrap();
        let store = StateStore::for_project(&crate::state::AppDirs::under(base), &root);
        (dir, store)
    }

    fn full_move() -> Op {
        Op::Move {
            moves: vec![
                ("a.md".to_owned(), "b/a.md".to_owned()),
                ("c".to_owned(), "d".to_owned()),
            ],
            docs: vec![DocUndo {
                path: "x.md".to_owned(),
                original: "y.md".to_owned(),
                hash_after: "0123456789abcdef".to_owned(),
                edits: vec![
                    UndoEdit {
                        start: 3,
                        end: 9,
                        old: "a.md".to_owned(),
                    },
                    UndoEdit {
                        start: 12,
                        end: 14,
                        old: String::new(),
                    },
                ],
                causes: vec![0, 1],
                moved_by: Some(1),
            }],
        }
    }

    /// Each kind seals its fields in one fixed order: written out here a second time, independently of the code.
    #[test]
    fn every_kind_seals_its_fields_in_one_fixed_order() {
        let base = tempfile::tempdir().unwrap();
        let (_dir, store) = project_store(base.path());
        let machine = store.machine().unwrap();
        let entry = |id, op| Entry {
            id,
            at_ms: 1,
            op,
            seal: String::new(),
        };
        let mut want = machine.seal(SEAL_KIND);
        want.num(7).field(b"move").num(2);
        want.field(b"a.md").field(b"b/a.md").field(b"c").field(b"d");
        want.num(1)
            .field(b"x.md")
            .field(b"y.md")
            .field(b"0123456789abcdef");
        want.num(2)
            .num(3)
            .num(9)
            .field(b"a.md")
            .num(12)
            .num(14)
            .field(b"");
        want.num(2).num(0).num(1).opt(Some(&1u64.to_be_bytes()));
        assert_eq!(entry(7, full_move()).seal_with(machine), want.sign());
        let new_file = Op::NewFile {
            path: "n.md".to_owned(),
        };
        let mut want = machine.seal(SEAL_KIND);
        want.num(1).field(b"newFile").field(b"n.md");
        assert_eq!(entry(1, new_file).seal_with(machine), want.sign());
        let new_folder = Op::NewFolder {
            path: "n".to_owned(),
        };
        let mut want = machine.seal(SEAL_KIND);
        want.num(1).field(b"newFolder").field(b"n");
        assert_eq!(entry(1, new_folder).seal_with(machine), want.sign());
        let delete = Op::Delete {
            paths: vec!["p".to_owned(), "q".to_owned()],
        };
        let mut want = machine.seal(SEAL_KIND);
        want.num(1).field(b"delete").num(2).field(b"p").field(b"q");
        assert_eq!(entry(1, delete).seal_with(machine), want.sign());
        let import = Op::Import {
            copied: vec!["c.md".to_owned()],
            moved: vec![ImportMoved {
                from: "/out/m.md".to_owned(),
                to: "m.md".to_owned(),
            }],
        };
        let mut want = machine.seal(SEAL_KIND);
        want.num(1).field(b"import").num(1).field(b"c.md");
        want.num(1).field(b"/out/m.md").field(b"m.md");
        assert_eq!(entry(1, import).seal_with(machine), want.sign());
    }

    #[test]
    fn a_change_to_any_sealed_field_breaks_the_seal() {
        let base = tempfile::tempdir().unwrap();
        let (_dir, store) = project_store(base.path());
        let machine = store.machine().unwrap();
        let entry = Entry {
            id: 4,
            at_ms: 1,
            op: full_move(),
            seal: String::new(),
        };
        let seal = entry.seal_with(machine);
        let alterations: Vec<fn(&mut Entry)> = vec![
            |e| e.id = 5,
            |e| {
                if let Op::Move { moves, .. } = &mut e.op {
                    moves[1].1 = "D".to_owned();
                }
            },
            |e| {
                if let Op::Move { docs, .. } = &mut e.op {
                    docs[0].edits[1].old = "x".to_owned();
                }
            },
            |e| {
                if let Op::Move { docs, .. } = &mut e.op {
                    docs[0].edits.pop();
                }
            },
            |e| {
                if let Op::Move { docs, .. } = &mut e.op {
                    docs[0].moved_by = None;
                }
            },
            |e| {
                if let Op::Move { docs, .. } = &mut e.op {
                    docs[0].causes = vec![1, 0];
                }
            },
        ];
        for (n, alter) in alterations.into_iter().enumerate() {
            let mut changed = entry.clone();
            alter(&mut changed);
            assert_ne!(changed.seal_with(machine), seal, "change {n}");
        }
        // when the entry happened is only shown
        let mut later = entry.clone();
        later.at_ms = 99;
        assert_eq!(later.seal_with(machine), seal);
    }

    #[test]
    fn marks_keep_the_folders_used_last() {
        let mut marks = Marks::default();
        for n in 0..MAX_MARK_ROWS + 5 {
            marks.touch(&format!("r{n}")).next = 1;
        }
        assert_eq!(marks.rows.len(), MAX_MARK_ROWS);
        assert!(marks.row("r4").is_none() && marks.row("r5").is_some());
        // used again: last, so it outlives the others
        marks.touch("r5").live.push(3);
        assert_eq!(marks.rows.last().map(|r| r.rid.as_str()), Some("r5"));
        marks.touch("new");
        assert!(marks.row("r6").is_none());
        assert_eq!(marks.row("r5").map(|r| r.live.clone()), Some(vec![3]));
    }

    /// A move that rewrote links in many documents makes a large journal: one of about 9 MiB still loads.
    #[test]
    fn a_large_journal_still_loads() {
        let base = tempfile::tempdir().unwrap();
        let store = crate::state::StateStore::app(&crate::state::AppDirs::under(base.path()));
        let doc = |n: usize| DocUndo {
            path: format!("docs/{n}.md"),
            original: format!("docs/{n}.md"),
            hash_after: "0123456789abcdef".to_owned(),
            edits: vec![UndoEdit {
                start: 0,
                end: 4,
                old: "x".repeat(4096),
            }],
            causes: vec![0],
            moved_by: None,
        };
        let mut journal = Journal::default();
        journal.push(
            1,
            Op::Move {
                moves: vec![("a".to_owned(), "b".to_owned())],
                docs: (0..2300).map(doc).collect(),
            },
        );
        let file = store.file::<Journal>(JOURNAL_FILE);
        file.save(&journal).unwrap();
        let len = std::fs::metadata(file.path()).unwrap().len();
        assert!(len > 9 * 1024 * 1024, "{len}");
        assert_eq!(file.load(), crate::state::Loaded::Found(journal));
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
