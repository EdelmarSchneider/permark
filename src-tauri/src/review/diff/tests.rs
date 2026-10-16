use super::*;

const NO_CHANGES: [Change; 0] = [];

/// A change as `(kind, old text, new text)`, the text being the blocks of that side from the first to the last.
fn shape(reviewed: &str, current: &str) -> Vec<(ChangeKind, String, String)> {
    let cmp = Comparison::new(reviewed, current);
    cmp.changes()
        .iter()
        .map(|c| {
            (
                c.kind,
                reviewed[c.old.start..c.old.end].to_owned(),
                current[c.new.start..c.new.end].to_owned(),
            )
        })
        .collect()
}

/// The blocks of a text as the diff compares them (line endings made alike).
fn block_texts(text: &str) -> Vec<String> {
    parse_blocks(text)
        .blocks
        .iter()
        .map(|b| text[b.start_byte..b.end_byte].replace("\r\n", "\n"))
        .collect()
}

/// Reverts one change after another (the first each time, the diff taken again) until nothing is left.
fn revert_everything(reviewed: &str, current: &str) -> String {
    let mut now = current.to_owned();
    for _ in 0..200 {
        let next = {
            let cmp = Comparison::new(reviewed, &now);
            if cmp.changes().is_empty() {
                return now;
            }
            cmp.reverted(0)
        };
        now = next;
    }
    panic!("reverting does not end: {now:?}");
}

/// Keeps one change after another until nothing is left.
fn keep_everything(reviewed: &str, current: &str) -> String {
    let mut now = reviewed.to_owned();
    for _ in 0..200 {
        let next = {
            let cmp = Comparison::new(&now, current);
            if cmp.changes().is_empty() {
                return now;
            }
            cmp.kept(0)
        };
        now = next;
    }
    panic!("keeping does not end: {now:?}");
}

#[test]
fn equal_texts_have_no_changes() {
    assert_eq!(diff("# A\n\ntext\n", "# A\n\ntext\n"), NO_CHANGES);
    assert_eq!(diff("", ""), NO_CHANGES);
    assert_eq!(diff("\n\n", ""), NO_CHANGES);
}

#[test]
fn blank_lines_and_final_newlines_between_blocks_are_not_changes() {
    let reviewed = "# A\n\ntext\n\n- x\n";
    assert_eq!(diff(reviewed, "# A\n\n\n\ntext\n\n\n- x"), NO_CHANGES);
    assert_eq!(diff(reviewed, "\n\n# A\n\ntext\n- x\n\n\n"), NO_CHANGES);
}

#[test]
fn a_byte_order_mark_is_not_a_change() {
    assert_eq!(diff("# A\n\ntext\n", "\u{FEFF}# A\n\ntext\n"), NO_CHANGES);
    assert_eq!(diff("\u{FEFF}# A\n\ntext\n", "# A\n\ntext\n"), NO_CHANGES);
}

#[test]
fn changing_every_line_ending_is_not_a_change() {
    let lf = "# A\n\nline one\nline two\n\n- x\n- y\n";
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(diff(lf, &crlf), NO_CHANGES);
    assert_eq!(diff(&crlf, lf), NO_CHANGES);
}

#[test]
fn an_added_paragraph_is_an_addition_with_nothing_on_the_old_side() {
    let reviewed = "# A\n\nfirst\n\nlast\n";
    let current = "# A\n\nfirst\n\nbrand new\n\nlast\n";
    assert_eq!(
        shape(reviewed, current),
        [(ChangeKind::Added, String::new(), "brand new".to_owned())]
    );
    let change = &diff(reviewed, current)[0];
    assert_eq!(change.old_blocks, 2..2);
    assert_eq!(change.new_blocks, 2..3);
    assert_eq!(change.old.start, change.old.end, "an empty place");
    assert_eq!(
        &reviewed[change.old.start..],
        "last\n",
        "the place is where the block would go"
    );
}

#[test]
fn a_removed_paragraph_is_a_removal() {
    let reviewed = "# A\n\nfirst\n\ngone\n\nlast\n";
    let current = "# A\n\nfirst\n\nlast\n";
    assert_eq!(
        shape(reviewed, current),
        [(ChangeKind::Removed, "gone".to_owned(), String::new())]
    );
}

#[test]
fn an_edited_paragraph_is_a_modification_that_pairs_both_versions() {
    let reviewed = "# A\n\nThe quick brown fox jumps over the lazy dog.\n\nend\n";
    let current = "# A\n\nThe quick brown fox leaps over the lazy dog today.\n\nend\n";
    let changes = diff(reviewed, current);
    assert_eq!(changes.len(), 1);
    let c = &changes[0];
    assert_eq!(c.kind, ChangeKind::Modified);
    assert_eq!(c.old_blocks, 1..2);
    assert_eq!(c.new_blocks, 1..2);
    assert_eq!(
        c.rows,
        [Row {
            old: Some(1),
            new: Some(1)
        }]
    );
    assert_eq!(
        &reviewed[c.old.start..c.old.end],
        "The quick brown fox jumps over the lazy dog."
    );
    assert_eq!(
        &current[c.new.start..c.new.end],
        "The quick brown fox leaps over the lazy dog today."
    );
}

#[test]
fn a_run_of_changed_blocks_is_one_change() {
    let reviewed = "keep\n\nold one\n\nold two\n\nkeep too\n";
    let current = "keep\n\nnew one\n\nnew two\n\nnew three\n\nkeep too\n";
    let changes = diff(reviewed, current);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::Modified);
    assert_eq!(changes[0].old_blocks, 1..3);
    assert_eq!(changes[0].new_blocks, 1..4);
}

#[test]
fn edits_apart_are_changes_apart() {
    let reviewed = "# T\n\nalpha one\n\nstable\n\nbeta two\n\n# End\n";
    let current = "# T\n\nalpha ONE and more\n\nstable\n\nbeta TWO also\n\n# End\n";
    let changes = diff(reviewed, current);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].new_blocks, 1..2);
    assert_eq!(changes[1].new_blocks, 3..4);
}

#[test]
fn rows_pair_the_edited_block_and_leave_the_others_alone() {
    let reviewed = "keep\n\nthe plan is to ship the beta in june\n\nkeep too\n";
    let current =
        "keep\n\nsomething unrelated entirely\n\nthe plan is to ship the beta in july\n\nkeep too\n";
    let changes = diff(reviewed, current);
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].rows,
        [
            Row {
                old: None,
                new: Some(1)
            },
            Row {
                old: Some(1),
                new: Some(2)
            },
        ]
    );
}

#[test]
fn blocks_with_nothing_in_common_are_a_removal_and_an_addition_not_an_edit() {
    let changes = diff(
        "keep\n\naaa bbb ccc\n\nend\n",
        "keep\n\nxxx yyy zzz\n\nend\n",
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::Modified);
    assert_eq!(
        changes[0].rows,
        [
            Row {
                old: Some(1),
                new: None
            },
            Row {
                old: None,
                new: Some(1)
            },
        ]
    );
}

#[test]
fn chinese_text_is_paired_by_its_characters() {
    let reviewed = "# 标题\n\n这是第一段文字，介绍项目的目标。\n";
    let current = "# 标题\n\n这是第一段文字，介绍项目的新目标。\n";
    let changes = diff(reviewed, current);
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].rows,
        [Row {
            old: Some(1),
            new: Some(1)
        }],
        "an edit, not a removal and an addition"
    );
}

#[test]
fn a_moved_block_is_a_removal_where_it_was_and_an_addition_where_it_went() {
    let reviewed = "moved block\n\nsecond\n\nthird\n\nfourth\n";
    let current = "second\n\nthird\n\nfourth\n\nmoved block\n";
    let changes = diff(reviewed, current);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].kind, ChangeKind::Removed);
    assert_eq!(changes[1].kind, ChangeKind::Added);
    assert_eq!(keep_everything(reviewed, current), current);
    assert_eq!(
        block_texts(&revert_everything(reviewed, current)),
        block_texts(reviewed)
    );
}

#[test]
fn repeated_blocks_are_matched_in_order() {
    let reviewed = "intro\n\n---\n\nbody\n\n---\n\nend\n";
    let current = "intro\n\n---\n\nbody\n\nextra\n\n---\n\nend\n";
    assert_eq!(
        shape(reviewed, current),
        [(ChangeKind::Added, String::new(), "extra".to_owned())]
    );
}

#[test]
fn a_front_matter_edit_is_a_change_of_its_own() {
    let reviewed = "---\ntitle: One\n---\n\n# Body\n";
    let current = "---\ntitle: Two\n---\n\n# Body\n";
    let changes = diff(reviewed, current);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::Modified);
    assert_eq!(changes[0].old_blocks, 0..1);
    assert_eq!(
        &current[changes[0].new.start..changes[0].new.end],
        "---\ntitle: Two\n---"
    );
}

#[test]
fn a_new_file_is_all_addition_and_an_emptied_one_all_removal() {
    let text = "# A\n\none\n\ntwo\n";
    let added = diff("", text);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].kind, ChangeKind::Added);
    assert_eq!(added[0].new_blocks, 0..3);
    let removed = diff(text, "");
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].kind, ChangeKind::Removed);
    assert_eq!(removed[0].old_blocks, 0..3);
}

#[test]
fn a_rewrite_of_the_whole_file_is_one_change_that_gives_back_both_versions_exactly() {
    let reviewed = "\u{FEFF}# Old title\r\n\r\nalpha beta gamma\r\n\r\n- delta\r\n";
    let current = "# Completely different\n\nnothing shared here at all\n";
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(cmp.changes().len(), 1);
    assert_eq!(cmp.changes()[0].kind, ChangeKind::Modified);
    assert_eq!(cmp.reverted(0), reviewed, "the whole file, as it was");
    assert_eq!(cmp.kept(0), current, "and as it is");
}

#[test]
fn a_file_too_different_to_pair_does_not_stall_the_diff() {
    // 2100 x 2100 different blocks: far over what is compared, so it is one change and no pairing
    let make = |tag: &str| {
        (0..2100)
            .map(|i| format!("{tag} paragraph number {i}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let (reviewed, current) = (make("old"), make("new"));
    let changes = diff(&reviewed, &current);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].old_blocks, 0..2100);
    assert_eq!(changes[0].new_blocks, 0..2100);
    assert_eq!(changes[0].rows.len(), 4200, "removed and added, unpaired");
}

#[test]
fn reverting_an_added_block_leaves_the_text_around_it_as_it_was() {
    let reviewed = "# A\n\nfirst\n\nlast\n";
    let current = "# A\n\nfirst\n\nbrand new\n\nlast\n";
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(cmp.reverted(0), reviewed);
    assert_eq!(cmp.kept(0), current);
}

#[test]
fn reverting_a_removed_block_puts_it_back_with_its_separator() {
    let reviewed = "# A\n\nfirst\n\ngone\n\nlast\n";
    let current = "# A\n\nfirst\n\nlast\n";
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(cmp.reverted(0), reviewed);
    assert_eq!(cmp.kept(0), current);
}

#[test]
fn an_addition_at_the_start_or_at_the_end_keeps_the_final_newline_in_place() {
    let reviewed = "middle\n";
    let at_start = "first\n\nmiddle\n";
    let at_end = "middle\n\nlast\n";
    for current in [at_start, at_end] {
        let cmp = Comparison::new(reviewed, current);
        assert_eq!(cmp.reverted(0), reviewed, "{current:?}");
        assert_eq!(cmp.kept(0), current, "{current:?}");
    }
    // and the same for removals
    for reviewed in [at_start, at_end] {
        let cmp = Comparison::new(reviewed, "middle\n");
        assert_eq!(cmp.reverted(0), reviewed, "{reviewed:?}");
        assert_eq!(cmp.kept(0), "middle\n", "{reviewed:?}");
    }
}

#[test]
fn a_reverted_edit_swaps_only_the_blocks() {
    let reviewed = "# A\n\n\nThe quick brown fox jumps.\n\n\nend\n";
    let current = "# A\n\nThe quick brown fox leaps over.\n\nend\n";
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(
        cmp.reverted(0),
        "# A\n\nThe quick brown fox jumps.\n\nend\n",
        "the file's own blank lines stay"
    );
    assert_eq!(
        cmp.kept(0),
        "# A\n\n\nThe quick brown fox leaps over.\n\n\nend\n"
    );
}

#[test]
fn a_reverted_block_takes_the_line_endings_of_the_file_it_goes_into() {
    let reviewed = "# A\n\nfirst\nsecond\n\nlast\n";
    let current = "# A\r\n\r\nlast\r\n";
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(cmp.changes().len(), 1);
    assert_eq!(
        cmp.reverted(0),
        "# A\r\n\r\nfirst\r\nsecond\r\n\r\nlast\r\n"
    );
}

#[test]
fn keeping_one_change_leaves_the_others_to_review() {
    let reviewed = "# T\n\nalpha one\n\nstable\n\nbeta two\n\n# End\n";
    let current = "# T\n\nalpha ONE and more\n\nstable\n\nbeta TWO also\n\n# End\n";
    let cmp = Comparison::new(reviewed, current);
    let after_keep = cmp.kept(0);
    assert_eq!(
        after_keep,
        "# T\n\nalpha ONE and more\n\nstable\n\nbeta two\n\n# End\n"
    );
    let again = diff(&after_keep, current);
    assert_eq!(again.len(), 1, "the other edit is still to review");
    let after_revert = cmp.reverted(1);
    assert_eq!(
        after_revert,
        "# T\n\nalpha ONE and more\n\nstable\n\nbeta two\n\n# End\n"
    );
}

#[test]
fn a_text_too_large_to_parse_has_no_blocks_to_compare() {
    assert_eq!(parse_blocks(&"x\n\n".repeat(3_000_000)).blocks.len(), 0);
    assert!(too_large(&"x".repeat(6 * 1024 * 1024)));
    assert!(!too_large("small"));
}

/// A small generator of documents: the same seed gives the same text.
struct Rng(u64);

impl Rng {
    fn next(&mut self, below: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % below as u64).expect("small")
    }
}

const PIECES: [&str; 12] = [
    "# Heading one",
    "## Heading two",
    "A paragraph with some ordinary words in it.",
    "Another paragraph, longer, with more words to share and compare.",
    "- item a\n- item b\n- item c",
    "1. first\n2. second",
    "```rust\nfn main() {}\n```",
    "> a quote\n> over two lines",
    "| a | b |\n|---|---|\n| 1 | 2 |",
    "---",
    "Closing remarks about the whole thing.",
    "Tiny.",
];

/// A document of 0 to 8 blocks drawn from `PIECES`, some of them changed a little.
fn random_doc(rng: &mut Rng, eol: &str) -> String {
    let count = rng.next(9);
    let mut blocks = Vec::new();
    for _ in 0..count {
        let mut piece = PIECES[rng.next(PIECES.len())].to_owned();
        if rng.next(4) == 0 {
            piece = piece.replace("words", "terms").replace("a ", "the ");
        }
        blocks.push(piece);
    }
    let gap = ["\n\n", "\n\n\n", "\n \n"][rng.next(3)];
    let mut text = blocks.join(gap);
    if rng.next(2) == 0 {
        text.push('\n');
    }
    text.replace('\n', eol)
}

#[test]
fn reverting_everything_gives_back_the_reviewed_blocks_and_keeping_everything_the_current_ones() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for round in 0..400 {
        let eol = if round % 5 == 0 { "\r\n" } else { "\n" };
        let reviewed = random_doc(&mut rng, eol);
        let current = random_doc(&mut rng, if round % 7 == 0 { "\r\n" } else { "\n" });
        assert_eq!(
            block_texts(&revert_everything(&reviewed, &current)),
            block_texts(&reviewed),
            "revert, round {round}: {reviewed:?} -> {current:?}"
        );
        assert_eq!(
            block_texts(&keep_everything(&reviewed, &current)),
            block_texts(&current),
            "keep, round {round}: {reviewed:?} -> {current:?}"
        );
    }
}

#[test]
fn a_change_is_always_made_of_whole_blocks_inside_the_text() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for round in 0..200 {
        let reviewed = random_doc(&mut rng, "\n");
        let current = random_doc(&mut rng, "\n");
        let cmp = Comparison::new(&reviewed, &current);
        let mut seen_old = 0;
        let mut seen_new = 0;
        for c in cmp.changes() {
            assert!(c.old_blocks.start >= seen_old && c.new_blocks.start >= seen_new);
            assert!(reviewed.is_char_boundary(c.old.start) && reviewed.is_char_boundary(c.old.end));
            assert!(current.is_char_boundary(c.new.start) && current.is_char_boundary(c.new.end));
            assert!(c.old.end <= reviewed.len() && c.new.end <= current.len());
            assert!(
                !c.old_blocks.is_empty() || !c.new_blocks.is_empty(),
                "round {round}: a change with nothing in it"
            );
            let rows_old: Vec<usize> = c.rows.iter().filter_map(|r| r.old).collect();
            let rows_new: Vec<usize> = c.rows.iter().filter_map(|r| r.new).collect();
            assert_eq!(rows_old, c.old_blocks.clone().collect::<Vec<_>>());
            assert_eq!(rows_new, c.new_blocks.clone().collect::<Vec<_>>());
            seen_old = c.old_blocks.end + 1;
            seen_new = c.new_blocks.end + 1;
        }
    }
}
