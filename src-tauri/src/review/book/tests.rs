use super::*;

const NOTHING: [Entry; 0] = [];

fn doc(path: &str, current: Option<u64>, read: Option<u64>) -> DocState {
    DocState {
        path: path.to_owned(),
        current,
        read,
    }
}

fn saved(entries: &[(&str, u64)]) -> BTreeMap<String, u64> {
    entries.iter().map(|(p, h)| ((*p).to_owned(), *h)).collect()
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

    book.opening("a.md");
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
    book.opening("a.md");
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    // the agent writes again while the document is open, and it is read again
    book.observe(&[], true, &[doc("a.md", Some(3), Some(2))]);
    book.opening("a.md");
    book.observe(&[], true, &[doc("a.md", Some(3), Some(3))]);
    assert_eq!(book.held("a.md"), Some(1), "all of it is still to review");
    assert_eq!(book.reviewed("a.md"), Some(3));
}

#[test]
fn the_held_version_goes_when_the_document_is_back_at_it() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.opening("a.md");
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
    book.opening("a.md");
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
    book.opening("a.md");
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    book.release("a.md");
    assert_eq!(book.baseline("a.md"), Some(2));
    assert_eq!(book.entries(), NOTHING);
    assert_eq!(book.roots(), HashSet::from([2]));
}

#[test]
fn opening_without_a_change_of_the_read_version_is_forgotten_by_the_next_look() {
    let mut book = seen(&[], &[doc("a.md", Some(1), Some(1))]);
    book.opening("a.md"); // opened with nothing new
    book.observe(&[], true, &[doc("a.md", Some(1), Some(1))]);
    // much later a bulk reading moves the version: that is not the person opening it
    book.observe(&[], true, &[doc("a.md", Some(2), Some(1))]);
    book.observe(&[], true, &[doc("a.md", Some(2), Some(2))]);
    assert_eq!(book.held("a.md"), None);
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
    book.opening("a.md");
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
    book.opening("a.md");
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
    book.opening("a.md");
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
    book.opening("a.md");
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
    book.opening("a.md");
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
    book.opening("a.md");
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
