#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::field_reassign_with_default,
    clippy::assert_is_empty
)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::handoff::{scope, sessions, Case};
use super::net::{net, net_outside, verify, Disk, Flag, Kind, NetOptions, TOP};
use super::*;
use crate::paths::Root;
use crate::state::AppDirs;
use crate::watch::{Change, ChangeKind, FsBatch};

const T0: u64 = 1_760_000_000_000;
const MIN: u64 = 60 * 1000;

/// A clock the test moves.
struct Time(Arc<AtomicU64>);

impl Time {
    fn new() -> Self {
        Self(Arc::new(AtomicU64::new(T0)))
    }
    fn clock(&self) -> Clock {
        let t = Arc::clone(&self.0);
        Arc::new(move || t.load(Ordering::SeqCst))
    }
    fn pass(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

fn store() -> (tempfile::TempDir, StateStore) {
    store_at(false)
}

/// A project's store: its state in the profile, or (`initialized`) in its own `.permark/local/`.
fn store_at(initialized: bool) -> (tempfile::TempDir, StateStore) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("p");
    std::fs::create_dir_all(&project).unwrap();
    if initialized {
        std::fs::create_dir(project.join(".permark")).unwrap();
    }
    let root = Root::open(&project).unwrap();
    let store = StateStore::for_project(&AppDirs::under(&dir.path().join("state")), &root);
    (dir, store)
}

fn file(op: FileOp, path: &str, from: Option<&str>) -> Fact {
    Fact::File {
        op,
        path: path.to_owned(),
        from: from.map(str::to_owned),
        undo: false,
    }
}

fn save(path: &str, sections: &[&str]) -> Fact {
    Fact::Save {
        path: path.to_owned(),
        sections: sections.iter().map(|s| (*s).to_owned()).collect(),
        blocks: 1,
    }
}

fn tag(added: bool, path: &str, tag: &str) -> Fact {
    Fact::Tag {
        added,
        path: path.to_owned(),
        tag: tag.to_owned(),
    }
}

fn comment(op: CommentOp, path: &str, id: &str) -> Fact {
    Fact::Comment {
        op,
        path: path.to_owned(),
        id: id.to_owned(),
        heading: Some("Plan".to_owned()),
    }
}

fn entries(facts: Vec<Fact>) -> Vec<Entry> {
    facts
        .into_iter()
        .zip(1_u64..)
        .map(|(fact, seq)| Entry {
            seq,
            at: T0 + seq * MIN,
            by: if matches!(fact, Fact::Outside { .. }) {
                By::Outside
            } else {
                By::Person
            },
            fact,
        })
        .collect()
}

fn kinds(rows: &[net::Row]) -> Vec<(String, Kind)> {
    rows.iter()
        .map(|r| (r.path.clone(), r.kind.clone()))
        .collect()
}

// ---- coalescing table ---------------------------------------------------------------------------------------------

#[test]
fn created_then_deleted_is_nothing() {
    let rows = net(
        &entries(vec![
            file(FileOp::New, "a.md", None),
            save("a.md", &["A"]),
            tag(true, "a.md", "x"),
            file(FileOp::Delete, "a.md", None),
        ]),
        NetOptions::default(),
    );
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn a_rename_chain_is_one_rename_and_back_is_nothing() {
    let rows = net(
        &entries(vec![
            file(FileOp::Move, "b.md", Some("a.md")),
            file(FileOp::Move, "c.md", Some("b.md")),
        ]),
        NetOptions::default(),
    );
    assert_eq!(
        kinds(&rows),
        [(
            "c.md".to_owned(),
            Kind::Renamed {
                from: "a.md".to_owned()
            }
        )]
    );
    let back = net(
        &entries(vec![
            file(FileOp::Move, "b.md", Some("a.md")),
            file(FileOp::Move, "a.md", Some("b.md")),
        ]),
        NetOptions::default(),
    );
    assert!(back.is_empty(), "{back:?}");
    let moved = net(
        &entries(vec![file(FileOp::Move, "docs/a.md", Some("a.md"))]),
        NetOptions::default(),
    );
    assert!(matches!(moved[0].kind, Kind::Moved { .. }));
}

#[test]
fn many_saves_are_one_edit_with_the_union_of_sections_and_follow_a_rename() {
    let rows = net(
        &entries(vec![
            save("a.md", &["Plan > Goals"]),
            save("a.md", &["Plan > Risks", "Plan > Goals"]),
            file(FileOp::Move, "b.md", Some("a.md")),
            save("b.md", &[TOP]),
        ]),
        NetOptions::default(),
    );
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(
        rows[1].kind,
        Kind::Edited {
            sections: vec![
                "Plan > Goals".to_owned(),
                "Plan > Risks".to_owned(),
                TOP.to_owned()
            ],
            blocks: 3
        }
    );
    assert_eq!(rows[1].path, "b.md");
    assert_eq!(rows[1].seq, 4, "the row stands for its last entry");
}

#[test]
fn undone_operations_go() {
    let undo = |op, path: &str, from: Option<&str>| Fact::File {
        op,
        path: path.to_owned(),
        from: from.map(str::to_owned),
        undo: true,
    };
    let rows = net(
        &entries(vec![
            file(FileOp::Delete, "old.md", None),
            undo(FileOp::Restore, "old.md", None),
            file(FileOp::New, "n.md", None),
            undo(FileOp::Delete, "n.md", None),
            file(FileOp::Move, "x/m.md", Some("m.md")),
            undo(FileOp::Move, "m.md", Some("x/m.md")),
            file(FileOp::Import, "pic.png", None),
            undo(FileOp::Delete, "pic.png", None),
        ]),
        NetOptions::default(),
    );
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn a_new_file_with_edits_is_new_and_a_deleted_one_is_deleted_at_its_first_path() {
    let rows = net(
        &entries(vec![
            file(FileOp::New, "n.md", None),
            save("n.md", &["N"]),
            file(FileOp::Move, "b.md", Some("a.md")),
            save("b.md", &["B"]),
            file(FileOp::Delete, "b.md", None),
        ]),
        NetOptions::default(),
    );
    assert_eq!(
        kinds(&rows),
        [
            ("n.md".to_owned(), Kind::New),
            ("a.md".to_owned(), Kind::Deleted)
        ]
    );
}

#[test]
fn comments_added_and_done_are_omitted_unless_asked_and_toggles_cancel() {
    let facts = vec![
        comment(CommentOp::Add, "a.md", "c1"),
        comment(CommentOp::Done, "a.md", "c1"),
        comment(CommentOp::Add, "a.md", "c2"),
        comment(CommentOp::Reply, "a.md", "c2"),
        comment(CommentOp::Done, "a.md", "c3"),
        comment(CommentOp::Reopen, "a.md", "c3"),
        comment(CommentOp::Done, "a.md", "c4"),
        comment(CommentOp::Reply, "a.md", "c5"),
        comment(CommentOp::Add, "a.md", "c6"),
        comment(CommentOp::Remove, "a.md", "c6"),
    ];
    let rows = net(&entries(facts.clone()), NetOptions::default());
    let ops: Vec<(String, CommentOp)> = rows
        .iter()
        .filter_map(|r| match &r.kind {
            Kind::Comment { op, id, .. } => Some((id.clone(), *op)),
            _ => None,
        })
        .collect();
    assert_eq!(
        ops,
        [
            ("c2".to_owned(), CommentOp::Add),
            ("c4".to_owned(), CommentOp::Done),
            ("c5".to_owned(), CommentOp::Reply)
        ]
    );
    let asked = net(
        &entries(facts),
        NetOptions {
            include_done_comments: true,
        },
    );
    assert!(asked
        .iter()
        .any(|r| matches!(&r.kind, Kind::Comment { id, op: CommentOp::Done, .. } if id == "c1")));
}

#[test]
fn a_tag_added_and_removed_is_nothing() {
    let rows = net(
        &entries(vec![
            tag(true, "a.md", "x"),
            tag(false, "a.md", "x"),
            tag(false, "a.md", "y"),
            tag(true, "a.md", "z"),
            tag(true, "a.md", "z"),
        ]),
        NetOptions::default(),
    );
    assert_eq!(
        kinds(&rows),
        [
            (
                "a.md".to_owned(),
                Kind::Tag {
                    added: false,
                    tag: "y".to_owned()
                }
            ),
            (
                "a.md".to_owned(),
                Kind::Tag {
                    added: true,
                    tag: "z".to_owned()
                }
            )
        ]
    );
}

#[test]
fn reviews_keep_the_last_choice_per_section_and_converts_are_kept() {
    let rows = net(
        &entries(vec![
            Fact::Review {
                op: ReviewOp::Keep,
                path: "a.md".to_owned(),
                section: Some("A".to_owned()),
            },
            Fact::Review {
                op: ReviewOp::Revert,
                path: "a.md".to_owned(),
                section: Some("A".to_owned()),
            },
            Fact::Convert {
                path: "d.json".to_owned(),
                to: "toon".to_owned(),
                saved: Some("d.toon".to_owned()),
            },
            Fact::Convert {
                path: "d.json".to_owned(),
                to: "toon".to_owned(),
                saved: Some("d.toon".to_owned()),
            },
        ]),
        NetOptions::default(),
    );
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(
        rows[0].kind,
        Kind::Review {
            op: ReviewOp::Revert,
            section: Some("A".to_owned())
        }
    );
}

#[test]
fn outside_changes_net_per_file() {
    let o = |change, path: &str, from: Option<&str>| Fact::Outside {
        change,
        path: path.to_owned(),
        from: from.map(str::to_owned),
    };
    let rows = net_outside(&entries(vec![
        o(OutsideChange::Created, "tmp.md", None),
        o(OutsideChange::Deleted, "tmp.md", None),
        o(OutsideChange::Modified, "a.md", None),
        o(OutsideChange::Modified, "a.md", None),
        o(OutsideChange::Renamed, "c.md", Some("b.md")),
        o(OutsideChange::Renamed, "d.md", Some("c.md")),
        o(OutsideChange::Deleted, "e.md", None),
        o(OutsideChange::Renamed, "g.md", Some("f.md")),
        o(OutsideChange::Renamed, "f.md", Some("g.md")),
    ]));
    let got: Vec<(String, OutsideChange)> = rows
        .iter()
        .map(|r| match &r.kind {
            Kind::Outside { change, .. } => (r.path.clone(), *change),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        got,
        [
            ("a.md".to_owned(), OutsideChange::Modified),
            ("d.md".to_owned(), OutsideChange::Renamed),
            ("e.md".to_owned(), OutsideChange::Deleted),
        ]
    );
}

// ---- verification -------------------------------------------------------------------------------------------------

#[derive(Default)]
struct FakeDisk {
    files: HashMap<String, (Vec<String>, u64)>,
    done: Vec<String>,
}

impl Disk for FakeDisk {
    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }
    fn headings(&self, path: &str) -> Option<Vec<String>> {
        self.files.get(path).map(|(h, _)| h.clone())
    }
    fn modified(&self, path: &str) -> Option<u64> {
        self.files.get(path).map(|(_, m)| *m)
    }
    fn comment_done(&self, id: &str) -> Option<bool> {
        Some(self.done.iter().any(|d| d == id))
    }
}

#[test]
fn verification_flags_gone_moved_section_gone_changed_since_and_drops_done_comments() {
    let mut journal = entries(vec![
        save("gone.md", &["A"]),
        save("moved.md", &["A"]),
        save("doc.md", &["Plan > Goals", "Plan > Old", TOP]),
        save("busy.md", &["A"]),
        comment(CommentOp::Add, "doc.md", "c1"),
        save("late.md", &["A"]),
    ]);
    let outside = |seq: u64, change, path: &str, from: Option<&str>| Entry {
        seq,
        at: T0 + seq * MIN,
        by: By::Outside,
        fact: Fact::Outside {
            change,
            path: path.to_owned(),
            from: from.map(str::to_owned),
        },
    };
    journal.push(outside(
        10,
        OutsideChange::Renamed,
        "elsewhere/moved.md",
        Some("moved.md"),
    ));
    journal.push(outside(11, OutsideChange::Modified, "busy.md", None));
    let mut disk = FakeDisk::default();
    let plain = |m| (vec!["A".to_owned()], m);
    disk.files
        .insert("elsewhere/moved.md".to_owned(), plain(T0));
    disk.files.insert(
        "doc.md".to_owned(),
        (vec!["Plan".to_owned(), "Plan > Goals".to_owned()], T0),
    );
    disk.files.insert("busy.md".to_owned(), plain(T0));
    // the file's time is well after the save: changed since (the app was closed: no watch entry)
    disk.files
        .insert("late.md".to_owned(), plain(T0 + 60 * MIN));
    disk.done.push("c1".to_owned());
    let rows = verify(
        net(&journal, NetOptions::default()),
        &disk,
        &journal,
        NetOptions::default(),
    );
    let flags: HashMap<String, Vec<Flag>> = rows
        .iter()
        .map(|r| (r.path.clone(), r.flags.clone()))
        .collect();
    assert_eq!(flags["gone.md"], [Flag::Gone]);
    assert_eq!(
        flags["moved.md"],
        [Flag::MovedTo("elsewhere/moved.md".to_owned())]
    );
    assert_eq!(
        flags["doc.md"],
        [Flag::SectionGone("Plan > Old".to_owned())]
    );
    assert_eq!(flags["busy.md"], [Flag::ChangedSince]);
    assert_eq!(flags["late.md"], [Flag::ChangedSince]);
    assert!(
        !rows.iter().any(|r| matches!(r.kind, Kind::Comment { .. })),
        "a comment already done is omitted"
    );
}

// ---- journal --------------------------------------------------------------------------------------------------------

fn change(path: &str, kind: ChangeKind, own: bool) -> Change {
    Change {
        path: path.to_owned(),
        kind,
        is_dir: false,
        own,
    }
}

fn batch(changes: Vec<Change>) -> FsBatch {
    FsBatch {
        changes,
        bulk: false,
        rescan: false,
        moved: Vec::new(),
    }
}

#[test]
fn the_journal_notes_outside_changes_only_merges_them_and_never_its_own_files() {
    let (_dir, store) = store();
    let time = Time::new();
    let log = ActivityLog::open(&store, time.clock());
    log.outside(&batch(vec![
        change("a.md", ChangeKind::Created, false),
        change("a.md", ChangeKind::Modified, false),
        change("own.md", ChangeKind::Modified, true),
        change(".permark/local/activity.json", ChangeKind::Modified, false),
        change(".git/index", ChangeKind::Modified, false),
        change("t.md", ChangeKind::Created, false),
        change("t.md", ChangeKind::Removed, false),
    ]));
    let journal = log.snapshot();
    let facts: Vec<&Fact> = journal.entries.iter().map(|e| &e.fact).collect();
    assert_eq!(
        facts,
        [&Fact::Outside {
            change: OutsideChange::Created,
            path: "a.md".to_owned(),
            from: None
        }]
    );
    // an action of the person ends the merging: a later change is a new entry
    log.person(save("a.md", &["A"]));
    log.outside(&batch(vec![change("a.md", ChangeKind::Modified, false)]));
    assert_eq!(log.snapshot().entries.len(), 3);
}

#[test]
fn the_journal_is_written_and_read_back() {
    let (_dir, store) = store();
    let time = Time::new();
    {
        let log = ActivityLog::open(&store, time.clock());
        log.person(save("a.md", &["A"]));
        log.closed();
        log.closed();
    }
    let log = ActivityLog::open(&store, time.clock());
    let journal = log.snapshot();
    assert_eq!(journal.entries.len(), 2, "one close marker: {journal:?}");
    assert_eq!(journal.next, 3);
}

#[test]
fn an_older_format_is_kept_aside_and_noted_until_the_next_hand_off() {
    let (_dir, store) = store();
    let path = store.file::<Journal>(JOURNAL_FILE).path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{"version":0,"data":{"old":[1,2,3]}}"#).unwrap();
    let time = Time::new();
    let log = ActivityLog::open(&store, time.clock());
    assert_eq!(log.snapshot().upgraded_from, Some(0));
    let kept = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().contains(".broken-"));
    assert!(kept, "the old entries are kept aside");
    log.person(save("a.md", &["A"]));
    assert!(scope(&log.snapshot(), time.0.load(Ordering::SeqCst)).upgraded);
    let _ = log.hand_off(2, None, "x".to_owned(), 1);
    assert_eq!(log.snapshot().upgraded_from, None);
}

/// An own journal of today (rows, rows dropped by retention, a hand-off's checkpoint + package, outside rows, a close)
/// loads exactly as it was written, and opening it without new rows writes nothing.
#[test]
fn an_own_journal_loads_unchanged_and_opening_it_writes_nothing() {
    let (_dir, store) = store();
    let time = Time::new();
    let before = {
        let log = ActivityLog::open(&store, time.clock());
        log.person(save("old.md", &["A"]));
        time.pass(KEEP_MS + MIN);
        log.person(save("a.md", &["A"]));
        let preview = scope(&log.snapshot(), log.now());
        let _ = log.hand_off(preview.upto, Some("abc".to_owned()), "rows".to_owned(), 1);
        log.outside(&batch(vec![change("b.md", ChangeKind::Created, false)]));
        log.closed();
        log.snapshot()
    };
    assert_eq!(before.trimmed_through, 1);
    assert_eq!(before.checkpoint.as_ref().map(|c| c.seq), Some(2));
    assert_eq!(before.package.as_ref().map(|p| p.seq), Some(3));
    assert_eq!(before.next, 6);
    let path = store.file::<Journal>(JOURNAL_FILE).path().to_path_buf();
    let bytes = std::fs::read(&path).unwrap();
    {
        let log = ActivityLog::open(&store, time.clock());
        assert_eq!(log.snapshot(), before);
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

/// A row of a planted journal, as the file holds it.
fn planted_row(seq: u64) -> serde_json::Value {
    serde_json::to_value(Entry {
        seq,
        at: T0,
        by: By::Person,
        fact: save("a.md", &["A"]),
    })
    .unwrap()
}

/// Puts `data` in place of the journal of `store` (today's format version); returns the journal's path.
fn plant(store: &StateStore, data: &serde_json::Value) -> std::path::PathBuf {
    let path = store.file::<Journal>(JOURNAL_FILE).path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let envelope = serde_json::json!({ "version": Journal::VERSION, "data": data });
    std::fs::write(&path, envelope.to_string()).unwrap();
    path
}

/// Whether a file moved aside sits next to `path`.
fn kept_aside(path: &std::path::Path) -> bool {
    std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().contains(".broken-"))
}

/// Any seq-bearing number above what a journal reaches (a planted file), or a `next` | row that leaves no seq for a
/// new row (every later push would be refused: the journal would record nothing again): moved aside like another
/// format version, a fresh journal starts and gives seqs from 1 (no panic, no wrap).
#[test]
fn a_journal_with_a_number_past_the_highest_seq_is_kept_aside_and_a_fresh_one_starts() {
    let big = MAX_SEQ + 1;
    let row = planted_row;
    let cases = [
        serde_json::json!({ "next": u64::MAX, "entries": [row(1)] }),
        serde_json::json!({ "next": big, "entries": [row(1)] }),
        serde_json::json!({ "next": MAX_SEQ, "entries": [] }),
        serde_json::json!({ "next": MAX_SEQ, "entries": [row(1)] }),
        serde_json::json!({ "next": 3, "entries": [row(1), row(MAX_SEQ)] }),
        serde_json::json!({ "next": 3, "entries": [row(1), row(MAX_SEQ - 1)] }),
        serde_json::json!({ "next": 3, "entries": [row(1), row(big)] }),
        serde_json::json!({ "next": 3, "entries": [row(1)], "trimmed_through": big }),
        serde_json::json!({ "next": 3, "entries": [row(1)], "checkpoint": { "seq": big, "at": T0 } }),
        serde_json::json!({
            "next": 3,
            "entries": [row(1)],
            "package": { "seq": big, "at": T0, "text": "rows", "rows": 1 }
        }),
    ];
    for data in cases {
        let (_dir, store) = store();
        let path = plant(&store, &data);
        let time = Time::new();
        let log = ActivityLog::open(&store, time.clock());
        assert_eq!(log.snapshot(), Journal::default(), "{data}");
        assert!(kept_aside(&path), "{data}");
        log.person(save("a.md", &["A"]));
        assert_eq!(log.snapshot().entries[0].seq, 1, "{data}");
    }
}

/// Once the last seq is given a new row is refused (a change outside, a hand-off): nothing wraps, no checkpoint |
/// package is noted without its marker. The next load starts a fresh journal (the file kept aside), so the journal
/// records again.
#[test]
fn a_journal_out_of_seqs_refuses_new_rows_then_starts_fresh_at_the_next_load() {
    let (_dir, store) = store();
    let path = plant(
        &store,
        &serde_json::json!({ "next": MAX_SEQ - 1, "entries": [] }),
    );
    let time = Time::new();
    {
        let log = ActivityLog::open(&store, time.clock());
        log.person(save("a.md", &["A"]));
        log.outside(&batch(vec![change("b.md", ChangeKind::Created, false)]));
        assert_eq!(log.hand_off(MAX_SEQ - 1, None, "rows".to_owned(), 1), None);
        let journal = log.snapshot();
        let seqs: Vec<u64> = journal.entries.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, [MAX_SEQ - 1], "{journal:?}");
        assert_eq!(journal.next, MAX_SEQ);
        assert_eq!(journal.checkpoint, None);
        assert_eq!(journal.package, None);
    }
    let time = Time::new();
    let log = ActivityLog::open(&store, time.clock());
    assert_eq!(log.snapshot(), Journal::default());
    assert!(kept_aside(&path));
    log.person(save("a.md", &["A"]));
    assert_eq!(log.snapshot().entries[0].seq, 1);
}

/// Every seq a journal gives is checked: from `next` at the top of `u64` or at the highest seq nothing is added (no
/// panic, no wrap); below it rows go on up to it, and a merged outside row out of seqs leaves the earlier row in place.
#[test]
fn a_journal_out_of_seqs_adds_nothing_and_never_wraps() {
    for next in [u64::MAX, MAX_SEQ] {
        let mut journal = Journal {
            next,
            ..Journal::default()
        };
        assert_eq!(journal.push(T0, By::Person, save("a.md", &["A"])), None);
        assert!(journal.entries.is_empty());
        assert_eq!(journal.next, next);
    }
    let mut journal = Journal {
        next: MAX_SEQ - 1,
        ..Journal::default()
    };
    journal.push_outside(T0, OutsideChange::Created, "a.md", None);
    assert_eq!(journal.next, MAX_SEQ);
    journal.push_outside(T0, OutsideChange::Modified, "a.md", None);
    assert_eq!(journal.entries.len(), 1);
    assert_eq!(journal.entries[0].seq, MAX_SEQ - 1);
    // a created + deleted pair needs no new row: it still nets to nothing
    journal.push_outside(T0, OutsideChange::Deleted, "a.md", None);
    assert!(journal.entries.is_empty());
}

/// `next` behind the rows (a planted file) is raised past the last one: a new row never repeats a seq.
#[test]
fn a_next_behind_the_rows_is_raised_and_a_new_row_never_repeats_a_seq() {
    let (_dir, store) = store();
    plant(
        &store,
        &serde_json::json!({ "next": 2, "entries": [planted_row(5)] }),
    );
    let time = Time::new();
    let log = ActivityLog::open(&store, time.clock());
    assert_eq!(log.snapshot().next, 6);
    log.person(save("b.md", &["B"]));
    let seqs: Vec<u64> = log.snapshot().entries.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, [5, 6]);
}

/// A checkpoint | package ahead of every seq the journal gave (a planted file) is brought back to the last one given:
/// new rows come after the hand-off, never count as handed off already.
#[test]
fn a_checkpoint_ahead_of_the_rows_is_brought_back_and_new_rows_follow_it() {
    let (_dir, store) = store();
    let far = 1_000_000_000_000_u64;
    plant(
        &store,
        &serde_json::json!({
            "next": 3,
            "entries": [planted_row(1), planted_row(2)],
            "checkpoint": { "seq": far, "at": T0 },
            "package": { "seq": far, "at": T0, "text": "rows", "rows": 1 }
        }),
    );
    let time = Time::new();
    let log = ActivityLog::open(&store, time.clock());
    let journal = log.snapshot();
    assert_eq!(journal.checkpoint.as_ref().map(|c| c.seq), Some(2));
    assert_eq!(journal.package.as_ref().map(|p| p.seq), Some(2));
    log.person(save("b.md", &["B"]));
    let journal = log.snapshot();
    assert_eq!(journal.since_checkpoint().len(), 1);
    assert_eq!(scope(&journal, log.now()).case, Case::One);
}

/// A trimmed mark ahead of every seq the journal gave (a planted file) is brought back to the last one given: rows
/// after the hand-off never count as dropped by retention.
#[test]
fn a_trimmed_mark_ahead_of_the_rows_is_brought_back() {
    let (_dir, store) = store();
    let far = 1_000_000_000_000_u64;
    plant(
        &store,
        &serde_json::json!({
            "next": 3,
            "entries": [planted_row(1), planted_row(2)],
            "trimmed_through": far,
            "checkpoint": { "seq": 2, "at": T0 }
        }),
    );
    let time = Time::new();
    let log = ActivityLog::open(&store, time.clock());
    assert_eq!(log.snapshot().trimmed_through, 2);
    log.person(save("b.md", &["B"]));
    assert!(!scope(&log.snapshot(), log.now()).trimmed);
}

/// A journal file over the journal's cap is never read, not even for its version: unreadable, left in place, never
/// written over.
#[test]
fn a_journal_over_its_cap_is_never_read_and_never_written_over() {
    let (_dir, store) = store();
    let path = store.file::<Journal>(JOURNAL_FILE).path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let head = r#"{"version":0,"data":{}}"#;
    let pad = usize::try_from(Journal::MAX_BYTES).unwrap() + 1 - head.len();
    std::fs::write(&path, format!("{head}{}", " ".repeat(pad))).unwrap();
    let time = Time::new();
    {
        let log = ActivityLog::open(&store, time.clock());
        assert_eq!(log.snapshot(), Journal::default());
        log.person(save("a.md", &["A"]));
    }
    assert_eq!(
        std::fs::metadata(&path).unwrap().len(),
        Journal::MAX_BYTES + 1
    );
    assert!(!kept_aside(&path));
}

/// A link in place of the journal in `.permark/` is never followed, not even for its version: unreadable, left as it
/// is, its target untouched.
#[cfg(unix)]
#[test]
fn a_link_in_place_of_the_journal_is_never_followed() {
    let (_dir, store) = store_at(true);
    let path = store.file::<Journal>(JOURNAL_FILE).path().to_path_buf();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("activity.json");
    let old = r#"{"version":0,"data":{}}"#;
    std::fs::write(&target, old).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let time = Time::new();
    {
        let log = ActivityLog::open(&store, time.clock());
        assert_eq!(log.snapshot(), Journal::default());
        log.person(save("a.md", &["A"]));
    }
    assert!(std::fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), old);
}

/// A FIFO in place of the journal (profile or `.permark/`) never makes the project's open wait: unreadable, left as it
/// is.
#[cfg(unix)]
#[test]
fn a_fifo_in_place_of_the_journal_never_makes_the_open_wait() {
    use std::os::unix::fs::FileTypeExt;
    for initialized in [false, true] {
        let (_dir, store) = store_at(initialized);
        let path = store.file::<Journal>(JOURNAL_FILE).path().to_path_buf();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let made = std::process::Command::new("mkfifo").arg(&path).status();
        assert!(made.is_ok_and(|s| s.success()), "mkfifo {}", path.display());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let log = ActivityLog::open(&store, Time::new().clock());
            let _ = tx.send(log.snapshot());
        });
        let journal = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the open waited on a FIFO");
        assert_eq!(journal, Journal::default(), "initialized: {initialized}");
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_fifo());
    }
}

#[test]
fn retention_drops_old_entries_then_outside_ones_first() {
    let (_dir, store) = store();
    let time = Time::new();
    let log = ActivityLog::open(&store, time.clock());
    log.person(save("old.md", &["A"]));
    time.pass(KEEP_MS + MIN);
    log.person(save("new.md", &["A"]));
    let journal = log.snapshot();
    assert_eq!(journal.entries.len(), 1);
    assert_eq!(journal.trimmed_through, 1);
    // over the count: the changes outside go before the person's actions
    for i in 0..KEEP_ENTRIES {
        log.outside(&batch(vec![change(
            &format!("f{i}.md"),
            ChangeKind::Created,
            false,
        )]));
    }
    log.person(save("last.md", &["A"]));
    let journal = log.snapshot();
    assert_eq!(journal.entries.len(), KEEP_ENTRIES);
    assert!(journal
        .entries
        .iter()
        .any(|e| e.fact.path() == Some("new.md")));
}

#[test]
fn listeners_hear_entries_until_dropped() {
    let (_dir, store) = store();
    let log = ActivityLog::open(&store, Time::new().clock());
    let heard = Arc::new(Mutex::new(Vec::new()));
    let listening = log.listen({
        let heard = Arc::clone(&heard);
        Arc::new(move |e: &Entry| heard.lock().unwrap().push(e.seq))
    });
    log.person(save("a.md", &["A"]));
    drop(listening);
    log.person(save("b.md", &["A"]));
    assert_eq!(*heard.lock().unwrap(), [1]);
}

// ---- sessions and the default scope (S1..S7) ----------------------------------------------------------------------

/// A journal with the person's actions at the given minutes after `T0`.
fn journal_at(minutes: &[u64]) -> Journal {
    let mut journal = Journal::default();
    for m in minutes {
        journal.push(T0 + m * MIN, By::Person, save("a.md", &["A"]));
    }
    journal
}

const H: u64 = 60;
const D: u64 = 24 * 60;

#[test]
fn sessions_split_at_two_hours_and_at_a_close() {
    let mut journal = journal_at(&[0, 30, 30 + 2 * H, 30 + 4 * H + 1]);
    assert_eq!(
        sessions(&journal.entries).len(),
        2,
        "a gap of exactly 2 h keeps the session"
    );
    journal.push(T0 + (30 + 5 * H) * MIN, By::Person, Fact::Closed);
    journal.push(
        T0 + (30 + 5 * H + 1) * MIN,
        By::Person,
        save("a.md", &["A"]),
    );
    assert_eq!(sessions(&journal.entries).len(), 3);
}

#[test]
fn s1_one_session_since_the_last_hand_off() {
    let mut journal = journal_at(&[0, 10]);
    journal.checkpoint = Some(Checkpoint {
        seq: 0,
        at: T0,
        head: None,
    });
    let s = scope(&journal, T0 + 20 * MIN);
    assert_eq!(s.case, Case::One);
    assert!(s.sessions[0].checked);
    assert_eq!(s.upto, 2);
}

#[test]
fn s2_several_sessions_none_stale_all_checked() {
    let mut journal = journal_at(&[0, 5 * H, 2 * D]);
    journal.checkpoint = Some(Checkpoint {
        seq: 0,
        at: T0,
        head: None,
    });
    let s = scope(&journal, T0 + 3 * D * MIN);
    assert_eq!(s.case, Case::Several);
    assert_eq!(s.sessions.len(), 3);
    assert!(s.sessions.iter().all(|x| x.checked && !x.stale));
}

#[test]
fn s3_stale_sessions_unchecked_and_all_stale_checks_the_latest() {
    let mut journal = journal_at(&[0, 20 * D]);
    journal.checkpoint = Some(Checkpoint {
        seq: 0,
        at: T0,
        head: None,
    });
    let s = scope(&journal, T0 + 21 * D * MIN);
    assert_eq!(s.case, Case::Stale);
    assert!(s.sessions[0].stale && !s.sessions[0].checked);
    assert!(!s.sessions[1].stale && s.sessions[1].checked);
    assert!(!s.all_stale);
    // every session stale: the latest is checked anyway, with the note
    let all = scope(&journal, T0 + 40 * D * MIN);
    assert_eq!(all.case, Case::Stale);
    assert!(all.all_stale);
    assert!(!all.sessions[0].checked && all.sessions[1].checked);
    // exactly 7 days is stale
    let edge = scope(&journal, T0 + (20 * D + 7 * D) * MIN);
    assert!(edge.sessions[1].stale);
}

#[test]
fn s4_outside_only_and_s5_nothing() {
    let mut journal = Journal::default();
    journal.checkpoint = Some(Checkpoint {
        seq: 0,
        at: T0,
        head: None,
    });
    assert_eq!(scope(&journal, T0).case, Case::Nothing);
    journal.push_outside(T0, OutsideChange::Modified, "a.md", None);
    let s = scope(&journal, T0);
    assert_eq!(s.case, Case::OutsideOnly);
    assert_eq!(s.outside.len(), 1);
    // a hand-off marker alone is not news
    let mut marked = Journal::default();
    marked.push(T0, By::Person, Fact::HandOff { rows: 3 });
    marked.checkpoint = Some(Checkpoint {
        seq: 0,
        at: T0,
        head: None,
    });
    assert_eq!(scope(&marked, T0).case, Case::Nothing);
}

#[test]
fn s6_first_hand_off_is_anchored_on_the_latest_session() {
    // last used a month ago: the latest session is checked, the ones a week older than it are not
    let journal = journal_at(&[0, 10 * D, 12 * D]);
    let s = scope(&journal, T0 + 45 * D * MIN);
    assert_eq!(s.case, Case::First);
    let checked: Vec<bool> = s.sessions.iter().map(|x| x.checked).collect();
    assert_eq!(checked, [false, true, true]);
    assert!(!s.all_stale);
}

#[test]
fn s7_entries_trimmed_by_retention_are_said() {
    let mut journal = journal_at(&[0]);
    journal.trimmed_through = 4;
    assert!(
        scope(&journal, T0).trimmed,
        "first hand-off with trimmed entries"
    );
    journal.checkpoint = Some(Checkpoint {
        seq: 5,
        at: T0,
        head: None,
    });
    assert!(
        !scope(&journal, T0).trimmed,
        "trimmed before the checkpoint"
    );
    journal.trimmed_through = 6;
    assert!(scope(&journal, T0).trimmed);
}

#[test]
fn a_hand_off_covers_what_the_preview_saw_and_later_entries_wait() {
    let (_dir, store) = store();
    let time = Time::new();
    let log = ActivityLog::open(&store, time.clock());
    log.person(save("a.md", &["A"]));
    let preview = scope(&log.snapshot(), log.now());
    // the person keeps working while the dialog is open
    log.person(save("b.md", &["B"]));
    let _ = log.hand_off(preview.upto, Some("abc".to_owned()), "rows".to_owned(), 1);
    let journal = log.snapshot();
    let next = scope(&journal, log.now());
    assert_eq!(next.case, Case::One);
    assert_eq!(next.person.len(), 1);
    assert_eq!(next.person[0].fact.path(), Some("b.md"));
    assert_eq!(journal.package.as_ref().unwrap().text, "rows");
    assert_eq!(
        journal.checkpoint.as_ref().unwrap().head.as_deref(),
        Some("abc")
    );
}

// ---- facts from the core -------------------------------------------------------------------------------------------

fn edit(at: u32, remove: u32, insert: &[&str]) -> crate::edit::LineEdit {
    crate::edit::LineEdit {
        at,
        remove,
        insert: insert.iter().map(|s| (*s).to_owned()).collect(),
    }
}

#[test]
fn a_save_names_the_sections_its_changed_lines_are_in() {
    // the file after the save (CRLF kept by the file; the sections read any break)
    let text = "---\r\ntags: [a]\r\n---\r\nIntro\r\n# Plan\r\n\r\nNew line\r\n## Goals\r\n\r\nGoal\r\n## Risks\r\nRisk\r\n";
    // line 7 inserted (0-based 6 of the old text), line 10 changed (0-based 8 after the shift)
    let fact = save_fact(
        "docs/plan.md",
        text,
        &[edit(6, 0, &["New line"]), edit(8, 1, &["Goal"])],
    );
    assert_eq!(
        fact,
        Fact::Save {
            path: "docs/plan.md".to_owned(),
            sections: vec!["Plan".to_owned(), "Plan > Goals".to_owned()],
            blocks: 2
        }
    );
    // before the first heading; an edit spanning two sections; a deletion
    let fact = save_fact("a.md", text, &[edit(3, 1, &["Intro"])]);
    assert!(
        matches!(&fact, Fact::Save { sections, .. } if sections == &[TOP.to_owned()]),
        "{fact:?}"
    );
    let fact = save_fact("a.md", text, &[edit(9, 2, &["Goal", "## Risks"])]);
    assert!(
        matches!(&fact, Fact::Save { sections, .. } if sections == &["Plan > Goals".to_owned(), "Plan > Risks".to_owned()]),
        "{fact:?}"
    );
    let fact = save_fact("a.md", text, &[edit(5, 3, &[])]);
    assert!(
        matches!(&fact, Fact::Save { sections, blocks: 1, .. } if !sections.is_empty()),
        "{fact:?}"
    );
    // not Markdown: blocks only
    let fact = save_fact("config.toml", "a = 1\n", &[edit(0, 1, &["a = 2"])]);
    assert!(matches!(&fact, Fact::Save { sections, blocks: 1, .. } if sections.is_empty()));
    // odd input never panics
    let _ = save_fact(
        "a.md",
        "",
        &[edit(u32::MAX, u32::MAX, &["x"]), edit(0, 0, &[])],
    );
}

#[test]
fn file_operations_and_their_undo_reach_the_journal_and_net_to_nothing() {
    use crate::fileops::{MoveSpec, NewKind};
    use crate::plugins::PluginRegistry;
    use crate::settings::AppSettings;
    use crate::workspace::{CoreEnv, ProjectCore};

    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("p q");
    std::fs::create_dir_all(project.join("docs")).unwrap();
    std::fs::write(project.join("keep.md"), "# Keep\n").unwrap();
    let env = CoreEnv::new(
        AppDirs::under(&dir.path().join("state")),
        Arc::new(PluginRegistry::new(Vec::new()).unwrap()),
        Arc::new(crate::events::NoEvents),
        crate::conditions::Distribution::Direct,
        AppSettings::default(),
    );
    let core = ProjectCore::open(Root::open(&project).unwrap(), &env);
    let ops = core.fileops();
    let made = ops.create("", "new ü", NewKind::File).unwrap();
    let plan = ops
        .plan_move(
            &[MoveSpec {
                from: made.clone(),
                to: format!("docs/{made}"),
            }],
            false,
        )
        .unwrap();
    ops.apply_move(plan.plan_id, false).unwrap();
    ops.undo().unwrap();
    ops.undo().unwrap();
    let journal = core.activity().snapshot();
    let facts: Vec<(FileOp, bool)> = journal
        .entries
        .iter()
        .filter_map(|e| match &e.fact {
            Fact::File { op, undo, .. } => Some((*op, *undo)),
            _ => None,
        })
        .collect();
    assert_eq!(
        facts,
        [
            (FileOp::New, false),
            (FileOp::Move, false),
            (FileOp::Move, true),
            (FileOp::Delete, true)
        ]
    );
    assert!(net(&journal.entries, NetOptions::default()).is_empty());
    // a delete stays one
    ops.delete(&["keep.md".to_owned()]).ok();
    let rows = net(&core.activity().snapshot().entries, NetOptions::default());
    if std::fs::metadata(project.join("keep.md")).is_err() {
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, Kind::Deleted);
    }
    // closing the project ends the session
    let journal = Arc::clone(core.activity());
    drop(core);
    assert_eq!(
        journal.snapshot().entries.last().map(|e| &e.fact),
        Some(&Fact::Closed)
    );
}

/// CI 39814680202: facts noted before the projects worker told the owners (or while they change) reach the project's
/// own journal once told, in order, with their own time; a plain folder (single owner) mirrors nothing.
#[test]
fn facts_before_the_owners_are_known_reach_the_project_journal_once_told() {
    use crate::projects::{OwnerSlot, Owners, OwnersChange, ProjectUid};
    let (dir, store) = store();
    let member_dir = dir.path().join("p/sub a");
    std::fs::create_dir_all(&member_dir).unwrap();
    let member = StateStore::for_project(
        &AppDirs::under(&dir.path().join("state")),
        &Root::open(&member_dir).unwrap(),
    );
    let owners = Owners::new(
        store.clone(),
        vec![OwnerSlot::member(
            ProjectUid::parse("k7m2q7xd2a").unwrap(),
            "sub a",
            member.clone(),
        )],
    );
    let time = Time::new();
    let log = ActivityLog::open(&store, time.clock());
    let member_journal = || match member.file::<Journal>(JOURNAL_FILE).load() {
        crate::state::Loaded::Found(j) => j,
        _ => Journal::default(),
    };
    // not told yet: kept (and the opened folder's journal has it at once)
    log.person(tag(true, "sub a/plan.md", "spec"));
    log.person(tag(true, "top.md", "root"));
    log.flush();
    assert_eq!(log.snapshot().entries.len(), 2);
    assert!(member_journal().entries.is_empty());
    // told: routed, its path relative to the project
    log.follow_owners(&OwnersChange::Resume(&owners));
    log.flush();
    let facts: Vec<Fact> = member_journal()
        .entries
        .into_iter()
        .map(|e| e.fact)
        .collect();
    assert_eq!(facts, [tag(true, "plan.md", "spec")]);
    // while the owners change: kept again, then routed
    log.follow_owners(&OwnersChange::Pause);
    log.person(tag(true, "sub a/later.md", "x"));
    log.flush();
    assert_eq!(member_journal().entries.len(), 1);
    log.follow_owners(&OwnersChange::Resume(&owners));
    log.flush();
    assert_eq!(member_journal().entries.len(), 2);
    // a plain folder: what waited mirrors nowhere
    log.follow_owners(&OwnersChange::Pause);
    log.person(tag(true, "sub a/third.md", "y"));
    log.follow_owners(&OwnersChange::Resume(&Owners::root_only(store.clone())));
    log.flush();
    assert_eq!(member_journal().entries.len(), 2);
}

/// The opened folder `p` with one included project `sub a`: its owners, the folder's journal (owners not told yet)
/// and the project's own state.
fn one_member() -> (
    tempfile::TempDir,
    StateStore,
    crate::projects::Owners,
    ActivityLog,
    Time,
) {
    use crate::projects::{OwnerSlot, Owners, ProjectUid};
    let (dir, store) = store();
    let member_dir = dir.path().join("p/sub a");
    std::fs::create_dir_all(&member_dir).unwrap();
    let member = StateStore::for_project(
        &AppDirs::under(&dir.path().join("state")),
        &Root::open(&member_dir).unwrap(),
    );
    let owners = Owners::new(
        store.clone(),
        vec![OwnerSlot::member(
            ProjectUid::parse("k7m2q7xd2a").unwrap(),
            "sub a",
            member.clone(),
        )],
    );
    let time = Time::new();
    let log = ActivityLog::open(&store, time.clock());
    (dir, member, owners, log, time)
}

fn journal_of(store: &StateStore) -> Journal {
    match store.file::<Journal>(JOURNAL_FILE).load() {
        crate::state::Loaded::Found(j) => j,
        _ => Journal::default(),
    }
}

/// N4 (verifier 2026-10-25): while the owners are not known at most [`MAX_WAITING`] facts wait, the oldest go first;
/// the opened folder's journal keeps them all.
#[test]
fn at_most_the_newest_waiting_facts_reach_the_project_journal() {
    use crate::projects::OwnersChange;
    let (_dir, member, owners, log, _time) = one_member();
    let over = 5;
    for i in 0..MAX_WAITING + over {
        log.person(tag(true, &format!("sub a/f{i}.md"), "t"));
    }
    log.flush();
    assert_eq!(log.snapshot().entries.len(), MAX_WAITING + over);
    log.follow_owners(&OwnersChange::Resume(&owners));
    log.flush();
    let facts: Vec<Fact> = journal_of(&member)
        .entries
        .into_iter()
        .map(|e| e.fact)
        .collect();
    let newest: Vec<Fact> = (over..MAX_WAITING + over)
        .map(|i| tag(true, &format!("f{i}.md"), "t"))
        .collect();
    assert_eq!(facts, newest);
}

/// N4 (verifier 2026-10-25): a change made outside PerMark before the owners are known reaches the project's own
/// journal once told, its path relative to the project; one outside every project stays in the folder's journal.
#[test]
fn an_outside_change_before_the_owners_are_known_reaches_the_project_journal_once_told() {
    use crate::projects::OwnersChange;
    let (_dir, member, owners, log, _time) = one_member();
    log.outside(&batch(vec![
        change("sub a/x.md", ChangeKind::Created, false),
        change("top.md", ChangeKind::Modified, false),
    ]));
    log.flush();
    assert_eq!(log.snapshot().entries.len(), 2);
    assert!(journal_of(&member).entries.is_empty());
    log.follow_owners(&OwnersChange::Resume(&owners));
    log.flush();
    let facts: Vec<Fact> = journal_of(&member)
        .entries
        .into_iter()
        .map(|e| e.fact)
        .collect();
    assert_eq!(
        facts,
        [Fact::Outside {
            change: OutsideChange::Created,
            path: "x.md".to_owned(),
            from: None
        }]
    );
}

/// N4 (verifier 2026-10-25): across a Pause and a Resume the project's journal keeps the order and the time of every
/// fact: the ones before, the ones that waited (actions and outside changes mixed), the ones after.
#[test]
fn facts_keep_their_order_and_time_across_a_pause_and_a_resume() {
    use crate::projects::OwnersChange;
    let (_dir, member, owners, log, time) = one_member();
    log.follow_owners(&OwnersChange::Resume(&owners));
    log.person(tag(true, "sub a/a.md", "t"));
    time.pass(MIN);
    log.follow_owners(&OwnersChange::Pause);
    log.person(tag(true, "sub a/b.md", "t"));
    time.pass(MIN);
    log.outside(&batch(vec![change(
        "sub a/c.md",
        ChangeKind::Created,
        false,
    )]));
    time.pass(MIN);
    log.person(tag(true, "sub a/d.md", "t"));
    time.pass(MIN);
    log.flush();
    let before: Vec<Fact> = journal_of(&member)
        .entries
        .into_iter()
        .map(|e| e.fact)
        .collect();
    assert_eq!(before, [tag(true, "a.md", "t")]);
    log.follow_owners(&OwnersChange::Resume(&owners));
    log.person(tag(true, "sub a/e.md", "t"));
    log.flush();
    let entries: Vec<(u64, Fact)> = journal_of(&member)
        .entries
        .into_iter()
        .map(|e| (e.at, e.fact))
        .collect();
    assert_eq!(
        entries,
        [
            (T0, tag(true, "a.md", "t")),
            (T0 + MIN, tag(true, "b.md", "t")),
            (
                T0 + 2 * MIN,
                Fact::Outside {
                    change: OutsideChange::Created,
                    path: "c.md".to_owned(),
                    from: None
                }
            ),
            (T0 + 3 * MIN, tag(true, "d.md", "t")),
            (T0 + 4 * MIN, tag(true, "e.md", "t")),
        ]
    );
}

#[test]
fn a_batch_of_many_outside_changes_is_noted_in_one_pass() {
    // a checkout | an unzip reaches the journal as one batch on the watch thread, under the journal's lock
    let (_dir, store) = store();
    let log = ActivityLog::open(&store, Time::new().clock());
    let changes = (0..50_000)
        .map(|i| {
            change(
                &format!("d{}/f{i}.md", i / 100),
                ChangeKind::Modified,
                false,
            )
        })
        .collect();
    let started = std::time::Instant::now();
    log.outside(&batch(changes));
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    let journal = log.snapshot();
    assert_eq!(journal.entries.len(), KEEP_ENTRIES);
    assert_eq!(
        journal.entries.last().and_then(|e| e.fact.path()),
        Some("d499/f49999.md")
    );
}

impl Journal {
    /// One change outside merged on its own (a run of one).
    fn push_outside(&mut self, at: u64, change: OutsideChange, path: &str, from: Option<&str>) {
        self.outside_run().push(at, change, path, from);
    }

    /// Reference for the property check: each change searches the tail from its end and the row it replaces leaves at
    /// once.
    fn push_outside_one_by_one(
        &mut self,
        at: u64,
        change: OutsideChange,
        path: &str,
        from: Option<&str>,
    ) {
        let key = if change == OutsideChange::Renamed {
            from.unwrap_or(path)
        } else {
            path
        };
        let tail = self
            .entries
            .iter()
            .rposition(|e| e.by != By::Outside)
            .map_or(0, |i| i + 1);
        let found = self.entries[tail..]
            .iter()
            .rposition(|e| e.fact.path() == Some(key))
            .map(|i| i + tail);
        let Some(index) = found else {
            let fact = Fact::Outside {
                change,
                path: path.to_owned(),
                from: from.map(str::to_owned),
            };
            let _ = self.push(at, By::Outside, fact);
            return;
        };
        let Fact::Outside {
            change: before,
            from: before_from,
            ..
        } = self.entries[index].fact.clone()
        else {
            return;
        };
        let replaced = match merged_outside((before, before_from), change, key, path, from) {
            Some((change, path, from)) => self
                .push(at, By::Outside, Fact::Outside { change, path, from })
                .is_some(),
            None => true,
        };
        if replaced {
            self.entries.remove(index);
        }
    }
}

mod outside_runs {
    use proptest::prelude::*;

    use super::*;

    const PATHS: [&str; 4] = ["a.md", "b.md", "c.md", "d.md"];

    fn kind() -> impl Strategy<Value = OutsideChange> {
        prop_oneof![
            Just(OutsideChange::Created),
            Just(OutsideChange::Modified),
            Just(OutsideChange::Deleted),
            Just(OutsideChange::Renamed),
        ]
    }

    /// (change, path, from) over a few paths, so that changes meet the rows of earlier ones.
    fn outside_change() -> impl Strategy<Value = Item> {
        (kind(), 0..PATHS.len(), proptest::option::of(0..PATHS.len()))
    }

    /// (change, path index, from index)
    type Item = (OutsideChange, usize, Option<usize>);

    /// A row already in the journal: the person's (ends the tail), an outside one, or an outside row holding another
    /// kind of fact (a file from elsewhere: a change meeting it is dropped).
    fn row() -> impl Strategy<Value = (u8, Item)> {
        (0..6_u8, outside_change())
    }

    fn journal_of(next: u64, rows: &[(u8, Item)]) -> Journal {
        let mut journal = Journal {
            next,
            ..Journal::default()
        };
        for (what, (change, path, from)) in rows {
            let path = PATHS[*path];
            let (by, fact) = match what {
                0 => (By::Person, save(path, &["A"])),
                1 => (By::Outside, tag(true, path, "t")),
                _ => (
                    By::Outside,
                    Fact::Outside {
                        change: *change,
                        path: path.to_owned(),
                        from: from.map(|f| PATHS[f].to_owned()),
                    },
                ),
            };
            let _ = journal.push(T0, by, fact);
        }
        journal
    }

    proptest! {
        #[test]
        fn a_run_merges_exactly_as_each_change_on_its_own(
            rows in proptest::collection::vec(row(), 0..12),
            changes in proptest::collection::vec(outside_change(), 0..40),
            near_the_end in proptest::bool::ANY,
        ) {
            // near the end: the seqs run out inside the run
            let next = if near_the_end { MAX_SEQ - 20 } else { 1 };
            let mut one_by_one = journal_of(next, &rows);
            let mut runs = one_by_one.clone();
            for (change, path, from) in &changes {
                let from = from.map(|f| PATHS[f]);
                one_by_one.push_outside_one_by_one(T0 + MIN, *change, PATHS[*path], from);
            }
            {
                let mut run = runs.outside_run();
                for (change, path, from) in &changes {
                    run.push(T0 + MIN, *change, PATHS[*path], from.map(|f| PATHS[f]));
                }
            }
            prop_assert_eq!(runs, one_by_one);
        }
    }
}
