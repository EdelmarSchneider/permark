use super::*;

const NOTHING: [Entry; 0] = [];

fn doc(path: &str, current: Option<u64>, read: Option<u64>) -> DocState {
    DocState {
        path: path.to_owned(),
        current,
        read,
    }
}

fn saved(entries: &[(&str, u64)]) -> Saved {
    entries
        .iter()
        .map(|(p, h)| {
            (
                (*p).to_owned(),
                SavedDoc {
                    hash: *h,
                    deleted_at: None,
                },
            )
        })
        .collect()
}

fn entry(path: &str, status: Status) -> Entry {
    Entry {
        path: path.to_owned(),
        status,
    }
}

/// A book that has seen `docs` with the disk looked at.
fn seen(saved_state: &[(&str, u64)], docs: &[DocState]) -> Book {
    let mut book = Book::new(saved(saved_state));
    book.observe(&[], true, docs);
    book
}

#[test]
fn a_document_seen_for_the_first_time_is_approved_as_it_was_read() {
    let mut book = Book::new(BTreeMap::new());
    let changed = book.observe(
        &[],
        true,
        &[doc("a.md", Some(1), Some(1)), doc("b.md", Some(2), Some(2))],
    );
    assert!(changed, "reviewed.json has something to say now");
    assert_eq!(book.saved(), saved(&[("a.md", 1), ("b.md", 2)]));
    assert_eq!(book.baseline("a.md"), Some(1));
    assert_eq!(book.entries(), NOTHING);
}

#[test]
fn an_approved_version_that_was_saved_stands_over_what_was_read() {
    // the person kept part of a change, and the app was closed
    let mut book = Book::new(saved(&[("a.md", 5)]));
    assert!(!book.observe(&[], true, &[doc("a.md", Some(9), Some(9))]));
    assert_eq!(book.baseline("a.md"), Some(5));
    assert_eq!(book.entries(), [entry("a.md", Status::Changed)]);
}

#[test]
fn a_document_that_was_never_read_is_new_and_has_no_baseline() {
    let book = seen(&[], &[doc("fresh.md", Some(3), None)]);
    assert_eq!(book.baseline("fresh.md"), None);
    assert_eq!(book.entries(), [entry("fresh.md", Status::New)]);
    assert_eq!(book.saved(), saved(&[]));
}

#[test]
fn reading_moves_the_approved_version_and_a_new_document_becomes_approved_when_read() {
    let mut book = seen(&[], &[doc("fresh.md", Some(3), None)]);
    assert!(book.observe(&[], true, &[doc("fresh.md", Some(3), Some(3))]));
    assert_eq!(book.reviewed("fresh.md"), Some(3));
    assert_eq!(book.entries(), NOTHING);
}

#[test]
fn what_an_agent_wrote_is_a_change_until_it_is_read() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    assert_eq!(book.entries(), [entry("a.md", Status::Changed)]);
    assert_eq!(book.baseline("a.md"), Some(1));
    // written and then put back: nothing to review
    book.observe(&[], true, &[doc("a.md", Some(1), Some(1))]);
    assert_eq!(book.entries(), NOTHING);
}

#[test]
fn opening_a_changed_document_holds_the_version_it_was_read_from() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);

    book.opening("a.md", 2);
    let changed = book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    assert!(changed);
    assert_eq!(book.reviewed("a.md"), Some(2), "it counts as read");
    assert_eq!(book.held("a.md"), Some(1));
    assert_eq!(book.baseline("a.md"), Some(1), "and the changes stay");
    assert_eq!(book.entries(), [entry("a.md", Status::Changed)]);
    assert_eq!(book.roots(), HashSet::from([1, 2]));
}

#[test]
fn a_document_read_without_opening_it_holds_nothing() {
    // mark all read, a folder marked read, a save by the person
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    assert_eq!(book.held("a.md"), None);
    assert_eq!(book.entries(), NOTHING);
}

#[test]
fn a_later_reading_keeps_the_version_that_is_held() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.opening("a.md", 2);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    // the agent writes again while the document is open, and it is read again
    book.observe(&[], true, &[doc("a.md", Some(3), Some(2))]);
    book.opening("a.md", 3);
    book.observe(&[], true, &[doc("a.md", Some(3), Some(3))]);
    assert_eq!(book.held("a.md"), Some(1), "all of it is still to review");
    assert_eq!(book.reviewed("a.md"), Some(3));
}

#[test]
fn the_held_version_goes_when_the_document_is_back_at_it() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.opening("a.md", 2);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    // every change was reverted: the file is what it was, and the app wrote it
    book.observe(&[], true, &[doc("a.md", Some(1), Some(1))]);
    assert_eq!(book.held("a.md"), None);
    assert_eq!(book.baseline("a.md"), Some(1));
    assert_eq!(book.entries(), NOTHING);
}

#[test]
fn a_save_of_the_person_while_a_version_is_held_does_not_release_it() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.opening("a.md", 2);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    book.observe(&[], true, &[doc("a.md", Some(7), Some(7))]);
    assert_eq!(book.reviewed("a.md"), Some(7));
    assert_eq!(book.held("a.md"), Some(1));
    assert_eq!(book.entries(), [entry("a.md", Status::Changed)]);
}

#[test]
fn releasing_lets_the_held_version_go() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.opening("a.md", 2);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    book.release("a.md");
    assert_eq!(book.baseline("a.md"), Some(2));
    assert_eq!(book.entries(), NOTHING);
    assert_eq!(book.roots(), HashSet::from([2]));
}

#[test]
fn opening_without_a_change_of_the_read_version_is_forgotten_by_the_next_look() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.opening("a.md", 1); // opened with nothing new
    book.observe(&[], true, &[doc("a.md", Some(1), Some(1))]);
    // much later a bulk reading moves the version: that is not the person opening it
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    assert_eq!(book.held("a.md"), None);
}

#[test]
fn a_look_at_the_project_that_comes_between_the_reading_and_its_own_look_does_not_lose_the_hold() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.opening("a.md", 2);
    // a look caused by the watcher, from before the reading was counted
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    assert_eq!(book.held("a.md"), Some(1));
}

#[test]
fn a_reading_of_one_version_does_not_hold_for_the_reading_of_another() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.opening("a.md", 5);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    assert_eq!(
        book.held("a.md"),
        None,
        "that was not the version the person opened"
    );
}

#[test]
fn marking_unread_keeps_the_approved_version_and_reading_again_moves_it() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(1), None)]);
    assert_eq!(book.reviewed("a.md"), Some(1));
    book.observe(&[], true, &[doc("a.md", Some(1), Some(1))]);
    assert_eq!(book.reviewed("a.md"), Some(1));
    // changed while marked unread, then read
    book.observe(&[], true, &[doc("a.md", Some(2), None)]);
    book.opening("a.md", 2);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    assert_eq!(book.held("a.md"), Some(1));
}

#[test]
fn a_deleted_document_keeps_its_approved_version_and_is_listed_once_the_disk_was_looked_at() {
    let mut book = Book::new(saved(&[("gone.md", 4), ("here.md", 5)]));
    book.observe(&[], false, &[doc("here.md", None, Some(5))]);
    assert_eq!(book.entries(), NOTHING, "nothing is known yet");
    book.observe(&[], true, &[doc("here.md", Some(5), Some(5))]);
    assert_eq!(book.entries(), [entry("gone.md", Status::Deleted)]);
    assert_eq!(book.reviewed("gone.md"), Some(4));
    assert_eq!(book.saved(), saved(&[("gone.md", 4), ("here.md", 5)]));
}

#[test]
fn a_document_deleted_and_made_again_is_compared_with_what_it_was() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[]);
    assert_eq!(book.entries(), [entry("a.md", Status::Deleted)]);
    book.observe(&[], true, &[doc("a.md", Some(9), None)]);
    assert_eq!(book.entries(), [entry("a.md", Status::Changed)]);
    assert_eq!(book.baseline("a.md"), Some(1));
}

#[test]
fn accepting_a_deletion_forgets_the_document() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[]);
    assert!(book.forget("a.md"));
    assert_eq!(book.entries(), NOTHING);
    assert_eq!(book.saved(), saved(&[]));
    assert!(!book.forget("a.md"));
}

#[test]
fn moved_documents_take_their_versions_along() {
    let mut book = seen(
        &[],
        &[
            doc("a.md", Some(1), Some(1)),
            doc("dir/b.md", Some(2), Some(2)),
        ],
    );
    book.observe(
        &[],
        true,
        &[
            doc("a.md", Some(11), Some(1)),
            doc("dir/b.md", Some(2), Some(2)),
        ],
    );
    book.opening("a.md", 11);
    book.observe(
        &[],
        true,
        &[
            doc("a.md", Some(11), Some(11)),
            doc("dir/b.md", Some(2), Some(2)),
        ],
    );
    assert_eq!(book.held("a.md"), Some(1));

    let moves = [
        ("a.md".to_owned(), "moved/a.md".to_owned()),
        ("dir/b.md".to_owned(), "dir2/b.md".to_owned()),
    ];
    book.observe(
        &moves,
        true,
        &[
            doc("moved/a.md", Some(11), Some(11)),
            doc("dir2/b.md", Some(2), Some(2)),
        ],
    );
    assert_eq!(book.saved(), saved(&[("moved/a.md", 11), ("dir2/b.md", 2)]));
    assert_eq!(book.held("moved/a.md"), Some(1));
    assert_eq!(book.held("a.md"), None);
    assert_eq!(book.entries(), [entry("moved/a.md", Status::Changed)]);
}

#[test]
fn a_document_moved_to_a_name_that_is_not_tracked_is_forgotten() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[("a.md".to_owned(), "a.txt".to_owned())], true, &[]);
    assert_eq!(book.saved(), saved(&[]));
    assert_eq!(book.entries(), NOTHING);
}

#[test]
fn a_move_while_a_document_is_being_opened_still_holds_the_version() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.opening("a.md", 2);
    book.observe(
        &[("a.md".to_owned(), "b.md".to_owned())],
        true,
        &[doc("b.md", Some(2), Some(2))],
    );
    assert_eq!(book.held("b.md"), Some(1));
}

#[test]
fn keeping_a_change_moves_the_baseline_and_not_what_was_read() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    // no version is held: the approved one moves
    assert!(book.set_baseline("a.md", 10));
    assert_eq!(book.reviewed("a.md"), Some(10));
    assert_eq!(book.saved(), saved(&[("a.md", 10)]));
    assert_eq!(book.entries(), [entry("a.md", Status::Changed)]);

    // with one held, that one moves, and the approved one stays what was read
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.opening("a.md", 2);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    assert_eq!(book.held("a.md"), Some(10));
    assert!(!book.set_baseline("a.md", 11), "reviewed.json is the same");
    assert_eq!(book.held("a.md"), Some(11));
    assert_eq!(book.reviewed("a.md"), Some(2));
    // all of it kept: the baseline is the document, and nothing is held
    assert!(!book.set_baseline("a.md", 2));
    assert_eq!(book.held("a.md"), None);
    assert_eq!(book.entries(), NOTHING);
}

#[test]
fn accepting_approves_the_document_as_it_is_and_lets_the_held_version_go() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.opening("a.md", 2);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    assert!(!book.accept("a.md", 2), "it is approved at that already");
    assert_eq!(book.held("a.md"), None);
    assert_eq!(book.entries(), NOTHING);
    // and a new document
    book.observe(
        &[],
        true,
        &[doc("a.md", Some(2), Some(2)), doc("n.md", Some(8), None)],
    );
    assert!(book.accept("n.md", 8));
    assert_eq!(book.entries(), NOTHING);
}

#[test]
fn entries_are_sorted_by_path_and_name_what_is_different() {
    let book = seen(
        &[("gone.md", 1), ("b.md", 2)],
        &[
            doc("c.md", Some(3), None),
            doc("b.md", Some(22), Some(2)),
            doc("a.md", Some(4), Some(4)),
        ],
    );
    assert_eq!(
        book.entries(),
        [
            entry("b.md", Status::Changed),
            entry("c.md", Status::New),
            entry("gone.md", Status::Deleted),
        ]
    );
}

#[test]
fn the_versions_that_need_a_copy_say_what_the_document_holds() {
    let mut book = seen(
        &[],
        &[doc("a.md", Some(1), Some(1)), doc("b.md", Some(5), Some(5))],
    );
    book.observe(
        &[],
        true,
        &[doc("a.md", Some(2), Some(1)), doc("b.md", Some(5), Some(5))],
    );
    book.opening("a.md", 2);
    book.observe(
        &[],
        true,
        &[doc("a.md", Some(2), Some(2)), doc("b.md", Some(5), Some(5))],
    );
    let mut wanted = book.wanted();
    wanted.sort_by(|x, y| (&x.path, x.hash).cmp(&(&y.path, y.hash)));
    assert_eq!(
        wanted,
        [
            Want {
                path: "a.md".to_owned(),
                hash: 1,
                current: Some(2)
            },
            Want {
                path: "a.md".to_owned(),
                hash: 2,
                current: Some(2)
            },
            Want {
                path: "b.md".to_owned(),
                hash: 5,
                current: Some(5)
            },
        ]
    );
}

#[test]
fn only_markdown_names_are_kept_from_a_saved_file() {
    let book = Book::new(saved(&[("a.md", 1), ("notes.txt", 2)]));
    assert_eq!(book.saved(), saved(&[("a.md", 1)]));
}

// Windows and macOS compare paths without regard to case; Linux does not.
#[test]
fn paths_are_compared_the_way_the_file_system_compares_them() {
    let mut book = seen(&[], &[doc("Docs/A.md", Some(1), Some(1))]);
    if crate::vault::CASE_INSENSITIVE {
        assert_eq!(book.reviewed("docs/a.md"), Some(1));
        book.observe(&[], true, &[doc("docs/a.md", Some(1), Some(1))]);
        assert_eq!(
            book.saved(),
            saved(&[("docs/a.md", 1)]),
            "spelled as last seen"
        );
    } else {
        assert_eq!(book.reviewed("docs/a.md"), None);
    }
}

// ---- documents that are gone ----

const DAY: u64 = 24 * 60 * 60 * 1000;

fn deleted_at(book: &Book, path: &str) -> Option<u64> {
    book.saved().get(path).and_then(|d| d.deleted_at)
}

#[test]
fn a_document_seen_to_be_gone_is_dated_once_and_a_document_that_is_back_loses_its_date() {
    let mut book = seen(
        &[],
        &[doc("a.md", Some(1), Some(1)), doc("b.md", Some(2), Some(2))],
    );
    assert!(!book.age_deleted(1000), "nothing is gone");
    book.observe(&[], true, &[doc("b.md", Some(2), Some(2))]);
    assert!(book.age_deleted(2000), "a.md is dated");
    assert_eq!(deleted_at(&book, "a.md"), Some(2000));
    assert_eq!(deleted_at(&book, "b.md"), None);
    assert!(!book.age_deleted(9000), "and keeps its first date");
    assert_eq!(deleted_at(&book, "a.md"), Some(2000));
    // it comes back (restored, or the branch was switched back)
    book.observe(
        &[],
        true,
        &[doc("a.md", Some(1), Some(1)), doc("b.md", Some(2), Some(2))],
    );
    assert!(book.age_deleted(9500));
    assert_eq!(deleted_at(&book, "a.md"), None);
}

#[test]
fn nothing_is_gone_before_the_disk_has_been_looked_at() {
    let mut book = Book::new(saved(&[("a.md", 1)]));
    book.observe(&[], false, &[doc("a.md", None, Some(1))]);
    assert!(!book.age_deleted(5000));
    // the document was not there when the disk was looked at
    book.observe(&[], true, &[]);
    assert!(book.age_deleted(6000));
    assert_eq!(deleted_at(&book, "a.md"), Some(6000));
}

#[test]
fn the_date_survives_a_restart() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[]);
    book.age_deleted(2000);
    let mut again = Book::new(book.saved());
    again.observe(&[], true, &[]);
    assert!(!again.age_deleted(3000), "the date it had stands");
    assert_eq!(deleted_at(&again, "a.md"), Some(2000));
}

#[test]
fn a_document_gone_for_thirty_days_is_forgotten_with_its_version() {
    let mut book = seen(
        &[],
        &[doc("a.md", Some(1), Some(1)), doc("b.md", Some(2), Some(2))],
    );
    book.observe(&[], true, &[]);
    book.age_deleted(10 * DAY);
    book.opening("a.md", 1);
    assert_eq!(book.entries().len(), 2, "both are listed as deleted");
    assert!(
        !book.age_deleted(10 * DAY + 29 * DAY),
        "29 days: still there"
    );
    assert_eq!(book.entries().len(), 2);
    assert!(book.age_deleted(10 * DAY + 30 * DAY), "30 days");
    assert_eq!(book.entries(), NOTHING);
    assert_eq!(book.saved(), saved(&[]));
    assert_eq!(book.roots(), HashSet::new(), "so its copy can go");
}

#[test]
fn the_oldest_deleted_documents_go_first_beyond_the_cap() {
    let docs: Vec<DocState> = (0..205)
        .map(|i| doc(&format!("d{i:03}.md"), Some(i + 1), Some(i + 1)))
        .collect();
    let mut book = seen(&[], &docs);
    // they disappear one at a time, a minute apart: d000 first
    for i in 0..205_u64 {
        book.observe(&[], true, &docs[usize::try_from(i + 1).expect("small")..]);
        book.age_deleted(100 * DAY + 60_000 * i);
    }
    let kept: Vec<String> = book.saved().keys().cloned().collect();
    assert_eq!(kept.len(), 200);
    assert_eq!(kept.first().map(String::as_str), Some("d005.md"));
    assert_eq!(kept.last().map(String::as_str), Some("d204.md"));
    assert_eq!(book.entries().len(), 200);
}

#[test]
fn an_approved_document_that_is_there_is_never_forgotten_whatever_its_age() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    assert!(!book.age_deleted(400 * DAY));
    assert_eq!(book.reviewed("a.md"), Some(1));
}

// ---- readings that never came to anything ----

#[test]
fn opening_a_document_that_is_read_already_leaves_nothing_waiting() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.opening("a.md", 1);
    assert!(book.opened.is_empty(), "the read version cannot move to it");
    // so an old reading cannot make a later one into a hold by mistake
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    book.observe(&[], true, &[doc("a.md", Some(1), Some(1))]);
    assert_eq!(book.held("a.md"), None);
}

#[test]
fn a_reading_of_a_document_that_the_project_does_not_have_is_forgotten() {
    // an ignored folder: its documents are never among the project's
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.opening("ignored/b.md", 5);
    book.opening("a.md", 9);
    assert_eq!(book.opened.len(), 2);
    book.observe(&[], true, &[doc("a.md", Some(1), Some(1))]);
    assert_eq!(
        book.opened.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a.md"],
        "a.md may still come to read 9"
    );
    book.observe(&[], true, &[]);
    assert!(book.opened.is_empty());
}

// ---- undoing a keep ----

#[test]
fn a_document_can_be_put_back_where_it_stood_if_it_still_stands_where_it_was_left() {
    let mut book = seen(&[], &[doc("a.md", Some(2), Some(1))]);
    let before = book.standing("a.md");
    assert_eq!(
        before,
        Standing {
            reviewed: Some(1),
            held: None
        }
    );
    // a change is kept: the approved version moves to 5
    assert!(book.set_baseline("a.md", 5));
    let after = book.standing("a.md");
    assert_eq!(after.reviewed, Some(5));
    // it stands at `after`, so it goes back
    assert_eq!(book.restore_standing("a.md", after, before), Some(true));
    assert_eq!(book.standing("a.md"), before);
    // and now it no longer stands at `after`
    assert_eq!(book.restore_standing("a.md", after, before), None);
}

#[test]
fn a_held_version_is_put_back_too() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.opening("a.md", 2);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    let before = book.standing("a.md");
    assert_eq!((before.reviewed, before.held), (Some(2), Some(1)));
    book.set_baseline("a.md", 2); // the last change is kept: nothing is left to review
    assert_eq!(book.held("a.md"), None);
    let after = book.standing("a.md");
    assert_eq!(book.restore_standing("a.md", after, before), Some(false));
    assert_eq!(book.standing("a.md"), before);
    assert_eq!(book.entries(), [entry("a.md", Status::Changed)]);
}

#[test]
fn a_forgotten_document_is_put_back_with_its_version() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[]);
    let before = book.standing("a.md");
    book.forget("a.md");
    let after = book.standing("a.md");
    assert_eq!(after, Standing::default());
    assert_eq!(book.restore_standing("a.md", after, before), Some(true));
    assert_eq!(book.reviewed("a.md"), Some(1));
    assert_eq!(book.entries(), [entry("a.md", Status::Deleted)]);
}

#[test]
fn a_standing_knows_which_copies_it_needs() {
    let standing = Standing {
        reviewed: Some(3),
        held: Some(4),
    };
    assert_eq!(standing.hashes().collect::<Vec<_>>(), [3, 4]);
    assert_eq!(Standing::default().hashes().count(), 0);
}
